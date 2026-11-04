// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Carts in the desk (ADR-0117), with real FLAC files decoded by the desk and local tone
//! sources in place of the room: a cart on air is heard in the programme and in every
//! return; a cued cart only in the presenter's ear; a bed loops without a gap and dips 12 dB
//! under an open microphone; a dumped cart never leaves the delay; the control socket's
//! words, busy players and play events. Needs GStreamer (run in the desk image:
//! `just desk-check`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, peak_dbfs, ratio_db, tone_power};
use weavesignal_desk::carts::{CartMode, Fire};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

static N: AtomicU32 = AtomicU32::new(1);

/// A lowercase UUID-shaped name, unique in this run.
fn uuid() -> String {
    let n = N.fetch_add(1, Ordering::SeqCst);
    format!(
        "{:08x}-0000-4000-8000-{:012x}",
        std::process::id(),
        u64::from(n)
    )
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("weavesignal-desk-carts-{}", uuid()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Write a FLAC tone as the media worker would leave it (its source's rate and channels):
/// `secs` of `freq` at `volume`. Returns the file's name.
fn flac(dir: &Path, freq: f64, secs: f64, rate: u32, channels: u32, volume: f64) -> String {
    gst::init().unwrap();
    let name = format!("{}.flac", uuid());
    let spb = rate / 100;
    let buffers = (secs * 100.0).round() as u32;
    let line = format!(
        "audiotestsrc num-buffers={buffers} samplesperbuffer={spb} freq={freq} volume={volume} ! audio/x-raw,format=S16LE,rate={rate},channels={channels} ! flacenc ! filesink location={}",
        dir.join(&name).display()
    );
    let p = gst::parse::launch(&line).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let bus = p.bus().unwrap();
    let msg = bus
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(30),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the FLAC was written");
    assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
    p.set_state(gst::State::Null).unwrap();
    name
}

fn config(dir: &Path, delay_ms: u32, srt_port: u16) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = {srt_port}

[programme.delay]
target_ms = {delay_ms}

[inputs.presenter]
[inputs.dj]

[carts]
dir = "{}"
"#,
            dir.display()
        ),
        "test",
    )
    .unwrap()
}

fn tone_at(pipeline: &gst::Pipeline, name: &str, freq: f64, volume: f64) -> gst::Pad {
    let bin = gst::Bin::with_name(name);
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property("freq", freq)
        .property("volume", volume)
        .property("samplesperbuffer", 480i32)
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property("caps", RAW_CAPS.parse::<gst::Caps>().unwrap())
        .build()
        .unwrap();
    bin.add_many([&src, &caps]).unwrap();
    src.link(&caps).unwrap();
    let ghost = gst::GhostPad::builder_with_target(&caps.static_pad("src").unwrap())
        .unwrap()
        .name("audio_0")
        .build();
    bin.add_pad(&ghost).unwrap();
    pipeline.add(&bin).unwrap();
    ghost.upcast()
}

fn run_for(p: &gst::Pipeline, d: Duration) {
    let deadline = Instant::now() + d;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        let bus = p.bus().unwrap();
        while let Some(msg) = bus.pop() {
            if let gst::MessageView::Error(e) = msg.view() {
                p.set_state(gst::State::Null).unwrap();
                panic!(
                    "pipeline error from {:?}: {}",
                    msg.src().map(|s| s.path_string()),
                    e.error()
                );
            }
        }
    }
}

struct Stop(gst::Pipeline, PathBuf);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn mm(id: &str) -> Bus {
    Bus::MixMinus(id.to_owned())
}

/// The cue test's cart: clear of the voices' tones and their harmonics.
const CUE_HZ: f64 = 4500.0;

/// Whether a tone is absent: under 30 dB over silence, or at least 60 dB under the loudest
/// voice on the bus (a window's leakage from a loud tone is not a cart), as tests/local.rs.
fn absent(t: &Tap, f: f64) -> bool {
    let s = t.samples();
    let me = ratio_db(tone_power(&s, f), 1.0);
    let loudest = [300.0, 1000.0]
        .iter()
        .map(|&g| ratio_db(tone_power(&s, g), 1.0))
        .fold(f64::MIN, f64::max);
    me < 30.0f64.max(loudest - 60.0)
}

/// A tone's power against silence, in dB: above about 60 dB it is there.
fn presence(t: &Tap, f: f64) -> f64 {
    ratio_db(tone_power(&t.samples(), f), 1.0)
}

