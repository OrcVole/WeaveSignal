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

/// A cart wall per show (1.0.2): a show's own banks beside the station's, changed only by those
/// who may change its step (a preparing window among them), and never touching another show's
/// or the station's.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_shows_own_banks_are_kept_apart_and_changed_by_its_people() {
    use weavesignal_model::staff::{self, Role};
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
    let staff = Arc::new(weavesignal_control::staff::Staff::new(Some(pool.clone())));
    // Three signed-in members of staff: Iona presents Steeped, Eilidh presents The Morning
    // Kettle, Calum produces.
    let mut tokens = std::collections::HashMap::new();
    let mut ids = std::collections::HashMap::new();
    for (username, name, role) in [
        ("iona", "Iona MacIntosh", Role::Presenter),
        ("eilidh", "Eilidh Fraser-MacLeod", Role::Presenter),
        ("calum", "Calum Robertson", Role::Producer),
    ] {
        let account = staff::create_account(
            &pool,
            station.id,
            username,
            name,
            role,
            &staff::hash_password("a long enough phrase").unwrap(),
        )
        .await
        .unwrap();
        let token = staff::new_token();
        staff::create_session(
            &pool,
            station.id,
            account.id,
            &staff::token_hash(&token),
            weavesignal_control::hub::now_ms() + 3_600_000,
        )
        .await
        .unwrap();
        tokens.insert(username, token);
        ids.insert(username, account.id);
    }
    staff.restore(station.id).await;
    for (slug, name, presenter) in [
        ("steeped", "Steeped", "iona"),
        ("morning-kettle", "The Morning Kettle", "eilidh"),
    ] {
        let input: weavesignal_model::shows::ShowInput = serde_json::from_value(
            serde_json::json!({"name": name, "colour_slot": 2, "presenter_account": ids[presenter]}),
        )
        .unwrap();
        weavesignal_model::shows::put_show(&pool, station.id, slug, &input)
            .await
            .unwrap();
    }
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff,
        hub: hub.clone(),
        join: weavesignal_control::config::JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));
    let as_ = |who: &str, method: &str, uri: &str, body: Body| {
        let bearer = tokens.get(who).cloned().unwrap_or_else(|| ADMIN.to_owned());
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(body)
            .unwrap()
    };
    let bank =
        |name: &str| Body::from(serde_json::json!({"name": name, "colour_slot": 4}).to_string());

    // The station's bank 1, shared by every show.
    let (code, _) = send(
        &app,
        req("PUT", "/api/v1/carts/banks/1", bank("Station effects")),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // Iona prepares Steeped in a preparing window: her show's bank 1 is hers to make.
    let (code, me) = send(
        &app,
        as_(
            "iona",
            "PUT",
            "/api/v1/sessions/me/mode",
            Body::from(r#"{"preparing": true}"#),
        ),
    )
    .await;
    assert_eq!(
        (code, &me["preparing"]),
        (StatusCode::OK, &serde_json::json!(true))
    );
    let (code, cat) = send(
        &app,
        as_(
            "iona",
            "PUT",
            "/api/v1/shows/steeped/carts/banks/1",
            bank("Dog woofs"),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cat}");
    assert_eq!(cat["show"], "steeped");
    let banks: Vec<(serde_json::Value, String)> = cat["banks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["show"].clone(), b["name"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        banks,
        vec![
            (serde_json::Value::Null, "Station effects".to_owned()),
            (serde_json::json!("steeped"), "Dog woofs".to_owned()),
        ]
    );
    // An upload into her bank goes through the worker as the station's do.
    let wav = weavesignal_media::wav_sine(44_100, 2, 1.0, 330.0, 0.5);
    let (code, woof) = send(
        &app,
        as_(
            "iona",
            "PUT",
            "/api/v1/shows/steeped/carts/banks/1/slots/1?label=Woof&mode=one_shot",
            Body::from(wav.clone()),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{woof}");
    assert_eq!(woof["show"], "steeped");
    // Eilidh, another show's presenter, may not touch Steeped's banks; Calum, a producer, may.
    let (code, _) = send(
        &app,
        as_(
            "eilidh",
            "PUT",
            "/api/v1/shows/steeped/carts/banks/2",
            bank("Not hers"),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(
        &app,
        as_(
            "eilidh",
            "DELETE",
            "/api/v1/shows/steeped/carts/banks/1/slots/1",
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = send(
        &app,
        as_(
            "calum",
            "PUT",
            "/api/v1/shows/steeped/carts/banks/2",
            bank("Producer's"),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // Eilidh's own show's bank 1 is apart from Steeped's and the station's.
    let (code, _) = send(
        &app,
        as_(
            "eilidh",
            "PUT",
            "/api/v1/shows/morning-kettle/carts/banks/1",
            bank("Cat noises"),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, miaow) = send(
        &app,
        as_(
            "eilidh",
            "PUT",
            "/api/v1/shows/morning-kettle/carts/banks/1/slots/1?label=Miaow&mode=one_shot",
            Body::from(wav),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{miaow}");
    let labels = |cat: &serde_json::Value| -> Vec<String> {
        cat["carts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["label"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, steeped) = send(
        &app,
        req("GET", "/api/v1/carts?show=steeped", Body::empty()),
    )
    .await;
    assert_eq!(labels(&steeped), vec!["Woof"]);
    let (_, kettle) = send(
        &app,
        req("GET", "/api/v1/carts?show=morning-kettle", Body::empty()),
    )
    .await;
    assert_eq!(labels(&kettle), vec!["Miaow"]);
    let (_, station_only) = send(&app, req("GET", "/api/v1/carts", Body::empty())).await;
    assert_eq!(station_only["banks"].as_array().unwrap().len(), 1);
    assert!(labels(&station_only).is_empty());

    // Removing Steeped's bank 1 takes Woof and its file, and leaves Miaow.
    let woof_file = woof["file"].as_str().unwrap().to_owned();
    assert!(dir.join(&woof_file).exists());
    let (code, cat) = send(
        &app,
        as_(
            "iona",
            "DELETE",
            "/api/v1/shows/steeped/carts/banks/1",
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cat}");
    assert!(!dir.join(woof_file).exists());
    assert!(labels(&cat).is_empty());
    let (code, _) = send(
        &app,
        as_(
            "iona",
            "DELETE",
            "/api/v1/shows/steeped/carts/banks/1",
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (_, kettle) = send(
        &app,
        req("GET", "/api/v1/carts?show=morning-kettle", Body::empty()),
    )
    .await;
    assert_eq!(labels(&kettle), vec!["Miaow"]);
    // A show that does not exist is 404, and the station key may change any show's banks.
    let (code, _) = send(
        &app,
        req("PUT", "/api/v1/shows/no-such-show/carts/banks/1", bank("x")),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = send(
        &app,
        req(
            "PUT",
            "/api/v1/shows/steeped/carts/banks/3",
            bank("By the key"),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let _ = std::fs::remove_dir_all(dir);
}
