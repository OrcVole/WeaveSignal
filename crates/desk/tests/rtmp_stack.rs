// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out against real servers on loopback (ADR-0230), never a real platform: MediaMTX
//! (both profiles, read back over RTSP; a refused key; the server going away; a dump),
//! OvenMediaEngine and PeerTube (both profiles; what they kept is read by the script from
//! their own logs and API). Ignored unless the servers are up: `just test-rtmp`
//! (`tests/rtmp/run.sh`). `rtmp_cost` measures the extra encode's CPU and needs no server.

mod rtmp_support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use weavesignal_desk::config::Config;
use weavesignal_desk::picture::{Frame, Yuv};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::rtmp::{Destination, Profile, SendState};
use weavesignal_proto::Role;

use rtmp_support::*;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run `just test-rtmp`"))
}

fn run_dir() -> PathBuf {
    PathBuf::from(env("WEAVESIGNAL_RTMP_RUN"))
}

/// `ffprobe` of a stream as a server serves it back.
fn probe_url(url: &str) -> String {
    let mut args = vec!["-v", "error", "-rw_timeout", "8000000"];
    if url.starts_with("rtsp://") {
        args.extend(["-rtsp_transport", "tcp"]);
    }
    let out = Command::new("ffprobe")
        .args(&args)
        .args([
            "-show_entries",
            "stream=codec_type,codec_name,profile,width,height,r_frame_rate,sample_rate,channels",
            "-of",
            "compact",
            url,
        ])
        .output()
        .expect("ffprobe runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    println!("{url}: {text}{}", String::from_utf8_lossy(&out.stderr));
    text
}

/// Record what a server serves back, as it comes, to a Matroska file.
fn record(url: &str, file: &Path, seconds: u32) -> Child {
    Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-rtsp_transport",
            "tcp",
            "-i",
            url,
            "-t",
        ])
        .arg(seconds.to_string())
        .args(["-c", "copy"])
        .arg(file)
        .stdin(Stdio::null())
        .spawn()
        .expect("ffmpeg runs")
}

