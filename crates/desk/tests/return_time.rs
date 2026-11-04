// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A return's timestamps are the mix's, not the moment its encoder got the block.
//!
//! The phone that called in (2026-11-03) received 50 packets a second and still concealed
//! about 38 % of what it played: each return block had been stamped with the pipeline's
//! running time when a worker of the pool encoded it, and the mixing thread wakes every 5 ms
//! and mixes two blocks at once when it is late. The stamps, and the RTP timestamps made from
//! them, were up to 5 ms off and sometimes two blocks almost together, and the receiver
//! filled each gap with concealment. Each block now carries the time of its place in the mix:
//! one block's length after the last, always. Needs GStreamer (`just desk-check`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::config::Config;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

fn config() -> Config {
    Config::from_toml(
        r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = 1

[inputs.presenter]
"#,
        "test",
    )
    .unwrap()
}

/// A live tone behind a ghost pad named like a LiveKit audio pad.
fn tone(pipeline: &gst::Pipeline, name: &str) -> gst::Pad {
    let bin = gst::Bin::with_name(name);
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property("freq", 1000.0)
        .property("volume", 0.3)
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

/// The first element whose name starts with `prefix`, waiting up to `within` for it.
fn element(p: &gst::Pipeline, prefix: &str, within: Duration) -> gst::Element {
    let deadline = Instant::now() + within;
    loop {
        let found = p
            .iterate_recurse()
            .into_iter()
            .flatten()
            .find(|e| e.name().starts_with(prefix));
        if let Some(e) = found {
            return e;
        }
        assert!(Instant::now() < deadline, "no element `{prefix}*`");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Record every buffer's PTS through `pad`.
fn stamps(pad: &gst::Pad) -> Arc<Mutex<Vec<u64>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        if let Some(pts) = info.buffer().and_then(|b| b.pts()) {
            s.lock().unwrap().push(pts.nseconds());
        }
        gst::PadProbeReturn::Ok
    });
    seen
}

/// The steps between consecutive stamps that are not one block (20 ms) long.
fn uneven(stamps: &[u64]) -> Vec<i64> {
    stamps
        .windows(2)
        .map(|w| w[1] as i64 - w[0] as i64)
        .filter(|&d| d != 20_000_000)
        .collect()
}

#[test]
fn a_returns_blocks_are_stamped_one_block_apart() {
    let desk = Desk::new(config(), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(&p, "presenter-voice"))
        .unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let enc = element(&p, "enc-mm-guest", Duration::from_secs(5));
    let into = stamps(&enc.static_pad("sink").unwrap());
    let out = stamps(&enc.static_pad("src").unwrap());
    std::thread::sleep(Duration::from_secs(3));
    p.set_state(gst::State::Null).unwrap();

    let into = into.lock().unwrap().clone();
    let out = out.lock().unwrap().clone();
    // opusenc stamps its first packet early by its lookahead (6.5 ms): from the second on.
    let (bad_in, bad_out) = (uneven(&into), uneven(out.get(1..).unwrap_or_default()));
    println!(
        "into the encoder: {} blocks, {} uneven steps {:?}; out: {} packets, {} uneven {:?}",
        into.len(),
        bad_in.len(),
        &bad_in[..bad_in.len().min(10)],
        out.len(),
        bad_out.len(),
        &bad_out[..bad_out.len().min(10)],
    );
    assert!(into.len() > 100, "about 150 blocks in 3 s: {}", into.len());
    assert!(
        bad_in.is_empty(),
        "uneven steps into the encoder: {bad_in:?}"
    );
    assert!(
        bad_out.is_empty(),
        "uneven steps out of the encoder: {bad_out:?}"
    );
}
