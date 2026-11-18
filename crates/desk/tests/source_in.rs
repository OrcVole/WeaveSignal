// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A host's own studio handed to the desk by the ingest (ADR-0280), over the desk's real
//! loopback listener: the source arrives standing by (at the desk, off air, out of the
//! programme), its status says how it came in and what the leveller does, on air it is heard in
//! the programme, and it leaves the desk when its connection closes. A wrong password is refused.
//! Needs GStreamer (`just desk-check`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::sources::Sources;
use weavesignal_proto::source_wire as wire;

const PASSWORD: &str = "sources-test-only";

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

/// Connect as the ingest does; `None` when the desk refused.
fn connect(port: u16, password: &str) -> Option<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut s = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("the desk's sources listener: {e}"),
        }
    };
    let auth = weavesignal_desk::feed::basic_auth("ingest", password);
    write!(
        s,
        "PUT /sources/source-abcd2345 HTTP/1.0\r\nAuthorization: {auth}\r\n{}: Finlay Grant\r\n{}: srt\r\n{}: aac 44.1 kHz stereo\r\n{}: abcd2345\r\n{}: steeped\r\n\r\n",
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
    if !line.contains("200") {
        return None;
    }
    s.write_all(&wire::wav_f32_header()).unwrap();
    Some(s)
}

