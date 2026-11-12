// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The beginner steps (ADR-0271, ADR-0274) through the control plane's routes against a real
//! PostgreSQL: the station's step comes from the show on air by the schedule, stays with the
//! show last on air between shows and across a restart, is full with no show ever on air; the
//! step route and the show editor's save change it under the same rule, log it and tell every
//! window; and the chat's moderation counts reach the studio's view.
//! Ignored unless asked for (`just test-db`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use jiff::ToSpan;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::app::{AppState, RoomState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig, RoomConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::studio::Studio;
use weavesignal_model::staff::Role;
use weavesignal_model::{Station, StationId, ensure_station};
use weavesignal_proto::Keys;

const ADMIN: &str = "steps-db-test";

async fn send(
    app: &Router,
    bearer: Option<&str>,
    method: &str,
    uri: &str,
    body: Value,
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
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, body)
}

struct Plane {
    app: Router,
    state: Arc<AppState>,
    pool: sqlx::PgPool,
    station: Station,
}

async fn pool() -> sqlx::PgPool {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    sqlx::PgPool::connect(&url).await.expect("connect")
}

async fn plane() -> Plane {
    let pool = pool().await;
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    plane_for(pool, station).await
}

/// A control plane over `station`: calling it again is a restart.
async fn plane_for(pool: sqlx::PgPool, station: Station) -> Plane {
    let staff = Arc::new(weavesignal_control::staff::Staff::new(Some(pool.clone())));
    staff.restore(station.id).await;
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    let live = hub.live.clone();
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("steps-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: Some(RoomState {
            config: RoomConfig::default(),
            keys: Keys {
                api_key: "weavesignal".into(),
                api_secret: "check-only-check-only-check-only-0".into(),
            },
        }),
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig {
            consent_contact: "info@kettle.example".into(),
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
        staff,
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: dir.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: Some(live),
    })));
    Plane {
        app,
        state,
        pool,
        station,
    }
}

