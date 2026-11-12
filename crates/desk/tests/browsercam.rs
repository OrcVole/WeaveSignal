// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A browser's camera at the desk. Ignored unless the stack is up:
//! `PLAYWRIGHT_CORE=… TEST=browsercam tests/room/run-stack.sh` starts it, with
//! `tests/room/browser-camera.mjs` on the host as the browser.
//!
//! The talk station's fault (2026-11-11): a presenter joined from Brave, whose camera Chromium
//! sends as AV1. The desk could not take AV1 from the room (its image had no AV1 depayloader), so
//! that person's subscription failed "not-linked" and was made again about three times a second:
//! their voice came in snippets and their camera never reached the picture. `cameras.rs` never saw
//! it, because its cameras are GStreamer's VP8. Here headless Chromium publishes as the join page
//! does; the desk must hold the person's one subscription, hear them and show their camera.

mod stack;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Room, linked, run_for, video_toml, wait_for};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

/// Tells the browser to leave when the test ends, passed or not.
struct Leave(PathBuf);

impl Drop for Leave {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"");
    }
}

#[test]
#[ignore = "needs the loopback stack and a browser: TEST=browsercam tests/room/run-stack.sh"]
fn a_browsers_av1_camera_reaches_the_picture_and_the_person_stays_subscribed() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    gst::init().unwrap();
    let run = PathBuf::from(stack::env("WEAVESIGNAL_STACK_DIR"));
    let room = Room::fresh("browsercam");
    let config = room.config(&format!(
        "[room]\npoll_ms = 300\n{}",
        video_toml(&room.name)
    ));
    let desk = Desk::new(config, Outputs::ALL).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (d, s) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(d, api, s))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();

    let who = "hamish";
    let token = room
        .keys
        .participant_with(&room.name, who, who, Role::Presenter, 600, false)
        .unwrap();
    let _leave = Leave(run.join("browser-leave"));
    let sent_file = run.join("browser-camera.json");
    let _ = std::fs::remove_file(&sent_file);
    std::fs::write(
        run.join("browser-join.json"),
        serde_json::json!({ "url": room.url, "token": token }).to_string(),
    )
    .unwrap();
    let t0 = Instant::now();

    // What Chromium sends: the test is about AV1, so a browser that sent VP8 proves nothing.
    let sent = wait_for(&desk, Duration::from_secs(60), || sent_file.exists())
        .and_then(|_| std::fs::read_to_string(&sent_file).ok())
        .expect("the browser never said what it sends (is browser-camera.mjs running?)");
    println!("the browser sends: {sent}");
    let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(
        sent["codec"].as_str(),
        Some("video/AV1"),
        "Chromium did not send AV1, so this run tests nothing"
    );
    // One picture size (apps/web/src/lib/rules.ts): the desk's AV1 decoder stops at a change of
    // size, which Chromium's default SVC (L2T3_KEY) makes all the time.
    assert!(
        sent["scalability"]
            .as_str()
            .is_some_and(|m| m.starts_with("L1")),
        "the page sends more than one picture size: {}",
        sent["scalability"]
    );

    let v = desk.video().unwrap().clone();
    let reached = wait_for(&desk, Duration::from_secs(45), || {
        linked(&desk, &[who]) == 1 && v.status().cameras.iter().any(|c| c == who)
    });
    println!("status: {}", serde_json::to_string(&v.status()).unwrap());
    println!("people: {}", serde_json::to_string(&desk.status()).unwrap());
    let reached = reached.expect("the browser's voice and camera did not both reach the desk");
    println!(
        "voice and camera at the desk {:.1} s after the join",
        t0.elapsed().as_secs_f64()
    );

    // Then hold: one subscription the whole time, no failure of theirs, the camera still live.
    let first = desk.subscription(who).expect("subscribed");
    let contained = run_for(&desk, Duration::from_secs(20));
    let theirs = contained.iter().filter(|id| id.as_str() == who).count();
    let last = desk.subscription(who).expect("still subscribed");
    let camera_live = v.status().cameras.iter().any(|c| c == who);
    let voice_linked = linked(&desk, &[who]) == 1;
    println!(
        "after 20 s: failures of theirs {theirs}, subscription age {:.1} s (was {:.1} s), camera {camera_live}, voice {voice_linked}",
        last.age.as_secs_f64(),
        first.age.as_secs_f64()
    );

    // Then loss: one camera buffer in 25 dropped for 8 s, inside the desk, as a lossy network
    // drops packets (the talk station: an AV1 decoder never recovered by itself, and the picture
    // went back to the slate for good). The camera must come back once the loss stops.
    let queue = desk
        .pipeline()
        .iterate_recurse()
        .into_iter()
        .flatten()
        .find(|e| {
            let n = e.name();
            n.starts_with(&format!("cam-{who}-")) && n.ends_with("-queue")
        })
        .expect("the camera's queue");
    let lossy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let probe = {
        let (lossy, count) = (lossy.clone(), count.clone());
        queue
            .static_pad("sink")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                use std::sync::atomic::Ordering::Relaxed;
                if lossy.load(Relaxed) && count.fetch_add(1, Relaxed) % 25 == 24 {
                    gst::PadProbeReturn::Drop
                } else {
                    gst::PadProbeReturn::Ok
                }
            })
            .unwrap()
    };
    run_for(&desk, Duration::from_secs(8));
    lossy.store(false, std::sync::atomic::Ordering::SeqCst);
    let lost_live = v.status().cameras.iter().any(|c| c == who);
    let back = wait_for(&desk, Duration::from_secs(10), || {
        v.status().cameras.iter().any(|c| c == who)
    });
    run_for(&desk, Duration::from_secs(2));
    let still = v.status().cameras.iter().any(|c| c == who);
    queue.static_pad("sink").unwrap().remove_probe(probe);
    println!(
        "loss: camera live at its end {lost_live}; back {:?} after it; live 2 s later {still}",
        back.map(|d| d.as_secs_f64())
    );
    let report = serde_json::json!({
        "loss_back_s": back.map(|d| d.as_secs_f64()),
        "sent": sent,
        "reached_s": reached.as_secs_f64(),
        "failures": theirs,
        "subscription_age_s": last.age.as_secs_f64(),
        "camera_live": camera_live,
        "voice_linked": voice_linked,
    });
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = watcher.join();
    if let Some(vid) = desk.video() {
        vid.stop();
    }
    desk.pipeline().set_state(gst::State::Null).unwrap();

    assert_eq!(
        theirs, 0,
        "the browser's subscription failed {theirs} times"
    );
    assert!(
        last.age >= first.age + Duration::from_secs(19),
        "the browser was subscribed afresh during the hold"
    );
    assert!(
        camera_live,
        "the browser's camera stopped reaching the desk"
    );
    assert!(
        voice_linked,
        "the browser's voice stopped reaching the desk"
    );
    assert!(
        back.is_some() && still,
        "after 8 s of loss the browser's camera did not come back to the desk"
    );
}
