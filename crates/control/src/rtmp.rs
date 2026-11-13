// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out (ADR-0229, ADR-0230; `rtmp-out`): the destinations the desk sends the programme's
//! picture and sound to, kept in the database, edited by an admin, and handed to the desk.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/rtmp` | admin | every destination (never its key) with the desk's view of it, the encoders, and the platforms' forms |
//! | `POST /api/v1/rtmp/destinations` | admin | `name`, `platform`, `url`, `key`, `profile`, `enabled`: made |
//! | `PUT /api/v1/rtmp/destinations/{id}` | admin | any of them; a key left out is kept |
//! | `PUT /api/v1/rtmp/destinations/{id}/enabled` | admin | `{"enabled": true}` turns it on, `false` off |
//! | `DELETE /api/v1/rtmp/destinations/{id}` | admin | remove it |
//!
//! **The key.** A stream key is a secret: it is sealed (ChaCha20-Poly1305, with the
//! destination's id as associated data) under a key derived from the token key
//! ([`crate::apikey::ApiKey::rtmp_key`]), and the feature fails closed without one, as tokens
//! do. No route answers with it; the desk is sent it over its Unix socket, in a request of its
//! own ([`crate::desk::DeskClient::rtmp_set`]), and reports only the destination's name.
//!
//! **To the desk.** Every few seconds, and at once after a change, the control plane reads the
//! destinations, names the set by an HMAC of what it would send (keys included, under the same
//! derived key), and sends it when the desk's status names another set: a desk that started
//! again is sent its destinations within seconds. A desk without video takes none, and the
//! list says so.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use hmac::{Hmac, KeyInit, Mac};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use uuid::Uuid;
use weavesignal_model::rtmp::{self as model, NewRtmpDestination, RtmpChange, RtmpDestination};

use crate::apikey::ApiKey;
use crate::app::{AppState, problem};
use crate::desk::{DeskRtmpDestination, DeskRtmpEncoder};
use crate::staff::Need;

/// How many destinations a station may have.
pub const MAX_DESTINATIONS: usize = 10;
/// How often the desk's set is compared with the database's.
const SYNC_EVERY: Duration = Duration::from_secs(3);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/rtmp", get(list))
        .route("/api/v1/rtmp/destinations", post(create))
        .route("/api/v1/rtmp/destinations/{id}", put(change).delete(remove))
        .route("/api/v1/rtmp/destinations/{id}/enabled", put(turn))
}

// ---- the key ----

const AAD_LABEL: &[u8] = b"weavesignal rtmp stream key ";

fn aead(key: &ApiKey) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &key.rtmp_key()).expect("a 32-byte key"))
}

fn aad(id: Uuid) -> Vec<u8> {
    let mut v = AAD_LABEL.to_vec();
    v.extend_from_slice(id.as_bytes());
    v
}

/// Seal a stream key for destination `id`: its nonce and its sealed form.
pub fn seal(key: &ApiKey, id: Uuid, stream_key: &str) -> ([u8; 12], Vec<u8>) {
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .expect("the system's random source");
    let mut data = stream_key.as_bytes().to_vec();
    aead(key)
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(id)),
            &mut data,
        )
        .expect("sealing a short key");
    (nonce, data)
}

/// Open a sealed stream key; `None` when it was sealed under another key or for another
/// destination (the token key changed: an admin enters the key again).
pub fn open(key: &ApiKey, id: Uuid, nonce: &[u8], sealed: &[u8]) -> Option<String> {
    let nonce: [u8; 12] = nonce.try_into().ok()?;
    let mut data = sealed.to_vec();
    let plain = aead(key)
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(id)),
            &mut data,
        )
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

// ---- the platforms ----

/// A platform's form, for the studio app: where its server URL and key are found.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Platform {
    pub platform: &'static str,
    pub name: &'static str,
    /// The usual server URL (the platform's own page is the one to trust).
    pub url: &'static str,
    pub profile: &'static str,
    /// Where the platform shows its server URL and stream key.
    pub where_to_find: &'static str,
}

