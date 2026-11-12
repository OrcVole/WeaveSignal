// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The consent words per show (ADR-0214, ADR-0215) through the control plane's routes against
//! a real PostgreSQL: a show on air by the schedule puts its presenter's contact first and the
//! station's as the host; a caller's consent record keeps the show and the contact exactly as
//! shown, and a later change to the show does not rewrite it; a show's presenter fields are
//! kept when a client leaves them out; and who may tie a staff account to a show (ADR-0218).
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
use weavesignal_model::{Station, StationId, ensure_station};
use weavesignal_proto::Keys;

const ADMIN: &str = "consent-db-test";
const STATION_CONTACT: &str = "info@kettle.example";

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
    pool: sqlx::PgPool,
    station: Station,
    staff: Arc<weavesignal_control::staff::Staff>,
}

async fn plane() -> Plane {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
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
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("consent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = router(Arc::new(AppState {
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
            consent_contact: STATION_CONTACT.into(),
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
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: dir.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
    })));
    Plane {
        app,
        pool,
        station,
        staff,
    }
}

/// A signed-in member of staff: their session token.
async fn staff_member(
    p: &Plane,
    username: &str,
    name: &str,
    role: weavesignal_model::staff::Role,
) -> (uuid::Uuid, String) {
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
    // The control plane reads sessions at start: this one is made after it.
    p.staff.restore(p.station.id).await;
    (account.id, token)
}

