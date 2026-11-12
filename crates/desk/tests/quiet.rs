// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A person whose microphone carries digital silence is a quiet person on a healthy link, not a
//! fault (issue 13): while their packets keep arriving, the desk keeps them linked and never
//! subscribes again, however long they say nothing. A voice muted in the room brings no packets
//! at all (LiveKit stops forwarding it), and that is no fault either while it lasts; anything
//! else that brings no packets still is (the room watch's unit tests in `room.rs`).
//! Ignored unless the stack is up: `TEST=quiet tests/room/run-stack.sh`.

mod stack;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Person, Room, linked, run_for, wait_for};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

/// A short link timeout, so a run passes the first wait (3 s) and the back-offs (6, 12, 24 s).
const TIMEOUT: Duration = Duration::from_secs(3);

/// A desk following `room` with its room watch: the desk, the watch's stop, and its thread.
fn desk_watching(room: &Room) -> (Desk, Arc<AtomicBool>, JoinHandle<()>) {
    let desk = Desk::new(
        room.config(&format!(
            "[room]\npoll_ms = 300\nlink_timeout_ms = {}\n",
            TIMEOUT.as_millis()
        )),
        Outputs::ALL,
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (d, s) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(d, api, s))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    (desk, stop, watcher)
}

fn finish(desk: &Desk, stop: &AtomicBool, watcher: JoinHandle<()>) {
    stop.store(true, Ordering::SeqCst);
    watcher.join().unwrap();
    desk.pipeline().set_state(gst::State::Null).unwrap();
}

#[test]
#[ignore = "needs the loopback stack: TEST=quiet tests/room/run-stack.sh"]
fn a_digitally_silent_microphone_stays_linked_and_is_not_subscribed_again() {
    gst::init().unwrap();
    let room = Room::fresh("i13-quiet");
    let (desk, stop, watcher) = desk_watching(&room);
    let _quiet = Person::join_silent(&room, "morag", Role::Guest);
    let _talker = Person::join(&room, "hamish", Role::Presenter, 1000.0);
    let both = ["morag", "hamish"];
    assert!(
        wait_for(&desk, Duration::from_secs(30), || linked(&desk, &both) == 2).is_some(),
        "both voices should reach the desk"
    );
    // The silent voice is routed and its packets arriving.
    assert!(
        wait_for(&desk, Duration::from_secs(10), || desk
            .subscription("morag")
            .is_some_and(
                |s| s.routed && s.silent.is_some_and(|d| d < Duration::from_secs(1))
            ))
        .is_some(),
        "the silent voice should be routed and arriving: {:?}",
        desk.subscription("morag")
    );
    let t0 = Instant::now();
    let run = TIMEOUT * 8;
    let mut worst_silent = Duration::ZERO;
    while t0.elapsed() < run {
        run_for(&desk, Duration::from_millis(500));
        assert_eq!(
            linked(&desk, &both),
            2,
            "a voice dropped after {:?}: {:?}",
            t0.elapsed(),
            desk.status()
        );
        let s = desk
            .subscription("morag")
            .expect("the silent voice keeps its subscription");
        worst_silent = worst_silent.max(s.silent.unwrap_or(Duration::MAX));
    }
    let s = desk.subscription("morag").unwrap();
    println!(
        "silent voice over {run:?}: subscription age {:?}, longest since a packet {worst_silent:?}",
        s.age
    );
    assert!(
        s.age > run,
        "the silent voice was subscribed again (age {:?} after {run:?})",
        s.age
    );
    assert!(
        worst_silent < TIMEOUT,
        "the silent voice's packets stopped for {worst_silent:?}"
    );
    finish(&desk, &stop, watcher);
}

#[test]
#[ignore = "needs the loopback stack: TEST=quiet tests/room/run-stack.sh"]
fn a_voice_muted_in_the_room_keeps_its_subscription_and_is_heard_again_on_unmuting() {
    gst::init().unwrap();
    let room = Room::fresh("i13-muted");
    let (desk, stop, watcher) = desk_watching(&room);
    let _person = Person::join(&room, "morag", Role::Guest, 1500.0);
    assert!(
        wait_for(&desk, Duration::from_secs(30), || linked(&desk, &["morag"])
            == 1)
        .is_some(),
        "the voice should reach the desk"
    );
    run_for(&desk, Duration::from_secs(1));
    room.mute_audio("morag", true);
    // LiveKit stops forwarding a muted track: the packets stop.
    let stopped = wait_for(&desk, Duration::from_secs(10), || {
        desk.subscription("morag")
            .and_then(|s| s.silent)
            .is_some_and(|d| d > Duration::from_secs(1))
    });
    assert!(
        stopped.is_some(),
        "packets kept arriving from a muted track: {:?}",
        desk.subscription("morag")
    );
    let t0 = Instant::now();
    let run = TIMEOUT * 6;
    while t0.elapsed() < run {
        run_for(&desk, Duration::from_millis(500));
        assert_eq!(
            linked(&desk, &["morag"]),
            1,
            "the muted voice was dropped after {:?}: {:?}",
            t0.elapsed(),
            desk.status()
        );
    }
    let s = desk.subscription("morag").unwrap();
    println!(
        "muted voice over {run:?}: subscription age {:?}, {:?} since a packet",
        s.age, s.silent
    );
    assert!(
        s.age > run,
        "the muted voice was subscribed again (age {:?} after {run:?})",
        s.age
    );
    room.mute_audio("morag", false);
    let back = wait_for(&desk, Duration::from_secs(10), || {
        desk.subscription("morag")
            .and_then(|s| s.silent)
            .is_some_and(|d| d < Duration::from_millis(500))
    });
    println!("unmuted: packets again after {back:?}");
    assert!(back.is_some(), "no packets after unmuting");
    let s = desk.subscription("morag").unwrap();
    assert!(
        s.age > run,
        "heard again on a new subscription (age {:?}), not the one kept",
        s.age
    );
    finish(&desk, &stop, watcher);
}
