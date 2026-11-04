// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A person whose link drops and who rejoins at once under the same identity is heard again
//! (M3's relay run, 2026-10-26: after a rejoin the desk heard nothing more from the person).
//! Ignored unless the stack is up: `TEST=rejoin tests/room/run-stack.sh`.

mod stack;

use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Person, Room, linked, run_for, wait_for};
use weavesignal_desk::analysis::{ratio_db, tone_power};
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

/// The tone's level in the programme over the last second, against silence.
fn heard(tap: &weavesignal_desk::analysis::Tap, freq: f64) -> f64 {
    let s = tap.samples();
    let last = &s[s.len().saturating_sub(48_000)..];
    ratio_db(tone_power(last, freq), 1.0)
}

#[test]
#[ignore = "needs the loopback stack: TEST=rejoin tests/room/run-stack.sh"]
fn a_person_who_rejoins_at_once_under_the_same_identity_is_heard_again() {
    gst::init().unwrap();
    let room = Room::fresh("m5-rejoin");
    let desk = Desk::new(
        room.config("[room]\npoll_ms = 300\nlink_timeout_ms = 4000\n"),
        Outputs::ALL,
    )
    .unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (d, s) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(d, api, s))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    let programme = desk.tap(&Bus::Programme).unwrap();
    let mut rows = Vec::new();
    let mut person = Some(Person::join(&room, "gregor", Role::Guest, 1500.0));
    assert!(
        wait_for(&desk, Duration::from_secs(30), || linked(
            &desk,
            &["gregor"]
        ) == 1)
        .is_some()
    );
    run_for(&desk, Duration::from_secs(3));
    let before = heard(&programme, 1500.0);
    assert!(before > 60.0, "not heard before the drop: {before:.1} dB");
    for round in 1..=3 {
        // The link drops (the process killed, as a lost network) and the page rejoins at once.
        drop(person.take());
        // Gone from the programme first (a second without the tone), so what follows is the
        // rejoined voice, not the last second of the old one.
        assert!(
            wait_for(&desk, Duration::from_secs(10), || heard(&programme, 1500.0)
                < 30.0)
            .is_some(),
            "round {round}: still heard after the drop"
        );
        person = Some(Person::join(&room, "gregor", Role::Guest, 1500.0));
        let t0 = Instant::now();
        let back = wait_for(&desk, Duration::from_secs(30), || {
            heard(&programme, 1500.0) > 60.0
        });
        let after = heard(&programme, 1500.0);
        println!(
            "round {round}: heard again after {:?} ({after:.1} dB)",
            back.map(|_| t0.elapsed())
        );
        rows.push(serde_json::json!({"round": round, "heard_after_ms": back.map(|d| d.as_millis() as u64)}));
        assert!(back.is_some(), "round {round}: not heard again within 30 s");
        run_for(&desk, Duration::from_secs(2));
    }
    println!("{}", serde_json::json!({ "rejoins": rows }));
    drop(person);
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    watcher.join().unwrap();
    desk.pipeline().set_state(gst::State::Null).unwrap();
}
