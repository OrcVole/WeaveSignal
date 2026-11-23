// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio's canary (1.3, the rehearsal build plan's section 1.7, step R4): a live
//! desk and a rehearsal desk, each the real `weavesignal-desk` binary in a process of its own,
//! on one LiveKit and one playout. Ignored unless the stack is up: `just test-rehearsal`.
//!
//! - **Absent.** Three people in the rehearsal room and a cart on the rehearsal desk sound
//!   1,733 Hz, a frequency nothing else uses; the live presenter sends 440 Hz. Over 60 s the
//!   decoded listener mount holds 440 Hz and no 1,733 Hz above −70 dBFS.
//! - **Present.** A monitor on a rehearsal return hears 1,733 Hz in the same minute, so the tone
//!   really was there to leak.
//! - **No ports.** The rehearsal desk's process owns no TCP listener, and none of the stack's
//!   SRT, feed or sources ports; the live desk owns its SRT and sources ports. (WebRTC's UDP
//!   client sockets belong to both desks, by design.) The rehearsal desk is given the sources and
//!   feed passwords, and still binds nothing.
//! - **Refusals.** The rehearsal desk's socket refuses the requests that reach beyond its room.
//! - **Standing by.** The three move to the live room with tokens carrying
//!   `weavesignal.standby_until` ahead: their tone stays out of the mount until the press, and is
//!   in it after. A presenter without the attribute goes on air at joining, as in 1.2.
//! - **Cost.** The rehearsal desk's memory (RSS) and CPU at rest and with six people and a bed,
//!   against the plan's pass marks (section 1.9).

mod stack;

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use stack::{Person, Room, env, flac};
use weavesignal_desk::analysis::{Chunk, Tap};
use weavesignal_desk::config::{FEED_PASSWORD_ENV, SECRET_ENV, SOURCES_PASSWORD_ENV};
use weavesignal_proto::Role;

/// The canary: a frequency nothing else in the stack sends.
const CANARY: f64 = 1_733.0;
/// The live presenter's tone.
const LIVE: f64 = 440.0;
/// The presenter who joins live without the attribute.
const TWIN: f64 = 1_100.0;
/// How long the listener mount may run behind the desk (measured 12 to 15 s): windows on the
/// mount that follow a change start this long after it.
const MOUNT_LAG: Duration = Duration::from_secs(25);
/// Absent means below this.
const ABSENT_DBFS: f64 = -70.0;
/// Present means above this.
const PRESENT_DBFS: f64 = -50.0;

/// A desk binary running in a process of its own, stopped when dropped.
struct DeskProcess {
    child: Child,
    socket: PathBuf,
}

