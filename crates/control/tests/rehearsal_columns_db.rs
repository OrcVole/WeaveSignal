// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Studios as columns and the rehearsal studio's bookings through the control plane's routes
//! against a real PostgreSQL (1.3.0, step R14): bookings made, refused when they would share
//! time, changed, deleted, and refused to those who do not run the show; Open refused while
//! another show's booking holds the studio; the week's `studios`, each episode's `studio` and
//! `from` (agreeing with the rota), `rehearsals`; and the public calendar file listing no
//! rehearsal. No desk answers here. Ignored unless asked for (`just test-db`).

use std::path::PathBuf;
use std::sync::Arc;
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
use weavesignal_control::config::{CartsConfig, JoinConfig, PublishingConfig, RehearsalConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::rehearsal::{Rehearsal, Store};
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_model::staff::Role;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "rehearsal-columns-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";
const H: i64 = 3_600_000;

struct Plane {
    app: Router,
    state: Arc<AppState>,
    pool: sqlx::PgPool,
    station: Station,
}

/// A control plane with the studio's and the shows' routes; the rehearsal studio on or off.
async fn plane(rehearsal: bool) -> Plane {
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
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    if rehearsal {
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
    }
    let live = hub.live.clone();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("rehearsal-columns-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        hub: hub.clone(),
        join: JoinConfig::default(),
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
        hub: Some(hub),
        video: None,
    })));
    Plane {
        app: layer(app, staff),
        state,
        pool,
        station,
    }
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
    req.extensions_mut().insert(ConnectInfo(
        "192.0.2.60:4000".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (code, v)
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

/// A member of staff with a session: their account's id and bearer.
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

fn now_ms() -> i64 {
    weavesignal_control::hub::now_ms() as i64
}

fn book(show: &str, start_ms: i64, end_ms: i64) -> Value {
    json!({"show": show, "start_ms": start_ms, "end_ms": end_ms})
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn bookings_are_made_changed_and_deleted_by_those_who_run_the_show() {
    let p = plane(true).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "the-morning-kettle", "The Morning Kettle").await;
    let t0 = (now_ms() / H + 26) * H;
    let range = format!(
        "/api/v1/rehearsal/bookings?from_ms={}&to_ms={}",
        t0 - H,
        t0 + 4 * H
    );

    let (code, first) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("steeped", t0, t0 + H),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{first}");
    assert_eq!(first["kind"], "booking");
    assert_eq!(first["show_name"], "Steeped");
    assert_eq!(first["colour_slot"], 3);
    assert_eq!(first["by"], "The station key");
    assert_eq!(
        (first["start_ms"].as_i64(), first["end_ms"].as_i64()),
        (Some(t0), Some(t0 + H))
    );
    let first_id = first["id"].as_str().unwrap().to_owned();

    // Sharing time: 409, naming the booking in the way.
    let (code, refused) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("the-morning-kettle", t0 + H / 2, t0 + 2 * H),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["clash"]["id"], first["id"]);
    assert!(refused["error"].as_str().unwrap().contains("Steeped"));
    // Touching is not sharing.
    let (code, second) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("the-morning-kettle", t0 + H, t0 + 2 * H),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{second}");

    // Times that cannot be: 422 naming the field.
    for (body, field) in [
        (book("steeped", t0 + 3 * H, t0 + 3 * H), "end_ms"),
        (book("steeped", t0 + 3 * H, t0 + 3 * H + 60_000), "end_ms"),
        (book("steeped", t0 + 3 * H, t0 + 3 * H + 25 * H), "end_ms"),
        (book("steeped", now_ms() - H, now_ms() + H), "start_ms"),
    ] {
        let (code, v) = send(
            &p.app,
            "POST",
            "/api/v1/rehearsal/bookings",
            Some(ADMIN),
            body,
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        assert_eq!(v["field"], field, "{v}");
    }
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("nae-such", t0 + 3 * H, t0 + 4 * H),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Listed by the time asked for, in start order.
    let (code, list) = send(&p.app, "GET", &range, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{list}");
    assert_eq!(list["enabled"], true);
    let ids: Vec<_> = list["bookings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    assert_eq!(ids, [first["id"].clone(), second["id"].clone()]);

    // Moved within its own time; onto the other's, refused; given to the other show.
    let one = format!("/api/v1/rehearsal/bookings/{first_id}");
    let (code, moved) = send(
        &p.app,
        "PATCH",
        &one,
        Some(ADMIN),
        json!({"start_ms": t0 + H / 4}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{moved}");
    assert_eq!(moved["start_ms"].as_i64(), Some(t0 + H / 4));
    assert_eq!(moved["end_ms"].as_i64(), Some(t0 + H));
    let (code, refused) = send(
        &p.app,
        "PATCH",
        &one,
        Some(ADMIN),
        json!({"end_ms": t0 + H + 60_000}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["clash"]["id"], second["id"]);
    let (code, given) = send(
        &p.app,
        "PATCH",
        &one,
        Some(ADMIN),
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{given}");
    assert_eq!(given["show"], "the-morning-kettle");

    // Another show's presenter may not touch it; its own presenter and a producer may.
    let (kirsty, kirsty_bearer) = member(&p, "kirsty", Role::Presenter).await;
    let (_, finlay_bearer) = member(&p, "finlay", Role::Presenter).await;
    let (_, gregor_bearer) = member(&p, "gregor", Role::Producer).await;
    let (code, v) = send(
        &p.app,
        "PUT",
        "/api/v1/shows/the-morning-kettle",
        Some(ADMIN),
        json!({"name": "The Morning Kettle", "colour_slot": 3, "presenter_account": kirsty}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let (code, _) = send(
        &p.app,
        "PATCH",
        &one,
        Some(&finlay_bearer),
        json!({"end_ms": t0 + H / 2}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(&finlay_bearer),
        book("the-morning-kettle", t0 + 3 * H, t0 + 4 * H),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, mine) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(&kirsty_bearer),
        book("the-morning-kettle", t0 + 3 * H, t0 + 4 * H),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{mine}");
    assert_eq!(mine["by"], "kirsty");
    // Kirsty may not give her booking to a show she does not present.
    let (code, _) = send(
        &p.app,
        "PATCH",
        &format!(
            "/api/v1/rehearsal/bookings/{}",
            mine["id"].as_str().unwrap()
        ),
        Some(&kirsty_bearer),
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // Reading is any staff member's.
    let (code, _) = send(&p.app, "GET", &range, Some(&finlay_bearer), Value::Null).await;
    assert_eq!(code, StatusCode::OK);

    // An API token reads with `schedule:read`, and never books.
    let (code, made) = send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(ADMIN),
        json!({"name": "Rota board", "scopes": ["schedule:read"]}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let secret = made["secret"].as_str().unwrap().to_owned();
    let (code, _) = send(&p.app, "GET", &range, Some(&secret), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    for (m, u, b) in [
        (
            "POST",
            "/api/v1/rehearsal/bookings",
            book("steeped", t0 + 5 * H, t0 + 6 * H),
        ),
        ("DELETE", one.as_str(), Value::Null),
    ] {
        let (code, _) = send(&p.app, m, u, Some(&secret), b).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{m} {u}");
    }

    // Deleted by a producer; gone after.
    let (code, _) = send(&p.app, "DELETE", &one, Some(&gregor_bearer), Value::Null).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = send(&p.app, "DELETE", &one, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // A booking that is over stays as it was.
    sqlx::query(
        "INSERT INTO rehearsal_booking (id, station_id, show_id, starts_at, ends_at, booked_by)
         SELECT $1, $2, id, now() - interval '3 hours', now() - interval '2 hours', 'Finlay'
         FROM show WHERE station_id = $2 AND slug = 'steeped'",
    )
    .bind(uuid::Uuid::from_u128(77))
    .bind(p.station.id.as_uuid())
    .execute(&p.pool)
    .await
    .unwrap();
    let old = format!("/api/v1/rehearsal/bookings/{}", uuid::Uuid::from_u128(77));
    let (code, _) = send(
        &p.app,
        "PATCH",
        &old,
        Some(ADMIN),
        json!({"end_ms": now_ms() + H}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = send(&p.app, "DELETE", &old, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn open_is_refused_while_another_shows_booking_holds_the_studio() {
    let p = plane(true).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "the-morning-kettle", "The Morning Kettle").await;
    let now = now_ms();
    let (code, b) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("steeped", now, now + H),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{b}");

    let (code, refused) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/open",
        Some(ADMIN),
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    let words = refused["error"].as_str().unwrap();
    assert!(words.contains("booked for Steeped"), "{words}");
    // The booked show opens, and opening it again is still 200.
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/open",
        Some(ADMIN),
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/open",
        Some(ADMIN),
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/end",
        Some(ADMIN),
        json!({}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);

    // A booking under way keeps its start, now in the past, when only its end is changed.
    let under_way = format!("/api/v1/rehearsal/bookings/{}", b["id"].as_str().unwrap());
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let (code, longer) = send(
        &p.app,
        "PATCH",
        &under_way,
        Some(ADMIN),
        json!({"end_ms": now + 2 * H}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{longer}");
    assert_eq!(longer["start_ms"], b["start_ms"]);
    assert_eq!(longer["end_ms"].as_i64(), Some(now + 2 * H));
    // Moving its start to an earlier time is still refused.
    let (code, v) = send(
        &p.app,
        "PATCH",
        &under_way,
        Some(ADMIN),
        json!({"start_ms": now - H}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["field"], "start_ms");

    // With the booking gone, the studio is first come.
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/rehearsal/bookings/{}", b["id"].as_str().unwrap()),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/open",
        Some(ADMIN),
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_week_has_the_studios_where_each_host_comes_from_and_the_rehearsals() {
    let p = plane(true).await;
    make_show(&p.app, "steeped", "Steeped").await;
    make_show(&p.app, "the-morning-kettle", "The Morning Kettle").await;
    episode(&p.app, "steeped", 3_600, 60).await;
    episode(&p.app, "the-morning-kettle", 3 * 3_600, 60).await;
    // The Morning Kettle's host sends from their own studio.
    let (code, made) = send(
        &p.app,
        "POST",
        "/api/v1/shows/the-morning-kettle/keys",
        Some(ADMIN),
        json!({"label": "Kirsty Wilson", "hint": "srt"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    // A booking ahead, and a session open now.
    let now = now_ms();
    let (code, booking) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("the-morning-kettle", now + 2 * H, now + 3 * H),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{booking}");
    let (code, opened) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/open",
        Some(ADMIN),
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{opened}");

    let week = format!(
        "/api/v1/schedule/episodes?from_ms={}&to_ms={}",
        now - H,
        now + 6 * H
    );
    let (code, w) = send(&p.app, "GET", &week, Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{w}");
    assert_eq!(
        w["studios"],
        json!([{"id": "studio", "name": "Studio"}, {"id": "rehearsal", "name": "Rehearsal"}])
    );
    let from_of = |slug: &str| {
        w["episodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["show"]["slug"] == slug)
            .map(|e| (e["studio"].clone(), e["from"].clone()))
            .unwrap()
    };
    assert_eq!(from_of("steeped"), (json!("studio"), json!("room")));
    assert_eq!(
        from_of("the-morning-kettle"),
        (json!("studio"), json!("own"))
    );

    // An undrafted date names where its host comes from as an episode does (1.3.1).
    for d in w["dates"].as_array().unwrap() {
        let slug = d["show"].as_str().unwrap();
        let expected = if slug == "the-morning-kettle" {
            "own"
        } else {
            "room"
        };
        assert_eq!(d["from"], expected, "{slug}: {d}");
    }

    // The rota says the same for the same airings.
    let (code, rota) = send(&p.app, "GET", "/api/v1/rota", Some(ADMIN), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{rota}");
    for slug in ["steeped", "the-morning-kettle"] {
        let row = rota["airings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["show"] == slug)
            .unwrap_or_else(|| panic!("{slug} in the rota: {rota}"));
        let rota_from = if row["from"] == "push" { "own" } else { "room" };
        assert_eq!(from_of(slug).1, json!(rota_from), "{slug}");
    }

    // The booking, then the open session (running until now).
    let r = w["rehearsals"].as_array().unwrap();
    assert_eq!(r.len(), 2, "{w}");
    assert_eq!(
        (r[0]["kind"].clone(), r[0]["id"].clone()),
        (json!("booking"), booking["id"].clone())
    );
    assert_eq!(r[1]["kind"], "session");
    assert_eq!(r[1]["show"], "steeped");
    assert_eq!(r[1]["open"], true);
    assert_eq!(r[1]["id"], opened["session"]["id"]);
    assert!(r[1]["end_ms"].as_i64().unwrap() >= r[1]["start_ms"].as_i64().unwrap());

    // The public calendar file never lists a rehearsal.
    let (code, ics) = send(
        &p.app,
        "GET",
        "/api/v1/public/schedule.ics",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let ics = ics.as_str().unwrap().to_lowercase();
    assert!(ics.contains("begin:vcalendar"), "{ics}");
    assert!(!ics.contains("rehears"), "{ics}");
    assert!(!ics.contains(booking["id"].as_str().unwrap()), "{ics}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn without_a_rehearsal_studio_the_week_has_one_column_and_nothing_is_booked() {
    let p = plane(false).await;
    make_show(&p.app, "steeped", "Steeped").await;
    let now = now_ms();
    let (code, w) = send(
        &p.app,
        "GET",
        &format!(
            "/api/v1/schedule/episodes?from_ms={}&to_ms={}",
            now - H,
            now + H
        ),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{w}");
    assert_eq!(w["studios"], json!([{"id": "studio", "name": "Studio"}]));
    assert_eq!(w["rehearsals"], json!([]));
    let (code, v) = send(
        &p.app,
        "POST",
        "/api/v1/rehearsal/bookings",
        Some(ADMIN),
        book("steeped", now + H, now + 2 * H),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    let (code, list) = send(
        &p.app,
        "GET",
        &format!(
            "/api/v1/rehearsal/bookings?from_ms={}&to_ms={}",
            now,
            now + H
        ),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list, json!({"enabled": false, "bookings": []}));
}