/// The words the player page shows now, and their version.
async fn words(app: &Router) -> (String, String) {
    let (code, body) = send(app, None, "GET", "/api/v1/player", Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    (
        body["call_in"]["consent_text"].as_str().unwrap().to_owned(),
        body["call_in"]["consent_version"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}

/// An episode of `slug` on air now by the schedule: series 1, episode 1, aired a minute ago
/// for an hour.
async fn on_air_now(app: &Router, slug: &str) {
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        &format!("/api/v1/shows/{slug}/series/1"),
        json!({}),
    )
    .await;
    assert!(code.is_success(), "{code} {body}");
    let (code, body) = send(
        app,
        Some(ADMIN),
        "POST",
        &format!("/api/v1/shows/{slug}/series/1/episodes"),
        json!({"count": 1}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let started = (jiff::Timestamp::now() - 60.seconds())
        .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap())
        .to_string();
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PATCH",
        &format!("/api/v1/shows/{slug}/series/1/episodes/1"),
        json!({"air_at": started, "minutes": 60}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_words_name_the_show_on_air_and_the_record_keeps_what_was_shown() {
    let p = plane().await;
    let app = &p.app;

    // No show on air: the station's contact alone (ADR-0215).
    let (text, _) = words(app).await;
    assert!(
        text.ends_with(&format!("contact {STATION_CONTACT}.")),
        "{text}"
    );
    assert!(!text.contains("hosted by"), "{text}");

    // A show with a presenter and a contact of its own, on air now by the schedule.
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/night-show",
        json!({
            "name": "The Night Show", "colour_slot": 4, "layout": "poster",
            "presenter": "Shona Campbell", "contact": "shona@example.org",
            "links": ["https://rumble.com/c/shona", " https://odysee.com/@shona "],
            "commercial_music": true
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let show_id = body["show"]["id"].as_str().unwrap().to_owned();
    assert_eq!(body["show"]["presenter"], "Shona Campbell");
    assert_eq!(body["show"]["contact"], "shona@example.org");
    assert_eq!(
        body["show"]["links"],
        json!(["https://rumble.com/c/shona", "https://odysee.com/@shona"])
    );
    assert_eq!(body["show"]["commercial_music"], true);
    on_air_now(app, "night-show").await;

    let (text, version) = words(app).await;
    let expected_contact =
        format!("shona@example.org. This station is hosted by Kettle FM: {STATION_CONTACT}");
    assert!(
        text.ends_with(&format!(
            "To have it taken down, contact {expected_contact}."
        )),
        "{text}"
    );
    assert!(text.starts_with("You'll be live on Kettle FM, "), "{text}");

    // A caller's consent against these words (given by the station, through the token route).
    let (code, body) = send(
        app,
        Some(ADMIN),
        "POST",
        "/api/v1/room/tokens",
        json!({
            "identity": "caller-dougal", "role": "caller", "name": "Dougal Thomson",
            "consent": {"accepted": true, "version": version}
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let (stored_show, presenter, contact, stored_words): (
        Option<uuid::Uuid>,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT show_id, presenter, contact, words FROM caller_consent
             WHERE station_id = $1 AND identity = $2",
    )
    .bind(p.station.id.as_uuid())
    .bind("caller-dougal")
    .fetch_one(&p.pool)
    .await
    .unwrap();
    assert_eq!(stored_show.map(|u| u.to_string()), Some(show_id.clone()));
    assert_eq!(presenter, "Shona Campbell");
    assert_eq!(contact, expected_contact);
    assert_eq!(stored_words, text);

    // The show's contact changes: the words change with it, the record does not. The body
    // leaves `presenter` out, which keeps it (an older client never wipes it).
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/night-show",
        json!({"name": "The Night Show", "colour_slot": 4, "layout": "poster", "contact": ""}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        body["show"]["presenter"], "Shona Campbell",
        "kept when left out"
    );
    assert_eq!(body["show"]["contact"], "", "cleared when sent empty");
    assert_eq!(body["show"]["commercial_music"], true, "kept when left out");
    let (text2, version2) = words(app).await;
    assert!(
        text2.ends_with(&format!("contact {STATION_CONTACT}.")),
        "{text2}"
    );
    assert_ne!(version2, version);
    let (contact_after,): (String,) = sqlx::query_as(
        "SELECT contact FROM caller_consent WHERE station_id = $1 AND identity = $2",
    )
    .bind(p.station.id.as_uuid())
    .bind("caller-dougal")
    .fetch_one(&p.pool)
    .await
    .unwrap();
    assert_eq!(contact_after, expected_contact, "what was agreed to stays");

    // A consent against the old words is refused: the page must read them again.
    let (code, body) = send(
        app,
        Some(ADMIN),
        "POST",
        "/api/v1/room/tokens",
        json!({
            "identity": "caller-isla", "role": "caller", "name": "Isla Murray",
            "consent": {"accepted": true, "version": version}
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["field"], "consent");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_presenter_may_name_only_themselves_as_a_shows_presenter() {
    let p = plane().await;
    let app = &p.app;
    let (shona, shona_token) = staff_member(
        &p,
        "shona",
        "Shona Campbell",
        weavesignal_model::staff::Role::Presenter,
    )
    .await;
    let (catriona, catriona_token) = staff_member(
        &p,
        "catriona",
        "Catriona MacKenzie-Buchanan",
        weavesignal_model::staff::Role::Presenter,
    )
    .await;

    // Themselves: allowed, and the wizard's fields land (ADR-0218, ADR-0219).
    let (code, body) = send(
        app,
        Some(&shona_token),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({
            "name": "Shona's Show", "colour_slot": 2, "layout": "poster",
            "presenter": "Shona Campbell", "contact": "shona@example.org",
            "slot": {"weekday": 5, "start": 19 * 60},
            "presenter_account": shona
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["presenter_account"], shona.to_string());
    assert_eq!(
        body["show"]["slot"],
        json!({"weekday": 5, "start": 1140, "minutes": null})
    );

    // Someone else: refused.
    let (code, body) = send(
        app,
        Some(&shona_token),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster", "presenter_account": catriona}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");

    // Left out: kept. Sent as null: cleared.
    let (code, body) = send(
        app,
        Some(&shona_token),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["presenter_account"], shona.to_string());
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster", "presenter_account": null}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["presenter_account"], Value::Null);

    // An admin (here the station key) names anyone.
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster", "presenter_account": catriona}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["presenter_account"], catriona.to_string());

    // The presenter's name and the contact the consent words name (ADR-0241): now Catriona's
    // show, so Shona may no longer change them, though a save that sends them unchanged passes.
    let show = |contact: &str| json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster", "contact": contact});
    let path = "/api/v1/shows/shonas-show";
    let (code, body) = send(
        app,
        Some(&shona_token),
        "PUT",
        path,
        show("shona@example.net"),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    let (code, body) = send(
        app,
        Some(&shona_token),
        "PUT",
        path,
        show("shona@example.org"),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, body) = send(
        app,
        Some(&catriona_token),
        "PUT",
        path,
        show("catriona@example.org"),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["show"]["contact"], "catriona@example.org");

    // Bad fields are refused with the field named: a link with a space, too many links.
    let (code, body) = send(
        app,
        Some(ADMIN),
        "PUT",
        "/api/v1/shows/shonas-show",
        json!({"name": "Shona's Show", "colour_slot": 2, "layout": "poster", "links": ["not a link"]}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"].as_str().unwrap().contains("link"), "{body}");
}
