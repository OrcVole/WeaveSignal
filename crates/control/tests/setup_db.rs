// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station's setup page and its look (ADR-0237) through the control plane's routes, against
//! a real PostgreSQL and the real media worker (this crate's binary, as `media-worker brand`):
//!
//! - **the lock**: no bearer, a wrong key, a presenter, a producer, an API token, and a station
//!   with no key and no staff are all refused, on every route that shows or changes anything;
//! - **what is stored**: the name and the contact win over the settings file, are in force at
//!   once (the player, the consent words and their version, call-in no longer waiting for a
//!   contact), survive a restart,
//!   and hand back to the file when cleared;
//! - **when setup is offered**: pending for the demo name or no contact, done by the settings
//!   file when it gives both, done for good once finished;
//! - **the look**: a colour, a picture re-encoded by the worker (an SVG, a damaged file and a
//!   body over the cap refused; the stored PNG is not the upload), served open with its version;
//! - **the stream password**: refused where the install has none, written to the overrides file
//!   where it has, never read back.
//!
//! Ignored unless asked for (`just test-db`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::api::{Api, layer};
use weavesignal_control::apikey::ApiKey;
use weavesignal_control::app::{AppState, RoomState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, RoomConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::setup::{self, InstallFacts, SetupState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const KEY: &str = "setup-db-test-station-key";
const TOKEN_KEY: &str = "setup-db-test-token-key-setup-db-test-token-key";

struct Plane {
    app: Router,
    overrides: PathBuf,
}

struct Opts {
    /// The settings file's name and contact.
    file_name: &'static str,
    file_contact: &'static str,
    /// The station key; `None` is a station with no key.
    key: Option<&'static str>,
    /// Whether the install has a stream password (the Cloudron package).
    stream_password: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            file_name: "Kettle FM",
            file_contact: "",
            key: Some(KEY),
            stream_password: false,
        }
    }
}

async fn database() -> sqlx::PgPool {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    sqlx::PgPool::connect(&url).await.expect("connect")
}

/// A control plane as `serve` builds it, over `id`'s station; a second over the same `id` is a
/// restart.
async fn plane(pool: &sqlx::PgPool, id: StationId, o: Opts) -> Plane {
    let station = ensure_station(pool, &Station::new(id, o.file_name).unwrap())
        .await
        .unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    hub.live.set_file_name(o.file_name);
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        Some(ApiKey::parse(TOKEN_KEY).unwrap()),
        Default::default(),
        true,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    staff.restore(station.id).await;
    staff.api.restore().await;
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("setup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let overrides = dir.join("setup.env");
    let setup_state = Arc::new(SetupState {
        station: station.id,
        db: Some(pool.clone()),
        admin_token: o.key.map(str::to_owned),
        staff: staff.clone(),
        hub: hub.clone(),
        file_contact: o.file_contact.to_owned(),
        overrides_file: o.stream_password.then(|| overrides.clone()),
        stream_auth: o.stream_password.then_some(false),
        facts: InstallFacts {
            public_url: "https://station.example".into(),
            stream_url: "https://listen.station.example/programme.opus".into(),
            room_url: "wss://room.station.example".into(),
            media_port: 7882,
            turn_tls_port: 15349,
            mail: "off".into(),
            listed: false,
        },
        carts: CartsConfig {
            worker: Some(env!("CARGO_BIN_EXE_weavesignal-control").into()),
            ..CartsConfig::default()
        },
        started: std::time::SystemTime::now() - Duration::from_secs(5),
    });
    setup::restore(&setup_state).await;
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        // A room, so that the one thing keeping call-in closed is what these tests change.
        room: Some(RoomState {
            config: RoomConfig::default(),
            keys: weavesignal_proto::Keys {
                api_key: "weavesignal".into(),
                api_secret: "check-only-check-only-check-only-0".into(),
            },
        }),
        admin_token: o.key.map(str::to_owned),
        staff: staff.clone(),
        hub,
        join: JoinConfig {
            consent_contact: o.file_contact.to_owned(),
            ..JoinConfig::default()
        },
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 15349,
        steering: Default::default(),
    }))
    .merge(setup::router(setup_state));
    Plane {
        app: layer(app, staff),
        overrides,
    }
}