fn fire(cart: &str, file: &str, mode: CartMode, cue: bool, gain_db: f32) -> Fire {
    Fire {
        cart: cart.to_owned(),
        file: file.to_owned(),
        gain_db,
        mode,
        cue,
        duration_ms: Some(3_000),
    }
}

/// The samples a tap recorded between two moments.
fn between(t: &Tap, from: Instant, to: Instant) -> Vec<i16> {
    t.chunks()
        .into_iter()
        .filter(|(at, _)| *at >= from && *at < to)
        .flat_map(|(_, s)| s)
        .collect()
}

#[test]
fn a_cart_on_air_is_heard_in_the_programme_and_every_return() {
    let dir = scratch();
    // 44.1 kHz mono, as an upload might come: the desk converts it to its 48 kHz stereo.
    let file = flac(&dir, 3000.0, 3.0, 44_100, 1, 0.4);
    let desk = Desk::new(config(&dir, 0, 1), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    for (id, role, f) in [
        ("presenter", Role::Presenter, 300.0),
        ("guest", Role::Guest, 1000.0),
    ] {
        desk.add_person(id, role).unwrap();
        desk.route_pad(id, &tone_at(&p, &format!("{id}-voice"), f, 0.3))
            .unwrap();
    }
    desk.add_person("caller", Role::Caller).unwrap();
    let taps = [
        ("programme", desk.tap(&Bus::Programme).unwrap()),
        ("presenter", desk.tap(&mm("presenter")).unwrap()),
        ("guest", desk.tap(&mm("guest")).unwrap()),
        ("caller waiting off air", desk.tap(&mm("caller")).unwrap()),
    ];
    p.set_state(gst::State::Playing).unwrap();
    run_for(&p, Duration::from_millis(300));
    let cart = uuid();
    let t0 = Instant::now();
    let fired = desk
        .fire_cart(&fire(&cart, &file, CartMode::OneShot, false, -6.0))
        .unwrap();
    let answered = t0.elapsed();
    assert_eq!((fired.player, fired.play), (1, 1));
    run_for(&p, Duration::from_millis(1200));
    let st = desk.carts();
    assert_eq!(st.sounding.len(), 1);
    assert!(st.sounding[0].remaining_ms.unwrap() < 2_000);
    for (who, t) in &taps {
        let cart_db = presence(t, 3000.0);
        println!("{who}: the cart at {cart_db:.0} dB (fire answered in {answered:?})");
        assert!(
            cart_db > 60.0,
            "{who} did not hear the cart: {cart_db:.1} dB"
        );
    }
    assert!(answered < Duration::from_millis(500));
}

#[test]
fn a_cued_cart_reaches_only_the_presenters_ear() {
    let dir = scratch();
    let file = flac(&dir, CUE_HZ, 3.0, 48_000, 2, 0.4);
    let desk = Desk::new(config(&dir, 0, 1), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    for (id, role, f) in [
        ("presenter", Role::Presenter, 300.0),
        ("guest", Role::Guest, 1000.0),
    ] {
        desk.add_person(id, role).unwrap();
        desk.route_pad(id, &tone_at(&p, &format!("{id}-voice"), f, 0.3))
            .unwrap();
    }
    desk.add_person("producer", Role::Producer).unwrap();
    let prog = desk.tap(&Bus::Programme).unwrap();
    let mmp = desk.tap(&mm("presenter")).unwrap();
    let mmg = desk.tap(&mm("guest")).unwrap();
    let mmprod = desk.tap(&mm("producer")).unwrap();
    let delayed = desk.tap_delayed();
    p.set_state(gst::State::Playing).unwrap();
    run_for(&p, Duration::from_millis(200));
    desk.fire_cart(&fire(&uuid(), &file, CartMode::OneShot, true, 0.0))
        .unwrap();
    run_for(&p, Duration::from_millis(1200));
    let cue = |t: &Tap| presence(t, CUE_HZ);
    println!(
        "cue: presenter {:.0} dB, guest {:.0} dB, producer {:.0} dB, programme {:.0} dB",
        cue(&mmp),
        cue(&mmg),
        cue(&mmprod),
        cue(&prog)
    );
    assert!(cue(&mmp) > 60.0, "the presenter did not hear the cue");
    for (who, t) in [
        ("the guest", &mmg),
        ("the producer", &mmprod),
        ("the programme", &prog),
        ("playout", &delayed),
    ] {
        assert!(absent(t, CUE_HZ), "{who} heard the cue: {:.1} dB", cue(t));
    }
    assert!(desk.carts().sounding[0].cue);
    // Esc: only the cue stops.
    assert_eq!(desk.stop_carts(Some(false)), 0);
    assert_eq!(desk.stop_carts(Some(true)), 1);
}

#[test]
fn a_bed_loops_without_a_gap_and_dips_12_db_under_an_open_microphone() {
    let dir = scratch();
    // Half a second, so it loops several times.
    let file = flac(&dir, 2000.0, 0.5, 48_000, 2, 0.4);
    let desk = Desk::new(config(&dir, 0, 1), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.route_pad("presenter", &tone_at(&p, "presenter-voice", 300.0, 0.0))
        .unwrap();
    desk.set_mute("presenter", true);
    let prog = desk.tap(&Bus::Programme).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    run_for(&p, Duration::from_millis(200));
    let bed = uuid();
    desk.fire_cart(&fire(&bed, &file, CartMode::Bed, false, 0.0))
        .unwrap();
    run_for(&p, Duration::from_millis(300));
    let closed_from = Instant::now();
    run_for(&p, Duration::from_millis(2000));
    let closed_to = Instant::now();
    // The level is measured over the last second of each phase (equal lengths: the power
    // is a sum).
    let closed_level_from = closed_to - Duration::from_millis(1000);
    assert!(!desk.carts().ducked && !desk.carts().mic_open);
    desk.set_mute("presenter", false);
    run_for(&p, Duration::from_millis(1000));
    let open_from = Instant::now();
    run_for(&p, Duration::from_millis(1000));
    let open_to = Instant::now();
    let st = desk.carts();
    assert!(st.mic_open && st.ducked && st.sounding[0].ducked);
    assert!(st.sounding[0].looping && st.sounding[0].remaining_ms.is_none());

    // Four passes of the half-second bed: every 20 ms block has it.
    let quietest = prog
        .chunks()
        .into_iter()
        .filter(|(at, _)| *at >= closed_from && *at < closed_to)
        .map(|(_, s)| peak_dbfs(&s))
        .fold(f64::MAX, f64::min);
    let closed = ratio_db(
        tone_power(&between(&prog, closed_level_from, closed_to), 2000.0),
        1.0,
    );
    let open = ratio_db(tone_power(&between(&prog, open_from, open_to), 2000.0), 1.0);
    println!(
        "bed: quietest block {quietest:.1} dBFS while it looped; {closed:.1} dB with the mic closed, {open:.1} dB open ({:.1} dB)",
        open - closed
    );
    assert!(
        quietest > -12.0,
        "the loop has a gap: a block at {quietest:.1} dBFS"
    );
    assert!(
        (open - closed + 12.0).abs() < 1.0,
        "the bed dipped {:.1} dB, not 12",
        closed - open
    );
    assert_eq!(desk.stop_cart(&bed), 1);
    run_for(&p, Duration::from_millis(500));
    assert!(desk.carts().sounding.is_empty());
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Playout's side of the SRT link, as in tests/local.rs.
fn playout_caller(port: u16, name: &str) -> (gst::Pipeline, Tap) {
    let p = gst::Pipeline::with_name(name);
    let src = gst::ElementFactory::make("srtsrc")
        .property("uri", format!("srt://127.0.0.1:{port}?mode=caller"))
        .property("latency", 20i32)
        .build()
        .unwrap();
    let demux = gst::ElementFactory::make("matroskademux").build().unwrap();
    let dec = gst::ElementFactory::make("opusdec").build().unwrap();
    p.add_many([&src, &demux, &dec]).unwrap();
    src.link(&demux).unwrap();
    let dec_weak = dec.downgrade();
    demux.connect_pad_added(move |_, pad| {
        if let Some(dec) = dec_weak.upgrade() {
            let _ = pad.link(&dec.static_pad("sink").unwrap());
        }
    });
    let tap = Tap::attach(&p, &dec, name).unwrap();
    (p, tap)
}

#[test]
fn a_dumped_cart_never_reaches_playout() {
    let dir = scratch();
    let marker = flac(&dir, 3000.0, 0.8, 48_000, 2, 0.4);
    let port = free_udp_port();
    let desk = Desk::new(
        config(&dir, 2_000, port),
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    desk.add_person("guest", Role::Guest).unwrap();
    let mmg = desk.tap(&mm("guest")).unwrap();
    let sent = desk.tap(&Bus::Programme).unwrap();
    let delayed = desk.tap_delayed();
    p.set_state(gst::State::Playing).unwrap();
    let (playout, heard) = playout_caller(port, "playout-cart-dump");
    let _stop_playout = Stop(playout.clone(), dir.clone());
    playout.set_state(gst::State::Playing).unwrap();
    let t0 = Instant::now();
    while desk.delay().state.rebuilding && t0.elapsed() < Duration::from_secs(10) {
        run_for(&p, Duration::from_millis(100));
    }
    assert!(!desk.delay().state.rebuilding, "the delay never filled");

    // The cart sounds 0.8 s; the dump comes 0.6 s after it ends, with all of it still held.
    desk.fire_cart(&fire(&uuid(), &marker, CartMode::OneShot, false, 0.0))
        .unwrap();
    run_for(&p, Duration::from_millis(1400));
    assert!(desk.carts().sounding.is_empty(), "the cart ended");
    assert!(desk.dump(), "the dump was refused: {:?}", desk.delay());
    run_for(&p, Duration::from_millis(3000));
    playout.set_state(gst::State::Null).unwrap();

    let sent_db = presence(&sent, 3000.0);
    let guest_db = presence(&mmg, 3000.0);
    let out_db = presence(&delayed, 3000.0);
    let heard_db = presence(&heard, 3000.0);
    println!(
        "dumped cart: in the programme {sent_db:.0} dB, the guest's ear {guest_db:.0} dB, out of the delay {out_db:.0} dB, at playout {heard_db:.0} dB ({:.1} s decoded)",
        heard.seconds()
    );
    assert!(sent_db > 60.0, "the cart never reached the programme");
    assert!(guest_db > 60.0, "the guest did not hear the cart live");
    assert!(heard.seconds() > 2.0, "playout decoded nothing");
    assert!(
        out_db < 30.0,
        "the dumped cart left the delay: {out_db:.1} dB"
    );
    assert!(
        heard_db < 30.0,
        "the dumped cart reached playout: {heard_db:.1} dB"
    );
}

#[test]
fn the_control_socket_fires_refuses_and_reports_every_play() {
    use weavesignal_desk::control::handle;
    let dir = scratch();
    let file = flac(&dir, 1000.0, 2.0, 48_000, 2, 0.2);
    let desk = Desk::new(config(&dir, 0, 1), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    p.set_state(gst::State::Playing).unwrap();
    let ask = |line: serde_json::Value| -> serde_json::Value {
        serde_json::to_value(handle(&desk, &line.to_string())).unwrap()
    };
    let cart = uuid();
    let fire = |file: &str| {
        serde_json::json!({"op": "cart_fire", "cart": cart, "file": file, "gain_db": -3.0,
            "mode": "one_shot", "duration_ms": 2000})
    };
    let r = ask(fire("../../etc/passwd"));
    assert_eq!(
        (r["ok"].as_bool(), r["code"].as_str()),
        (Some(false), Some("invalid"))
    );
    let r = ask(fire(&format!("{}.flac", uuid())));
    assert_eq!(r["code"], "missing", "{r}");
    for n in 1..=8u64 {
        let r = ask(fire(&file));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            (r["pool"].as_str(), r["player"].as_u64()),
            (Some("shot"), Some(n))
        );
    }
    let r = ask(fire(&file));
    assert_eq!(r["code"], "busy", "{r}");
    run_for(&p, Duration::from_millis(300));
    let st = ask(serde_json::json!({"op": "status"}));
    assert_eq!(st["carts"]["sounding"].as_array().unwrap().len(), 8);
    assert_eq!(st["carts"]["play_seq"], 8);
    let r = ask(serde_json::json!({"op": "cart_stop_all"}));
    assert_eq!(r["stopped"], 8);
    run_for(&p, Duration::from_millis(600));
    let plays = ask(serde_json::json!({"op": "cart_plays", "after": 4}));
    let plays = plays["plays"].as_array().unwrap();
    assert_eq!(plays.len(), 12, "fires 5 to 8 and eight ends");
    let ended: Vec<_> = plays.iter().filter(|e| e["event"] == "ended").collect();
    assert_eq!(ended.len(), 8);
    assert!(ended.iter().all(|e| e["end"] == "stopped"
        && e["played_ms"].as_u64().unwrap() > 200
        && e["cart"] == cart.as_str()));
    let st = ask(serde_json::json!({"op": "status"}));
    assert!(st["carts"]["sounding"].as_array().unwrap().is_empty());
    assert_eq!(st["carts"]["play_seq"], 16);
}
