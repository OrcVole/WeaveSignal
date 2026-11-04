// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video path in a real LiveKit room (M5). Ignored unless the stack is up:
//! `TEST=cameras tests/room/run-stack.sh` starts it and runs this in the desk image (with
//! `WEAVESIGNAL_DESK_GPU=1`, NVENC).
//!
//! People publish a tone and a solid-colour camera from processes of their own, VP8 as a
//! browser sends it; the desk subscribes, cuts, encodes and writes HLS, and publishes the live
//! picture as `desk-pgm`, which a guest's page watches (ADR-0028). Then what four cameras
//! cost the desk, against four voices without video.

mod stack;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use stack::{Person, Room, linked, run_for, video_toml, wait_for};
use weavesignal_desk::census::{cpu_by_thread_kind, cpu_share, threads};
use weavesignal_desk::picture::{Frame, Yuv};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_desk::vision::{PictureMode, Shot};
use weavesignal_proto::Role;

const RED: u32 = 0xffdc_2828;
const BLUE: u32 = 0xff28_3cdc;

fn yuv(argb: u32) -> Yuv {
    Yuv::from_rgb((argb >> 16) as u8, (argb >> 8) as u8, argb as u8)
}

fn middle(f: &Frame) -> Yuv {
    f.mean(f.width / 3, f.height / 3, f.width / 3, f.height / 3)
}

fn follow(
    desk: &Desk,
) -> (
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let c = desk.config();
    let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
    let (d, s) = (desk.clone(), stop.clone());
    (stop, std::thread::spawn(move || room::watch(d, api, s)))
}

/// What a guest's page sees: the desk's picture participant, decoded, the latest frame kept.
fn viewer(room: &Room) -> (gst::Pipeline, Arc<Mutex<Option<Frame>>>) {
    let p = gst::Pipeline::with_name("viewer");
    let src = gst::ElementFactory::make("livekitwebrtcsrc")
        .build()
        .unwrap();
    let sig = src.property::<glib::Object>("signaller");
    sig.set_property("ws-url", &room.url);
    sig.set_property("api-key", &room.keys.api_key);
    sig.set_property("secret-key", &room.keys.api_secret);
    sig.set_property("room-name", &room.name);
    sig.set_property("identity", "viewer-guest");
    sig.set_property("participant-name", "viewer-guest");
    sig.set_property("producer-peer-id", "desk-pgm");
    let last = Arc::new(Mutex::new(None));
    let (pw, l2) = (p.downgrade(), last.clone());
    src.connect_pad_added(move |_, pad| {
        let Some(p) = pw.upgrade() else { return };
        let sink: gst::Element = if pad.name().starts_with("video") {
            let conv = gst::ElementFactory::make("videoconvert").build().unwrap();
            let app = gst_app::AppSink::builder()
                .caps(&"video/x-raw,format=I420".parse::<gst::Caps>().unwrap())
                .sync(false)
                .build();
            let l3 = l2.clone();
            app.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let st = sample.caps().unwrap().structure(0).unwrap().to_owned();
                        let (w, h) = (
                            st.get::<i32>("width").unwrap() as usize,
                            st.get::<i32>("height").unwrap() as usize,
                        );
                        let map = sample.buffer().unwrap().map_readable().unwrap();
                        *l3.lock().unwrap() = Frame::from_bytes(w, h, map.as_slice());
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
            p.add_many([&conv, app.upcast_ref()]).unwrap();
            conv.link(&app).unwrap();
            app.sync_state_with_parent().unwrap();
            conv.sync_state_with_parent().unwrap();
            conv
        } else {
            let fs = gst::ElementFactory::make("fakesink").build().unwrap();
            p.add(&fs).unwrap();
            fs.sync_state_with_parent().unwrap();
            fs
        };
        let _ = pad.link(&sink.static_pad("sink").unwrap());
    });
    p.add(&src).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    (p, last)
}

