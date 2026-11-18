// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The monitor link's view, events and controls (ADR-0280, design 4.5; `GET /monitor`,
//! `/monitor/events`, `POST /monitor/go`, `/not-yet`, `/cue`, `/handover`, `/end-show`, `/dump`)
//! against a real PostgreSQL, a desk that remembers what it was told, the post's rule and a
//! stand-in for LiveKit's room service: what the host sees, what each control refuses and in
//! what words, what each does through the desk's own paths, the stream, and the monitor room's
//! seats leaving with an ended key. Ignored unless asked for (`just test-db`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use jiff::ToSpan;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::api::{Api, ApiConfig, layer};
use weavesignal_control::apikey::ApiKey;
use weavesignal_control::app::{AppState, RoomState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig, RoomConfig};
use weavesignal_control::desk::{DeskClient, StationMode};
use weavesignal_control::hub::{Hub, RoomAdmin};
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "monitor-api-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";
const LK_SECRET: &str = "monitor-api-db-test-livekit-secret-0123456789";

struct Plane {
    app: Router,
    state: Arc<AppState>,
    pool: sqlx::PgPool,
    desk: Arc<Mutex<DeskState>>,
    onair: Arc<weavesignal_control::onair::OnAir>,
    /// What the stand-in room service was asked to remove: `(room, identity)`.
    removed: Removed,
}

struct P {
    identity: String,
    name: String,
    role: &'static str,
    on_air: bool,
    cue: bool,
    /// For a source: how long the desk has had it.
    connected_ms: Option<u64>,
}

#[derive(Default)]
struct DeskState {
    people: Vec<P>,
    /// `on:<identity>`, `off:<identity>`, `cue:<identity>:<on>`, `dump`, in the order told.
    commands: Vec<String>,
    /// What the profanity delay holds now.
    delay_ms: u32,
}

/// A desk that holds whoever the test says, takes the commands the controls give, and
/// remembers them.
fn live_desk(path: &std::path::Path) -> Arc<Mutex<DeskState>> {
    let state = Arc::new(Mutex::new(DeskState::default()));
    let listener = UnixListener::bind(path).unwrap();
    let shared = state.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let shared = shared.clone();
            std::thread::spawn(move || {
                let mut w = conn.try_clone().unwrap();
                for line in BufReader::new(conn).lines() {
                    let Ok(line) = line else { return };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let mut st = shared.lock().unwrap();
                    let answer = match req["op"].as_str() {
                        Some("status") => {
                            let people: Vec<Value> = st
                                .people
                                .iter()
                                .map(|p| {
                                    let mut v = json!({
                                        "identity": p.identity, "name": p.name, "role": p.role,
                                        "on_air": p.on_air, "mute": false, "cue": p.cue, "talk": false,
                                        "talk_to": null, "present": true, "away_ms": null, "linked": true,
                                        "lufs": -23.0, "lufs_peak": -20.0, "underruns": 0, "late_ms": 0,
                                        "ring_ms": 300, "camera": false
                                    });
                                    if let Some(ms) = p.connected_ms {
                                        v["source"] = json!({"way": "srt", "format": "mpegts/aac 48 kHz stereo",
                                            "key": p.identity.trim_start_matches("source-"), "show": "x",
                                            "connected_ms": ms, "leveller_db": 0.0, "level_db": -20.0,
                                            "monitor": true});
                                    }
                                    v
                                })
                                .collect();
                            json!({"ok": true, "playout": {"connected": true}, "people": people,
                                   "delay": {"delay_ms": st.delay_ms, "target_ms": 8000, "rebuilding": false}})
                        }
                        Some("on_air") => {
                            let id = req["identity"].as_str().unwrap().to_owned();
                            let on = req["on"].as_bool().unwrap_or(false);
                            st.commands
                                .push(format!("{}:{id}", if on { "on" } else { "off" }));
                            if let Some(p) = st.people.iter_mut().find(|p| p.identity == id) {
                                p.on_air = on;
                            }
                            json!({"ok": true})
                        }
                        Some("cue") => {
                            let id = req["identity"].as_str().unwrap().to_owned();
                            let on = req["on"].as_bool().unwrap_or(false);
                            st.commands.push(format!("cue:{id}:{on}"));
                            if let Some(p) = st.people.iter_mut().find(|p| p.identity == id) {
                                p.cue = on;
                            }
                            json!({"ok": true})
                        }
                        Some("dump") => {
                            if st.delay_ms == 0 {
                                json!({"ok": false, "error": "the delay holds nothing to dump"})
                            } else {
                                st.commands.push("dump".into());
                                st.delay_ms = 0;
                                json!({"ok": true})
                            }
                        }
                        _ => json!({"ok": true}),
                    };
                    drop(st);
                    if writeln!(w, "{answer}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    state
}

/// A stand-in for LiveKit's room service that answers 200 and records `RemoveParticipant`.
type Removed = Arc<Mutex<Vec<(String, String)>>>;

fn room_service() -> (String, Removed) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let removed = Arc::new(Mutex::new(Vec::new()));
    let seen = removed.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { return };
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    return;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                if first.contains("RemoveParticipant")
                    && let Ok(v) = serde_json::from_slice::<Value>(&body)
                {
                    seen.lock().unwrap().push((
                        v["room"].as_str().unwrap_or_default().to_owned(),
                        v["identity"].as_str().unwrap_or_default().to_owned(),
                    ));
                }
                let _ = conn.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
            });
        }
    });
    (url, removed)
}

