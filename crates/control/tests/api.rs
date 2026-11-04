// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The public API's gate without a database (ADR-0195): what a scoped token reaches and what it
//! does not, the open routes' CORS and limits, the now-playing stream, and the events the hub's
//! and playout's changes make. The tokens are put straight into the live set; making them
//! through the routes is in `tokens_db.rs`.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use weavesignal_control::api::{Api, ApiConfig, layer};
use weavesignal_control::apikey::ApiKey;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig};
use weavesignal_control::desk::{DeskClient, DeskStatus};
use weavesignal_control::events;
use weavesignal_control::hub::Hub;
use weavesignal_control::onair::{Links, NowPlaying, OnAir};
use weavesignal_control::playout::PlayoutClient;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_control::zone::StationZone;
use weavesignal_model::tokens::{ApiToken, StoredToken};
use weavesignal_model::{Station, StationId};

const KEY: &str = "api-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";

struct Plane {
    app: Router,
    staff: Arc<Staff>,
    hub: Arc<Hub>,
    station: Station,
}

fn config() -> ApiConfig {
    ApiConfig {
        public_per_minute: 0,
        token_per_minute: 0,
        streams_per_address: 0,
        ..ApiConfig::default()
    }
}

fn plane(config: ApiConfig, gated: bool) -> Plane {
    let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Api::new(
        None,
        station.id,
        Some(ApiKey::parse(TOKEN_KEY).unwrap()),
        config,
        true,
    );
    let staff = Arc::new(Staff::new(None).with_api(Arc::new(api)));
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: None,
        room: None,
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        hub: hub.clone(),
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
        db: None,
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        publishing: Default::default(),
        zone: Arc::new(StationZone::new("Europe/London")),
    })));
    let app = if gated {
        layer(app, staff.clone())
    } else {
        app
    };
    Plane {
        app,
        staff,
        hub,
        station,
    }
}

/// A live token with these scopes; its secret.
fn token(p: &Plane, scopes: &[&str], expires_ms: Option<i64>) -> (Uuid, String) {
    let secret = format!("rsa_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let id = Uuid::new_v4();
    p.staff.api.add(StoredToken {
        token: ApiToken {
            id,
            name: "Stream deck".into(),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            prefix: secret[..8].to_owned(),
            created_by: "morag".into(),
            created_ms: 0,
            expires_ms,
            last_used_ms: None,
            revoked_ms: None,
        },
        secret_hash: ApiKey::parse(TOKEN_KEY).unwrap().token_hash(&secret),
    });
    (id, secret)
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    extra: &[(&str, &str)],
    body: Value,
) -> Response<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        req = req.header("authorization", format!("Bearer {b}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    app.clone().oneshot(req.body(body).unwrap()).await.unwrap()
}

/// A body the route accepts, so the answer comes from the credential check and not from the
/// body being refused first.
fn body_for(method: &str, uri: &str) -> Value {
    match (method, uri) {
        ("POST", "/api/v1/carts/commands") => json!({"op": "fade_all"}),
        ("POST", "/api/v1/desk/commands") => json!({"op": "on_air", "identity": "x", "on": true}),
        ("POST", "/api/v1/screener/commands") => json!({"op": "screen", "identity": "x"}),
        ("PUT", "/api/v1/carts/banks/1") => json!({"name": "News", "colour_slot": 1}),
        ("POST", "/api/v1/staff") => {
            json!({"username": "x", "name": "X", "role": "admin", "password": "a long enough phrase"})
        }
        ("POST", "/api/v1/tokens") => json!({"name": "x", "scopes": ["carts:read"]}),
        ("POST", "/api/v1/webhooks") => {
            json!({"name": "x", "url": "https://hooks.example/x", "events": ["on_air.changed"]})
        }
        ("POST", "/api/v1/room/tokens") => json!({"identity": "x", "role": "presenter"}),
        ("POST", "/api/v1/invites") => json!({"role": "guest"}),
        ("PUT", "/api/v1/shows/kettle") => json!({"name": "Kettle", "colour_slot": 1}),
        ("PUT", "/api/v1/playout/settings") => {
            json!({"time_zone": "UTC", "artist_separation_min": 60,
            "title_separation_min": 180, "album_separation_min": 60, "mode": "live", "default_clock": null})
        }
        ("POST", "/api/v1/retention/consents/1/erase") => json!({"by": "x"}),
        _ => Value::Null,
    }
}

async fn status(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
) -> (StatusCode, Value) {
    let res = call(app, method, uri, bearer, &[], body_for(method, uri)).await;
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// Past the credential check: not refused as unauthorised or forbidden.
fn passed(code: StatusCode) -> bool {
    code != StatusCode::UNAUTHORIZED && code != StatusCode::FORBIDDEN
}

#[tokio::test]
async fn a_token_passes_only_the_routes_whose_scope_it_holds() {
    let p = plane(config(), true);
    let (_, secret) = token(&p, &["carts:read", "desk:control"], None);

    // What it holds.
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&secret)).await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = status(&p.app, "POST", "/api/v1/desk/commands", Some(&secret)).await;
    assert!(passed(code), "{code}");

    // The same resource, another kind of use: the refusal names the scope.
    let (code, v) = status(&p.app, "POST", "/api/v1/carts/commands", Some(&secret)).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("carts:fire"), "{v}");
    // Another resource, in the shows' router as well as the studio's.
    for (m, u) in [
        ("GET", "/api/v1/shows"),
        ("PUT", "/api/v1/shows/kettle"),
        ("GET", "/api/v1/schedule/episodes?from_ms=1&to_ms=2"),
        ("GET", "/api/v1/studio"),
        ("GET", "/api/v1/schedule/week"),
        ("POST", "/api/v1/screener/commands"),
        ("PUT", "/api/v1/carts/banks/1"),
    ] {
        let (code, _) = status(&p.app, m, u, Some(&secret)).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{m} {u}");
    }
}

