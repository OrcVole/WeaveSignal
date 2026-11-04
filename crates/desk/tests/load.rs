// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What the desk costs per person: its CPU with 4, 8 and 13 people in the room, as a share of
//! one core. Ignored unless the stack is up: `just test-load` (a release build, as the
//! station runs it).
//!
//! The live staging show (2026-10-25) used 3.4 to 4.6 cores for the whole station with about
//! 13 people and 18 monitors at the desk. Each person here publishes a tone from a process
//! of their own, so the CPU counted is the desk's. `WEAVESIGNAL_LOAD_MONITORS` adds that many
//! listen-only monitors per step, joined with the role in `WEAVESIGNAL_LOAD_MONITOR_ROLE`
//! (`producer`, as the staging driver did, or `listener`). `WEAVESIGNAL_LOAD_CARTS` (2 by
//! default) carts are then fired at each step, a bed and one-shots decoded from FLAC as the
//! cart wall plays them (ADR-0117), and the CPU measured again with them sounding.

mod stack;

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Person, Room, flac, linked, run_for, wait_for};
use weavesignal_desk::carts::{CartMode, Fire};
use weavesignal_desk::census::{census, cpu_by_thread_kind, cpu_share, thread_names, threads};
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

#[test]
#[ignore = "needs the loopback stack: just test-load"]
fn what_the_desk_costs_per_person() {
    gst::init().unwrap();
    let monitors: usize = std::env::var("WEAVESIGNAL_LOAD_MONITORS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let monitor_role: Role = std::env::var("WEAVESIGNAL_LOAD_MONITOR_ROLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(Role::Producer);
    let steps: Vec<usize> = std::env::var("WEAVESIGNAL_LOAD_STEPS")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.parse().ok()).collect())
        .unwrap_or_else(|| vec![4, 8, 13]);
    let carts: usize = std::env::var("WEAVESIGNAL_LOAD_CARTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let cart_dir =
        std::env::temp_dir().join(format!("weavesignal-load-carts-{}", std::process::id()));
    std::fs::create_dir_all(&cart_dir).unwrap();
    // A bed (it loops), then one-shots long enough to sound through the measurement.
    let cart_files: Vec<(String, CartMode)> = (0..carts)
        .map(|i| {
            let mode = if i == 0 {
                CartMode::Bed
            } else {
                CartMode::OneShot
            };
            let secs = if i == 0 { 10.0 } else { 40.0 };
            (flac(&cart_dir, 2500.0 + 300.0 * i as f64, secs), mode)
        })
        .collect();
    let room = Room::fresh("m1-load");
    let desk = Desk::new(
        room.config(&format!(
            "[room]\npoll_ms = 300\nleave_grace_ms = 1000\n\n[carts]\ndir = \"{}\"\n",
            cart_dir.display()
        )),
        Outputs::ALL,
    )
    .unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (desk, stop) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(desk, api, stop))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    run_for(&desk, Duration::from_secs(5));
    let idle = cpu_share(Duration::from_secs(5));
    let mut rows = vec![serde_json::json!({"people": 0, "cpu_pct": (idle * 100.0).round()})];
    for (step, &n) in steps.iter().enumerate() {
        let cast: Vec<(String, Role, f64)> = (0..n)
            .map(|i| {
                let role = match i {
                    0 => Role::Presenter,
                    i if i % 4 == 3 => Role::Caller,
                    _ => Role::Guest,
                };
                (format!("s{step}-p{i:02}"), role, 400.0 + 150.0 * i as f64)
            })
            .collect();
        let mut people: Vec<Person> = cast
            .iter()
            .map(|(id, role, f)| Person::join(&room, id, *role, *f))
            .collect();
        let names: Vec<&str> = cast.iter().map(|(id, ..)| id.as_str()).collect();
        assert!(
            wait_for(&desk, Duration::from_secs(45), || linked(&desk, &names)
                == n)
            .is_some(),
            "{n} people: the desk linked only {}",
            linked(&desk, &names)
        );
        for (id, role, _) in &cast {
            if *role == Role::Caller {
                desk.set_on_air(id, true);
            }
        }
        for m in 0..monitors {
            let (target, target_role, _) = &cast[m % n];
            people.push(Person::ear(
                &room,
                &weavesignal_proto::room_for(&room.name, *target_role, target),
                &format!("s{step}-mon{m:02}"),
                monitor_role,
                &format!("desk-mm-{}", names[m % n]),
            ));
        }
        run_for(&desk, Duration::from_secs(8));
        let cpu = cpu_share(Duration::from_secs(10));
        let c = census(desk.pipeline());
        let kinds: Vec<String> = cpu_by_thread_kind(Duration::from_secs(5))
            .into_iter()
            .take(12)
            .map(|(k, v)| format!("{k} {:.1} %", v * 100.0))
            .collect();
        println!("{n} people, where the CPU goes: {kinds:?}");
        let mut by_kind = std::collections::BTreeMap::<String, usize>::new();
        for (name, count) in thread_names() {
            let kind: String = name.chars().filter(|c| !c.is_ascii_digit()).collect();
            *by_kind.entry(kind).or_default() += count;
        }
        println!("{n} people, threads by kind: {by_kind:?}");
        let row = serde_json::json!({
            "people": n,
            "monitors": monitors,
            "monitor_role": monitor_role.as_str(),
            "at_desk": desk.status().len(),
            "cpu_pct": (cpu * 100.0).round(),
            "threads": threads(),
            "mix": desk.mix_stats(),
            "elements": c.elements,
            "pads": c.pads,
        });
        println!("{row}");
        rows.push(row);
        if !cart_files.is_empty() {
            for (i, (file, mode)) in cart_files.iter().enumerate() {
                let fired = desk.fire_cart(&Fire {
                    cart: format!("00000000-0000-4000-8000-{i:012x}"),
                    file: file.clone(),
                    gain_db: -6.0,
                    mode: *mode,
                    cue: false,
                    duration_ms: None,
                });
                assert!(fired.is_ok(), "cart {i}: {fired:?}");
            }
            run_for(&desk, Duration::from_secs(3));
            let cpu = cpu_share(Duration::from_secs(10));
            let sounding = desk.carts().sounding.len();
            let row = serde_json::json!({
                "people": n,
                "carts_sounding": sounding,
                "cpu_pct": (cpu * 100.0).round(),
                "threads": threads(),
                "mix": desk.mix_stats(),
            });
            println!("{row}");
            assert_eq!(sounding, cart_files.len(), "every cart sounded through");
            rows.push(row);
            desk.stop_carts(None);
            run_for(&desk, Duration::from_secs(1));
        }
        people.into_iter().for_each(Person::leave);
        assert!(
            wait_for(&desk, Duration::from_secs(60), || desk.status().is_empty()).is_some(),
            "the desk still holds people"
        );
        run_for(&desk, Duration::from_secs(3));
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    watcher.join().unwrap();
    desk.pipeline().set_state(gst::State::Null).unwrap();
    let _ = std::fs::remove_dir_all(&cart_dir);
    let report = serde_json::json!({ "steps": rows });
    println!("{report}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }
}
