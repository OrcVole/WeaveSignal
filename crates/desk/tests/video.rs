// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video path with local cameras and tones in place of the room (M5): a camera's picture
//! reaches the HLS output, the vision mixer follows the speaker, the slate when nobody is on
//! camera, picture and sound leave the delay together, and a video failure leaves the sound on
//! air. Run in the desk image (`just desk-check`); the encoder is SVT-AV1 unless the container
//! has the GPU (`WEAVESIGNAL_DESK_GPU=1`), when it is NVENC.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::feed::{basic_auth, wav_header};
use weavesignal_desk::hls_sound::Source;
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::picture::{Frame, Yuv};
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_desk::video::Encoder;
use weavesignal_desk::vision::{PictureMode, Shot};
use weavesignal_proto::Role;

const RED: (u8, u8, u8) = (220, 40, 40);
const BLUE: (u8, u8, u8) = (40, 60, 220);
/// The slate's background and accent (video.rs).
const SLATE_BG: (u8, u8, u8) = (0x15, 0x17, 0x1c);
const ACCENT: (u8, u8, u8) = (0xd9, 0x3a, 0x2b);

fn scratch(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "weavesignal-video-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(dir: &Path, delay_ms: u32, extra: &str) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = 26999

[programme.delay]
target_ms = {delay_ms}

[inputs.presenter]
[inputs.dj]

[video]
enabled = true
lower_third_ms = 0

[video.hls]
dir = "{}"
{extra}
"#,
            dir.display()
        ),
        "test",
    )
    .map(|mut c| {
        // No feed from playout unless a test sends one: tests run at once, on one port.
        c.video.playout_feed.enabled = false;
        c
    })
    .unwrap()
}

/// A live solid-colour camera, 640x480 at 20 fps, behind a ghost pad named like LiveKit's.
fn camera(pipeline: &gst::Pipeline, name: &str, rgb: (u8, u8, u8)) -> gst::Pad {
    let bin = gst::Bin::with_name(name);
    let src = gst::ElementFactory::make("videotestsrc")
        .property("is-live", true)
        .property_from_str("pattern", "solid-color")
        .property(
            "foreground-color",
            0xff00_0000u32 | (u32::from(rgb.0) << 16) | (u32::from(rgb.1) << 8) | u32::from(rgb.2),
        )
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property(
            "caps",
            "video/x-raw,format=I420,width=640,height=480,framerate=20/1"
                .parse::<gst::Caps>()
                .unwrap(),
        )
        .build()
        .unwrap();
    bin.add_many([&src, &caps]).unwrap();
    src.link(&caps).unwrap();
    let ghost = gst::GhostPad::builder_with_target(&caps.static_pad("src").unwrap())
        .unwrap()
        .name("video_0")
        .build();
    bin.add_pad(&ghost).unwrap();
    pipeline.add(&bin).unwrap();
    ghost.upcast()
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

/// Sets a pipeline to NULL when dropped, so a failed assertion does not leave it playing.
struct Stop(Desk);

impl Drop for Stop {
    fn drop(&mut self) {
        if let Some(v) = self.0.video() {
            v.stop();
        }
        let _ = self.0.pipeline().set_state(gst::State::Null);
    }
}

fn start(desk: &Desk) -> Stop {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    Stop(desk.clone())
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn running_ns(desk: &Desk) -> u64 {
    let p = desk.pipeline();
    (p.clock().unwrap().time() - p.base_time().unwrap()).nseconds()
}

/// One rendition, as a player would fetch it: the playlist's init section and every segment it
/// lists, joined into one file.
fn fetch(dir: &Path, rendition: &str) -> PathBuf {
    let d = dir.join(rendition);
    let playlist = std::fs::read_to_string(d.join("index.m3u8")).expect("a playlist");
    let init = playlist
        .lines()
        .find_map(|l| l.strip_prefix("#EXT-X-MAP:URI=\""))
        .map(|l| l.trim_end_matches('"').to_owned())
        .expect("an init section");
    let mut bytes = std::fs::read(d.join(&init)).unwrap();
    let segments: Vec<&str> = playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect();
    assert!(!segments.is_empty(), "no segments in {rendition}");
    for s in &segments {
        bytes.extend(std::fs::read(d.join(s)).unwrap());
    }
    let out = dir.join(format!("{}-joined.mp4", rendition.replace('/', "-")));
    std::fs::write(&out, bytes).unwrap();
    out
}

/// Every picture in a joined rendition, decoded to raw frames (never scored as a container,
/// bench 01's trap), with its timestamp in ns.
fn pictures(file: &Path) -> Vec<(u64, Frame)> {
    let p = gst::parse::launch(&format!(
        "filesrc location={} ! qtdemux ! av1parse ! dav1ddec ! videoconvert ! video/x-raw,format=I420 ! appsink name=out sync=false",
        file.display()
    ))
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    let sink = p
        .by_name("out")
        .unwrap()
        .downcast::<gst_app::AppSink>()
        .unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let mut out = Vec::new();
    while let Some(s) = sink.try_pull_sample(gst::ClockTime::from_seconds(10)) {
        let caps = s.caps().unwrap().structure(0).unwrap().to_owned();
        let (w, h) = (
            caps.get::<i32>("width").unwrap() as usize,
            caps.get::<i32>("height").unwrap() as usize,
        );
        let buf = s.buffer().unwrap();
        let map = buf.map_readable().unwrap();
        out.push((
            buf.pts().unwrap().nseconds(),
            Frame::from_bytes(w, h, map.as_slice()).expect("a whole picture"),
        ));
    }
    p.set_state(gst::State::Null).unwrap();
    out
}

/// The sound in a joined audio rendition, mono 16-bit, and its first timestamp in ns.
fn sound(file: &Path) -> (u64, Vec<i16>) {
    let p = gst::parse::launch(&format!(
        "filesrc location={} ! qtdemux ! opusparse ! opusdec ! audioconvert ! audio/x-raw,format=S16LE,channels=1,rate=48000 ! appsink name=out sync=false",
        file.display()
    ))
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    let sink = p
        .by_name("out")
        .unwrap()
        .downcast::<gst_app::AppSink>()
        .unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let mut first = None;
    let mut out = Vec::new();
    while let Some(s) = sink.try_pull_sample(gst::ClockTime::from_seconds(10)) {
        let buf = s.buffer().unwrap();
        first.get_or_insert(buf.pts().unwrap().nseconds());
        let map = buf.map_readable().unwrap();
        out.extend(
            map.as_slice()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b)),
        );
    }
    p.set_state(gst::State::Null).unwrap();
    (first.expect("some sound"), out)
}

