// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The cart wall against a real PostgreSQL and the real media worker (this crate's binary, as
//! `media-worker`): banks, an upload through the worker into a slot, the demo pack's Opus,
//! FILE MISSING when a file goes, and the slot emptied. Ignored unless asked for:
//! `WEAVESIGNAL_TEST_DATABASE_URL=postgres://... cargo test -p weavesignal-control -- --ignored`
//! (`just test-db`).

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::CartsConfig;
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "carts-db-test";

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn req(method: &str, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {ADMIN}"))
        .header("content-type", "application/json")
        .body(body)
        .unwrap()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_upload_becomes_a_cart_through_the_media_worker() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("carts-{}", uuid::Uuid::new_v4()));
    let config = CartsConfig {
        media_dir: dir.clone(),
        worker: Some(env!("CARGO_BIN_EXE_weavesignal-control").into()),
        ..CartsConfig::default()
    };
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(config, Some((pool.clone(), station.id))),
    );
    hub.start_carts().await;
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        hub: hub.clone(),
        join: weavesignal_control::config::JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));

    // A slot in a bank nobody made is refused before the worker runs.
    let wav = weavesignal_media::wav_sine(44_100, 2, 2.0, 440.0, 0.5);
    let (code, body) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/1?label=Tone&mode=one_shot",
            Body::from(wav.clone()),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (code, cat) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1",
            Body::from(r#"{"name": "The Morning Kettle", "colour_slot": 3}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cat}");
    assert_eq!(cat["banks"][0]["colour_slot"], 3);

    // A WAV: measured, its gain stored, written as FLAC under a UUID name.
    let (code, cart) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/1?label=Tone&mode=one_shot&detail=440%20Hz",
            Body::from(wav),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{cart}");
    assert_eq!(cart["duration_ms"], 2000);
    // A 440 Hz sine at half scale in stereo is about -6.7 LUFS; the gain brings it to -23.
    let lufs = cart["loudness_lufs"].as_f64().unwrap();
    let gain = cart["gain_db"].as_f64().unwrap();
    assert!((lufs - -6.7).abs() < 0.3, "{cart}");
    assert!((lufs + gain - -23.0).abs() < 0.05, "{cart}");
    let file = cart["file"].as_str().unwrap().to_owned();
    assert!(weavesignal_model::carts::is_cart_file(&file));
    let flac = std::fs::read(dir.join(&file)).unwrap();
    assert_eq!(&flac[..4], b"fLaC");

    // The demo pack's Ogg Opus goes through the same worker.
    let opus = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/carts-demo/rimshot-1.opus"
    ))
    .unwrap();
    let (code, rim) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/2?label=Rimshot&mode=one_shot&licence=CC%20BY%203.0",
            Body::from(opus),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{rim}");
    // Cut to about -16 LUFS (CREDITS.md); a sound under a second measures a little lower.
    assert!(
        (rim["loudness_lufs"].as_f64().unwrap() - -16.0).abs() < 2.5,
        "{rim}"
    );

    // Rubbish is refused by the worker, in words, and nothing is written.
    let before = std::fs::read_dir(&dir).unwrap().count();
    let (code, body) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/3?label=Junk&mode=one_shot",
            Body::from(&b"<html>not audio</html>"[..]),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("not an audio file")
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), before);

    // A file that goes is FILE MISSING once the wall looks at the disk again.
    std::fs::remove_file(dir.join(&file)).unwrap();
    hub.check_cart_files();
    let (_, cat) = send(&app, req("GET", "/api/v1/carts", Body::empty())).await;
    let missing: Vec<(i64, bool)> = cat["carts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["slot"].as_i64().unwrap(), c["missing"].as_bool().unwrap()))
        .collect();
    assert_eq!(missing, vec![(1, true), (2, false)]);
    let cmd = serde_json::json!({"op": "fire", "cart": cart["id"]});
    let (code, body) = send(
        &app,
        req(
            "POST",
            "/api/v1/carts/commands",
            Body::from(cmd.to_string()),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().starts_with("FILE MISSING"));

    // Emptying a slot takes its file with it.
    let rim_file = rim["file"].as_str().unwrap().to_owned();
    let (code, cat) = send(
        &app,
        req("DELETE", "/api/v1/carts/banks/1/slots/2", Body::empty()),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cat}");
    assert!(!dir.join(rim_file).exists());
    assert_eq!(cat["carts"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
