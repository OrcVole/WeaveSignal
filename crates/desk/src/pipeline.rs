// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk: GStreamer for the room and the link to playout, one Rust mixing pass for every
//! bus (M3).
//!
//! **Why one pass.** The M1–M2 desk built the matrix from GStreamer elements: a mixer, a
//! silent bed and a limiter per bus, a gate per route, a queue per publisher. At 13 people
//! that was about 460 threads, each waking every 10–20 ms and handing audio to the next; on a
//! busy shared host every handover is a chance to be scheduled late, and one late link is an
//! audible gap (the staging run, 2026-10-25). The arithmetic itself is microseconds per block.
//! So the matrix is now a loop: every 20 ms one thread takes a block from every voice, runs
//! each voice's input chain and meter, sums the programme and every mix-minus through their
//! gates, limits each bus at −1 dBFS, puts the programme through the profanity delay, and
//! hands each finished bus to a small pool of encoders. Nothing inside the pass waits for
//! another thread.
//!
//! **Into the desk.** Each person's voice arrives through a `livekitwebrtcsrc` as RTP: the
//! desk takes it before `webrtcsrc` would decode it (a `decodebin3` with a thread of its own),
//! and depayloads and decodes the Opus on the receiving jitter buffer's own thread, which then
//! leaves the samples in the voice's [`InputRing`].
//!
//! **Out of the desk.** Each return is Opus-encoded by a worker of the pool (one per core, at
//! most four by default) and pushed through a short leaky queue into its `livekitwebrtcsink`,
//! which takes encoded audio as it is: a publisher that stops taking audio drops its own and
//! holds up no worker (ADR-0168). The programme is encoded on a thread of its own and goes on
//! as Matroska to the SRT listener that Liquidsoap connects to (ADR-0086), behind a leaky
//! queue. A desk outputs from its first moment: the pass runs whether anyone is there or not, and a bus with nobody on it
//! is a block of silence (spike 01: Liquidsoap drops an SRT sender that sends only a header).
//!
//! **Carts** are sources of the same pass ([`crate::carts`], ADR-0117): what sounds on air is
//! added to the programme and to every return before their limiters, so the presenter and
//! guests hear it and the profanity delay holds it; what is cued reaches the presenters' and
//! DJs' returns only. Each cart is decoded by a small pipeline of its own, never this one.
//!
//! People come and go: [`Desk::add_person`] makes their mix-minus, [`Desk::subscribe`]
//! subscribes to their voice, [`Desk::remove_person`] takes it all down. Which voice reaches
//! which bus is the matrix's rule ([`crate::matrix`]) and the person's switches (on air, mute,
//! cue, talk), read afresh by every pass; a gate that changes ramps across one block.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use weavesignal_mixer::{
    Chain, DelayLine, DelayState, InputRing, Limiter, LoudnessMeter, mix_into,
};
use weavesignal_proto::{Keys, Role, check_identity};

use crate::analysis::Tap;
use crate::carts::{CartError, CartsStatus, Deck, Decoder, Fire, Fired, PlayEvent};
use crate::config::{Config, FEED_PASSWORD_ENV, LiveKit};
use crate::feed::Feed;
use crate::hls_sound::{HlsSound, Source, StationMode};
use crate::matrix::{Bus, Switches, gate_to_bus, owner};
use crate::video::{DeskSide, Video, VisionPerson};

/// 16-bit audio: what the test sources send and the measuring taps record.
pub const RAW_CAPS: &str = "audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved";
/// What a voice is turned into before its ring: 32-bit float, so a mix can exceed full scale
/// until its limiter.
pub const MIX_CAPS: &str = "audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved";
pub const RATE: u32 = 48_000;
const CHANNELS: usize = 2;

/// What the desk sends out. Tests turn the network outputs off.
#[derive(Clone, Copy, Debug)]
pub struct Outputs {
    /// The programme to Liquidsoap over SRT.
    pub srt: bool,
    /// Each mix-minus back into the room.
    pub mix_minus_to_room: bool,
}

impl Outputs {
    pub const ALL: Outputs = Outputs {
        srt: true,
        mix_minus_to_room: true,
    };
    pub const NONE: Outputs = Outputs {
        srt: false,
        mix_minus_to_room: false,
    };
}

/// A person the desk knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub role: Role,
    /// The LiveKit room they are in: their booth for a caller (ADR-0193), `None` for the
    /// studio room. The desk's two connections for them join the same room.
    pub room: Option<String>,
    /// The name others see, from the room (the token's name); may be empty.
    pub name: String,
    pub switches: Switches,
    /// When they were seen leaving the room, while they are away.
    pub away_since: Option<Instant>,
}

/// A person, as the status reports them.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PersonStatus {
    pub identity: String,
    pub name: String,
    pub role: Role,
    pub on_air: bool,
    pub mute: bool,
    pub cue: bool,
    /// Holding talkback to presenters and DJs.
    pub talk: bool,
    /// Holding talk to this one person.
    pub talk_to: Option<String>,
    pub present: bool,
    /// How long they have been out of the room, while they are.
    pub away_ms: Option<u64>,
    /// Whether their voice reaches the desk.
    pub linked: bool,
    /// Their voice's short-term loudness (3 s) in LUFS; `None` for silence or no voice.
    pub lufs: Option<f32>,
    /// The highest momentary loudness (400 ms) of the same 3 s, the meter's peak mark.
    pub lufs_peak: Option<f32>,
    /// Their voice's ring: blocks it ran out within, audio that came after its place was
    /// read (dropped), and what it holds ahead of the read now.
    pub underruns: u64,
    pub late_ms: u64,
    pub ring_ms: u32,
    /// Their camera has a picture at the desk (video on).
    pub camera: bool,
}

/// Whether playout is connected to the programme's SRT listener, and since when.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlayoutLink {
    pub connected: bool,
    /// How long it has been in this state; `None` before playout ever connected.
    pub for_ms: Option<u64>,
}

/// The profanity delay as the studio sees it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct DelayStatus {
    #[serde(flatten)]
    pub state: DelayState,
    /// How long ago the last dump was, if there was one.
    pub dumped_ms_ago: Option<u64>,
}

/// The video stream's sound (ADR-0172).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HlsSoundStatus {
    /// The desk's delayed programme, or playout's output fed back.
    pub source: Source,
    /// Someone is live, as heard after the delay.
    pub live: bool,
    /// Playout is connected to the desk's feed port.
    pub feed_connected: bool,
    /// Connections from playout since the desk started.
    pub feed_connections: u64,
}

/// How the mixing pass is keeping time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct MixStats {
    /// Blocks mixed since the desk started.
    pub blocks: u64,
    /// Blocks mixed without a voice that was late past the wait.
    pub late: u64,
    /// The longest pass, in microseconds.
    pub max_pass_us: u64,
    /// Whether the mixing thread runs with real-time scheduling.
    pub realtime: bool,
}

struct LinkState {
    callers: usize,
    changed: Option<Instant>,
}

/// One voice at the desk: a person's audio pad, the elements that bring it to its ring, and
/// what the pass does with it.
struct Input {
    /// The subscription it came from ([`Desk::route_pad`]'s callers get one of their own).
    serial: u64,
    /// This input's own key, for its gains on every bus.
    key: u64,
    /// The pad the voice arrives on, to unlink it.
    pad: gst::Pad,
    elements: Vec<gst::Element>,
    ring: Arc<InputRing>,
    chain: Option<Chain>,
    meter: LoudnessMeter,
    /// This block's samples.
    buf: Vec<f32>,
}

/// A bus: its limiter, the gain each input had last block, and where its blocks go.
struct BusState {
    /// Tells a rebuilt return from the one it replaced.
    serial: u64,
    limiter: Limiter,
    gains: HashMap<u64, f32>,
    acc: Vec<f32>,
    taps: Vec<Tap>,
    /// The return's encoder and publisher, once made (none for the programme).
    out: Option<Arc<Output>>,
    elements: Vec<gst::Element>,
}

impl BusState {
    fn new(serial: u64, limiter: Limiter) -> Self {
        BusState {
            serial,
            limiter,
            gains: HashMap::new(),
            acc: Vec::new(),
            taps: Vec::new(),
            out: None,
            elements: Vec::new(),
        }
    }
}

struct Sub {
    serial: u64,
    /// The participant's session in the room; a rejoin has a new one.
    sid: String,
    element: gst::Element,
    since: Instant,
    routed: bool,
    /// The elements for pads the desk does not mix as a voice: a camera's hand-off to the
    /// video path, or a sink for a picture the desk does not take (video off).
    extras: Vec<gst::Element>,
}

/// What a person owns in the pipeline, so a failure inside it is theirs alone.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    /// Their voice: the subscription, and the elements that bring it to the desk.
    Voice(u64),
    /// Their return: its encoder and publisher.
    Return(u64),
}

struct State {
    people: BTreeMap<String, Person>,
    buses: BTreeMap<Bus, BusState>,
    inputs: BTreeMap<String, Vec<Input>>,
    subs: BTreeMap<String, Sub>,
    /// Each element a person owns, by name (names are unique in the pipeline), directly in
    /// the pipeline; elements inside it (a `livekitwebrtcsrc`'s) are found through it.
    owners: HashMap<String, (String, Part)>,
    /// When each person's return was last made again after a failure.
    remade: HashMap<String, Instant>,
    delay: DelayLine,
    delayed: Vec<f32>,
    delayed_taps: Vec<Tap>,
    dumped_at: Option<Instant>,
    programme_out: Option<Arc<Output>>,
    /// The programme after the delay, to the HLS audio rendition (video on).
    hls_audio: Option<Arc<Output>>,
    /// The HLS audio's choice between the desk and playout's feed (ADR-0172), and its blocks.
    hls_sound: HlsSound,
    /// After the switch: two sources near full scale can sum past it mid-crossfade.
    hls_limiter: Limiter,
    hls_buf: Vec<f32>,
    hls_taps: Vec<Tap>,
    /// Playout's feed for this block.
    feed_buf: Vec<f32>,
    stats: MixStats,
    /// Where the pass reads every voice next, on the timeline (running time in frames).
    read_pos: Option<i64>,
    /// The cart players.
    deck: Deck,
}

