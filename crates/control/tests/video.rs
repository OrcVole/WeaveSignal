// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video routes (ADR-0010) against the real media worker (this crate's binary, as
//! `media-worker still`): a picture becomes the slate's still, anything else is refused, the
//! file is where the desk looks, and the cameras' thumbnails come from the desk. No database.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;
use weavesignal_control::app::{AppState, router};
use weavesignal_control::carts::CartsLink;
use weavesignal_control::config::{CartsConfig, VideoConfig};
use weavesignal_control::desk::DeskClient;
use weavesignal_control::hub::Hub;
use weavesignal_control::studio::Studio;
use weavesignal_model::{Station, StationId};

const ADMIN: &str = "video-test";

fn png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let mut w = e.write_header().unwrap();
    let data: Vec<u8> = rgb
        .iter()
        .copied()
        .cycle()
        .take(3 * (width * height) as usize)
        .collect();
    w.write_image_data(&data).unwrap();
    w.finish().unwrap();
    out
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    (
        code,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn req(method: &str, uri: &str, bearer: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut r = Request::builder().method(method).uri(uri);
    if let Some(b) = bearer {
        r = r.header("authorization", format!("Bearer {b}"));
    }
    r.body(Body::from(body)).unwrap()
}

/// A desk that answers `thumbs` only. Its socket is under the system's temporary directory,
/// not `dir`: a Unix socket's path must be shorter than `SUN_LEN` (108 bytes), and a
/// worktree's `CARGO_TARGET_TMPDIR` can be longer than that.
fn fake_desk(_dir: &std::path::Path, video_on: bool) -> std::path::PathBuf {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = std::env::temp_dir().join(format!("rs-desk-{}.sock", &id[..12]));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                    let answer = match (v["op"].as_str(), video_on) {
                        (Some("thumbs"), true) => {
                            serde_json::json!({"ok": true, "thumbs": {"eilidh": "/9j/4AAQ"}})
                        }
                        (Some("thumbs"), false) => {
                            serde_json::json!({"ok": false, "error": "video is off"})
                        }
                        _ => serde_json::json!({"ok": true}),
                    };
                    let _ = w.write_all(format!("{answer}\n").as_bytes()).await;
                }
            });
        }
    });
    path
}

fn make_app(dir: &std::path::Path, desk: Option<&std::path::Path>) -> (axum::Router, VideoConfig) {
    let video = VideoConfig {
        still_file: dir.join("stills").join("slate.png"),
        still_max_width: 640,
        still_max_height: 360,
        still_max_upload_mb: 1,
        demo_still: None,
    };
    let carts = CartsConfig {
        media_dir: dir.join("carts"),
        worker: Some(env!("CARGO_BIN_EXE_weavesignal-control").into()),
        ..CartsConfig::default()
    };
    let hub = Hub::new(
        "Kettle FM",
        Studio::default(),
        DeskClient::new(desk),
        None,
        Duration::from_millis(250),
        CartsLink::new(carts, None),
    );
    let app = router(Arc::new(AppState {
        station: Station::new(StationId::new_random(), "Kettle FM").unwrap(),
        db: None,
        room: None,
        admin_token: Some(ADMIN.into()),
        staff: Default::default(),
        hub,
        join: Default::default(),
        video: video.clone(),
        joins: Default::default(),
        turn_tls_port: 5349,
        steering: Default::default(),
    }));
    (app, video)
}

