// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows end to end (1.3.2, PLAN 19b): `tests/keyed/run-stack.sh` starts Liquidsoap
//! (playout with its processing chain on, and the listener process), a stand-in desk and
//! PostgreSQL; this starts the control plane and drives it as staff and listeners would:
//!
//! - a show is set to `key` through the studio's route (playout is asked whether it can keep it,
//!   and says yes) and given a one-minute episode a little ahead; nothing is sent to playout by
//!   the test: the control plane's driver opens and closes the window from the schedule;
//! - a key for it is made and redeemed through the listener's route (`/api/v1/keyed/redeem`);
//! - one public listener stays on the public mount for the whole run, and one key holder on the
//!   keyed stream the control plane carries (`/listen/k/<show>/<token>/programme-keyed.opus`);
//! - the stand-in desk plays 660 Hz (public) and, from the episode's start to its end, 1000 Hz
//!   (the keyed show): the public listener must hear no 1000 Hz at all, and the slate (the
//!   station's fallback files, 440 Hz) meanwhile; the key holder must hear the keyed show;
//! - halfway through, the key is revoked through the staff route: the keyed stream ends within
//!   seconds, and the key is refused afterwards; a token for another show is refused.
//!
//! `WEAVESIGNAL_KEYED_NEGATIVE=1` turns the desk to 1000 Hz eight seconds before the window's
//! lead, outside any keyed window: the public listener must then hear 1000 Hz, which shows the
//! test can see what it is there to catch.
//!
//! The script writes the token and the public key it got to the run folder, so that it can try
//! the edge's njs check against the control plane's real token after this test.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STATION: &str = "5b1e7c2a-3d4f-4a6b-8c9d-0e1f2a3b4c5d";
const ADMIN: &str = "keyed-stack-test-keyed-stack-test";
const TOKEN_KEY: &str = "keyed-stack-token-key-keyed-stack-token-key";
/// The slate (fallback files), the public desk and the keyed desk.
const TONES: [f32; 3] = [440.0, 660.0, 1000.0];

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set: run tests/keyed/run-stack.sh"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
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
    fn send(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let url = format!("{}{path}", self.base);
        let auth = bearer.map(|b| format!("Bearer {b}"));
        let r = match (method, &auth) {
            ("GET", Some(a)) => self.agent.get(&url).header("Authorization", a).call(),
            ("GET", None) => self.agent.get(&url).call(),
            ("PUT", Some(a)) => self
                .agent
                .put(&url)
                .header("Authorization", a)
                .send_json(body),
            ("POST", Some(a)) => self
                .agent
                .post(&url)
                .header("Authorization", a)
                .send_json(body),
            (_, _) => self
                .agent
                .post(&url)
                .header("X-Forwarded-For", "203.0.113.7")
                .send_json(body),
        };
        // Not answering (yet) is a code of 0, so `wait` can try again.
        let Ok(mut r) = r else {
            return (0, serde_json::Value::Null);
        };
        let code = r.status().as_u16();
        (code, r.body_mut().read_json().unwrap_or_default())
    }
}

/// One 20 ms Opus packet heard: when, how loud, and each tone's share of its energy.
#[derive(Clone, Copy, Debug)]
struct Heard {
    at: i64,
    rms: f32,
    share: [f32; 3],
}

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

/// Follow one Ogg Opus stream until it ends (or `stop`), decoding every packet.
fn listen(
    url: String,
    heard: Arc<Mutex<Vec<Heard>>>,
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(None)
        .http_status_as_error(false)
        .build()
        .into();
    let res = loop {
        match agent.get(&url).call() {
            Ok(r) if r.status() == 200 => break r,
            _ if stop.load(Ordering::SeqCst) => return,
            _ => std::thread::sleep(Duration::from_millis(300)),
        }
    };
    let mut body = res.into_body().into_reader();
    let mut decoder: Option<opus_decoder::OpusDecoder> = None;
    let mut channels = 2usize;
    let mut pcm = vec![0f32; 5760 * 2];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let mut head = [0u8; 27];
        if body.read_exact(&mut head).is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }
        assert_eq!(&head[..4], b"OggS", "the stream is Ogg");
        let mut table = vec![0u8; usize::from(head[26])];
        if body.read_exact(&mut table).is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }
        let mut data = vec![0u8; table.iter().map(|&l| usize::from(l)).sum()];
        if body.read_exact(&mut data).is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }
        let at = now_ms();
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
            let mut sh = [0f32; 3];
            for (i, f) in TONES.iter().enumerate() {
                sh[i] = share(s, channels, *f);
            }
            heard.lock().unwrap().push(Heard { at, rms, share: sh });
        }
    }
}

