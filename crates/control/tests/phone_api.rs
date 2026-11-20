// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line's routes (17f) against the router, with no database and no desk.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::phone::config::PhoneConfig;
use weavesignal_control::phone::line::{Context, Line};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::{CallIn, Consent, ConsentBy, PhoneCaller, Studio};
use weavesignal_model::caller_id::{BanKeys, livekit_identity};
use weavesignal_model::{Station, StationId};

const KEY: &str = "phone-test-station-key";

fn plane(with_line: bool) -> (Router, Arc<Hub>) {
    let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    if with_line {
        let line = Line::new(
            PhoneConfig {
                enabled: true,
                numbers: vec!["+441314960000".into()],
                allowed_addresses: vec!["127.0.0.1/32".into()],
                public_ip: "127.0.0.1".into(),
                country_code: "44".into(),
                // The shipped voices from the repository; no copy in a words folder.
                defaults_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/phone"),
                words_dir: std::env::temp_dir().join("weavesignal-phone-api-no-words"),
                ..PhoneConfig::default()
            },
            Some(BanKeys::new([4; 32])),
        );
        line.set_context(Context {
            call_in_open: true,
            consent_today: String::new(),
            station_recording: None,
            default_present: true,
            ..Context::default()
        });
        assert!(hub.phone.set(Arc::new(line)).is_ok());
    }
    let app = router(Arc::new(AppState {
        station,
        db: None,
        room: None,
        admin_token: Some(KEY.into()),
        staff: Arc::new(Staff::new(None)),
        hub: hub.clone(),
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));
    (app, hub)
}

async fn call(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {KEY}"));
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn phone_caller(hub: &Hub, id: &str, from: &str, withheld: bool) {
    let keys = BanKeys::new([4; 32]);
    hub.with_studio(|s, _| {
        s.phone_call(
            CallIn {
                identity: id.into(),
                name: String::new(),
                topic: String::new(),
                consent: Consent {
                    at_ms: 0,
                    version: "0123456789abcdef".into(),
                    by: ConsentBy::Caller,
                    text: "words".into(),
                },
            },
            PhoneCaller {
                tail: weavesignal_model::caller_id::livekit_tail(from).into(),
                withheld,
                ban_key: (!withheld).then(|| keys.of_identity(&livekit_identity(from))),
            },
        )
    });
}

#[tokio::test]
async fn the_line_off_says_so_and_needs_a_bearer() {
    let (app, _) = plane(false);
    let (s, v) = call(&app, "GET", "/api/v1/phone", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["open"], false);
    assert_eq!(v["closed"], "off");
    assert!(
        v["consent_words_help"]
            .as_str()
            .unwrap()
            .contains("An internet-only station may turn it off if")
    );
    let res = app
        .clone()
        .oneshot(Request::get("/api/v1/phone").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let (s, _) = call(
        &app,
        "POST",
        "/api/v1/phone/takedown",
        Some(json!({"number": "07700 900123"})),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_line_on_shows_its_default_words() {
    let (app, _) = plane(true);
    let (s, v) = call(&app, "GET", "/api/v1/phone", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["open"], true);
    assert_eq!(v["lines"], 8);
    assert_eq!(v["words"]["kind"], "default");
    assert!(
        v["words"]["text"]
            .as_str()
            .unwrap()
            .contains("Press any key")
    );
}

#[tokio::test]
async fn bans_by_caller_and_by_number_keep_no_number() {
    let (app, hub) = plane(true);
    phone_caller(&hub, "phone-Xk3pQ9mZr2Ta", "+447700900123", false);
    phone_caller(&hub, "phone-Wk3pQ9mZr2Tw", "anonymous", true);
    let (s, v) = call(
        &app,
        "POST",
        "/api/v1/phone/bans",
        Some(json!({"identity": "phone-Xk3pQ9mZr2Ta", "reason": "abusive"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v[0]["tail"], "0123");
    let key = v[0]["ban_key"].as_str().unwrap().to_owned();
    // Withheld: refused, in the screener's words.
    let (s, v) = call(
        &app,
        "POST",
        "/api/v1/phone/bans",
        Some(json!({"identity": "phone-Wk3pQ9mZr2Tw"})),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"], "Cannot ban a withheld number");
    // Not a phone caller.
    let (s, _) = call(
        &app,
        "POST",
        "/api/v1/phone/bans",
        Some(json!({"identity": "caller-1"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // By number (the station key is an admin): every form; the caller's own key among them.
    let (s, v) = call(
        &app,
        "POST",
        "/api/v1/phone/bans",
        Some(json!({"number": "07700 900123", "reason": "", "days": 30})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let made: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["ban_key"].as_str().unwrap())
        .collect();
    assert!(made.contains(&key.as_str()));
    let (_, list) = call(&app, "GET", "/api/v1/phone/bans", None).await;
    let text = list.to_string();
    assert!(!text.contains("7700900123"), "{text}");
    // Lift it; twice is 404.
    let (s, _) = call(&app, "DELETE", &format!("/api/v1/phone/bans/{key}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(&app, "DELETE", &format!("/api/v1/phone/bans/{key}"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Neither or both: refused.
    let (s, _) = call(
        &app,
        "POST",
        "/api/v1/phone/bans",
        Some(json!({"reason": "x"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    // The takedown search needs the database.
    let (s, _) = call(
        &app,
        "POST",
        "/api/v1/phone/takedown",
        Some(json!({"number": "07700 900123"})),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_words_to_record_name_the_stations_contact_and_need_a_database() {
    let (app, _) = plane(true);
    let (_, v) = call(&app, "GET", "/api/v1/phone", None).await;
    let to_record = v["words_to_record"].as_str().unwrap();
    assert!(
        to_record.ends_with("Press any key to carry on."),
        "{to_record}"
    );
    assert_eq!(v["recording"], Value::Null);
    let res = app
        .clone()
        .oneshot(
            Request::put("/api/v1/phone/words")
                .header("authorization", format!("Bearer {KEY}"))
                .body(Body::from(vec![0u8; 64]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let (s, _) = call(&app, "DELETE", "/api/v1/phone/words", None).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    let (off, _) = plane(false);
    let (s, _) = call(&off, "DELETE", "/api/v1/phone/words", None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_admin_turns_the_words_off_and_chooses_the_voice_from_the_next_call() {
    let (app, _) = plane(true);
    let (_, v) = call(&app, "GET", "/api/v1/phone", None).await;
    assert_eq!(v["consent_words"], true);
    assert_eq!(v["default_voice"], "alba");
    let (s, v) = call(
        &app,
        "PUT",
        "/api/v1/phone/consent",
        Some(json!({"default_voice": "edinburgh"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["default_voice"], "edinburgh");
    assert_eq!(v["consent_words"], true);
    assert_eq!(v["words"]["kind"], "default");
    // A field left out stays as it was.
    let (s, v) = call(
        &app,
        "PUT",
        "/api/v1/phone/consent",
        Some(json!({"consent_words": false})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["consent_words"], false);
    assert_eq!(v["default_voice"], "edinburgh");
    assert_eq!(v["words"]["kind"], "off");
    assert_eq!(v["open"], true);
    // `own` with no recording closes the line while the words are on.
    let (_, v) = call(
        &app,
        "PUT",
        "/api/v1/phone/consent",
        Some(json!({"consent_words": true, "default_voice": "own"})),
    )
    .await;
    assert_eq!(v["closed"], json!({"words": "no_recording"}));
    assert_eq!(v["open"], false);
    // Nothing to change, an unknown voice, an unknown field.
    for body in [
        json!({}),
        json!({"default_voice": "glasgow"}),
        json!({"consent_voice": "alba"}),
    ] {
        let (s, _) = call(&app, "PUT", "/api/v1/phone/consent", Some(body.clone())).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let (off, _) = plane(false);
    let (s, _) = call(
        &off,
        "PUT",
        "/api/v1/phone/consent",
        Some(json!({"consent_words": false})),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // An admin's: a bearer is needed.
    let res = app
        .clone()
        .oneshot(
            Request::put("/api/v1/phone/consent")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"consent_words":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_two_shipped_voices_can_be_heard() {
    let (app, _) = plane(true);
    for (file, shipped) in [
        ("alba.opus", "phone-words-alba.opus"),
        ("edinburgh.opus", "phone-words-edinburgh.opus"),
    ] {
        let res = app
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/phone/consent/default/{file}"))
                    .header("authorization", format!("Bearer {KEY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        assert_eq!(res.headers()["content-type"], "audio/ogg");
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let on_disk = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/phone")
                .join(shipped),
        )
        .unwrap();
        assert_eq!(bytes.as_ref(), on_disk.as_slice());
        assert!(bytes.starts_with(b"OggS"));
    }
    for bad in ["own.opus", "glasgow.opus", "..%2Fphone-words-alba.opus"] {
        let (s, _) = call(
            &app,
            "GET",
            &format!("/api/v1/phone/consent/default/{bad}"),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{bad}");
    }
    let res = app
        .clone()
        .oneshot(
            Request::get("/api/v1/phone/consent/default/alba.opus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let (off, _) = plane(false);
    let (s, _) = call(&off, "GET", "/api/v1/phone/consent/default/alba.opus", None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}