async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(b) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let (code, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (code, headers, bytes)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let bytes = if body.is_null() {
        Vec::new()
    } else {
        body.to_string().into_bytes()
    };
    let (code, _, bytes) = raw(app, method, uri, bearer, "application/json", bytes).await;
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, v)
}

async fn upload(
    app: &Router,
    what: &str,
    bearer: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let (code, _, bytes) = raw(
        app,
        "PUT",
        &format!("/api/v1/branding/{what}"),
        bearer,
        "application/octet-stream",
        body,
    )
    .await;
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, v)
}

/// A PNG of one colour, with a comment in it as a camera or an editor leaves metadata.
fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.add_text_chunk("Comment".into(), "made at 55.95 N 3.19 W".into())
        .unwrap();
    let mut w = e.write_header().unwrap();
    let data: Vec<u8> = rgba
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    w.write_image_data(&data).unwrap();
    w.finish().unwrap();
    out
}

fn size_of(png: &[u8]) -> (u32, u32) {
    let d = png::Decoder::new(std::io::Cursor::new(png));
    let r = d.read_info().unwrap();
    (r.info().width, r.info().height)
}

/// Whether the player says call-in waits for a takedown contact. (No desk runs here, so
/// call-in is never open; with a contact the reason given is the desk's.)
fn waits_for_contact(player: &Value) -> bool {
    player["call_in"]["why_not"]
        .as_str()
        .is_some_and(|w| w.contains("has not said how to ask for a call to be taken down"))
}

