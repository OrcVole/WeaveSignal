// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The monitor link's routes (ADR-0280, design 4.5) against a real PostgreSQL: the monitor secret
//! made with a key finds its host and no other; a wrong secret is refused and counted by
//! address; the host's own view says what the desk holds of their source and the station's clock;
//! the room token is a listener's for the key's monitor room with no recorder grant, two seats
//! and a third refused; an ended key's link says so. Ignored unless asked for (`just test-db`).

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
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
use weavesignal_control::config::RoomConfig;
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "sources-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";
const LK_SECRET: &str = "monitor-db-test-livekit-secret-0123456789";
const INGEST: &str = "ingest-test-only-ingest-test-only-0";

#[allow(dead_code)]
struct Plane {
    app: Router,
    state: Arc<AppState>,
    pool: sqlx::PgPool,
    station: Station,
}

async fn plane() -> Plane {
    plane_with_desk(None).await
}

async fn plane_with_desk(desk: Option<&std::path::Path>) -> Plane {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
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
        DeskClient::new(desk),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    let live = hub.live.clone();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sources-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: Some(RoomState {
            config: RoomConfig::default(),
            keys: weavesignal_proto::Keys {
                api_key: "monitor-test".into(),
                api_secret: LK_SECRET.into(),
            },
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
            cards_dir: dir.join("cards"),
            episodes_dir: dir.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: Some(live),
        hub: None,
        video: None,
    })));
    Plane {
        app: layer(app, staff),
        state,
        pool,
        station,
    }
}

/// A request; `peer` is the connection's address as the server would see it.
async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
    peer: Option<&str>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(b) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let mut req = req.body(body).unwrap();
    if let Some(p) = peer {
        req.extensions_mut()
            .insert(ConnectInfo(p.parse::<SocketAddr>().unwrap()));
    }
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, v)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    call(app, method, uri, bearer, body, Some("192.0.2.50:4000"), &[]).await
}

/// The ingest asking, from loopback with its bearer.
async fn ingest(app: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    call(
        app,
        method,
        uri,
        Some(INGEST),
        body,
        Some("127.0.0.1:40000"),
        &[],
    )
    .await
}

