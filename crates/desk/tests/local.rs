// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk's pipeline with local tone sources in place of the room: no LiveKit, no network
//! beyond an SRT link on loopback. Needs GStreamer with the base, good and bad plugins (run
//! in the desk image: `just desk-check`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, peak_dbfs, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, ErrorScope, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

/// Settings with no input processing, so tone levels stay as sent.
fn config(srt_port: u16) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = {srt_port}

# Tones are measured by their pitch, and the delay's rebuild plays them a little slow.
[programme.delay]
target_ms = 0

[inputs.presenter]
[inputs.dj]
"#
        ),
        "test",
    )
    .unwrap()
}

/// A live sine at `freq` and `volume`, behind a ghost pad named like a LiveKit audio pad.
fn tone_at(pipeline: &gst::Pipeline, name: &str, freq: f64, volume: f64) -> (gst::Bin, gst::Pad) {
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
    (bin, ghost.upcast())
}

fn tone(pipeline: &gst::Pipeline, name: &str, freq: f64) -> gst::Pad {
    tone_at(pipeline, name, freq, 0.3).1
}

fn errors(p: &gst::Pipeline) {
    let bus = p.bus().unwrap();
    while let Some(msg) = bus.pop() {
        if let gst::MessageView::Error(e) = msg.view() {
            p.set_state(gst::State::Null).unwrap();
            panic!(
                "pipeline error from {:?}: {} ({:?})",
                msg.src().map(|s| s.path_string()),
                e.error(),
                e.debug()
            );
        }
    }
}

fn run_for(p: &gst::Pipeline, d: Duration) {
    let deadline = Instant::now() + d;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        errors(p);
    }
}

fn play_for(desk: &Desk, d: Duration) {
    let p = desk.pipeline();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, d);
}

fn tap(desk: &Desk, bus: Bus, _name: &str) -> Tap {
    desk.tap(&bus).unwrap()
}

fn mm(id: &str) -> Bus {
    Bus::MixMinus(id.to_owned())
}

/// `a` against `b` in a recording, in dB.
fn balance(t: &Tap, a: f64, b: f64) -> f64 {
    let s = t.samples();
    ratio_db(tone_power(&s, a), tone_power(&s, b))
}

/// A tone's power against silence, in dB: above about 60 dB it is there.
fn presence(t: &Tap, f: f64) -> f64 {
    ratio_db(tone_power(&t.samples(), f), 1.0)
}

/// The tones these tests send.
const TONES: [f64; 6] = [300.0, 1000.0, 2000.0, 3000.0, 5000.0, 7000.0];

/// Whether a tone is absent: under 30 dB over silence, or at least 60 dB under the loudest
/// other tone on the bus (a window's leakage from a loud tone is not a voice).
fn absent(t: &Tap, f: f64) -> bool {
    let s = t.samples();
    let me = ratio_db(tone_power(&s, f), 1.0);
    let loudest = TONES
        .iter()
        .filter(|&&g| g != f)
        .map(|&g| ratio_db(tone_power(&s, g), 1.0))
        .fold(f64::MIN, f64::max);
    me < 30.0f64.max(loudest - 60.0)
}

/// Sets a pipeline to NULL when dropped, so a failed assertion does not leave it playing.
struct Stop(gst::Pipeline);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
    }
}

#[test]
fn every_bus_outputs_from_its_first_moment_with_nobody_in_the_room() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    let prog = tap(&desk, Bus::Programme, "prog");
    let mmg = tap(&desk, mm("guest"), "mm");
    play_for(&desk, Duration::from_millis(1000));
    // 20 ms blocks: about 50 in a second; allow for start-up.
    assert!(prog.buffers() > 40, "programme: {} buffers", prog.buffers());
    assert!(mmg.buffers() > 40, "mix-minus: {} buffers", mmg.buffers());
    assert!(peak_dbfs(&prog.samples()) < -90.0, "the bed is silent");
}

#[test]
fn each_mix_minus_leaves_out_its_own_voice() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    desk.route_pad("guest", &tone(p, "guest-voice", 3000.0))
        .unwrap();
    assert_eq!(desk.links(), 2);
    let prog = tap(&desk, Bus::Programme, "prog");
    let mm_guest = tap(&desk, mm("guest"), "mmg");
    let mm_pres = tap(&desk, mm("presenter"), "mmp");
    play_for(&desk, Duration::from_millis(1500));

    let guest_own = balance(&mm_guest, 3000.0, 1000.0);
    let presenter_own = balance(&mm_pres, 1000.0, 3000.0);
    let programme_balance = balance(&prog, 3000.0, 1000.0);
    println!(
        "guest own voice {guest_own:.1} dB, presenter own voice {presenter_own:.1} dB, programme 3k/1k {programme_balance:.1} dB"
    );
    assert!(
        guest_own < -60.0,
        "the guest hears themselves: {guest_own:.1} dB"
    );
    assert!(
        presenter_own < -60.0,
        "the presenter hears themselves: {presenter_own:.1} dB"
    );
    assert!(
        programme_balance.abs() < 3.0,
        "the programme carries both: {programme_balance:.1} dB"
    );
}