impl DeskProcess {
    fn start(name: &str, dir: &Path, toml: &str, secret: &str) -> DeskProcess {
        let config = dir.join(format!("{name}.toml"));
        std::fs::write(&config, toml).unwrap();
        let log = std::fs::File::create(dir.join(format!("{name}.log"))).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_weavesignal-desk"))
            .arg("--config")
            .arg(&config)
            .env(SECRET_ENV, secret)
            // Both desks are handed the passwords: the rehearsal desk must still bind nothing.
            .env(SOURCES_PASSWORD_ENV, "room-test-only")
            .env(FEED_PASSWORD_ENV, "room-test-only")
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "weavesignal_desk=info".into()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("the desk binary runs");
        let socket = dir.join(format!("{name}.sock"));
        DeskProcess { child, socket }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// One request on the control socket; `None` while it does not answer.
    fn ask(&self, req: &serde_json::Value) -> Option<serde_json::Value> {
        let mut s = UnixStream::connect(&self.socket).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        writeln!(s, "{req}").ok()?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    }

    fn status(&self) -> Option<serde_json::Value> {
        self.ask(&serde_json::json!({"op": "status"}))
    }

    /// Whether everyone named is at the desk with their voice linked.
    fn linked(&self, ids: &[&str]) -> bool {
        self.status().is_some_and(|s| {
            ids.iter().all(|id| {
                s["people"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|p| p["identity"] == *id && p["linked"] == true)
            })
        })
    }

    fn on_air(&self, id: &str) -> Option<bool> {
        let s = self.status()?;
        s["people"]
            .as_array()?
            .iter()
            .find(|p| p["identity"] == id)
            .and_then(|p| p["on_air"].as_bool())
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for DeskProcess {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

// ---------------------------------------------------------------------------------------
// Levels.

/// A tone's level in dBFS in each one-second block of `s`: Goertzel over a Hann window of
/// 48,000 samples (1 Hz bins), so 440 Hz's fourth harmonic (1,760 Hz) is 27 bins from the
/// canary and its leakage far below the floor. A sine of amplitude A gives |X| = A·N/4.
fn tone_dbfs_per_second(s: &[i16], freq: f64) -> Vec<f64> {
    const N: usize = 48_000;
    let w = 2.0 * std::f64::consts::PI * freq / 48_000.0;
    let coeff = 2.0 * w.cos();
    s.as_chunks::<N>()
        .0
        .iter()
        .map(|block| {
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for (i, x) in block.iter().enumerate() {
                let h = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos();
                let s0 = f64::from(*x) * h + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let mag = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt();
            let amp = 4.0 * mag / N as f64;
            20.0 * (amp.max(1e-9) / 32_768.0).log10()
        })
        .collect()
}

fn max(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied().unwrap_or(f64::NEG_INFINITY)
}

/// The whole signal's level in dBFS (RMS) over `s`.
fn rms_dbfs(s: &[i16]) -> f64 {
    if s.is_empty() {
        return f64::NEG_INFINITY;
    }
    let ms = s.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / s.len() as f64;
    20.0 * (ms.sqrt().max(1e-9) / 32_768.0).log10()
}

/// The longest wait between two recorded buffers from `from` on, in ms: a listener that
/// stalled shows here.
fn longest_gap_ms(chunks: &[Chunk], from: Instant, to: Option<Instant>) -> u128 {
    let t: Vec<Instant> = chunks
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| *t >= from && to.is_none_or(|e| *t < e))
        .collect();
    t.windows(2)
        .map(|w| w[1].duration_since(w[0]).as_millis())
        .max()
        .unwrap_or(0)
}

/// What the mount carried over a stretch: the canary, the live presenter, the twin, the whole.
fn mount_now(chunks: &[Chunk], from: Instant, to: Option<Instant>) -> serde_json::Value {
    let s = between(chunks, from, to);
    let r = |v: f64| (v * 10.0).round() / 10.0;
    serde_json::json!({
        "seconds": s.len() / 48_000,
        "rms_dbfs": r(rms_dbfs(&s)),
        "canary_max_dbfs": r(max(&tone_dbfs_per_second(&s, CANARY))),
        "live_median_dbfs": r(median(&tone_dbfs_per_second(&s, LIVE))),
        "twin_median_dbfs": r(median(&tone_dbfs_per_second(&s, TWIN))),
        "longest_gap_ms": longest_gap_ms(chunks, from, to),
    })
}

/// The samples recorded from `from` on (and before `to`, if given).
fn between(chunks: &[Chunk], from: Instant, to: Option<Instant>) -> Vec<i16> {
    chunks
        .iter()
        .filter(|(t, _)| *t >= from && to.is_none_or(|e| *t < e))
        .flat_map(|(_, s)| s.iter().copied())
        .collect()
}

// ---------------------------------------------------------------------------------------
// Listening.

/// A listener on the mount, as the room test's: Ogg Opus from icecast.server, decoded.
fn listener(url: &str) -> (gst::Pipeline, Tap) {
    let p = gst::Pipeline::with_name("listener");
    let src = gst::ElementFactory::make("souphttpsrc")
        .property("location", url)
        .property("is-live", true)
        .property("iradio-mode", false)
        .build()
        .unwrap();
    let demux = gst::ElementFactory::make("oggdemux").build().unwrap();
    let dec = gst::ElementFactory::make("opusdec").build().unwrap();
    p.add_many([&src, &demux, &dec]).unwrap();
    src.link(&demux).unwrap();
    let dec_weak = dec.downgrade();
    demux.connect_pad_added(move |_, pad| {
        if let Some(dec) = dec_weak.upgrade() {
            let _ = pad.link(&dec.static_pad("sink").unwrap());
        }
    });
    let tap = Tap::attach(&p, &dec, "listener").unwrap();
    (p, tap)
}

/// A monitor (the station admin's `listener` token, recorder grant) on one desk publisher in
/// `room`: what a rehearsal return carries.
fn monitor(room: &Room, in_room: &str, producer: &str) -> (gst::Pipeline, Arc<Mutex<Option<Tap>>>) {
    let p = gst::Pipeline::with_name("monitor");
    let src = gst::ElementFactory::make("livekitwebrtcsrc")
        .name("monitor-src")
        .build()
        .unwrap();
    let token = room.keys.monitor(in_room, "ear-canary", "", 600).unwrap();
    let sig = src.property::<glib::Object>("signaller");
    sig.set_property("ws-url", &room.url);
    sig.set_property("auth-token", &token);
    sig.set_property("room-name", in_room);
    sig.set_property("producer-peer-id", producer);
    let slot: Arc<Mutex<Option<Tap>>> = Arc::default();
    let (weak, slot2) = (p.downgrade(), slot.clone());
    src.connect_pad_added(move |_, pad| {
        let Some(p) = weak.upgrade() else {
            return;
        };
        let mut slot = slot2.lock().unwrap();
        if pad.name().starts_with("audio") && slot.is_none() {
            *slot = Some(Tap::attach_pad(&p, pad, "monitor-tap").unwrap());
        } else {
            let fs = gst::ElementFactory::make("fakesink").build().unwrap();
            p.add(&fs).unwrap();
            fs.sync_state_with_parent().unwrap();
            let _ = pad.link(&fs.static_pad("sink").unwrap());
        }
    });
    p.add(&src).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    (p, slot)
}

// ---------------------------------------------------------------------------------------
// Ports and cost, from /proc.

/// The socket inodes a process holds.
fn sockets_of(pid: u32) -> HashSet<u64> {
    let mut out = HashSet::new();
    if let Ok(rd) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_link(e.path())
                && let Some(n) = t
                    .to_string_lossy()
                    .strip_prefix("socket:[")
                    .and_then(|r| r.strip_suffix(']'))
                    .and_then(|n| n.parse().ok())
            {
                out.insert(n);
            }
        }
    }
    out
}

/// `(local port, state, inode)` of every socket in `/proc/net/<file>`.
fn net(file: &str) -> Vec<(u16, u8, u64)> {
    let text = std::fs::read_to_string(format!("/proc/net/{file}")).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let port = u16::from_str_radix(f.get(1)?.rsplit(':').next()?, 16).ok()?;
            let st = u8::from_str_radix(f.get(3)?, 16).ok()?;
            let inode = f.get(9)?.parse().ok()?;
            Some((port, st, inode))
        })
        .collect()
}

