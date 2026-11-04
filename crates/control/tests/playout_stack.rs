// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The playout layer on the real stack (M4): `tests/playout/run-playout.sh` starts Liquidsoap
//! (playout and the listener process), a stand-in desk sending a 1 kHz tone, and PostgreSQL;
//! this starts the control plane, builds a library of three tracks (600, 700 and 800 Hz), a
//! clock and a log, and follows one listener connection throughout:
//!
//! - the desk is heard; it falls silent and after the dead-air time automation plays the log,
//!   with a dead-air notice; it sounds again and is back on air;
//! - playout is killed and restarted: the listener stays connected and hears the station's
//!   fallback (440 Hz), then the desk again;
//! - in automation mode the log plays though the desk sounds;
//! - the per-play log has each track with its ISRC and the listener; the audience report has
//!   a session of at least a minute; now playing names the track and what comes next, until
//!   the station is a US statutory webcaster, when what comes next is never sent;
//! - the legal logger records while the Jurisdiction keeps recordings, each hour registered
//!   for retention, and stops when it keeps none.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STATION: &str = "8d0c2f4e-6a1b-4c3d-9e5f-7a8b9c0d1e2f";
const ADMIN: &str = "playout-stack-test";
/// The frequencies a listener can tell apart: the fallback, the three tracks, the desk.
const TONES: [f32; 5] = [440.0, 600.0, 700.0, 800.0, 1000.0];

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set: run tests/playout/run-playout.sh"))
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Api {
    base: String,
    agent: ureq::Agent,
}

impl Api {
    fn get(&self, path: &str) -> Option<serde_json::Value> {
        let mut r = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {ADMIN}"))
            .call()
            .ok()?;
        r.body_mut().read_json().ok()
    }

    fn text(&self, path: &str) -> String {
        let mut r = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {ADMIN}"))
            .call()
            .unwrap();
        r.body_mut().read_to_string().unwrap()
    }

    fn send(&self, method: &str, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {ADMIN}");
        let mut r = match method {
            "PUT" => self
                .agent
                .put(&url)
                .header("Authorization", &auth)
                .send_json(body),
            _ => self
                .agent
                .post(&url)
                .header("Authorization", &auth)
                .send_json(body),
        }
        .expect("the control plane answers");
        let code = r.status().as_u16();
        (code, r.body_mut().read_json().unwrap_or_default())
    }

    fn upload(&self, path: &str, bytes: Vec<u8>) -> (u16, serde_json::Value) {
        let mut r = self
            .agent
            .put(format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {ADMIN}"))
            .send(&bytes[..])
            .expect("the control plane answers");
        let code = r.status().as_u16();
        (code, r.body_mut().read_json().unwrap_or_default())
    }
}

/// One Opus packet heard: when, how loud, and which of [`TONES`] it mostly is (if any).
#[derive(Clone, Copy, Debug)]
struct Heard {
    at: Instant,
    rms: f32,
    tone: Option<f32>,
}

/// Goertzel power of `f` in the first channel, as a share of the packet's energy.
fn share(s: &[f32], channels: usize, f: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * f / 48_000.0;
    let (mut s1, mut s2, mut energy, mut n) = (0f32, 0f32, 0f32, 0usize);
    for frame in s.chunks(channels) {
        let x = frame[0];
        energy += x * x;
        n += 1;
        let s0 = x + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
    if energy > 0.0 {
        (2.0 * power / (n.max(1) as f32 * energy)).min(1.0)
    } else {
        0.0
    }
}

/// One connection, as a player makes it; `ended` is set if the stream ever ends.
fn listen(url: &str, heard: Arc<Mutex<Vec<Heard>>>, ended: Arc<AtomicBool>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(None)
        .build()
        .into();
    let res = loop {
        // The mount appears some seconds after the listener process starts.
        match agent.get(url).call() {
            Ok(r) => break r,
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    let mut body = res.into_body().into_reader();
    let mut decoder: Option<opus_decoder::OpusDecoder> = None;
    let mut channels = 2usize;
    let mut pcm = vec![0f32; 5760 * 2];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let mut head = [0u8; 27];
        if body.read_exact(&mut head).is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }
        assert_eq!(&head[..4], b"OggS", "the stream is Ogg");
        let mut table = vec![0u8; usize::from(head[26])];
        body.read_exact(&mut table).unwrap();
        let mut data = vec![0u8; table.iter().map(|&l| usize::from(l)).sum()];
        if body.read_exact(&mut data).is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }
        let at = Instant::now();
        let mut off = 0;
        for &lace in &table {
            carry.extend_from_slice(&data[off..off + usize::from(lace)]);
            off += usize::from(lace);
            if lace == 255 {
                continue;
            }
            let packet = std::mem::take(&mut carry);
            if packet.starts_with(b"OpusHead") {
                channels = usize::from(packet[9]);
                decoder = Some(opus_decoder::OpusDecoder::new(48_000, channels).unwrap());
                continue;
            }
            if packet.starts_with(b"OpusTags") {
                continue;
            }
            let Some(d) = decoder.as_mut() else { continue };
            let n = d.decode_float(&packet, &mut pcm, false).unwrap_or(0);
            let s = &pcm[..n * channels];
            let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
            let tone = TONES
                .iter()
                .map(|f| (*f, share(s, channels, *f)))
                .filter(|(_, sh)| *sh > 0.6)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(f, _)| f);
            heard.lock().unwrap().push(Heard { at, rms, tone });
        }
    }
}

