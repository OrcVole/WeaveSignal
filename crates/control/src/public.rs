// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The routes the player and join pages use (M3). None needs the admin bearer; the ones
//! that act for one person take that person's pass (`Authorization: Bearer rsp1...`).
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/player` | anyone | the station, the show, the stream, whether call-in is open, and the consent words with their version |
//! | `GET /api/v1/call-in/challenge` | a listener | a proof-of-work challenge for the next call ([`crate::challenge`]) |
//! | `POST /api/v1/call-in` | a listener | name, topic, consent and the answered challenge: the consent is logged, then a caller's room token and pass |
//! | `GET /api/v1/pass` | a pass | what the pass is for: role, name, station, expiry (the invite page) |
//! | `POST /api/v1/pass/token` | a pass | a fresh room token for the pass's identity: joining, and rejoining within the held 5 min |
//! | `GET /api/v1/pass/events` | a pass | that person's own state as server-sent events ([`MeState`]) |
//! | `POST /api/v1/pass/leave` | a pass | a caller hangs up: out of the queue, no held place, the pass ended |
//! | `POST /api/v1/invites` | admin | an invite pass for a guest, a remote DJ or a presenter, and its link |
//!
//! **The consent rule.** No caller token is signed without a consent on file: the call-in
//! route refuses a request without `consent.accepted = true` against the words' current
//! version, logs the consent (who, when, which words) to the control plane's log and, with a
//! database, to `caller_consent`, and only then signs. The admin token route asks the same of
//! a caller token (consent given by the station on the caller's behalf).

// The `Err` of these helpers is the response to send, built once per refused request.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::IntervalStream;
use weavesignal_proto::{PassClaims, PassKind, Role};

use crate::app::{AppState, TokenRequest, problem, refusal};
use crate::hub::now_ms;
use crate::join::{
    Limiter, MeState, NAME_MAX, TOPIC_MAX, clean_line, consent_version, consent_words, me_state,
    mint_identity,
};
use crate::studio::{CallIn, Consent, ConsentBy};

/// How often a person's state is looked at for their event stream.
const ME_TICK: Duration = Duration::from_millis(300);
/// How long an ended pass is remembered: the longest a pass lives.
const ENDED_KEEP_MS: u64 = 24 * 3600 * 1000;

/// Call-in's rate limit, and the passes the studio has ended (a dropped or departed caller).
#[derive(Debug, Default)]
pub struct Joins {
    limiter: Mutex<Limiter>,
    ended: Mutex<BTreeMap<String, u64>>,
    /// Call-in's challenges: the key and the answers already used (ADR-0006).
    challenges: crate::challenge::Challenges,
}

impl Joins {
    /// End an identity's pass: no more tokens, and its page says the call ended.
    pub fn end(&self, identity: &str) {
        let now = now_ms();
        let mut ended = self.ended.lock().expect("joins lock");
        ended.retain(|_, at| now.saturating_sub(*at) < ENDED_KEEP_MS);
        ended.insert(identity.to_owned(), now);
    }

    /// Put back an end read from the database at start (a later end in memory wins).
    pub fn restore(&self, identity: &str, at_ms: u64) {
        let mut ended = self.ended.lock().expect("joins lock");
        let at = ended.entry(identity.to_owned()).or_insert(at_ms);
        *at = (*at).max(at_ms);
    }

    /// Whether an identity's pass was ended at or after `since_s` (a pass issued later, for a
    /// new call, is not ended by an old drop).
    pub fn ended(&self, identity: &str, since_s: u64) -> bool {
        self.ended
            .lock()
            .expect("joins lock")
            .get(identity)
            .is_some_and(|&at| at >= since_s * 1000)
    }
}

/// End a person's pass (ADR-0119): it buys no more room tokens, now and after a restart, and
/// a caller's call ends with it (out of the queue, no held place, its consent's period starts).
/// Taking them out of the room is the caller's next step: ended first, a page that rejoins at
/// once finds its pass refused.
pub async fn end_pass(state: &AppState, identity: &str) {
    state.joins.end(identity);
    if let Some(pool) = &state.db
        && let Err(e) = weavesignal_model::passes::end_pass(pool, state.station.id, identity).await
    {
        tracing::warn!(%identity, error = %e, "cannot keep the ended pass in the database; it is ended until the control plane restarts");
    }
    if state.hub.read_studio(|s| s.call_in_of(identity).is_some()) {
        state.hub.with_studio(|s, now| s.forget(identity, now));
        crate::retention::call_ended(state, identity).await;
    }
}

/// Read back the passes the studio ended, at start.
pub async fn restore_ended_passes(state: &AppState) -> usize {
    let Some(pool) = &state.db else { return 0 };
    match weavesignal_model::passes::ended_passes(pool, state.station.id).await {
        Ok(list) => {
            for (identity, at_ms) in &list {
                state.joins.restore(identity, *at_ms);
            }
            list.len()
        }
        Err(e) => {
            tracing::warn!(error = %e, "cannot read back the ended passes; passes ended before this start buy tokens again until they expire");
            0
        }
    }
}

/// The consent the caller gave on the page: that they accepted, and the words' version.
#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsentGiven {
    pub accepted: bool,
    pub version: String,
}

/// A problem about one field, for the page to put beside it.
fn field_problem(code: StatusCode, field: &str, detail: impl Into<String>) -> Response {
    #[derive(Serialize)]
    struct P<'a> {
        error: String,
        field: &'a str,
    }
    (
        code,
        Json(P {
            error: detail.into(),
            field,
        }),
    )
        .into_response()
}

/// The consent words the station shows now, and their version: `{kept}` from the station's
/// Jurisdiction setting (its programme recordings' period; No retention without a database).
async fn words(state: &AppState) -> (String, String) {
    let setting = match &state.db {
        Some(pool) => weavesignal_model::retention::load_setting(pool, state.station.id)
            .await
            .unwrap_or_default(),
        None => weavesignal_model::retention::Setting::default(),
    };
    let kept = crate::join::kept_phrase(setting.periods().programme_recordings);
    let w = consent_words(
        &state.join.consent_text,
        &state.station.name,
        &kept,
        &state.join.consent_contact,
    );
    let v = consent_version(&w);
    (w, v)
}

/// Whether the words name a contact the station has not given: call-in waits for it.
fn contact_missing(state: &AppState) -> bool {
    state.join.consent_text.contains("{contact}") && state.join.consent_contact.trim().is_empty()
}

/// Check a consent against the words shown now: `Err` is the refusal to send.
async fn check_consent(state: &AppState, given: Option<&ConsentGiven>) -> Result<String, Response> {
    let (_, version) = words(state).await;
    match given {
        Some(c) if c.accepted && c.version == version => Ok(version),
        Some(c) if c.accepted => Err(field_problem(
            StatusCode::CONFLICT,
            "consent",
            "The words about recording have changed since this page loaded. Read them again and tick the box.",
        )),
        _ => Err(field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "consent",
            "Tick the box to agree to your call being broadcast and recorded. You cannot call without it.",
        )),
    }
}

/// Keep a consent: in the studio for the screener, in the log, and in the database if there
/// is one. The log line carries who, when and which words.
async fn keep_consent(state: &AppState, call: CallIn) -> Result<(), Response> {
    let c = &call.consent;
    tracing::info!(
        target: "weavesignal::consent",
        identity = %call.identity,
        name = %call.name,
        topic = %call.topic,
        by = ?c.by,
        at_ms = c.at_ms,
        version = %c.version,
        words = %c.text,
        "consent to record given"
    );
    if let Some(pool) = &state.db {
        let record = weavesignal_model::ConsentRecord {
            identity: call.identity.clone(),
            name: call.name.clone(),
            topic: call.topic.clone(),
            given_by: match c.by {
                ConsentBy::Caller => "caller",
                ConsentBy::Station => "station",
            },
            words: c.text.clone(),
            words_version: c.version.clone(),
            accepted_at_ms: c.at_ms,
        };
        if let Err(e) = weavesignal_model::record_consent(pool, state.station.id, &record).await {
            // A consent that cannot be kept is not given: no token.
            tracing::error!(error = %e, identity = %call.identity, "cannot keep the consent");
            return Err(problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not save your consent, so the call cannot go ahead. Try again in a minute.",
            ));
        }
    }
    state.hub.with_studio(|s, _| s.call_in(call));
    Ok(())
}

/// The admin token route's caller: consent given by the station on the caller's behalf.
pub(crate) async fn record_station_consent(
    state: &AppState,
    req: &TokenRequest,
) -> Result<(), Response> {
    let version = check_consent(state, req.consent.as_ref()).await?;
    let (text, _) = words(state).await;
    keep_consent(
        state,
        CallIn {
            identity: req.identity.clone(),
            name: req.name.clone(),
            topic: String::new(),
            consent: Consent {
                at_ms: now_ms(),
                version,
                by: ConsentBy::Station,
                text,
            },
        },
    )
    .await
}

// ---- the player page ----

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlayerInfo {
    pub station: String,
    pub show: String,
    /// The listener stream through the proxy; empty when not set up.
    pub stream_url: String,
    pub presenter: String,
    pub presenter_live: bool,
    /// Listeners hear the show now.
    pub show_on_air: bool,
    pub call_in: CallInInfo,
    /// TURN over TLS, for networks and browsers that block WebRTC's UDP (ADR-0059, ADR-0080).
    pub relay: RelayInfo,
    /// The picture and its sound as HLS, while the desk has video on (ADR-0009); `None`
    /// otherwise, and the player plays `stream_url`.
    pub video: Option<VideoInfo>,
}

/// Where the player finds the HLS: `live` (AV1 with Opus) and `audio` (the same Opus alone:
/// the audio-only toggle, ADR-0026, and older Apple devices, ADR-0024).
#[derive(Serialize, utoipa::ToSchema)]
pub struct VideoInfo {
    pub live: String,
    pub audio: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CallInInfo {
    pub open: bool,
    /// Why not, in a sentence for the listener.
    pub why_not: Option<String>,
    pub consent_text: String,
    pub consent_version: String,
    pub name_max: usize,
    pub topic_max: usize,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RelayInfo {
    pub turn_tls_port: u16,
}

fn call_in_closed(state: &AppState) -> Option<String> {
    if !state.join.call_in {
        Some("This station does not take calls from the player page.".to_owned())
    } else if state.room.is_none() {
        Some("Calls are not set up at this station yet.".to_owned())
    } else if contact_missing(state) {
        Some("Calls are not set up at this station yet: it has not said how to ask for a call to be taken down.".to_owned())
    } else if !state.hub.view().desk.reachable {
        Some(
            "The studio is not taking calls right now. Keep listening and try again later."
                .to_owned(),
        )
    } else {
        None
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/player",
    operation_id = "public_player",
    tag = "public",
    summary = "The player page's facts",
    description = "The station, the show, the stream, whether the presenter is live, whether call-in is open (and the consent words), and the HLS addresses while the desk has video on.",
    responses(
        (status = 200, description = "The facts.", body = PlayerInfo),
    ),
    security(()),
)]
pub async fn player(State(state): State<Arc<AppState>>) -> Json<PlayerInfo> {
    let view = state.hub.view();
    let facts = me_state(
        "",
        Role::Listener,
        &view,
        state.hub.desk_status().as_ref(),
        false,
    );
    let why_not = call_in_closed(&state);
    let (text, version) = words(&state).await;
    Json(PlayerInfo {
        station: state.station.name.clone(),
        show: view.show.title.clone(),
        stream_url: state.join.stream_url.clone(),
        presenter: facts.presenter,
        presenter_live: facts.presenter_live,
        show_on_air: facts.show_on_air,
        call_in: CallInInfo {
            open: why_not.is_none(),
            why_not,
            consent_text: text,
            consent_version: version,
            name_max: NAME_MAX,
            topic_max: TOPIC_MAX,
        },
        relay: RelayInfo {
            turn_tls_port: state.turn_tls_port,
        },
        video: video_on(&state).then(|| {
            let base = state.join.hls_url.trim_end_matches('/');
            VideoInfo {
                live: format!("{base}/live.m3u8"),
                audio: format!("{base}/audio.m3u8"),
            }
        }),
    })
}

/// Whether the desk is writing HLS now.
fn video_on(state: &AppState) -> bool {
    state
        .hub
        .desk_status()
        .and_then(|d| d.video)
        .is_some_and(|v| v.hls)
}

/// A file of the desk's HLS (`/hls/{path}`): playlists never cached, segments briefly. Only
/// names the desk writes (letters, digits, `-`, `_`, `.`; at most two levels) are served,
/// and nothing while video is off.
#[utoipa::path(
    get,
    path = "/hls/{path}",
    operation_id = "public_hls_file",
    tag = "public",
    summary = "A file of the desk's HLS",
    description = "Playlists never cached, segments briefly. Only names the desk writes (letters, digits, `-`, `_`, `.`; at most two levels) are served, and nothing while video is off. The multivariant playlists name the steering route while steering has edges (ADR-0159).",
    params(("path" = String, Path, description = "A playlist or segment, such as `live.m3u8` or `audio/seg1.m4s`")),
    responses(
        (status = 200, description = "The file.", body = crate::openapi::File, content_type = "application/vnd.apple.mpegurl"),
        (status = 404, response = crate::openapi::NotFound),
    ),
    security(()),
)]
pub async fn hls_file(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    let parts: Vec<&str> = path.split('/').collect();
    let safe = !path.is_empty()
        && path.len() <= 96
        && parts.len() <= 2
        && parts.iter().all(|c| {
            !c.is_empty()
                && !c.starts_with('.')
                && c.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        });
    let kind = match path.rsplit_once('.').map(|(_, e)| e) {
        Some("m3u8") => Some(("application/vnd.apple.mpegurl", "no-cache")),
        Some("m4s") | Some("mp4") if path.starts_with("audio/") => {
            Some(("audio/mp4", "public, max-age=30"))
        }
        Some("m4s") | Some("mp4") => Some(("video/mp4", "public, max-age=30")),
        _ => None,
    };
    let (Some((mime, cache)), true) = (kind, safe) else {
        return problem(StatusCode::NOT_FOUND, "no such file".to_owned());
    };
    if !video_on(&state) {
        return problem(StatusCode::NOT_FOUND, "video is off".to_owned());
    }
    let file = state.join.hls_dir.join(&path);
    let read = tokio::task::spawn_blocking(move || std::fs::read(file)).await;
    match read {
        Ok(Ok(bytes)) => {
            // The desk's top-level playlists are the multivariant ones; they get the
            // steering line while steering has edges (ADR-0159).
            let bytes = match state.steering.playlist_line() {
                Some(line) if parts.len() == 1 && mime.ends_with("mpegurl") => {
                    with_steering(bytes, &line)
                }
                _ => bytes,
            };
            (
                [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
                bytes,
            )
                .into_response()
        }
        _ => problem(StatusCode::NOT_FOUND, "no such file".to_owned()),
    }
}

/// A multivariant playlist with the steering line as its second line, after `#EXTM3U`. Any
/// other text (not UTF-8, no `#EXTM3U`, or already steered) is returned as it was.
fn with_steering(bytes: Vec<u8>, line: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return bytes;
    };
    match text.strip_prefix("#EXTM3U\n") {
        Some(rest) if !text.contains("#EXT-X-CONTENT-STEERING") => {
            format!("#EXTM3U\n{line}\n{rest}").into_bytes()
        }
        _ => bytes,
    }
}