/// The TCP ports a process listens on, and the UDP ports it holds.
fn ports_of(pid: u32) -> (Vec<u16>, Vec<u16>) {
    let mine = sockets_of(pid);
    let tcp = ["tcp", "tcp6"]
        .iter()
        .flat_map(|f| net(f))
        .filter(|(_, st, ino)| *st == 0x0A && mine.contains(ino))
        .map(|(p, ..)| p)
        .collect();
    let udp = ["udp", "udp6"]
        .iter()
        .flat_map(|f| net(f))
        .filter(|(.., ino)| mine.contains(ino))
        .map(|(p, ..)| p)
        .collect();
    (tcp, udp)
}

fn rss_mib(pid: u32) -> f64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

fn cpu_ticks(pid: u32) -> u64 {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    // After the command's name in parentheses: state is field 3, utime 14, stime 15.
    let rest = s.rsplit_once(')').map_or("", |(_, r)| r);
    let f: Vec<&str> = rest.split_whitespace().collect();
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    n(11) + n(12)
}

fn clock_ticks() -> f64 {
    Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(100.0)
}

/// The process's CPU over `d`, as a share of one core (1.0 = one core), and its RSS after.
fn cost(pid: u32, d: Duration) -> (f64, f64) {
    let a = cpu_ticks(pid);
    std::thread::sleep(d);
    let b = cpu_ticks(pid);
    (
        (b - a) as f64 / clock_ticks() / d.as_secs_f64(),
        rss_mib(pid),
    )
}

fn threads_of(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/task")).map_or(0, |d| d.count())
}

// ---------------------------------------------------------------------------------------

fn desks_toml(room: &Room, carts: &Path, dir: &Path, sources_port: u16) -> (String, String) {
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
    let common = format!(
        r#"
[livekit]
url = "{url}"
api_key = "{key}"
room = "{{room}}"

# Tones are measured by their pitch: no input processing, and no delay to rebuild.
[inputs.presenter]
[inputs.dj]

[programme.delay]
target_ms = 0

[room]
poll_ms = 300

[carts]
dir = "{carts}"
"#,
        url = room.url,
        key = room.keys.api_key,
        carts = carts.display(),
    );
    let live = format!(
        "{}\n[programme]\nsrt_port = {srt_port}\n\n[desk]\nidentity = \"desk\"\nrealtime = false\nworkers = 2\n\n[control]\nsocket = \"{}\"\n\n[sources]\nport = {sources_port}\n",
        common.replace("{room}", &room.name),
        dir.join("live.sock").display()
    );
    let rehearsal = format!(
        "{}\n[desk]\nmode = \"rehearsal\"\nidentity = \"desk-rehearsal\"\nrealtime = false\nworkers = 2\n\n[control]\nsocket = \"{}\"\n",
        common.replace("{room}", &weavesignal_proto::rehearsal_room(&room.name)),
        dir.join("rehearsal.sock").display()
    );
    (live, rehearsal)
}

