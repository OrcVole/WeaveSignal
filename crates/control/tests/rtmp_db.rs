// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out's destinations against a real PostgreSQL (ADR-0230, `rtmp-out`): an admin adds,
//! changes, turns and removes them; the stream key is sealed in the database and never in an
//! answer; the desk (a stand-in on a Unix socket) is sent the set with the keys, and again
//! after it restarts. Ignored unless asked for (`just test-db`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::api::{Api, ApiConfig, layer};
use weavesignal_control::apikey::ApiKey;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::rtmp;
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const KEY: &str = "rtmp-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";
const STREAM_KEY: &str = "sk_live_rumble_42?d=1&s=2";

/// A desk with video: answers `status` with the set it holds, and keeps each `rtmp` request.
#[derive(Default)]
struct FakeDesk {
    set: Option<String>,
    sent: Vec<Value>,
}

fn fake_desk(path: &std::path::Path) -> Arc<Mutex<FakeDesk>> {
    let desk = Arc::new(Mutex::new(FakeDesk::default()));
    let listener = UnixListener::bind(path).unwrap();
    let d = desk.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let d = d.clone();
            std::thread::spawn(move || {
                let mut w = conn.try_clone().unwrap();
                for line in BufReader::new(conn).lines() {
                    let Ok(line) = line else { return };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let answer = match req["op"].as_str() {
                        Some("status") => {
                            let set = d.lock().unwrap().set.clone();
                            json!({"ok": true, "people": [], "playout": {"connected": false},
                                   "video": {"rtmp": {"set": set, "destinations": [], "encoders": []}}})
                        }
                        Some("rtmp") => {
                            let mut g = d.lock().unwrap();
                            g.set = req["set"].as_str().map(str::to_owned);
                            g.sent.push(req);
                            json!({"ok": true})
                        }
                        _ => json!({"ok": false, "error": "unknown"}),
                    };
                    if writeln!(w, "{answer}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    desk
}

async fn setup() -> (
    Router,
    Arc<AppState>,
    sqlx::PgPool,
    Arc<Mutex<FakeDesk>>,
    PathBuf,
) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "RTMP FM").unwrap(),
    )
    .await
    .unwrap();
    let dir = std::env::temp_dir().join(format!("weavesignal-rtmp-db-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("desk.sock");
    let desk = fake_desk(&socket);
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(Some(&socket)),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        Some(ApiKey::parse(TOKEN_KEY).unwrap()),
        ApiConfig::default(),
        true,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    let state = Arc::new(AppState {
        station,
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    });
    let app = layer(router(state.clone()), staff);
    (app, state, pool, desk, dir)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        req = req.header("authorization", format!("Bearer {b}"));
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_admin_keeps_rtmp_destinations_and_the_desk_gets_them_with_their_keys() {
    let (app, state, pool, desk, dir) = setup().await;

    // Only an admin.
    let (code, _) = send(&app, "GET", "/api/v1/rtmp", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    // Made: the key is not in the answer.
    let (code, made) = send(
        &app,
        "POST",
        "/api/v1/rtmp/destinations",
        Some(KEY),
        json!({"name": "Rumble", "platform": "rumble", "url": "rtmp://rtmp.rumble.com/live/", "key": STREAM_KEY}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    assert!(
        !made.to_string().contains("sk_live"),
        "the key came back: {made}"
    );
    assert_eq!(made["url"], "rtmp://rtmp.rumble.com/live");
    assert_eq!(made["profile"], "compatible");
    assert_eq!(made["enabled"], false);
    let id = made["id"].as_str().unwrap().to_owned();

    // Refused: a bad URL, a key with a space, the same name.
    for (body, field) in [
        (
            json!({"name": "x", "platform": "custom", "url": "http://x/live", "key": "k"}),
            "url",
        ),
        (
            json!({"name": "x", "platform": "custom", "url": "rtmp://x/live", "key": "a b"}),
            "key",
        ),
        (
            json!({"name": "x", "platform": "twitch", "url": "rtmp://x/live", "key": "k"}),
            "platform",
        ),
        (
            json!({"name": "x", "platform": "custom", "url": "rtmp://x/live", "key": "k", "profile": "h265"}),
            "profile",
        ),
    ] {
        let (code, v) = send(&app, "POST", "/api/v1/rtmp/destinations", Some(KEY), body).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        assert_eq!(v["field"], field);
    }
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/rtmp/destinations",
        Some(KEY),
        json!({"name": "Rumble", "platform": "custom", "url": "rtmp://x/live", "key": "k"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);

    // Sealed in the database: not there in the clear.
    let (sealed,): (Vec<u8>,) =
        sqlx::query_as("SELECT key_sealed FROM rtmp_destination WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !sealed.windows(7).any(|w| w == b"sk_live"),
        "the key is in the clear"
    );

    // The desk is sent the set, with the key.
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    {
        let d = desk.lock().unwrap();
        assert_eq!(d.sent.len(), 1);
        assert_eq!(d.sent[0]["destinations"][0]["key"], STREAM_KEY);
        assert_eq!(
            d.sent[0]["destinations"][0]["url"],
            "rtmp://rtmp.rumble.com/live"
        );
        assert_eq!(d.sent[0]["destinations"][0]["enabled"], false);
    }
    // Nothing more while the desk holds it.
    rtmp::sync(&state).await;
    assert_eq!(desk.lock().unwrap().sent.len(), 1);

    // Turned on: sent again, on.
    let (code, on) = send(
        &app,
        "PUT",
        &format!("/api/v1/rtmp/destinations/{id}/enabled"),
        Some(KEY),
        json!({"enabled": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{on}");
    assert_eq!(on["enabled"], true);
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    {
        let d = desk.lock().unwrap();
        assert_eq!(d.sent.last().unwrap()["destinations"][0]["enabled"], true);
    }

    // A change without a key keeps the key; with one, the desk gets the new one.
    let (code, ch) = send(
        &app,
        "PUT",
        &format!("/api/v1/rtmp/destinations/{id}"),
        Some(KEY),
        json!({"profile": "open"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{ch}");
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    assert_eq!(
        desk.lock().unwrap().sent.last().unwrap()["destinations"][0]["key"],
        STREAM_KEY
    );
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/rtmp/destinations/{id}"),
        Some(KEY),
        json!({"key": "sk_new"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    assert_eq!(
        desk.lock().unwrap().sent.last().unwrap()["destinations"][0]["key"],
        "sk_new"
    );

    // The desk restarts and holds no set: sent again.
    let n = desk.lock().unwrap().sent.len();
    desk.lock().unwrap().set = None;
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    assert_eq!(desk.lock().unwrap().sent.len(), n + 1);

    // Listed without the key, with its form for each platform.
    let (code, list) = send(&app, "GET", "/api/v1/rtmp", Some(KEY), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(!list.to_string().contains("sk_new"));
    assert_eq!(list["destinations"][0]["key_readable"], true);
    assert_eq!(list["video"], true);
    assert_eq!(
        list["platforms"].as_array().unwrap().len(),
        7,
        "YouTube, Owncast and X joined the first four (Gab not yet: its live streaming is not open)"
    );

    // Removed: the desk is sent an empty set.
    let (code, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/rtmp/destinations/{id}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    state.hub.poll_once().await;
    rtmp::sync(&state).await;
    assert_eq!(
        desk.lock().unwrap().sent.last().unwrap()["destinations"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every form the studio app offers can be saved (1.0.2's presets answered 500: the table's check
/// still named only the first four platforms; migration 0035).
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn every_platform_the_studio_offers_can_be_saved() {
    let (app, _state, _pool, _desk, dir) = setup().await;
    for p in rtmp::PLATFORMS {
        let url = if p.url.is_empty() {
            "rtmps://live.example/app"
        } else {
            p.url
        };
        let (code, made) = send(
            &app,
            "POST",
            "/api/v1/rtmp/destinations",
            Some(KEY),
            json!({"name": format!("Preset {}", p.platform), "platform": p.platform,
                   "url": url, "key": "k123", "profile": p.profile}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{}: {made}", p.platform);
        assert_eq!(made["platform"], p.platform);
    }
    let _ = std::fs::remove_dir_all(dir);
}
