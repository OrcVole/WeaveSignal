// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What RTMP out's tests share (`rtmp.rs`, `rtmp_stack.rs`): a desk with video and tone and
//! camera sources, and readers for what came out.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use weavesignal_desk::analysis::{ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::picture::{Frame, Yuv};
use weavesignal_desk::pipeline::{Desk, RAW_CAPS};
use weavesignal_desk::rtmp::{Destination, Profile};

pub const RED: (u8, u8, u8) = (220, 40, 40);
pub const BLUE: (u8, u8, u8) = (40, 60, 220);

pub fn scratch(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "weavesignal-rtmp-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn config(dir: &Path, delay_ms: u32, extra: &str) -> Config {
    Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "local"

[programme]
srt_port = 26998

[programme.delay]
target_ms = {delay_ms}

[inputs.presenter]
[inputs.dj]

[video]
enabled = true
lower_third_ms = 0

[video.hls]
dir = "{}"

[video.rtmp]
retry_min_s = 2
retry_max_s = 8
{extra}
"#,
            dir.join("hls").display()
        ),
        "test",
    )
    .map(|mut c| {
        c.video.playout_feed.enabled = false;
        c
    })
    .unwrap()
}

pub fn dest(name: &str, url: &str, key: &str, profile: Profile) -> Destination {
    Destination {
        name: name.into(),
        url: url.into(),
        key: key.into(),
        profile,
        enabled: true,
    }
}

pub fn source(
    pipeline: &gst::Pipeline,
    name: &str,
    video: Option<(u8, u8, u8)>,
    freq: f64,
) -> gst::Pad {
    let bin = gst::Bin::with_name(name);
    let (src, caps, pad) = match video {
        Some(rgb) => (
            gst::ElementFactory::make("videotestsrc")
                .property("is-live", true)
                .property_from_str("pattern", "solid-color")
                .property(
                    "foreground-color",
                    0xff00_0000u32
                        | (u32::from(rgb.0) << 16)
                        | (u32::from(rgb.1) << 8)
                        | u32::from(rgb.2),
                )
                .build()
                .unwrap(),
            "video/x-raw,format=I420,width=640,height=480,framerate=20/1",
            "video_0",
        ),
        None => (
            gst::ElementFactory::make("audiotestsrc")
                .property("is-live", true)
                .property("freq", freq)
                .property("volume", 0.3)
                .property("samplesperbuffer", 480i32)
                .build()
                .unwrap(),
            RAW_CAPS,
            "audio_0",
        ),
    };
    let cf = gst::ElementFactory::make("capsfilter")
        .property("caps", caps.parse::<gst::Caps>().unwrap())
        .build()
        .unwrap();
    bin.add_many([&src, &cf]).unwrap();
    src.link(&cf).unwrap();
    let ghost = gst::GhostPad::builder_with_target(&cf.static_pad("src").unwrap())
        .unwrap()
        .name(pad)
        .build();
    bin.add_pad(&ghost).unwrap();
    pipeline.add(&bin).unwrap();
    ghost.upcast()
}

pub fn add(desk: &Desk, id: &str, rgb: (u8, u8, u8), freq: f64) {
    let a = source(desk.pipeline(), &format!("{id}-voice"), None, freq);
    desk.route_pad(id, &a).unwrap();
    a.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
    let v = source(desk.pipeline(), &format!("{id}-cam"), Some(rgb), 0.0);
    desk.route_camera(id, &v).unwrap();
    v.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
}

pub struct Stop(pub Desk);

impl Drop for Stop {
    fn drop(&mut self) {
        if let Some(v) = self.0.video() {
            v.stop();
        }
        let _ = self.0.pipeline().set_state(gst::State::Null);
    }
}

pub fn start(desk: &Desk) -> Stop {
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

pub fn run_for(desk: &Desk, d: Duration) {
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

/// `ffprobe`'s view of an FLV file: each stream's codec and its frames, read through.
pub fn probe(file: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_entries",
            "stream=codec_type,codec_name,profile,width,height,r_frame_rate,sample_rate,channels,nb_read_frames",
            "-of",
            "compact",
        ])
        .arg(file)
        .output()
        .expect("ffprobe runs (ffmpeg-free is in the desk image)");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    println!(
        "{}: {text}{}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

/// The sound of an FLV file as mono 16-bit at 48 kHz, through FFmpeg.
pub fn flv_sound(file: &Path) -> Vec<i16> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "s16le", "-"])
        .output()
        .expect("ffmpeg runs");
    out.stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

/// Every picture of a `compatible` file, decoded by GStreamer (flvdemux, OpenH264).
pub fn flv_pictures(file: &Path) -> Vec<Frame> {
    let p = gst::parse::launch(&format!(
        "filesrc location={} ! flvdemux ! h264parse ! openh264dec ! videoconvert ! video/x-raw,format=I420 ! appsink name=out sync=false",
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
        let map = s.buffer().unwrap().map_readable().unwrap();
        out.push(Frame::from_bytes(w, h, map.as_slice()).expect("a whole picture"));
    }
    p.set_state(gst::State::Null).unwrap();
    out
}

pub fn middle(f: &Frame) -> Yuv {
    f.mean(f.width / 3, f.height / 3, f.width / 3, f.height / 3)
}

pub fn per_second(s: &[i16], f: f64) -> Vec<f64> {
    s.chunks(48_000)
        .filter(|c| c.len() == 48_000)
        .map(|c| ratio_db(tone_power(c, f), 1.0))
        .collect()
}

pub fn size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// A camera with a moving picture (SMPTE bars sliding, with a ball), 1280x720 at 30 fps: an
/// encoder's honest work, unlike a solid colour.
pub fn add_moving(desk: &Desk, id: &str, freq: f64) {
    let a = source(desk.pipeline(), &format!("{id}-voice"), None, freq);
    desk.route_pad(id, &a).unwrap();
    a.parent_element()
        .unwrap()
        .sync_state_with_parent()
        .unwrap();
    let bin = gst::Bin::with_name(&format!("{id}-cam"));
    let src = gst::ElementFactory::make("videotestsrc")
        .property("is-live", true)
        .property_from_str("pattern", "smpte")
        .property("horizontal-speed", 6i32)
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property(
            "caps",
            "video/x-raw,format=I420,width=1280,height=720,framerate=30/1"
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
    desk.pipeline().add(&bin).unwrap();
    let pad: gst::Pad = ghost.upcast();
    desk.route_camera(id, &pad).unwrap();
    bin.sync_state_with_parent().unwrap();
}

/// This process's CPU time so far, in seconds (user and system, from `/proc/self/stat`).
pub fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after = &stat[stat.rfind(')').unwrap() + 2..];
    let f: Vec<&str> = after.split_whitespace().collect();
    let ticks: f64 = f[11].parse::<f64>().unwrap() + f[12].parse::<f64>().unwrap();
    ticks / 100.0
}

/// The share of one core this process used over `d`, as a percentage.
pub fn cpu_over(desk: &Desk, d: Duration) -> f64 {
    let (c0, t0) = (cpu_seconds(), Instant::now());
    run_for(desk, d);
    (cpu_seconds() - c0) / t0.elapsed().as_secs_f64() * 100.0
}