/// Send `seconds` of a 1 kHz tone at `amp`, paced in 20 ms pieces.
fn send_tone(s: &mut TcpStream, seconds: f32, amp: f32, start: &mut u64) {
    let pieces = (seconds * 50.0) as usize;
    let begun = Instant::now();
    for k in 0..pieces {
        let mut bytes = Vec::with_capacity(960 * 8);
        for _ in 0..960 {
            let v = amp * (2.0 * std::f32::consts::PI * 1000.0 * *start as f32 / 48_000.0).sin();
            *start += 1;
            bytes.extend_from_slice(&v.to_le_bytes());
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        s.write_all(&bytes).unwrap();
        let due = begun + Duration::from_millis(20 * (k as u64 + 1));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

#[test]
fn a_source_stands_by_then_airs_and_leaves_with_its_connection() {
    let port = 23718;
    let desk = Desk::new(config(port), Outputs::NONE).unwrap();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    let _sources =
        Sources::start(&desk, &desk.config().sources, Some(PASSWORD.into())).expect("listening");

    assert!(
        connect(port, "wrong").is_none(),
        "a wrong password is refused"
    );

    let programme: Tap = desk.tap(&Bus::Programme).expect("the programme's tap");
    let mut s = connect(port, PASSWORD).expect("taken");
    let mut t = 0;
    send_tone(&mut s, 2.0, 0.1, &mut t);
    let me = desk
        .status()
        .into_iter()
        .find(|p| p.identity == "source-abcd2345")
        .expect("at the desk");
    assert!(!me.on_air, "standing by, not on air");
    assert!(
        me.linked && me.lufs.is_some(),
        "its sound reaches the desk: {me:?}"
    );
    let src = me.source.as_ref().expect("a source's facts");
    assert_eq!((src.way.as_str(), src.show.as_str()), ("srt", "steeped"));
    assert_eq!(me.name, "Finlay Grant");
    // -23 dB RMS of a 0.1 tone is -23 dBFS: the leveller wants about +0 dB, never more than 12.
    assert!(src.leveller_db.abs() <= 12.0);
    let off_air = programme.samples();
    assert!(
        ratio_db(tone_power(&off_air, 1000.0), 1.0) < 20.0,
        "a source standing by is not in the programme"
    );

    assert!(desk.set_on_air("source-abcd2345", true));
    let before = programme.samples().len();
    send_tone(&mut s, 2.0, 0.1, &mut t);
    let on_air = programme.samples()[before..].to_vec();
    assert!(
        ratio_db(tone_power(&on_air, 1000.0), tone_power(&off_air, 1000.0)) > 40.0,
        "on air it is heard"
    );

    drop(s);
    let deadline = Instant::now() + Duration::from_secs(5);
    while desk.person("source-abcd2345").is_some() {
        assert!(
            Instant::now() < deadline,
            "the source left with its connection"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = desk.pipeline().set_state(gst::State::Null);
}

/// The RMS of each `window`-sample stretch of `samples`, in linear full-scale units.
fn rms_windows(samples: &[i16], window: usize) -> Vec<f64> {
    samples
        .chunks_exact(window)
        .map(|w| {
            let sum: f64 = w.iter().map(|&s| (f64::from(s) / 32768.0).powi(2)).sum();
            (sum / window as f64).sqrt()
        })
        .collect()
}

/// The schedule's move over `fade_ms` (ADR-0280): the gate ramps across blocks, rising from
/// silence to the source's level and falling back, evenly, with no step bigger than the fade
/// allows; a person's switch in the middle of one is at once. Counted in blocks, not in the
/// clock: the windows are of samples, whatever the pass did when.
#[test]
fn a_faded_on_rises_and_a_faded_off_falls_over_the_fade_and_a_press_is_at_once() {
    let port = 23719;
    let desk = Desk::new(config(port), Outputs::NONE).unwrap();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    let _sources =
        Sources::start(&desk, &desk.config().sources, Some(PASSWORD.into())).expect("listening");
    let programme: Tap = desk.tap(&Bus::Programme).expect("the programme's tap");
    let mut s = connect(port, PASSWORD).expect("taken");
    let mut t = 0;
    send_tone(&mut s, 2.0, 0.1, &mut t);
    assert!(!desk.person("source-abcd2345").unwrap().switches.on_air);

    // A faded On: two seconds.
    assert!(desk.set_on_air_faded("source-abcd2345", true, 2000));
    assert!(
        desk.person("source-abcd2345").unwrap().switches.on_air,
        "on air at once as far as the desk's state goes"
    );
    let before = programme.samples().len();
    send_tone(&mut s, 4.5, 0.1, &mut t);
    let rose = programme.samples()[before..].to_vec();
    let start = rose
        .iter()
        .position(|&x| x != 0)
        .expect("something was heard");
    let rose = &rose[start..];
    let w = rms_windows(&rose[..2 * 48_000], 4_800); // 100 ms
    let (first, middle, last) = (w[0], w[w.len() / 2], w[w.len() - 1]);
    assert!(
        first < 0.1 * last,
        "it starts from silence: {first} against {last}"
    );
    assert!(
        middle > 0.3 * last && middle < 0.7 * last,
        "halfway through it is about half: {middle} against {last}"
    );
    for pair in w.windows(2) {
        assert!(pair[1] >= pair[0] * 0.97, "it only rises: {w:?}");
    }
    let after = rms_windows(&rose[3 * 48_000..], 4_800);
    let steady = after.iter().sum::<f64>() / after.len() as f64;
    assert!(
        (steady - last).abs() < 0.25 * steady && steady > 0.0,
        "after the fade it is level: {steady} against {last}"
    );

    // A faded Off: one second down.
    assert!(desk.set_on_air_faded("source-abcd2345", false, 1000));
    let before = programme.samples().len();
    send_tone(&mut s, 3.0, 0.1, &mut t);
    let fell = programme.samples()[before..].to_vec();
    let w = rms_windows(&fell[..48_000 + 4_800 * 2], 4_800);
    assert!(
        w[0] > 0.7 * steady && w[0] < 1.2 * steady,
        "it starts where it was: {} against {steady}",
        w[0]
    );
    for pair in w.windows(2) {
        assert!(pair[1] <= pair[0] * 1.03, "it only falls: {w:?}");
    }
    let tail = rms_windows(&fell[fell.len() - 48_000..], 4_800);
    assert!(
        tail.iter().all(|&r| r < 0.001),
        "and it is gone at the end: {tail:?}"
    );

    // A press in the middle of a fade is at once: back on, then a plain off cuts it within a block.
    assert!(desk.set_on_air_faded("source-abcd2345", true, 2000));
    send_tone(&mut s, 0.3, 0.1, &mut t);
    assert!(desk.set_on_air("source-abcd2345", false));
    send_tone(&mut s, 0.3, 0.1, &mut t);
    let before = programme.samples().len();
    send_tone(&mut s, 0.5, 0.1, &mut t);
    let quiet = rms_windows(&programme.samples()[before + 4_800..], 4_800);
    assert!(
        quiet.iter().all(|&r| r < 0.0005),
        "a press ends the fade and the gate: {quiet:?}"
    );
    assert!(
        !desk.set_on_air_faded("nobody", true, 2000),
        "an unknown person"
    );
    drop(s);
    let _ = desk.pipeline().set_state(gst::State::Null);
}
