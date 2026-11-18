// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A host's monitor mix at the desk (ADR-0280, design 4.5), without a room: two hosts' own
//! studios reach the desk over its real loopback listener, one sending 1 kHz and the other
//! 2 kHz. Opening the first host's monitor mix, they hear the programme without themselves (the
//! other's tone, never their own, however loud and on air they are), the cue bus (the other
//! cued, off air) and talk addressed to them (the other off air and uncued). Each block of the
//! monitor mix is stamped one block after the last, from its place in the mix, never from the
//! clock at encode time (the choppy caller return of 2026-11-03; `return_time.rs`). Needs
//! GStreamer (`just desk-check`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::sources::Sources;
use weavesignal_proto::source_wire as wire;

const PASSWORD: &str = "monitor-test-only";
const HOST: &str = "abcd2345";
const OTHER: &str = "wxyz6789";

fn config(port: u16) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = 1

[sources]
port = {port}
"#
        ),
        "test",
    )
    .unwrap()
}

/// Connect as the ingest does for the key `key`.
fn connect(port: u16, key: &str) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut s = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("the desk's sources listener: {e}"),
        }
    };
    let auth = weavesignal_desk::feed::basic_auth("ingest", PASSWORD);
    write!(
        s,
        "PUT /sources/source-{key} HTTP/1.0\r\nAuthorization: {auth}\r\n{}: Host {key}\r\n{}: srt\r\n{}: aac 48 kHz stereo\r\n{}: {key}\r\n{}: steeped\r\n\r\n",
        wire::NAME,
        wire::WAY,
        wire::FORMAT,
        wire::KEY,
        wire::SHOW
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(s.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("200"), "the desk refused {key}: {line}");
    s.write_all(&wire::wav_f32_header()).unwrap();
    s
}

