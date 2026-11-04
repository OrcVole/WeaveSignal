// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! How long a listener's connection to icecast.server lasts, and how it ends: the desk
//! sending a steady programme through Liquidsoap (the constant 96 kbit/s stream) for
//! `WEAVESIGNAL_LISTEN_SECONDS` (600 by default) while a GStreamer listener (`souphttpsrc`, no
//! ICY metadata) follows the stream and reconnects whenever it ends. A measurement, not in
//! `ci`: `TEST=listen tests/room/run-stack.sh`. The staging run's recorder reconnected 53
//! times; this says whether the same happens on loopback, and whether the server or the
//! client ended each connection (an end of stream is the server closing; an error is the
//! client giving up or the connection breaking).

use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::Tap;
use weavesignal_desk::config::Config;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run the stack test"))
}

fn listener(url: &str, n: u32) -> (gst::Pipeline, Tap) {
    let p = gst::Pipeline::with_name(&format!("listener-{n}"));
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
    let tap = Tap::attach(&p, &dec, &format!("heard-{n}")).unwrap();
    (p, tap)
}

#[test]
#[ignore = "a measurement on the loopback stack: TEST=listen tests/room/run-stack.sh"]
fn how_long_a_listener_stays_connected() {
    gst::init().unwrap();
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
    let seconds: u64 = std::env::var("WEAVESIGNAL_LISTEN_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let config = Config::from_toml(
        &format!(
            "[livekit]\nurl = \"ws://127.0.0.1:1\"\napi_key = \"unused\"\nroom = \"unused\"\n\n[programme]\nsrt_port = {srt_port}\n\n[inputs.presenter]\n"
        ),
        "listen test",
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
        .property("freq", 440.0f64)
        .property("volume", 0.1f64)
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
    p.set_state(gst::State::Playing).unwrap();

    let url = env("WEAVESIGNAL_ROOM_LISTENER_URL");
    let t0 = Instant::now();
    let mut n = 0;
    let mut current: Option<(gst::Pipeline, Tap, Instant)> = None;
    let mut retry_at = Instant::now() + Duration::from_secs(3);
    let mut ends = Vec::new();
    let mut playout_down = 0u32;
    while t0.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(msg) = p.bus().unwrap().pop_filtered(&[gst::MessageType::Error]) {
            let (scope, _) = desk.classify_error(&msg);
            panic!("desk error ({scope:?}): {msg:?}");
        }
        if !desk.playout().connected {
            playout_down += 1;
        }
        if let Some((lp, tap, since)) = &current {
            let mut end = None;
            while let Some(msg) = lp.bus().unwrap().pop() {
                match msg.view() {
                    gst::MessageView::Eos(_) => {
                        end = Some("end of stream: the server closed".to_owned())
                    }
                    gst::MessageView::Error(e) => {
                        end = Some(format!(
                            "error from {}: {} ({})",
                            msg.src()
                                .map(|s| s.path_string().to_string())
                                .unwrap_or_default(),
                            e.error(),
                            e.debug().map(|d| d.to_string()).unwrap_or_default()
                        ))
                    }
                    _ => {}
                }
            }
            if let Some(why) = end {
                let row = serde_json::json!({
                    "connection": n,
                    "opened_at_s": since.duration_since(t0).as_secs_f64(),
                    "lasted_s": since.elapsed().as_secs_f64(),
                    "decoded_s": tap.seconds(),
                    "ended": why,
                });
                println!("{row}");
                ends.push(row);
                lp.set_state(gst::State::Null).unwrap();
                current = None;
                retry_at = Instant::now() + Duration::from_secs(1);
            }
        } else if Instant::now() >= retry_at {
            n += 1;
            let (lp, tap) = listener(&url, n);
            lp.set_state(gst::State::Playing).unwrap();
            current = Some((lp, tap, Instant::now()));
        }
    }
    let last = current.as_ref().map(|(_, tap, since)| {
        serde_json::json!({
            "connection": n,
            "opened_at_s": since.duration_since(t0).as_secs_f64(),
            "lasted_s": since.elapsed().as_secs_f64(),
            "decoded_s": tap.seconds(),
            "ended": "still open at the end of the run",
        })
    });
    if let Some((lp, ..)) = current.take() {
        lp.set_state(gst::State::Null).unwrap();
    }
    p.set_state(gst::State::Null).unwrap();
    let report = serde_json::json!({
        "seconds": seconds,
        "connections": n,
        "ended_early": ends,
        "last": last,
        "playout_down_samples": playout_down,
        "mix": desk.mix_stats(),
    });
    println!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }
}