#[test]
fn a_person_joining_late_hears_everyone_already_there() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    // The guest arrives after the presenter's voice is routed: their mix-minus must still
    // carry the presenter.
    desk.add_person("guest", Role::Guest).unwrap();
    let mmg = tap(&desk, mm("guest"), "mmg");
    play_for(&desk, Duration::from_millis(1000));
    let heard = presence(&mmg, 1000.0);
    assert!(
        heard > 60.0,
        "the late guest does not hear the presenter: {heard:.1} dB"
    );
}

#[test]
fn taking_a_voice_off_air_takes_it_off_every_bus_it_airs_on() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    desk.route_pad("guest", &tone(p, "guest-voice", 3000.0))
        .unwrap();
    assert!(desk.set_on_air("guest", false));
    assert!(!desk.set_on_air("stranger", false));
    let prog = tap(&desk, Bus::Programme, "prog");
    let mmp = tap(&desk, mm("presenter"), "mmp");
    play_for(&desk, Duration::from_millis(1000));
    let r = balance(&prog, 3000.0, 1000.0);
    assert!(
        r < -60.0,
        "the guest off air is still in the programme: {r:.1} dB"
    );
    assert!(
        absent(&mmp, 3000.0),
        "the presenter still hears the guest off air: {:.1} dB",
        presence(&mmp, 3000.0)
    );
}

#[test]
fn a_caller_waits_with_the_screener_until_sent_to_air() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    for (id, role) in [
        ("hamish", Role::Presenter),
        ("callum", Role::Caller),
        ("iona", Role::Caller),
        ("moira", Role::Screener),
    ] {
        desk.add_person(id, role).unwrap();
    }
    desk.route_pad("iona", &tone(p, "iona-voice", 7000.0))
        .unwrap();
    // The screener talks to Callum (her `T`, held): he hears her, Iona waiting does not.
    assert!(desk.set_talk_to("moira", "callum", true));
    desk.route_pad("hamish", &tone(p, "hamish-voice", 1000.0))
        .unwrap();
    desk.route_pad("callum", &tone(p, "callum-voice", 3000.0))
        .unwrap();
    desk.route_pad("moira", &tone(p, "moira-voice", 5000.0))
        .unwrap();
    let prog = tap(&desk, Bus::Programme, "prog");
    let mm_hamish = tap(&desk, mm("hamish"), "mm-hamish");
    let mm_callum = tap(&desk, mm("callum"), "mm-callum");
    let mm_moira = tap(&desk, mm("moira"), "mm-moira");
    let mm_iona = tap(&desk, mm("iona"), "mm-iona");
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_millis(1200));

    // Off air: only the screener hears the caller; the caller hears the programme and the
    // screener; the screener never reaches listeners.
    let prog_caller = presence(&prog, 3000.0);
    let hamish_caller = presence(&mm_hamish, 3000.0);
    let moira_caller = presence(&mm_moira, 3000.0);
    let prog_screener = presence(&prog, 5000.0);
    let callum_screener = presence(&mm_callum, 5000.0);
    let hamish_screener = presence(&mm_hamish, 5000.0);
    let callum_presenter = presence(&mm_callum, 1000.0);
    println!(
        "off air (dB over silence): caller in programme {prog_caller:.0}, to presenter {hamish_caller:.0}, \
         to screener {moira_caller:.0}; screener in programme {prog_screener:.0}, to caller {callum_screener:.0}, \
         to presenter {hamish_screener:.0}; presenter to caller {callum_presenter:.0}"
    );
    assert!(
        absent(&prog, 3000.0) && absent(&mm_hamish, 3000.0),
        "the caller leaks before air"
    );
    assert!(moira_caller > 60.0, "the screener cannot hear the caller");
    assert!(
        callum_screener > 60.0,
        "the caller cannot hear the screener"
    );
    assert!(
        callum_presenter > 60.0,
        "the caller waiting cannot hear the show"
    );
    assert!(
        absent(&mm_iona, 5000.0),
        "a caller the screener is not talking to hears her"
    );
    assert!(
        presence(&mm_moira, 7000.0) > 60.0,
        "the screener cannot hear a second caller"
    );
    assert!(absent(&mm_iona, 3000.0), "one waiting caller hears another");
    assert!(absent(&prog, 5000.0), "the screener is on air");
    assert!(
        absent(&mm_hamish, 5000.0),
        "the screener reaches the presenter without talkback"
    );

    // To air, and talkback held: the caller is in the programme, the screener in the
    // presenter's ear only.
    let prog2 = tap(&desk, Bus::Programme, "prog2");
    let mm_hamish2 = tap(&desk, mm("hamish"), "mm-hamish2");
    assert!(desk.set_on_air("callum", true));
    assert!(desk.set_talk("moira", true));
    run_for(p, Duration::from_millis(1200));
    p.set_state(gst::State::Null).unwrap();
    let prog_caller = presence(&prog2, 3000.0);
    let prog_screener = presence(&prog2, 5000.0);
    let hamish_screener = presence(&mm_hamish2, 5000.0);
    println!(
        "on air: caller in programme {prog_caller:.0}; talkback: screener to presenter {hamish_screener:.0}, in programme {prog_screener:.0}"
    );
    assert!(
        prog_caller > 60.0,
        "the caller sent to air is not in the programme"
    );
    assert!(
        hamish_screener > 60.0,
        "talkback does not reach the presenter"
    );
    assert!(absent(&prog2, 5000.0), "talkback reached listeners");
}

