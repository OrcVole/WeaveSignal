// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk in a real LiveKit room, sending to real Liquidsoap: M1's room checks on loopback.
//! Ignored unless the stack is up; `just test-room` starts it and runs this in the desk image.
//!
//! Three people join with tokens signed as the control plane signs them, each carrying a
//! role: a presenter clicking at 1 kHz every 2 s, a guest at 3 kHz every 2.9 s, and a caller at
//! 5 kHz every 2.5 s. The desk is told nobody's name: it finds them in the room, as the
//! binary does. An "ear" per person (a monitor) subscribes to their mix-minus after a real
//! round trip; an ordinary token on the caller's mix-minus must get nothing (ADR-0118).
//! The caller waits off air until 30 s; the guest's link drops at 40 s and comes back at
//! 46 s. A listener joins icecast.server late and decodes the Opus stream. Every click is
//! stamped on arrival on one monotonic clock, as spike 01 measured.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{
    Chunk, Onset, Tap, delays_ms, median, onsets, ratio_db, tone_power,
};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, ErrorScope, Outputs, RAW_CAPS};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::{Keys, Role};

/// People publish cameras (`WEAVESIGNAL_ROOM_VIDEO=1`, or `cameras` for cameras with the
/// desk's video off: which side any change comes from).
fn video_on() -> bool {
    std::env::var("WEAVESIGNAL_ROOM_VIDEO").is_ok_and(|v| v == "1" || v == "cameras")
}

fn desk_video_on() -> bool {
    std::env::var("WEAVESIGNAL_ROOM_VIDEO").is_ok_and(|v| v == "1")
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run `just test-room`"))
}

struct Room {
    url: String,
    keys: Keys,
    name: String,
}

impl Room {
    /// A person joins with a token carrying their role, as the control plane issues it to a
    /// native publisher: publish-only, since a `livekitwebrtcsink` holding the subscribe grant
    /// sends no media (see `Keys::participant_with`); each person's ear listens separately.
    fn join_as(&self, el: &gst::Element, identity: &str, role: Role) {
        // A caller joins their own booth (ADR-0193), anyone else the studio room.
        let room = weavesignal_proto::room_for(&self.name, role, identity);
        let token = self
            .keys
            .participant_with(&room, identity, identity, role, 600, false)
            .unwrap();
        let sig = el.property::<glib::Object>("signaller");
        sig.set_property("ws-url", &self.url);
        sig.set_property("auth-token", &token);
        sig.set_property("room-name", &room);
    }

    /// The room a person of this role is in: a caller's booth, or the studio room.
    fn room_of(&self, identity: &str, role: Role) -> String {
        weavesignal_proto::room_for(&self.name, role, identity)
    }

