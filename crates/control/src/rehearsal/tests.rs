// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio's link and routes against two fake desks, a live one and a rehearsal one
//! (steps R6 and R7): commands reach only the rehearsal desk, a desk in the wrong mode is sent
//! nothing, the studio's view and the per-play log see nothing of a rehearsal, and the rights
//! and the cart rule follow the session's show.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use weavesignal_model::carts::{Bank, Cart, CartMode};
use weavesignal_model::rehearsal::{Session, ShowRef};
use weavesignal_model::staff::Role as Staff;
use weavesignal_model::{Station, StationId};

use super::*;
use crate::app::{AppState, router};
use crate::config::{CartsConfig, JoinConfig};
use crate::desk::fake::{self, FakeDesk};
use crate::desk::tests::tempdir;
use crate::studio::Studio;

const ADMIN: &str = "rehearsal-test-key";

struct World {
    app: Router,
    state: Arc<AppState>,
    live: Arc<Mutex<FakeDesk>>,
    rehearsal: Arc<Mutex<FakeDesk>>,
    dir: PathBuf,
    /// The station's cart, Steeped's own, and The Morning Kettle's own.
    carts: [Uuid; 3],
    /// Steeped's presenter's account.
    presenter: Uuid,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn show(slug: &str, name: &str, presenter: Option<Uuid>) -> ShowRef {
    ShowRef {
        id: Uuid::new_v4(),
        slug: slug.into(),
        name: name.into(),
        presenter_account: presenter,
    }
}

fn cart(slot: u8, label: &str, show: Option<&str>, file: &str) -> Cart {
    Cart {
        id: Uuid::new_v4(),
        bank: 1,
        slot,
        label: label.into(),
        detail: String::new(),
        mode: CartMode::OneShot,
        file: Some(file.into()),
        duration_ms: 3_000,
        gain_db: -7.0,
        loudness_lufs: Some(-16.0),
        true_peak_dbtp: Some(-1.5),
        ramp_ms: None,
        origin: String::new(),
        licence: String::new(),
        show: show.map(str::to_owned),
    }
}

/// A station with a live desk, a rehearsal desk (`rehearsal_mode`: what it says it mixes), the
/// rehearsal studio on, two shows and three carts.
fn world_with(rehearsal_mode: Option<&str>, sessions: Vec<Session>) -> World {
    let dir = tempdir();
    let live = fake::spawn(&dir.join("desk.sock"));
    let rehearsal = fake::spawn(&dir.join("desk-rehearsal.sock"));
    rehearsal.lock().unwrap().desk_mode = rehearsal_mode.map(str::to_owned);
    rehearsal.lock().unwrap().people = vec![json!({"identity": "eilidh",
        "name": "Eilidh Fraser-MacLeod", "role": "presenter", "on_air": true, "talk": false,
        "present": true, "linked": true})];
    let cfg = CartsConfig {
        media_dir: dir.join("carts"),
        ..CartsConfig::default()
    };
    std::fs::create_dir_all(&cfg.media_dir).unwrap();
    let hub = Hub::new(
        "Kettle FM",
        Studio::new("The Morning Kettle", vec![]),
        DeskClient::new(Some(&dir.join("desk.sock"))),
        None,
        Duration::from_millis(250),
        crate::carts::CartsLink::new(cfg.clone(), None),
    );
    let carts = vec![
        cart(1, "Station ident", None, "a.flac"),
        cart(2, "Steeped sting", Some("steeped"), "b.flac"),
        cart(3, "Kettle whistle", Some("the-morning-kettle"), "c.flac"),
    ];
    for f in ["a.flac", "b.flac", "c.flac"] {
        std::fs::write(cfg.media_dir.join(f), b"fLaC").unwrap();
    }
    let ids = [carts[0].id, carts[1].id, carts[2].id];
    let bank = Bank {
        number: 1,
        name: "Idents".into(),
        colour_slot: 3,
        show: None,
    };
    hub.with_inner(|i| i.carts.replace(vec![bank], carts, &cfg.media_dir));
    let presenter = Uuid::new_v4();
    let store = Store::Memory(Mutex::new(Memory {
        shows: vec![
            show("steeped", "Steeped", Some(presenter)),
            show(
                "the-morning-kettle",
                "The Morning Kettle",
                Some(Uuid::new_v4()),
            ),
        ],
        sessions,
        ..Memory::default()
    }));
    let config = RehearsalConfig {
        enabled: true,
        socket: Some(dir.join("desk-rehearsal.sock")),
        ..RehearsalConfig::default()
    };
    let r = Rehearsal::new(config, "studio", store, Duration::from_millis(250));
    assert!(hub.rehearsal.set(Arc::new(r)).is_ok());
    let state = Arc::new(AppState {
        station: Station::new(StationId::new_random(), "Kettle FM").unwrap(),
        db: None,
        room: Some(crate::app::RoomState {
            config: crate::config::RoomConfig::default(),
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
    World {
        app: router(state.clone()),
        state,
        live,
        rehearsal,
        dir,
        carts: ids,
        presenter,
    }
}

fn keys() -> weavesignal_proto::Keys {
    weavesignal_proto::Keys {
        api_key: "weavesignal".into(),
        api_secret: "test-only-secret-test-only-secret-0".into(),
    }
}

fn world() -> World {
    world_with(Some("rehearsal"), Vec::new())
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {bearer}"));
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

async fn post(app: &Router, uri: &str, bearer: &str, body: Value) -> (StatusCode, Value) {
    send(app, "POST", uri, bearer, body).await
}

/// The requests a fake desk was sent other than `status`, raw (`cart_plays` included).
fn sent(desk: &Mutex<FakeDesk>) -> Vec<String> {
    desk.lock()
        .unwrap()
        .lines
        .iter()
        .filter(|l| l["op"] != "status")
        .map(|l| l["op"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test]
async fn commands_reach_only_the_rehearsal_desk_and_the_studio_sees_nothing() {
    let w = world();
    let mut studio = w.state.hub.subscribe();
    studio.mark_unchanged();
    let mut plays = w.state.hub.subscribe_plays();

    let (code, view) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        ADMIN,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{view}");
    assert_eq!(view["session"]["show"], "steeped");
    assert_eq!(view["session"]["opened_by"], "The station key");

    let [station_cart, steeped_cart, kettle_cart] = w.carts;
    for cmd in [
        json!({"op": "on_air", "identity": "eilidh", "on": false}),
        json!({"op": "mute", "identity": "eilidh", "on": true}),
        json!({"op": "cue", "identity": "eilidh", "on": true}),
        json!({"op": "talk", "identity": "eilidh", "on": true}),
        json!({"op": "talk_to", "identity": "eilidh", "to": "gregor", "on": true}),
        json!({"op": "cart_fire", "cart": station_cart}),
        json!({"op": "cart_fire", "cart": steeped_cart, "cue": true}),
        json!({"op": "cart_stop", "cart": station_cart}),
        json!({"op": "cart_stop_all"}),
    ] {
        let (code, body) = post(&w.app, "/api/v1/rehearsal/commands", ADMIN, cmd.clone()).await;
        assert_eq!(code, StatusCode::OK, "{cmd}: {body}");
    }
    assert_eq!(
        sent(&w.rehearsal),
        [
            "on_air",
            "mute",
            "cue",
            "talk",
            "talk_to",
            "cart_fire",
            "cart_fire",
            "cart_stop",
            "cart_stop_all"
        ]
    );
    {
        let f = w.rehearsal.lock().unwrap();
        let on = f.lines.iter().find(|l| l["op"] == "on_air").unwrap();
        assert!(on.get("fade_ms").is_none());
        let fire = f.lines.iter().find(|l| l["op"] == "cart_fire").unwrap();
        assert_eq!(fire["file"], "a.flac");
        assert_eq!(fire["gain_db"], -7.0);
    }
    // The view says `on`, never `on_air`.
    let (_, view) = send(&w.app, "GET", "/api/v1/rehearsal", ADMIN, Value::Null).await;
    assert_eq!(view["people"][0]["on"], false);
    assert!(view["people"][0].get("on_air").is_none());
    assert_eq!(view["carts_sounding"].as_array().unwrap().len(), 2);

    // Another show's own cart does not fire in this rehearsal, whatever is on air.
    let (code, body) = post(
        &w.app,
        "/api/v1/rehearsal/commands",
        ADMIN,
        json!({"op": "cart_fire", "cart": kettle_cart}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("in rehearsal"));

    // What a rehearsal desk does not take is refused here, naming it, and never sent.
    for op in [
        "dump",
        "picture",
        "mode",
        "kit",
        "phone_admit",
        "status",
        "caption",
    ] {
        let (code, body) = post(
            &w.app,
            "/api/v1/rehearsal/commands",
            ADMIN,
            json!({"op": op}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{op}");
        assert!(
            body["error"].as_str().unwrap().contains(&format!("`{op}`")),
            "{body}"
        );
    }
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/commands",
        ADMIN,
        json!({"op": "mute", "identity": "eilidh", "on": true, "fade_ms": 300}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    // Nothing reached the live desk, not even a status; the studio's view (what its events, the
    // webhooks and the chat follow) did not move; no play was recorded for the per-play log.
    assert!(w.live.lock().unwrap().lines.is_empty());
    assert!(!studio.has_changed().unwrap());
    assert!(w.state.hub.cart_plays().is_empty());
    assert!(plays.try_recv().is_err());
    assert!(!sent(&w.rehearsal).iter().any(|op| op == "cart_plays"));
    let studio_view = serde_json::to_string(&*w.state.hub.view()).unwrap();
    assert!(!studio_view.contains("eilidh") && !studio_view.contains("Steeped"));

    // The live desk's own routes still go to the live desk only.
    let before = sent(&w.rehearsal).len();
    let (code, _) = post(
        &w.app,
        "/api/v1/desk/commands",
        ADMIN,
        json!({"op": "mute", "identity": "eilidh", "on": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(sent(&w.live), ["mute"]);
    assert_eq!(sent(&w.rehearsal).len(), before);
}

#[tokio::test]
async fn whoever_may_change_the_shows_step_runs_its_rehearsal_live_or_preparing() {
    let w = world();
    let staff = &w.state.staff;
    let presenter = staff.signed_in_as_for_test(Staff::Presenter, w.presenter);
    let other = staff.signed_in_for_test(Staff::Presenter);
    let producer = staff.signed_in_for_test(Staff::Producer);

    // Nothing to drive before a session is open.
    let mute = json!({"op": "mute", "identity": "eilidh", "on": true});
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/commands",
        &presenter,
        mute.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = post(&w.app, "/api/v1/rehearsal/end", &presenter, Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT);

    // A preparing window may (ADR-0276): nothing here is on air.
    let (code, body) = send(
        &w.app,
        "PUT",
        "/api/v1/sessions/me/mode",
        &presenter,
        json!({"preparing": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, _) = post(&w.app, "/api/v1/desk/commands", &presenter, mute.clone()).await;
    assert_eq!(
        code,
        StatusCode::CONFLICT,
        "the live desk refuses a preparing window"
    );
    let (code, view) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &presenter,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{view}");
    assert_eq!(view["session"]["opened_by"], "Morag");
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/commands",
        &presenter,
        mute.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // Again for the same show: the same session.
    let (code, again) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &presenter,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(again["session"]["id"], view["session"]["id"]);

    // Another show's presenter may neither open Steeped's nor drive it.
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &other,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = post(&w.app, "/api/v1/rehearsal/commands", &other, mute.clone()).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = post(&w.app, "/api/v1/rehearsal/end", &other, Value::Null).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // But anyone on the staff may look.
    let (code, _) = send(&w.app, "GET", "/api/v1/rehearsal", &other, Value::Null).await;
    assert_eq!(code, StatusCode::OK);

    // One show at a time: a producer opening another is told who is rehearsing what.
    let (code, body) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &producer,
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Morag is rehearsing Steeped"),
        "{body}"
    );
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &producer,
        json!({"show": "nae-such"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // A producer may end it; then the studio is free, and the session is new when reopened.
    let (code, ended) = post(&w.app, "/api/v1/rehearsal/end", &producer, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{ended}");
    assert!(ended["session"].is_null());
    assert_eq!(
        sent(&w.rehearsal).last().map(String::as_str),
        Some("cart_stop_all")
    );
    let (code, _) = post(&w.app, "/api/v1/rehearsal/commands", &presenter, mute).await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, next) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &producer,
        json!({"show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    assert_ne!(next["session"]["id"], view["session"]["id"]);
    assert!(w.live.lock().unwrap().lines.is_empty());
}

#[tokio::test]
async fn a_desk_that_is_not_a_rehearsal_desk_is_sent_nothing() {
    // A live desk (or one from before rehearsal) behind the rehearsal socket.
    for mode in [None, Some("live")] {
        let w = world_with(mode, Vec::new());
        let (code, _) = post(
            &w.app,
            "/api/v1/rehearsal/open",
            ADMIN,
            json!({"show": "steeped"}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
        let (code, body) = post(
            &w.app,
            "/api/v1/rehearsal/commands",
            ADMIN,
            json!({"op": "on_air", "identity": "eilidh", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{mode:?}: {body}");
        assert!(
            body["error"].as_str().unwrap().contains("nothing was sent"),
            "{body}"
        );
        assert!(
            sent(&w.rehearsal).is_empty(),
            "{mode:?}: only its status was asked"
        );
        let r = w.state.hub.rehearsal.get().unwrap();
        r.poll_once(BTreeMap::new()).await;
        let v = r.view();
        assert!(!v.desk.linked);
        assert!(v.desk.error.as_deref().unwrap().contains("live desk"));
        assert!(v.people.is_empty());
    }

    // And the live link sends nothing to a rehearsal desk behind the live socket.
    let w = world();
    w.live.lock().unwrap().desk_mode = Some("rehearsal".into());
    let hub = &w.state.hub;
    let refused = hub
        .command(&DeskCommand::Mute {
            identity: "eilidh".into(),
            on: true,
        })
        .await;
    assert!(matches!(
        refused,
        Err(DeskError::WrongMode {
            expected: "live",
            found: "rehearsal",
            ..
        })
    ));
    hub.poll_once().await;
    assert!(sent(&w.live).is_empty());
    // A live desk from before rehearsal says nothing, and is the live desk.
    w.live.lock().unwrap().desk_mode = None;
    hub.command(&DeskCommand::Mute {
        identity: "eilidh".into(),
        on: true,
    })
    .await
    .unwrap();
    assert_eq!(sent(&w.live), ["mute"]);
}

#[tokio::test]
async fn a_restart_reads_the_open_session_back() {
    let open = Session {
        id: Uuid::new_v4(),
        show_id: Uuid::new_v4(),
        show_slug: "steeped".into(),
        show_name: "Steeped".into(),
        presenter_account: None,
        opened_by: "Finlay".into(),
        opened_at_ms: 1_763_700_000_000,
        ended_at_ms: None,
        ended: None,
        moved_at_ms: None,
        moved: Vec::new(),
        opening: None,
    };
    let w = world_with(Some("rehearsal"), vec![open.clone()]);
    let r = w.state.hub.rehearsal.get().unwrap();
    assert!(r.session().is_none());
    r.restore().await.unwrap();
    assert_eq!(r.session().map(|s| s.id), Some(open.id));
    let (_, view) = send(&w.app, "GET", "/api/v1/rehearsal", ADMIN, Value::Null).await;
    assert_eq!(view["session"]["opened_by"], "Finlay");
    assert_eq!(view["enabled"], true);
    assert_eq!(view["max_people"], 6);
}

#[tokio::test]
async fn a_session_with_nobody_in_it_ends_after_the_idle_time() {
    let w = world();
    let r = w.state.hub.rehearsal.get().unwrap();
    let (code, _) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        ADMIN,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    let empty = || {
        Ok(DeskStatus {
            desk_mode: Some(crate::desk::DeskMode::Rehearsal),
            started_ms: None,
            ..DeskStatus::default()
        })
    };
    let t0 = 10_000_000;
    // Someone present keeps it open however long; then empty from t0.
    let mut st = empty().unwrap();
    st.people = w
        .rehearsal
        .lock()
        .unwrap()
        .people
        .iter()
        .map(|p| serde_json::from_value(p.clone()).unwrap())
        .collect();
    r.take_status(Ok(st), BTreeMap::new(), t0 - 5_000_000);
    assert!(r.idle(t0).is_none());
    r.take_status(empty(), BTreeMap::new(), t0);
    let idle_ms = r.config.idle_end_s * 1000;
    assert!(r.idle(t0 + idle_ms - 1).is_none());
    // A desk that does not answer neither starts nor stops the count.
    r.take_status(Err(DeskError::NotConfigured), BTreeMap::new(), t0 + 10);
    assert!(r.idle(t0 + idle_ms).is_some());
}

#[tokio::test]
async fn a_station_without_a_rehearsal_studio_says_so() {
    let dir = tempdir();
    let hub = Hub::new(
        "Kettle FM",
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        crate::carts::CartsLink::none(),
    );
    let state = Arc::new(AppState {
        station: Station::new(StationId::new_random(), "Kettle FM").unwrap(),
        db: None,
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    });
    let app = router(state);
    let (code, view) = send(&app, "GET", "/api/v1/rehearsal", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(view["enabled"], false);
    assert!(view["session"].is_null());
    let (code, body) = post(
        &app,
        "/api/v1/rehearsal/open",
        ADMIN,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["error"].as_str().unwrap().contains("REHEARSAL"));
    // Without a bearer, nothing.
    let res = app
        .oneshot(
            Request::get("/api/v1/rehearsal")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_settings_refuse_a_rehearsal_room_that_looks_like_a_booth_or_the_live_desk() {
    let ok = RehearsalConfig::default();
    assert!(ok.validate("desk").is_ok());
    assert_eq!(ok.room("studio"), "studio-rehearsal");
    let on = RehearsalConfig {
        enabled: true,
        ..RehearsalConfig::default()
    };
    assert!(on.validate("desk").is_err(), "on needs a socket");
    for suffix in [
        "-booth-x",
        "-phone",
        "-monitor-a",
        "rehearsal",
        "-Rehearsal",
        "-",
    ] {
        let c = RehearsalConfig {
            room_suffix: suffix.into(),
            ..RehearsalConfig::default()
        };
        assert!(c.validate("desk").is_err(), "{suffix}");
    }
    let same = RehearsalConfig {
        desk_identity: "desk".into(),
        ..RehearsalConfig::default()
    };
    assert!(same.validate("desk").is_err());
    let full = RehearsalConfig {
        max_people: 0,
        ..RehearsalConfig::default()
    };
    assert!(full.validate("desk").is_err());
    let text = "[station]\nid = \"6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10\"\nname = \"Kettle FM\"\n\n[rehearsal]\nenabled = true\nsocket = \"/run/weavesignal/desk-rehearsal.sock\"\n";
    let c = crate::config::Config::from_toml(text, "test").unwrap();
    assert!(c.rehearsal.enabled);
    assert_eq!(c.rehearsal.max_people, 6);
    assert_eq!(c.rehearsal.idle_end_s, 1_800);
}

// --- R8: invitations and passes to the rehearsal studio ----------------------------------------

/// An invitation as the route answers it.
async fn invite(w: &World, bearer: &str, body: Value) -> (StatusCode, Value) {
    post(&w.app, "/api/v1/invites", bearer, body).await
}

async fn token(w: &World, pass: &str) -> (StatusCode, Value) {
    post(&w.app, "/api/v1/pass/token", pass, json!({})).await
}

async fn open_steeped(w: &World) -> Value {
    let (code, view) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        ADMIN,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{view}");
    view
}

#[tokio::test]
async fn a_rehearsal_link_needs_an_open_session_of_its_show_and_rights_over_it() {
    let w = world();
    let other = w.state.staff.signed_in_for_test(Staff::Presenter);
    let guest = json!({"role": "guest", "name": "Gregor Stewart", "studio": "rehearsal", "show": "steeped"});
    // Not open yet.
    let (code, body) = invite(&w, ADMIN, guest.clone()).await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("Steeped"));
    // No show named, a show that does not exist, a show named for the live studio.
    let (code, body) = invite(&w, ADMIN, json!({"role": "guest", "studio": "rehearsal"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["field"], "show");
    let (code, _) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "studio": "rehearsal", "show": "no-such-show"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = invite(&w, ADMIN, json!({"role": "guest", "show": "steeped"})).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    let session = open_steeped(&w).await["session"]["id"].clone();
    // Another show's presenter may not invite to Steeped's rehearsal.
    let (code, _) = invite(&w, &other, guest.clone()).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // The station key may; the pass names the session and leads to the rehearsal studio.
    let (code, inv) = invite(&w, ADMIN, guest.clone()).await;
    assert_eq!(code, StatusCode::CREATED, "{inv}");
    assert_eq!(inv["studio"], "rehearsal");
    let claims = keys().verify_pass(inv["pass"].as_str().unwrap()).unwrap();
    assert_eq!(claims.studio, weavesignal_proto::Studio::Rehearsal);
    assert_eq!(claims.session.as_deref(), session.as_str());
    // A session that is not this show's: refused.
    let (code, _) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "studio": "rehearsal", "show": "the-morning-kettle"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_rehearsal_pass_never_buys_a_token_for_the_studio_room() {
    let w = world();
    open_steeped(&w).await;
    let (_, inv) = invite(
        &w,
        ADMIN,
        json!({"role": "presenter", "name": "Eilidh Fraser-MacLeod", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    let pass = inv["pass"].as_str().unwrap().to_owned();
    let (code, t) = token(&w, &pass).await;
    assert_eq!(code, StatusCode::CREATED, "{t}");
    assert_eq!(t["room"], "studio-rehearsal");
    let id = t["identity"].as_str().unwrap();
    assert_eq!(t["hear"], format!("desk-rehearsal-mm-{id}"));
    assert!(t["see"].is_null());
    let claims = keys().verify(t["token"].as_str().unwrap()).unwrap();
    assert_eq!(claims.video.room.as_deref(), Some("studio-rehearsal"));
    assert_ne!(claims.video.room.as_deref(), Some("studio"));
    // What the pass is, for its page.
    let (code, info) = send(&w.app, "GET", "/api/v1/pass", &pass, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(info["studio"], "rehearsal");
    assert_eq!(info["show"], "Steeped");
    assert!(info["not_now"].is_null());

    // Ended: it buys nothing more, and says why.
    let (code, _) = post(&w.app, "/api/v1/rehearsal/end", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    let (code, body) = token(&w, &pass).await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("not open"));
    let (_, info) = send(&w.app, "GET", "/api/v1/pass", &pass, Value::Null).await;
    assert!(info["not_now"].as_str().unwrap().contains("not open"));
    // Opening the studio again for the same show does not bring the old link back.
    open_steeped(&w).await;
    let (code, _) = token(&w, &pass).await;
    assert_eq!(code, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_rehearsal_room_is_refused_when_full_and_follows_the_rehearsed_shows_step() {
    let w = world();
    open_steeped(&w).await;
    // Six present at the rehearsal desk (its default `max_people`).
    let six: Vec<Value> = ["eilidh", "gregor", "kirsty", "finlay", "iona", "callum"]
        .iter()
        .map(|id| {
            json!({"identity": id, "name": "", "role": "guest", "on_air": true,
            "talk": false, "present": true, "linked": true})
        })
        .collect();
    w.rehearsal.lock().unwrap().people = six;
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    r.poll_once(BTreeMap::new()).await;
    let (_, inv) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    let (code, body) = token(&w, inv["pass"].as_str().unwrap()).await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("full"));
    // Someone already in the room rejoins.
    let back = keys()
        .pass_with(
            weavesignal_proto::PassKind::Invite,
            Role::Guest,
            "gregor",
            "Gregor Stewart",
            600,
            weavesignal_proto::PassScope::rehearsal(&r.session().unwrap().id.to_string()),
        )
        .unwrap()
        .0;
    assert_eq!(token(&w, &back).await.0, StatusCode::CREATED);

    // At Solo, Steeped's rehearsal takes only its presenter's own link: a guest is refused
    // whatever step the show on air runs at.
    w.rehearsal.lock().unwrap().people.clear();
    r.poll_once(BTreeMap::new()).await;
    if let Store::Memory(m) = &r.store {
        m.lock()
            .unwrap()
            .steps
            .insert("steeped".into(), weavesignal_model::shows::Step::Solo);
    }
    let (code, body) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let presenter = w
        .state
        .staff
        .signed_in_as_for_test(Staff::Presenter, w.presenter);
    let (code, own) = invite(
        &w,
        &presenter,
        json!({"for_me": true, "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{own}");
    let (code, t) = token(&w, own["pass"].as_str().unwrap()).await;
    assert_eq!(code, StatusCode::CREATED, "{t}");
    assert_eq!(t["identity"], crate::steps::own_identity(w.presenter));
    assert_eq!(t["room"], "studio-rehearsal");
}

#[tokio::test]
async fn every_live_invitation_arrives_standing_by_and_a_call_does_not() {
    let w = world();
    for role in ["presenter", "guest", "dj"] {
        let (code, inv) = invite(&w, ADMIN, json!({"role": role})).await;
        assert_eq!(code, StatusCode::CREATED, "{inv}");
        assert_eq!(inv["studio"], "live");
        let (code, t) = token(&w, inv["pass"].as_str().unwrap()).await;
        assert_eq!(code, StatusCode::CREATED, "{t}");
        assert_eq!(t["room"], "studio");
        let c = keys().verify(t["token"].as_str().unwrap()).unwrap();
        assert_eq!(
            c.standby_until_ms(),
            Some(weavesignal_proto::UNTIL_PUT_ON_AIR),
            "{role}"
        );
    }
}

// --- R9: the move and the opening state ---------------------------------------------------------

/// A world with Steeped airing from `start_ms` for an hour, a grace of 120 s and a fade of 2 s.
fn world_airing(start_ms: i64) -> World {
    let w = world();
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    if let Store::Memory(m) = &r.store {
        let mut m = m.lock().unwrap();
        let steeped = m.shows.iter().find(|s| s.slug == "steeped").unwrap().id;
        m.airings.push((steeped, start_ms, start_ms + 3_600_000));
        m.grace_s = 120;
        m.fade_ms = 2_000;
    }
    w
}

/// The rehearsal desk with Steeped's presenter on, Gregor on and muted, Kirsty off.
fn rehearsing(w: &World) -> String {
    let host = crate::steps::own_identity(w.presenter);
    let p = |id: &str, role: &str, on: bool, mute: bool| {
        json!({"identity": id, "name": "", "role": role, "on_air": on, "mute": mute,
            "talk": false, "present": true, "linked": true})
    };
    w.rehearsal.lock().unwrap().people = vec![
        p(&host, "presenter", true, false),
        p("gregor", "guest", true, true),
        p("kirsty", "guest", false, false),
    ];
    host
}

#[tokio::test]
async fn the_move_is_refused_without_an_airing_within_twelve_hours() {
    let now = now_ms() as i64;
    let w = world_airing(now + 13 * 3_600_000);
    rehearsing(&w);
    let (code, _) = post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "no session");
    open_steeped(&w).await;
    let (code, body) = post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("twelve hours"));
    // Still rehearsing: nothing was moved.
    assert!(w.state.hub.rehearsal.get().unwrap().session().is_some());
}

#[tokio::test]
async fn the_move_writes_the_opening_ends_the_session_and_sends_each_page_its_live_pass() {
    let now = now_ms() as i64;
    let start = now + 600_000;
    let w = world_airing(start);
    let host = rehearsing(&w);
    open_steeped(&w).await;
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    let session = r.session().unwrap().id;
    // Gregor's page, on his rehearsal link.
    let gregor = keys()
        .pass_with(
            weavesignal_proto::PassKind::Invite,
            Role::Guest,
            "gregor",
            "Gregor Stewart",
            600,
            weavesignal_proto::PassScope::rehearsal(&session.to_string()),
        )
        .unwrap()
        .0;

    let (code, moved) = post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{moved}");
    assert_eq!(moved["airing_start_ms"], start);
    assert_eq!(moved["opening"]["host"], host.as_str());
    let mut on = vec![host.clone(), "gregor".to_owned()];
    on.sort();
    assert_eq!(moved["opening"]["on"], json!(on));
    assert_eq!(moved["opening"]["mute"], json!(["gregor"]));
    assert_eq!(moved["opening"]["standby_until_ms"], start + 120_000);
    assert_eq!(moved["moved"].as_array().unwrap().len(), 3);
    // The session is closed: the rehearsal link buys nothing, and says where they went.
    assert!(r.session().is_none());
    let (code, body) = token(&w, &gregor).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("live studio"));
    // The page of someone who was in it is told it takes them there; one opened after the move
    // by someone who was not is told it goes nowhere (1.3.1).
    let (code, info) = send(&w.app, "GET", "/api/v1/pass", &gregor, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{info}");
    assert!(
        info["not_now"]
            .as_str()
            .unwrap()
            .contains("takes you there")
    );
    let morag = keys()
        .pass_with(
            weavesignal_proto::PassKind::Invite,
            Role::Guest,
            "morag",
            "Morag Fraser",
            600,
            weavesignal_proto::PassScope::rehearsal(&session.to_string()),
        )
        .unwrap()
        .0;
    let (code, info) = send(&w.app, "GET", "/api/v1/pass", &morag, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{info}");
    let said = info["not_now"].as_str().unwrap();
    assert!(!said.contains("takes you there"), "{said}");
    assert!(said.contains("link to the live studio"), "{said}");
    // The studio's view knows who was moved and the opening state, for any window (1.3.1).
    let moved_standing = crate::post::moved_standing(&w.state, now);
    assert_eq!(moved_standing.len(), 3);
    assert_eq!(moved_standing["gregor"], start + 120_000);
    let opening = crate::post::opening_for(&w.state, "steeped", now).unwrap();
    assert_eq!(opening.on, on);
    assert_eq!(opening.mute, ["gregor"]);
    assert_eq!(opening.host.as_deref(), Some(host.as_str()));
    assert_eq!(opening.standby_until_ms, start + 120_000);
    assert!(crate::post::opening_for(&w.state, "the-morning-kettle", now).is_none());
    // Nothing was sent to the live desk by the move itself.
    assert!(
        sent(&w.live)
            .iter()
            .all(|op| op != "on_air" && op != "mute")
    );

    // His page's events carry the move: a live pass for the same person, to the live studio.
    let res = w
        .app
        .clone()
        .oneshot(
            Request::get("/api/v1/pass/events")
                .header("authorization", format!("Bearer {gregor}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut body = res.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(3), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
    let me: Value = serde_json::from_str(data).unwrap();
    assert_eq!(me["studio"], "rehearsal");
    assert_eq!(me["show_on_air"], false);
    let live = me["move"]["pass"].as_str().unwrap();
    assert!(me["move"]["link"].as_str().unwrap().ends_with(live));
    assert_eq!(me["move"]["show"], "Steeped");
    let c = keys().verify_pass(live).unwrap();
    assert_eq!(c.studio, weavesignal_proto::Studio::Live);
    assert_eq!((c.id.as_str(), c.role), ("gregor", Role::Guest));
    assert_eq!(c.show.as_deref(), Some("steeped"));
    assert_eq!(c.standby_until_ms, Some((start + 120_000) as u64));
    // After a reload the live pass still names the show it was made for, whatever is on air.
    assert_eq!(c.show_name.as_deref(), Some("Steeped"));
    let (code, info) = send(&w.app, "GET", "/api/v1/pass", live, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{info}");
    assert_eq!(info["show"], "Steeped");
    assert_eq!(info["studio"], "live");
    // The live pass buys a token for the studio room, standing by until a press.
    let (code, t) = token(&w, live).await;
    assert_eq!(code, StatusCode::CREATED, "{t}");
    assert_eq!(t["room"], "studio");
    let tc = keys().verify(t["token"].as_str().unwrap()).unwrap();
    assert_eq!(
        tc.standby_until_ms(),
        Some(weavesignal_proto::UNTIL_PUT_ON_AIR)
    );

    // Someone who was not moved is sent no move.
    assert!(r.move_of(session, "morag", now).is_none());
    assert!(r.move_of(session, "gregor", now).is_some());
}

/// The live desk with the moved people standing by, and Hamish (the outgoing presenter) on air.
fn at_live_desk(w: &World, host: &str) {
    let p = |id: &str, role: &str, on: bool| {
        json!({"identity": id, "name": "", "role": role, "on_air": on, "mute": false,
            "talk": false, "present": true, "linked": true})
    };
    w.live.lock().unwrap().people = vec![
        p("hamish", "presenter", true),
        p(host, "presenter", false),
        p("gregor", "guest", false),
        p("kirsty", "guest", false),
    ];
}

fn live_person(w: &World, id: &str) -> Value {
    w.live
        .lock()
        .unwrap()
        .people
        .iter()
        .find(|p| p["identity"] == id)
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn the_opening_is_applied_when_the_host_goes_on_air_and_only_once() {
    let now = now_ms() as i64;
    let w = world_airing(now + 600_000);
    let host = rehearsing(&w);
    open_steeped(&w).await;
    let (code, _) = post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    at_live_desk(&w, &host);
    w.state.hub.poll_once().await;
    // Everyone moved stands by: a look applies nothing.
    let look = || apply_openings(&w.state, model::Moment::Look, None);
    assert_eq!(look().await, 0);
    assert_eq!(live_person(&w, "gregor")["on_air"], false);
    // A press of Kirsty's On key (a moved guest) is not the moment while the host was moved.
    let (code, _) = post(
        &w.app,
        "/api/v1/desk/commands",
        ADMIN,
        json!({"op": "on_air", "identity": "kirsty", "on": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(live_person(&w, "gregor")["on_air"], false);
    // The host's own On key (or the handover's first press): applied at once. Gregor was on
    // and muted in rehearsal: muted first, then on air, with the rules' fade.
    w.live.lock().unwrap().lines.clear();
    let (code, _) = post(
        &w.app,
        "/api/v1/desk/commands",
        ADMIN,
        json!({"op": "on_air", "identity": host, "on": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let g = live_person(&w, "gregor");
    assert_eq!(
        (g["on_air"].as_bool(), g["mute"].as_bool()),
        (Some(true), Some(true))
    );
    {
        let f = w.live.lock().unwrap();
        let ops: Vec<(String, String)> = f
            .lines
            .iter()
            .filter(|l| l["op"] == "mute" || l["op"] == "on_air")
            .map(|l| {
                (
                    l["op"].as_str().unwrap().to_owned(),
                    l["identity"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            ops,
            [
                ("on_air".to_owned(), host.clone()),
                ("mute".to_owned(), "gregor".to_owned()),
                ("on_air".to_owned(), "gregor".to_owned()),
            ]
        );
        let on = f
            .lines
            .iter()
            .find(|l| l["op"] == "on_air" && l["identity"] == "gregor")
            .unwrap();
        assert_eq!(on["fade_ms"], 2_000);
    }
    // Once: taking Gregor off and looking again does not put him back.
    let (code, _) = post(
        &w.app,
        "/api/v1/desk/commands",
        ADMIN,
        json!({"op": "on_air", "identity": "gregor", "on": false}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(look().await, 0);
    assert_eq!(
        apply_openings(
            &w.state,
            model::Moment::TakeAir,
            Some(w.state.hub.rehearsal.get().unwrap().moves()[0].show_id)
        )
        .await,
        0
    );
    assert_eq!(live_person(&w, "gregor")["on_air"], false);
}

#[tokio::test]
async fn taking_the_air_from_automation_for_the_show_is_its_moment_and_absent_people_are_skipped() {
    let now = now_ms() as i64;
    let w = world_airing(now - 60_000);
    let host = rehearsing(&w);
    open_steeped(&w).await;
    let (code, _) = post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    at_live_desk(&w, &host);
    // Gregor's link is down at the live desk.
    for p in w.live.lock().unwrap().people.iter_mut() {
        if p["identity"] == "gregor" {
            p["present"] = json!(false);
        }
    }
    w.state.hub.poll_once().await;
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    let steeped = r.moves()[0].show_id;
    // The air taken back while another show is on by the schedule: not this opening's moment.
    assert_eq!(
        apply_openings(&w.state, model::Moment::TakeAir, Some(Uuid::new_v4())).await,
        0
    );
    // For Steeped: applied, the host put on, Gregor skipped (not here), Kirsty left (off).
    assert_eq!(
        apply_openings(&w.state, model::Moment::TakeAir, Some(steeped)).await,
        1
    );
    assert_eq!(live_person(&w, &host)["on_air"], true);
    assert_eq!(live_person(&w, "gregor")["on_air"], false);
    assert_eq!(live_person(&w, "kirsty")["on_air"], false);
    assert!(
        r.moves()[0]
            .opening
            .as_ref()
            .unwrap()
            .applied_at_ms
            .is_some()
    );
}

#[tokio::test]
async fn a_second_move_for_the_same_show_replaces_the_first_opening() {
    let now = now_ms() as i64;
    let w = world_airing(now + 600_000);
    rehearsing(&w);
    open_steeped(&w).await;
    assert_eq!(
        post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    let first = r.moves()[0].id;
    open_steeped(&w).await;
    assert_eq!(
        post(&w.app, "/api/v1/rehearsal/move", ADMIN, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let moves = r.moves();
    assert_eq!(moves.len(), 1);
    assert_ne!(moves[0].id, first);
    // The first's people are no longer offered its move.
    assert!(r.move_of(first, "gregor", now).is_none());
}

// --- 1.3.1: strips given by the station ----------------------------------------------------------

fn desk_person(id: &str) -> Value {
    json!({"identity": id, "name": "", "role": "guest", "on_air": false, "mute": false,
        "talk": false, "present": true, "linked": true})
}

async fn slots_in_view(w: &World) -> Vec<(String, u64)> {
    let r = w.state.hub.rehearsal.get().unwrap().clone();
    r.poll_once(Default::default()).await;
    let (code, view) = send(&w.app, "GET", "/api/v1/rehearsal", ADMIN, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    view["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["identity"].as_str().unwrap().to_owned(),
                p["slot"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_station_gives_each_person_a_strip_and_keeps_it_while_they_are_listed() {
    let w = world();
    open_steeped(&w).await;
    w.rehearsal.lock().unwrap().people = vec![
        desk_person("eilidh"),
        desk_person("gregor"),
        desk_person("kirsty"),
    ];
    assert_eq!(
        slots_in_view(&w).await,
        [
            ("eilidh".into(), 1),
            ("gregor".into(), 2),
            ("kirsty".into(), 3)
        ]
    );
    // Gregor leaves and Morag arrives first in the desk's list: the others keep their strips,
    // and she takes the lowest free one.
    w.rehearsal.lock().unwrap().people = vec![
        desk_person("morag"),
        desk_person("eilidh"),
        desk_person("kirsty"),
    ];
    assert_eq!(
        slots_in_view(&w).await,
        [
            ("morag".into(), 2),
            ("eilidh".into(), 1),
            ("kirsty".into(), 3)
        ]
    );
    // Someone who goes and comes back is placed afresh, in the lowest free strip.
    w.rehearsal.lock().unwrap().people = vec![desk_person("kirsty")];
    assert_eq!(slots_in_view(&w).await, [("kirsty".into(), 3)]);
    w.rehearsal.lock().unwrap().people = vec![desk_person("kirsty"), desk_person("eilidh")];
    assert_eq!(
        slots_in_view(&w).await,
        [("kirsty".into(), 3), ("eilidh".into(), 1)]
    );
}

// --- 1.3.1: whether the asker may run the session ----------------------------------------------------

#[tokio::test]
async fn the_view_says_whether_the_asker_may_run_the_open_session() {
    let w = world();
    let staff = &w.state.staff;
    let presenter = staff.signed_in_as_for_test(Staff::Presenter, w.presenter);
    let other = staff.signed_in_for_test(Staff::Presenter);
    let producer = staff.signed_in_for_test(Staff::Producer);
    let may = |code_view: (StatusCode, Value)| {
        assert_eq!(code_view.0, StatusCode::OK, "{}", code_view.1);
        code_view.1["may_run"].as_bool().unwrap()
    };
    let get = |bearer: String| {
        let app = w.app.clone();
        async move { send(&app, "GET", "/api/v1/rehearsal", &bearer, Value::Null).await }
    };
    // Nothing open: nobody has anything to run.
    for who in [ADMIN, presenter.as_str(), producer.as_str()] {
        assert!(!may(get(who.to_owned()).await), "{who}");
    }
    let (code, view) = post(
        &w.app,
        "/api/v1/rehearsal/open",
        &presenter,
        json!({"show": "steeped"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{view}");
    assert_eq!(
        view["may_run"], true,
        "the answer to Open is the opener's own"
    );
    // The show's presenter, a producer and the station key; not another show's presenter.
    assert!(may(get(presenter.clone()).await));
    assert!(may(get(producer.clone()).await));
    assert!(may(get(ADMIN.to_owned()).await));
    assert!(!may(get(other.clone()).await));
    // The commands' and the end's answers carry the asker's own as well.
    let (code, view) = post(
        &w.app,
        "/api/v1/rehearsal/commands",
        &presenter,
        json!({"op": "mute", "identity": "eilidh", "on": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(view["may_run"], true);
    let (code, view) = post(&w.app, "/api/v1/rehearsal/end", &presenter, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert_eq!(view["may_run"], false, "nothing is open any more");
}

// --- 1.3.1: taking one person out -------------------------------------------------------------------

#[tokio::test]
async fn one_person_can_be_taken_out_and_their_link_is_ended_for_the_session() {
    let w = world();
    let staff = &w.state.staff;
    let presenter = staff.signed_in_as_for_test(Staff::Presenter, w.presenter);
    let other = staff.signed_in_for_test(Staff::Presenter);
    let remove = |who: &str, id: &str| {
        let app = w.app.clone();
        let (who, id) = (who.to_owned(), id.to_owned());
        async move {
            post(
                &app,
                "/api/v1/rehearsal/remove",
                &who,
                json!({"identity": id}),
            )
            .await
        }
    };
    // No session, nothing to take anyone out of.
    let (code, _) = remove(ADMIN, "gregor").await;
    assert_eq!(code, StatusCode::CONFLICT);
    open_steeped(&w).await;
    let (_, gregor) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "name": "Gregor Stewart", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    let (_, kirsty) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "name": "Kirsty Wilson", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    let (gid, kid) = (
        gregor["identity"].as_str().unwrap().to_owned(),
        kirsty["identity"].as_str().unwrap().to_owned(),
    );
    let (gpass, kpass) = (
        gregor["pass"].as_str().unwrap().to_owned(),
        kirsty["pass"].as_str().unwrap().to_owned(),
    );
    assert_eq!(token(&w, &gpass).await.0, StatusCode::CREATED);
    w.rehearsal.lock().unwrap().people = vec![desk_person(&gid), desk_person(&kid)];
    w.state
        .hub
        .rehearsal
        .get()
        .unwrap()
        .poll_once(Default::default())
        .await;

    // Only whoever may run the session; the rehearsal desk is not a person; strangers are 404.
    let (code, _) = remove(&other, &gid).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = remove(ADMIN, "desk-rehearsal").await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    let (code, _) = remove(ADMIN, "nobody").await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Gregor's link is ended; Kirsty's is not; the session carries on.
    let (code, view) = remove(&presenter, &gid).await;
    assert_eq!(code, StatusCode::OK, "{view}");
    assert!(view["session"].is_object());
    assert_eq!(view["may_run"], true);
    let (code, body) = token(&w, &gpass).await;
    assert_eq!(code, StatusCode::GONE, "{body}");
    let (_, info) = send(&w.app, "GET", "/api/v1/pass", &gpass, Value::Null).await;
    assert_eq!(info["ended"], true);
    assert_eq!(token(&w, &kpass).await.0, StatusCode::CREATED);
    // A fresh invitation is a new person, and works.
    let (_, again) = invite(
        &w,
        ADMIN,
        json!({"role": "guest", "name": "Gregor Stewart", "studio": "rehearsal", "show": "steeped"}),
    )
    .await;
    assert_eq!(
        token(&w, again["pass"].as_str().unwrap()).await.0,
        StatusCode::CREATED
    );
}