// What each platform takes was checked on 2026-11-11 (research recce 30 and the 1.0.1 test run);
// re-check before each release. "compatible" is H.264 and AAC, which every platform here takes;
// "open" is AV1 and Opus over Enhanced RTMP, which only some take, and the hint says so.
/// Gab is not offered: its live streaming is not open to streamers yet (the test station,
/// 2026-11-12: Go Live Now answered 502; Gab says it is working on it). The database still allows
/// `gab` (migration 0035), so it can come back as a form when Gab opens it.
pub const PLATFORMS: [Platform; 7] = [
    Platform {
        platform: "youtube",
        name: "YouTube",
        url: "rtmps://a.rtmps.youtube.com:443/live2",
        profile: "compatible",
        where_to_find: "YouTube Studio: Create, Go live, Stream: the Stream URL and the Stream key. YouTube takes AV1 video over Enhanced RTMP, but its RTMP ingest is reported to take AAC sound only, not Opus, so Open (AV1 and Opus) is likely refused: keep Compatible until there is an AV1-and-AAC profile. YouTube re-encodes every stream; 1440p gets viewers its better VP9 or AV1 copies.",
    },
    Platform {
        platform: "owncast",
        name: "Owncast",
        url: "rtmp://owncast.example:1935/live",
        profile: "compatible",
        where_to_find: "Owncast's admin: Server Setup, Stream Keys, for the key; the server is your Owncast's address with its RTMP port (on Cloudron, the app's RTMP port). Owncast takes H.264 and AAC only: keep Compatible (it refuses Open).",
    },
    Platform {
        platform: "x",
        name: "X",
        url: "",
        profile: "compatible",
        where_to_find: "X (Premium needed): Creator Studio, Live Studio (once Media Studio Producer), New Livestream, an RTMP source in your region: its RTMPS URL and stream key. Then start the broadcast on X. X takes H.264 and AAC: keep Compatible.",
    },
    Platform {
        platform: "rumble",
        name: "Rumble",
        url: "rtmp://rtmp.rumble.com/live",
        profile: "compatible",
        where_to_find: "Rumble: Go Live, then Get the Streamer Configuration: the Server URL and the Stream Key. Keep Compatible.",
    },
    Platform {
        platform: "odysee",
        name: "Odysee",
        url: "rtmp://publish.odysee.live/live",
        profile: "compatible",
        where_to_find: "Odysee: Go Live, your livestream, RTMP Setup: the Server URL and the Stream Key. Keep Compatible.",
    },
    Platform {
        platform: "peertube",
        name: "PeerTube",
        url: "rtmp://your-instance.example:1935/live",
        profile: "compatible",
        where_to_find: "PeerTube: Publish, Go live, then the live's page: the Live RTMP Url (or RTMPS Url, port 1936) and the Live stream key. Keep Compatible unless your instance says it takes AV1.",
    },
    Platform {
        platform: "custom",
        name: "Another RTMP server",
        url: "rtmp://server.example/live",
        profile: "compatible",
        where_to_find: "Your server's documentation: an rtmp:// or rtmps:// URL and a stream key. Choose Open (AV1 and Opus) only if it says it takes Enhanced RTMP.",
    },
];

// ---- checks ----

fn field(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": detail.into(), "field": field })),
    )
        .into_response()
}

/// A name the desk can show and log: letters, digits, spaces and `-_.`, 1 to 40 of them.
pub fn check_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty()
        || n.chars().count() > 40
        || !n.chars().all(|c| c.is_alphanumeric() || " -_.".contains(c))
    {
        return Err(
            "Give it a name of 1 to 40 letters, digits, spaces, dashes, dots or underscores."
                .into(),
        );
    }
    Ok(n.to_owned())
}

/// An `rtmp://` or `rtmps://` server URL with a host, without the key.
pub fn check_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let rest = u
        .strip_prefix("rtmp://")
        .or_else(|| u.strip_prefix("rtmps://"))
        .ok_or("The server URL starts with rtmp:// or rtmps://.")?;
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') {
        return Err("The server URL has no host.".into());
    }
    // The table refuses fewer than 9 characters (migration 0024): say so here, not as a 500.
    if u.chars().count() < 9 {
        return Err("The server URL is too short to be a server's address.".into());
    }
    if u.len() > 500
        || u.chars().any(|c| c.is_whitespace() || c.is_control())
        || u.contains(['?', '#', '@'])
    {
        return Err("The server URL is the platform's address only, without spaces, a query or a password; the key goes in its own field.".into());
    }
    Ok(u.to_owned())
}

/// A stream key: printable, no spaces, at most 500 characters.
pub fn check_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() || k.len() > 500 || k.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The stream key is the platform's key as it shows it, without spaces.".into());
    }
    Ok(k.to_owned())
}

