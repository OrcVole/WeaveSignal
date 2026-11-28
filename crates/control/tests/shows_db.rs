// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Shows, cards and feeds through the control plane's routes against a real PostgreSQL (M8's
//! done-when): a show and a season drafted from its rule, an episode set and put on air, its
//! files tagged with its card, and the feed listing it with both enclosures and its card.
//! Ignored unless asked for (`just test-db`).

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;
use weavesignal_control::config::PublishingConfig;
use weavesignal_control::shows::{ShowsState, router};
use weavesignal_model::{Station, StationId, ensure_station};

const ADMIN: &str = "shows-db-test";

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    (
        code,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    json_as(app, ADMIN, method, uri, body).await
}

async fn json_as(
    app: &axum::Router,
    bearer: &str,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (code, bytes) = send(
        app,
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_aired_show_appears_in_its_feed_with_both_enclosures_and_its_card() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("shows-{}", uuid::Uuid::new_v4()));
    let episodes_dir = dir.join("episodes");
    std::fs::create_dir_all(&episodes_dir).unwrap();
    let app = router(Arc::new(ShowsState {
        station,
        db: Some(pool),
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            owner_email: "podcasts@kettle.example".into(),
            copyright: "2026 Kettle FM".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: episodes_dir.clone(),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: None,
    }));

    // A name that cannot fit refuses the save, with the field named, and nothing is kept.
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/too-long",
        serde_json::json!({
            "name": "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society, Extended Edition, With Added Weather For The Inshore Waters And The Coastal Stations",
            "colour_slot": 12, "layout": "poster", "hosts": ["Hamish Wallace"]
        }),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"].as_str().unwrap().contains("show name"));
    let (code, _) = json(
        &app,
        "GET",
        "/api/v1/shows/too-long",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // The show, drawn on save.
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night",
        serde_json::json!({
            "name": "Saturday Night Kettle",
            "description": "Live music, requests and the Forth after dark, every Saturday from eight.",
            "colour_slot": 3, "layout": "poster",
            "slot": {"weekday": 6, "start": 1200, "minutes": 180},
            "hosts": ["Shona Campbell", "Catriona MacKenzie-Buchanan"]
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let cards = &body["show"]["cards"];
    for which in ["link", "post", "square"] {
        let file = cards[which].as_str().unwrap();
        assert!(dir.join("cards").join(file).exists(), "{file}");
    }
    assert!(
        cards["alt"]
            .as_str()
            .unwrap()
            .starts_with("Saturday Night Kettle, Saturdays 20:00 on Kettle FM")
    );

    // The card is served, as a PNG.
    let (code, png) = send(
        &app,
        Request::get(format!("/cards/{}", cards["link"].as_str().unwrap()))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(png.starts_with(b"\x89PNG"));

    // Series 3 from Saturday 6 September 2036 (a start in the future, since drafting never goes
    // into the past), ten planned; the season drafted at once.
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night/series/3",
        serde_json::json!({"starts_on": "2036-09-06", "planned_episodes": 10}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["resolved"]["series"]["first"], "2036-09-06");
    assert_eq!(body["resolved"]["series"]["last"], "2036-11-08");
    assert!(
        body["resolved"]["inherited"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "hosts")
    );
    let (code, body) = json(
        &app,
        "POST",
        "/api/v1/shows/saturday-night/series/3/episodes",
        serde_json::json!({"count": 10}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    assert_eq!(body.as_array().unwrap().len(), 10);
    let seventh = &body[6];
    assert_eq!(seventh["episode"]["number"], 7);
    assert!(seventh["episode"]["cards"]["square"].is_string());

    // Episode 7: its title and guests.
    let (code, body) = json(
        &app,
        "PATCH",
        "/api/v1/shows/saturday-night/series/3/episodes/7",
        serde_json::json!({"title": "Ghost Train night", "guests": ["Firth Lights", "Dr Una Robertson"]}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        body["episode"]["cards"]["alt"],
        "Saturday Night Kettle, Series 3 Episode 7, Ghost Train night, Saturday 18 October 2036 at 20:00 BST on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan, guests Firth Lights and Dr Una Robertson. kettle.example/saturday-night/s3/e7"
    );

    // Not on air yet: not in the feed.
    let (code, xml) = send(
        &app,
        Request::get("/feeds/saturday-night.xml")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(!String::from_utf8_lossy(&xml).contains("<item>"));

    // On air: frozen.
    let (code, body) = json(
        &app,
        "POST",
        "/api/v1/shows/saturday-night/series/3/episodes/7/air",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["resolved"]["frozen"], true);
    let (code, _) = json(
        &app,
        "PATCH",
        "/api/v1/shows/saturday-night/series/3/episodes/7",
        serde_json::json!({"title": "Rewritten"}),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::CONFLICT,
        "what was broadcast is not rewritten"
    );

    // The recorder's files, tagged with the card as the last step.
    std::fs::copy(
        root().join("crates/feeds/tests/fixtures/tone.mp3"),
        episodes_dir.join("saturday-night-s3e7.mp3"),
    )
    .unwrap();
    std::fs::copy(
        root().join("assets/carts-demo/ding-1.opus"),
        episodes_dir.join("saturday-night-s3e7.opus"),
    )
    .unwrap();
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night/series/3/episodes/7/files",
        serde_json::json!({"enclosure": "saturday-night-s3e7.mp3", "opus": "saturday-night-s3e7.opus"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let square = std::fs::read(
        dir.join("cards")
            .join(body["episode"]["cards"]["square"].as_str().unwrap()),
    )
    .unwrap();
    for f in ["saturday-night-s3e7.mp3", "saturday-night-s3e7.opus"] {
        let back = weavesignal_feeds::tags::read(&episodes_dir.join(f)).unwrap();
        assert_eq!(back.title.as_deref(), Some("Ghost Train night"));
        assert_eq!(back.track, Some(7));
        assert!(
            back.cover.as_deref() == Some(&square[..]),
            "{f} carries the card"
        );
    }
    let (code, _) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night/series/3/episodes/7/files",
        serde_json::json!({"enclosure": "../../etc/passwd"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    let (code, _) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night/series/3/episodes/7/files",
        serde_json::json!({"enclosure": "saturday-night-s3e7.opus"}),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an Opus enclosure (ADR-0018)"
    );

    // The feed: the aired episode, both enclosures, its card.
    let (code, xml) = send(
        &app,
        Request::get("/feeds/saturday-night.xml")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let xml = String::from_utf8(xml).unwrap();
    assert_eq!(xml.matches("<item>").count(), 1, "{xml}");
    assert!(xml.contains("<title>Ghost Train night</title>"));
    assert!(xml.contains(
        r#"<enclosure url="https://kettle.example/media/episodes/saturday-night-s3e7.mp3""#
    ));
    assert!(xml.contains(r#"type="audio/ogg""#) && xml.contains(r#"codecs="opus""#));
    assert!(xml.contains(
        r#"<podcast:source uri="https://kettle.example/media/episodes/saturday-night-s3e7.opus"/>"#
    ));
    assert!(xml.contains(&format!(
        r#"<itunes:image href="https://kettle.example/cards/{}"/>"#,
        body["episode"]["cards"]["square"].as_str().unwrap()
    )));
    assert!(xml.contains(
        r#"<podcast:person role="guest" group="cast">Dr Una Robertson</podcast:person>"#
    ));
    assert!(
        xml.contains(r#"<podcast:locked owner="podcasts@kettle.example">yes</podcast:locked>"#)
    );
    // The guid is kept: the same on the next fetch.
    let guid = xml
        .split("<podcast:guid>")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .to_owned();
    let (_, again) = send(
        &app,
        Request::get("/feeds/saturday-night.xml")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(String::from_utf8(again).unwrap().contains(&guid));

    // The enclosure is served with ranges, as podcast apps ask for it.
    let (code, part) = send(
        &app,
        Request::get("/media/episodes/saturday-night-s3e7.mp3")
            .header(header::RANGE, "bytes=0-9")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.len(), 10);
    let (code, _) = send(
        &app,
        Request::get("/media/episodes/..%2F..%2Fcards")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_erased_guest_leaves_the_aired_card_and_its_old_files_go() {
    // ADR-0122 through the routes: the aired episode's card is drawn again without her, and
    // the old files that named her are deleted.
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("erase-{}", uuid::Uuid::new_v4()));
    let app = router(Arc::new(ShowsState {
        station,
        db: Some(pool),
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: dir.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: None,
    }));
    let ok = |(code, body): (StatusCode, serde_json::Value)| {
        assert!(code.is_success(), "{code} {body}");
        body
    };
    ok(json(
        &app,
        "PUT",
        "/api/v1/shows/kettle-talk",
        serde_json::json!({"name": "Kettle Talk", "colour_slot": 2, "layout": "slate",
            "slot": {"weekday": 2, "start": 600, "minutes": 60}, "hosts": ["Shona Campbell"]}),
    )
    .await);
    ok(json(
        &app,
        "PUT",
        "/api/v1/shows/kettle-talk/series/1",
        serde_json::json!({"starts_on": "2026-09-01"}),
    )
    .await);
    ok(json(
        &app,
        "POST",
        "/api/v1/shows/kettle-talk/series/1/episodes",
        serde_json::json!({"count": 1}),
    )
    .await);
    ok(json(
        &app,
        "PATCH",
        "/api/v1/shows/kettle-talk/series/1/episodes/1",
        serde_json::json!({"title": "The ferries", "guests": ["Morag Ross", "Gregor Stewart"]}),
    )
    .await);
    let aired = ok(json(
        &app,
        "POST",
        "/api/v1/shows/kettle-talk/series/1/episodes/1/air",
        serde_json::Value::Null,
    )
    .await);
    let old = aired["episode"]["cards"].clone();
    assert!(old["alt"].as_str().unwrap().contains("Morag Ross"));
    let old_link = old["link"].as_str().unwrap().to_owned();
    assert!(dir.join("cards").join(&old_link).exists());

    let people = ok(json(&app, "GET", "/api/v1/people", serde_json::Value::Null).await);
    let morag = people
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Morag Ross")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let gone = ok(json(
        &app,
        "POST",
        &format!("/api/v1/people/{morag}/erase"),
        serde_json::Value::Null,
    )
    .await);
    assert_eq!(gone["name"], "Morag Ross");
    assert_eq!(gone["episodes"], 1);
    assert_eq!(gone["cards_redrawn"], 1);
    assert!(gone["card_files_deleted"].as_u64().unwrap() >= 1, "{gone}");
    assert!(
        !dir.join("cards").join(&old_link).exists(),
        "the old card is deleted"
    );

    let series = ok(json(
        &app,
        "GET",
        "/api/v1/shows/kettle-talk/series/1",
        serde_json::Value::Null,
    )
    .await);
    let ep = &series["episodes"][0];
    let alt = ep["episode"]["cards"]["alt"].as_str().unwrap();
    assert!(!alt.contains("Morag Ross"), "{alt}");
    assert!(alt.contains("Gregor Stewart"), "{alt}");
    assert_eq!(
        ep["resolved"]["guests"],
        serde_json::json!(["Gregor Stewart"])
    );
    let new_link = ep["episode"]["cards"]["link"].as_str().unwrap();
    assert!(dir.join("cards").join(new_link).exists());

    let (code, _) = json(
        &app,
        "POST",
        &format!("/api/v1/people/{morag}/erase"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "erased once");
}

/// The first `<guid>` of a feed's items (the episode's id), and the channel's `podcast:guid`.
fn guids(xml: &str) -> (String, String) {
    let part = |after: &str, close: &str| {
        let rest = xml.split(after).nth(1).expect(after);
        let rest = rest.split_once('>').unwrap().1;
        rest.split(close).next().unwrap().to_owned()
    };
    (
        part("<item>", "</guid>")
            .split("<guid")
            .nth(1)
            .map(|g| g.split_once('>').unwrap().1.to_owned())
            .unwrap_or_default(),
        part("<podcast:guid", "</podcast:guid>"),
    )
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn staff_correct_an_aired_episode_and_the_feed_keeps_its_id() {
    // ADR-0177 through the routes, and ADR-0176 on the save routes: words the cards cannot
    // draw are refused with the field named, and nothing is kept.
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    // A producer, signed in: the log names her, not the station key.
    let account = weavesignal_model::staff::create_account(
        &pool,
        station.id,
        "morag",
        "Morag Ross",
        weavesignal_model::staff::Role::Producer,
        &weavesignal_model::staff::hash_password("a long enough phrase").unwrap(),
    )
    .await
    .unwrap();
    let token = weavesignal_model::staff::new_token();
    weavesignal_model::staff::create_session(
        &pool,
        station.id,
        account.id,
        &weavesignal_model::staff::token_hash(&token),
        weavesignal_control::hub::now_ms() + 3_600_000,
    )
    .await
    .unwrap();
    // Another producer, who did not make the episodes (ADR-0198).
    let other = weavesignal_model::staff::create_account(
        &pool,
        station.id,
        "gregor",
        "Gregor Stewart",
        weavesignal_model::staff::Role::Producer,
        &weavesignal_model::staff::hash_password("another long phrase").unwrap(),
    )
    .await
    .unwrap();
    let other_token = weavesignal_model::staff::new_token();
    weavesignal_model::staff::create_session(
        &pool,
        station.id,
        other.id,
        &weavesignal_model::staff::token_hash(&other_token),
        weavesignal_control::hub::now_ms() + 3_600_000,
    )
    .await
    .unwrap();
    let staff = Arc::new(weavesignal_control::staff::Staff::new(Some(pool.clone())));
    staff.restore(station.id).await;
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fix-{}", uuid::Uuid::new_v4()));
    let episodes_dir = dir.join("episodes");
    std::fs::create_dir_all(&episodes_dir).unwrap();
    let app = router(Arc::new(ShowsState {
        station,
        db: Some(pool),
        admin_token: Some(ADMIN.into()),
        staff,
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            place: "Dunfermline".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: episodes_dir.clone(),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: None,
    }));
    let ok = |(code, body): (StatusCode, serde_json::Value)| {
        assert!(code.is_success(), "{code} {body}");
        body
    };
    let base = "/api/v1/shows/kettle-talk";
    let ep = format!("{base}/series/1/episodes/1");

    // ADR-0176: a show name in a script the cards lack is refused, naming the field.
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/kanji",
        serde_json::json!({"name": "\u{6f22}\u{5b57} Hour", "colour_slot": 1, "layout": "poster"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("show name"),
        "{body}"
    );
    let (code, _) = json(&app, "GET", "/api/v1/shows/kanji", serde_json::Value::Null).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "nothing was kept");

    ok(json(
        &app,
        "PUT",
        base,
        serde_json::json!({"name": "Kettle Talk", "colour_slot": 2, "layout": "slate",
            "slot": {"weekday": 2, "start": 600, "minutes": 60}, "hosts": ["Shona Campbell"]}),
    )
    .await);
    ok(json(
        &app,
        "PUT",
        &format!("{base}/series/1"),
        serde_json::json!({"starts_on": "2026-09-01"}),
    )
    .await);
    // ... on an episode's title, a guest's name and a series' title too, and no guest is
    // remembered from the refused save.
    // Morag drafts them, so she may correct them once aired (ADR-0198).
    ok(json_as(
        &app,
        &token,
        "POST",
        &format!("{base}/series/1/episodes"),
        serde_json::json!({"count": 2}),
    )
    .await);
    for (patch, field) in [
        (
            serde_json::json!({"title": "\u{4f60}\u{597d}"}),
            "episode title",
        ),
        (
            serde_json::json!({"guests": ["\u{05e9}\u{05dc}\u{05d5}\u{05dd}"]}),
            "guest name",
        ),
        (
            serde_json::json!({"hosts": ["\u{0928}\u{092e}"]}),
            "host name",
        ),
    ] {
        let (code, body) = json(&app, "PATCH", &ep, patch).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains(field),
            "{field}: {body}"
        );
    }
    let (code, body) = json(
        &app,
        "PUT",
        &format!("{base}/series/1"),
        serde_json::json!({"title": "\u{4f60}\u{597d}"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("series title"),
        "{body}"
    );
    // Drafting an episode checks its first title and guests too, before it keeps either.
    let (code, body) = json(
        &app,
        "POST",
        &format!("{base}/series/1/episodes"),
        serde_json::json!({"title": "Ferries", "guests": ["\u{05e9}\u{05dc}\u{05d5}\u{05dd}"]}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("guest name"),
        "{body}"
    );
    let drafted = ok(json(
        &app,
        "GET",
        &format!("{base}/series/1"),
        serde_json::Value::Null,
    )
    .await);
    assert_eq!(
        drafted["episodes"].as_array().map(Vec::len),
        Some(2),
        "the refused draft kept no episode: {drafted}"
    );
    let people = ok(json(&app, "GET", "/api/v1/people", serde_json::Value::Null).await);
    assert!(
        people
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["name"] == "Shona Campbell"),
        "no one from a refused save is remembered: {people}"
    );
    let body = ok(json(
        &app,
        "GET",
        &ep.replace("/episodes/1", ""),
        serde_json::Value::Null,
    )
    .await);
    assert_eq!(
        body["episodes"][0]["episode"]["title"], "",
        "the episode is as it was"
    );

    // ADR-0188 through the routes: the kind is carried by the API.
    assert_eq!(body["episodes"][0]["episode"]["kind"], "live");
    let marked = ok(json(
        &app,
        "PATCH",
        &ep,
        serde_json::json!({"kind": "prerecorded"}),
    )
    .await);
    assert_eq!(marked["episode"]["kind"], "prerecorded");
    assert_eq!(marked["resolved"]["episode"]["kind"], "prerecorded");
    let (code, _) = json(&app, "PATCH", &ep, serde_json::json!({"kind": "studio-b"})).await;
    assert!(code.is_client_error(), "only live or prerecorded");

    // Episode 1: title and guests as first typed, aired, its files published.
    ok(json(
        &app,
        "PATCH",
        &ep,
        serde_json::json!({"title": "The ferrys", "guests": ["Morag Ross", "Gregor Stuart"]}),
    )
    .await);
    // Not aired: nothing to correct yet.
    let fix = serde_json::json!({
        "title": "The ferries", "guests": ["Morag Ross", "Gregor Stewart"],
        "reason": "Misspelt the title and Gregor's surname."
    });
    let (code, body) = json_as(
        &app,
        &token,
        "POST",
        &format!("{ep}/corrections"),
        fix.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    ok(json(&app, "POST", &format!("{ep}/air"), serde_json::Value::Null).await);
    std::fs::copy(
        root().join("crates/feeds/tests/fixtures/tone.mp3"),
        episodes_dir.join("kettle-talk-s1e1.mp3"),
    )
    .unwrap();
    let before = ok(json(
        &app,
        "PUT",
        &format!("{ep}/files"),
        serde_json::json!({"enclosure": "kettle-talk-s1e1.mp3"}),
    )
    .await);
    let old_link = before["episode"]["cards"]["link"]
        .as_str()
        .unwrap()
        .to_owned();
    let feed = || async {
        let (code, xml) = send(
            &app,
            Request::get("/feeds/kettle-talk.xml")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        String::from_utf8(xml).unwrap()
    };
    let xml = feed().await;
    assert!(xml.contains("<title>The ferrys</title>") && xml.contains("Gregor Stuart"));
    let ids = guids(&xml);
    assert!(!ids.0.is_empty(), "{xml}");

    // The old way is still closed: an aired episode refuses a plain edit.
    let (code, _) = json(&app, "PATCH", &ep, serde_json::json!({"title": "x"})).await;
    assert_eq!(code, StatusCode::CONFLICT);

    // Refused corrections keep nothing: no bearer, no reason, no change, words undrawable.
    let url = format!("{ep}/corrections");
    let (code, _) = send(
        &app,
        Request::post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(fix.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    let (code, _) = json_as(
        &app,
        &token,
        "POST",
        &url,
        serde_json::json!({"title": "The ferries"}),
    )
    .await;
    assert!(code.is_client_error(), "a reason is needed");
    let (code, body) = json_as(
        &app,
        &token,
        "POST",
        &url,
        serde_json::json!({"title": "The ferrys", "reason": "No change at all."}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (code, body) = json_as(
        &app,
        &token,
        "POST",
        &url,
        serde_json::json!({"guests": ["\u{4f60}\u{597d}"], "reason": "A guest in Chinese."}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("guest name"),
        "{body}"
    );
    let log = ok(json(&app, "GET", &url, serde_json::Value::Null).await);
    assert!(log.as_array().unwrap().is_empty(), "{log}");

    // ADR-0198: a producer who did not make the episode may not correct it; nothing is kept.
    let (code, body) = json_as(&app, &other_token, "POST", &url, fix.clone()).await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("made this episode"),
        "{body}"
    );
    let log = ok(json(&app, "GET", &url, serde_json::Value::Null).await);
    assert!(log.as_array().unwrap().is_empty(), "{log}");

    // The correction: cards redrawn, files tagged again, feed item changed under the same id.
    let done = ok(json_as(&app, &token, "POST", &url, fix).await);
    assert_eq!(done["correction"]["made_by"], "morag", "{done}");
    assert_eq!(
        done["correction"]["reason"],
        "Misspelt the title and Gregor's surname."
    );
    assert_eq!(done["correction"]["old_values"]["title"], "The ferrys");
    assert_eq!(done["correction"]["new_values"]["title"], "The ferries");
    assert_eq!(done["resolved"]["frozen"], true);
    assert_eq!(done["resolved"]["episode"]["title"], "The ferries");
    assert_eq!(done["files_retagged"], true, "{done}");
    let alt = done["episode"]["cards"]["alt"].as_str().unwrap();
    assert!(
        alt.contains("The ferries") && alt.contains("Gregor Stewart"),
        "{alt}"
    );
    let new_link = done["episode"]["cards"]["link"].as_str().unwrap();
    assert_ne!(new_link, old_link, "a changed card has a new name");
    assert!(dir.join("cards").join(new_link).exists());
    let tags = weavesignal_feeds::tags::read(&episodes_dir.join("kettle-talk-s1e1.mp3")).unwrap();
    assert_eq!(tags.title.as_deref(), Some("The ferries"));
    let square = std::fs::read(
        dir.join("cards")
            .join(done["episode"]["cards"]["square"].as_str().unwrap()),
    )
    .unwrap();
    assert!(
        tags.cover.as_deref() == Some(&square[..]),
        "the file carries the new card"
    );

    let xml = feed().await;
    assert_eq!(xml.matches("<item>").count(), 1);
    assert!(xml.contains("<title>The ferries</title>") && !xml.contains("The ferrys"));
    assert!(xml.contains("Gregor Stewart") && !xml.contains("Gregor Stuart"));
    assert_eq!(guids(&xml), ids, "the item's guid and the feed's are kept");

    // The log names who and why, and keeps the frozen original.
    let log = ok(json(&app, "GET", &url, serde_json::Value::Null).await);
    assert_eq!(log.as_array().unwrap().len(), 1);
    assert_eq!(log[0]["made_by"], "morag");
    assert_eq!(
        log[0]["old_values"]["guests"],
        serde_json::json!(["Morag Ross", "Gregor Stuart"])
    );

    // A correction by the station key is logged as the station key.
    let second = ok(json(
        &app,
        "POST",
        &url,
        serde_json::json!({"description": "The island ferries.", "reason": "Added a description."}),
    )
    .await);
    assert_eq!(second["correction"]["made_by"], "station key");

    // Erasing the misspelt guest redacts the log as well as what aired.
    let people = ok(json(&app, "GET", "/api/v1/people", serde_json::Value::Null).await);
    let stuart = people
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Gregor Stuart")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let gone = ok(json(
        &app,
        "POST",
        &format!("/api/v1/people/{stuart}/erase"),
        serde_json::Value::Null,
    )
    .await);
    assert_eq!(gone["corrections_redacted"], 1, "{gone}");
    let log = ok(json(&app, "GET", &url, serde_json::Value::Null).await);
    assert_eq!(log[0]["redacted"], true);
    assert!(!log.to_string().contains("Gregor Stuart"), "{log}");
    assert_eq!(
        log[0]["old_values"]["title"], "The ferrys",
        "the rest of the original stays"
    );
}

// ---- the guided New show: rules, previews, clashes (release 1.0.4, ADR-0278) ----
//
// Dates are in 2036, so that "never into the past" keeps the tests true as the clock moves.
// 2036-09-02 is a Tuesday, 2036-12-02 the first Tuesday of December, 2036-03-30 the Sunday the
// clocks go forward.

use weavesignal_model::staff::Role;

struct Rig {
    app: axum::Router,
    pool: sqlx::PgPool,
    station: StationId,
    /// The cards' and the published files' folders.
    dir: PathBuf,
    producer: String,
    presenter: String,
    admin: String,
    screener: String,
}

async fn rig() -> Rig {
    rig_at("https://kettle.example").await
}

/// The rig with the station's public address as given (`publishing.public_url`).
async fn rig_at(public_url: &str) -> Rig {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let mut tokens = Vec::new();
    for (name, role) in [
        ("morag", Role::Producer),
        ("shona", Role::Presenter),
        ("alasdair", Role::Admin),
        ("ishbel", Role::Screener),
    ] {
        let account = weavesignal_model::staff::create_account(
            &pool,
            station.id,
            name,
            name,
            role,
            &weavesignal_model::staff::hash_password("a long enough phrase").unwrap(),
        )
        .await
        .unwrap();
        let token = weavesignal_model::staff::new_token();
        weavesignal_model::staff::create_session(
            &pool,
            station.id,
            account.id,
            &weavesignal_model::staff::token_hash(&token),
            weavesignal_control::hub::now_ms() + 3_600_000,
        )
        .await
        .unwrap();
        tokens.push(token);
    }
    let staff = Arc::new(weavesignal_control::staff::Staff::new(Some(pool.clone())));
    staff.restore(station.id).await;
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("rules-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("episodes")).unwrap();
    let station_id = station.id;
    let app = router(Arc::new(ShowsState {
        station,
        db: Some(pool.clone()),
        admin_token: Some(ADMIN.into()),
        staff,
        publishing: PublishingConfig {
            public_url: public_url.into(),
            place: "Dunfermline".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: dir.join("episodes"),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: Some(weavesignal_control::config::VideoConfig {
            still_file: dir.join("stills/slate.png"),
            ..Default::default()
        }),
    }));
    let screener = tokens.pop().unwrap();
    let admin = tokens.pop().unwrap();
    let presenter = tokens.pop().unwrap();
    let producer = tokens.pop().unwrap();
    Rig {
        app,
        pool,
        station: station_id,
        dir,
        producer,
        presenter,
        admin,
        screener,
    }
}

fn ok_body((code, body): (StatusCode, serde_json::Value)) -> serde_json::Value {
    assert!(code.is_success(), "{code} {body}");
    body
}

fn show_body(name: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut v = serde_json::json!({"name": name, "colour_slot": 3, "layout": "poster"});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn weekly(weekday: u8, start: u16, minutes: u16, starts_on: &str) -> serde_json::Value {
    serde_json::json!({
        "starts_on": starts_on, "start": start, "minutes": minutes,
        "repeat": {"kind": "weekly", "every": 1, "weekdays": [weekday]}
    })
}

fn first_tuesday() -> serde_json::Value {
    serde_json::json!({
        "starts_on": "2036-12-02", "start": 1200, "minutes": 60,
        "repeat": {"kind": "monthly_nth", "every": 1, "nth": 1, "weekday": 2},
        "end": {"kind": "count", "count": 3}
    })
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_new_show_is_made_with_its_first_episode_in_one_save_and_a_count_counts_episodes() {
    let r = rig().await;
    let body = ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club",
            show_body(
                "The Folk Club",
                serde_json::json!({"rule": first_tuesday(), "draft_first": true, "step": "solo"}),
            ),
        )
        .await,
    );
    assert_eq!(body["show"]["rule"]["repeat"]["kind"], "monthly_nth");
    assert!(
        body["show"]["slot"].is_null(),
        "not slot-shaped, so an older studio sees none"
    );
    assert_eq!(
        body["show"]["rule_words"]["sentence"],
        "Every month on the first Tuesday, 20:00 to 21:00, from 2 December 2036, for 3 episodes."
    );
    assert_eq!(body["show"]["rule_words"]["last_on"], "2037-02-03");
    assert_eq!(body["show"]["next_on"]["on"], "2036-12-02");
    assert_eq!(body["series"].as_array().unwrap().len(), 1);
    assert_eq!(body["series"][0]["number"], 1);
    assert!(
        body["series"][0]["rule"].is_null(),
        "series 1 follows the show"
    );
    let ep = &body["drafted"]["episode"];
    assert_eq!(ep["number"], 1);
    assert_eq!(ep["occurrence"], "2036-12-02");

    // The same save sent again (a retry) drafts nothing more.
    let again = ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club",
            show_body(
                "The Folk Club",
                serde_json::json!({"rule": first_tuesday(), "draft_first": true, "step": "solo"}),
            ),
        )
        .await,
    );
    assert!(again["drafted"].is_null());
    assert_eq!(again["show"]["rule_words"]["last_on"], "2037-02-03");

    // The rest of the run: two more, then it is complete.
    let (code, made) = json(
        &r.app,
        "POST",
        "/api/v1/shows/folk-club/series/1/episodes",
        serde_json::json!({"count": 5}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let dates: Vec<&str> = made
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["episode"]["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, ["2037-01-06", "2037-02-03"]);
    let (code, body) = json(
        &r.app,
        "POST",
        "/api/v1/shows/folk-club/series/1/episodes",
        serde_json::json!({"count": 1}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    let shown = ok_body(
        json(
            &r.app,
            "GET",
            "/api/v1/shows/folk-club",
            serde_json::Value::Null,
        )
        .await,
    );
    assert_eq!(shown["show"]["next_on"]["on"], "2036-12-02", "still to air");

    // A skipped date does not use up an episode: the run is one date longer.
    let skipped = ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club2",
            show_body(
                "The Folk Club Again",
                serde_json::json!({"rule": {
                    "starts_on": "2036-12-02", "start": 1200,
                    "repeat": {"kind": "monthly_nth", "every": 1, "nth": 1, "weekday": 2},
                    "end": {"kind": "count", "count": 3}, "skip": ["2037-01-06"]
                }}),
            ),
        )
        .await,
    );
    // 2037-01-06 is the first Tuesday of January 2037.
    assert_eq!(skipped["show"]["rule_words"]["last_on"], "2037-03-03");
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club2/series/1",
            serde_json::json!({}),
        )
        .await,
    );
    let (code, made) = json(
        &r.app,
        "POST",
        "/api/v1/shows/folk-club2/series/1/episodes",
        serde_json::json!({"count": 9}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    let dates: Vec<&str> = made
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["episode"]["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, ["2036-12-02", "2037-02-03", "2037-03-03"]);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_presenter_is_refused_a_clash_and_nothing_is_saved_while_others_are_warned() {
    let r = rig().await;
    // Saturday Night Kettle: Saturdays 20:00 to 23:00, the first drafted.
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/saturday-night",
            show_body(
                "Saturday Night Kettle",
                serde_json::json!({"rule": weekly(6, 1200, 180, "2036-09-06"), "draft_first": true,
                                    "hosts": ["Shona Campbell"]}),
            ),
        )
        .await,
    );
    // A show with a rule and no episode yet: Fridays 18:00 to 19:00.
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/friday-late",
            show_body(
                "Friday Late",
                serde_json::json!({"rule": weekly(5, 1080, 60, "2036-09-05")}),
            ),
        )
        .await,
    );
    let once = |date: &str, start: u16| serde_json::json!({"starts_on": date, "start": start, "minutes": 60, "repeat": {"kind": "once"}});
    // Over a drafted episode: the presenter is refused, naming the other show.
    let (code, body) = json_as(
        &r.app,
        &r.presenter,
        "PUT",
        "/api/v1/shows/newcomer",
        show_body(
            "The Newcomer",
            serde_json::json!({"rule": once("2036-09-06", 1290), "draft_first": true}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["field"], "rule");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Saturday Night Kettle has that time."),
        "{body}"
    );
    assert_eq!(body["clashes"][0]["slug"], "saturday-night");
    assert_eq!(body["clashes"][0]["kind"], "episode");
    assert_eq!(body["clashes"][0]["overlap_minutes"], 60);
    assert_eq!(body["clashes"][0]["hosts"][0], "Shona Campbell");
    let (code, _) = json(
        &r.app,
        "GET",
        "/api/v1/shows/newcomer",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "the show was not made either");
    // Over a date another show's rule gives, with no episode yet.
    let (code, body) = json_as(
        &r.app,
        &r.presenter,
        "PUT",
        "/api/v1/shows/newcomer",
        show_body(
            "The Newcomer",
            serde_json::json!({"rule": once("2036-09-05", 1110), "draft_first": true}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["clashes"][0]["kind"], "rule");
    assert_eq!(body["clashes"][0]["slug"], "friday-late");
    // The preview says beforehand what would be refused, on the first date only.
    let (code, preview) = json_as(
        &r.app,
        &r.presenter,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": weekly(5, 1110, 60, "2036-09-05"), "limit": 3}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{preview}");
    assert_eq!(preview["clash_count"], 3);
    // Every date is in Friday Late's way (18:00 to 19:00): the nearest clear start, half an hour
    // at a time and later first, is 19:00.
    assert_eq!(preview["clear"]["start"], 1140, "{preview}");
    assert_eq!(preview["clear"]["minutes"], 60);
    // A one-off inside Saturday Night Kettle (20:00 to 23:00): 23:00 is nearer than 19:00.
    let (_, one) = json(
        &r.app,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": once("2036-09-06", 1290), "limit": 3}),
    )
    .await;
    assert_eq!(one["clear"]["start"], 1380, "{one}");
    // A date that clashes alone is put right on its row: nothing is offered.
    let (_, clear) = json(
        &r.app,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": once("2036-09-06", 900), "limit": 3}),
    )
    .await;
    assert_eq!(clear["clash_count"], 0, "{clear}");
    assert!(clear["clear"].is_null(), "{clear}");
    assert_eq!(preview["dates"][0]["first"], true);
    assert_eq!(preview["dates"][0]["refused"], true);
    assert_eq!(preview["dates"][1]["refused"], false);
    assert_eq!(preview["dates"][1]["clashes"][0]["slug"], "friday-late");
    // A presenter who saves without drafting is not refused: later dates are looked at only
    // when they come to be drafted.
    ok_body(
        json_as(
            &r.app,
            &r.presenter,
            "PUT",
            "/api/v1/shows/newcomer",
            show_body(
                "The Newcomer",
                serde_json::json!({"rule": once("2036-09-05", 1110)}),
            ),
        )
        .await,
    );
    let (code, body) = json_as(
        &r.app,
        &r.presenter,
        "POST",
        "/api/v1/shows/newcomer/series/1/episodes",
        serde_json::json!({"count": 1}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "no series yet: {body}");
    // A producer is warned and goes on.
    let body = ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PUT",
            "/api/v1/shows/pre-empt",
            show_body(
                "Pre-empt",
                serde_json::json!({"rule": once("2036-09-06", 1290), "draft_first": true}),
            ),
        )
        .await,
    );
    assert_eq!(body["warnings"][0]["slug"], "saturday-night");
    assert_eq!(body["drafted"]["episode"]["number"], 1);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn previewing_a_rule_says_it_back_marks_skips_and_the_clocks_and_writes_nothing() {
    let r = rig().await;
    let preview = |body: serde_json::Value| async {
        let (code, v) = json(&r.app, "POST", "/api/v1/schedule/rule/preview", body).await;
        assert_eq!(code, StatusCode::OK, "{v}");
        v
    };
    // 01:30 on Sundays across the clocks going forward (30 March 2036).
    let v = preview(serde_json::json!({
        "rule": {"starts_on": "2036-03-23", "start": 90, "minutes": 60,
                 "repeat": {"kind": "weekly", "every": 1, "weekdays": [7]},
                 "skip": ["2036-04-06"]},
        "limit": 3
    }))
    .await;
    assert_eq!(v["ok"], true);
    assert_eq!(v["zone"], "Europe/London");
    let d = v["dates"].as_array().unwrap();
    assert_eq!(
        d.len(),
        4,
        "three that air and one struck through between them"
    );
    assert_eq!(d[0]["on"], "2036-03-23");
    assert_eq!(d[1]["on"], "2036-03-30");
    assert_eq!(d[1]["clock"], "forward");
    assert_eq!(
        d[1]["start"], "02:30",
        "airs an hour late, as a slot always did"
    );
    assert_eq!(d[1]["end"], "03:30");
    assert_eq!(d[2]["on"], "2036-04-06");
    assert_eq!(d[2]["state"], "skipped");
    assert_eq!(d[3]["on"], "2036-04-13");
    assert_eq!(d[3]["state"], "airs");
    assert_eq!(v["more"], true);
    assert_eq!(d[0]["number"], 1);
    assert_eq!(d[3]["number"], 3, "a skipped date takes no number");

    // Nothing was written.
    let all = ok_body(json(&r.app, "GET", "/api/v1/shows", serde_json::Value::Null).await);
    assert!(all.as_array().unwrap().is_empty());

    // A one-off whose time has passed, and a rule with a mistake in it, by field.
    let v = preview(serde_json::json!({
        "rule": {"starts_on": "2020-01-06", "start": 600, "repeat": {"kind": "once"}}
    }))
    .await;
    assert_eq!(v["ok"], false);
    assert_eq!(v["problems"][0]["field"], "starts_on");
    assert!(
        v["problems"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("That time has passed.")
    );
    let v = preview(serde_json::json!({
        "rule": {"starts_on": "2036-09-02", "start": 1500, "minutes": 60,
                 "repeat": {"kind": "weekly", "every": 13, "weekdays": []}}
    }))
    .await;
    assert_eq!(v["ok"], false);
    let fields: Vec<&str> = v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["start", "repeat.every", "repeat.weekdays"]);
    assert!(v["words"].is_null());
    let v = preview(serde_json::json!({
        "rule": {"starts_on": "2036-09-02", "start": 600, "repeat": {"kind": "weekly", "every": 1, "weekdays": [2]},
                 "end": {"kind": "until", "until": "2036-08-01"}}
    }))
    .await;
    assert_eq!(v["problems"][0]["field"], "end");
    assert!(
        v["problems"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("This makes no dates.")
    );

    // Refused as a request: an unknown field, and a limit out of range.
    let (code, _) = json(
        &r.app,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": {"start": 600, "repeat": {"kind": "once"}}, "colour": 1}),
    )
    .await;
    assert!(code.is_client_error());
    let (code, _) = json(
        &r.app,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": weekly(2, 600, 60, "2036-09-02"), "limit": 61}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn changing_a_rule_moves_the_drafted_episodes_in_order_and_leaves_the_rest() {
    let r = rig().await;
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/tuesdays",
            show_body(
                "Tuesdays",
                serde_json::json!({"rule": weekly(2, 1200, 60, "2036-09-02"), "draft_first": true}),
            ),
        )
        .await,
    );
    let base = "/api/v1/shows/tuesdays/series/1";
    let (code, made) = json(
        &r.app,
        "POST",
        &format!("{base}/episodes"),
        serde_json::json!({"count": 4}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");
    // Titles that must survive the move; episode 3 is moved by hand to the Wednesday evening.
    for (n, title) in [(2, "Second"), (4, "Fourth"), (5, "Fifth")] {
        ok_body(
            json(
                &r.app,
                "PATCH",
                &format!("{base}/episodes/{n}"),
                serde_json::json!({"title": title}),
            )
            .await,
        );
    }
    ok_body(
        json(
            &r.app,
            "PATCH",
            &format!("{base}/episodes/3"),
            serde_json::json!({"title": "By hand", "air_at": "2036-09-17T21:00:00+01:00[Europe/London]"}),
        )
        .await,
    );
    // Episode 1 airs: frozen.
    ok_body(
        json(
            &r.app,
            "POST",
            &format!("{base}/episodes/1/air"),
            serde_json::Value::Null,
        )
        .await,
    );

    // The preview says what saving would do, before it is saved.
    let new_rule = weekly(3, 1200, 60, "2036-09-03");
    let (code, v) = json(
        &r.app,
        "POST",
        "/api/v1/schedule/rule/preview",
        serde_json::json!({"rule": new_rule, "show": "tuesdays"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["change"]["aired"], 1);
    let moved: Vec<(i64, &str)> = v["change"]["moved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["number"].as_i64().unwrap(), m["to_on"].as_str().unwrap()))
        .collect();
    assert_eq!(
        moved,
        [(2, "2036-09-03"), (4, "2036-09-10"), (5, "2036-09-17")]
    );
    assert_eq!(v["change"]["by_hand"][0]["number"], 3);
    let (_, shown) = json(&r.app, "GET", base, serde_json::Value::Null).await;
    assert_eq!(
        shown["episodes"][1]["episode"]["occurrence"], "2036-09-09",
        "nothing saved yet"
    );

    // Saved: they move, keeping their numbers and titles.
    let (code, v) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/tuesdays/rule",
        serde_json::json!({"rule": new_rule, "drafted": "move"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["aired"], 1);
    assert_eq!(v["moved"].as_array().unwrap().len(), 3);
    assert_eq!(v["by_hand"][0]["number"], 3);
    assert_eq!(v["rule_words"]["short"], "Wed, 20:00–21:00");
    let (_, shown) = json(&r.app, "GET", base, serde_json::Value::Null).await;
    let eps = shown["episodes"].as_array().unwrap();
    let at = |i: usize| {
        (
            eps[i]["episode"]["number"].as_i64().unwrap(),
            eps[i]["episode"]["title"].as_str().unwrap().to_owned(),
            eps[i]["episode"]["occurrence"].as_str().unwrap().to_owned(),
            eps[i]["resolved"]["episode"]["air_at"].as_i64().unwrap(),
        )
    };
    // 2036-09-03 20:00 BST is 19:00 UTC.
    let wed = |day: u32| {
        format!("2036-09-{day:02}T19:00:00Z")
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_second()
    };
    assert_eq!(
        at(0).2,
        "2036-09-02",
        "the aired episode is frozen where it was"
    );
    assert_eq!(at(1), (2, "Second".into(), "2036-09-03".into(), wed(3)));
    assert_eq!(at(2).2, "2036-09-16", "a person put it there, so it stays");
    assert_eq!(at(3), (4, "Fourth".into(), "2036-09-10".into(), wed(10)));
    assert_eq!(at(4), (5, "Fifth".into(), "2036-09-17".into(), wed(17)));

    // Logged, with who; and a change that leaves them.
    let (code, v) = json_as(
        &r.app,
        &r.producer,
        "PUT",
        "/api/v1/shows/tuesdays/rule",
        serde_json::json!({"rule": weekly(4, 1200, 60, "2036-09-04"), "drafted": "leave"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["moved"].as_array().unwrap().len(), 0);
    assert_eq!(v["left"].as_array().unwrap().len(), 3);
    let log: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT c.by_label, c.drafted, c.moved FROM show_rule_change c
         JOIN show s ON s.id = c.show_id WHERE s.slug = 'tuesdays' ORDER BY c.id",
    )
    .fetch_all(&r.pool)
    .await
    .unwrap();
    assert_eq!(
        log.iter()
            .map(|(_, d, m)| (d.as_str(), *m))
            .collect::<Vec<_>>(),
        // Making the show is not a change; the two changes after it are.
        [("move", 3), ("leave", 0)]
    );
    assert_eq!(log[1].0, "morag");

    // `rule` must be sent: leaving it out is a mistake, not a clear.
    let (code, _) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/tuesdays/rule",
        serde_json::json!({"drafted": "move"}),
    )
    .await;
    assert!(code.is_client_error());
    // A window's token for its save (1.0.5) is taken; one that is not a plain token is refused.
    let (code, v) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/tuesdays/rule",
        serde_json::json!({"rule": weekly(4, 1200, 60, "2036-09-04"), "drafted": "leave", "change": "3f2a9c1e-7b4d"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let (code, _) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/tuesdays/rule",
        serde_json::json!({"rule": weekly(4, 1200, 60, "2036-09-04"), "drafted": "leave", "change": "not a token"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    // A series' own rule, and following the show again.
    let (code, v) = json(
        &r.app,
        "PUT",
        &format!("{base}/rule"),
        serde_json::json!({"rule": weekly(5, 1200, 60, "2036-09-05"), "drafted": "leave"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["series"]["rule"]["repeat"]["weekdays"][0], 5);
    let (code, v) = json(
        &r.app,
        "PUT",
        &format!("{base}/rule"),
        serde_json::json!({"rule": null}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert!(v["series"]["rule"].is_null());
    let (code, _) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/nobody/rule",
        serde_json::json!({"rule": null}),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_older_studio_keeps_a_rule_it_cannot_see_and_a_slot_still_means_a_weekly_rule() {
    let r = rig().await;
    let put = |extra: serde_json::Value| {
        let app = r.app.clone();
        async move {
            ok_body(
                json(
                    &app,
                    "PUT",
                    "/api/v1/shows/wobble",
                    show_body("Wobble", extra),
                )
                .await,
            )
        }
    };
    // A monthly rule; an older studio saves the show again without a slot (null) and without a
    // rule: it could not see the rule, so it is kept.
    let v = put(serde_json::json!({"rule": first_tuesday()})).await;
    assert_eq!(v["show"]["rule"]["repeat"]["kind"], "monthly_nth");
    let v = put(serde_json::json!({"slot": null})).await;
    assert_eq!(v["show"]["rule"]["repeat"]["kind"], "monthly_nth", "kept");
    // It sets a weekly slot there: the weekly rule the slot means replaces the rule.
    let v = put(serde_json::json!({"slot": {"weekday": 3, "start": 600, "minutes": 60}})).await;
    assert_eq!(v["show"]["rule"]["repeat"]["weekdays"][0], 3);
    assert_eq!(v["show"]["slot"]["weekday"], 3);
    assert!(v["show"]["rule"]["starts_on"].is_null());
    // The same slot sent again with every save changes nothing.
    put(serde_json::json!({"rule": {"start": 600, "minutes": 60,
        "repeat": {"kind": "weekly", "every": 1, "weekdays": [3]}}}))
    .await;
    let v = put(serde_json::json!({"slot": {"weekday": 3, "start": 600, "minutes": 60}})).await;
    assert_eq!(v["show"]["rule"]["repeat"]["weekdays"][0], 3);
    assert_eq!(v["show"]["slot"]["start"], 600);
    // A rule with a start date is not slot-shaped: an older studio sees `slot: null`, and a slot
    // it then sets is an explicit act that replaces the rule.
    let v = put(serde_json::json!({"rule": {
        "starts_on": "2036-09-03", "start": 600, "minutes": 60,
        "repeat": {"kind": "weekly", "every": 1, "weekdays": [3]}}}))
    .await;
    assert!(v["show"]["slot"].is_null());
    let v = put(serde_json::json!({"slot": null})).await;
    assert_eq!(v["show"]["rule"]["starts_on"], "2036-09-03", "kept");
    let v = put(serde_json::json!({"slot": {"weekday": 3, "start": 600, "minutes": 60}})).await;
    assert!(v["show"]["rule"]["starts_on"].is_null());
    // Neither sent, on a slot-shaped rule: cleared, as Clear does today.
    put(serde_json::json!({"rule": null})).await;
    put(serde_json::json!({"slot": {"weekday": 3, "start": 600}})).await;
    let v = put(serde_json::json!({})).await;
    assert!(v["show"]["rule"].is_null());
    assert!(v["show"]["slot"].is_null());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_station_with_no_public_address_makes_shows_with_their_cards_off() {
    // A loopback install (1.2.1): 1.2.0 refused New show with 503 "cards are off".
    let r = rig_at("").await;
    let body = ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club",
            show_body(
                "The Folk Club",
                serde_json::json!({"rule": first_tuesday(), "draft_first": true, "hosts": ["Shona Campbell"]}),
            ),
        )
        .await,
    );
    assert_eq!(body["show"]["name"], "The Folk Club");
    assert!(
        body["show"]["cards"].is_null(),
        "no cards are kept: {}",
        body["show"]["cards"]
    );
    assert_eq!(
        body["drafted"]["episode"]["number"], 1,
        "series 1 and its first episode too"
    );
    assert!(body["drafted"]["episode"]["cards"].is_null());
    assert!(
        std::fs::read_dir(r.dir.join("cards")).map_or(true, |d| d.count() == 0),
        "no card file written"
    );
    // A change saves as well.
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/folk-club",
            show_body("The Folk Club on Tuesdays", serde_json::json!({})),
        )
        .await,
    );
    // The names are still checked against the cards: one that cannot fit is refused.
    let (code, body) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/too-long",
        show_body(
            "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society, Extended Edition, With Added Weather For The Inshore Waters And The Coastal Stations",
            serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    // Asking for the cards themselves says why they are off, and what turns them on.
    let (code, body) = json(
        &r.app,
        "POST",
        "/api/v1/shows/folk-club/cards",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let why = body["error"].as_str().unwrap();
    assert!(
        why.contains("no public address") && why.contains("weavesignal.sh --public"),
        "{why}"
    );
}

/// The studio makes the calendar links it gives out (Subscribe…, ADR-0281) from the station's
/// public address, which the week's reply carries: on Cloudron the address the studio talks to
/// is behind the login, and a link made from it would lead listeners to a login page.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_week_says_the_public_address_or_that_there_is_none() {
    let week = "/api/v1/schedule/episodes?from_ms=2114380800000&to_ms=2114985600000";
    let r = rig_at("https://kettle.example/").await;
    let v = ok_body(json(&r.app, "GET", week, serde_json::Value::Null).await);
    assert_eq!(
        v["public_url"], "https://kettle.example",
        "no trailing slash"
    );
    // Any role that reads the week reads it: a presenter subscribes to their own show.
    let (code, v) = json_as(&r.app, &r.presenter, "GET", week, serde_json::Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["public_url"], "https://kettle.example");
    // A station for this machine only has none, and says so with null (never the key left out,
    // which is how a studio tells a station too old to have calendar files).
    let r = rig_at("").await;
    let v = ok_body(json(&r.app, "GET", week, serde_json::Value::Null).await);
    assert!(v.as_object().unwrap().contains_key("public_url"));
    assert!(v["public_url"].is_null());
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn the_week_gets_the_dates_no_episode_fills() {
    let r = rig().await;
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/dotted",
            show_body(
                "Dotted",
                serde_json::json!({"rule": {
                    "starts_on": "2036-09-02", "start": 1200, "minutes": 60,
                    "repeat": {"kind": "weekly", "every": 1, "weekdays": [2]},
                    "skip": ["2036-09-09"]}, "draft_first": true}),
            ),
        )
        .await,
    );
    let from = "2036-09-01T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond();
    let to = "2036-09-30T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond();
    let v = ok_body(
        json(
            &r.app,
            "GET",
            &format!("/api/v1/schedule/episodes?from_ms={from}&to_ms={to}"),
            serde_json::Value::Null,
        )
        .await,
    );
    assert_eq!(
        v["episodes"].as_array().unwrap().len(),
        1,
        "the drafted one"
    );
    let on: Vec<&str> = v["dates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["on"].as_str().unwrap())
        .collect();
    // The first is drafted and the ninth skipped.
    assert_eq!(on, ["2036-09-16", "2036-09-23"]);
    assert_eq!(v["dates"][0]["show"], "dotted");
    assert_eq!(v["dates"][0]["number"], 2);
    assert_eq!(v["dates"][0]["minutes"], 60);
    assert_eq!(v["dates"][0]["short"], "Tue, 20:00–21:00");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn skipping_a_date_a_drafted_episode_holds_shifts_the_rest_along_in_order() {
    // Episodes swap dates inside one transaction: 1 takes 2's date, 2 takes 3's, 3 the next
    // (the deferred uniqueness of an episode's date).
    let r = rig().await;
    ok_body(
        json(
            &r.app,
            "PUT",
            "/api/v1/shows/shift",
            show_body(
                "Shift",
                serde_json::json!({"rule": weekly(2, 1200, 60, "2036-09-02"), "draft_first": true}),
            ),
        )
        .await,
    );
    let base = "/api/v1/shows/shift/series/1";
    ok_body(
        json(
            &r.app,
            "POST",
            &format!("{base}/episodes"),
            serde_json::json!({"count": 2}),
        )
        .await,
    );
    let mut skipping = weekly(2, 1200, 60, "2036-09-02");
    skipping["skip"] = serde_json::json!(["2036-09-02"]);
    let (code, v) = json(
        &r.app,
        "PUT",
        "/api/v1/shows/shift/rule",
        serde_json::json!({"rule": skipping}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let moved: Vec<(i64, &str, &str)> = v["moved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["number"].as_i64().unwrap(),
                m["from_on"].as_str().unwrap(),
                m["to_on"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        moved,
        [
            (1, "2036-09-02", "2036-09-09"),
            (2, "2036-09-09", "2036-09-16"),
            (3, "2036-09-16", "2036-09-23")
        ]
    );
    let (_, shown) = json(&r.app, "GET", base, serde_json::Value::Null).await;
    let dates: Vec<&str> = shown["episodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["episode"]["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, ["2036-09-09", "2036-09-16", "2036-09-23"]);
}

// ---- Delete show (release 1.0.4, ADR-0279) ----

async fn count(r: &Rig, sql: &'static str, id: uuid::Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&r.pool)
        .await
        .unwrap()
}

async fn show_id(r: &Rig, slug: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM show WHERE station_id = $1 AND slug = $2")
        .bind(r.station.as_uuid())
        .bind(slug)
        .fetch_one(&r.pool)
        .await
        .unwrap()
}

/// A show with its first episode drafted and one more, a step change and a rule change in its
/// logs, and a cart bank of its own.
async fn a_show_with_everything(r: &Rig, slug: &str, name: &str) -> uuid::Uuid {
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PUT",
            &format!("/api/v1/shows/{slug}"),
            show_body(
                name,
                serde_json::json!({"rule": first_tuesday(), "draft_first": true, "step": "solo"}),
            ),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            &format!("/api/v1/shows/{slug}/series/1/episodes"),
            serde_json::json!({"count": 1}),
        )
        .await,
    );
    // The step change is logged by the save; the rule change by the rule route.
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PUT",
            &format!("/api/v1/shows/{slug}"),
            show_body(name, serde_json::json!({"step": "full"})),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PUT",
            &format!("/api/v1/shows/{slug}/rule"),
            serde_json::json!({"rule": weekly(3, 1200, 60, "2036-09-03"), "drafted": "leave"}),
        )
        .await,
    );
    let id = show_id(r, slug).await;
    weavesignal_model::carts::put_bank(
        &r.pool,
        r.station,
        Some(id),
        &weavesignal_model::carts::Bank {
            number: 1,
            name: "Own effects".into(),
            colour_slot: 3,
            show: Some(slug.into()),
        },
    )
    .await
    .unwrap();
    id
}

fn card_files_of(v: &serde_json::Value) -> Vec<String> {
    ["link", "post", "square"]
        .iter()
        .filter_map(|k| v["show"]["cards"][k].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn an_admin_or_a_producer_deletes_a_show_that_never_aired_with_everything_that_goes_with_it()
{
    let r = rig().await;
    for (who, bearer, slug, name) in [
        // Names no other test makes: card files are named by their content and shared by every
        // station in the database, so a show of the same name elsewhere would keep them.
        (
            "a producer",
            r.producer.clone(),
            "bothy-hour",
            "The Bothy Hour",
        ),
        ("an admin", r.admin.clone(), "peat-fire", "Peat Fire Radio"),
        ("the key", ADMIN.to_owned(), "dram-time", "Dram Time"),
    ] {
        let id = a_show_with_everything(&r, slug, name).await;
        let view = ok_body(
            json_as(
                &r.app,
                &bearer,
                "GET",
                &format!("/api/v1/shows/{slug}"),
                serde_json::Value::Null,
            )
            .await,
        );
        let files = card_files_of(&view);
        assert_eq!(files.len(), 3, "{who}: the show's cards are drawn");
        for f in &files {
            assert!(r.dir.join("cards").join(f).exists(), "{who}: {f}");
        }
        // A slate still of its own sits beside the desk's file.
        let still = r.dir.join("stills").join(format!("show-{id}.png"));
        std::fs::create_dir_all(still.parent().unwrap()).unwrap();
        std::fs::write(&still, b"png").unwrap();
        assert!(count(&r, "SELECT count(*) FROM series WHERE show_id = $1", id).await >= 1);
        assert!(
            count(
                &r,
                "SELECT count(*) FROM show_step_change WHERE show_id = $1",
                id
            )
            .await
                >= 1
        );
        assert!(
            count(
                &r,
                "SELECT count(*) FROM show_rule_change WHERE show_id = $1",
                id
            )
            .await
                >= 1
        );
        assert_eq!(
            count(&r, "SELECT count(*) FROM cart_bank WHERE show_id = $1", id).await,
            1
        );

        let (code, gone) = json_as(
            &r.app,
            &bearer,
            "DELETE",
            &format!("/api/v1/shows/{slug}"),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{who}: {gone}");
        assert_eq!(gone["name"], name);
        assert_eq!(gone["series"], 1);
        assert_eq!(gone["episodes"], 2);
        assert_eq!(gone["cart_banks"], 1);
        for (what, sql) in [
            ("the show", "SELECT count(*) FROM show WHERE id = $1"),
            (
                "its series",
                "SELECT count(*) FROM series WHERE show_id = $1",
            ),
            (
                "its episodes",
                "SELECT count(*) FROM episode e JOIN series s ON s.id = e.series_id WHERE s.show_id = $1",
            ),
            (
                "its step log",
                "SELECT count(*) FROM show_step_change WHERE show_id = $1",
            ),
            (
                "its rule log",
                "SELECT count(*) FROM show_rule_change WHERE show_id = $1",
            ),
            (
                "its cart banks",
                "SELECT count(*) FROM cart_bank WHERE show_id = $1",
            ),
        ] {
            assert_eq!(count(&r, sql, id).await, 0, "{who}: {what} is gone");
        }
        for f in &files {
            assert!(
                !r.dir.join("cards").join(f).exists(),
                "{who}: {f} is deleted"
            );
        }
        assert!(!still.exists(), "{who}: its slate still is deleted");
        assert!(gone["files_deleted"].as_u64().unwrap() >= 4, "{gone}");
        // It is gone from the list and the week, and a second delete is a 404.
        let (code, _) = json_as(
            &r.app,
            &bearer,
            "GET",
            &format!("/api/v1/shows/{slug}"),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND);
        let (code, _) = json_as(
            &r.app,
            &bearer,
            "DELETE",
            &format!("/api/v1/shows/{slug}"),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{who}");
    }
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_presenter_a_screener_and_a_stranger_cannot_delete_and_nothing_goes() {
    let r = rig().await;
    let id = a_show_with_everything(&r, "ceilidh", "The Ceilidh").await;
    for (who, bearer, want) in [
        ("a presenter", r.presenter.as_str(), StatusCode::FORBIDDEN),
        ("a screener", r.screener.as_str(), StatusCode::FORBIDDEN),
        ("a stranger", "not-a-token", StatusCode::UNAUTHORIZED),
    ] {
        let (code, body) = json_as(
            &r.app,
            bearer,
            "DELETE",
            "/api/v1/shows/ceilidh",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, want, "{who}: {body}");
    }
    let (code, _) = json_as(
        &r.app,
        &r.producer,
        "DELETE",
        "/api/v1/shows/no-such-show",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    assert_eq!(
        count(&r, "SELECT count(*) FROM show WHERE id = $1", id).await,
        1
    );
    assert_eq!(
        count(&r, "SELECT count(*) FROM cart_bank WHERE show_id = $1", id).await,
        1
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_presenter_of_the_show_is_refused_too() {
    let r = rig().await;
    let account: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM staff_account WHERE station_id = $1 AND username = 'shona'",
    )
    .bind(r.station.as_uuid())
    .fetch_one(&r.pool)
    .await
    .unwrap();
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "PUT",
            "/api/v1/shows/own-show",
            show_body(
                "Own Show",
                serde_json::json!({"presenter_account": account}),
            ),
        )
        .await,
    );
    let (code, body) = json_as(
        &r.app,
        &r.presenter,
        "DELETE",
        "/api/v1/shows/own-show",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Only an admin or a producer deletes a show.");
    let id = show_id(&r, "own-show").await;
    assert_eq!(
        count(&r, "SELECT count(*) FROM show WHERE id = $1", id).await,
        1
    );
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_show_that_has_aired_or_is_on_air_stays_and_nothing_is_removed() {
    use jiff::ToSpan;
    let r = rig().await;
    // One that has aired: put on air, which freezes it.
    let id = a_show_with_everything(&r, "aired-show", "The Aired Show").await;
    let at = |s: i64| {
        (jiff::Timestamp::now() + s.seconds())
            .to_zoned(jiff::tz::TimeZone::get("Europe/London").unwrap())
            .to_string()
    };
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            "/api/v1/shows/aired-show/series/1/episodes/1/air",
            serde_json::json!({}),
        )
        .await,
    );
    let (code, body) = json_as(
        &r.app,
        &r.producer,
        "DELETE",
        "/api/v1/shows/aired-show",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "The Aired Show has aired 1 episode, so it stays as the station's record: only a show that has never aired can be deleted."
    );
    assert_eq!(
        count(&r, "SELECT count(*) FROM show WHERE id = $1", id).await,
        1
    );
    assert_eq!(
        count(&r, "SELECT count(*) FROM series WHERE show_id = $1", id).await,
        1
    );
    assert_eq!(
        count(&r, "SELECT count(*) FROM cart_bank WHERE show_id = $1", id).await,
        1
    );
    assert_eq!(
        count(
            &r,
            "SELECT count(*) FROM episode e JOIN series s ON s.id = e.series_id WHERE s.show_id = $1",
            id
        )
        .await,
        2,
        "both episodes stay"
    );
    // The admin gets the same answer, and a show on air now is refused in its own words.
    let (code, _) = json_as(
        &r.app,
        &r.admin,
        "DELETE",
        "/api/v1/shows/aired-show",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "PUT",
            "/api/v1/shows/on-now",
            show_body("On Now", serde_json::json!({})),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "PUT",
            "/api/v1/shows/on-now/series/1",
            serde_json::json!({}),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "POST",
            "/api/v1/shows/on-now/series/1/episodes",
            serde_json::json!({"count": 1}),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "PATCH",
            "/api/v1/shows/on-now/series/1/episodes/1",
            serde_json::json!({"air_at": at(-600), "minutes": 60}),
        )
        .await,
    );
    let (code, body) = json_as(
        &r.app,
        ADMIN,
        "DELETE",
        "/api/v1/shows/on-now",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "On Now is on air now, so it cannot be deleted."
    );
    // A draft whose time has passed without anyone pressing On air now has aired all the same:
    // nothing freezes an episode by itself, and its record stays (ADR-0279).
    ok_body(
        json_as(
            &r.app,
            ADMIN,
            "PATCH",
            "/api/v1/shows/on-now/series/1/episodes/1",
            serde_json::json!({"air_at": at(-7200), "minutes": 30}),
        )
        .await,
    );
    let (code, body) = json_as(
        &r.app,
        ADMIN,
        "DELETE",
        "/api/v1/shows/on-now",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "On Now has aired 1 episode, so it stays as the station's record: only a show that has never aired can be deleted."
    );
}

// ---- calendar files (release 1.0.5, ADR-0281) ----

/// A calendar another system exported: a weekly show over Saturday Night Kettle's time, the
/// last Friday of each month, every other day (not a rule a show can say), an hourly bulletin
/// (refused) and a one-off.
fn other_systems_calendar() -> String {
    [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//Elsewhere//EN",
        "BEGIN:VEVENT",
        "UID:ceilidh@elsewhere",
        "SUMMARY:Late Ceilidh",
        "DESCRIPTION:Reels\\, jigs and requests.",
        "DTSTART;TZID=Europe/London:20360906T210000",
        "DTEND;TZID=Europe/London:20360906T223000",
        "RRULE:FREQ=WEEKLY;BYDAY=SA",
        "END:VEVENT",
        "BEGIN:VEVENT",
        "UID:folk@elsewhere",
        "SUMMARY:Folk Night",
        "DTSTART;TZID=Europe/London:20360926T190000",
        "DURATION:PT2H",
        "RRULE:FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1",
        "END:VEVENT",
        "BEGIN:VEVENT",
        "UID:alt@elsewhere",
        "SUMMARY:Alternate Mornings",
        "DTSTART;TZID=Europe/London:20360901T070000",
        "DURATION:PT1H",
        "RRULE:FREQ=DAILY;INTERVAL=2;COUNT=30",
        "END:VEVENT",
        "BEGIN:VEVENT",
        "UID:news@elsewhere",
        "SUMMARY:News on the hour",
        "DTSTART;TZID=Europe/London:20360901T070000",
        "RRULE:FREQ=HOURLY",
        "END:VEVENT",
        "BEGIN:VEVENT",
        "UID:burns@elsewhere",
        "SUMMARY:Burns Supper",
        "DTSTART;TZID=Europe/London:20370125T193000",
        "DTEND;TZID=Europe/London:20370125T223000",
        "END:VEVENT",
        "END:VCALENDAR",
        "",
    ]
    .join("\r\n")
}

async fn shows_count(r: &Rig) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM show WHERE station_id = $1")
        .bind(r.station.as_uuid())
        .fetch_one(&r.pool)
        .await
        .unwrap()
}

async fn get_text(app: &axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let res = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (code, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_calendar_file_is_previewed_made_into_shows_and_published_again() {
    let r = rig().await;
    // The station's own show, Saturdays 20:00 to 23:00 from 6 September 2036, with a drafted
    // episode whose title is not to be published before it airs.
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PUT",
            "/api/v1/shows/saturday-night",
            show_body(
                "Saturday Night Kettle",
                serde_json::json!({"rule": weekly(6, 1200, 180, "2036-09-06"), "draft_first": true}),
            ),
        )
        .await,
    );
    ok_body(
        json_as(
            &r.app,
            &r.producer,
            "PATCH",
            "/api/v1/shows/saturday-night/series/1/episodes/1",
            serde_json::json!({"title": "Secret Guest Night"}),
        )
        .await,
    );
    let before = shows_count(&r).await;
    let file = other_systems_calendar();

    // Who may: not a presenter, a screener or a stranger.
    for (bearer, want) in [
        (r.presenter.as_str(), StatusCode::FORBIDDEN),
        (r.screener.as_str(), StatusCode::FORBIDDEN),
        ("not-a-token", StatusCode::UNAUTHORIZED),
    ] {
        let (code, body) = json_as(
            &r.app,
            bearer,
            "POST",
            "/api/v1/schedule/import/preview",
            serde_json::json!({"ics": file}),
        )
        .await;
        assert_eq!(code, want, "{body}");
    }

    // A producer's preview: five items in the file's order, nothing written.
    let body = ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            "/api/v1/schedule/import/preview",
            serde_json::json!({"ics": file, "limit": 3}),
        )
        .await,
    );
    assert_eq!(shows_count(&r).await, before, "a preview writes nothing");
    assert_eq!(body["zone"], "Europe/London");
    assert_eq!(body["events"], 5);
    assert_eq!(body["ready"], 4);
    let items = body["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "Late Ceilidh",
            "Folk Night",
            "Alternate Mornings",
            "News on the hour",
            "Burns Supper"
        ]
    );
    let mappings: Vec<&str> = items
        .iter()
        .map(|i| i["mapping"].as_str().unwrap())
        .collect();
    assert_eq!(mappings, ["exact", "exact", "dates", "refused", "exact"]);
    let ceilidh = &items[0];
    assert_eq!(ceilidh["description"], "Reels, jigs and requests.");
    assert_eq!(ceilidh["slug"], "late-ceilidh");
    assert_eq!(
        ceilidh["words"]["sentence"],
        "Every week on Saturday, 21:00 to 22:30, from 6 September 2036."
    );
    assert_eq!(ceilidh["dates"].as_array().unwrap().len(), 3);
    assert_eq!(ceilidh["dates"][0]["on"], "2036-09-06");
    assert_eq!(ceilidh["clash_count"], 3, "{ceilidh}");
    assert_eq!(ceilidh["dates"][0]["clashes"][0]["slug"], "saturday-night");
    assert_eq!(
        items[1]["rule"]["repeat"],
        serde_json::json!({"kind": "monthly_nth", "every": 1, "nth": -1, "weekday": 5})
    );
    assert!(
        items[2]["reason"]
            .as_str()
            .unwrap()
            .contains("every 2 days")
    );
    assert!(items[3]["rule"].is_null() && items[3]["slug"].is_null());
    assert!(
        items[3]["reason"]
            .as_str()
            .unwrap()
            .contains("within a day")
    );
    assert_eq!(items[4]["rule"]["repeat"]["kind"], "once");

    // The file itself, as text/calendar, reads the same.
    let res = r
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/schedule/import/preview")
                .header(header::AUTHORIZATION, format!("Bearer {}", r.admin))
                .header(header::CONTENT_TYPE, "text/calendar; charset=utf-8")
                .body(Body::from(file.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // A file that is not a calendar: 422, a sentence, the field named. Too many events: 413.
    let (code, body) = json_as(
        &r.app,
        &r.producer,
        "POST",
        "/api/v1/schedule/import/preview",
        serde_json::json!({"ics": "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:2036"}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["field"], "ics");
    assert!(body["error"].as_str().unwrap().ends_with('.'));
    let many: String = (0..201)
        .map(|i| {
            format!(
                "BEGIN:VEVENT\r\nUID:{i}\r\nSUMMARY:S\r\nDTSTART:20370101T120000Z\r\nEND:VEVENT\r\n"
            )
        })
        .collect();
    let (code, body) = json_as(
        &r.app,
        &r.producer,
        "POST",
        "/api/v1/schedule/import/preview",
        serde_json::json!({"ics": format!("BEGIN:VCALENDAR\r\n{many}END:VCALENDAR\r\n")}),
    )
    .await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE, "{body}");

    // Make four, one renamed. A presenter may not; a rule that cannot stand refuses the lot.
    let chosen: Vec<serde_json::Value> = items
        .iter()
        .filter(|i| i["mapping"] != "refused")
        .map(|i| {
            let name = if i["name"] == "Late Ceilidh" {
                "The Late Ceilidh".into()
            } else {
                i["name"].clone()
            };
            serde_json::json!({"name": name, "description": i["description"], "rule": i["rule"]})
        })
        .collect();
    let (code, _) = json_as(
        &r.app,
        &r.presenter,
        "POST",
        "/api/v1/schedule/import",
        serde_json::json!({"items": chosen}),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let mut broken = chosen.clone();
    broken[1]["rule"]["repeat"]["nth"] = serde_json::json!(5);
    let (code, body) = json_as(
        &r.app,
        &r.producer,
        "POST",
        "/api/v1/schedule/import",
        serde_json::json!({"items": broken}),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["field"], "items[1].rule");
    assert!(body["error"].as_str().unwrap().starts_with("“Folk Night”"));
    assert_eq!(shows_count(&r).await, before, "nothing made");

    let (code, body) = json_as(
        &r.app,
        &r.producer,
        "POST",
        "/api/v1/schedule/import",
        serde_json::json!({"items": chosen}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let made = body["made"].as_array().unwrap();
    let slugs: Vec<&str> = made.iter().map(|s| s["slug"].as_str().unwrap()).collect();
    assert_eq!(
        slugs,
        [
            "the-late-ceilidh",
            "folk-night",
            "alternate-mornings",
            "burns-supper"
        ]
    );
    for s in made {
        assert_eq!(s["step"], "solo");
        assert!(s["cards"]["square"].is_string(), "{s}");
        assert!(s["rule_words"]["sentence"].is_string());
    }
    assert_eq!(made[0]["rule"], items[0]["rule"]);
    let episodes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM episode e JOIN series s ON s.id = e.series_id JOIN show sh ON sh.id = s.show_id WHERE sh.station_id = $1 AND sh.slug <> 'saturday-night'",
    )
    .bind(r.station.as_uuid())
    .fetch_one(&r.pool)
    .await
    .unwrap();
    assert_eq!(episodes, 0, "no episode is drafted");
    let warnings = body["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{body}");
    assert_eq!(warnings[0]["show"], "the-late-ceilidh");
    assert_eq!(warnings[0]["clashes"][0]["slug"], "saturday-night");

    // The same again: the paths are made free, and nothing is overwritten.
    let body = ok_body(
        json_as(
            &r.app,
            &r.admin,
            "POST",
            "/api/v1/schedule/import",
            serde_json::json!({"items": [chosen[1].clone()]}),
        )
        .await,
    );
    assert_eq!(body["made"][0]["slug"], "folk-night-2");
    // Its card prints its own address, not the first Folk Night's.
    assert!(
        body["made"][0]["cards"]["alt"]
            .as_str()
            .unwrap()
            .ends_with("kettle.example/folk-night-2"),
        "{body}"
    );
    assert_eq!(shows_count(&r).await, before + 5);

    // Published: the whole schedule and one show, no bearer, as calendar files.
    let (code, headers, all) = get_text(&r.app, "/api/v1/public/schedule.ics").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/calendar; charset=utf-8"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=30");
    assert!(all.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(all.contains("BEGIN:VTIMEZONE\r\nTZID:Europe/London\r\n"));
    assert_eq!(all.matches("\r\nBEGIN:VEVENT\r\n").count(), 6);
    assert!(all.contains("SUMMARY:Saturday Night Kettle\r\n"));
    // No show page answers at the station's address yet, so no URL is given.
    assert!(!all.contains("\r\nURL:"));
    assert!(
        !all.contains("Secret Guest"),
        "a drafted title is not published"
    );
    let (code, _, one) = get_text(&r.app, "/api/v1/public/shows/the-late-ceilidh.ics").await;
    assert_eq!(code, StatusCode::OK);
    assert!(one.contains("X-WR-CALNAME:The Late Ceilidh on Kettle FM\r\n"));
    assert!(one.contains("RRULE:FREQ=WEEKLY;BYDAY=SA\r\n"));
    for missing in [
        "/api/v1/public/shows/no-such-show.ics",
        "/api/v1/public/shows/the-late-ceilidh",
    ] {
        let (code, _, _) = get_text(&r.app, missing).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{missing}");
    }

    // And what was published reads back as the same rule.
    let body = ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            "/api/v1/schedule/import/preview",
            serde_json::json!({"ics": one}),
        )
        .await,
    );
    assert_eq!(body["items"][0]["mapping"], "exact");
    assert_eq!(body["items"][0]["rule"], items[0]["rule"]);
    assert_eq!(body["items"][0]["slug"], "the-late-ceilidh-2");
}

#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_stations_own_calendar_read_back_is_already_there_and_no_clash_with_itself() {
    // The test station's case (the 1.0.5 run): its own schedule.ics imported into itself
    // proposed every show, each a CONFLICT with itself on every date.
    let r = rig().await;
    let put = |slug: &'static str, name: &'static str, extra: serde_json::Value| {
        let (app, producer) = (r.app.clone(), r.producer.clone());
        async move {
            ok_body(
                json_as(
                    &app,
                    &producer,
                    "PUT",
                    &format!("/api/v1/shows/{slug}"),
                    show_body(name, extra),
                )
                .await,
            )
        }
    };
    // A show with an episode drafted (its own clash would be an `episode`); weekdays with a
    // skipped date; an old weekly slot with no start date; one date.
    put(
        "saturday-night",
        "Saturday Night Kettle",
        serde_json::json!({"rule": weekly(6, 1200, 180, "2036-09-06"), "draft_first": true}),
    )
    .await;
    put(
        "kettle-breakfast",
        "Kettle Breakfast",
        serde_json::json!({"rule": {
            "starts_on": "2036-09-01", "start": 420, "minutes": 180,
            "repeat": {"kind": "weekly", "every": 1, "weekdays": [1, 2, 3, 4, 5]},
            "skip": ["2036-09-03"]
        }}),
    )
    .await;
    put(
        "kettle-drive",
        "Kettle Drive",
        serde_json::json!({"slot": {"weekday": 3, "start": 960, "minutes": 120}}),
    )
    .await;
    put(
        "budget-night",
        "Budget Night Special",
        serde_json::json!({"rule": {
            "starts_on": "2036-11-19", "start": 1080, "minutes": 90, "repeat": {"kind": "once"}
        }}),
    )
    .await;
    // And one that really does overlap another: Mondays 06:00 to 08:00, over Kettle Breakfast.
    put(
        "early-kettle",
        "Early Kettle",
        serde_json::json!({"rule": weekly(1, 360, 120, "2036-09-01")}),
    )
    .await;
    let before = shows_count(&r).await;

    let (code, _, file) = get_text(&r.app, "/api/v1/public/schedule.ics").await;
    assert_eq!(code, StatusCode::OK);
    let body = ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            "/api/v1/schedule/import/preview",
            serde_json::json!({"ics": file, "limit": 6}),
        )
        .await,
    );
    assert_eq!(shows_count(&r).await, before, "a preview writes nothing");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 5, "{body}");
    assert_eq!(body["ready"], 5);
    let own = [
        ("Saturday Night Kettle", "saturday-night"),
        ("Kettle Breakfast", "kettle-breakfast"),
        ("Kettle Drive", "kettle-drive"),
        ("Budget Night Special", "budget-night"),
        ("Early Kettle", "early-kettle"),
    ];
    for (name, slug) in own {
        let i = items
            .iter()
            .find(|i| i["name"] == name)
            .unwrap_or_else(|| panic!("{name} is proposed: {body}"));
        assert_eq!(i["mapping"], "exact", "{i}");
        // It is the show the station has, by its path and under the name the station gives it.
        assert_eq!(
            i["already"],
            serde_json::json!({"slug": slug, "name": name}),
            "{i}"
        );
        // Made again it would take a second path; that is still said.
        assert_ne!(i["slug"], slug, "{i}");
        // Its own times are no clash, on any date, drafted or not.
        let dates = i["dates"].as_array().unwrap();
        assert!(!dates.is_empty(), "{i}");
        for d in dates {
            for c in d["clashes"].as_array().unwrap() {
                assert_ne!(c["slug"], slug, "{name} clashes with itself: {d}");
            }
        }
        let clashing = dates
            .iter()
            .filter(|d| !d["clashes"].as_array().unwrap().is_empty())
            .count();
        assert_eq!(i["clash_count"], clashing, "{i}");
        match name {
            // Another show's time is a clash as before: Mondays, each with the other.
            "Kettle Breakfast" => {
                assert!(clashing > 0, "{i}");
                let first = dates
                    .iter()
                    .find(|d| !d["clashes"].as_array().unwrap().is_empty())
                    .unwrap();
                assert_eq!(first["on"], "2036-09-01");
                assert_eq!(first["clashes"][0]["slug"], "early-kettle");
                assert_eq!(first["clashes"].as_array().unwrap().len(), 1);
            }
            "Early Kettle" => {
                assert_eq!(i["clash_count"], 6, "{i}");
                assert!(
                    dates
                        .iter()
                        .all(|d| d["clashes"][0]["slug"] == "kettle-breakfast")
                );
            }
            _ => assert_eq!(i["clash_count"], 0, "{i}"),
        }
    }

    // The same name over other times is not the show: it clashes with it, as it should.
    let moved = file.replace("T200000", "T193000");
    assert_ne!(moved, file);
    let body = ok_body(
        json_as(
            &r.app,
            &r.producer,
            "POST",
            "/api/v1/schedule/import/preview",
            serde_json::json!({"ics": moved, "limit": 3}),
        )
        .await,
    );
    let saturday = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "Saturday Night Kettle")
        .unwrap()
        .clone();
    assert!(saturday["already"].is_null(), "{saturday}");
    assert_eq!(saturday["clash_count"], 3, "{saturday}");
    assert_eq!(
        saturday["dates"][0]["clashes"][0]["slug"], "saturday-night",
        "{saturday}"
    );

    // A name that differs only in case and spaces is the show; another name is not.
    for (renamed, there) in [
        ("  saturday   NIGHT kettle ", true),
        ("Saturday Kettle", false),
    ] {
        let body = ok_body(
            json_as(
                &r.app,
                &r.producer,
                "POST",
                "/api/v1/schedule/import/preview",
                serde_json::json!({"ics": file.replace("SUMMARY:Saturday Night Kettle", &format!("SUMMARY:{renamed}")), "limit": 3}),
            )
            .await,
        );
        let i = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["rule"]["repeat"]["weekdays"] == serde_json::json!([6]))
            .unwrap()
            .clone();
        assert_eq!(!i["already"].is_null(), there, "{renamed}: {i}");
        assert_eq!(
            i["clash_count"],
            if there { 0 } else { 3 },
            "{renamed}: {i}"
        );
    }
}

/// A keyed show promises that only key holders hear it (1.3.3): its episodes' files are in no
/// feed, public list or file route while it is keyed, and the file route serves only files an
/// episode names.
#[tokio::test]
#[ignore = "needs WEAVESIGNAL_TEST_DATABASE_URL"]
async fn a_keyed_shows_episode_files_are_held_back_while_it_is_keyed() {
    let url =
        std::env::var("WEAVESIGNAL_TEST_DATABASE_URL").expect("WEAVESIGNAL_TEST_DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let station = ensure_station(
        &pool,
        &Station::new(StationId::new_random(), "Kettle FM").unwrap(),
    )
    .await
    .unwrap();
    let station_id = station.id;
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("keyed-{}", uuid::Uuid::new_v4()));
    let episodes_dir = dir.join("episodes");
    std::fs::create_dir_all(&episodes_dir).unwrap();
    let app = router(Arc::new(ShowsState {
        station,
        db: Some(pool.clone()),
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        publishing: PublishingConfig {
            public_url: "https://kettle.example".into(),
            cards_dir: dir.join("cards"),
            episodes_dir: episodes_dir.clone(),
            ..PublishingConfig::default()
        },
        zone: Arc::new(weavesignal_control::zone::StationZone::new("Europe/London")),
        live: None,
        hub: None,
        video: None,
    }));

    // File names no other test uses: every station's episodes share the folder.
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let name = |n: &str| format!("{n}-{tag}.mp3");
    let aired = name("aired");
    let aired_opus = format!("aired-{tag}.opus");
    let unaired = name("unaired");
    let open = name("open");
    let shared = name("shared");
    let replacement = name("replacement");
    let stray = name("stray");
    for f in [&aired, &unaired, &open, &shared, &replacement, &stray] {
        std::fs::copy(
            root().join("crates/feeds/tests/fixtures/tone.mp3"),
            episodes_dir.join(f),
        )
        .unwrap();
    }
    std::fs::copy(
        root().join("assets/carts-demo/ding-1.opus"),
        episodes_dir.join(&aired_opus),
    )
    .unwrap();
    std::fs::write(episodes_dir.join(".retag-half.mp3"), b"half written").unwrap();

    let ok = |code: StatusCode, body: &serde_json::Value| assert!(code.is_success(), "{body}");
    for (slug, title, colour) in [
        ("members", "The Members Hour", 3),
        ("open-desk", "Open Desk", 4),
    ] {
        let base = format!("/api/v1/shows/{slug}");
        let (c, b) = json(
            &app,
            "PUT",
            &base,
            serde_json::json!({"name": title, "colour_slot": colour, "hosts": ["Isla Fraser"]}),
        )
        .await;
        ok(c, &b);
        let (c, b) = json(
            &app,
            "PUT",
            &format!("{base}/series/1"),
            serde_json::json!({}),
        )
        .await;
        ok(c, &b);
        let (c, b) = json(
            &app,
            "POST",
            &format!("{base}/series/1/episodes"),
            serde_json::json!({"count": 3}),
        )
        .await;
        ok(c, &b);
        let (c, b) = json(
            &app,
            "POST",
            &format!("{base}/series/1/episodes/1/air"),
            serde_json::json!({}),
        )
        .await;
        ok(c, &b);
    }
    let attach = async |slug: &str, episode: u32, files: serde_json::Value| {
        let (c, b) = json(
            &app,
            "PUT",
            &format!("/api/v1/shows/{slug}/series/1/episodes/{episode}/files"),
            files,
        )
        .await;
        assert_eq!(c, StatusCode::OK, "{b}");
    };
    attach(
        "members",
        1,
        serde_json::json!({"enclosure": aired, "opus": aired_opus}),
    )
    .await;
    attach("members", 2, serde_json::json!({"enclosure": unaired})).await;
    attach("members", 3, serde_json::json!({"enclosure": shared})).await;
    attach("open-desk", 1, serde_json::json!({"enclosure": open})).await;
    attach("open-desk", 2, serde_json::json!({"enclosure": shared})).await;

    let fetch = async |method: &str, uri: String, range: bool| {
        let mut req = Request::builder().method(method).uri(uri);
        if range {
            req = req.header(header::RANGE, "bytes=0-9");
        }
        let (code, bytes) = send(&app, req.body(Body::empty()).unwrap()).await;
        (code, String::from_utf8_lossy(&bytes).into_owned())
    };
    let file = async |f: &str| fetch("GET", format!("/media/episodes/{f}"), false).await.0;
    let items = async |slug: &str| {
        let (code, xml) = fetch("GET", format!("/feeds/{slug}.xml"), false).await;
        assert_eq!(code, StatusCode::OK, "{xml}");
        xml.matches("<item>").count()
    };
    let listed = async || {
        let (code, body) = fetch("GET", "/api/v1/public/episodes?limit=50".into(), false).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        body
    };
    let show = weavesignal_model::shows::get_show(&pool, station_id, "members")
        .await
        .unwrap()
        .expect("the show")
        .id;
    let rule = async |rule: &str, opens_at_ms: Option<i64>| {
        weavesignal_model::listener_keys_db::set_access(
            &pool,
            station_id,
            &weavesignal_model::listener_keys_db::Access {
                show_id: show,
                rule: rule.into(),
                opens_at_ms,
                slate: "fallback".into(),
                slate_note: String::new(),
                updated_ms: 0,
                updated_by: "shows-db-test".into(),
            },
            "shows-db-test",
        )
        .await
        .unwrap();
    };

    // Public: as before. A file no episode names is not served, nor a half-written copy.
    assert_eq!(items("members").await, 1);
    assert!(listed().await.contains(&aired));
    for f in [&aired, &aired_opus, &unaired, &open, &shared] {
        assert_eq!(file(f).await, StatusCode::OK, "{f}");
    }
    let encoded = aired.replacen('-', "%2D", 1);
    assert_eq!(file(&encoded).await, StatusCode::OK);
    let (code, part) = fetch("GET", format!("/media/episodes/{aired}"), true).await;
    assert_eq!(
        (code, part.is_empty()),
        (StatusCode::PARTIAL_CONTENT, false)
    );
    let (code, body) = fetch("HEAD", format!("/media/episodes/{aired}"), false).await;
    assert_eq!((code, body.len()), (StatusCode::OK, 0));
    for f in [
        stray.as_str(),
        ".retag-half.mp3",
        "%2Eretag-half.mp3",
        "..%2F..%2Fcards",
        "sub%2Fx.mp3",
        "sub/x.mp3",
    ] {
        assert_eq!(file(f).await, StatusCode::NOT_FOUND, "{f}");
    }

    // Keyed: nothing of the show is left in the feed, the list or the file route; the other
    // show is untouched, but for the file it shares with the keyed one.
    for opens in [None, Some(4_000_000_000_000)] {
        rule("key", opens).await;
        assert_eq!(items("members").await, 0, "{opens:?}");
        assert_eq!(items("open-desk").await, 1);
        let list = listed().await;
        assert!(
            !list.contains(&aired) && !list.contains(&aired_opus),
            "{list}"
        );
        assert!(list.contains(&open), "{list}");
        for f in [&aired, &aired_opus, &unaired, &shared, &encoded] {
            assert_eq!(file(f).await, StatusCode::NOT_FOUND, "{f} {opens:?}");
            for (method, range) in [("HEAD", false), ("GET", true)] {
                let (code, _) = fetch(method, format!("/media/episodes/{f}"), range).await;
                assert_eq!(code, StatusCode::NOT_FOUND, "{method} {f}");
            }
        }
        assert_eq!(file(&open).await, StatusCode::OK);
    }

    // Opened to everyone, and public again: as before.
    for (kind, opens) in [("key", Some(1_000)), ("public", None)] {
        rule(kind, opens).await;
        assert_eq!(items("members").await, 1, "{kind}");
        assert!(listed().await.contains(&aired), "{kind}");
        assert_eq!(file(&aired).await, StatusCode::OK, "{kind}");
        assert_eq!(file(&unaired).await, StatusCode::OK, "{kind}");
    }

    // A file an episode no longer names is not served any more.
    attach("members", 1, serde_json::json!({"enclosure": replacement})).await;
    assert_eq!(file(&replacement).await, StatusCode::OK);
    assert_eq!(file(&aired).await, StatusCode::NOT_FOUND);

    // The rules cannot be read: nothing is served.
    pool.close().await;
    assert_eq!(file(&replacement).await, StatusCode::INTERNAL_SERVER_ERROR);
}
