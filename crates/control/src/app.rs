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
//! | `POST /api/v1/show/end` | End show: everyone off air, the carts faded, then the air to automation once the profanity delay has played out (ADR-0272) |
//! | `POST /api/v1/playout/mode` | hand the air to automation (`auto`) or take it back (`live`) without ending anything (ADR-0272) |
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

use crate::carts::{CartCommand, CartMeta, CartProblem, Owner};
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
        .route("/api/v1/show/end", post(end_show))
        .route("/api/v1/playout/mode", post(set_mode))
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
                    staff_first,
                )),
        )
        // A show's own banks (1.0.2): the same, for the show's presenter, a producer or an admin.
        .route(
            "/api/v1/shows/{show}/carts/banks/{number}",
            put(put_show_bank).delete(delete_show_bank),
        )
        .route(
            "/api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}",
            put(import_show_cart)
                .delete(delete_show_cart)
                .layer(DefaultBodyLimit::max(upload_limit))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    staff_first,
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
        // rtmp-out: RTMP out's destinations (ADR-0230).
        .merge(crate::rtmp::routes())
        // The beginner steps (ADR-0271): a show's step.
        .merge(crate::steps::routes())
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

/// The staff check as middleware, for routes whose extractor reads a large body: a handler's
/// own check runs only after `Bytes` has buffered the whole upload (64 or 200 MB), so a
/// stranger could fill memory before being refused (review 0025, P1). It asks only for a
/// member of staff or the station key; the handler then checks what its route needs (it was
/// called `admin_first`, which said more than it checked: issue 7).
pub(crate) async fn staff_first(
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
    /// The station's name in force: the one set on the setup page, else the settings file's
    /// (ADR-0237).
    pub fn station_name(&self) -> String {
        self.hub.live.name()
    }

    /// The takedown contact in force: the one set on the setup page, else the settings
    /// file's `join.consent_contact` (ADR-0237).
    pub fn consent_contact(&self) -> String {
        self.hub
            .live
            .contact_set()
            .unwrap_or_else(|| self.join.consent_contact.clone())
    }

    /// The studio's view for one account: the shared view, with whether this account may change
    /// the step filled in (ADR-0271).
    pub fn view_for(&self, actor: &crate::staff::Actor) -> crate::studio::View {
        let mut v = (*self.hub.view()).clone();
        v.step.may_change = crate::steps::may_change(
            actor,
            self.hub.read_studio(|s| s.step_policy().presenter_account),
        );
        v
    }

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

/// As [`refusal`], for what changes what is on air (ADR-0276): a staff session whose window is
/// preparing a show is refused with 409. The station key and API tokens are never preparing.
pub(crate) fn on_air_refusal(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    state.check(headers, crate::staff::Need::OnAir).err()
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
    description = "A short-lived LiveKit token for one person, with their role in it for the desk to read. A `listener` token is a monitor that hears every return (ADR-0118): only an admin may ask for one (403 otherwise). A `caller` token records the caller's consent on the station's behalf, against the words the station shows now (`consent`). A `guest`, `dj`, `screener` or `caller` token for a seat the show on air's step has switched off is refused (409, ADR-0271), unless that person is in the room already.",
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
    // A seat the show on air's step has switched off (ADR-0271). The presenter's seat is the
    // studio's own and stays; anyone the desk already lists is never sent away.
    {
        let policy = state.hub.read_studio(|s| s.step_policy());
        let in_room = state
            .hub
            .desk_status()
            .is_some_and(|d| d.people.iter().any(|p| p.identity == req.identity));
        if !in_room
            && req.role != Role::Presenter
            && let Some(hidden) = crate::steps::hidden_seat(policy.step, req.role, false)
        {
            return problem(StatusCode::CONFLICT, hidden.when_making(policy.step));
        }
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
    let actor = match state.check(&headers, crate::staff::Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    Json(state.view_for(&actor)).into_response()
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
    let actor = match state.check(&headers, crate::staff::Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    // The view is shared; whether this account may change the step is its own (ADR-0271).
    let events = state.clone();
    let stream = WatchStream::new(state.hub.subscribe()).map(move |view| {
        let mut view = (*view).clone();
        view.step.may_change = crate::steps::may_change(
            &actor,
            events
                .hub
                .read_studio(|s| s.step_policy().presenter_account),
        );
        Ok::<_, Infallible>(
            Event::default()
                .event("state")
                .json_data(&view)
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
    let actor = match state.check(&headers, crate::staff::Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
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
    Json(state.view_for(&actor)).into_response()
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
    description = "The same requests as the desk's control socket, without `status`: `on_air`, `mute`, `cue`, `talk`, `talk_to`, or `dump` the profanity delay. Answers with the new studio view. A caller cannot go on air or in cue without their consent to record on file (409). The show on air's step switches some off (409, ADR-0271): `dump` below `callers`, the handover (the next DJ going on air) and `picture` below `full`, where the picture is held on the slate.",
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
    let actor = match state.check(&headers, crate::staff::Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
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
    // What the show on air's step has switched off (ADR-0271): Dump below Callers, the
    // handover and the picture's modes below Full. The station's own holding of the slate does
    // not come through here.
    let step = state.hub.read_studio(|s| s.step_policy().step);
    let target_is_dj = match &cmd {
        DeskCommand::OnAir { identity, .. } => state
            .hub
            .desk_status()
            .and_then(|d| d.people.into_iter().find(|p| &p.identity == identity))
            .is_some_and(|p| p.role == Role::Dj),
        _ => false,
    };
    if let Some(why) = crate::steps::command_refusal(step, &cmd, target_is_dj) {
        return problem(StatusCode::CONFLICT, why);
    }
    match state.hub.command(&cmd).await {
        Ok(()) => Json(state.view_for(&actor)).into_response(),
        Err(e) => desk_problem(&e),
    }
}

/// What End show did, and the view after it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EndShowResult {
    /// The new studio view, as every command answers.
    pub view: crate::studio::View,
    /// How many people and sources were taken off air.
    pub off_air: u32,
    /// When the air goes to automation: `0` it already was there; a number of milliseconds is
    /// how long until the profanity delay has played out (the goodbye is not cut off) and the
    /// station is switched; `null` this station has no playout layer to hand the air to, so
    /// everyone is off air and nothing more happened.
    pub automation_in_ms: Option<u64>,
}

/// How long past the profanity delay's last held second automation waits, so the delay's tail
/// has left the desk and crossed the SRT link before the switch.
const END_SHOW_GRACE_MS: u64 = 1_000;

#[utoipa::path(
    post,
    path = "/api/v1/show/end",
    operation_id = "end_show",
    tag = "desk",
    summary = "End the show",
    description = "Takes everyone off air (presenters, DJs, guests, callers; callers stay in the room and the queue), fades every cart, then hands the air to automation by setting the station's mode to `auto`. The switch waits for the profanity delay to play out, so the last words are heard; `automation_in_ms` says how long. If somebody has gone back on air by then the switch is skipped, and the studio gets a notice saying why. With no desk answering nothing is on air to take off, and the air still goes to automation. With no playout layer (`automation_in_ms` null) there is nothing to hand it to. Any staff member's, like the desk's own switches.",
    responses(
        (status = 200, description = "What was done, and the new studio view.", body = EndShowResult),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn end_show(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = on_air_refusal(&state, &headers) {
        return r;
    }
    let on_air: Vec<String> = state
        .hub
        .desk_status()
        .map(|d| {
            d.people
                .into_iter()
                .filter(|p| p.on_air)
                .map(|p| p.identity)
                .collect()
        })
        .unwrap_or_default();
    for identity in &on_air {
        let cmd = DeskCommand::OnAir {
            identity: identity.clone(),
            on: false,
        };
        if let Err(e) = state.hub.command(&cmd).await {
            return desk_problem(&e);
        }
    }
    // A bed or a sweeper must not carry on over a show that has ended. A desk without a cart
    // wall has nothing to fade.
    if let Err(e) = state.hub.cart_command(&CartCommand::FadeAll).await {
        tracing::debug!(error = %e, "End show: no carts faded");
    }
    let hold_ms = state
        .hub
        .desk_status()
        .and_then(|d| d.delay)
        .map_or(0, |d| u64::from(d.delay_ms));
    let automation_in_ms = match state.hub.onair.get().cloned() {
        None => None,
        Some(_)
            if state.hub.read_studio(|s| s.station_mode())
                == Some(crate::desk::StationMode::Auto) =>
        {
            Some(0)
        }
        Some(onair) => {
            let wait = hold_ms + END_SHOW_GRACE_MS;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                if let Err(e) = onair.hand_over_after_end_show().await {
                    tracing::warn!(error = %e, "End show could not hand the air to automation");
                }
            });
            Some(wait)
        }
    };
    tracing::info!(
        off_air = on_air.len(),
        ?automation_in_ms,
        "the show was ended"
    );
    Json(EndShowResult {
        view: (*state.hub.view()).clone(),
        off_air: on_air.len() as u32,
        automation_in_ms,
    })
    .into_response()
}

/// The body of `POST /api/v1/playout/mode`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeRequest {
    /// `auto`: automation holds the air and the desk never airs (the lamp says AUTO, ADR-0183).
    /// `live`: the desk first, automation on dead air.
    pub mode: crate::desk::StationMode,
}

#[utoipa::path(
    post,
    path = "/api/v1/playout/mode",
    operation_id = "set_mode",
    tag = "playout",
    summary = "Hand the air to automation, or take it back",
    description = "Sets the station's mode (ADR-0045) without changing any other playout setting, and answers with the new studio view. Any staff member's: `PUT /api/v1/playout/settings` stays an admin's. Anyone on air at the desk is not taken off air; in `auto` the desk is simply not heard, and in `live` it is heard again at once. The change is immediate (profanity delay and all); End show (`POST /api/v1/show/end`) is the route that takes people off air and waits for the delay. 503 when the station has no playout layer.",
    request_body = ModeRequest,
    responses(
        (status = 200, description = "The new studio view.", body = crate::studio::View),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn set_mode(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ModeRequest>,
) -> Response {
    if let Some(r) = on_air_refusal(&state, &headers) {
        return r;
    }
    let Some(onair) = state.hub.onair.get() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the station's mode needs the database and playout's socket",
        );
    };
    match onair.set_mode(req.mode).await {
        Ok(()) => Json(state.hub.view()).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "the mode was not saved");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("the database refused: {e}"),
            )
        }
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
    let actor = match state.check(&headers, crate::staff::Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
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
        Ok(()) => Json(state.view_for(&actor)).into_response(),
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
    let actor = match state.check(&headers, crate::staff::Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    state.hub.with_studio(|s, now| match cmd {
        ChatCommand::Mode { mode, on } => s.set_mode(mode, on),
        ChatCommand::Influx { on } => s.influx(on, now),
    });
    Json(state.view_for(&actor)).into_response()
}

fn cart_problem(e: &CartProblem) -> Response {
    let code = match e {
        CartProblem::NoCart(_) | CartProblem::NoBank(_) => StatusCode::NOT_FOUND,
        CartProblem::Missing(_)
        | CartProblem::AlreadySounding(_)
        | CartProblem::OnAir(_)
        | CartProblem::OtherShow(_)
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

/// Which show's own banks to list beside the station's.
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct CatalogueQuery {
    /// A show's slug: its own banks and carts come beside the station's. Left out, the
    /// station's alone.
    #[serde(default)]
    show: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/carts",
    operation_id = "carts_catalogue",
    tag = "carts",
    summary = "The banks and carts",
    description = "The station's banks and carts, which every show shares, and with `show` that show's own beside them (each owner's banks are numbered 1 to 5 apart; a bank or cart names its show, or `null` for the station's). A studio window asks for its show: a live window the show the station's step comes from, a preparing window the show it prepares. Each cart with FILE MISSING. Fetch it again when the studio view's `carts.version` moves.",
    params(CatalogueQuery),
    responses(
        (status = 200, description = "The catalogue.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn carts_catalogue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<CatalogueQuery>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let show = q.show.filter(|s| !s.is_empty());
    Json(state.hub.catalogue(show.as_deref())).into_response()
}

/// Fire, stop, stop the last, fade all. Answers with the studio's view, as the desk's do.
#[utoipa::path(
    post,
    path = "/api/v1/carts/commands",
    operation_id = "cart_command",
    tag = "carts",
    summary = "Fire, stop or cue a cart",
    description = "`toggle`, `fire`, `stop`, `cue` a cart; `stop_last`; `fade_all`; `clear_cue`. Carts sound in the desk (ADR-0117). A cart in a show's own bank starts only while that show is the one the station's step comes from (409 otherwise); anything sounding can be stopped. Answers with the new studio view.",
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
    let actor = match state.check(&headers, crate::staff::Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.hub.cart_command(&cmd).await {
        Ok(()) => Json(state.view_for(&actor)).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct BankRequest {
    name: String,
    colour_slot: u8,
}

impl BankRequest {
    fn bank(self, number: u8) -> weavesignal_model::carts::Bank {
        weavesignal_model::carts::Bank {
            number,
            name: self.name.trim().to_owned(),
            colour_slot: self.colour_slot,
            show: None,
        }
    }
}

/// The show a show's cart routes name, and whether this bearer may change its banks: the
/// station key, an admin, a producer or the show's presenter, as for its step (ADR-0271). A
/// preparing window may: its own show's banks are not on air.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn show_owner(state: &AppState, headers: &HeaderMap, slug: &str) -> Result<Owner, Response> {
    let actor = state.check(headers, crate::staff::Need::Staff)?;
    let Some(pool) = &state.db else {
        return Err(cart_problem(&CartProblem::NoDatabase));
    };
    let show = match weavesignal_model::shows::get_show(pool, state.station.id, slug).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err(problem(
                StatusCode::NOT_FOUND,
                format!("show `{slug}` does not exist"),
            ));
        }
        Err(e) => return Err(cart_problem(&CartProblem::from(e))),
    };
    if !crate::steps::may_change(&actor, show.presenter_account) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Only an admin, a producer or the show's presenter changes its cart banks.",
        ));
    }
    Ok(Owner::Show {
        id: show.id,
        slug: show.slug,
    })
}

#[utoipa::path(
    put,
    path = "/api/v1/carts/banks/{number}",
    operation_id = "put_bank",
    tag = "carts",
    summary = "Make or rename a station bank",
    description = "A bank of the station's, shared by every show. A show's own banks are `/api/v1/shows/{show}/carts/banks/{number}`. Answers with the station's catalogue.",
    params(("number" = u8, Path, description = "The bank's number, 1 to 5")),
    request_body = BankRequest,
    responses(
        (status = 200, description = "The station's catalogue.", body = crate::carts::Catalogue),
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
    match state.hub.put_bank(&Owner::Station, req.bank(number)).await {
        Ok(()) => Json(state.hub.catalogue(None)).into_response(),
        Err(e) => cart_problem(&e),
    }
}

/// An audio file as the body, the cart's words in the query: the media worker makes it a cart.
#[utoipa::path(
    put,
    path = "/api/v1/carts/banks/{bank}/slots/{slot}",
    operation_id = "import_cart",
    tag = "carts",
    summary = "Put an audio file in a station slot",
    description = "An audio file as the body, the cart's words in the query. The media worker reads it in its sandbox, brings it to the station's loudness and makes it the cart in that slot of the station's bank.",
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
    import_into(&state, &Owner::Station, bank, slot, meta, body).await
}

async fn import_into(
    state: &AppState,
    owner: &Owner,
    bank: u8,
    slot: u8,
    meta: CartMeta,
    body: Bytes,
) -> Response {
    match state
        .hub
        .import_cart(owner, bank, slot, meta, body.to_vec())
        .await
    {
        Ok(cart) => (StatusCode::CREATED, Json(cart)).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/carts/banks/{bank}/slots/{slot}",
    operation_id = "delete_cart",
    tag = "carts",
    summary = "Empty a station slot",
    params(("bank" = u8, Path, description = "The bank's number"), ("slot" = u8, Path, description = "The slot in the bank")),
    responses(
        (status = 200, description = "The station's catalogue.", body = crate::carts::Catalogue),
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
    match state.hub.delete_cart(&Owner::Station, bank, slot).await {
        Ok(()) => Json(state.hub.catalogue(None)).into_response(),
        Err(e) => cart_problem(&e),
    }
}

// ---- a show's own banks (1.0.2) ----

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/carts/banks/{number}",
    operation_id = "put_show_bank",
    tag = "carts",
    summary = "Make or rename a show's own bank",
    description = "A bank of the show's own, beside the station's: numbered 1 to 5 apart from the station's, and changed without touching another show's or the station's. In that show's studio windows its bank takes the place of the station's bank of the same number. The station key, an admin, a producer or the show's presenter may, from a preparing window too; an API token never. Answers with the station's catalogue and the show's.",
    params(("show" = String, Path, description = "The show's slug"), ("number" = u8, Path, description = "The bank's number, 1 to 5")),
    request_body = BankRequest,
    responses(
        (status = 200, description = "The station's catalogue and the show's.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_show_bank(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((show, number)): Path<(String, u8)>,
    Json(req): Json<BankRequest>,
) -> Response {
    let owner = match show_owner(&state, &headers, &show).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    match state.hub.put_bank(&owner, req.bank(number)).await {
        Ok(()) => Json(state.hub.catalogue(owner.slug())).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/shows/{show}/carts/banks/{number}",
    operation_id = "delete_show_bank",
    tag = "carts",
    summary = "Remove a show's own bank",
    description = "Removes the bank with its carts and their files; a cart of it that is sounding is stopped. In the show's windows the station's bank of that number, if made, is back in its place. Who may: as for making it.",
    params(("show" = String, Path, description = "The show's slug"), ("number" = u8, Path, description = "The bank's number, 1 to 5")),
    responses(
        (status = 200, description = "The station's catalogue and the show's.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_show_bank(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((show, number)): Path<(String, u8)>,
) -> Response {
    let owner = match show_owner(&state, &headers, &show).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    match state.hub.delete_show_bank(&owner, number).await {
        Ok(()) => Json(state.hub.catalogue(owner.slug())).into_response(),
        Err(e) => cart_problem(&e),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}",
    operation_id = "import_show_cart",
    tag = "carts",
    summary = "Put an audio file in a show's slot",
    description = "As `PUT /api/v1/carts/banks/{bank}/slots/{slot}`, into the show's own bank. Who may: as for the show's banks.",
    params(("show" = String, Path, description = "The show's slug"), ("bank" = u8, Path, description = "The bank's number"), ("slot" = u8, Path, description = "The slot in the bank"), CartMeta),
    request_body(content = crate::openapi::File, content_type = "application/octet-stream", description = "The audio file (any format the media worker reads), at most `carts.max_upload_mb`."),
    responses(
        (status = 201, description = "The cart.", body = weavesignal_model::carts::Cart),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 413, description = "The file is larger than `carts.max_upload_mb`.", body = crate::openapi::Problem),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn import_show_cart(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((show, bank, slot)): Path<(String, u8, u8)>,
    Query(meta): Query<CartMeta>,
    body: Bytes,
) -> Response {
    let owner = match show_owner(&state, &headers, &show).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    import_into(&state, &owner, bank, slot, meta, body).await
}

#[utoipa::path(
    delete,
    path = "/api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}",
    operation_id = "delete_show_cart",
    tag = "carts",
    summary = "Empty a show's slot",
    params(("show" = String, Path, description = "The show's slug"), ("bank" = u8, Path, description = "The bank's number"), ("slot" = u8, Path, description = "The slot in the bank")),
    responses(
        (status = 200, description = "The station's catalogue and the show's.", body = crate::carts::Catalogue),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The cart wall failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_show_cart(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((show, bank, slot)): Path<(String, u8, u8)>,
) -> Response {
    let owner = match show_owner(&state, &headers, &show).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    match state.hub.delete_cart(&owner, bank, slot).await {
        Ok(()) => Json(state.hub.catalogue(owner.slug())).into_response(),
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
    description = "The station's identifier and its name in force: the one set on the setup page, else the settings file's (ADR-0237).",
    responses(
        (status = 200, description = "The station.", body = Station),
    ),
    security(()),
)]
async fn station(State(state): State<Arc<AppState>>) -> Json<Station> {
    Json(Station {
        id: state.station.id,
        name: state.station_name(),
    })
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
    end_show,
    set_mode,
    screener_command,
    end_pass,
    chat_modes,
    carts_catalogue,
    cart_command,
    put_bank,
    import_cart,
    delete_cart,
    put_show_bank,
    delete_show_bank,
    import_show_cart,
    delete_show_cart
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
            show: None,
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
            show: None,
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
        let file = _hub.catalogue(None).carts[0].cart.file.clone().unwrap();
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

    /// A cart wall per show (1.0.2): a show's own cart is listed only with its show, fires only
    /// while that show is the one the step comes from, and once sounding can always be stopped.
    #[tokio::test]
    async fn a_shows_own_cart_fires_only_in_its_show_and_can_always_be_stopped() {
        use weavesignal_model::carts::{Bank, Cart, CartMode};
        let (app, hub, fake, [applause, _, _], dir) = cart_app().await;
        let media = hub.carts.config.media_dir.clone();
        let file = format!("{}.flac", uuid::Uuid::new_v4());
        std::fs::write(media.join(&file), b"fLaC").unwrap();
        let woof = Cart {
            id: uuid::Uuid::new_v4(),
            bank: 1,
            slot: 1,
            label: "Woof".into(),
            detail: String::new(),
            mode: CartMode::OneShot,
            file: Some(file),
            duration_ms: 1_000,
            gain_db: 0.0,
            loudness_lufs: None,
            true_peak_dbtp: None,
            ramp_ms: None,
            origin: String::new(),
            licence: String::new(),
            show: Some("steeped".into()),
        };
        hub.with_inner(|i| {
            let mut banks = i.carts.banks.clone();
            banks.push(Bank {
                number: 1,
                name: "Dog woofs".into(),
                colour_slot: 5,
                show: Some("steeped".into()),
            });
            let mut carts = i.carts.carts.clone();
            carts.push(woof.clone());
            i.carts.replace(banks, carts, &media);
        });
        // Listed with its show only, beside the station's bank 1.
        let (_, cat) = get_admin(app.clone(), "/api/v1/carts").await;
        assert_eq!(cat["banks"].as_array().unwrap().len(), 1);
        assert!(cat["show"].is_null());
        let (_, cat) = get_admin(app.clone(), "/api/v1/carts?show=steeped").await;
        assert_eq!(cat["show"], "steeped");
        let owners: Vec<&serde_json::Value> = cat["banks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| &b["show"])
            .collect();
        assert_eq!(
            owners,
            vec![&serde_json::Value::Null, &serde_json::json!("steeped")]
        );
        assert_eq!(cat["carts"].as_array().unwrap().len(), 4);
        // No show holds the step: Steeped's cart does not fire, and the desk is not asked.
        let fire = serde_json::json!({"op": "toggle", "cart": woof.id});
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/carts/commands",
            Some(ADMIN),
            fire.clone(),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("another show"),
            "{body}"
        );
        assert!(fake.lock().unwrap().ops().is_empty());
        // The station's carts fire in any show.
        let station = serde_json::json!({"op": "fire", "cart": applause});
        let (code, _) =
            post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), station).await;
        assert_eq!(code, StatusCode::OK);
        // Steeped takes the air: its cart fires, and the view names it for every window.
        let show_of = |slug: &str, name: &str| {
            let mut s = crate::steps::test_show(crate::steps::Step::Full, None);
            s.slug = slug.into();
            s.name = name.into();
            s
        };
        let steeped = show_of("steeped", "Steeped");
        hub.with_studio(|s, _| s.set_step(crate::steps::facts_from(Some(&steeped), true, None)));
        let (code, view) = post_json(
            app.clone(),
            "/api/v1/carts/commands",
            Some(ADMIN),
            fire.clone(),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{view}");
        let sounding = view["carts"]["sounding"].as_array().unwrap();
        assert!(sounding.iter().any(|s| s["label"] == "Woof"), "{view}");
        // The next show takes the air while it sounds: it can still be stopped.
        let kettle = show_of("morning-kettle", "The Morning Kettle");
        hub.with_studio(|s, _| s.set_step(crate::steps::facts_from(Some(&kettle), true, None)));
        let (code, view) = post_json(app, "/api/v1/carts/commands", Some(ADMIN), fire).await;
        assert_eq!(code, StatusCode::OK, "{view}");
        assert_eq!(
            fake.lock().unwrap().ops(),
            vec!["cart_fire", "cart_fire", "cart_stop"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn on_air_person(identity: &str, role: &str) -> serde_json::Value {
        serde_json::json!({"identity": identity, "name": identity, "role": role, "on_air": true,
            "mute": false, "cue": false, "talk": false, "present": true, "linked": true})
    }

    #[tokio::test]
    async fn end_show_takes_everyone_off_air_and_fades_the_carts() {
        let (app, hub, fake, [applause, _, bed], dir) = cart_app().await;
        for c in [bed, applause] {
            let cmd = serde_json::json!({"op": "fire", "cart": c});
            let (code, _) =
                post_json(app.clone(), "/api/v1/carts/commands", Some(ADMIN), cmd).await;
            assert_eq!(code, StatusCode::OK);
        }
        {
            let mut f = fake.lock().unwrap();
            f.people = vec![
                on_air_person("eilidh", "presenter"),
                on_air_person("niall", "caller"),
            ];
            f.people.push({
                let mut quiet = on_air_person("morag", "guest");
                quiet["on_air"] = serde_json::json!(false);
                quiet
            });
            f.delay_ms = Some(3_000);
        }
        hub.poll_once().await;

        // A bearer is needed, like every desk switch.
        let (code, _) =
            post_json(app.clone(), "/api/v1/show/end", None, serde_json::json!({})).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);

        let (code, body) = post_json(
            app.clone(),
            "/api/v1/show/end",
            Some(ADMIN),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(
            body["off_air"], 2,
            "the two on air, not the one already off"
        );
        // This test station has no playout layer: nothing to hand the air to, and it says so.
        assert_eq!(body["automation_in_ms"], serde_json::Value::Null);
        assert!(body["view"]["sources"].is_array());
        {
            let f = fake.lock().unwrap();
            assert!(
                f.people.iter().all(|p| p["on_air"] == false),
                "{:?}",
                f.people
            );
            let ops = f.ops();
            assert_eq!(ops.iter().filter(|o| *o == "on_air").count(), 2, "{ops:?}");
            assert!(
                ops.contains(&"cart_stop_all".to_owned()),
                "carts faded: {ops:?}"
            );
        }
        assert!(
            hub.carts_view().sounding.iter().all(|s| s.stopping),
            "every cart is fading"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn end_show_with_no_desk_answering_still_answers() {
        let (app, _) = app();
        let (code, body) =
            post_json(app, "/api/v1/show/end", Some(ADMIN), serde_json::json!({})).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["off_air"], 0);
        assert_eq!(body["automation_in_ms"], serde_json::Value::Null);
    }

    async fn put_json_as(
        app: &Router,
        uri: &str,
        bearer: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let res = app
            .clone()
            .oneshot(
                Request::put(uri)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    /// ADR-0276: a window preparing a show cannot change what is on air, and can again once it
    /// is live; the station key is always live.
    #[tokio::test]
    async fn a_preparing_window_cannot_change_what_is_on_air_until_it_is_live() {
        let station = Station::new(StationId::new_random(), "Kettle FM").unwrap();
        let state = Arc::new(AppState {
            station,
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
        let app = router(state.clone());
        let me = state
            .staff
            .signed_in_for_test(weavesignal_model::staff::Role::Presenter);

        // A session starts live, and says so.
        let (code, body) = get_with(&app, "/api/v1/sessions/me", &me).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["preparing"], false);

        // It prepares: `me` says so, and every on-air route refuses it.
        let (code, body) = put_json_as(
            &app,
            "/api/v1/sessions/me/mode",
            &me,
            serde_json::json!({"preparing": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["preparing"], true);
        assert_eq!(body["kind"], "staff");
        let (_, body) = get_with(&app, "/api/v1/sessions/me", &me).await;
        assert_eq!(body["preparing"], true);

        let on_air_routes = [
            (
                "/api/v1/desk/commands",
                serde_json::json!({"op": "mute", "identity": "morag", "on": true}),
            ),
            ("/api/v1/show/end", serde_json::json!({})),
            ("/api/v1/playout/mode", serde_json::json!({"mode": "auto"})),
            (
                "/api/v1/carts/commands",
                serde_json::json!({"op": "fade_all"}),
            ),
            (
                "/api/v1/screener/commands",
                serde_json::json!({"op": "screen", "identity": "morag"}),
            ),
            (
                "/api/v1/passes/end",
                serde_json::json!({"identity": "morag"}),
            ),
        ];
        for (uri, req) in &on_air_routes {
            let (code, body) = post_json(app.clone(), uri, Some(&me), req.clone()).await;
            assert_eq!(code, StatusCode::CONFLICT, "{uri}: {body}");
            assert_eq!(body["error"], crate::staff::PREPARING_REFUSAL, "{uri}");
        }
        // Reading the live desk stays open to it, and so does the station key.
        let (code, _) = get_with(&app, "/api/v1/studio", &me).await;
        assert_eq!(code, StatusCode::OK);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/show/end",
            Some(ADMIN),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");

        // Back to live: the same routes are no longer refused on that account (there is no desk
        // in this test, so the answer is whatever the desk says, not 409 from the mode).
        let (code, body) = put_json_as(
            &app,
            "/api/v1/sessions/me/mode",
            &me,
            serde_json::json!({"preparing": false}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["preparing"], false);
        for (uri, req) in &on_air_routes {
            let (_, body) = post_json(app.clone(), uri, Some(&me), req.clone()).await;
            assert_ne!(
                body["error"],
                crate::staff::PREPARING_REFUSAL,
                "{uri}: {body}"
            );
        }
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/show/end",
            Some(&me),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");

        // The station key has no window to prepare.
        let (code, body) = put_json_as(
            &app,
            "/api/v1/sessions/me/mode",
            ADMIN,
            serde_json::json!({"preparing": true}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["error"], "The station key is always live.");
        let (_, body) = get_with(&app, "/api/v1/sessions/me", ADMIN).await;
        assert_eq!(body["preparing"], false);
        assert_eq!(body["kind"], "station");
        // A mode needs the one field and nothing else.
        let (code, _) = put_json_as(
            &app,
            "/api/v1/sessions/me/mode",
            &me,
            serde_json::json!({"preparing": true, "extra": 1}),
        )
        .await;
        assert!(code.is_client_error(), "{code}");
    }

    #[tokio::test]
    async fn the_mode_route_is_staffs_and_needs_the_playout_layer() {
        let (app, _) = app();
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/playout/mode",
            None,
            serde_json::json!({"mode": "auto"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/playout/mode",
            Some(ADMIN),
            serde_json::json!({"mode": "auto"}),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        // Only the two words, and nothing else in the body.
        for bad in [
            serde_json::json!({"mode": "off"}),
            serde_json::json!({"mode": "auto", "time_zone": "UTC"}),
        ] {
            let (code, _) = post_json(app.clone(), "/api/v1/playout/mode", Some(ADMIN), bad).await;
            assert!(code.is_client_error(), "{code}");
        }
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
            "The studio has ended this invitation. Ask the station for a new link."
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
            (Some(21_000), Some(false)),
            "the 13 s setting plus the desk's 8 s profanity delay, labelled as a setting (ADR-0103, ADR-0165)"
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
            v["listener_delay"]["ms"], 13_000,
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

    // ---- the beginner steps (ADR-0271) ----

    use crate::steps::Step;

    /// The station at `step`, as if a show presented by `presenter` were on air.
    fn at_step(state: &AppState, step: Step, presenter: Option<uuid::Uuid>) {
        let facts =
            crate::steps::facts_from(Some(&crate::steps::test_show(step, presenter)), true, None);
        state.hub.with_studio(|s, _| s.set_step(facts));
    }

    async fn get_with(app: &Router, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
        let res = app
            .clone()
            .oneshot(
                Request::get(uri)
                    .header("authorization", format!("Bearer {bearer}"))
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
    async fn below_callers_call_in_is_closed_and_a_call_cannot_be_queued() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let (_, player) = get_json(app.clone(), "/api/v1/player").await;
        assert_eq!(player["call_in"]["open"], true, "no show yet: full");
        let consent = serde_json::json!({"accepted": true, "version": version()});
        for step in [Step::Solo, Step::Guests] {
            at_step(&state, step, None);
            let (_, player) = get_json(app.clone(), "/api/v1/player").await;
            assert_eq!(player["call_in"]["open"], false, "{step}");
            assert_eq!(
                player["call_in"]["why_not"], "The lines are closed for this show.",
                "{step}"
            );
            // A page opened earlier cannot queue a caller: the request says the same words.
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/call-in",
                None,
                call("Niall", consent.clone()),
            )
            .await;
            assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{step} {body}");
            assert_eq!(body["error"], "The lines are closed for this show.");
        }
        // When the step rises the lines open at once.
        for step in [Step::Callers, Step::Full] {
            at_step(&state, step, None);
            let (_, player) = get_json(app.clone(), "/api/v1/player").await;
            assert_eq!(player["call_in"]["open"], true, "{step}");
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/call-in",
                None,
                call("Niall", consent.clone()),
            )
            .await;
            assert_eq!(code, StatusCode::CREATED, "{step} {body}");
        }
    }

    #[tokio::test]
    async fn dump_and_the_pictures_modes_are_refused_below_their_steps() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        at_step(&state, Step::Solo, None);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "dump"}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .starts_with("Dump is off at Solo"),
            "{body}"
        );
        // The delay was not touched.
        let (_, v) = get_auth(app.clone(), "/api/v1/studio").await;
        assert_eq!(v["desk"]["delay"]["dumps"], 0);
        for step in [Step::Solo, Step::Guests, Step::Callers] {
            at_step(&state, step, None);
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/desk/commands",
                Some(ADMIN),
                serde_json::json!({"op": "picture", "mode": "auto"}),
            )
            .await;
            assert_eq!(code, StatusCode::CONFLICT, "{step} {body}");
        }
        // The profanity delay itself and the other switches are the step's no concern.
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "mute", "identity": "eilidh", "on": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        at_step(&state, Step::Callers, None);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "dump"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        at_step(&state, Step::Full, None);
        let (code, body) = post_json(
            app,
            "/api/v1/desk/commands",
            Some(ADMIN),
            serde_json::json!({"op": "picture", "mode": "auto"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
    }

    #[tokio::test]
    async fn an_invitation_for_a_hidden_seat_is_refused_with_the_field_named() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let ask = |role: &'static str| {
            post_json(
                app.clone(),
                "/api/v1/invites",
                Some(ADMIN),
                serde_json::json!({"role": role}),
            )
        };
        at_step(&state, Step::Solo, None);
        for role in ["guest", "presenter", "dj"] {
            let (code, body) = ask(role).await;
            assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "solo {role} {body}");
            assert_eq!(body["field"], "role", "{body}");
        }
        let (_, body) = ask("dj").await;
        assert_eq!(
            body["error"],
            "An invitation for the next show's DJ comes with Full."
        );
        at_step(&state, Step::Guests, None);
        assert_eq!(ask("guest").await.0, StatusCode::CREATED);
        assert_eq!(ask("presenter").await.0, StatusCode::CREATED);
        assert_eq!(ask("dj").await.0, StatusCode::UNPROCESSABLE_ENTITY);
        at_step(&state, Step::Callers, None);
        assert_eq!(ask("dj").await.0, StatusCode::UNPROCESSABLE_ENTITY);
        at_step(&state, Step::Full, None);
        assert_eq!(ask("dj").await.0, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn a_pass_for_a_hidden_seat_is_refused_when_used_and_stays_good_for_later() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let invite = |role: &'static str| {
            keys()
                .pass(
                    weavesignal_proto::PassKind::Invite,
                    role.parse().unwrap(),
                    &format!("{role}-0a1b2c3d4e5f"),
                    "",
                    600,
                )
                .unwrap()
                .0
        };
        let guest = invite("guest");
        let dj = invite("dj");
        at_step(&state, Step::Solo, None);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&guest),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        assert_eq!(
            body["error"],
            "The studio is not taking guests during this show. Your link still works later: ask whoever sent it when to join."
        );
        // The join page asks first.
        let (code, info) = get_with(&app, "/api/v1/pass", &guest).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(info["ended"], false, "the pass stays good");
        assert!(
            info["not_now"]
                .as_str()
                .unwrap()
                .contains("not taking guests")
        );
        at_step(&state, Step::Guests, None);
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&guest),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&dj),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        assert!(body["error"].as_str().unwrap().contains("next DJ"));
        let (_, info) = get_with(&app, "/api/v1/pass", &guest).await;
        assert!(info["not_now"].is_null(), "nothing in the way at Guests");
        at_step(&state, Step::Full, None);
        let (code, _) =
            post_json(app, "/api/v1/pass/token", Some(&dj), serde_json::json!({})).await;
        assert_eq!(code, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn a_person_already_in_the_room_is_never_sent_away_by_a_step() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        at_step(&state, Step::Solo, None);
        // The fake desk lists "niall" (a caller) and "elspeth" (a screener): they are in the room.
        for (identity, role) in [
            ("elspeth", weavesignal_proto::Role::Screener),
            ("niall", weavesignal_proto::Role::Guest),
        ] {
            let (pass, _) = keys()
                .pass(weavesignal_proto::PassKind::Invite, role, identity, "", 600)
                .unwrap();
            let (code, body) = post_json(
                app.clone(),
                "/api/v1/pass/token",
                Some(&pass),
                serde_json::json!({}),
            )
            .await;
            assert_eq!(code, StatusCode::CREATED, "{identity} {body}");
        }
        // A token for someone the desk does not list is another matter.
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "screener-new", "role": "screener"}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "elspeth", "role": "screener"}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        let (code, body) = post_json(
            app,
            "/api/v1/room/tokens",
            Some(ADMIN),
            serde_json::json!({"identity": "eilidh-2", "role": "presenter"}),
        )
        .await;
        assert_eq!(
            code,
            StatusCode::CREATED,
            "the presenter's seat is the studio's own: {body}"
        );
    }

    #[tokio::test]
    async fn the_solo_djs_own_link_is_allowed_at_every_step_for_the_shows_presenter() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let me = uuid::Uuid::new_v4();
        let stranger = uuid::Uuid::new_v4();
        let mine = state
            .staff
            .signed_in_as_for_test(weavesignal_model::staff::Role::Presenter, me);
        let theirs = state
            .staff
            .signed_in_as_for_test(weavesignal_model::staff::Role::Presenter, stranger);
        at_step(&state, Step::Solo, Some(me));
        let ask = |bearer: &str, body: serde_json::Value| {
            let (app, bearer) = (app.clone(), bearer.to_owned());
            async move { post_json(app, "/api/v1/invites", Some(&bearer), body).await }
        };
        let (code, inv) = ask(&mine, serde_json::json!({"for_me": true})).await;
        assert_eq!(code, StatusCode::CREATED, "{inv}");
        assert_eq!(inv["role"], "presenter");
        assert_eq!(inv["name"], "Morag");
        assert_eq!(inv["identity"], crate::steps::own_identity(me));
        // Used at Solo, it joins: it is the presenter's seat.
        let (code, body) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(inv["pass"].as_str().unwrap()),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
        // A second presenter is a guest's seat, which Solo has not got.
        let (code, body) = ask(&mine, serde_json::json!({"role": "presenter"})).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        // Another account cannot make the link while the show runs at Solo, and the station
        // key has no account to make it for.
        let (code, body) = ask(&theirs, serde_json::json!({"for_me": true})).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
        let (code, body) = ask(ADMIN, serde_json::json!({"for_me": true})).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        // The link cannot be turned into another role.
        let (code, _) = ask(&mine, serde_json::json!({"for_me": true, "role": "guest"})).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        // And one made for another presenter is not this show's presenter's.
        let (other, _) = keys()
            .pass(
                weavesignal_proto::PassKind::Invite,
                Role::Presenter,
                &crate::steps::own_identity(stranger),
                "",
                600,
            )
            .unwrap();
        let (code, _) = post_json(
            app.clone(),
            "/api/v1/pass/token",
            Some(&other),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT);
        // From Guests on, any member of staff may make one.
        at_step(&state, Step::Guests, Some(me));
        let (code, body) = ask(&theirs, serde_json::json!({"for_me": true})).await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
    }

    #[tokio::test]
    async fn the_view_says_whether_this_account_may_change_the_step() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        use weavesignal_model::staff::Role as Staff;
        let presenter = uuid::Uuid::new_v4();
        at_step(&state, Step::Solo, Some(presenter));
        assert!(
            !state.hub.view().step.may_change,
            "the shared view cannot say it for anyone"
        );
        let sessions = [
            (ADMIN.to_owned(), true),
            (state.staff.signed_in_for_test(Staff::Admin), true),
            (state.staff.signed_in_for_test(Staff::Producer), true),
            (
                state
                    .staff
                    .signed_in_as_for_test(Staff::Presenter, presenter),
                true,
            ),
            (state.staff.signed_in_for_test(Staff::Presenter), false),
            (state.staff.signed_in_for_test(Staff::Screener), false),
        ];
        for (bearer, may) in sessions {
            let (code, v) = get_with(&app, "/api/v1/studio", &bearer).await;
            assert_eq!(code, StatusCode::OK, "{v}");
            assert_eq!(v["step"]["may_change"], may, "{bearer}");
            assert_eq!(v["step"]["step"], "solo");
            assert_eq!(v["step"]["show"]["name"], "The Morning Kettle");
            assert_eq!(v["step"]["on_air"], true);
        }
    }

    #[tokio::test]
    async fn the_events_say_it_per_account_too() {
        let (app, state) = join_app(JoinConfig::default(), true).await;
        at_step(&state, Step::Guests, None);
        let res = app
            .oneshot(
                Request::get("/api/v1/studio/events")
                    .header("authorization", format!("Bearer {ADMIN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut body = res.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        let json = text
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .expect("a data line");
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["step"]["step"], "guests");
        assert_eq!(v["step"]["may_change"], true);
    }

    #[tokio::test]
    async fn waiting_callers_are_released_when_the_lines_close_and_their_page_says_why() {
        use crate::steps::ReleaseCause;
        let (app, state) = join_app(JoinConfig::default(), true).await;
        // Niall is at the desk's list as a caller, waiting. He called in once, and has a pass.
        let (pass, claims) = keys()
            .pass(
                weavesignal_proto::PassKind::Call,
                Role::Caller,
                "niall",
                "Niall",
                600,
            )
            .unwrap();
        state.hub.with_studio(|s, now| {
            s.call_in(crate::studio::CallIn {
                identity: "niall".into(),
                name: "Niall".into(),
                topic: "No bus".into(),
                consent: crate::studio::Consent {
                    at_ms: now,
                    version: "v".into(),
                    by: crate::studio::ConsentBy::Caller,
                    text: "words".into(),
                },
            })
        });
        state.hub.poll_once().await;
        assert!(
            state
                .hub
                .view()
                .callers
                .iter()
                .any(|c| c.identity == "niall"),
            "waiting in the queue"
        );
        crate::steps::release_waiting(&state, ReleaseCause::ShowTookAir).await;
        assert!(state.hub.view().callers.is_empty(), "out of the queue");
        assert!(state.joins.ended("niall", claims.iat), "pass ended");
        assert_eq!(
            state.joins.released("niall", claims.iat),
            Some(ReleaseCause::ShowTookAir)
        );
        // Their page: ended, with the cause.
        let res = app
            .clone()
            .oneshot(
                Request::get("/api/v1/pass/events")
                    .header("authorization", format!("Bearer {pass}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut body = res.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(text.contains("\"phase\":\"ended\""), "{text}");
        assert!(text.contains("\"released\":\"show_took_air\""), "{text}");
        // And the pass buys nothing more.
        let (code, _) = post_json(
            app,
            "/api/v1/pass/token",
            Some(&pass),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(code, StatusCode::GONE);
    }

    #[tokio::test]
    async fn the_caller_on_the_desks_strip_stays_when_the_lines_close() {
        use crate::steps::ReleaseCause;
        let (app, state) = join_app(JoinConfig::default(), true).await;
        state.hub.with_studio(|s, now| {
            s.call_in(crate::studio::CallIn {
                identity: "niall".into(),
                name: "Niall".into(),
                topic: "No bus".into(),
                consent: crate::studio::Consent {
                    at_ms: now,
                    version: "v".into(),
                    by: crate::studio::ConsentBy::Caller,
                    text: "words".into(),
                },
            })
        });
        state.hub.poll_once().await;
        let (code, v) = post_json(
            app,
            "/api/v1/screener/commands",
            Some(ADMIN),
            serde_json::json!({"op": "send", "identity": "niall"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{v}");
        state.hub.poll_once().await;
        assert_eq!(
            state.hub.read_studio(|s| s.slot_of("niall")),
            Some(4),
            "on the strip"
        );
        crate::steps::release_waiting(&state, ReleaseCause::StepMovedDown).await;
        assert_eq!(
            state.hub.view().callers.len(),
            1,
            "still there until dropped"
        );
        assert!(!state.joins.ended("niall", 0));
    }

    /// The desk's last status with one more caller in the room.
    fn with_caller(state: &AppState, identity: &str) {
        let mut desk = state.hub.desk_status().expect("the fake desk answers");
        let mut person = desk.people[0].clone();
        person.identity = identity.into();
        person.name = identity.into();
        person.role = Role::Caller;
        person.on_air = false;
        desk.people.push(person);
        state.hub.with_studio(|s, now| {
            s.call_in(crate::studio::CallIn {
                identity: identity.into(),
                name: identity.into(),
                topic: "No bus".into(),
                consent: crate::studio::Consent {
                    at_ms: now,
                    version: "v".into(),
                    by: crate::studio::ConsentBy::Caller,
                    text: "words".into(),
                },
            });
            s.sync(&desk, now);
        });
    }

    #[tokio::test]
    async fn a_look_at_a_lower_step_releases_the_waiting_with_the_right_cause_and_keeps_doing_so() {
        use crate::steps::{ReleaseCause, apply, facts_from, test_show};
        let (app, state) = join_app(JoinConfig::default(), true).await;
        let kettle = |step| facts_from(Some(&test_show(step, None)), true, None);
        let other = |step| {
            let mut s = test_show(step, None);
            s.id = uuid::Uuid::from_u128(2);
            s.slug = "steeped".into();
            s.name = "Steeped".into();
            facts_from(Some(&s), true, None)
        };
        let waiting =
            |state: &AppState, id: &str| state.hub.view().callers.iter().any(|c| c.identity == id);
        let player = || get_json(app.clone(), "/api/v1/player");

        // Niall waits while the show runs at Callers: nothing is released, the lines are open.
        apply(&state, kettle(Step::Callers)).await;
        state.hub.poll_once().await;
        assert!(waiting(&state, "niall"));
        assert_eq!(player().await.1["call_in"]["open"], true);

        // Eilidh moves the same show down to Solo: Niall is let go, "step moved down", and every
        // window is told who did it.
        state.hub.live.step_moved("Eilidh");
        apply(&state, kettle(Step::Solo)).await;
        assert!(!waiting(&state, "niall"));
        assert_eq!(
            state.joins.released("niall", 0),
            Some(ReleaseCause::StepMovedDown)
        );
        assert_eq!(player().await.1["call_in"]["open"], false);
        let titles: Vec<String> = state
            .hub
            .view()
            .notices
            .iter()
            .filter(|n| n.kind == "step.moved")
            .map(|n| n.title.clone())
            .collect();
        assert_eq!(titles, ["Eilidh moved The Morning Kettle to Solo."]);

        // A call accepted a moment before the lines closed reaches the desk afterwards: the next
        // look lets it go too, for the same reason.
        with_caller(&state, "jessie");
        assert!(waiting(&state, "jessie"));
        apply(&state, kettle(Step::Solo)).await;
        assert!(!waiting(&state, "jessie"));
        assert_eq!(
            state.joins.released("jessie", 0),
            Some(ReleaseCause::StepMovedDown)
        );

        // The lines open at once when the step rises, and nobody is let go.
        apply(&state, kettle(Step::Callers)).await;
        assert_eq!(player().await.1["call_in"]["open"], true);
        with_caller(&state, "iona");
        apply(&state, kettle(Step::Callers)).await;
        assert!(waiting(&state, "iona"));

        // Another show that takes no calls takes the air: Iona is let go for that.
        apply(&state, other(Step::Guests)).await;
        assert!(!waiting(&state, "iona"));
        assert_eq!(
            state.joins.released("iona", 0),
            Some(ReleaseCause::ShowTookAir)
        );
        // Moving the show between Guests and Solo lets no one else go, and says so once.
        apply(&state, other(Step::Solo)).await;
        assert_eq!(
            state
                .hub
                .view()
                .notices
                .iter()
                .filter(|n| n.kind == "step.moved")
                .count(),
            3,
            "Eilidh's move down, the move back up to Callers, and this one; a show taking the air at another step is no one's move"
        );
    }
}
