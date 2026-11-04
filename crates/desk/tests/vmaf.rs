// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The picture's quality at each rung (ADR-0027), measured as bench 01 measured it: the desk's
//! own encoder settings and CMAF writer, then the segments decoded to raw frames (never scored
//! as a container: bench 01's trap) and scored with VMAF against the same frames before
//! encoding. Ignored: it needs Xiph's talking-head clips in `target/clips/` (bench 01's
//! `KristenAndSara_1280x720_60.y4m` and `FourPeople_1280x720_60.y4m`) and FFmpeg with libvmaf
//! (the desk image has both tools). `just test-vmaf`; with `WEAVESIGNAL_DESK_GPU=1`, NVENC too.

use std::path::{Path, PathBuf};
use std::process::Command;

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::config::Rung;
use weavesignal_desk::video::{Encoder, encoder_chain};

fn ffmpeg(args: &[&str]) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .expect("ffmpeg runs (the desk image has it)");
    assert!(
        out.status.success(),
        "ffmpeg {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The desk's encode of `canvas_yuv` (854x480 at 20 fps, Y4M) for `rung`, written as
/// CMAF HLS into `dir`; the segments' bytes.
fn encode(canvas_yuv: &Path, rung: &Rung, enc: Encoder, dir: &Path) -> u64 {
    let canvas = Rung::PRESETS[0];
    std::fs::create_dir_all(dir).unwrap();
    let p = gst::Pipeline::new();
    let src = gst::ElementFactory::make("filesrc")
        .property("location", canvas_yuv.to_str().unwrap())
        .build()
        .unwrap();
    // Y4M, not raw: GStreamer pads an 854-pixel row to 856, FFmpeg's raw frames do not.
    let parse = gst::ElementFactory::make("y4mdec").build().unwrap();
    let mut chain = vec![src, parse];
    chain.extend(encoder_chain(rung, &canvas, enc, 2).unwrap());
    // The leaky queue at the head of the chain is for live use; here every frame counts.
    chain[2].set_property_from_str("leaky", "no");
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    chain.push(
        gst::ElementFactory::make("hlscmafsink")
            .property("sync", false)
            .property("target-duration", 2u32)
            .property("playlist-length", 0u32)
            .property("max-files", 0u32)
            .property("location", at("seg%05d.m4s"))
            .property("init-location", at("init%05d.mp4"))
            .property("playlist-location", at("index.m3u8"))
            .build()
            .unwrap(),
    );
    p.add_many(&chain).unwrap();
    gst::Element::link_many(&chain).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let msg = p
        .bus()
        .unwrap()
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(300),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the encode finished");
    p.set_state(gst::State::Null).unwrap();
    assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "m4s"))
        .map(|e| e.metadata().unwrap().len())
        .sum()
}

/// The playlist's init section and segments joined, as a player fetches them.
fn join(dir: &Path) -> PathBuf {
    let playlist = std::fs::read_to_string(dir.join("index.m3u8")).unwrap();
    let mut bytes = Vec::new();
    for line in playlist.lines() {
        if let Some(init) = line.strip_prefix("#EXT-X-MAP:URI=\"") {
            bytes.extend(std::fs::read(dir.join(init.trim_end_matches('"'))).unwrap());
        } else if !line.starts_with('#') && !line.is_empty() {
            bytes.extend(std::fs::read(dir.join(line)).unwrap());
        }
    }
    let out = dir.join("joined.mp4");
    std::fs::write(&out, bytes).unwrap();
    out
}

/// The frames the encoder sees for `rung`, before encoding, as Y4M.
fn reference_for(canvas_y4m: &Path, r: &Rung, out: &Path) {
    let line = format!(
        "filesrc location={} ! y4mdec ! videoscale ! video/x-raw,width={},height={} ! videorate drop-only=true ! video/x-raw,framerate={}/1 ! videoconvert ! video/x-raw,format=I420 ! y4menc ! filesink location={}",
        canvas_y4m.display(),
        r.width,
        r.height,
        r.fps,
        out.display()
    );
    let p = gst::parse::launch(&line).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let msg = p
        .bus()
        .unwrap()
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(120),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .unwrap();
    p.set_state(gst::State::Null).unwrap();
    assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
}