/// Send both hosts' tones for `seconds`, each in its own 20 ms pieces, in step.
fn send(a: &mut TcpStream, b: &mut TcpStream, seconds: f32, pos: &mut u64) {
    let pieces = (seconds * 50.0) as usize;
    let begun = Instant::now();
    for k in 0..pieces {
        for (s, hz) in [(&mut *a, 1000.0f32), (&mut *b, 2000.0f32)] {
            let mut bytes = Vec::with_capacity(960 * 8);
            for i in 0..960u64 {
                let t = (*pos + i) as f32 / 48_000.0;
                let v = 0.1 * (2.0 * std::f32::consts::PI * hz * t).sin();
                bytes.extend_from_slice(&v.to_le_bytes());
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            s.write_all(&bytes).unwrap();
        }
        *pos += 960;
        let due = begun + Duration::from_millis(20 * (k as u64 + 1));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

/// What a tap heard of each tone over the next `seconds`: (1 kHz, 2 kHz) in dB over a floor.
fn heard(
    tap: &Tap,
    a: &mut TcpStream,
    b: &mut TcpStream,
    seconds: f32,
    pos: &mut u64,
) -> (f64, f64) {
    // Half a second to settle (a switch fades over a block, and the cushion holds the last
    // switch's sound for a moment), then the measure.
    send(a, b, 0.5, pos);
    let before = tap.samples().len();
    send(a, b, seconds, pos);
    let s = tap.samples()[before..].to_vec();
    (
        ratio_db(tone_power(&s, 1000.0), 1.0),
        ratio_db(tone_power(&s, 2000.0), 1.0),
    )
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

#[test]
fn a_hosts_monitor_hears_the_others_on_air_cued_and_addressed_never_themselves() {
    let port = 23719;
    let desk = Desk::new(config(port), Outputs::NONE).unwrap();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    let _sources =
        Sources::start(&desk, &desk.config().sources, Some(PASSWORD.into())).expect("listening");
    let (me, other) = (format!("source-{HOST}"), format!("source-{OTHER}"));

    let mut a = connect(port, HOST);
    let mut b = connect(port, OTHER);
    let mut pos = 0;
    send(&mut a, &mut b, 1.0, &mut pos);
    assert!(desk.monitors().is_empty(), "no monitor mix until asked");
    assert!(
        !desk.open_monitor("nobody00").unwrap(),
        "a host that is not at the desk has no monitor mix"
    );

    assert!(desk.open_monitor(HOST).unwrap());
    assert!(
        desk.open_monitor(HOST).unwrap(),
        "opening it again changes nothing"
    );
    assert_eq!(desk.monitors().into_iter().collect::<Vec<_>>(), vec![HOST]);
    let status = desk
        .status()
        .into_iter()
        .find(|p| p.identity == me)
        .and_then(|p| p.source)
        .expect("a source's facts");
    assert!(status.monitor, "the status says the monitor is open");
    assert!(
        !desk
            .status()
            .into_iter()
            .find(|p| p.identity == other)
            .and_then(|p| p.source)
            .expect("the other's facts")
            .monitor
    );

    let monitor: Tap = desk
        .tap(&Bus::MixMinus(me.clone()))
        .expect("the monitor mix's tap");
    let programme: Tap = desk.tap(&Bus::Programme).expect("the programme's tap");
    // Each block is stamped from its place in the mix: one block apart, always.
    let enc = element(
        desk.pipeline(),
        &format!("enc-mm-{me}"),
        Duration::from_secs(5),
    );
    let seen = Arc::new(Mutex::new(Vec::<u64>::new()));
    {
        let s = seen.clone();
        enc.static_pad("sink")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if let Some(pts) = info.buffer().and_then(|b| b.pts()) {
                    s.lock().unwrap().push(pts.nseconds());
                }
                gst::PadProbeReturn::Ok
            });
    }

    // Both on air: the programme carries both, the monitor only the other.
    assert!(desk.set_on_air(&me, true) && desk.set_on_air(&other, true));
    let (own, theirs) = heard(&monitor, &mut a, &mut b, 2.0, &mut pos);
    println!(
        "both on air, the monitor: 1 kHz (their own) {own:.0} dB, 2 kHz (the other) {theirs:.0} dB"
    );
    assert!(
        theirs - own > 40.0,
        "the monitor hears the other and not itself"
    );
    // What a tone sounds like here, for what is silence: nothing within 40 dB of it.
    let full = theirs;
    let p = programme.samples();
    assert!(
        ratio_db(tone_power(&p, 1000.0), 1.0) > 20.0
            && ratio_db(tone_power(&p, 2000.0), 1.0) > 20.0,
        "the programme carries both, or the check above proves nothing"
    );

    // The other off air and uncued: nothing but their own to hear, which is not sent back.
    assert!(desk.set_on_air(&other, false));
    let (own, theirs) = heard(&monitor, &mut a, &mut b, 2.0, &mut pos);
    println!("other off air: 1 kHz {own:.0} dB, 2 kHz {theirs:.0} dB");
    assert!(
        own < full - 40.0 && theirs < full - 40.0,
        "the monitor is silent: {own} {theirs} against {full}"
    );

    // Cued, off air: on the cue bus.
    assert!(desk.set_cue(&other, true));
    let (own, theirs) = heard(&monitor, &mut a, &mut b, 2.0, &mut pos);
    println!("other cued: 1 kHz {own:.0} dB, 2 kHz {theirs:.0} dB");
    assert!(
        theirs > full - 10.0 && own < full - 40.0,
        "the cue bus: {own} {theirs} against {full}"
    );
    assert!(desk.set_cue(&other, false));

    // Talk addressed to the host: off air, uncued, still heard.
    assert!(desk.set_talk_to(&other, &me, true));
    let (own, theirs) = heard(&monitor, &mut a, &mut b, 2.0, &mut pos);
    println!("other talking to them: 1 kHz {own:.0} dB, 2 kHz {theirs:.0} dB");
    assert!(
        theirs > full - 10.0 && own < full - 40.0,
        "talk addressed to them: {own} {theirs} against {full}"
    );
    assert!(desk.set_talk_to(&other, &me, false));
    let (_, theirs) = heard(&monitor, &mut a, &mut b, 1.0, &mut pos);
    assert!(
        theirs < full - 40.0,
        "silent again once the talk is let go: {theirs}"
    );

    let pts = seen.lock().unwrap().clone();
    assert!(
        pts.len() > 300,
        "the monitor mix's blocks reached its encoder: {}",
        pts.len()
    );
    let uneven: Vec<i64> = pts
        .windows(2)
        .map(|w| w[1] as i64 - w[0] as i64)
        .filter(|&d| d != 20_000_000)
        .collect();
    println!(
        "{} blocks, {} not one block after the last",
        pts.len(),
        uneven.len()
    );
    assert!(uneven.is_empty(), "uneven steps (ns): {uneven:?}");

    // Closing it takes the mix and its encoder down and leaves the source where it was.
    desk.close_monitor(HOST);
    assert!(desk.monitors().is_empty());
    assert!(desk.person(&me).is_some() && desk.tap(&Bus::MixMinus(me.clone())).is_none());
    // And it can be opened again, as when a page comes back.
    assert!(desk.open_monitor(HOST).unwrap());
    assert!(!desk.monitors().is_empty());
    // A source that leaves takes its monitor mix with it.
    drop(a);
    let deadline = Instant::now() + Duration::from_secs(5);
    while desk.person(&me).is_some() {
        assert!(
            Instant::now() < deadline,
            "the source left with its connection"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        desk.monitors().is_empty(),
        "no monitor mix without its source"
    );
    let _ = desk.pipeline().set_state(gst::State::Null);
}