// ---- call-in ----

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CallInRequest {
    pub name: String,
    pub topic: String,
    #[serde(default)]
    pub consent: Option<ConsentGiven>,
    /// The answered challenge ([`crate::challenge`]); needed unless `join.challenge_bits` is 0.
    #[serde(default)]
    pub challenge: Option<crate::challenge::Answer>,
}

/// What a person gets to join: a room token, its pass, and the one track to listen to.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Joined {
    pub pass: String,
    pub token: String,
    pub url: String,
    pub room: String,
    pub identity: String,
    pub role: Role,
    pub name: String,
    /// The desk's participant that publishes this person's mix-minus: the only one to
    /// subscribe to (every mix-minus is visible to everyone in the room).
    pub hear: String,
    /// While the desk has video on: the desk's participant that publishes the programme
    /// picture, the only picture to subscribe to (ADR-0028), and the page offers the camera.
    pub see: Option<String>,
    pub pass_expires_at_ms: u64,
}

/// The caller's address for the per-address limit. Behind the station's reverse proxy
/// (ADR-0048 rule 3) every connection comes from loopback, so a loopback peer is no address at
/// all unless `join.forwarded_for` takes the proxy's header: counting the proxy would turn six
/// calls per address into six calls an hour for the whole station.
pub(crate) fn peer(
    forwarded_for: bool,
    headers: &HeaderMap,
    conn: Option<SocketAddr>,
) -> Option<IpAddr> {
    if forwarded_for && let Some(ip) = forwarded_address(headers) {
        return Some(ip).filter(|ip| !ip.is_loopback());
    }
    conn.map(|c| c.ip()).filter(|ip| !ip.is_loopback())
}