/// A plane with a desk at a socket, the playout layer's control (no playout behind it), the
/// room service's stand-in, and the post's rules set as the test says.
async fn plane(rules: impl FnOnce(&mut Value)) -> Plane {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ws-mapi-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let desk = live_desk(&dir.join("desk.sock"));
    let (lk_url, removed) = room_service();
    let keys = || weavesignal_proto::Keys {
        api_key: "monitor-test".into(),
        api_secret: LK_SECRET.into(),
    };
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        Some(ApiKey::parse(TOKEN_KEY).unwrap()),
        ApiConfig::default(),
        true,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    staff.restore(station.id).await;
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(Some(&dir.join("desk.sock"))),
        Some(RoomAdmin {
            api_url: lk_url,
            room: "studio".into(),
            keys: keys(),
        }),
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    let live = hub.live.clone();
    let cards =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mapi-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&cards).unwrap();
    let state = Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: Some(RoomState {
            config: RoomConfig::default(),
            keys: keys(),
        }),
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig {
            stream_url: "https://kettle.example/programme.opus".into(),
            ..JoinConfig::default()
        },
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    });
    let app = router(state.clone()).merge(shows::router(Arc::new(ShowsState {
        station: station.clone(),
        db: Some(pool.clone()),
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            cards_dir: cards.join("cards"),
            episodes_dir: cards.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: Some(live),
        hub: Some(state.hub.clone()),
        video: None,
    })));
    let onair = weavesignal_control::onair::OnAir::new(
        station.id,
        pool.clone(),
        weavesignal_control::onair::Links {
            playout: weavesignal_control::playout::PlayoutClient::new(None),
            listeners: weavesignal_control::playout::PlayoutClient::new(None),
            status_url: String::new(),
            library_dir: PathBuf::from("/nonexistent"),
            logger_dir: PathBuf::from("/nonexistent"),
            listener_delay: Duration::ZERO,
            library_lufs: -16.0,
            library_max_seconds: 600.0,
        },
        &state.hub,
    );
    state.hub.onair.set(onair.clone()).ok().unwrap();
    // Playout answers: the desk is what listeners hear (the stand-in has no playout to ask).
    onair.announce(weavesignal_control::onair::NowPlaying {
        on_air: "desk".into(),
        ..Default::default()
    });
    let p = Plane {
        app: layer(app, staff),
        state,
        pool,
        desk,
        onair,
        removed,
    };
    let (_, mut r) = send(
        &p,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    rules(&mut r);
    let (code, v) = send(&p, "PUT", "/api/v1/settings/sources", Some(ADMIN), r).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    p
}

async fn call(
    p: &Plane,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
    peer: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(b) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let mut req = req.body(body).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    let res = p.app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, v)
}

async fn send(
    p: &Plane,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    call(p, method, uri, bearer, body, "192.0.2.50:4000").await
}

/// A host's press on their monitor link.
async fn press(p: &Plane, monitor: &str, route: &str, body: Value) -> (StatusCode, Value) {
    send(
        p,
        "POST",
        &format!("/api/v1/monitor/{route}"),
        Some(monitor),
        body,
    )
    .await
}