impl State {
    fn own(&mut self, elements: &[gst::Element], identity: &str, part: &Part) {
        for e in elements {
            self.owners
                .insert(e.name().to_string(), (identity.to_owned(), part.clone()));
        }
    }

    fn disown(&mut self, elements: &[gst::Element]) {
        for e in elements {
            self.owners.remove(e.name().as_str());
        }
    }
}

struct Inner {
    pipeline: gst::Pipeline,
    config: Config,
    outputs: Outputs,
    state: Mutex<State>,
    links: AtomicUsize,
    serial: AtomicU64,
    playout: Mutex<LinkState>,
    /// Frames in one block.
    block: usize,
    pool: Pool,
    /// The programme's own encoder thread: with as many stuck publishers as the shared pool
    /// has workers, the programme would otherwise wait behind them and SRT carry nothing,
    /// with no error raised (review 0025, D3).
    programme_pool: Pool,
    /// One fire at a time picks a free player and starts its decoder.
    firing: Mutex<()>,
    /// The video path, when the station has it on (ADR-0009).
    video: Option<Video>,
    /// Playout's output back to the desk, for the video's sound (ADR-0172; video on).
    feed: Option<Feed>,
}

/// The desk. Cheap to clone: every clone is the same desk.
#[derive(Clone)]
pub struct Desk {
    inner: Arc<Inner>,
}

/// What a pipeline error means.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorScope {
    /// It came from something one person owns: their voice (the subscription and what brings
    /// it to the desk) or their return. Only that is taken down and made again; every other
    /// bus and the programme go on.
    Participant(String),
    /// It came from something the desk has already taken down: nothing to do.
    Stale,
    /// It came from the programme or its link to playout: the desk cannot go on, and its
    /// supervisor restarts it.
    Fatal,
}

/// What to take down and make again after a person's failure ([`Desk::repair`]).
#[derive(Debug)]
pub struct Repair {
    identity: String,
    part: Part,
}

/// A person's subscription, as the room watch sees it.
#[derive(Clone, Debug)]
pub struct SubscriptionInfo {
    pub sid: String,
    pub routed: bool,
    pub age: Duration,
    /// How long since the voice last brought audio; `None` before it ever did.
    pub silent: Option<Duration>,
}

fn make(factory: &str, name: Option<&str>) -> Result<gst::Element> {
    let b = gst::ElementFactory::make(factory);
    let b = match name {
        Some(n) => b.name(n),
        None => b,
    };
    b.build().with_context(|| {
        format!("GStreamer element `{factory}` is missing (is its plugin installed?)")
    })
}

fn caps_filter(caps: &str) -> Result<gst::Element> {
    Ok(gst::ElementFactory::make("capsfilter")
        .property("caps", caps.parse::<gst::Caps>().expect("valid caps"))
        .build()?)
}