#[test]
#[ignore = "needs the loopback stack: just test-rehearsal"]
fn rehearsal_never_reaches_the_air_and_moved_people_stand_by_until_the_press() {
    gst::init().unwrap();
    let stack_dir = PathBuf::from(env("WEAVESIGNAL_STACK_DIR"));
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
    let sources_port: u16 = env("WEAVESIGNAL_ROOM_SOURCES_PORT").parse().unwrap();
    let feed_port: u16 = env("WEAVESIGNAL_ROOM_DESK_FEED_PORT").parse().unwrap();
    let listen_url = env("WEAVESIGNAL_ROOM_LISTENER_URL");
    let room = Room::fresh("r130-live");
    let rehearsal_room = Room {
        url: room.url.clone(),
        keys: room.keys.clone(),
        name: weavesignal_proto::rehearsal_room(&room.name),
    };
    let dir = stack_dir.join(format!("rehearsal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let carts = dir.join("carts");
    std::fs::create_dir_all(&carts).unwrap();
    let bed = flac(&carts, CANARY, 10.0);

    let (live_toml, rehearsal_toml) = desks_toml(&room, &carts, &dir, sources_port);
    let secret = room.keys.api_secret.clone();
    let mut live = DeskProcess::start("live", &dir, &live_toml, &secret);
    let mut rehearsal = DeskProcess::start("rehearsal", &dir, &rehearsal_toml, &secret);
    assert!(
        wait(Duration::from_secs(30), || live.status().is_some()
            && rehearsal.status().is_some()),
        "a desk's socket never answered (logs in {})",
        dir.display()
    );
    let (ls, rs) = (live.status().unwrap(), rehearsal.status().unwrap());
    assert_eq!(ls["desk_mode"], "live");
    assert_eq!(rs["desk_mode"], "rehearsal");
    let mut report = serde_json::Map::new();

    // At rest: nobody in the rehearsal room.
    std::thread::sleep(Duration::from_secs(5));
    let (cpu, rss) = cost(rehearsal.pid(), Duration::from_secs(10));
    let rest = serde_json::json!({
        "cpu_pct_of_one_core": (cpu * 1000.0).round() / 10.0,
        "rss_mib": (rss * 10.0).round() / 10.0,
        "threads": threads_of(rehearsal.pid()),
    });
    println!("rehearsal desk at rest: {rest}");
    report.insert("rehearsal_at_rest".into(), rest);

    // The people: the live presenter at 440 Hz, three rehearsing at the canary.
    let hamish = Person::join(&room, "hamish", Role::Presenter, LIVE);
    let mut rehearsing: Vec<Person> = [
        ("eilidh", Role::Presenter),
        ("gregor", Role::Guest),
        ("kirsty", Role::Guest),
    ]
    .iter()
    .map(|(id, role)| Person::join(&rehearsal_room, id, *role, CANARY))
    .collect();
    let three = ["eilidh", "gregor", "kirsty"];
    assert!(
        wait(Duration::from_secs(45), || rehearsal.linked(&three)),
        "the rehearsal desk did not link the three: {:?}",
        rehearsal.status()
    );
    assert!(
        wait(Duration::from_secs(45), || live.linked(&["hamish"])),
        "the live desk did not link the presenter"
    );
    assert!(
        wait(Duration::from_secs(20), || live
            .status()
            .is_some_and(|s| s["playout"]["connected"] == true)),
        "playout never connected to the live desk"
    );
    // Rehearsing guests are in the rehearsal desk's mix (by their role), unlike at the live desk.
    assert_eq!(rehearsal.on_air("gregor"), Some(true));
    // A bed at the canary on the rehearsal desk.
    let fired = rehearsal
        .ask(
            &serde_json::json!({"op": "cart_fire", "cart": "00000000-0000-4000-8000-000000000173",
            "file": bed, "gain_db": -6.0, "mode": "bed"}),
        )
        .unwrap();
    assert_eq!(fired["ok"], true, "the rehearsal desk's cart: {fired}");

    // The listener, and a monitor on Gregor's rehearsal return.
    let (lis, lis_tap) = {
        let mut got = None;
        let t0 = Instant::now();
        while got.is_none() && t0.elapsed() < Duration::from_secs(60) {
            let (p, tap) = listener(&listen_url);
            p.set_state(gst::State::Playing).unwrap();
            if wait(Duration::from_secs(5), || tap.buffers() > 50) {
                got = Some((p, tap));
            } else {
                p.set_state(gst::State::Null).unwrap();
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        got.expect("the listener mount never played")
    };
    let (mon, mon_slot) = monitor(&room, &rehearsal_room.name, "desk-rehearsal-mm-gregor");

    // The minute.
    let from = Instant::now();
    std::thread::sleep(Duration::from_secs(60));
    let to = Instant::now();
    let mount = between(&lis_tap.chunks(), from, Some(to));
    let canary = tone_dbfs_per_second(&mount, CANARY);
    let live_tone = tone_dbfs_per_second(&mount, LIVE);
    let returned = mon_slot
        .lock()
        .unwrap()
        .as_ref()
        .map(|t| between(&t.chunks(), from, Some(to)))
        .unwrap_or_default();
    let in_return = tone_dbfs_per_second(&returned, CANARY);
    let absent = serde_json::json!({
        "mount_seconds": canary.len(),
        "canary_in_mount_max_dbfs": (max(&canary) * 10.0).round() / 10.0,
        "live_tone_in_mount_median_dbfs": (median(&live_tone) * 10.0).round() / 10.0,
        "return_seconds": in_return.len(),
        "canary_in_rehearsal_return_median_dbfs": (median(&in_return) * 10.0).round() / 10.0,
    });
    println!("the canary minute: {absent}");
    report.insert(
        "mount_in_the_canary_minute".into(),
        mount_now(&lis_tap.chunks(), from, Some(to)),
    );
    report.insert("canary".into(), absent);

    // Ports, while both desks run with people in their rooms.
    let (r_tcp, r_udp) = ports_of(rehearsal.pid());
    let (l_tcp, l_udp) = ports_of(live.pid());
    let ports = serde_json::json!({
        "rehearsal_tcp_listening": r_tcp, "rehearsal_udp": r_udp,
        "live_tcp_listening": l_tcp, "live_udp": l_udp,
        "srt_port": srt_port, "sources_port": sources_port, "feed_port": feed_port,
    });
    println!("ports: {ports}");
    if let Ok(o) = Command::new("ss").args(["-ltnupH"]).output() {
        println!("ss -ltnupH:\n{}", String::from_utf8_lossy(&o.stdout));
    }
    report.insert("ports".into(), ports);

    // Refusals on the rehearsal desk's socket.
    for op in [
        serde_json::json!({"op": "dump"}),
        serde_json::json!({"op": "cart_plays", "after": 0}),
        serde_json::json!({"op": "mode", "mode": "auto"}),
        serde_json::json!({"op": "kit", "set": "s", "bind": "127.0.0.1", "outs": []}),
        serde_json::json!({"op": "now_playing", "title": "T"}),
    ] {
        let r = rehearsal.ask(&op).unwrap();
        assert_eq!(r["code"], "rehearsal", "{op}: {r}");
    }

    // Cost with six rehearsing and the bed sounding.
    let more: Vec<Person> = ["finlay", "iona", "callum"]
        .iter()
        .map(|id| Person::join(&rehearsal_room, id, Role::Guest, CANARY))
        .collect();
    let six = ["eilidh", "gregor", "kirsty", "finlay", "iona", "callum"];
    assert!(
        wait(Duration::from_secs(45), || rehearsal.linked(&six)),
        "the rehearsal desk did not link six: {:?}",
        rehearsal.status()
    );
    std::thread::sleep(Duration::from_secs(5));
    let sounding = rehearsal.status().unwrap()["carts"]["sounding"]
        .as_array()
        .map_or(0, Vec::len);
    let cost_from = Instant::now();
    let (cpu6, rss6) = cost(rehearsal.pid(), Duration::from_secs(10));
    report.insert(
        "mount_while_six_rehearse".into(),
        mount_now(&lis_tap.chunks(), cost_from, None),
    );
    let six_row = serde_json::json!({
        "people": 6, "carts_sounding": sounding,
        "cpu_pct_of_one_core": (cpu6 * 1000.0).round() / 10.0,
        "rss_mib": (rss6 * 10.0).round() / 10.0,
        "threads": threads_of(rehearsal.pid()),
    });
    println!("rehearsal desk with six and a bed: {six_row}");
    report.insert("rehearsal_six_and_a_bed".into(), six_row);
    more.into_iter().for_each(Person::leave);
    rehearsal.ask(&serde_json::json!({"op": "cart_stop_all"}));

    // The move: the three leave the rehearsal room and join the live room standing by.
    rehearsing.drain(..).for_each(Person::leave);
    let until = weavesignal_desk::room::now_ms() + 10 * 60 * 1000;
    let moved: Vec<Person> = [
        ("eilidh", Role::Presenter),
        ("gregor", Role::Guest),
        ("kirsty", Role::Guest),
    ]
    .iter()
    .map(|(id, role)| {
        let token = room
            .keys
            .participant_standing_by(&room.name, id, id, *role, 600, false, until)
            .unwrap();
        Person::join_with_token(&room, &room.name, id, CANARY, &token)
    })
    .collect();
    // A presenter without the attribute, for contrast: on air at joining, as in 1.2.
    let twin = Person::join(&room, "morag", Role::Presenter, TWIN);
    let all = ["eilidh", "gregor", "kirsty", "morag"];
    assert!(
        wait(Duration::from_secs(45), || live.linked(&all)),
        "the live desk did not link the moved people: {:?}",
        live.status()
    );
    for id in three {
        assert_eq!(live.on_air(id), Some(false), "{id} arrived on air");
    }
    assert_eq!(live.on_air("morag"), Some(true), "the twin stood by");
    // The listener hears the desk some seconds late (icecast.server's pre-buffer and the
    // listener process's cushion: about 12 to 15 s in the first runs), so the window starts
    // once the mount has caught up with the joins.
    std::thread::sleep(MOUNT_LAG);
    let from = Instant::now();
    std::thread::sleep(Duration::from_secs(20));
    let to = Instant::now();
    let before = between(&lis_tap.chunks(), from, Some(to));
    report.insert(
        "mount_before_press".into(),
        mount_now(&lis_tap.chunks(), from, Some(to)),
    );
    report.insert(
        "live_desk_before_press".into(),
        live.status().unwrap_or_default()["people"].clone(),
    );
    let canary_before = tone_dbfs_per_second(&before, CANARY);
    let twin_before = tone_dbfs_per_second(&before, TWIN);
    // The press, as the handover's.
    for id in three {
        let r = live
            .ask(&serde_json::json!({"op": "on_air", "identity": id, "on": true}))
            .unwrap();
        assert_eq!(r["ok"], true, "{id}: {r}");
    }
    let pressed = Instant::now();
    std::thread::sleep(MOUNT_LAG + Duration::from_secs(15));
    // When the canary first reached the mount after the press: the mount's lag.
    let since_press = tone_dbfs_per_second(&between(&lis_tap.chunks(), pressed, None), CANARY);
    let lag_s = since_press.iter().position(|l| *l > PRESENT_DBFS);
    let from = pressed + MOUNT_LAG;
    let after = between(&lis_tap.chunks(), from, None);
    report.insert(
        "mount_after_press".into(),
        mount_now(&lis_tap.chunks(), from, None),
    );
    report.insert(
        "live_desk_after_press".into(),
        live.status().unwrap_or_default()["people"].clone(),
    );
    report.insert(
        "live_desk_mix".into(),
        live.status().unwrap_or_default()["mix"].clone(),
    );
    let canary_after = tone_dbfs_per_second(&after, CANARY);
    let standing = serde_json::json!({
        "canary_in_mount_before_press_max_dbfs": (max(&canary_before) * 10.0).round() / 10.0,
        "twin_in_mount_before_press_median_dbfs": (median(&twin_before) * 10.0).round() / 10.0,
        "canary_in_mount_after_press_median_dbfs": (median(&canary_after) * 10.0).round() / 10.0,
        "seconds_before": canary_before.len(), "seconds_after": canary_after.len(),
        "mount_lag_after_press_s": lag_s,
    });
    println!("standing by: {standing}");
    report.insert("standing_by".into(), standing);
    let alive = (live.alive(), rehearsal.alive());

    drop((moved, twin, hamish));
    mon.set_state(gst::State::Null).unwrap();
    lis.set_state(gst::State::Null).unwrap();
    let report = serde_json::Value::Object(report);
    println!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, format!("{report:#}\n"));
    }

    // The verdicts, after the report is written.
    assert_eq!(alive, (true, true), "a desk stopped during the test");
    assert!(canary.len() >= 50, "under 50 s of the mount was heard");
    assert!(
        max(&canary) < ABSENT_DBFS,
        "the canary reached the listener mount: {:.1} dBFS",
        max(&canary)
    );
    assert!(
        median(&live_tone) > PRESENT_DBFS,
        "the live presenter is not in the mount, so the minute proves nothing: {:.1} dBFS",
        median(&live_tone)
    );
    assert!(
        in_return.len() >= 30 && median(&in_return) > PRESENT_DBFS,
        "the canary was not on the rehearsal return ({} s, {:.1} dBFS)",
        in_return.len(),
        median(&in_return)
    );
    assert!(
        r_tcp.is_empty(),
        "the rehearsal desk listens on TCP {r_tcp:?}"
    );
    for p in [srt_port, sources_port, feed_port, 7313, 7317, 7318] {
        assert!(
            !r_udp.contains(&p) && !r_tcp.contains(&p),
            "the rehearsal desk holds port {p}"
        );
    }
    assert!(
        l_udp.contains(&srt_port),
        "the live desk's SRT port is not its own: {l_udp:?}"
    );
    assert!(
        l_tcp.contains(&sources_port),
        "the live desk's sources port is not its own: {l_tcp:?}"
    );
    assert!(sounding >= 1, "the bed was not sounding while measured");
    assert!(
        max(&canary_before) < ABSENT_DBFS,
        "a moved person was on air before the press: {:.1} dBFS",
        max(&canary_before)
    );
    assert!(
        median(&twin_before) > PRESENT_DBFS,
        "the presenter without the attribute was not on air at joining"
    );
    assert!(
        median(&canary_after) > PRESENT_DBFS,
        "the moved people were not on air after the press: {:.1} dBFS",
        median(&canary_after)
    );
    let _ = std::fs::remove_dir_all(&carts);
}

// ---------------------------------------------------------------------------------------
// The move through the control plane (R9): the real control plane, with PostgreSQL, drives
// both desks. Three guests rehearse on rehearsal links; one is taken out of the rehearsal's
// mix; Move to the live studio sends each page a live pass on its event stream; they join the
// live room standing by; the digit of one of them puts that one on air, and the control plane
// puts on the other who was on in rehearsal (the opening state), never the one who was off.

/// The control plane binary in a process of its own, stopped when dropped.
struct ControlProcess {
    child: Child,
    base: String,
    key: String,
}

impl Drop for ControlProcess {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        std::thread::sleep(Duration::from_millis(500));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into()
}

impl ControlProcess {
    /// A request with a bearer; the status and the body as JSON (null when there is none).
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {bearer}");
        let a = agent();
        let res = match (method, body) {
            ("GET", _) => a.get(&url).header("Authorization", &auth).call(),
            ("PUT", b) => a
                .put(&url)
                .header("Authorization", &auth)
                .send_json(b.unwrap_or_else(|| serde_json::json!({}))),
            ("PATCH", b) => a
                .patch(&url)
                .header("Authorization", &auth)
                .send_json(b.unwrap_or_else(|| serde_json::json!({}))),
            (_, b) => a
                .post(&url)
                .header("Authorization", &auth)
                .send_json(b.unwrap_or_else(|| serde_json::json!({}))),
        };
        let mut res = res.unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        let code = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        (code, serde_json::from_str(&text).unwrap_or_default())
    }

    /// Whether the rehearsal desk is linked. A refused connection (the control plane is still
    /// starting: it runs its migrations first) is "not yet", not a panic.
    fn rehearsal_desk_linked(&self, bearer: &str) -> bool {
        let res = agent()
            .get(&format!("{}/api/v1/rehearsal", self.base))
            .header("Authorization", &format!("Bearer {bearer}"))
            .call();
        let Ok(mut res) = res else { return false };
        let text = res.body_mut().read_to_string().unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(&text)
            .map(|v| v["desk"]["linked"] == true)
            .unwrap_or(false)
    }

    fn ok(&self, method: &str, path: &str, body: serde_json::Value) -> serde_json::Value {
        let (code, v) = self.call(method, path, &self.key.clone(), Some(body));
        assert!((200..300).contains(&code), "{method} {path}: {code} {v}");
        v
    }

    /// The first `me` event on a pass's event stream that carries a move.
    fn move_of(&self, pass: &str) -> serde_json::Value {
        let res = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .new_agent()
            .get(&format!("{}/api/v1/pass/events", self.base))
            .header("Authorization", &format!("Bearer {pass}"))
            .call()
            .expect("the event stream");
        let mut reader = BufReader::new(res.into_body().into_reader());
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if let Some(data) = line.trim_end().strip_prefix("data: ") {
                let me: serde_json::Value = serde_json::from_str(data).unwrap();
                if !me["move"].is_null() {
                    return me;
                }
            }
            line.clear();
        }
        panic!("the event stream ended without a move");
    }
}