#[test]
fn the_producers_talkback_reaches_only_the_presenter_and_only_while_held() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    for (id, role) in [
        ("hamish", Role::Presenter),
        ("gregor", Role::Guest),
        ("ailsa", Role::Producer),
    ] {
        desk.add_person(id, role).unwrap();
    }
    desk.route_pad("ailsa", &tone(p, "ailsa-voice", 2000.0))
        .unwrap();
    let prog = tap(&desk, Bus::Programme, "prog");
    let mmh = tap(&desk, mm("hamish"), "mmh");
    let mmg = tap(&desk, mm("gregor"), "mmg");
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_millis(800));
    let before = presence(&mmh, 2000.0);
    let mmh2 = tap(&desk, mm("hamish"), "mmh2");
    assert!(desk.set_talk("ailsa", true));
    run_for(p, Duration::from_millis(800));
    p.set_state(gst::State::Null).unwrap();
    assert!(
        before < 30.0,
        "talkback reached the presenter before it was held"
    );
    assert!(
        presence(&mmh2, 2000.0) > 60.0,
        "held talkback did not reach the presenter"
    );
    assert!(absent(&mmg, 2000.0), "talkback reached a guest");
    assert!(absent(&prog, 2000.0), "talkback reached listeners");
}

#[test]
fn cue_mute_and_talk_to_one_person_move_only_the_voices_they_should() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    for (id, role) in [
        ("hamish", Role::Presenter),
        ("gregor", Role::Guest),
        ("finlay", Role::Dj),
    ] {
        desk.add_person(id, role).unwrap();
    }
    desk.route_pad("hamish", &tone(p, "hamish-voice", 1000.0))
        .unwrap();
    desk.route_pad("gregor", &tone(p, "gregor-voice", 3000.0))
        .unwrap();
    desk.route_pad("finlay", &tone(p, "finlay-voice", 5000.0))
        .unwrap();
    // Finlay off air but cued: the presenter hears him, listeners and the guest do not.
    assert!(desk.set_cue("finlay", true));
    // Gregor muted: off the programme and the presenter's ear, his on-air state kept.
    assert!(desk.set_mute("gregor", true));
    let prog = tap(&desk, Bus::Programme, "prog");
    let mmh = tap(&desk, mm("hamish"), "mmh");
    let mmg = tap(&desk, mm("gregor"), "mmg");
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_millis(1200));
    assert!(
        presence(&mmh, 5000.0) > 60.0,
        "the cue did not reach the presenter"
    );
    assert!(absent(&prog, 5000.0), "a cued source reached listeners");
    assert!(absent(&mmg, 5000.0), "a cued source reached a guest");
    assert!(absent(&prog, 3000.0), "a muted guest is on air");
    assert!(
        desk.status()
            .iter()
            .any(|s| s.identity == "gregor" && s.on_air && s.mute)
    );

    // The presenter holds talk to Gregor: Gregor hears him, the programme loses him.
    // (Taps after the switch has settled: buffers already queued still carry the old mix.)
    assert!(desk.set_talk_to("hamish", "gregor", true));
    assert!(desk.set_mute("gregor", false));
    run_for(p, Duration::from_millis(300));
    let prog2 = tap(&desk, Bus::Programme, "prog2");
    let mmg2 = tap(&desk, mm("gregor"), "mmg2");
    let mmf2 = tap(&desk, mm("finlay"), "mmf2");
    run_for(p, Duration::from_millis(1200));
    assert!(
        presence(&mmg2, 1000.0) > 60.0,
        "the guest does not hear the presenter"
    );
    assert!(absent(&prog2, 1000.0), "private talk reached listeners");
    assert!(absent(&mmf2, 1000.0), "private talk reached someone else");
    assert!(
        presence(&prog2, 3000.0) > 60.0,
        "the unmuted guest is not back on air"
    );

    // The meter reads each voice: a 0.3 sine at 1 kHz is about −10.5 LUFS.
    let st = desk.status();
    let hamish = st.iter().find(|s| s.identity == "hamish").unwrap();
    let lufs = hamish.lufs.expect("the presenter's voice is metered");
    assert!((lufs + 10.5).abs() < 1.0, "{lufs}");
    assert!(hamish.lufs_peak.unwrap() >= lufs - 0.1);
    assert!(desk.set_talk_to("hamish", "gregor", false));
    assert!(!desk.set_talk_to("hamish", "nobody", true));
    p.set_state(gst::State::Null).unwrap();
}

