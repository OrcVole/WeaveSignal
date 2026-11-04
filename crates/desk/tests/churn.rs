// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk across sessions: people join and leave the room again and again, and once the
//! room is empty the desk must hold what it held before anyone came (the staging run,
//! 2026-10-25: after three shows it used about two cores with the room empty, 193 % against
//! about 18 % after a restart). Ignored unless the stack is up: `just test-churn`.
//!
//! Each cycle three people join with new identities (a presenter, a guest and a caller), are
//! linked, talk for a few seconds, and drop; the desk lets them go after its leave grace. The
//! people publish from processes of their own, so the threads and CPU counted here are the
//! desk's (and this test's).

mod stack;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Person, Room, linked, run_for, wait_for};
use weavesignal_desk::census::{Census, census, cpu_share, thread_names, threads};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

#[derive(Debug)]
struct Idle {
    census: Census,
    threads: usize,
    cpu: f64,
    desk_in_room: usize,
}

fn idle(desk: &Desk, room: &Room) -> Idle {
    run_for(desk, Duration::from_secs(3));
    let cpu = cpu_share(Duration::from_secs(5));
    Idle {
        census: census(desk.pipeline()),
        threads: threads(),
        cpu,
        desk_in_room: room
            .identities()
            .iter()
            .filter(|i| i.starts_with("desk-"))
            .count(),
    }
}