fn vmaf(dist: &Path, reference: &Path, r: &Rung, log: &Path) -> f64 {
    let size = format!("{}x{}", r.width, r.height);
    let fps = r.fps.to_string();
    let raw = |p: &Path| -> Vec<String> {
        [
            "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", &size, "-r", &fps, "-i",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([p.to_string_lossy().into_owned()])
        .collect()
    };
    let mut args = raw(dist);
    args.extend(["-i".to_owned(), reference.to_string_lossy().into_owned()]);
    args.extend(
        [
            "-lavfi",
            &format!(
                "[0:v][1:v]libvmaf=log_fmt=json:log_path={}:n_threads=8",
                log.display()
            ),
            "-f",
            "null",
            "-",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    ffmpeg(&refs);
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(log).unwrap()).unwrap();
    v["pooled_metrics"]["vmaf"]["mean"].as_f64().unwrap()
}

#[test]
#[ignore = "needs Xiph's clips in target/clips and FFmpeg with libvmaf: just test-vmaf"]
fn vmaf_at_each_rung() {
    gst::init().unwrap();
    let clips = PathBuf::from(
        std::env::var("WEAVESIGNAL_CLIPS").unwrap_or_else(|_| "target/clips".to_owned()),
    );
    let out = std::env::temp_dir().join(format!("weavesignal-vmaf-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let mut encoders = vec![Encoder::Svt];
    if Encoder::nvenc_present() {
        encoders.insert(0, Encoder::Nvenc);
    }
    let canvas = Rung::PRESETS[0];
    let mut rows = Vec::new();
    for clip in ["KristenAndSara", "FourPeople"] {
        let src = clips.join(format!("{clip}_1280x720_60.y4m"));
        assert!(
            src.is_file(),
            "{} is missing (see this test's notes)",
            src.display()
        );
        // What the vision mixer hands the encoders: the camera at the canvas's size and rate.
        let canvas_yuv = out.join(format!("{clip}-canvas.y4m"));
        ffmpeg(&[
            "-i",
            src.to_str().unwrap(),
            "-vf",
            &format!(
                "fps={},scale={}:{}:flags=lanczos",
                canvas.fps, canvas.width, canvas.height
            ),
            "-pix_fmt",
            "yuv420p",
            canvas_yuv.to_str().unwrap(),
        ]);
        for r in Rung::PRESETS {
            // The reference at the rung's own size and rate, made from the same frames by the
            // same scaler and frame-rate converter as the encode, so the frames line up.
            let reference = out.join(format!("{clip}-{}-ref.y4m", r.name));
            reference_for(&canvas_yuv, &r, &reference);
            for &enc in &encoders {
                let dir = out.join(format!("{clip}-{}-{enc:?}", r.name));
                let bytes = encode(&canvas_yuv, &r, enc, &dir);
                // Decoded to raw frames first, then scored.
                let dist = dir.join("decoded.yuv");
                ffmpeg(&[
                    "-i",
                    join(&dir).to_str().unwrap(),
                    // Every decoded frame once: no frames repeated to fill a rate.
                    "-fps_mode",
                    "passthrough",
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "rawvideo",
                    dist.to_str().unwrap(),
                ]);
                let frame = (r.width * r.height * 3 / 2) as u64;
                // Y4M: a 44-ish byte header, and "FRAME\n" before each frame.
                let frames_ref = std::fs::metadata(&reference).unwrap().len() / (frame + 6);
                let frames_dist = std::fs::metadata(&dist).unwrap().len() / frame;
                let score = vmaf(&dist, &reference, &r, &dir.join("vmaf.json"));
                let seconds = frames_ref as f64 / f64::from(r.fps);
                let kbps = bytes as f64 * 8.0 / seconds / 1000.0;
                let row = serde_json::json!({
                    "clip": clip, "rung": r.name, "encoder": enc, "target_kbps": r.kbps,
                    "kbps": (kbps * 10.0).round() / 10.0, "vmaf": (score * 100.0).round() / 100.0,
                    "frames": frames_dist, "frames_equal": frames_ref == frames_dist,
                });
                println!("{row}");
                rows.push(row);
            }
        }
    }
    let report = serde_json::json!({ "rows": rows });
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }
    let _ = std::fs::remove_dir_all(&out);
    // The default rung keeps bench 01's quality: NVENC's 94.3 to 96.4 at 480p20, 600 kbit/s;
    // SVT-AV1 a couple of points behind.
    for row in &rows {
        if row["rung"] == "480p20" {
            let floor = if row["encoder"] == "nvenc" {
                93.0
            } else {
                90.0
            };
            let v = row["vmaf"].as_f64().unwrap();
            assert!(v >= floor, "{row}");
        }
    }
}
