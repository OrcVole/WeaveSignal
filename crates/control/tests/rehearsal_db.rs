// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio's routes against a real PostgreSQL (1.3.0, steps R6 and R7): a show
//! found by its slug, the session kept across a restart of the control plane, one show at a
//! time, and the end. No desk answers here (the fake-socket tests are the crate's own).
//! Ignored unless asked for (`just test-db`).

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, RehearsalConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::rehearsal::{Rehearsal, Store};
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "rehearsal-db-test";

async fn pool() -> sqlx::PgPool {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    sqlx::PgPool::connect(&url).await.expect("connect")
}

/// A control plane over `station` with the rehearsal studio on: calling it again is a restart.
async fn plane(pool: &sqlx::PgPool, station: &Station) -> Router {
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    let r = Rehearsal::new(
        RehearsalConfig {
            enabled: true,
            ..RehearsalConfig::default()
        },
        "studio",
        Store::Db {
            pool: pool.clone(),
            station: station.id,
        },
        Duration::from_millis(250),
    );
    r.restore().await.unwrap();
    assert!(hub.rehearsal.set(Arc::new(r)).is_ok());
    router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: Arc::new(weavesignal_control::staff::Staff::new(Some(pool.clone()))),
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }))
}

async fn send(app: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {ADMIN}"));
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

async fn show(pool: &sqlx::PgPool, station: StationId, slug: &str, name: &str) {
    sqlx::query(
        "INSERT INTO show (id, station_id, slug, name, colour_slot) VALUES ($1, $2, $3, $4, 2)",
    )
    .bind(Uuid::new_v4())
    .bind(station.as_uuid())
    .bind(slug)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_session_is_kept_across_a_restart_and_ends_once() {
    let pool = pool().await;
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    show(&pool, station.id, "steeped", "Steeped").await;
    show(
        &pool,
        station.id,
        "the-morning-kettle",
        "The Morning Kettle",
    )
    .await;

    let app = plane(&pool, &station).await;
    let (code, view) = send(
        &app,
        "POST",
        "/api/v1/rehearsal/open",
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{view}");
    assert_eq!(view["session"]["show_name"], "Steeped");
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/rehearsal/open",
        json!({"show": "nae-such"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // A restart reads the session back; another show is still refused.
    let app = plane(&pool, &station).await;
    let (_, after) = send(&app, "GET", "/api/v1/rehearsal", Value::Null).await;
    assert_eq!(after["session"]["id"], view["session"]["id"]);
    let (code, body) = send(
        &app,
        "POST",
        "/api/v1/rehearsal/open",
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");

    // No desk answers: a command says so and is not kept anywhere.
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/rehearsal/commands",
        json!({"op": "mute", "identity": "eilidh", "on": true}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);

    let (code, ended) = send(&app, "POST", "/api/v1/rehearsal/end", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{ended}");
    assert!(ended["session"].is_null());
    let (how,): (String,) =
        sqlx::query_as("SELECT ended_how FROM rehearsal_session WHERE station_id = $1")
            .bind(station.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(how, "ended");
    // After a restart there is still nothing open, and the other show may have the studio.
    let app = plane(&pool, &station).await;
    let (_, view) = send(&app, "GET", "/api/v1/rehearsal", Value::Null).await;
    assert!(view["session"].is_null());
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/rehearsal/open",
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
}
