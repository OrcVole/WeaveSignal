// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Hosts in their own studios (ADR-0280, ADR-0284) through the control plane's routes against a
//! real PostgreSQL: show keys made, listed and ended by those who run the show, their secrets
//! shown once; the ingest's internal routes, refused unless they come from the ingest; admission
//! in the slot (standing by, with a slot), outside it (a private test, or refused with the next
//! door), with a wrong key (and an address refused after five), with an ended or a turned-away
//! key; the report and `GET /sources`; the rules. Ignored unless asked for (`just test-db`).

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
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::staff::Role;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "sources-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";
const INGEST: &str = "ingest-test-only-ingest-test-only-0";

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
        room: None,
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

async fn member(p: &Plane, username: &str, role: Role) -> (uuid::Uuid, String) {
    let account = weavesignal_model::staff::create_account(
        &p.pool,
        p.station.id,
        username,
        username,
        role,
        &weavesignal_model::staff::hash_password("a long enough phrase").unwrap(),
    )
    .await
    .unwrap();
    let token = weavesignal_model::staff::new_token();
    weavesignal_model::staff::create_session(
        &p.pool,
        p.station.id,
        account.id,
        &weavesignal_model::staff::token_hash(&token),
        weavesignal_control::hub::now_ms() + 3_600_000,
    )
    .await
    .unwrap();
    p.state.staff.restore(p.station.id).await;
    (account.id, token)
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

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn show_keys_are_made_by_those_who_run_the_show_and_shown_once() {
    let p = plane().await;
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;

    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let key_id = made["key"]["key_id"].as_str().unwrap().to_owned();
    let secret = made["secret"].as_str().unwrap().to_owned();
    let monitor = made["monitor_secret"].as_str().unwrap().to_owned();
    assert_eq!((key_id.len(), secret.len(), monitor.len()), (8, 24, 24));
    assert_eq!(
        made["details"]["icecast"]["password"],
        format!("{key_id}-{secret}")
    );
    assert_eq!(made["details"]["icecast"]["port"], 8005);
    assert_eq!(made["details"]["srt"]["streamid"], key_id.as_str());
    assert_eq!(made["details"]["srt"]["passphrase"], secret.as_str());
    assert_eq!(made["details"]["host"], "kettle.example");
    assert!(
        made["details"]["srt"]["url"]
            .as_str()
            .unwrap()
            .starts_with("srt://kettle.example:9710?streamid=")
    );
    assert!(made["as_text"].as_str().unwrap().contains(&secret));
    assert!(made["key"].get("key_salt").is_none() && made["key"].get("monitor_hash").is_none());

    // Never again: the list holds neither secret, and the show now comes from its host.
    let (code, list) = send(
        &p.app,
        "GET",
        "/api/v1/shows/steeped/keys",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{list}");
    assert_eq!(list["source"], "push");
    assert_eq!(list["keys"].as_array().unwrap().len(), 1);
    let text = list.to_string();
    assert!(
        !text.contains(&secret) && !text.contains(&monitor),
        "{list}"
    );

    // Who may: a producer and the show's presenter; not a screener, a presenter of nothing, or
    // any API token.
    let (_, producer) = member(&p, "shona", Role::Producer).await;
    let (_, screener) = member(&p, "moira", Role::Screener).await;
    let (finlay, presenter) = member(&p, "finlay", Role::Presenter).await;
    let body = json!({"label": "Stand-in", "hint": "icecast"});
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/shows/steeped/keys",
        Some(&producer),
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/shows/steeped/keys",
        Some(&screener),
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/shows/steeped/keys",
        Some(&presenter),
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "not his show yet");
    sqlx::query(
        "UPDATE show SET presenter_account = $1 WHERE slug = 'steeped' AND station_id = $2",
    )
    .bind(finlay)
    .bind(p.station.id.as_uuid())
    .execute(&p.pool)
    .await
    .unwrap();
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/shows/steeped/keys",
        Some(&presenter),
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "his show now");
    let (code, tok) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(ADMIN),
        json!({"name": "Board", "scopes": ["sources:read", "shows:write"]}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{tok}");
    let token = tok["secret"].as_str().unwrap().to_owned();
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/shows/steeped/keys",
        Some(&token),
        body.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "a token never makes a key");
    let (code, _) = send(
        &p.app,
        "GET",
        "/api/v1/shows/steeped/keys",
        Some(&token),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, v) = send(&p.app, "GET", "/api/v1/sources", Some(&token), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "sources:read reads the sources: {v}");

    // Bad words.
    for (b, field) in [
        (json!({"label": "", "hint": "srt"}), "label"),
        (json!({"label": "x", "hint": "rtmp"}), "hint"),
    ] {
        let (code, v) = send(&p.app, "POST", "/api/v1/shows/steeped/keys", Some(ADMIN), b).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(v["field"], field);
    }
    let (code, _) = send(&p.app, "POST", "/api/v1/shows/nope/keys", Some(ADMIN), body).await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Ending the show's keys sends it back to the room.
    let (_, list) = send(
        &p.app,
        "GET",
        "/api/v1/shows/steeped/keys",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    for k in list["keys"].as_array().unwrap() {
        let id = k["key_id"].as_str().unwrap();
        let (code, v) = send(
            &p.app,
            "DELETE",
            &format!("/api/v1/shows/steeped/keys/{id}"),
            Some(ADMIN),
            Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["ended"], true);
    }
    let (_, list) = send(
        &p.app,
        "GET",
        "/api/v1/shows/steeped/keys",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(list["source"], "room");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_ingests_routes_answer_only_the_ingest() {
    let p = plane().await;
    let uri = "/api/v1/internal/sources/config";
    // No bearer set at all: off, whoever asks.
    let (code, _) = ingest(&p.app, "GET", uri, Value::Null).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    let (code, cfg) = ingest(&p.app, "GET", uri, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{cfg}");
    assert_eq!(
        cfg["listen"], false,
        "no key yet: the ingest does not listen"
    );
    assert_eq!(
        (cfg["port_icecast"].as_u64(), cfg["port_srt"].as_u64()),
        (Some(8005), Some(9710))
    );
    // Through a proxy, from elsewhere, with another bearer, or the station key: refused.
    let refusals = [
        (
            Some(INGEST),
            Some("127.0.0.1:1"),
            vec![("x-forwarded-for", "198.51.100.4")],
        ),
        (
            Some(INGEST),
            Some("127.0.0.1:1"),
            vec![("forwarded", "for=198.51.100.4")],
        ),
        (Some(INGEST), Some("192.0.2.9:1"), vec![]),
        (Some(INGEST), None, vec![]),
        (Some(ADMIN), Some("127.0.0.1:1"), vec![]),
        (None, Some("127.0.0.1:1"), vec![]),
    ];
    for (bearer, peer, headers) in refusals {
        let (code, _) = call(&p.app, "GET", uri, bearer, Value::Null, peer, &headers).await;
        assert!(
            code == StatusCode::FORBIDDEN || code == StatusCode::UNAUTHORIZED,
            "{bearer:?} {peer:?} {headers:?}: {code}"
        );
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_key_stands_by_in_its_slot_tests_outside_it_and_is_refused_when_wrong() {
    let p = plane().await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle-drive", "Kettle Drive").await;
    episode(&p.app, "steeped", -600, 120).await;
    episode(&p.app, "kettle-drive", 3 * 86_400, 120).await;
    let steeped = make_key(&p, "steeped", "Finlay Grant", "icecast").await;
    let drive = make_key(&p, "kettle-drive", "Heather Douglas", "srt").await;
    let (sk, ss) = (
        steeped["key"]["key_id"].as_str().unwrap().to_owned(),
        steeped["secret"].as_str().unwrap().to_owned(),
    );
    let (dk, ds) = (
        drive["key"]["key_id"].as_str().unwrap().to_owned(),
        drive["secret"].as_str().unwrap().to_owned(),
    );
    let (_, cfg) = ingest(
        &p.app,
        "GET",
        "/api/v1/internal/sources/config",
        Value::Null,
    )
    .await;
    assert_eq!(cfg["listen"], true, "a key exists: listen");

    // In its slot, Icecast with the right secret: stand by in slot 5, the DJ's.
    let a = admit(&p, &sk, Some(&ss), "icecast", "198.51.100.4").await;
    assert_eq!(a["decision"], "standby", "{a}");
    assert_eq!(a["slot"], 5);
    assert_eq!(a["identity"], format!("source-{sk}"));
    assert_eq!(a["name"], "Finlay Grant");
    assert_eq!(a["show"], "steeped");
    assert!(
        a["passphrase"].is_null(),
        "Icecast is never sent the secret"
    );
    assert!(a["door_closes_ms"].is_i64());

    // Outside its slot: a private test, fifteen minutes at most; SRT gets the passphrase.
    let t = admit(&p, &dk, None, "srt", "198.51.100.5").await;
    assert_eq!(t["decision"], "test", "{t}");
    assert_eq!(t["max_ms"], 15 * 60_000);
    assert_eq!(t["passphrase"], ds.as_str());
    assert!(t["slot"].is_null());

    // With tests off, refused, and told when the slot's door opens.
    let mut rules: Value = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await
    .1;
    rules["test_outside"] = json!(false);
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        rules.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let r = admit(&p, &dk, None, "srt", "198.51.100.5").await;
    assert_eq!(r["decision"], "refused");
    assert_eq!(r["reason"], "not_your_slot");
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("Kettle Drive is next at"),
        "{r}"
    );

    // A wrong secret, an unknown key, rubbish: wrong_key; five from one address in a minute
    // refuse that address for ten minutes, and nobody else.
    let w = admit(
        &p,
        &sk,
        Some("not-the-secret-at-all-xx"),
        "icecast",
        "203.0.113.66",
    )
    .await;
    assert_eq!(w["reason"], "wrong_key");
    admit(&p, "zzzzzzzz", Some("x"), "icecast", "203.0.113.66").await;
    admit(&p, "../etc", Some("x"), "icecast", "203.0.113.66").await;
    admit(&p, &sk, None, "icecast", "203.0.113.66").await;
    // The fifth comes back from the ingest: an SRT handshake that failed.
    let (code, _) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/events",
        json!({"key_id": dk, "event": "refused", "reason": "wrong_key", "way": "srt", "address": "203.0.113.66"}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let blocked = admit(&p, &sk, Some(&ss), "icecast", "203.0.113.66").await;
    assert_eq!(blocked["reason"], "too_many", "{blocked}");
    assert!(blocked["retry_after_ms"].as_i64().unwrap() > 500_000);
    let other = admit(&p, &sk, Some(&ss), "icecast", "203.0.113.67").await;
    assert_eq!(other["decision"], "standby");

    // The ingest reports what it holds; GET /sources shows it with the key's facts.
    let now = weavesignal_control::hub::now_ms();
    let (code, reply) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/report",
        json!({"connections": [
            {"key_id": sk, "mode": "standby", "way": "icecast", "format": "mp3 44.1 kHz stereo",
             "since_ms": now - 8_000, "kbps": 128, "level_db": -18.5, "title": "Song\u{7} by Band"}
        ]}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{reply}");
    assert_eq!(reply["close"], json!([]));
    let (code, v) = send(&p.app, "GET", "/api/v1/sources", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["listening"], true);
    let src = v["sources"].as_array().unwrap();
    assert_eq!(src.len(), 1, "only Steeped is connected or due: {v}");
    assert_eq!(src[0]["state"], "standby");
    assert_eq!(src[0]["way"], "icecast");
    assert_eq!(src[0]["format"], "mp3 44.1 kHz stereo");
    assert_eq!(src[0]["title"], "Song by Band", "metadata is cleaned");
    assert!(src[0]["steady_ms"].as_u64().unwrap() >= 8_000);
    assert_eq!(src[0]["monitor_open"], false);

    // Disconnect turns the key away for the rest of the slot, and the report asks the ingest to
    // close it; let back in, it stands by again.
    let (code, d) = send(
        &p.app,
        "POST",
        &format!("/api/v1/sources/{sk}/disconnect"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{d}");
    let (_, reply) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/report",
        json!({"connections": [{"key_id": sk, "mode": "standby", "way": "icecast", "since_ms": now}]}),
    )
    .await;
    assert_eq!(reply["close"], json!([sk]));
    let away = admit(&p, &sk, Some(&ss), "icecast", "198.51.100.4").await;
    assert_eq!(away["reason"], "turned_away");
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/sources/{sk}/disconnect"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        admit(&p, &sk, Some(&ss), "icecast", "198.51.100.4").await["decision"],
        "standby"
    );

    // A new key in place of the old: the old ends now, and is refused as ended.
    let (code, renewed) = send(
        &p.app,
        "POST",
        &format!("/api/v1/shows/steeped/keys/{sk}/renew"),
        Some(ADMIN),
        json!({"old": "now"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{renewed}");
    assert_eq!(renewed["key"]["label"], "Finlay Grant");
    let ended = admit(&p, &sk, Some(&ss), "icecast", "198.51.100.4").await;
    assert_eq!(ended["reason"], "ended");
    let nk = renewed["key"]["key_id"].as_str().unwrap();
    let ns = renewed["secret"].as_str().unwrap();
    assert_eq!(
        admit(&p, nk, Some(ns), "icecast", "198.51.100.4").await["decision"],
        "standby"
    );

    // The key's row in Settings carries its last refusal, without the address.
    let (_, list) = send(
        &p.app,
        "GET",
        "/api/v1/shows/steeped/keys",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    let old = list["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["key_id"] == sk.as_str())
        .unwrap();
    assert_eq!(old["last_refusal"]["reason"], "ended");
    assert!(old["last_refusal"].get("address").is_none());
    assert!(old["revoked_ms"].is_i64());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_rules_are_an_admins_and_checked() {
    let p = plane().await;
    let (code, rules) = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(rules["grace_s"], 120);
    assert_eq!(rules["door_before_min"], 30);
    assert_eq!(rules["late_count_s"], 20);
    let (_, producer) = member(&p, "shona", Role::Producer).await;
    let (code, _) = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(&producer),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    for (field, value) in [
        ("grace_s", json!(700)),
        ("door_before_min", json!(500)),
        ("port_srt", json!(8005)),
    ] {
        let mut bad = rules.clone();
        bad[field] = value;
        let (code, v) = send(&p.app, "PUT", "/api/v1/settings/sources", Some(ADMIN), bad).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
        assert_eq!(v["field"], field);
    }
    let mut good = rules.clone();
    good["grace_s"] = json!(0);
    good["port_icecast"] = json!(18005);
    let (code, _) = send(&p.app, "PUT", "/api/v1/settings/sources", Some(ADMIN), good).await;
    assert_eq!(code, StatusCode::OK);
    let (_, now) = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(
        (now["grace_s"].as_i64(), now["port_icecast"].as_i64()),
        (Some(0), Some(18005))
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_installations_ports_win_over_the_rules() {
    // Cloudron's dashboard or the Compose settings publish the two ports, so the settings file's
    // `ports.source_icecast` and `ports.source_srt` are the ones in force; 0 is a way turned off.
    let p = plane().await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    p.state
        .hub
        .sources
        .set_fixed_ports(weavesignal_control::sources::FixedPorts {
            icecast: Some(28005),
            srt: Some(0),
            whip: None,
        });
    let (code, rules) = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (rules["port_icecast"].as_i64(), rules["port_srt"].as_i64()),
        (Some(28005), Some(0))
    );
    let (_, cfg) = ingest(
        &p.app,
        "GET",
        "/api/v1/internal/sources/config",
        Value::Null,
    )
    .await;
    assert_eq!(
        (cfg["port_icecast"].as_u64(), cfg["port_srt"].as_u64()),
        (Some(28005), Some(0))
    );
    // Sent back as shown, the other rules change; another port is refused, naming the setting.
    let mut same = rules.clone();
    same["grace_s"] = json!(60);
    let (code, v) = send(&p.app, "PUT", "/api/v1/settings/sources", Some(ADMIN), same).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        (v["grace_s"].as_i64(), v["port_srt"].as_i64()),
        (Some(60), Some(0))
    );
    let mut moved = rules.clone();
    moved["port_icecast"] = json!(8005);
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        moved,
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["field"], "port_icecast");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("ports.source_icecast"),
        "{v}"
    );
    // A key's details carry the installation's port, and no SRT address while SRT is off.
    make_show(&p.app, "steeped", "Steeped").await;
    let key = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    assert_eq!(key["details"]["icecast"]["port"], 28005, "{key}");
    assert!(key["details"]["srt"]["url"].is_null(), "{key}");
    let text = key["as_text"].as_str().unwrap();
    assert!(
        text.contains("Port: 28005") && !text.contains("SRT ("),
        "{text}"
    );
}

/// A request with a text body, as a WHIP client sends it; the answer's status and text.
async fn whip_call(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: &str,
    body: &str,
) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/sdp")
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::from(body.to_owned()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "192.0.2.60:5000".parse::<SocketAddr>().unwrap(),
    ));
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn whip_is_a_third_way_in_with_the_keys_secret_as_its_bearer() {
    // 17d: a key may be made for WHIP; the ingest admits a WHIP connection with the bearer's
    // secret as it does Icecast's password; the log keeps `whip`; WHIP's port is a rule and an
    // installation's setting, never SRT's number; the public route says when WHIP is off.
    let p = plane().await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;
    let made = make_key(&p, "steeped", "Morag Campbell", "whip").await;
    assert_eq!(made["key"]["hint"], "whip");
    let key_id = made["key"]["key_id"].as_str().unwrap().to_owned();
    let secret = made["secret"].as_str().unwrap().to_owned();
    assert_eq!(made["details"]["whip"]["port"], 9712, "{made}");
    assert_eq!(made["details"]["whip"]["bearer_token"], secret.as_str());
    assert!(
        made["details"]["whip"]["url"].is_null(),
        "no public address in this plane: no WHIP address is made up"
    );
    assert!(made["as_text"].as_str().unwrap().contains("WHIP"), "{made}");

    let (_, cfg) = ingest(
        &p.app,
        "GET",
        "/api/v1/internal/sources/config",
        Value::Null,
    )
    .await;
    assert_eq!(cfg["port_whip"], 9712, "{cfg}");

    let a = admit(&p, &key_id, Some(&secret), "whip", "198.51.100.14").await;
    assert_eq!(a["decision"], "standby", "{a}");
    assert_eq!(a["identity"], format!("source-{key_id}"));
    assert!(a["passphrase"].is_null(), "WHIP is never sent the secret");
    let wrong = admit(
        &p,
        &key_id,
        Some("WRONGWRONGWRONGWRONGWRON"),
        "whip",
        "198.51.100.15",
    )
    .await;
    assert_eq!(
        (wrong["decision"].as_str(), wrong["reason"].as_str()),
        (Some("refused"), Some("wrong_key")),
        "{wrong}"
    );
    let bare = admit(&p, &key_id, None, "whip", "198.51.100.15").await;
    assert_eq!(
        bare["reason"], "wrong_key",
        "WHIP without a secret is a wrong key: {bare}"
    );
    let way: Option<String> = sqlx::query_scalar(
        "SELECT way FROM source_log WHERE key_id = $1 AND event = 'refused' ORDER BY id DESC LIMIT 1",
    )
    .bind(&key_id)
    .fetch_one(&p.pool)
    .await
    .unwrap();
    assert_eq!(way.as_deref(), Some("whip"));
    let (code, v) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/events",
        json!({"key_id": key_id, "event": "ended", "detail": "the host's software disconnected", "way": "whip", "address": "198.51.100.14"}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{v}");

    // WHIP's port: kept, and refused when it is SRT's.
    let mut rules: Value = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await
    .1;
    assert_eq!(rules["port_whip"], 9712, "{rules}");
    rules["port_whip"] = json!(19712);
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        rules.clone(),
    )
    .await;
    assert_eq!(
        (code, v["port_whip"].as_i64()),
        (StatusCode::OK, Some(19712)),
        "{v}"
    );
    rules["port_whip"] = rules["port_srt"].clone();
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        rules.clone(),
    )
    .await;
    assert_eq!(
        (code, v["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("port_whip")),
        "{v}"
    );
    // A studio that does not know WHIP yet sends no `port_whip`: the default is taken.
    let mut older = rules.clone();
    older.as_object_mut().unwrap().remove("port_whip");
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        older,
    )
    .await;
    assert_eq!(
        (code, v["port_whip"].as_i64()),
        (StatusCode::OK, Some(9712)),
        "{v}"
    );

    // The public route: no ingest listening, it says so; the installation's 0, WHIP is off.
    p.state.hub.sources.set_ingest_whip(1);
    let (code, text) = whip_call(
        &p.app,
        "POST",
        &format!("/whip/{key_id}"),
        &secret,
        "v=0\r\n",
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{text}");
    assert!(text.contains("cannot take WHIP just now"), "{text}");
    p.state
        .hub
        .sources
        .set_fixed_ports(weavesignal_control::sources::FixedPorts {
            whip: Some(0),
            ..Default::default()
        });
    let (code, text) = whip_call(
        &p.app,
        "POST",
        &format!("/whip/{key_id}"),
        &secret,
        "v=0\r\n",
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{text}");
    assert!(text.contains("has not turned WHIP on"), "{text}");
    let (_, cfg) = ingest(
        &p.app,
        "GET",
        "/api/v1/internal/sources/config",
        Value::Null,
    )
    .await;
    assert_eq!(cfg["port_whip"], 0, "{cfg}");
    let key = make_key(&p, "steeped", "Morag Campbell", "whip").await;
    assert!(!key["as_text"].as_str().unwrap().contains("WHIP"), "{key}");
}

/// A desk that holds one source, as the real desk reports it (`tests/source_in.rs` drives the
/// real one), and takes `on_air` for it.
fn fake_desk(path: &std::path::Path, identity: String) -> Arc<Mutex<bool>> {
    let on_air = Arc::new(Mutex::new(false));
    let listener = UnixListener::bind(path).unwrap();
    let flag = on_air.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let (flag, identity) = (flag.clone(), identity.clone());
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
                                "source": {"way": "srt", "format": "mpegts/aac 48 kHz stereo", "key": "k",
                                           "show": "steeped", "connected_ms": 12000, "leveller_db": -2.5, "level_db": -20.9}
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

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_studio_and_the_sources_say_what_the_desk_holds() {
    // A Unix socket's path must be short: the system's temporary directory, as rtmp_db does.
    let dir = std::env::temp_dir().join(format!(
        "ws-src-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("desk.sock");
    let p = plane_with_desk(Some(&sock)).await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let k = made["key"]["key_id"].as_str().unwrap().to_owned();
    let on_air = fake_desk(&sock, format!("source-{k}"));
    // Admitted: slot 5 is promised; the desk then reports the source, which takes it.
    let a = admit(&p, &k, None, "srt", "198.51.100.4").await;
    assert_eq!(
        (a["decision"].as_str(), a["slot"].as_u64()),
        (Some("standby"), Some(5))
    );
    p.state.hub.poll_once().await;
    let (code, studio) = send(&p.app, "GET", "/api/v1/studio", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{studio}");
    let src = studio["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["identity"] == format!("source-{k}"))
        .unwrap_or_else(|| panic!("the source is among the studio's sources: {studio}"))
        .clone();
    assert_eq!(src["slot"], 5);
    assert_eq!(src["role"], "source");
    assert_eq!(src["on_air"], false, "standing by");
    assert_eq!(src["source"]["way"], "srt");
    assert_eq!(src["source"]["format"], "mpegts/aac 48 kHz stereo");
    assert_eq!(src["source"]["leveller_db"], -2.5);
    assert_eq!(src["source"]["connected_ms"], 12000);
    assert_eq!(src["lufs"], -23.4);
    let (_, v) = send(&p.app, "GET", "/api/v1/sources", Some(ADMIN), Value::Null).await;
    let s = &v["sources"][0];
    assert_eq!(
        (s["state"].as_str(), s["slot"].as_u64()),
        (Some("standby"), Some(5)),
        "{v}"
    );
    assert_eq!(s["leveller_db"], -2.5);
    assert_eq!(s["level"], -23.4);

    // Put on air as anyone is, at the show's default step: the desk's own command.
    let (code, r) = send(
        &p.app,
        "POST",
        "/api/v1/desk/commands",
        Some(ADMIN),
        json!({"op": "on_air", "identity": format!("source-{k}"), "on": true}),
    )
    .await;
    assert!(code.is_success(), "{code}: {r}");
    assert!(*on_air.lock().unwrap());
    p.state.hub.poll_once().await;
    let (_, v) = send(&p.app, "GET", "/api/v1/sources", Some(ADMIN), Value::Null).await;
    assert_eq!(v["sources"][0]["state"], "on_air", "{v}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_source_standing_by_when_its_door_closes_becomes_a_test_unless_it_is_on_air() {
    let dir = std::env::temp_dir().join(format!(
        "ws-src-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("desk.sock");
    let p = plane_with_desk(Some(&sock)).await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    make_show(&p.app, "steeped", "Steeped").await;
    // Ended three hours ago: the door (two hours after the end at most) is shut.
    episode(&p.app, "steeped", -5 * 3600, 120).await;
    let made = make_key(&p, "steeped", "Finlay Grant", "srt").await;
    let k = made["key"]["key_id"].as_str().unwrap().to_owned();
    let on_air = fake_desk(&sock, format!("source-{k}"));
    let now = weavesignal_control::hub::now_ms();
    let report = json!({"connections": [
        {"key_id": k, "mode": "standby", "way": "srt", "since_ms": now - 5_000}
    ]});
    let ask = || async {
        ingest(
            &p.app,
            "POST",
            "/api/v1/internal/sources/report",
            report.clone(),
        )
        .await
        .1
    };

    // Nothing is known of who is on air before the desk has answered: nothing is touched.
    assert_eq!(ask().await["to_test"], json!([]));
    // Off the air, the door shut: a test, for the rules' fifteen minutes from now.
    p.state.hub.poll_once().await;
    let r = ask().await;
    assert_eq!(
        r["to_test"],
        json!([{"key_id": k, "max_ms": 15 * 60_000}]),
        "{r}"
    );
    assert_eq!(r["close"], json!([]));
    // A test is never asked about again.
    let (_, r) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/report",
        json!({"connections": [{"key_id": k, "mode": "test", "way": "srt", "since_ms": now}]}),
    )
    .await;
    assert_eq!(r["to_test"], json!([]));
    // On the air it is never touched; off again, the next look takes it.
    *on_air.lock().unwrap() = true;
    p.state.hub.poll_once().await;
    assert_eq!(ask().await["to_test"], json!([]));
    *on_air.lock().unwrap() = false;
    p.state.hub.poll_once().await;
    assert_eq!(ask().await["to_test"].as_array().unwrap().len(), 1);

    // With tests outside the slot off there is no test to become: the connection is closed.
    let mut rules: Value = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await
    .1;
    rules["test_outside"] = json!(false);
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/settings/sources",
        Some(ADMIN),
        rules,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let r = ask().await;
    assert_eq!(r["close"], json!([k]), "{r}");
    assert_eq!(r["to_test"], json!([]));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn wrong_keys_from_a_private_address_are_logged_but_never_block_it() {
    let p = plane().await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;
    let made = make_key(&p, "steeped", "Finlay Grant", "icecast").await;
    let (k, s) = (
        made["key"]["key_id"].as_str().unwrap().to_owned(),
        made["secret"].as_str().unwrap().to_owned(),
    );
    for addr in ["172.17.0.1", "fd00::5"] {
        for _ in 0..8 {
            let w = admit(&p, &k, Some("not-the-secret-at-all-xx"), "icecast", addr).await;
            assert_eq!(w["reason"], "wrong_key", "{w}");
        }
        let ok = admit(&p, &k, Some(&s), "icecast", addr).await;
        assert_eq!(ok["decision"], "standby", "never blocked: {ok}");
    }
    // A public address is blocked on the fifth, as before.
    for _ in 0..5 {
        admit(
            &p,
            &k,
            Some("not-the-secret-at-all-xx"),
            "icecast",
            "198.51.100.77",
        )
        .await;
    }
    let b = admit(&p, &k, Some(&s), "icecast", "198.51.100.77").await;
    assert_eq!(b["reason"], "too_many", "{b}");
}

// ---- the post's rule (ADR-0280; design sections 3.3 to 3.6 and 3.8) ----

/// One person (or source) at the fake desk.
#[derive(Clone)]
struct At {
    identity: String,
    name: String,
    role: &'static str,
    on_air: bool,
    /// For a source: how long the desk has had it unbroken.
    connected_ms: Option<u64>,
}

impl At {
    fn source(key_id: &str, name: &str, connected_ms: u64) -> At {
        At {
            identity: format!("source-{key_id}"),
            name: name.into(),
            role: "source",
            on_air: false,
            connected_ms: Some(connected_ms),
        }
    }

    fn dj(identity: &str, name: &str, on_air: bool) -> At {
        At {
            identity: identity.into(),
            name: name.into(),
            role: "dj",
            on_air,
            connected_ms: None,
        }
    }
}

#[derive(Default)]
struct DeskState {
    people: Vec<At>,
    /// `on:<identity>` and `off:<identity>`, in the order the desk was told.
    commands: Vec<String>,
    /// The `fade_ms` each of them came with (`None`: at once), in the same order.
    fades: Vec<Option<u64>>,
}

/// A desk that holds whoever the test says, takes `on_air` for them, and remembers what it was told.
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
                    let answer = match req["op"].as_str() {
                        Some("status") => {
                            let st = shared.lock().unwrap();
                            let people: Vec<Value> = st
                                .people
                                .iter()
                                .map(|p| {
                                    let mut v = json!({
                                        "identity": p.identity, "name": p.name, "role": p.role,
                                        "on_air": p.on_air, "mute": false, "cue": false, "talk": false,
                                        "talk_to": null, "present": true, "away_ms": null, "linked": true,
                                        "lufs": -23.0, "lufs_peak": -20.0, "underruns": 0, "late_ms": 0,
                                        "ring_ms": 300, "camera": false
                                    });
                                    if let Some(ms) = p.connected_ms {
                                        v["source"] = json!({"way": "srt", "format": "mpegts/aac 48 kHz stereo",
                                            "key": p.identity.trim_start_matches("source-"), "show": "x",
                                            "connected_ms": ms, "leveller_db": 0.0, "level_db": -20.0});
                                    }
                                    v
                                })
                                .collect();
                            json!({"ok": true, "playout": {"connected": true}, "people": people})
                        }
                        Some("on_air") => {
                            let id = req["identity"].as_str().unwrap().to_owned();
                            let on = req["on"].as_bool().unwrap_or(false);
                            let mut st = shared.lock().unwrap();
                            st.commands
                                .push(format!("{}:{id}", if on { "on" } else { "off" }));
                            st.fades.push(req["fade_ms"].as_u64());
                            if let Some(p) = st.people.iter_mut().find(|p| p.identity == id) {
                                p.on_air = on;
                            }
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
    state
}

fn fades(desk: &Arc<Mutex<DeskState>>) -> Vec<Option<u64>> {
    desk.lock().unwrap().fades.clone()
}

/// A plane with a desk at a socket, the playout layer's control (no playout behind it) and the rules
/// set short, so a test runs in seconds.
async fn post_plane(
    rules: impl FnOnce(&mut Value),
) -> (
    Plane,
    Arc<Mutex<DeskState>>,
    Arc<weavesignal_control::onair::OnAir>,
) {
    let dir = std::env::temp_dir().join(format!(
        "ws-post-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("desk.sock");
    let desk = live_desk(&sock);
    let p = plane_with_desk(Some(&sock)).await;
    let onair = weavesignal_control::onair::OnAir::new(
        p.station.id,
        p.pool.clone(),
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
        &p.state.hub,
    );
    p.state.hub.onair.set(onair.clone()).ok().unwrap();
    let (_, mut r) = send(
        &p.app,
        "GET",
        "/api/v1/settings/sources",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    rules(&mut r);
    let (code, v) = send(&p.app, "PUT", "/api/v1/settings/sources", Some(ADMIN), r).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    (p, desk, onair)
}

/// One look of the post's rule, with the desk read first.
async fn look(p: &Plane) -> weavesignal_model::post::Plan {
    p.state.hub.poll_once().await;
    weavesignal_control::post::tick(&p.state).await.unwrap()
}

fn key_id_of(made: &Value) -> String {
    made["key"]["key_id"].as_str().unwrap().to_owned()
}

fn commands(desk: &Arc<Mutex<DeskState>>) -> Vec<String> {
    desk.lock().unwrap().commands.clone()
}

async fn log_of(p: &Plane, key: &str) -> Vec<(String, Option<String>)> {
    weavesignal_model::sources::key_log(&p.pool, p.station.id, key, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|l| (l.event, l.reason))
        .collect()
}

fn mode_of(p: &Plane) -> Option<weavesignal_control::desk::StationMode> {
    p.state.hub.read_studio(|s| s.station_mode())
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn at_the_post_automation_hands_the_air_to_the_next_source_once() {
    use weavesignal_control::desk::StationMode;
    let (p, desk, onair) = post_plane(|_| {}).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    // Steeped ends in three seconds, Kettle Drive's post.
    episode(&p.app, "steeped", -3597, 60).await;
    episode(&p.app, "kettle", 3, 30).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    desk.lock()
        .unwrap()
        .people
        .push(At::source(&k, "Finlay Grant", 600_000));
    onair.set_mode(StationMode::Auto).await.unwrap();

    // Before the post: standing by, with its time to go on air; nothing done.
    let plan = look(&p).await;
    assert!(commands(&desk).is_empty());
    let post = plan.post.expect("a post to show");
    assert_eq!(post.show.as_ref().unwrap().slug, "kettle");
    assert_eq!(post.source.as_deref(), Some(k.as_str()));
    assert_eq!(
        post.state,
        Some(weavesignal_model::post::SourceState::StandingBy)
    );
    assert_eq!(
        post.on_air_at_ms,
        Some(post.show.as_ref().unwrap().start_ms)
    );
    let (_, studio) = send(&p.app, "GET", "/api/v1/studio", Some(ADMIN), Value::Null).await;
    assert_eq!(studio["post"]["show"]["slug"], "kettle", "{studio}");
    let (_, listed) = send(&p.app, "GET", "/api/v1/sources", Some(ADMIN), Value::Null).await;
    assert_eq!(listed["post"]["state"], "standing_by", "{listed}");

    // At the post: on air, the station live, written down as the schedule's.
    tokio::time::sleep(Duration::from_millis(3300)).await;
    look(&p).await;
    assert_eq!(commands(&desk), [format!("on:source-{k}")]);
    assert_eq!(
        fades(&desk),
        [Some(2000)],
        "the schedule's move fades over fade_ms"
    );
    assert_eq!(mode_of(&p), Some(StationMode::Live));
    assert!(
        log_of(&p, &k)
            .await
            .contains(&("on_air".into(), Some("schedule".into())))
    );
    // Not again: a second look finds it on air and the log says so.
    look(&p).await;
    look(&p).await;
    assert_eq!(commands(&desk).len(), 1);
    // A producer takes it off air: the schedule does not fight them.
    desk.lock().unwrap().people[0].on_air = false;
    look(&p).await;
    look(&p).await;
    assert_eq!(commands(&desk).len(), 1, "{:?}", commands(&desk));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_grace_lets_a_dj_keep_the_air_until_it_ends_and_a_hold_stops_it() {
    let (p, desk, _onair) = post_plane(|r| r["grace_s"] = json!(3)).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "steeped", -3601, 60).await;
    episode(&p.app, "kettle", -1, 30).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    {
        let mut d = desk.lock().unwrap();
        d.people.push(At::dj("dj-heather", "Heather", true));
        d.people.push(At::source(&k, "Finlay Grant", 600_000));
    }
    // The post has come: the DJ on air keeps it, the grace counts.
    let plan = look(&p).await;
    assert!(commands(&desk).is_empty());
    let post = plan.post.unwrap();
    let ends = post.grace_ends_ms.expect("a grace is counting");
    assert_eq!(post.holders, ["Heather"]);
    // A producer holds this handover.
    let (_, producer) = member(&p, "shona", Role::Producer).await;
    let (_, presenter) = member(&p, "ewan", Role::Presenter).await;
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/air/post/hold",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{v}");
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/air/post/hold",
        Some(&producer),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let plan = look(&p).await;
    assert!(commands(&desk).is_empty(), "held: {:?}", commands(&desk));
    let post = plan.post.unwrap();
    assert_eq!(
        (post.held_by.as_deref(), post.grace_ends_ms),
        (Some("shona"), None)
    );
    // Let go: the grace was over, so the schedule hands over at the next look, with the two steps.
    let (code, _) = send(
        &p.app,
        "DELETE",
        "/api/v1/air/post/hold",
        Some(&producer),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(weavesignal_control::hub::now_ms() as i64 >= ends);
    look(&p).await;
    assert_eq!(
        commands(&desk),
        [format!("on:source-{k}"), "off:dj-heather".to_owned()]
    );
    let log = log_of(&p, &k).await;
    assert!(
        log.contains(&("on_air".into(), Some("schedule".into()))),
        "{log:?}"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_late_host_goes_on_air_after_the_count_and_go_and_not_yet_are_the_hosts_own() {
    use weavesignal_control::desk::StationMode;
    let (p, desk, onair) = post_plane(|r| r["late_count_s"] = json!(3)).await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "kettle", -600, 60).await;
    let late = key_id_of(&make_key(&p, "kettle", "Late Lena", "srt").await);
    let wary = key_id_of(&make_key(&p, "kettle", "Wary Will", "srt").await);
    onair.set_mode(StationMode::Auto).await.unwrap();
    // Lena stood by two seconds ago: the count runs three seconds from then.
    desk.lock()
        .unwrap()
        .people
        .push(At::source(&late, "Late Lena", 7_000));
    let plan = look(&p).await;
    assert!(commands(&desk).is_empty());
    let post = plan.post.unwrap();
    assert!(post.late_ends_ms.is_some(), "{post:?}");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // (the desk's own count of her connection has moved on with the clock)
    desk.lock().unwrap().people[0].connected_ms = Some(8_500);
    look(&p).await;
    assert_eq!(commands(&desk), [format!("on:source-{late}")]);
    assert_eq!(
        fades(&desk),
        [Some(2000)],
        "the count's end is the schedule's: it fades"
    );
    assert_eq!(mode_of(&p), Some(StationMode::Live));

    // Will arrives next, on a station that is automation's again; he says not yet.
    onair.set_mode(StationMode::Auto).await.unwrap();
    {
        let mut d = desk.lock().unwrap();
        d.people[0].on_air = false;
        d.people.push(At::source(&wary, "Wary Will", 7_000));
        d.commands.clear();
        d.fades.clear();
    }
    // (Lena's own airing is the same: her line says on air this airing, so only Will is a candidate.)
    p.state.hub.poll_once().await;
    let (code, v) = send(
        &p.app,
        "POST",
        &format!("/api/v1/sources/{wary}/not-yet"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    tokio::time::sleep(Duration::from_millis(3500)).await;
    look(&p).await;
    assert!(commands(&desk).is_empty(), "not yet: {:?}", commands(&desk));
    // ...and goes on air at his own press.
    let (code, v) = send(
        &p.app,
        "POST",
        &format!("/api/v1/sources/{wary}/go"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(commands(&desk), [format!("on:source-{wary}")]);
    assert_eq!(fades(&desk), [None], "Go on air now is a press: at once");
    assert_eq!(mode_of(&p), Some(StationMode::Live));
    // A key with no one at the desk, and someone else presenting: refused.
    let (code, _) = send(
        &p.app,
        "POST",
        &format!("/api/v1/sources/{late}/go"),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "someone else presents");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_fade_of_nothing_and_a_persons_switch_at_the_desk_are_at_once() {
    use weavesignal_control::desk::StationMode;
    // The rules' fade is 0: the schedule's move is at once too.
    let (p, desk, onair) = post_plane(|r| r["fade_ms"] = json!(0)).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "steeped", -3597, 60).await;
    episode(&p.app, "kettle", 2, 30).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    desk.lock()
        .unwrap()
        .people
        .push(At::source(&k, "Finlay Grant", 600_000));
    onair.set_mode(StationMode::Auto).await.unwrap();
    look(&p).await;
    tokio::time::sleep(Duration::from_millis(2300)).await;
    look(&p).await;
    assert_eq!(commands(&desk), [format!("on:source-{k}")]);
    assert_eq!(fades(&desk), [None], "a fade of 0 is not sent");

    // A switch at the desk through the API is a press, and a fade the client asks for is dropped.
    let (code, r) = send(
        &p.app,
        "POST",
        "/api/v1/desk/commands",
        Some(ADMIN),
        json!({"op": "on_air", "identity": format!("source-{k}"), "on": false, "fade_ms": 5000}),
    )
    .await;
    assert!(code.is_success(), "{code}: {r}");
    assert_eq!(commands(&desk).last().unwrap(), &format!("off:source-{k}"));
    assert_eq!(
        fades(&desk).last().unwrap(),
        &None,
        "a person's press is at once"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_dropped_link_is_covered_by_automation_and_the_return_goes_back_on_air() {
    use weavesignal_control::desk::StationMode;
    let (p, desk, _onair) = post_plane(|r| {
        r["silence_s"] = json!(2);
        r["steady_s"] = json!(2);
    })
    .await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "kettle", -600, 60).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    // On air, as a press put them; the schedule writes it down as the desk's.
    desk.lock().unwrap().people.push({
        let mut a = At::source(&k, "Finlay Grant", 600_000);
        a.on_air = true;
        a
    });
    look(&p).await;
    assert!(
        log_of(&p, &k)
            .await
            .contains(&("on_air".into(), Some("desk".into())))
    );
    assert!(commands(&desk).is_empty(), "the schedule only watched");

    // The link goes: the loss is marked, the place kept, the studio and the list say so.
    desk.lock().unwrap().people.clear();
    let plan = look(&p).await;
    let lost = plan.post.unwrap().link_lost.expect("link lost");
    assert_eq!(lost.key_id, k);
    assert_eq!(lost.place_kept_until_ms - lost.lost_ms, 300_000);
    assert_ne!(mode_of(&p), Some(StationMode::Auto), "not covered yet");
    let (_, listed) = send(&p.app, "GET", "/api/v1/sources", Some(ADMIN), Value::Null).await;
    let s = &listed["sources"][0];
    assert_eq!(s["state"], "link_lost", "{listed}");
    assert_eq!(s["place_kept_until_ms"], lost.place_kept_until_ms);
    // After the silence with nobody else on air, automation covers.
    tokio::time::sleep(Duration::from_millis(2300)).await;
    look(&p).await;
    assert_eq!(mode_of(&p), Some(StationMode::Auto));
    assert!(log_of(&p, &k).await.iter().any(|(e, _)| e == "covered"));
    look(&p).await;
    // It comes back (the desk puts a new connection off air); steady for two seconds, it goes back.
    desk.lock()
        .unwrap()
        .people
        .push(At::source(&k, "Finlay Grant", 500));
    look(&p).await;
    assert!(commands(&desk).is_empty(), "not steady yet");
    desk.lock().unwrap().people[0].connected_ms = Some(2_500);
    look(&p).await;
    assert_eq!(commands(&desk), [format!("on:source-{k}")]);
    assert_eq!(
        fades(&desk),
        [Some(2000)],
        "back after a lost link fades in"
    );
    assert_eq!(mode_of(&p), Some(StationMode::Live));
    assert!(log_of(&p, &k).await.iter().any(|(e, _)| e == "back"));
    look(&p).await;
    assert_eq!(commands(&desk).len(), 1, "once");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn off_air_lets_a_kept_place_go_at_once_and_is_logged_and_the_return_stands_by() {
    use weavesignal_control::desk::StationMode;
    // A long silence: only the press can end the place before the test does.
    let (p, desk, _onair) = post_plane(|r| {
        r["silence_s"] = json!(30);
        r["steady_s"] = json!(2);
    })
    .await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "kettle", -600, 60).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    let off_air = format!("/api/v1/sources/{k}/off-air");

    // No place is kept for a source that is on air, or never was.
    let (code, _) = send(&p.app, "POST", &off_air, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "nothing lost");
    desk.lock().unwrap().people.push({
        let mut a = At::source(&k, "Finlay Grant", 600_000);
        a.on_air = true;
        a
    });
    look(&p).await;
    let (code, _) = send(&p.app, "POST", &off_air, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "on air, not lost");

    // The link goes. Before the silence is over nothing covers it; the press lets the place
    // go and automation takes the air with it, so listeners are not left with nothing.
    desk.lock().unwrap().people.clear();
    let plan = look(&p).await;
    assert!(plan.post.unwrap().link_lost.is_some());
    assert_ne!(mode_of(&p), Some(StationMode::Auto));
    let (code, v) = send(&p.app, "POST", &off_air, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["covered"], true, "{v}");
    assert_eq!(mode_of(&p), Some(StationMode::Auto));
    let log = log_of(&p, &k).await;
    assert!(
        log.contains(&("place_let_go".into(), Some("station key".into()))),
        "logged, by whom: {log:?}"
    );
    assert!(log.iter().any(|(e, _)| e == "covered"), "{log:?}");
    // The place is gone from the view, and a second press has nothing to let go.
    let plan = look(&p).await;
    assert!(
        plan.post.is_none_or(|v| v.link_lost.is_none()),
        "the tile leaves"
    );
    let (code, _) = send(&p.app, "POST", &off_air, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "let go already");
    // It reconnects and stands by: nothing puts it back on air by itself.
    desk.lock()
        .unwrap()
        .people
        .push(At::source(&k, "Finlay Grant", 2_500));
    look(&p).await;
    assert!(
        commands(&desk).is_empty(),
        "stands by: {:?}",
        commands(&desk)
    );
    assert_eq!(mode_of(&p), Some(StationMode::Auto));
    // A stranger's bearer is refused.
    let (code, _) = send(&p.app, "POST", &off_air, None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_drop_during_the_grace_hands_over_at_once_and_lets_the_place_go() {
    let (p, desk, _onair) = post_plane(|r| {
        r["silence_s"] = json!(2);
        r["grace_s"] = json!(600);
    })
    .await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "steeped", -3605, 60).await;
    episode(&p.app, "kettle", -5, 30).await;
    let old = key_id_of(&make_key(&p, "steeped", "Old Olive", "srt").await);
    let next = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    {
        let mut d = desk.lock().unwrap();
        d.people.push({
            let mut a = At::source(&old, "Old Olive", 3_600_000);
            a.on_air = true;
            a
        });
        d.people.push(At::source(&next, "Finlay Grant", 600_000));
    }
    // The grace is on (ten minutes): nothing moves. The loss is then marked.
    look(&p).await;
    assert!(commands(&desk).is_empty());
    desk.lock()
        .unwrap()
        .people
        .retain(|a| a.identity != format!("source-{old}"));
    look(&p).await;
    tokio::time::sleep(Duration::from_millis(2300)).await;
    look(&p).await;
    assert_eq!(
        commands(&desk),
        [format!("on:source-{next}")],
        "handed over at once"
    );
    let log = log_of(&p, &old).await;
    assert!(log.iter().any(|(e, _)| e == "place_let_go"), "{log:?}");
    assert!(
        !log.iter().any(|(e, _)| e == "covered"),
        "automation did not cover: {log:?}"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn end_show_passes_the_air_to_the_next_host_standing_by() {
    let (p, desk, _onair) = post_plane(|_| {}).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "steeped", -1800, 60).await;
    episode(&p.app, "kettle", 1800, 60).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    {
        let mut d = desk.lock().unwrap();
        d.people.push(At::dj("dj-heather", "Heather", true));
        d.people.push(At::source(&k, "Finlay Grant", 600_000));
    }
    p.state.hub.poll_once().await;
    let (code, v) = send(&p.app, "POST", "/api/v1/show/end", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["passed_to"], "Finlay Grant", "{v}");
    assert_eq!(v["automation_in_ms"], Value::Null);
    assert_eq!(
        commands(&desk),
        [format!("on:source-{k}"), "off:dj-heather".to_owned()]
    );
    assert_eq!(
        fades(&desk),
        [Some(2000), Some(2000)],
        "End show passing the air crosses over: the one coming on and the one going off"
    );
    // With nobody standing by it is as it was: everyone off, and automation's.
    let (p, desk, _onair) = post_plane(|_| {}).await;
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -1800, 60).await;
    desk.lock()
        .unwrap()
        .people
        .push(At::dj("dj-heather", "Heather", true));
    p.state.hub.poll_once().await;
    let (code, v) = send(&p.app, "POST", "/api/v1/show/end", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["passed_to"], Value::Null);
    assert_eq!(commands(&desk), ["off:dj-heather".to_owned()]);
    assert_eq!(
        fades(&desk),
        [None],
        "with nobody to pass to it is at once, as before"
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn holding_a_handover_is_for_an_admin_or_a_producer_and_needs_one() {
    let (p, desk, _onair) = post_plane(|_| {}).await;
    let (_, producer) = member(&p, "shona", Role::Producer).await;
    // No show ahead: nothing to hold.
    look(&p).await;
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/air/post/hold",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "kettle", 600, 30).await;
    desk.lock()
        .unwrap()
        .people
        .push(At::dj("dj-heather", "Heather", true));
    let plan = look(&p).await;
    assert_eq!(plan.post.unwrap().show.unwrap().slug, "kettle");
    for who in [Some(ADMIN), Some(producer.as_str())] {
        let (code, v) = send(&p.app, "POST", "/api/v1/air/post/hold", who, Value::Null).await;
        assert_eq!(code, StatusCode::OK, "{v}");
    }
    let plan = look(&p).await;
    assert!(plan.post.unwrap().held_by.is_some());
    let (code, _) = send(&p.app, "POST", "/api/v1/air/post/hold", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn someone_on_air_before_the_post_and_taken_off_after_it_is_left_alone() {
    let (p, desk, _onair) = post_plane(|_| {}).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "kettle", "Kettle Drive").await;
    episode(&p.app, "steeped", -3597, 60).await;
    episode(&p.app, "kettle", 3, 30).await;
    let k = key_id_of(&make_key(&p, "kettle", "Finlay Grant", "srt").await);
    desk.lock().unwrap().people.push({
        let mut a = At::source(&k, "Finlay Grant", 600_000);
        a.on_air = true;
        a
    });
    // On air before the post (a press, as End show's pass is): only watched.
    look(&p).await;
    assert!(commands(&desk).is_empty());
    tokio::time::sleep(Duration::from_millis(3300)).await;
    look(&p).await;
    // After the post a producer takes it off, with nobody else on air: the schedule leaves it.
    desk.lock().unwrap().people[0].on_air = false;
    look(&p).await;
    look(&p).await;
    look(&p).await;
    assert!(commands(&desk).is_empty(), "{:?}", commands(&desk));
    let log = log_of(&p, &k).await;
    assert!(
        log.contains(&("off_air".into(), Some("desk".into()))),
        "{log:?}"
    );
    // And End show does not put them back on air either.
    p.state.hub.poll_once().await;
    let (code, v) = send(&p.app, "POST", "/api/v1/show/end", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["passed_to"], Value::Null, "{v}");
    assert!(commands(&desk).is_empty(), "{:?}", commands(&desk));
}

/// Phase 17g: the bridge and VBAN as ways of their own (migration 0045's CHECKs), and the kit's
/// settings that bind a VBAN stream to a key.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_bridge_and_vban_come_in_as_ways_of_their_own() {
    let p = plane().await;
    p.state.hub.sources.set_ingest_token(Some(INGEST.into()));
    p.state.hub.sources.set_bridge_port(7320);
    make_show(&p.app, "steeped", "Steeped").await;
    episode(&p.app, "steeped", -600, 120).await;
    // A key made for the bridge: the hint is a way of its own.
    let made = make_key(&p, "steeped", "Finlay Grant", "bridge").await;
    assert_eq!(made["key"]["hint"], "bridge", "{made}");
    let (k, s) = (
        made["key"]["key_id"].as_str().unwrap().to_owned(),
        made["secret"].as_str().unwrap().to_owned(),
    );

    // The bridge carries the Icecast password: right, it stands by; wrong, it is logged.
    let a = admit(&p, &k, Some(&s), "bridge", "198.51.100.4").await;
    assert_eq!(a["decision"], "standby", "{a}");
    assert!(a["passphrase"].is_null());
    let w = admit(
        &p,
        &k,
        Some("not-the-secret-at-all"),
        "bridge",
        "198.51.100.9",
    )
    .await;
    assert_eq!(w["reason"], "wrong_key", "{w}");
    assert_eq!(
        admit(&p, &k, None, "bridge", "198.51.100.4").await["decision"],
        "refused"
    );

    // VBAN: refused until the installation allows it and the kit binds the stream.
    let vban = |stream: &str, addr: &str| json!({"key_id": k, "way": "vban", "address": addr, "stream": stream});
    let (_, r) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/admit",
        vban("Stream1", "192.168.1.20"),
    )
    .await;
    assert_eq!(r["decision"], "refused", "{r}");
    let kit = json!({"bind": "192.168.1.10",
        "vban_in": [{"address": "192.168.1.20", "name": "Stream1", "key_id": k}]});
    let (code, v) = send(&p.app, "PUT", "/api/v1/kit", Some(ADMIN), kit.clone()).await;
    assert_eq!(code, StatusCode::CONFLICT, "not allowed here yet: {v}");
    p.state.hub.sources.set_vban_allowed(Ok(()));
    let (code, v) = send(&p.app, "PUT", "/api/v1/kit", Some(ADMIN), kit.clone()).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["allowed"], true);
    let (_, r) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/admit",
        vban("Stream1", "192.168.1.20"),
    )
    .await;
    assert_eq!(r["decision"], "standby", "{r}");
    for (stream, addr) in [("Stream2", "192.168.1.20"), ("Stream1", "192.168.1.21")] {
        let (_, r) = ingest(
            &p.app,
            "POST",
            "/api/v1/internal/sources/admit",
            vban(stream, addr),
        )
        .await;
        assert_eq!(r["decision"], "refused", "{stream} from {addr}: {r}");
    }
    // A VBAN event is logged with its way.
    let (code, v) = ingest(
        &p.app,
        "POST",
        "/api/v1/internal/sources/events",
        json!({"key_id": k, "event": "admitted", "reason": null, "detail": "", "way": "vban", "address": "192.168.1.20"}),
    )
    .await;
    assert!(code.is_success(), "{v}");

    // The ingest is told the bridge's port and the bindings.
    let (_, cfg) = ingest(
        &p.app,
        "GET",
        "/api/v1/internal/sources/config",
        Value::Null,
    )
    .await;
    assert_eq!(cfg["port_bridge"], 7320);
    assert_eq!(cfg["vban"]["bind"], "192.168.1.10");
    assert_eq!(cfg["vban"]["rules"][0]["name"], "Stream1");

    // The kit is checked, and a preset sets its routing.
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/kit",
        Some(ADMIN),
        json!({"bind": "203.0.113.5", "vban_in": kit["vban_in"]}),
    )
    .await;
    assert_eq!(
        (code, v["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("bind")),
        "{v}"
    );
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/kit/preset",
        Some(ADMIN),
        json!({"preset": "transmitter", "address": "192.168.1.40"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["kit"]["vban_out"][0]["source"], "programme");
    assert_eq!(v["kit"]["vban_in"][0]["key_id"], k.as_str());
    let (code, _) = send(&p.app, "GET", "/api/v1/kit/clock", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    // Not an API token's, and not without a bearer.
    let (code, _) = send(&p.app, "GET", "/api/v1/kit", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}
