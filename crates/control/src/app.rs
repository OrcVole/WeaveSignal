// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The HTTP routes.
//!
//! The studio app's routes (a staff member's session, or the station key; ADR-0127, and
//! [`crate::staff`] for signing in):
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/studio` | the studio's view: sources in their slots, callers, chat modes, notices |
//! | `GET /api/v1/studio/events` | the same view as server-sent events, one `state` event per change |
//! | `POST /api/v1/desk/commands` | a desk switch: `on_air`, `mute`, `cue`, `talk`, `talk_to`; or `dump` the profanity delay |
//! | `POST /api/v1/screener/commands` | `screen`, `send`, `back`, `decline`, `notes`, `release`, `drop` |
//! | `POST /api/v1/passes/end` | end anyone's pass: off air, out of the room, no new tokens, across a restart (ADR-0119) |
//! | `POST /api/v1/chat/modes` | a chat mode (`slow`, `hold_new`, `links_trusted`) or influx, with its undo |
//! | `GET /api/v1/carts` | the banks and carts, each with FILE MISSING; fetched again when the view's `carts.version` moves |
//! | `POST /api/v1/carts/commands` | `toggle`, `fire`, `stop`, `cue` a cart; `stop_last`; `fade_all`; `clear_cue` (carts sound in the desk, ADR-0117) |
//! | `PUT /api/v1/carts/banks/{n}` | make or rename a bank: `name`, `colour_slot` |
//! | `PUT /api/v1/carts/banks/{bank}/slots/{slot}?label=&mode=` | an audio file as the body: the media worker normalises it into that slot |
//! | `DELETE /api/v1/carts/banks/{bank}/slots/{slot}` | empty a slot |
//!
//! | `POST /api/v1/invites` | an invite link for a guest, a remote DJ or a presenter (`public.rs`) |
//! | `/api/v1/retention/...` | the Jurisdiction setting, holds, the sweep, recordings and consent links ([`crate::retention`]) |
//!
//! Each command answers with the new view, so the app shows the change without waiting.
//!
//! The public routes for the player and join pages (no bearer; a pass where one is named) are
//! in [`crate::public`].

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;
use tower_http::trace::TraceLayer;
use weavesignal_model::Station;
use weavesignal_proto::{Keys, Role, check_identity};

use crate::carts::{CartCommand, CartMeta, CartProblem};
use crate::config::{JoinConfig, RoomConfig};
use crate::desk::{DeskCommand, DeskError};
use crate::hub::Hub;
use crate::studio::{Mode, StudioError};

pub struct AppState {
    pub station: Station,
    pub db: Option<sqlx::PgPool>,
    /// The room's settings and LiveKit's keys; `None` when the secret is not set.
    pub room: Option<RoomState>,
    /// The station key: the station's own credential, an admin's powers; with `None` only
    /// staff sessions open the studio's routes.
    pub admin_token: Option<String>,
    /// Staff sessions and sign-in failures, in memory (ADR-0127).
    pub staff: Arc<crate::staff::Staff>,
    /// The studio: the desk link, the screener's queue, the chat's modes.
    pub hub: Arc<Hub>,
    /// How people join from a browser: the player's stream, call-in, consent, invites.
    pub join: JoinConfig,
    /// The slate's still: where it is kept and how big it may be (ADR-0010).
    pub video: crate::config::VideoConfig,
    /// Call-in's rate limit and the passes the studio has ended.
    pub joins: crate::public::Joins,
    /// TURN over TLS's port, for the join pages' relay option (ADR-0080).
    pub turn_tls_port: u16,
    /// Content steering (ADR-0051): while it has edges, the multivariant playlists served at
    /// `/hls/` name its route (ADR-0159).
    pub steering: crate::steering::SteeringConfig,
}

pub struct RoomState {
    pub config: RoomConfig,
    pub keys: Keys,
}

pub fn router(state: Arc<AppState>) -> Router {
    let upload_limit =
        usize::try_from(state.hub.carts.config.max_upload_mb * 1024 * 1024).unwrap_or(usize::MAX);
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/station", get(station))
        .route("/api/v1/openapi.json", get(crate::openapi::openapi_json))
        .route("/api/v1/room/tokens", post(room_token))
        .route("/api/v1/studio", get(studio_view))
        .route("/api/v1/studio/events", get(studio_events))
        .route("/api/v1/desk/commands", post(desk_command))
        .route("/api/v1/screener/commands", post(screener_command))
        .route("/api/v1/passes/end", post(end_pass))
        .route("/api/v1/chat/modes", post(chat_modes))
        .route("/api/v1/invites", post(crate::public::invite))
        .route("/api/v1/player", get(crate::public::player))
        .route("/hls/{*path}", get(crate::public::hls_file))
        .route("/api/v1/call-in", post(crate::public::call_in))
        .route(
            "/api/v1/call-in/challenge",
            get(crate::public::call_in_challenge),
        )
        .route("/api/v1/pass", get(crate::public::pass_info))
        .route("/api/v1/pass/token", post(crate::public::pass_token))
        .route("/api/v1/pass/events", get(crate::public::pass_events))
        .route("/api/v1/pass/leave", post(crate::public::pass_leave))
        // The cart wall (ADR-0020).
        .route("/api/v1/carts", get(carts_catalogue))
        .route("/api/v1/carts/commands", post(cart_command))
        .route("/api/v1/carts/banks/{number}", put(put_bank))
        .route(
            "/api/v1/carts/banks/{bank}/slots/{slot}",
            put(import_cart)
                .delete(delete_cart)
                .layer(DefaultBodyLimit::max(upload_limit))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    admin_first,
                )),
        )
        // The video slate's still and the cameras' thumbnails (M5, ADR-0010).
        .merge(crate::video::routes(&state))
        // Playout, the schedule, the logs and now playing (M4).
        .merge(crate::onair_api::routes(&state))
        // Retention: the Jurisdiction setting, holds, the sweeper (ADRs 0113-0116).
        .merge(crate::retention::routes())
        // Staff accounts and sign-in (ADR-0127).
        .merge(crate::staff::routes())
        // The public API (ADR-0195): API tokens, webhooks and the open station facts.
        .merge(crate::tokens::routes())
        .merge(crate::webhooks::routes())
        .merge(crate::open::station_routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// A problem, as JSON with a plain sentence.
pub(crate) fn problem(code: StatusCode, detail: impl Into<String>) -> Response {
    #[derive(Serialize, utoipa::ToSchema)]
    struct Problem {
        error: String,
    }
    (
        code,
        Json(Problem {
            error: detail.into(),
        }),
    )
        .into_response()
}

/// Compare in time that does not depend on where the inputs differ.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The admin check as middleware, for routes whose extractor reads a large body: a handler's
/// own check runs only after `Bytes` has buffered the whole upload (64 or 200 MB), so a
/// stranger could fill memory before being refused (review 0025, P1). The handler still
/// checks too.
pub(crate) async fn admin_first(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if let Some(r) = refusal(&state, req.headers()) {
        return r;
    }
    next.run(req).await
}

impl AppState {
    /// Who the request comes from, if they may do what the route needs (ADR-0127).
    #[allow(clippy::result_large_err)] // the refusal is the response itself
    pub fn check(
        &self,
        headers: &HeaderMap,
        need: crate::staff::Need,
    ) -> Result<crate::staff::Actor, Response> {
        crate::staff::check(self.admin_token.as_deref(), &self.staff, headers, need)
    }
}

/// `None` when the request comes from a member of staff or the station key; otherwise the
/// refusal to send. Running the studio needs no more.
pub(crate) fn refusal(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    state.check(headers, crate::staff::Need::Staff).err()
}

/// As [`refusal`], for what only an admin may do: retention, erasure, personal data, the staff
/// list, playout's settings, monitors that hear every return.
pub(crate) fn admin_refusal(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    state.check(headers, crate::staff::Need::Admin).err()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenRequest {
    /// The person's identity in the room (rules in the reference for roles).
    pub identity: String,
    /// One of the seven roles.
    pub role: Role,
    /// The name others see: at most 120 characters, no control characters.
    #[serde(default)]
    pub name: String,
    /// Seconds; the station's `room.token_ttl_s` when left out, and never more.
    #[serde(default)]
    pub ttl_s: Option<u64>,
    /// Whether the token may subscribe (default yes). A native GStreamer publisher (the
    /// studio app's native path) asks for a publish-only token and listens on a second one.
    #[serde(default = "yes")]
    pub subscribe: bool,
    /// A caller's token needs their consent to record, given on their behalf by the station
    /// (the player page's call-in route records the caller's own).
    #[serde(default)]
    pub consent: Option<crate::public::ConsentGiven>,
    /// A listener (a monitor) for one caller's booth, by the caller's identity (ADR-0193);
    /// left out, a monitor joins the studio room.
    #[serde(default)]
    pub booth: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    /// The LiveKit access token.
    pub token: String,
    /// Where browsers reach LiveKit.
    pub url: String,
    /// The room it is for: a caller's booth for a caller, the named booth for a monitor that
    /// asked for one, the studio room for anyone else.
    pub room: String,
    pub identity: String,
    pub role: Role,
    /// Seconds the token can be used to join.
    pub ttl_s: u64,
}

/// A short-lived room token for one person (a presenter, a remote DJ, a guest...), with their
/// role in it for the desk to read. Any member of staff; a `listener` token, an admin only.
#[utoipa::path(
    post,
    path = "/api/v1/room/tokens",
    operation_id = "room_token",
    tag = "room",
    summary = "A room token for one person",
    description = "A short-lived LiveKit token for one person, with their role in it for the desk to read. A `listener` token is a monitor that hears every return (ADR-0118): only an admin may ask for one (403 otherwise). A `caller` token records the caller's consent on the station's behalf, against the words the station shows now (`consent`).",
    request_body = TokenRequest,
    responses(
        (status = 201, description = "The token.", body = TokenResponse),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn room_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TokenRequest>,
) -> Response {
    // A listener's token is a monitor: LiveKit's recorder grant hears every return past the
    // room's limits (ADR-0118), so only an admin issues one.
    let refused = if req.role == Role::Listener {
        admin_refusal(&state, &headers)
    } else {
        refusal(&state, &headers)
    };
    if let Some(r) = refused {
        return r;
    }
    let Some(room) = &state.room else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "the room is not configured: set {}",
                crate::config::LIVEKIT_SECRET_ENV
            ),
        );
    };
    if let Err(e) = check_identity(&req.identity, &room.config.desk_identity) {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string());
    }
    if let Some(caller) = &req.booth
        && let Err(e) = check_identity(caller, &room.config.desk_identity)
    {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string());
    }
    if !req.role.may_publish() && !req.subscribe {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a listener's token only listens: it cannot be publish-only",
        );
    }
    if req.name.chars().count() > 120 || req.name.chars().any(char::is_control) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a name is at most 120 characters, without control characters",
        );
    }
    if req.role == Role::Caller {
        // No caller reaches the desk without consent to record: an admin's caller token
        // records it on the caller's behalf, against the words the station shows now.
        if let Err(r) = crate::public::record_station_consent(&state, &req).await {
            return r;
        }
    }
    let ttl_s = req
        .ttl_s
        .unwrap_or(room.config.token_ttl_s)
        .min(room.config.token_ttl_s);
    // A sender asks for a publish-only token (ADR-0090); the grant is the request's. A
    // listener is a monitor: it hears every track past the pages' limits (ADR-0118), which
    // only this admin route gives, never a pass.
    // A caller joins their own booth (ADR-0193); a monitor the booth it names, or the studio.
    let room_name = match (&req.booth, req.role) {
        (Some(caller), Role::Listener) => weavesignal_proto::booth(&room.config.name, caller),
        (Some(_), _) => {
            return problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "only a listener's token names a booth: a caller's token is for their own",
            );
        }
        (None, role) => weavesignal_proto::room_for(&room.config.name, role, &req.identity),
    };
    let token = if req.role == Role::Listener {
        room.keys
            .monitor(&room_name, &req.identity, &req.name, ttl_s)
    } else {
        room.keys.participant_with(
            &room_name,
            &req.identity,
            &req.name,
            req.role,
            ttl_s,
            req.subscribe,
        )
    };
    match token {
        Ok(token) => {
            tracing::info!(identity = %req.identity, role = %req.role, ttl_s, "room token issued");
            (
                StatusCode::CREATED,
                Json(TokenResponse {
                    token,
                    url: room.config.public_url.clone(),
                    room: room_name,
                    identity: req.identity,
                    role: req.role,
                    ttl_s,
                }),
            )
                .into_response()
        }
        Err(e) => problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/studio",
    operation_id = "studio_view",
    tag = "studio",
    summary = "The studio's view",
    description = "Sources in their slots, callers, seats, chat modes, notices, the playout link and the station mode. The studio app fetches it once and then follows `GET /api/v1/studio/events`.",
    responses(
        (status = 200, description = "The studio's view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn studio_view(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    Json(state.hub.view()).into_response()
}

/// The studio's view as server-sent events: the current view at once, then one per change.
#[utoipa::path(
    get,
    path = "/api/v1/studio/events",
    operation_id = "studio_events",
    tag = "studio",
    summary = "The studio's view as events",
    description = "Server-sent events: one `state` event at once whose data is the studio view as JSON, then one per change; a keep-alive comment every 10 seconds.",
    responses(
        (status = 200, description = "An event stream (`text/event-stream`).", body = String, content_type = "text/event-stream"),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn studio_events(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let stream = WatchStream::new(state.hub.subscribe()).map(|view| {
        Ok::<_, Infallible>(
            Event::default()
                .event("state")
                .json_data(&*view)
                .expect("the view serialises"),
        )
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

/// Take someone out of the room after their pass has ended.
async fn remove_from_room(state: &AppState, identity: &str) -> Result<(), Box<Response>> {
    match &state.hub.room {
        Some(room) => room.remove(identity).await.map_err(|e| {
            tracing::warn!(%identity, error = %format!("{e:#}"), "cannot remove them from the room");
            Box::new(problem(
                StatusCode::BAD_GATEWAY,
                format!("their pass is ended and they are off air, but LiveKit did not remove them: {e:#}"),
            ))
        }),
        None => {
            tracing::warn!(%identity, "no LiveKit secret: the pass is ended and they are off air, but still in the room");
            Ok(())
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EndPassRequest {
    /// The person whose pass ends.
    pub identity: String,
}

/// End anyone's pass (ADR-0119, reopening ADR-0108): off air and out of the cue, the pass
/// refused from now on (across a restart), then out of the room. Someone not in the room yet
/// (a leaked link nobody has used) is ended all the same.
#[utoipa::path(
    post,
    path = "/api/v1/passes/end",
    operation_id = "end_pass",
    tag = "room",
    summary = "End anyone's pass",
    description = "Off air and out of the cue, the pass refused from now on (across a restart), then out of the room (ADR-0119). Someone not in the room yet (a leaked link nobody has used) is ended all the same.",
    request_body = EndPassRequest,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 502, description = "Their pass is ended and they are off air, but LiveKit did not remove them.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn end_pass(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<EndPassRequest>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let identity = req.identity;
    let desk_prefix = state
        .room
        .as_ref()
        .map(|r| r.config.desk_identity.as_str())
        .unwrap_or("desk");
    if let Err(e) = check_identity(&identity, desk_prefix) {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string());
    }
    let hub = &state.hub;
    let at_desk = hub
        .desk_status()
        .is_some_and(|d| d.people.iter().any(|p| p.identity == identity));
    if at_desk {
        for cmd in [
            DeskCommand::OnAir {
                identity: identity.clone(),
                on: false,
            },
            DeskCommand::Cue {
                identity: identity.clone(),
                on: false,
            },
        ] {
            if let Err(e) = hub.desk.send(&cmd).await {
                return desk_problem(&e);
            }
        }
    }
    crate::public::end_pass(&state, &identity).await;
    if let Err(r) = remove_from_room(&state, &identity).await {
        return *r;
    }
    tracing::info!(%identity, at_desk, "pass ended by the studio");
    Json(hub.view()).into_response()
}

fn desk_problem(e: &DeskError) -> Response {
    let code = match e {
        DeskError::NotConfigured | DeskError::Unreachable { .. } => StatusCode::SERVICE_UNAVAILABLE,
        DeskError::Refused(_) | DeskError::Rejected { .. } => StatusCode::CONFLICT,
    };
    problem(code, e.to_string())
}

fn studio_problem(e: &StudioError) -> Response {
    let code = match e {
        StudioError::NoCaller(_) => StatusCode::NOT_FOUND,
        StudioError::NotNow(_) => StatusCode::CONFLICT,
    };
    problem(code, e.to_string())
}

#[utoipa::path(
    post,
    path = "/api/v1/desk/commands",
    operation_id = "desk_command",
    tag = "desk",
    summary = "A desk switch",
    description = "The same requests as the desk's control socket, without `status`: `on_air`, `mute`, `cue`, `talk`, `talk_to`, or `dump` the profanity delay. Answers with the new studio view. A caller cannot go on air or in cue without their consent to record on file (409).",
    request_body = crate::desk::DeskCommand,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn desk_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(cmd): Json<DeskCommand>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    // A caller goes on air or in cue only with their consent to record on file, however the
    // presenter takes them (the screener's send, or R on slot 4, ADR-0098).
    if let DeskCommand::OnAir { identity, on: true } | DeskCommand::Cue { identity, on: true } =
        &cmd
    {
        let role = state
            .hub
            .desk_status()
            .and_then(|d| d.people.into_iter().find(|p| &p.identity == identity))
            .map(|p| p.role);
        if let Some(role) = role
            && !state.hub.read_studio(|s| s.may_reach_desk(identity, role))
        {
            return problem(
                StatusCode::CONFLICT,
                format!(
                    "`{identity}` has no consent to record on file; a caller cannot go on air or in cue without it"
                ),
            );
        }
    }
    match state.hub.command(&cmd).await {
        Ok(()) => Json(state.hub.view()).into_response(),
        Err(e) => desk_problem(&e),
    }
}

/// What the screener (and the presenter, for a decline) does to a caller.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenerCommand {
    /// Open the screening conversation (the queue's selection).
    Screen { identity: String },
    /// Send to the desk (`P`), or send again after a decline.
    Send { identity: String },
    /// Back to the queue (`W`).
    Back { identity: String },
    /// The presenter declines a caller sent to the desk (DESK BUSY).
    Decline {
        identity: String,
        #[serde(default)]
        reason: String,
    },
    /// What the presenter reads: one line, the note, the name on air.
    Notes {
        identity: String,
        #[serde(default)]
        line: Option<String>,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        name_on_air: Option<String>,
    },
    /// Let a dropped caller's held place go (`⇧C`).
    Release { identity: String },
    /// Drop the caller (`⌃D ⌃D`): off air, out of the room, out of the queue.
    Drop { identity: String },
}

#[utoipa::path(
    post,
    path = "/api/v1/screener/commands",
    operation_id = "screener_command",
    tag = "screener",
    summary = "What the screener does to a caller",
    description = "`screen`, `send`, `back`, `decline`, `notes`, `release` or `drop`. 404 for someone who is not a caller; 409 when the command does not fit the caller's state; `drop` answers 502 when the caller is off air but LiveKit did not remove them from the room.",
    request_body = ScreenerCommand,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 502, description = "The caller is off air, but LiveKit did not remove them from the room.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn screener_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(cmd): Json<ScreenerCommand>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let hub = &state.hub;
    let on_air = |id: &str| {
        hub.desk_status()
            .and_then(|d| d.people.into_iter().find(|p| p.identity == id))
            .is_some_and(|p| p.on_air)
    };
    let done = match cmd {
        ScreenerCommand::Screen { identity } => hub.with_studio(|s, now| s.screen(&identity, now)),
        ScreenerCommand::Send { identity } => hub.with_studio(|s, now| s.send(&identity, now)),
        ScreenerCommand::Back { identity } => {
            let live = on_air(&identity);
            hub.with_studio(|s, now| s.back(&identity, live, now))
        }
        ScreenerCommand::Decline { identity, reason } => {
            hub.with_studio(|s, now| s.decline(&identity, reason.trim(), now))
        }
        ScreenerCommand::Notes {
            identity,
            line,
            note,
            name_on_air,
        } => hub.with_studio(|s, _| {
            s.notes(
                &identity,
                line.as_deref(),
                note.as_deref(),
                name_on_air.as_deref(),
            )
        }),
        ScreenerCommand::Release { identity } => hub.with_studio(|s, _| s.release(&identity)),
        ScreenerCommand::Drop { identity } => {
            let known = hub.desk_status().is_some_and(|d| {
                d.people
                    .iter()
                    .any(|p| p.identity == identity && p.role == Role::Caller)
            });
            if !known {
                return problem(
                    StatusCode::NOT_FOUND,
                    format!("`{identity}` is not a caller at the desk"),
                );
            }
            // Off air first, so a rejoin within the desk's grace does not come back on air.
            for cmd in [
                DeskCommand::OnAir {
                    identity: identity.clone(),
                    on: false,
                },
                DeskCommand::Cue {
                    identity: identity.clone(),
                    on: false,
                },
            ] {
                if let Err(e) = hub.desk.send(&cmd).await {
                    return desk_problem(&e);
                }
            }
            // A deliberate drop leaves no held place (ADR-0101) and ends their pass: to call
            // again they go through the lobby, and consent, again.
            crate::public::end_pass(&state, &identity).await;
            if let Err(r) = remove_from_room(&state, &identity).await {
                return *r;
            }
            tracing::info!(caller = %identity, "caller dropped");
            Ok(())
        }
    };
    match done {
        Ok(()) => Json(hub.view()).into_response(),
        Err(e) => studio_problem(&e),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatCommand {
    Mode {
        mode: Mode,
        on: bool,
    },
    /// On: five settings at once; off: the earlier settings back (ADR-0068).
    Influx {
        on: bool,
    },
}

#[utoipa::path(
    post,
    path = "/api/v1/chat/modes",
    operation_id = "chat_modes",
    tag = "chat",
    summary = "A chat mode",
    description = "A chat mode (`slow`, `hold_new`, `links_trusted`) on or off, or influx (five settings at once, the earlier ones back when turned off, ADR-0068). The studio's modes; the chat service reads them. Answers with the new studio view.",
    request_body = ChatCommand,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn chat_modes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(cmd): Json<ChatCommand>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    state.hub.with_studio(|s, now| match cmd {
        ChatCommand::Mode { mode, on } => s.set_mode(mode, on),
        ChatCommand::Influx { on } => s.influx(on, now),
    });
    Json(state.hub.view()).into_response()
}

fn cart_problem(e: &CartProblem) -> Response {
    let code = match e {
        CartProblem::NoCart(_) => StatusCode::NOT_FOUND,
        CartProblem::Missing(_)
        | CartProblem::AlreadySounding(_)
        | CartProblem::OnAir(_)
        | CartProblem::NothingSounding
        | CartProblem::Busy(_) => StatusCode::CONFLICT,
        CartProblem::Desk(DeskError::NotConfigured | DeskError::Unreachable { .. })
        | CartProblem::NoDatabase => StatusCode::SERVICE_UNAVAILABLE,
        CartProblem::Desk(DeskError::Refused(_) | DeskError::Rejected { .. }) => {
            StatusCode::BAD_GATEWAY
        }
        CartProblem::Invalid(_) | CartProblem::Refused(_) => StatusCode::UNPROCESSABLE_ENTITY,
        CartProblem::Failed(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    if code.is_server_error() {
        tracing::warn!(error = %e, "cart wall");
    }
    problem(code, e.to_string())
}

#[utoipa::path(
    get,
    path = "/api/v1/carts",
    operation_id = "carts_catalogue",
    tag = "carts",
    summary = "The banks and carts",
    description = "Each cart with FILE MISSING. Fetch it again when the studio view's `carts.version` moves.",
    responses(
        (status = 200, description = "The catalogue.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn carts_catalogue(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    Json(state.hub.catalogue()).into_response()
}

/// Fire, stop, stop the last, fade all. Answers with the studio's view, as the desk's do.
#[utoipa::path(
    post,
    path = "/api/v1/carts/commands",
    operation_id = "cart_command",
    tag = "carts",
    summary = "Fire, stop or cue a cart",
    description = "`toggle`, `fire`, `stop`, `cue` a cart; `stop_last`; `fade_all`; `clear_cue`. Carts sound in the desk (ADR-0117). Answers with the new studio view.",
    request_body = crate::carts::CartCommand,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 502, description = "The desk refused the command.", body = crate::openapi::Problem),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn cart_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(cmd): Json<CartCommand>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    match state.hub.cart_command(&cmd).await {
        Ok(()) => Json(state.hub.view()).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct BankRequest {
    name: String,
    colour_slot: u8,
}

#[utoipa::path(
    put,
    path = "/api/v1/carts/banks/{number}",
    operation_id = "put_bank",
    tag = "carts",
    summary = "Make or rename a bank",
    params(("number" = u8, Path, description = "The bank's number")),
    request_body = BankRequest,
    responses(
        (status = 200, description = "The new catalogue.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_bank(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(number): Path<u8>,
    Json(req): Json<BankRequest>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let bank = weavesignal_model::carts::Bank {
        number,
        name: req.name.trim().to_owned(),
        colour_slot: req.colour_slot,
    };
    match state.hub.put_bank(bank).await {
        Ok(()) => Json(state.hub.catalogue()).into_response(),
        Err(e) => cart_problem(&e),
    }
}

/// An audio file as the body, the cart's words in the query: the media worker makes it a cart.
#[utoipa::path(
    put,
    path = "/api/v1/carts/banks/{bank}/slots/{slot}",
    operation_id = "import_cart",
    tag = "carts",
    summary = "Put an audio file in a slot",
    description = "An audio file as the body, the cart's words in the query. The media worker reads it in its sandbox, brings it to the station's loudness and makes it the cart in that slot.",
    params(("bank" = u8, Path, description = "The bank's number"), ("slot" = u8, Path, description = "The slot in the bank"), CartMeta),
    request_body(content = crate::openapi::File, content_type = "application/octet-stream", description = "The audio file (any format the media worker reads), at most `carts.max_upload_mb`."),
    responses(
        (status = 201, description = "The cart.", body = weavesignal_model::carts::Cart),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 413, description = "The file is larger than `carts.max_upload_mb`.", body = crate::openapi::Problem),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn import_cart(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bank, slot)): Path<(u8, u8)>,
    Query(meta): Query<CartMeta>,
    body: Bytes,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    match state.hub.import_cart(bank, slot, meta, body.to_vec()).await {
        Ok(cart) => (StatusCode::CREATED, Json(cart)).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/carts/banks/{bank}/slots/{slot}",
    operation_id = "delete_cart",
    tag = "carts",
    summary = "Empty a slot",
    params(("bank" = u8, Path, description = "The bank's number"), ("slot" = u8, Path, description = "The slot in the bank")),
    responses(
        (status = 200, description = "The new catalogue.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_cart(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bank, slot)): Path<(u8, u8)>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    match state.hub.delete_cart(bank, slot).await {
        Ok(()) => Json(state.hub.catalogue()).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
struct Health {
    status: &'static str,
    version: &'static str,
    database: &'static str,
}

/// Liveness and the database's reachability. 200 when the service and, if configured, its
/// database answer; 503 when the database does not.
#[utoipa::path(
    get,
    path = "/health",
    operation_id = "health",
    tag = "station",
    summary = "Liveness",
    description = "200 when the service and, if there is one, its database answer; 503 with `status` `degraded` when the database does not. Used by the proxy and the container healthcheck.",
    responses(
        (status = 200, description = "Alive.", body = Health),
        (status = 503, description = "The database does not answer.", body = Health),
    ),
    security(()),
)]
async fn health(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Health>) {
    let database = match &state.db {
        None => "not configured",
        Some(pool) => match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => "ok",
            Err(e) => {
                tracing::warn!(error = %e, "health: the database did not answer");
                "unreachable"
            }
        },
    };
    let code = if database == "unreachable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let status = if code.is_success() { "ok" } else { "degraded" };
    (
        code,
        Json(Health {
            status,
            version: env!("CARGO_PKG_VERSION"),
            database,
        }),
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/station",
    operation_id = "station",
    tag = "station",
    summary = "The station",
    description = "The station's identifier and name.",
    responses(
        (status = 200, description = "The station.", body = Station),
    ),
    security(()),
)]
async fn station(State(state): State<Arc<AppState>>) -> Json<Station> {
    Json(state.station.clone())
}

// ---- the description (ADR-0195) ----

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    health,
    station,
    room_token,
    studio_view,
    studio_events,
    desk_command,
    screener_command,
    end_pass,
    chat_modes,
    carts_catalogue,
    cart_command,
    put_bank,
    import_cart,
    delete_cart
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use weavesignal_model::StationId;

    use crate::desk::DeskClient;
    use crate::studio::Studio;

    const ADMIN: &str = "admin-test-token";

    fn keys() -> Keys {
        Keys {
            api_key: "weavesignal".into(),
            api_secret: "test-only-secret-test-only-secret-0".into(),
        }
    }

    fn hub(socket: Option<&std::path::Path>) -> Arc<Hub> {
        Hub::new(
            "Kettle FM",
            Studio::new("The Morning Kettle", vec![]),
            DeskClient::new(socket),
            None,
            Duration::from_millis(250),
            crate::carts::CartsLink::none(),
        )
    }

    fn app() -> (Router, Station) {
        let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
        let state = Arc::new(AppState {
            station: station.clone(),
            db: None,
            room: Some(RoomState {
                config: RoomConfig::default(),
                keys: keys(),
            }),
            admin_token: Some(ADMIN.into()),
            staff: Default::default(),
            hub: hub(None),
            join: JoinConfig::default(),
            video: Default::default(),
            joins: Default::default(),
            turn_tls_port: 5349,
            steering: Default::default(),
        });
        (router(state), station)
    }

    async fn post_json(
        app: Router,
        uri: &str,
        bearer: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::post(uri).header("content-type", "application/json");
        if let Some(b) = bearer {
            req = req.header("authorization", format!("Bearer {b}"));
        }
        let res = app
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    /// A hub with a fake desk and three carts in memory: one with its file, one whose file
    /// is gone, one a bed.
    async fn cart_app() -> (
        Router,
        Arc<Hub>,
        Arc<std::sync::Mutex<crate::desk::fake::FakeDesk>>,
        [uuid::Uuid; 3],
        std::path::PathBuf,
    ) {
        use weavesignal_model::carts::{Bank, Cart, CartMode};
        let dir = crate::desk::tests::tempdir();
        let fake = crate::desk::fake::spawn(&dir.join("desk.sock"));
        let cfg = crate::config::CartsConfig {
            media_dir: dir.join("carts"),
            ..crate::config::CartsConfig::default()
        };
        std::fs::create_dir_all(&cfg.media_dir).unwrap();
        let hub = Hub::new(
            "Kettle FM",
            Studio::new("The Morning Kettle", vec![]),
            DeskClient::new(Some(&dir.join("desk.sock"))),
            None,
            Duration::from_millis(250),
            crate::carts::CartsLink::new(cfg.clone(), None),
        );
        let file = |present: bool| {
            let f = format!("{}.flac", uuid::Uuid::new_v4());
            if present {
                std::fs::write(cfg.media_dir.join(&f), b"fLaC").unwrap();
            }
            Some(f)
        };
        let cart = |slot: u8, label: &str, mode, file| Cart {
            id: uuid::Uuid::new_v4(),
            bank: 1,
            slot,
            label: label.into(),
            detail: String::new(),
            mode,
            file,
            duration_ms: 3_000,
            gain_db: -7.0,
            loudness_lufs: Some(-16.0),
            true_peak_dbtp: Some(-1.5),
            ramp_ms: None,
            origin: String::new(),
            licence: String::new(),
        };
        let carts = vec![
            cart(1, "Applause", CartMode::OneShot, file(true)),
            cart(2, "Show close", CartMode::OneShot, file(false)),
            cart(3, "Back-anno bed", CartMode::Bed, file(true)),
        ];
        let ids = [carts[0].id, carts[1].id, carts[2].id];
        let bank = Bank {
            number: 1,
            name: "The Morning Kettle".into(),
            colour_slot: 3,
        };
        hub.with_inner(|i| i.carts.replace(vec![bank], carts, &cfg.media_dir));
        let state = Arc::new(AppState {
            station: Station::new(StationId::new_random(), "Kettle FM").unwrap(),
            db: None,
            room: None,
            admin_token: Some(ADMIN.into()),
            staff: Default::default(),
            hub: hub.clone(),
            join: JoinConfig::default(),
            video: Default::default(),
            joins: Default::default(),
            turn_tls_port: 5349,
            steering: Default::default(),
        });
        (router(state), hub, fake, ids, dir)
    }

    async fn get_admin(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let req = Request::get(uri)
            .header("authorization", format!("Bearer {ADMIN}"))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn a_cart_key_fires_the_cart_at_the_desk_and_a_second_press_fades_it() {
        let (app, hub, fake, [applause, _, _], dir) = cart_app().await;
        let cmd = serde_json::json!({"op": "toggle", "cart": applause});
        let (code, view) = post_json(
            app.clone(),
            "/api/v1/carts/commands",
            Some(ADMIN),
            cmd.clone(),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{view}");
        let s = &view["carts"]["sounding"][0];
        assert_eq!(s["cart"], applause.to_string());
        assert_eq!(
            (s["stopping"].as_bool(), s["cue"].as_bool()),
            (Some(false), Some(false))
        );
        assert_eq!(
            s["ends_ms"].as_u64().unwrap() - s["started_ms"].as_u64().unwrap(),
            3_000
        );
        let fire = fake.lock().unwrap().lines[0].clone();
        assert_eq!(fire["op"], "cart_fire");
        assert_eq!(fire["cart"], applause.to_string());
        assert_eq!(
            (fire["gain_db"].as_f64(), fire["mode"].as_str()),
            (Some(-7.0), Some("one_shot"))
        );
        assert_eq!(
            (fire["cue"].as_bool(), fire["duration_ms"].as_u64()),
            (Some(false), Some(3_000))
        );
        let (code, view) = post_json(app, "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::OK, "{view}");
        assert_eq!(view["carts"]["sounding"][0]["stopping"], true);
        assert_eq!(fake.lock().unwrap().ops(), vec!["cart_fire", "cart_stop"]);
        // The desk's status is folded in, and its play events are recorded for the log.
        let mut plays = hub.subscribe_plays();
        fake.lock().unwrap().finish_fades();
        hub.poll_once().await;
        assert!(hub.carts_view().sounding.is_empty());
        assert!(hub.carts_view().desk);
        let recorded = hub.cart_plays();
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            (recorded[0].event.as_str(), recorded[1].event.as_str()),
            ("fired", "ended")
        );
        assert_eq!(recorded[1].end.as_deref(), Some("stopped"));
        assert_eq!(plays.try_recv().unwrap().event, "fired");
        assert_eq!(plays.try_recv().unwrap().event, "ended");
        hub.poll_once().await;
        assert_eq!(hub.cart_plays().len(), 2, "each event is recorded once");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_cart_without_its_file_says_file_missing_and_does_nothing() {
        let (app, _hub, fake, [applause, show_close, _], dir) = cart_app().await;
        let (code, cat) = get_admin(app.clone(), "/api/v1/carts").await;
        assert_eq!(code, StatusCode::OK);
        let missing: Vec<bool> = cat["carts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["missing"].as_bool().unwrap())
            .collect();
        assert_eq!(missing, vec![false, true, false]);
        let cmd = serde_json::json!({"op": "fire", "cart": show_close});
        let (code, body) = post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().starts_with("FILE MISSING"),
            "{body}"
        );
        assert!(
            fake.lock().unwrap().ops().is_empty(),
            "the desk was not asked"
        );
        // A file the desk cannot find is FILE MISSING too.
        let file = _hub.catalogue().carts[0].cart.file.clone().unwrap();
        fake.lock().unwrap().missing.push(file);
        let cmd = serde_json::json!({"op": "fire", "cart": applause});
        let (code, body) = post_json(app, "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().starts_with("FILE MISSING"),
            "{body}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn space_stops_the_last_and_fade_all_stops_everything() {
        let (app, hub, fake, [applause, _, bed], dir) = cart_app().await;
        for c in [bed, applause] {
            let cmd = serde_json::json!({"op": "fire", "cart": c});
            let (code, _) =
                post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), cmd).await;
            assert_eq!(code, StatusCode::OK);
        }
        // Firing what is already sounding is refused (its key would stop it instead).
        let cmd = serde_json::json!({"op": "fire", "cart": bed});
        let (code, _) = post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::CONFLICT);
        let last = serde_json::json!({"op": "stop_last"});
        let (code, view) =
            post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), last).await;
        assert_eq!(code, StatusCode::OK);
        let stopping: Vec<(String, bool)> = view["carts"]["sounding"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["cart"].as_str().unwrap().to_owned(),
                    s["stopping"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            stopping,
            vec![(bed.to_string(), false), (applause.to_string(), true)]
        );
        let all = serde_json::json!({"op": "fade_all"});
        let (code, _) = post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), all).await;
        assert_eq!(code, StatusCode::OK);
        let last_line = fake.lock().unwrap().lines.last().unwrap().clone();
        assert_eq!(
            last_line,
            serde_json::json!({"op": "cart_stop_all", "cue": null})
        );
        fake.lock().unwrap().finish_fades();
        hub.poll_once().await;
        assert!(hub.carts_view().sounding.is_empty());
        let (code, _) = post_json(
            app,
            "/api/v1/carts/commands",
            Some(ADMIN),
            serde_json::json!({"op": "stop_last"}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "nothing left to stop");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn shift_and_a_key_cue_a_cart_and_esc_or_its_key_takes_it_out() {
        let (app, hub, fake, [applause, _, bed], dir) = cart_app().await;
        let post = |cmd: serde_json::Value| {
            let app = app.clone();
            async move { post_json(app, "/api/v1/carts/commands", Some(ADMIN), cmd).await }
        };
        let (code, view) = post(serde_json::json!({"op": "cue", "cart": applause})).await;
        assert_eq!(code, StatusCode::OK, "{view}");
        assert_eq!(view["carts"]["sounding"][0]["cue"], true);
        assert_eq!(fake.lock().unwrap().lines[0]["cue"], true);
        // Space is for what is on air: a cue is not stopped by it.
        let (code, _) = post(serde_json::json!({"op": "stop_last"})).await;
        assert_eq!(code, StatusCode::CONFLICT);
        // Its key fires it on air from the start, out of the cue.
        let (code, view) = post(serde_json::json!({"op": "toggle", "cart": applause})).await;
        assert_eq!(code, StatusCode::OK, "{view}");
        assert_eq!(
            fake.lock().unwrap().ops(),
            vec!["cart_fire", "cart_stop", "cart_fire"]
        );
        assert_eq!(fake.lock().unwrap().lines.last().unwrap()["cue"], false);
        // On air, it cannot be cued as well.
        let (code, _) = post(serde_json::json!({"op": "cue", "cart": applause})).await;
        assert_eq!(code, StatusCode::CONFLICT);
        // A bed cued, then Esc.
        post(serde_json::json!({"op": "cue", "cart": bed})).await;
        let (code, view) = post(serde_json::json!({"op": "clear_cue"})).await;
        assert_eq!(code, StatusCode::OK);
        let last = fake.lock().unwrap().lines.last().unwrap().clone();
        assert_eq!(
            last,
            serde_json::json!({"op": "cart_stop_all", "cue": true})
        );
        let bed_now = view["carts"]["sounding"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["cart"] == bed.to_string())
            .unwrap()
            .clone();
        assert_eq!(bed_now["stopping"], true);
        // Shift and the key again takes a cue out.
        fake.lock().unwrap().finish_fades();
        hub.poll_once().await;
        post(serde_json::json!({"op": "cue", "cart": bed})).await;
        post(serde_json::json!({"op": "cue", "cart": bed})).await;
        assert_eq!(fake.lock().unwrap().ops().last().unwrap(), "cart_stop");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_desk_says_when_beds_are_dipped_and_a_full_pool_is_busy() {
        let (app, hub, fake, [applause, _, bed], dir) = cart_app().await;
        let cmd = serde_json::json!({"op": "fire", "cart": bed});
        post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        fake.lock().unwrap().mic_open = true;
        hub.poll_once().await;
        assert!(hub.carts_view().ducked && hub.carts_view().mic_open);
        fake.lock().unwrap().mic_open = false;
        hub.poll_once().await;
        assert!(!hub.carts_view().ducked);
        // Eight one-shots sounding (other carts, as the desk sees them): the ninth is busy.
        for n in 0..8 {
            fake.lock().unwrap().sounding.push((
                uuid::Uuid::new_v4().to_string(),
                "shot".into(),
                n + 1,
                "one_shot".into(),
                false,
                100 + u64::from(n),
                false,
            ));
        }
        hub.poll_once().await;
        let cmd = serde_json::json!({"op": "fire", "cart": applause});
        let (code, body) = post_json(app, "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("every one-shot player"),
            "{body}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn without_a_desk_or_a_database_the_wall_says_so() {
        let (a, _) = app();
        let cmd = serde_json::json!({"op": "fire", "cart": uuid::Uuid::new_v4()});
        let (code, _) = post_json(a, "/api/v1/carts/commands", Some(ADMIN), cmd).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "no such cart");
        let (a, _) = app();
        let req = Request::put("/api/v1/carts/banks/1/slots/1?label=Ding&mode=one_shot")
            .header("authorization", format!("Bearer {ADMIN}"))
            .body(Body::from(vec![0u8; 16]))
            .unwrap();
        let res = a.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let (a, _) = app();
        let (code, _) = post_json(
            a,
            "/api/v1/carts/commands",
            None,
            serde_json::json!({"op": "fade_all"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_dj_gets_a_short_lived_token_with_their_role() {
        let (app, _) = app();
        let (code, body) = post_json(
            app,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "eilidh", "name": "Eilidh Fraser", "role": "dj", "ttl_s": 86400}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        assert_eq!(body["ttl_s"], 600, "never longer than the station allows");
        let claims = keys().verify(body["token"].as_str().unwrap()).unwrap();
        assert_eq!(claims.sub, "eilidh");
        assert_eq!(claims.role(), Some(Role::Dj));
        assert_eq!(claims.video.room.as_deref(), Some("studio"));
        assert!(!claims.video.can_update_own_metadata);
        assert!(claims.exp - claims.nbf <= 610);
    }

    #[tokio::test]
    async fn a_sender_gets_a_publish_only_token_when_it_asks() {
        // M1's route took `subscribe` and signed every token with the subscribe grant: a
        // native publisher holding it sends no media (ADR-0090). Found in the M2 run.
        for (asked, want) in [(Some(false), false), (Some(true), true), (None, true)] {
            let (app, _) = app();
            let mut body = serde_json::json!({"identity": "eilidh", "role": "presenter"});
            if let Some(a) = asked {
                body["subscribe"] = a.into();
            }
            let (code, body) = post_json(app, "/api/v1/room/tokens", Some(ADMIN), body).await;
            assert_eq!(code, StatusCode::CREATED, "{body}");
            let claims = keys().verify(body["token"].as_str().unwrap()).unwrap();
            assert_eq!(claims.video.can_subscribe, Some(want), "asked {asked:?}");
            assert_eq!(claims.video.can_publish, Some(true));
        }
    }

    #[tokio::test]
    async fn a_listener_gets_a_token_that_only_listens() {
        // Monitors and test drivers' ears: the desk gives them no mix-minus (every token
        // carries a role, and the staging run's ears each became a desk person with a bus).
        let (a, _) = app();
        let body = serde_json::json!({"identity": "ear-gregor", "role": "listener"});
        let (code, body) = post_json(a, "/api/v1/room/tokens", Some(ADMIN), body).await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        let claims = keys().verify(body["token"].as_str().unwrap()).unwrap();
        assert_eq!(claims.role(), Some(Role::Listener));
        assert_eq!(claims.video.can_publish, Some(false));
        assert_eq!(claims.video.can_subscribe, Some(true));
        assert!(
            claims.video.recorder,
            "a monitor hears past the pages' limits"
        );
        let (a, _) = app();
        let body =
            serde_json::json!({"identity": "ear-gregor", "role": "listener", "subscribe": false});
        let (code, _) = post_json(a, "/api/v1/room/tokens", Some(ADMIN), body).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn tokens_need_the_admin_bearer() {
        let body = serde_json::json!({"identity": "x", "role": "guest"});
        let (a, _) = app();
        let (code, _) = post_json(a, "/api/v1/room/tokens", None, body.clone()).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        let (a, _) = app();
        let (code, _) = post_json(a, "/api/v1/room/tokens", Some("wrong"), body).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_desks_identities_and_unknown_roles_are_refused() {
        let (a, _) = app();
        let (code, body) = post_json(
            a,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "desk-mm-guest", "role": "guest"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        let (a, _) = app();
        let (code, _) = post_json(
            a,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "x", "role": "admin"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn a_monitor_can_name_a_callers_booth_and_others_join_the_studio() {
        // ADR-0193: callers are each in their own booth; a monitor for one names it.
        let studio = RoomConfig::default().name;
        let (a, _) = app();
        let (code, body) = post_json(
            a,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "ear-1", "role": "listener", "booth": "c7f3"}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        assert_eq!(body["room"], weavesignal_proto::booth(&studio, "c7f3"));
        let (a, _) = app();
        let (_, body) = post_json(
            a,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "fraser", "role": "guest"}),
        )
        .await;
        assert_eq!(body["room"], studio);
        let (a, _) = app();
        let (code, _) = post_json(
            a,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "fraser", "role": "guest", "booth": "c7f3"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn an_upload_is_refused_before_its_body_is_read() {
        // Review 0025, P1: a body that fails as soon as anything reads it. Before the
        // middleware, extracting `Bytes` read it first (up to 64 or 200 MB) and failed there;
        // now the stranger is refused with the body untouched.
        for uri in [
            "/api/v1/carts/banks/1/slots/1?title=x",
            "/api/v1/library/tracks?title=x",
        ] {
            let (a, _) = app();
            let body = Body::from_stream(tokio_stream::once(Err::<axum::body::Bytes, _>(
                std::io::Error::other("the body was read before the bearer was checked"),
            )));
            let res = a
                .oneshot(Request::put(uri).body(body).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
    }

    #[tokio::test]
    async fn without_an_admin_token_the_route_is_off() {
        let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
        let app = router(Arc::new(AppState {
            station,
            db: None,
            room: None,
            admin_token: None,
            staff: Default::default(),
            hub: hub(None),
            join: JoinConfig::default(),
            video: Default::default(),
            joins: Default::default(),
            turn_tls_port: 5349,
            steering: Default::default(),
        }));
        let (code, body) = post_json(
            app,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "x", "role": "guest"}),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("WEAVESIGNAL_CONTROL_ADMIN_TOKEN")
        );
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn health_answers_without_a_database() {
        let (app, _) = app();
        let (code, body) = get_json(app, "/health").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["database"], "not configured");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn the_description_needs_no_bearer_and_is_the_committed_one() {
        let (app, _) = app();
        let (code, body) = get_json(app, "/api/v1/openapi.json").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["openapi"], "3.1.0");
        assert_eq!(body["info"]["version"], "1");
        assert!(body["paths"]["/api/v1/studio"]["get"]["security"].is_array());
        let committed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../docs/openapi/weavesignal-v1.json"
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(body, committed);
    }

    #[tokio::test]
    async fn the_station_carries_its_identifier() {
        let (app, station) = app();
        let (code, body) = get_json(app, "/api/v1/station").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["id"], station.id.to_string());
        assert_eq!(body["name"], "Kettle FM");
    }

    /// A desk on a Unix socket that keeps its people's switches, as the real one does.
    fn fake_desk() -> std::path::PathBuf {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let dir = std::env::temp_dir().join(format!("weavesignal-app-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("desk.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let person = |id: &str, name: &str, role: &str, on_air: bool| {
            serde_json::json!({"identity": id, "name": name, "role": role, "on_air": on_air,
                "mute": false, "cue": false, "talk": false, "talk_to": null, "present": true,
                "away_ms": null, "linked": true, "lufs": -23.0, "lufs_peak": -20.0})
        };
        let people = Arc::new(std::sync::Mutex::new(vec![
            person("eilidh", "Eilidh Fraser-MacLeod", "presenter", true),
            person("niall", "Niall Anderson", "caller", false),
            person("elspeth", "Elspeth Gordon", "screener", false),
        ]));
        // The delay: full at 8 s, then each dump empties it and it rebuilds.
        let delay = Arc::new(std::sync::Mutex::new(serde_json::json!({"delay_ms": 8000,
            "target_ms": 8000, "rebuilding": false, "dumps": 0, "dumped_ms_ago": null})));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let people = people.clone();
                let delay = delay.clone();
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                        let op = v["op"].as_str().unwrap().to_owned();
                        let answer = {
                            let mut ps = people.lock().unwrap();
                            let mut d = delay.lock().unwrap();
                            if op == "status" {
                                serde_json::json!({"ok": true, "people": *ps,
                                    "playout": {"connected": false, "for_ms": 14000},
                                    "delay": *d,
                                    "video": {"mode": {"mode": "auto"}, "shot": "person",
                                        "identity": "eilidh", "cameras": ["eilidh"],
                                        "hls": true, "room": true}})
                            } else if op == "picture" || op == "now_playing" {
                                serde_json::json!({"ok": true})
                            } else if op == "dump" {
                                if d["delay_ms"] == 0 {
                                    serde_json::json!({"ok": false, "error": "the delay holds nothing to dump"})
                                } else {
                                    let dumps = d["dumps"].as_u64().unwrap() + 1;
                                    *d = serde_json::json!({"delay_ms": 0, "target_ms": 8000,
                                        "rebuilding": true, "dumps": dumps, "dumped_ms_ago": 0});
                                    serde_json::json!({"ok": true, "delay": *d})
                                }
                            } else if let Some(p) =
                                ps.iter_mut().find(|p| p["identity"] == v["identity"])
                            {
                                match op.as_str() {
                                    "talk_to" => {
                                        p["talk_to"] = if v["on"] == true {
                                            v["to"].clone()
                                        } else {
                                            serde_json::Value::Null
                                        }
                                    }
                                    field => p[field] = v["on"].clone(),
                                }
                                serde_json::json!({"ok": true})
                            } else {
                                serde_json::json!({"ok": false, "error": "`x` is not at the desk"})
                            }
                        };
                        w.write_all(format!("{answer}\n").as_bytes()).await.unwrap();
                    }
                });
            }
        });
        path
    }

    #[tokio::test]
    async fn the_desks_hls_is_served_with_its_types_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("weavesignal-hls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("v0")).unwrap();
        std::fs::write(dir.join("live.m3u8"), "#EXTM3U\n").unwrap();
        std::fs::write(dir.join("v0/seg00001.m4s"), b"moof").unwrap();
        std::fs::write(dir.join("secret.txt"), b"no").unwrap();
        let join = JoinConfig {
            hls_dir: dir.clone(),
            ..JoinConfig::default()
        };
        let (app, _) = join_app(join, false).await;
        let get = |uri: &str| {
            let app = app.clone();
            let uri = uri.to_owned();
            async move {
                let res = app
                    .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let ty = res
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .map(|v| v.to_str().unwrap().to_owned());
                let cache = res
                    .headers()
                    .get(header::CACHE_CONTROL)
                    .map(|v| v.to_str().unwrap().to_owned());
                (res.status(), ty, cache)
            }
        };
        let (code, ty, cache) = get("/hls/live.m3u8").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(ty.as_deref(), Some("application/vnd.apple.mpegurl"));
        assert_eq!(cache.as_deref(), Some("no-cache"));
        let (code, ty, _) = get("/hls/v0/seg00001.m4s").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(ty.as_deref(), Some("video/mp4"));
        for bad in [
            "/hls/secret.txt",
            "/hls/../live.m3u8",
            "/hls/v0/../../live.m3u8",
            "/hls/.hidden.m3u8",
            "/hls/a/b/c.m3u8",
            "/hls/v0/nothing.m4s",
        ] {
            assert_eq!(get(bad).await.0, StatusCode::NOT_FOUND, "{bad}");
        }
        let (code, p) = get_json(app.clone(), "/api/v1/player").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(p["video"]["live"], "/hls/live.m3u8");
        assert_eq!(p["video"]["audio"], "/hls/audio.m3u8");
        let (code, v) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "picture", "mode": "slate"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["desk"]["picture"]["shot"], "person");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_multivariant_playlists_name_the_steering_route_while_it_has_edges() {
        let dir = std::env::temp_dir().join(format!("weavesignal-hls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("v0")).unwrap();
        // What the desk writes (`video::multivariant`), and a media playlist beside it.
        let multivariant = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"opus\"\naudio/index.m3u8\n";
        std::fs::write(dir.join("live.m3u8"), multivariant).unwrap();
        std::fs::write(dir.join("audio.m3u8"), multivariant).unwrap();
        std::fs::write(
            dir.join("v0/index.m3u8"),
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n",
        )
        .unwrap();
        let join = || JoinConfig {
            hls_dir: dir.clone(),
            ..JoinConfig::default()
        };
        let body = |app: Router, uri: &'static str| async move {
            let res = app
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            let b = axum::body::to_bytes(res.into_body(), 1 << 20)
                .await
                .unwrap();
            String::from_utf8(b.to_vec()).unwrap()
        };
        let edge = |pathway: &str, host: &str| crate::steering::EdgeConfig {
            pathway: pathway.into(),
            host: host.into(),
            weight: 1,
            check_url: String::new(),
        };

        // With edges: the line is the second line of both multivariant playlists, once.
        let steering = crate::steering::SteeringConfig {
            edges: vec![edge("edge-1", "edge-1.edges.example")],
            ..Default::default()
        };
        let (app, _) = join_app_steered(join(), false, steering).await;
        let line = "#EXT-X-CONTENT-STEERING:SERVER-URI=\"/api/v1/steering\",PATHWAY-ID=\".\"";
        for uri in ["/hls/live.m3u8", "/hls/audio.m3u8"] {
            let text = body(app.clone(), uri).await;
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines[0], "#EXTM3U", "{text}");
            assert_eq!(lines[1], line, "{text}");
            assert_eq!(text.matches("EXT-X-CONTENT-STEERING").count(), 1, "{text}");
            assert!(
                text.contains("#EXT-X-STREAM-INF"),
                "the rest is kept: {text}"
            );
        }
        // A media playlist is not steered.
        let media = body(app, "/hls/v0/index.m3u8").await;
        assert!(!media.contains("STEERING"), "{media}");

        // With `clone_from` empty the line names the route alone.
        let steering = crate::steering::SteeringConfig {
            edges: vec![edge("origin", "")],
            clone_from: String::new(),
            ..Default::default()
        };
        let (app, _) = join_app_steered(join(), false, steering).await;
        let text = body(app, "/hls/live.m3u8").await;
        assert!(
            text.contains("\n#EXT-X-CONTENT-STEERING:SERVER-URI=\"/api/v1/steering\"\n"),
            "{text}"
        );

        // With no edges, steering is off and the playlist is the desk's own.
        let (app, _) = join_app_steered(join(), false, Default::default()).await;
        assert_eq!(body(app, "/hls/live.m3u8").await, multivariant);
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn studio_app() -> Router {
        join_app(JoinConfig::default(), true).await.0
    }

    /// The consent words' version for the test station.
    fn version() -> String {
        crate::join::consent_version(&crate::join::consent_words(
            crate::config::CONSENT_TEXT,
            "Kettle FM",
            "only until the show ends",
            TEST_CONTACT,
        ))
    }

    /// The studio against the fake desk; `room` adds LiveKit's keys, so tokens and passes work.
    async fn join_app(join: JoinConfig, room: bool) -> (Router, Arc<AppState>) {
        // The call-in challenge has its own test below (and crate::challenge's).
        join_app_challenged(
            JoinConfig {
                challenge_bits: 0,
                ..join
            },
            room,
        )
        .await
    }

    /// The test station's contact for the consent words (call-in waits for one).
    const TEST_CONTACT: &str = "calls@kettle.example";

    async fn join_app_challenged(join: JoinConfig, room: bool) -> (Router, Arc<AppState>) {
        join_app_steered(join, room, Default::default()).await
    }

    async fn join_app_steered(
        join: JoinConfig,
        room: bool,
        steering: crate::steering::SteeringConfig,
    ) -> (Router, Arc<AppState>) {
        let join = JoinConfig {
            consent_contact: if join.consent_contact.is_empty() {
                TEST_CONTACT.into()
            } else {
                join.consent_contact
            },
            ..join
        };
        let path = fake_desk();
        let hub = hub(Some(&path));
        hub.poll_once().await;
        let state = Arc::new(AppState {
            station: Station::new(StationId::new_random(), "Kettle FM").unwrap(),
            db: None,
            room: room.then(|| RoomState {
                config: RoomConfig::default(),
                keys: keys(),
            }),
            admin_token: Some(ADMIN.into()),
            staff: Default::default(),
            hub,
            join,
            video: Default::default(),
            joins: Default::default(),
            turn_tls_port: 5349,
            steering,
        });
        (router(state.clone()), state)
    }

    fn call(name: &str, consent: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"name": name, "topic": "No bus after 23:10 from the Infirmary", "consent": consent})
    }

    #[tokio::test]
    async fn a_caller_gets_no_token_without_consent_to_record() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let (code, p) = get_json(app.clone(), "/api/v1/player").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(p["call_in"]["open"], true, "{p}");
        assert!(
            p["call_in"]["consent_text"].as_str().unwrap()
                == "You'll be live on Kettle FM, and we may keep your call in our archive and podcast only until the show ends. To have it taken down, contact calls@kettle.example."
        );
        let v = p["call_in"]["consent_version"].as_str().unwrap().to_owned();
        assert_eq!(v, version());

        for (consent, want) in [
            (serde_json::Value::Null, StatusCode::UNPROCESSABLE_ENTITY),
            (
                serde_json::json!({"accepted": false, "version": v}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                serde_json::json!({"accepted": true, "version": "0000000000000000"}),
                StatusCode::CONFLICT,
            ),
        ] {
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/call-in",
                None,
                call("Jessie Brown", consent.clone()),
            )
            .await;
            assert_eq!(code, want, "{consent}: {body}");
            assert_eq!(body["field"], "consent");
            assert!(body.get("token").is_none() && body.get("pass").is_none());
        }
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/call-in",
            None,
            call("  ", serde_json::json!({"accepted": true, "version": v})),
        )
        .await;
        assert_eq!(
            (code, body["field"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("name"))
        );

        let (code, body) = post_json(
            app.clone(),
            "/api/v1/call-in",
            None,
            call(
                "Jessie Brown",
                serde_json::json!({"accepted": true, "version": v}),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        let id = body["identity"].as_str().unwrap().to_owned();
        assert!(id.starts_with("caller-"));
        assert_eq!(body["hear"], format!("desk-mm-{id}"));
        let claims = keys().verify(body["token"].as_str().unwrap()).unwrap();
        assert_eq!(
            (claims.role(), claims.sub.as_str()),
            (Some(Role::Caller), id.as_str())
        );
        assert_eq!(claims.name, "Jessie Brown");
        assert_eq!(claims.video.can_subscribe, Some(true));
        let pass = keys().verify_pass(body["pass"].as_str().unwrap()).unwrap();
        assert_eq!(pass.id, id);
        let kept = state
            .hub
            .read_studio(|s| s.call_in_of(&id).cloned())
            .unwrap();
        assert_eq!(kept.consent.by, crate::studio::ConsentBy::Caller);
        assert_eq!(kept.consent.version, v);
        assert!(
            kept.consent.text.contains("Kettle FM"),
            "the words agreed to are kept"
        );
        assert_eq!(kept.topic, "No bus after 23:10 from the Infirmary");
    }

    // ---- call-in's challenge (ADR-0006) ----

    #[tokio::test]
    async fn call_in_needs_the_answered_challenge_once_and_before_the_rate_limit() {
        let tight = JoinConfig {
            challenge_bits: 10,
            challenge_bits_busy: 12,
            calls_per_minute: 2,
            ..JoinConfig::default()
        };
        let (app, _) = join_app_challenged(tight, true).await;
        let ok = serde_json::json!({"accepted": true, "version": version()});
        // No answer, or a forged one: refused beside the challenge, and nothing counted.
        for bad in [
            serde_json::Value::Null,
            serde_json::json!({"token": "rsc1.9999999999999.0.00.00", "nonce": "0"}),
        ] {
            for _ in 0..3 {
                let mut body = call("Jessie", ok.clone());
                body["challenge"] = bad.clone();
                let (code, b) = post_json(app.clone(), "/api/v1/call-in", None, body).await;
                assert_eq!(
                    (code, b["field"].as_str()),
                    (StatusCode::UNPROCESSABLE_ENTITY, Some("challenge"))
                );
            }
        }
        let answer = |c: &serde_json::Value| {
            let salt = c["salt"].as_str().unwrap();
            let bits = u8::try_from(c["bits"].as_u64().unwrap()).unwrap();
            serde_json::json!({"token": c["token"], "nonce": crate::challenge::solve(salt, bits).to_string()})
        };
        let (code, c) = get_json(app.clone(), "/api/v1/call-in/challenge").await;
        assert_eq!((code, c["bits"].as_u64()), (StatusCode::OK, Some(10)));
        let mut body = call("Jessie", ok.clone());
        body["challenge"] = answer(&c);
        let (code, b) = post_json(app.clone(), "/api/v1/call-in", None, body.clone()).await;
        assert_eq!(code, StatusCode::CREATED, "{b}");
        // The same answer twice: refused, even though the minute has room.
        let (code, b) = post_json(app.clone(), "/api/v1/call-in", None, body).await;
        assert_eq!(b["field"], "challenge", "{code} {b}");
        // One call in a minute of two is half used: the next challenge is the busy one.
        let (_, c) = get_json(app.clone(), "/api/v1/call-in/challenge").await;
        assert_eq!(c["bits"], 12);
        let mut body = call("Iona", ok.clone());
        body["challenge"] = answer(&c);
        let (code, _) = post_json(app.clone(), "/api/v1/call-in", None, body).await;
        assert_eq!(code, StatusCode::CREATED);
        let (_, c) = get_json(app.clone(), "/api/v1/call-in/challenge").await;
        let mut body = call("Morag", ok);
        body["challenge"] = answer(&c);
        let (code, _) = post_json(app, "/api/v1/call-in", None, body).await;
        assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn the_admin_route_needs_consent_for_a_caller_token_too() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let ask = |consent: Option<serde_json::Value>| {
            let mut body = serde_json::json!({"identity": "dougal", "role": "caller", "name": "Dougal Thomson"});
            if let Some(c) = consent {
                body["consent"] = c;
            }
            post_json(app.clone(), "/api/v1/room/tokens", Some(ADMIN), body)
        };
        let (code, _) = ask(None).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(state.hub.read_studio(|s| s.call_in_of("dougal").is_none()));
        let (code, body) = ask(Some(
            serde_json::json!({"accepted": true, "version": version()}),
        ))
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        let kept = state
            .hub
            .read_studio(|s| s.call_in_of("dougal").cloned())
            .unwrap();
        assert_eq!(kept.consent.by, crate::studio::ConsentBy::Station);
        // Other roles need none.
        let (code, _) = post_json(
            app,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "gregor", "role": "guest"}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn a_caller_without_consent_cannot_be_put_on_air_or_cued() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        for op in ["on_air", "cue"] {
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/desk/commands",
                Some(ADMIN),
                serde_json::json!({"op": op, "identity": "niall", "on": true}),
            )
            .await;
            assert_eq!(code, StatusCode::CONFLICT, "{op}: {body}");
            assert!(body["error"].as_str().unwrap().contains("consent"));
        }
        // Taking him off is always allowed; anyone else needs nothing.
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "on_air", "identity": "niall", "on": false}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "cue", "identity": "eilidh", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        let (code, _) = post_json(app.clone(), "/api/v1/room/tokens", Some(ADMIN),
            serde_json::json!({"identity": "niall", "role": "caller", "consent": {"accepted": true, "version": version()}})).await;
        assert_eq!(code, StatusCode::CREATED);
        assert!(state.hub.read_studio(|s| s.call_in_of("niall").is_some()));
        let (code, v) = post_json(
            app,
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "on_air", "identity": "niall", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["callers"][0]["consent"]["by"], "station");
    }

    #[tokio::test]
    async fn a_pass_rejoins_until_the_caller_hangs_up() {
        let (app, _) = join_app(JoinConfig::default(), true).await;
        let (_, body) = post_json(
            app.clone(),
            "/api/v1/call-in",
            None,
            call(
                "Iona MacIntosh",
                serde_json::json!({"accepted": true, "version": version()}),
            ),
        )
        .await;
        let pass = body["pass"].as_str().unwrap().to_owned();
        let id = body["identity"].as_str().unwrap().to_owned();
        let (code, again) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&pass),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{again}");
        assert_eq!(
            again["identity"],
            id.as_str(),
            "the same identity: the same place in the queue"
        );
        let token = body["token"].as_str().unwrap().to_owned();
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&token),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "a room token is not a pass");
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            None,
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/pass/leave",
            Some(&pass),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&pass),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::GONE, "{body}");
    }

    #[tokio::test]
    async fn the_station_invites_a_guest_a_dj_or_a_presenter() {
        let (app, _) = join_app(JoinConfig::default(), true).await;
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/invites",
            None,
            serde_json::json!({"role": "guest"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        for role in ["caller", "listener", "screener"] {
            let (code, _) = post_json(
                app.clone(),
                "/api/v1/invites",
                Some(ADMIN),
                serde_json::json!({"role": role}),
            )
            .await;
            assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{role}");
        }
        let (code, inv) = post_json(
            app.clone(),
            "/api/v1/invites",
            Some(ADMIN),
            serde_json::json!({"role": "dj", "name": "Finlay Grant", "ttl_s": 999_999}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{inv}");
        let pass = inv["pass"].as_str().unwrap().to_owned();
        assert_eq!(inv["link"], format!("/join/#{pass}"));
        let claims = keys().verify_pass(&pass).unwrap();
        assert_eq!(
            claims.exp - claims.iat,
            14_400,
            "never longer than the station allows"
        );
        let res = app
            .clone()
            .oneshot(
                Request::get("/api/v1/pass")
                    .header("authorization", format!("Bearer {pass}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let info: serde_json::Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(
            (
                info["role"].as_str(),
                info["name"].as_str(),
                info["kind"].as_str()
            ),
            (Some("dj"), Some("Finlay Grant"), Some("invite"))
        );
        let (code, t) = post_json(
            app,
            "/api/v1/pass/token",
            Some(&pass),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
        let claims = keys().verify(t["token"].as_str().unwrap()).unwrap();
        assert_eq!(claims.role(), Some(Role::Dj));
        assert!(
            !claims.video.recorder && !claims.video.hidden,
            "a pass never hears past a publisher's limits (ADR-0118)"
        );
    }

    #[tokio::test]
    async fn the_studio_ends_an_invite_and_it_buys_no_more_tokens() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let invite = |name: &str| {
            let app = app.clone();
            let name = name.to_owned();
            async move {
                let (code, inv) = post_json(
                    app,
                    "/api/v1/invites",
                    Some(ADMIN),
                    serde_json::json!({"role": "guest", "name": name}),
                )
                .await;
                assert_eq!(code, StatusCode::CREATED, "{inv}");
                let pass = inv["pass"].as_str().unwrap().to_owned();
                let id = keys().verify_pass(&pass).unwrap().id;
                (pass, id)
            }
        };
        let token = |pass: String| {
            let app = app.clone();
            async move {
                post_json(
                    app,
                    "/api/v1/pass/token",
                    Some(&pass),
                    serde_json::json!({}),
                )
                .await
            }
        };
        let (leaked, leaked_id) = invite("Morag Ross").await;
        let (other, _) = invite("Gregor Stewart").await;
        assert_eq!(token(leaked.clone()).await.0, StatusCode::CREATED);

        let end = |identity: &str, bearer: Option<&'static str>| {
            post_json(
                app.clone(),
                "/api/v1/passes/end",
                bearer,
                serde_json::json!({ "identity": identity }),
            )
        };
        assert_eq!(end(&leaked_id, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            end("desk-mm-x", Some(ADMIN)).await.0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "never the desk's own"
        );
        let (code, view) = end(&leaked_id, Some(ADMIN)).await;
        assert_eq!(code, StatusCode::OK, "{view}");

        let (code, body) = token(leaked.clone()).await;
        assert_eq!(code, StatusCode::GONE);
        assert_eq!(
            body["error"],
            "The studio has ended this invite. Ask the station for a new link."
        );
        assert!(state.joins.ended(&leaked_id, 0));
        assert_eq!(
            token(other).await.0,
            StatusCode::CREATED,
            "another person's invite still works"
        );
        // A new invite for the same person, made after the end, works.
        state
            .joins
            .restore("guest-later", crate::hub::now_ms() - 5_000);
        assert!(
            !state
                .joins
                .ended("guest-later", crate::hub::now_ms() / 1000 + 1)
        );
    }

    #[tokio::test]
    async fn call_in_waits_for_the_station_to_say_how_to_ask_for_a_takedown() {
        // The default words name {contact}; a station that has not given one takes no calls.
        let join = JoinConfig {
            consent_contact: "   ".into(),
            ..JoinConfig::default()
        };
        let (app, _) = join_app(join, true).await;
        let (code, p) = get_json(app.clone(), "/api/v1/player").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(p["call_in"]["open"], false, "{p}");
        assert!(
            p.to_string()
                .contains("how to ask for a call to be taken down"),
            "{p}"
        );
        let (code, _) = post_json(
            app,
            "/api/v1/call-in",
            None,
            call("Morag Ross", serde_json::json!(null)),
        )
        .await;
        assert_ne!(code, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn call_in_is_closed_without_a_room_or_when_turned_off_and_is_rate_limited() {
        let (app, _) = join_app(JoinConfig::default(), false).await;
        let (_, p) = get_json(app.clone(), "/api/v1/player").await;
        assert_eq!(p["call_in"]["open"], false);
        let (code, _) = post_json(
            app,
            "/api/v1/call-in",
            None,
            call(
                "Jessie",
                serde_json::json!({"accepted": true, "version": version()}),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        let off = JoinConfig {
            call_in: false,
            ..JoinConfig::default()
        };
        let (app, _) = join_app(off, true).await;
        let (_, p) = get_json(app, "/api/v1/player").await;
        assert!(
            p["call_in"]["why_not"]
                .as_str()
                .unwrap()
                .contains("does not take calls")
        );
        let tight = JoinConfig {
            calls_per_minute: 1,
            ..JoinConfig::default()
        };
        let (app, _) = join_app(tight, true).await;
        let ok = serde_json::json!({"accepted": true, "version": version()});
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/call-in",
            None,
            call("Jessie", ok.clone()),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
        let (code, _) = post_json(app, "/api/v1/call-in", None, call("Iona", ok)).await;
        assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn a_pass_follows_its_own_state_only() {
        let (app, _) = join_app(JoinConfig::default(), true).await;
        let (_, inv) = post_json(
            app.clone(),
            "/api/v1/invites",
            Some(ADMIN),
            serde_json::json!({"role": "guest", "name": "Gregor"}),
        )
        .await;
        let res = app
            .oneshot(
                Request::get("/api/v1/pass/events")
                    .header(
                        "authorization",
                        format!("Bearer {}", inv["pass"].as_str().unwrap()),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let mut body = res.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(text.starts_with("event: me\ndata: {"), "{text}");
        assert!(
            text.contains("\"phase\":\"joining\""),
            "the desk has not seen the guest: {text}"
        );
        assert!(text.contains("\"presenter\":\"Eilidh Fraser-MacLeod\""));
        assert!(!text.contains("Niall"), "nobody else's name");
    }

    async fn get_auth(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(
                Request::get(uri)
                    .header("authorization", format!("Bearer {ADMIN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn the_studio_routes_need_the_admin_bearer() {
        for uri in ["/api/v1/studio", "/api/v1/studio/events"] {
            let (a, _) = app();
            let res = a
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
        for (uri, body) in [
            (
                "/api/v1/desk/commands",
                serde_json::json!({"op": "mute", "identity": "eilidh", "on": true}),
            ),
            (
                "/api/v1/screener/commands",
                serde_json::json!({"op": "send", "identity": "niall"}),
            ),
            (
                "/api/v1/chat/modes",
                serde_json::json!({"op": "influx", "on": true}),
            ),
        ] {
            let (a, _) = app();
            let (code, _) = post_json(a, uri, Some("wrong"), body).await;
            assert_eq!(code, StatusCode::UNAUTHORIZED, "{uri}");
        }
    }

    #[tokio::test]
    async fn without_a_desk_the_view_says_so_and_commands_are_503() {
        let (a, _) = app();
        let (code, v) = get_auth(a, "/api/v1/studio").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(v["desk"]["reachable"], false);
        assert!(
            v["desk"]["error"]
                .as_str()
                .unwrap()
                .contains("not configured")
        );
        let (a, _) = app();
        let (code, body) = post_json(
            a,
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "on_air", "identity": "eilidh", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    }

    #[tokio::test]
    async fn the_view_carries_slots_names_levels_and_the_playout_link() {
        let (code, v) = get_auth(studio_app().await, "/api/v1/studio").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(v["desk"]["reachable"], true);
        assert_eq!(v["desk"]["playout"]["connected"], false);
        assert_eq!(v["show"]["title"], "The Morning Kettle");
        assert_eq!(
            (
                v["listener_delay"]["ms"].as_u64(),
                v["listener_delay"]["measured"].as_bool()
            ),
            (Some(23_000), Some(false)),
            "the 15 s setting plus the desk's 8 s profanity delay, labelled as a setting (ADR-0103, ADR-0165)"
        );
        let src = &v["sources"][0];
        assert_eq!(
            (src["slot"].as_u64(), src["name"].as_str()),
            (Some(1), Some("Eilidh Fraser-MacLeod"))
        );
        assert_eq!(src["lufs"], -23.0);
        assert_eq!(
            v["sources"].as_array().unwrap().len(),
            1,
            "a waiting caller and the screener have no slot"
        );
        assert_eq!(v["seats"]["screener"], "elspeth");
        assert_eq!(v["callers"][0]["state"], "waiting");
    }

    #[tokio::test]
    async fn desk_commands_reach_the_desk_and_come_back_in_the_view() {
        let app = studio_app().await;
        let (code, v) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "mute", "identity": "eilidh", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["sources"][0]["mute"], true);
        let (code, v) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "talk_to", "identity": "elspeth", "to": "niall", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "on_air", "identity": "nobody", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        let (_, v) = get_auth(app.clone(), "/api/v1/studio").await;
        assert_eq!(v["desk"]["delay"]["delay_ms"], 8000);
        assert_eq!(v["desk"]["delay"]["rebuilding"], false);
        // Del Del: the delay empties and rebuilds; a second dump with nothing held is refused.
        let (code, v) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "dump"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["desk"]["delay"]["rebuilding"], true);
        assert_eq!(v["desk"]["delay"]["dumps"], 1);
        assert_eq!(
            v["listener_delay"]["ms"], 15_000,
            "a dump leaves the profanity delay empty, so only the setting remains"
        );
        assert!(v["desk"]["delay"]["dumped_at_ms"].is_u64());
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "dump"}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        let (code, _) = post_json(
            app,
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "fade_all"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn the_screener_sends_a_caller_the_desk_declines_and_the_screener_drops() {
        let app = studio_app().await;
        let say = |op: serde_json::Value| {
            post_json(app.clone(), "/api/v1/screener/commands", Some(ADMIN), op)
        };
        let (code, v) = say(serde_json::json!({"op": "screen", "identity": "niall"})).await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["callers"][0]["state"], "screening");
        let (_, v) = say(serde_json::json!({"op": "notes", "identity": "niall", "line": "Night porter", "name_on_air": "Niall"})).await;
        assert_eq!(v["callers"][0]["line"], "Night porter");
        let (code, body) = say(serde_json::json!({"op": "send", "identity": "niall"})).await;
        assert_eq!(code, StatusCode::CONFLICT, "no consent on file: {body}");
        // Consent given on his behalf by the station (the admin token route's way).
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "niall", "role": "caller", "name": "Niall Anderson",
                "consent": {"accepted": true, "version": version()}}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        let (_, _) = say(serde_json::json!({"op": "send", "identity": "niall"})).await;
        // The next poll seats him on strip 4.
        let (_, v) = get_auth(app.clone(), "/api/v1/studio").await;
        let _ = v;
        let (code, v) = say(
            serde_json::json!({"op": "decline", "identity": "niall", "reason": "in a two-way"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["desk_taking_callers"], false);
        assert_eq!(v["callers"][0]["declined"]["reason"], "in a two-way");
        let (code, _) = say(serde_json::json!({"op": "release", "identity": "niall"})).await;
        assert_eq!(
            code,
            StatusCode::CONFLICT,
            "a caller on the line has no held place to let go"
        );
        let (code, _) = say(serde_json::json!({"op": "send", "identity": "nobody"})).await;
        assert_eq!(code, StatusCode::NOT_FOUND);
        let (code, v) = say(serde_json::json!({"op": "drop", "identity": "niall"})).await;
        assert_eq!(code, StatusCode::OK, "{v}");
        assert!(v["callers"].as_array().unwrap().is_empty());
        let (code, _) = say(serde_json::json!({"op": "drop", "identity": "eilidh"})).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "only a caller can be dropped");
    }

    #[tokio::test]
    async fn influx_is_one_command_and_its_undo_another() {
        let app = studio_app().await;
        let (_, v) = post_json(
            app.clone(),
            "/api/v1/chat/modes",
            Some(ADMIN),
            serde_json::json!({"op": "mode", "mode": "links_trusted", "on": true}),
        )
        .await;
        assert_eq!(v["chat"]["links_trusted"], true);
        let (code, v) = post_json(
            app.clone(),
            "/api/v1/chat/modes",
            Some(ADMIN),
            serde_json::json!({"op": "influx", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(
            (
                v["chat"]["influx"].as_bool(),
                v["chat"]["slow"].as_bool(),
                v["chat"]["accounts_only"].as_bool()
            ),
            (Some(true), Some(true), Some(true))
        );
        assert!(
            v["notices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["kind"] == "chat.influx")
        );
        let (_, v) = post_json(
            app,
            "/api/v1/chat/modes",
            Some(ADMIN),
            serde_json::json!({"op": "influx", "on": false}),
        )
        .await;
        assert_eq!(
            (
                v["chat"]["influx"].as_bool(),
                v["chat"]["slow"].as_bool(),
                v["chat"]["links_trusted"].as_bool()
            ),
            (Some(false), Some(false), Some(true))
        );
    }

    #[tokio::test]
    async fn the_event_stream_opens_with_the_current_view() {
        let res = studio_app()
            .await
            .oneshot(
                Request::get("/api/v1/studio/events")
                    .header("authorization", format!("Bearer {ADMIN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/event-stream");
        let mut body = res.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(text.starts_with("event: state\ndata: {"), "{text}");
        assert!(text.contains("Eilidh Fraser-MacLeod"));
    }

    #[tokio::test]
    async fn an_unknown_route_is_404() {
        let (app, _) = app();
        let res = app
            .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