/// A signed-in member of staff: their account and session token.
async fn staff_member(p: &Plane, username: &str, name: &str, role: Role) -> (uuid::Uuid, String) {
    let account = weavesignal_model::staff::create_account(
        &p.pool,
        p.station.id,
        username,
        name,
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

async fn make_show(app: &Router, slug: &str, name: &str, extra: Value) -> Value {
    let mut body = json!({"name": name, "colour_slot": 2});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (code, v) = send(
        app,
        Some(ADMIN),
        "PUT",
        &format!("/api/v1/shows/{slug}"),
        body,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    v
}

/// Series 1, episode 1 of `slug` starting `from_now_s` seconds from now (negative: ago), for
/// `minutes`.
async fn episode(app: &Router, slug: &str, from_now_s: i64, minutes: u32) {
    let (code, _) = send(
        app,
        Some(ADMIN),
        "PUT",
        &format!("/api/v1/shows/{slug}/series/1"),
        json!({}),
    )
    .await;
    assert!(code.is_success());
    let (code, body) = send(
        app,
        Some(ADMIN),
        "POST",
        &format!("/api/v1/shows/{slug}/series/1/episodes"),
        json!({"count": 1}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let at = (jiff::Timestamp::now() + from_now_s.seconds())
        .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap())
        .to_string();
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PATCH",
        &format!("/api/v1/shows/{slug}/series/1/episodes/1"),
        json!({"air_at": at, "minutes": minutes}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

async fn studio(p: &Plane) -> Value {
    let (code, v) = send(&p.app, Some(ADMIN), "GET", "/api/v1/studio", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    v
}

async fn put_step(p: &Plane, bearer: Option<&str>, step: &str) -> (StatusCode, Value) {
    send(
        &p.app,
        bearer,
        "PUT",
        "/api/v1/shows/morning-kettle/step",
        json!({ "step": step }),
    )
    .await
}

async fn save_show(p: &Plane, bearer: &str, step: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({"name": "The Morning Kettle", "colour_slot": 2});
    if let Some(s) = step {
        body["step"] = json!(s);
    }
    send(
        &p.app,
        Some(bearer),
        "PUT",
        "/api/v1/shows/morning-kettle",
        body,
    )
    .await
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_step_follows_the_show_on_air_and_is_kept_between_shows_and_across_a_restart() {
    let p = plane().await;
    // No show has ever been on air: full, and call-in is not closed by a step.
    weavesignal_control::steps::refresh(&p.state).await;
    let v = studio(&p).await;
    assert_eq!(v["step"]["step"], "full");
    assert!(v["step"]["show"].is_null() && v["step"]["on_air"] == false);

    // The Morning Kettle (Solo) is on air now; Steeped (Full) starts in two hours.
    make_show(&p.app, "morning-kettle", "The Morning Kettle", json!({})).await;
    make_show(&p.app, "steeped", "Steeped", json!({"step": "full"})).await;
    episode(&p.app, "morning-kettle", -600, 60).await;
    episode(&p.app, "steeped", 7_200, 60).await;
    weavesignal_control::steps::refresh(&p.state).await;
    let v = studio(&p).await;
    assert_eq!(v["step"]["step"], "solo", "a new show starts at Solo");
    assert_eq!(v["step"]["show"]["slug"], "morning-kettle");
    assert_eq!(v["step"]["show"]["name"], "The Morning Kettle");
    assert_eq!(v["step"]["on_air"], true);
    assert_eq!(v["step"]["next"]["slug"], "steeped");
    assert_eq!(v["step"]["next"]["step"], "full");
    assert!(v["step"]["next"]["start_ms"].as_u64().unwrap() > 0);
    // Call-in is closed with the design's words.
    let (_, player) = send(&p.app, None, "GET", "/api/v1/player", Value::Null).await;
    assert_eq!(player["call_in"]["open"], false);
    assert_eq!(
        player["call_in"]["why_not"],
        "The lines are closed for this show."
    );

    // Its time ends: no show is on air, and the station keeps its step.
    let (code, body) = send(
        &p.app,
        Some(ADMIN),
        "PATCH",
        "/api/v1/shows/morning-kettle/series/1/episodes/1",
        json!({"air_at": (jiff::Timestamp::now() - 3.hours())
            .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap()).to_string(),
            "minutes": 60}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    weavesignal_control::steps::refresh(&p.state).await;
    let v = studio(&p).await;
    assert_eq!(v["step"]["step"], "solo", "the show last on air");
    assert_eq!(v["step"]["on_air"], false);
    assert_eq!(v["step"]["show"]["slug"], "morning-kettle");

    // The control plane starts again: it reads the same from the schedule before serving.
    let again = plane_for(p.pool.clone(), p.station.clone()).await;
    assert_eq!(
        studio(&again).await["step"]["step"],
        "full",
        "before it has looked"
    );
    weavesignal_control::steps::refresh(&again.state).await;
    let v = studio(&again).await;
    assert_eq!(v["step"]["step"], "solo");
    assert_eq!(v["step"]["show"]["slug"], "morning-kettle");

    // Steeped takes the air: Full, the lines open.
    let (code, body) = send(
        &p.app,
        Some(ADMIN),
        "PATCH",
        "/api/v1/shows/steeped/series/1/episodes/1",
        json!({"air_at": (jiff::Timestamp::now() - 60.seconds())
            .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap()).to_string(),
            "minutes": 60}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    weavesignal_control::steps::refresh(&again.state).await;
    let v = studio(&again).await;
    assert_eq!(
        (v["step"]["step"].as_str(), v["step"]["on_air"].as_bool()),
        (Some("full"), Some(true))
    );
    assert!(v["step"]["next"].is_null());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_step_route_checks_who_logs_the_change_and_tells_every_window() {
    let p = plane().await;
    let (presenter_id, presenter) =
        staff_member(&p, "eilidh", "Eilidh Fraser-MacLeod", Role::Presenter).await;
    let (_, other) = staff_member(&p, "gregor", "Gregor Munro", Role::Presenter).await;
    let (_, producer) = staff_member(&p, "shona", "Shona Campbell", Role::Producer).await;
    let (_, screener) = staff_member(&p, "ailsa", "Ailsa Reid", Role::Screener).await;
    make_show(
        &p.app,
        "morning-kettle",
        "The Morning Kettle",
        json!({"presenter_account": presenter_id}),
    )
    .await;
    episode(&p.app, "morning-kettle", -600, 60).await;
    weavesignal_control::steps::refresh(&p.state).await;
    assert_eq!(studio(&p).await["step"]["step"], "solo");

    assert_eq!(
        put_step(&p, None, "guests").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        put_step(&p, Some(&other), "guests").await.0,
        StatusCode::FORBIDDEN,
        "another presenter"
    );
    assert_eq!(
        put_step(&p, Some(&screener), "guests").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put_step(&p, Some(ADMIN), "nonsense").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (code, _) = send(
        &p.app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/nope/step",
        json!({"step": "full"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // Nothing moved.
    assert_eq!(studio(&p).await["step"]["step"], "solo");

    // The show's own presenter moves it up, and it takes effect at once (no wait for a look).
    let (code, body) = put_step(&p, Some(&presenter), "guests").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        (body["previous"].as_str(), body["step"].as_str()),
        (Some("solo"), Some("guests"))
    );
    assert_eq!(body["station"]["step"], "guests");
    assert_eq!(body["station"]["may_change"], true);
    let v = studio(&p).await;
    assert_eq!(v["step"]["step"], "guests");
    let titles: Vec<&str> = v["notices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "step.moved")
        .map(|n| n["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Eilidh moved The Morning Kettle to Guests."]);
    // A producer and an admin may; the same step again is no change and no notice.
    assert_eq!(
        put_step(&p, Some(&producer), "callers").await.0,
        StatusCode::OK
    );
    assert_eq!(put_step(&p, Some(ADMIN), "callers").await.0, StatusCode::OK);
    assert_eq!(studio(&p).await["step"]["step"], "callers");
    let (_, body) = send(
        &p.app,
        Some(&producer),
        "GET",
        "/api/v1/studio",
        Value::Null,
    )
    .await;
    assert_eq!(body["step"]["may_change"], true);
    let (_, body) = send(&p.app, Some(&other), "GET", "/api/v1/studio", Value::Null).await;
    assert_eq!(body["step"]["may_change"], false);

    // Each change was logged, with who.
    let show = weavesignal_model::shows::get_show(&p.pool, p.station.id, "morning-kettle")
        .await
        .unwrap()
        .unwrap();
    let log = weavesignal_model::shows::step_changes(&p.pool, show.id)
        .await
        .unwrap();
    assert_eq!(
        log.iter()
            .map(|c| (c.from.as_str(), c.to.as_str(), c.by.as_str()))
            .collect::<Vec<_>>(),
        vec![("solo", "guests", "eilidh"), ("guests", "callers", "shona"),]
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_show_editors_save_changes_the_step_under_the_same_rule() {
    let p = plane().await;
    let (presenter_id, presenter) =
        staff_member(&p, "eilidh", "Eilidh Fraser-MacLeod", Role::Presenter).await;
    let (_, other) = staff_member(&p, "gregor", "Gregor Munro", Role::Presenter).await;
    let v = make_show(
        &p.app,
        "morning-kettle",
        "The Morning Kettle",
        json!({"presenter_account": presenter_id}),
    )
    .await;
    assert_eq!(v["show"]["step"], "solo");
    // Left out: kept. The same step sent: fine. Another: only for those who may.
    assert_eq!(save_show(&p, &other, None).await.0, StatusCode::OK);
    assert_eq!(save_show(&p, &other, Some("solo")).await.0, StatusCode::OK);
    let (code, body) = save_show(&p, &other, Some("full")).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    let (code, body) = save_show(&p, &presenter, Some("callers")).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["step"], "callers");
    let (_, body) = save_show(&p, &other, None).await;
    assert_eq!(
        body["show"]["step"], "callers",
        "a save without it keeps it"
    );
    // And the change is in the log, with who.
    let show = weavesignal_model::shows::get_show(&p.pool, p.station.id, "morning-kettle")
        .await
        .unwrap()
        .unwrap();
    let log = weavesignal_model::shows::step_changes(&p.pool, show.id)
        .await
        .unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].from.as_str(), log[0].to.as_str(), log[0].by.as_str()),
        ("solo", "callers", "eilidh")
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_views_chat_counts_are_what_waits_for_a_moderator() {
    let p = plane().await;
    weavesignal_control::steps::refresh(&p.state).await;
    let v = studio(&p).await;
    assert_eq!(
        (
            v["chat"]["held"].as_u64(),
            v["chat"]["open_reports"].as_u64()
        ),
        (Some(0), Some(0))
    );
    sqlx::query(
        "INSERT INTO chat_message (station_id, id, kind, body, server_at, state)
         VALUES ($1, 1, 'chat', 'hello', now(), 'held')",
    )
    .bind(p.station.id.as_uuid())
    .execute(&p.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_report (id, station_id, kind, message_id, reason)
         VALUES (gen_random_uuid(), $1, 'report', 1, 'abuse')",
    )
    .bind(p.station.id.as_uuid())
    .execute(&p.pool)
    .await
    .unwrap();
    weavesignal_control::steps::refresh(&p.state).await;
    let v = studio(&p).await;
    assert_eq!(
        (
            v["chat"]["held"].as_u64(),
            v["chat"]["open_reports"].as_u64()
        ),
        (Some(1), Some(1))
    );
}
