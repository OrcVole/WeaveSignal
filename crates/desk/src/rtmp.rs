// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out (ADR-0229, ADR-0230): the programme's picture and sound, behind the profanity
//! delay, to video platforms such as Rumble, Odysee and PeerTube.
//!
//! **What is sent.** The picture is the frame the picture pass sends HLS, after the delay; the
//! sound is the video's sound after the delay (ADR-0172), the block the mixing pass sends the
//! HLS audio. A dump empties both, so RTMP never carries what was dumped.
//!
//! **Two profiles** (ADR-0230), chosen per destination:
//! - `compatible`: H.264 (OpenH264, or NVENC when this machine has a session free) and AAC
//!   (VisualOn's `voaacenc`, Apache-2.0; never fdk-aac), classic FLV;
//! - `open`: AV1 (NVENC or SVT-AV1, as for HLS) and Opus, over Enhanced RTMP.
//!
//! **Failure stays here.** Each profile in use has one encode pipeline (picture and sound in,
//! encoded frames out to app sinks), run only while a destination of that profile is on. Each
//! destination has its own sending pipeline, `appsrc ! rtmp2sink`, fed whole FLV tags
//! ([`crate::flv`]) by the encode pipeline's sinks. A destination that refuses the key or goes
//! away fails its own pipeline alone and is tried again with a growing wait; a failed encode
//! pipeline is made again after 2 s; neither touches HLS, the room, the sound or the other
//! destinations.
//!
//! **The key is a secret.** It reaches the desk over its socket with the destination, is put
//! on the end of the URL only inside `rtmp2sink`, and is cut out of every error before the
//! error is logged or reported ([`crate::flv::redact`]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::{Deserialize, Serialize};

use crate::config::{EncoderChoice, H264Choice, Rung, Video as Settings};
use crate::flv::{self, Amf, Interleave, Kind, Tag};
use crate::video::{self as v, DeskSide, Encoder};

/// A destination's formats (ADR-0230).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// H.264 and AAC, classic RTMP.
    Compatible,
    /// AV1 and Opus, Enhanced RTMP.
    Open,
}

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::Compatible, Profile::Open];

    /// Its slot in the desk's sound outputs.
    pub fn slot(self) -> usize {
        match self {
            Profile::Compatible => 0,
            Profile::Open => 1,
        }
    }
}

/// One destination, as the control plane sends it.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub name: String,
    /// `rtmp://` or `rtmps://`, without the key: the platform's server URL.
    pub url: String,
    /// The stream key, put on the end of the URL as its last part.
    pub key: String,
    pub profile: Profile,
    pub enabled: bool,
}

impl std::fmt::Debug for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Destination")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("key", &"•••")
            .field("profile", &self.profile)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Destination {
    /// The URL `rtmp2sink` connects to: the server URL and the key.
    fn location(&self) -> String {
        format!("{}/{}", self.url.trim_end_matches('/'), self.key)
    }

    /// Whether it can be sent to at all: an RTMP or RTMPS URL with a host, and a key.
    pub fn check(&self) -> Result<(), String> {
        let rest = self
            .url
            .strip_prefix("rtmp://")
            .or_else(|| self.url.strip_prefix("rtmps://"))
            .ok_or_else(|| format!("`{}`: the URL starts rtmp:// or rtmps://", self.name))?;
        if rest.split('/').next().is_none_or(str::is_empty) {
            return Err(format!("`{}`: the URL has no host", self.name));
        }
        if self.key.is_empty()
            || self
                .key
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(format!(
                "`{}`: the stream key is empty or has spaces",
                self.name
            ));
        }
        if self.name.trim().is_empty() {
            return Err("a destination has no name".into());
        }
        Ok(())
    }
}

/// Where a destination is.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SendState {
    /// Turned off.
    Off,
    /// On, waiting for its encoder's first picture keyframe.
    Waiting,
    /// Connecting, or connected and not yet answered.
    Connecting,
    /// Sending, and the server has answered.
    Live,
    /// It failed; tried again in `retry_in_ms`.
    Retrying,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DestinationStatus {
    pub name: String,
    pub profile: Profile,
    pub enabled: bool,
    pub state: SendState,
    pub connected: bool,
    /// How long it has been connected.
    pub connected_ms: Option<u64>,
    /// The last failure, with the key cut out.
    pub last_error: Option<String>,
    pub last_error_ms_ago: Option<u64>,
    /// Bytes sent on this connection, and on all of them since the desk started.
    pub bytes: u64,
    pub bytes_total: u64,
    /// Connections made since the desk started.
    pub connects: u32,
    pub retry_in_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EncodeStatus {
    pub profile: Profile,
    /// `openh264`, `nvenc` (H.264) or `svt`, `nvenc` (AV1).
    pub encoder: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    /// Both sequence headers are known, so a destination can start.
    pub ready: bool,
    /// Tags made since it started.
    pub tags: u64,
}

/// RTMP out, as the desk's status reports it.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct RtmpStatus {
    /// The control plane's name for the set of destinations the desk holds; `None` until it
    /// has sent one (it sends again when this differs from its own).
    pub set: Option<String>,
    pub destinations: Vec<DestinationStatus>,
    /// The encoders running now.
    pub encoders: Vec<EncodeStatus>,
}

/// What a destination's tags go to: its `rtmp2sink`, or (tests) a file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SinkKind {
    Rtmp,
    File(PathBuf),
}

// ---------------------------------------------------------------------------------------