/// The address the station's own proxy saw: the **last** `X-Forwarded-For` entry. A proxy that
/// appends (Cloudron's nginx, `$proxy_add_x_forwarded_for`) keeps whatever the client sent in
/// front of it, so the first entry is the client's to forge; one that replaces the header
/// (the Compose bundle's Caddy) sends a single entry, which is also the last.
pub(crate) fn forwarded_address(headers: &HeaderMap) -> Option<IpAddr> {
    weavesignal_proto::last_forwarded(
        headers
            .get_all("x-forwarded-for")
            .iter()
            .map(|v| v.to_str().ok()),
    )
}

fn joined(state: &AppState, claims: &PassClaims, pass: String) -> Response {
    let Some(room) = &state.room else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Joining is not set up at this station yet.",
        );
    };
    // A caller joins their own booth (ADR-0193); anyone else the studio room.
    let room_name = weavesignal_proto::room_for(&room.config.name, claims.role, &claims.id);
    match room.keys.participant_with(
        &room_name,
        &claims.id,
        &claims.name,
        claims.role,
        room.config.token_ttl_s,
        true,
    ) {
        Ok(token) => (
            StatusCode::CREATED,
            Json(Joined {
                pass,
                token,
                url: room.config.public_url.clone(),
                room: room_name,
                identity: claims.id.clone(),
                role: claims.role,
                name: claims.name.clone(),
                hear: format!("{}-mm-{}", room.config.desk_identity, claims.id),
                // The programme picture is published in the studio room; a caller in their
                // booth hears the programme and is not shown it (ADR-0193).
                see: state
                    .hub
                    .desk_status()
                    .and_then(|d| d.video)
                    .filter(|_| claims.role != Role::Caller)
                    .map(|_| format!("{}-pgm", room.config.desk_identity)),
                pass_expires_at_ms: claims.exp * 1000,
            }),
        )
            .into_response(),
        Err(e) => problem(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/call-in",
    operation_id = "public_call_in",
    tag = "public",
    summary = "Call the studio",
    description = "A listener's call from the player page, with their consent to record (the words from `GET /api/v1/player`) and the answered challenge. Answers with a pass and a room token for the caller's booth. 409 (`field` `consent`) when the words have changed since the page loaded; 429 over the per-address limit.",
    request_body = CallInRequest,
    responses(
        (status = 201, description = "The pass and the token.", body = Joined),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, description = "Too many calls from this address just now.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The token could not be signed.", body = crate::openapi::Problem),
    ),
    security(()),
)]
pub async fn call_in(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(req): Json<CallInRequest>,
) -> Response {
    if let Some(why) = call_in_closed(&state) {
        return problem(StatusCode::SERVICE_UNAVAILABLE, why);
    }
    let Some(room) = &state.room else {
        unreachable!("call_in_closed checks the room");
    };
    let name = match clean_line(&req.name, NAME_MAX, "Your name") {
        Ok(n) if !n.is_empty() => n,
        Ok(_) => {
            return field_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "name",
                "Add your name, or the name you'd like said on air.",
            );
        }
        Err(e) => return field_problem(StatusCode::UNPROCESSABLE_ENTITY, "name", e),
    };
    let topic = match clean_line(&req.topic, TOPIC_MAX, "What you'd like to talk about") {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            return field_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "topic",
                "Say in a few words what you'd like to talk about.",
            );
        }
        Err(e) => return field_problem(StatusCode::UNPROCESSABLE_ENTITY, "topic", e),
    };
    let version = match check_consent(&state, req.consent.as_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    // The challenge before the rate limit, so requests without the work count against nobody.
    if state.join.challenge_bits > 0
        && let Err(why) = state
            .joins
            .challenges
            .redeem(req.challenge.as_ref(), now_ms())
    {
        tracing::info!(refused = %why, "call-in: challenge refused");
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "challenge",
            "The page's check before calling did not go through. Press Call the studio again.",
        );
    }
    let addr = peer(
        state.join.forwarded_for,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
    );
    let allowed = state.joins.limiter.lock().expect("joins lock").allow(
        addr,
        now_ms(),
        state.join.calls_per_address_per_hour,
        state.join.calls_per_minute,
    );
    if !allowed {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many calls from here just now. Wait a few minutes and try again.",
        );
    }
    let identity = mint_identity(Role::Caller);
    let (pass, claims) = match room.keys.pass(
        PassKind::Call,
        Role::Caller,
        &identity,
        &name,
        state.join.call_pass_ttl_s,
    ) {
        Ok(p) => p,
        Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let (text, _) = words(&state).await;
    if let Err(r) = keep_consent(
        &state,
        CallIn {
            identity: identity.clone(),
            name,
            topic,
            consent: Consent {
                at_ms: now_ms(),
                version,
                by: ConsentBy::Caller,
                text,
            },
        },
    )
    .await
    {
        return r;
    }
    tracing::info!(caller = %identity, "call-in: caller token issued");
    joined(&state, &claims, pass)
}

