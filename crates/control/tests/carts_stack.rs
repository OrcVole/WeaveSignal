// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The cart wall on the real stack: `tests/carts/run-carts.sh` starts the desk (a release
//! build, its profanity delay at 3 s), Liquidsoap (playout and icecast.server) and PostgreSQL,
//! then runs this. It starts the control plane (the release build) with the demo pack as its
//! banks, listens to the listener stream as a player would, fires carts over HTTP, and finds
//! them there (ADR-0117: carts sound in the desk); it times key press to stream; it checks a
//! bed ducks under a sweeper, that a cued cart never reaches listeners, that a cart dumped
//! from the delay never does either, and FILE MISSING when a file goes.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STATION: &str = "5b2f7c0e-8d1a-4e63-9f0b-2a7c4d6e8f10";
const ADMIN: &str = "carts-stack-test";

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set: run tests/carts/run-carts.sh"))
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

    fn post(&self, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        let mut r = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {ADMIN}"))
            .send_json(body)
            .expect("the control plane answers");
        let code = r.status().as_u16();
        (code, r.body_mut().read_json().unwrap_or_default())
    }
}

/// One Opus packet heard: when its page arrived, how loud it is, and how much of it is a 1 kHz
/// tone.
#[derive(Clone, Copy, Debug)]
struct Heard {
    at: Instant,
    rms: f32,
    tone: f32,
}

/// Listen to the stream as a player does: HTTP, Ogg pages, Opus decoded to samples.
fn listen(url: &str, heard: Arc<Mutex<Vec<Heard>>>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(None)
        .build()
        .into();
    let res = agent.get(url).call().expect("the listener stream answers");
    let mut body = res.into_body().into_reader();
    let mut decoder: Option<opus_decoder::OpusDecoder> = None;
    let mut channels = 2usize;
    let mut pcm = vec![0f32; 5760 * 2];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let mut head = [0u8; 27];
        if body.read_exact(&mut head).is_err() {
            return;
        }
        assert_eq!(&head[..4], b"OggS", "the stream is Ogg");
        let mut table = vec![0u8; usize::from(head[26])];
        body.read_exact(&mut table).unwrap();
        let mut data = vec![0u8; table.iter().map(|&l| usize::from(l)).sum()];
        body.read_exact(&mut data).unwrap();
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
            // Goertzel at 1 kHz on the first channel, as a share of the packet's energy.
            let w = 2.0 * std::f32::consts::PI * 1000.0 / 48_000.0;
            let (mut s1, mut s2) = (0f32, 0f32);
            let mut energy = 0f32;
            for frame in s.chunks(channels) {
                let x = frame[0];
                energy += x * x;
                let s0 = x + 2.0 * w.cos() * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let power = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
            let tone = if energy > 0.0 {
                (2.0 * power / (n.max(1) as f32 * energy)).min(1.0)
            } else {
                0.0
            };
            heard.lock().unwrap().push(Heard { at, rms, tone });
        }
    }
}

