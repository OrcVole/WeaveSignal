// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The profanity delay's dump, heard from the listener's side: the desk's programme through
//! the delay, SRT, Liquidsoap and icecast.server on loopback, to a listener decoding the Opus
//! stream. Ignored unless the stack is up: `just test-dump`.
//!
//! A presenter's tone source is silent while the delay fills; a marker tone goes in, the
//! delay is dumped while it still holds the marker, and a second tone follows. The listener
//! must hear the second tone and never the marker.

use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Chunk, Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run `just test-dump`"))
}

const DELAY_MS: u32 = 3_000;
const MARKER: f64 = 1000.0;
/// The second tone, as the rebuilding delay plays it (2 % slow) and as it is once rebuilt.
const AFTER: [f64; 2] = [2940.0, 3000.0];

fn errors(p: &gst::Pipeline) {
    let bus = p.bus().unwrap();
    while let Some(msg) = bus.pop() {
        if let gst::MessageView::Error(e) = msg.view() {
            panic!(
                "desk error from {:?}: {}",
                msg.src().map(|s| s.path_string()),
                e.error()
            );
        }
    }
}

/// A listener as `tests/playout.rs` has it: no ICY metadata, Ogg Opus decoded.
fn listener(url: &str) -> (gst::Pipeline, Tap) {
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
    let tap = Tap::attach(&p, &dec, "heard").unwrap();
    (p, tap)
}

/// A tone's level in each whole second of a recording, in dB over silence.
fn per_second(chunks: &[Chunk], f: f64) -> Vec<f64> {
    let s: Vec<i16> = chunks.iter().flat_map(|(_, s)| s.iter().copied()).collect();
    s.chunks(48_000)
        .filter(|c| c.len() == 48_000)
        .map(|c| ratio_db(tone_power(c, f), 1.0))
        .collect()
}

fn run_for(p: &gst::Pipeline, d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        std::thread::sleep(Duration::from_millis(50));
        errors(p);
    }
}

#[test]
#[ignore = "needs the loopback stack: just test-dump"]
fn the_listener_never_hears_what_was_dumped() {
    gst::init().unwrap();
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
    let config = Config::from_toml(
        &format!(
            "[livekit]\nurl = \"ws://127.0.0.1:1\"\napi_key = \"unused\"\nroom = \"unused\"\n\n[programme]\nsrt_port = {srt_port}\n\n[programme.delay]\ntarget_ms = {DELAY_MS}\n\n[inputs.presenter]\n"
        ),
        "dump test",
    )
    .unwrap();
    let desk = Desk::new(
        config,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline().clone();
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property("freq", MARKER)
        .property("volume", 0.0f64)
        .property("samplesperbuffer", 480i32)
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property("caps", RAW_CAPS.parse::<gst::Caps>().unwrap())
        .build()
        .unwrap();
    p.add_many([&src, &caps]).unwrap();
    src.link(&caps).unwrap();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.route_pad("presenter", &caps.static_pad("src").unwrap())
        .unwrap();
    let sent = desk.tap(&Bus::Programme).unwrap();
    p.set_state(gst::State::Playing).unwrap();

    // The delay fills through silence, and playout connects.
    let t0 = Instant::now();
    while (desk.delay().state.rebuilding || !desk.playout().connected)
        && t0.elapsed() < Duration::from_secs(30)
    {
        run_for(&p, Duration::from_millis(200));
    }
    assert!(desk.playout().connected, "playout never connected");
    assert!(!desk.delay().state.rebuilding, "the delay never filled");

    // A listener, decoding before the marker is sent.
    let url = env("WEAVESIGNAL_ROOM_LISTENER_URL");
    let (mut lis, mut heard) = listener(&url);
    lis.set_state(gst::State::Playing).unwrap();
    let t1 = Instant::now();
    while heard.seconds() < 2.0 && t1.elapsed() < Duration::from_secs(60) {
        run_for(&p, Duration::from_millis(200));
        let ended = lis
            .bus()
            .unwrap()
            .pop_filtered(&[gst::MessageType::Error, gst::MessageType::Eos]);
        if ended.is_some() {
            lis.set_state(gst::State::Null).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            (lis, heard) = listener(&url);
            lis.set_state(gst::State::Playing).unwrap();
        }
    }
    assert!(
        heard.seconds() >= 2.0,
        "the listener never decoded the stream"
    );

    // The marker, then the dump while the delay still holds all of it, then the second tone.
    src.set_property("volume", 0.3f64);
    run_for(&p, Duration::from_millis(1000));
    src.set_property("volume", 0.0f64);
    run_for(&p, Duration::from_millis(800));
    let held = desk.delay();
    assert!(desk.dump(), "the dump was refused: {held:?}");
    src.set_property("freq", AFTER[1]);
    src.set_property("volume", 0.3f64);

    // Until the listener has heard the second tone for a few seconds.
    let t2 = Instant::now();
    let mut after_s = 0.0;
    while t2.elapsed() < Duration::from_secs(90) {
        run_for(&p, Duration::from_millis(500));
        after_s = AFTER
            .iter()
            .map(|f| {
                per_second(&heard.chunks(), *f)
                    .iter()
                    .filter(|d| **d > 60.0)
                    .count()
            })
            .max()
            .unwrap_or(0) as f64;
        if after_s >= 4.0 {
            break;
        }
    }
    let ended_early = lis
        .bus()
        .unwrap()
        .pop_filtered(&[gst::MessageType::Error, gst::MessageType::Eos])
        .is_some();
    lis.set_state(gst::State::Null).unwrap();
    p.set_state(gst::State::Null).unwrap();

    let chunks = heard.chunks();
    let marker = per_second(&chunks, MARKER);
    let tone: Vec<f64> = per_second(&chunks, AFTER[0])
        .into_iter()
        .zip(per_second(&chunks, AFTER[1]))
        .map(|(a, b)| a.max(b))
        .collect();
    let marker_sent = ratio_db(tone_power(&sent.samples(), MARKER), 1.0);
    // In each second the listener heard, the marker against the louder of silence (30 dB
    // over digital silence) and 40 dB under the second tone there. The marker was sent at
    // the second tone's level, so had it aired it would sit near 0 dB under it; a tone's onset
    // and the rebuild's resampling leave a trace of it across the spectrum, 55 dB down or more.
    let worst = marker
        .iter()
        .zip(&tone)
        .map(|(m, t)| m - 30.0f64.max(t - 40.0))
        .fold(f64::MIN, f64::max);
    let report = serde_json::json!({
        "delay_before_dump": held.state,
        "marker_sent_db": marker_sent,
        "listener_seconds": heard.seconds(),
        "listener_ended_early": ended_early,
        "marker_per_second_db": marker,
        "after_per_second_db": tone,
        "marker_margin_db": worst,
    });
    println!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }
    assert!(marker_sent > 60.0, "the marker never reached the programme");
    assert!(
        after_s >= 4.0,
        "the listener did not hear the programme after the dump"
    );
    assert!(
        worst < 0.0,
        "the listener heard the dumped marker: {worst:.1} dB over the allowance"
    );
}