/// A challenge for the next call: harder while the station's minute is at least half used.
#[utoipa::path(
    get,
    path = "/api/v1/call-in/challenge",
    operation_id = "public_call_in_challenge",
    tag = "public",
    summary = "A challenge to answer before calling",
    description = "A proof-of-work challenge (ADR-0006); harder while the station is busy. Never cached.",
    responses(
        (status = 200, description = "The challenge.", body = crate::challenge::Challenge),
    ),
    security(()),
)]
pub async fn call_in_challenge(State(state): State<Arc<AppState>>) -> Response {
    let now = now_ms();
    let busy = state
        .joins
        .limiter
        .lock()
        .expect("joins lock")
        .station_load(now)
        * 2
        >= state.join.calls_per_minute as usize;
    let bits = if busy {
        state.join.challenge_bits_busy
    } else {
        state.join.challenge_bits
    };
    let c = state
        .joins
        .challenges
        .issue(bits, now, state.join.challenge_ttl_s * 1000);
    ([(header::CACHE_CONTROL, "no-store")], Json(c)).into_response()
}

// ---- passes ----

/// The pass in `Authorization: Bearer rsp1...`, checked; `Err` is the refusal.
fn pass_of(state: &AppState, headers: &HeaderMap) -> Result<(String, PassClaims), Response> {
    let Some(room) = &state.room else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Joining is not set up at this station yet.",
        ));
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    match room.keys.verify_pass(given) {
        Ok(c) => Ok((given.to_owned(), c)),
        Err(weavesignal_proto::TokenError::Expired) => Err(problem(
            StatusCode::GONE,
            "This link has expired. Ask the station for a new one.",
        )),
        Err(_) => Err(problem(
            StatusCode::UNAUTHORIZED,
            "This link is not valid. Check you copied all of it, or ask the station for a new one.",
        )),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PassInfo {
    pub kind: PassKind,
    pub role: Role,
    pub identity: String,
    pub name: String,
    pub station: String,
    pub show: String,
    pub expires_at_ms: u64,
    pub ended: bool,
    pub relay: RelayInfo,
}

#[utoipa::path(
    get,
    path = "/api/v1/pass",
    operation_id = "public_pass_info",
    tag = "join",
    summary = "What a pass is",
    description = "The pass is the bearer: from an invite's link or a call-in. Says who it is for and whether the studio has ended it.",
    responses(
        (status = 200, description = "The pass.", body = PassInfo),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 410, description = "This link has expired. Ask the station for a new one.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("pass" = [])),
)]
pub async fn pass_info(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, c) = match pass_of(&state, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    Json(PassInfo {
        kind: c.kind,
        role: c.role,
        identity: c.id.clone(),
        name: c.name.clone(),
        station: state.station.name.clone(),
        show: state.hub.view().show.title.clone(),
        expires_at_ms: c.exp * 1000,
        ended: state.joins.ended(&c.id, c.iat),
        relay: RelayInfo {
            turn_tls_port: state.turn_tls_port,
        },
    })
    .into_response()
}