/// A sending pipeline's side of the tags: it waits for the headers and a keyframe, then counts
/// time from that keyframe.
struct Feed {
    src: gst_app::AppSrc,
    started: bool,
    base_ms: u64,
    /// The newest timestamp sent, so a late tag never goes backwards.
    last_ts: u32,
    /// Set when the destination did not keep up: it is dropped and connects again.
    overrun: bool,
    backlog_bytes: u64,
}

impl Feed {
    fn push_bytes(&mut self, bytes: Vec<u8>, ts: u32) {
        let mut b = gst::Buffer::from_mut_slice(bytes);
        {
            let b = b.get_mut().expect("a new buffer is writable");
            let t = gst::ClockTime::from_mseconds(u64::from(ts));
            b.set_pts(t);
            b.set_dts(t);
        }
        if self.src.current_level_bytes() > self.backlog_bytes {
            self.overrun = true;
            return;
        }
        let _ = self.src.push_buffer(b);
    }

    /// Send one tag, starting the connection's stream at a keyframe with the headers.
    fn take(&mut self, tag: &Tag, headers: &Headers) {
        if self.overrun {
            return;
        }
        if !self.started {
            if !(tag.kind == Kind::Video && tag.key) {
                return;
            }
            let (Some(vs), Some(aseq)) = (&headers.video, &headers.audio) else {
                return;
            };
            self.push_bytes(flv::file_header(true, true), 0);
            self.push_bytes(headers.meta.bytes(0), 0);
            self.push_bytes(vs.bytes(0), 0);
            self.push_bytes(aseq.bytes(0), 0);
            self.started = true;
            tracing::debug!(
                at_ms = tag.dts_ms,
                "rtmp: a destination starts at a keyframe"
            );
            self.base_ms = tag.dts_ms;
            self.last_ts = 0;
        }
        let Some(rel) = tag.dts_ms.checked_sub(self.base_ms) else {
            return;
        };
        let ts = (rel.min(u64::from(u32::MAX)) as u32).max(self.last_ts);
        self.last_ts = ts;
        self.push_bytes(tag.bytes(ts), ts);
    }
}

/// The headers every new connection is sent first.
#[derive(Default)]
struct Headers {
    meta: Tag,
    video: Option<Tag>,
    audio: Option<Tag>,
}

impl Default for Tag {
    fn default() -> Self {
        flv::metadata(&[])
    }
}

/// The tags of one profile's encode, shared by its sinks' callbacks and its destinations.
struct Shared {
    profile: Profile,
    headers: Mutex<Headers>,
    interleave: Mutex<Interleave>,
    feeds: Mutex<Vec<(u64, Arc<Mutex<Feed>>)>>,
    /// AV1's format from the caps, until the first keyframe gives its sequence header.
    av1: Mutex<Option<flv::Av1Format>>,
    tags: AtomicU64,
    /// The picture's and the sound's bitrates together, for a destination's backlog.
    kbps: AtomicU64,
}

impl Shared {
    fn emit(&self, tag: Tag) {
        self.tags.fetch_add(1, Ordering::Relaxed);
        let due = self.interleave.lock().expect("interleave").push(tag);
        if due.is_empty() {
            return;
        }
        let headers = self.headers.lock().expect("headers");
        tracing::trace!(
            n = due.len(),
            feeds = self.feeds.lock().expect("feeds").len(),
            "rtmp: tags due"
        );
        let feeds: Vec<Arc<Mutex<Feed>>> = self
            .feeds
            .lock()
            .expect("feeds")
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for t in &due {
            for f in &feeds {
                f.lock().expect("feed").take(t, &headers);
            }
        }
    }

    /// A picture from the encode's sink.
    fn on_video(&self, sample: &gst::Sample) {
        let (Some(buf), Some(caps)) = (sample.buffer(), sample.caps()) else {
            return;
        };
        let Some(s) = caps.structure(0) else { return };
        let Ok(map) = buf.map_readable() else { return };
        let pts = buf.pts().map(gst::ClockTime::mseconds);
        let dts = buf.dts().map(gst::ClockTime::mseconds).or(pts).unwrap_or(0);
        let key = !buf.flags().contains(gst::BufferFlags::DELTA_UNIT);
        match self.profile {
            Profile::Compatible => {
                {
                    let mut h = self.headers.lock().expect("headers");
                    if let Ok(cd) = s.get::<gst::Buffer>("codec_data")
                        && let Ok(m) = cd.map_readable()
                    {
                        let seq = flv::avc_sequence(m.as_slice());
                        if h.video.as_ref() != Some(&seq) {
                            h.video = Some(seq);
                        }
                    }
                    if h.video.is_none() {
                        return;
                    }
                }
                let cts = pts.map_or(0, |p| p as i64 - dts as i64) as i32;
                self.emit(flv::avc_frame(dts, key, cts, map.as_slice()));
            }
            Profile::Open => {
                if key && self.headers.lock().expect("headers").video.is_none() {
                    let format = {
                        let mut f = self.av1.lock().expect("av1 format");
                        if f.is_none() {
                            *f = flv::Av1Format::from_caps(
                                s.get::<&str>("profile").unwrap_or("main"),
                                s.get::<&str>("level").unwrap_or("3.1"),
                                s.get::<&str>("tier").unwrap_or("main"),
                                s.get::<u32>("bit-depth-luma").unwrap_or(8),
                                s.get::<&str>("chroma-format").unwrap_or("4:2:0"),
                            );
                        }
                        f.clone()
                    };
                    if let (Some(f), Some(sh)) = (format, flv::sequence_header(map.as_slice())) {
                        self.headers.lock().expect("headers").video =
                            Some(flv::av1_sequence(&flv::av1c(&f, sh)));
                    }
                }
                if self.headers.lock().expect("headers").video.is_none() {
                    return;
                }
                self.emit(flv::av1_frame(dts, key, map.as_slice()));
            }
        }
    }