    /// Every identity LiveKit lists in one room.
    fn identities_in(&self, room: &str) -> Vec<String> {
        let api = format!(
            "{}/twirp/livekit.RoomService/ListParticipants",
            self.url.replacen("ws://", "http://", 1)
        );
        let token = self.keys.room_admin(room, 60);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .into();
        let Ok(mut res) = agent
            .post(&api)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(serde_json::json!({ "room": room }))
        else {
            return Vec::new();
        };
        let body = res.body_mut().read_to_string().unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let mut ids: Vec<String> = v["participants"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["identity"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// A monitor (a listener the station's admin asked for; the desk ignores it), subscribed
    /// to one producer: it hears a mix-minus that lets only its own person subscribe.
    fn ear(&self, el: &gst::Element, identity: &str, producer: &str, room: &str) {
        let token = self.keys.monitor(room, identity, identity, 600).unwrap();
        self.listen(el, &token, producer, room);
    }

    /// Someone with an ordinary token (a listener's role, so the desk ignores it) trying to
    /// hear a producer that does not allow them: what a modified page could try (ADR-0118).
    fn intruder(&self, el: &gst::Element, identity: &str, producer: &str, room: &str) {
        let token = self
            .keys
            .participant_with(room, identity, identity, Role::Listener, 600, true)
            .unwrap();
        self.listen(el, &token, producer, room);
    }

    fn listen(&self, el: &gst::Element, token: &str, producer: &str, room: &str) {
        let sig = el.property::<glib::Object>("signaller");
        sig.set_property("ws-url", &self.url);
        sig.set_property("auth-token", token);
        sig.set_property("room-name", room);
        sig.set_property("producer-peer-id", producer);
    }
}

/// A clicking person published into the room from a pipeline of their own, as each person
/// is their own client; and a tap on what they sent.
fn publish(
    room: &Room,
    identity: &str,
    role: Role,
    click: (f64, u64, u32),
    serial: u32,
) -> (gst::Pipeline, Tap) {
    let (freq, every_ms, periods) = click;
    let p = gst::Pipeline::with_name(&format!("person-{identity}-{serial}"));
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property_from_str("wave", "ticks")
        .property("tick-interval", every_ms * 1_000_000)
        .property("freq", freq)
        .property("volume", 0.8f64)
        .property("sine-periods-per-tick", periods)
        .property("samplesperbuffer", 480i32)
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property("caps", RAW_CAPS.parse::<gst::Caps>().unwrap())
        .build()
        .unwrap();
    let tee = gst::ElementFactory::make("tee").build().unwrap();
    let q = gst::ElementFactory::make("queue").build().unwrap();
    let sink = gst::ElementFactory::make("livekitwebrtcsink")
        .name(format!("pub-{identity}-{serial}"))
        .build()
        .unwrap();
    room.join_as(&sink, identity, role);
    let els = [src, caps, tee.clone(), q, sink.clone()];
    p.add_many(&els).unwrap();
    gst::Element::link_many(&els).unwrap();
    if video_on() {
        // A camera beside the voice (WEAVESIGNAL_ROOM_VIDEO=1): the room's figures with video on.
        // Not a live source: a live videotestsrc reports a frame's latency (50 ms at 20 fps),
        // and the person's pipeline then holds their voice back 40 ms to match it, which a
        // browser, capturing each on its own, does not. Paced to the clock instead.
        let cam = gst::ElementFactory::make("videotestsrc")
            .property("is-live", false)
            .property_from_str("pattern", "smpte")
            .build()
            .unwrap();
        let pace = gst::ElementFactory::make("clocksync").build().unwrap();
        let vcaps = gst::ElementFactory::make("capsfilter")
            .property(
                "caps",
                "video/x-raw,width=640,height=360,framerate=20/1"
                    .parse::<gst::Caps>()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let vq = gst::ElementFactory::make("queue").build().unwrap();
        p.add_many([&cam, &vcaps, &pace, &vq]).unwrap();
        gst::Element::link_many([&cam, &vcaps, &pace, &vq]).unwrap();
        vq.link(&sink).unwrap();
        // VP8, as a browser other than Chromium sends it, and as the cameras test sends it.
        sink.set_property("video-caps", "video/x-vp8".parse::<gst::Caps>().unwrap());
    }
    let tap = Tap::attach(&p, &tee, &format!("sent-{identity}-{serial}")).unwrap();
    (p, tap)
}

/// What one person hears: their mix-minus, subscribed from the room. The tap appears when
/// the room hands the subscriber a pad.
type EarSlot = Arc<Mutex<Option<Tap>>>;

fn ear(world: &gst::Pipeline, room: &Room, of: &str, role: Role) -> EarSlot {
    let src = gst::ElementFactory::make("livekitwebrtcsrc")
        .name(format!("ear-{of}"))
        .build()
        .unwrap();
    room.ear(
        &src,
        &format!("ear-{of}"),
        &format!("desk-mm-{of}"),
        &room.room_of(of, role),
    );
    let slot = EarSlot::default();
    let world_weak = world.downgrade();
    let slot2 = slot.clone();
    let name = format!("heard-by-{of}");
    src.connect_pad_added(move |_, pad| {
        let Some(world) = world_weak.upgrade() else {
            return;
        };
        let mut slot = slot2.lock().unwrap();
        if pad.name().starts_with("audio") && slot.is_none() {
            *slot = Some(Tap::attach_pad(&world, pad, &name).unwrap());
        } else {
            let fs = gst::ElementFactory::make("fakesink").build().unwrap();
            world.add(&fs).unwrap();
            fs.sync_state_with_parent().unwrap();
            pad.link(&fs.static_pad("sink").unwrap()).unwrap();
        }
    });
    world.add(&src).unwrap();
    slot
}

/// Someone listening where they are not allowed: counts the media pads the room hands them,
/// which should stay at none.
fn intruder(
    world: &gst::Pipeline,
    room: &Room,
    of: &str,
    role: Role,
) -> Arc<std::sync::atomic::AtomicUsize> {
    let src = gst::ElementFactory::make("livekitwebrtcsrc")
        .name(format!("intruder-{of}"))
        .build()
        .unwrap();
    // Inside the person's own room (a caller's booth): the strongest place to try from.
    room.intruder(
        &src,
        &format!("intruder-{of}"),
        &format!("desk-mm-{of}"),
        &room.room_of(of, role),
    );
    let pads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (world_weak, pads2) = (world.downgrade(), pads.clone());
    src.connect_pad_added(move |_, pad| {
        pads2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let Some(world) = world_weak.upgrade() else {
            return;
        };
        let fs = gst::ElementFactory::make("fakesink").build().unwrap();
        world.add(&fs).unwrap();
        fs.sync_state_with_parent().unwrap();
        let _ = pad.link(&fs.static_pad("sink").unwrap());
    });
    world.add(&src).unwrap();
    pads
}

fn listener(url: &str) -> (gst::Pipeline, Tap) {
    // iradio-mode=false: otherwise souphttpsrc asks for ICY metadata, and icecast.server
    // splices ICY blocks into the Ogg stream (bench 02). No do-timestamp: it turns the
    // source's segment into TIME, oggdemux takes its seqnum only from a BYTES segment, and
    // each chain's segment and EOS then raise a gst_event_set_seqnum CRITICAL.
    let p = gst::Pipeline::with_name("listener");
    let src = gst::ElementFactory::make("souphttpsrc")
        .property("location", url)
        .property("is-live", true)
        .property("iradio-mode", false)
        .build()
        .unwrap();
    let demux = gst::ElementFactory::make("oggdemux").build().unwrap();
    let dec = gst::ElementFactory::make("opusdec").build().unwrap();
    p.add_many([&src, &demux, &dec]).unwrap();
    src.link(&demux).unwrap();
    let dec_weak = dec.downgrade();
    demux.connect_pad_added(move |_, pad| {
        if let Some(dec) = dec_weak.upgrade() {
            let _ = pad.link(&dec.static_pad("sink").unwrap());
        }
    });
    let tap = Tap::attach(&p, &dec, "listener").unwrap();
    (p, tap)
}

fn first_error(p: &gst::Pipeline) -> Option<gst::Message> {
    let bus = p.bus()?;
    while let Some(msg) = bus.pop() {
        if let gst::MessageView::Error(_) = msg.view() {
            return Some(msg);
        }
    }
    None
}

fn describe(msg: &gst::Message) -> String {
    let gst::MessageView::Error(e) = msg.view() else {
        return String::new();
    };
    format!(
        "{}: {} ({:?})",
        msg.src()
            .map(|s| s.path_string().to_string())
            .unwrap_or_default(),
        e.error(),
        e.debug()
    )
}

#[derive(Debug)]
struct Stats {
    n: usize,
    p50: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}

impl Stats {
    fn of(v: &[f64]) -> Stats {
        let v: Vec<f64> = v.iter().skip(2).copied().collect(); // the first two are join transients
        Stats {
            n: v.len(),
            p50: median(&v),
            min: v.iter().copied().reduce(f64::min),
            max: v.iter().copied().reduce(f64::max),
        }
    }

    fn json(&self) -> String {
        let f = |x: Option<f64>| x.map_or("null".to_owned(), |v| format!("{v:.1}"));
        format!(
            "{{\"n\": {}, \"p50\": {}, \"min\": {}, \"max\": {}}}",
            self.n,
            f(self.p50),
            f(self.min),
            f(self.max)
        )
    }
}

/// The samples of a recording between `from` and `to` seconds after `t0`.
fn window(chunks: &[Chunk], t0: Instant, from: f64, to: f64) -> Vec<i16> {
    chunks
        .iter()
        .filter(|(t, _)| {
            let s = t.saturating_duration_since(t0).as_secs_f64();
            s >= from && s < to
        })
        .flat_map(|(_, s)| s.iter().copied())
        .collect()
}

/// A tone's level in a stretch of samples, in dB over silence.
fn level(s: &[i16], f: f64) -> f64 {
    ratio_db(tone_power(s, f), 1.0)
}

/// How loud one person's clicks come back in another recording, against how loud they were
/// sent, in dB: 0 dB is their voice at full level, -40 dB is 40 dB down. Each of their
/// clicks in `span` is looked for in the 100 ms from 20 ms after it was sent, at
/// its own frequency; clicks within 250 ms of anyone else's are skipped, so another voice's
/// click transients (which spread across the spectrum) are not counted as leakage.
fn return_db(
    sent: &[Chunk],
    others: &[Onset],
    recv: &[Chunk],
    freq: f64,
    span: (Instant, Instant),
) -> (f64, usize) {
    const N: usize = 4800;
    let own = onsets(sent, LOUD);
    let (mut got, mut gave, mut n) = (0.0, 0.0, 0);
    let ms = |m: u64| Duration::from_millis(m);
    for o in own.iter().filter(|o| o.at >= span.0 && o.at < span.1) {
        if others
            .iter()
            .any(|x| x.at + ms(250) > o.at && x.at < o.at + ms(250))
        {
            continue;
        }
        let cut = |chunks: &[Chunk], a: Instant, b: Instant| -> Vec<i16> {
            chunks
                .iter()
                .filter(|(t, _)| *t >= a && *t < b)
                .flat_map(|(_, s)| s.iter().copied())
                .collect()
        };
        let r = cut(recv, o.at + ms(20), o.at + ms(200));
        let s = cut(sent, o.at - ms(20), o.at + ms(200));
        if r.len() < N || s.len() < N {
            continue;
        }
        got += tone_power(&r[..N], freq);
        gave += tone_power(&s[..N], freq);
        n += 1;
    }
    (ratio_db(got, gave), n)
}

const LOUD: i16 = 3000;

const PRESENTER: (f64, u64, u32) = (1000.0, 2000, 10);
// 2.9 s against the presenter's 2 s: their clicks seldom coincide and merge in a mix.
const GUEST: (f64, u64, u32) = (3000.0, 2900, 30);
// Inside the phone band (300-3,400 Hz): callers sound like a phone line by default (2026-11-01).
const CALLER: (f64, u64, u32) = (2000.0, 2500, 20);
const CALLER_TO_AIR_S: f64 = 30.0;
const GUEST_LEAVES_S: f64 = 40.0;
const GUEST_BACK_S: f64 = 46.0;

#[test]
#[ignore = "needs the loopback stack: just test-room"]
fn the_desk_finds_three_people_in_the_room_and_puts_them_on_air() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    gst::init().unwrap();
    let seconds: f64 = std::env::var("WEAVESIGNAL_ROOM_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80.0);
    assert!(seconds >= 70.0, "the room test needs at least 70 s");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let room = Room {
        url: env("WEAVESIGNAL_ROOM_LIVEKIT_URL"),
        keys: Keys {
            api_key: env("WEAVESIGNAL_ROOM_API_KEY"),
            api_secret: env("WEAVESIGNAL_ROOM_API_SECRET"),
        },
        // A fresh room per run: LiveKit keeps departed identities briefly (spike 01).
        name: format!("m1-room-{stamp}"),
    };
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();

    let mut config = Config::from_toml(
        &format!(
            r#"
[livekit]
url = "{}"
api_key = "{}"
room = "{}"

[programme]
srt_port = {srt_port}

[programme.delay]
target_ms = 0

[room]
poll_ms = 300

# Clicks are not speech: no input processing, so levels stay as sent.
[inputs.presenter]
{video}
"#,
            room.url,
            room.keys.api_key,
            room.name,
            video = if desk_video_on() {
                format!(
                    "\n[video]\nenabled = true\n\n[video.hls]\ndir = \"{}/hls-{}\"\n",
                    std::env::var("WEAVESIGNAL_STACK_DIR").unwrap_or_else(|_| "/tmp".into()),
                    room.name
                )
            } else {
                String::new()
            }
        ),
        "room test",
    )
    .unwrap();
    config.livekit.api_secret = room.keys.api_secret.clone();
    let jitter = config.desk.jitter_buffer_ms;

    // The desk, following the room as the binary does.
    let desk = Desk::new(config, Outputs::ALL).unwrap();
    let programme = desk.tap(&Bus::Programme).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (desk, stop) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(desk, api, stop))
    };

    // The people.
    let world = gst::Pipeline::with_name("world");
    let (presenter, sent_presenter) = publish(&room, "hamish", Role::Presenter, PRESENTER, 1);
    let (guest, sent_guest_1) = publish(&room, "gregor", Role::Guest, GUEST, 1);
    let (caller, sent_caller) = publish(&room, "callum", Role::Caller, CALLER, 1);
    let mut people = vec![presenter, guest.clone(), caller];
    let ear_guest = ear(&world, &room, "gregor", Role::Guest);
    let ear_presenter = ear(&world, &room, "hamish", Role::Presenter);
    let ear_caller = ear(&world, &room, "callum", Role::Caller);
    // The caller's return carries the screener's private talk: no one else may hear it, even
    // from inside the caller's booth.
    let intruder_on_caller = intruder(&world, &room, "callum", Role::Caller);
    let mut booth_checked = false;

    let t0 = Instant::now();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    world.set_state(gst::State::Playing).unwrap();
    for p in &people {
        p.set_state(gst::State::Playing).unwrap();
    }
    let at = |s: f64| t0 + Duration::from_secs_f64(s);

    // A listener joins late, as icecast.server's pre-buffer allows (about 12 s after the
    // source connects), and retries until the mount answers.
    let listen_url = env("WEAVESIGNAL_ROOM_LISTENER_URL");
    let mut lis: Option<(gst::Pipeline, Tap)> = None;
    let mut next_try = at(25.0);
    let mut caller_sent = false;
    let mut guest_gone = false;
    let mut sent_guest_2: Option<Tap> = None;
    let mut participant_errors = Vec::new();
    while t0.elapsed() < Duration::from_secs_f64(seconds) {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(msg) = first_error(desk.pipeline()) {
            match desk.handle_error(&msg) {
                ErrorScope::Participant(id) => {
                    println!(
                        "[{:6.1}] participant error ({id}): {}",
                        t0.elapsed().as_secs_f64(),
                        describe(&msg)
                    );
                    participant_errors.push(id);
                }
                ErrorScope::Stale => {}
                ErrorScope::Fatal => panic!("desk error: {}", describe(&msg)),
            }
        }
        for p in people.iter().chain([&world]) {
            if let Some(msg) = first_error(p) {
                panic!("test participants error: {}", describe(&msg));
            }
        }
        if !caller_sent && Instant::now() >= at(CALLER_TO_AIR_S) {
            assert!(
                desk.set_on_air("callum", true),
                "the desk never found the caller"
            );
            caller_sent = true;
            println!("[{:6.1}] caller to air", t0.elapsed().as_secs_f64());
        }
        // ADR-0193: the caller is in a booth of their own; the studio room never lists them,
        // and the booth lists no one but them, the desk and the test's own two listeners.
        if caller_sent && !booth_checked {
            booth_checked = true;
            let studio = room.identities_in(&room.name);
            assert!(
                !studio.iter().any(|i| i == "callum"),
                "the caller is in the studio room: {studio:?}"
            );
            let booth = room.identities_in(&room.room_of("callum", Role::Caller));
            let strangers: Vec<&String> = booth
                .iter()
                .filter(|i| {
                    !(i.as_str() == "callum"
                        || i.starts_with("desk-")
                        || i.as_str() == "ear-callum"
                        || i.as_str() == "intruder-callum")
                })
                .collect();
            println!("the caller's booth: {booth:?}");
            assert!(
                strangers.is_empty(),
                "others in the caller's booth: {strangers:?}"
            );
            assert!(
                booth.iter().any(|i| i == "callum"),
                "the caller is not in their booth: {booth:?}"
            );
        }
        if !guest_gone && Instant::now() >= at(GUEST_LEAVES_S) {
            guest.set_state(gst::State::Null).unwrap();
            people.retain(|p| p != &guest);
            guest_gone = true;
            println!(
                "[{:6.1}] the guest's link drops",
                t0.elapsed().as_secs_f64()
            );
        }
        if guest_gone && sent_guest_2.is_none() && Instant::now() >= at(GUEST_BACK_S) {
            let (back, tap) = publish(&room, "gregor", Role::Guest, GUEST, 2);
            back.set_state(gst::State::Playing).unwrap();
            people.push(back);
            sent_guest_2 = Some(tap);
            println!("[{:6.1}] the guest rejoins", t0.elapsed().as_secs_f64());
        }
        if let Some((p, tap)) = &lis
            && tap.buffers() == 0
            && let Some(msg) = first_error(p)
        {
            println!(
                "[{:6.1}] listener: {}; retrying in 3 s",
                t0.elapsed().as_secs_f64(),
                describe(&msg)
            );
            p.set_state(gst::State::Null).unwrap();
            lis = None;
            next_try = Instant::now() + Duration::from_secs(3);
        }
        if lis.is_none() && Instant::now() >= next_try {
            let (p, tap) = listener(&listen_url);
            p.set_state(gst::State::Playing).unwrap();
            println!("[{:6.1}] listener started", t0.elapsed().as_secs_f64());
            lis = Some((p, tap));
            next_try = Instant::now() + Duration::from_secs(3600);
        }
    }
    let status = desk.status();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    watcher.join().unwrap();
    for p in people.iter().chain([&world]) {
        p.set_state(gst::State::Null).unwrap();
    }
    desk.pipeline().set_state(gst::State::Null).unwrap();
    let listener_tap = lis.map(|(p, t)| {
        p.set_state(gst::State::Null).unwrap();
        t
    });

    // --- analysis -----------------------------------------------------------------------
    let take = |slot: &EarSlot, who: &str| {
        slot.lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| panic!("{who}'s ear never received a pad from desk-mm-{who}"))
    };
    let ear_g = take(&ear_guest, "gregor");
    let ear_p = take(&ear_presenter, "hamish");
    let ear_c = take(&ear_caller, "callum");
    let stolen = intruder_on_caller.load(std::sync::atomic::Ordering::SeqCst);
    println!("an ordinary token on the caller's return: {stolen} pads (want 0)");
    assert_eq!(
        stolen, 0,
        "an ordinary token heard the caller's mix-minus (ADR-0118)"
    );
    let sent_p = onsets(&sent_presenter.chunks(), LOUD);
    let mut sent_g = onsets(&sent_guest_1.chunks(), LOUD);
    if let Some(t) = &sent_guest_2 {
        sent_g.extend(onsets(&t.chunks(), LOUD));
    }
    // The presenter's ear before the caller goes to air holds only the guest's high clicks.
    let ear_p_before: Vec<Chunk> = ear_p
        .chunks()
        .into_iter()
        .filter(|(t, _)| *t < at(CALLER_TO_AIR_S))
        .collect();
    let prog_before: Vec<Chunk> = programme
        .chunks()
        .into_iter()
        .filter(|(t, _)| *t < at(CALLER_TO_AIR_S))
        .collect();

    let guest_hears_presenter = Stats::of(&delays_ms(
        &sent_p,
        &onsets(&ear_g.chunks(), LOUD),
        false,
        1.9,
    ));
    let presenter_hears_guest =
        Stats::of(&delays_ms(&sent_g, &onsets(&ear_p_before, LOUD), true, 2.8));
    let caller_hears_presenter = Stats::of(&delays_ms(
        &sent_p,
        &onsets(&ear_c.chunks(), LOUD),
        false,
        1.9,
    ));
    let programme_presenter = Stats::of(&delays_ms(
        &sent_p,
        &onsets(&programme.chunks(), LOUD),
        false,
        1.9,
    ));
    let programme_guest = Stats::of(&delays_ms(&sent_g, &onsets(&prog_before, LOUD), true, 2.8));
    // Own voice and the caller's gate, by each person's own clicks (see `return_db`).
    let sent_c = onsets(&sent_caller.chunks(), LOUD);
    let all_but = |me: &[Onset]| -> Vec<Onset> {
        [&sent_p, &sent_g, &sent_c]
            .into_iter()
            .filter(|v| v.as_ptr() != me.as_ptr())
            .flat_map(|v| v.iter().copied())
            .collect()
    };
    let whole = (at(8.0), at(seconds));
    let off_air = (at(8.0), at(CALLER_TO_AIR_S - 0.3));
    let on_air = (at(CALLER_TO_AIR_S + 1.0), at(seconds));
    let (guest_own_db, n_g) = return_db(
        &sent_guest_1.chunks(),
        &all_but(&sent_g),
        &ear_g.chunks(),
        GUEST.0,
        (at(8.0), at(GUEST_LEAVES_S)),
    );
    let (presenter_own_db, n_p) = return_db(
        &sent_presenter.chunks(),
        &all_but(&sent_p),
        &ear_p.chunks(),
        PRESENTER.0,
        whole,
    );
    let (caller_own_db, n_c) = return_db(
        &sent_caller.chunks(),
        &all_but(&sent_c),
        &ear_c.chunks(),
        CALLER.0,
        whole,
    );
    let prog = programme.chunks();
    let caller_vs = |recv: &[Chunk], span| {
        return_db(
            &sent_caller.chunks(),
            &all_but(&sent_c),
            recv,
            CALLER.0,
            span,
        )
    };
    let (caller_off_in_programme, n_off) = caller_vs(&prog, off_air);
    let (caller_on_in_programme, n_on) = caller_vs(&prog, on_air);
    let (caller_off_to_presenter, _) = caller_vs(&ear_p.chunks(), off_air);
    let (caller_on_to_presenter, _) = caller_vs(&ear_p.chunks(), on_air);
    // The guest's return.
    let back = GUEST_BACK_S + 6.0;
    let pres_hears_guest_back = level(&window(&ear_p.chunks(), t0, back, seconds), 3000.0);
    let prog_guest_back = level(&window(&prog, t0, back, seconds), 3000.0);
    let guest_hears_after_return = level(&window(&ear_g.chunks(), t0, back, seconds), 1000.0);
    // The listener.
    let lis_s = listener_tap.as_ref().map_or(0.0, Tap::seconds);
    let lis_clicks = onsets(
        &listener_tap.as_ref().map(Tap::chunks).unwrap_or_default(),
        LOUD,
    );
    let lis_low = lis_clicks.iter().filter(|o| !o.high).count();
    let lis_high = lis_clicks.iter().filter(|o| o.high).count();
    let linked: Vec<String> = status
        .iter()
        .map(|s| {
            format!(
                "{}:{}:{} (ring {} ms, {} under, {} ms late)",
                s.identity,
                s.role,
                if s.linked { "linked" } else { "unlinked" },
                s.ring_ms,
                s.underruns,
                s.late_ms
            )
        })
        .collect();

    if let Some(v) = desk.video() {
        println!("video: {}", serde_json::to_string(&v.status()).unwrap());
    }
    let report = format!(
        "{{\n  \"room\": \"{}\",\n  \"seconds\": {seconds},\n  \"jitter_buffer_ms\": {jitter},\n  \"guest_hears_presenter_ms\": {},\n  \"presenter_hears_guest_ms\": {},\n  \"caller_hears_presenter_ms\": {},\n  \"programme_presenter_ms\": {},\n  \"programme_guest_ms\": {},\n  \"guest_own_voice_db\": {guest_own_db:.1},\n  \"presenter_own_voice_db\": {presenter_own_db:.1},\n  \"caller_own_voice_db\": {caller_own_db:.1},\n  \"own_voice_clicks\": {{\"guest\": {n_g}, \"presenter\": {n_p}, \"caller\": {n_c}}},\n  \"caller_in_programme_off_air_db\": {caller_off_in_programme:.1},\n  \"caller_in_programme_on_air_db\": {caller_on_in_programme:.1},\n  \"caller_to_presenter_off_air_db\": {caller_off_to_presenter:.1},\n  \"caller_to_presenter_on_air_db\": {caller_on_to_presenter:.1},\n  \"presenter_hears_guest_after_return_db\": {pres_hears_guest_back:.1},\n  \"programme_guest_after_return_db\": {prog_guest_back:.1},\n  \"guest_hears_presenter_after_return_db\": {guest_hears_after_return:.1},\n  \"participant_errors\": {},\n  \"desk_at_end\": {linked:?},\n  \"listener_seconds\": {lis_s:.1},\n  \"listener_clicks\": {{\"low\": {lis_low}, \"high\": {lis_high}}}\n}}\n",
        room.name,
        guest_hears_presenter.json(),
        presenter_hears_guest.json(),
        caller_hears_presenter.json(),
        programme_presenter.json(),
        programme_guest.json(),
        participant_errors.len(),
    );
    println!("{report}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, &report);
    }

    // --- M1's room checks -----------------------------------------------------------------
    let p50 = guest_hears_presenter
        .p50
        .expect("the guest heard no presenter clicks");
    assert!(
        guest_hears_presenter.n >= 10,
        "too few clicks matched: {}",
        guest_hears_presenter.n
    );
    assert!(
        p50 < 250.0,
        "the guest's mix-minus is {p50:.1} ms behind (M1: under 250 ms median)"
    );
    assert!(
        presenter_hears_guest.n >= 5,
        "the presenter heard too few guest clicks"
    );
    assert!(
        caller_hears_presenter.n >= 10,
        "the caller waiting did not hear the show"
    );
    assert!(
        guest_own_db <= -40.0,
        "the guest hears their own voice at {guest_own_db:.1} dB (M1: at least 40 dB down)"
    );
    assert!(
        presenter_own_db <= -40.0,
        "the presenter hears their own voice at {presenter_own_db:.1} dB"
    );
    assert!(
        caller_own_db <= -40.0,
        "the caller hears their own voice at {caller_own_db:.1} dB"
    );
    assert!(
        programme_presenter.n >= 10 && programme_guest.n >= 5,
        "the programme lacks a voice"
    );
    assert!(
        n_g >= 3 && n_p >= 3 && n_c >= 3 && n_off >= 3 && n_on >= 3,
        "too few lone clicks to measure own voice and the caller's gate"
    );
    assert!(
        caller_off_in_programme <= -40.0 && caller_off_to_presenter <= -40.0,
        "the caller was heard before going to air ({caller_off_in_programme:.1} dB in the programme, {caller_off_to_presenter:.1} dB to the presenter)"
    );
    assert!(
        caller_on_in_programme > -12.0 && caller_on_to_presenter > -12.0,
        "the caller sent to air is not heard ({caller_on_in_programme:.1} dB in the programme, {caller_on_to_presenter:.1} dB to the presenter)"
    );
    assert!(
        pres_hears_guest_back > 60.0 && prog_guest_back > 60.0,
        "the guest who rejoined is not mixed again ({pres_hears_guest_back:.1} dB to the presenter)"
    );
    assert!(
        guest_hears_after_return > 60.0,
        "the guest who rejoined does not hear the presenter"
    );
    assert!(
        status.iter().filter(|s| s.linked).count() == 3,
        "the desk does not have all three linked at the end: {linked:?}"
    );
    assert!(
        lis_s > 5.0,
        "the late listener decoded only {lis_s:.1} s of Opus"
    );
    assert!(
        lis_low >= 2 && lis_high >= 1,
        "the listener heard no clicks ({lis_low} low, {lis_high} high)"
    );
}