async fn make_show(app: &Router, slug: &str, name: &str) {
    let (code, v) = send(
        app,
        "PUT",
        &format!("/api/v1/shows/{slug}"),
        Some(ADMIN),
        json!({"name": name, "colour_slot": 3}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
}

/// Series 1, episode 1 of `slug`, starting `from_now_s` from now, for `minutes`.
async fn episode(app: &Router, slug: &str, from_now_s: i64, minutes: u32) {
    let (code, _) = send(
        app,
        "PUT",
        &format!("/api/v1/shows/{slug}/series/1"),
        Some(ADMIN),
        json!({}),
    )
    .await;
    assert!(code.is_success());
    let (code, body) = send(
        app,
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
        app,
        "PATCH",
        &format!("/api/v1/shows/{slug}/series/1/episodes/1"),
        Some(ADMIN),
        json!({"air_at": at, "minutes": minutes}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

async fn make_key(p: &Plane, show: &str, label: &str, hint: &str) -> Value {
    let (code, made) = send(
        &p.app,
        "POST",
        &format!("/api/v1/shows/{show}/keys"),
        Some(ADMIN),
        json!({"label": label, "hint": hint}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    made
}

async fn admit(p: &Plane, key_id: &str, secret: Option<&str>, way: &str, addr: &str) -> Value {
    let mut body = json!({"key_id": key_id, "way": way, "address": addr});
    if let Some(s) = secret {
        body["secret"] = json!(s);
    }
    let (code, v) = ingest(&p.app, "POST", "/api/v1/internal/sources/admit", body).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    v
}

/// A desk that holds one source, as the real desk reports it (`tests/source_in.rs` drives the
/// real one), and takes `on_air` for it.
fn fake_desk(path: &std::path::Path, key: String) -> Arc<Mutex<bool>> {
    let identity = format!("source-{key}");
    let on_air = Arc::new(Mutex::new(false));
    let listener = UnixListener::bind(path).unwrap();
    let flag = on_air.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let (flag, identity, key) = (flag.clone(), identity.clone(), key.clone());
            std::thread::spawn(move || {
                let mut w = conn.try_clone().unwrap();
                for line in BufReader::new(conn).lines() {
                    let Ok(line) = line else { return };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let answer = match req["op"].as_str() {
                        Some("status") => {
                            json!({"ok": true, "playout": {"connected": true}, "people": [{
                                "identity": identity, "name": "Finlay Grant", "role": "source",
                                "on_air": *flag.lock().unwrap(), "mute": false, "cue": false, "talk": false,
                                "talk_to": null, "present": true, "away_ms": null, "linked": true,
                                "lufs": -23.4, "lufs_peak": -20.1, "underruns": 0, "late_ms": 0, "ring_ms": 300,
                                "camera": false,
                                "source": {"way": "srt", "format": "mpegts/aac 48 kHz stereo", "key": key,
                                           "show": "steeped", "connected_ms": 12000, "leveller_db": -2.5, "level_db": -20.9, "monitor": true}
                            }]})
                        }
                        Some("on_air") => {
                            *flag.lock().unwrap() = req["on"].as_bool().unwrap_or(false);
                            json!({"ok": true})
                        }
                        _ => json!({"ok": true}),
                    };
                    if writeln!(w, "{answer}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    on_air
}

fn keys() -> weavesignal_proto::Keys {
    weavesignal_proto::Keys {
        api_key: "monitor-test".into(),
        api_secret: LK_SECRET.into(),
    }
}

const ENDED: &str = "This link no longer works. Ask the station for a new one.";

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_monitor_secret_finds_its_host_and_the_view_is_theirs_alone() {
    // A Unix socket's path must be short: the system's temporary directory.
    let dir = std::env::temp_dir().join(format!(
        "ws-mon-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("desk.sock");
    let p = plane_with_desk(Some(&sock)).await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    p.state.hub.zone.set("Europe/London");
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;
    make_show(&p.app, "kettle-drive", "Kettle Drive").await;
    episode(&p.app, "kettle-drive", -600, 120).await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let other = make_key(&p, "kettle-drive", "Shona Campbell", "icecast").await;
    let k = made["key"]["key_id"].as_str().unwrap().to_owned();
    let monitor = made["monitor_secret"].as_str().unwrap().to_owned();
    let secret = made["secret"].as_str().unwrap().to_owned();
    let other_monitor = other["monitor_secret"].as_str().unwrap().to_owned();
    let _on_air = fake_desk(&sock, k.clone());

    // No secret, a made-up one, the key's own secret, a station key: none is a monitor link.
    for bearer in [
        None,
        Some("abcdefghabcdefghabcdefgh"),
        Some(secret.as_str()),
        Some(ADMIN),
    ] {
        let (code, v) = call(
            &p.app,
            "GET",
            "/api/v1/monitor",
            bearer,
            Value::Null,
            None,
            &[],
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{bearer:?}: {v}");
        assert_eq!(v["error"], ENDED);
    }

    // Admitted and on the desk: the host sees their own source, with the desk's figures.
    admit(&p, &k, None, "srt", "198.51.100.4").await;
    p.state.hub.poll_once().await;
    let (code, v) = call(
        &p.app,
        "GET",
        "/api/v1/monitor",
        Some(&monitor),
        Value::Null,
        None,
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["label"], "Finlay Grant");
    assert_eq!(
        (v["show"].as_str(), v["show_name"].as_str()),
        (Some("steeped"), Some("Steeped"))
    );
    assert_eq!(v["state"], "standby");
    assert_eq!(v["way"], "srt");
    assert_eq!(v["format"], "mpegts/aac 48 kHz stereo");
    assert_eq!(v["level"], -23.4);
    assert_eq!(v["leveller_db"], -2.5);
    assert_eq!(
        v["monitor_open"], true,
        "the desk says the mix is open: {v}"
    );
    assert_eq!(v["station"]["zone"], "Europe/London");
    let now = weavesignal_control::hub::now_ms();
    let said = v["station"]["now_ms"].as_u64().unwrap();
    assert!(
        said <= now && now - said < 5_000,
        "the station's clock: {v}"
    );
    assert!(v["door_closes_ms"].is_i64(), "{v}");
    // Nothing of anyone else's but what the design shows (the next show and its host, in the
    // rundown), and no secret, no key and no slug, anywhere in it.
    let other_key = other["key"]["key_id"].as_str().unwrap();
    let text = v.to_string();
    for forbidden in [
        secret.as_str(),
        monitor.as_str(),
        other_monitor.as_str(),
        other_key,
        "kettle-drive",
    ] {
        assert!(!text.contains(forbidden), "{forbidden} in {v}");
    }
    let mut without_rundown = v.clone();
    without_rundown["rundown"] = Value::Null;
    assert!(
        !without_rundown.to_string().contains("Shona"),
        "another host's name is only in the rundown: {v}"
    );
    // Another host's link shows that host, not this one's.
    let (_, w) = call(
        &p.app,
        "GET",
        "/api/v1/monitor",
        Some(&other_monitor),
        Value::Null,
        None,
        &[],
    )
    .await;
    assert_eq!(w["label"], "Shona Campbell");
    assert_eq!(w["state"], "not_connected");
    assert_eq!(w["monitor_open"], false);
    assert!(w["way"].is_null() && w["level"].is_null(), "{w}");

    // The same secret is not a key's secret: it admits nothing.
    let (code, v) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/admit",
        json!({"key_id": k, "way": "icecast", "address": "198.51.100.4", "secret": monitor}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_ne!(
        v["decision"], "standby",
        "a monitor secret is not a key: {v}"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn wrong_monitor_secrets_are_counted_by_address_and_refused_after_five() {
    let p = plane().await;
    make_show(&p.app, "steeped", "Steeped").await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let monitor = made["monitor_secret"].as_str().unwrap().to_owned();
    let from = Some("203.0.113.9:5000");
    for _ in 0..5 {
        let (code, _) = call(
            &p.app,
            "GET",
            "/api/v1/monitor",
            Some("abcdefghabcdefghabcdefgh"),
            Value::Null,
            from,
            &[],
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }
    // Even the right one is refused from that address for now, and others are not.
    let (code, v) = call(
        &p.app,
        "GET",
        "/api/v1/monitor",
        Some(&monitor),
        Value::Null,
        from,
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS, "{v}");
    let (code, _) = call(
        &p.app,
        "GET",
        "/api/v1/monitor",
        Some(&monitor),
        Value::Null,
        Some("203.0.113.10:5000"),
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_room_token_is_a_listeners_for_the_monitor_room_two_seats_and_no_more() {
    let p = plane().await;
    make_show(&p.app, "steeped", "Steeped").await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let k = made["key"]["key_id"].as_str().unwrap().to_owned();
    let monitor = made["monitor_secret"].as_str().unwrap().to_owned();
    let post = |p: &Plane| {
        let app = p.app.clone();
        let monitor = monitor.clone();
        async move {
            call(
                &app,
                "POST",
                "/api/v1/monitor/token",
                Some(&monitor),
                Value::Null,
                None,
                &[],
            )
            .await
        }
    };
    // Without the secret: nothing.
    let (code, _) = call(
        &p.app,
        "POST",
        "/api/v1/monitor/token",
        None,
        Value::Null,
        None,
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    let (code, one) = post(&p).await;
    assert_eq!(code, StatusCode::CREATED, "{one}");
    let (code, two) = post(&p).await;
    assert_eq!(code, StatusCode::CREATED, "{two}");
    let (code, three) = post(&p).await;
    assert_eq!(
        code,
        StatusCode::CONFLICT,
        "a third browser is refused: {three}"
    );

    assert_eq!(one["room"], format!("studio-monitor-{k}"));
    assert_eq!(one["identity"], format!("monitor-{k}-1"));
    assert_eq!(two["identity"], format!("monitor-{k}-2"));
    assert_eq!(one["ttl_s"], 600);
    assert_eq!(one["url"], "wss://station.example");
    for t in [&one, &two] {
        let claims = keys().verify(t["token"].as_str().unwrap()).unwrap();
        assert_eq!(
            claims.video.room.as_deref(),
            Some(format!("studio-monitor-{k}").as_str())
        );
        assert_eq!(claims.sub, t["identity"].as_str().unwrap());
        assert_eq!(claims.role(), Some(weavesignal_proto::Role::Listener));
        assert_eq!(claims.video.can_publish, Some(false), "it only listens");
        assert_eq!(claims.video.can_subscribe, Some(true));
        assert!(
            !claims.video.recorder,
            "no recorder grant: only what the desk allows its seats"
        );
        assert!(!claims.video.room_admin && !claims.video.hidden);
        assert!(claims.exp - claims.nbf <= 610, "a short life");
    }
    assert_eq!(
        weavesignal_proto::monitor_seats(&k),
        vec![
            one["identity"].as_str().unwrap().to_owned(),
            two["identity"].as_str().unwrap().to_owned()
        ]
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_ended_key_ends_its_monitor_link_with_the_words_the_page_shows() {
    let p = plane().await;
    make_show(&p.app, "steeped", "Steeped").await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let k = made["key"]["key_id"].as_str().unwrap().to_owned();
    let monitor = made["monitor_secret"].as_str().unwrap().to_owned();
    let (code, _) = call(
        &p.app,
        "GET",
        "/api/v1/monitor",
        Some(&monitor),
        Value::Null,
        None,
        &[],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, v) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/shows/steeped/keys/{k}"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    for (method, uri) in [
        ("GET", "/api/v1/monitor"),
        ("POST", "/api/v1/monitor/token"),
    ] {
        let (code, v) = call(&p.app, method, uri, Some(&monitor), Value::Null, None, &[]).await;
        assert_eq!(code, StatusCode::GONE, "{method} {uri}: {v}");
        assert_eq!(v["error"], ENDED);
    }
}