async fn view(p: &Plane, monitor: &str) -> Value {
    let (code, v) = send(p, "GET", "/api/v1/monitor", Some(monitor), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    v
}

async fn make_show(p: &Plane, slug: &str, name: &str, step: &str) {
    let (code, v) = send(
        p,
        "PUT",
        &format!("/api/v1/shows/{slug}"),
        Some(ADMIN),
        json!({"name": name, "colour_slot": 3, "step": step}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
}

async fn episode(p: &Plane, slug: &str, from_now_s: i64, minutes: u32) {
    let (code, _) = send(
        p,
        "PUT",
        &format!("/api/v1/shows/{slug}/series/1"),
        Some(ADMIN),
        json!({}),
    )
    .await;
    assert!(code.is_success());
    let (code, body) = send(
        p,
        "POST",
        &format!("/api/v1/shows/{slug}/series/1/episodes"),
        Some(ADMIN),
        json!({"count": 1}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let at = (jiff::Timestamp::now() + from_now_s.seconds())
        .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap())
        .to_string();
    let (code, body) = send(
        p,
        "PATCH",
        &format!("/api/v1/shows/{slug}/series/1/episodes/1"),
        Some(ADMIN),
        json!({"air_at": at, "minutes": minutes}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

/// A host: their key id and monitor secret, with a source at the desk for `connected_ms`.
struct Host {
    key: String,
    monitor: String,
    identity: String,
}

async fn host(p: &Plane, show: &str, label: &str, connected_ms: Option<u64>) -> Host {
    let (code, made) = send(
        p,
        "POST",
        &format!("/api/v1/shows/{show}/keys"),
        Some(ADMIN),
        json!({"label": label, "hint": "srt"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let key = made["key"]["key_id"].as_str().unwrap().to_owned();
    let identity = format!("source-{key}");
    if connected_ms.is_some() {
        p.desk.lock().unwrap().people.push(P {
            identity: identity.clone(),
            name: label.into(),
            role: "source",
            on_air: false,
            cue: false,
            connected_ms,
        });
    }
    Host {
        key,
        monitor: made["monitor_secret"].as_str().unwrap().to_owned(),
        identity,
    }
}

fn dj(p: &Plane, identity: &str, name: &str, on_air: bool) {
    p.desk.lock().unwrap().people.push(P {
        identity: identity.into(),
        name: name.into(),
        role: "dj",
        on_air,
        cue: false,
        connected_ms: None,
    });
}

fn set_on_air(p: &Plane, identity: &str, on: bool) {
    for q in &mut p.desk.lock().unwrap().people {
        if q.identity == identity {
            q.on_air = on;
        }
    }
}

fn commands(p: &Plane) -> Vec<String> {
    p.desk.lock().unwrap().commands.clone()
}

/// One look of the post's rule, with the desk read first, and the step brought in line.
async fn look(p: &Plane) {
    p.state.hub.poll_once().await;
    weavesignal_control::steps::refresh(&p.state).await;
    weavesignal_control::post::tick(&p.state).await.unwrap();
}

async fn log_of(p: &Plane, key: &str) -> Vec<(String, Option<String>)> {
    weavesignal_model::sources::key_log(&p.pool, p.state.station.id, key, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|l| (l.event, l.reason))
        .collect()
}

fn why(v: &Value) -> &str {
    v["error"].as_str().unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_view_says_where_the_host_stands_and_what_they_may_press() {
    let p = plane(|r| r["late_count_s"] = json!(120)).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    // Steeped started ten minutes ago; Kettle Drive follows it.
    episode(&p, "steeped", -600, 60).await;
    episode(&p, "kettle", 3000, 60).await;
    // Finlay connected thirty seconds ago, after his post: the late count runs (120 s here).
    let finlay = host(&p, "steeped", "Finlay Grant", Some(30_000)).await;
    let heather = host(&p, "kettle", "Heather Douglas", Some(600_000)).await;
    p.onair.set_mode(StationMode::Auto).await.unwrap();
    p.state.hub.with_studio_if(|s| {
        s.set_listener_delay(13_000);
        true
    });
    look(&p).await;

    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["state"], "standby", "{v}");
    assert_eq!(
        v["says"],
        "Standing by. The station hears you. Listeners do not."
    );
    assert_eq!(v["then"]["kind"], "late_count", "{v}");
    assert_eq!(v["then"]["words"], "Automation holds your air.");
    let now = v["station"]["now_ms"].as_i64().unwrap();
    let due = v["then"]["at_ms"].as_i64().unwrap();
    assert!(
        due > now + 80_000 && due <= now + 120_000,
        "the count to the late count's end: {due} against {now}"
    );
    assert_eq!(v["post"]["mine"], true);
    assert_eq!(v["post"]["show_name"], "Steeped");
    assert_eq!(v["post"]["late_ends_ms"], v["then"]["at_ms"]);
    assert_eq!(v["step"]["step"], "full");
    assert_eq!(v["step"]["handover"], true);
    // After you: Kettle Drive, its host not on the desk yet from Finlay's side? She is: standing by.
    assert_eq!(v["rundown"]["you"]["show_name"], "Steeped");
    assert_eq!(v["rundown"]["after"]["show_name"], "Kettle Drive");
    assert_eq!(v["rundown"]["after"]["host"], "Heather Douglas");
    assert_eq!(v["rundown"]["after"]["state"], "standing_by");
    assert!(v["rundown"]["before"].is_null());
    assert!(v["insert"].is_null());
    // What he may press: Go and Not yet, nothing of the on-air controls.
    assert_eq!(v["controls"]["go"]["may"], true, "{v}");
    assert_eq!(v["controls"]["not_yet"]["may"], true);
    for c in ["cue", "bring", "hand_over", "end_show", "dump"] {
        assert_eq!(v["controls"][c]["may"], false, "{c}: {v}");
        assert_eq!(v["controls"][c]["why_not"], "You are not on air.");
    }
    // Nothing of Heather's key, id or secret anywhere in his view.
    let text = v.to_string();
    for forbidden in [
        heather.key.as_str(),
        heather.monitor.as_str(),
        finlay.monitor.as_str(),
    ] {
        assert!(!text.contains(forbidden), "{forbidden} in {v}");
    }

    // Her view: her show has not started, so Go says so; nothing of her is Finlay's to press.
    let h = view(&p, &heather.monitor).await;
    assert_eq!(h["state"], "standby");
    assert_eq!(h["controls"]["go"]["may"], false);
    assert_eq!(
        h["controls"]["go"]["why_not"],
        "Your show has not started. You go on air at its time."
    );
    assert_eq!(h["rundown"]["before"]["show_name"], "Steeped");
    assert!(
        h["rundown"]["after"].is_null(),
        "nothing after: automation follows"
    );
    let (code, e) = press(&p, &heather.monitor, "go", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(
        why(&e),
        "Your show has not started. You go on air at its time."
    );
    let (code, e) = press(&p, &heather.monitor, "not-yet", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "There is no count to stop.");
    assert!(commands(&p).is_empty(), "{:?}", commands(&p));

    // Not yet: the count stops; the host goes on air by their own press after it.
    let (code, d) = press(&p, &finlay.monitor, "not-yet", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["done"], "not_yet");
    assert!(
        log_of(&p, &finlay.key)
            .await
            .contains(&("not_yet".into(), Some("Finlay Grant (monitor)".into())))
    );
    look(&p).await;
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["controls"]["not_yet"]["may"], false);
    assert_eq!(
        v["controls"]["go"]["may"], true,
        "the host's own press stays: {v}"
    );
    assert!(
        commands(&p).is_empty(),
        "the schedule does not put him on air after Not yet"
    );
    let (code, d) = press(&p, &finlay.monitor, "go", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["done"], "go");
    assert_eq!(commands(&p), [format!("on:{}", finlay.identity)]);
    assert_eq!(
        p.state.hub.read_studio(|s| s.station_mode()),
        Some(StationMode::Live),
        "going on air took the air back from automation"
    );
    assert_eq!(d["view"]["state"], "on_air", "{d}");
    assert_eq!(d["view"]["controls"]["go"]["why_not"], "You are on air.");
    let log = log_of(&p, &finlay.key).await;
    assert!(
        log.contains(&("go".into(), Some("Finlay Grant (monitor)".into()))),
        "{log:?}"
    );
    assert!(
        log.contains(&("on_air".into(), Some("Finlay Grant (monitor)".into()))),
        "{log:?}"
    );
    // On air: the lamp says how far behind listeners are.
    assert!(
        d["view"]["says"]
            .as_str()
            .unwrap()
            .starts_with("On air. Listeners hear you, about 13 s behind."),
        "{d}"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_handover_is_two_presses_at_full_and_the_hosts_alone_to_make() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    episode(&p, "steeped", -600, 60).await;
    episode(&p, "kettle", 3000, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    let heather = host(&p, "kettle", "Heather Douglas", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;

    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["state"], "on_air", "{v}");
    assert_eq!(v["next_host"]["name"], "Heather Douglas");
    assert_eq!(v["next_host"]["state"], "standing_by");
    assert_eq!(v["next_host"]["cued"], false);
    // On air with the next host standing by: Cue and Bring, End show, Dump (Full has it all).
    for c in ["cue", "bring", "end_show", "dump"] {
        assert_eq!(v["controls"][c]["may"], true, "{c}: {v}");
    }
    assert_eq!(v["controls"]["hand_over"]["may"], false);
    assert_eq!(
        v["controls"]["hand_over"]["why_not"],
        "Bring Heather Douglas on air first."
    );
    assert_eq!(
        v["controls"]["end_show_says"],
        "Heather Douglas is standing by: the air passes to them as you leave."
    );
    // The grace is not running: the post has not come.
    assert!(v["post"]["grace_ends_ms"].is_null());

    // Cue: the next host in his headphones, and no identity in the request.
    let (code, d) = press(&p, &finlay.monitor, "cue", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["view"]["next_host"]["cued"], true);
    assert_eq!(commands(&p), [format!("cue:{}:true", heather.identity)]);
    let (code, e) = press(&p, &finlay.monitor, "cue", json!({"identity": "x"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    let (code, _) = press(&p, &finlay.monitor, "cue", json!({"on": false})).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        commands(&p).last().unwrap(),
        &format!("cue:{}:false", heather.identity)
    );

    // The second press before the first: refused, in words. Nothing named in the body but the step.
    let (code, e) = press(
        &p,
        &finlay.monitor,
        "handover",
        json!({"step": "hand_over"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "Bring Heather Douglas on air first.");
    let (code, _) = press(&p, &finlay.monitor, "handover", json!({"step": "both"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    let (code, _) = press(&p, &finlay.monitor, "handover", Value::Null).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    // Heather is not on air, so her controls are not Finlay's to press and hers are refused.
    let (code, e) = press(&p, &heather.monitor, "handover", json!({"step": "bring"})).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "You are not on air.");

    // The first press: Heather beside him, the overlap. Nobody comes off.
    let before = commands(&p).len();
    let (code, d) = press(&p, &finlay.monitor, "handover", json!({"step": "bring"})).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["done"], "bring");
    assert_eq!(
        commands(&p)[before..],
        [format!("on:{}", heather.identity)],
        "{:?}",
        commands(&p)
    );
    assert_eq!(d["view"]["next_host"]["state"], "on_air");
    assert_eq!(d["view"]["controls"]["hand_over"]["may"], true);
    assert_eq!(d["view"]["controls"]["bring"]["may"], false);
    assert_eq!(
        d["view"]["controls"]["bring"]["why_not"],
        "Heather Douglas is on air already."
    );
    let log = log_of(&p, &heather.key).await;
    assert!(
        log.contains(&("on_air".into(), Some("Finlay Grant (monitor)".into()))),
        "{log:?}"
    );

    // The second press: he comes off, she holds the air. Written down as taken off.
    let (code, d) = press(
        &p,
        &finlay.monitor,
        "handover",
        json!({"step": "hand_over"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["done"], "hand_over");
    assert_eq!(
        commands(&p).last().unwrap(),
        &format!("off:{}", finlay.identity)
    );
    assert_eq!(d["view"]["state"], "standby", "{d}");
    let h = view(&p, &heather.monitor).await;
    assert_eq!(h["state"], "on_air");
    assert!(
        log_of(&p, &finlay.key)
            .await
            .contains(&("off_air".into(), Some("Finlay Grant (monitor)".into())))
    );
    // The schedule does not take her off or put him back: his press was the handover.
    look(&p).await;
    look(&p).await;
    let n = commands(&p).len();
    look(&p).await;
    assert_eq!(commands(&p).len(), n);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn below_full_there_is_no_overlap_and_below_callers_no_dump() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "solo").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    episode(&p, "steeped", -600, 60).await;
    episode(&p, "kettle", 3000, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    let _heather = host(&p, "kettle", "Heather Douglas", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;

    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["step"]["step"], "solo");
    assert_eq!(
        (v["step"]["dump"].as_bool(), v["step"]["handover"].as_bool()),
        (Some(false), Some(false))
    );
    let (code, e) = press(&p, &finlay.monitor, "dump", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "Dump is off at Solo. It comes on at Callers.");
    let (code, e) = press(&p, &finlay.monitor, "handover", json!({"step": "bring"})).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "The overlap is off at Solo. It comes on at Full.");
    // Cue and End show stay.
    assert_eq!(v["controls"]["cue"]["may"], true);
    assert_eq!(v["controls"]["end_show"]["may"], true);
    assert!(
        !commands(&p).iter().any(|c| c == "dump"),
        "{:?}",
        commands(&p)
    );

    // Callers: Dump is there, the overlap is not.
    let (code, v) = send(
        &p,
        "PUT",
        "/api/v1/shows/steeped/step",
        Some(ADMIN),
        json!({"step": "callers"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    look(&p).await;
    p.desk.lock().unwrap().delay_ms = 8_200;
    p.state.hub.poll_once().await;
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["controls"]["dump"]["may"], true, "{v}");
    assert_eq!(v["controls"]["bring"]["may"], false);
    let (code, d) = press(&p, &finlay.monitor, "dump", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["done"], "dump");
    assert_eq!(d["lost_ms"], 8_200, "how much listeners lost: {d}");
    assert_eq!(commands(&p).last().unwrap(), "dump");
    // The view says DUMPED, and how many seconds listeners lost.
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["dumped"]["seconds"], 8, "{v}");
    assert_eq!(v["dumped"]["lost_ms"], 8_200, "{v}");
    assert_eq!(
        v["dumped"]["words"],
        "Dumped. Listeners lost 8 s. Carry on, and do not repeat the words."
    );
    // The desk has nothing more to throw away: its refusal reaches the host.
    let (code, e) = press(&p, &finlay.monitor, "dump", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert!(why(&e).contains("nothing to dump"), "{e}");
    // A Dump from the desk's own route says DUMPED on the link too, with its own figure.
    p.desk.lock().unwrap().delay_ms = 5_000;
    p.state.hub.poll_once().await;
    let (code, v) = send(
        &p,
        "POST",
        "/api/v1/desk/commands",
        Some(ADMIN),
        json!({"op": "dump"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["dumped"]["seconds"], 5, "the desk's dump: {v}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn end_my_show_is_scoped_to_the_host_and_passes_the_air_on() {
    // No late count: the schedule does not put him on air by itself, the test does.
    let p = plane(|r| r["late_count_s"] = json!(0)).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    episode(&p, "steeped", -600, 60).await;
    episode(&p, "kettle", 3000, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(30_000)).await;
    let heather = host(&p, "kettle", "Heather Douglas", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();

    // Not on air: nothing to end.
    look(&p).await;
    let (code, e) = press(&p, &finlay.monitor, "end-show", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "You are not on air.");

    // On air beside a DJ: only he leaves, the DJ keeps the air, nothing is passed on and
    // nothing goes to automation.
    dj(&p, "dj-duncan", "Duncan Boyd", true);
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["controls"]["end_show_says"], "Duncan Boyd stays on air.");
    let (code, d) = press(&p, &finlay.monitor, "end-show", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(commands(&p), [format!("off:{}", finlay.identity)]);
    assert!(
        d["passed_to"].is_null() && d["automation_in_ms"].is_null(),
        "{d}"
    );
    assert!(
        p.desk
            .lock()
            .unwrap()
            .people
            .iter()
            .any(|q| q.identity == "dj-duncan" && q.on_air),
        "the DJ still holds the air"
    );

    // Alone on air with Heather standing by: the air passes to her as he leaves (the desk's own
    // End show, for him).
    p.desk
        .lock()
        .unwrap()
        .people
        .retain(|q| q.identity != "dj-duncan");
    p.desk.lock().unwrap().commands.clear();
    set_on_air(&p, &finlay.identity, true);
    // The desk's status is read before the post's look, so the log knows he is on air.
    look(&p).await;
    let (code, d) = press(&p, &finlay.monitor, "end-show", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["passed_to"], "Heather Douglas", "{d}");
    assert!(
        d["automation_in_ms"].is_null(),
        "nothing goes to automation: {d}"
    );
    assert_eq!(
        commands(&p),
        [
            format!("on:{}", heather.identity),
            format!("off:{}", finlay.identity)
        ]
    );
    assert!(
        log_of(&p, &heather.key)
            .await
            .contains(&("on_air".into(), Some("Finlay Grant (monitor)".into())))
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn ending_my_show_with_nobody_after_goes_to_automation_once_the_delay_has_played_out() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    episode(&p, "steeped", -600, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["controls"]["end_show_says"], "Automation takes the air.");
    assert!(v["next_host"].is_null(), "{v}");
    let (code, d) = press(&p, &finlay.monitor, "end-show", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert!(d["passed_to"].is_null());
    assert!(d["automation_in_ms"].is_u64(), "{d}");
    assert_eq!(commands(&p), [format!("off:{}", finlay.identity)]);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn only_this_hosts_secret_acts_for_this_host_and_an_ended_one_for_nobody() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    episode(&p, "steeped", -600, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;
    let mut n = 0;
    for route in ["go", "not-yet", "cue", "handover", "end-show", "dump"] {
        // No secret, a made-up one, a station key: none acts (each from its own address, for
        // wrong secrets are counted by address).
        for bearer in [None, Some("abcdefghabcdefghabcdefgh"), Some(ADMIN)] {
            n += 1;
            let (code, e) = call(
                &p,
                "POST",
                &format!("/api/v1/monitor/{route}"),
                bearer,
                json!({"step": "bring"}),
                &format!("192.0.2.{}:4000", 100 + n),
            )
            .await;
            assert_eq!(code, StatusCode::UNAUTHORIZED, "{route} {bearer:?}: {e}");
        }
    }
    assert!(commands(&p).is_empty());
    // The key ends: every route says so in the page's words, and nothing is done.
    let (code, _) = send(
        &p,
        "DELETE",
        &format!("/api/v1/shows/steeped/keys/{}", finlay.key),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    for route in ["go", "not-yet", "cue", "handover", "end-show", "dump"] {
        let (code, e) = press(&p, &finlay.monitor, route, json!({"step": "bring"})).await;
        assert_eq!(code, StatusCode::GONE, "{route}: {e}");
        assert_eq!(
            why(&e),
            "This link no longer works. Ask the station for a new one."
        );
    }
    assert!(commands(&p).is_empty());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn every_control_refuses_what_is_not_this_hosts_to_do_now_and_says_why() {
    let p = plane(|r| r["late_count_s"] = json!(0)).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    episode(&p, "steeped", -600, 60).await;
    episode(&p, "kettle", 3000, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(30_000)).await;
    let heather = host(&p, "kettle", "Heather Douglas", Some(600_000)).await;
    p.onair.set_mode(StationMode::Live).await.unwrap();
    // A producer has Heather in cue, and a DJ in the room holds the air.
    dj(&p, "dj-duncan", "Duncan Boyd", true);
    for q in &mut p.desk.lock().unwrap().people {
        if q.identity == heather.identity {
            q.cue = true;
        }
    }
    look(&p).await;

    // Standing by, not on air: none of the on-air controls is his; a producer's cue stays.
    for route in ["cue", "end-show", "dump"] {
        let (code, e) = press(&p, &finlay.monitor, route, Value::Null).await;
        assert_eq!(code, StatusCode::CONFLICT, "{route}: {e}");
        assert_eq!(why(&e), "You are not on air.", "{route}");
    }
    let (code, e) = press(&p, &finlay.monitor, "cue", json!({"on": false})).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(
        why(&e),
        "You are not on air.",
        "taking a cue out is the on-air host's too"
    );
    let (code, e) = press(&p, &finlay.monitor, "handover", json!({"step": "bring"})).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(why(&e), "You are not on air.");
    // Go while a DJ holds the air.
    let (code, e) = press(&p, &finlay.monitor, "go", Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(
        why(&e),
        "Duncan Boyd is still on air. You take the air when they hand over, or when the schedule does."
    );
    assert!(
        p.desk
            .lock()
            .unwrap()
            .people
            .iter()
            .any(|q| q.identity == heather.identity && q.cue),
        "the producer's cue is untouched"
    );
    assert!(commands(&p).is_empty(), "{:?}", commands(&p));

    // On air with nobody after: Cue and the handover have no one to work on.
    p.desk
        .lock()
        .unwrap()
        .people
        .retain(|q| q.identity != "dj-duncan" && q.identity != heather.identity);
    set_on_air(&p, &finlay.identity, true);
    look(&p).await;
    for (route, body) in [
        ("cue", Value::Null),
        ("handover", json!({"step": "bring"})),
        ("handover", json!({"step": "hand_over"})),
    ] {
        let (code, e) = press(&p, &finlay.monitor, route, body).await;
        assert_eq!(code, StatusCode::CONFLICT, "{route}: {e}");
        assert!(
            why(&e).starts_with("Heather Douglas has not connected yet.")
                || why(&e) == "Nobody is standing by after you.",
            "{route}: {e}"
        );
    }
    // And the same words for her not being there yet, through the view.
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["controls"]["cue"]["may"], false);
    assert_eq!(v["then"]["kind"], "keeps_air", "{v}");
    assert_eq!(
        v["then"]["words"],
        "Heather Douglas has not connected. You keep the air."
    );
    assert!(commands(&p).is_empty(), "{:?}", commands(&p));
}

/// The next whole SSE frame of a body, with a deadline.
async fn frame(body: &mut axum::body::BodyDataStream, within: Duration) -> Option<String> {
    use tokio_stream::StreamExt;
    let mut out = String::new();
    loop {
        let next = tokio::time::timeout(within, body.next())
            .await
            .ok()??
            .ok()?;
        out.push_str(&String::from_utf8_lossy(&next));
        if out.contains("\n\n") {
            return Some(out);
        }
    }
}

fn data_of(frame: &str) -> Value {
    let line = frame
        .lines()
        .find_map(|l| l.strip_prefix("data:"))
        .expect("a data line");
    serde_json::from_str(line.trim()).expect("JSON")
}

async fn stream(p: &Plane, bearer: &str) -> axum::response::Response {
    let mut req = Request::builder()
        .method("GET")
        .uri("/api/v1/monitor/events")
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "192.0.2.60:4000".parse::<SocketAddr>().unwrap(),
    ));
    p.app.clone().oneshot(req).await.unwrap()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_stream_sends_the_view_every_second_ends_with_the_key_and_takes_the_seats_out() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    episode(&p, "steeped", -600, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    p.state.hub.poll_once().await;

    // No secret in the address, a wrong one in the header: refused as the view is.
    let (code, _) = send(&p, "GET", "/api/v1/monitor/events", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let res = stream(&p, "abcdefghabcdefghabcdefgh").await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = stream(&p, &finlay.monitor).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = res.into_body().into_data_stream();
    let first = frame(&mut body, Duration::from_secs(5))
        .await
        .expect("a frame");
    assert!(first.contains("event: view"), "{first}");
    let v = data_of(&first);
    assert_eq!(v["state"], "standby");
    assert_eq!(v["label"], "Finlay Grant");
    let t1 = v["station"]["now_ms"].as_u64().unwrap();
    let second = frame(&mut body, Duration::from_secs(5))
        .await
        .expect("a second frame");
    let t2 = data_of(&second)["station"]["now_ms"].as_u64().unwrap();
    assert!(
        t2 > t1 && t2 - t1 < 5_000,
        "the clock moves: {t1} then {t2}"
    );

    // He goes on air: the stream says so within a couple of looks.
    set_on_air(&p, &finlay.identity, true);
    p.state.hub.poll_once().await;
    let mut said = false;
    for _ in 0..4 {
        let f = frame(&mut body, Duration::from_secs(5))
            .await
            .expect("a frame");
        if data_of(&f)["state"] == "on_air" {
            said = true;
            break;
        }
    }
    assert!(said, "the stream carried the change");

    // The key ends: the stream says so and closes, and both seats leave the monitor room.
    let (code, _) = send(
        &p,
        "DELETE",
        &format!("/api/v1/shows/steeped/keys/{}", finlay.key),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let mut ended = None;
    for _ in 0..4 {
        match frame(&mut body, Duration::from_secs(5)).await {
            Some(f) if f.contains("event: ended") => {
                ended = Some(f);
                break;
            }
            Some(_) => {}
            None => break,
        }
    }
    let ended = ended.expect("an `ended` event");
    assert_eq!(
        data_of(&ended)["error"],
        "This link no longer works. Ask the station for a new one."
    );
    assert!(
        frame(&mut body, Duration::from_secs(3)).await.is_none(),
        "the stream closes after it"
    );
    let room = format!("studio-monitor-{}", finlay.key);
    let seats = weavesignal_proto::monitor_seats(&finlay.key);
    let removed = p.removed.lock().unwrap().clone();
    for seat in &seats {
        assert!(
            removed.contains(&(room.clone(), seat.clone())),
            "{seat} was not removed from {room}: {removed:?}"
        );
    }
    // And a new connection is refused with the same words.
    let res = stream(&p, &finlay.monitor).await;
    assert_eq!(res.status(), StatusCode::GONE);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_link_holds_six_streams_and_a_seventh_waits() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    episode(&p, "steeped", -600, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", None).await;
    let mut open = Vec::new();
    for _ in 0..6 {
        let res = stream(&p, &finlay.monitor).await;
        assert_eq!(res.status(), StatusCode::OK);
        open.push(res);
    }
    let res = stream(&p, &finlay.monitor).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res.headers().contains_key(header::RETRY_AFTER));
    // A page that closes frees its place within a look.
    drop(open);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let res = stream(&p, &finlay.monitor).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn ending_a_key_by_renewing_it_takes_its_seats_out_too() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    let finlay = host(&p, "steeped", "Finlay Grant", None).await;
    let (code, v) = send(
        &p,
        "POST",
        &format!("/api/v1/shows/steeped/keys/{}/renew", finlay.key),
        Some(ADMIN),
        json!({"old": "now"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let removed = p.removed.lock().unwrap().clone();
    let room = format!("studio-monitor-{}", finlay.key);
    assert_eq!(
        removed
            .iter()
            .filter(|(r, _)| *r == room)
            .map(|(_, i)| i.clone())
            .collect::<Vec<_>>(),
        weavesignal_proto::monitor_seats(&finlay.key)
    );
    // The new key's link is another's.
    let new = v["monitor_secret"].as_str().unwrap();
    assert_eq!(view(&p, new).await["state"], "not_connected");
    let (code, _) = send(
        &p,
        "GET",
        "/api/v1/monitor",
        Some(&finlay.monitor),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::GONE);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn deleting_a_show_takes_every_one_of_its_keys_seats_out_with_no_stream_open() {
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    make_show(&p, "kettle", "Kettle Drive", "full").await;
    let finlay = host(&p, "steeped", "Finlay Grant", None).await;
    let shona = host(&p, "steeped", "Shona Reid", None).await;
    let heather = host(&p, "kettle", "Heather Douglas", None).await;
    // No page has an event stream open: nothing but the delete can take the seats out.
    let (code, v) = send(
        &p,
        "DELETE",
        "/api/v1/shows/steeped",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let removed = p.removed.lock().unwrap().clone();
    for gone in [&finlay, &shona] {
        let room = format!("studio-monitor-{}", gone.key);
        assert_eq!(
            removed
                .iter()
                .filter(|(r, _)| *r == room)
                .map(|(_, i)| i.clone())
                .collect::<Vec<_>>(),
            weavesignal_proto::monitor_seats(&gone.key),
            "{}'s two seats leave their room",
            gone.key
        );
    }
    assert!(
        !removed
            .iter()
            .any(|(r, _)| *r == format!("studio-monitor-{}", heather.key)),
        "another show's key keeps its seats: {removed:?}"
    );
    // Their links answer as an ended one does; the other show's still works.
    let (code, _) = send(
        &p,
        "GET",
        "/api/v1/monitor",
        Some(&finlay.monitor),
        Value::Null,
    )
    .await;
    assert!(
        code == StatusCode::GONE || code == StatusCode::UNAUTHORIZED,
        "{code}"
    );
    assert_eq!(view(&p, &heather.monitor).await["state"], "not_connected");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_next_insert_is_what_the_log_can_say_of_it_and_no_more() {
    use weavesignal_model::playout::{LogItem, insert_items};
    let p = plane(|_| {}).await;
    make_show(&p, "steeped", "Steeped", "full").await;
    episode(&p, "steeped", -600, 60).await;
    let finlay = host(&p, "steeped", "Finlay Grant", Some(600_000)).await;
    assert!(view(&p, &finlay.monitor).await["insert"].is_null());
    let now = weavesignal_control::hub::now_ms() as i64;
    let item = |at: i64, kind: &str, ms: u32| LogItem {
        id: 0,
        planned_at_ms: at,
        hour_ms: now / 3_600_000 * 3_600_000,
        clock: None,
        slot: 0,
        kind: kind.into(),
        track: None,
        cart: None,
        category: String::new(),
        duration_ms: ms,
        status: "planned".into(),
        note: String::new(),
    };
    insert_items(
        &p.pool,
        p.state.station.id,
        &[
            item(now + 30_000, "track", 200_000),
            item(now + 168_000, "cart", 7_000),
            item(now + 400_000, "cart", 3_000),
        ],
    )
    .await
    .unwrap();
    let v = view(&p, &finlay.monitor).await;
    assert_eq!(v["insert"]["at_ms"], now + 168_000, "{v}");
    assert_eq!(v["insert"]["duration_ms"], 7_000);
    assert_eq!(
        v["insert"]["plays"], "unknown",
        "over the show or instead of it is not in the log's model"
    );
}