/// The last pictures of the HLS's first rendition, decoded to raw frames.
fn hls_pictures(dir: &std::path::Path) -> Vec<Frame> {
    let d = dir.join("v0");
    let playlist = std::fs::read_to_string(d.join("index.m3u8")).unwrap();
    let init = playlist
        .lines()
        .find_map(|l| l.strip_prefix("#EXT-X-MAP:URI=\""))
        .unwrap()
        .trim_end_matches('"')
        .to_owned();
    let mut bytes = std::fs::read(d.join(init)).unwrap();
    let last = playlist
        .lines()
        .rfind(|l| !l.starts_with('#') && !l.is_empty())
        .unwrap();
    bytes.extend(std::fs::read(d.join(last)).unwrap());
    let f = dir.join("last.mp4");
    std::fs::write(&f, bytes).unwrap();
    let p = gst::parse::launch(&format!(
        "filesrc location={} ! qtdemux ! av1parse ! dav1ddec ! videoconvert ! video/x-raw,format=I420 ! appsink name=out sync=false",
        f.display()
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
    while let Some(s) = sink.try_pull_sample(gst::ClockTime::from_seconds(5)) {
        let st = s.caps().unwrap().structure(0).unwrap().to_owned();
        let map = s.buffer().unwrap().map_readable().unwrap();
        out.extend(Frame::from_bytes(
            st.get::<i32>("width").unwrap() as usize,
            st.get::<i32>("height").unwrap() as usize,
            map.as_slice(),
        ));
    }
    p.set_state(gst::State::Null).unwrap();
    out
}

#[test]
#[ignore = "needs the loopback stack: TEST=cameras tests/room/run-stack.sh"]
fn cameras_through_the_room_reach_the_hls_and_the_guests_see_the_desks_picture() {
    gst::init().unwrap();
    let room = Room::fresh("m5-cameras");
    let config = room.config(&format!(
        "[room]\npoll_ms = 300\n{}",
        video_toml(&room.name)
    ));
    let dir = config.video.hls.dir.clone();
    let desk = Desk::new(config, Outputs::ALL).unwrap();
    let (stop, watcher) = follow(&desk);
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    let t0 = Instant::now();
    let _hamish = Person::join_with_camera(&room, "hamish", Role::Presenter, 1000.0, RED);
    let _gregor = Person::join_with_camera(&room, "gregor", Role::Guest, 3000.0, BLUE);
    let v = desk.video().unwrap().clone();
    let both = wait_for(&desk, Duration::from_secs(45), || {
        linked(&desk, &["hamish", "gregor"]) == 2 && v.status().cameras.len() == 2
    });
    let status = v.status();
    println!("status: {}", serde_json::to_string(&status).unwrap());
    let both = both.expect("both cameras reached the desk");
    println!(
        "both cameras at the desk after {:.1} s",
        t0.elapsed().as_secs_f64()
    );
    // Hold Gregor's camera by hand, then read the HLS and the room's picture.
    v.set_mode(PictureMode::Person {
        identity: "gregor".into(),
    });
    let (viewer, seen) = viewer(&room);
    run_for(&desk, Duration::from_secs(8));
    assert_eq!(
        v.status().shot,
        Shot::Person {
            identity: "gregor".into()
        }
    );
    let pics = hls_pictures(&dir);
    assert!(!pics.is_empty(), "no pictures in the HLS");
    let m = middle(pics.last().unwrap());
    println!("HLS, middle of the last picture: {m:?}");
    assert!(
        m.distance(&yuv(BLUE)) < 16.0,
        "the HLS shows {m:?}, not Gregor's camera"
    );
    let room_frame = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the guest's page saw no picture");
    let rm = middle(&room_frame);
    println!(
        "the room's picture: {}x{}, middle {rm:?}",
        room_frame.width, room_frame.height
    );
    assert!(rm.distance(&yuv(BLUE)) < 20.0, "the room shows {rm:?}");
    // Hamish's thumbnail along the bottom, at the right.
    let (w, h) = (room_frame.width, room_frame.height);
    let (tw, th) = (w / 6, h / 6);
    let thumb = room_frame.mean(w - 8 - tw + tw / 4, h - 8 - th + th / 4, tw / 2, th / 2);
    println!("the room's thumbnail: {thumb:?}");
    assert!(
        thumb.distance(&yuv(RED)) < 24.0,
        "no thumbnail of Hamish: {thumb:?}"
    );
    // Back to the rule: two voices, both speaking; a camera on air either way.
    v.set_mode(PictureMode::Auto);
    run_for(&desk, Duration::from_secs(2));
    assert!(matches!(v.status().shot, Shot::Person { .. }));
    let report = serde_json::json!({
        "cameras_linked_s": both.as_secs_f64(),
        "hls_middle": format!("{m:?}"),
        "room_picture": format!("{}x{}", room_frame.width, room_frame.height),
        "status": v.status(),
    });
    println!("{report}");
    let _ = viewer.set_state(gst::State::Null);
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    watcher.join().unwrap();
    v.stop();
    desk.pipeline().set_state(gst::State::Null).unwrap();
}

/// GPU figures, whole-GPU (the workstation's desktop included), if `nvidia-smi` is here.
fn gpu_sample() -> Option<serde_json::Value> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,utilization.encoder,utilization.decoder,memory.used,encoder.stats.sessionCount",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let v: Vec<f64> = line
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    (v.len() == 5).then(|| {
        serde_json::json!({"gpu_pct": v[0], "encoder_pct": v[1], "decoder_pct": v[2],
            "memory_mb": v[3], "nvenc_sessions": v[4]})
    })
}

fn measure(desk: &Desk, label: &str) -> serde_json::Value {
    run_for(desk, Duration::from_secs(6));
    let gpu: Vec<serde_json::Value> = (0..5)
        .filter_map(|_| {
            std::thread::sleep(Duration::from_millis(400));
            gpu_sample()
        })
        .collect();
    let cpu = cpu_share(Duration::from_secs(10));
    let kinds: Vec<String> = cpu_by_thread_kind(Duration::from_secs(5))
        .into_iter()
        .take(10)
        .map(|(k, v)| format!("{k} {:.1} %", v * 100.0))
        .collect();
    let row = serde_json::json!({
        "case": label,
        "cpu_pct": (cpu * 1000.0).round() / 10.0,
        "threads": threads(),
        "mix": desk.mix_stats(),
        "video": desk.video().map(|v| v.status()),
        "gpu": gpu.last(),
        "cpu_by_thread": kinds,
    });
    println!("{row}");
    row
}

#[test]
#[ignore = "needs the loopback stack: TEST=cameras tests/room/run-stack.sh (RELEASE=1 for figures)"]
fn what_four_cameras_cost_the_desk() {
    gst::init().unwrap();
    let mut rows = Vec::new();
    for video in [false, true] {
        let room = Room::fresh(if video {
            "m5-cost-video"
        } else {
            "m5-cost-audio"
        });
        let extra = if video {
            format!("[room]\npoll_ms = 300\n{}", video_toml(&room.name))
        } else {
            "[room]\npoll_ms = 300\n".to_owned()
        };
        let desk = Desk::new(room.config(&extra), Outputs::ALL).unwrap();
        let (stop, watcher) = follow(&desk);
        desk.pipeline().set_state(gst::State::Playing).unwrap();
        let names = ["c0", "c1", "c2", "c3"];
        let colours = [RED, BLUE, 0xff28_c828, 0xffe0_c020];
        let people: Vec<Person> = names
            .iter()
            .zip(colours)
            .enumerate()
            .map(|(i, (id, c))| {
                let role = if i == 0 { Role::Presenter } else { Role::Guest };
                let f = 500.0 + 700.0 * i as f64;
                if video {
                    Person::join_with_camera(&room, id, role, f, c)
                } else {
                    Person::join(&room, id, role, f)
                }
            })
            .collect();
        assert!(
            wait_for(&desk, Duration::from_secs(45), || {
                linked(&desk, &names) == 4
                    && desk.video().is_none_or(|v| v.status().cameras.len() == 4)
            })
            .is_some(),
            "the four did not all reach the desk"
        );
        rows.push(measure(
            &desk,
            if video {
                "4 people, 4 cameras, video on"
            } else {
                "4 people, video off"
            },
        ));
        people.into_iter().for_each(Person::leave);
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        watcher.join().unwrap();
        if let Some(v) = desk.video() {
            v.stop();
        }
        desk.pipeline().set_state(gst::State::Null).unwrap();
        std::thread::sleep(Duration::from_secs(2));
    }
    let report = serde_json::json!({ "steps": rows });
    println!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }
}
