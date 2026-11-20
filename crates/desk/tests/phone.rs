// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line at the desk (17f, module 9), with a local tone in place of the booth's caller
//! and a real FLAC as the words: a phone caller is held out of every bus (the screener's ear too)
//! until admitted; the words are the caller's whole return while they play and reach nobody
//! else; their end is counted; keys are numbered with the words' version; the socket's three
//! requests answer as the control plane expects; a hang-up takes the caller off the desk and
//! hands the booth to the room watch. Needs GStreamer (run in the desk image: `just desk-check`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::control::handle;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::phone::PhoneSeen;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_desk::room::Seen;
use weavesignal_proto::Role;

const PERSON: &str = "phone-Xk3pQ9mZr2Ta";
const ROOM: &str = "local-phone_0123_Xk3pQ9mZr2Ta";
const SIP: &str = "sip_a8acc3a90a7b4e4d";
/// The caller's voice, the words, and the screener's voice: far apart.
const VOICE_HZ: f64 = 700.0;
const WORDS_HZ: f64 = 2500.0;
const SCREENER_HZ: f64 = 300.0;

static N: AtomicU32 = AtomicU32::new(1);

/// A folder of the test's own: the tests run at once, and each removes its folder at the end.
fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "weavesignal-desk-phone-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `secs` of a tone as FLAC, the way the media worker leaves the station's words.
fn flac(dir: &Path, name: &str, freq: f64, secs: f64) {
    gst::init().unwrap();
    let line = format!(
        "audiotestsrc num-buffers={} samplesperbuffer=480 freq={freq} volume=0.3 ! audio/x-raw,format=S16LE,rate=48000,channels=1 ! flacenc ! filesink location={}",
        (secs * 100.0).round() as u32,
        dir.join(name).display()
    );
    let p = gst::parse::launch(&line).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let msg = p
        .bus()
        .unwrap()
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(30),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the FLAC was written");
    assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
    p.set_state(gst::State::Null).unwrap();
}

fn config(dir: &Path) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme.delay]
target_ms = 0

[phone]
words_dir = "{}"
"#,
            dir.display()
        ),
        "test",
    )
    .unwrap()
}