#[tokio::test]
async fn the_shows_routes_take_tokens_by_their_scopes_too() {
    let p = plane(config(), true);
    let (_, read) = token(&p, &["shows:read", "schedule:read"], None);
    let (_, write) = token(&p, &["shows:write"], None);
    // Past the credential check: the shows need the database, which this plane has not.
    for (m, u) in [
        ("GET", "/api/v1/shows"),
        ("GET", "/api/v1/schedule/episodes?from_ms=1&to_ms=2"),
    ] {
        let (code, _) = status(&p.app, m, u, Some(&read)).await;
        assert!(passed(code), "{m} {u} {code}");
    }
    let (code, v) = status(&p.app, "PUT", "/api/v1/shows/kettle", Some(&read)).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("shows:write"), "{v}");
    let (code, _) = status(&p.app, "GET", "/api/v1/shows", Some(&write)).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "write does not imply read");
    // People are personal data: an admin's.
    for t in [&read, &write] {
        let (code, _) = status(&p.app, "GET", "/api/v1/people", Some(t)).await;
        assert_eq!(code, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn a_token_never_reaches_an_admin_route_whatever_its_scopes() {
    let p = plane(config(), true);
    // Every scope there is.
    let all: Vec<&str> = weavesignal_control::scopes::SCOPES
        .iter()
        .map(|(n, _)| *n)
        .collect();
    let (_, secret) = token(&p, &all, None);
    for (m, u) in [
        ("GET", "/api/v1/staff"),
        ("POST", "/api/v1/staff"),
        ("GET", "/api/v1/tokens"),
        ("POST", "/api/v1/tokens"),
        ("GET", "/api/v1/webhooks"),
        ("POST", "/api/v1/webhooks"),
        ("GET", "/api/v1/retention"),
        ("GET", "/api/v1/retention/holds"),
        ("POST", "/api/v1/retention/consents/1/erase"),
        ("GET", "/api/v1/people"),
        ("GET", "/api/v1/playout/settings"),
        ("PUT", "/api/v1/playout/settings"),
        ("GET", "/api/v1/sessions/me"),
        ("POST", "/api/v1/room/tokens"),
        ("POST", "/api/v1/invites"),
    ] {
        let (code, _) = status(&p.app, m, u, Some(&secret)).await;
        // Refused by the credential check, except where no route answers to the method.
        assert!(
            code == StatusCode::FORBIDDEN
                || code == StatusCode::METHOD_NOT_ALLOWED
                || (code == StatusCode::UNPROCESSABLE_ENTITY && body_for(m, u).is_null()),
            "{m} {u} gave {code}"
        );
    }
    // Making a token is an admin's: not even with `tokens` in the body.
    let (code, v) = status(&p.app, "POST", "/api/v1/tokens", Some(&secret)).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{v}");
}

#[tokio::test]
async fn unknown_revoked_and_expired_tokens_are_not_tokens() {
    let p = plane(config(), true);
    let (id, secret) = token(&p, &["carts:read"], None);
    let (_, expired) = token(&p, &["carts:read"], Some(1));
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&secret)).await;
    assert_eq!(code, StatusCode::OK);
    for bad in [
        "rsa_00000000000000000000000000000000".to_owned(),
        format!("{secret}x"),
        expired,
    ] {
        let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&bad)).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{bad}");
    }
    p.staff.api.remove(id);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&secret)).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED, "revoked at once");
}

