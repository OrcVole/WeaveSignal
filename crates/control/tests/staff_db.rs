// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Staff accounts and sign-in through the control plane's routes against a real PostgreSQL
//! (ADR-0127): the station key makes the first admin; each person signs in as themselves; only
//! an admin reaches retention, personal data and the staff list; an account turned off, a
//! password set by an admin or a sign-out ends sessions at once; sessions outlive a restart;
//! repeated failures are refused for a while; the emailed code signs staff in once, says the same
//! to anyone who asks, and is spent by tries, a new code, a new address, a new password and
//! turning the account off. Ignored unless asked for (`just test-db`).

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
use weavesignal_control::config::{MailConfig, MailMode};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::mailkey::MailKey;
use weavesignal_control::staff::{Mailer, Staff};
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId, ensure_station};

const KEY: &str = "staff-db-test-station-key";

/// A control plane as `serve` builds it; a new one over the same database is a restart.
async fn control_plane(pool: &sqlx::PgPool, station: &Station) -> Router {
    control_plane_with(pool, station, None, false).await
}

/// As [`control_plane`], with mail (an outbox directory of its own) and, if asked, the
/// forwarded address counted as the peer (so a test can speak from several addresses).
async fn control_plane_with(
    pool: &sqlx::PgPool,
    station: &Station,
    mailer: Option<Mailer>,
    forwarded_for: bool,
) -> Router {
    let hub = Hub::new(
        &station.name,
        Studio::default(),
        DeskClient::new(None),
        None,
        Duration::from_millis(250),
        CartsLink::new(CartsConfig::default(), None),
    );
    let staff = Arc::new(Staff::new(Some(pool.clone())).with_mailer(mailer));
    staff.restore(station.id).await;
    router(Arc::new(AppState {
        station: station.clone(),
        db: Some(pool.clone()),
        room: None,
        admin_token: Some(KEY.into()),
        staff,
        hub,
        join: JoinConfig {
            forwarded_for,
            ..JoinConfig::default()
        },
        video: Default::default(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }))
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
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        req = req.header("authorization", format!("Bearer {b}"));
    }
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

async fn sign_in(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/v1/sessions",
        None,
        json!({"username": username, "password": password}),
    )
    .await
}