/// Route a test's camera to a person, then set it playing (the desk is already playing).
fn add_camera(desk: &Desk, id: &str, rgb: (u8, u8, u8)) {
    let pad = camera(desk.pipeline(), &format!("{id}-cam"), rgb);
    desk.route_camera(id, &pad).unwrap();
    pad.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
}

/// Route a test's tone to a person, then set it playing.
fn add_voice(desk: &Desk, id: &str, freq: f64) {
    let pad = tone(desk.pipeline(), &format!("{id}-voice"), freq);
    desk.route_pad(id, &pad).unwrap();
    pad.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
}

fn yuv(c: (u8, u8, u8)) -> Yuv {
    Yuv::from_rgb(c.0, c.1, c.2)
}

/// The middle of a picture.
fn middle(f: &Frame) -> Yuv {
    f.mean(f.width / 3, f.height / 3, f.width / 3, f.height / 3)
}

fn wait_shot(desk: &Desk, want: &Shot, limit: Duration) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if &desk.video().unwrap().status().shot == want {
            return true;
        }
        if t0.elapsed() + Duration::from_millis(60) >= limit {
            println!(
                "status: {}",
                serde_json::to_string(&desk.video().unwrap().status()).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn person(id: &str) -> Shot {
    Shot::Person {
        identity: id.into(),
    }
}

#[test]
fn a_cameras_picture_reaches_the_hls_output_with_the_programmes_sound() {
    gst::init().unwrap();
    let dir = scratch("reach");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    desk.add_person("callum", Role::Presenter).unwrap();
    add_voice(&desk, "callum", 1000.0);
    add_camera(&desk, "callum", RED);
    assert!(
        wait_shot(&desk, &person("callum"), Duration::from_secs(5)),
        "never cut to the camera"
    );
    run_for(&desk, Duration::from_secs(7));

    let status = desk.video().unwrap().status();
    println!("video status: {}", serde_json::to_string(&status).unwrap());
    assert!(status.hls);
    assert_eq!(status.cameras, vec!["callum".to_owned()]);
    let encoder = status.rungs[0].encoder;
    println!("encoder: {encoder:?}");

    let live = std::fs::read_to_string(dir.join("live.m3u8")).unwrap();
    assert!(
        live.contains("av01.0.04M.08,opus") && live.contains("RESOLUTION=854x480"),
        "{live}"
    );
    let audio_only = std::fs::read_to_string(dir.join("audio.m3u8")).unwrap();
    assert!(!audio_only.contains("av01"), "{audio_only}");
    let v0 = std::fs::read_to_string(dir.join("v0/index.m3u8")).unwrap();
    assert!(v0.contains("#EXT-X-TARGETDURATION:2"), "{v0}");

    let pics = pictures(&fetch(&dir, "v0"));
    assert!(pics.len() >= 60, "only {} pictures", pics.len());
    let (w, h) = (pics[0].1.width, pics[0].1.height);
    assert_eq!((w, h), (854, 480));
    // The last two seconds: the camera's red in the middle, black pillars at the sides (a
    // 4:3 camera on a 16:9 picture).
    let recent = &pics[pics.len() - 40..];
    for (t, f) in recent {
        let m = middle(f);
        assert!(
            m.distance(&yuv(RED)) < 12.0,
            "at {t}: {m:?} is not the camera's red"
        );
        let side = f.mean(0, 0, 80, h);
        assert!(side.y < 24.0, "at {t}: no pillar ({side:?})");
    }
    // Two-second segments, keyframe at each start: 40 pictures apart at 20 fps.
    let gaps: Vec<u64> = pics.windows(2).map(|w| w[1].0 - w[0].0).collect();
    assert!(
        gaps.iter().all(|g| (49_000_000..=51_000_000).contains(g)),
        "irregular frame times"
    );

    let (_, samples) = sound(&fetch(&dir, "audio"));
    let s: Vec<i16> = samples[samples.len().saturating_sub(48_000)..].to_vec();
    let present = ratio_db(tone_power(&s, 1000.0), 1.0);
    assert!(
        present > 60.0,
        "the programme's tone is missing from the HLS audio: {present:.1} dB"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_slate_when_nobody_is_on_air_on_camera() {
    gst::init().unwrap();
    let dir = scratch("slate");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap();
    v.set_now_playing(weavesignal_desk::video::NowPlaying {
        station: Some("Radio Kettle".into()),
        show: Some("The Morning Kettle".into()),
        title: Some("Skye Boat Song".into()),
        artist: Some("The Corries".into()),
    });
    // Someone on air without a camera; someone with a camera, off air.
    desk.add_person("callum", Role::Presenter).unwrap();
    desk.add_person("moira", Role::Dj).unwrap();
    add_voice(&desk, "callum", 1000.0);
    add_camera(&desk, "moira", BLUE);
    run_for(&desk, Duration::from_secs(7));
    assert_eq!(v.status().shot, Shot::Slate);
    let pics = pictures(&fetch(&dir, "v0"));
    let recent = &pics[pics.len() - 40..];
    for (t, f) in recent {
        let corner = f.mean(8, 8, 120, 60);
        assert!(
            corner.distance(&yuv(SLATE_BG)) < 10.0,
            "at {t}: {corner:?} is not the slate"
        );
        let bar = f.mean(0, f.height - 6, f.width, 4);
        assert!(
            bar.distance(&yuv(ACCENT)) < 16.0,
            "at {t}: no accent bar ({bar:?})"
        );
        // The text: the middle is not plain background.
        let text = f.mean(f.width / 4, f.height / 3, f.width / 2, f.height / 3);
        assert!(text.y > corner.y + 4.0, "at {t}: no text on the slate");
    }
    // The DJ put on air: their camera, at once.
    desk.set_on_air("moira", true);
    assert!(wait_shot(&desk, &person("moira"), Duration::from_secs(2)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_vision_mixer_follows_the_speaker_and_the_studios_override() {
    gst::init().unwrap();
    let dir = scratch("follow");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap();
    desk.add_person("callum", Role::Presenter).unwrap();
    desk.add_person("gregor", Role::Guest).unwrap();
    add_voice(&desk, "callum", 1000.0);
    add_camera(&desk, "callum", RED);
    add_camera(&desk, "gregor", BLUE);
    // Callum speaks, Gregor does not: Callum.
    assert!(wait_shot(&desk, &person("callum"), Duration::from_secs(5)));
    run_for(&desk, Duration::from_secs(3));
    assert_eq!(v.status().shot, person("callum"));
    // Callum off air: Gregor at once.
    desk.set_on_air("callum", false);
    assert!(wait_shot(
        &desk,
        &person("gregor"),
        Duration::from_millis(500)
    ));
    // F7: the slate, whoever is on air; then a camera held by hand, then the rule again.
    v.set_mode(PictureMode::Slate);
    assert!(wait_shot(&desk, &Shot::Slate, Duration::from_millis(500)));
    v.set_mode(PictureMode::Person {
        identity: "callum".into(),
    });
    assert!(wait_shot(
        &desk,
        &person("callum"),
        Duration::from_millis(500)
    ));
    run_for(&desk, Duration::from_secs(5));
    let pics = pictures(&fetch(&dir, "v0"));
    let last = &pics.last().unwrap().1;
    assert!(
        middle(last).distance(&yuv(RED)) < 12.0,
        "{:?}",
        middle(last)
    );
    v.set_mode(PictureMode::Auto);
    assert!(wait_shot(
        &desk,
        &person("gregor"),
        Duration::from_millis(500)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn picture_and_sound_leave_the_delay_together() {
    gst::init().unwrap();
    let dir = scratch("delay");
    let desk = Desk::new(config(&dir, 2_000, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    desk.add_person("moira", Role::Dj).unwrap(); // a DJ joins off air
    add_voice(&desk, "moira", 1000.0);
    add_camera(&desk, "moira", RED);
    run_for(&desk, Duration::from_secs(5));
    let delayed = desk.tap_delayed();
    let on_at = running_ns(&desk);
    desk.set_on_air("moira", true);
    run_for(&desk, Duration::from_secs(7));
    let d = delayed.samples();
    println!(
        "delayed programme: {} samples, tone in its last second {:.1} dB; delay {:?}",
        d.len(),
        ratio_db(tone_power(&d[d.len() - 48_000..], 1000.0), 1.0),
        desk.delay()
    );

    let pics = pictures(&fetch(&dir, "v0"));
    let first_red = pics
        .iter()
        .find(|(_, f)| middle(f).distance(&yuv(RED)) < 12.0)
        .map(|(t, _)| *t)
        .expect("the camera reached the output");
    let (start_ns, samples) = sound(&fetch(&dir, "audio"));
    // The first 10 ms window with the tone in it.
    let win = 480;
    // The level of each 10 ms (the only sound is the tone), in dB.
    let levels: Vec<f64> = samples
        .chunks(win)
        .map(|c| {
            let ms = c.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / c.len() as f64;
            10.0 * (ms + 1.0).log10()
        })
        .collect();
    let loudest = levels.iter().copied().fold(f64::MIN, f64::max);
    // The first window that begins a tenth of a second of the tone.
    let first_tone = (0..levels.len().saturating_sub(10))
        .find(|&i| levels[i..i + 10].iter().all(|d| *d > loudest - 10.0))
        .map(|i| start_ns + (i * win) as u64 * 1_000_000_000 / 48_000)
        .expect("the voice reached the output");
    let picture_after = first_red as i64 - on_at as i64;
    let sound_after = first_tone as i64 - on_at as i64;
    println!(
        "on air at {on_at} ns: picture {} ms later, sound {} ms later",
        picture_after / 1_000_000,
        sound_after / 1_000_000
    );
    // Both held by the delay's 2 s, and within two frames of each other.
    assert!(
        (1_800..=2_400).contains(&(picture_after / 1_000_000)),
        "picture after {picture_after} ns"
    );
    assert!(
        (1_800..=2_400).contains(&(sound_after / 1_000_000)),
        "sound after {sound_after} ns"
    );
    assert!(
        (picture_after - sound_after).abs() < 100_000_000,
        "picture and sound are {} ms apart",
        (picture_after - sound_after) / 1_000_000
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_video_failure_leaves_the_sound_on_air() {
    gst::init().unwrap();
    let dir = scratch("fail");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap().clone();
    desk.add_person("callum", Role::Presenter).unwrap();
    add_voice(&desk, "callum", 1000.0);
    add_camera(&desk, "callum", RED);
    run_for(&desk, Duration::from_secs(4));
    assert!(v.status().hls);
    let tap = desk.tap(&Bus::Programme).unwrap();
    let before = desk.mix_stats();
    assert!(v.fail_encoders(), "the encoders were not running");
    // The desk's own pipeline knows nothing of it, and the sound never stopped.
    run_for(&desk, Duration::from_secs(5));
    let after = desk.mix_stats();
    assert_eq!(
        after.late, before.late,
        "the mixing pass was late after the video failure"
    );
    let s = tap.samples();
    let windows: Vec<f64> = s
        .chunks(4_800)
        .filter(|c| c.len() == 4_800)
        .map(|c| ratio_db(tone_power(c, 1000.0), 1.0))
        .collect();
    assert!(windows.len() >= 45, "{} windows", windows.len());
    assert!(
        windows.iter().all(|d| *d > 60.0),
        "a gap in the programme: {windows:?}"
    );
    // And the video came back by itself.
    let back = Instant::now();
    while !v.status().hls && back.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(v.status().hls, "the encoders never started again");
    let n = v.frames_pushed();
    run_for(&desk, Duration::from_secs(1));
    assert!(v.frames_pushed() > n + 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lower_rungs_are_written_beside_the_default() {
    gst::init().unwrap();
    let dir = scratch("rungs");
    let mut c = config(&dir, 0, "");
    c.video.rungs = vec!["480p20".into(), "360p20".into(), "240p15".into()];
    let desk = Desk::new(c, Outputs::NONE).unwrap();
    let _stop = start(&desk);
    desk.add_person("callum", Role::Presenter).unwrap();
    add_camera(&desk, "callum", BLUE);
    run_for(&desk, Duration::from_secs(7));
    let live = std::fs::read_to_string(dir.join("live.m3u8")).unwrap();
    assert_eq!(live.matches("#EXT-X-STREAM-INF").count(), 3, "{live}");
    for (i, (w, h, fps)) in [(854, 480, 20), (640, 360, 20), (426, 240, 15)]
        .iter()
        .enumerate()
    {
        let pics = pictures(&fetch(&dir, &format!("v{i}")));
        let f = &pics.last().unwrap().1;
        assert_eq!((f.width, f.height), (*w, *h), "rung {i}");
        assert!(
            middle(f).distance(&yuv(BLUE)) < 14.0,
            "rung {i}: {:?}",
            middle(f)
        );
        let span = pics.last().unwrap().0 - pics[0].0;
        let rate = (pics.len() - 1) as f64 * 1e9 / span as f64;
        assert!(
            (rate - f64::from(*fps)).abs() < 0.5,
            "rung {i}: {rate:.2} fps"
        );
    }
    let status = desk.video().unwrap().status();
    println!("encoders: {:?}", status.rungs);
    if Encoder::nvenc_present() {
        assert!(status.rungs.iter().all(|r| r.encoder == Encoder::Nvenc));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_caption_from_the_studio_sits_over_any_shot() {
    gst::init().unwrap();
    let dir = scratch("caption");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap();
    // The slate, nobody on air: a featured chat message over it.
    v.set_caption(
        Some("Morag in Oban: the kettle's on for the whole glen"),
        20_000,
    );
    run_for(&desk, Duration::from_secs(6));
    let pics = pictures(&fetch(&dir, "v0"));
    let f = &pics.last().unwrap().1;
    let (w, h) = (f.width, f.height);
    let sh = (h / 6) & !1;
    let (x0, y0) = ((w / 16) & !1, h - h / 8 - sh);
    let stripe = f.mean(x0 + 1, y0 + 6, 3, sh - 12);
    assert!(
        stripe.distance(&yuv(ACCENT)) < 30.0,
        "no caption's accent at its left: {stripe:?}"
    );
    let text = f.mean(x0 + sh / 4, y0 + sh / 4, w / 2, sh / 2);
    assert!(
        text.y > yuv(SLATE_BG).y + 4.0,
        "no caption's text: {text:?}"
    );
    // Taken down: the slate's own picture there again.
    v.set_caption(None, 0);
    run_for(&desk, Duration::from_secs(4));
    let pics = pictures(&fetch(&dir, "v0"));
    let f = &pics.last().unwrap().1;
    let stripe = f.mean(x0 + 1, y0 + 6, 3, sh - 12);
    assert!(
        stripe.distance(&yuv(SLATE_BG)) < 10.0,
        "the caption stayed: {stripe:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A PNG of one colour, as the media worker writes them (ADR-0010).
fn write_png(path: &Path, w: u32, h: u32, rgb: (u8, u8, u8)) {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header().unwrap();
    let data: Vec<u8> = [rgb.0, rgb.1, rgb.2]
        .iter()
        .copied()
        .cycle()
        .take(3 * (w * h) as usize)
        .collect();
    wr.write_image_data(&data).unwrap();
    wr.finish().unwrap();
    // As the control plane writes it: all or nothing.
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, out).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

#[test]
fn the_slates_still_comes_changes_and_goes_while_the_desk_runs() {
    gst::init().unwrap();
    let dir = scratch("still");
    let still_file = dir.join("stills").join("slate.png");
    std::fs::create_dir_all(still_file.parent().unwrap()).unwrap();
    let mut cfg = config(&dir, 0, "");
    cfg.video.still_file = still_file.clone();
    let desk = Desk::new(cfg, Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap();
    v.set_now_playing(weavesignal_desk::video::NowPlaying {
        station: Some("Radio Kettle".into()),
        title: Some("Skye Boat Song".into()),
        ..Default::default()
    });
    // The top of the slate, above the words of a text-only slate and inside the still's room.
    let upper = |f: &Frame| f.mean(f.width / 2 - 60, 24, 120, 40);

    // No file: text only, the background over the upper part.
    run_for(&desk, Duration::from_secs(5));
    assert!(!v.status().still);
    let pics = pictures(&fetch(&dir, "v0"));
    let m = upper(&pics.last().unwrap().1);
    assert!(
        m.distance(&yuv(SLATE_BG)) < 10.0,
        "not the plain slate: {m:?}"
    );

    // A file appears: within a few seconds it is on the slate, and the words are under it.
    write_png(&still_file, 640, 360, RED);
    run_for(&desk, Duration::from_secs(7));
    assert!(v.status().still, "the desk never read the picture");
    let pics = pictures(&fetch(&dir, "v0"));
    let f = &pics.last().unwrap().1;
    assert!(
        upper(f).distance(&yuv(RED)) < 12.0,
        "no still: {:?}",
        upper(f)
    );
    let band = f.mean(
        f.width / 4,
        f.height * 7 / 9 + 4,
        f.width / 2,
        f.height / 9 - 8,
    );
    assert!(
        band.y > yuv(SLATE_BG).y + 3.0,
        "no words under the still: {band:?}"
    );

    // It changes: the new one is shown.
    write_png(&still_file, 400, 400, BLUE);
    run_for(&desk, Duration::from_secs(7));
    let pics = pictures(&fetch(&dir, "v0"));
    let m = upper(&pics.last().unwrap().1);
    assert!(
        m.distance(&yuv(BLUE)) < 12.0,
        "the new still is not shown: {m:?}"
    );

    // It goes: text only again.
    std::fs::remove_file(&still_file).unwrap();
    run_for(&desk, Duration::from_secs(7));
    assert!(!v.status().still);
    let pics = pictures(&fetch(&dir, "v0"));
    let m = upper(&pics.last().unwrap().1);
    assert!(m.distance(&yuv(SLATE_BG)) < 10.0, "the still stayed: {m:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A JPEG, decoded to I420.
fn decode_jpeg(jpeg: &[u8]) -> Frame {
    let p = gst::parse::launch(
        "appsrc name=in ! jpegparse ! jpegdec ! videoconvert ! video/x-raw,format=I420 ! appsink name=out sync=false",
    )
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    let src = p
        .by_name("in")
        .unwrap()
        .downcast::<gst_app::AppSrc>()
        .unwrap();
    let sink = p
        .by_name("out")
        .unwrap()
        .downcast::<gst_app::AppSink>()
        .unwrap();
    p.set_state(gst::State::Playing).unwrap();
    src.push_buffer(gst::Buffer::from_slice(jpeg.to_vec()))
        .unwrap();
    src.end_of_stream().unwrap();
    let s = sink
        .try_pull_sample(gst::ClockTime::from_seconds(5))
        .expect("a decoded picture");
    let caps = s.caps().unwrap().structure(0).unwrap().to_owned();
    let (w, h) = (
        caps.get::<i32>("width").unwrap() as usize,
        caps.get::<i32>("height").unwrap() as usize,
    );
    let map = s.buffer().unwrap().map_readable().unwrap();
    let f = Frame::from_bytes(w, h, map.as_slice()).expect("a whole picture");
    p.set_state(gst::State::Null).unwrap();
    f
}

#[test]
fn each_camera_gets_a_small_thumbnail_for_the_studio() {
    use base64::Engine;
    gst::init().unwrap();
    let dir = scratch("thumbs");
    let desk = Desk::new(config(&dir, 0, ""), Outputs::NONE).unwrap();
    let _stop = start(&desk);
    let v = desk.video().unwrap();
    // Nothing yet: no cameras, no thumbnails.
    assert!(v.thumbs().unwrap().is_empty());
    desk.add_person("callum", Role::Presenter).unwrap();
    desk.add_person("moira", Role::Dj).unwrap();
    add_camera(&desk, "callum", RED);
    add_camera(&desk, "moira", BLUE);
    run_for(&desk, Duration::from_secs(3));
    let thumbs = v.thumbs().unwrap();
    assert_eq!(
        thumbs.keys().map(String::as_str).collect::<Vec<_>>(),
        ["callum", "moira"]
    );
    let decode = |id: &str| {
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(&thumbs[id])
            .unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8], "not a JPEG");
        assert!(jpeg.len() < 20_000, "{} bytes is not small", jpeg.len());
        decode_jpeg(&jpeg)
    };
    let (a, b) = (decode("callum"), decode("moira"));
    assert_eq!((a.width, a.height), (192, 106));
    assert!(middle(&a).distance(&yuv(RED)) < 16.0, "{:?}", middle(&a));
    assert!(middle(&b).distance(&yuv(BLUE)) < 16.0, "{:?}", middle(&b));
    // Asked again at once, the same pictures (the desk does not encode for every ask).
    assert_eq!(v.thumbs().unwrap(), thumbs);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Playout's side of the feed back to the desk (ADR-0172), as `output.icecast` sends it: a
/// source request, a WAV header, then 16-bit stereo PCM in real time; here a tone.
struct FakePlayout {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakePlayout {
    fn start(port: u16, freq: f64) -> FakePlayout {
        let mut stream = None;
        let t0 = Instant::now();
        while stream.is_none() && t0.elapsed() < Duration::from_secs(5) {
            stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok();
            if stream.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let mut stream = stream.expect("the desk never listened for the feed");
        write!(
            stream,
            "SOURCE /feed HTTP/1.0\r\nAuthorization: {}\r\nContent-Type: audio/wav\r\n\r\n",
            basic_auth("source", "check-only")
        )
        .unwrap();
        let mut reply = [0u8; 12];
        stream.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"HTTP/1.0 200", "the desk refused the feed");
        stream.write_all(&wav_header(2)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            let mut sent = 0u64;
            while !flag.load(Ordering::Relaxed) {
                // 20 ms at a time, on time.
                let due = start + Duration::from_millis(sent / 48);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
                let mut bytes = Vec::with_capacity(960 * 4);
                for i in 0..960u64 {
                    let t = (sent + i) as f64 / 48_000.0;
                    let v = ((t * freq * std::f64::consts::TAU).sin() * 0.3 * 32767.0) as i16;
                    bytes.extend(v.to_le_bytes());
                    bytes.extend(v.to_le_bytes());
                }
                if stream.write_all(&bytes).is_err() {
                    break;
                }
                sent += 960;
            }
        });
        FakePlayout {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for FakePlayout {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Run until the video's sound is as `want` says, for at most `limit`; whether it got there.
fn wait_sound(
    desk: &Desk,
    limit: Duration,
    want: impl Fn(&weavesignal_desk::pipeline::HlsSoundStatus) -> bool,
) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if want(&desk.hls_sound()) {
            return true;
        }
        run_for(desk, Duration::from_millis(50));
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

#[test]
fn the_video_sound_is_the_desk_while_someone_is_live_and_playout_otherwise() {
    // ADR-0172: playout's output comes back to the desk; the HLS sound is the desk's delayed
    // programme while a strip is open on air, and playout's otherwise. A DJ (not on air when
    // they join) speaks at 1 kHz; playout plays 3 kHz.
    gst::init().unwrap();
    let dir = scratch("feed");
    let mut c = config(&dir, 1_000, "");
    c.video.playout_feed.enabled = true;
    c.video.playout_feed.port = 22_717;
    c.video.playout_feed.crossfade_ms = 100;
    c.video.playout_feed.off_air_hold_ms = 500;
    let desk = Desk::new(c, Outputs::NONE).unwrap();
    let hls = desk.tap_hls_sound();
    let _stop = start(&desk);
    let t0 = Instant::now();
    desk.add_person("callum", Role::Dj).unwrap();
    add_voice(&desk, "callum", 1000.0);

    // Playout away: the desk's own programme, from the first moment.
    run_for(&desk, Duration::from_millis(1500));
    let away = desk.hls_sound();
    assert_eq!(away.source, Source::Desk);
    assert!(!away.feed_connected);
    assert!(
        hls.seconds() > 1.2,
        "the video's sound waited for playout: {:.2} s in 1.5 s",
        hls.seconds()
    );

    // Playout connects; nobody is live: playout's sound.
    let playout = FakePlayout::start(22_717, 3000.0);
    assert!(
        wait_sound(&desk, Duration::from_secs(5), |s| s.source
            == Source::Playout),
        "playout's sound never came: {:?}",
        desk.hls_sound()
    );
    let on_feed = Instant::now() + Duration::from_millis(200);
    run_for(&desk, Duration::from_millis(1000));
    let fed = desk.hls_sound();
    let (feed_3k, feed_1k) = (
        level_since(&hls, on_feed, 3000.0),
        level_since(&hls, on_feed, 1000.0),
    );
    assert!(fed.feed_connected && fed.feed_connections == 1, "{fed:?}");
    assert_eq!(fed.source, Source::Playout);
    let delay = desk.delay().state;
    assert!(!delay.rebuilding, "the delay is not full yet: {delay:?}");

    // The DJ opens their strip: live for viewers once the delay (1 s) has played out.
    assert!(desk.set_on_air("callum", true));
    run_for(&desk, Duration::from_millis(600));
    let early = desk.hls_sound();
    wait_sound(&desk, Duration::from_secs(5), |s| s.source == Source::Desk);
    let on_desk = Instant::now() + Duration::from_millis(200);
    run_for(&desk, Duration::from_millis(700));
    let live = desk.hls_sound();
    let (live_1k, live_3k) = (
        level_since(&hls, on_desk, 1000.0),
        level_since(&hls, on_desk, 3000.0),
    );

    // Off air: the desk for the delay and the hold (1.5 s), then playout.
    assert!(desk.set_on_air("callum", false));
    run_for(&desk, Duration::from_millis(1200));
    let held = desk.hls_sound();
    wait_sound(&desk, Duration::from_secs(5), |s| {
        s.source == Source::Playout
    });
    let back = Instant::now() + Duration::from_millis(200);
    run_for(&desk, Duration::from_millis(600));
    let after = desk.hls_sound();
    let back_3k = level_since(&hls, back, 3000.0);

    // Playout stops: the desk's programme again, and no gap.
    drop(playout);
    wait_sound(&desk, Duration::from_secs(5), |s| {
        s.source == Source::Desk && !s.feed_connected
    });
    let gone_at = Instant::now() + Duration::from_millis(200);
    run_for(&desk, Duration::from_millis(600));
    let gone = desk.hls_sound();
    let gone_3k = level_since(&hls, gone_at, 3000.0);
    let (secs, elapsed) = (hls.seconds(), t0.elapsed().as_secs_f64());

    println!(
        "feed: 3 kHz {feed_3k:.0} dB, 1 kHz {feed_1k:.0} dB; 0.6 s after on air {early:?}; \
         live: 1 kHz {live_1k:.0} dB, 3 kHz {live_3k:.0} dB ({live:?}); 1.2 s after off air \
         {held:?}; back on playout 3 kHz {back_3k:.0} dB ({after:?}); playout gone {gone:?}, \
         3 kHz {gone_3k:.0} dB; {secs:.1} s of sound in {elapsed:.1} s"
    );
    assert!(
        feed_3k > 60.0 && feed_1k < 30.0,
        "nobody live: not playout's sound"
    );
    assert_eq!(
        early.source,
        Source::Playout,
        "live before the delay played out"
    );
    assert_eq!(live.source, Source::Desk);
    assert!(live.live);
    // A window's leakage from a loud tone is not a sound: 60 dB under it is absent.
    assert!(
        live_1k > 60.0 && live_3k < live_1k - 60.0,
        "live: not the desk's sound"
    );
    assert_eq!(
        held.source,
        Source::Desk,
        "playout before the hold was over"
    );
    assert_eq!(after.source, Source::Playout);
    assert!(back_3k > 60.0, "off air: playout's sound did not come back");
    assert_eq!(gone.source, Source::Desk);
    assert!(!gone.feed_connected);
    assert!(gone_3k < 30.0, "playout gone, its sound still there");
    assert!(
        secs > elapsed - 0.5,
        "the video's sound had a gap: {secs:.1} s in {elapsed:.1} s"
    );
    // Playout starts again: it connects afresh, with a header of its own, and is heard.
    let _again = FakePlayout::start(22_717, 3000.0);
    wait_sound(&desk, Duration::from_secs(5), |s| {
        s.source == Source::Playout && s.feed_connections == 2
    });
    let again = desk.hls_sound();
    println!("playout started again: {again:?}");
    assert!(
        again.feed_connected && again.feed_connections == 2,
        "{again:?}"
    );
    assert_eq!(again.source, Source::Playout);

    // The station's mode, over the control socket (ADR-0183): in auto mode automation holds
    // the air, so viewers hear playout with the DJ's strip open; back in live mode, the desk.
    use weavesignal_desk::control::handle;
    let ask = |line: serde_json::Value| -> serde_json::Value {
        serde_json::to_value(handle(&desk, &line.to_string())).unwrap()
    };
    assert_eq!(ask(serde_json::json!({"op": "status"}))["mode"], "live");
    assert!(desk.set_on_air("callum", true));
    assert!(
        wait_sound(&desk, Duration::from_secs(5), |s| s.source == Source::Desk
            && s.live),
        "live again: {:?}",
        desk.hls_sound()
    );
    let flipped = Instant::now();
    assert_eq!(
        ask(serde_json::json!({"op": "mode", "mode": "auto"}))["ok"],
        true
    );
    assert!(
        wait_sound(&desk, Duration::from_secs(5), |s| s.source
            == Source::Playout),
        "auto mode: still the desk {:?}",
        desk.hls_sound()
    );
    let to_auto = flipped.elapsed();
    let auto = desk.hls_sound();
    let st = ask(serde_json::json!({"op": "status"}));
    assert_eq!(st["mode"], "auto");
    assert_eq!(st["sound"]["source"], "playout");
    let r = ask(serde_json::json!({"op": "mode", "mode": "automation"}));
    assert_eq!(
        (r["ok"].as_bool(), r["code"].as_str()),
        (Some(false), Some("invalid"))
    );
    let flipped = Instant::now();
    assert_eq!(
        ask(serde_json::json!({"op": "mode", "mode": "live"}))["ok"],
        true
    );
    assert!(
        wait_sound(&desk, Duration::from_secs(5), |s| s.source == Source::Desk),
        "live mode: still playout {:?}",
        desk.hls_sound()
    );
    let to_live = flipped.elapsed();
    println!("mode: to auto in {to_auto:?} ({auto:?}), back to the desk in {to_live:?}");
    assert!(auto.live, "the DJ is still live at the desk");
    // No delay and no hold either way: the crossfade (100 ms) and a poll or two.
    assert!(to_auto < Duration::from_millis(600), "{to_auto:?}");
    assert!(to_live < Duration::from_millis(600), "{to_live:?}");
    assert!(desk.set_on_air("callum", false));

    // Playout's sound reached the HLS audio rendition itself.
    let (_, samples) = sound(&fetch(&dir, "audio"));
    let heard = ratio_db(tone_power(&samples, 3000.0), 1.0);
    assert!(
        heard > 60.0,
        "playout's sound is not in the HLS audio: {heard:.0} dB"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