#[tokio::test]
async fn without_the_gate_no_token_is_accepted() {
    let p = plane(config(), false);
    let (_, secret) = token(&p, &["carts:read"], None);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&secret)).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED, "the check fails closed");
}

#[tokio::test]
async fn a_station_without_a_key_has_no_tokens() {
    let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let staff = Arc::new(Staff::new(None));
    let app = layer(
        router(Arc::new(AppState {
            station,
            db: None,
            room: None,
            admin_token: Some(KEY.into()),
            staff: staff.clone(),
            hub,
            join: JoinConfig::default(),
            video: Default::default(),
            joins: Default::default(),
            turn_tls_port: 5349,
            steering: Default::default(),
        })),
        staff,
    );
    let (code, v) = status(&app, "GET", "/api/v1/carts", Some("rsa_whatever")).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("WEAVESIGNAL_CONTROL_TOKEN_KEY"),
        "{v}"
    );
}

#[tokio::test]
async fn the_station_key_and_the_other_bearers_are_as_they_were() {
    let p = plane(config(), true);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(KEY)).await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", None).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some("rss_notasession")).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, _) = status(&p.app, "GET", "/api/v1/retention", Some(KEY)).await;
    assert_ne!(code, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_token_is_limited_per_minute() {
    let p = plane(
        ApiConfig {
            token_per_minute: 3,
            ..config()
        },
        true,
    );
    let (_, one) = token(&p, &["carts:read"], None);
    let (_, two) = token(&p, &["carts:read"], None);
    for _ in 0..3 {
        let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&one)).await;
        assert_eq!(code, StatusCode::OK);
    }
    let res = call(&p.app, "GET", "/api/v1/carts", Some(&one), &[], Value::Null).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res.headers().get("retry-after").is_some());
    // Another token, and the station key, are not held back by it.
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(&two)).await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(KEY)).await;
    assert_eq!(code, StatusCode::OK);
}