fn check_platform(p: &str) -> Result<String, String> {
    if PLATFORMS.iter().any(|x| x.platform == p) {
        Ok(p.to_owned())
    } else {
        let names: Vec<&str> = PLATFORMS.iter().map(|x| x.platform).collect();
        Err(format!("The platform is one of {}.", names.join(", ")))
    }
}

fn check_profile(p: &str) -> Result<String, String> {
    match p {
        "compatible" | "open" => Ok(p.to_owned()),
        _ => Err(
            "The profile is compatible (H.264 and AAC) or open (AV1 and Opus over Enhanced RTMP)."
                .into(),
        ),
    }
}

// ---- routes ----

fn db_problem(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "rtmp: database error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "the database refused that",
    )
}

/// The admin, the database and the key, or the refusal.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn ready<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<(String, &'a sqlx::PgPool, &'a ApiKey), Response> {
    let actor = state.check(headers, Need::Admin)?;
    let Some(pool) = state.db.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "RTMP destinations are kept in the database",
        ));
    };
    let Some(key) = state.staff.api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "RTMP stream keys are sealed under a key: set {} (or the station key)",
                ApiKey::ENV
            ),
        ));
    };
    Ok((actor.label(), pool, key))
}

/// A destination as the studio shows it: never its key.
#[derive(Serialize, utoipa::ToSchema)]
struct Shown {
    #[serde(flatten)]
    destination: RtmpDestination,
    /// The key can be read with the station's key; `false` after the token key changed (enter
    /// it again).
    key_readable: bool,
    /// The desk's view of it; `None` when the desk has not got it yet (or has no video).
    desk: Option<DeskRtmpDestination>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Listed {
    destinations: Vec<Shown>,
    /// The desk runs video, which RTMP out needs.
    video: bool,
    /// The encoders running at the desk.
    encoders: Vec<DeskRtmpEncoder>,
    platforms: Vec<Platform>,
}

fn shown(state: &AppState, key: &ApiKey, d: RtmpDestination) -> Shown {
    let desk = state
        .hub
        .desk_status()
        .and_then(|s| s.video)
        .and_then(|v| v.rtmp)
        .and_then(|r| r.destinations.into_iter().find(|x| x.name == d.name));
    Shown {
        key_readable: open(key, d.id, &d.key_nonce, &d.key_sealed).is_some(),
        desk,
        destination: d,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/rtmp",
    operation_id = "rtmp_list",
    tag = "rtmp",
    summary = "RTMP out's destinations",
    description = "Admin only. Every destination (never its stream key) with the desk's view of it (`desk`: its state, the last error with the key cut out, the bytes sent), whether the desk runs video (RTMP out needs it), the encoders running, and each platform's form.",
    responses(
        (status = 200, description = "The destinations.", body = Listed),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let video = state.hub.desk_status().and_then(|s| s.video);
    match model::list(pool, state.station.id).await {
        Ok(all) => Json(Listed {
            destinations: all.into_iter().map(|d| shown(&state, key, d)).collect(),
            video: video.is_some(),
            encoders: video
                .and_then(|v| v.rtmp)
                .map(|r| r.encoders)
                .unwrap_or_default(),
            platforms: PLATFORMS.to_vec(),
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct RtmpNewBody {
    name: String,
    /// `youtube`, `owncast`, `x`, `rumble`, `odysee`, `peertube` or `custom`.
    platform: String,
    /// The server URL, `rtmp://` or `rtmps://`, without the key.
    url: String,
    /// The stream key; never shown again.
    #[schema(write_only)]
    key: String,
    /// `compatible` (the default: H.264 and AAC) or `open` (AV1 and Opus).
    #[serde(default)]
    profile: Option<String>,
    /// Off unless asked.
    #[serde(default)]
    enabled: bool,
}

type NewFields = (String, String, String, String, String);
type ChangeFields = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn check_new(req: &RtmpNewBody) -> Result<NewFields, Response> {
    Ok((
        check_name(&req.name).map_err(|e| field("name", e))?,
        check_platform(&req.platform).map_err(|e| field("platform", e))?,
        check_url(&req.url).map_err(|e| field("url", e))?,
        check_key(&req.key).map_err(|e| field("key", e))?,
        check_profile(req.profile.as_deref().unwrap_or("compatible"))
            .map_err(|e| field("profile", e))?,
    ))
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn check_change(req: &RtmpChangeBody) -> Result<ChangeFields, Response> {
    let opt = |v: &Option<String>, f: fn(&str) -> Result<String, String>, name: &str| {
        v.as_deref().map(f).transpose().map_err(|e| field(name, e))
    };
    Ok((
        opt(&req.name, check_name, "name")?,
        opt(&req.platform, check_platform, "platform")?,
        opt(&req.url, check_url, "url")?,
        opt(&req.key, check_key, "key")?,
        opt(&req.profile, check_profile, "profile")?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/rtmp/destinations",
    operation_id = "rtmp_create",
    tag = "rtmp",
    summary = "Add an RTMP destination",
    description = "Admin only. `name`, `platform`, `url` (the server URL, `rtmp://` or `rtmps://`), `key` (the stream key, sealed in the database and never shown again), `profile` (`compatible`, the default, or `open`) and `enabled` (off unless asked). At most 10 (409); a name in use is 409 too.",
    request_body = RtmpNewBody,
    responses(
        (status = 201, description = "The destination, without its key.", body = weavesignal_model::rtmp::RtmpDestination),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RtmpNewBody>,
) -> Response {
    let (by, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let checked = check_new(&req);
    let (name, platform, url, stream_key, profile) = match checked {
        Ok(c) => c,
        Err(r) => return r,
    };
    match model::list(pool, state.station.id).await {
        Ok(all) if all.len() >= MAX_DESTINATIONS => {
            return problem(
                StatusCode::CONFLICT,
                format!(
                    "A station may have {MAX_DESTINATIONS} RTMP destinations. Remove one first."
                ),
            );
        }
        Ok(_) => {}
        Err(e) => return db_problem(e),
    }
    let id = Uuid::new_v4();
    let (nonce, sealed) = seal(key, id, &stream_key);
    let new = NewRtmpDestination {
        id,
        name: &name,
        platform: &platform,
        url: &url,
        key_nonce: &nonce,
        key_sealed: &sealed,
        profile: &profile,
        enabled: req.enabled,
        created_by: &by,
    };
    match model::create(pool, state.station.id, &new).await {
        Ok(Some(d)) => {
            tracing::info!(destination = %d.name, platform = %d.platform, by, "rtmp: destination added");
            sync_soon(&state);
            (StatusCode::CREATED, Json(d)).into_response()
        }
        Ok(None) => problem(
            StatusCode::CONFLICT,
            format!("A destination is already called `{name}`."),
        ),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct RtmpChangeBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    url: Option<String>,
    /// A new stream key; left out, the old one is kept.
    #[serde(default)]
    #[schema(write_only)]
    key: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[utoipa::path(
    put,
    path = "/api/v1/rtmp/destinations/{id}",
    operation_id = "rtmp_change",
    tag = "rtmp",
    summary = "Change an RTMP destination",
    description = "Admin only. Any of `name`, `platform`, `url`, `key`, `profile` and `enabled`; a field left out is kept (the key too). A destination whose URL, key or profile changes connects again.",
    params(("id" = Uuid, Path, description = "The destination's id.")),
    request_body = RtmpChangeBody,
    responses(
        (status = 200, description = "The destination as it is now, without its key.", body = weavesignal_model::rtmp::RtmpDestination),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn change(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<RtmpChangeBody>,
) -> Response {
    let (by, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let checked = check_change(&req);
    let (name, platform, url, stream_key, profile) = match checked {
        Ok(c) => c,
        Err(r) => return r,
    };
    let sealed = stream_key.as_deref().map(|k| seal(key, id, k));
    let c = RtmpChange {
        name: name.as_deref(),
        platform: platform.as_deref(),
        url: url.as_deref(),
        key: sealed.as_ref().map(|(n, s)| (&n[..], &s[..])),
        profile: profile.as_deref(),
        enabled: req.enabled,
    };
    match model::update(pool, state.station.id, id, &c).await {
        Ok(Some(d)) => {
            tracing::info!(destination = %d.name, key_changed = sealed.is_some(), by, "rtmp: destination changed");
            sync_soon(&state);
            Json(d).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such destination"),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            problem(StatusCode::CONFLICT, "Another destination has that name.")
        }
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct RtmpTurnBody {
    enabled: bool,
}

#[utoipa::path(
    put,
    path = "/api/v1/rtmp/destinations/{id}/enabled",
    operation_id = "rtmp_turn",
    tag = "rtmp",
    summary = "Turn an RTMP destination on or off",
    description = "Admin only. `{\"enabled\": true}` starts sending the programme there (behind the delay; the desk needs video on), `false` stops it. The encode for a profile runs only while one of its destinations is on.",
    params(("id" = Uuid, Path, description = "The destination's id.")),
    request_body = RtmpTurnBody,
    responses(
        (status = 200, description = "The destination as it is now, without its key.", body = weavesignal_model::rtmp::RtmpDestination),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn turn(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<RtmpTurnBody>,
) -> Response {
    let (by, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let c = RtmpChange {
        enabled: Some(req.enabled),
        ..RtmpChange::default()
    };
    match model::update(pool, state.station.id, id, &c).await {
        Ok(Some(d)) => {
            tracing::info!(destination = %d.name, on = req.enabled, by, "rtmp: destination turned");
            sync_soon(&state);
            Json(d).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such destination"),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/rtmp/destinations/{id}",
    operation_id = "rtmp_remove",
    tag = "rtmp",
    summary = "Remove an RTMP destination",
    description = "Admin only. The desk stops sending there, and its key is gone.",
    params(("id" = Uuid, Path, description = "The destination's id.")),
    responses(
        (status = 204, description = "Removed."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let (by, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match model::delete(pool, state.station.id, id).await {
        Ok(true) => {
            tracing::info!(destination = %id, by, "rtmp: destination removed");
            sync_soon(&state);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such destination"),
        Err(e) => db_problem(e),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(list, create, change, turn, remove))]
pub(crate) struct Api;

// ---- to the desk ----

/// What the desk is sent: each destination with its key opened. A key that cannot be opened
/// (the token key changed) leaves its destination out, and says so.
fn desk_request(key: &ApiKey, all: &[RtmpDestination]) -> (String, serde_json::Value) {
    let mut list = Vec::new();
    for d in all {
        // Left out; `sync` says so when it sends the set, not on every comparison.
        let Some(k) = open(key, d.id, &d.key_nonce, &d.key_sealed) else {
            continue;
        };
        list.push(json!({
            "name": d.name,
            "url": d.url,
            "key": k,
            "profile": d.profile,
            "enabled": d.enabled,
        }));
    }
    let body = serde_json::to_vec(&list).expect("JSON");
    let mut m = Hmac::<Sha256>::new_from_slice(&key.rtmp_key()).expect("HMAC takes any key");
    m.update(b"weavesignal rtmp set ");
    m.update(&body);
    let set: String = m.finalize().into_bytes()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (
        set.clone(),
        json!({"op": "rtmp", "set": set, "destinations": list}),
    )
}

/// Send the desk its destinations if the set it holds is not the database's. Nothing when the
/// desk has no video, no database or no key.
pub async fn sync(state: &AppState) {
    let (Some(pool), Some(key)) = (state.db.as_ref(), state.staff.api.key()) else {
        return;
    };
    let Some(video) = state.hub.desk_status().and_then(|s| s.video) else {
        return;
    };
    let all = match model::list(pool, state.station.id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "rtmp: cannot read the destinations");
            return;
        }
    };
    let (set, request) = desk_request(key, &all);
    let has = video.rtmp.as_ref().and_then(|r| r.set.clone());
    if has.as_deref() == Some(set.as_str()) {
        return;
    }
    // A desk that has never been sent a set and has nothing to send needs nothing.
    if has.is_none() && all.is_empty() && video.rtmp.is_none() {
        return;
    }
    match state.hub.desk.rtmp_set(request).await {
        Ok(()) => {
            // Once per set sent, not every few seconds: the comparison above runs all day.
            for d in &all {
                if open(key, d.id, &d.key_nonce, &d.key_sealed).is_none() {
                    tracing::warn!(destination = %d.name, "rtmp: its stream key cannot be read (the token key changed?); enter it again");
                }
            }
            tracing::info!(
                set,
                destinations = all.len(),
                "rtmp: the desk has its destinations"
            );
            state.hub.poll_once().await;
        }
        Err(e) => {
            tracing::warn!(error = %e, "rtmp: the desk did not take its destinations; trying again shortly")
        }
    }
}

fn sync_soon(state: &Arc<AppState>) {
    let s = state.clone();
    tokio::spawn(async move {
        sync(&s).await;
    });
}

/// Keep the desk's destinations the database's, every few seconds.
pub fn spawn_sync(state: Arc<AppState>) {
    if state.db.is_none() || !state.hub.desk.configured() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SYNC_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            sync(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ApiKey {
        ApiKey::parse("0123456789abcdef0123456789abcdef").unwrap()
    }

    #[test]
    fn a_sealed_key_opens_only_for_its_destination_and_under_its_key() {
        let id = Uuid::new_v4();
        let (n, s) = seal(&key(), id, "live_abc123?d=1&s=2");
        assert!(
            !s.windows(6).any(|w| w == b"abc123"),
            "the key is in the clear"
        );
        assert_eq!(
            open(&key(), id, &n, &s).as_deref(),
            Some("live_abc123?d=1&s=2")
        );
        assert_eq!(open(&key(), Uuid::new_v4(), &n, &s), None);
        let other = ApiKey::parse("ffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(open(&other, id, &n, &s), None);
        let (n2, _) = seal(&key(), id, "live_abc123?d=1&s=2");
        assert_ne!(n, n2, "a fresh nonce each time");
    }

    #[test]
    fn urls_keys_and_names_are_checked() {
        assert_eq!(
            check_url(" rtmp://rtmp.rumble.com/live/ ").unwrap(),
            "rtmp://rtmp.rumble.com/live"
        );
        assert!(check_url("rtmps://tube.example:1936/live").is_ok());
        for bad in [
            "http://x/live",
            "rtmp://",
            "rtmp://:1935/live",
            "rtmp://a/live?key=1",
            "rtmp://u:p@a/live",
            "rtmp://a b/live",
            "rtmp://a",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        assert!(check_key("abc?d=1&s=2&t=3").is_ok());
        assert!(check_key("a b").is_err() && check_key("").is_err());
        assert_eq!(check_name(" Rumble main ").unwrap(), "Rumble main");
        assert!(check_name("a/b").is_err() && check_name("").is_err());
        assert!(check_profile("open").is_ok() && check_profile("h264").is_err());
        assert!(check_platform("peertube").is_ok() && check_platform("twitch").is_err());
    }

    #[test]
    fn every_platform_defaults_to_compatible_and_its_hint_names_the_profile() {
        assert!(PLATFORMS.iter().any(|p| p.platform == "youtube"));
        for p in PLATFORMS {
            assert_eq!(p.profile, "compatible", "{}", p.platform);
            assert!(
                p.where_to_find.contains("Compatible") || p.where_to_find.contains("Open"),
                "{} says which profile to use",
                p.platform
            );
        }
        let yt = PLATFORMS.iter().find(|p| p.platform == "youtube").unwrap();
        assert!(yt.where_to_find.contains("Open") && check_url(yt.url).is_ok());
    }

    #[test]
    fn the_set_names_what_is_sent_keys_included_and_changes_with_it() {
        let k = key();
        let id = Uuid::new_v4();
        let d = |enabled: bool, sk: &str| {
            let (n, s) = seal(&k, id, sk);
            RtmpDestination {
                id,
                name: "rumble".into(),
                platform: "rumble".into(),
                url: "rtmp://rtmp.rumble.com/live".into(),
                profile: "compatible".into(),
                enabled,
                created_by: "t".into(),
                created_ms: 0,
                updated_ms: 0,
                key_nonce: n.to_vec(),
                key_sealed: s,
            }
        };
        let (a, req) = desk_request(&k, &[d(true, "k1")]);
        let (b, _) = desk_request(&k, &[d(true, "k1")]);
        assert_eq!(
            a, b,
            "the same destinations, the same set (whatever the nonce)"
        );
        assert_ne!(a, desk_request(&k, &[d(false, "k1")]).0);
        assert_ne!(a, desk_request(&k, &[d(true, "k2")]).0);
        assert!(!a.contains("k1"));
        assert_eq!(req["op"], "rtmp");
        assert_eq!(req["destinations"][0]["key"], "k1");
        // Under another key nothing opens, and nothing is sent.
        let other = ApiKey::parse("ffffffffffffffffffffffffffffffff").unwrap();
        let (_, req) = desk_request(&other, &[d(true, "k1")]);
        assert_eq!(req["destinations"].as_array().unwrap().len(), 0);
    }
}
