// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out (ADR-0230) with local cameras and tones in place of the room, the destinations'
//! tags written to FLV files in place of a server (the stack test, `just test-rtmp`, sends to
//! real local servers): both profiles carry the delayed programme, a dump takes the held
//! picture and sound out of RTMP too, a destination that fails is tried again with a growing
//! wait while everything else goes on, and a failed encoder comes back alone. Run in the desk
//! image (`just desk-check`).

mod rtmp_support;

use std::time::{Duration, Instant};

use gstreamer as gst;
use weavesignal_desk::analysis::{ratio_db, tone_power};
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::picture::Yuv;
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::rtmp::{Profile, SendState};
use weavesignal_proto::Role;

use rtmp_support::*;

#[test]
fn both_profiles_carry_the_programme_picture_and_sound() {
    gst::init().unwrap();
    let dir = scratch("both");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    v.rtmp().set_destinations_to_files(
        "t1",
        vec![
            dest("compat", "rtmp://unused/live", "k1", Profile::Compatible),
            dest("open", "rtmp://unused/live", "k2", Profile::Open),
        ],
        &dir,
    );
    desk.add_person("callum", Role::Presenter).unwrap();
    add(&desk, "callum", RED, 1000.0);
    desk.set_on_air("callum", true);
    run_for(&desk, Duration::from_secs(10));
    let status = v.status();
    println!("rtmp: {}", serde_json::to_string(&status.rtmp).unwrap());
    assert!(status.hls, "HLS stopped");
    assert_eq!(status.rtmp.set.as_deref(), Some("t1"));
    assert_eq!(status.rtmp.encoders.len(), 2);
    assert!(
        status
            .rtmp
            .destinations
            .iter()
            .all(|d| d.state == SendState::Live),
        "{:?}",
        status.rtmp.destinations
    );
    // Take the destinations away, so each file is closed (the files are then whole).
    v.rtmp().set_destinations_to_files("end", vec![], &dir);
    std::thread::sleep(Duration::from_millis(300));

    let c = probe(&dir.join("compat.flv"));
    assert!(
        c.contains("codec_name=h264") && c.contains("profile=") && c.contains("width=854"),
        "{c}"
    );
    assert!(
        c.contains("codec_name=aac") && c.contains("sample_rate=44100"),
        "{c}"
    );
    let o = probe(&dir.join("open.flv"));
    assert!(
        o.contains("codec_name=av1") && o.contains("width=854"),
        "{o}"
    );
    assert!(
        o.contains("codec_name=opus") && o.contains("sample_rate=48000"),
        "{o}"
    );

    for name in ["compat", "open"] {
        let s = flv_sound(&dir.join(format!("{name}.flv")));
        let levels = per_second(&s, 1000.0);
        println!("{name}: tone per second {levels:.1?}");
        assert!(
            levels.len() >= 6,
            "{name}: only {} s of sound",
            levels.len()
        );
        assert!(
            levels[1..].iter().all(|d| *d > 55.0),
            "{name}: the programme's tone is missing: {levels:?}"
        );
    }
    let pics = flv_pictures(&dir.join("compat.flv"));
    assert!(pics.len() >= 120, "only {} pictures", pics.len());
    let last = middle(pics.last().unwrap());
    assert!(
        last.distance(&Yuv::from_rgb(RED.0, RED.1, RED.2)) < 14.0,
        "{last:?} is not the camera"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dump_takes_the_held_picture_and_sound_out_of_rtmp_too() {
    gst::init().unwrap();
    let dir = scratch("dump");
    let desk = Desk::new(config(&dir, 4_000, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    v.rtmp().set_destinations_to_files(
        "t",
        vec![dest(
            "compat",
            "rtmp://unused/live",
            "k",
            Profile::Compatible,
        )],
        &dir,
    );
    desk.add_person("moira", Role::Dj).unwrap();
    desk.add_person("gregor", Role::Presenter).unwrap();
    add(&desk, "moira", RED, 1000.0);
    add(&desk, "gregor", BLUE, 3000.0);
    // A presenter starts on air: off until after the dump.
    desk.set_on_air("gregor", false);
    // The delay fills through silence and the slate.
    let t0 = Instant::now();
    while desk.delay().state.rebuilding && t0.elapsed() < Duration::from_secs(30) {
        run_for(&desk, Duration::from_millis(200));
    }
    assert!(!desk.delay().state.rebuilding, "the delay never filled");
    run_for(&desk, Duration::from_secs(1));
    // The marker goes in and is held...
    desk.set_on_air("moira", true);
    run_for(&desk, Duration::from_millis(1_500));
    desk.set_on_air("moira", false);
    run_for(&desk, Duration::from_millis(500));
    // ...and dumped before it comes out; then the second voice.
    assert!(desk.dump(), "nothing was held to dump");
    desk.set_on_air("gregor", true);
    run_for(&desk, Duration::from_secs(10));
    // Take the destinations away, so each file is closed (the files are then whole).
    v.rtmp().set_destinations_to_files("end", vec![], &dir);
    std::thread::sleep(Duration::from_millis(300));

    let file = dir.join("compat.flv");
    let s = flv_sound(&file);
    let marker = per_second(&s, 1000.0);
    // The rebuilding delay plays 2 % slow.
    let after: Vec<f64> = per_second(&s, 2940.0)
        .into_iter()
        .zip(per_second(&s, 3000.0))
        .map(|(a, b)| a.max(b))
        .collect();
    println!("marker per second {marker:.1?}\nafter per second {after:.1?}");
    // Heard, the marker reads about 125 dB here (2026-11-06); what the codec leaks of the
    // second voice around 1 kHz reads 65 or less.
    assert!(
        marker.iter().all(|d| *d < 90.0),
        "the dumped marker reached RTMP: {marker:?}"
    );
    assert!(
        after.iter().rev().take(4).all(|d| *d > 55.0),
        "the voice after the dump is missing: {after:?}"
    );
    let pics = flv_pictures(&file);
    let red = Yuv::from_rgb(RED.0, RED.1, RED.2);
    let blue = Yuv::from_rgb(BLUE.0, BLUE.1, BLUE.2);
    let reds = pics
        .iter()
        .filter(|f| middle(f).distance(&red) < 14.0)
        .count();
    let blues = pics
        .iter()
        .filter(|f| middle(f).distance(&blue) < 14.0)
        .count();
    println!(
        "{} pictures: {reds} of the dumped camera, {blues} of the one after",
        pics.len()
    );
    assert_eq!(reds, 0, "the dumped picture reached RTMP");
    assert!(blues > 40, "the picture after the dump is missing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_destination_that_fails_is_tried_again_and_harms_nothing_else() {
    gst::init().unwrap();
    let dir = scratch("fail");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    desk.add_person("callum", Role::Presenter).unwrap();
    add(&desk, "callum", RED, 1000.0);
    desk.set_on_air("callum", true);
    run_for(&desk, Duration::from_secs(3));
    // One destination that works, and one that nothing answers (port 9, discard).
    v.rtmp().set_destinations_to_files(
        "t",
        vec![
            dest("good", "rtmp://unused/live", "k", Profile::Compatible),
            dest(
                "bad",
                "rtmp://127.0.0.1:9/live",
                "do-not-print-me-123",
                Profile::Compatible,
            ),
        ],
        &dir,
    );
    run_for(&desk, Duration::from_secs(2));
    let good = dir.join("good.flv");
    let good_before = size(&good);
    let tap = desk.tap(&Bus::Programme).unwrap();
    let before = desk.mix_stats();
    let pushed = v.frames_pushed();
    run_for(&desk, Duration::from_secs(12));
    let st = v.status();
    println!("rtmp: {}", serde_json::to_string(&st.rtmp).unwrap());
    let bad = st
        .rtmp
        .destinations
        .iter()
        .find(|d| d.name == "bad")
        .unwrap();
    let ok = st
        .rtmp
        .destinations
        .iter()
        .find(|d| d.name == "good")
        .unwrap();
    assert_eq!(ok.state, SendState::Live);
    assert_eq!(ok.connects, 1, "the good destination was disturbed");
    println!(
        "good destination: {good_before} bytes, then {}",
        size(&good)
    );
    assert!(
        size(&good) > good_before + 20_000,
        "the good destination stopped growing"
    );
    assert!(
        bad.connects >= 3,
        "tried only {} times in 12 s",
        bad.connects
    );
    let err = bad.last_error.clone().expect("the failure is reported");
    assert!(
        !err.contains("do-not-print-me-123"),
        "the key is in the error: {err}"
    );
    assert!(
        matches!(bad.state, SendState::Retrying | SendState::Connecting),
        "{:?}",
        bad.state
    );
    let shown = serde_json::to_string(&st).unwrap();
    assert!(
        !shown.contains("do-not-print-me-123"),
        "the key is in the status"
    );
    // Nothing else was touched: HLS, the picture pass, the mixing pass.
    assert!(st.hls);
    assert!(v.frames_pushed() > pushed + 200, "the picture pass slowed");
    assert_eq!(
        desk.mix_stats().late,
        before.late,
        "the mixing pass was late"
    );
    let s = tap.samples();
    let windows: Vec<f64> = s
        .chunks(4_800)
        .filter(|c| c.len() == 4_800)
        .map(|c| ratio_db(tone_power(c, 1000.0), 1.0))
        .collect();
    assert!(windows.iter().all(|d| *d > 60.0), "a gap in the programme");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_encoder_comes_back_and_the_other_profile_and_hls_go_on() {
    gst::init().unwrap();
    let dir = scratch("encoder");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    v.rtmp().set_destinations_to_files(
        "t",
        vec![
            dest("compat", "rtmp://unused/live", "k1", Profile::Compatible),
            dest("open", "rtmp://unused/live", "k2", Profile::Open),
        ],
        &dir,
    );
    desk.add_person("callum", Role::Presenter).unwrap();
    add(&desk, "callum", RED, 1000.0);
    desk.set_on_air("callum", true);
    run_for(&desk, Duration::from_secs(5));
    let compat = dir.join("compat.flv");
    let grown = size(&compat);
    assert!(v.rtmp().fail_encode(Profile::Open));
    run_for(&desk, Duration::from_secs(1));
    let st = v.status();
    assert!(
        st.rtmp
            .encoders
            .iter()
            .all(|e| e.profile == Profile::Compatible),
        "the failed encoder still runs: {:?}",
        st.rtmp.encoders
    );
    assert!(st.hls);
    run_for(&desk, Duration::from_secs(4));
    let st = v.status();
    println!("rtmp: {}", serde_json::to_string(&st.rtmp).unwrap());
    assert_eq!(
        st.rtmp.encoders.len(),
        2,
        "the open encoder did not come back"
    );
    assert!(
        size(&compat) > grown + 50_000,
        "compatible stopped while open failed"
    );
    let open = st
        .rtmp
        .destinations
        .iter()
        .find(|d| d.name == "open")
        .unwrap();
    assert_eq!(open.state, SendState::Live);
    assert!(open.connects >= 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turned_off_nothing_is_encoded() {
    gst::init().unwrap();
    let dir = scratch("off");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    let mut d = dest("compat", "rtmp://unused/live", "k", Profile::Compatible);
    d.enabled = false;
    v.rtmp().set_destinations_to_files("t", vec![d], &dir);
    run_for(&desk, Duration::from_secs(3));
    let st = v.status();
    assert!(st.rtmp.encoders.is_empty(), "{:?}", st.rtmp.encoders);
    assert_eq!(st.rtmp.destinations[0].state, SendState::Off);
    assert!(!dir.join("compat.flv").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
