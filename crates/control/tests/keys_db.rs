// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows against a real PostgreSQL (1.3.2): keys made, found, changed, paused, revoked and
//! replaced; redeem and renew with their tokens (valid, expired, revoked, wrong show, replayed);
//! the limits under concurrent redeems; families, the CSV export and the leak report; the
//! lost-key request; the events; the API token scope and the roles; and what the database holds
//! (never a key's text). Ignored unless asked for (`just test-db`).

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use weavesignal_control::access;
use weavesignal_control::access_token::{self, TokenError, TokenMedia, TokenSigner};
use weavesignal_control::api::{Api, ApiConfig, layer};
use weavesignal_control::apikey::ApiKey;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, JoinConfig, KeysConfig, PublishingConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::events;
use weavesignal_control::hub::Hub;
use weavesignal_control::shows::{self, ShowsState};
use weavesignal_control::staff::Staff;
use weavesignal_control::studio::Studio;
use weavesignal_control::zone::StationZone;
use weavesignal_model::listener_keys_db as keysdb;
use weavesignal_model::{Station, StationId, ensure_station};

const KEY: &str = "keys-db-test-station-key";
const TOKEN_KEY: &str = "0123456789abcdef0123456789abcdef";

struct Plane {
    app: Router,
    staff: Arc<Staff>,
}

fn keys_config() -> KeysConfig {
    KeysConfig {
        // The limits are tested on their own; the other tests run free of them.
        redeem_per_minute: 0,
        failures_per_ten_minutes: 0,
        lost_per_hour: 0,
        ..KeysConfig::default()
    }
}

async fn plane(pool: &sqlx::PgPool, station: &Station, token_key: &str, keys: KeysConfig) -> Plane {
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Arc::new(
        Api::new(
            Some(pool.clone()),
            station.id,
            Some(ApiKey::parse(token_key).unwrap()),
            ApiConfig {
                public_per_minute: 0,
                token_per_minute: 0,
                streams_per_address: 0,
                ..ApiConfig::default()
            },
            true,
        )
        .with_keys(keys),
    );
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    staff.restore(station.id).await;
    staff.api.restore().await;
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        hub,
        join: JoinConfig {
            forwarded_for: true,
            public_url: "https://kettle.example".into(),
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
        admin_token: Some(KEY.into()),
        staff: staff.clone(),
        publishing: PublishingConfig::default(),
        zone: Arc::new(StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: None,
    })));
    Plane {
        app: layer(app, staff.clone()),
        staff,
    }
}

async fn setup(name: &str) -> (sqlx::PgPool, Station) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(&pool, &Station::new(StationId::new_random(), name).unwrap())
        .await
        .unwrap();
    // Every station here has a keyed show on air, so a key for every keyed show has a window
    // to be redeemed for (1.3.2: a token is for one show's window).
    show(&pool, &station, "the-keyed-hour").await;
    (pool, station)
}

/// A show with no rule (public) and nothing on the schedule.
async fn plain_show(pool: &sqlx::PgPool, station: &Station, slug: &str) -> Uuid {
    let input: weavesignal_model::shows::ShowInput =
        serde_json::from_value(json!({"name": slug, "colour_slot": 2})).unwrap();
    weavesignal_model::shows::put_show(pool, station.id, slug, &input)
        .await
        .unwrap();
    weavesignal_model::shows::get_show(pool, station.id, slug)
        .await
        .unwrap()
        .unwrap()
        .id
}

async fn send_h(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    addr: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        req = req.header("authorization", format!("Bearer {b}"));
    }
    if let Some(a) = addr {
        req = req.header("x-forwarded-for", a);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let (code, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, headers, String::from_utf8_lossy(&bytes).into_owned())
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let (code, _, text) = send_h(app, method, uri, bearer, None, body).await;
    (code, serde_json::from_str(&text).unwrap_or_default())
}

/// A request from a listener at `addr`.
async fn listen(app: &Router, uri: &str, addr: &str, body: Value) -> (StatusCode, Value) {
    let (code, _, text) = send_h(app, "POST", uri, None, Some(addr), body).await;
    (code, serde_json::from_str(&text).unwrap_or_default())
}

fn ok(r: (StatusCode, Value)) -> Value {
    assert!(r.0.is_success(), "{} {}", r.0, r.1);
    r.1
}

async fn make(p: &Plane, body: Value) -> Value {
    let r = send(&p.app, "POST", "/api/v1/listener-keys", Some(KEY), body).await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    r.1
}

/// A keyed show on air now (1.3.2): its rule is `key` and an episode runs from an hour ago for
/// a day, so a key that opens it is redeemed for a token for this window.
async fn show(pool: &sqlx::PgPool, station: &Station, slug: &str) -> Uuid {
    let id = plain_show(pool, station, slug).await;
    weavesignal_model::listener_keys_db::set_access(
        pool,
        station.id,
        &weavesignal_model::listener_keys_db::Access {
            show_id: id,
            rule: "key".into(),
            opens_at_ms: None,
            slate: "announcement".into(),
            slate_note: String::new(),
            updated_ms: 0,
            updated_by: String::new(),
        },
        "test",
    )
    .await
    .unwrap();
    let series = Uuid::new_v4();
    sqlx::query("INSERT INTO series (id, station_id, show_id, number) VALUES ($1, $2, $3, 1)")
        .bind(series)
        .bind(station.id.as_uuid())
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at, minutes)
         VALUES ($1, $2, $3, 1, now() - interval '1 hour', 1440)",
    )
    .bind(Uuid::new_v4())
    .bind(station.id.as_uuid())
    .bind(series)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn signer() -> Box<dyn TokenSigner> {
    Box::new(access_token::P256Signer::from_secret(
        &ApiKey::parse(TOKEN_KEY).unwrap().listener_token_key(),
    ))
}

/// Whether a token opens `show` for `want` (as an edge would judge it, less the clock).
fn opens(c: &access_token::Claims, show: Uuid, want: TokenMedia) -> bool {
    c.show == show.to_string() && c.media.opens(want)
}

fn device_of(c: &access_token::Claims) -> Uuid {
    Uuid::parse_str(c.device.as_deref().expect("a device")).unwrap()
}

fn claims_of(token: &str) -> Result<access_token::Claims, TokenError> {
    let now = weavesignal_control::hub::now_ms() as i64 / 1000;
    access_token::decode(signer().as_ref(), token, now, 0)
}