#[test]
fn the_programme_limiter_keeps_a_loud_sum_under_the_ceiling() {
    // Two voices at about -1 dBFS each sum to about +5 dBFS: in 16-bit integers the mixer
    // clipped them; in float the limiter brings them under -1 dBFS.
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone_at(p, "pv", 1000.0, 0.9).1)
        .unwrap();
    desk.route_pad("guest", &tone_at(p, "gv", 1500.0, 0.9).1)
        .unwrap();
    let prog = tap(&desk, Bus::Programme, "prog");
    play_for(&desk, Duration::from_millis(1500));
    // Leave out the first 100 ms: the look-ahead's settling.
    let s: Vec<i16> = prog.samples().into_iter().skip(4800).collect();
    let peak = peak_dbfs(&s);
    println!("programme peak with two voices near full scale: {peak:.2} dBFS");
    assert!(
        peak <= -0.9,
        "the programme peaks at {peak:.2} dBFS (ceiling -1)"
    );
    assert!(peak > -4.0, "the limiter took too much: {peak:.2} dBFS");
    let b = balance(&prog, 1000.0, 1500.0);
    assert!(
        b.abs() < 1.0,
        "the limiter unbalanced the voices: {b:.1} dB"
    );
}

#[test]
fn the_presenters_input_chain_brings_a_quiet_presenter_up() {
    // The default presenter chain (the spike's live run): compressor, +5.6 dB steered by the
    // leveller toward -25 dB of speech, limiter. A presenter at about -37 dBFS RMS against an
    // equal guest with no chain comes out louder, by the make-up and a little leveller.
    let desk = Desk::new(
        Config::from_toml(
            "[livekit]\nurl = \"ws://127.0.0.1:1\"\napi_key = \"u\"\nroom = \"local\"\n",
            "t",
        )
        .unwrap(),
        Outputs::NONE,
    )
    .unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone_at(p, "pv", 300.0, 0.02).1)
        .unwrap();
    desk.route_pad("guest", &tone_at(p, "gv", 3000.0, 0.02).1)
        .unwrap();
    let prog = tap(&desk, Bus::Programme, "prog");
    play_for(&desk, Duration::from_millis(3000));
    let lift = balance(&prog, 300.0, 3000.0);
    println!("presenter over an equal guest after 3 s: {lift:.1} dB");
    assert!(
        lift > 5.0,
        "the presenter's chain lifted them only {lift:.1} dB"
    );
    assert!(
        lift < 5.6 + 6.0 * 0.5 + 1.0,
        "the leveller moved faster than 0.5 dB per half second: {lift:.1} dB"
    );
}

#[test]
fn a_person_who_leaves_and_comes_back_is_mixed_again() {
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    let (gbin, gpad) = tone_at(p, "guest-voice", 3000.0, 0.3);
    desk.route_pad("guest", &gpad).unwrap();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_millis(600));

    // The guest's link drops: their source stops, and the desk lets them go entirely.
    gbin.set_state(gst::State::Null).unwrap();
    p.remove(&gbin).unwrap();
    desk.remove_person("guest");
    assert!(
        desk.tap(&mm("guest")).is_none(),
        "the leaver's mix-minus is still there"
    );
    let prog_gone = tap(&desk, Bus::Programme, "prog-gone");
    run_for(p, Duration::from_millis(600));
    // 20 ms blocks: about 30 in 600 ms.
    assert!(
        prog_gone.buffers() > 22,
        "the programme stalled when a person left: {} blocks in 600 ms",
        prog_gone.buffers()
    );
    assert!(
        absent(&prog_gone, 3000.0),
        "the leaver is still in the programme"
    );

    // They rejoin: a new source under the same identity.
    desk.add_person("guest", Role::Guest).unwrap();
    let (gbin2, gpad2) = tone_at(p, "guest-voice-2", 3000.0, 0.3);
    desk.route_pad("guest", &gpad2).unwrap();
    gbin2.sync_state_with_parent().unwrap();
    let prog_back = tap(&desk, Bus::Programme, "prog-back");
    let mmg_back = tap(&desk, mm("guest"), "mmg-back");
    let mmp_back = tap(&desk, mm("presenter"), "mmp-back");
    run_for(p, Duration::from_millis(1000));
    p.set_state(gst::State::Null).unwrap();
    assert!(
        presence(&prog_back, 3000.0) > 60.0,
        "the returning guest is not in the programme"
    );
    assert!(
        presence(&mmp_back, 3000.0) > 60.0,
        "the presenter does not hear the returning guest"
    );
    let own = balance(&mmg_back, 3000.0, 1000.0);
    assert!(
        own < -60.0,
        "the returning guest hears themselves: {own:.1} dB"
    );
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn the_room_keeps_flowing_while_playout_is_not_connected() {
    // The desk's SRT listener with nobody connected: srtsink drops what it is given. Behind
    // the leaky queues the mixers must keep running, or every guest stops hearing every
    // other guest (spike 01).
    let desk = Desk::new(
        config(free_udp_port()),
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    let mmg = tap(&desk, mm("guest"), "mm");
    play_for(&desk, Duration::from_millis(3000));
    let secs = mmg.seconds();
    println!("mix-minus carried {secs:.2} s of audio in 3 s with playout not connected");
    assert!(secs > 2.5, "the mix-minus stalled: {secs:.2} s in 3 s");
}

/// Playout's side of the SRT link, as Liquidsoap does it: a caller to the desk's listener
/// that demuxes Matroska and decodes Opus.
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

/// Wait up to `limit` for a tap to hold a second of audio; how long it took.
fn first_second(desk: &gst::Pipeline, tap: &Tap, limit: Duration) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        run_for(desk, Duration::from_millis(100));
        if tap.seconds() > 1.0 {
            return Some(t0.elapsed());
        }
    }
    None
}