/// A fresh room token for the pass's identity. A caller's needs their consent still on file:
/// in the studio, or, after a restart of the control plane, in the database while their call
/// has not ended (it is put back in the studio).
#[utoipa::path(
    post,
    path = "/api/v1/pass/token",
    operation_id = "public_pass_token",
    tag = "join",
    summary = "A room token for a pass",
    description = "Signs a token for the person's room: a caller's booth, or the studio. 410 when the studio has ended the pass or no longer has the call on file.",
    responses(
        (status = 201, description = "The pass and the token.", body = Joined),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 410, description = "This link has expired, or the studio has ended it.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The token could not be signed.", body = crate::openapi::Problem),
    ),
    security(("pass" = [])),
)]
pub async fn pass_token(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (pass, c) = match pass_of(&state, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    if state.joins.ended(&c.id, c.iat) {
        return problem(
            StatusCode::GONE,
            if c.role == Role::Caller {
                "This call has ended. To call again, start from the player page."
            } else {
                "The studio has ended this invite. Ask the station for a new link."
            },
        );
    }
    if c.role == Role::Caller
        && state.hub.read_studio(|s| s.call_in_of(&c.id).is_none())
        && !crate::retention::restore_consent(&state, &c.id).await
    {
        return problem(
            StatusCode::GONE,
            "The studio no longer has your call on file. To call again, start from the player page.",
        );
    }
    joined(&state, &c, pass)
}

/// One person's own state, as server-sent events: at once, then on each change.
#[utoipa::path(
    get,
    path = "/api/v1/pass/events",
    operation_id = "public_pass_events",
    tag = "join",
    summary = "A person's state, as events",
    description = "Server-sent events: a `me` event whose data is the person's state as JSON, at once and when it changes; a keep-alive every 10 seconds.",
    responses(
        (status = 200, description = "An event stream (`text/event-stream`).", body = String, content_type = "text/event-stream"),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 410, description = "This link has expired. Ask the station for a new one.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("pass" = [])),
)]
pub async fn pass_events(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, c) = match pass_of(&state, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let mut last = String::new();
    let stream = IntervalStream::new(tokio::time::interval(ME_TICK)).filter_map(move |_| {
        let ended = state.joins.ended(&c.id, c.iat);
        let me: MeState = me_state(
            &c.id,
            c.role,
            &state.hub.view(),
            state.hub.desk_status().as_ref(),
            ended,
        );
        let json = serde_json::to_string(&me).expect("the state serialises");
        if json == last {
            None
        } else {
            last = json.clone();
            Some(Ok::<_, Infallible>(Event::default().event("me").data(json)))
        }
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

/// A caller hangs up: out of the queue with no held place, out of the room, the pass ended.
/// Anyone else's page simply leaves the room.
#[utoipa::path(
    post,
    path = "/api/v1/pass/leave",
    operation_id = "public_pass_leave",
    tag = "join",
    summary = "Hang up",
    description = "A caller's call ends: out of the queue and the room, no held place. For anyone else it does nothing.",
    responses(
        (status = 204, description = "Done."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 410, description = "This link has expired. Ask the station for a new one.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("pass" = [])),
)]
pub async fn pass_leave(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, c) = match pass_of(&state, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    if c.role == Role::Caller {
        state.joins.end(&c.id);
        state.hub.with_studio(|s, now| s.forget(&c.id, now));
        crate::retention::call_ended(&state, &c.id).await;
        if let Some(room) = &state.hub.room
            && let Err(e) = room.remove(&c.id).await
        {
            tracing::warn!(caller = %c.id, error = %format!("{e:#}"), "cannot remove the caller who hung up");
        }
        tracing::info!(caller = %c.id, "caller hung up");
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---- invites (admin) ----

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteRequest {
    pub role: Role,
    #[serde(default)]
    pub name: String,
    /// Seconds; the station's `join.invite_ttl_s` when left out, and never more.
    #[serde(default)]
    pub ttl_s: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Invite {
    pub pass: String,
    pub identity: String,
    pub role: Role,
    pub name: String,
    pub expires_at_ms: u64,
    /// The join page's path with the pass in the fragment, which browsers never send.
    pub link: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/invites",
    operation_id = "public_invite",
    tag = "join",
    summary = "An invite link",
    description = "A link for a guest, a remote DJ or a presenter. A caller joins another way (422).",
    request_body = InviteRequest,
    responses(
        (status = 201, description = "The invite.", body = Invite),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
pub async fn invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<InviteRequest>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let Some(room) = &state.room else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the room is not configured: set WEAVESIGNAL_LIVEKIT_API_SECRET",
        );
    };
    if !matches!(req.role, Role::Guest | Role::Dj | Role::Presenter) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "an invite is for a guest, a remote DJ or a presenter; a {} joins another way",
                req.role
            ),
        );
    }
    let name = match clean_line(&req.name, 120, "a name") {
        Ok(n) => n,
        Err(e) => return problem(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    let ttl = req
        .ttl_s
        .unwrap_or(state.join.invite_ttl_s)
        .min(state.join.invite_ttl_s);
    let identity = mint_identity(req.role);
    match room
        .keys
        .pass(PassKind::Invite, req.role, &identity, &name, ttl)
    {
        Ok((pass, claims)) => {
            tracing::info!(identity = %identity, role = %req.role, ttl_s = ttl, "invite made");
            (
                StatusCode::CREATED,
                Json(Invite {
                    link: format!("/join/#{pass}"),
                    pass,
                    identity,
                    role: req.role,
                    name,
                    expires_at_ms: claims.exp * 1000,
                }),
            )
                .into_response()
        }
        Err(e) => problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

// ---- the description (ADR-0195) ----

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    player,
    hls_file,
    call_in,
    call_in_challenge,
    pass_info,
    pass_token,
    pass_events,
    pass_leave,
    invite
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behind_the_proxy_the_peer_is_the_forwarded_address_or_none() {
        let mut h = HeaderMap::new();
        let proxy: SocketAddr = "127.0.0.1:50123".parse().unwrap();
        let direct: SocketAddr = "192.0.2.7:50123".parse().unwrap();
        // The proxy's own address is never counted as a caller's.
        assert_eq!(peer(false, &h, Some(proxy)), None);
        assert_eq!(peer(false, &h, Some(direct)), Some(direct.ip()));
        h.insert("x-forwarded-for", "198.51.100.4".parse().unwrap());
        assert_eq!(
            peer(true, &h, Some(proxy)),
            Some("198.51.100.4".parse().unwrap())
        );
        assert_eq!(
            peer(false, &h, Some(proxy)),
            None,
            "the header is ignored unless trusted"
        );
        // A proxy on this machine forwarding a local client: still no address.
        h.insert("x-forwarded-for", "127.0.0.1".parse().unwrap());
        assert_eq!(peer(true, &h, Some(proxy)), None);
    }

    #[test]
    fn a_forged_forwarded_for_is_not_the_callers_address() {
        // The client sent "6.6.6.6, 7.7.7.7"; a proxy that appends added the address it saw.
        let proxy: SocketAddr = "127.0.0.1:50123".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            "6.6.6.6, 7.7.7.7,198.51.100.4".parse().unwrap(),
        );
        assert_eq!(
            peer(true, &h, Some(proxy)),
            Some("198.51.100.4".parse().unwrap())
        );
        // The client's own header line, then the proxy's: the proxy's last entry counts.
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", "6.6.6.6".parse().unwrap());
        h.append(
            "x-forwarded-for",
            "203.0.113.9, 2001:db8::5".parse().unwrap(),
        );
        assert_eq!(
            peer(true, &h, Some(proxy)),
            Some("2001:db8::5".parse().unwrap())
        );
        // A forged first entry varied per request changes nothing: one caller, one address.
        let a = |forged: &str| {
            let mut h = HeaderMap::new();
            h.insert(
                "x-forwarded-for",
                format!("{forged}, 198.51.100.4").parse().unwrap(),
            );
            peer(true, &h, Some(proxy))
        };
        assert_eq!(a("10.0.0.1"), a("10.0.0.2"));
        // Rubbish in the last entry is no address, not the forged one before it.
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "6.6.6.6, unknown".parse().unwrap());
        assert_eq!(peer(true, &h, Some(proxy)), None);
    }
}