fn mkv_pictures(file: &Path) -> Vec<Frame> {
    let p = gst::parse::launch(&format!(
        "filesrc location={} ! matroskademux ! h264parse ! openh264dec ! videoconvert ! video/x-raw,format=I420 ! appsink name=out sync=false",
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

fn stack_desk(name: &str, delay_ms: u32, extra: &str) -> (Desk, Stop, PathBuf) {
    gst::init().unwrap();
    let dir = scratch(name);
    let desk = Desk::new(config(&dir, delay_ms, extra), Outputs::NONE).unwrap();
    let stop = start(&desk);
    (desk, stop, dir)
}

fn d(name: &str, url: &str, key: &str, profile: Profile) -> Destination {
    dest(name, url, key, profile)
}

fn wait_live(desk: &Desk, names: &[&str], limit: Duration) {
    let t0 = Instant::now();
    loop {
        let st = desk.video().unwrap().status().rtmp;
        let live = names.iter().all(|n| {
            st.destinations
                .iter()
                .any(|x| x.name == *n && x.state == SendState::Live && x.bytes > 0)
        });
        if live {
            return;
        }
        assert!(
            t0.elapsed() < limit,
            "not live in {limit:?}: {}",
            serde_json::to_string(&st).unwrap()
        );
        run_for(desk, Duration::from_millis(200));
    }
}

#[test]
#[ignore = "needs MediaMTX on loopback: just test-rtmp"]
fn mediamtx_takes_both_profiles() {
    let rtmp = env("WEAVESIGNAL_RTMP_MEDIAMTX");
    let rtsp = env("WEAVESIGNAL_RTMP_RTSP");
    let (desk, _stop, _dir) = stack_desk("mtx-both", 0, "");
    let v = desk.video().unwrap().clone();
    desk.add_person("callum", Role::Presenter).unwrap();
    add(&desk, "callum", RED, 1000.0);
    v.rtmp()
        .set_destinations(
            "s1",
            vec![
                d("mtx-compatible", &rtmp, "compat", Profile::Compatible),
                d("mtx-open", &rtmp, "open", Profile::Open),
            ],
        )
        .unwrap();
    wait_live(
        &desk,
        &["mtx-compatible", "mtx-open"],
        Duration::from_secs(20),
    );
    run_for(&desk, Duration::from_secs(4));
    let c = probe_url(&format!("{rtsp}/compat"));
    assert!(
        c.contains("codec_name=h264") && c.contains("width=854"),
        "{c}"
    );
    assert!(
        c.contains("codec_name=aac") && c.contains("sample_rate=44100"),
        "{c}"
    );
    let o = probe_url(&format!("{rtsp}/open"));
    assert!(
        o.contains("codec_name=av1") && o.contains("width=854"),
        "{o}"
    );
    assert!(
        o.contains("codec_name=opus") && o.contains("sample_rate=48000"),
        "{o}"
    );
    println!(
        "status: {}",
        serde_json::to_string(&v.status().rtmp).unwrap()
    );
}

#[test]
#[ignore = "needs MediaMTX on loopback: just test-rtmp"]
fn a_refused_key_is_tried_again_and_a_server_that_goes_away_is_found_again() {
    let rtmp = env("WEAVESIGNAL_RTMP_MEDIAMTX");
    let (desk, _stop, _dir) = stack_desk("mtx-fail", 0, "");
    let v = desk.video().unwrap().clone();
    desk.add_person("callum", Role::Presenter).unwrap();
    add(&desk, "callum", RED, 1000.0);
    v.rtmp()
        .set_destinations(
            "s2",
            vec![
                d("good", &rtmp, "compat2", Profile::Compatible),
                d(
                    "refused",
                    &rtmp,
                    "wrong-key-do-not-print",
                    Profile::Compatible,
                ),
            ],
        )
        .unwrap();
    wait_live(&desk, &["good"], Duration::from_secs(20));
    run_for(&desk, Duration::from_secs(12));
    let st = v.status().rtmp;
    println!("after 12 s: {}", serde_json::to_string(&st).unwrap());
    let refused = st
        .destinations
        .iter()
        .find(|x| x.name == "refused")
        .unwrap();
    let good = st.destinations.iter().find(|x| x.name == "good").unwrap();
    assert!(
        refused.connects >= 3,
        "the refused key was tried {} times",
        refused.connects
    );
    assert_ne!(refused.state, SendState::Live);
    let e = refused.last_error.clone().expect("the refusal is reported");
    assert!(
        !e.contains("wrong-key-do-not-print"),
        "the key is in the error: {e}"
    );
    assert_eq!(good.connects, 1, "the good destination was disturbed");
    assert_eq!(good.state, SendState::Live);
    assert!(v.status().hls);

    // The server goes away for 5 s.
    let run = run_dir();
    let _ = std::fs::remove_file(run.join("mediamtx-restarted"));
    std::fs::write(run.join("restart-mediamtx"), b"").unwrap();
    let t0 = Instant::now();
    while !run.join("mediamtx-restarted").exists() {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the script never restarted MediaMTX"
        );
        run_for(&desk, Duration::from_millis(500));
    }
    let gone = v.status().rtmp;
    let good = gone.destinations.iter().find(|x| x.name == "good").unwrap();
    println!("server back: {}", serde_json::to_string(good).unwrap());
    assert!(good.last_error.is_some(), "the loss was not noticed");
    let back = Instant::now();
    loop {
        let st = v.status().rtmp;
        let g = st.destinations.iter().find(|x| x.name == "good").unwrap();
        if g.state == SendState::Live && g.bytes > 0 && g.connects >= 2 {
            println!(
                "connected again {:.1} s after the server came back ({} connections)",
                back.elapsed().as_secs_f64(),
                g.connects
            );
            break;
        }
        assert!(
            back.elapsed() < Duration::from_secs(70),
            "never connected again: {}",
            serde_json::to_string(g).unwrap()
        );
        run_for(&desk, Duration::from_millis(250));
    }
    assert!(v.status().hls, "HLS stopped");
}

#[test]
#[ignore = "needs MediaMTX on loopback: just test-rtmp"]
fn the_platform_never_gets_what_was_dumped() {
    let rtmp = env("WEAVESIGNAL_RTMP_MEDIAMTX");
    let rtsp = env("WEAVESIGNAL_RTMP_RTSP");
    let (desk, _stop, dir) = stack_desk("mtx-dump", 4_000, "");
    let v = desk.video().unwrap().clone();
    desk.add_person("moira", Role::Dj).unwrap();
    desk.add_person("gregor", Role::Presenter).unwrap();
    add(&desk, "moira", RED, 1000.0);
    add(&desk, "gregor", BLUE, 3000.0);
    desk.set_on_air("gregor", false);
    v.rtmp()
        .set_destinations("s3", vec![d("dump", &rtmp, "dump", Profile::Compatible)])
        .unwrap();
    let t0 = Instant::now();
    while desk.delay().state.rebuilding && t0.elapsed() < Duration::from_secs(30) {
        run_for(&desk, Duration::from_millis(200));
    }
    wait_live(&desk, &["dump"], Duration::from_secs(20));
    run_for(&desk, Duration::from_secs(2));
    let file = dir.join("dump.mkv");
    let mut rec = record(&format!("{rtsp}/dump"), &file, 22);
    run_for(&desk, Duration::from_secs(2));
    desk.set_on_air("moira", true);
    run_for(&desk, Duration::from_millis(1_500));
    desk.set_on_air("moira", false);
    run_for(&desk, Duration::from_millis(500));
    assert!(desk.dump(), "nothing was held to dump");
    desk.set_on_air("gregor", true);
    run_for(&desk, Duration::from_secs(16));
    let _ = rec.wait();
    let s = {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&file)
            .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "s16le", "-"])
            .output()
            .unwrap();
        out.stdout
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect::<Vec<i16>>()
    };
    let marker = per_second(&s, 1000.0);
    let after: Vec<f64> = per_second(&s, 2940.0)
        .into_iter()
        .zip(per_second(&s, 3000.0))
        .map(|(a, b)| a.max(b))
        .collect();
    println!("read back from MediaMTX: marker {marker:.1?}\nafter {after:.1?}");
    assert!(marker.len() >= 15, "only {} s read back", marker.len());
    assert!(
        marker.iter().all(|d| *d < 90.0),
        "the dumped marker reached the server: {marker:?}"
    );
    assert!(
        after.iter().rev().take(4).all(|d| *d > 55.0),
        "the voice after the dump is missing"
    );
    let pics = mkv_pictures(&file);
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
        "{} pictures read back: {reds} of the dumped camera, {blues} after",
        pics.len()
    );
    assert_eq!(reds, 0, "the dumped picture reached the server");
    assert!(blues > 40);
}