    /// A block of sound from the encode's sink.
    fn on_audio(&self, sample: &gst::Sample) {
        let (Some(buf), Some(caps)) = (sample.buffer(), sample.caps()) else {
            return;
        };
        let Some(s) = caps.structure(0) else { return };
        let Ok(map) = buf.map_readable() else { return };
        let dts = buf.pts().map(gst::ClockTime::mseconds).unwrap_or(0);
        {
            let mut h = self.headers.lock().expect("headers");
            if h.audio.is_none() {
                let seq = match self.profile {
                    Profile::Compatible => s.get::<gst::Buffer>("codec_data").ok().and_then(|b| {
                        b.map_readable()
                            .ok()
                            .map(|m| flv::aac_sequence(m.as_slice()))
                    }),
                    Profile::Open => s
                        .get::<gst::Array>("streamheader")
                        .ok()
                        .and_then(|a| a.first().and_then(|v| v.get::<gst::Buffer>().ok()))
                        .and_then(|b| {
                            b.map_readable()
                                .ok()
                                .map(|m| flv::opus_sequence(m.as_slice()))
                        }),
                };
                h.audio = seq;
            }
            if h.audio.is_none() {
                return;
            }
        }
        // Opus's header packets also come through as buffers: they are in the sequence start.
        if buf.flags().contains(gst::BufferFlags::HEADER) {
            return;
        }
        let tag = match self.profile {
            Profile::Compatible => flv::aac_frame(dts, map.as_slice()),
            Profile::Open => flv::opus_frame(dts, map.as_slice()),
        };
        self.emit(tag);
    }
}

/// One profile's encode pipeline.
struct Encode {
    pipeline: gst::Pipeline,
    picture: gst_app::AppSrc,
    shared: Arc<Shared>,
    status: EncodeStatus,
    nvenc: bool,
}

/// One destination's sending pipeline.
struct Sender {
    pipeline: gst::Pipeline,
    sink: Option<gst::Element>,
    feed: Arc<Mutex<Feed>>,
    feed_id: u64,
    since: Instant,
}

struct Dest {
    d: Destination,
    sink: SinkKind,
    sender: Option<Sender>,
    failures: u32,
    retry_at: Option<Instant>,
    last_error: Option<(String, Instant)>,
    bytes: u64,
    bytes_total: u64,
    connects: u32,
}

struct State {
    set: Option<String>,
    dests: Vec<Dest>,
    encodes: [Option<Encode>; 2],
    encode_retry: [Option<Instant>; 2],
    next_feed: u64,
}

struct Inner {
    settings: Settings,
    canvas: Rung,
    state: Mutex<State>,
}

/// RTMP out. Cheap to clone.
#[derive(Clone)]
pub struct Rtmp {
    inner: Arc<Inner>,
}

