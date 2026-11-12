// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The listener stream with nobody in the room. Ignored unless the stack is up:
//! `just test-empty`.
//!
//! The staging run (2026-10-25): with the room empty the stream answered 200 and then
//! trickled, since digital silence encodes to almost nothing in Opus; with a guest on air it
//! carried 37 to 45 KB every 6 s. A player that buffers by bytes waits minutes on a trickle and
//! may give up. The stream must keep a steady byte rate however quiet the programme.

mod stack;

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Room, env, run_for, wait_for};
use weavesignal_desk::pipeline::{Desk, Outputs};

/// Read the stream from `url` as a listener does, without asking for ICY metadata, noting
/// when each read returned and how many bytes it brought. Runs until the process ends.
fn listen(url: &str) -> Arc<Mutex<Vec<(Instant, usize)>>> {
    let reads = Arc::new(Mutex::new(Vec::new()));
    let (url, out) = (url.to_owned(), reads.clone());
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .into();
        loop {
            let Ok(mut res) = agent.get(&url).call() else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            if res.status() != 200 {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            let mut body = res.body_mut().as_reader();
            let mut buf = [0u8; 4096];
            while let Ok(n) = body.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.lock().unwrap().push((Instant::now(), n));
            }
        }
    });
    reads
}

#[test]
#[ignore = "needs the loopback stack: just test-empty"]
fn the_listener_stream_keeps_a_steady_byte_rate_with_the_room_empty() {
    gst::init().unwrap();
    let room = Room::fresh("m1-empty");
    let desk = Desk::new(room.config(""), Outputs::ALL).unwrap();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    assert!(
        wait_for(&desk, Duration::from_secs(15), || desk.playout().connected).is_some(),
        "playout never connected"
    );
    // The mount appears some 25 s after the stack starts; the listener retries until then.
    let reads = listen(&env("WEAVESIGNAL_ROOM_LISTENER_URL"));
    let first = wait_for(&desk, Duration::from_secs(60), || {
        !reads.lock().unwrap().is_empty()
    });
    assert!(first.is_some(), "the listener mount never answered");
    let t0 = Instant::now();
    // The first seconds are the listener server's burst on connect; measure after them.
    let (skip, windows) = (Duration::from_secs(6), 3);
    run_for(&desk, skip + Duration::from_secs(6 * windows));
    desk.pipeline().set_state(gst::State::Null).unwrap();

    let reads = reads.lock().unwrap().clone();
    let per_window: Vec<usize> = (0..windows)
        .map(|w| {
            let a = t0 + skip + Duration::from_secs(6 * w);
            let b = a + Duration::from_secs(6);
            reads
                .iter()
                .filter(|(t, _)| *t >= a && *t < b)
                .map(|(_, n)| n)
                .sum()
        })
        .collect();
    let times: Vec<Instant> = reads
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| *t >= t0 + skip)
        .collect();
    let longest_gap = times
        .windows(2)
        .map(|w| w[1] - w[0])
        .max()
        .unwrap_or(Duration::MAX);
    println!(
        "{}",
        serde_json::json!({
            "bytes_per_6s": per_window,
            "longest_gap_ms": longest_gap.as_millis() as u64,
            "reads": times.len(),
        })
    );
    // Opus at the stack's constant rate (ADR-0233; 96 kbit/s by default) is 72 KB in 6 s; on
    // air, the staging run carried 37 to 45 KB. A trickle is far below; take 40 % of the rate.
    let kbps: usize = std::env::var("WEAVESIGNAL_ROOM_OPUS_KBPS")
        .ok()
        .and_then(|k| k.parse().ok())
        .unwrap_or(96);
    let floor = kbps * 125 * 6 * 4 / 10;
    assert!(
        per_window.iter().all(|&b| b >= floor),
        "the empty room's stream trickles: {per_window:?} bytes per 6 s ({kbps} kbit/s set)"
    );
    assert!(
        longest_gap < Duration::from_secs(2),
        "the stream went {} ms without a byte",
        longest_gap.as_millis()
    );
}