/// A member of staff made with the station key, signed in: their session.
async fn staff(app: &Router, username: &str, role: &str) -> String {
    let phrase = format!("{username}'s long pass phrase");
    let (code, v) = send(
        app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": username, "name": username, "role": role, "password": phrase}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let (code, v) = send(
        app,
        "POST",
        "/api/v1/sessions",
        None,
        json!({"username": username, "password": phrase}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    v["token"].as_str().unwrap().to_owned()
}

/// Every route that shows or changes the setup or the look, with a body that would be taken.
fn locked_routes() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("GET", "/api/v1/setup", Value::Null),
        ("PUT", "/api/v1/setup", json!({"name": "Taken Over FM"})),
        ("POST", "/api/v1/setup/finish", Value::Null),
        (
            "PUT",
            "/api/v1/setup/stream-password",
            json!({"enabled": true, "password": "taken-over-2026"}),
        ),
        (
            "PUT",
            "/api/v1/branding/accent",
            json!({"accent": "#ff0000"}),
        ),
        ("DELETE", "/api/v1/branding/icon", Value::Null),
        ("DELETE", "/api/v1/branding/banner", Value::Null),
    ]
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_setup_routes_open_only_to_an_admin() {
    let pool = database().await;
    let id = StationId::new_random();
    let p = plane(
        &pool,
        id,
        Opts {
            stream_password: true,
            ..Opts::default()
        },
    )
    .await;
    let presenter = staff(&p.app, "isla", "presenter").await;
    let producer = staff(&p.app, "hamish", "producer").await;
    let screener = staff(&p.app, "ailsa", "screener").await;
    let admin = staff(&p.app, "morag", "admin").await;
    // An API token with every scope there is: still not an admin (ADR-0195).
    let scopes: Vec<&str> = weavesignal_control::scopes::SCOPES
        .iter()
        .map(|s| s.0)
        .collect();
    let (code, made) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(KEY),
        json!({"name": "everything", "scopes": scopes}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let token = made["secret"].as_str().unwrap().to_owned();
    let icon = png(128, 128, [200, 30, 30, 255]);

    for (method, uri, body) in locked_routes() {
        for (who, bearer, want) in [
            ("nobody", None, StatusCode::UNAUTHORIZED),
            (
                "a wrong key",
                Some("not-the-station-key"),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "an ended session",
                Some("rss_0000000000000000000000000000000000000000"),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "a presenter",
                Some(presenter.as_str()),
                StatusCode::FORBIDDEN,
            ),
            ("a producer", Some(producer.as_str()), StatusCode::FORBIDDEN),
            ("a screener", Some(screener.as_str()), StatusCode::FORBIDDEN),
            ("an API token", Some(token.as_str()), StatusCode::FORBIDDEN),
        ] {
            let (code, v) = send(&p.app, method, uri, bearer, body.clone()).await;
            assert_eq!(code, want, "{who}: {method} {uri}: {v}");
        }
    }
    // The uploads are refused before their body is read, by the same rule.
    for what in ["icon", "banner"] {
        for (who, bearer, want) in [
            ("nobody", None, StatusCode::UNAUTHORIZED),
            (
                "a wrong key",
                Some("not-the-station-key"),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "a presenter",
                Some(presenter.as_str()),
                StatusCode::FORBIDDEN,
            ),
            ("an API token", Some(token.as_str()), StatusCode::FORBIDDEN),
        ] {
            let (code, v) = upload(&p.app, what, bearer, icon.clone()).await;
            assert_eq!(code, want, "{who}: PUT {what}: {v}");
        }
    }
    // Nothing any of them sent took effect.
    let (code, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["name"]["value"], "Kettle FM");
    assert_eq!(v["status"], "pending");
    assert_eq!(
        v["branding"],
        json!({"accent": null, "icon": null, "banner": null})
    );
    assert!(!p.overrides.exists());
    // The station key and an admin's session both open them.
    let (code, v) = send(&p.app, "GET", "/api/v1/setup", Some(&admin), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup",
        Some(&admin),
        json!({"name": "Radio Ceilidh"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["updated_by"], "morag");
    // The open routes show only the look, and never the setup.
    let (code, v) = send(&p.app, "GET", "/api/v1/branding", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v, json!({"accent": null, "icon": null, "banner": null}));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_station_with_no_key_and_no_staff_stays_closed() {
    let pool = database().await;
    let p = plane(
        &pool,
        StationId::new_random(),
        Opts {
            key: None,
            ..Opts::default()
        },
    )
    .await;
    for (method, uri, body) in locked_routes() {
        for bearer in [None, Some(""), Some("anything"), Some("setup")] {
            let (code, v) = send(&p.app, method, uri, bearer, body.clone()).await;
            assert!(
                code == StatusCode::SERVICE_UNAVAILABLE || code == StatusCode::UNAUTHORIZED,
                "{method} {uri} with {bearer:?}: {code} {v}"
            );
        }
    }
    let (code, _) = upload(
        &p.app,
        "icon",
        Some("anything"),
        png(128, 128, [1, 2, 3, 255]),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let (code, v) = send(&p.app, "GET", "/api/v1/station", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["name"], "Kettle FM");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_name_and_contact_win_over_the_settings_file_at_once_and_after_a_restart() {
    let pool = database().await;
    let id = StationId::new_random();
    let p = plane(&pool, id, Opts::default()).await;

    // A new station: the demo's name, no contact, call-in closed, setup offered.
    let (_, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(v["status"], "pending");
    assert_eq!(v["done_by"], Value::Null);
    assert_eq!(v["missing"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["name"],
        json!({"value": "Kettle FM", "source": "settings", "settings_value": "Kettle FM"})
    );
    assert_eq!(
        v["contact"],
        json!({"value": "", "source": "settings", "settings_value": ""})
    );
    assert_eq!(v["stream_password"]["available"], false);
    assert_eq!(v["install"]["turn_tls_port"], 15349);
    let (_, before) = send(&p.app, "GET", "/api/v1/player", None, Value::Null).await;
    assert_eq!(before["station"], "Kettle FM");
    assert!(waits_for_contact(&before), "{before}");

    // What is refused, with the field named.
    for (body, field) in [
        (json!({"name": "x".repeat(201)}), "name"),
        (json!({"name": "Two\nLines"}), "name"),
        (json!({"contact": "c".repeat(201)}), "contact"),
        (
            json!({"contact": "calls@station.example\nand more"}),
            "contact",
        ),
    ] {
        let (code, v) = send(&p.app, "PUT", "/api/v1/setup", Some(KEY), body).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        assert_eq!(v["field"], field, "{v}");
    }
    let (code, _) = send(
        &p.app,
        "PUT",
        "/api/v1/setup",
        Some(KEY),
        json!({"nme": "typo"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    // Saved: in force at once, everywhere the control plane says the name or the contact.
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup",
        Some(KEY),
        json!({"name": "  Radio Ceilidh  ", "contact": "calls@station.example"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        v["name"],
        json!({"value": "Radio Ceilidh", "source": "page", "settings_value": "Kettle FM"})
    );
    assert_eq!(v["contact"]["value"], "calls@station.example");
    assert_eq!(v["contact"]["source"], "page");
    assert_eq!(v["updated_by"], "station key");
    // Not finished, but nothing is missing: no longer offered.
    assert_eq!(v["status"], "done");
    assert_eq!(v["done_by"], "settings");
    let (_, after) = send(&p.app, "GET", "/api/v1/player", None, Value::Null).await;
    assert_eq!(after["station"], "Radio Ceilidh");
    assert!(!waits_for_contact(&after), "{after}");
    let words = after["call_in"]["consent_text"].as_str().unwrap();
    assert!(
        words.contains("Radio Ceilidh") && words.contains("calls@station.example"),
        "{words}"
    );
    // The words changed, so their version did: callers are asked again (ADR-0124).
    assert_ne!(
        after["call_in"]["consent_version"],
        before["call_in"]["consent_version"]
    );
    let (_, v) = send(&p.app, "GET", "/api/v1/station", None, Value::Null).await;
    assert_eq!(v["name"], "Radio Ceilidh");
    let (_, v) = send(&p.app, "GET", "/api/v1/studio", Some(KEY), Value::Null).await;
    assert_eq!(v["station"], "Radio Ceilidh", "{v}");
    // The database's own row for the station, which the chat reads when it starts.
    let (stored,): (String,) = sqlx::query_as("SELECT name FROM station WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "Radio Ceilidh");

    // A restart with the same settings file (as Cloudron rewrites control.toml): the page's
    // answers are still in force, and the file's name did not come back.
    let p = plane(&pool, id, Opts::default()).await;
    let (_, v) = send(&p.app, "GET", "/api/v1/player", None, Value::Null).await;
    assert_eq!(v["station"], "Radio Ceilidh");
    assert!(!waits_for_contact(&v), "{v}");
    assert_eq!(
        v["call_in"]["consent_version"],
        after["call_in"]["consent_version"]
    );
    let (stored,): (String,) = sqlx::query_as("SELECT name FROM station WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "Radio Ceilidh");

    // Finished: a time is kept, once.
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/setup/finish",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["done_by"], "page");
    let at = v["completed_at_ms"].as_u64().unwrap();
    let (_, v) = send(
        &p.app,
        "POST",
        "/api/v1/setup/finish",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(v["completed_at_ms"].as_u64().unwrap(), at);
    // Saving again does not undo the finish.
    let (_, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup",
        Some(KEY),
        json!({"name": "Radio Ceilidh", "contact": "calls@station.example"}),
    )
    .await;
    assert_eq!(v["completed_at_ms"].as_u64().unwrap(), at);

    // Cleared: both go back to the settings file's. Setup was finished, so it stays done even
    // though the name is the demo's again.
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup",
        Some(KEY),
        json!({"name": "", "contact": "  "}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["name"]["value"], "Kettle FM");
    assert_eq!(v["name"]["source"], "settings");
    assert_eq!(v["contact"]["value"], "");
    assert_eq!(v["status"], "done");
    let (_, v) = send(&p.app, "GET", "/api/v1/player", None, Value::Null).await;
    assert_eq!(v["station"], "Kettle FM");
    assert!(waits_for_contact(&v), "{v}");
    assert_eq!(
        v["call_in"]["consent_version"],
        before["call_in"]["consent_version"]
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_station_named_by_its_settings_file_is_not_offered_setup() {
    let pool = database().await;
    // As installed from the website wizard's file, or upgraded from 1.0.0 with both set.
    let p = plane(
        &pool,
        StationId::new_random(),
        Opts {
            file_name: "Paddy's Wireless",
            file_contact: "https://station.example/takedown",
            ..Opts::default()
        },
    )
    .await;
    let (_, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(v["status"], "done");
    assert_eq!(v["done_by"], "settings");
    assert_eq!(v["missing"], json!([]));
    assert_eq!(v["completed_at_ms"], Value::Null);
    assert_eq!(v["name"]["source"], "settings");
    let (_, v) = send(&p.app, "GET", "/api/v1/player", None, Value::Null).await;
    assert!(!waits_for_contact(&v), "{v}");

    // With a name of its own and no contact it is still offered: call-in is closed.
    let p = plane(
        &pool,
        StationId::new_random(),
        Opts {
            file_name: "Paddy's Wireless",
            ..Opts::default()
        },
    )
    .await;
    let (_, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(v["status"], "pending");
    assert_eq!(v["missing"].as_array().unwrap().len(), 1);

    // A new name in the settings file reaches a station that set none on the page.
    let id = StationId::new_random();
    let _ = plane(&pool, id, Opts::default()).await;
    let p = plane(
        &pool,
        id,
        Opts {
            file_name: "Renamed In The File",
            ..Opts::default()
        },
    )
    .await;
    let (_, v) = send(&p.app, "GET", "/api/v1/station", None, Value::Null).await;
    assert_eq!(v["name"], "Renamed In The File");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_look_is_a_colour_and_pictures_the_worker_wrote() {
    let pool = database().await;
    let id = StationId::new_random();
    let p = plane(&pool, id, Opts::default()).await;

    // The accent: six hex digits, stored lower case; anything else refused; empty clears.
    for bad in ["red", "#fa0", "#b4532a; background:url(x)", "rgb(1,2,3)"] {
        let (code, v) = send(
            &p.app,
            "PUT",
            "/api/v1/branding/accent",
            Some(KEY),
            json!({"accent": bad}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {v}");
        assert_eq!(v["field"], "accent");
    }
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/branding/accent",
        Some(KEY),
        json!({"accent": "B4532A"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["accent"], "#b4532a");

    // The icon: 512 by 256 with a comment in it becomes 256 square, transparent padding, clean.
    let source = png(512, 256, [200, 30, 30, 255]);
    assert!(source.windows(7).any(|w| w == b"made at"));
    let (code, v) = upload(&p.app, "icon", Some(KEY), source.clone()).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["icon"]["width"], 256);
    assert_eq!(v["icon"]["height"], 256);
    let url = v["icon"]["url"].as_str().unwrap().to_owned();
    assert!(url.starts_with("/api/v1/branding/icon?v="), "{url}");
    let version = url.rsplit('=').next().unwrap().to_owned();
    assert_eq!(version.len(), 16);

    // Served open, as PNG, kept for good under its version, checked each time without it.
    let (code, h, served) = raw(&p.app, "GET", &url, None, "", Vec::new()).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(h[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        h[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(h[header::ETAG], format!("\"{version}\""));
    assert_eq!(size_of(&served), (256, 256));
    assert_ne!(served, source, "the upload itself is never served");
    assert!(
        !served.windows(7).any(|w| w == b"made at"),
        "metadata survived"
    );
    let (_, h, _) = raw(&p.app, "GET", "/api/v1/branding/icon", None, "", Vec::new()).await;
    assert_eq!(h[header::CACHE_CONTROL], "no-cache");
    let (_, h, _) = raw(
        &p.app,
        "GET",
        "/api/v1/branding/icon?v=0000000000000000",
        None,
        "",
        Vec::new(),
    )
    .await;
    assert_eq!(h[header::CACHE_CONTROL], "no-cache");
    let req = Request::builder()
        .uri(&url)
        .header(header::IF_NONE_MATCH, format!("\"{version}\""))
        .body(Body::empty())
        .unwrap();
    let res = p.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);

    // The banner: cropped to three to one, fitted inside 1,200 by 400.
    let (code, v) = upload(
        &p.app,
        "banner",
        Some(KEY),
        png(1800, 1200, [10, 90, 160, 255]),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["banner"]["width"], 1200);
    assert_eq!(v["banner"]["height"], 400);
    assert_eq!(v["icon"]["url"], url, "the icon is untouched");

    // What is refused, each with a sentence, and nothing stored changes.
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#.to_vec();
    let mut cut = png(256, 256, [1, 2, 3, 255]);
    cut.truncate(cut.len() / 2);
    for (what, body, says) in [
        (
            "icon",
            svg.clone(),
            "not a picture this worker reads (PNG or JPEG)",
        ),
        (
            "banner",
            svg,
            "not a picture this worker reads (PNG or JPEG)",
        ),
        (
            "icon",
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00".to_vec(),
            "not a picture this worker reads (PNG or JPEG)",
        ),
        ("icon", cut, "the file is damaged"),
        (
            "icon",
            png(32, 32, [0, 0, 0, 255]),
            "the icon is 32 by 32 pixels; each side is at least 64",
        ),
        (
            "banner",
            png(400, 300, [0, 0, 0, 255]),
            "the banner is 400 by 300 pixels; it is at least 600 wide and 200 tall",
        ),
        ("icon", Vec::new(), "Send the picture as the request's body"),
    ] {
        let (code, v) = upload(&p.app, what, Some(KEY), body).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{what}: {v}");
        assert!(v["error"].as_str().unwrap().contains(says), "{what}: {v}");
    }
    // Over the cap: refused by size, before the worker is started.
    let (code, _) = upload(&p.app, "icon", Some(KEY), vec![0x89; 1024 * 1024 + 1]).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);
    let (code, _) = upload(&p.app, "banner", Some(KEY), vec![0x89; 4 * 1024 * 1024 + 1]).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);
    let (_, v) = send(&p.app, "GET", "/api/v1/branding", None, Value::Null).await;
    assert_eq!(v["accent"], "#b4532a");
    assert_eq!(v["icon"]["url"], url);
    assert_eq!(v["banner"]["width"], 1200);

    // It outlives a restart, and the setup view carries it.
    let p = plane(&pool, id, Opts::default()).await;
    let (_, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(v["branding"]["icon"]["url"], url);
    assert_eq!(v["branding"]["accent"], "#b4532a");

    // Removed: the pages go back to the product's own, and the picture's address is gone.
    let (code, v) = send(
        &p.app,
        "DELETE",
        "/api/v1/branding/icon",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["icon"], Value::Null);
    assert_eq!(v["banner"]["width"], 1200);
    let (code, _, _) = raw(&p.app, "GET", &url, None, "", Vec::new()).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = send(
        &p.app,
        "DELETE",
        "/api/v1/branding/banner",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/branding/accent",
        Some(KEY),
        json!({"accent": ""}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v, json!({"accent": null, "icon": null, "banner": null}));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_stream_password_is_written_for_the_start_script_only_where_the_install_has_one() {
    let pool = database().await;
    // Compose and Quadlets: no stream password to set.
    let p = plane(&pool, StationId::new_random(), Opts::default()).await;
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup/stream-password",
        Some(KEY),
        json!({"enabled": true, "password": "kettle-fm-2026"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert!(!p.overrides.exists());

    // The Cloudron package.
    let p = plane(
        &pool,
        StationId::new_random(),
        Opts {
            stream_password: true,
            ..Opts::default()
        },
    )
    .await;
    let (_, v) = send(&p.app, "GET", "/api/v1/setup", Some(KEY), Value::Null).await;
    assert_eq!(
        v["stream_password"],
        json!({"available": true, "enabled": false, "asked": null, "restart_needed": false})
    );
    for bad in [
        "short",
        "has space 1",
        "$(reboot)123",
        "semi;colon12",
        "quote\"12345",
        "line\nbreak12",
    ] {
        let (code, v) = send(
            &p.app,
            "PUT",
            "/api/v1/setup/stream-password",
            Some(KEY),
            json!({"enabled": true, "password": bad}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {v}");
        assert_eq!(v["field"], "password");
    }
    assert!(!p.overrides.exists());
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup/stream-password",
        Some(KEY),
        json!({"enabled": true, "password": "kettle-fm-2026"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        v["stream_password"],
        json!({"available": true, "enabled": false, "asked": true, "restart_needed": true})
    );
    // The password is in the file and nowhere in any answer.
    assert!(!v.to_string().contains("kettle-fm-2026"));
    let text = std::fs::read_to_string(&p.overrides).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(
        lines,
        ["LISTENERS_AUTH=on", "LISTENERS_PASSWORD=kettle-fm-2026"]
    );
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(&p.overrides)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // Off, with no new password: the one set before is kept for when it is turned on again.
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/setup/stream-password",
        Some(KEY),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["stream_password"]["asked"], false);
    let text = std::fs::read_to_string(&p.overrides).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(
        lines,
        ["LISTENERS_AUTH=off", "LISTENERS_PASSWORD=kettle-fm-2026"]
    );
}
