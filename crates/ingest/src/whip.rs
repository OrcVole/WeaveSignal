// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WHIP from a host's own software (17d), glued to the supervisor as Icecast and SRT are: the
//! same `admit`, one connection per key, the 2 Mbit/s cap, the report loop and the decoding
//! worker, with `way = "whip"` and the worker's `opus-rtp` format.
//!
//! The host's software posts its offer to the station's public domain (`/whip/{key id}`); the
//! control plane passes the request on to [`serve`]'s loopback listener (`ingest_whip`) with the
//! ingest's bearer and the client's address. The media comes to the one UDP port of
//! [`crate::whip_mux`], ICE-lite with the station's public address as its only candidate.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::opus_frames;
use crate::supervisor::{Extras, Ingest, claim, full, session};
use crate::whip_http::{self, Request, Response};
use crate::whip_mux::{self, MuxHandle};
use crate::whip_rtc::{self, WhipError};

/// The header with the ingest's bearer, from the control plane.
pub const INGEST_HEADER: &str = "x-weavesignal-ingest";
/// The header with the address the host's request came from, as the control plane saw it.
pub const ADDRESS_HEADER: &str = "x-weavesignal-address";
/// The most a forwarded request's head may be.
const MAX_HEAD: usize = 16 * 1024;

/// The WHIP side while it listens.
struct Whip {
    mux: MuxHandle,
    candidate: SocketAddr,
    /// The session each key's connection is, for its `DELETE`.
    sessions: Mutex<HashMap<String, String>>,
    next: AtomicU64,
}

/// Aborts a task when dropped: the mux goes when [`serve`] is stopped.
struct Owned(JoinHandle<()>);

