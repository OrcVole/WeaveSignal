// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video's sound from playout with both real ends (ADR-0172; review 0028, A1): the desk with
//! video on sends its programme to the real playout over SRT, and the real playout sends its
//! output back to the desk's feed port. While nobody is live the video's sound is playout's
//! (its fallback, a 3 kHz file, once the desk has been quiet for its dead-air time); while a
//! DJ's strip is open on air it is the desk's programme (the DJ's 1 kHz); off air again, it is
//! playout's. Ignored unless the stack is up: `just test-feed` starts LiveKit, playout and the
//! listener process with `desk_feed` and the fallback set, and runs this in the desk image.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::hls_sound::Source;
use weavesignal_desk::pipeline::{Desk, HlsSoundStatus, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run `just test-feed`"))
}

fn scratch() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("weavesignal-feed-stack-{stamp}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A live tone, behind a ghost pad named like LiveKit's audio pad.
fn tone(pipeline: &gst::Pipeline, name: &str, freq: f64) -> gst::Pad {
    let bin = gst::Bin::with_name(name);
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property("freq", freq)
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

fn run_for(desk: &Desk, d: Duration) {
    let t0 = Instant::now();
    let bus = desk.pipeline().bus().unwrap();
    while t0.elapsed() < d {
        while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
            if let gst::MessageView::Error(e) = msg.view() {
                panic!(
                    "desk error from {:?}: {}",
                    msg.src().map(|s| s.path_string()),
                    e.error()
                );
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_sound(desk: &Desk, limit: Duration, want: impl Fn(&HlsSoundStatus) -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if want(&desk.hls_sound()) {
            return true;
        }
        run_for(desk, Duration::from_millis(100));
    }
    want(&desk.hls_sound())
}

/// A tone's level, in dB over silence, in what a tap recorded after `from`.
fn level_since(t: &Tap, from: Instant, freq: f64) -> f64 {
    let s: Vec<i16> = t
        .chunks()
        .into_iter()
        .filter(|(at, _)| *at >= from)
        .flat_map(|(_, s)| s)
        .collect();
    ratio_db(tone_power(&s, freq), 1.0)
}

/// Waits until the video's sound carries `freq` for a second, and not `other` (at least 40 dB
/// below it), measured in one-second windows; returns how long it took and the levels of `freq`
/// and `other` in that second.
fn until_tone(
    desk: &Desk,
    tap: &Tap,
    freq: f64,
    other: f64,
    limit: Duration,
) -> Option<(f64, f64, f64)> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        let from = Instant::now();
        run_for(desk, Duration::from_secs(1));
        let (want, not) = (level_since(tap, from, freq), level_since(tap, from, other));
        eprintln!(
            "  {:.1} s: {freq} Hz {want:.1} dB, {other} Hz {not:.1} dB ({:?})",
            t0.elapsed().as_secs_f64(),
            desk.hls_sound().source
        );
        // The other tone at least 40 dB down: a loud 1 kHz tone puts its own third harmonic at
        // 3 kHz about 90 dB below it (60 dB here), so an absolute floor would fail on that.
        if want > 50.0 && want - not > 40.0 {
            return Some((t0.elapsed().as_secs_f64(), want, not));
        }
    }
    None
}

#[test]
#[ignore = "needs LiveKit, playout and the listener process: just test-feed"]
fn the_video_sound_comes_from_the_real_playout_unless_someone_is_live() {
    gst::init().unwrap();
    let srt_port = env("WEAVESIGNAL_ROOM_SRT_PORT");
    let feed_port = env("WEAVESIGNAL_ROOM_DESK_FEED_PORT");
    let dir = scratch();
    let mut config = Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "unused"

[programme]
srt_port = {srt_port}

[programme.delay]
target_ms = 1000

[inputs.dj]

[video]
enabled = true
lower_third_ms = 0

[video.hls]
dir = "{}"

[video.playout_feed]
port = {feed_port}
crossfade_ms = 100
off_air_hold_ms = 500
"#,
            dir.join("hls").display()
        ),
        "feed stack test",
    )
    .unwrap();
    config.video.playout_feed.enabled = true;
    let desk = Desk::new(
        config,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let hls = desk.tap_hls_sound();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    struct Stop(Desk);
    impl Drop for Stop {
        fn drop(&mut self) {
            if let Some(v) = self.0.video() {
                v.stop();
            }
            let _ = self.0.pipeline().set_state(gst::State::Null);
        }
    }
    let _stop = Stop(desk.clone());
    desk.add_person("callum", Role::Dj).unwrap();
    let pad = tone(desk.pipeline(), "callum-voice", 1000.0);
    desk.route_pad("callum", &pad).unwrap();
    pad.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
    let t0 = Instant::now();

    // Playout connects to the desk's SRT listener and to its feed port by itself.
    assert!(
        wait_sound(&desk, Duration::from_secs(60), |s| s.feed_connected),
        "the real playout never connected its feed to the desk: {:?}",
        desk.hls_sound()
    );
    let connected_s = t0.elapsed().as_secs_f64();

    // Nobody live: playout's sound, its fallback (3 kHz) once the desk has been quiet.
    let fallback = until_tone(&desk, &hls, 3000.0, 1000.0, Duration::from_secs(40));
    let first = desk.hls_sound();

    // The DJ opens their strip: the desk's programme (1 kHz) once the delay has played out.
    assert!(desk.set_on_air("callum", true));
    let live = until_tone(&desk, &hls, 1000.0, 3000.0, Duration::from_secs(15));
    let on_air = desk.hls_sound();

    // Off air again: playout's sound, the fallback again once the desk is quiet.
    assert!(desk.set_on_air("callum", false));
    let back = until_tone(&desk, &hls, 3000.0, 1000.0, Duration::from_secs(40));
    let after = desk.hls_sound();

    println!(
        "feed connected after {connected_s:.1} s; fallback {fallback:?} ({first:?}); live {live:?} \
         ({on_air:?}); back on playout {back:?} ({after:?}); {:.1} s of sound in {:.1} s",
        hls.seconds(),
        t0.elapsed().as_secs_f64()
    );
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let report = serde_json::json!({
            "feed_connected_s": connected_s,
            "fallback": fallback, "live": live, "back": back,
            "feed_connections": after.feed_connections,
        });
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    let (_, f3, f1) = fallback.expect("playout's fallback never reached the video's sound");
    assert_eq!(first.source, Source::Playout, "{first:?}");
    let (_, l1, l3) = live.expect("the live DJ never reached the video's sound");
    assert_eq!(on_air.source, Source::Desk, "{on_air:?}");
    back.expect("playout's sound did not come back after the DJ went off air");
    assert_eq!(after.source, Source::Playout, "{after:?}");
    assert!(f3 > 50.0 && f3 - f1 > 40.0 && l1 > 50.0 && l1 - l3 > 40.0);
}