fn leaky_queue(ms: u32) -> Result<gst::Element> {
    Ok(gst::ElementFactory::make("queue")
        .property("max-size-buffers", 0u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", u64::from(ms) * 1_000_000)
        .property_from_str("leaky", "downstream")
        .build()?)
}

/// How one of the desk's connections joins the room (ADR-0118).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Join<'a> {
    /// Listens to one person (`producer`): hidden from the room, and heard past the limits
    /// the person's page sets, which let no one else subscribe.
    Input { producer: &'a str },
    /// Publishes. `only`: the one person who may subscribe (a mix-minus's own); `None`:
    /// anyone in the room (the programme picture).
    Output { only: Option<&'a str> },
}

/// The patched signaller's property that limits who may subscribe to a publisher
/// (`spike/desk-image/patches`).
pub const ALLOWED_SUBSCRIBERS: &str = "allowed-subscribers";

/// Whether this GStreamer's LiveKit signaller can limit who hears a publisher: without it,
/// every mix-minus would be open to the whole room, so a desk that publishes to the room
/// refuses to start.
pub fn signaller_limits_subscribers() -> Result<()> {
    let sink = make("livekitwebrtcsink", None)?;
    let sig = sink.property::<glib::Object>("signaller");
    if sig.find_property(ALLOWED_SUBSCRIBERS).is_none() {
        bail!(
            "this GStreamer's LiveKit signaller has no `{ALLOWED_SUBSCRIBERS}`, so every \
             mix-minus would be open to the whole room: build the image with the patches in \
             spike/desk-image/patches (ADR-0118)"
        );
    }
    Ok(())
}

/// Set the LiveKit signaller's properties on a `livekitwebrtcsrc` or `livekitwebrtcsink`, and
/// trim the `webrtcbin` it makes before it negotiates: no data channels (the signaller adds
/// two, `_reliable` and `_lossy`, which the desk never uses: each is two threads, and their
/// SCTP association more; the room's own channels toward a subscriber are closed as they
/// open) and no UPnP. A sink's sessions run in pipelines of their own, out
/// of the desk's sight, so this is done through the signaller's `webrtcbin-ready`, which
/// this handler, connected first, also keeps from reaching the signaller's own.
pub(crate) fn signal(
    element: &gst::Element,
    lk: &LiveKit,
    room: &str,
    identity: &str,
    join: Join<'_>,
) {
    let sig = element.property::<glib::Object>("signaller");
    sig.connect("webrtcbin-ready", false, |values| {
        if let Some(Ok(bin)) = values.get(2).map(|v| v.get::<gst::Element>()) {
            no_upnp(&bin);
            // The room opens channels of its own toward a subscriber; closed at once, they
            // take their threads with them.
            bin.connect("on-data-channel", false, |values| {
                if let Some(Ok(channel)) = values.get(1).map(|v| v.get::<glib::Object>()) {
                    channel.emit_by_name::<()>("close", &[]);
                }
                None
            });
        }
        if let Some(Ok(sig)) = values.first().map(|v| v.get::<glib::Object>()) {
            sig.stop_signal_emission_by_name("webrtcbin-ready");
        }
        None
    });
    let keys = Keys {
        api_key: lk.api_key.clone(),
        api_secret: lk.api_secret.clone(),
    };
    let token = match join {
        Join::Input { .. } => keys.desk_input(room, identity),
        Join::Output { .. } => keys.desk_output(room, identity),
    };
    sig.set_property("ws-url", &lk.url);
    sig.set_property("auth-token", &token);
    sig.set_property("room-name", room);
    sig.set_property("identity", identity);
    sig.set_property("participant-name", identity);
    match join {
        Join::Input { producer } => sig.set_property("producer-peer-id", producer),
        Join::Output { only: Some(person) } => {
            sig.set_property(ALLOWED_SUBSCRIBERS, gst::Array::new([person]))
        }
        Join::Output { only: None } => {}
    }
}

/// Stop elements and take them out of the pipeline. An element that would not stop is
/// logged: taken out while still running, it would go on using CPU with nothing to show it.
fn stop_and_remove(pipeline: &gst::Pipeline, elements: &[gst::Element]) {
    for e in elements {
        if let Err(err) = e.set_state(gst::State::Null) {
            tracing::warn!(element = %e.name(), error = %err, "an element would not stop");
        }
    }
    for e in elements {
        let _ = pipeline.remove(e);
    }
}

/// The least time between two remakes of one person's return after failures.
const REMAKE_EVERY: Duration = Duration::from_secs(5);

/// Blocks an output may have waiting for its encoder before the oldest are dropped. A
/// worker of the pool that is encoding a block is busy until the encoder has passed it on, so
/// what keeps a stuck publisher from holding the worker is [`RETURN_QUEUE_MS`].
const OUTPUT_BACKLOG: usize = 8;

/// The overflow buffer between each return's encoder and its publisher (ADR-0168): leaky, so
/// a publisher that stops taking audio drops its own oldest Opus frames, and the worker that
/// encoded them is free at once for every other return. Without it a stuck publisher held a
/// worker of the shared pool for as long as it was stuck, and every return queued behind it
/// (the stall test, with one worker: the other returns sent nothing). It costs one thread per
/// return, the queue's own. The size is the old queue's before each publisher (`5a82457`).
const RETURN_QUEUE_MS: u32 = 200;

/// A leaky queue that answers the allocation query itself. The query is serialized, so a
/// queue holds it until everything before it has gone out: with a stuck element after it,
/// whatever sent the query would be held too (`5a82457`, found by the stall test). Nothing
/// before such a queue needs an allocation from what is after it.
fn overflow_queue(name: &str, ms: u32) -> Result<gst::Element> {
    let q = leaky_queue(ms)?;
    q.set_property("name", name);
    q.static_pad("sink").expect("queue sink").add_probe(
        gst::PadProbeType::QUERY_DOWNSTREAM,
        |_, info| match info.query() {
            Some(query) if matches!(query.view(), gst::QueryView::Allocation(_)) => {
                gst::PadProbeReturn::Handled
            }
            _ => gst::PadProbeReturn::Ok,
        },
    );
    Ok(q)
}

// ---------------------------------------------------------------------------------------
// Outputs and the encoder pool.

/// Where one bus's blocks go after the pass: an Opus encoder's sink pad, pushed by a worker
/// of the pool, in order. The pass only queues a block and wakes a worker; a worker drains
/// one output's queue at a time, so one output's blocks are never encoded out of order, and
/// different outputs are encoded at once.
struct Output {
    name: String,
    /// Blocks waiting for the encoder, each with its timestamp (running time, ns).
    pending: Mutex<VecDeque<(Vec<i16>, u64)>>,
    scheduled: AtomicBool,
    closed: AtomicBool,
    sink: Mutex<OutPad>,
    dropped: AtomicU64,
}

struct OutPad {
    pad: gst::Pad,
    caps: gst::Caps,
    started: bool,
    block_ns: u64,
}

impl Output {
    fn new(name: &str, pad: gst::Pad, block_ns: u64) -> Arc<Output> {
        Arc::new(Output {
            name: name.to_owned(),
            pending: Mutex::new(VecDeque::with_capacity(OUTPUT_BACKLOG)),
            scheduled: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            sink: Mutex::new(OutPad {
                pad,
                caps: RAW_CAPS.parse().expect("valid caps"),
                started: false,
                block_ns,
            }),
            dropped: AtomicU64::new(0),
        })
    }

    /// Queue a block for encoding, stamped `pts`: its place in the mix as running time (ns),
    /// not when it is encoded (the pass's side: never waits on the encoder).
    fn offer(self: &Arc<Self>, block: Vec<i16>, pts: u64, pool: &Pool) {
        if self.closed.load(Ordering::Relaxed) {
            return;
        }
        {
            let mut q = self.pending.lock().expect("output queue");
            if q.len() >= OUTPUT_BACKLOG {
                q.pop_front();
                let n = self.dropped.fetch_add(1, Ordering::Relaxed);
                if n.is_multiple_of(500) {
                    tracing::warn!(output = %self.name, dropped = n + 1, "an output is not keeping up; its oldest blocks are dropped");
                }
            }
            q.push_back((block, pts));
        }
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            pool.send(self.clone());
        }
    }

    /// Encode and push everything queued (a worker's side).
    fn drain(&self) {
        let mut sink = self.sink.lock().expect("output sink");
        loop {
            let next = self.pending.lock().expect("output queue").pop_front();
            match next {
                Some((block, pts)) => {
                    if !self.closed.load(Ordering::Relaxed) {
                        sink.push(&block, &self.name, pts);
                    }
                }
                None => {
                    self.scheduled.store(false, Ordering::Release);
                    if self.pending.lock().expect("output queue").is_empty()
                        || self.scheduled.swap(true, Ordering::AcqRel)
                    {
                        break;
                    }
                }
            }
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.pending.lock().expect("output queue").clear();
    }
}

impl OutPad {
    fn start(&mut self, name: &str) -> bool {
        let ok = self
            .pad
            .send_event(gst::event::StreamStart::new(&format!("weavesignal-{name}")))
            && self.pad.send_event(gst::event::Caps::new(&self.caps))
            && self
                .pad
                .send_event(gst::event::Segment::new(&gst::FormattedSegment::<
                    gst::ClockTime,
                >::new()));
        self.started = ok;
        ok
    }

    fn push(&mut self, block: &[i16], name: &str, pts: u64) {
        let first = !self.started;
        if first && !self.start(name) {
            return;
        }
        let bytes: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut buffer = gst::Buffer::from_mut_slice(bytes);
        {
            let b = buffer.get_mut().expect("a new buffer is writable");
            // A block is mixed only once its time has passed, so a sink that syncs (the
            // publisher's `clocksync`) never waits for it.
            b.set_pts(gst::ClockTime::from_nseconds(pts));
            b.set_duration(gst::ClockTime::from_nseconds(self.block_ns));
            if first {
                b.set_flags(gst::BufferFlags::DISCONT);
            }
        }
        match self.pad.chain(buffer) {
            Ok(_) => {}
            Err(gst::FlowError::Flushing) => self.started = false,
            Err(e) => {
                tracing::debug!(output = name, error = ?e, "an output did not take a block");
                if e == gst::FlowError::NotNegotiated {
                    self.started = false;
                }
            }
        }
    }
}

/// The encoders: a few threads, each draining whichever output was ready first.
struct Pool {
    tx: Mutex<Option<mpsc::Sender<Arc<Output>>>>,
}

impl Pool {
    fn start(name: &str, workers: usize, realtime: bool) -> Pool {
        let (tx, rx) = mpsc::channel::<Arc<Output>>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..workers.max(1) {
            let rx = rx.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("{name}-{i}"))
                .spawn(move || {
                    if realtime {
                        crate::realtime::ask(crate::realtime::ENCODER_PRIORITY);
                    }
                    loop {
                        let job = rx.lock().expect("pool queue").recv();
                        match job {
                            Ok(out) => out.drain(),
                            Err(_) => break,
                        }
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "an encoder thread could not start");
            }
        }
        Pool {
            tx: Mutex::new(Some(tx)),
        }
    }

    fn send(&self, out: Arc<Output>) {
        if let Some(tx) = self.tx.lock().expect("pool sender").as_ref() {
            let _ = tx.send(out);
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The video pipelines stopped here and now, not by the vision thread later: a process
        // that exits with NVENC still encoding hangs in the driver's teardown (M5's room test).
        if let Some(v) = &self.video {
            v.stop();
        }
        if let Some(f) = &self.feed {
            f.stop();
        }
        // The workers end when the sender goes.
        self.pool.tx.lock().expect("pool sender").take();
        self.programme_pool.tx.lock().expect("pool sender").take();
    }
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect()
}

// ---------------------------------------------------------------------------------------
// The desk.

impl Desk {
    /// Build the programme's output and start the mixing pass. People join with
    /// [`Desk::add_person`].
    pub fn new(config: Config, outputs: Outputs) -> Result<Self> {
        gst::init()?;
        if outputs.mix_minus_to_room {
            signaller_limits_subscribers()?;
        }
        let pipeline = gst::Pipeline::with_name("desk");
        let block = RATE as usize * config.desk.mix_buffer_ms as usize / 1000;
        let programme = BusState::new(0, Limiter::new(&config.programme.limiter, CHANNELS));
        let delay = DelayLine::new(&config.programme.delay, CHANNELS);
        let pool = Pool::start("desk-enc", config.desk.worker_count(), config.desk.realtime);
        let programme_pool = Pool::start("desk-pgm-enc", 1, config.desk.realtime);
        let video = config
            .video
            .enabled
            .then(|| Video::new(&config, outputs.mix_minus_to_room));
        let fs = &config.video.playout_feed;
        let feed = if config.video.enabled && fs.enabled {
            let password = std::env::var(FEED_PASSWORD_ENV)
                .ok()
                .filter(|p| !p.is_empty());
            Some(Feed::start(fs, password, &pipeline)?)
        } else {
            None
        };
        let frames = |ms: u32| RATE as usize * ms as usize / 1000;
        let hls_sound = HlsSound::new(
            CHANNELS,
            frames(fs.crossfade_ms),
            frames(fs.off_air_hold_ms),
        );
        let desk = Desk {
            inner: Arc::new(Inner {
                pipeline,
                state: Mutex::new(State {
                    people: BTreeMap::new(),
                    buses: BTreeMap::from([(Bus::Programme, programme)]),
                    inputs: BTreeMap::new(),
                    subs: BTreeMap::new(),
                    owners: HashMap::new(),
                    remade: HashMap::new(),
                    delay,
                    delayed: Vec::new(),
                    delayed_taps: Vec::new(),
                    dumped_at: None,
                    programme_out: None,
                    hls_audio: None,
                    hls_sound,
                    hls_limiter: Limiter::new(&config.programme.limiter, CHANNELS),
                    hls_buf: Vec::new(),
                    hls_taps: Vec::new(),
                    feed_buf: Vec::new(),
                    stats: MixStats::default(),
                    read_pos: None,
                    deck: Deck::new(CHANNELS, Some(crate::carts::reaper())),
                }),
                config,
                outputs,
                links: AtomicUsize::new(0),
                serial: AtomicU64::new(1),
                playout: Mutex::new(LinkState {
                    callers: 0,
                    changed: None,
                }),
                block,
                pool,
                programme_pool,
                firing: Mutex::new(()),
                video,
                feed,
            }),
        };
        if outputs.srt {
            desk.add_programme_out()?;
        }
        desk.set_jitter_buffer(desk.inner.config.desk.jitter_buffer_ms);
        desk.start_mixing()?;
        if let Some(v) = &desk.inner.video {
            let side: Weak<dyn DeskSide> = Arc::downgrade(&desk.inner) as Weak<dyn DeskSide>;
            v.start(side)?;
        }
        Ok(desk)
    }

    /// The video path, when it is on.
    pub fn video(&self) -> Option<&Video> {
        self.inner.video.as_ref()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().expect("desk state lock")
    }

    fn next_serial(&self) -> u64 {
        self.inner.serial.fetch_add(1, Ordering::SeqCst)
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn pipeline(&self) -> &gst::Pipeline {
        &self.inner.pipeline
    }

    /// How many voice pads have been routed so far.
    pub fn links(&self) -> usize {
        self.inner.links.load(Ordering::SeqCst)
    }

    fn block_ns(&self) -> u64 {
        u64::from(self.inner.config.desk.mix_buffer_ms) * 1_000_000
    }

    /// Record what leaves a bus from now on (tests, meters); `None` for a bus the desk does
    /// not have.
    pub fn tap(&self, bus: &Bus) -> Option<Tap> {
        let mut st = self.lock();
        let b = st.buses.get_mut(bus)?;
        let t = Tap::new();
        b.taps.push(t.clone());
        Some(t)
    }

    /// Record the programme as it leaves the profanity delay, toward playout.
    pub fn tap_delayed(&self) -> Tap {
        let t = Tap::new();
        self.lock().delayed_taps.push(t.clone());
        t
    }

    /// Record the video stream's sound as it goes to the HLS audio encoder (ADR-0172): the
    /// desk's delayed programme while someone is live, playout's feed otherwise.
    pub fn tap_hls_sound(&self) -> Tap {
        let t = Tap::new();
        self.lock().hls_taps.push(t.clone());
        t
    }

    /// The station's mode, from the control plane (ADR-0045, ADR-0183): in auto mode the video's
    /// sound is playout's feed whoever is live (ADR-0172). Setting the mode it has changes nothing.
    pub fn set_mode(&self, mode: StationMode) {
        let mut st = self.lock();
        if st.hls_sound.mode() != mode {
            tracing::info!(?mode, "the station's mode");
            st.hls_sound.set_mode(mode);
        }
    }

    /// The station's mode as the desk was last told it; live until it is told.
    pub fn mode(&self) -> StationMode {
        self.lock().hls_sound.mode()
    }

    /// The video stream's sound now: where it comes from, and the feed from playout.
    pub fn hls_sound(&self) -> HlsSoundStatus {
        let st = self.lock();
        HlsSoundStatus {
            source: st.hls_sound.source(),
            live: st.hls_sound.live(),
            feed_connected: self.inner.feed.as_ref().is_some_and(Feed::connected),
            feed_connections: self.inner.feed.as_ref().map_or(0, Feed::connections),
        }
    }

    pub fn person(&self, identity: &str) -> Option<Person> {
        self.lock().people.get(identity).cloned()
    }

    /// Everyone the desk knows, for the control socket's status.
    pub fn status(&self) -> Vec<PersonStatus> {
        let cameras = self
            .inner
            .video
            .as_ref()
            .map(|v| v.status().cameras)
            .unwrap_or_default();
        let st = self.lock();
        st.people
            .iter()
            .map(|(id, p)| {
                let inputs = st.inputs.get(id);
                let last = inputs.and_then(|v| v.last());
                let loud = last.map(|i| i.meter.reading()).unwrap_or_default();
                let ring = last.map(|i| i.ring.stats()).unwrap_or_default();
                PersonStatus {
                    identity: id.clone(),
                    name: p.name.clone(),
                    role: p.role,
                    on_air: p.switches.on_air,
                    mute: p.switches.mute,
                    cue: p.switches.cue,
                    talk: p.switches.talk,
                    talk_to: p.switches.talk_to.clone(),
                    present: p.away_since.is_none(),
                    away_ms: p.away_since.map(|t| t.elapsed().as_millis() as u64),
                    linked: inputs.is_some_and(|v| !v.is_empty()),
                    lufs: loud.short_term,
                    lufs_peak: loud.momentary_max,
                    underruns: ring.underruns,
                    late_ms: ring.late_frames * 1000 / u64::from(RATE),
                    ring_ms: (ring.frames as u64 * 1000 / u64::from(RATE)) as u32,
                    camera: cameras.contains(id),
                }
            })
            .collect()
    }

    /// Whether playout is connected to the programme's SRT listener.
    pub fn playout(&self) -> PlayoutLink {
        let l = self.inner.playout.lock().expect("playout lock");
        PlayoutLink {
            connected: l.callers > 0,
            for_ms: l.changed.map(|t| t.elapsed().as_millis() as u64),
        }
    }

    /// The profanity delay.
    pub fn delay(&self) -> DelayStatus {
        let st = self.lock();
        DelayStatus {
            state: st.delay.state(),
            dumped_ms_ago: st.dumped_at.map(|t| t.elapsed().as_millis() as u64),
        }
    }

    /// Dump the profanity delay: everything held is thrown away, listeners jump to the live
    /// programme, and the delay builds back up. Allowed while the delay rebuilds (ADR-0166);
    /// `false` only when there is no delay or nothing is held.
    pub fn dump(&self) -> bool {
        let mut st = self.lock();
        let held = st.delay.state().delay_ms;
        let ok = st.delay.dump();
        if ok {
            st.dumped_at = Some(Instant::now());
            tracing::warn!(dumped_ms = held, "the delay was dumped");
        }
        ok
    }

    /// How the mixing pass is keeping time.
    pub fn mix_stats(&self) -> MixStats {
        self.lock().stats
    }

    // -----------------------------------------------------------------------------------
    // Carts.

    /// Fire a cart: its file checked and opened from the cart directory, its decoder started
    /// and given up to `carts.start_wait_ms` for its first audio, then a free player of its
    /// pool takes it from the next pass. Busy when every player of the pool is sounding.
    pub fn fire_cart(&self, fire: &Fire) -> Result<Fired, CartError> {
        fire.check()?;
        let settings = &self.inner.config.carts;
        let path = settings.dir.join(&fire.file);
        if !path.is_file() {
            return Err(CartError::Missing(fire.file.clone()));
        }
        let pool = fire.mode.pool();
        let _one = self.inner.firing.lock().expect("cart fire lock");
        let index = self
            .lock()
            .deck
            .free(pool)
            .ok_or(CartError::Busy(pool.word()))?;
        let decode = |why: String| CartError::Decode {
            file: fire.file.clone(),
            why,
        };
        let (decoder, feed) = Decoder::start(
            &path,
            fire.mode == crate::carts::CartMode::Bed,
            CHANNELS,
            Duration::from_millis(u64::from(settings.ahead_ms)),
        )
        .map_err(|e| decode(format!("{e:#}")))?;
        let started = feed.wait_for(
            self.inner.block,
            Duration::from_millis(u64::from(settings.start_wait_ms)),
        );
        if let Some(why) = feed.failed() {
            return Err(decode(why));
        }
        if !started {
            return Err(decode(format!("no audio in {} ms", settings.start_wait_ms)));
        }
        Ok(self.lock().deck.insert(fire, index, feed, Some(decoder)))
    }

    /// Fade out every player sounding `cart` over 300 ms; how many.
    pub fn stop_cart(&self, cart: &str) -> usize {
        self.lock().deck.stop(cart)
    }

    /// Fade out every cart sounding; `cue`: only those cued (`Some(true)`), or only those on
    /// air (`Some(false)`).
    pub fn stop_carts(&self, cue: Option<bool>) -> usize {
        self.lock().deck.stop_all(cue)
    }

    /// What is sounding, and whether beds are dipped.
    pub fn carts(&self) -> CartsStatus {
        self.lock().deck.status()
    }

    /// The play events after `after`, for the per-play log.
    pub fn cart_plays(&self, after: u64) -> Vec<PlayEvent> {
        self.lock().deck.plays_after(after)
    }

    fn playout_caller(&self, delta: isize) {
        let mut l = self.inner.playout.lock().expect("playout lock");
        let was = l.callers > 0;
        l.callers = l.callers.saturating_add_signed(delta);
        if was != (l.callers > 0) || l.changed.is_none() {
            l.changed = Some(Instant::now());
            tracing::info!(connected = l.callers > 0, "playout link");
        }
    }

    /// Note the names the room gives people (the token's name).
    pub fn note_names<'a>(&self, names: impl IntoIterator<Item = (&'a str, &'a str)>) {
        let mut st = self.lock();
        for (id, name) in names {
            if let Some(p) = st.people.get_mut(id)
                && p.name != name
            {
                p.name = name.to_owned();
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // The mixing pass.

    fn start_mixing(&self) -> Result<()> {
        let weak = Arc::downgrade(&self.inner);
        let realtime = self.inner.config.desk.realtime;
        std::thread::Builder::new()
            .name("desk-mix".into())
            .spawn(move || mix_loop(weak, realtime))
            .context("cannot start the mixing thread")?;
        Ok(())
    }

    /// The pipeline's running time now, in frames; `None` before it has a clock.
    fn running_frames(&self) -> Option<i64> {
        crate::feed::running_frames(&self.inner.pipeline)
    }

    /// Mix every block that is due. A block is due once its time has passed and every voice
    /// that is flowing has arrived for it, or, for a voice that is late, once the wait
    /// (`input_prebuffer_ms`) is over: as GStreamer's live mixers wait, but in one thread, so
    /// a voice's delay through the desk is how late it is, not a fixed cushion. Several blocks
    /// at once when catching up; from the clock again after a stall or a restart.
    fn mix_due(&self) {
        let Some(now) = self.running_frames() else {
            self.lock().read_pos = None;
            return;
        };
        let b = self.inner.block as i64;
        let wait = i64::from(RATE) * i64::from(self.inner.config.desk.input_prebuffer_ms) / 1000;
        let mut st = self.lock();
        let mut pos = match st.read_pos {
            Some(p) if p <= now && now - p <= 10 * b + wait => p,
            // Never before the pipeline's start: a block there would be stamped 0, on top of
            // the next.
            _ => (now - b).max(0),
        };
        let mut mixed = 0;
        while now >= pos + b && mixed < 8 {
            let flowing_since = now - 2 * b - wait;
            let ready = st
                .inputs
                .values()
                .flatten()
                .all(|i| i.ring.ready(pos + b, flowing_since));
            if !ready && now < pos + b + wait {
                break;
            }
            let t = Instant::now();
            self.mix_block(&mut st, pos, t);
            let took = t.elapsed().as_micros() as u64;
            st.stats.blocks += 1;
            if !ready {
                st.stats.late += 1;
            }
            st.stats.max_pass_us = st.stats.max_pass_us.max(took);
            pos += b;
            mixed += 1;
        }
        st.read_pos = Some(pos);
    }

    /// One pass for the block at `pos`: every voice's block in, every bus out.
    fn mix_block(&self, st: &mut State, pos: i64, now: Instant) {
        let n = self.inner.block * CHANNELS;
        let pool = &self.inner.pool;
        let programme_pool = &self.inner.programme_pool;
        let State {
            people,
            inputs,
            buses,
            delay,
            delayed,
            delayed_taps,
            programme_out,
            hls_audio,
            hls_sound,
            hls_limiter,
            hls_buf,
            hls_taps,
            feed_buf,
            deck,
            ..
        } = st;
        for list in inputs.values_mut() {
            for input in list.iter_mut() {
                input.buf.resize(n, 0.0);
                input.ring.pull(pos, &mut input.buf);
                if let Some(c) = &mut input.chain {
                    c.process(&mut input.buf);
                }
                input.meter.process(&input.buf);
            }
        }
        // Carts: beds dip while a microphone is open on air, worked out from the same
        // switches this pass mixes with.
        let mic_open = people.values().any(|p| {
            p.away_since.is_none()
                && p.role.can_air()
                && p.switches.on_air
                && !p.switches.mute
                && p.switches.talk_to.is_none()
        });
        deck.render(self.inner.block, mic_open);
        // Someone is live: a strip open on air, muted or not (ADR-0172, ADR-0062's OPEN).
        let live = people
            .values()
            .any(|p| p.away_since.is_none() && p.role.can_air() && p.switches.on_air);
        // Playout's feed for the same block: read every pass, so what is not used is dropped.
        // It is there when this block holds some of it (not while a new connection fills its
        // cushion) and it has not been away for `absent_ms`.
        let mut feed_here = false;
        if let Some(f) = &self.inner.feed {
            feed_buf.resize(n, 0.0);
            let some = f.ring().pull(pos, feed_buf);
            let absent =
                i64::from(self.inner.config.video.playout_feed.absent_ms) * i64::from(RATE) / 1000;
            feed_here = some && f.ring().arrived().is_some_and(|a| a + absent >= pos);
        }
        let feed = feed_here.then_some(&feed_buf[..]);
        let (cart_air, cart_cue) = (deck.air(), deck.cue());
        // Every output's block is stamped with its place in the mix, never with when a worker
        // got to encode it: the mixing thread wakes every few milliseconds and mixes two
        // blocks at once when late, so stamps taken from the clock stepped 21, 21, 21, 21,
        // 16 ms, opusenc passed them on, and a caller's phone concealed a gap at nearly every
        // packet (38 % of what it played, 2026-11-03; `tests/return_time.rs`).
        let pts = (pos as u128 * 1_000_000_000 / u128::from(RATE)) as u64;
        for (bus, bs) in buses.iter_mut() {
            let owner_role = match bus {
                Bus::Programme => None,
                Bus::MixMinus(id) => people.get(id).map(|p| p.role),
            };
            bs.acc.clear();
            bs.acc.resize(n, 0.0);
            for (src, list) in inputs.iter() {
                let Some(p) = people.get(src) else { continue };
                let open = gate_to_bus(src, p.role, bus, owner_role)
                    .is_some_and(|g| p.switches.passes(g, owner(bus)));
                let target = if open { 1.0 } else { 0.0 };
                for input in list {
                    let g = bs.gains.entry(input.key).or_insert(0.0);
                    mix_into(&mut bs.acc, &input.buf, CHANNELS, *g, target);
                    *g = target;
                }
            }
            // Carts on air reach every bus; cued carts only the presenters' and DJs' ears.
            if let Some(a) = cart_air {
                mix_into(&mut bs.acc, a, CHANNELS, 1.0, 1.0);
            }
            if let Some(c) = cart_cue
                && owner_role.is_some_and(Role::hears_talkback)
            {
                mix_into(&mut bs.acc, c, CHANNELS, 1.0, 1.0);
            }
            bs.limiter.process(&mut bs.acc);
            for t in &bs.taps {
                t.push_f32(now, &bs.acc);
            }
            match bus {
                Bus::Programme => {
                    delayed.resize(n, 0.0);
                    delay.process(&bs.acc, delayed);
                    for t in delayed_taps.iter() {
                        t.push_f32(now, delayed);
                    }
                    if let Some(o) = programme_out {
                        o.offer(to_i16(delayed), pts, programme_pool);
                    }
                    // The video's sound: the desk while someone is live, as heard after the
                    // delay; playout's output otherwise (ADR-0172).
                    let held = u64::from(delay.state().delay_ms) * u64::from(RATE) / 1000;
                    hls_sound.render(live, held, delayed, feed, hls_buf);
                    hls_limiter.process(hls_buf);
                    for t in hls_taps.iter() {
                        t.push_f32(now, hls_buf);
                    }
                    if let Some(o) = hls_audio {
                        o.offer(to_i16(hls_buf), pts, pool);
                    }
                }
                Bus::MixMinus(_) => {
                    if let Some(o) = &bs.out {
                        o.offer(to_i16(&bs.acc), pts, pool);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // Building and taking down.

    /// The programme as Opus in Matroska on an SRT listener for Liquidsoap, behind a leaky
    /// queue. With nobody connected, `srtsink` drops what it is given.
    fn add_programme_out(&self) -> Result<()> {
        let p = &self.inner.config.programme;
        let enc = gst::ElementFactory::make("opusenc")
            .name("programme-opus")
            .property("bitrate", p.opus_bitrate as i32)
            .property_from_str(
                "frame-size",
                &self.inner.config.desk.mix_buffer_ms.to_string(),
            )
            .build()
            .context("GStreamer element `opusenc` is missing")?;
        let mux = gst::ElementFactory::make("matroskamux")
            .name("programme-mux")
            .property("streamable", true)
            .build()?;
        let q = leaky_queue(p.queue_ms)?;
        let srt = gst::ElementFactory::make("srtsink")
            .name("programme-srt")
            .property("uri", p.srt_uri())
            .property("latency", p.srt_latency_ms as i32)
            .property("wait-for-connection", false)
            .property("sync", false)
            .property("async", false)
            .build()
            .context("GStreamer element `srtsink` is missing (gst-plugins-bad's SRT plugin)")?;
        // Playout connecting and leaving: whether listeners can hear the room (ADR-0062).
        for (signal, delta) in [("caller-added", 1isize), ("caller-removed", -1)] {
            let weak: Weak<Inner> = Arc::downgrade(&self.inner);
            srt.connect(signal, false, move |_| {
                if let Some(inner) = weak.upgrade() {
                    Desk { inner }.playout_caller(delta);
                }
                None
            });
        }
        let pipeline = &self.inner.pipeline;
        pipeline.add_many([&enc, &mux, &q, &srt])?;
        gst::Element::link_many([&enc, &mux, &q, &srt])?;
        for e in [&enc, &mux, &q, &srt] {
            e.sync_state_with_parent()?;
        }
        let out = Output::new(
            "programme",
            enc.static_pad("sink").expect("opusenc sink"),
            self.block_ns(),
        );
        self.lock().programme_out = Some(out);
        tracing::info!(uri = %p.srt_uri(), "programme: Opus in Matroska on SRT, waiting for playout to connect");
        Ok(())
    }

    /// A return's encoder and publisher, made and started outside the desk's lock (starting
    /// a `livekitwebrtcsink` connects to the room).
    fn make_return_out(
        &self,
        identity: &str,
        serial: u64,
    ) -> Result<(Arc<Output>, Vec<gst::Element>)> {
        let pipeline = &self.inner.pipeline;
        let n = self.next_serial();
        let enc = gst::ElementFactory::make("opusenc")
            .name(format!("enc-mm-{identity}-{n}"))
            .property("bitrate", self.inner.config.desk.return_opus_bitrate as i32)
            .property_from_str(
                "frame-size",
                &self.inner.config.desk.mix_buffer_ms.to_string(),
            )
            .build()
            .context("GStreamer element `opusenc` is missing")?;
        let name = format!("pub-mm-{identity}-{n}");
        // Tests without a room publish nowhere.
        let sink = if self.inner.outputs.mix_minus_to_room {
            let sink = make("livekitwebrtcsink", Some(&name))?;
            let room_identity = format!("{}-mm-{identity}", self.inner.config.desk.identity);
            let room = self.room_of(identity);
            signal(
                &sink,
                &self.inner.config.livekit,
                &room,
                &room_identity,
                Join::Output {
                    only: Some(identity),
                },
            );
            tracing::info!(participant = %identity, identity = %room_identity, "mix-minus published to the room");
            sink
        } else {
            gst::ElementFactory::make("fakesink")
                .name(&name)
                .property("sync", false)
                .property("async", false)
                .build()?
        };
        let queue = overflow_queue(&format!("queue-mm-{identity}-{n}"), RETURN_QUEUE_MS)?;
        let elements = vec![enc.clone(), queue.clone(), sink.clone()];
        // Theirs before it is in the pipeline: a publisher that fails as it connects (a room
        // that refuses it, a dozen joining at once) is their failure, not the desk's.
        self.lock().own(&elements, identity, &Part::Return(serial));
        let started = (|| -> Result<()> {
            pipeline.add_many(&elements)?;
            gst::Element::link_many(&elements)?;
            for e in elements.iter().rev() {
                e.sync_state_with_parent()?;
            }
            Ok(())
        })();
        if let Err(e) = started {
            stop_and_remove(pipeline, &elements);
            self.lock().disown(&elements);
            return Err(e);
        }
        let out = Output::new(
            &format!("mm-{identity}"),
            enc.static_pad("sink").expect("opusenc sink"),
            self.block_ns(),
        );
        Ok((out, elements))
    }

    /// Make a person's return and attach it to their bus, if the bus is still the one with
    /// `serial`; otherwise it is taken down again.
    fn attach_return(&self, identity: &str, serial: u64) -> Result<()> {
        let (out, elements) = self.make_return_out(identity, serial)?;
        let mut st = self.lock();
        let bus = Bus::MixMinus(identity.to_owned());
        let current = st
            .buses
            .get(&bus)
            .is_some_and(|b| b.serial == serial && b.out.is_none());
        if current {
            if let Some(b) = st.buses.get_mut(&bus) {
                b.out = Some(out);
                b.elements = elements;
            }
        } else {
            drop(st);
            out.close();
            stop_and_remove(&self.inner.pipeline, &elements);
            self.lock().disown(&elements);
        }
        Ok(())
    }

    /// The LiveKit room a person's connections join: their booth, or the studio room.
    fn room_of(&self, identity: &str) -> String {
        self.person(identity)
            .and_then(|p| p.room)
            .unwrap_or_else(|| self.inner.config.livekit.room.clone())
    }

    /// A person joins: their mix-minus is made (the matrix routes everyone already at the
    /// desk into it from the next pass), and its publisher started outside the desk's lock. A
    /// person the desk already knows under the same role is welcomed back to the mix-minus
    /// they had.
    pub fn add_person(&self, identity: &str, role: Role) -> Result<()> {
        self.add_person_in(identity, role, None)
    }

    /// As [`Self::add_person`], in a given LiveKit room: a caller's booth (ADR-0193), or the
    /// studio room with `None`. Someone seen in another room than before joins afresh there.
    pub fn add_person_in(&self, identity: &str, role: Role, room: Option<&str>) -> Result<()> {
        check_identity(identity, &self.inner.config.desk.identity)?;
        if !role.at_desk() {
            bail!("`{identity}` is a {role}: the desk takes no listeners");
        }
        if let Some(p) = self.person(identity) {
            if p.role == role && p.room.as_deref() == room {
                if let Some(p) = self.lock().people.get_mut(identity) {
                    p.away_since = None;
                }
                return Ok(());
            }
            if p.role == role {
                tracing::info!(participant = %identity, from = ?p.room, to = ?room, "seen in another room");
            } else {
                tracing::info!(participant = %identity, from = %p.role, to = %role, "role changed");
            }
            self.remove_person(identity);
        }
        let n = self.next_serial();
        {
            let mut st = self.lock();
            st.people.insert(
                identity.to_owned(),
                Person {
                    role,
                    room: room.map(str::to_owned),
                    name: String::new(),
                    switches: Switches::at_join(role),
                    away_since: None,
                },
            );
            st.buses.insert(
                Bus::MixMinus(identity.to_owned()),
                BusState::new(
                    n,
                    Limiter::new(&self.inner.config.programme.limiter, CHANNELS),
                ),
            );
        }
        tracing::info!(participant = %identity, %role, "joined the desk");
        self.attach_return(identity, n)
    }

    /// Take a person's return out of the desk's state (its bus too, unless `keep_bus`), and
    /// hand back its elements to be stopped outside the lock. `serial`: only the bus with
    /// this serial.
    fn take_return(
        &self,
        st: &mut State,
        identity: &str,
        serial: Option<u64>,
        keep_bus: bool,
    ) -> Vec<gst::Element> {
        let bus = Bus::MixMinus(identity.to_owned());
        if !st
            .buses
            .get(&bus)
            .is_some_and(|b| serial.is_none_or(|n| b.serial == n))
        {
            return Vec::new();
        }
        let (out, elements) = if keep_bus {
            let b = st.buses.get_mut(&bus).expect("checked above");
            (b.out.take(), std::mem::take(&mut b.elements))
        } else {
            let b = st.buses.remove(&bus).expect("checked above");
            (b.out, b.elements)
        };
        if let Some(o) = out {
            o.close();
        }
        elements
    }

    /// Mark a person away: their voice is dropped at once, their mix-minus kept for now.
    pub fn mark_away(&self, identity: &str) {
        self.drop_subscription(identity);
        if let Some(p) = self.lock().people.get_mut(identity)
            && p.away_since.is_none()
        {
            p.away_since = Some(Instant::now());
            tracing::info!(participant = %identity, "away");
        }
    }

    /// A person leaves for good: their voice, their mix-minus and every route into it.
    pub fn remove_person(&self, identity: &str) {
        self.drop_subscription(identity);
        let mut st = self.lock();
        let inputs = self.take_inputs(&mut st, identity, None);
        let doomed = self.take_return(&mut st, identity, None, false);
        st.people.remove(identity);
        st.remade.remove(identity);
        // Nobody keeps talking to someone who has gone.
        let held: Vec<String> = st
            .people
            .iter()
            .filter(|(_, p)| p.switches.talk_to.as_deref() == Some(identity))
            .map(|(id, _)| id.clone())
            .collect();
        drop(st);
        // Not under the lock: stopping a publisher waits for its threads.
        self.stop_inputs(inputs);
        stop_and_remove(&self.inner.pipeline, &doomed);
        self.lock().disown(&doomed);
        for id in held {
            self.set_talk_to(&id, identity, false);
        }
        tracing::info!(participant = %identity, "left the desk");
    }

    /// Take a person's voices out of the desk's state (`serial`: only the one from that
    /// subscription); their elements are stopped with [`Desk::stop_inputs`], outside the lock.
    fn take_inputs(&self, st: &mut State, identity: &str, serial: Option<u64>) -> Vec<Input> {
        let all = st.inputs.remove(identity).unwrap_or_default();
        let (gone, kept): (Vec<Input>, Vec<Input>) = all
            .into_iter()
            .partition(|i| serial.is_none_or(|n| i.serial == n));
        if !kept.is_empty() {
            st.inputs.insert(identity.to_owned(), kept);
        }
        for input in &gone {
            for b in st.buses.values_mut() {
                b.gains.remove(&input.key);
            }
        }
        gone
    }

    fn stop_inputs(&self, inputs: Vec<Input>) {
        for input in inputs {
            if let Some(peer) = input.pad.peer() {
                let _ = input.pad.unlink(&peer);
            }
            stop_and_remove(&self.inner.pipeline, &input.elements);
            self.lock().disown(&input.elements);
        }
    }

    /// Subscribe to a person's voice in the room with a `livekitwebrtcsrc` of its own (spike
    /// 01's measured pattern); their audio is routed as it arrives. `sid` is their session.
    pub fn subscribe(&self, identity: &str, sid: &str) -> Result<()> {
        if self.person(identity).is_none() {
            bail!("cannot subscribe to `{identity}`: not at the desk");
        }
        let serial = self.next_serial();
        let src = make(
            "livekitwebrtcsrc",
            Some(&format!("src-{identity}-{serial}")),
        )?;
        let room_identity = format!("{}-in-{identity}-{serial}", self.inner.config.desk.identity);
        if self.inner.video.is_some() {
            // Only what the desk can decode: never H.264 or H.265 (ADR-0028's room codecs).
            let codecs: Vec<&str> = self
                .inner
                .config
                .video
                .camera_codecs
                .iter()
                .map(String::as_str)
                .collect();
            src.set_property("video-codecs", gst::Array::new(codecs));
        }
        let room = self.room_of(identity);
        signal(
            &src,
            &self.inner.config.livekit,
            &room,
            &room_identity,
            Join::Input { producer: identity },
        );
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        let source = identity.to_owned();
        src.connect_pad_added(move |_src, pad| {
            let Some(inner) = weak.upgrade() else { return };
            let desk = Desk { inner };
            if let Err(e) = desk.route_subscribed(&source, serial, pad) {
                tracing::error!(participant = %source, error = %format!("{e:#}"), "cannot route a pad");
            }
        });
        {
            let mut st = self.lock();
            st.own(std::slice::from_ref(&src), identity, &Part::Voice(serial));
            st.subs.insert(
                identity.to_owned(),
                Sub {
                    serial,
                    sid: sid.to_owned(),
                    element: src.clone(),
                    since: Instant::now(),
                    routed: false,
                    extras: Vec::new(),
                },
            );
        }
        // Not under the lock: the element's threads call back into the desk.
        self.inner.pipeline.add(&src)?;
        src.sync_state_with_parent()?;
        tracing::info!(participant = %identity, sid, "subscribing");
        Ok(())
    }

    /// A person's current subscription, if any.
    pub fn subscription(&self, identity: &str) -> Option<SubscriptionInfo> {
        let now = self.running_frames();
        let st = self.lock();
        let s = st.subs.get(identity)?;
        let arrived = st
            .inputs
            .get(identity)
            .and_then(|v| v.iter().find(|i| i.serial == s.serial))
            .and_then(|i| i.ring.arrived());
        let silent = now
            .zip(arrived)
            .map(|(n, a)| Duration::from_millis(((n - a).max(0) * 1000 / i64::from(RATE)) as u64));
        Some(SubscriptionInfo {
            sid: s.sid.clone(),
            routed: s.routed,
            age: s.since.elapsed(),
            silent: if s.routed {
                silent.or(Some(s.since.elapsed()))
            } else {
                silent
            },
        })
    }

    /// Drop a person's subscription and their voice at the desk; their mix-minus stays.
    pub fn drop_subscription(&self, identity: &str) {
        self.drop_voice(identity, None);
    }

    /// Drop a person's voice: their subscription (only the one with `serial`, if given) and
    /// what brings it to the desk. The room watch subscribes again on its next look.
    fn drop_voice(&self, identity: &str, serial: Option<u64>) {
        let sub = {
            let mut st = self.lock();
            if st
                .subs
                .get(identity)
                .is_some_and(|s| serial.is_none_or(|n| s.serial == n))
            {
                st.subs.remove(identity)
            } else {
                None
            }
        };
        if let Some(v) = &self.inner.video {
            v.camera_detach(identity, sub.as_ref().map(|s| s.serial).or(serial));
        }
        if let Some(sub) = &sub {
            // Not under the lock: stopping the element waits for its threads. It stays owned
            // until it is out of the pipeline, so an error it posts meanwhile is still theirs.
            let mut els = vec![sub.element.clone()];
            els.extend(sub.extras.iter().cloned());
            stop_and_remove(&self.inner.pipeline, &els);
            self.lock().disown(&els);
            tracing::info!(participant = %identity, "subscription dropped");
        }
        let gone = {
            let mut st = self.lock();
            self.take_inputs(&mut st, identity, serial)
        };
        self.stop_inputs(gone);
    }

    fn route_subscribed(&self, source: &str, serial: u64, pad: &gst::Pad) -> Result<()> {
        if pad.name().starts_with("video") && self.inner.video.is_some() {
            return self.route_camera_pad(source, serial, pad, false);
        }
        if !pad.name().starts_with("audio") {
            let sink = gst::ElementFactory::make("fakesink")
                .property("sync", false)
                .property("async", false)
                .build()?;
            {
                let mut st = self.lock();
                if st.subs.get(source).is_none_or(|s| s.serial != serial) {
                    tracing::debug!(participant = %source, "a pad from a dropped subscription");
                    return Ok(());
                }
                st.own(std::slice::from_ref(&sink), source, &Part::Voice(serial));
                if let Some(s) = st.subs.get_mut(source) {
                    s.extras.push(sink.clone());
                }
            }
            self.inner.pipeline.add(&sink)?;
            sink.sync_state_with_parent()?;
            pad.link(&sink.static_pad("sink").expect("fakesink sink"))?;
            return Ok(());
        }
        if self.route_input(source, Some(serial), pad, true)? {
            if let Some(s) = self.lock().subs.get_mut(source) {
                s.routed = true;
            }
        } else {
            tracing::debug!(participant = %source, "a pad from a dropped subscription");
        }
        Ok(())
    }

    /// Route a person's picture pad (a test's camera, raw or encoded) to the video path, as a
    /// subscribed camera is. Video must be on.
    pub fn route_camera(&self, source: &str, pad: &gst::Pad) -> Result<()> {
        if self.person(source).is_none() {
            bail!("`{source}` is not at the desk");
        }
        let serial = self.next_serial();
        self.route_camera_pad(source, serial, pad, true)
    }

    /// A camera's pad: through a leaky queue to an `appsink` that takes it encoded (so the
    /// room's element never decodes pictures in the desk's pipeline) and hands each frame to
    /// the video path, stamped with the desk's running time. The two elements are the
    /// person's, like their voice's, so anything wrong in them is theirs.
    fn route_camera_pad(
        &self,
        source: &str,
        serial: u64,
        pad: &gst::Pad,
        test: bool,
    ) -> Result<()> {
        let Some(video) = self.inner.video.clone() else {
            bail!("video is off");
        };
        let n = self.inner.links.fetch_add(1, Ordering::SeqCst);
        let q = gst::ElementFactory::make("queue")
            .name(format!("cam-{source}-{n}-queue"))
            .property("max-size-buffers", 0u32)
            .property("max-size-bytes", 0u32)
            .property("max-size-time", 1_000_000_000u64)
            .property_from_str("leaky", "downstream")
            .build()?;
        let mut caps = video.camera_caps();
        if test {
            caps.merge(gst::Caps::builder("video/x-raw").build());
        }
        let sink = gst_app::AppSink::builder()
            .name(format!("cam-{source}-{n}-sink"))
            .caps(&caps)
            .sync(false)
            .build();
        sink.set_property("async", false);
        {
            let v = video.clone();
            let id = source.to_owned();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(pts) = sample.buffer().and_then(|b| picture_time(s, &sample, b))
                        {
                            v.camera_sample(&id, serial, &sample, pts);
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
        }
        let elements = vec![q.clone(), sink.clone().upcast()];
        {
            let mut st = self.lock();
            if !test && st.subs.get(source).is_none_or(|s| s.serial != serial) {
                tracing::debug!(participant = %source, "a camera from a dropped subscription");
                return Ok(());
            }
            st.own(&elements, source, &Part::Voice(serial));
            if let Some(s) = st.subs.get_mut(source).filter(|s| s.serial == serial) {
                s.extras.extend(elements.iter().cloned());
            }
        }
        let pipeline = &self.inner.pipeline;
        pipeline.add_many(&elements)?;
        q.link(&sink)?;
        for e in elements.iter().rev() {
            e.sync_state_with_parent()?;
        }
        video.camera_attach(source, serial, sink.static_pad("sink"));
        pad.link(&q.static_pad("sink").expect("queue sink"))
            .with_context(|| format!("cannot link {source}'s camera"))?;
        tracing::info!(participant = %source, pad = %pad.name(), "camera routed to the video path");
        Ok(())
    }

    /// Route a person's raw audio pad (a test's tone, say) into the desk: converted to the
    /// desk's format and left in a ring for the mixing pass.
    pub fn route_pad(&self, source: &str, pad: &gst::Pad) -> Result<()> {
        self.route_input(source, None, pad, false).map(|_| ())
    }

    /// Route a pad. `rtp`: the pad carries Opus in RTP (a subscription), depayloaded and
    /// decoded here, on the thread that delivers it. With `sub`, only while that subscription
    /// is the person's current one (checked again when the voice is put in place, so a pad
    /// never outlives its subscription); `false` when it is not.
    fn route_input(
        &self,
        source: &str,
        sub: Option<u64>,
        pad: &gst::Pad,
        rtp: bool,
    ) -> Result<bool> {
        let current =
            |st: &State| sub.is_none_or(|n| st.subs.get(source).is_some_and(|s| s.serial == n));
        let role = {
            let st = self.lock();
            if !current(&st) {
                return Ok(false);
            }
            let Some(person) = st.people.get(source) else {
                bail!("`{source}` is not at the desk");
            };
            person.role
        };
        let serial = sub.unwrap_or_else(|| self.next_serial());
        let n = self.inner.links.fetch_add(1, Ordering::SeqCst);
        let mut elements = Vec::new();
        if rtp {
            elements.push(make(
                "rtpopusdepay",
                Some(&format!("in-{source}-{n}-depay")),
            )?);
            elements.push(
                gst::ElementFactory::make("opusdec")
                    .name(format!("in-{source}-{n}-dec"))
                    .property("plc", true)
                    .build()
                    .context("GStreamer element `opusdec` is missing")?,
            );
        }
        elements.push(make(
            "audioconvert",
            Some(&format!("in-{source}-{n}-convert")),
        )?);
        elements.push(make("audioresample", None)?);
        elements.push(caps_filter(MIX_CAPS)?);
        let sink = gst_app::AppSink::builder()
            .name(format!("in-{source}-{n}-sink"))
            .caps(&MIX_CAPS.parse::<gst::Caps>().expect("valid caps"))
            .sync(false)
            .build();
        sink.set_property("async", false);
        let ring = Arc::new(InputRing::new(CHANNELS, RATE as usize));
        {
            let ring = ring.clone();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(buf) = sample.buffer()
                            && let Ok(map) = buf.map_readable()
                        {
                            let samples: Vec<f32> = map
                                .as_slice()
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|b| f32::from_le_bytes(*b))
                                .collect();
                            if let Some((pos, now)) =
                                timeline_position(s, &sample, buf, samples.len())
                            {
                                ring.push(pos, &samples, now);
                            }
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
        }
        elements.push(sink.upcast());
        let pipeline = &self.inner.pipeline;
        let part = Part::Voice(serial);
        // Theirs before it is in the pipeline, so an error it posts at once is theirs.
        self.lock().own(&elements, source, &part);
        let started = (|| -> Result<()> {
            pipeline.add_many(&elements)?;
            gst::Element::link_many(&elements)?;
            for e in elements.iter().rev() {
                e.sync_state_with_parent()?;
            }
            Ok(())
        })();
        if let Err(e) = started {
            stop_and_remove(pipeline, &elements);
            self.lock().disown(&elements);
            return Err(e);
        }
        let chain_settings = self.inner.config.chain(role);
        let mut input = Input {
            serial,
            key: self.next_serial(),
            pad: pad.clone(),
            elements: elements.clone(),
            ring,
            chain: (!chain_settings.is_empty()).then(|| Chain::new(&chain_settings, CHANNELS)),
            meter: LoudnessMeter::new(CHANNELS),
            buf: vec![0.0; self.inner.block * CHANNELS],
        };
        let mut st = self.lock();
        if !current(&st) || !st.people.contains_key(source) {
            drop(st);
            stop_and_remove(pipeline, &elements);
            self.lock().disown(&elements);
            return Ok(false);
        }
        // A source in the pipeline the desk did not make (a test's tone) is theirs too, and
        // goes with their voice; a subscription's own element is already theirs.
        if let Some(parent) = pad.parent_element()
            && parent
                .parent()
                .is_some_and(|p| &p == pipeline.upcast_ref::<gst::Object>())
            && !st.owners.contains_key(parent.name().as_str())
        {
            st.own(std::slice::from_ref(&parent), source, &part);
            input.elements.insert(0, parent);
        }
        st.inputs.entry(source.to_owned()).or_default().push(input);
        drop(st);
        if let Err(e) = pad.link(&elements[0].static_pad("sink").expect("a sink pad")) {
            self.drop_voice(source, Some(serial));
            bail!("cannot link {}'s pad {}: {e}", source, pad.name());
        }
        tracing::info!(participant = %source, %role, pad = %pad.name(), "routed");
        Ok(true)
    }

    fn apply_switches(&self, identity: &str, f: impl FnOnce(&mut Switches)) -> bool {
        let mut st = self.lock();
        let Some(p) = st.people.get_mut(identity) else {
            return false;
        };
        f(&mut p.switches);
        true
    }

    /// Put a person on air or take them off; `false` when the desk does not know them.
    pub fn set_on_air(&self, identity: &str, on: bool) -> bool {
        let known = self.apply_switches(identity, |s| s.on_air = on);
        if known {
            tracing::info!(participant = %identity, on_air = on, "on air switched");
        }
        known
    }

    /// Hold or release a person's talkback; `false` when the desk does not know them.
    pub fn set_talk(&self, identity: &str, on: bool) -> bool {
        self.apply_switches(identity, |s| s.talk = on)
    }

    /// Mute or unmute a person: off every bus they air on, on-air state kept.
    pub fn set_mute(&self, identity: &str, on: bool) -> bool {
        self.apply_switches(identity, |s| s.mute = on)
    }

    /// Cue a person into the presenters' and DJs' ears (pre-fade listen), or clear it.
    pub fn set_cue(&self, identity: &str, on: bool) -> bool {
        self.apply_switches(identity, |s| s.cue = on)
    }

    /// Hold talk from `identity` to `to` alone, or release it (only if it is to `to`). `false`
    /// when the desk knows either of them not.
    pub fn set_talk_to(&self, identity: &str, to: &str, on: bool) -> bool {
        if identity == to || self.person(to).is_none() {
            return false;
        }
        self.apply_switches(identity, |s| {
            if on {
                s.talk_to = Some(to.to_owned());
            } else if s.talk_to.as_deref() == Some(to) {
                s.talk_to = None;
            }
        })
    }

    /// What an error message on the pipeline's bus means, and what to repair. It belongs to
    /// whoever owns the element of the pipeline it came from (or the element that element is
    /// inside, as for anything in a `livekitwebrtcsrc`): one person's voice or return is
    /// theirs alone; the programme and its link to playout are fatal. Quick, and changes
    /// nothing: the repair, which can take a while, is [`Desk::repair`].
    pub fn classify_error(&self, msg: &gst::Message) -> (ErrorScope, Option<Repair>) {
        let pipeline = self.inner.pipeline.upcast_ref::<gst::Object>();
        let Some(src) = msg.src() else {
            return (ErrorScope::Fatal, None);
        };
        if src == pipeline {
            return (ErrorScope::Fatal, None);
        }
        // The ancestor that sits directly in the pipeline.
        let mut top = src.clone();
        loop {
            match top.parent() {
                Some(p) if &p == pipeline => break,
                Some(p) => top = p,
                // Out of the pipeline already: taken down since it posted.
                None => return (ErrorScope::Stale, None),
            }
        }
        match self.lock().owners.get(top.name().as_str()) {
            Some((identity, part)) => (
                ErrorScope::Participant(identity.clone()),
                Some(Repair {
                    identity: identity.clone(),
                    part: part.clone(),
                }),
            ),
            None => (ErrorScope::Fatal, None),
        }
    }

    /// Take down what failed and make it again: a voice is dropped (the room watch subscribes
    /// again on its next look); a return's encoder and publisher are made again, and the
    /// person keeps their mix-minus and their switches (on air, mute, cue). A repair already
    /// overtaken (the subscription or bus replaced since) does nothing.
    pub fn repair(&self, repair: Repair) {
        let Repair { identity, part } = repair;
        match part {
            Part::Voice(serial) => {
                tracing::warn!(participant = %identity, "a voice failed; dropping it to subscribe again");
                self.drop_voice(&identity, Some(serial));
            }
            Part::Return(serial) => {
                let doomed = {
                    let mut st = self.lock();
                    self.take_return(&mut st, &identity, Some(serial), true)
                };
                if doomed.is_empty() {
                    return;
                }
                tracing::warn!(participant = %identity, "a return failed; making it again");
                stop_and_remove(&self.inner.pipeline, &doomed);
                // At most once every few seconds: with LiveKit down, every publisher fails
                // at once and at once again.
                let last = self.lock().remade.get(&identity).copied();
                if let Some(wait) = last.and_then(|t| REMAKE_EVERY.checked_sub(t.elapsed())) {
                    std::thread::sleep(wait);
                }
                {
                    let mut st = self.lock();
                    st.disown(&doomed);
                    st.remade.insert(identity.clone(), Instant::now());
                }
                if let Err(e) = self.attach_return(&identity, serial) {
                    tracing::error!(participant = %identity, error = %format!("{e:#}"), "cannot make the return again");
                }
            }
        }
    }

    /// Classify an error and repair what it broke, here and now ([`Desk::classify_error`],
    /// then [`Desk::repair`]).
    pub fn handle_error(&self, msg: &gst::Message) -> ErrorScope {
        let (scope, repair) = self.classify_error(msg);
        if let Some(r) = repair {
            self.repair(r);
        }
        scope
    }

    /// Set the receive jitter buffer on every `webrtcbin`, `rtpbin` and `rtpjitterbuffer` that
    /// appears inside the pipeline, including inside LiveKit's bins: `livekitwebrtcsrc` has
    /// no property for it (spike 01). Each `webrtcbin`'s UPnP is turned off on the way.
    fn set_jitter_buffer(&self, ms: u32) {
        fn apply(el: &gst::Element, ms: u32) {
            let Some(f) = el.factory() else { return };
            if f.name() == "webrtcbin" {
                no_upnp(el);
            }
            if matches!(
                f.name().as_str(),
                "webrtcbin" | "rtpbin" | "rtpjitterbuffer"
            ) && el.find_property("latency").is_some()
            {
                el.set_property("latency", ms);
                tracing::debug!(element = %el.name(), ms, "jitter buffer set");
            }
            // No lip-sync between a person's voice and camera: rtpbin would hold the voice
            // back to the picture's timing (about 40 ms more each way with cameras on, M5's
            // room test). The voice comes first; the vision mixer stamps pictures by their own
            // arrival on the same clock.
            if f.name() == "rtpbin" && el.find_property("max-ts-offset").is_some() {
                el.set_property("max-ts-offset", 1i64);
            }
        }
        self.inner
            .pipeline
            .connect_deep_element_added(move |_, _, el| {
                apply(el, ms);
                if let Some(bin) = el.downcast_ref::<gst::Bin>() {
                    for child in bin.iterate_recurse().into_iter().flatten() {
                        apply(&child, ms);
                    }
                }
            });
    }
}

/// Turn UPnP port mapping off in a `webrtcbin`'s ICE agent: the desk's peers are on the same
/// host or reached through LiveKit's one UDP port (ADR-0023), never through a home router,
/// and libnice keeps a thread per agent for it (26 of the desk's threads at 13 people).
fn no_upnp(webrtcbin: &gst::Element) {
    let agent = webrtcbin
        .find_property("ice-agent")
        .map(|_| webrtcbin.property::<glib::Object>("ice-agent"))
        .and_then(|ice| {
            ice.find_property("agent")
                .map(|_| ice.property::<glib::Object>("agent"))
        });
    if let Some(agent) = agent
        && agent.find_property("upnp").is_some()
    {
        agent.set_property("upnp", false);
    }
}

/// Where a buffer's first frame belongs on the desk's timeline (running time in frames), and
/// the time now: its timestamp, as the jitter buffer dejittered it; or, for a buffer without
/// one or with one far from now (another time base), when it arrived less its length.
fn timeline_position(
    sink: &gst_app::AppSink,
    sample: &gst::Sample,
    buf: &gst::BufferRef,
    samples: usize,
) -> Option<(i64, i64)> {
    let ns_to_frames =
        |t: gst::ClockTime| (t.nseconds() as i128 * i128::from(RATE) / 1_000_000_000) as i64;
    let now = match (sink.clock(), sink.base_time()) {
        (Some(c), Some(base)) => c.time().checked_sub(base)?,
        _ => return None,
    };
    let stamped = sample
        .segment()
        .and_then(|s| s.downcast_ref::<gst::ClockTime>())
        .zip(buf.pts())
        .and_then(|(seg, pts)| seg.to_running_time(pts));
    let now_f = ns_to_frames(now);
    match stamped {
        Some(t) if now.nseconds().abs_diff(t.nseconds()) < 1_000_000_000 => {
            Some((ns_to_frames(t), now_f))
        }
        _ => Some((now_f - (samples / CHANNELS) as i64, now_f)),
    }
}

/// Where a picture belongs on the desk's timeline (running time, ns): its timestamp, or when
/// it arrived if it has none or one far from now.
fn picture_time(
    sink: &gst_app::AppSink,
    sample: &gst::Sample,
    buf: &gst::BufferRef,
) -> Option<u64> {
    let now = match (sink.clock(), sink.base_time()) {
        (Some(c), Some(base)) => c.time().checked_sub(base)?,
        _ => return None,
    };
    let stamped = sample
        .segment()
        .and_then(|s| s.downcast_ref::<gst::ClockTime>())
        .zip(buf.pts())
        .and_then(|(seg, pts)| seg.to_running_time(pts));
    Some(match stamped {
        Some(t) if now.nseconds().abs_diff(t.nseconds()) < 2_000_000_000 => t.nseconds(),
        _ => now.nseconds(),
    })
}

impl DeskSide for Inner {
    fn running_ns(&self) -> Option<u64> {
        let p = &self.pipeline;
        let (clock, base) = (p.clock()?, p.base_time()?);
        if p.current_state() != gst::State::Playing {
            return None;
        }
        clock.time().checked_sub(base).map(|t| t.nseconds())
    }

    fn people(&self) -> Vec<VisionPerson> {
        let st = self.state.lock().expect("desk state lock");
        st.people
            .iter()
            .map(|(id, p)| VisionPerson {
                identity: id.clone(),
                name: p.name.clone(),
                audible: p.away_since.is_none()
                    && p.role.can_air()
                    && p.switches.on_air
                    && !p.switches.mute
                    && p.switches.talk_to.is_none(),
                loudness: st
                    .inputs
                    .get(id)
                    .and_then(|v| v.last())
                    .and_then(|i| i.meter.momentary()),
                may_be_seen: p.role.can_air() && p.role != Role::Caller,
            })
            .collect()
    }

    fn delay_ms(&self) -> u32 {
        self.state
            .lock()
            .expect("desk state lock")
            .delay
            .state()
            .delay_ms
    }

    fn hls_audio(&self, pad: Option<gst::Pad>) {
        let block_ns = u64::from(self.config.desk.mix_buffer_ms) * 1_000_000;
        let out = pad.map(|p| Output::new("hls-audio", p, block_ns));
        let old = std::mem::replace(
            &mut self.state.lock().expect("desk state lock").hls_audio,
            out,
        );
        if let Some(o) = old {
            o.close();
        }
    }

    fn anyone(&self) -> bool {
        !self
            .state
            .lock()
            .expect("desk state lock")
            .people
            .is_empty()
    }
}

/// How often the mixing thread looks for a block that is due.
const POLL: Duration = Duration::from_millis(5);

/// The mixing thread: every few milliseconds, mix whatever blocks are due, until the desk is
/// gone.
fn mix_loop(weak: Weak<Inner>, realtime: bool) {
    let rt = realtime && crate::realtime::ask(crate::realtime::MIX_PRIORITY);
    loop {
        std::thread::sleep(POLL);
        let Some(inner) = weak.upgrade() else { break };
        let desk = Desk { inner };
        desk.lock().stats.realtime = rt;
        desk.mix_due();
    }
}
