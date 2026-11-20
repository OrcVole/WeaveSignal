// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WHIP's public routes (17d): a host's OBS posts its offer to `/whip/{key id}` on the station's
//! public domain, with the show key's secret as the bearer, and ends with a `DELETE` of the
//! session's address. The control plane decides nothing here: it passes the request on to the
//! ingest's loopback listener (`ports.ingest_whip`) with the ingest's bearer and the client's
//! address, and passes the ingest's answer back. The ingest then asks `admit` as it does for
//! Icecast, so the key, the slot's door, the wrong-key count and the one-connection rule are the
//! sources' own.
//!
//! `PATCH` (trickle ICE, an ICE restart) is not offered: the router answers it 405.
//!
//! `OPTIONS` says what the endpoint takes and offers no ICE servers: the station is ICE-lite
//! with its own address as the one candidate, and asks no STUN or TURN of anyone (ADR-0290).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, post};
use axum::{Extension, Router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::app::AppState;
use crate::hub::now_ms;

/// The longest offer taken (the ingest's own limit).
const MAX_OFFER: usize = 32 * 1024;
/// The most of the ingest's answer read.
const MAX_ANSWER: usize = 64 * 1024;
/// The header with the ingest's bearer, as the ingest reads it.
const INGEST_HEADER: &str = "x-weavesignal-ingest";
/// The header with the client's address, as the ingest reads it.
const ADDRESS_HEADER: &str = "x-weavesignal-address";
/// The ingest's answer's headers passed back.
const PASSED: [&str; 6] = [
    "content-type",
    "location",
    "allow",
    "accept-post",
    "www-authenticate",
    "retry-after",
];

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/whip/{key}", post(offer).options(options))
        .route("/whip/{key}/{session}", delete(end))
        .layer(DefaultBodyLimit::max(MAX_OFFER + 1024))
}

/// A refusal as WHIP's clients show it: plain text.
fn refusal(status: StatusCode, why: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("{why}\n"),
    )
        .into_response()
}

#[utoipa::path(
    post,
    path = "/whip/{key}",
    operation_id = "whip_offer",
    tag = "sources",
    summary = "Send from OBS over WHIP",
    description = "WHIP (RFC 9725) for a host's own software (17d): the body is an SDP offer (`application/sdp`) and the bearer is the show key's secret (OBS's *Bearer Token*; `<key id>-<secret>` is taken too). The reply is `201` with the SDP answer and the session's address in `Location`. The station answers ICE-lite with one host candidate, its public IPv4 address and its WHIP port (`ports.source_whip`, UDP, off unless the installation turns it on), and takes the Opus sound only: a picture the software sends is received and discarded, so OBS needs no change. From there the source is a host's own studio like Icecast and SRT (ADR-0280): standing by in its show's slot, a private test outside it, refused with a wrong key (five wrong keys from an address in a minute refuse it for ten minutes). No trickle ICE.",
    params(("key" = String, Path, description = "The show key's id (8 characters)")),
    request_body(content = String, content_type = "application/sdp", description = "The SDP offer."),
    responses(
        (status = 201, description = "The SDP answer; `Location` is the session's address.", content_type = "application/sdp", body = String),
        (status = 400, description = "The offer has no Opus sound, or is not one the station can read.", content_type = "text/plain", body = String),
        (status = 401, description = "No bearer, or the wrong key.", content_type = "text/plain", body = String),
        (status = 403, description = "Refused: not the key's slot with tests off, the key ended, or turned away.", content_type = "text/plain", body = String),
        (status = 409, description = "Another connection is using this key.", content_type = "text/plain", body = String),
        (status = 413, description = "The offer is over 32 KB.", content_type = "text/plain", body = String),
        (status = 415, description = "Not `application/sdp`.", content_type = "text/plain", body = String),
        (status = 429, description = "Too many wrong keys from this address.", content_type = "text/plain", body = String),
        (status = 503, description = "WHIP is off at this station, or the ingest or the desk is busy or away.", content_type = "text/plain", body = String),
    ),
    security(("show_key" = [])),
)]
async fn offer(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Bytes,
) -> Response {
    pass(
        &state,
        Method::POST,
        &format!("/whip/{key}"),
        &headers,
        conn,
        body,
    )
    .await
}

/// `OPTIONS`: what the endpoint takes. Not in the API description: no bearer, no body.
async fn options(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    pass(
        &state,
        Method::OPTIONS,
        &format!("/whip/{key}"),
        &headers,
        conn,
        Bytes::new(),
    )
    .await
}

#[utoipa::path(
    delete,
    path = "/whip/{key}/{session}",
    operation_id = "whip_end",
    tag = "sources",
    summary = "End a WHIP session",
    description = "The host's software ends its WHIP session (the address `Location` gave), with the same bearer as its offer: the source ends as when Icecast or SRT disconnects.",
    params(
        ("key" = String, Path, description = "The show key's id"),
        ("session" = String, Path, description = "The session, from `Location`"),
    ),
    responses(
        (status = 200, description = "Ended."),
        (status = 401, description = "No bearer, or the wrong key.", content_type = "text/plain", body = String),
        (status = 404, description = "No such session: it has ended already.", content_type = "text/plain", body = String),
        (status = 503, description = "WHIP is off at this station.", content_type = "text/plain", body = String),
    ),
    security(("show_key" = [])),
)]
async fn end(
    State(state): State<Arc<AppState>>,
    Path((key, session)): Path<(String, String)>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    pass(
        &state,
        Method::DELETE,
        &format!("/whip/{key}/{session}"),
        &headers,
        conn,
        Bytes::new(),
    )
    .await
}