impl Drop for Owned {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Take WHIP: media on `udp` (reached at `candidate`), requests from the control plane on the
/// settings' `whip_http`. Runs until aborted.
pub(crate) async fn serve(shared: Ingest, udp: SocketAddr, candidate: SocketAddr) {
    let socket = loop {
        match UdpSocket::bind(udp).await {
            Ok(s) => break s,
            Err(e) => {
                tracing::error!(%udp, error = %e, "cannot listen for WHIP's media; trying again in 10 s");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    };
    let http = shared.settings.whip_http;
    let listener = loop {
        match TcpListener::bind(http).await {
            Ok(l) => break l,
            Err(e) => {
                tracing::error!(%http, error = %e, "cannot listen for WHIP's requests; trying again in 10 s");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    };
    let (mux, runner) = whip_mux::new(candidate);
    let _mux = Owned(tokio::spawn(runner.run(socket)));
    let whip = Arc::new(Whip {
        mux,
        candidate,
        sessions: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
    });
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let (s, w) = (shared.clone(), whip.clone());
                tokio::spawn(async move {
                    if let Err(e) = request(s, w, stream).await {
                        tracing::info!(error = %format!("{e:#}"), "a WHIP request failed");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "WHIP's request listener");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// Equal in constant time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A forwarded request: method, path, headers, body.
type Http = (String, String, Vec<(String, String)>, Vec<u8>);

async fn read_request(stream: &mut TcpStream) -> Result<Http> {
    let mut buf = Vec::with_capacity(4096);
    let mut piece = [0u8; 4096];
    let end = loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut piece))
            .await
            .context("no request within 10 s")??;
        if n == 0 {
            bail!("closed before its request");
        }
        buf.extend_from_slice(&piece[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        if buf.len() > MAX_HEAD {
            bail!("a head over {MAX_HEAD} bytes");
        }
    };
    let mut body = buf.split_off(end + 4);
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_owned(), v.trim().to_owned()))
        })
        .collect();
    let length: usize = whip_http::header(&headers, "content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if length > whip_http::MAX_BODY {
        // Read nothing more: the parse refuses it by its size.
        body.resize(length.min(whip_http::MAX_BODY + 1), 0);
        return Ok((method, path, headers, body));
    }
    while body.len() < length {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut piece))
            .await
            .context("the body did not arrive within 10 s")??;
        if n == 0 {
            bail!("closed before its body");
        }
        body.extend_from_slice(&piece[..n]);
    }
    body.truncate(length);
    Ok((method, path, headers, body))
}

async fn request(shared: Ingest, whip: Arc<Whip>, mut stream: TcpStream) -> Result<()> {
    let (method, path, headers, body) = read_request(&mut stream).await?;
    let given = whip_http::header(&headers, INGEST_HEADER).unwrap_or_default();
    let answer = if !same(given.as_bytes(), shared.settings.token.as_bytes()) {
        whip_http::refusal(401, "Not the control plane.")
    } else {
        let addr = whip_http::header(&headers, ADDRESS_HEADER)
            .unwrap_or("unknown")
            .chars()
            .filter(|c| c.is_ascii_hexdigit() || matches!(c, '.' | ':'))
            .take(64)
            .collect::<String>();
        match whip_http::parse(&method, &path, &headers, &body) {
            Err(bad) => bad.response(),
            Ok(Request::Options) => whip_http::options(),
            Ok(Request::Offer {
                key_id,
                secret,
                sdp,
            }) => offer(&shared, &whip, key_id, secret, &sdp, addr).await,
            Ok(Request::End {
                key_id,
                session,
                secret,
            }) => end(&shared, &whip, &key_id, &session, &secret),
        }
    };
    stream.write_all(&answer.to_bytes()).await?;
    stream.shutdown().await.ok();
    Ok(())
}

/// The status for a refusal's reason: Icecast's, with an in-use key a conflict and a full desk
/// or a busy station unavailable.
fn status_for(reason: Option<&str>) -> u16 {
    match reason {
        Some("wrong_key" | "too_many") => 401,
        Some("in_use") => 409,
        Some("desk_full") => 503,
        _ => 403,
    }
}

async fn offer(
    shared: &Ingest,
    whip: &Arc<Whip>,
    key_id: String,
    secret: String,
    sdp: &str,
    addr: String,
) -> Response {
    if full(shared) {
        return whip_http::refusal(503, "The station is busy. Try again in a minute.");
    }
    let admitted = match shared
        .control
        .admit(&key_id, Some(&secret), "whip", &addr, false)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "WHIP: cannot ask the control plane");
            return whip_http::refusal(503, "The station cannot take sources just now.");
        }
    };
    if admitted.decision == "refused" {
        let why = admitted.message.clone().unwrap_or_default();
        return whip_http::refusal(status_for(admitted.reason.as_deref()), &why);
    }
    let (rtc, answer) = match whip_rtc::answer(sdp, whip.candidate, Instant::now()) {
        Ok(x) => x,
        Err(e) => {
            // An offer the station cannot take is a refusal for its format; a failure of the
            // station's own is only logged here.
            let status = if matches!(e, WhipError::Station(_)) {
                tracing::warn!(key = %key_id, error = %e, "WHIP: cannot answer an offer");
                500
            } else {
                shared
                    .control
                    .event(
                        &key_id,
                        "refused",
                        Some("format"),
                        &e.to_string(),
                        "whip",
                        Some(&addr),
                    )
                    .await;
                400
            };
            let why = match e {
                WhipError::NoAudio => {
                    "Your software offers no Opus sound. Turn on its audio track (Opus).".to_owned()
                }
                other => other.to_string(),
            };
            return whip_http::refusal(status, &why);
        }
    };
    if !claim(shared, &key_id) {
        shared
            .control
            .event(&key_id, "refused", Some("in_use"), "", "whip", Some(&addr))
            .await;
        return whip_http::refusal(
            409,
            "Another connection is using this key. Close your other software first.",
        );
    }
    // Not a secret: a `DELETE` needs the key's secret as well.
    let session_id = format!(
        "{:x}{:04x}",
        crate::supervisor::now_ms(),
        whip.next.fetch_add(1, Ordering::Relaxed) & 0xffff
    );
    let Some(mut media) = whip.mux.add(session_id.clone(), rtc) else {
        return whip_http::refusal(503, "The station cannot take WHIP just now.");
    };
    whip.sessions
        .lock()
        .expect("whip lock")
        .insert(key_id.clone(), session_id.clone());
    let (tx, rx) = mpsc::channel::<Bytes>(128);
    let (s, w, k, sid) = (
        shared.clone(),
        whip.clone(),
        key_id.clone(),
        session_id.clone(),
    );
    tokio::spawn(async move {
        let carry = tokio::spawn(async move {
            while let Some((ts, payload)) = media.recv().await {
                if tx
                    .send(Bytes::from(opus_frames::write(ts, &payload)))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
        session(
            s,
            k.clone(),
            "whip",
            addr,
            admitted,
            "opus-rtp",
            rx,
            Some(secret),
            Extras::default(),
        )
        .await;
        carry.abort();
        let mut sessions = w.sessions.lock().expect("whip lock");
        if sessions.get(&k) == Some(&sid) {
            sessions.remove(&k);
        }
    });
    whip_http::created(&answer, &format!("/whip/{key_id}/{session_id}"))
}

fn end(shared: &Ingest, whip: &Whip, key_id: &str, session: &str, secret: &str) -> Response {
    let known = whip
        .sessions
        .lock()
        .expect("whip lock")
        .get(key_id)
        .is_some_and(|s| s == session);
    if !known {
        return whip_http::refusal(404, "No such session: it has ended already.");
    }
    let proved = shared
        .conns
        .lock()
        .expect("ingest lock")
        .get(key_id)
        .and_then(|c| c.secret.as_deref())
        .is_some_and(|s| same(s.as_bytes(), secret.as_bytes()));
    if !proved {
        return whip_http::refusal(401, "The key is wrong.");
    }
    whip.mux.end(session);
    whip_http::ended()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_map_as_icecasts_with_a_conflict_and_a_full_desk() {
        assert_eq!(status_for(Some("wrong_key")), 401);
        assert_eq!(status_for(Some("too_many")), 401);
        assert_eq!(status_for(Some("in_use")), 409);
        assert_eq!(status_for(Some("desk_full")), 503);
        assert_eq!(status_for(Some("not_your_slot")), 403);
        assert_eq!(status_for(None), 403);
    }

    #[test]
    fn same_is_equality() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
    }
}
