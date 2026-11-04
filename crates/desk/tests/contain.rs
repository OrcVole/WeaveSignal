// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! One person's failure stays theirs, in a real LiveKit room with real playout. Ignored unless
//! the stack is up: `just test-contain`.
//!
//! The staging run (2026-10-25): a dozen people joined at once, one subscription failed, and
//! the desk stopped, taking playout's SRT link and the listeners with it. Here failures are
//! forced inside one person's subscription, their input chain and their mix-minus publisher,
//! while the others go on; and twelve people join at once.

mod stack;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use stack::{Person, Room, linked, pump_errors, wait_for};
use weavesignal_desk::analysis::{Chunk, Tap, ratio_db, tone_power};
use weavesignal_desk::matrix::Bus;
use weavesignal_desk::pipeline::{Desk, Outputs};
use weavesignal_desk::room::{self, RoomApi};
use weavesignal_proto::Role;

struct Running {
    desk: Desk,
    stop: Arc<AtomicBool>,
    watcher: std::thread::JoinHandle<()>,
    programme: Tap,
}

fn start(room: &Room) -> Running {
    let desk = Desk::new(room.config("[room]\npoll_ms = 300\n"), Outputs::ALL).unwrap();
    let programme = desk.tap(&Bus::Programme).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let (desk, stop) = (desk.clone(), stop.clone());
        std::thread::spawn(move || room::watch(desk, api, stop))
    };
    desk.pipeline().set_state(gst::State::Playing).unwrap();
    Running {
        desk,
        stop,
        watcher,
        programme,
    }
}

impl Running {
    fn finish(self) -> Tap {
        self.stop.store(true, Ordering::SeqCst);
        self.watcher.join().unwrap();
        self.desk.pipeline().set_state(gst::State::Null).unwrap();
        self.programme
    }
}

/// The element directly in the desk's pipeline whose name starts with `prefix`.
fn element(desk: &Desk, prefix: &str, suffix: &str) -> gst::Element {
    desk.pipeline()
        .children()
        .into_iter()
        .find(|e| e.name().starts_with(prefix) && e.name().ends_with(suffix))
        .unwrap_or_else(|| panic!("no element {prefix}…{suffix} in the desk"))
}

/// Post an error from `el`, as a failing element does.
fn fail(el: &gst::Element) {
    println!("forcing an error from {}", el.path_string());
    let msg = gst::message::Error::builder(gst::CoreError::Failed, "forced by the test")
        .src(el)
        .build();
    let _ = el.post_message(msg);
}

/// A tone's level in each whole second of a recording from `from` on, in dB over silence.
fn per_second(chunks: &[Chunk], from: Instant, freq: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut t = from;
    let end = chunks.last().map_or(from, |c| c.0);
    while t + Duration::from_secs(1) <= end {
        let s: Vec<i16> = chunks
            .iter()
            .filter(|(at, _)| *at >= t && *at < t + Duration::from_secs(1))
            .flat_map(|(_, s)| s.iter().copied())
            .collect();
        out.push(ratio_db(tone_power(&s, freq), 1.0));
        t += Duration::from_secs(1);
    }
    out
}

fn last_seconds(chunks: &[Chunk], secs: f64) -> Vec<i16> {
    let end = chunks.last().map_or(Instant::now(), |c| c.0);
    chunks
        .iter()
        .filter(|(t, _)| *t + Duration::from_secs_f64(secs) >= end)
        .flat_map(|(_, s)| s.iter().copied())
        .collect()
}