#[test]
fn every_playout_that_connects_decodes_the_programme_from_its_first_moment() {
    // Playout connects late, goes away, and a new one connects, as when Liquidsoap starts
    // after the desk and then restarts. Each connection must start with Matroska's header, or
    // the new playout cannot decode: a desk that called out to playout reconnected without
    // it (GStreamer's matroskademux: "Invalid header") and sometimes not at all (2026-10-25).
    let port = free_udp_port();
    let desk = Desk::new(
        config(port),
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    let mmg = tap(&desk, mm("guest"), "mmg");
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    // The desk is up before playout.
    run_for(p, Duration::from_secs(2));
    // The link the master lamp reads (ADR-0062): down until playout connects.
    let never = desk.playout();
    let mut links = Vec::new();

    let (first, first_tap) = playout_caller(port, "playout-1");
    let _stop1 = Stop(first.clone());
    first.set_state(gst::State::Playing).unwrap();
    let first_after = first_second(p, &first_tap, Duration::from_secs(8));
    run_for(p, Duration::from_secs(1));
    links.push(desk.playout().connected);
    first.set_state(gst::State::Null).unwrap();

    // Playout is down for two seconds; the desk keeps mixing.
    run_for(p, Duration::from_secs(2));
    links.push(desk.playout().connected);

    let (second, second_tap) = playout_caller(port, "playout-2");
    let _stop2 = Stop(second.clone());
    second.set_state(gst::State::Playing).unwrap();
    let second_after = first_second(p, &second_tap, Duration::from_secs(8));
    run_for(p, Duration::from_secs(1));
    links.push(desk.playout().connected);
    second.set_state(gst::State::Null).unwrap();
    println!("playout link: before {never:?}, then connected {links:?}");
    assert!(!never.connected && never.for_ms.is_none());
    assert_eq!(
        links,
        [true, false, true],
        "the desk did not see playout come, go and come back"
    );
    let room_secs = mmg.seconds();
    println!(
        "late playout decoded a second {first_after:?} after connecting; restarted playout {second_after:?}; the guest's mix-minus carried {room_secs:.1} s"
    );
    assert!(
        first_after.is_some(),
        "a playout that connected after the desk started could not decode"
    );
    assert!(
        second_after.is_some(),
        "a restarted playout could not decode the programme"
    );
    for (t, which) in [(&first_tap, "late"), (&second_tap, "restarted")] {
        assert!(
            presence(t, 1000.0) > 60.0,
            "the {which} playout heard no presenter"
        );
    }
    assert!(
        room_secs > 6.0,
        "the room stalled while playout came and went: {room_secs:.1} s"
    );
}

/// The element directly in the desk's pipeline whose name starts with `prefix` and ends with
/// `suffix`.
fn element(desk: &Desk, prefix: &str, suffix: &str) -> gst::Element {
    desk.pipeline()
        .children()
        .into_iter()
        .find(|e| e.name().starts_with(prefix) && e.name().ends_with(suffix))
        .unwrap_or_else(|| panic!("no element {prefix}…{suffix} in the desk"))
}

/// Post an error from `el`, as a failing element does.
fn fail(el: &gst::Element) {
    let msg = gst::message::Error::builder(gst::CoreError::Failed, "forced by the test")
        .src(el)
        .build();
    let _ = el.post_message(msg);
}

/// Handle every error waiting on the desk's bus as the binary does.
fn handle_errors(desk: &Desk) -> Vec<ErrorScope> {
    let bus = desk.pipeline().bus().unwrap();
    let mut out = Vec::new();
    while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
        out.push(desk.handle_error(&msg));
    }
    out
}