fn wait<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Wait until a second of one of `tones` is heard after `since`; returns when it began.
fn until_tone(
    heard: &Mutex<Vec<Heard>>,
    since: Instant,
    tones: &[f32],
    what: &str,
    secs: u64,
) -> Instant {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        {
            let h = heard.lock().unwrap();
            let mut run: Option<Instant> = None;
            let mut count = 0;
            for p in h.iter().filter(|p| p.at >= since) {
                if p.tone.is_some_and(|t| tones.contains(&t)) && p.rms > 0.01 {
                    run.get_or_insert(p.at);
                    count += 1;
                    // 50 packets of 20 ms: a second of it.
                    if count >= 50 {
                        return run.unwrap();
                    }
                } else {
                    run = None;
                    count = 0;
                }
            }
            if Instant::now() >= deadline {
                // Say what was heard instead.
                let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
                for p in h.iter().filter(|p| p.at >= since) {
                    *seen
                        .entry(match p.tone {
                            Some(t) => format!("{t} Hz"),
                            None if p.rms > 0.01 => "other sound".into(),
                            None => "silence".into(),
                        })
                        .or_default() += 1;
                }
                panic!("timed out waiting for {what}; heard (20 ms packets): {seen:?}");
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "needs the stack: tests/playout/run-playout.sh"]
fn the_playout_layer_on_the_stack() {
    if std::env::var("WEAVESIGNAL_PLAYOUT_STACK").is_err() {
        eprintln!("skipped: run tests/playout/run-playout.sh");
        return;
    }
    let started = Instant::now();
    let run = std::path::PathBuf::from(env("WEAVESIGNAL_PLAYOUT_RUN"));
    let bin = env("WEAVESIGNAL_CONTROL_BIN");
    let port = env("WEAVESIGNAL_PLAYOUT_CONTROL_PORT");
    let listeners_port = env("WEAVESIGNAL_PLAYOUT_LISTENERS_PORT");
    let config = run.join("control.toml");
    std::fs::write(
        &config,
        format!(
            "[station]\nid = \"{STATION}\"\nname = \"Kettle FM\"\n\n[ports]\ncontrol = {port}\nlisteners = {listeners_port}\n\n\
             [playout]\nsocket = \"{sock}\"\nlisteners_socket = \"{lsock}\"\nlibrary_dir = \"{lib}\"\npoll_ms = 100\n\n\
             [carts]\nmedia_dir = \"{carts}\"\nworker = \"{bin}\"\n\n\
             [studio]\nlistener_delay_ms = 0\n\n\
             [retention]\nrecordings_dir = \"{rec}\"\n\n\
             [log]\nformat = \"json\"\n",
            sock = run.join("sock/playout.sock").display(),
            lsock = run.join("sock/listeners.sock").display(),
            lib = run.join("library").display(),
            carts = run.join("carts").display(),
            rec = run.join("recordings").display(),
        ),
    )
    .unwrap();
    let log = std::fs::File::create(run.join("control.log")).unwrap();
    let _control = Kill(
        Command::new(&bin)
            .args(["--config", config.to_str().unwrap(), "serve"])
            .env(
                "WEAVESIGNAL_DATABASE_URL",
                env("WEAVESIGNAL_TEST_DATABASE_URL"),
            )
            .env("WEAVESIGNAL_CONTROL_ADMIN_TOKEN", ADMIN)
            .env_remove("RUST_LOG")
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let api = Api {
        base: format!("http://127.0.0.1:{port}"),
        agent: ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into(),
    };
    wait("the control plane", 60, || api.get("/api/v1/playout"));

    // Retention keeps programme recordings and the per-play log (a licensed UK station).
    let (code, body) = api.send(
        "PUT",
        "/api/v1/retention",
        serde_json::json!({"preset": "uk_ofcom", "by": "playout stack test"}),
    );
    assert_eq!(code, 200, "{body}");

    // The library: three tracks of 8 s at 600, 700 and 800 Hz, through the media worker.
    let mut tracks = Vec::new();
    for (i, f) in [600.0, 700.0, 800.0].iter().enumerate() {
        let wav = weavesignal_media::wav_sine(48_000, 2, 8.0, *f, 0.3);
        let (code, t) = api.upload(
            &format!(
                "/api/v1/library/tracks?title=Tone%20{f}&artist=Band%20{i}&album=Tones&label=Test%20Label&category=a&isrc=GBAAA2600{i:03}"
            ),
            wav,
        );
        assert_eq!(code, 201, "{t}");
        tracks.push(t);
    }
    let clock = "11111111-2222-4333-8444-555555555555";
    let (code, body) = api.send(
        "PUT",
        &format!("/api/v1/schedule/clocks/{clock}"),
        serde_json::json!({"name": "Tones", "slots": [{"category": "a"}]}),
    );
    assert_eq!(code, 200, "{body}");
    let settings = |mode: &str| {
        serde_json::json!({
            "time_zone": "UTC", "artist_separation_min": 0, "title_separation_min": 0,
            "album_separation_min": 0, "mode": mode, "default_clock": clock
        })
    };
    let (code, body) = api.send("PUT", "/api/v1/playout/settings", settings("live"));
    assert_eq!(code, 200, "{body}");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let (code, body) = api.send(
        "POST",
        "/api/v1/schedule/generate",
        serde_json::json!({"to_ms": now_ms + 3 * 3_600_000}),
    );
    assert_eq!(code, 200, "{body}");
    assert!(body["items"].as_u64().unwrap() > 100, "{body}");

    // One listener, for the whole test.
    let heard = Arc::new(Mutex::new(Vec::<Heard>::new()));
    let ended = Arc::new(AtomicBool::new(false));
    let (h, e) = (heard.clone(), ended.clone());
    let url = format!("http://127.0.0.1:{listeners_port}/programme.opus");
    std::thread::spawn(move || listen(&url, h, e));
    let desk_first = until_tone(&heard, started, &[1000.0], "the desk on air", 90);
    eprintln!("desk heard {:?} after the start", desk_first - started);

    // Dead air: the desk falls silent; after 4 s automation plays the log.
    let quiet = run.join("desk-quiet");
    std::fs::write(&quiet, b"").unwrap();
    let silenced = Instant::now();
    let auto_at = until_tone(
        &heard,
        silenced,
        &[600.0, 700.0, 800.0],
        "automation after dead air",
        40,
    );
    let dead_air_s = (auto_at - silenced).as_secs_f64();
    eprintln!("dead air: automation heard {dead_air_s:.1} s after the desk fell silent");
    let notices = |kind: &str| {
        api.get("/api/v1/studio")
            .and_then(|v| v["notices"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .any(|n| n["kind"] == kind)
    };
    wait("the dead-air notice", 10, || {
        notices("playout.dead_air").then_some(())
    });
    assert_eq!(
        api.get("/api/v1/playout").unwrap()["playout"]["on_air"],
        "automation"
    );

    // The per-play log: a track with its ISRC and the listener counted.
    let play = wait("a track in the per-play log with its listener", 40, || {
        let v = api.get("/api/v1/reports/plays")?;
        v["plays"]
            .as_array()?
            .iter()
            .find(|p| p["kind"] == "track" && p["listeners"].as_u64() >= Some(1))
            .cloned()
    });
    assert!(
        play["isrc"].as_str().unwrap().starts_with("GBAAA2600"),
        "{play}"
    );
    assert_eq!(play["album"], "Tones");
    // Now playing: the track, and what comes next.
    let np = wait("now playing to name a track", 20, || {
        let v = api.get("/api/v1/now-playing")?;
        (v["now"]["kind"] == "track").then_some(v)
    });
    assert_eq!(np["up_next_shown"], true, "{np}");
    assert!(!np["next"].as_array().unwrap().is_empty(), "{np}");

    // The desk sounds again: it is back on air.
    std::fs::remove_file(&quiet).unwrap();
    let back = Instant::now();
    until_tone(&heard, back, &[1000.0], "the desk back after dead air", 20);
    wait("the back-on-air notice", 10, || {
        notices("playout.dead_air_end").then_some(())
    });

    // Playout killed and restarted under the listener: the fallback, then the desk.
    let done = run.join("playout-restarted");
    let _ = std::fs::remove_file(&done);
    std::fs::write(run.join("restart-playout"), b"").unwrap();
    let fault = Instant::now();
    let fallback_at = until_tone(
        &heard,
        fault,
        &[440.0],
        "the station's fallback while playout is down",
        40,
    );
    wait("the restart", 60, || done.exists().then_some(()));
    let desk_back = until_tone(
        &heard,
        fallback_at,
        &[1000.0],
        "the desk after playout's restart",
        60,
    );
    eprintln!(
        "playout killed: fallback heard after {:.1} s, the desk again after {:.1} s",
        (fallback_at - fault).as_secs_f64(),
        (desk_back - fault).as_secs_f64()
    );
    assert!(
        !ended.load(Ordering::SeqCst),
        "the listener's connection never ended"
    );

    // Automation mode: the log plays though the desk sounds.
    let (code, body) = api.send("PUT", "/api/v1/playout/settings", settings("auto"));
    assert_eq!(code, 200, "{body}");
    let auto = Instant::now();
    until_tone(
        &heard,
        auto,
        &[600.0, 700.0, 800.0],
        "automation in auto mode",
        30,
    );

    // The legal logger: recording, its hour registered for retention.
    let rec = wait("a logger recording with sound in it", 60, || {
        let v = api.get("/api/v1/logger")?;
        let r = v["recordings"].as_array()?.first()?.clone();
        (v["on"] == true && r["bytes"].as_u64()? > 10_000).then_some(r)
    });
    let registered = wait("the logger hour registered for retention", 30, || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = sqlx::PgPool::connect(&env("WEAVESIGNAL_TEST_DATABASE_URL"))
                .await
                .ok()?;
            let (n,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM recording WHERE kind = 'programme' AND file LIKE 'logger/%'",
            )
            .fetch_one(&pool)
            .await
            .ok()?;
            (n > 0).then_some(n)
        })
    });
    eprintln!(
        "logger: {} ({} bytes), {registered} recording(s) registered",
        rec["name"], rec["bytes"]
    );

    // The audience: a session of at least a minute, and listener hours.
    let aud = wait("a listener session of at least a minute", 90, || {
        let v = api.get("/api/v1/reports/audience")?;
        (v["sessions"].as_u64()? >= 1).then_some(v)
    });
    assert!(aud["listener_hours"].as_f64().unwrap() > 0.0, "{aud}");
    assert!(aud["tracks"].as_u64().unwrap() >= 1, "{aud}");
    let csv = api.text("/api/v1/reports/plays?format=csv");
    assert!(csv.starts_with("started_at,ended_at,kind,title"), "{csv}");
    assert!(csv.contains("GBAAA2600"), "{csv}");
    let sx = api.text("/api/v1/reports/plays?format=soundexchange");
    assert!(sx.lines().count() >= 2, "{sx}");

    // A US statutory webcaster: never what comes next; and a preset keeping no recordings stops
    // the logger.
    let (code, body) = api.send(
        "PUT",
        "/api/v1/retention",
        serde_json::json!({"preset": "us_webcaster", "by": "playout stack test"}),
    );
    assert_eq!(code, 200, "{body}");
    let np = wait("now playing without what comes next", 20, || {
        let v = api.get("/api/v1/now-playing")?;
        (v["up_next_shown"] == false && v.get("next").is_none()).then_some(v)
    });
    let logger_us = api.get("/api/v1/logger").unwrap();
    let (code, body) = api.send(
        "PUT",
        "/api/v1/retention",
        serde_json::json!({"preset": "none", "by": "playout stack test"}),
    );
    assert_eq!(code, 200, "{body}");
    wait("the logger to stop under No retention", 30, || {
        (api.get("/api/v1/logger")?["on"] == false).then_some(())
    });
    assert!(
        !ended.load(Ordering::SeqCst),
        "the listener's connection never ended"
    );

    let report = serde_json::json!({
        "desk_first_heard_s": (desk_first - started).as_secs_f64(),
        "dead_air_to_automation_s": dead_air_s,
        "playout_killed_fallback_heard_s": (fallback_at - fault).as_secs_f64(),
        "playout_killed_desk_back_s": (desk_back - fault).as_secs_f64(),
        "listener_connections": 1,
        "audience": {"sessions": aud["sessions"], "listener_hours": aud["listener_hours"], "tracks_per_hour": aud["tracks_per_hour"]},
        "logger_first_file": rec["name"],
        "now_playing_us": np,
        "logger_under_us_webcaster": logger_us["on"],
    });
    eprintln!("{report:#}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_PLAYOUT_REPORT") {
        std::fs::write(path, report.to_string()).unwrap();
    }
}