fn from(addr: &str) -> [(&'static str, &str); 1] {
    [("x-forwarded-for", addr)]
}

#[tokio::test]
async fn the_open_routes_are_limited_per_address_by_the_last_forwarded_entry() {
    let p = plane(
        ApiConfig {
            public_per_minute: 2,
            ..config()
        },
        true,
    );
    let get = |a: &'static str| {
        let app = p.app.clone();
        async move {
            call(
                &app,
                "GET",
                "/api/v1/public/station",
                None,
                &from(a),
                Value::Null,
            )
            .await
            .status()
        }
    };
    // A client forging the first entry does not get a new allowance each time.
    assert_ne!(
        get("1.1.1.1, 203.0.113.9").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_ne!(
        get("2.2.2.2, 203.0.113.9").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    let res = call(
        &p.app,
        "GET",
        "/api/v1/public/station",
        None,
        &from("3.3.3.3, 203.0.113.9"),
        Value::Null,
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        res.headers().get("access-control-allow-origin").unwrap(),
        "*",
        "a browser can read the refusal"
    );
    // Another address has its own.
    assert_ne!(get("203.0.113.10").await, StatusCode::TOO_MANY_REQUESTS);
    // Routes that are not open are not counted against the address.
    for _ in 0..5 {
        let (code, _) = status(&p.app, "GET", "/api/v1/carts", Some(KEY)).await;
        assert_eq!(code, StatusCode::OK);
    }
}

fn header(res: &Response<Body>, name: &str) -> Option<String> {
    res.headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_owned())
}

#[tokio::test]
async fn only_the_open_routes_allow_any_origin() {
    let p = plane(config(), true);
    let origin = [("origin", "https://kettle.example")];
    for uri in [
        "/api/v1/now-playing",
        "/api/v1/now-playing/events",
        "/api/v1/public/station",
        "/api/v1/public/schedule",
        "/api/v1/public/episodes",
    ] {
        let res = call(&p.app, "GET", uri, None, &origin, Value::Null).await;
        assert_eq!(
            header(&res, "access-control-allow-origin").as_deref(),
            Some("*"),
            "{uri}"
        );
        assert_eq!(
            header(&res, "access-control-allow-credentials"),
            None,
            "{uri}"
        );
        // A browser's preflight is answered, and offers reading only.
        let pre = call(
            &p.app,
            "OPTIONS",
            uri,
            None,
            &[
                ("origin", "https://kettle.example"),
                ("access-control-request-method", "GET"),
            ],
            Value::Null,
        )
        .await;
        assert_eq!(pre.status(), StatusCode::NO_CONTENT, "{uri}");
        assert_eq!(
            header(&pre, "access-control-allow-origin").as_deref(),
            Some("*")
        );
        assert_eq!(
            header(&pre, "access-control-allow-methods").as_deref(),
            Some("GET, HEAD, OPTIONS")
        );
    }
    for uri in [
        "/api/v1/carts",
        "/api/v1/studio",
        "/api/v1/player",
        "/api/v1/tokens",
        "/api/v1/staff",
        "/health",
    ] {
        let res = call(&p.app, "GET", uri, Some(KEY), &origin, Value::Null).await;
        assert_eq!(header(&res, "access-control-allow-origin"), None, "{uri}");
        let pre = call(&p.app, "OPTIONS", uri, None, &origin, Value::Null).await;
        assert_eq!(header(&pre, "access-control-allow-origin"), None, "{uri}");
        assert_ne!(pre.status(), StatusCode::NO_CONTENT, "{uri}");
    }
    // A write on an open path is not offered to other origins.
    let res = call(
        &p.app,
        "POST",
        "/api/v1/public/station",
        None,
        &origin,
        json!({}),
    )
    .await;
    assert_eq!(header(&res, "access-control-allow-origin"), None);
}

#[tokio::test]
async fn open_answers_follow_the_listing_setting_and_the_rest_of_the_api_never_is() {
    for listed in [false, true] {
        let p = plane(config(), true);
        let app = p.app.layer(axum::middleware::from_fn_with_state(
            listed,
            weavesignal_control::web::robots,
        ));
        for uri in ["/api/v1/public/station", "/api/v1/now-playing"] {
            let res = call(&app, "GET", uri, None, &[], Value::Null).await;
            assert_eq!(
                header(&res, "x-robots-tag").as_deref(),
                (!listed).then_some("noindex, nofollow"),
                "{uri} listed={listed}"
            );
        }
        let res = call(&app, "GET", "/api/v1/player", None, &[], Value::Null).await;
        assert_eq!(
            header(&res, "x-robots-tag").as_deref(),
            Some("noindex, nofollow"),
            "/api/v1/player listed={listed}"
        );
    }
}

#[tokio::test]
async fn the_station_facts_are_the_players_public_ones_and_nobody_else() {
    let p = plane(config(), true);
    let (code, v) = status(&p.app, "GET", "/api/v1/public/station", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["station"], "Kettle FM");
    assert_eq!(v["live"], false);
    assert_eq!(v["call_in_open"], false);
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    for k in ["presenter", "consent_text", "relay", "callers", "chat"] {
        assert!(!keys.contains(&k), "{k} leaked into {keys:?}");
    }
}

fn announce(p: &Plane, on_air: &str, title: &str) -> Arc<OnAir> {
    let onair = p
        .hub
        .onair
        .get()
        .cloned()
        .unwrap_or_else(|| panic!("no playout layer"));
    onair.announce(NowPlaying {
        on_air: on_air.into(),
        now: Some(weavesignal_control::onair::Current {
            kind: "track".into(),
            title: title.into(),
            artist: "Fraser Macleod".into(),
            album: String::new(),
            started_at_ms: 0,
        }),
        next: None,
        up_next_shown: false,
    });
    onair
}

fn with_onair(p: &Plane) {
    // A playout layer that is not running: nothing polls, and `announce` drives it.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap();
    let onair = OnAir::new(
        p.station.id,
        pool,
        Links {
            playout: PlayoutClient::new(None),
            listeners: PlayoutClient::new(None),
            status_url: String::new(),
            library_dir: "/nonexistent".into(),
            logger_dir: "/nonexistent".into(),
            listener_delay: Duration::ZERO,
            library_lufs: -16.0,
            library_max_seconds: 600.0,
        },
        &p.hub,
    );
    p.hub.onair.set(onair).ok().expect("set once");
}

/// The next chunk of a streaming body, as text, within a few seconds.
async fn next_chunk(body: &mut Body) -> String {
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("a chunk in time")
        .expect("the stream is open")
        .expect("no error");
    String::from_utf8_lossy(frame.into_data().unwrap().as_ref()).into_owned()
}

#[tokio::test]
async fn the_now_playing_stream_is_open_and_delivers_a_change() {
    let p = plane(config(), true);
    with_onair(&p);
    let res = call(
        &p.app,
        "GET",
        "/api/v1/now-playing/events",
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        header(&res, "content-type").as_deref(),
        Some("text/event-stream")
    );
    assert_eq!(
        header(&res, "access-control-allow-origin").as_deref(),
        Some("*")
    );
    let mut body = res.into_body();
    // The state as it is now first...
    let first = next_chunk(&mut body).await;
    assert!(
        first.contains("event: now_playing") && first.contains("\"unknown\""),
        "{first}"
    );
    // ...then each change.
    announce(&p, "automation", "The Kettle Song");
    let change = next_chunk(&mut body).await;
    assert!(
        change.contains("\"on_air\":\"automation\"") && change.contains("The Kettle Song"),
        "{change}"
    );
}

#[tokio::test]
async fn one_address_holds_a_few_streams_open_and_no_more() {
    let p = plane(
        ApiConfig {
            streams_per_address: 2,
            ..config()
        },
        true,
    );
    with_onair(&p);
    let open = |a: &'static str| {
        let app = p.app.clone();
        async move {
            call(
                &app,
                "GET",
                "/api/v1/now-playing/events",
                None,
                &from(a),
                Value::Null,
            )
            .await
        }
    };
    let one = open("198.51.100.7").await;
    let two = open("198.51.100.7").await;
    assert_eq!(
        (one.status(), two.status()),
        (StatusCode::OK, StatusCode::OK)
    );
    assert_eq!(
        open("198.51.100.7").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(open("198.51.100.8").await.status(), StatusCode::OK);
    // A stream that ends gives its place back.
    drop(one);
    assert_eq!(open("198.51.100.7").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn changes_at_the_station_become_events() {
    let p = plane(config(), true);
    with_onair(&p);
    let mut rx = p.staff.api.events.subscribe();
    events::spawn_sources(&p.hub, &p.staff, None, p.station.id);
    tokio::task::yield_now().await;

    // Playout's now-playing.
    announce(&p, "desk", "Live from the studio");
    let mut seen = Vec::new();
    for _ in 0..2 {
        let e = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        seen.push((e.kind, e.data.clone()));
    }
    let kinds: Vec<_> = seen.iter().map(|(k, _)| *k).collect();
    assert!(
        kinds.contains(&"now_playing.changed") && kinds.contains(&"on_air.changed"),
        "{kinds:?}"
    );
    let on_air = seen.iter().find(|(k, _)| *k == "on_air.changed").unwrap();
    assert_eq!(on_air.1["on_air"], "desk");
    assert_eq!(on_air.1["previous"], "unknown");
    assert_eq!(on_air.1["live"], true);
    // The same change again is no change.
    announce(&p, "desk", "Live from the studio");
    // A caller joining the queue.
    let desk: DeskStatus = serde_json::from_value(json!({ "people": [
        {"identity": "presenter-iain", "role": "presenter", "on_air": true, "talk": false, "present": true, "linked": true},
        {"identity": "caller-7", "role": "caller", "on_air": false, "talk": false, "present": true, "linked": true},
    ]}))
    .unwrap();
    p.hub.with_studio(|s, now| s.sync(&desk, now));
    let e = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(e.kind, "caller.queued");
    assert_eq!(e.data["identity"], "caller-7");
    assert_eq!(e.data["ordinal"], 1);
    assert!(
        e.data.get("name").is_none()
            && e.data.get("topic").is_none()
            && e.data.get("consent").is_none(),
        "{}",
        e.data
    );
    // Seeing them again is not another arrival.
    p.hub.with_studio(|s, now| s.sync(&desk, now + 1000));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), rx.recv())
            .await
            .is_err()
    );
}