/// Packets in which `tone` (an index of [`TONES`]) is present: loud enough and more than a
/// tenth of the packet's energy (a 20 ms packet that holds 5 ms of it is caught).
fn with_tone(h: &[Heard], tone: usize) -> Vec<Heard> {
    h.iter()
        .filter(|p| p.rms > 0.003 && p.share[tone] > 0.1)
        .copied()
        .collect()
}

/// Packets in which `tone` leads (over half the energy), as seconds.
fn seconds_of(h: &[Heard], tone: usize) -> f64 {
    h.iter()
        .filter(|p| p.rms > 0.003 && p.share[tone] > 0.5)
        .count() as f64
        * 0.02
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

fn sleep_until(ms: i64) {
    let d = ms - now_ms();
    if d > 0 {
        std::thread::sleep(Duration::from_millis(d as u64));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the stack: tests/keyed/run-stack.sh"]
async fn a_keyed_show_is_kept_from_the_public_and_heard_with_a_key_until_it_is_revoked() {
    if std::env::var("WEAVESIGNAL_KEYED_STACK").is_err() {
        eprintln!("skipped: run tests/keyed/run-stack.sh");
        return;
    }
    let negative = std::env::var("WEAVESIGNAL_KEYED_NEGATIVE").is_ok_and(|v| v == "1");
    let run = std::path::PathBuf::from(env("WEAVESIGNAL_KEYED_RUN"));
    let port = env("WEAVESIGNAL_KEYED_CONTROL_PORT");
    let listeners_port = env("WEAVESIGNAL_KEYED_LISTENERS_PORT");
    let db_url = env("WEAVESIGNAL_TEST_DATABASE_URL");
    // The player in a real browser, when run-stack.sh was given Playwright: the public pages'
    // build, served by the control plane, and the driver that plays a key's show from them.
    let browser = std::env::var("WEAVESIGNAL_KEYED_BROWSER").ok();
    let web = std::env::var("WEAVESIGNAL_KEYED_WEB_DIR")
        .map(|d| format!("[web]\ndir = \"{d}\"\n\n"))
        .unwrap_or_default();
    let config = run.join("control.toml");
    std::fs::write(
        &config,
        format!(
            "[station]\nid = \"{STATION}\"\nname = \"Kettle FM\"\n\n[ports]\ncontrol = {port}\nlisteners = {listeners_port}\n\n\
             [playout]\nsocket = \"{sock}\"\nlisteners_socket = \"{lsock}\"\nlibrary_dir = \"{lib}\"\npoll_ms = 100\n\n\
             [studio]\nlistener_delay_ms = 0\n\n\
             [keys]\nwindow_lead_s = 2\nwindow_tail_s = 3\nhls_lag_s = 120\nredeem_per_minute = 0\nfailures_per_ten_minutes = 0\n\n\
             [join]\nforwarded_for = true\nstream_url = \"http://127.0.0.1:{listeners_port}/programme.opus\"\n\n\
             {web}[log]\nformat = \"json\"\n",
            sock = run.join("sock/playout.sock").display(),
            lsock = run.join("sock/listeners.sock").display(),
            lib = run.join("library").display(),
        ),
    )
    .unwrap();
    let log = std::fs::File::create(run.join("control.log")).unwrap();
    let _control = Kill(
        Command::new(env("WEAVESIGNAL_CONTROL_BIN"))
            .args(["--config", config.to_str().unwrap(), "serve"])
            .env("WEAVESIGNAL_DATABASE_URL", &db_url)
            .env("WEAVESIGNAL_CONTROL_ADMIN_TOKEN", ADMIN)
            .env("WEAVESIGNAL_CONTROL_TOKEN_KEY", TOKEN_KEY)
            .env("RUST_LOG", "info,weavesignal_control::keyed=debug")
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
    wait("the control plane", 90, || {
        let (c, _) = api.send(
            "GET",
            "/api/v1/playout",
            Some(ADMIN),
            serde_json::Value::Null,
        );
        (c == 200).then_some(())
    });

    // The desk on air: live, so the stand-in desk is the programme.
    let clock = "11111111-2222-4333-8444-555555555555";
    let (code, body) = api.send(
        "PUT",
        &format!("/api/v1/schedule/clocks/{clock}"),
        Some(ADMIN),
        serde_json::json!({"name": "Tones", "slots": [{"category": "a"}]}),
    );
    assert_eq!(code, 200, "{body}");
    let (code, body) = api.send(
        "PUT",
        "/api/v1/playout/settings",
        Some(ADMIN),
        serde_json::json!({"time_zone": "UTC", "artist_separation_min": 0, "title_separation_min": 0,
                           "album_separation_min": 0, "mode": "live", "default_clock": clock}),
    );
    assert_eq!(code, 200, "{body}");

    // A show, set to `key` through the studio's route: playout answers that it has keyed shows.
    let (code, body) = api.send(
        "PUT",
        "/api/v1/shows/members-hour",
        Some(ADMIN),
        serde_json::json!({"name": "The Members' Hour", "colour_slot": 3}),
    );
    assert!(code == 200 || code == 201, "{code} {body}");
    let pool = sqlx::PgPool::connect(&db_url).await.unwrap();
    let (show_id,): (uuid::Uuid,) =
        sqlx::query_as("SELECT id FROM show WHERE station_id = $1 AND slug = 'members-hour'")
            .bind(uuid::Uuid::parse_str(STATION).unwrap())
            .fetch_one(&pool)
            .await
            .expect("the show");
    let show = show_id.to_string();
    let (code, body) = api.send(
        "PUT",
        "/api/v1/shows/members-hour/access",
        Some(ADMIN),
        serde_json::json!({"rule": "key", "slate": "fallback"}),
    );
    assert_eq!(code, 200, "{body}");

    // The public mount and the keyed one answer before the show starts.
    let public_url = format!("http://127.0.0.1:{listeners_port}/programme.opus");
    wait("the public mount", 120, || {
        ureq::get(&public_url)
            .config()
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .call()
            .ok()
            .map(|_| ())
    });

    // A one-minute episode, starting 45 s from now (whole seconds: the schedule's unit).
    let t0 = (now_ms() / 1000 + 45) * 1000;
    let t1 = t0 + 60_000;
    let series = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO series (id, station_id, show_id, number) VALUES ($1, $2, $3::uuid, 1)",
    )
    .bind(series)
    .bind(uuid::Uuid::parse_str(STATION).unwrap())
    .bind(&show)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at, minutes)
         VALUES ($1, $2, $3, 1, to_timestamp($4::float8 / 1000), 1)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(uuid::Uuid::parse_str(STATION).unwrap())
    .bind(series)
    .bind(t0 as f64)
    .execute(&pool)
    .await
    .unwrap();

    // A key, redeemed before the show: a token for its window, which waits for it.
    let (code, made) = api.send(
        "POST",
        "/api/v1/listener-keys",
        Some(ADMIN),
        serde_json::json!({"label": "Ishbel", "shows": ["members-hour"], "media": "sound"}),
    );
    assert_eq!(code, 201, "{made}");
    let key_id = made["key"]["id"].as_str().unwrap().to_owned();
    let text = made["text"].as_str().unwrap().to_owned();
    // The windows are read again within 15 s: wait until the redeem sees this one.
    let granted = wait("a token for the show's window", 30, || {
        let (code, r) = api.send(
            "POST",
            "/api/v1/keyed/redeem",
            None,
            serde_json::json!({"key": text}),
        );
        if code != 200 {
            eprintln!("redeem: {code} {r}");
        }
        (code == 200).then_some(r)
    });
    let token = granted["token"].as_str().unwrap().to_owned();
    assert_eq!(granted["show"]["id"], show.as_str());
    assert!(granted["starts_ms"].as_i64().unwrap() <= t0 - 2_000);
    assert!(granted["expires_ms"].as_i64().unwrap() >= t1);
    let keyed_url = format!("{}{}", api.base, granted["urls"]["opus"].as_str().unwrap());
    assert!(keyed_url.contains(&token));
    let (code, signing) = api.send(
        "GET",
        "/api/v1/listener-keys/signing",
        Some(ADMIN),
        serde_json::Value::Null,
    );
    assert_eq!(code, 200, "{signing}");
    assert_eq!(signing["alg"], "p256");
    // A sound key is given the sound's playlist, never the picture's, which it is refused.
    let hls = granted["urls"]["hls"].as_str().unwrap().to_owned();
    assert_eq!(
        hls,
        format!("/hls/k/{show}/{token}/audio.m3u8"),
        "a sound key's HLS address"
    );
    std::fs::write(run.join("hls.txt"), &hls).unwrap();
    std::fs::write(run.join("token.txt"), &token).unwrap();
    std::fs::write(run.join("show.txt"), &show).unwrap();
    std::fs::write(
        run.join("public_key.txt"),
        signing["public_key"].as_str().unwrap(),
    )
    .unwrap();

    // Before its window, the keyed stream is refused (the token is not valid yet).
    let (code, _) = api.send(
        "GET",
        &format!("/listen/k/{show}/{token}/programme-keyed.opus"),
        None,
        serde_json::Value::Null,
    );
    assert_eq!(code, 403);

    // The public listener, for the whole run.
    let stop = Arc::new(AtomicBool::new(false));
    let public = Arc::new(Mutex::new(Vec::new()));
    let public_ended = Arc::new(AtomicBool::new(false));
    {
        let (h, e, s, u) = (
            public.clone(),
            public_ended.clone(),
            stop.clone(),
            public_url.clone(),
        );
        std::thread::spawn(move || listen(u, h, e, s));
    }
    let desk_keyed = run.join("desk-keyed");
    let _ = std::fs::remove_file(&desk_keyed);
    if negative {
        // Outside any window: the public must hear it, or the test is blind.
        sleep_until(t0 - 10_000);
        std::fs::write(&desk_keyed, "").unwrap();
        sleep_until(t0 - 6_000);
        let _ = std::fs::remove_file(&desk_keyed);
    }

    // A second key, for the player in a browser, which opens its link as the window opens.
    let (code, made2) = api.send(
        "POST",
        "/api/v1/listener-keys",
        Some(ADMIN),
        serde_json::json!({"label": "Morag Fraser", "shows": ["members-hour"], "media": "sound"}),
    );
    assert_eq!(code, 201, "{made2}");
    let key2_id = made2["key"]["id"].as_str().unwrap().to_owned();
    let revoked_mark = run.join("browser-revoked");
    let _ = std::fs::remove_file(&revoked_mark);

    // The key holder joins once the window (with its lead) is open.
    sleep_until(t0 - 1_000);
    let player = browser.as_ref().map(|script| {
        Kill(
            Command::new("node")
                .arg(script)
                .env("BASE", &api.base)
                .env("KEY", made2["text"].as_str().unwrap())
                .env("SHOW", &show)
                .env("REVOKED", &revoked_mark)
                .stdout(std::fs::File::create(run.join("player.log")).unwrap())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        )
    });
    let keyed = Arc::new(Mutex::new(Vec::new()));
    let keyed_ended = Arc::new(AtomicBool::new(false));
    {
        let (h, e, s, u) = (
            keyed.clone(),
            keyed_ended.clone(),
            stop.clone(),
            keyed_url.clone(),
        );
        std::thread::spawn(move || listen(u, h, e, s));
    }
    // A token for another show, or none, opens nothing.
    let (code, _) = api.send(
        "GET",
        &format!(
            "/listen/k/{}/{token}/programme-keyed.opus",
            uuid::Uuid::new_v4()
        ),
        None,
        serde_json::Value::Null,
    );
    assert_eq!(code, 403);

    // The show: the desk plays the keyed tone from its start to its end.
    sleep_until(t0);
    std::fs::write(&desk_keyed, "").unwrap();
    // The player's facts and the studio's view say which keyed show is on, and that playout keeps
    // it from the public, with what the public hears.
    sleep_until(t0 + 6_000);
    let (code, p) = api.send("GET", "/api/v1/player", None, serde_json::Value::Null);
    assert_eq!(code, 200, "{p}");
    assert_eq!(
        (p["show_id"].as_str(), p["keyed"].as_bool()),
        (Some(show.as_str()), Some(true)),
        "the player's facts: {p}"
    );
    let (code, v) = api.send(
        "GET",
        "/api/v1/studio",
        Some(ADMIN),
        serde_json::Value::Null,
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(
        v["keyed"]["show"],
        show.as_str(),
        "the studio's view: {}",
        v["keyed"]
    );
    assert_eq!(
        v["keyed"]["kept"], true,
        "the studio's view: {}",
        v["keyed"]
    );
    assert_eq!(
        v["keyed"]["confirmed"], true,
        "playout answered, so what the public hears is confirmed: {}",
        v["keyed"]
    );
    assert_eq!(
        v["keyed"]["public_hears"], "fallback",
        "the studio's view: {}",
        v["keyed"]
    );

    // The sound key's HLS address passes the token check (no picture in this stack, so the
    // origin then answers 404, video off); the picture's refuses the same token for its medium.
    let (code, e) = api.send("GET", &hls, None, serde_json::Value::Null);
    assert_eq!(
        code, 404,
        "a sound key's own HLS address passes the token check (then: video is off): {e}"
    );
    let picture = hls.replace("/audio.m3u8", "/live.m3u8");
    let (code, e) = api.send("GET", &picture, None, serde_json::Value::Null);
    assert_eq!(
        (code, e["code"].as_str()),
        (403, Some("wrong_media")),
        "a sound key is refused the picture: {e}"
    );

    // A key for the picture carries the show's sound (the operator, 2026-11-25): its token
    // passes the check on the picture's address and on the sound's beside it (video is off in
    // this stack, so each then answers 404; a token refused for its medium answers 403).
    let (code, made_v) = api.send(
        "POST",
        "/api/v1/listener-keys",
        Some(ADMIN),
        serde_json::json!({"label": "Mairi Sinclair", "shows": ["members-hour"], "media": "video"}),
    );
    assert_eq!(code, 201, "{made_v}");
    let (code, granted_v) = api.send(
        "POST",
        "/api/v1/keyed/redeem",
        None,
        serde_json::json!({"key": made_v["text"]}),
    );
    assert_eq!(code, 200, "a video key is redeemed: {granted_v}");
    let hls_v = granted_v["urls"]["hls"].as_str().unwrap().to_owned();
    assert!(hls_v.ends_with("/live.m3u8"), "{hls_v}");
    assert!(
        granted_v["urls"]["opus"]
            .as_str()
            .is_some_and(|u| u.ends_with("/programme-keyed.opus")),
        "a video key is given the keyed stream's address too: {granted_v}"
    );
    for (what, at) in [
        ("the picture", hls_v.clone()),
        (
            "the sound beside it",
            hls_v.replace("/live.m3u8", "/audio.m3u8"),
        ),
    ] {
        let (code, e) = api.send("GET", &at, None, serde_json::Value::Null);
        assert_eq!(
            code, 404,
            "a video key's token passes the check for {what} (then: video is off): {e}"
        );
    }

    // The browser's key, revoked while its player plays.
    if player.is_some() {
        sleep_until(t0 + 22_000);
        std::fs::write(&revoked_mark, "").unwrap();
        let (code, body) = api.send(
            "POST",
            &format!("/api/v1/listener-keys/{key2_id}/revoke"),
            Some(ADMIN),
            serde_json::json!({"reason": "the browser's check"}),
        );
        assert_eq!(code, 200, "{body}");
    }

    let revoke_at = t0 + 30_000;
    sleep_until(revoke_at);
    let keyed_before: f64 = seconds_of(&keyed.lock().unwrap(), 2);
    let (code, body) = api.send(
        "POST",
        &format!("/api/v1/listener-keys/{key_id}/revoke"),
        Some(ADMIN),
        serde_json::json!({"reason": "the stack test"}),
    );
    assert_eq!(code, 200, "{body}");
    let ended_after = wait("the keyed stream to end after the revoke", 15, || {
        keyed_ended
            .load(Ordering::SeqCst)
            .then(|| now_ms() - revoke_at)
    });
    let (code, e) = api.send(
        "POST",
        "/api/v1/keyed/redeem",
        None,
        serde_json::json!({"key": text}),
    );
    assert_eq!((code, e["code"].as_str()), (403, Some("revoked")));
    let (code, r) = api.send(
        "GET",
        "/api/v1/keyed/revoked",
        None,
        serde_json::Value::Null,
    );
    assert_eq!(code, 200);
    assert!(
        r["revoked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == key_id.as_str()),
        "{r}"
    );
    let (code, _) = api.send(
        "GET",
        &format!("/listen/k/{show}/{token}/programme-keyed.opus"),
        None,
        serde_json::Value::Null,
    );
    assert_eq!(code, 403, "a revoked key's token is refused at once");

    sleep_until(t1);
    let _ = std::fs::remove_file(&desk_keyed);
    // The browser's verdict (it gives up on its own within a minute of the revoke).
    let player_ok = player.map(|mut p| {
        let status = wait("the player in the browser to finish", 60, || {
            p.0.try_wait().ok().flatten()
        });
        let said = std::fs::read_to_string(run.join("player.log")).unwrap_or_default();
        eprintln!("the player in a browser:\n{said}");
        status.success()
    });
    // After the window and its tail, the public hears the desk again.
    sleep_until(t1 + 3_000 + 12_000);
    stop.store(true, Ordering::SeqCst);
    let (code, k) = api.send(
        "GET",
        "/api/v1/public/keyed-shows",
        None,
        serde_json::Value::Null,
    );
    assert_eq!(code, 200, "{k}");

    let public = public.lock().unwrap().clone();
    let leaks = with_tone(&public, 2);
    let report = serde_json::json!({
        "negative": negative,
        "public_seconds": public.len() as f64 * 0.02,
        "public_1khz_packets": leaks.len(),
        "public_first_1khz_ms_from_t0": leaks.first().map(|p| p.at - t0),
        "public_slate_s": seconds_of(&public, 0),
        "public_desk_s": seconds_of(&public, 1),
        "keyed_1khz_s_before_revoke": keyed_before,
        "keyed_ended_ms_after_revoke": ended_after,
        "browser_player_passed": player_ok,
    });
    std::fs::write(
        run.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!("{report:#}");
    if negative {
        assert!(
            !leaks.is_empty(),
            "the negative control heard no 1 kHz on the public mount: the test is blind"
        );
        return;
    }
    assert!(
        leaks.is_empty(),
        "the public heard the keyed show: {} packets of 1 kHz, the first {:?} ms from its start",
        leaks.len(),
        leaks.first().map(|p| p.at - t0)
    );
    assert!(
        !public_ended.load(Ordering::SeqCst),
        "the public listener was dropped"
    );
    assert!(
        seconds_of(&public, 0) > 40.0,
        "the public heard the slate for the show ({} s)",
        seconds_of(&public, 0)
    );
    assert!(
        seconds_of(&public, 1) > 20.0,
        "the public heard the desk before and after ({} s)",
        seconds_of(&public, 1)
    );
    // The keyed mount, like the public one, runs about 13 s behind playout: in the 30 s before
    // the revoke the holder heard about 17 s of the show.
    assert!(
        keyed_before > 10.0,
        "the key holder heard the keyed show ({keyed_before} s before the revoke)"
    );
    assert!(
        ended_after < 6_000,
        "the revoke ended the keyed stream {ended_after} ms after"
    );
    assert_ne!(
        player_ok,
        Some(false),
        "the player in a browser failed: see player.log in the run folder"
    );
}
