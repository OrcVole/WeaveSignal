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
use weavesignal_control::carts::{CartsLink, station_config};
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
    // A 440 Hz sine at half scale in stereo is about -6.7 LUFS; the gain brings it to the target
    // the settings leave to the station, -16 for a station made since 1.3.3.
    let lufs = cart["loudness_lufs"].as_f64().unwrap();
    let gain = cart["gain_db"].as_f64().unwrap();
    assert!((lufs - -6.7).abs() < 0.3, "{cart}");
    assert!((lufs + gain - -16.0).abs() < 0.05, "{cart}");
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

    // A cart is changed where it is and keeps its sound (1.2.1): a new name, a second line, a
    // mode and a ramp; the same id, the same file, the same measured gain.
    let (code, tone) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/1",
            Body::from(
                r#"{"label": "Concert A", "detail": "", "mode": "bed", "ramp_ms": 500, "licence": "CC0 1.0"}"#,
            ),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{tone}");
    assert_eq!(tone["id"], cart["id"]);
    assert_eq!(tone["file"], cart["file"]);
    assert_eq!(tone["gain_db"], cart["gain_db"]);
    assert_eq!(
        (
            &tone["label"],
            &tone["detail"],
            &tone["mode"],
            &tone["ramp_ms"],
            &tone["licence"]
        ),
        (
            &serde_json::json!("Concert A"),
            &serde_json::json!(""),
            &serde_json::json!("bed"),
            &serde_json::json!(500),
            &serde_json::json!("CC0 1.0")
        )
    );
    // A move never lands on another cart (409, naming it), and nothing has moved.
    let (code, body) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/1",
            Body::from(r#"{"slot": 2}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("Rimshot"),
        "{body}"
    );
    // A bank nobody made is 404; an empty slot is 404.
    let (code, body) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/1",
            Body::from(r#"{"bank": 4}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");
    let (code, _) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/7",
            Body::from(r#"{"label": "Nobody"}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // To an empty slot it goes, its key with it, and comes back; the ramp is taken away.
    let (code, moved) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/1",
            Body::from(r#"{"slot": 24, "ramp_ms": null}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{moved}");
    assert_eq!(
        (
            &moved["id"],
            &moved["slot"],
            &moved["ramp_ms"],
            &moved["label"]
        ),
        (
            &cart["id"],
            &serde_json::json!(24),
            &serde_json::Value::Null,
            &serde_json::json!("Concert A")
        )
    );
    let (_, cat) = send(&app, req("GET", "/api/v1/carts", Body::empty())).await;
    let slots: Vec<i64> = cat["carts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["slot"].as_i64().unwrap())
        .collect();
    assert_eq!(slots, vec![2, 24]);
    assert!(dir.join(&file).exists(), "the move kept its file");
    let (code, _) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/24",
            Body::from(r#"{"slot": 1, "mode": "one_shot"}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // A replaced sound is a new file under the same slot: the old file goes.
    let (code, again) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/1?label=Concert%20A&mode=one_shot",
            Body::from(weavesignal_media::wav_sine(44_100, 2, 1.0, 440.0, 0.25)),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{again}");
    assert_eq!(again["duration_ms"], 1000);
    assert!(!dir.join(&file).exists(), "the sound it replaced is gone");
    let file = again["file"].as_str().unwrap().to_owned();
    let cart = again;

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
    // A show's cart is changed by its people, as its banks are (1.2.1): Eilidh may not rename
    // Woof; Iona may, from her preparing window, and may move it to the producer's bank 2. A
    // move stays within the show's own banks: the station's bank 1 is not a place for it.
    let woof_slot = "/api/v1/shows/steeped/carts/banks/1/slots/1";
    let (code, _) = send(
        &app,
        as_(
            "eilidh",
            "PATCH",
            woof_slot,
            Body::from(r#"{"label": "Hers now"}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, woof2) = send(
        &app,
        as_(
            "iona",
            "PATCH",
            woof_slot,
            Body::from(r#"{"label": "Woof, twice", "bank": 2, "slot": 6}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{woof2}");
    assert_eq!(
        (&woof2["id"], &woof2["show"], &woof2["bank"], &woof2["slot"]),
        (
            &woof["id"],
            &serde_json::json!("steeped"),
            &serde_json::json!(2),
            &serde_json::json!(6)
        )
    );
    // The station's routes do not reach a show's cart: the station's bank 2 was never made,
    // and its bank 1 slot 1 is empty.
    let (code, _) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/2/slots/6",
            Body::from(r#"{"label": "Not the station's"}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = send(
        &app,
        as_(
            "iona",
            "PATCH",
            "/api/v1/shows/steeped/carts/banks/2/slots/6",
            Body::from(r#"{"label": "Woof", "bank": 1, "slot": 1}"#),
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

/// Deleting a show (ADR-0279) takes its own banks and carts with it: their files go from the
/// disk, and the open wall is read again, so no window is left listing a bank that is gone. The
/// station's banks and another show's stay.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn deleting_a_show_takes_its_cart_banks_files_and_the_walls_listing() {
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
        .join(format!("carts-del-{}", uuid::Uuid::new_v4()));
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
    for slug in ["steeped", "morning-kettle"] {
        let input: weavesignal_model::shows::ShowInput =
            serde_json::from_value(serde_json::json!({"name": slug, "colour_slot": 2})).unwrap();
        weavesignal_model::shows::put_show(&pool, station.id, slug, &input)
            .await
            .unwrap();
    }
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: staff.clone(),
        hub: hub.clone(),
        join: weavesignal_control::config::JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }))
    .merge(weavesignal_control::shows::router(Arc::new(
        weavesignal_control::shows::ShowsState {
            station: station.clone(),
            db: Some(pool.clone()),
            admin_token: Some(ADMIN.into()),
            staff,
            publishing: weavesignal_control::config::PublishingConfig {
                public_url: "https://kettle.example".into(),
                place: "Dunfermline".into(),
                cards_dir: dir.join("cards"),
                episodes_dir: dir.join("episodes"),
                ..Default::default()
            },
            zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
            live: None,
            hub: Some(hub.clone()),
            video: None,
        },
    )));
    let bank =
        |name: &str| Body::from(serde_json::json!({"name": name, "colour_slot": 4}).to_string());
    let wav = weavesignal_media::wav_sine(44_100, 2, 1.0, 330.0, 0.5);
    let mut files = Vec::new();
    for (slug, label) in [("steeped", "Woof"), ("morning-kettle", "Miaow")] {
        let (code, _) = send(
            &app,
            req(
                "PUT",
                &format!("/api/v1/shows/{slug}/carts/banks/1"),
                bank(label),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        let (code, cart) = send(
            &app,
            req(
                "PUT",
                &format!("/api/v1/shows/{slug}/carts/banks/1/slots/1?label={label}&mode=one_shot"),
                Body::from(wav.clone()),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{cart}");
        files.push(cart["file"].as_str().unwrap().to_owned());
    }
    let wall = |show: &str| serde_json::to_value(hub.catalogue(Some(show))).unwrap();
    let banks_of = |v: &serde_json::Value| -> Vec<String> {
        v["banks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(banks_of(&wall("steeped")), vec!["Woof"]);
    assert!(dir.join(&files[0]).exists());

    let (code, gone) = send(&app, req("DELETE", "/api/v1/shows/steeped", Body::empty())).await;
    assert_eq!(code, StatusCode::OK, "{gone}");
    assert_eq!(gone["cart_banks"], 1);
    assert_eq!(gone["carts"], 1);
    // The cart's file is gone, and the wall in memory no longer lists the bank.
    assert!(!dir.join(&files[0]).exists());
    assert!(banks_of(&wall("steeped")).is_empty(), "{}", wall("steeped"));
    // The other show's bank, its cart and its file are as they were.
    assert_eq!(banks_of(&wall("morning-kettle")), vec!["Miaow"]);
    assert!(dir.join(&files[1]).exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// Loading the default pack again (1.2.1): a station that loaded the pack of 1.0.0 to 1.2.0 at
/// its first start gets a newer pack's sounds on asking. Into a bank that holds carts it asks
/// first, then replaces only the default carts nobody changed; a cart a person renamed or added
/// stays, and an old default the pack no longer has a sound for goes. An empty bank loads
/// directly. The packs here are made for the test, the old one under three of the old pack's
/// own ids and names.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_default_pack_is_loaded_again_and_keeps_what_is_a_persons() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("pack-{}", uuid::Uuid::new_v4()));
    let (dir, pack) = (root.join("carts"), root.join("pack"));
    std::fs::create_dir_all(&pack).unwrap();
    let sound = |name: &str, hz: f64| {
        std::fs::write(
            pack.join(name),
            weavesignal_media::wav_sine(44_100, 1, 0.5, hz, 0.4),
        )
        .unwrap()
    };
    let entry = |id: &str, label: &str, bank: u8, slot: u8, category: &str, looping: bool| {
        // The old pack's three carts carry the licences it gave them: a credit is part of what
        // makes a cart still the pack's own.
        let licence = match id {
            "applause-2" => "Public domain",
            "laugh-1" => "CC BY 3.0",
            _ => "CC0 1.0",
        };
        serde_json::json!({"id": id, "label": label, "file": format!("{id}.wav"), "category": category,
            "suggested": {"bank": bank, "slot": slot}, "loop": looping, "licence": licence})
    };
    // The old pack, as a 1.2.0 station loaded it.
    for id in ["applause-1", "applause-2", "laugh-1"] {
        sound(&format!("{id}.wav"), 440.0);
    }
    let old = serde_json::json!({"banks": {"1": "Reactions and comedy"}, "carts": [
        entry("applause-1", "Applause after a concert", 1, 1, "applause", false),
        entry("applause-2", "Applause, short", 1, 2, "applause", false),
        entry("laugh-1", "Audience laugh, short", 1, 6, "laugh", false),
    ]});
    std::fs::write(pack.join("carts.json"), old.to_string()).unwrap();
    let config = CartsConfig {
        media_dir: dir.clone(),
        worker: Some(env!("CARGO_BIN_EXE_weavesignal-control").into()),
        demo_pack: Some(pack.clone()),
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
    // A new station loads the pack by itself.
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
    let wall = |app: axum::Router, n: usize, hub: Arc<Hub>| async move {
        for _ in 0..400 {
            let (_, cat) = send(&app, req("GET", "/api/v1/carts", Body::empty())).await;
            if hub.carts_view().importing.is_none() && cat["carts"].as_array().unwrap().len() == n {
                return cat;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the wall never held {n} carts");
    };
    let at = |cat: &serde_json::Value, bank: i64, slot: i64| -> serde_json::Value {
        cat["carts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["bank"] == bank && c["slot"] == slot)
            .cloned()
            .unwrap_or_default()
    };
    let cat = wall(app.clone(), 3, hub.clone()).await;
    let (old_1, old_2, old_6) = (at(&cat, 1, 1), at(&cat, 1, 2), at(&cat, 1, 6));
    assert_eq!(old_1["origin"], "demo:applause-1");

    // The station's people make it theirs: one default renamed, one sound of their own added.
    let (code, renamed) = send(
        &app,
        req(
            "PATCH",
            "/api/v1/carts/banks/1/slots/2",
            Body::from(r#"{"label": "Our applause"}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{renamed}");
    let (code, own) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/7?label=Station%20ident&mode=one_shot&origin=ident.wav",
            Body::from(weavesignal_media::wav_sine(44_100, 1, 0.5, 880.0, 0.4)),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{own}");

    // A newer release's pack takes the old one's place on the disk.
    for id in ["clap", "swish", "rain", "ident"] {
        sound(&format!("{id}.wav"), 660.0);
    }
    let new = serde_json::json!({"banks": {"1": "Reactions and comedy", "2": "Idents"}, "carts": [
        entry("clap", "Clap", 1, 1, "applause", false),
        entry("swish", "Swish", 1, 2, "whoosh", false),
        entry("rain", "Rain (loop)", 1, 3, "weather", true),
        entry("ident", "Ident", 2, 1, "jingle", false),
    ]});
    std::fs::write(pack.join("carts.json"), new.to_string()).unwrap();

    // The pack as the wall's dialog reads it: two banks, and the two defaults nobody changed.
    let (code, view) = send(&app, req("GET", "/api/v1/carts/pack", Body::empty())).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(view["available"], true);
    assert_eq!(view["banks"].as_array().unwrap().len(), 2);
    assert_eq!(view["banks"][0]["carts"][1]["mode"], "sweeper");
    let mut untouched: Vec<String> = view["untouched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    untouched.sort();
    let mut expected = vec![
        old_1["id"].as_str().unwrap().to_owned(),
        old_6["id"].as_str().unwrap().to_owned(),
    ];
    expected.sort();
    assert_eq!(untouched, expected);

    // Into a bank that holds carts it asks first, and nothing has changed.
    let load = |replace: bool| {
        Body::from(
            serde_json::json!({"loads": [{"bank": 1, "into": 1}, {"bank": 2, "into": 5}], "replace": replace})
                .to_string(),
        )
    };
    let (code, body) = send(&app, req("POST", "/api/v1/carts/pack", load(false))).await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "bank 1 holds 4 carts: 2 default carts nobody changed would be replaced or removed, and 2 carts of yours would stay; send `replace: true` to go on"
    );
    let (_, cat) = send(&app, req("GET", "/api/v1/carts", Body::empty())).await;
    assert_eq!(at(&cat, 1, 1)["id"], old_1["id"]);
    // A pack bank that is not there, a station bank off the wall, one named twice: 422.
    for bad in [
        r#"{"loads": [{"bank": 9, "into": 1}], "replace": true}"#,
        r#"{"loads": [{"bank": 1, "into": 6}], "replace": true}"#,
        r#"{"loads": [{"bank": 1, "into": 3}, {"bank": 2, "into": 3}], "replace": true}"#,
        r#"{"loads": [], "replace": true}"#,
    ] {
        let (code, body) = send(&app, req("POST", "/api/v1/carts/pack", Body::from(bad))).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
    }

    // Asked to go on, it says what it does and does it: bank 1 where the defaults were, the
    // pack's bank 2 into the station's empty bank 5.
    let (code, started) = send(&app, req("POST", "/api/v1/carts/pack", load(true))).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["loading"], 3);
    assert_eq!(
        started["plans"][0],
        serde_json::json!({"bank": 1, "name": "Reactions and comedy", "into": 1, "made": true,
            "load": 2, "replace": 1, "remove": 1, "keep": [2, 7], "left_out": 1})
    );
    assert_eq!(
        started["plans"][1],
        serde_json::json!({"bank": 2, "name": "Idents", "into": 5, "made": false,
            "load": 1, "replace": 0, "remove": 0, "keep": [], "left_out": 0})
    );
    // Clap, Our applause, Rain, Station ident, and Ident in bank 5.
    let cat = wall(app.clone(), 5, hub.clone()).await;
    let clap = at(&cat, 1, 1);
    assert_eq!(
        (&clap["label"], &clap["origin"]),
        (&serde_json::json!("Clap"), &serde_json::json!("demo:clap"))
    );
    assert_ne!(clap["id"], old_1["id"]);
    // What is a person's is as it was: the same carts, the same sounds.
    let kept = at(&cat, 1, 2);
    assert_eq!(
        (&kept["id"], &kept["label"], &kept["file"]),
        (
            &old_2["id"],
            &serde_json::json!("Our applause"),
            &old_2["file"]
        )
    );
    assert_eq!(at(&cat, 1, 7)["id"], own["id"]);
    assert_eq!(at(&cat, 1, 3)["mode"], "bed");
    // The old default the pack has nothing for is gone, with its file and the replaced one's.
    assert_eq!(at(&cat, 1, 6), serde_json::Value::Null);
    for gone in [&old_1, &old_6] {
        assert!(!dir.join(gone["file"].as_str().unwrap()).exists());
    }
    for stays in [&old_2, &own, &clap] {
        assert!(dir.join(stays["file"].as_str().unwrap()).exists());
    }
    // Bank 5 was made under the pack's name for it.
    let five = cat["banks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["number"] == 5)
        .cloned()
        .unwrap();
    assert_eq!(
        (&five["name"], &five["colour_slot"]),
        (&serde_json::json!("Idents"), &serde_json::json!(5))
    );
    assert_eq!(at(&cat, 5, 1)["label"], "Ident");
    // Loaded again, the new defaults are the ones nobody changed: nothing of a person's moves.
    let (code, again) = send(&app, req("POST", "/api/v1/carts/pack", load(true))).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["plans"][0]["keep"], serde_json::json!([2, 7]));
    assert_eq!(again["plans"][0]["remove"], 0);
    let _ = wall(app.clone(), 5, hub.clone()).await;
    let _ = std::fs::remove_dir_all(root);
}

/// A station's cart wall on its own database: a bank, a tone through the media worker, and what
/// the wall says its target is. Returns (the catalogue's `loudness_lufs`, the tone's measured
/// loudness plus its stored gain).
async fn level_on(pool: &sqlx::PgPool, station: &Station, config: CartsConfig) -> (f64, f64) {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("carts-level-{}", uuid::Uuid::new_v4()));
    let config = CartsConfig {
        media_dir: dir.clone(),
        worker: Some(env!("CARGO_BIN_EXE_weavesignal-control").into()),
        ..config
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
        db: Some(pool.clone()),
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
    let (code, cat) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1",
            Body::from(r#"{"name": "Level", "colour_slot": 3}"#),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{cat}");
    let wav = weavesignal_media::wav_sine(44_100, 2, 2.0, 440.0, 0.5);
    let (code, cart) = send(
        &app,
        req(
            "PUT",
            "/api/v1/carts/banks/1/slots/1?label=Tone&mode=one_shot",
            Body::from(wav),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{cart}");
    let _ = std::fs::remove_dir_all(&dir);
    (
        cat["loudness_lufs"].as_f64().unwrap(),
        cart["loudness_lufs"].as_f64().unwrap() + cart["gain_db"].as_f64().unwrap(),
    )
}

/// 1.3.3 brings a new installation's carts to -16 LUFS, and keeps every station from before at
/// -23 (or at whatever its settings say): migration 0073 marks the stations it finds, and a
/// station's settings that set a target win either way. Proved on two databases of their own:
/// one migrated as 1.3.2 left it (migrations 0001 to 0072, a station in it) and then started by
/// this release, and one this release makes from nothing.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn new_stations_carts_are_minus_16_and_an_upgraded_station_keeps_minus_23() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let admin = sqlx::PgPool::connect(&url).await.expect("connect");
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let (upgraded_db, fresh_db) = (
        format!("carts_upgraded_{}", &tag[..12]),
        format!("carts_fresh_{}", &tag[..12]),
    );
    for db in [&upgraded_db, &fresh_db] {
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db}")))
            .execute(&admin)
            .await
            .unwrap();
    }
    let at = |db: &str| format!("{}/{db}", url.rsplit_once('/').unwrap().0);
    let settings = CartsConfig::default();
    assert_eq!(settings.loudness_lufs, None, "the settings leave it out");

    // 1.3.2's database: its migrations, 0001 to 0072 (the last 1.3.2 shipped), and its station
    // as 1.3.2's start wrote it.
    let src = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../model/migrations"));
    let old = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("migrations-1.3.2-{tag}"));
    std::fs::create_dir_all(&old).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        if name
            .get(..4)
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| n <= 72)
        {
            std::fs::copy(src.join(&name), old.join(&name)).unwrap();
        }
    }
    assert!(old.join("0072_keyed_interval.sql").is_file());
    assert!(!old.join("0073_carts_loudness_default.sql").exists());
    let pool = sqlx::PgPool::connect(&at(&upgraded_db)).await.unwrap();
    sqlx::migrate::Migrator::new(old.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let id = StationId::new_random();
    sqlx::query("INSERT INTO station (id, name) VALUES ($1, $2)")
        .bind(id.as_uuid())
        .bind("Kettle FM")
        .execute(&pool)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&old);

    // Upgraded: this release starts on it, twice. It stays at -23, and so do its uploads.
    for _ in 0..2 {
        let station = ensure_station(&pool, &Station::new(id, "Kettle FM").unwrap())
            .await
            .unwrap();
        let cfg = station_config(settings.clone(), &pool, station.id)
            .await
            .unwrap();
        assert_eq!(cfg.target(), -23.0, "an upgraded station keeps -23");
        let (said, played) = level_on(&pool, &station, cfg).await;
        assert_eq!(said, -23.0);
        assert!((played - -23.0).abs() < 0.05, "{played}");
    }
    let station = Station::new(id, "Kettle FM").unwrap();
    // Its settings (or CARTS_LOUDNESS_LUFS) move it to -16.
    let set = CartsConfig {
        loudness_lufs: Some(-16.0),
        ..settings.clone()
    };
    let cfg = station_config(set, &pool, station.id).await.unwrap();
    let (said, played) = level_on(&pool, &station, cfg).await;
    assert_eq!(said, -16.0);
    assert!((played - -16.0).abs() < 0.05, "{played}");

    // A new installation: an empty database, its station made at its first start; -16 then
    // and at every start after.
    let fresh = sqlx::PgPool::connect(&at(&fresh_db)).await.unwrap();
    let new_id = StationId::new_random();
    for _ in 0..2 {
        let station = ensure_station(&fresh, &Station::new(new_id, "Ochil Radio").unwrap())
            .await
            .unwrap();
        let cfg = station_config(settings.clone(), &fresh, station.id)
            .await
            .unwrap();
        assert_eq!(cfg.target(), -16.0, "a new station's carts are at -16");
        let (said, played) = level_on(&fresh, &station, cfg).await;
        assert_eq!(said, -16.0);
        assert!((played - -16.0).abs() < 0.05, "{played}");
    }
    // Its settings can keep it at -23.
    let set = CartsConfig {
        loudness_lufs: Some(-23.0),
        ..settings.clone()
    };
    let cfg = station_config(set, &fresh, new_id).await.unwrap();
    assert_eq!(cfg.target(), -23.0);

    pool.close().await;
    fresh.close().await;
    for db in [&upgraded_db, &fresh_db] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {db} WITH (FORCE)"
        )))
        .execute(&admin)
        .await
        .unwrap();
    }
}