/// `secs` since the Unix epoch as `YYYY-MM-DDTHH:MM:SS+00:00[UTC]`.
fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00[UTC]",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The token a `livekitwebrtcsink` may publish with, made from the one the control plane gave a
/// page. A page's token may subscribe (it hears its return) and a sink whose token may subscribe
/// sends no media (M1), so this keeps the room, identity, name and the standing-by attribute and
/// takes the subscribing away.
fn publisher_token(room: &Room, page_token: &str, role: Role) -> String {
    let c = room
        .keys
        .verify(page_token)
        .expect("the page's token verifies");
    assert_eq!(
        c.video.can_subscribe,
        Some(true),
        "a page's token may subscribe"
    );
    let in_room = c.video.room.clone().expect("the token names a room");
    match c
        .attributes
        .get(weavesignal_proto::STANDBY_ATTRIBUTE)
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(until) => room
            .keys
            .participant_standing_by(&in_room, &c.sub, &c.name, role, 600, false, until),
        None => room
            .keys
            .participant_with(&in_room, &c.sub, &c.name, role, 600, false),
    }
    .unwrap()
}

#[test]
#[ignore = "needs the loopback stack with PostgreSQL and the control plane: just test-rehearsal"]
fn the_move_brings_people_to_the_live_room_standing_by_and_the_digit_puts_them_on() {
    gst::init().unwrap();
    let stack_dir = PathBuf::from(env("WEAVESIGNAL_STACK_DIR"));
    let sources_port: u16 = env("WEAVESIGNAL_ROOM_SOURCES_PORT").parse().unwrap();
    let control_port: u16 = env("WEAVESIGNAL_ROOM_CONTROL_PORT").parse().unwrap();
    let database = env("WEAVESIGNAL_ROOM_DATABASE_URL");
    let control_bin = env("WEAVESIGNAL_CONTROL_BIN");
    let room = Room::fresh("r130-move");
    let rehearsal_room = Room {
        url: room.url.clone(),
        keys: room.keys.clone(),
        name: weavesignal_proto::rehearsal_room(&room.name),
    };
    let dir = stack_dir.join(format!("move-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let carts = dir.join("carts");
    std::fs::create_dir_all(&carts).unwrap();
    let (live_toml, rehearsal_toml) = desks_toml(&room, &carts, &dir, sources_port);
    let secret = room.keys.api_secret.clone();
    let live = DeskProcess::start("live", &dir, &live_toml, &secret);
    let rehearsal = DeskProcess::start("rehearsal", &dir, &rehearsal_toml, &secret);
    assert!(
        wait(Duration::from_secs(30), || live.status().is_some()
            && rehearsal.status().is_some()),
        "a desk's socket never answered (logs in {})",
        dir.display()
    );

    // The control plane: both desk links, the rehearsal studio on, the station key.
    let key = "move-test-only-station-key";
    let http = room.url.replace("ws://", "http://");
    let control_toml = format!(
        r#"[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = {control_port}
[room]
public_url = "{url}"
api_url = "{http}"
api_key = "{api_key}"
name = "{name}"
[desk]
socket = "{live}"
poll_ms = 250
[rehearsal]
enabled = true
socket = "{rehearsal}"
[log]
format = "pretty"
filter = "info"
"#,
        url = room.url,
        api_key = room.keys.api_key,
        name = room.name,
        live = dir.join("live.sock").display(),
        rehearsal = dir.join("rehearsal.sock").display(),
    );
    std::fs::write(dir.join("control.toml"), control_toml).unwrap();
    let child = Command::new(&control_bin)
        .arg("--config")
        .arg(dir.join("control.toml"))
        .arg("serve")
        .env("WEAVESIGNAL_LIVEKIT_API_SECRET", &secret)
        .env("WEAVESIGNAL_CONTROL_ADMIN_TOKEN", key)
        .env("WEAVESIGNAL_DATABASE_URL", &database)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("control.log")).unwrap())
        .spawn()
        .expect("the control plane runs");
    let control = ControlProcess {
        child,
        base: format!("http://127.0.0.1:{control_port}"),
        key: key.into(),
    };
    assert!(
        wait(Duration::from_secs(60), || control
            .rehearsal_desk_linked(key)),
        "the control plane never linked the rehearsal desk (log in {})",
        dir.display()
    );

    // Steeped, at Full, airing in ten minutes for an hour.
    control.ok(
        "PUT",
        "/api/v1/shows/steeped",
        serde_json::json!({"name": "Steeped", "colour_slot": 2}),
    );
    control.ok(
        "PUT",
        "/api/v1/shows/steeped/step",
        serde_json::json!({"step": "full"}),
    );
    control.ok(
        "PUT",
        "/api/v1/shows/steeped/series/1",
        serde_json::json!({}),
    );
    control.ok(
        "POST",
        "/api/v1/shows/steeped/series/1/episodes",
        serde_json::json!({"count": 1}),
    );
    let start_s = weavesignal_desk::room::now_ms() / 1000 + 600;
    control.ok(
        "PATCH",
        "/api/v1/shows/steeped/series/1/episodes/1",
        serde_json::json!({"air_at": utc(start_s), "minutes": 60}),
    );
    control.ok(
        "POST",
        "/api/v1/rehearsal/open",
        serde_json::json!({"show": "steeped"}),
    );

    // Three guests on rehearsal links, each through its pass's token, at the canary.
    let names = ["eilidh", "gregor", "kirsty"];
    let mut passes = Vec::new();
    let mut ids = Vec::new();
    let mut rehearsing = Vec::new();
    for n in names {
        let inv = control.ok(
            "POST",
            "/api/v1/invites",
            serde_json::json!({"role": "guest", "name": n, "studio": "rehearsal", "show": "steeped"}),
        );
        let pass = inv["pass"].as_str().unwrap().to_owned();
        let (code, t) = control.call(
            "POST",
            "/api/v1/pass/token",
            &pass,
            Some(serde_json::json!({})),
        );
        assert_eq!(code, 201, "{t}");
        assert_eq!(
            t["room"],
            rehearsal_room.name.as_str(),
            "a rehearsal link joins the rehearsal room"
        );
        let id = t["identity"].as_str().unwrap().to_owned();
        rehearsing.push(Person::join_with_token(
            &rehearsal_room,
            &rehearsal_room.name,
            &id,
            CANARY,
            &publisher_token(&room, t["token"].as_str().unwrap(), Role::Guest),
        ));
        passes.push(pass);
        ids.push(id);
    }
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert!(
        wait(Duration::from_secs(45), || rehearsal.linked(&id_refs)),
        "the rehearsal desk did not link the three: {:?}",
        rehearsal.status()
    );
    // Kirsty out of the rehearsal's mix; Eilidh and Gregor in it.
    control.ok(
        "POST",
        "/api/v1/rehearsal/commands",
        serde_json::json!({"op": "on_air", "identity": ids[2], "on": false}),
    );
    assert_eq!(rehearsal.on_air(&ids[2]), Some(false));

    // Move to the live studio.
    let moved = control.ok("POST", "/api/v1/rehearsal/move", serde_json::json!({}));
    assert_eq!(moved["moved"].as_array().unwrap().len(), 3, "{moved}");
    let mut on: Vec<&str> = vec![&ids[0], &ids[1]];
    on.sort();
    assert_eq!(moved["opening"]["on"], serde_json::json!(on), "{moved}");
    assert!(
        moved["opening"]["host"].is_null(),
        "no host was moved: {moved}"
    );
    // The rehearsal links buy nothing now.
    let (code, _) = control.call(
        "POST",
        "/api/v1/pass/token",
        &passes[0],
        Some(serde_json::json!({})),
    );
    assert_eq!(
        code, 409,
        "a rehearsal link still bought a token after the move"
    );

    // Each page: the move on its event stream, then the live room on the new pass.
    rehearsing.drain(..).for_each(Person::leave);
    let mut joined = Vec::new();
    for (pass, id) in passes.iter().zip(&ids) {
        let me = control.move_of(pass);
        assert_eq!(me["studio"], "rehearsal");
        let live_pass = me["move"]["pass"].as_str().unwrap();
        let (code, t) = control.call(
            "POST",
            "/api/v1/pass/token",
            live_pass,
            Some(serde_json::json!({})),
        );
        assert_eq!(code, 201, "{t}");
        assert_eq!(t["room"], room.name.as_str());
        assert_eq!(t["identity"], id.as_str());
        joined.push(Person::join_with_token(
            &room,
            &room.name,
            id,
            CANARY,
            &publisher_token(&room, t["token"].as_str().unwrap(), Role::Guest),
        ));
    }
    assert!(
        wait(Duration::from_secs(45), || live.linked(&id_refs)),
        "the live desk did not link the moved people: {:?}",
        live.status()
    );
    // Standing by, and still after a few looks of the control plane.
    std::thread::sleep(Duration::from_secs(4));
    let before: Vec<Option<bool>> = ids.iter().map(|id| live.on_air(id)).collect();

    // The digit: Gregor on air by a press at the desk.
    let (code, v) = control.call(
        "POST",
        "/api/v1/desk/commands",
        key,
        Some(serde_json::json!({"op": "on_air", "identity": ids[1], "on": true})),
    );
    assert_eq!(code, 200, "{v}");
    let eilidh_on = wait(Duration::from_secs(10), || {
        live.on_air(&ids[0]) == Some(true)
    });
    std::thread::sleep(Duration::from_secs(3));
    let after: Vec<Option<bool>> = ids.iter().map(|id| live.on_air(id)).collect();
    let report = serde_json::json!({
        "before_press": before, "after_press": after, "opening": moved["opening"],
        "eilidh_on_within_10_s": eilidh_on,
    });
    println!("the move: {report}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path.replace(".json", "-move.json"), format!("{report:#}\n"));
    }
    drop(joined);
    drop(control);
    assert_eq!(
        before,
        [Some(false), Some(false), Some(false)],
        "a moved person was on air before the press"
    );
    assert_eq!(
        after,
        [Some(true), Some(true), Some(false)],
        "after the digit: Gregor by the press, Eilidh by the opening, Kirsty (off in rehearsal) standing by"
    );
    let _ = std::fs::remove_dir_all(&carts);
}
