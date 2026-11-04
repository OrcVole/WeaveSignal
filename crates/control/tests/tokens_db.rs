// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The public API against a real PostgreSQL (ADR-0195): tokens made, used, listed, revoked and
//! read back after a restart; webhooks made, guarded, delivered with a valid signature, retried
//! and recorded; and the open routes' data, with an erased guest gone from it. Ignored unless
//! asked for (`just test-db`). Ports 23400-23499 are this file's.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::post;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower::ServiceExt;
use weavesignal_control::api::{Api, ApiConfig, layer};
use weavesignal_control::apikey::{ApiKey, delivery_signature};
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::events;
use weavesignal_control::hub::Hub;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_control::webhooks;
use weavesignal_control::zone::StationZone;
use weavesignal_model::{Station, StationId, ensure_station};

const KEY: &str = "tokens-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";

struct Plane {
    app: Router,
    staff: Arc<Staff>,
}

/// A control plane as `serve` builds it (the studio's routes and the shows', the gate over
/// both); a new one over the same pool is a restart.
async fn plane(
    pool: &sqlx::PgPool,
    station: &Station,
    key: Option<&str>,
    config: ApiConfig,
    dir: Option<&std::path::Path>,
) -> Plane {
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        key.map(|k| ApiKey::parse(k).unwrap()),
        config,
        true,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    staff.restore(station.id).await;
    staff.api.restore().await;
    let publishing = match dir {
        Some(d) => PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            cards_dir: d.join("cards"),
            episodes_dir: d.join("episodes"),
            ..PublishingConfig::default()
        },
        None => PublishingConfig::default(),
    };
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig {
            forwarded_for: true,
            ..JoinConfig::default()
        },
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }))
    .merge(shows::router(Arc::new(ShowsState {
        station: station.clone(),
        db: Some(pool.clone()),
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        publishing,
        zone: Arc::new(StationZone::new("Europe/London")),
    })));
    Plane {
        app: layer(app, staff.clone()),
        staff,
    }
}

async fn setup(name: &str) -> (sqlx::PgPool, Station) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(&pool, &Station::new(StationId::new_random(), name).unwrap())
        .await
        .unwrap();
    (pool, station)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let (code, _, v) = send_h(app, method, uri, bearer, body).await;
    (code, v)
}

async fn send_h(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
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
    let (code, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        code,
        headers,
        serde_json::from_slice(&bytes).unwrap_or_default(),
    )
}

async fn eventually<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..80 {
        if let Some(v) = f().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never happened: {what}");
}