#[test]
fn a_failure_in_one_persons_voice_or_mix_minus_stays_theirs() {
    // The staging run (2026-10-25): one failed subscription among a dozen people joining
    // stopped the whole desk, then playout's link and the listeners. Only what is the
    // person's own may go: their voice (subscription, input chain, routes) or their mix-minus.
    let desk = Desk::new(
        config(free_udp_port()),
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    for (id, role) in [
        ("presenter", Role::Presenter),
        ("guest", Role::Guest),
        ("caller", Role::Caller),
    ] {
        desk.add_person(id, role).unwrap();
    }
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    desk.route_pad("guest", &tone(p, "guest-voice", 3000.0))
        .unwrap();
    desk.route_pad("caller", &tone(p, "caller-voice", 5000.0))
        .unwrap();
    assert!(desk.set_on_air("caller", true));
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_millis(500));

    // The guest's input chain fails: their voice goes, nobody else's.
    fail(&element(&desk, "in-guest-", "-convert"));
    // The caller's return fails: it is made again, and they stay on air.
    fail(&element(&desk, "pub-mm-caller-", ""));
    let scopes = handle_errors(&desk);
    assert_eq!(
        scopes,
        [
            ErrorScope::Participant("guest".into()),
            ErrorScope::Participant("caller".into())
        ],
        "a person's failure was not kept to them"
    );
    assert!(
        desk.person("caller").unwrap().switches.on_air,
        "the caller lost their place on air when their mix-minus was made again"
    );
    assert!(
        !p.children()
            .iter()
            .any(|e| e.name().starts_with("in-guest-")),
        "the guest's failed input chain is still in the desk"
    );

    // The programme and its link are the desk's own: fatal.
    let prog = element(&desk, "programme-opus", "");
    let (scope, _) = desk.classify_error(
        &gst::message::Error::builder(gst::CoreError::Failed, "forced")
            .src(&prog)
            .build(),
    );
    assert_eq!(
        scope,
        ErrorScope::Fatal,
        "a programme failure was not fatal"
    );
    // An error from something the desk has already taken down: nothing to do.
    let gone = gst::ElementFactory::make("queue").build().unwrap();
    let (scope, _) = desk.classify_error(
        &gst::message::Error::builder(gst::CoreError::Failed, "late")
            .src(&gone)
            .build(),
    );
    assert_eq!(scope, ErrorScope::Stale);

    let prog_after = tap(&desk, Bus::Programme, "prog-after");
    let mmp_after = tap(&desk, mm("presenter"), "mmp-after");
    let mmc_after = tap(&desk, mm("caller"), "mmc-after");
    run_for(p, Duration::from_millis(1000));
    p.set_state(gst::State::Null).unwrap();
    assert!(
        prog_after.seconds() > 0.8,
        "the programme stopped: {:.2} s in 1 s",
        prog_after.seconds()
    );
    assert!(
        presence(&prog_after, 1000.0) > 60.0 && presence(&prog_after, 5000.0) > 60.0,
        "the programme lost a voice that did not fail"
    );
    assert!(
        absent(&prog_after, 3000.0),
        "the failed voice is still in the programme"
    );
    assert!(
        presence(&mmp_after, 5000.0) > 60.0,
        "the presenter no longer hears the caller"
    );
    assert!(
        presence(&mmc_after, 1000.0) > 60.0 && absent(&mmc_after, 5000.0),
        "the caller's remade mix-minus is wrong"
    );
    // The guest's voice went: their routes too, and their tone with it.
    let left: Vec<String> = p
        .children()
        .iter()
        .map(|e| e.name().to_string())
        .filter(|n| n.starts_with("in-guest-") || n == "guest-voice")
        .collect();
    assert!(left.is_empty(), "the guest's voice left {left:?} behind");
}

#[test]
fn a_stuck_publisher_does_not_stall_anyone_else() {
    // A mix-minus publisher that stops taking audio must not stop its bus's mixer: each voice
    // reaches every bus from one thread, so a mixer that stops taking audio stops every voice
    // that feeds it, everywhere, the programme included.
    let desk = Desk::new(config(1), Outputs::NONE).unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    desk.route_pad("guest", &tone(p, "guest-voice", 3000.0))
        .unwrap();
    // The presenter's mix-minus publisher stops taking audio.
    let stuck = element(&desk, "pub-mm-presenter-", "")
        .static_pad("sink")
        .unwrap();
    let probe = stuck
        .add_probe(gst::PadProbeType::BLOCK_DOWNSTREAM, |_, _| {
            gst::PadProbeReturn::Ok
        })
        .unwrap();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    // Long enough for any queue in the guest's way to fill.
    run_for(p, Duration::from_millis(2500));
    let prog = tap(&desk, Bus::Programme, "prog");
    let mmg = tap(&desk, mm("guest"), "mmg");
    run_for(p, Duration::from_millis(1500));
    stuck.remove_probe(probe);
    p.set_state(gst::State::Null).unwrap();
    println!(
        "while the presenter's publisher was stuck: programme guest {:.1} dB, presenter {:.1} dB; the guest hears the presenter at {:.1} dB",
        presence(&prog, 3000.0),
        presence(&prog, 1000.0),
        presence(&mmg, 1000.0)
    );
    // Both tones are sent at the same level: a guest who was cut off for most of the window
    // is tens of dB under the presenter.
    assert!(
        presence(&prog, 3000.0) > presence(&prog, 1000.0) - 6.0,
        "the guest left the programme when the presenter's mix-minus stalled"
    );
    assert!(
        presence(&prog, 1000.0) > 60.0 && presence(&mmg, 1000.0) > 60.0,
        "the presenter's voice stalled"
    );
}