async fn setup(name: &str) -> (sqlx::PgPool, Station) {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(&pool, &Station::new(StationId::new_random(), name).unwrap())
        .await
        .unwrap();
    (pool, station)
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn staff_sign_in_as_themselves_and_only_admins_do_admin_things() {
    let (pool, station) = setup("Staff FM").await;
    let app = control_plane(&pool, &station).await;

    // No accounts yet: nobody signs in, and the station key makes the first admin.
    let (code, _) = sign_in(&app, "morag", "a long enough phrase").await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, morag) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": " Morag ", "name": "Morag Campbell", "role": "admin", "password": "a long enough phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{morag}");
    assert_eq!(morag["username"], "morag");
    assert!(morag.get("password_hash").is_none());

    // The rules: length only, and the username is taken once.
    let (code, p) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": "fraser", "name": "Fraser", "role": "presenter", "password": "short"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p["field"], "password");
    let (code, p) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": "morag", "name": "Another", "role": "screener", "password": "a long enough phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p["field"], "username");

    // Morag signs in and makes a presenter.
    let (code, s) = sign_in(&app, "MORAG", "a long enough phrase").await;
    assert_eq!(code, StatusCode::CREATED, "{s}");
    let admin = s["token"].as_str().unwrap().to_owned();
    assert!(admin.starts_with("rss_"));
    let (code, me) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(me["kind"], "staff");
    assert_eq!(me["role"], "admin");
    assert_eq!(me["account"]["name"], "Morag Campbell");
    let (code, fraser) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(&admin),
        json!({"username": "fraser", "name": "Fraser Macleod", "role": "presenter", "password": "the presenter's phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    let fraser_id = fraser["id"].as_str().unwrap().to_owned();

    // Fraser runs the studio but not retention, personal data or the staff list.
    let (_, s) = sign_in(&app, "fraser", "the presenter's phrase").await;
    let presenter = s["token"].as_str().unwrap().to_owned();
    let (code, _) = send(&app, "GET", "/api/v1/carts", Some(&presenter), Value::Null).await;
    assert_ne!(code, StatusCode::UNAUTHORIZED);
    assert_ne!(code, StatusCode::FORBIDDEN);
    for (method, uri) in [
        ("GET", "/api/v1/retention"),
        ("GET", "/api/v1/retention/holds"),
        ("GET", "/api/v1/staff"),
    ] {
        let (code, p) = send(&app, method, uri, Some(&presenter), Value::Null).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{uri}: {p}");
    }
    let (code, _) = send(&app, "GET", "/api/v1/retention", Some(&admin), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    let (code, list) = send(&app, "GET", "/api/v1/staff", Some(&admin), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list["accounts"].as_array().unwrap().len(), 2);

    // An admin cannot turn themselves off or down by a slip.
    let morag_id = morag["id"].as_str().unwrap();
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{morag_id}"),
        Some(&admin),
        json!({"role": "producer"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

    // A role change lands at once: Fraser made an admin reaches the staff list.
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{fraser_id}"),
        Some(&admin),
        json!({"role": "admin"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = send(&app, "GET", "/api/v1/staff", Some(&presenter), Value::Null).await;
    assert_eq!(code, StatusCode::OK);

    // Turned off: Fraser's session ends now, and he cannot sign in again.
    let (code, off) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{fraser_id}"),
        Some(&admin),
        json!({"disabled": true, "role": "presenter"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(off["disabled_ms"].is_number());
    let (code, _) = send(&app, "GET", "/api/v1/carts", Some(&presenter), Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, _) = sign_in(&app, "fraser", "the presenter's phrase").await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    // Back on, with a password an admin set; the old one no longer works.
    send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{fraser_id}"),
        Some(&admin),
        json!({"disabled": false}),
    )
    .await;
    let (code, _) = send(
        &app,
        "POST",
        &format!("/api/v1/staff/{fraser_id}/password"),
        Some(&admin),
        json!({"password": "a new presenter phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = sign_in(&app, "fraser", "the presenter's phrase").await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, s) = sign_in(&app, "fraser", "a new presenter phrase").await;
    assert_eq!(code, StatusCode::CREATED);
    let presenter = s["token"].as_str().unwrap().to_owned();

    // Fraser changes his own: his other sessions end, this one stays.
    let (_, s) = sign_in(&app, "fraser", "a new presenter phrase").await;
    let other = s["token"].as_str().unwrap().to_owned();
    let (code, p) = send(
        &app,
        "POST",
        "/api/v1/sessions/me/password",
        Some(&presenter),
        json!({"current": "wrong", "new": "fraser's own phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p["field"], "current");
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/sessions/me/password",
        Some(&presenter),
        json!({"current": "a new presenter phrase", "new": "fraser's own phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&other),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);

    // A restart keeps live sessions, and not the ended ones.
    let app = control_plane(&pool, &station).await;
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&other),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    // Signing out ends the session, here and across a restart.
    let (code, _) = send(
        &app,
        "DELETE",
        "/api/v1/sessions/me",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let app = control_plane(&pool, &station).await;
    let (code, _) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&presenter),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);

    // The station key is still the station's: an admin, with nothing to sign out of.
    let (code, me) = send(&app, "GET", "/api/v1/sessions/me", Some(KEY), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(me["kind"], "station");
    let (code, _) = send(
        &app,
        "DELETE",
        "/api/v1/sessions/me",
        Some(KEY),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn repeated_failures_are_refused_for_a_while_even_with_the_right_password() {
    let (pool, station) = setup("Lockout FM").await;
    let app = control_plane(&pool, &station).await;
    let (code, _) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": "ailsa", "name": "Ailsa", "role": "screener", "password": "the screener's phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    for _ in 0..5 {
        let (code, _) = sign_in(&app, "ailsa", "a wrong guess here").await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }
    let (code, _) = sign_in(&app, "ailsa", "the screener's phrase").await;
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    // An unknown username answers as a wrong password does.
    let (code, p) = sign_in(&app, "nobody", "a wrong guess here").await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(
        p["error"],
        "That username and password do not match an account here."
    );
}

// ---- the emailed code (ADR-0127) ----

const MAIL_KEY: &str = "staff-db-test-mail-key-0123456789abcdef";

/// A mailer that writes to an outbox of its own under the target directory.
fn outbox_mailer(station: &Station) -> (Mailer, std::path::PathBuf) {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("staff-outbox-{}", uuid::Uuid::new_v4().simple()));
    let mailer = Mailer {
        config: MailConfig {
            mode: MailMode::Outbox,
            from: "studio@station.example".into(),
            outbox_dir: dir.clone(),
            ..MailConfig::default()
        },
        key: MailKey::parse(MAIL_KEY).unwrap(),
        station: station.name.clone(),
    };
    (mailer, dir)
}

/// The emails written so far, oldest first.
fn outbox(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

/// The code in the newest email, once there are `n` emails (the send happens after the answer).
async fn code_from(dir: &std::path::Path, n: usize) -> String {
    for _ in 0..100 {
        let mails = outbox(dir);
        if mails.len() >= n {
            let subject = mails[n - 1]
                .lines()
                .find(|l| l.starts_with("Subject: "))
                .expect("a subject")
                .to_owned();
            let code = subject["Subject: ".len()..]
                .split_whitespace()
                .next()
                .unwrap()
                .to_owned();
            assert_eq!(code.len(), 6, "{subject}");
            return code;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no email {n} in the outbox");
}

/// Long enough for a send that was going to happen to have happened.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}

async fn ask(app: &Router, username: &str, email: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/v1/sessions/code",
        None,
        json!({"username": username, "email": email}),
    )
    .await
}

async fn finish(app: &Router, username: &str, email: &str, code: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/api/v1/sessions/code/finish",
        Some("sign-in"),
        json!({"username": username, "email": email, "code": code}),
    )
    .await
}

async fn make_account(app: &Router, username: &str, role: &str, password: &str) -> String {
    let (code, a) = send(
        app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": username, "name": username, "role": role, "password": password}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{a}");
    a["id"].as_str().unwrap().to_owned()
}

async fn set_address(app: &Router, id: &str, email: Value) -> (StatusCode, Value) {
    send(
        app,
        "PUT",
        &format!("/api/v1/staff/{id}/email"),
        Some(KEY),
        json!({ "email": email }),
    )
    .await
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_emailed_code_signs_staff_in_once_and_the_address_is_never_kept() {
    let (pool, station) = setup("Code FM").await;
    let (mailer, dir) = outbox_mailer(&station);
    let app = control_plane_with(&pool, &station, Some(mailer), false).await;
    let id = make_account(&app, "morag", "producer", "a long enough phrase").await;
    let (code, a) = set_address(&app, &id, json!("Morag.Campbell@Mail.example")).await;
    assert_eq!(code, StatusCode::OK, "{a}");
    assert_eq!(a["has_email"], true);
    assert!(!a.to_string().contains("mail.example"), "{a}");

    // The database holds a keyed hash, not the address and not the plain SHA-256 of it.
    let (hash,): (Vec<u8>,) =
        sqlx::query_as("SELECT email_hash FROM staff_account WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hash.len(), 32);
    use sha2::{Digest, Sha256};
    assert_ne!(
        hash,
        Sha256::digest(b"morag.campbell@mail.example").to_vec()
    );

    // Asking: 202, then one email to the address typed, with a code in its subject.
    let (code, asked) = ask(&app, "Morag", " morag.campbell@MAIL.example ").await;
    assert_eq!(code, StatusCode::ACCEPTED, "{asked}");
    let digits = code_from(&dir, 1).await;
    let mail = &outbox(&dir)[0];
    assert!(
        mail.contains("To: morag.campbell@mail.example\r\n"),
        "{mail}"
    );
    let (stored,): (Vec<u8>,) = sqlx::query_as(
        "SELECT code_hash FROM staff_signin WHERE account_id = $1::uuid AND NOT used",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.len(), 32, "a keyed hash, not the digits");
    assert_ne!(stored, Sha256::digest(digits.as_bytes()).to_vec());

    // A wrong code, then the right one: a session that works as a password's does.
    let wrong = if digits == "000000" {
        "000001"
    } else {
        "000000"
    };
    let (code, p) = finish(&app, "morag", "morag.campbell@mail.example", wrong).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(
        p["error"],
        "That code does not match, or has run out. Ask for a new one."
    );
    // The right code with another address does not sign in either.
    let (code, _) = finish(&app, "morag", "someone.else@mail.example", &digits).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, signed) = finish(&app, "morag", "Morag.Campbell@mail.example", &digits).await;
    assert_eq!(code, StatusCode::CREATED, "{signed}");
    let token = signed["token"].as_str().unwrap();
    assert!(token.starts_with("rss_"));
    assert_eq!(signed["account"]["username"], "morag");
    let (code, me) = send(&app, "GET", "/api/v1/sessions/me", Some(token), Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{me}");
    assert_eq!(me["role"], "producer");

    // Once: the same code again is refused with the same words.
    let (code, p) = finish(&app, "morag", "morag.campbell@mail.example", &digits).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(
        p["error"],
        "That code does not match, or has run out. Ask for a new one."
    );
    // The password still works beside it.
    assert_eq!(
        sign_in(&app, "morag", "a long enough phrase").await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn asking_for_a_code_tells_a_stranger_nothing() {
    let (pool, station) = setup("Quiet FM").await;
    let (mailer, dir) = outbox_mailer(&station);
    let app = control_plane_with(&pool, &station, Some(mailer), false).await;
    let morag = make_account(&app, "morag", "admin", "a long enough phrase").await;
    let ailsa = make_account(&app, "ailsa", "screener", "the screener's phrase").await;
    set_address(&app, &morag, json!("morag@mail.example")).await;
    set_address(&app, &ailsa, json!("ailsa@mail.example")).await;
    // Ailsa leaves.
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{ailsa}"),
        Some(KEY),
        json!({"disabled": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);

    let answers = [
        ask(&app, "morag", "morag@mail.example").await,
        // The right username, another address; another username, the right address.
        ask(&app, "morag", "nobody@mail.example").await,
        ask(&app, "ailsa", "morag@mail.example").await,
        // An unknown username; a username that is not one; an account that is off.
        ask(&app, "nobody", "nobody@mail.example").await,
        ask(&app, "no body!", "nobody@mail.example").await,
        ask(&app, "ailsa", "ailsa@mail.example").await,
    ];
    for (code, body) in &answers {
        assert_eq!(*code, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body, &answers[0].1);
    }
    settle().await;
    assert_eq!(
        outbox(&dir).len(),
        1,
        "one email, to the one account that matched"
    );
    assert!(outbox(&dir)[0].contains("To: morag@mail.example"));

    // A malformed address is the same sentence for everyone, and sends nothing.
    let (code, p) = ask(&app, "morag", "not an address").await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p["field"], "email");

    // Finishing: an unknown username, an off account and a wrong code say the same words.
    let said = finish(&app, "morag", "morag@mail.example", "123456").await;
    assert_eq!(said.0, StatusCode::UNAUTHORIZED);
    for (u, e) in [
        ("nobody", "nobody@mail.example"),
        ("ailsa", "ailsa@mail.example"),
    ] {
        assert_eq!(finish(&app, u, e, "123456").await, said);
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_station_without_mail_offers_no_code() {
    let (pool, station) = setup("Offline FM").await;
    let app = control_plane(&pool, &station).await;
    let id = make_account(&app, "morag", "admin", "a long enough phrase").await;
    let (code, p) = ask(&app, "morag", "morag@mail.example").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(p["error"].as_str().unwrap().contains("password"), "{p}");
    assert_eq!(
        finish(&app, "morag", "morag@mail.example", "123456")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // No address can be set without the key that hides it; one can always be taken away.
    let (code, _) = set_address(&app, &id, json!("morag@mail.example")).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let (code, a) = set_address(&app, &id, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{a}");
    // The password is untouched.
    assert_eq!(
        sign_in(&app, "morag", "a long enough phrase").await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn codes_are_limited_by_account_by_address_and_by_wrong_guesses() {
    let (pool, station) = setup("Limits FM").await;
    let (mailer, dir) = outbox_mailer(&station);
    let app = control_plane_with(&pool, &station, Some(mailer), true).await;
    let id = make_account(&app, "morag", "admin", "a long enough phrase").await;
    set_address(&app, &id, json!("morag@mail.example")).await;

    // Per account: three codes in ten minutes, the fourth ask is answered alike and sends none.
    for _ in 0..4 {
        let (code, _) = ask(&app, "morag", "morag@mail.example").await;
        assert_eq!(code, StatusCode::ACCEPTED);
        settle().await;
    }
    assert_eq!(outbox(&dir).len(), 3);
    // Only the newest code is live: the first cannot be used after the second was made.
    let newest = code_from(&dir, 3).await;

    // Per address: ten asks, the eleventh is refused whoever it asks for. (Four are counted.)
    let from = |n: u8| format!("203.0.113.{n}");
    let ask_from = |addr: String| {
        let app = app.clone();
        async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/sessions/code")
                .header("content-type", "application/json")
                .header("x-forwarded-for", addr)
                .body(Body::from(
                    json!({"username": "nobody", "email": "nobody@mail.example"}).to_string(),
                ))
                .unwrap();
            app.oneshot(req).await.unwrap().status()
        }
    };
    for _ in 0..10 {
        assert_eq!(ask_from(from(7)).await, StatusCode::ACCEPTED);
    }
    assert_eq!(ask_from(from(7)).await, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(ask_from(from(8)).await, StatusCode::ACCEPTED);

    // Wrong guesses: five lock the username's emailed code out, the right one included,
    // while its password is not touched.
    let wrong = if newest == "000000" {
        "000001"
    } else {
        "000000"
    };
    for _ in 0..5 {
        let (code, _) = finish(&app, "morag", "morag@mail.example", wrong).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }
    let (code, _) = finish(&app, "morag", "morag@mail.example", &newest).await;
    assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        sign_in(&app, "morag", "a long enough phrase").await.0,
        StatusCode::CREATED
    );
    // ...and the other way: wrong passwords do not lock the emailed code.
    let (_, other) = (
        0,
        make_account(&app, "fraser", "presenter", "the presenter's phrase").await,
    );
    set_address(&app, &other, json!("fraser@mail.example")).await;
    for _ in 0..5 {
        sign_in(&app, "fraser", "a wrong guess here").await;
    }
    assert_eq!(
        sign_in(&app, "fraser", "the presenter's phrase").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let (code, _) = ask(&app, "fraser", "fraser@mail.example").await;
    assert_eq!(code, StatusCode::ACCEPTED);
    let fraser_code = code_from(&dir, 4).await;
    let (code, v) = finish(&app, "fraser", "fraser@mail.example", &fraser_code).await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_code_is_spent_by_a_new_address_a_new_password_and_turning_the_account_off() {
    let (pool, station) = setup("Spent FM").await;
    let (mailer, dir) = outbox_mailer(&station);
    let app = control_plane_with(&pool, &station, Some(mailer), false).await;
    let id = make_account(&app, "morag", "admin", "a long enough phrase").await;
    set_address(&app, &id, json!("morag@mail.example")).await;

    // A new address: the code sent to the old one is no use, even for the old address.
    ask(&app, "morag", "morag@mail.example").await;
    let c1 = code_from(&dir, 1).await;
    set_address(&app, &id, json!("morag.new@mail.example")).await;
    assert_eq!(
        finish(&app, "morag", "morag@mail.example", &c1).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        finish(&app, "morag", "morag.new@mail.example", &c1).await.0,
        StatusCode::UNAUTHORIZED
    );
    // The old address no longer names her.
    ask(&app, "morag", "morag@mail.example").await;
    settle().await;
    assert_eq!(outbox(&dir).len(), 1);

    // A new password (by an admin).
    ask(&app, "morag", "morag.new@mail.example").await;
    let c2 = code_from(&dir, 2).await;
    let (code, _) = send(
        &app,
        "POST",
        &format!("/api/v1/staff/{id}/password"),
        Some(KEY),
        json!({"password": "another long phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    assert_eq!(
        finish(&app, "morag", "morag.new@mail.example", &c2).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Turning the account off: the code, and the session it would have made.
    ask(&app, "morag", "morag.new@mail.example").await;
    let c3 = code_from(&dir, 3).await;
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{id}"),
        Some(KEY),
        json!({"disabled": true}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        finish(&app, "morag", "morag.new@mail.example", &c3).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Taking the address away leaves no way in by code. (Morag has had her three codes for
    // these ten minutes, so this is another account's.)
    let ailsa = make_account(&app, "ailsa", "screener", "the screener's phrase").await;
    set_address(&app, &ailsa, json!("ailsa@mail.example")).await;
    ask(&app, "ailsa", "ailsa@mail.example").await;
    let c4 = code_from(&dir, 4).await;
    let (code, a) = set_address(&app, &ailsa, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(a["has_email"], false);
    assert_eq!(
        finish(&app, "ailsa", "ailsa@mail.example", &c4).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn who_may_set_an_address_and_one_address_is_one_account() {
    let (pool, station) = setup("Address FM").await;
    let (mailer, _dir) = outbox_mailer(&station);
    let app = control_plane_with(&pool, &station, Some(mailer), false).await;
    let morag = make_account(&app, "morag", "admin", "a long enough phrase").await;
    let fraser = make_account(&app, "fraser", "presenter", "the presenter's phrase").await;
    let (_, signed) = sign_in(&app, "fraser", "the presenter's phrase").await;
    let fraser_token = signed["token"].as_str().unwrap().to_owned();

    // Someone who is not an admin cannot set another's address.
    let (code, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/staff/{morag}/email"),
        Some(&fraser_token),
        json!({"email": "x@mail.example"}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // Their own needs their password, and a malformed address is refused under its field.
    let own = |current: &str, email: Value| {
        let app = app.clone();
        let t = fraser_token.clone();
        let current = current.to_owned();
        async move {
            send(
                &app,
                "PUT",
                "/api/v1/sessions/me/email",
                Some(&t),
                json!({"current": current, "email": email}),
            )
            .await
        }
    };
    let (code, p) = own("not my password", json!("fraser@mail.example")).await;
    assert_eq!(
        (code, p["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("current"))
    );
    let (code, p) = own("the presenter's phrase", json!("fraser at mail")).await;
    assert_eq!(
        (code, p["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("email"))
    );
    let (code, p) = own("the presenter's phrase", json!(5)).await;
    assert_eq!(
        (code, p["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("email"))
    );
    let (code, a) = own("the presenter's phrase", json!("fraser@mail.example")).await;
    assert_eq!(code, StatusCode::OK, "{a}");
    assert_eq!(a["has_email"], true);
    // The signed-in session sees it at once.
    let (_, me) = send(
        &app,
        "GET",
        "/api/v1/sessions/me",
        Some(&fraser_token),
        Value::Null,
    )
    .await;
    assert_eq!(me["account"]["has_email"], true);

    // One address, one account (compared without regard to case).
    let (code, p) = set_address(&app, &morag, json!("Fraser@Mail.example")).await;
    assert_eq!(
        (code, p["field"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("email")),
        "{p}"
    );
    // The station key has no address of its own to set.
    let (code, _) = send(
        &app,
        "PUT",
        "/api/v1/sessions/me/email",
        Some(KEY),
        json!({"current": "x", "email": null}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    let _ = fraser;
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_code_takes_five_tries_one_use_and_the_newest_code_alone() {
    use weavesignal_model::staff::{CodeCheck, check_code, issue_code};
    let (pool, station) = setup("Tries FM").await;
    let app = control_plane(&pool, &station).await;
    let id: uuid::Uuid = make_account(&app, "morag", "admin", "a long enough phrase")
        .await
        .parse()
        .unwrap();
    let (h1, h2) = (vec![1u8; 32], vec![2u8; 32]);

    // Five guesses at a code, right or wrong, and then it is spent.
    assert!(issue_code(&pool, station.id, id, &h1, 3).await.unwrap());
    for _ in 0..5 {
        assert_eq!(
            check_code(&pool, station.id, id, |_| false).await.unwrap(),
            CodeCheck::Wrong
        );
    }
    assert_eq!(
        check_code(&pool, station.id, id, |w| w == h1)
            .await
            .unwrap(),
        CodeCheck::Spent
    );

    // A new code spends the older one: only the newest is ever live.
    assert!(issue_code(&pool, station.id, id, &h1, 3).await.unwrap());
    assert!(issue_code(&pool, station.id, id, &h2, 3).await.unwrap());
    assert_eq!(
        check_code(&pool, station.id, id, |w| w == h1)
            .await
            .unwrap(),
        CodeCheck::Wrong
    );
    // A third ask in the window is the last the account gets; a fourth is not kept.
    assert!(!issue_code(&pool, station.id, id, &h1, 3).await.unwrap());
    // Two requests with the right code at once: one signs in.
    let (a, b) = tokio::join!(
        check_code(&pool, station.id, id, |w| w == h2),
        check_code(&pool, station.id, id, |w| w == h2)
    );
    let mut got = [a.unwrap(), b.unwrap()];
    got.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(
        got.iter().filter(|c| **c == CodeCheck::Good).count(),
        1,
        "{got:?}"
    );
    assert_eq!(
        check_code(&pool, station.id, id, |w| w == h2)
            .await
            .unwrap(),
        CodeCheck::Spent
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_hold_names_the_signed_in_admin_not_the_name_typed() {
    // Review 0028, C4: since staff sign in as themselves, a retention record names them from
    // their session; only the station key, which is no one in particular, names someone in `by`.
    let (pool, station) = setup("Hold Names FM").await;
    let app = control_plane(&pool, &station).await;
    let (code, made) = send(
        &app,
        "POST",
        "/api/v1/staff",
        Some(KEY),
        json!({"username": "eilidh", "name": "Eilidh Ross", "role": "admin", "password": "a long enough phrase"}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let (_, s) = sign_in(&app, "eilidh", "a long enough phrase").await;
    let session = s["token"].as_str().unwrap().to_owned();
    let window = json!({"reason": "A listener's complaint", "by": "Somebody Else", "from_ms": 1_793_000_000_000u64, "to_ms": 1_793_000_600_000u64, "records": []});

    let (code, h) = send(
        &app,
        "POST",
        "/api/v1/retention/holds",
        Some(&session),
        window.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{h}");
    assert_eq!(h["placed_by"], "eilidh", "the session names who placed it");
    let id = h["id"].as_i64().unwrap();
    let (code, h) = send(
        &app,
        "POST",
        &format!("/api/v1/retention/holds/{id}/release"),
        Some(&session),
        json!({"by": "Somebody Else", "reason": "The complaint is closed"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{h}");
    assert_eq!(h["released_by"], "eilidh");

    // The station key is no one: the name it gives is the one kept.
    let (code, h) = send(&app, "POST", "/api/v1/retention/holds", Some(KEY), window).await;
    assert_eq!(code, StatusCode::CREATED, "{h}");
    assert_eq!(h["placed_by"], "Somebody Else");
}