fn now_ms() -> i64 {
    weavesignal_control::hub::now_ms() as i64
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_key_is_made_found_changed_extended_paused_and_revoked_and_the_database_never_holds_its_text()
 {
    let (pool, station) = setup("Keys FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let steeped = show(&pool, &station, "steeped").await;

    let made = make(
        &p,
        json!({"label": "Ishbel Fraser", "email": "Ishbel@Example.org", "shows": ["steeped"], "note": "patron 41"}),
    )
    .await;
    let id = made["key"]["id"].as_str().unwrap().to_owned();
    let text = made["text"].as_str().unwrap().to_owned();
    assert_eq!(text.len(), 14, "{text}");
    assert_eq!(made["key"]["kind"], "personal");
    assert_eq!(made["key"]["media"], "both");
    assert_eq!(made["key"]["max_devices"], 2);
    assert_eq!(made["key"]["all_shows"], false);
    assert_eq!(made["key"]["shows"], json!([steeped]));
    assert_eq!(made["key"]["has_email"], true);
    assert_eq!(
        made["link"],
        format!("https://kettle.example/listen/#key={text}")
    );
    // Nothing the list says holds the text or the email.
    let (code, list) = send(
        &p.app,
        "GET",
        "/api/v1/listener-keys",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list["total"], 1);
    let raw = list.to_string();
    assert!(
        !raw.contains(&text) && !raw.contains("example.org"),
        "{raw}"
    );

    // The database holds the text sealed and hashed, never plain; the email the same.
    let row: (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT text_hash, text_sealed, email_sealed FROM listener_key WHERE id = $1",
    )
    .bind(Uuid::parse_str(&id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let plain = text.replace('-', "");
    for col in [&row.0, &row.1, &row.2] {
        let s = String::from_utf8_lossy(col);
        assert!(!s.contains(&plain) && !s.contains("example.org"));
    }

    // Found by what a person types: any case, any dashes.
    let typed = text.to_lowercase().replace('-', " ");
    let (code, found) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/find",
        Some(KEY),
        json!({"text": typed}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{found}");
    assert_eq!(found["id"], id);
    let (code, _) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/find",
        Some(KEY),
        json!({"text": "AAAA-AAAA-AAAA"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Changed in place: the text stays the same.
    let (code, changed) = send(
        &p.app,
        "PATCH",
        &format!("/api/v1/listener-keys/{id}"),
        Some(KEY),
        json!({"label": "Ishbel M. Fraser", "max_devices": null, "valid_until_ms": now_ms() + 3_600_000}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    assert_eq!(changed["label"], "Ishbel M. Fraser");
    assert!(changed["max_devices"].is_null());
    let until = changed["valid_until_ms"].as_i64().unwrap();
    let (_, shown) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/text"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(shown["text"], text);
    assert_eq!(shown["email"], "Ishbel@Example.org");

    // Extended by days from the end; a key with no end has nothing to extend.
    let (code, ext) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/extend"),
        Some(KEY),
        json!({"by_days": 7}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{ext}");
    assert_eq!(ext["valid_until_ms"], until + 7 * 86_400_000);
    let (_, bare) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys",
        Some(KEY),
        json!({"label": "No end"}),
    )
    .await;
    let bare_id = bare["key"]["id"].as_str().unwrap();
    let (code, e) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{bare_id}/extend"),
        Some(KEY),
        json!({"by_days": 7}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{e}");

    // Paused, resumed, revoked; revoked is final.
    let mut events = p.staff.api.events.subscribe();
    for (verb, state) in [("pause", "paused"), ("resume", "active")] {
        let (code, k) = send(
            &p.app,
            "POST",
            &format!("/api/v1/listener-keys/{id}/{verb}"),
            Some(KEY),
            json!({"reason": "late payment"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{k}");
        assert_eq!(k["state"], state);
    }
    let (_, k) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/revoke"),
        Some(KEY),
        json!({"reason": "refund"}),
    )
    .await;
    assert_eq!(k["state"], "revoked");
    assert_eq!(k["state_reason"], "refund");
    // Revoking again changes nothing and sends nothing more.
    let (code, _) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/revoke"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let e = events.try_recv().expect("key.ended");
    assert_eq!(e.kind, "key.ended");
    assert_eq!(e.data["reason"], "revoked");
    assert_eq!(e.data["label"], "Ishbel M. Fraser");
    assert!(!e.data.to_string().contains(&plain) && !e.data.to_string().contains("example.org"));
    assert!(events.try_recv().is_err(), "one ended event");
    for (uri, body) in [
        ("pause", json!({})),
        ("resume", json!({})),
        ("extend", json!({"by_days": 1})),
    ] {
        let (code, _) = send(
            &p.app,
            "POST",
            &format!("/api/v1/listener-keys/{id}/{uri}"),
            Some(KEY),
            body,
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{uri}");
    }
    let (code, _) = send(
        &p.app,
        "PATCH",
        &format!("/api/v1/listener-keys/{id}"),
        Some(KEY),
        json!({"note": "x"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);

    // The log names who did what, with no text and no values of changed fields.
    let (_, log) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{id}/log"),
        Some(KEY),
        Value::Null,
    )
    .await;
    let actions: Vec<_> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for a in [
        "created",
        "changed",
        "text_shown",
        "extended",
        "paused",
        "resumed",
        "revoked",
    ] {
        assert!(actions.contains(&a), "{a} in {actions:?}");
    }
    let raw = log.to_string();
    assert!(
        !raw.contains(&plain) && !raw.contains("Ishbel") && !raw.contains("example.org"),
        "{raw}"
    );
    assert!(raw.contains("station key"));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_key_is_redeemed_for_a_token_that_opens_its_shows_and_is_renewed_and_refused_when_wrong()
{
    let (pool, station) = setup("Redeem FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let a = show(&pool, &station, "friday-supporters").await;
    let b = show(&pool, &station, "morning-kettle").await;
    let made = make(
        &p,
        json!({"label": "Ishbel", "shows": ["friday-supporters"], "media": "sound"}),
    )
    .await;
    let text = made["text"].as_str().unwrap().to_owned();

    // Typed lower case with spaces; a first visit has no device.
    let typed = text.to_lowercase().replace('-', " ");
    let (code, r) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.5",
        json!({"key": typed}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let token = r["token"].as_str().unwrap().to_owned();
    let device = Uuid::parse_str(r["device"].as_str().unwrap()).unwrap();
    // The name staff gave the key is theirs: the holder is not sent it (1.3.3).
    assert!(r["key"].get("label").is_none(), "{r}");
    assert!(!r.to_string().contains("Ishbel"), "{r}");
    assert_eq!(r["key"]["shows"][0]["slug"], "friday-supporters");
    assert_eq!(r["renew_in_s"], 60);
    assert!(r["key"]["ends_soon"] == false);
    // The key's text is never in what the holder is sent back.
    assert!(
        !r.to_string()
            .replace('-', "")
            .contains(&text.replace('-', ""))
    );

    // The token opens that show's sound and not its picture, nor another show.
    let c = claims_of(&token).expect("a good token");
    assert!(opens(&c, a, TokenMedia::Sound));
    assert!(!opens(&c, a, TokenMedia::Video));
    assert!(!opens(&c, b, TokenMedia::Sound));
    assert_eq!(device_of(&c), device);
    // It is for the show's window (an hour ago for a day, with its margins), and the answer
    // names the show and the keyed addresses with the token in their path.
    assert!(c.exp * 1000 > now_ms() + 22 * 3_600_000, "{}", c.exp);
    assert_eq!(r["show"]["slug"], "friday-supporters");
    assert_eq!(
        r["urls"]["opus"],
        format!("/listen/k/{a}/{token}/programme-keyed.opus")
    );
    // A sound key's HLS is the sound's playlist: it is refused the picture's (19f).
    assert_eq!(r["urls"]["hls"], format!("/hls/k/{a}/{token}/audio.m3u8"));

    // Asking for another show by name is refused.
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.5",
        json!({"key": text, "show": "morning-kettle", "device": device}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert_eq!(e["code"], "does_not_open_show");
    // A refusal names nothing of the key's own either (1.3.3).
    assert!(
        e.get("label").is_none() && !e.to_string().contains("Ishbel"),
        "{e}"
    );
    // and the right one is not.
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.5",
        json!({"key": text, "show": "friday-supporters", "device": device}),
    )
    .await);

    // Renewed: a fresh token for the same device.
    let (code, r2) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.5",
        json!({"token": token}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{r2}");
    assert_eq!(
        device_of(&claims_of(r2["token"].as_str().unwrap()).unwrap()),
        device
    );
    // A changed token, another signer's, and rubbish are not tokens.
    let mut forged = token.clone();
    forged.push('x');
    for t in [forged.as_str(), "nonsense", ""] {
        let (code, e) = listen(
            &p.app,
            "/api/v1/keyed/renew",
            "203.0.113.5",
            json!({"token": t}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{t}");
        assert_eq!(e["code"], "token_invalid");
    }
    let other = access_token::P256Signer::from_secret(&[9; 32]);
    let strange = other.mint(&claims_of(&token).unwrap());
    let (code, _) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.5",
        json!({"token": strange}),
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    // An ended token is 401 token_ended; the player redeems again.
    let mut old = claims_of(&token).unwrap();
    old.nbf -= 200_000;
    old.exp = now_ms() / 1000 - 10;
    let ended = signer().mint(&old);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.5",
        json!({"token": ended}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("token_ended"))
    );
    // A token for a device the key never had (a made-up claim, validly signed) cannot renew.
    let mut ghost = claims_of(&token).unwrap();
    ghost.device = Some(Uuid::new_v4().to_string());
    let ghost = signer().mint(&ghost);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.5",
        json!({"token": ghost}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("device_signed_out"))
    );

    // Not a key, and not a key's shape: the same words.
    for k in ["AAAA-BBBB-CCCC", "nonsense", ""] {
        let (code, e) = listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "203.0.113.5",
            json!({"key": k}),
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{k}");
        assert_eq!(e["code"], "not_recognised");
    }
}

/// A key for the picture carries the show's sound (the operator, 2026-11-25): its token opens
/// the keyed streams and the HLS audio rendition as well as the picture, exactly as a key for
/// both does, and the redeem hands it the sound's addresses beside the picture's.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_video_key_opens_the_picture_and_the_shows_sound() {
    let (pool, station) = setup("Picture FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let a = show(&pool, &station, "friday-supporters").await;
    for (label, media) in [("Ishbel", "video"), ("Mairi", "both")] {
        let made = make(
            &p,
            json!({"label": label, "shows": ["friday-supporters"], "media": media}),
        )
        .await;
        // The key is kept as it was made: staff see the kind they chose.
        assert_eq!(made["key"]["media"], media);
        let (code, r) = listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "203.0.113.6",
            json!({"key": made["text"]}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{media}: {r}");
        assert_eq!(r["key"]["media"], media);
        let token = r["token"].as_str().unwrap();
        let c = claims_of(token).expect("a good token");
        assert!(opens(&c, a, TokenMedia::Video), "{media}: the picture");
        assert!(opens(&c, a, TokenMedia::Sound), "{media}: the sound");
        // What the origin and an edge ask of each address under the keyed HLS.
        for rest in [
            "live.m3u8",
            "v0/seg00001.m4s",
            "audio.m3u8",
            "audio/seg00001.m4s",
        ] {
            assert!(
                opens(&c, a, weavesignal_keyed::hls_media(rest)),
                "{media}: {rest}"
            );
        }
        assert_eq!(
            r["urls"]["opus"],
            format!("/listen/k/{a}/{token}/programme-keyed.opus"),
            "{media}"
        );
        assert_eq!(
            r["urls"]["hls"],
            format!("/hls/k/{a}/{token}/live.m3u8"),
            "{media}"
        );
        // The renewal keeps it so.
        let (code, r2) = listen(
            &p.app,
            "/api/v1/keyed/renew",
            "203.0.113.6",
            json!({"token": token}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{media}: {r2}");
        let c2 = claims_of(r2["token"].as_str().unwrap()).unwrap();
        assert!(
            opens(&c2, a, TokenMedia::Sound) && opens(&c2, a, TokenMedia::Video),
            "{media}: renewed"
        );
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_window_a_pause_and_a_revoke_bite_at_the_next_renewal_and_a_token_never_outlives_the_key()
{
    let (pool, station) = setup("Window FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let friday = show(&pool, &station, "friday").await;

    // Not started: the answer says when it opens.
    let future = now_ms() + 86_400_000;
    let early = make(&p, json!({"label": "Early", "valid_from_ms": future})).await;
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.1",
        json!({"key": early["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("not_yet_valid"))
    );
    assert_eq!(e["from_ms"], future);

    // A key that ends in 30 seconds gets a token that ends there too, not in two minutes.
    let soon = now_ms() + 30_000;
    let brief = make(&p, json!({"label": "Brief", "valid_until_ms": soon})).await;
    let (_, r) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.1",
        json!({"key": brief["text"]}),
    )
    .await;
    let c = claims_of(r["token"].as_str().unwrap()).unwrap();
    assert!(c.exp <= soon / 1000, "{} > {}", c.exp, soon / 1000);
    assert_eq!(r["key"]["ends_soon"], true);

    // Ended (the window set in the past directly, as time would): refused, with when.
    let id = Uuid::parse_str(brief["key"]["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE listener_key SET valid_from = now() - interval '2 hours', valid_until = now() - interval '1 hour' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "198.51.100.1",
        json!({"token": r["token"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("ended"))
    );
    assert!(e["until_ms"].as_i64().unwrap() < now_ms());

    // Paused and revoked: the holder is cut off at the next renewal; resume brings it back.
    let k = make(&p, json!({"label": "Cut"})).await;
    let kid = k["key"]["id"].as_str().unwrap();
    let (_, r) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.2",
        json!({"key": k["text"]}),
    )
    .await;
    let token = r["token"].as_str().unwrap().to_owned();
    ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{kid}/pause"),
        Some(KEY),
        Value::Null,
    )
    .await);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "198.51.100.2",
        json!({"token": token}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("paused"))
    );
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.2",
        json!({"key": k["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("paused"))
    );
    ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{kid}/resume"),
        Some(KEY),
        Value::Null,
    )
    .await);
    ok(listen(
        &p.app,
        "/api/v1/keyed/renew",
        "198.51.100.2",
        json!({"token": token}),
    )
    .await);
    ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{kid}/revoke"),
        Some(KEY),
        Value::Null,
    )
    .await);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "198.51.100.2",
        json!({"token": token}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("revoked"))
    );

    // A key shortened below now is refused at once; a key changed to open other shows follows.
    let k = make(&p, json!({"label": "Moved", "shows": ["friday"]})).await;
    let kid = k["key"]["id"].as_str().unwrap();
    let (_, r) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.3",
        json!({"key": k["text"]}),
    )
    .await;
    let tok = r["token"].as_str().unwrap();
    ok(send(
        &p.app,
        "PATCH",
        &format!("/api/v1/listener-keys/{kid}"),
        Some(KEY),
        json!({"all_shows": true}),
    )
    .await);
    let (_, r2) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "198.51.100.3",
        json!({"token": tok}),
    )
    .await;
    // The token was for Friday and stays for Friday: a renewal is for the same show.
    let c = claims_of(r2["token"].as_str().unwrap()).unwrap();
    assert_eq!(c.show, friday.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn device_listen_and_people_limits_hold_when_many_redeem_at_once() {
    let (pool, station) = setup("Limits FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;

    // Two devices at a time, eight phones at once: exactly two get in.
    let k = make(&p, json!({"label": "Two devices"})).await;
    let mut tasks = Vec::new();
    for i in 0..8 {
        let app = p.app.clone();
        let key = k["text"].clone();
        tasks.push(tokio::spawn(async move {
            listen(
                &app,
                "/api/v1/keyed/redeem",
                &format!("203.0.113.{i}"),
                json!({"key": key}),
            )
            .await
        }));
    }
    let mut granted = 0;
    let mut refused = Vec::new();
    for t in tasks {
        let (code, body) = t.await.unwrap();
        if code == StatusCode::OK {
            granted += 1;
        } else {
            refused.push((code, body));
        }
    }
    assert_eq!(granted, 2, "{refused:?}");
    assert_eq!(refused.len(), 6);
    for (code, body) in &refused {
        assert_eq!(*code, StatusCode::CONFLICT);
        assert_eq!(body["code"], "too_many_devices");
        assert!(body.get("label").is_none(), "{body}");
        assert_eq!(body["devices"].as_array().unwrap().len(), 2);
        assert_eq!(body["max_devices"], 2);
    }
    let kid = k["key"]["id"].as_str().unwrap();
    let (_, devices) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{kid}/devices"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(devices["devices"].as_array().unwrap().len(), 2);

    // The holder signs one out by the key; its place is free at once and its token is refused.
    let (_, r) = listen(
        &p.app,
        "/api/v1/keyed/devices",
        "203.0.113.50",
        json!({"key": k["text"]}),
    )
    .await;
    let first = r["devices"][0]["id"].as_str().unwrap().to_owned();
    let (code, _) = listen(
        &p.app,
        "/api/v1/keyed/devices/end",
        "203.0.113.50",
        json!({"key": k["text"], "device": first}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.60",
        json!({"key": k["text"]}),
    )
    .await);

    // Listens: three, not used up by reloads or renewals.
    let l = make(
        &p,
        json!({"label": "Event ticket", "max_listens": 3, "max_devices": null}),
    )
    .await;
    let lid = l["key"]["id"].as_str().unwrap().to_owned();
    let mut device = None;
    let mut token = String::new();
    for i in 0..3 {
        let r = ok(listen(
            &p.app,
            "/api/v1/keyed/redeem",
            &format!("192.0.2.{i}"),
            json!({"key": l["text"]}),
        )
        .await);
        if i == 0 {
            device = Some(r["device"].clone());
            token = r["token"].as_str().unwrap().to_owned();
        }
    }
    let (_, k3) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{lid}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(k3["listens_used"], 3);
    // A reload by the first device (within the gap) is the same listen; a renewal is none.
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.0",
        json!({"key": l["text"], "device": device}),
    )
    .await);
    ok(listen(
        &p.app,
        "/api/v1/keyed/renew",
        "192.0.2.0",
        json!({"token": token}),
    )
    .await);
    let (_, k3) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{lid}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(k3["listens_used"], 3);
    // The fourth listen, on a new device, is refused; the listen in progress is not cut.
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.9",
        json!({"key": l["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::CONFLICT, Some("listens_used_up"))
    );
    ok(listen(
        &p.app,
        "/api/v1/keyed/renew",
        "192.0.2.0",
        json!({"token": token}),
    )
    .await);
    // Back after the gap, the same device starts a fourth listen and is refused too.
    sqlx::query(
        "UPDATE listener_key_device SET last_seen_at = now() - interval '1 hour' WHERE key_id = $1",
    )
    .bind(Uuid::parse_str(&lid).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.0",
        json!({"key": l["text"], "device": device}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::CONFLICT, Some("listens_used_up"))
    );

    // People: a group key for two people, devices unlimited.
    let g = make(
        &p,
        json!({"label": "Choir", "kind": "group", "max_people": 2, "max_devices": null}),
    )
    .await;
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.20",
        json!({"key": g["text"]}),
    )
    .await);
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.21",
        json!({"key": g["text"]}),
    )
    .await);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.22",
        json!({"key": g["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::CONFLICT, Some("people_limit"))
    );

    // A signed-out device cannot renew.
    let s = make(&p, json!({"label": "Signed out"})).await;
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "192.0.2.30",
        json!({"key": s["text"]}),
    )
    .await);
    let sid = s["key"]["id"].as_str().unwrap();
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!(
            "/api/v1/listener-keys/{sid}/devices/{}",
            r["device"].as_str().unwrap()
        ),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "192.0.2.30",
        json!({"token": r["token"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("device_signed_out"))
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn replace_revokes_one_key_and_issues_another_to_the_same_recipient() {
    let (pool, station) = setup("Replace FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    show(&pool, &station, "patrons").await;
    let fam = ok(send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/families",
        Some(KEY),
        json!({"label": "Autumn patrons",
               "recipients": [
                   {"label": "Ishbel Fraser", "email": "ishbel@example.org"},
                   {"label": "Callum Reid"},
                   {"label": "Morag Campbell", "note": "tier 2"}],
               "template": {"shows": ["patrons"], "max_listens": 10}}),
    )
    .await);
    assert_eq!(fam["family"]["keys"], 3);
    let keys = fam["keys"].as_array().unwrap();
    let ishbel = &keys[0];
    let others: Vec<_> = keys[1..].iter().collect();
    let old_id = ishbel["key"]["id"].as_str().unwrap().to_owned();
    // The leaker's key was used once (a listen).
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.9",
        json!({"key": ishbel["text"]}),
    )
    .await);

    let mut events = p.staff.api.events.subscribe();
    let rep = ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{old_id}/replace"),
        Some(KEY),
        Value::Null,
    )
    .await);
    assert_eq!(rep["old"]["state"], "revoked");
    assert_eq!(rep["old"]["state_reason"], "replaced");
    assert_eq!(rep["new"]["label"], "Ishbel Fraser");
    assert_eq!(rep["new"]["family_id"], fam["family"]["id"]);
    assert_eq!(rep["new"]["replaces_id"], old_id);
    assert_eq!(rep["old"]["replaced_by_id"], rep["new"]["id"]);
    assert_eq!(rep["new"]["max_listens"], 10);
    // The listens already used carry over: a replacement is not a fresh allowance.
    assert_eq!(rep["new"]["listens_used"], 1);
    assert_ne!(rep["text"], ishbel["text"]);
    let e = events.try_recv().expect("key.ended");
    assert_eq!(
        (e.kind, e.data["reason"].as_str()),
        ("key.ended", Some("replaced"))
    );

    // The old text is dead, the new one works; the other recipients' keys are untouched.
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.9",
        json!({"key": ishbel["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("revoked"))
    );
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.9",
        json!({"key": rep["text"]}),
    )
    .await);
    for o in others {
        ok(listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "198.51.100.10",
            json!({"key": o["text"]}),
        )
        .await);
    }
    // The email carried over (the lost-key request still finds the person).
    let (_, shown) = send(
        &p.app,
        "POST",
        &format!(
            "/api/v1/listener-keys/{}/text",
            rep["new"]["id"].as_str().unwrap()
        ),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(shown["email"], "ishbel@example.org");
    // A leaked copy of the old text finds the old key, and so the recipient.
    let (_, found) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/find",
        Some(KEY),
        json!({"text": ishbel["text"]}),
    )
    .await;
    assert_eq!(found["id"], old_id);
    assert_eq!(found["label"], "Ishbel Fraser");
    // A revoked key cannot be replaced again.
    let (code, _) = send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{old_id}/replace"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    // Both keys' logs say so.
    let (_, log) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{old_id}/log"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert!(log.to_string().contains("\"replaced\""));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_family_is_issued_exported_and_traced_when_one_key_is_passed_on() {
    let (pool, station) = setup("Family FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    show(&pool, &station, "quiz").await;

    // Tickets: anonymous keys with a prefix; named ones with a hostile name.
    let fam = ok(send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/families",
        Some(KEY),
        json!({"label": "Quiz tickets", "count": 5, "prefix": "Ticket", "template": {"shows": ["quiz"]}}),
    )
    .await);
    let keys = fam["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 5);
    assert_eq!(keys[0]["key"]["label"], "Ticket 001");
    assert_eq!(keys[4]["key"]["label"], "Ticket 005");
    let texts: std::collections::BTreeSet<_> =
        keys.iter().map(|k| k["text"].as_str().unwrap()).collect();
    assert_eq!(texts.len(), 5);
    let (code, dup) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/families",
        Some(KEY),
        json!({"label": "Quiz tickets", "count": 1, "template": {}}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{dup}");
    let (code, bad) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/families",
        Some(KEY),
        json!({"label": "Broken", "recipients": [{"label": "Fine"}, {"label": "Bad", "email": "not an address"}], "template": {}}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        bad["error"].as_str().unwrap().contains("Recipient 2"),
        "{bad}"
    );
    // A failed family leaves nothing behind.
    let (_, fams) = send(
        &p.app,
        "GET",
        "/api/v1/listener-keys/families",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(fams["families"].as_array().unwrap().len(), 1);

    ok(send(
        &p.app,
        "POST",
        "/api/v1/listener-keys/families",
        Some(KEY),
        json!({"label": "Press", "recipients": [{"label": "=HYPERLINK(\"http://evil\")", "note": "a, \"b\""}], "template": {}}),
    )
    .await);
    let press = {
        let (_, f) = send(
            &p.app,
            "GET",
            "/api/v1/listener-keys/families",
            Some(KEY),
            Value::Null,
        )
        .await;
        f["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["label"] == "Press")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (code, headers, csv) = send_h(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/families/{press}/export"),
        Some(KEY),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv")
    );
    assert!(csv.starts_with("name,email,note,key,link,state\r\n"));
    assert!(csv.contains("\"'=HYPERLINK("), "{csv}");
    assert!(csv.contains("https://kettle.example/listen/#key="));
    // The family's tickets export with every text.
    let tickets = fam["family"]["id"].as_str().unwrap();
    let (_, _, csv) = send_h(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/families/{tickets}/export"),
        Some(KEY),
        None,
        Value::Null,
    )
    .await;
    for t in &texts {
        assert!(csv.contains(t), "{t}");
    }

    // The leak report lists every key of the family.
    let (code, report) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/leaks?family={tickets}"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{report}");
    assert_eq!(report["entries"].as_array().unwrap().len(), 5);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_key_on_many_addresses_scores_high_and_the_report_says_why() {
    let (pool, station) = setup("Leak FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    show(&pool, &station, "leaky").await;
    let fam = ok(send(&p.app, "POST", "/api/v1/listener-keys/families", Some(KEY),
        json!({"label": "Press pack", "count": 3, "template": {"shows": ["leaky"], "max_devices": null}})).await);
    let keys = fam["keys"].as_array().unwrap();
    // Key 2 is passed on: eight addresses and eight devices in the same hour.
    for i in 0..8 {
        ok(listen(
            &p.app,
            "/api/v1/keyed/redeem",
            &format!("203.0.113.{}", 100 + i),
            json!({"key": keys[1]["text"]}),
        )
        .await);
    }
    // Key 0 is one person at home and on the phone.
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.1",
        json!({"key": keys[0]["text"]}),
    )
    .await);
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "198.51.100.2",
        json!({"key": keys[0]["text"]}),
    )
    .await);
    // A key posted in public that holds its default limit (two devices): eight addresses try
    // it and six are turned away. The report still sees all eight.
    let posted = make(&p, json!({"label": "Posted", "shows": ["leaky"]})).await;
    let mut turned_away = 0;
    for i in 0..8 {
        let (code, _) = listen(
            &p.app,
            "/api/v1/keyed/redeem",
            &format!("192.0.2.{}", 100 + i),
            json!({"key": posted["text"]}),
        )
        .await;
        if code == StatusCode::CONFLICT {
            turned_away += 1;
        }
    }
    assert_eq!(turned_away, 6);
    let (_, report) = send(
        &p.app,
        "GET",
        "/api/v1/listener-keys/leaks",
        Some(KEY),
        Value::Null,
    )
    .await;
    let posted_row = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"]["label"] == "Posted")
        .unwrap()
        .clone();
    assert_eq!(posted_row["devices"], 2, "{posted_row}");
    assert_eq!(posted_row["addresses"], 8, "{posted_row}");
    assert_eq!(posted_row["spike"], true);
    assert!(posted_row["score"].as_u64().unwrap() > 0, "{posted_row}");
    assert!(posted_row["reasons"].to_string().contains("addresses"));
    let e = report["entries"].as_array().unwrap();
    assert_eq!(e[0]["key"]["label"], "Press pack 002");
    assert_eq!(e[0]["addresses"], 8);
    assert_eq!(e[0]["devices"], 8);
    assert_eq!(e[0]["spike"], true);
    assert!(e[0]["peak_new_addresses_in_an_hour"].as_u64().unwrap() >= 8);
    let reasons = e[0]["reasons"].to_string();
    assert!(
        reasons.contains("addresses") && reasons.contains("within one hour"),
        "{reasons}"
    );
    assert!(e[0]["score"].as_u64().unwrap() > 0);
    // The one passed on with no limit tops the report; the posted one with its limit held is
    // next; one person at home and on the phone scores nothing.
    assert_eq!(e[1]["key"]["label"], "Posted");
    let quiet = e
        .iter()
        .find(|x| x["key"]["label"] == "Press pack 001")
        .unwrap();
    assert_eq!(quiet["score"], 0, "{quiet}");
    assert_eq!(quiet["addresses"], 2);
    assert!(quiet["reasons"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_lost_key_request_answers_the_same_matched_or_not_and_is_limited() {
    let (pool, station) = setup("Lost FM").await;
    let mut cfg = keys_config();
    cfg.lost_per_hour = 4;
    let p = plane(&pool, &station, TOKEN_KEY, cfg).await;
    let k = make(
        &p,
        json!({"label": "Ishbel", "email": "Ishbel@Example.org"}),
    )
    .await;
    let kid = k["key"]["id"].as_str().unwrap().to_owned();

    let (c1, h1, b1) = send_h(
        &p.app,
        "POST",
        "/api/v1/keyed/lost",
        None,
        Some("203.0.113.1"),
        json!({"email": "ishbel@example.org"}),
    )
    .await;
    let (c2, h2, b2) = send_h(
        &p.app,
        "POST",
        "/api/v1/keyed/lost",
        None,
        Some("203.0.113.2"),
        json!({"email": "nobody@example.org"}),
    )
    .await;
    let (c3, _, b3) = send_h(
        &p.app,
        "POST",
        "/api/v1/keyed/lost",
        None,
        Some("203.0.113.3"),
        json!({"email": "not an address"}),
    )
    .await;
    assert_eq!(
        (c1, c2, c3),
        (
            StatusCode::ACCEPTED,
            StatusCode::ACCEPTED,
            StatusCode::ACCEPTED
        )
    );
    assert_eq!(b1, b2);
    assert_eq!(b1, b3);
    assert_eq!(h1.get("cache-control"), h2.get("cache-control"));
    assert!(!b1.contains("example.org") && !b1.contains(k["text"].as_str().unwrap()));

    // With mail off the request is logged on the matched key, for staff to send it themselves.
    let mut seen = false;
    for _ in 0..60 {
        let (_, log) = send(
            &p.app,
            "GET",
            &format!("/api/v1/listener-keys/{kid}/log"),
            Some(KEY),
            Value::Null,
        )
        .await;
        if log.to_string().contains("lost_key_asked") {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(seen, "the request was logged on the key");
    assert!(
        !log_has_lost(&p, &pool, "nobody").await,
        "an unmatched address leaves no trace on a key"
    );
    // The same address is limited, matched or not; another email from the same address too.
    let mut last = StatusCode::ACCEPTED;
    for i in 0..6 {
        let (c, _, _) = send_h(
            &p.app,
            "POST",
            "/api/v1/keyed/lost",
            None,
            Some("203.0.113.77"),
            json!({"email": format!("x{i}@example.org")}),
        )
        .await;
        last = c;
        if c == StatusCode::TOO_MANY_REQUESTS {
            assert!(i >= 4, "{i}");
            break;
        }
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    // A flood about one address from many places is answered the same and sends nothing more:
    // one log entry per matched request up to the per-email limit of three an hour.
    for i in 0..8 {
        let (c, _, _) = send_h(
            &p.app,
            "POST",
            "/api/v1/keyed/lost",
            None,
            Some(&format!("192.0.2.{i}")),
            json!({"email": "ishbel@example.org"}),
        )
        .await;
        assert!(c == StatusCode::ACCEPTED || c == StatusCode::TOO_MANY_REQUESTS);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM listener_key_log WHERE key_id = $1 AND action = 'lost_key_asked'",
    )
    .bind(Uuid::parse_str(&kid).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!((1..=3).contains(&n), "{n}");
}

async fn log_has_lost(_p: &Plane, pool: &sqlx::PgPool, _who: &str) -> bool {
    // Only the one key has an email; nothing else may be logged.
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM listener_key_log WHERE action = 'lost_key_asked' AND key_id NOT IN (SELECT id FROM listener_key WHERE email_hash IS NOT NULL)",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    n > 0
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn wrong_keys_from_one_address_are_limited_and_another_address_is_not() {
    let (pool, station) = setup("Guess FM").await;
    let mut cfg = keys_config();
    cfg.failures_per_ten_minutes = 3;
    cfg.redeem_per_minute = 100;
    let p = plane(&pool, &station, TOKEN_KEY, cfg).await;
    let k = make(&p, json!({"label": "Right"})).await;
    for _ in 0..3 {
        let (code, _) = listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "203.0.113.1",
            json!({"key": "AAAA-AAAA-AAAA"}),
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }
    // Even the right key is refused from there for a while, with a Retry-After.
    let (code, headers, _) = send_h(
        &p.app,
        "POST",
        "/api/v1/keyed/redeem",
        None,
        Some("203.0.113.1"),
        json!({"key": k["text"]}),
    )
    .await;
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));
    // Another address is not held back by it.
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.2",
        json!({"key": k["text"]}),
    )
    .await);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_roles_and_the_token_scope_open_the_key_routes_and_no_more() {
    let (pool, station) = setup("Roles FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    show(&pool, &station, "supporters").await;

    // Staff of each role.
    let mut bearer = std::collections::BTreeMap::new();
    for (name, role) in [
        ("morag", "admin"),
        ("kirsty", "producer"),
        ("fraser", "presenter"),
        ("iona", "screener"),
    ] {
        ok(send(&p.app, "POST", "/api/v1/staff", Some(KEY), json!({"username": name, "name": name, "role": role, "password": "a long enough phrase"})).await);
        let s = ok(send(
            &p.app,
            "POST",
            "/api/v1/sessions",
            None,
            json!({"username": name, "password": "a long enough phrase"}),
        )
        .await);
        bearer.insert(name, s["token"].as_str().unwrap().to_owned());
    }
    // A producer and an admin manage keys; a presenter and a screener do not.
    for who in ["morag", "kirsty"] {
        let (code, _) = send(
            &p.app,
            "GET",
            "/api/v1/listener-keys",
            Some(&bearer[who]),
            Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{who}");
    }
    for who in ["fraser", "iona"] {
        for (m, u) in [
            ("GET", "/api/v1/listener-keys"),
            ("POST", "/api/v1/listener-keys"),
            ("GET", "/api/v1/listener-keys/leaks"),
            ("GET", "/api/v1/shows/supporters/access"),
        ] {
            let (code, _) = send(&p.app, m, u, Some(&bearer[who]), json!({"label": "x"})).await;
            assert_eq!(code, StatusCode::FORBIDDEN, "{who} {m} {u}");
        }
    }
    // No bearer, and a wrong one.
    for b in [None, Some("nope")] {
        let (code, _) = send(&p.app, "GET", "/api/v1/listener-keys", b, Value::Null).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }

    // Erasing a key is an admin's: a producer is refused.
    let k = make(&p, json!({"label": "To erase"})).await;
    let kid = k["key"]["id"].as_str().unwrap().to_owned();
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"]}),
    )
    .await);
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/listener-keys/{kid}"),
        Some(&bearer["kirsty"]),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);

    // An API token with `keys:manage` manages keys... and nothing else.
    let t = ok(send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(KEY),
        json!({"name": "Ticket shop", "scopes": ["keys:manage"]}),
    )
    .await);
    let tok = t["secret"].as_str().unwrap().to_owned();
    let made = ok(send(
        &p.app,
        "POST",
        "/api/v1/listener-keys",
        Some(&tok),
        json!({"label": "Ticket 7"}),
    )
    .await);
    let mid = made["key"]["id"].as_str().unwrap().to_owned();
    for (m, u, body) in [
        ("GET", "/api/v1/listener-keys".to_owned(), Value::Null),
        ("GET", format!("/api/v1/listener-keys/{mid}"), Value::Null),
        (
            "PATCH",
            format!("/api/v1/listener-keys/{mid}"),
            json!({"note": "paid"}),
        ),
        (
            "POST",
            format!("/api/v1/listener-keys/{mid}/pause"),
            Value::Null,
        ),
        (
            "POST",
            format!("/api/v1/listener-keys/{mid}/resume"),
            Value::Null,
        ),
        (
            "POST",
            format!("/api/v1/listener-keys/{mid}/replace"),
            Value::Null,
        ),
        ("GET", "/api/v1/listener-keys/leaks".to_owned(), Value::Null),
        (
            "GET",
            "/api/v1/shows/supporters/access".to_owned(),
            Value::Null,
        ),
    ] {
        let (code, b) = send(&p.app, m, &u, Some(&tok), body).await;
        assert!(code.is_success(), "{m} {u}: {code} {b}");
    }
    for (m, u, body) in [
        (
            "DELETE",
            format!("/api/v1/listener-keys/{kid}"),
            Value::Null,
        ),
        (
            "PUT",
            "/api/v1/shows/supporters/access".to_owned(),
            json!({"rule": "key"}),
        ),
        ("GET", "/api/v1/staff".to_owned(), Value::Null),
        ("GET", "/api/v1/tokens".to_owned(), Value::Null),
        ("GET", "/api/v1/studio".to_owned(), Value::Null),
    ] {
        let (code, _) = send(&p.app, m, &u, Some(&tok), body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{m} {u}");
    }
    // A token without the scope reaches none of it.
    let t2 = ok(send(
        &p.app,
        "POST",
        "/api/v1/tokens",
        Some(KEY),
        json!({"name": "Other", "scopes": ["shows:read"]}),
    )
    .await);
    let (code, _) = send(
        &p.app,
        "GET",
        "/api/v1/listener-keys",
        Some(t2["secret"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);

    // The admin erases it, and its use records and devices go with it.
    let (code, _) = send(
        &p.app,
        "DELETE",
        &format!("/api/v1/listener-keys/{kid}"),
        Some(&bearer["morag"]),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    for table in [
        "listener_key_use",
        "listener_key_device",
        "listener_key_log",
    ] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE key_id = $1"
        )))
        .bind(Uuid::parse_str(&kid).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_shows_rule_opens_to_everyone_at_its_time_and_the_public_list_says_so() {
    let (pool, station) = setup("Rule FM").await;
    let mut cfg = keys_config();
    cfg.get_a_key_text = "Write to hello@kettle.example".into();
    let p = plane(&pool, &station, TOKEN_KEY, cfg).await;
    plain_show(&pool, &station, "early").await;
    plain_show(&pool, &station, "open").await;

    // No rule: public.
    let (_, a) = send(
        &p.app,
        "GET",
        "/api/v1/shows/early/access",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(a["rule"], "public");
    // A producer sets it; the public list shows the lock and the time it opens.
    let opens = now_ms() + 3_600_000;
    let a = ok(send(
        &p.app,
        "PUT",
        "/api/v1/shows/early/access",
        Some(KEY),
        json!({"rule": "key", "opens_at_ms": opens, "slate": "teaser", "slate_note": "Back at 8"}),
    )
    .await);
    assert_eq!(
        (a["rule"].as_str(), a["slate"].as_str()),
        (Some("key"), Some("teaser"))
    );
    let (code, list) = send(
        &p.app,
        "GET",
        "/api/v1/public/keyed-shows",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list["get_a_key_text"], "Write to hello@kettle.example");
    let shows: Vec<&Value> = list["shows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["slug"] != "the-keyed-hour")
        .collect();
    assert_eq!(shows.len(), 1);
    assert_eq!(
        (shows[0]["slug"].as_str(), shows[0]["now"].as_str()),
        (Some("early"), Some("key"))
    );
    assert_eq!(shows[0]["opens_at_ms"], opens);
    // Once that time has come the show is public by itself; no staff action.
    sqlx::query("UPDATE show_access SET opens_at = now() - interval '1 minute' WHERE show_id = (SELECT id FROM show WHERE slug = 'early' AND station_id = $1)")
        .bind(station.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (_, list) = send(
        &p.app,
        "GET",
        "/api/v1/public/keyed-shows",
        None,
        Value::Null,
    )
    .await;
    let early = list["shows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slug"] == "early")
        .unwrap()
        .clone();
    assert_eq!(early["now"], "public");
    // The list says nothing of keys or who holds them.
    assert!(!list.to_string().contains("label"));
    // A bad rule or slate is a 422.
    for body in [
        json!({"rule": "secret"}),
        json!({"rule": "key", "slate": "silence"}),
    ] {
        let (code, _) = send(&p.app, "PUT", "/api/v1/shows/early/access", Some(KEY), body).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let (code, _) = send(
        &p.app,
        "PUT",
        "/api/v1/shows/nosuch/access",
        Some(KEY),
        json!({"rule": "key"}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // A key that names a show that does not exist is refused.
    let (code, e) = send(
        &p.app,
        "POST",
        "/api/v1/listener-keys",
        Some(KEY),
        json!({"label": "x", "shows": ["nosuch"]}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e["field"], "shows");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn first_use_and_expiry_are_each_told_to_webhooks_once_and_never_with_the_text() {
    let (pool, station) = setup("Events FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let mut rx = p.staff.api.events.subscribe();
    let k = make(&p, json!({"label": "Ticket 12", "email": "buyer@example.org", "max_devices": null, "valid_until_ms": now_ms() + 600_000})).await;
    let plain = k["text"].as_str().unwrap().replace('-', "");
    for i in 0..3 {
        ok(listen(
            &p.app,
            "/api/v1/keyed/redeem",
            &format!("203.0.113.{i}"),
            json!({"key": k["text"]}),
        )
        .await);
    }
    let first = rx.try_recv().expect("first used");
    assert_eq!(first.kind, events::KEY_FIRST_USED);
    assert_eq!(first.data["label"], "Ticket 12");
    assert!(rx.try_recv().is_err(), "first use is told once");
    assert!(
        !first.data.to_string().contains(&plain) && !first.data.to_string().contains("example.org")
    );

    // Not ended yet: the sweep says nothing. Past its end: once, and not again.
    let cfg = keys_config();
    let (n, _) = access::sweep(&pool, station.id, &p.staff.api.events, &cfg, now_ms())
        .await
        .unwrap();
    assert_eq!(n, 0);
    let id = Uuid::parse_str(k["key"]["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE listener_key SET valid_from = now() - interval '2 hours', valid_until = now() - interval '1 minute' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (n, _) = access::sweep(&pool, station.id, &p.staff.api.events, &cfg, now_ms())
        .await
        .unwrap();
    assert_eq!(n, 1);
    let ended = rx.try_recv().expect("ended");
    assert_eq!(
        (ended.kind, ended.data["reason"].as_str()),
        (events::KEY_ENDED, Some("expired"))
    );
    assert!(
        !ended.data.to_string().contains(&plain) && !ended.data.to_string().contains("example.org")
    );
    let (n, _) = access::sweep(&pool, station.id, &p.staff.api.events, &cfg, now_ms())
        .await
        .unwrap();
    assert_eq!(n, 0, "told once, across a restart too");
    // Extended, then ended again: it may be told again.
    ok(send(
        &p.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/extend"),
        Some(KEY),
        json!({"until_ms": now_ms() + 600_000}),
    )
    .await);
    sqlx::query("UPDATE listener_key SET valid_until = now() - interval '1 second' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (n, _) = access::sweep(&pool, station.id, &p.staff.api.events, &cfg, now_ms())
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn old_use_records_are_pruned_and_a_changed_token_key_leaves_texts_unreadable() {
    let (pool, station) = setup("Prune FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let k = make(&p, json!({"label": "Old"})).await;
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"]}),
    )
    .await);
    let id = Uuid::parse_str(k["key"]["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE listener_key_use SET hour = now() - interval '200 days' WHERE key_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let removed = keysdb::prune(&pool, station.id, 90).await.unwrap();
    assert_eq!(removed, 1);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM listener_key_use WHERE key_id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // The key still works: only its use records went.
    ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"]}),
    )
    .await);

    // The station's token key changes (a restart with another key): no typed text finds a key
    // and no sealed text can be read; staff are told to replace, not given rubbish.
    let p2 = plane(
        &pool,
        &station,
        "fedcba9876543210fedcba9876543210",
        keys_config(),
    )
    .await;
    let (code, e) = listen(
        &p2.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_recognised"))
    );
    let (code, e) = send(
        &p2.app,
        "POST",
        &format!("/api/v1/listener-keys/{id}/text"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert!(e["error"].as_str().unwrap().contains("Replace"));
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn without_a_station_key_keys_fail_closed() {
    let (pool, station) = setup("Closed FM").await;
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let api = Arc::new(Api::new(
        Some(pool.clone()),
        station.id,
        None,
        ApiConfig::default(),
        true,
    ));
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_api(api));
    let app = router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(KEY.into()),
        staff,
        hub,
        join: JoinConfig::default(),
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/listener-keys",
        Some(KEY),
        json!({"label": "x"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let (code, _) = listen(
        &app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": "KETM-7QMP-2D4X"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_token_for_a_named_show_names_only_that_show_and_follows_the_key_at_renewal() {
    let (pool, station) = setup("Narrow FM").await;
    let p = plane(&pool, &station, TOKEN_KEY, keys_config()).await;
    let a = show(&pool, &station, "friday").await;
    let b = show(&pool, &station, "saturday").await;
    // A key whose show has nothing on the schedule is told so, and that uses no device and no
    // listen (1.3.2: a token is for a show's window).
    plain_show(&pool, &station, "someday").await;
    let idle = make(
        &p,
        json!({"label": "Idle", "shows": ["someday"], "max_devices": 1}),
    )
    .await;
    for _ in 0..3 {
        let (code, e) = listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "203.0.113.9",
            json!({"key": idle["text"]}),
        )
        .await;
        assert_eq!(
            (code, e["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("not_scheduled"))
        );
    }
    // A key redeemed an hour before its show renews before the show starts.
    let later = plain_show(&pool, &station, "later").await;
    weavesignal_model::listener_keys_db::set_access(
        &pool,
        station.id,
        &weavesignal_model::listener_keys_db::Access {
            show_id: later,
            rule: "key".into(),
            opens_at_ms: None,
            slate: "announcement".into(),
            slate_note: String::new(),
            updated_ms: 0,
            updated_by: String::new(),
        },
        "test",
    )
    .await
    .unwrap();
    let series = Uuid::new_v4();
    sqlx::query("INSERT INTO series (id, station_id, show_id, number) VALUES ($1, $2, $3, 1)")
        .bind(series)
        .bind(station.id.as_uuid())
        .bind(later)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO episode (id, station_id, series_id, number, air_at, minutes)
         VALUES ($1, $2, $3, 1, now() + interval '1 hour', 60)",
    )
    .bind(Uuid::new_v4())
    .bind(station.id.as_uuid())
    .bind(series)
    .execute(&pool)
    .await
    .unwrap();
    p.staff.api.access.live.stale();
    let ahead = make(&p, json!({"label": "Ahead", "shows": ["later"]})).await;
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.9",
        json!({"key": ahead["text"]}),
    )
    .await);
    assert!(
        r["starts_ms"].as_i64().unwrap() > now_ms() + 3_000_000,
        "{r}"
    );
    let r2 = ok(listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.9",
        json!({"token": r["token"]}),
    )
    .await);
    assert_eq!(r2["show"]["slug"], "later");
    let idle_id = idle["key"]["id"].as_str().unwrap();
    let (_, d) = send(
        &p.app,
        "GET",
        &format!("/api/v1/listener-keys/{idle_id}/devices"),
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(d["devices"].as_array().map(Vec::len), Some(0), "{d}");
    let k = make(
        &p,
        json!({"label": "Two shows", "shows": ["friday", "saturday"], "max_devices": null}),
    )
    .await;
    let kid = k["key"]["id"].as_str().unwrap().to_owned();

    // No show named: one of its shows on now (a token is for one show, so an edge compares
    // one id). Friday named: Friday.
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"]}),
    )
    .await);
    let c = claims_of(r["token"].as_str().unwrap()).unwrap();
    assert!(
        c.show == a.to_string() || c.show == b.to_string(),
        "{}",
        c.show
    );
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"], "show": "friday"}),
    )
    .await);
    let c = claims_of(r["token"].as_str().unwrap()).unwrap();
    assert!(opens(&c, a, TokenMedia::Sound) && !opens(&c, b, TokenMedia::Sound));
    // The renewal keeps it narrow.
    let r2 = ok(listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.1",
        json!({"token": r["token"]}),
    )
    .await);
    assert_eq!(
        claims_of(r2["token"].as_str().unwrap()).unwrap().show,
        a.to_string()
    );

    // Staff take Friday off the key: at the next renewal the holder is told it does not open it.
    ok(send(
        &p.app,
        "PATCH",
        &format!("/api/v1/listener-keys/{kid}"),
        Some(KEY),
        json!({"shows": ["saturday"]}),
    )
    .await);
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/renew",
        "203.0.113.1",
        json!({"token": r2["token"]}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("does_not_open_show"))
    );
    let (code, e) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.1",
        json!({"key": k["text"], "show": "friday"}),
    )
    .await;
    assert_eq!(
        (code, e["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("does_not_open_show"))
    );

    // A key for every keyed show asked for one show: narrowed too.
    let all = make(&p, json!({"label": "All", "max_devices": null})).await;
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.2",
        json!({"key": all["text"], "show": "saturday"}),
    )
    .await);
    let c = claims_of(r["token"].as_str().unwrap()).unwrap();
    assert_eq!(c.show, b.to_string());
    let r = ok(listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.2",
        json!({"key": all["text"]}),
    )
    .await);
    let c = claims_of(r["token"].as_str().unwrap()).unwrap();
    // No show named: a show on now. Every keyed show here is one, the station's own keyed hour
    // from `setup` too, and which of them is not part of the rule; the later show is not on now.
    let hour = weavesignal_model::shows::get_show(&pool, station.id, "the-keyed-hour")
        .await
        .unwrap()
        .unwrap()
        .id;
    assert!(
        [a, b, hour].iter().any(|s| c.show == s.to_string()),
        "{} is not a show on now",
        c.show
    );
    assert_ne!(c.show, later.to_string());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn without_an_address_nobody_is_counted_and_the_station_is_never_shut() {
    let (pool, station) = setup("Nowhere FM").await;
    let mut cfg = keys_config();
    cfg.failures_per_ten_minutes = 3;
    cfg.redeem_per_minute = 5;
    cfg.lost_per_hour = 2;
    let p = plane(&pool, &station, TOKEN_KEY, cfg).await;
    let k = make(&p, json!({"label": "Right", "max_devices": null})).await;
    // A proxy that tells nothing of the client: no address. The same wrong key, twenty times,
    // from "everyone", locks nobody out; the right key and a lost-key request still work.
    for _ in 0..20 {
        let (code, _, _) = send_h(
            &p.app,
            "POST",
            "/api/v1/keyed/redeem",
            None,
            None,
            json!({"key": "AAAA-AAAA-AAAA"}),
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }
    for _ in 0..8 {
        let (code, _, b) = send_h(
            &p.app,
            "POST",
            "/api/v1/keyed/redeem",
            None,
            None,
            json!({"key": k["text"]}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{b}");
    }
    for _ in 0..6 {
        let (code, _, _) = send_h(
            &p.app,
            "POST",
            "/api/v1/keyed/lost",
            None,
            None,
            json!({"email": "someone@example.org"}),
        )
        .await;
        assert_eq!(code, StatusCode::ACCEPTED);
    }
    // With an address the same limits do bite.
    for _ in 0..3 {
        listen(
            &p.app,
            "/api/v1/keyed/redeem",
            "203.0.113.9",
            json!({"key": "AAAA-AAAA-AAAA"}),
        )
        .await;
    }
    let (code, _) = listen(
        &p.app,
        "/api/v1/keyed/redeem",
        "203.0.113.9",
        json!({"key": k["text"]}),
    )
    .await;
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
}