fn tone_at(pipeline: &gst::Pipeline, name: &str, freq: f64) -> gst::Pad {
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

fn run_for(p: &gst::Pipeline, d: Duration) {
    let deadline = Instant::now() + d;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        while let Some(msg) = p.bus().unwrap().pop() {
            if let gst::MessageView::Error(e) = msg.view() {
                p.set_state(gst::State::Null).unwrap();
                panic!("pipeline error: {}", e.error());
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

/// A tone's power in what a tap recorded since `from`, in dB over silence (above about 60 dB it
/// is there).
fn since(t: &Tap, from: Instant, f: f64) -> f64 {
    let s: Vec<i16> = t
        .chunks()
        .into_iter()
        .filter(|(at, _)| *at >= from)
        .flat_map(|(_, s)| s)
        .collect();
    ratio_db(tone_power(&s, f), 1.0)
}

fn booth() -> (Seen, PhoneSeen) {
    (
        Seen {
            identity: PERSON.into(),
            name: "Phone 0123".into(),
            sid: "PA_phone".into(),
            role: Role::Caller,
            has_audio: true,
            muted: false,
            room: Some(ROOM.into()),
        },
        PhoneSeen {
            person: PERSON.into(),
            room: ROOM.into(),
            tail: "0123".into(),
            sip_identity: SIP.into(),
        },
    )
}

#[test]
fn a_phone_caller_is_held_until_admitted_and_hears_the_words_alone() {
    let dir = scratch();
    flac(&dir, "phone-words-default.flac", WORDS_HZ, 1.0);
    let desk = Desk::new(config(&dir), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    let ask = |line: serde_json::Value| -> serde_json::Value {
        serde_json::to_value(handle(&desk, &line.to_string())).unwrap()
    };

    // The room watch finds the booth; the caller joins the desk in it.
    let seen = desk.note_phone_booths(vec![booth()]);
    assert_eq!(seen.len(), 1);
    desk.add_person_in(PERSON, Role::Caller, Some(ROOM))
        .unwrap();
    desk.route_pad(PERSON, &tone_at(&p, "phone-voice", VOICE_HZ))
        .unwrap();
    desk.add_person("screener", Role::Screener).unwrap();
    desk.route_pad("screener", &tone_at(&p, "screener-voice", SCREENER_HZ))
        .unwrap();
    // The screener talks to the caller, as at the screener's `T`.
    desk.set_talk_to("screener", PERSON, true);
    let screener_ear = desk.tap(&Bus::MixMinus("screener".into())).unwrap();
    let caller_ear = desk.tap(&Bus::MixMinus(PERSON.into())).unwrap();
    let programme = desk.tap(&Bus::Programme).unwrap();
    p.set_state(gst::State::Playing).unwrap();

    // Not answered, not admitted: the status says so, and nobody hears the caller.
    let st = ask(serde_json::json!({"op": "status"}));
    let b = &st["phone"]["booths"][0];
    assert_eq!(b["person"], PERSON, "{st}");
    assert_eq!(b["sip_identity"], SIP);
    assert_eq!(
        (b["answered"].as_bool(), b["admitted"].as_bool()),
        (Some(false), Some(false))
    );
    let t0 = Instant::now();
    run_for(&p, Duration::from_millis(600));
    let held = since(&screener_ear, t0, VOICE_HZ);
    println!("held: the caller in the screener's ear at {held:.0} dB");
    assert!(
        held < 30.0,
        "a held phone caller reached the screener: {held:.1} dB"
    );

    // The words: answered, heard in the caller's ear alone, and counted when they end.
    let r = ask(serde_json::json!({"op": "phone_words", "person": PERSON,
        "file": "phone-words-default.flac", "version": "v1", "gain_db": 0.0}));
    assert_eq!(r["ok"], true, "{r}");
    let t1 = Instant::now();
    run_for(&p, Duration::from_millis(500));
    let words_in_ear = since(&caller_ear, t1, WORDS_HZ);
    // From a moment after the words started: the blocks mixed before them carry the screener.
    let screener_in_ear = since(&caller_ear, t1 + Duration::from_millis(150), SCREENER_HZ);
    let words_on_air = since(&programme, t1, WORDS_HZ);
    println!(
        "words: in the caller's ear {words_in_ear:.0} dB (the screener under them {screener_in_ear:.0} dB), on air {words_on_air:.0} dB"
    );
    assert!(words_in_ear > 60.0, "the caller did not hear the words");
    assert!(screener_in_ear < 30.0, "the words are not the whole return");
    assert!(words_on_air < 30.0, "the words reached the programme");
    let st = ask(serde_json::json!({"op": "status"}));
    assert_eq!(st["phone"]["booths"][0]["answered"], true);
    assert_eq!(st["phone"]["booths"][0]["words_playing"], "v1");

    // A key while they play carries their version.
    desk.phone_key(PERSON, 5);
    run_for(&p, Duration::from_millis(1000));
    let st = ask(serde_json::json!({"op": "status"}));
    let b = &st["phone"]["booths"][0];
    assert_eq!(
        (b["words_playing"].is_null(), b["words_done"].as_u64()),
        (true, Some(1)),
        "{st}"
    );
    let k = &st["phone"]["keys"][0];
    assert_eq!((k["seq"].as_u64(), k["code"].as_u64()), (Some(1), Some(5)));
    assert_eq!(k["words_version"], "v1");

    // Admitted: the screener hears the caller now.
    let r = ask(serde_json::json!({"op": "phone_admit", "person": PERSON}));
    assert_eq!(r["ok"], true, "{r}");
    let t2 = Instant::now();
    run_for(&p, Duration::from_millis(600));
    let through = since(&screener_ear, t2, VOICE_HZ);
    println!("admitted: the caller in the screener's ear at {through:.0} dB");
    assert!(
        through > 60.0,
        "an admitted caller did not reach the screener"
    );

    // Refusals: a path for a file, someone not in a booth.
    let r = ask(serde_json::json!({"op": "phone_words", "person": PERSON,
        "file": "../x.flac", "version": "v1"}));
    assert_eq!(
        (r["ok"].as_bool(), r["code"].as_str()),
        (Some(false), Some("phone"))
    );
    let r = ask(serde_json::json!({"op": "phone_admit", "person": "screener"}));
    assert_eq!(r["code"], "phone");

    // The hang-up: off the desk at once, and the booth to the room watch.
    let r = ask(serde_json::json!({"op": "phone_hang_up", "person": PERSON}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(desk.person(PERSON).is_none());
    assert_eq!(
        desk.take_phone_hang_ups(),
        vec![(ROOM.to_owned(), SIP.to_owned())]
    );
    assert!(
        desk.note_phone_booths(vec![booth()]).is_empty(),
        "a booth hung up came back"
    );
}

#[test]
fn with_the_words_off_the_admission_answers_the_call() {
    let dir = scratch();
    let desk = Desk::new(config(&dir), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    desk.note_phone_booths(vec![booth()]);
    desk.add_person_in(PERSON, Role::Caller, Some(ROOM))
        .unwrap();
    p.set_state(gst::State::Playing).unwrap();
    assert!(!desk.phone_status().booths[0].answered);
    desk.phone_admit(PERSON).unwrap();
    let b = &desk.phone_status().booths[0];
    assert!(b.answered && b.admitted);
    assert_eq!(b.words_done, 0);
    // A missing words file is refused, not played.
    assert!(desk.phone_words(PERSON, "missing.flac", "v1", 0.0).is_err());
}

#[test]
fn the_shipped_words_play_from_ogg_opus_to_the_end() {
    use weavesignal_desk::analysis::peak_dbfs;
    let dir = scratch();
    let shipped =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/phone/phone-words-alba.opus");
    std::fs::copy(&shipped, dir.join("phone-words-alba.opus")).unwrap();
    let desk = Desk::new(config(&dir), Outputs::NONE).unwrap();
    let p = desk.pipeline().clone();
    let _stop = Stop(p.clone(), dir.clone());
    desk.note_phone_booths(vec![booth()]);
    desk.add_person_in(PERSON, Role::Caller, Some(ROOM))
        .unwrap();
    let caller_ear = desk.tap(&Bus::MixMinus(PERSON.into())).unwrap();
    let programme = desk.tap(&Bus::Programme).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    run_for(&p, Duration::from_millis(300));
    desk.phone_words(PERSON, "phone-words-alba.opus", "v-alba", 0.0)
        .unwrap();
    assert!(desk.phone_status().booths[0].answered);
    let t = Instant::now();
    run_for(&p, Duration::from_millis(2000));
    let heard: Vec<i16> = caller_ear
        .chunks()
        .into_iter()
        .filter(|(at, _)| *at >= t)
        .flat_map(|(_, s)| s)
        .collect();
    let on_air: Vec<i16> = programme
        .chunks()
        .into_iter()
        .flat_map(|(_, s)| s)
        .collect();
    let (ear_db, air_db) = (peak_dbfs(&heard), peak_dbfs(&on_air));
    println!(
        "the shipped words: the caller's ear peaks at {ear_db:.1} dBFS, the programme at {air_db:.1}"
    );
    assert!(ear_db > -20.0, "the caller did not hear the shipped words");
    assert!(air_db < -60.0, "the words reached the programme");
    // About 7 s long: done once, then the return is a waiting caller's again.
    let deadline = Instant::now() + Duration::from_secs(10);
    while desk.phone_status().booths[0].words_done == 0 && Instant::now() < deadline {
        run_for(&p, Duration::from_millis(200));
    }
    let b = &desk.phone_status().booths[0];
    assert_eq!((b.words_done, b.words_playing.as_deref()), (1, None));
}
