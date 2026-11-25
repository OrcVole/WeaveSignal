// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows at the origin (1.3.2): the doors a key holder's player goes through at the
//! station itself, each checking the token in its path. Edges check the same tokens the same
//! way (`deploy/images/edge/keyed.js`) and fetch from here, so the origin is never an open door.
//!
//! | Route | What |
//! |---|---|
//! | `GET /hls/k/{show}/{token}/{*path}` | the desk's HLS for one keyed show: its segments and the public ones, never another keyed show's |
//! | `GET /listen/k/{show}/{token}/{mount}` | the keyed Opus or MP3 stream, carried from the listener process on loopback and ended at once when the key is revoked or paused, the token ends, or the show's window closes |
//! | `GET /api/v1/keyed/revoked` | the keys revoked or paused lately, for edges (`EDGE_KEYED_REVOKED_URL`) |
//!
//! **Why the origin carries keyed streams itself** (the spike's question 1, taken as
//! recommended): icecast.server accepts every listener and cannot end one, so a revoke could
//! not reach a listener connected to it. The control plane carries each keyed listener's bytes
//! (96 kbit/s each) instead; edges carry the crowd.
//!
//! **The public `/hls/`** ([`crate::public::hls_file`]) is held back during a keyed window and
//! for `keys.hls_lag_s` after it (its playlists answer 403 with `code: keyed`, so the player
//! shows the lock and plays the public sound, the slate), and never serves a segment that may
//! hold a keyed moment, whatever a playlist says.

// The `Err` of these helpers is the response to send, built once per refused request.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use weavesignal_keyed::Segment;

use crate::access_token::{self, TokenError, TokenMedia};
use crate::app::{AppState, problem};
use crate::hub::now_ms;

/// The listener process's keyed mounts (`listeners.keyed` in playout's data).
pub const KEYED_OPUS: &str = "programme-keyed.opus";
pub const KEYED_MP3: &str = "programme-keyed.mp3";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/hls/k/{show}/{token}/{*path}", get(keyed_hls))
        .route("/listen/k/{show}/{token}/{mount}", get(keyed_stream))
        .route("/api/v1/keyed/revoked", get(revoked))
}

fn refused(why: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(json!({ "error": "This needs a key for the show on now.", "code": why })),
    )
        .into_response()
}

fn why(e: &TokenError) -> &'static str {
    match e {
        TokenError::Malformed => "malformed",
        TokenError::BadSignature => "bad_signature",
        TokenError::NotYet => "not_yet",
        TokenError::Expired => "expired",
        TokenError::WrongShow => "wrong_show",
        TokenError::WrongMedia => "wrong_media",
        TokenError::Revoked => "revoked",
    }
}

/// The token's claims for this show and medium, or the refusal.
fn checked(
    state: &AppState,
    token: &str,
    show: &str,
    want: TokenMedia,
) -> Result<access_token::Claims, Response> {
    let rt = &state.staff.api.access;
    let Some(key) = state.staff.api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Keys are not set up at this station.",
        ));
    };
    access_token::check(
        rt.signer(key),
        token,
        show,
        want,
        (now_ms() / 1000) as i64,
        rt.config.clock_skew_s as i64,
        |k| rt.live.is_revoked(k),
    )
    .map_err(|e| refused(why(&e)))
}

// ---- the keyed HLS ----