/// A path segment as it may be passed on: letters and digits only.
fn plain(path: &str) -> bool {
    path.strip_prefix("/whip/").is_some_and(|rest| {
        rest.split('/')
            .all(|p| !p.is_empty() && p.len() <= 64 && p.chars().all(|c| c.is_ascii_alphanumeric()))
    })
}

/// Hand the request to the ingest and its answer back.
async fn pass(
    state: &AppState,
    method: Method,
    path: &str,
    headers: &HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Bytes,
) -> Response {
    if !plain(path) {
        return refusal(
            StatusCode::NOT_FOUND,
            "There is no WHIP endpoint at this address.",
        );
    }
    if body.len() > MAX_OFFER {
        return refusal(StatusCode::PAYLOAD_TOO_LARGE, "The offer is too large.");
    }
    let live = &state.hub.sources;
    let Some(token) = live.ingest_token() else {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "This station does not take WHIP: its ingest is not set up.",
        );
    };
    let Some(pool) = state.db.as_ref() else {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "This station does not take WHIP: it has no database.",
        );
    };
    let rules = match weavesignal_model::sources::get_settings(pool, state.station.id).await {
        Ok(s) => live.in_force(s),
        Err(e) => {
            tracing::error!(error = %e, "whip: database");
            return refusal(StatusCode::INTERNAL_SERVER_ERROR, "The database failed.");
        }
    };
    if rules.port_whip == 0 {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "This station has not turned WHIP on. Send over Icecast or SRT instead.",
        );
    }
    let addr = crate::public::peer(
        state.join.forwarded_for,
        headers,
        conn.as_ref().map(|Extension(ConnectInfo(a))| *a),
    );
    let addr = match addr {
        Some(ip) => ip.to_string(),
        // Loopback with no proxy's address: a test bench, never limited.
        None => conn.map_or_else(
            || "127.0.0.1".to_owned(),
            |Extension(ConnectInfo(a))| a.ip().to_string(),
        ),
    };
    if let Some(until) = live.blocked_until(&addr, now_ms() as i64) {
        let mut res = refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many wrong keys from your address. Try again in a few minutes.",
        );
        let wait = ((until - now_ms() as i64).max(1000) / 1000).to_string();
        if let Ok(v) = HeaderValue::from_str(&wait) {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return res;
    }
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: ingest\r\n{INGEST_HEADER}: {token}\r\n{ADDRESS_HEADER}: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for name in [header::AUTHORIZATION, header::CONTENT_TYPE] {
        if let Some(v) = headers.get(&name).and_then(|v| v.to_str().ok()) {
            if v.contains(['\r', '\n']) {
                continue;
            }
            head.push_str(&format!("{name}: {v}\r\n"));
        }
    }
    head.push_str("\r\n");
    let port = live.ingest_whip();
    match forward(port, head.as_bytes(), &body).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(port, error = %format!("{e:#}"), "whip: the ingest did not answer");
            refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station cannot take WHIP just now: try again in a minute.",
            )
        }
    }
}

/// Send the request to the ingest on loopback and read its whole answer.
async fn forward(port: u16, head: &[u8], body: &[u8]) -> anyhow::Result<Response> {
    use anyhow::Context;
    let wait = Duration::from_secs(15);
    let mut stream = tokio::time::timeout(
        Duration::from_secs(3),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .context("the ingest did not answer")??;
    stream.write_all(head).await?;
    stream.write_all(body).await?;
    let mut raw = Vec::new();
    tokio::time::timeout(
        wait,
        (&mut stream).take(MAX_ANSWER as u64).read_to_end(&mut raw),
    )
    .await
    .context("the ingest's answer took too long")??;
    parse_answer(&raw).context("the ingest's answer is not HTTP")
}

/// The ingest's answer as a response: its status, the headers WHIP needs, its body.
fn parse_answer(raw: &[u8]) -> Option<Response> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let status = StatusCode::from_u16(status).ok()?;
    let mut res = Response::builder().status(status);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        if PASSED.contains(&name.as_str()) {
            res = res.header(name, value.trim());
        }
    }
    res.body(Body::from(raw[end + 4..].to_vec())).ok()
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(offer, end))]
pub struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_paths_are_passed_on() {
        assert!(plain("/whip/abcd2345"));
        assert!(plain("/whip/abcd2345/s1"));
        assert!(!plain("/whip/"));
        assert!(!plain("/whip/abcd2345/"));
        assert!(!plain("/whip/ab%20cd"));
        assert!(!plain("/whip/a b"));
        assert!(!plain("/other/abcd2345"));
    }

    #[tokio::test]
    async fn the_ingests_answer_is_passed_back_with_whips_headers_only() {
        let raw = b"HTTP/1.1 201 Created\r\nContent-Type: application/sdp\r\nLocation: /whip/abcd2345/s1\r\nX-Other: no\r\nContent-Length: 5\r\nConnection: close\r\n\r\nv=0\r\n";
        let r = parse_answer(raw).unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
        assert_eq!(r.headers()["location"], "/whip/abcd2345/s1");
        assert_eq!(r.headers()["content-type"], "application/sdp");
        assert!(r.headers().get("x-other").is_none());
        let body = axum::body::to_bytes(r.into_body(), 100).await.unwrap();
        assert_eq!(&body[..], b"v=0\r\n");
        assert!(parse_answer(b"garbage").is_none());
    }
}