/// Watch playout's link while running for `d`: whether it stayed connected throughout.
fn run_watching_playout(desk: &Desk, d: Duration, drops: &mut usize) {
    let t0 = Instant::now();
    while t0.elapsed() < d {
        pump_errors(desk);
        if !desk.playout().connected {
            *drops += 1;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Wait up to `limit` for `f`, watching playout's link; how long it took.
fn wait_watching(
    desk: &Desk,
    limit: Duration,
    drops: &mut usize,
    mut f: impl FnMut() -> bool,
) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        run_watching_playout(desk, Duration::from_millis(50), drops);
        if f() {
            return Some(t0.elapsed());
        }
    }
    None
}

fn has(desk: &Desk, el: &gst::Element) -> bool {
    desk.pipeline().children().iter().any(|e| e == el)
}

fn has_prefix(desk: &Desk, prefix: &str) -> bool {
    desk.pipeline()
        .children()
        .iter()
        .any(|e| e.name().starts_with(prefix))
}

#[test]
#[ignore = "needs the loopback stack: just test-contain"]
fn a_failure_inside_one_persons_chain_stays_theirs() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    gst::init().unwrap();
    let room = Room::fresh("m1-contain");
    let run = start(&room);
    let desk = run.desk.clone();
    let names = ["hamish", "gregor", "callum"];
    let _people = [
        Person::join(&room, names[0], Role::Presenter, 500.0),
        Person::join(&room, names[1], Role::Guest, 700.0),
        Person::join(&room, names[2], Role::Caller, 900.0),
    ];
    assert!(
        wait_for(&desk, Duration::from_secs(30), || linked(&desk, &names)
            == 3)
        .is_some(),
        "the desk did not link all three: {:?}",
        desk.status()
    );
    assert!(desk.set_on_air("callum", true));
    // Playout connects to the desk's SRT listener within a second or two of the desk.
    assert!(
        wait_for(&desk, Duration::from_secs(15), || desk.playout().connected).is_some(),
        "playout never connected"
    );
    let from = Instant::now();
    let mut drops = 0;
    run_watching_playout(&desk, Duration::from_secs(3), &mut drops);

    // Inside the guest's subscription (an element inside their livekitwebrtcsrc).
    let src = element(&desk, "src-gregor-", "");
    let inner = src
        .downcast_ref::<gst::Bin>()
        .and_then(|b| b.iterate_recurse().into_iter().flatten().next())
        .unwrap_or(src.clone());
    fail(&inner);
    // Their subscription is dropped, and the room watch makes a new one that links.
    let a = wait_watching(&desk, Duration::from_secs(20), &mut drops, || {
        !has(&desk, &src) && has_prefix(&desk, "src-gregor-") && linked(&desk, &names) == 3
    });
    assert!(
        a.is_some(),
        "the guest was not subscribed again after a subscription failure"
    );

    // The guest's input chain.
    let chain = element(&desk, "in-gregor-", "-convert");
    fail(&chain);
    let b = wait_watching(&desk, Duration::from_secs(20), &mut drops, || {
        !has(&desk, &chain) && linked(&desk, &names) == 3
    });
    assert!(
        b.is_some(),
        "the guest was not subscribed again after an input chain failure"
    );

    // The caller's mix-minus publisher.
    let old = element(&desk, "pub-mm-callum-", "");
    fail(&old);
    let c = wait_watching(&desk, Duration::from_secs(10), &mut drops, || {
        !has(&desk, &old) && has_prefix(&desk, "pub-mm-callum-")
    });
    assert!(c.is_some(), "the caller's mix-minus was not made again");
    assert!(
        desk.person("callum").unwrap().switches.on_air,
        "the caller lost their place on air"
    );
    run_watching_playout(&desk, Duration::from_secs(5), &mut drops);
    let in_room = room.identities();
    let status = desk.status();
    let programme = run.finish().chunks();

    let presenter = per_second(&programme, from, 500.0);
    let tail = last_seconds(&programme, 3.0);
    let level = |f: f64| ratio_db(tone_power(&tail, f), 1.0);
    println!(
        "{}",
        serde_json::json!({
            "guest_back_after_subscription_failure_s": a.map(|d| d.as_secs_f64()),
            "guest_back_after_chain_failure_s": b.map(|d| d.as_secs_f64()),
            "caller_mix_minus_remade_after_s": c.map(|d| d.as_secs_f64()),
            "presenter_per_second_db": presenter,
            "tail_db": {"presenter": level(500.0), "guest": level(700.0), "caller": level(900.0)},
            "playout_down_samples": drops,
        })
    );
    assert_eq!(drops, 0, "playout's link dropped while one person failed");
    assert!(
        presenter.len() >= 8 && presenter.iter().all(|&d| d > 60.0),
        "the presenter's voice was interrupted in the programme: {presenter:?}"
    );
    assert!(
        level(700.0) > 60.0 && level(900.0) > 60.0,
        "the guest or the caller is not back in the programme"
    );
    assert!(
        status.iter().filter(|s| s.linked).count() == 3,
        "not all three linked at the end: {status:?}"
    );
    assert!(
        in_room.iter().any(|i| i == "desk-mm-callum"),
        "the caller's remade mix-minus is not in the room: {in_room:?}"
    );
}