/// Both profiles to the servers the script names (OvenMediaEngine, PeerTube); what each kept is
/// read by the script afterwards. `WEAVESIGNAL_RTMP_TARGETS` is `name=profile=url=key`, one per
/// line.
#[test]
#[ignore = "needs the servers the script starts: just test-rtmp"]
fn send_to_the_named_servers() {
    let targets = env("WEAVESIGNAL_RTMP_TARGETS");
    let seconds: u64 = std::env::var("WEAVESIGNAL_RTMP_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let list: Vec<Destination> = targets
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.trim().splitn(4, '=').collect();
            let profile = if f[1] == "open" {
                Profile::Open
            } else {
                Profile::Compatible
            };
            d(f[0], f[2], f[3], profile)
        })
        .collect();
    let names: Vec<String> = list.iter().map(|x| x.name.clone()).collect();
    let (desk, _stop, _dir) = stack_desk("servers", 0, "");
    let v = desk.video().unwrap().clone();
    desk.add_person("callum", Role::Presenter).unwrap();
    add_moving(&desk, "callum", 1000.0);
    v.rtmp().set_destinations("s4", list).unwrap();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(seconds) {
        run_for(&desk, Duration::from_secs(5));
        println!(
            "{:>3} s: {}",
            t0.elapsed().as_secs(),
            serde_json::to_string(&v.status().rtmp).unwrap()
        );
    }
    let st = v.status().rtmp;
    std::fs::write(
        run_dir().join("servers-status.json"),
        serde_json::to_string_pretty(&st).unwrap(),
    )
    .unwrap();
    for n in names {
        let x = st.destinations.iter().find(|x| x.name == n).unwrap();
        println!(
            "{n}: {:?}, {} bytes, {} connections, last error {:?}",
            x.state, x.bytes, x.connects, x.last_error
        );
    }
}

/// The extra encode's cost (a measurement, not a pass mark): the desk's CPU with a moving
/// camera on air, video on, then with each profile sending (to files), at the default 480p20
/// canvas and at 720p30. Run with `--release`.
#[test]
#[ignore = "a measurement: just test-rtmp-cost"]
fn rtmp_cost() {
    let window = Duration::from_secs(20);
    for canvas in ["", "720p30"] {
        gst::init().unwrap();
        let dir = scratch("cost");
        let mut cfg: Config = config(&dir, 0, "");
        cfg.video.canvas = canvas.to_owned();
        let desk = Desk::new(cfg, Outputs::NONE).unwrap();
        let stop = start(&desk);
        let v = desk.video().unwrap().clone();
        desk.add_person("callum", Role::Presenter).unwrap();
        add_moving(&desk, "callum", 1000.0);
        run_for(&desk, Duration::from_secs(8));
        let base = cpu_over(&desk, window);
        let mut rows = vec![("video on, HLS, no RTMP".to_owned(), base)];
        for (label, list) in [
            (
                "+ compatible (H.264, AAC)",
                vec![dest("c", "rtmp://unused/live", "k", Profile::Compatible)],
            ),
            (
                "+ open (AV1, Opus)",
                vec![dest("o", "rtmp://unused/live", "k", Profile::Open)],
            ),
            (
                "+ both",
                vec![
                    dest("c", "rtmp://unused/live", "k", Profile::Compatible),
                    dest("o", "rtmp://unused/live", "k", Profile::Open),
                ],
            ),
        ] {
            v.rtmp().set_destinations_to_files(label, list, &dir);
            run_for(&desk, Duration::from_secs(6));
            let cpu = cpu_over(&desk, window);
            println!(
                "rtmp: {}",
                serde_json::to_string(&v.status().rtmp.encoders).unwrap()
            );
            rows.push((label.to_owned(), cpu));
            v.rtmp().set_destinations_to_files("off", vec![], &dir);
            run_for(&desk, Duration::from_secs(3));
        }
        let c = v.canvas();
        println!(
            "canvas {} ({}x{} at {} fps), RTMP rung the canvas:",
            c.name, c.width, c.height, c.fps
        );
        for (label, cpu) in &rows {
            println!(
                "  {label:<28} {cpu:6.1} % of one core   ({:+.1})",
                cpu - base
            );
        }
        drop(stop);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