/// The desk's status straight from its socket.
fn desk_status(sock: &std::path::Path) -> Option<serde_json::Value> {
    use std::io::{BufRead, Write};
    let mut s = std::os::unix::net::UnixStream::connect(sock).ok()?;
    s.write_all(b"{\"op\":\"status\"}\n").ok()?;
    let mut r = std::io::BufReader::new(s.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    serde_json::from_str(line.trim()).ok()
}

/// Wait until the listener has heard `quiet` of silence in a row (what was in flight through
/// the delay, playout and the listener server has passed).
fn wait_quiet(heard: &Mutex<Vec<Heard>>, quiet: Duration, limit: Duration) {
    let t0 = Instant::now();
    loop {
        let calm = {
            let h = heard.lock().unwrap();
            let last_loud = h.iter().rev().find(|h| h.rms >= 0.001).map(|h| h.at);
            let newest = h.last().map(|h| h.at);
            match (last_loud, newest) {
                (Some(l), Some(n)) => n.duration_since(l) >= quiet,
                (None, Some(_)) => true,
                _ => false,
            }
        };
        if calm {
            return;
        }
        assert!(t0.elapsed() < limit, "the listener stream never went quiet");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The loudest packet the listener heard after `from`, until `until` after `at`.
fn loudest_after(heard: &Mutex<Vec<Heard>>, from: usize, at: Instant, until: Duration) -> f32 {
    heard.lock().unwrap()[from..]
        .iter()
        .filter(|h| h.at >= at && h.at < at + until)
        .map(|h| h.rms)
        .fold(0.0, f32::max)
}

fn wait<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn cart_id(cat: &serde_json::Value, demo: &str) -> String {
    cat["carts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["origin"] == format!("demo:{demo}"))
        .unwrap_or_else(|| panic!("no demo cart {demo}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
#[ignore = "needs the stack: tests/carts/run-carts.sh"]
fn carts_fire_into_the_listener_stream() {
    // Only the stack script sets this: an `--ignored` run elsewhere (just test-db) skips it.
    if std::env::var("WEAVESIGNAL_CARTS_STACK").is_err() {
        eprintln!("skipped: run tests/carts/run-carts.sh");
        return;
    }
    let run = std::path::PathBuf::from(env("WEAVESIGNAL_CARTS_RUN"));
    let bin = env("WEAVESIGNAL_CONTROL_BIN");
    let port = env("WEAVESIGNAL_CARTS_CONTROL_PORT");
    let pack = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/carts-demo");
    let config = run.join("control.toml");
    std::fs::write(
        &config,
        format!(
            "[station]\nid = \"{STATION}\"\nname = \"Kettle FM\"\n\n[ports]\ncontrol = {port}\n\n\
             [desk]\nsocket = \"{sock}\"\npoll_ms = 100\n\n\
             [carts]\nmedia_dir = \"{media}\"\ndemo_pack = \"{pack}\"\nworker = \"{bin}\"\n\n\
             [log]\nformat = \"json\"\n",
            sock = run.join("sock/desk.sock").display(),
            media = run.join("carts").display(),
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
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into(),
    };

    // The demo pack loads through the media worker at first start: 50 carts in 4 banks.
    let loading = Instant::now();
    let cat = wait("the demo pack", 300, || {
        let view = api.get("/api/v1/studio")?;
        let cat = api.get("/api/v1/carts")?;
        (view["carts"]["importing"].is_null()
            && view["carts"]["desk"] == true
            && cat["carts"].as_array()?.len() == 50)
            .then_some(cat)
    });
    let pack_s = loading.elapsed().as_secs_f64();
    eprintln!("demo pack loaded in {pack_s:.1} s");
    assert_eq!(cat["banks"].as_array().unwrap().len(), 4);
    assert!(
        cat["carts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["missing"] == false)
    );
    let lufs: Vec<f64> = cat["carts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["loudness_lufs"].as_f64().unwrap() + c["gain_db"].as_f64().unwrap())
        .collect();
    let at_target = lufs.iter().filter(|l| (**l - -23.0).abs() < 0.1).count();
    eprintln!("{at_target} of 50 carts sound at -23 LUFS; the rest are held under -1 dBTP");

    // A listener, from now on.
    let heard = Arc::new(Mutex::new(Vec::<Heard>::new()));
    let h = heard.clone();
    let url = env("WEAVESIGNAL_CARTS_LISTENER_URL");
    std::thread::spawn(move || listen(&url, h));
    wait("the listener's first audio", 30, || {
        (heard.lock().unwrap().len() > 100).then_some(())
    });
    // Let the listener's burst of past (silent) audio pass.
    std::thread::sleep(Duration::from_secs(2));
    let quiet = heard.lock().unwrap().iter().all(|h| h.rms < 0.001);
    assert!(quiet, "the programme is silent before any cart fires");

    // The 1 kHz bleep: fire it as its key does, and find it in the stream.
    let bleep = cart_id(&cat, "bleep-1");
    let before = heard.lock().unwrap().len();
    let pressed = Instant::now();
    let (code, view) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "toggle", "cart": bleep}),
    );
    let answered = pressed.elapsed();
    assert_eq!(code, 200, "{view}");
    assert_eq!(view["carts"]["sounding"][0]["cart"], bleep.as_str());
    // The desk's own share: ask it until its player has started; the key's time less the
    // player's elapsed time is when the sound entered the desk's mix.
    let sock = run.join("sock/desk.sock");
    let desk_start = wait("the desk to start the bleep", 5, || {
        let st = desk_status(&sock)?;
        let s = st["carts"]["sounding"]
            .as_array()?
            .iter()
            .find(|s| s["cart"] == bleep.as_str())?
            .clone();
        let elapsed = s["elapsed_ms"].as_u64()?;
        (elapsed > 0).then(|| {
            pressed
                .elapsed()
                .saturating_sub(Duration::from_millis(elapsed))
        })
    });
    let delay_ms = desk_status(&sock).unwrap()["delay"]["delay_ms"]
        .as_u64()
        .unwrap();
    let onset = wait("the bleep in the listener stream", 40, || {
        heard.lock().unwrap()[before..]
            .iter()
            .find(|h| h.rms > 0.01)
            .copied()
    });
    let latency = onset.at.duration_since(pressed);
    // It is the bleep: a 1 kHz tone, about 0.9 s of it.
    std::thread::sleep(Duration::from_secs(3));
    let (tone_packets, loud_samples) = {
        let all = heard.lock().unwrap();
        let loud: Vec<&Heard> = all[before..].iter().filter(|h| h.rms > 0.01).collect();
        let tonal = loud.iter().filter(|h| h.tone > 0.8).count();
        (tonal as f64 / loud.len() as f64, loud.len() as u64 * 960)
    };
    eprintln!(
        "bleep: HTTP answer {answered:?}; heard in the stream {latency:?} after the key; \
         {:.2} s loud, {:.0} % of it a 1 kHz tone",
        loud_samples as f64 / 48_000.0,
        tone_packets * 100.0
    );
    assert!(tone_packets > 0.8, "the sound heard is the 1 kHz bleep");
    assert!((0.6..1.3).contains(&(loud_samples as f64 / 48_000.0)));
    assert!(answered < Duration::from_millis(500));

    eprintln!(
        "the desk started the bleep {desk_start:?} after the key; the delay holds {delay_ms} ms; \
         the rest, {:?}, is playout, the listener server's buffer and the stream",
        latency
            .saturating_sub(desk_start)
            .saturating_sub(Duration::from_millis(delay_ms))
    );

    // A bed loops and keeps sounding; a sweeper ducks it, and it comes back after.
    let bed = cart_id(&cat, "bed-1");
    let whoosh = cart_id(&cat, "whoosh-1");
    let (code, _) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "toggle", "cart": bed}),
    );
    assert_eq!(code, 200);
    std::thread::sleep(Duration::from_millis(500));
    let view = api.get("/api/v1/studio").unwrap();
    assert_eq!(view["carts"]["ducked"], false, "{}", view["carts"]);
    let (code, _) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "toggle", "cart": whoosh}),
    );
    assert_eq!(code, 200);
    wait("the beds to duck under the sweeper", 3, || {
        (api.get("/api/v1/studio")?["carts"]["ducked"] == true).then_some(())
    });
    wait("the beds to come back after the sweeper", 10, || {
        (api.get("/api/v1/studio")?["carts"]["ducked"] == false).then_some(())
    });
    let view = api.get("/api/v1/studio").unwrap();
    let sounding = view["carts"]["sounding"].as_array().unwrap();
    assert_eq!(sounding.len(), 1, "only the bed sounds now");
    assert!(sounding[0]["ends_ms"].is_null(), "a bed has no end");
    // Space: the cart started last fades.
    let (code, _) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "stop_last"}),
    );
    assert_eq!(code, 200);
    wait("the bed to stop", 3, || {
        api.get("/api/v1/studio")?["carts"]["sounding"]
            .as_array()?
            .is_empty()
            .then_some(())
    });
    // Its tail through the delay and the stream (the bleep's own latency, nearly all of it
    // the delay and icecast.server's buffer), then quiet.
    std::thread::sleep(latency + Duration::from_secs(1));
    wait_quiet(&heard, Duration::from_secs(3), Duration::from_secs(90));
    // Whatever left the desk reaches the listener within the bleep's own latency.
    let watch = latency + Duration::from_secs(4);

    // A cued cart (⇧ and its key) is in the presenter's ear only: listeners never hear it.
    let before = heard.lock().unwrap().len();
    let cued_at = Instant::now();
    let (code, view) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "cue", "cart": bleep}),
    );
    assert_eq!(code, 200, "{view}");
    assert_eq!(view["carts"]["sounding"][0]["cue"], true);
    std::thread::sleep(watch);
    let cue_heard = loudest_after(&heard, before, cued_at, Duration::from_secs(60));
    eprintln!("cued bleep: loudest packet at the listener {cue_heard:.5} RMS");
    assert!(
        cue_heard < 0.001,
        "listeners heard a cued cart: {cue_heard}"
    );

    // A cart fired on air and dumped from the delay before it airs never reaches listeners.
    wait("the delay to be full", 30, || {
        (desk_status(&sock)?["delay"]["rebuilding"] == false).then_some(())
    });
    let before = heard.lock().unwrap().len();
    let fired_at = Instant::now();
    let (code, _) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "fire", "cart": bleep}),
    );
    assert_eq!(code, 200);
    std::thread::sleep(Duration::from_millis(1_400));
    let (code, body) = api.post("/api/v1/desk/commands", serde_json::json!({"op": "dump"}));
    assert_eq!(code, 200, "the dump: {body}");
    std::thread::sleep(watch);
    let dumped_heard = loudest_after(&heard, before, fired_at, Duration::from_secs(60));
    let plays = desk_status(&sock).unwrap()["carts"]["play_seq"].clone();
    eprintln!(
        "dumped bleep: loudest packet at the listener {dumped_heard:.5} RMS; desk play events {plays}"
    );
    let flowing = heard.lock().unwrap()[before..].len();
    assert!(
        flowing > 250,
        "the stream stopped after the dump: {flowing} packets"
    );
    assert!(
        dumped_heard < 0.001,
        "listeners heard a dumped cart: {dumped_heard}"
    );

    // FILE MISSING: a cart's file goes; the wall says so within its look at the disk, and the
    // key does nothing; the file back, the cart is back.
    let ding = cat["carts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["origin"] == "demo:ding-1")
        .unwrap()
        .clone();
    let file = run.join("carts").join(ding["file"].as_str().unwrap());
    let aside = run.join("ding-aside.flac");
    std::fs::rename(&file, &aside).unwrap();
    wait("FILE MISSING", 10, || {
        let v = api.get("/api/v1/studio")?;
        v["carts"]["missing"]
            .as_array()?
            .iter()
            .any(|m| m == &ding["id"])
            .then_some(())
    });
    let (code, body) = api.post(
        "/api/v1/carts/commands",
        serde_json::json!({"op": "toggle", "cart": ding["id"]}),
    );
    assert_eq!(code, 409, "{body}");
    assert!(body["error"].as_str().unwrap().starts_with("FILE MISSING"));
    std::fs::rename(&aside, &file).unwrap();
    wait("the cart back", 10, || {
        let v = api.get("/api/v1/studio")?;
        v["carts"]["missing"].as_array()?.is_empty().then_some(())
    });

    let report = serde_json::json!({
        "demo_pack_load_s": pack_s,
        "carts_at_target": at_target,
        "fire_http_ms": answered.as_secs_f64() * 1000.0,
        "key_to_listener_stream_ms": latency.as_secs_f64() * 1000.0,
        "key_to_desk_start_ms": desk_start.as_secs_f64() * 1000.0,
        "delay_ms": delay_ms,
        "cued_loudest_rms_at_listener": cue_heard,
        "dumped_loudest_rms_at_listener": dumped_heard,
        "bleep_tone_share": tone_packets,
    });
    eprintln!("{report}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_CARTS_REPORT") {
        std::fs::write(path, report.to_string()).unwrap();
    }
}