fn temp() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("video-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_picture_becomes_the_slates_still_and_can_be_taken_away() {
    let dir = temp();
    let (app, video) = make_app(&dir, None);

    let (code, body) = send(&app, req("GET", "/api/v1/video/still", Some(ADMIN), vec![])).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["present"],
        false
    );

    // A big picture is fitted inside 640 by 360, in the worker, and lands where the desk looks.
    let (code, body) = send(
        &app,
        req(
            "PUT",
            "/api/v1/video/still",
            Some(ADMIN),
            png(1280, 360, [200, 40, 40]),
        ),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (info["width"].as_u64(), info["height"].as_u64()),
        (Some(640), Some(180))
    );
    // Kept as the station's still, and copied where the desk looks (ADR-0200: no show is on
    // air, so the station's shows).
    let on_disk = std::fs::read(video.still_file.with_file_name("station.png")).unwrap();
    assert_eq!(std::fs::read(&video.still_file).unwrap(), on_disk);
    assert_eq!(&on_disk[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(info["bytes"].as_u64(), Some(on_disk.len() as u64));

    let (code, image) = send(
        &app,
        req("GET", "/api/v1/video/still/image", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!((code, image), (StatusCode::OK, on_disk));

    let (code, _) = send(
        &app,
        req("DELETE", "/api/v1/video/still", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    assert!(!video.still_file.exists());
    let (code, _) = send(
        &app,
        req("GET", "/api/v1/video/still/image", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // Taking away what is not there is no error.
    let (code, _) = send(
        &app,
        req("DELETE", "/api/v1/video/still", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn what_is_not_a_picture_is_refused_and_nothing_is_written() {
    let dir = temp();
    let (app, video) = make_app(&dir, None);
    let (code, body) = send(
        &app,
        req(
            "PUT",
            "/api/v1/video/still",
            Some(ADMIN),
            b"RIFF....WAVEfmt not a picture".to_vec(),
        ),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("PNG or JPEG"));
    assert!(!video.still_file.exists());
    // Too large for `still_max_upload_mb` (1): refused before the worker runs.
    let (code, _) = send(
        &app,
        req(
            "PUT",
            "/api/v1/video/still",
            Some(ADMIN),
            vec![0u8; 1024 * 1024 + 1],
        ),
    )
    .await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn every_video_route_wants_a_bearer() {
    let dir = temp();
    let (app, video) = make_app(&dir, None);
    for (method, uri) in [
        ("GET", "/api/v1/video/still"),
        ("PUT", "/api/v1/video/still"),
        ("DELETE", "/api/v1/video/still"),
        ("GET", "/api/v1/video/still/image"),
        ("GET", "/api/v1/video/thumbs"),
        ("GET", "/api/v1/shows/kettle-talk/still"),
        ("PUT", "/api/v1/shows/kettle-talk/still"),
        ("DELETE", "/api/v1/shows/kettle-talk/still"),
        ("GET", "/api/v1/shows/kettle-talk/still/image"),
    ] {
        for bearer in [None, Some("not-the-key")] {
            let (code, _) = send(&app, req(method, uri, bearer, png(8, 8, [1, 2, 3]))).await;
            assert_eq!(code, StatusCode::UNAUTHORIZED, "{method} {uri} {bearer:?}");
        }
    }
    assert!(!video.still_file.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_cameras_thumbnails_come_from_the_desk() {
    let dir = temp();
    let on_sock = fake_desk(&dir, true);
    let (app, _) = make_app(&dir, Some(&on_sock));
    let (code, body) = send(
        &app,
        req("GET", "/api/v1/video/thumbs", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["thumbs"]["eilidh"], "/9j/4AAQ");

    // With video off the desk refuses, and the answer says so.
    let off = temp();
    let off_sock = fake_desk(&off, false);
    let (app, _) = make_app(&off, Some(&off_sock));
    let (code, _) = send(
        &app,
        req("GET", "/api/v1/video/thumbs", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);

    // And with no desk at all, it is unavailable.
    let (app, _) = make_app(&dir, None);
    let (code, _) = send(
        &app,
        req("GET", "/api/v1/video/thumbs", Some(ADMIN), vec![]),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&off);
    let _ = std::fs::remove_file(&on_sock);
    let _ = std::fs::remove_file(&off_sock);
}

#[tokio::test]
async fn a_shows_still_needs_the_database() {
    let dir = temp();
    let (app, video) = make_app(&dir, None);
    let (code, _) = send(
        &app,
        req(
            "PUT",
            "/api/v1/shows/kettle-talk/still",
            Some(ADMIN),
            png(8, 8, [1, 2, 3]),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!video.still_file.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