#[utoipa::path(
    get,
    path = "/hls/k/{show}/{token}/{path}",
    operation_id = "keyed_hls_file",
    tag = "listen",
    summary = "A file of a keyed show's HLS",
    description = "No bearer: the token in the path is the credential (`ks1.…`, from redeeming a key). Checked on every playlist and segment: its signature, its time, the show, and the medium (`audio.m3u8` and `audio/…` need sound, the rest video); a key revoked or paused is refused at once. Serves the public segments and this show's, never another keyed show's (each segment is judged by when it was written against the times the public gate was shut). Answers `Cache-Control: private`. 403 with a `code` when refused; 404 while video is off.",
    params(
        ("show" = String, Path, description = "The show's id, as the token names it"),
        ("token" = String, Path, description = "The token"),
        ("path" = String, Path, description = "A playlist or segment, such as `live.m3u8` or `audio/seg1.m4s`"),
    ),
    responses(
        (status = 200, description = "The file.", body = crate::openapi::File, content_type = "application/vnd.apple.mpegurl"),
        (status = 403, description = "The token does not open this file now (`code` says why)."),
        (status = 404, response = crate::openapi::NotFound),
    ),
    security(()),
)]
async fn keyed_hls(
    State(state): State<Arc<AppState>>,
    Path((show, token, path)): Path<(String, String, String)>,
) -> Response {
    let Some((mime, _)) = crate::public::hls_kind(&path) else {
        return problem(StatusCode::NOT_FOUND, "no such file");
    };
    let want = match weavesignal_keyed::hls_media(&path) {
        weavesignal_keyed::Media::Sound => TokenMedia::Sound,
        _ => TokenMedia::Video,
    };
    if let Err(r) = checked(&state, &token, &show, want) {
        return r;
    }
    if !crate::public::video_on(&state) {
        return problem(StatusCode::NOT_FOUND, "video is off");
    }
    let rt = &state.staff.api.access;
    if let Some(pool) = state.db.as_ref() {
        rt.live.refresh(pool, state.station.id).await;
    }
    let Some((bytes, written)) = crate::public::read_hls(&state, &path).await else {
        return problem(StatusCode::NOT_FOUND, "no such file");
    };
    match rt.live.classify(written) {
        Segment::Public => {}
        Segment::Keyed(s) if s == show => {}
        _ => return refused("other_show"),
    }
    let cache = if mime.ends_with("mpegurl") {
        "private, no-cache"
    } else {
        "private, max-age=30"
    };
    // No steering line: a keyed playlist's relative addresses carry the token; the edges'
    // steering would send the player to public paths.
    (
        [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
        bytes,
    )
        .into_response()
}

// ---- the keyed streams ----

/// How much of the listener process's answer is read for its headers.
const HEAD_MAX: usize = 8 * 1024;

#[utoipa::path(
    get,
    path = "/listen/k/{show}/{token}/{mount}",
    operation_id = "keyed_stream",
    tag = "listen",
    summary = "A keyed show's stream",
    description = "No bearer: the token in the path is the credential, and it must open sound. `mount` is `programme-keyed.opus` or `programme-keyed.mp3` (when the MP3 stream is on). Only while the show's window is open (with its margins). The stream is the keyed programme from the listener process, carried by the control plane so that it can end it: at once when the key is revoked or paused, when the token ends, and when the show's window closes. 403 with a `code` when refused.",
    params(
        ("show" = String, Path, description = "The show's id, as the token names it"),
        ("token" = String, Path, description = "The token"),
        ("mount" = String, Path, description = "`programme-keyed.opus` or `programme-keyed.mp3`"),
    ),
    responses(
        (status = 200, description = "The stream, until it is ended.", content_type = "audio/ogg"),
        (status = 403, description = "The token does not open this stream now (`code` says why)."),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn keyed_stream(
    State(state): State<Arc<AppState>>,
    Path((show, token, mount)): Path<(String, String, String)>,
) -> Response {
    if mount != KEYED_OPUS && mount != KEYED_MP3 {
        return problem(StatusCode::NOT_FOUND, "no such stream");
    }
    let claims = match checked(&state, &token, &show, TokenMedia::Sound) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(show_id) = Uuid::parse_str(&show) else {
        return refused("wrong_show");
    };
    let rt = &state.staff.api.access;
    if let Some(pool) = state.db.as_ref() {
        rt.live.refresh(pool, state.station.id).await;
    }
    // Only while this show alone is being served: until the last keyed show's listeners have
    // been dropped and the keyed mounts started again, their buffer may hold its end.
    let serving = rt.live.serving(now_ms() as i64);
    if !serving.contains(&show_id) {
        return refused("not_on_now");
    }
    if serving.len() > 1 {
        return refused("changing_show");
    }
    let Some(addr) = rt.listeners_addr.get().copied() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The listener process is not set up here.",
        );
    };
    let (up, content_type, first) = match open_mount(addr, &mount).await {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!(error = %e, mount, "keyed shows: the keyed mount did not answer");
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The keyed stream is not up.",
            );
        }
    };
    let end_s = claims.exp + rt.config.clock_skew_s as i64;
    let live = rt.live.clone();
    let key = claims.key.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    tokio::spawn(async move {
        let mut up = up;
        if !first.is_empty() && tx.send(Ok(Bytes::from(first))).await.is_err() {
            return;
        }
        let mut buf = vec![0u8; 16 * 1024];
        let why = loop {
            let now = now_ms() as i64;
            if now / 1000 >= end_s {
                break "the token ended";
            }
            if live.is_revoked(&key) {
                break "the key was revoked or paused";
            }
            if !live.serving(now).contains(&show_id) {
                break "the show's window closed";
            }
            match tokio::time::timeout(Duration::from_secs(1), up.read(&mut buf)).await {
                Err(_) => continue,
                Ok(Ok(0)) | Ok(Err(_)) => break "the keyed mount ended",
                Ok(Ok(n)) => {
                    if tx
                        .send(Ok(Bytes::copy_from_slice(&buf[..n])))
                        .await
                        .is_err()
                    {
                        break "the listener left";
                    }
                }
            }
        };
        tracing::debug!(why, "keyed shows: a keyed stream ended");
    });
    let mut res = Response::new(Body::from_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    ));
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("audio/ogg")),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// Open a mount on the listener process (loopback, HTTP/1.0): the connection, its content type
/// and what came after the headers.
async fn open_mount(
    addr: SocketAddr,
    mount: &str,
) -> std::io::Result<(tokio::net::TcpStream, String, Vec<u8>)> {
    let mut s = tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::other("connect timed out"))??;
    s.write_all(
        format!("GET /{mount} HTTP/1.0\r\nHost: {addr}\r\nUser-Agent: weavesignal-control\r\n\r\n")
            .as_bytes(),
    )
    .await?;
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    let end = loop {
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
            .await
            .map_err(|_| std::io::Error::other("no answer"))??;
        if n == 0 {
            return Err(std::io::Error::other("closed before its headers"));
        }
        head.extend_from_slice(&buf[..n]);
        if let Some(i) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if head.len() > HEAD_MAX {
            return Err(std::io::Error::other("headers too long"));
        }
    };
    let text = String::from_utf8_lossy(&head[..end]).to_string();
    let mut lines = text.lines();
    let status = lines.next().unwrap_or("");
    if status.split_whitespace().nth(1) != Some("200") {
        return Err(std::io::Error::other(format!("answered {status}")));
    }
    let ty = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.trim().to_owned())
        .unwrap_or_else(|| "audio/ogg".to_owned());
    Ok((s, ty, head[end + 4..].to_vec()))
}

// ---- the revoked list ----

#[utoipa::path(
    get,
    path = "/api/v1/keyed/revoked",
    operation_id = "keyed_revoked",
    tag = "listen",
    summary = "Keys revoked or paused lately",
    description = "No bearer: for edges (`EDGE_KEYED_REVOKED_URL`), which read it every half-minute or so and refuse these keys' tokens at once, and end their streams. `{\"revoked\": [key ids]}`: every key revoked or paused within two days (longer than any token lasts). Ids only, never a name or a key's text.",
    responses(
        (status = 200, description = "The ids.", body = serde_json::Value),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn revoked(State(state): State<Arc<AppState>>) -> Response {
    let Some(pool) = state.db.as_ref() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Keys are kept in the database.",
        );
    };
    match crate::keyed::read_revoked(pool, state.station.id).await {
        Ok(ids) => (
            [(header::CACHE_CONTROL, "no-store")],
            axum::Json(json!({ "revoked": ids })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "keyed shows: cannot read the revoked keys");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
        }
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(keyed_hls, keyed_stream, revoked))]
pub(crate) struct Api;