impl Rtmp {
    pub fn new(settings: &Settings, canvas: Rung) -> Rtmp {
        Rtmp {
            inner: Arc::new(Inner {
                settings: settings.clone(),
                canvas,
                state: Mutex::new(State {
                    set: None,
                    dests: Vec::new(),
                    encodes: [None, None],
                    encode_retry: [None, None],
                    next_feed: 0,
                }),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().expect("rtmp state")
    }

    /// The picture RTMP sends.
    pub fn rung(&self) -> Rung {
        self.inner.settings.rtmp_rung()
    }

    /// Take the control plane's destinations, named `set`. A destination whose URL, key or
    /// profile changed connects again; one turned off stops; the rest go on as they are.
    pub fn set_destinations(&self, set: &str, list: Vec<Destination>) -> Result<(), String> {
        for d in &list {
            d.check()?;
        }
        let mut names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
        names.sort_unstable();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return Err("two destinations have the same name".into());
        }
        self.apply(set, list, SinkKind::Rtmp);
        Ok(())
    }

    /// Tests: a destination whose URL is `rtmp://unused/…` writes its tags to `<name>.flv` in
    /// `dir` instead of sending them; the others are sent as usual.
    #[doc(hidden)]
    pub fn set_destinations_to_files(
        &self,
        set: &str,
        list: Vec<Destination>,
        dir: &std::path::Path,
    ) {
        let sinks: Vec<SinkKind> = list
            .iter()
            .map(|d| {
                if d.url.starts_with("rtmp://unused/") {
                    SinkKind::File(dir.join(format!("{}.flv", d.name)))
                } else {
                    SinkKind::Rtmp
                }
            })
            .collect();
        self.apply_with(set, list, sinks);
    }

    fn apply(&self, set: &str, list: Vec<Destination>, sink: SinkKind) {
        let sinks = vec![sink; list.len()];
        self.apply_with(set, list, sinks);
    }

    fn apply_with(&self, set: &str, list: Vec<Destination>, sinks: Vec<SinkKind>) {
        let mut s = self.state();
        s.set = Some(set.to_owned());
        let mut old: Vec<Dest> = std::mem::take(&mut s.dests);
        let mut next = Vec::with_capacity(list.len());
        for (d, sink) in list.into_iter().zip(sinks) {
            if let Some(i) = old.iter().position(|o| o.d.name == d.name) {
                let mut o = old.swap_remove(i);
                let same = o.d.url == d.url
                    && o.d.key == d.key
                    && o.d.profile == d.profile
                    && o.sink == sink;
                if !same || !d.enabled {
                    stop_sender(o.sender.take());
                    o.failures = 0;
                    o.retry_at = None;
                }
                if !same {
                    o.last_error = None;
                }
                o.d = d;
                o.sink = sink;
                next.push(o);
            } else {
                next.push(new_dest(d, sink));
            }
        }
        for o in old {
            stop_sender(o.sender);
        }
        tracing::info!(
            set,
            destinations = ?next.iter().map(|d| (d.d.name.as_str(), d.d.profile, d.d.enabled)).collect::<Vec<_>>(),
            "rtmp: destinations"
        );
        s.dests = next;
    }

    /// The frame the picture pass sends HLS, after the delay (from its thread).
    pub fn picture(&self, buffer: &gst::Buffer) {
        let srcs: Vec<gst_app::AppSrc> = self
            .state()
            .encodes
            .iter()
            .flatten()
            .map(|e| e.picture.clone())
            .collect();
        for s in srcs {
            let _ = s.push_buffer(buffer.clone());
        }
    }

    pub fn status(&self) -> RtmpStatus {
        let now = Instant::now();
        let mut s = self.state();
        let mut out = Vec::new();
        for d in s.dests.iter_mut() {
            let (connected, live_ms, bytes) = match &d.sender {
                Some(snd) => {
                    let (inb, outb) = sink_bytes(snd.sink.as_ref());
                    (
                        inb > 0,
                        Some(now.duration_since(snd.since).as_millis() as u64),
                        outb,
                    )
                }
                None => (false, None, 0),
            };
            if d.sender.is_some() {
                d.bytes = bytes;
            }
            let state = if !d.d.enabled {
                SendState::Off
            } else if d.sender.is_some() {
                if matches!(d.sink, SinkKind::File(_)) || connected {
                    SendState::Live
                } else {
                    SendState::Connecting
                }
            } else if d.retry_at.is_some_and(|t| t > now) {
                SendState::Retrying
            } else {
                SendState::Waiting
            };
            out.push(DestinationStatus {
                name: d.d.name.clone(),
                profile: d.d.profile,
                enabled: d.d.enabled,
                state,
                connected: connected && d.sender.is_some(),
                connected_ms: if connected { live_ms } else { None },
                last_error: d.last_error.as_ref().map(|(e, _)| e.clone()),
                last_error_ms_ago: d
                    .last_error
                    .as_ref()
                    .map(|(_, at)| now.duration_since(*at).as_millis() as u64),
                bytes: if d.sender.is_some() { d.bytes } else { 0 },
                bytes_total: d.bytes_total + if d.sender.is_some() { d.bytes } else { 0 },
                connects: d.connects,
                retry_in_ms: d
                    .retry_at
                    .filter(|t| *t > now && d.sender.is_none())
                    .map(|t| t.duration_since(now).as_millis() as u64),
            });
        }
        RtmpStatus {
            set: s.set.clone(),
            destinations: out,
            encoders: s
                .encodes
                .iter()
                .flatten()
                .map(|e| {
                    let h = e.shared.headers.lock().expect("headers");
                    EncodeStatus {
                        ready: h.video.is_some() && h.audio.is_some(),
                        tags: e.shared.tags.load(Ordering::Relaxed),
                        ..e.status.clone()
                    }
                })
                .collect(),
        }
    }

    /// Start, check and stop what is needed: called from the picture pass's housekeeping.
    pub fn housekeeping(&self, desk: &dyn DeskSide) {
        let now = Instant::now();
        let settings = &self.inner.settings.rtmp;
        for profile in Profile::ALL {
            let slot = profile.slot();
            let wanted = self
                .state()
                .dests
                .iter()
                .any(|d| d.d.enabled && d.d.profile == profile);
            // A failed encode stops; it is made again after 2 s.
            let failed = self.state().encodes[slot]
                .as_ref()
                .and_then(|e| v::bus_error(&e.pipeline));
            if let Some(e) = failed {
                tracing::error!(?profile, error = %e, "rtmp: the encoder failed; HLS and the sound go on, it starts again in 2 s");
                self.stop_encode(profile, desk, true);
            }
            let running = self.state().encodes[slot].is_some();
            if wanted && !running {
                let due = self.state().encode_retry[slot].is_none_or(|t| now >= t);
                if due {
                    match self.start_encode(profile, desk) {
                        Ok(()) => self.state().encode_retry[slot] = None,
                        Err(e) => {
                            tracing::error!(?profile, error = %format!("{e:#}"), "rtmp: cannot start the encoder; trying again in 5 s");
                            self.state().encode_retry[slot] = Some(now + Duration::from_secs(5));
                        }
                    }
                }
            } else if !wanted && running {
                self.stop_encode(profile, desk, false);
            }
        }
        // The destinations.
        let mut s = self.state();
        let shared: [Option<Arc<Shared>>; 2] = [
            s.encodes[0].as_ref().map(|e| e.shared.clone()),
            s.encodes[1].as_ref().map(|e| e.shared.clone()),
        ];
        let mut next_feed = s.next_feed;
        for d in s.dests.iter_mut() {
            // A failed or too-slow sender stops and waits.
            let failure =
                d.sender.as_ref().and_then(|snd| {
                    v::bus_error(&snd.pipeline)
                        .map(|e| flv::redact(&e, &d.d.key))
                        .or_else(|| {
                            snd.feed.lock().expect("feed").overrun.then(|| {
                                "the destination did not keep up with the stream".to_owned()
                            })
                        })
                });
            if let Some(e) = failure {
                let lived = d.sender.as_ref().map(|snd| now.duration_since(snd.since));
                if lived.is_some_and(|l| l > Duration::from_secs(30)) {
                    d.failures = 0;
                }
                d.failures += 1;
                let wait = flv::backoff_s(d.failures, settings.retry_min_s, settings.retry_max_s);
                tracing::warn!(destination = %d.d.name, error = %e, retry_s = wait, "rtmp: a destination failed; the others, HLS and the sound go on");
                d.last_error = Some((short_error(&e), now));
                d.retry_at = Some(now + Duration::from_secs(u64::from(wait)));
                let snd = d.sender.take();
                if let Some(snd) = &snd {
                    detach(&shared[d.d.profile.slot()], snd.feed_id);
                }
                d.bytes_total += d.bytes;
                d.bytes = 0;
                stop_sender(snd);
            }
            let encode = &shared[d.d.profile.slot()];
            if !d.d.enabled || encode.is_none() {
                if let Some(snd) = d.sender.take() {
                    detach(encode, snd.feed_id);
                    d.bytes_total += d.bytes;
                    d.bytes = 0;
                    stop_sender(Some(snd));
                }
                continue;
            }
            if d.sender.is_none() && d.retry_at.is_none_or(|t| now >= t) {
                let encode = encode.as_ref().expect("checked above");
                match start_sender(d, settings.backlog_ms, encode) {
                    Ok(snd) => {
                        next_feed += 1;
                        encode
                            .feeds
                            .lock()
                            .expect("feeds")
                            .push((next_feed, snd.feed.clone()));
                        d.sender = Some(Sender {
                            feed_id: next_feed,
                            ..snd
                        });
                        d.connects += 1;
                        tracing::info!(destination = %d.d.name, profile = ?d.d.profile, "rtmp: connecting");
                    }
                    Err(e) => {
                        let e = flv::redact(&format!("{e:#}"), &d.d.key);
                        d.failures += 1;
                        let wait =
                            flv::backoff_s(d.failures, settings.retry_min_s, settings.retry_max_s);
                        tracing::warn!(destination = %d.d.name, error = %e, retry_s = wait, "rtmp: cannot start a destination");
                        d.last_error = Some((short_error(&e), now));
                        d.retry_at = Some(now + Duration::from_secs(u64::from(wait)));
                    }
                }
            }
        }
        s.next_feed = next_feed;
    }

    /// Stop every encode and destination (the desk is stopping, or video stopped).
    pub fn stop(&self, desk: Option<&dyn DeskSide>) {
        let mut s = self.state();
        for d in s.dests.iter_mut() {
            stop_sender(d.sender.take());
        }
        for slot in 0..2 {
            if let Some(e) = s.encodes[slot].take() {
                let _ = e.pipeline.set_state(gst::State::Null);
                if e.nvenc {
                    v::release_nvenc();
                }
            }
        }
        drop(s);
        if let Some(desk) = desk {
            for p in Profile::ALL {
                desk.rtmp_audio(p.slot(), None);
            }
        }
    }

    /// Fail a profile's encode, as a dying encoder would (tests).
    #[doc(hidden)]
    pub fn fail_encode(&self, profile: Profile) -> bool {
        let s = self.state();
        let Some(e) = s.encodes[profile.slot()].as_ref() else {
            return false;
        };
        let msg =
            gst::message::Error::builder(gst::CoreError::Failed, "failure injected by a test")
                .src(&e.pipeline)
                .build();
        e.pipeline.post_message(msg).is_ok()
    }

    fn stop_encode(&self, profile: Profile, desk: &dyn DeskSide, failed: bool) {
        desk.rtmp_audio(profile.slot(), None);
        let mut s = self.state();
        let slot = profile.slot();
        if let Some(e) = s.encodes[slot].take() {
            let _ = e.pipeline.set_state(gst::State::Null);
            if e.nvenc {
                v::release_nvenc();
            }
        }
        if failed {
            s.encode_retry[slot] = Some(Instant::now() + Duration::from_secs(2));
        }
        // Its destinations connect again with the next encode's headers.
        for d in s.dests.iter_mut().filter(|d| d.d.profile == profile) {
            if let Some(snd) = d.sender.take() {
                d.bytes_total += d.bytes;
                d.bytes = 0;
                stop_sender(Some(snd));
            }
        }
    }

    fn start_encode(&self, profile: Profile, desk: &dyn DeskSide) -> Result<()> {
        let built = build_encode(&self.inner.settings, &self.inner.canvas, profile)?;
        let (encode, audio_pad) = built;
        if let Err(e) = encode.pipeline.set_state(gst::State::Playing) {
            let _ = encode.pipeline.set_state(gst::State::Null);
            if encode.nvenc {
                v::release_nvenc();
            }
            return Err(anyhow!("the RTMP encoder would not start: {e}"));
        }
        desk.rtmp_audio(profile.slot(), Some(audio_pad));
        tracing::info!(?profile, encoder = %encode.status.encoder, width = encode.status.width, height = encode.status.height, fps = encode.status.fps, kbps = encode.status.video_kbps, "rtmp: encoding");
        self.state().encodes[profile.slot()] = Some(encode);
        Ok(())
    }
}

/// The status's form of a GStreamer error: the message without the element's path and the
/// source file (the log keeps the whole line).
fn short_error(e: &str) -> String {
    let rest = match e.find(": ") {
        Some(i) if e.starts_with('/') => &e[i + 2..],
        _ => e,
    };
    let rest = rest.split(" (../").next().unwrap_or(rest);
    rest.trim().to_owned()
}

fn new_dest(d: Destination, sink: SinkKind) -> Dest {
    Dest {
        d,
        sink,
        sender: None,
        failures: 0,
        retry_at: None,
        last_error: None,
        bytes: 0,
        bytes_total: 0,
        connects: 0,
    }
}

fn detach(shared: &Option<Arc<Shared>>, id: u64) {
    if let Some(s) = shared {
        s.feeds.lock().expect("feeds").retain(|(i, _)| *i != id);
    }
}

fn stop_sender(s: Option<Sender>) {
    if let Some(s) = s {
        let _ = s.pipeline.set_state(gst::State::Null);
    }
}

/// `rtmp2sink`'s bytes in and out on this connection.
fn sink_bytes(sink: Option<&gst::Element>) -> (u64, u64) {
    let Some(sink) = sink else { return (0, 0) };
    let Some(stats) = sink.property::<Option<gst::Structure>>("stats") else {
        return (0, 0);
    };
    (
        stats.get::<u64>("in-bytes-total").unwrap_or(0),
        stats.get::<u64>("out-bytes-total").unwrap_or(0),
    )
}

/// One destination's pipeline: whole FLV tags into `rtmp2sink` (or a file, for tests).
fn start_sender(d: &Dest, backlog_ms: u32, encode: &Arc<Shared>) -> Result<Sender> {
    let p = gst::Pipeline::with_name(&format!("rtmp-out-{}", d.d.name));
    let src = gst_app::AppSrc::builder()
        .caps(&gst::Caps::builder("video/x-flv").build())
        .format(gst::Format::Time)
        .is_live(true)
        .build();
    // At the profile's own rate: the picture's bitrate and the sound's, for `backlog_ms`.
    let kbps = encode.kbps.load(Ordering::Relaxed).max(500);
    let backlog_bytes = kbps * 1000 / 8 * u64::from(backlog_ms) / 1000;
    src.set_max_bytes(backlog_bytes * 2);
    let (sink, rtmp) = match &d.sink {
        SinkKind::Rtmp => {
            let s = gst::ElementFactory::make("rtmp2sink")
                .property("location", d.d.location())
                .property("async-connect", true)
                .build()
                .context("GStreamer element `rtmp2sink` is missing (gst-plugins-bad rtmp2)")?;
            (s.clone(), Some(s))
        }
        SinkKind::File(path) => (
            gst::ElementFactory::make("filesink")
                .property("location", path.to_string_lossy().as_ref())
                .property("sync", false)
                .build()?,
            None,
        ),
    };
    p.add_many([src.upcast_ref(), &sink])?;
    gst::Element::link_many([src.upcast_ref(), &sink])?;
    p.set_state(gst::State::Playing)
        .map_err(|e| anyhow!("the destination's pipeline would not start: {e}"))?;
    Ok(Sender {
        pipeline: p,
        sink: rtmp,
        feed: Arc::new(Mutex::new(Feed {
            src,
            started: false,
            base_ms: 0,
            last_ts: 0,
            overrun: false,
            backlog_bytes,
        })),
        feed_id: 0,
        since: Instant::now(),
    })
}

fn appsink(name: &str, shared: &Arc<Shared>, video: bool) -> gst_app::AppSink {
    let sink = gst_app::AppSink::builder()
        .name(name)
        .sync(false)
        .max_buffers(0)
        .build();
    let sh = shared.clone();
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |s| {
                let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                if video {
                    sh.on_video(&sample);
                } else {
                    sh.on_audio(&sample);
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    sink
}

/// Scale and rate the canvas's frames down to the RTMP rung.
fn fit_chain(r: &Rung, canvas: &Rung) -> Result<Vec<gst::Element>> {
    let mut out = Vec::new();
    if (r.width, r.height) != (canvas.width, canvas.height) {
        out.push(v::el("videoscale")?);
        out.push(v::capsfilter(
            gst::Caps::builder("video/x-raw")
                .field("width", r.width as i32)
                .field("height", r.height as i32)
                .build(),
        )?);
    }
    if r.fps != canvas.fps {
        out.push(
            gst::ElementFactory::make("videorate")
                .property("drop-only", true)
                .build()?,
        );
        out.push(v::capsfilter(
            gst::Caps::builder("video/x-raw")
                .field("framerate", gst::Fraction::new(r.fps as i32, 1))
                .build(),
        )?);
    }
    Ok(out)
}

/// One profile's encode: the delayed picture and sound in, encoded frames to two app sinks.
/// The sound's sink pad goes to the mixing pass.
fn build_encode(
    settings: &Settings,
    canvas: &Rung,
    profile: Profile,
) -> Result<(Encode, gst::Pad)> {
    let r = settings.rtmp_rung();
    let rs = &settings.rtmp;
    let p = gst::Pipeline::with_name(match profile {
        Profile::Compatible => "rtmp-compatible",
        Profile::Open => "rtmp-open",
    });
    let picture = gst_app::AppSrc::builder()
        .name("rtmp-picture")
        .caps(&v::caps_raw(canvas))
        .format(gst::Format::Time)
        .is_live(false)
        .max_buffers(40)
        .build();
    picture.set_property_from_str("leaky-type", "downstream");
    let shared = Arc::new(Shared {
        profile,
        headers: Mutex::new(Headers::default()),
        interleave: Mutex::new(Interleave::new(800)),
        feeds: Mutex::new(Vec::new()),
        av1: Mutex::new(None),
        tags: AtomicU64::new(0),
        kbps: AtomicU64::new(0),
    });
    let gop = r.fps * rs.keyframe_s;
    let mut chain: Vec<gst::Element> = vec![picture.clone().upcast(), v::leaky(2_000)?];
    let (encoder, kbps, nvenc, audio_first, audio_chain);
    match profile {
        Profile::Compatible => {
            kbps = if rs.compatible.video_kbps == 0 {
                r.h264_kbps()
            } else {
                rs.compatible.video_kbps
            };
            chain.extend(fit_chain(&r, canvas)?);
            let use_nvenc = rs.compatible.h264 != H264Choice::Openh264
                && gst::ElementFactory::find("nvh264enc").is_some()
                && gst::ElementFactory::find("cudaupload").is_some()
                && v::take_nvenc(settings.nvenc_sessions);
            if rs.compatible.h264 == H264Choice::Nvenc && !use_nvenc {
                tracing::warn!(
                    "rtmp: NVENC was asked for H.264 but has no session free (or this machine has none); OpenH264 on the CPU encodes it"
                );
            }
            if use_nvenc {
                chain.push(v::el("videoconvert")?);
                chain.push(v::capsfilter(
                    gst::Caps::builder("video/x-raw")
                        .field("format", "NV12")
                        .build(),
                )?);
                chain.push(v::el("cudaupload")?);
                chain.push(
                    gst::ElementFactory::make("nvh264enc")
                        .property("bitrate", kbps)
                        .property("gop-size", gop as i32)
                        .property_from_str("rc-mode", "cbr")
                        .property_from_str("preset", "p4")
                        .property_from_str("tune", "ultra-low-latency")
                        .property("bframes", 0u32)
                        .property("zerolatency", true)
                        .property("vbv-buffer-size", kbps)
                        .build()
                        .context("GStreamer element `nvh264enc` is missing")?,
                );
                encoder = "nvenc";
            } else {
                chain.push(v::el("videoconvert")?);
                chain.push(
                    gst::ElementFactory::make("openh264enc")
                        .property("bitrate", kbps * 1000)
                        .property("max-bitrate", kbps * 1000)
                        .property_from_str("rate-control", "bitrate")
                        .property("gop-size", gop)
                        .property("scene-change-detection", false)
                        .build()
                        .context("GStreamer element `openh264enc` is missing (gstreamer1-plugin-openh264)")?,
                );
                encoder = "openh264";
            }
            nvenc = use_nvenc;
            chain.push(v::capsfilter(
                gst::Caps::builder("video/x-h264")
                    .field("profile", rs.compatible.h264_profile.as_str())
                    .build(),
            )?);
            chain.push(v::el("h264parse")?);
            chain.push(v::capsfilter(
                gst::Caps::builder("video/x-h264")
                    .field("stream-format", "avc")
                    .field("alignment", "au")
                    .build(),
            )?);
            let resample = v::el("audioresample")?;
            let conv = v::el("audioconvert")?;
            let rate = v::capsfilter(
                gst::Caps::builder("audio/x-raw")
                    .field("rate", rs.compatible.aac_rate as i32)
                    .field("channels", 2i32)
                    .build(),
            )?;
            // VisualOn's AAC encoder (vo-aacenc, Apache-2.0), by name: never fdk-aac
            // (ADR-0230), which the image does not carry. Raw AAC with its codec_data.
            let aac = gst::ElementFactory::make("voaacenc")
                .property("bitrate", (rs.compatible.aac_kbps * 1000) as i32)
                .build()
                .context("GStreamer element `voaacenc` is missing (the desk image builds it)")?;
            let parse = v::el("aacparse")?;
            let raw = v::capsfilter(
                gst::Caps::builder("audio/mpeg")
                    .field("mpegversion", 4i32)
                    .field("stream-format", "raw")
                    .build(),
            )?;
            audio_first = resample.clone();
            audio_chain = vec![resample, conv, rate, aac, parse, raw];
        }
        Profile::Open => {
            kbps = if rs.open.video_kbps == 0 {
                r.av1_kbps()
            } else {
                rs.open.video_kbps
            };
            let want_nvenc = settings.encoder != EncoderChoice::Svt
                && Encoder::nvenc_present()
                && v::take_nvenc(settings.nvenc_sessions);
            let enc = if want_nvenc {
                Encoder::Nvenc
            } else {
                Encoder::Svt
            };
            let rung = Rung { kbps, ..r };
            // The HLS rung's own chain: its leaky queue, the scaling, the encoder, av1parse.
            let mut av1 = v::encoder_chain(&rung, canvas, enc, rs.keyframe_s)?;
            av1.remove(0);
            chain.extend(av1);
            chain.push(v::capsfilter(
                gst::Caps::builder("video/x-av1")
                    .field("stream-format", "obu-stream")
                    .field("alignment", "tu")
                    .build(),
            )?);
            encoder = if want_nvenc { "nvenc" } else { "svt" };
            nvenc = want_nvenc;
            let opus = gst::ElementFactory::make("opusenc")
                .property("bitrate", (rs.open.opus_kbps * 1000) as i32)
                .property_from_str("frame-size", "20")
                .build()
                .context("GStreamer element `opusenc` is missing")?;
            audio_first = opus.clone();
            audio_chain = vec![opus];
        }
    }
    let vsink = appsink("rtmp-video", &shared, true);
    chain.push(vsink.upcast());
    let asink = appsink("rtmp-audio", &shared, false);
    let mut audio = audio_chain;
    audio.push(v::leaky(1_000)?);
    audio.push(asink.upcast());
    let built = (|| -> Result<()> {
        p.add_many(&chain)?;
        gst::Element::link_many(&chain)?;
        p.add_many(&audio)?;
        gst::Element::link_many(&audio)?;
        Ok(())
    })();
    if let Err(e) = built {
        if nvenc {
            v::release_nvenc();
        }
        return Err(e);
    }
    let (vcodec, acodec) = match profile {
        Profile::Compatible => (7.0, 10.0),
        Profile::Open => (
            flv::fourcc_number(&flv::AV1),
            flv::fourcc_number(&flv::OPUS),
        ),
    };
    let (arate, akbps) = match profile {
        Profile::Compatible => (rs.compatible.aac_rate, rs.compatible.aac_kbps),
        Profile::Open => (48_000, rs.open.opus_kbps),
    };
    shared.headers.lock().expect("headers").meta = flv::metadata(&[
        ("duration", Amf::Number(0.0)),
        ("width", Amf::Number(f64::from(r.width))),
        ("height", Amf::Number(f64::from(r.height))),
        ("framerate", Amf::Number(f64::from(r.fps))),
        ("videocodecid", Amf::Number(vcodec)),
        ("videodatarate", Amf::Number(f64::from(kbps))),
        ("audiocodecid", Amf::Number(acodec)),
        ("audiodatarate", Amf::Number(f64::from(akbps))),
        ("audiosamplerate", Amf::Number(f64::from(arate))),
        ("audiosamplesize", Amf::Number(16.0)),
        ("stereo", Amf::Bool(true)),
        ("encoder", Amf::Text("WeaveSignal".into())),
    ]);
    shared
        .kbps
        .store(u64::from(kbps + akbps), Ordering::Relaxed);
    let pad = audio_first
        .static_pad("sink")
        .ok_or_else(|| anyhow!("the RTMP sound encoder has no sink pad"))?;
    Ok((
        Encode {
            pipeline: p,
            picture,
            shared,
            status: EncodeStatus {
                profile,
                encoder: encoder.to_owned(),
                width: r.width,
                height: r.height,
                fps: r.fps,
                video_kbps: kbps,
                ready: false,
                tags: 0,
            },
            nvenc,
        },
        pad,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dest(name: &str, url: &str, key: &str) -> Destination {
        Destination {
            name: name.into(),
            url: url.into(),
            key: key.into(),
            profile: Profile::Compatible,
            enabled: true,
        }
    }

    #[test]
    fn a_destination_needs_an_rtmp_url_with_a_host_and_a_key_without_spaces() {
        assert!(
            dest("r", "rtmp://rtmp.rumble.com/live", "abc")
                .check()
                .is_ok()
        );
        assert!(
            dest("p", "rtmps://tube.example:1936/live/", "k")
                .check()
                .is_ok()
        );
        assert!(dest("x", "http://a/live", "k").check().is_err());
        assert!(dest("x", "rtmp:///live", "k").check().is_err());
        assert!(dest("x", "rtmp://a/live", "").check().is_err());
        assert!(dest("x", "rtmp://a/live", "a b").check().is_err());
        assert!(dest(" ", "rtmp://a/live", "k").check().is_err());
    }

    #[test]
    fn an_error_is_shown_without_the_element_path_or_the_source_file() {
        let e = "/GstPipeline:rtmp-out-x/GstRtmp2Sink:rtmp2sink3: Failed to connect: 'publish' cmd failed: connection closed remotely (../gst/rtmp2/gstrtmp2sink.c(1155): send_connect_error (): /GstPipeline:rtmp-out-x)";
        assert_eq!(
            short_error(e),
            "Failed to connect: 'publish' cmd failed: connection closed remotely"
        );
        assert_eq!(
            short_error("the destination did not keep up"),
            "the destination did not keep up"
        );
    }

    #[test]
    fn the_key_goes_on_the_end_of_the_url_and_never_into_debug_output() {
        let d = dest("rumble", "rtmp://rtmp.rumble.com/live/", "sk_live_123");
        assert_eq!(d.location(), "rtmp://rtmp.rumble.com/live/sk_live_123");
        let shown = format!("{d:?}");
        assert!(!shown.contains("sk_live_123"), "{shown}");
        assert!(shown.contains("rtmp.rumble.com"));
    }

    #[test]
    fn a_new_connection_starts_at_a_keyframe_after_the_headers_with_time_from_zero() {
        gst::init().unwrap();
        let src = gst_app::AppSrc::builder().build();
        let mut f = Feed {
            src: src.clone(),
            started: false,
            base_ms: 0,
            last_ts: 0,
            overrun: false,
            backlog_bytes: 1 << 30,
        };
        let h = Headers {
            meta: flv::metadata(&[]),
            video: Some(flv::avc_sequence(&[1])),
            audio: Some(flv::aac_sequence(&[0x12, 0x10])),
        };
        f.take(&flv::aac_frame(1000, &[1]), &h);
        f.take(&flv::avc_frame(1010, false, 0, &[1]), &h);
        assert!(!f.started, "nothing before a keyframe");
        f.take(&flv::avc_frame(1050, true, 0, &[1]), &h);
        assert!(f.started);
        assert_eq!(f.base_ms, 1050);
        f.take(&flv::aac_frame(1040, &[1]), &h);
        f.take(&flv::aac_frame(1090, &[1]), &h);
        assert_eq!(f.last_ts, 40);
        // header, metadata, two sequence headers, the keyframe and the sound after it.
        assert_eq!(src.current_level_buffers(), 6);
    }

    #[test]
    fn a_destination_that_does_not_keep_up_is_marked() {
        gst::init().unwrap();
        let src = gst_app::AppSrc::builder().build();
        let mut f = Feed {
            src,
            started: false,
            base_ms: 0,
            last_ts: 0,
            overrun: false,
            backlog_bytes: 10,
        };
        let h = Headers {
            meta: flv::metadata(&[]),
            video: Some(flv::avc_sequence(&[1])),
            audio: Some(flv::aac_sequence(&[0x12, 0x10])),
        };
        f.take(&flv::avc_frame(0, true, 0, &[0; 64]), &h);
        f.take(&flv::avc_frame(50, false, 0, &[0; 64]), &h);
        assert!(f.overrun);
    }
}