#[test]
fn a_stuck_publisher_does_not_starve_the_programme() {
    // Review 0025, D3: one shared encoder thread, and that one stuck pushing into a mix-minus
    // publisher that takes nothing. The programme must still reach playout: it has its own
    // encoder thread. Before, it queued behind the stuck return, SRT carried nothing, and no
    // error said so.
    let port = free_udp_port();
    let mut c = config(port);
    c.desk.workers = 1;
    let desk = Desk::new(
        c,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.add_person("guest", Role::Guest).unwrap();
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    let stuck = element(&desk, "pub-mm-guest-", "")
        .static_pad("sink")
        .unwrap();
    let probe = stuck
        .add_probe(gst::PadProbeType::BLOCK_DOWNSTREAM, |_, _| {
            gst::PadProbeReturn::Ok
        })
        .unwrap();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    let (playout, heard) = playout_caller(port, "playout-stuck");
    let _stop_playout = Stop(playout.clone());
    playout.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_secs(4));
    stuck.remove_probe(probe);
    playout.set_state(gst::State::Null).unwrap();
    p.set_state(gst::State::Null).unwrap();
    println!(
        "with the only shared encoder stuck: playout decoded {:.1} s, the presenter at {:.1} dB",
        heard.seconds(),
        presence(&heard, 1000.0)
    );
    assert!(
        presence(&heard, 1000.0) > 60.0,
        "the programme stalled behind a stuck return"
    );
}

/// Count the buffers that reach an element's sink pad from now on.
fn count_buffers(el: &gst::Element) -> Arc<AtomicU64> {
    let n = Arc::new(AtomicU64::new(0));
    let c = n.clone();
    el.static_pad("sink")
        .unwrap()
        .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            c.fetch_add(1, Ordering::Relaxed);
            gst::PadProbeReturn::Ok
        })
        .unwrap();
    n
}