#[test]
#[ignore = "needs the loopback stack: just test-contain"]
fn twelve_people_join_at_once() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    gst::init().unwrap();
    let room = Room::fresh("m1-twelve");
    let run = start(&room);
    let desk = run.desk.clone();
    assert!(
        wait_for(&desk, Duration::from_secs(15), || desk.playout().connected).is_some(),
        "playout never connected"
    );
    let cast: Vec<(String, Role, f64)> = (0..12)
        .map(|i| {
            let role = match i {
                0 => Role::Presenter,
                1..=7 => Role::Guest,
                _ => Role::Caller,
            };
            (format!("p{i:02}"), role, 400.0 + 200.0 * i as f64)
        })
        .collect();
    let t0 = Instant::now();
    let _people: Vec<Person> = cast
        .iter()
        .map(|(id, role, f)| Person::join(&room, id, *role, *f))
        .collect();
    let names: Vec<&str> = cast.iter().map(|(id, ..)| id.as_str()).collect();
    let mut drops = 0;
    let mut all = None;
    while t0.elapsed() < Duration::from_secs(45) {
        run_watching_playout(&desk, Duration::from_millis(200), &mut drops);
        if linked(&desk, &names) == names.len() {
            all = Some(t0.elapsed());
            break;
        }
    }
    assert!(
        all.is_some(),
        "the desk linked only {} of 12 in 45 s: {:?}",
        linked(&desk, &names),
        desk.status()
    );
    for (id, role, _) in &cast {
        if *role == Role::Caller {
            assert!(desk.set_on_air(id, true));
        }
    }
    run_watching_playout(&desk, Duration::from_secs(4), &mut drops);
    let programme = run.finish().chunks();
    let tail = last_seconds(&programme, 3.0);
    let levels: Vec<f64> = cast
        .iter()
        .map(|(_, _, f)| ratio_db(tone_power(&tail, *f), 1.0))
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "all_linked_after_s": all.map(|d| d.as_secs_f64()),
            "programme_db": levels,
            "playout_down_samples": drops,
        })
    );
    assert_eq!(
        drops, 0,
        "playout's link dropped while twelve people joined"
    );
    assert!(
        levels.iter().all(|&d| d > 60.0),
        "a voice is missing from the programme: {levels:?}"
    );
}

#[test]
#[ignore = "needs the loopback stack: just test-contain"]
fn twelve_people_joining_while_the_desk_restarts_cannot_stop_it() {
    // The staging run, after the containment fix: the desk exited once as the participants
    // joined. Here the desk is stopped and started again three times, and each time twelve
    // new people join at the moment it starts, before playout has reconnected; every error
    // is handled as the binary handles it, and a fatal one fails the test with its source.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    gst::init().unwrap();
    let room = Room::fresh("m3-restart");
    let mut rows = Vec::new();
    for cycle in 0..3 {
        let cast: Vec<(String, Role, f64)> = (0..12)
            .map(|i| {
                let role = match i {
                    0 => Role::Presenter,
                    1..=7 => Role::Guest,
                    _ => Role::Caller,
                };
                (format!("c{cycle}-p{i:02}"), role, 400.0 + 200.0 * i as f64)
            })
            .collect();
        // People and desk at once.
        let people: Vec<Person> = cast
            .iter()
            .map(|(id, role, f)| Person::join(&room, id, *role, *f))
            .collect();
        let run = start(&room);
        let desk = run.desk.clone();
        let names: Vec<&str> = cast.iter().map(|(id, ..)| id.as_str()).collect();
        let t0 = Instant::now();
        let all = wait_for(&desk, Duration::from_secs(45), || {
            linked(&desk, &names) == names.len()
        });
        assert!(
            all.is_some(),
            "cycle {cycle}: the desk linked only {} of 12: {:?}",
            linked(&desk, &names),
            desk.status()
        );
        for (id, role, _) in &cast {
            if *role == Role::Caller {
                assert!(desk.set_on_air(id, true));
            }
        }
        let playout = wait_for(&desk, Duration::from_secs(20), || desk.playout().connected);
        stack::run_for(&desk, Duration::from_secs(3));
        let mix = desk.mix_stats();
        let programme = run.finish().chunks();
        let tail = last_seconds(&programme, 2.0);
        let missing: Vec<&str> = cast
            .iter()
            .filter(|(_, _, f)| ratio_db(tone_power(&tail, *f), 1.0) <= 60.0)
            .map(|(id, ..)| id.as_str())
            .collect();
        let row = serde_json::json!({
            "cycle": cycle,
            "all_linked_after_s": t0.elapsed().as_secs_f64().min(all.map_or(f64::MAX, |d| d.as_secs_f64())),
            "playout_back_after_s": playout.map(|d| d.as_secs_f64()),
            "missing_from_programme": missing,
            "mix": mix,
        });
        println!("{row}");
        rows.push(row);
        assert!(
            playout.is_some(),
            "cycle {cycle}: playout never reconnected"
        );
        assert!(
            missing.is_empty(),
            "cycle {cycle}: missing from the programme: {missing:?}"
        );
        // Everyone leaves with the desk; the next cycle's people are new.
        drop(people);
    }
    println!("{}", serde_json::json!({ "restarts": rows }));
}
