// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Counting what the desk holds: its elements and pads, the process's threads, and the CPU
//! its threads have used. A desk whose room has emptied must come back to what it held
//! before anyone joined (the staging run, 2026-10-25: after three shows it used about two
//! cores with the room empty).

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;

/// What a pipeline holds, counted through every bin inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Census {
    pub elements: usize,
    pub pads: usize,
}

/// Count every element and pad inside `pipeline`, at any depth. Elements a bin runs outside
/// itself (a `webrtcsink`'s per-session pipelines) are not inside it: the thread count and
/// [`cpu_share`] see those.
pub fn census(pipeline: &gst::Pipeline) -> Census {
    let mut c = Census::default();
    for el in pipeline.iterate_recurse().into_iter().flatten() {
        c.elements += 1;
        c.pads += el.pads().len();
    }
    c
}

/// The process's threads, from `/proc/self/status`; 0 where there is no such file.
pub fn threads() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Threads:"))
                .and_then(|n| n.trim().parse().ok())
        })
        .unwrap_or(0)
}

/// The process's threads by name (`/proc/self/task/*/comm`), counted: what a leak leaves.
pub fn thread_names() -> std::collections::BTreeMap<String, usize> {
    let mut m = std::collections::BTreeMap::new();
    if let Ok(dir) = std::fs::read_dir("/proc/self/task") {
        for t in dir.flatten() {
            if let Ok(name) = std::fs::read_to_string(t.path().join("comm")) {
                *m.entry(name.trim().to_owned()).or_insert(0) += 1;
            }
        }
    }
    m
}

/// The time on CPU of every live thread of the process, in nanoseconds, from each thread's
/// `schedstat` (exact, and in nanoseconds whatever the clock tick).
fn cpu_ns() -> u64 {
    let Ok(dir) = std::fs::read_dir("/proc/self/task") else {
        return 0;
    };
    dir.flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("schedstat")).ok())
        .filter_map(|s| {
            s.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
        })
        .sum()
}

/// The share of one core the process used over `window` (1.0 is one core busy), measured by
/// sleeping through it. Threads that end inside the window are not counted.
pub fn cpu_share(window: Duration) -> f64 {
    let a = cpu_ns();
    let t = std::time::Instant::now();
    std::thread::sleep(window);
    let b = cpu_ns();
    b.saturating_sub(a) as f64 / t.elapsed().as_nanos().max(1) as f64
}

/// Each live thread's time on CPU in nanoseconds, by task id, with its name.
fn cpu_by_task() -> std::collections::HashMap<String, (String, u64)> {
    let mut m = std::collections::HashMap::new();
    let Ok(dir) = std::fs::read_dir("/proc/self/task") else {
        return m;
    };
    for t in dir.flatten() {
        let name = std::fs::read_to_string(t.path().join("comm")).unwrap_or_default();
        let ns = std::fs::read_to_string(t.path().join("schedstat"))
            .ok()
            .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse().ok()))
            .unwrap_or(0);
        m.insert(
            t.file_name().to_string_lossy().into_owned(),
            (name.trim().to_owned(), ns),
        );
    }
    m
}

/// Where the CPU went over `window`, by thread name with its digits taken out (GStreamer
/// names streaming threads after their element and pad, `queue12:src`), as shares of one
/// core, the largest first: a profile without a profiler.
pub fn cpu_by_thread_kind(window: Duration) -> Vec<(String, f64)> {
    let a = cpu_by_task();
    let t = std::time::Instant::now();
    std::thread::sleep(window);
    let b = cpu_by_task();
    let secs = t.elapsed().as_nanos().max(1) as f64;
    let mut kinds = std::collections::BTreeMap::<String, f64>::new();
    for (tid, (name, ns)) in b {
        let before = a.get(&tid).map_or(0, |x| x.1);
        let kind: String = name.chars().filter(|c| !c.is_ascii_digit()).collect();
        *kinds.entry(kind).or_default() += ns.saturating_sub(before) as f64 / secs;
    }
    let mut v: Vec<(String, f64)> = kinds.into_iter().collect();
    v.sort_by(|x, y| y.1.total_cmp(&x.1));
    v
}