#[test]
fn a_stuck_return_drops_its_own_audio_and_holds_up_no_other_return() {
    // ADR-0168: each return has an overflow buffer between its encoder and its publisher, so
    // a guest whose publisher stops taking audio loses their own and holds no encoder. With
    // one shared encoder thread, a stuck publisher used to hold it: every other return, and
    // anything else the pool encodes, stopped behind it with no error raised. The programme
    // has its own thread (`a_stuck_publisher_does_not_starve_the_programme`).
    let port = free_udp_port();
    let mut c = config(port);
    c.desk.workers = 1;
    let desk = Desk::new(
        c,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    for (id, role) in [
        ("presenter", Role::Presenter),
        ("guest", Role::Guest),
        ("caller", Role::Caller),
    ] {
        desk.add_person(id, role).unwrap();
    }
    desk.route_pad("presenter", &tone(p, "presenter-voice", 1000.0))
        .unwrap();
    let guest_pub = element(&desk, "pub-mm-guest-", "");
    let presenter_out = count_buffers(&element(&desk, "pub-mm-presenter-", ""));
    let caller_out = count_buffers(&element(&desk, "pub-mm-caller-", ""));
    let guest_out = count_buffers(&guest_pub);
    let stuck = guest_pub.static_pad("sink").unwrap();
    let probe = stuck
        .add_probe(gst::PadProbeType::BLOCK_DOWNSTREAM, |_, _| {
            gst::PadProbeReturn::Ok
        })
        .unwrap();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    let (playout, heard) = playout_caller(port, "playout-stuck-return");
    let _stop_playout = Stop(playout.clone());
    playout.set_state(gst::State::Playing).unwrap();
    run_for(p, Duration::from_secs(3));
    let (presenter_stuck, caller_stuck) = (
        presenter_out.load(Ordering::Relaxed),
        caller_out.load(Ordering::Relaxed),
    );
    let programme_stuck = heard.seconds();

    // The guest's publisher takes audio again: their return comes back by itself.
    stuck.remove_probe(probe);
    let guest_before = guest_out.load(Ordering::Relaxed);
    run_for(p, Duration::from_millis(1500));
    let guest_after = guest_out.load(Ordering::Relaxed) - guest_before;
    let presenter_after = presenter_out.load(Ordering::Relaxed) - presenter_stuck;
    playout.set_state(gst::State::Null).unwrap();
    p.set_state(gst::State::Null).unwrap();
    println!(
        "with the only shared encoder and the guest's publisher stuck for 3 s: the presenter's \
         return sent {presenter_stuck} frames, the caller's {caller_stuck}, playout decoded \
         {programme_stuck:.1} s; once it took audio again the guest's return sent {guest_after} \
         frames in 1.5 s (the presenter's {presenter_after})"
    );
    // 20 ms frames: 150 in 3 s, less the start.
    assert!(
        presenter_stuck > 100 && caller_stuck > 100,
        "a stuck return held up the others: presenter {presenter_stuck}, caller {caller_stuck} frames in 3 s"
    );
    assert!(
        presence(&heard, 1000.0) > 60.0 && programme_stuck > 1.0,
        "the programme stalled behind a stuck return"
    );
    assert!(
        guest_after > 40,
        "the guest's return did not come back: {guest_after} frames in 1.5 s"
    );
    assert!(
        presenter_after > 50,
        "the presenter's return stopped when the guest's came back: {presenter_after} frames"
    );
}

#[test]
fn dumped_audio_never_reaches_playout() {
    // The profanity delay at 2 s: it builds up through silence at half speed, a marker tone
    // goes in, the delay is dumped while the marker is still held, and what follows is a
    // second tone. Playout must decode the second tone and never the marker.
    let port = free_udp_port();
    let mut c = config(port);
    c.programme.delay.target_ms = 2_000;
    let desk = Desk::new(
        c,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    desk.add_person("presenter", Role::Presenter).unwrap();
    let (bin, pad) = tone_at(p, "presenter-voice", 1000.0, 0.0);
    desk.route_pad("presenter", &pad).unwrap();
    let src = bin
        .children()
        .into_iter()
        .find(|e| e.factory().is_some_and(|f| f.name() == "audiotestsrc"))
        .unwrap();
    let sent = tap(&desk, Bus::Programme, "sent");
    let delayed = desk.tap_delayed();
    let _stop = Stop(p.clone());
    p.set_state(gst::State::Playing).unwrap();
    let (playout, heard) = playout_caller(port, "playout-dump");
    let _stop_playout = Stop(playout.clone());
    playout.set_state(gst::State::Playing).unwrap();

    // Silence until the delay is full.
    let t0 = Instant::now();
    while desk.delay().state.rebuilding && t0.elapsed() < Duration::from_secs(10) {
        run_for(p, Duration::from_millis(100));
    }
    let full = desk.delay();
    assert!(!full.state.rebuilding, "the delay never filled: {full:?}");
    assert!(desk.dump(), "a full delay refused a dump");
    // That dump emptied a delay of silence; let it fill again.
    let t1 = Instant::now();
    while desk.delay().state.rebuilding && t1.elapsed() < Duration::from_secs(10) {
        run_for(p, Duration::from_millis(100));
    }
    assert!(!desk.delay().state.rebuilding);

    // The marker: 800 ms of 1 kHz, then silence, and the dump 600 ms after it ends, with the
    // whole marker still held (the delay is 2 s).
    src.set_property("volume", 0.3f64);
    run_for(p, Duration::from_millis(800));
    src.set_property("volume", 0.0f64);
    run_for(p, Duration::from_millis(600));
    let before = desk.delay();
    assert!(desk.dump(), "the dump was refused: {before:?}");
    let after = desk.delay();
    assert!(after.state.rebuilding && after.state.dumps == 2);
    assert!(!desk.dump(), "a second dump with nothing held was taken");
    // Then the second tone, long enough to come through the rebuilding delay.
    let second = Instant::now();
    src.set_property("freq", 3000.0f64);
    src.set_property("volume", 0.3f64);
    run_for(p, Duration::from_millis(4_000));
    playout.set_state(gst::State::Null).unwrap();
    p.set_state(gst::State::Null).unwrap();

    // Before the second tone only the marker or silence can come out: there, nothing at all
    // may. After it, the marker must still be absent next to the tone (a window's leakage and
    // the rebuild's resampling put a trace of any loud tone everywhere, 60 dB down or more).
    let until = |t: &Tap, end: Instant| -> Vec<i16> {
        t.chunks()
            .into_iter()
            .filter(|(at, _)| *at < end)
            .flat_map(|(_, s)| s)
            .collect()
    };
    let level = |s: &[i16], f: f64| ratio_db(tone_power(s, f), 1.0);
    let marker_sent = presence(&sent, 1000.0);
    let marker_out = level(&until(&delayed, second), 1000.0);
    let marker_heard = level(&until(&heard, second), 1000.0);
    // The rebuild plays the second tone 2 % slow: 2,940 Hz, a bin over 10 Hz wide each side.
    let after_heard = presence(&heard, 2940.0).max(presence(&heard, 3000.0));
    println!(
        "delay before the dump {:?}, after {:?}; marker sent {marker_sent:.0} dB, out of the delay {marker_out:.0} dB, at playout {marker_heard:.0} dB; the tone after the dump at playout {after_heard:.0} dB ({:.1} s decoded)",
        before.state,
        after.state,
        heard.seconds()
    );
    assert!(marker_sent > 60.0, "the marker never reached the programme");
    assert!(
        marker_out < 30.0,
        "dumped audio left the delay: {marker_out:.1} dB"
    );
    assert!(
        marker_heard < 30.0,
        "dumped audio reached playout: {marker_heard:.1} dB"
    );
    assert!(
        after_heard > 60.0,
        "the programme after the dump did not reach playout: {after_heard:.1} dB"
    );
}
