// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Retention's routes against a real PostgreSQL: the setting, holds, the sweep, and a waiting
//! caller's consent surviving a control-plane restart (M3's known fault). Ignored unless asked
//! for: `WEAVESIGNAL_TEST_DATABASE_URL=postgres://... cargo test -p weavesignal-control --test
//! retention_db -- --ignored` (`just test-db`).

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::{Extension, Router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use weavesignal_control::app::{AppState, RoomState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, RoomConfig};
use weavesignal_control::desk::{DeskClient, DeskError, DeskStatus};
use weavesignal_control::hub::Hub;
use weavesignal_control::retention::{RetentionLink, restore_consents};
use weavesignal_control::studio::Studio;
use weavesignal_model::{ConsentRecord, Station, StationId, ensure_station, record_consent};
use weavesignal_proto::{Keys, PassKind, Role};

const ADMIN: &str = "retention-db-test";

fn keys() -> Keys {
    Keys {
        api_key: "weavesignal".into(),
        api_secret: "check-only-check-only-check-only-0".into(),
    }
}

/// A control plane as `serve` builds it, with a fresh studio: a new one is a restart.
fn control_plane(
    pool: &sqlx::PgPool,
    station: &Station,
    dir: &std::path::Path,
) -> (Arc<AppState>, Router) {
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), Some((pool.clone(), station.id))),
    );
    let state = Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: Some(RoomState {
            config: RoomConfig::default(),
            keys: keys(),
        }),
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    });
    let link = Arc::new(RetentionLink {
        recordings_dir: dir.to_owned(),
        sweep_every: Duration::from_secs(3600),
    });
    let app = router(state.clone()).layer(Extension(link));
    (state, app)
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn admin(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {ADMIN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn with_pass(method: &str, uri: &str, pass: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {pass}"))
        .body(Body::empty())
        .unwrap()
}

async fn setup(name: &str) -> (sqlx::PgPool, Station, std::path::PathBuf) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(&pool, &Station::new(StationId::new_random(), name).unwrap())
        .await
        .unwrap();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("retention-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    (pool, station, dir)
}

fn now_ms() -> u64 {
    weavesignal_control::hub::now_ms()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_waiting_callers_consent_survives_a_restart() {
    let (pool, station, dir) = setup("Kettle FM").await;
    let identity = "caller-0a1b2c3d4e5f";
    // What the call-in route does: keep the consent, then sign the caller's pass.
    record_consent(
        &pool,
        station.id,
        &ConsentRecord {
            identity: identity.into(),
            name: "Jessie Brown".into(),
            topic: "No bus after 23:10".into(),
            given_by: "caller",
            words: "Your call may be broadcast live and recorded.".into(),
            words_version: "0123456789abcdef".into(),
            accepted_at_ms: now_ms(),
            show_id: None,
            presenter: String::new(),
            contact: String::new(),
        },
    )
    .await
    .unwrap();
    let (pass, _) = keys()
        .pass(
            PassKind::Call,
            Role::Caller,
            identity,
            "Jessie Brown",
            14_400,
        )
        .unwrap();

    // The control plane restarts: a fresh studio, nothing in memory.
    let (state, app) = control_plane(&pool, &station, &dir);
    assert!(state.hub.read_studio(|s| s.call_in_of(identity).is_none()));
    // Their link dropped meanwhile; the rejoin finds the consent in the database.
    let (code, body) = send(&app, with_pass("POST", "/api/v1/pass/token", &pass)).await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    assert_eq!(body["identity"], identity);
    let back = state
        .hub
        .read_studio(|s| s.call_in_of(identity).cloned())
        .unwrap();
    assert_eq!(back.name, "Jessie Brown");
    assert_eq!(back.consent.version, "0123456789abcdef");
    assert!(
        state
            .hub
            .read_studio(|s| s.may_reach_desk(identity, Role::Caller))
    );

    // Another restart reads it back at start, before any rejoin.
    let (state, _) = control_plane(&pool, &station, &dir);
    assert_eq!(restore_consents(&state).await, 1);
    assert!(state.hub.read_studio(|s| s.call_in_of(identity).is_some()));

    // They hang up: the call ends in the database too, so after the next restart the pass
    // gets no token, though nothing in memory remembers the hang-up.
    let (_, app) = control_plane(&pool, &station, &dir);
    let (code, _) = send(&app, with_pass("POST", "/api/v1/pass/leave", &pass)).await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (state, app) = control_plane(&pool, &station, &dir);
    assert_eq!(restore_consents(&state).await, 0);
    let (code, body) = send(&app, with_pass("POST", "/api/v1/pass/token", &pass)).await;
    assert_eq!(code, StatusCode::GONE, "{body}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_setting_holds_and_the_sweep_through_the_routes() {
    let (pool, station, dir) = setup("Kettle FM").await;
    let (_, app) = control_plane(&pool, &station, &dir);

    // A new station is on No retention, with the licence warning, and sees all fourteen.
    let (code, v) = send(&app, admin("GET", "/api/v1/retention", json!(null))).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["setting"]["preset"], "none");
    assert_eq!(v["presets"].as_array().unwrap().len(), 14);
    assert_eq!(v["warnings"][0]["level"], "serious");
    assert_eq!(v["rows"].as_array().unwrap().len(), 8);
    assert_eq!(v["sweep_every_s"], 3600);
    // Without the bearer, nothing.
    let anon = Request::builder()
        .uri("/api/v1/retention")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, anon).await.0, StatusCode::UNAUTHORIZED);

    // Canada, licensed: 28 days, its source, and the 56-day complaint rule.
    let (code, v) = send(
        &app,
        admin(
            "PUT",
            "/api/v1/retention",
            json!({"preset": "ca_licensed", "by": "Morag Ross"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["rows"][0]["period"], json!({"keep": "days", "days": 28}));
    assert!(
        v["rows"][0]["source"]
            .as_str()
            .unwrap()
            .contains("Radio Regulations")
    );
    assert_eq!(v["complaint_hold"]["days"], 56);
    assert_eq!(v["setting"]["updated_by"], "Morag Ross");
    assert!(v["warnings"].as_array().unwrap().is_empty());

    // Custom under the floor: saved, with a warning naming the rule.
    let mut periods = v["presets"][9]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["kind"].as_str().unwrap().to_owned(), r["period"].clone()))
        .collect::<serde_json::Map<_, _>>();
    periods.insert(
        "programme_recordings".into(),
        json!({"keep": "days", "days": 7}),
    );
    let custom = json!({"preset": "custom", "based_on": "ca_licensed", "periods": periods, "by": "Morag Ross"});
    let (code, v) = send(&app, admin("PUT", "/api/v1/retention", custom.clone())).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["setting"]["preset"], "custom");
    assert_eq!(v["warnings"][0]["kind"], "programme_recordings");
    assert!(
        v["warnings"][0]["source"]
            .as_str()
            .unwrap()
            .contains("8(5)(a)")
    );
    // A preset with periods, or a change with no name, is refused with a sentence.
    let (code, e) = send(
        &app,
        admin(
            "PUT",
            "/api/v1/retention",
            json!({"preset": "uk_ofcom", "periods": periods, "by": "x"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    let (code, _) = send(
        &app,
        admin(
            "PUT",
            "/api/v1/retention",
            json!({"preset": "uk_ofcom", "by": " "}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    // A recording registered as the logger would: 10 days old, kept under 28, gone under 7.
    std::fs::write(dir.join("hour.opus"), b"audio").unwrap();
    let now = now_ms();
    let (code, r) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/recordings",
            json!({"kind": "programme", "label": "14:00", "file": "hour.opus",
                   "started_at_ms": now - 10 * 86_400_000 - 3_600_000, "ended_at_ms": now - 10 * 86_400_000}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{r}");
    let id = r["id"].as_i64().unwrap();
    let (code, _) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/recordings",
            json!({"kind": "programme", "file": "../x", "started_at_ms": now}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    // The preview says what the 7 days would delete, and deletes nothing.
    let (code, p) = send(
        &app,
        admin("POST", "/api/v1/retention/preview", custom.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{p}");
    assert_eq!(
        (
            p["sweep"]["dry_run"].as_bool(),
            p["sweep"]["recordings_deleted"].as_u64()
        ),
        (Some(true), Some(1))
    );
    assert_eq!(p["warnings"][0]["kind"], "programme_recordings", "{p}");
    assert!(dir.join("hour.opus").exists());

    // A hold on it: the sweep keeps it.
    let (code, h) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/holds",
            json!({"reason": "Listener complaint about the 14:00 phone-in", "by": "Iain Gunn",
                   "records": [{"kind": "recording", "id": id}]}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{h}");
    let hold = h["id"].as_i64().unwrap();
    let (code, e) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/holds",
            json!({"reason": "x", "by": "y"}),
        ),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a hold names something: {e}"
    );
    let (code, s) = send(&app, admin("POST", "/api/v1/retention/sweep", json!(null))).await;
    assert_eq!(code, StatusCode::OK, "{s}");
    assert_eq!(
        (s["recordings_deleted"].as_u64(), s["held"].as_u64()),
        (Some(0), Some(1))
    );
    assert!(dir.join("hour.opus").exists());

    // Released, with who and why: the next sweep deletes it.
    let (code, h) = send(
        &app,
        admin(
            "POST",
            &format!("/api/v1/retention/holds/{hold}/release"),
            json!({"by": "Iain Gunn", "reason": "Complaint withdrawn"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{h}");
    assert_eq!(h["released_by"], "Iain Gunn");
    let (code, _) = send(
        &app,
        admin(
            "POST",
            &format!("/api/v1/retention/holds/{hold}/release"),
            json!({"by": "Iain Gunn", "reason": "again"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (_, s) = send(&app, admin("POST", "/api/v1/retention/sweep", json!(null))).await;
    assert_eq!(s["recordings_deleted"], 1, "{s}");
    assert!(!dir.join("hour.opus").exists());
    let (_, list) = send(&app, admin("GET", "/api/v1/retention/holds", json!(null))).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        list[0]["reason"],
        "Listener complaint about the 14:00 phone-in"
    );
    let (_, v) = send(&app, admin("GET", "/api/v1/retention", json!(null))).await;
    assert_eq!(v["last_sweep"]["recordings_deleted"], 1, "{v}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_open_hold_past_its_date_is_flagged_and_never_released() {
    use weavesignal_model::retention as model;
    let (pool, station, dir) = setup("Teacup FM").await;
    let id = station.id;
    let by = "Morag Ross";
    model::save_setting(
        &pool,
        id,
        &model::Setting {
            preset: "ca_licensed".into(),
            ..model::Setting::default()
        },
        by,
    )
    .await
    .unwrap();
    let now = now_ms();
    let day = 86_400_000u64;
    let place = |from: u64| model::NewHold {
        reason: "CRTC complaint".into(),
        by: by.into(),
        from_ms: Some(from),
        to_ms: Some(from + 3_600_000),
        records: vec![],
    };
    // Broadcast 100 days ago (56-day rule: late), 10 days ago (not yet), and a late one released.
    let late = model::place_hold(&pool, id, &place(now - 100 * day))
        .await
        .unwrap();
    let recent = model::place_hold(&pool, id, &place(now - 10 * day))
        .await
        .unwrap();
    let freed = model::place_hold(&pool, id, &place(now - 120 * day))
        .await
        .unwrap();
    model::release_hold(&pool, id, freed.id, by, "closed")
        .await
        .unwrap();

    let setting = model::load_setting(&pool, id).await.unwrap();
    let flagged = model::holds_past_date(&pool, id, &setting, now)
        .await
        .unwrap();
    assert_eq!(
        flagged.iter().map(|(h, _)| h.id).collect::<Vec<_>>(),
        vec![late.id]
    );
    assert_eq!(flagged[0].1, now - 100 * day + 56 * day);
    // A sweep does not release it, and the start-up check names it without changing it.
    let link = RetentionLink {
        recordings_dir: dir.clone(),
        sweep_every: Duration::from_secs(3600),
    };
    weavesignal_control::retention::sweep_once(&pool, id, &link)
        .await
        .unwrap();
    weavesignal_control::retention::warn_at_start(&pool, id).await;
    let after = model::hold(&pool, id, late.id).await.unwrap();
    assert!(after.released_at_ms.is_none());
    assert!(model::hold(&pool, id, recent.id).await.is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_released_holds_record_goes_the_moderation_period_after_its_release() {
    use weavesignal_model::retention as model;
    let (pool, station, dir) = setup("Saucer FM").await;
    let id = station.id;
    let by = "Morag Ross";
    // UK, Ofcom-licensed: moderation records 365 days.
    let setting = model::Setting {
        preset: "uk_ofcom".into(),
        ..model::Setting::default()
    };
    model::save_setting(&pool, id, &setting, by).await.unwrap();
    let now = now_ms();
    let place = |reason: &str| model::NewHold {
        reason: reason.into(),
        by: by.into(),
        from_ms: Some(now - 1_000_000_000),
        to_ms: Some(now - 999_000_000),
        records: vec![],
    };
    let old = model::place_hold(&pool, id, &place("old")).await.unwrap();
    let recent = model::place_hold(&pool, id, &place("recent"))
        .await
        .unwrap();
    let open = model::place_hold(&pool, id, &place("open")).await.unwrap();
    for (h, days) in [(&old, 400), (&recent, 300)] {
        model::release_hold(&pool, id, h.id, by, "closed")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE retention_hold SET released_at = now() - make_interval(days => $2) WHERE id = $1",
        )
        .bind(h.id)
        .bind(days)
        .execute(&pool)
        .await
        .unwrap();
    }

    // A preview says one would go and deletes nothing.
    let dry = model::sweep(&pool, id, &setting, Some(&dir), true)
        .await
        .unwrap();
    assert_eq!(dry.holds_deleted, 1);
    assert!(model::hold(&pool, id, old.id).await.is_ok());

    let report = model::sweep(&pool, id, &setting, Some(&dir), false)
        .await
        .unwrap();
    assert_eq!(report.holds_deleted, 1);
    assert!(matches!(
        model::hold(&pool, id, old.id).await,
        Err(model::HoldError::NotFound(_))
    ));
    assert!(model::hold(&pool, id, recent.id).await.is_ok());
    assert!(model::hold(&pool, id, open.id).await.is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

/// The desk's status with these callers, each `(identity, on_air)`, and a presenter on air.
fn desk_with(callers: &[(&str, bool)]) -> DeskStatus {
    let person = |identity: &str, role: &str, on_air: bool| {
        json!({"identity": identity, "role": role, "on_air": on_air, "talk": false,
               "present": true, "linked": true})
    };
    let mut people = vec![person("presenter-iain", "presenter", true)];
    people.extend(callers.iter().map(|(id, on)| person(id, "caller", *on)));
    serde_json::from_value(json!({ "people": people })).unwrap()
}

async fn register_recording(
    app: &Router,
    started_ms: u64,
    ended_ms: Option<u64>,
    file: &str,
) -> i64 {
    let (code, r) = send(
        app,
        admin(
            "POST",
            "/api/v1/retention/recordings",
            json!({"kind": "programme", "file": file,
                   "started_at_ms": started_ms, "ended_at_ms": ended_ms}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{r}");
    r["id"].as_i64().unwrap()
}

async fn linked(pool: &sqlx::PgPool, identity: &str, recording: i64) -> bool {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM consent_recording cr JOIN caller_consent cc ON cc.id = cr.consent_id
         WHERE cc.identity = $1 AND cr.recording_id = $2",
    )
    .bind(identity)
    .bind(recording)
    .fetch_one(pool)
    .await
    .unwrap();
    n == 1
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_desks_status_records_who_was_on_air_and_only_they_are_linked_to_a_recording() {
    let (pool, station, dir) = setup("Kettle FM").await;
    let (state, app) = control_plane(&pool, &station, &dir);
    let hub = &state.hub;
    let now = now_ms();
    let on = "caller-aaaaaaaaaaaa";
    let waiting = "caller-bbbbbbbbbbbb";
    let screened = "caller-cccccccccccc";
    for id in [on, waiting, screened] {
        weavesignal_model::record_consent(
            &pool,
            station.id,
            &ConsentRecord {
                identity: id.into(),
                name: "Jessie Brown".into(),
                topic: "No bus after 23:10".into(),
                given_by: "caller",
                words: "Your call may be broadcast live and recorded.".into(),
                words_version: "0123456789abcdef".into(),
                accepted_at_ms: now - 60_000,
                show_id: None,
                presenter: String::new(),
                contact: String::new(),
            },
        )
        .await
        .unwrap();
    }
    let open_stretches = || async {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM consent_on_air a JOIN caller_consent cc ON cc.id = a.consent_id
             WHERE cc.station_id = $1 AND a.to_at IS NULL",
        )
        .bind(station.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
        n
    };

    // Everyone in the room but nobody on air: nothing recorded. A presenter on air is not a
    // caller and has no consent.
    hub.take_status(Ok(desk_with(&[
        (on, false),
        (waiting, false),
        (screened, false),
    ])))
    .await;
    assert_eq!(open_stretches().await, 0);
    // The presenter puts one caller on air, by whatever means: the status shows it.
    hub.take_status(Ok(desk_with(&[
        (on, true),
        (waiting, false),
        (screened, false),
    ])))
    .await;
    assert_eq!(open_stretches().await, 1);
    // Polling again changes nothing.
    hub.take_status(Ok(desk_with(&[
        (on, true),
        (waiting, false),
        (screened, false),
    ])))
    .await;
    assert_eq!(open_stretches().await, 1);

    // A recording that covers the call takes the consent of the caller who was on air and of
    // no other, though all three calls overlap it.
    std::fs::write(dir.join("hour.opus"), b"audio").unwrap();
    let rec = register_recording(&app, now - 120_000, Some(now + 60_000), "hour.opus").await;
    assert!(linked(&pool, on, rec).await);
    assert!(!linked(&pool, waiting, rec).await);
    assert!(!linked(&pool, screened, rec).await);

    // The caller comes off air: the stretch closes. A recording made after that does not
    // carry them.
    hub.take_status(Ok(desk_with(&[
        (on, false),
        (waiting, false),
        (screened, false),
    ])))
    .await;
    assert_eq!(open_stretches().await, 0);
    std::fs::write(dir.join("later.opus"), b"audio").unwrap();
    let later = register_recording(&app, now + 5_000, Some(now + 3_600_000), "later.opus").await;
    assert!(!linked(&pool, on, later).await);

    // On air again, then the desk stops answering: the stretch stays open (erring on the side
    // of linking) until the desk answers again without them on air.
    hub.take_status(Ok(desk_with(&[(on, true)]))).await;
    assert_eq!(open_stretches().await, 1);
    hub.take_status(Err(DeskError::Unreachable {
        path: "desk.sock".into(),
        reason: "gone".into(),
    }))
    .await;
    assert_eq!(open_stretches().await, 1, "no desk is no news");
    hub.take_status(Ok(desk_with(&[(on, false)]))).await;
    assert_eq!(open_stretches().await, 0);
    let (stretches,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM consent_on_air a JOIN caller_consent cc ON cc.id = a.consent_id
         WHERE cc.station_id = $1 AND cc.identity = $2",
    )
    .bind(station.id.as_uuid())
    .bind(on)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stretches, 2, "on, off, on, off: two stretches");

    // A restart finds a stretch a crash left open and ends it at its first poll.
    sqlx::query("UPDATE caller_consent SET call_ended_at = NULL WHERE identity = $1")
        .bind(on)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO consent_on_air (consent_id) SELECT id FROM caller_consent WHERE identity = $1",
    )
    .bind(on)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(open_stretches().await, 1);
    let (restarted, _) = control_plane(&pool, &station, &dir);
    restarted
        .hub
        .take_status(Ok(desk_with(&[(on, false)])))
        .await;
    assert_eq!(open_stretches().await, 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_hold_is_extended_through_the_route_and_the_flag_clears_until_the_new_date() {
    use weavesignal_model::retention as model;
    let (pool, station, dir) = setup("Teacup FM").await;
    let (_, app) = control_plane(&pool, &station, &dir);
    let id = station.id;
    let by = "Morag Ross";
    let day = 86_400_000u64;
    model::save_setting(
        &pool,
        id,
        &model::Setting {
            preset: "ca_licensed".into(),
            ..model::Setting::default()
        },
        by,
    )
    .await
    .unwrap();
    let now = now_ms();
    let (code, h) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/holds",
            json!({"reason": "CRTC complaint", "by": by,
                   "from_ms": now - 100 * day, "to_ms": now - 100 * day + 3_600_000}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{h}");
    let hold = h["id"].as_i64().unwrap();
    let uri = format!("/api/v1/retention/holds/{hold}/extend");
    let flagged = |at: u64| {
        let pool = pool.clone();
        async move {
            let setting = model::load_setting(&pool, id).await.unwrap();
            model::holds_past_date(&pool, id, &setting, at)
                .await
                .unwrap()
                .len()
        }
    };
    assert_eq!(flagged(now).await, 1, "100 days on, past the 56-day date");

    // Only an admin extends; a date that is not later, or no name, is refused with a sentence.
    let anon = Request::builder()
        .method("POST")
        .uri(&uri)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"until_ms": now + day, "by": by, "reason": "r"}).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&app, anon).await.0, StatusCode::UNAUTHORIZED);
    for body in [
        json!({"until_ms": now - 50 * day, "by": by, "reason": "Still open"}),
        json!({"until_ms": now + 10 * day, "by": " ", "reason": "Still open"}),
        json!({"until_ms": now + 10 * day, "by": by, "reason": ""}),
    ] {
        let (code, e) = send(&app, admin("POST", &uri, body)).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    }
    assert_eq!(flagged(now).await, 1, "refused extensions change nothing");

    let (code, h) = send(
        &app,
        admin(
            "POST",
            &uri,
            json!({"until_ms": now + 30 * day, "by": by, "reason": "The regulator asked for the tapes again"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{h}");
    assert_eq!(h["extended_by"], by);
    assert_eq!(
        h["extend_reason"],
        "The regulator asked for the tapes again"
    );
    assert!(
        h["extended_until_ms"]
            .as_u64()
            .unwrap()
            .abs_diff(now + 30 * day)
            < 5
    );
    assert!(h["released_at_ms"].is_null());
    assert_eq!(flagged(now).await, 0, "extended: no longer flagged");
    assert_eq!(
        flagged(now + 31 * day).await,
        1,
        "flagged again after the new date"
    );
    // The list shows it.
    let (_, list) = send(&app, admin("GET", "/api/v1/retention/holds", json!(null))).await;
    assert_eq!(list[0]["extended_by"], by);

    // A released hold cannot be extended; one that is not there is not found.
    let (code, _) = send(
        &app,
        admin(
            "POST",
            &format!("/api/v1/retention/holds/{hold}/release"),
            json!({"by": by, "reason": "closed"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = send(
        &app,
        admin(
            "POST",
            &uri,
            json!({"until_ms": now + 90 * day, "by": by, "reason": "again"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = send(
        &app,
        admin(
            "POST",
            "/api/v1/retention/holds/999999999/extend",
            json!({"until_ms": now + 90 * day, "by": by, "reason": "again"}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(&dir).ok();
}
