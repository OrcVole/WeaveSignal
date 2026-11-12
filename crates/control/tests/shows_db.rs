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

    // Series 3 from 5 September, ten planned; the season drafted at once.
    let (code, body) = json(
        &app,
        "PUT",
        "/api/v1/shows/saturday-night/series/3",
        serde_json::json!({"starts_on": "2026-09-05", "planned_episodes": 10}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["resolved"]["series"]["first"], "2026-09-05");
    assert_eq!(body["resolved"]["series"]["last"], "2026-11-07");
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
        "Saturday Night Kettle, Series 3 Episode 7, Ghost Train night, Saturday 17 October 2026 at 20:00 BST on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan, guests Firth Lights and Dr Una Robertson. kettle.example/saturday-night/s3/e7"
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