fn config() -> ApiConfig {
    ApiConfig {
        public_per_minute: 0,
        token_per_minute: 0,
        streams_per_address: 0,
        ..ApiConfig::default()
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_admin_makes_a_token_it_works_in_its_scope_and_revoking_ends_it() {
    let (pool, station) = setup("Tokens FM").await;
    let p = plane(&pool, &station, Some(TOKEN_KEY), config(), None).await;

    // The station key makes a token; its secret is in the answer, once.
    let (code, made) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(KEY),
        json!({"name": " Stream deck ", "scopes": ["carts:read", "carts:fire", "carts:read"], "expires_in_days": 30}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let secret = made["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("rsa_") && secret.len() == 68);
    assert_eq!(made["token"]["name"], "Stream deck");
    assert_eq!(made["token"]["scopes"], json!(["carts:fire", "carts:read"]));
    assert_eq!(made["token"]["created_by"], "station key");
    assert!(made["token"]["expires_ms"].is_i64());
    let id = made["token"]["id"].as_str().unwrap().to_owned();
    assert_eq!(made["token"]["prefix"], secret[..8]);

    // The list never has a secret, nor a hash.
    let (code, list) = send(&p.app, "GET", "/api/v1/tokens", Some(KEY), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list["tokens"].as_array().unwrap().len(), 1);
    assert!(!list.to_string().contains(&secret[4..]), "{list}");
    assert!(!list.to_string().contains("secret_hash"));
    assert!(list["scopes"].as_array().unwrap().len() >= 10);

    // It works where it holds the scope, and not elsewhere.
    let (code, _) = send(&p.app, "GET", "/api/v1/carts", Some(&secret), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = send(&p.app, "GET", "/api/v1/studio", Some(&secret), Value::Null).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // It is not an admin: not the tokens, not the staff, not even a read of the list.
    for (m, u, b) in [
        ("GET", "/api/v1/tokens", Value::Null),
        (
            "POST",
            "/api/v1/tokens",
            json!({"name": "x", "scopes": ["carts:read"]}),
        ),
        ("DELETE", &*format!("/api/v1/tokens/{id}"), Value::Null),
        ("GET", "/api/v1/staff", Value::Null),
    ] {
        let (code, _) = send(&p.app, m, u, Some(&secret), b).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{m} {u}");
    }

    // Its last use is written.
    let used = eventually("last_used_at", || async {
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT (extract(epoch FROM last_used_at) * 1000)::bigint FROM api_token WHERE id = $1::uuid",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap()
    })
    .await;
    assert!(used > 0);
    // The database holds a hash, not the secret.
    let kept: Vec<u8> = sqlx::query_scalar("SELECT secret_hash FROM api_token WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept.len(), 32);
    assert_ne!(kept, secret.as_bytes());
    assert_eq!(kept, ApiKey::parse(TOKEN_KEY).unwrap().token_hash(&secret));

    // A restart reads it back.
    let again = plane(&pool, &station, Some(TOKEN_KEY), config(), None).await;
    let (code, _) = send(
        &again.app,
        "GET",
        "/api/v1/carts",
        Some(&secret),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "tokens outlive a restart");
    // Another key finds none.
    let other = plane(
        &pool,
        &station,
        Some("fedcba9876543210fedcba9876543210"),
        config(),
        None,
    )
    .await;
    let (code, _) = send(
        &other.app,
        "GET",
        "/api/v1/carts",
        Some(&secret),
        Value::Null,
    )
    .await;
    assert_eq!(
        code,
        StatusCode::UNAUTHORIZED,
        "a different key revokes them all"
    );

    // Revoked: at once, and for good.
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/tokens/{id}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = send(&p.app, "GET", "/api/v1/carts", Some(&secret), Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let restarted = plane(&pool, &station, Some(TOKEN_KEY), config(), None).await;
    let (code, _) = send(
        &restarted.app,
        "GET",
        "/api/v1/carts",
        Some(&secret),
        Value::Null,
    )
    .await;
    assert_eq!(
        code,
        StatusCode::UNAUTHORIZED,
        "and a restart does not bring it back"
    );
    let (_, list) = send(&p.app, "GET", "/api/v1/tokens", Some(KEY), Value::Null).await;
    assert!(list["tokens"][0]["revoked_ms"].is_i64(), "{list}");
    let (code, _) = send(
        &p.app,
        "DELETE",
        "/api/v1/tokens/00000000-0000-0000-0000-000000000000",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn only_an_admin_makes_tokens_and_what_they_are_asked_for_is_checked() {
    let (pool, station) = setup("Tokens FM").await;
    let p = plane(&pool, &station, Some(TOKEN_KEY), config(), None).await;
    // A presenter signs in and cannot.
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": "fraser", "name": "Fraser Macleod", "role": "presenter", "password": "the presenter's phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let (_, s) = send(
        &p.app,
        "POST",
        "/api/v1/sessions",
        None,
        json!({"username": "fraser", "password": "the presenter's phrase"}),
    )
    .await;
    let presenter = s["token"].as_str().unwrap().to_owned();
    let (code, _) = send(
        &p.app,
        "GET",
        "/api/v1/tokens",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(&presenter),
        json!({"name": "x", "scopes": ["carts:read"]}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(&p.app, "GET", "/api/v1/tokens", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    // What is asked for.
    for (body, field) in [
        (json!({"name": "", "scopes": ["carts:read"]}), "name"),
        (json!({"name": "x", "scopes": []}), "scopes"),
        (json!({"name": "x", "scopes": ["admin"]}), "scopes"),
        (
            json!({"name": "x", "scopes": ["carts:read"], "expires_in_days": 0}),
            "expires_in_days",
        ),
        (
            json!({"name": "x", "scopes": ["carts:read"], "expires_in_days": 99999}),
            "expires_in_days",
        ),
    ] {
        let (code, v) = send(&p.app, "POST", "/api/v1/tokens", Some(KEY), body.clone()).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(v["field"], field, "{body}");
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn without_a_key_tokens_and_webhooks_are_off_and_say_which_variable() {
    let (pool, station) = setup("Tokens FM").await;
    let p = plane(&pool, &station, None, config(), None).await;
    for (m, u, b) in [
        (
            "POST",
            "/api/v1/tokens",
            json!({"name": "x", "scopes": ["carts:read"]}),
        ),
        ("GET", "/api/v1/tokens", Value::Null),
        ("GET", "/api/v1/webhooks", Value::Null),
    ] {
        let (code, v) = send(&p.app, m, u, Some(KEY), b).await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{m} {u}");
        assert!(
            v["error"]
                .as_str()
                .unwrap()
                .contains("WEAVESIGNAL_CONTROL_TOKEN_KEY"),
            "{v}"
        );
    }
}

// ---- webhooks ----

type Hit = (HeaderMap, Vec<u8>);

/// A receiver on a loopback port: it answers with these statuses in turn, then 200, and hands
/// every request it gets to the test.
async fn receiver(port: u16, answers: &[u16]) -> mpsc::UnboundedReceiver<Hit> {
    let (tx, rx) = mpsc::unbounded_channel();
    let answers = Arc::new(Mutex::new(answers.iter().copied().collect::<VecDeque<_>>()));
    let app = Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, body: Bytes| {
            let (tx, answers) = (tx.clone(), answers.clone());
            async move {
                tx.send((headers, body.to_vec())).ok();
                let code = answers.lock().unwrap().pop_front().unwrap_or(200);
                StatusCode::from_u16(code).unwrap()
            }
        }),
    );
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    rx
}

async fn hit(rx: &mut mpsc::UnboundedReceiver<Hit>) -> Hit {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a delivery in time")
        .expect("the receiver is open")
}

async fn make_hook(app: &Router, name: &str, url: &str, events: Value) -> (String, String) {
    let (code, v) = send(
        app,
        "POST",
        "/api/v1/webhooks",
        Some(KEY),
        json!({"name": name, "url": url, "events": events}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    (
        v["webhook"]["id"].as_str().unwrap().to_owned(),
        v["secret"].as_str().unwrap().to_owned(),
    )
}

async fn hook_state(pool: &sqlx::PgPool, id: &str) -> (Option<i32>, i32, Option<String>) {
    sqlx::query_as("SELECT last_status, failures, last_error FROM webhook WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_event_reaches_its_webhooks_signed_and_a_failure_is_retried_and_recorded() {
    let (pool, station) = setup("Hooks FM").await;
    let p = plane(
        &pool,
        &station,
        Some(TOKEN_KEY),
        ApiConfig {
            webhooks_allow_private: true,
            ..config()
        },
        None,
    )
    .await;
    webhooks::spawn_worker(p.staff.clone(), vec![Duration::from_millis(100)]);

    let mut good = receiver(23401, &[]).await;
    let mut flaky = receiver(23402, &[500]).await;
    let mut other = receiver(23403, &[]).await;
    let (good_id, good_secret) = make_hook(
        &p.app,
        "Website",
        "http://127.0.0.1:23401/hook",
        json!(["on_air.changed", "caller.queued"]),
    )
    .await;
    let (flaky_id, flaky_secret) = make_hook(
        &p.app,
        "Flaky",
        "http://127.0.0.1:23402/hook",
        json!(["on_air.changed"]),
    )
    .await;
    let (_, _) = make_hook(
        &p.app,
        "Other events",
        "http://127.0.0.1:23403/hook",
        json!(["episode.started"]),
    )
    .await;
    let (dead_id, _) = make_hook(
        &p.app,
        "Nobody home",
        "http://127.0.0.1:23498/hook",
        json!(["on_air.changed"]),
    )
    .await;
    // Off, and so deaf.
    let (off_id, _) = make_hook(
        &p.app,
        "Off",
        "http://127.0.0.1:23401/hook",
        json!(["on_air.changed"]),
    )
    .await;
    let (code, _) = send(
        &p.app,
        "PUT",
        &format!("/api/v1/webhooks/{off_id}"),
        Some(KEY),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);

    p.staff.api.events.publish(
        events::ON_AIR_CHANGED,
        json!({"on_air": "desk", "previous": "automation", "live": true}),
    );

    // The website gets it, once, with a signature anyone with the secret can check.
    let (headers, body) = hit(&mut good).await;
    let event: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(event["type"], "on_air.changed");
    assert_eq!(event["data"]["on_air"], "desk");
    assert!(event["at_ms"].as_u64().unwrap() > 0);
    assert_eq!(
        headers["x-weavesignal-signature"].to_str().unwrap(),
        signed(&good_secret, &headers, &body)
    );
    assert!(
        headers["x-weavesignal-signature"]
            .to_str()
            .unwrap()
            .starts_with("sha256=")
    );
    assert_ne!(
        headers["x-weavesignal-signature"].to_str().unwrap(),
        signed(&flaky_secret, &headers, &body),
        "each webhook has its own secret"
    );
    assert_eq!(headers["x-weavesignal-event"], "on_air.changed");
    assert_eq!(
        headers["x-weavesignal-delivery"].to_str().unwrap(),
        event["id"].as_str().unwrap()
    );
    assert_eq!(headers["content-type"], "application/json");

    // The flaky one answered 500, was tried again with the same delivery id, and was
    // recorded as delivered.
    let (first, first_body) = hit(&mut flaky).await;
    let (second, second_body) = hit(&mut flaky).await;
    assert_eq!(
        first["x-weavesignal-delivery"],
        second["x-weavesignal-delivery"]
    );
    assert_eq!(first_body, second_body);
    assert_eq!(
        second["x-weavesignal-signature"].to_str().unwrap(),
        signed(&flaky_secret, &second, &second_body)
    );
    eventually("flaky recorded as delivered", || async {
        let (status, failures, err) = hook_state(&pool, &flaky_id).await;
        (status == Some(200) && failures == 0 && err.is_none()).then_some(())
    })
    .await;

    // A webhook nobody answers is recorded as failing, with why.
    let (status, failures, err) = eventually("dead recorded", || async {
        let s = hook_state(&pool, &dead_id).await;
        (s.1 >= 2).then_some(s)
    })
    .await;
    assert_eq!(status, Some(0));
    assert!(failures >= 2);
    assert!(err.is_some());

    // The good one is recorded as delivered; the others heard nothing.
    eventually("good recorded", || async {
        (hook_state(&pool, &good_id).await.0 == Some(200)).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(other.try_recv().is_err(), "it did not ask for this event");
    assert!(
        good.try_recv().is_err(),
        "once, and the off one on its port is deaf too"
    );

    // The list shows how each is doing, and no secret.
    let (_, list) = send(&p.app, "GET", "/api/v1/webhooks", Some(KEY), Value::Null).await;
    let shown = list.to_string();
    assert!(
        !shown.contains("whsec_") && !shown.contains("salt"),
        "{shown}"
    );
    assert_eq!(list["events"].as_array().unwrap().len(), 5);
    let flaky_row = list["webhooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == flaky_id)
        .unwrap();
    assert_eq!(flaky_row["last_status"], 200);
    assert!(flaky_row["last_success_ms"].is_i64());

    // A new secret: the old one signs no more.
    let (code, v) = send(
        &p.app,
        "POST",
        &format!("/api/v1/webhooks/{good_id}/secret"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let renewed = v["secret"].as_str().unwrap().to_owned();
    assert_ne!(renewed, good_secret);
    p.staff
        .api
        .events
        .publish(events::CALLER_QUEUED, json!({"identity": "caller-1"}));
    let (headers, body) = hit(&mut good).await;
    assert_eq!(
        headers["x-weavesignal-signature"].to_str().unwrap(),
        signed(&renewed, &headers, &body)
    );
    assert_ne!(
        headers["x-weavesignal-signature"].to_str().unwrap(),
        signed(&good_secret, &headers, &body)
    );

    // Removed, it is not told.
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/webhooks/{good_id}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    p.staff
        .api
        .events
        .publish(events::CALLER_QUEUED, json!({"identity": "caller-2"}));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(good.try_recv().is_err());
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/webhooks/{good_id}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_webhook_must_be_https_to_a_public_address_unless_the_station_allows_otherwise() {
    let (pool, station) = setup("Hooks FM").await;
    let p = plane(&pool, &station, Some(TOKEN_KEY), config(), None).await;
    for url in [
        "https://10.0.0.5/x",
        "https://192.168.1.20:8443/x",
        "https://127.0.0.1/x",
        "https://[::1]/x",
        "https://localhost/x",
        "https://169.254.169.254/latest/meta-data",
        "http://127.0.0.1:23401/hook",
        "http://hooks.example/x",
        "https://user:pass@hooks.example/x",
        "not a web address",
    ] {
        let (code, v) = send(
            &p.app,
            "POST",
            "/api/v1/webhooks",
            Some(KEY),
            json!({"name": "x", "url": url, "events": ["on_air.changed"]}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {v}");
        assert_eq!(v["field"], "url", "{url}");
    }
    // Changing an address is checked the same way.
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/webhooks",
        Some(KEY),
        json!({"name": "ok", "url": "https://93.184.216.34/hook", "events": ["on_air.changed"]}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let id = v["webhook"]["id"].as_str().unwrap();
    let (code, v) = send(
        &p.app,
        "PUT",
        &format!("/api/v1/webhooks/{id}"),
        Some(KEY),
        json!({"url": "https://10.1.1.1/x"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    // And the rest of what is asked.
    for (body, field) in [
        (
            json!({"name": "", "url": "https://93.184.216.34/x", "events": ["on_air.changed"]}),
            "name",
        ),
        (
            json!({"name": "x", "url": "https://93.184.216.34/x", "events": []}),
            "events",
        ),
        (
            json!({"name": "x", "url": "https://93.184.216.34/x", "events": ["everything"]}),
            "events",
        ),
    ] {
        let (code, v) = send(&p.app, "POST", "/api/v1/webhooks", Some(KEY), body.clone()).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(v["field"], field);
    }
    // Admin only: a token with every scope cannot.
    let all: Vec<&str> = weavesignal_control::scopes::SCOPES
        .iter()
        .map(|(n, _)| *n)
        .collect();
    let (_, t) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(KEY),
        json!({"name": "all", "scopes": all}),
    )
    .await;
    let secret = t["secret"].as_str().unwrap();
    let (code, _) = send(&p.app, "GET", "/api/v1/webhooks", Some(secret), Value::Null).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
}

// ---- the open routes' data ----

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_open_routes_show_the_coming_week_and_published_episodes_and_never_an_erased_guest() {
    let (pool, station) = setup("Kettle FM").await;
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("open-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("episodes")).unwrap();
    let p = plane(&pool, &station, Some(TOKEN_KEY), config(), Some(&dir)).await;
    let ok = |(code, body): (StatusCode, Value)| {
        assert!(code.is_success(), "{code} {body}");
        body
    };
    let tomorrow = jiff::Zoned::now()
        .with_time_zone(jiff::tz::TimeZone::get("Europe/London").unwrap())
        .tomorrow()
        .unwrap()
        .weekday()
        .to_monday_one_offset();
    ok(send(&p.app, "PUT", "/api/v1/shows/kettle-talk", Some(KEY), json!({
        "name": "Kettle Talk", "description": "Talk from the Kettle.", "colour_slot": 2, "layout": "slate",
        "slot": {"weekday": tomorrow, "start": 1200, "minutes": 60}, "hosts": ["Shona Campbell"]
    })).await);
    ok(send(
        &p.app,
        "PUT",
        "/api/v1/shows/kettle-talk/series/1",
        Some(KEY),
        json!({"planned_episodes": 4}),
    )
    .await);
    ok(send(
        &p.app,
        "POST",
        "/api/v1/shows/kettle-talk/series/1/episodes",
        Some(KEY),
        json!({"count": 2}),
    )
    .await);
    ok(send(
        &p.app,
        "PATCH",
        "/api/v1/shows/kettle-talk/series/1/episodes/1",
        Some(KEY),
        json!({"title": "The ferries", "guests": ["Morag Ross", "Gregor Stewart"]}),
    )
    .await);

    // No bearer, any origin: the coming week has the first episode and not the second.
    let (code, headers, week) =
        send_h(&p.app, "GET", "/api/v1/public/schedule", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{week}");
    assert_eq!(headers["access-control-allow-origin"], "*");
    assert_eq!(week["zone"], "Europe/London");
    let eps = week["episodes"].as_array().unwrap();
    assert_eq!(eps.len(), 1, "{week}");
    let e = &eps[0];
    assert_eq!(e["show"]["slug"], "kettle-talk");
    assert_eq!(e["show"]["name"], "Kettle Talk");
    // Not aired: its slot only. Its words and guests are not announced until it airs.
    assert_eq!(e["aired"], false);
    assert_eq!(e["title"], Value::Null, "{e}");
    assert_eq!(e["description"], Value::Null, "{e}");
    assert_eq!(e["guests"], json!([]), "{e}");
    assert_eq!(e["number"], 1);
    assert_eq!(e["minutes"], 60);
    assert_eq!(e["hosts"], json!(["Shona Campbell"]));
    assert!(!week.to_string().contains("ferries"), "{week}");
    assert!(!week.to_string().contains("Morag"), "{week}");
    assert!(e["air_at"].as_str().unwrap().contains("T20:00:00"), "{e}");
    assert!(e["air_at_ms"].as_u64().unwrap() > 0);
    // Nothing of the studio's own, nothing that names a person by id.
    let text = week.to_string();
    for private in ["inherited", "frozen", "feed_guid", "person", "\"slot\""] {
        assert!(!text.contains(private), "{private} in {text}");
    }
    let (_, two) = send(
        &p.app,
        "GET",
        "/api/v1/public/schedule?days=14",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(two["episodes"].as_array().unwrap().len(), 2);
    for bad in ["days=0", "days=15", "days=x"] {
        let (code, _) = send(
            &p.app,
            "GET",
            &format!("/api/v1/public/schedule?{bad}"),
            None,
            Value::Null,
        )
        .await;
        assert!(code.is_client_error(), "{bad}");
    }

    // Nothing published yet.
    let (code, v) = send(&p.app, "GET", "/api/v1/public/episodes", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["episodes"], json!([]));

    // On air, with files: published.
    ok(send(
        &p.app,
        "POST",
        "/api/v1/shows/kettle-talk/series/1/episodes/1/air",
        Some(KEY),
        Value::Null,
    )
    .await);
    std::fs::copy(
        root().join("crates/feeds/tests/fixtures/tone.mp3"),
        dir.join("episodes/kettle-talk-s1e1.mp3"),
    )
    .unwrap();
    std::fs::copy(
        root().join("assets/carts-demo/ding-1.opus"),
        dir.join("episodes/kettle-talk-s1e1.opus"),
    )
    .unwrap();
    ok(send(
        &p.app,
        "PUT",
        "/api/v1/shows/kettle-talk/series/1/episodes/1/files",
        Some(KEY),
        json!({"enclosure": "kettle-talk-s1e1.mp3", "opus": "kettle-talk-s1e1.opus"}),
    )
    .await);
    let (code, headers, v) = send_h(
        &p.app,
        "GET",
        "/api/v1/public/episodes?limit=5",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(headers["access-control-allow-origin"], "*");
    let ep = &v["episodes"][0];
    assert_eq!(ep["title"], "The ferries");
    assert_eq!(
        ep["audio"]["url"],
        "https://kettle.example/media/episodes/kettle-talk-s1e1.mp3"
    );
    assert_eq!(ep["audio"]["mime"], "audio/mpeg");
    assert!(ep["audio_opus"]["url"].as_str().unwrap().ends_with(".opus"));
    assert!(
        ep["image"]
            .as_str()
            .unwrap()
            .starts_with("https://kettle.example/cards/")
    );
    assert_eq!(ep["link"], "https://kettle.example/kettle-talk/s1/e1");
    assert_eq!(ep["feed"], "https://kettle.example/feeds/kettle-talk.xml");
    assert_eq!(ep["guests"], json!(["Morag Ross", "Gregor Stewart"]));
    assert_eq!(ep["aired"], true);
    // Aired, the schedule shows its words too.
    let (_, two) = send(
        &p.app,
        "GET",
        "/api/v1/public/schedule?days=14",
        None,
        Value::Null,
    )
    .await;
    let first = &two["episodes"][0];
    assert_eq!(
        (first["title"].clone(), first["aired"].clone()),
        (json!("The ferries"), json!(true)),
        "{two}"
    );
    let (_, none) = send(
        &p.app,
        "GET",
        "/api/v1/public/episodes?before_ms=1",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(none["episodes"], json!([]), "paged back past the start");
    let (code, _) = send(
        &p.app,
        "GET",
        "/api/v1/public/episodes?limit=0",
        None,
        Value::Null,
    )
    .await;
    assert!(code.is_client_error());

    // Erased at her request, she is in neither.
    let people = ok(send(&p.app, "GET", "/api/v1/people", Some(KEY), Value::Null).await);
    let morag = people
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "Morag Ross")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/people/{morag}/erase"),
        Some(KEY),
        Value::Null,
    )
    .await);
    for uri in ["/api/v1/public/schedule?days=14", "/api/v1/public/episodes"] {
        let (code, v) = send(&p.app, "GET", uri, None, Value::Null).await;
        assert_eq!(code, StatusCode::OK);
        assert!(!v.to_string().contains("Morag"), "{uri}: {v}");
        assert!(v.to_string().contains("Gregor Stewart"), "{uri}: {v}");
    }

    // The station's facts.
    let (code, v) = send(&p.app, "GET", "/api/v1/public/station", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["station"], "Kettle FM");
    std::fs::remove_dir_all(&dir).ok();
}

/// The signature a delivery should carry: over its `X-WeaveSignal-Timestamp`, a full stop and the
/// body. The timestamp is this minute's (a receiver refuses an old one).
fn signed(secret: &str, headers: &axum::http::HeaderMap, body: &[u8]) -> String {
    let ts: u64 = headers["x-weavesignal-timestamp"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        now.abs_diff(ts) < 300,
        "the timestamp is not fresh: {ts} at {now}"
    );
    delivery_signature(secret, ts, body)
}