#[test]
#[ignore = "needs the loopback stack: just test-churn"]
fn the_desk_comes_back_to_its_baseline_after_people_join_and_leave() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    gst::init().unwrap();
    let cycles: usize = std::env::var("WEAVESIGNAL_CHURN_CYCLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let room = Room::fresh("m1-churn");
    // Leave grace 1 s: the desk lets a leaver go soon after, so cycles are short.
    let config = room.config("[room]\npoll_ms = 300\nleave_grace_ms = 1000\n");
    let desk = Desk::new(config, Outputs::ALL).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (desk, stop) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(desk, api, stop))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    // Playout connects and the desk settles before the baseline.
    run_for(&desk, Duration::from_secs(8));
    let before = idle(&desk, &room);
    let names_before = thread_names();
    println!("baseline, the room empty: {before:?}");

    let mut busy = Vec::new();
    let mut idle_threads = Vec::new();
    for k in 0..cycles {
        let ids = [
            (format!("c{k}-hamish"), Role::Presenter, 500.0),
            (format!("c{k}-gregor"), Role::Guest, 700.0),
            (format!("c{k}-callum"), Role::Caller, 900.0),
        ];
        let mut people: Vec<Person> = ids
            .iter()
            .map(|(id, role, f)| Person::join(&room, id, *role, *f))
            .collect();
        // Someone who comes and goes before the desk has them, as a page reloaded at once.
        let flash = Person::join(&room, &format!("c{k}-flash"), Role::Guest, 1100.0);
        std::thread::sleep(Duration::from_millis(300));
        if k % 2 == 0 {
            flash.leave();
        } else {
            drop(flash);
        }
        let names: Vec<&str> = ids.iter().map(|(id, ..)| id.as_str()).collect();
        let took = wait_for(&desk, Duration::from_secs(30), || {
            linked(&desk, &names) == names.len()
        });
        assert!(
            took.is_some(),
            "cycle {k}: the desk linked only {} of 3: {:?}",
            linked(&desk, &names),
            desk.status()
        );
        // An ear on the presenter's return, with a role in its token (the staging driver's).
        people.push(Person::ear(
            &room,
            &room.name,
            &format!("c{k}-ear"),
            Role::Guest,
            &format!("desk-mm-{}", names[0]),
        ));
        // The caller's subscription is dropped again and again while it is still being made,
        // as when a starved desk times links out; then the guest's, once linked, as after a
        // failure. The room watch makes each again.
        for _ in 0..4 {
            run_for(&desk, Duration::from_millis(120));
            desk.drop_subscription(names[2]);
        }
        run_for(&desk, Duration::from_secs(2));
        desk.drop_subscription(names[1]);
        let again = wait_for(&desk, Duration::from_secs(20), || {
            linked(&desk, &names) == names.len()
        });
        assert!(
            again.is_some(),
            "cycle {k}: the guest was not subscribed again"
        );
        run_for(&desk, Duration::from_secs(2));
        busy.push((
            census(desk.pipeline()),
            threads(),
            cpu_share(Duration::from_secs(2)),
        ));
        // Every other cycle the links drop instead of leaving, as on a bad network.
        if k % 2 == 0 {
            people.into_iter().for_each(Person::leave);
        } else {
            drop(people);
        }
        let gone = wait_for(&desk, Duration::from_secs(60), || desk.status().is_empty());
        assert!(
            gone.is_some(),
            "cycle {k}: the desk still holds people after they left: {:?}",
            desk.status()
        );
        run_for(&desk, Duration::from_secs(2));
        idle_threads.push(threads());
        println!(
            "cycle {k}: linked after {:.1} s, let go after {:.1} s; while busy {:?}; threads after {}",
            took.unwrap().as_secs_f64(),
            gone.unwrap().as_secs_f64(),
            busy.last().unwrap(),
            idle_threads.last().unwrap()
        );
    }
    // LiveKit closes the desk's own connections a moment after they leave.
    run_for(&desk, Duration::from_secs(5));
    let after = idle(&desk, &room);
    println!("after {cycles} cycles, the room empty: {after:?}");
    // A subscription dropped while it was still joining leaves LiveKit without a goodbye, so
    // LiveKit lists it until its own timeout (about 20 s); it must not stay.
    let listed = room.identities();
    let ghosts_gone = wait_for(&desk, Duration::from_secs(40), || {
        !room.identities().iter().any(|i| i.starts_with("desk-"))
    });
    println!("LiveKit listed {listed:?}; the desk's own gone after {ghosts_gone:?}");
    let names_after = thread_names();
    let grown: Vec<String> = names_after
        .iter()
        .filter(|(n, c)| names_before.get(*n).copied().unwrap_or(0) < **c)
        .map(|(n, c)| format!("{n}: {} -> {c}", names_before.get(n).copied().unwrap_or(0)))
        .collect();
    println!("threads that grew: {grown:?}");
    stop.store(true, Ordering::SeqCst);
    watcher.join().unwrap();
    desk.pipeline().set_state(gst::State::Null).unwrap();

    let report = serde_json::json!({
        "cycles": cycles,
        "before": {"elements": before.census.elements, "pads": before.census.pads,
                   "threads": before.threads, "cpu": before.cpu, "desk_in_room": before.desk_in_room},
        "after": {"elements": after.census.elements, "pads": after.census.pads,
                  "threads": after.threads, "cpu": after.cpu, "desk_in_room": after.desk_in_room},
        "idle_threads_per_cycle": idle_threads,
        "threads_grown": grown,
        "desk_connections_gone_after_s": ghosts_gone.map(|d| d.as_secs_f64()),
        "busy": busy.iter().map(|(c, t, cpu)| serde_json::json!(
            {"elements": c.elements, "pads": c.pads, "threads": t, "cpu": cpu})).collect::<Vec<_>>(),
    });
    println!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }

    assert_eq!(
        after.census, before.census,
        "the desk's pipeline holds more than before anyone joined"
    );
    assert!(
        ghosts_gone.is_some(),
        "the desk left connections of its own in the room: {listed:?}"
    );
    // A few threads may come and go (GLib's pools); a leaked session holds several.
    assert!(
        after.threads <= before.threads + 4,
        "threads grew from {} to {}",
        before.threads,
        after.threads
    );
    assert!(
        after.cpu <= before.cpu * 1.5 + 0.05,
        "idle CPU grew from {:.0} % to {:.0} % of a core",
        before.cpu * 100.0,
        after.cpu * 100.0
    );
}
