// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The monitor link's routes (ADR-0280; design 4.5 and section 10): what a host with a show key
//! hears of the station, sees of their own source, and may press, with a secret of their own and
//! no account.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/monitor` | the host, by the monitor secret | their view ([`MonitorView`]): the lamp and what happens next, the post, before and after, the next insert, the step, the last refusal, what they may press |
//! | `GET /api/v1/monitor/events` | the same | server-sent events: that view at once and then every second (`view`), or `ended` and the stream closes |
//! | `POST /api/v1/monitor/token` | the same | a 600 s listen-only room token for the host's monitor room |
//! | `POST /api/v1/monitor/go`, `/not-yet` | the same | the late count's *Go on air now* and *Not yet*, or *Go on air now* at the show's time |
//! | `POST /api/v1/monitor/cue` | the same | the next host in the host's headphones (`{"on": true}`) |
//! | `POST /api/v1/monitor/handover` | the same | the handover's two presses (`{"step": "bring"}`, then `{"step": "hand_over"}`) |
//! | `POST /api/v1/monitor/end-show` | the same | End show, for this host |
//! | `POST /api/v1/monitor/dump` | the same | Dump, while on air |
//!
//! **The secret** is the key's second secret, shown once with the key and kept as a keyed hash
//! ([`ApiKey::monitor_hash`], `show_key.monitor_hash`); it rides as a bearer, never in an address.
//! It finds its key by that hash, so a host sends nothing else. A wrong one is counted by address
//! (five a minute, then ten minutes refused, as a wrong key is), and an ended key's is refused
//! with the words the page shows.
//!
//! **The controls** act through what exists and never a second path: *Go on air now* and *Not
//! yet* are the desk's own ([`crate::post::go_now`]), the handover's presses the post's
//! ([`crate::post::bring_on`], [`crate::post::take_off`]), End show the desk's
//! ([`crate::app::end_show_run`], scoped to this host), Dump and Cue the desk's commands. Each is
//! refused unless the same rule that draws the page's buttons ([`crate::monitor_view::controls`])
//! says it is this host's to do now, with the words it gives for why not (409). No request names
//! a person: the next host is worked out here.
//!
//! **The audio** is the desk's monitor mix in a room of its own (`<room>-monitor-<key id>`),
//! which the desk makes while a seat is taken ([`weavesignal_proto::monitor`]). Its token is a
//! plain `listener` token (it cannot publish) with no recorder grant, so it hears only what the
//! desk's publisher allows its two seats (ADR-0118). Two browsers may listen; a third is refused
//! (409). A seat is taken when LiveKit lists its identity in the room, or was handed out within
//! the last 30 s (a page that has not joined yet). When the key ends, both seats are removed
//! from the room ([`remove_seats`]).
//!
//! **Not here:** the page `monitor/`, the plain audio address, the desk's crossfade on the
//! schedule's moves.

// Every helper here returns the refusal itself as its error, as the handlers send it.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio_stream::wrappers::ReceiverStream;
use weavesignal_model::post::Cause;
use weavesignal_model::shows::{self, Show};
use weavesignal_model::sources::{self as model, ShowKey, WrongKeys};
use weavesignal_proto::{Role, source_wire};

use crate::app::{AppState, problem};
use crate::desk::DeskCommand;
use crate::hub::{Hub, RoomAdmin, now_ms};
use crate::monitor_view::{self as view, Facts, MonitorView};

/// How long a monitor token can be used to join.
pub const MONITOR_TTL_S: u64 = 600;
/// How long a seat handed out stays taken while its page has not joined (LiveKit says nothing
/// of it yet).
const RESERVED: Duration = Duration::from_secs(30);
/// What the page says when the link's secret is not (or no longer) good (design section 9).
const ENDED_WORDS: &str = "This link no longer works. Ask the station for a new one.";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/monitor", get(get_view))
        .route("/api/v1/monitor/events", get(events))
        .route("/api/v1/monitor/token", post(token))
        .route("/api/v1/monitor/go", post(go))
        .route("/api/v1/monitor/not-yet", post(not_yet))
        .route("/api/v1/monitor/cue", post(cue))
        .route("/api/v1/monitor/handover", post(handover))
        .route("/api/v1/monitor/end-show", post(end_show))
        .route("/api/v1/monitor/dump", post(dump))
        .layer(Extension(Arc::new(Live::default())))
}

/// How many pages may hold an event stream open for one link: two browsers and a reconnect or
/// two. A link beyond it is told to wait.
const MAX_STREAMS: usize = 6;

/// The monitor links' state in memory: wrong secrets by address, and the seats handed out.
#[derive(Default)]
pub struct Live {
    wrong: Mutex<WrongKeys>,
    /// Per key id, when each seat was last handed out.
    reserved: Mutex<HashMap<String, [Option<Instant>; weavesignal_proto::monitor::SEATS]>>,
    /// How many event streams each key has open now.
    streams: Mutex<HashMap<String, usize>>,
}

impl Live {
    /// The first seat of `key_id` that is free, taken now; `listening` is who LiveKit lists in
    /// the monitor room (`None` when it could not be asked: then only what was handed out
    /// lately counts).
    fn take_seat(
        &self,
        key_id: &str,
        listening: Option<&[String]>,
        now: Instant,
    ) -> Option<String> {
        let mut reserved = self.reserved.lock().expect("monitor seats");
        if reserved.len() > 4096 {
            reserved.retain(|_, seats| {
                seats
                    .iter()
                    .flatten()
                    .any(|t| now.duration_since(*t) < RESERVED)
            });
        }
        let seats = reserved.entry(key_id.to_owned()).or_default();
        for (i, handed) in seats.iter_mut().enumerate() {
            let identity = weavesignal_proto::monitor_seat(key_id, i + 1);
            let lately = handed.is_some_and(|t| now.duration_since(t) < RESERVED);
            let present = listening.is_some_and(|l| l.contains(&identity));
            if !lately && !present {
                *handed = Some(now);
                return Some(identity);
            }
        }
        None
    }
}

/// Holds one of a key's event-stream places until dropped.
struct StreamPlace {
    live: Arc<Live>,
    key_id: String,
}

impl Drop for StreamPlace {
    fn drop(&mut self) {
        let mut streams = self.live.streams.lock().expect("monitor streams");
        if let Some(n) = streams.get_mut(&self.key_id) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                streams.remove(&self.key_id);
            }
        }
    }
}

impl Live {
    fn take_stream(self: &Arc<Self>, key_id: &str) -> Option<StreamPlace> {
        let mut streams = self.streams.lock().expect("monitor streams");
        let n = streams.entry(key_id.to_owned()).or_default();
        if *n >= MAX_STREAMS {
            return None;
        }
        *n += 1;
        Some(StreamPlace {
            live: self.clone(),
            key_id: key_id.to_owned(),
        })
    }
}

/// Take both of a key's seats out of its monitor room, now and again a few seconds later (a
/// page that reconnects by itself within the token's life finds the room empty and the key
/// ended). Called wherever a key ends; without LiveKit it does nothing.
pub(crate) fn remove_seats(hub: &Arc<Hub>, key_id: &str) {
    if hub.room.is_none() {
        return;
    }
    let hub = hub.clone();
    let key_id = key_id.to_owned();
    tokio::spawn(async move {
        for wait in [0u64, 5] {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            let Some(admin) = &hub.room else { return };
            if let Err(e) = admin.remove_monitor(&key_id).await {
                tracing::warn!(key = %key_id, error = %format!("{e:#}"), "monitor: cannot remove the seats of an ended key");
            }
        }
    });
}

fn db_failed(e: &sqlx::Error) -> Response {
    tracing::error!(error = %e, "monitor: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

/// The address a request came from, for the wrong-secret limit: `None` for a request from
/// loopback with no forwarded address (behind a proxy that does not say), which is not limited.
fn address(
    state: &AppState,
    headers: &HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Option<String> {
    crate::public::peer(
        state.join.forwarded_for,
        headers,
        conn.map(|Extension(ConnectInfo(a))| a),
    )
    .map(|ip| ip.to_string())
}

/// The host a request is from: their key and show, or the refusal.
async fn host(
    state: &AppState,
    live: &Live,
    headers: &HeaderMap,
    addr: Option<&str>,
) -> Result<(ShowKey, Show), Response> {
    let now = now_ms() as i64;
    if let Some(addr) = addr
        && let Some(until) = live
            .wrong
            .lock()
            .expect("monitor wrong secrets")
            .blocked_until(addr, now)
    {
        let mut res = problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many wrong links from your address. Try again in a few minutes.",
        );
        let wait = ((until - now).max(1000) / 1000).to_string();
        if let Ok(v) = HeaderValue::from_str(&wait) {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return Err(res);
    }
    let secret = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| (8..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric()));
    let wrong = |live: &Live| {
        if let Some(addr) = addr {
            live.wrong
                .lock()
                .expect("monitor wrong secrets")
                .wrong(addr, now);
        }
        problem(StatusCode::UNAUTHORIZED, ENDED_WORDS)
    };
    let Some(secret) = secret else {
        return Err(wrong(live));
    };
    let Some(pool) = state.db.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the monitor link needs the station's database",
        ));
    };
    let Some(key) = state.staff.api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the monitor link needs the token key",
        ));
    };
    let hash = key.monitor_hash(secret);
    let row =
        sqlx::query("SELECT key_id FROM show_key WHERE station_id = $1 AND monitor_hash = $2")
            .bind(state.station.id.as_uuid())
            .bind(&hash[..])
            .fetch_optional(pool)
            .await
            .map_err(|e| db_failed(&e))?;
    let Some(row) = row else {
        return Err(wrong(live));
    };
    let key_id: String = row.try_get("key_id").map_err(|e| db_failed(&e))?;
    match load_host(state, &key_id).await? {
        Some(host) => Ok(host),
        None => Err(wrong(live)),
    }
}

/// A key by its id, with its show: `None` when there is no such key, a refusal when it has
/// ended (410, the words the page shows).
async fn load_host(state: &AppState, key_id: &str) -> Result<Option<(ShowKey, Show)>, Response> {
    let now = now_ms() as i64;
    let pool = state.db.as_ref().expect("the caller checked");
    let Some(key) = model::key_by_key_id(pool, state.station.id, key_id)
        .await
        .map_err(|e| db_failed(&e))?
    else {
        return Ok(None);
    };
    if key.revoked_ms.is_some_and(|r| r <= now) {
        return Err(problem(StatusCode::GONE, ENDED_WORDS));
    }
    let show = shows::list_shows(pool, state.station.id)
        .await
        .map_err(|e| db_failed(&e))?
        .into_iter()
        .find(|s| s.id == key.show_id);
    match show {
        Some(show) => Ok(Some((key, show))),
        None => Err(problem(StatusCode::GONE, ENDED_WORDS)),
    }
}

/// The host's facts now, and their view.
async fn current(
    state: &AppState,
    key: &ShowKey,
    show: &Show,
) -> Result<(Facts, MonitorView), Response> {
    let pool = state.db.as_ref().expect("the caller checked");
    let facts = view::gather(state, pool, key, show)
        .await
        .map_err(|e| db_failed(&e))?;
    let v = view::build(&facts, view::last_refusal(&facts.log));
    Ok((facts, v))
}

#[utoipa::path(
    get,
    path = "/api/v1/monitor",
    operation_id = "monitor_view",
    tag = "monitor",
    summary = "The monitor link's view",
    description = "What the host behind a monitor link sees: their own source's state (`on_air`, `open`, `standby`, `link_lost`, `test`, `not_connected`) and the lamp's sentence, what happens next with the instant it counts to (`then.at_ms`, on the station's clock), the post (the grace, a hold, the late count) as far as it is theirs, before you, you and after you, the next insert as far as the log can say, the show's step, the next host, the last refusal in words, what the station hears of them, and what they may press now (`controls`, each with `why_not`). The bearer is the monitor secret from the key's making; it needs no account. 401 for a secret that is not one; 410 when the key has ended; 429 for too many wrong secrets from one address.",
    responses(
        (status = 200, description = "The host's own view.", body = MonitorView),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn get_view(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let addr = address(&state, &headers, conn);
    let (key, show) = match host(&state, &live, &headers, addr.as_deref()).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    match current(&state, &key, &show).await {
        Ok((_, v)) => Json(v).into_response(),
        Err(r) => r,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/monitor/events",
    operation_id = "monitor_events",
    tag = "monitor",
    summary = "The monitor link's view, as events",
    description = "Server-sent events: a `view` event whose data is the same JSON as `GET /api/v1/monitor`, at once and then every second (its `station.now_ms` is the station's clock, for the page's counts); a keep-alive comment every 10 seconds. When the key ends the stream says `ended` (its data: `{\"error\": \"This link no longer works. Ask the station for a new one.\"}`) and closes. The bearer is the monitor secret, in the `Authorization` header (never in the address). Up to six streams a link; a seventh is refused with 429.",
    responses(
        (status = 200, description = "An event stream (`text/event-stream`).", body = String, content_type = "text/event-stream"),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn events(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let addr = address(&state, &headers, conn);
    let (key, _) = match host(&state, &live, &headers, addr.as_deref()).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Some(place) = live.take_stream(&key.key_id) else {
        let mut res = problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many pages are open on this link. Close one.",
        );
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
        return res;
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(4);
    let key_id = key.key_id.clone();
    tokio::spawn(async move {
        let _place = place;
        let mut every = tokio::time::interval(Duration::from_secs(1));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // The first look is at once; a closed page ends the stream here.
            tokio::select! {
                _ = every.tick() => {}
                () = tx.closed() => return,
            }
            // The key may have ended, or its show gone, since the last look.
            let (key, show) = match load_host(&state, &key_id).await {
                Ok(Some(host)) => host,
                Ok(None) | Err(_) => {
                    remove_seats(&state.hub, &key_id);
                    let ended = Event::default()
                        .event("ended")
                        .json_data(serde_json::json!({ "error": ENDED_WORDS }))
                        .expect("the words serialise");
                    let _ = tx.send(Ok(ended)).await;
                    return;
                }
            };
            let Ok((_, v)) = current(&state, &key, &show).await else {
                continue;
            };
            let event = Event::default()
                .event("view")
                .json_data(&v)
                .expect("the view serialises");
            if tx.send(Ok(event)).await.is_err() {
                return;
            }
        }
    });
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

/// A listen-only room token for the host's monitor room.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MonitorToken {
    /// The LiveKit access token.
    pub token: String,
    /// Where browsers reach LiveKit.
    pub url: String,
    /// The monitor room: `<room>-monitor-<key id>`.
    pub room: String,
    /// The seat the token is for.
    pub identity: String,
    /// Seconds the token can be used to join.
    pub ttl_s: u64,
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/token",
    operation_id = "monitor_token",
    tag = "monitor",
    summary = "A room token for the monitor link",
    description = "A 600 s listen-only token for the host's monitor room, where the desk publishes the programme without them, the cue bus and talk addressed to them (ADR-0118: only the link's two seats may subscribe). Two browsers may listen at once: a third is refused with 409. The bearer is the monitor secret. 410 when the key has ended; 503 when the room is not set up.",
    responses(
        (status = 201, description = "The token.", body = MonitorToken),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The token could not be signed.", body = crate::openapi::Problem),
    ),
    security(("monitor" = [])),
)]
async fn token(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let addr = address(&state, &headers, conn);
    let (key, show) = match host(&state, &live, &headers, addr.as_deref()).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Some(room) = &state.room else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The monitor's sound is not set up at this station yet.",
        );
    };
    let monitor_room = weavesignal_proto::monitor_room(&room.config.name, &key.key_id);
    let listening = match &state.hub.room {
        Some(admin) => match listed(admin, &monitor_room).await {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "monitor: cannot ask LiveKit who listens");
                None
            }
        },
        None => None,
    };
    let Some(identity) = live.take_seat(&key.key_id, listening.as_deref(), Instant::now()) else {
        return problem(
            StatusCode::CONFLICT,
            "Two browsers are listening already. Close one, or wait a few seconds if you have just reloaded.",
        );
    };
    // A listener's token with no recorder grant: what the desk publishes there reaches the
    // seats it names and no one else (ADR-0118). Never `Keys::monitor`, which hears every track.
    match room.keys.participant_with(
        &monitor_room,
        &identity,
        &format!("Monitor {}", show.name),
        Role::Listener,
        MONITOR_TTL_S,
        true,
    ) {
        Ok(token) => {
            tracing::info!(key = %key.key_id, %identity, "monitor token issued");
            (
                StatusCode::CREATED,
                Json(MonitorToken {
                    token,
                    url: room.config.public_url.clone(),
                    room: monitor_room,
                    identity,
                    ttl_s: MONITOR_TTL_S,
                }),
            )
                .into_response()
        }
        Err(e) => problem(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---- the controls ----

/// What a control answers: what it did, and the host's view after it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MonitorDone {
    /// `go`, `not_yet`, `cue`, `bring`, `hand_over`, `end_show` or `dump`.
    pub done: String,
    /// The host's view after it, as `GET /api/v1/monitor` gives it.
    pub view: MonitorView,
    /// End show: the next host's name when the air passed to them.
    pub passed_to: Option<String>,
    /// End show: how long until automation takes the air (`0` it holds it already); absent when
    /// nothing goes to automation (the air passed on, another presenter keeps it, or there is
    /// no playout layer).
    pub automation_in_ms: Option<u64>,
    /// Dump: how much of the programme listeners lost, in ms (what the delay held).
    pub lost_ms: Option<u64>,
}

/// A host's press, as the log writes it: `reason` holds at most 60 characters.
fn by_of(key: &ShowKey) -> String {
    let label: String = key.label.chars().take(44).collect();
    format!("{label} (monitor)")
}

/// The host, and their facts, for a control: or the refusal.
async fn control_host(
    state: &AppState,
    live: &Live,
    headers: &HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Result<(ShowKey, Show, Facts), Response> {
    let addr = address(state, headers, conn);
    let (key, show) = host(state, live, headers, addr.as_deref()).await?;
    let (facts, _) = current(state, &key, &show).await?;
    Ok((key, show, facts))
}

/// The refusal for a control that may not be pressed now: 409 with its words.
fn refused(c: &view::Control) -> Option<Response> {
    (!c.may).then(|| {
        problem(
            StatusCode::CONFLICT,
            c.why_not
                .clone()
                .unwrap_or_else(|| "That is not yours to do now.".to_owned()),
        )
    })
}

async fn done(
    state: &AppState,
    key: &ShowKey,
    show: &Show,
    what: &str,
    extra: impl FnOnce(&mut MonitorDone),
) -> Response {
    match current(state, key, show).await {
        Ok((_, view)) => {
            let mut d = MonitorDone {
                done: what.to_owned(),
                view,
                passed_to: None,
                automation_in_ms: None,
                lost_ms: None,
            };
            extra(&mut d);
            Json(d).into_response()
        }
        Err(r) => r,
    }
}

/// A request body that may be empty: its JSON, or the default.
fn body_of<T: serde::de::DeserializeOwned + Default>(bytes: &Bytes) -> Result<T, Response> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(bytes).map_err(|e| {
        problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("the request is not what this route takes: {e}"),
        )
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/go",
    operation_id = "monitor_go",
    tag = "monitor",
    summary = "Go on air now",
    description = "The host's own press: skips the late count, or takes the air at the show's time. It is the desk's own Go on air now (`POST /api/v1/sources/{key}/go`), for this host's source only. Refused (409, with the words) unless the source is standing by and steady, the show's time has come, and nobody else presents. The bearer is the monitor secret.",
    responses(
        (status = 200, description = "On air.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn go(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Some(r) = refused(&view::controls(&facts).go) {
        return r;
    }
    if let Err(r) = crate::post::go_now(&state, &key.key_id, &by_of(&key)).await {
        return r;
    }
    done(&state, &key, &show, "go", |_| {}).await
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/not-yet",
    operation_id = "monitor_not_yet",
    tag = "monitor",
    summary = "Not yet: stop the late count",
    description = "Stops the late count for this host's airing: the schedule will not put them on air by itself, and they go on air by their own press (`go`). The desk's own Not yet (`POST /api/v1/sources/{key}/not-yet`), for this host's source only. Refused (409) unless their count is running.",
    responses(
        (status = 200, description = "The count is stopped.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn not_yet(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Some(r) = refused(&view::controls(&facts).not_yet) {
        return r;
    }
    if let Err(r) = crate::post::not_yet_now(&state, &key.key_id, &by_of(&key)).await {
        return r;
    }
    done(&state, &key, &show, "not_yet", |_| {}).await
}

/// The body of `POST /api/v1/monitor/cue`.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CueRequest {
    /// Put the next host in the headphones (`true`, the default) or take them out.
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

impl Default for CueRequest {
    fn default() -> Self {
        CueRequest { on: true }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/cue",
    operation_id = "monitor_cue",
    tag = "monitor",
    summary = "Cue the next host",
    description = "Puts the next host (`next_host` in the view: the show after this one's host standing by, or the DJ in the room waiting) in this host's headphones, or takes them out (`{\"on\": false}`). The body may be empty. The desk's own cue, for the one person the station works out: no identity is taken from the request. Refused (409) unless this host is on air and the next is at the desk and not on air.",
    request_body(content = Option<CueRequest>, description = "`{\"on\": true}` by default."),
    responses(
        (status = 200, description = "MonitorDone.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn cue(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Bytes,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let req: CueRequest = match body_of(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let target = view::next_target(&facts);
    // Cue is global (a producer's cue is the same cue), so only a host on air touches it, in or
    // out. Putting the next host in is the rule's; taking them out wants only someone to take out.
    if req.on || target.is_none() {
        if let Some(r) = refused(&view::controls(&facts).cue) {
            return r;
        }
    } else if !facts.on_air() {
        return problem(StatusCode::CONFLICT, "You are not on air.");
    }
    let Some(target) = target else {
        return problem(StatusCode::CONFLICT, "Nobody is standing by after you.");
    };
    if let Err(e) = state
        .hub
        .command(&DeskCommand::Cue {
            identity: target.identity,
            on: req.on,
        })
        .await
    {
        return crate::app::desk_problem(&e);
    }
    done(&state, &key, &show, "cue", |_| {}).await
}

/// The body of `POST /api/v1/monitor/handover`.
#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HandoverRequest {
    /// `bring`: the first press, the next host on air beside this one (the overlap). `hand_over`:
    /// the second, everyone else presenting comes off air and the next holds it.
    pub step: HandoverStep,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandoverStep {
    #[default]
    Bring,
    HandOver,
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/handover",
    operation_id = "monitor_handover",
    tag = "monitor",
    summary = "The handover's two presses",
    description = "`{\"step\": \"bring\"}` puts the next host on air beside this one (the overlap); `{\"step\": \"hand_over\"}` then takes this host (and anyone else presenting) off air, so the next holds it. The two presses of the desk's own handover (ADR-0231), for the one next host the station works out. Refused (409, with the words) unless this host is on air, the show runs at Full, the next is at the desk, and (for the second) the first press has been made.",
    request_body = HandoverRequest,
    responses(
        (status = 200, description = "MonitorDone.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn handover(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Bytes,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let req: HandoverRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("`step` is `bring` or `hand_over`: {e}"),
            );
        }
    };
    let c = view::controls(&facts);
    let by = by_of(&key);
    match req.step {
        HandoverStep::Bring => {
            if let Some(r) = refused(&c.bring) {
                return r;
            }
            let Some(next) = view::next_target(&facts) else {
                return problem(StatusCode::CONFLICT, "Nobody is standing by after you.");
            };
            // Beside this host: nobody comes off, and the station's mode is not touched (a press
            // does not take the air back from automation; Go on air now does).
            if let Err(e) = crate::post::bring_on(
                &state,
                &next.subject,
                &next.identity,
                next.show.unwrap_or(facts.show.id),
                Cause::Handover,
                &[],
                false,
                facts.settings.fade_ms,
                &by,
            )
            .await
            {
                return problem(StatusCode::CONFLICT, format!("{e:#}"));
            }
            done(&state, &key, &show, "bring", |_| {}).await
        }
        HandoverStep::HandOver => {
            if let Some(r) = refused(&c.hand_over) {
                return r;
            }
            let takeoff = view::handover_takeoff(&facts);
            let next = view::next_target(&facts)
                .map(|n| n.identity)
                .unwrap_or_default();
            if let Err(e) = crate::post::take_off(&state, &takeoff, &next, &by).await {
                return crate::app::desk_problem(&e);
            }
            done(&state, &key, &show, "hand_over", |_| {}).await
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/end-show",
    operation_id = "monitor_end_show",
    tag = "monitor",
    summary = "End my show",
    description = "The desk's End show (`POST /api/v1/show/end`), scoped to this host. With nobody else presenting it is the whole of it: everyone on air comes off, the carts fade, and the air passes to the next host standing by (`passed_to`) or goes to automation once the profanity delay has played out (`automation_in_ms`). With another presenter on air, only this host leaves and that presenter keeps the air. Refused (409) unless this host is on air. The page asks once first; `controls.end_show_says` is what it says follows.",
    responses(
        (status = 200, description = "What was done.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn end_show(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Some(r) = refused(&view::controls(&facts).end_show) {
        return r;
    }
    let identity = source_wire::identity(&key.key_id);
    let result = match crate::app::end_show_run(&state, Some(&identity), &by_of(&key)).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    done(&state, &key, &show, "end_show", |d| {
        d.passed_to = result.passed_to.clone();
        d.automation_in_ms = result.automation_in_ms;
    })
    .await
}

#[utoipa::path(
    post,
    path = "/api/v1/monitor/dump",
    operation_id = "monitor_dump",
    tag = "monitor",
    summary = "Dump",
    description = "Throws away what the profanity delay holds (the desk's own Dump, on every outgoing stream), for the host who is on air. `lost_ms` is how much of the programme listeners lost. Refused (409, with the words) unless this host is on air and the show's step has Dump (Callers and up). The page asks for two presses within three seconds; nothing here counts them.",
    responses(
        (status = 200, description = "Dumped.", body = MonitorDone),
        (status = 401, description = "Not a monitor link's secret.", body = crate::openapi::Problem),
        (status = 409, response = crate::openapi::Conflict),
        (status = 410, description = "The key has ended.", body = crate::openapi::Problem),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("monitor" = [])),
)]
async fn dump(
    State(state): State<Arc<AppState>>,
    Extension(live): Extension<Arc<Live>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    let (key, show, facts) = match control_host(&state, &live, &headers, conn).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Some(r) = refused(&view::controls(&facts).dump) {
        return r;
    }
    let held_ms = state
        .hub
        .desk_status()
        .and_then(|d| d.delay)
        .map_or(0, |d| u64::from(d.delay_ms));
    if let Err(e) = state.hub.command(&DeskCommand::Dump).await {
        return crate::app::desk_problem(&e);
    }
    tracing::info!(key = %key.key_id, lost_ms = held_ms, "monitor: the host dumped the delay");
    done(&state, &key, &show, "dump", |d| d.lost_ms = Some(held_ms)).await
}

/// Who LiveKit lists in `room` now (identities); a room that is not there is empty.
async fn listed(admin: &RoomAdmin, room: &str) -> anyhow::Result<Vec<String>> {
    let url = format!(
        "{}/twirp/livekit.RoomService/ListParticipants",
        admin.api_url.trim_end_matches('/')
    );
    let bearer = admin.keys.room_admin(room, 60);
    let body = serde_json::json!({ "room": room });
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut res = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .send_json(body)?;
        match res.status().as_u16() {
            404 => Ok(Vec::new()),
            200 => {
                let v: serde_json::Value = res.body_mut().read_json()?;
                Ok(v["participants"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|p| p["state"] != "DISCONNECTED")
                            .filter_map(|p| p["identity"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default())
            }
            s => anyhow::bail!(
                "LiveKit answered {s}: {}",
                res.body_mut().read_to_string().unwrap_or_default()
            ),
        }
    })
    .await?
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(get_view, events, token, go, not_yet, cue, handover, end_show, dump),
    components(schemas(
        MonitorView,
        MonitorToken,
        MonitorDone,
        CueRequest,
        HandoverRequest,
        HandoverStep,
        view::StationClock,
        view::LastRefusal,
        view::Then,
        view::MonitorPost,
        view::HeadsUpLine,
        view::RundownRow,
        view::Rundown,
        view::Insert,
        view::StepInfo,
        view::NextHostView,
        view::Control,
        view::Controls,
    ))
)]
pub struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_seats_and_a_third_is_refused() {
        let live = Live::default();
        let t = Instant::now();
        assert_eq!(
            live.take_seat("k3yid0ab", None, t).as_deref(),
            Some("monitor-k3yid0ab-1")
        );
        assert_eq!(
            live.take_seat("k3yid0ab", None, t).as_deref(),
            Some("monitor-k3yid0ab-2")
        );
        assert_eq!(live.take_seat("k3yid0ab", None, t), None);
        // Another link has seats of its own.
        assert!(live.take_seat("other000", None, t).is_some());
    }

    #[test]
    fn a_seat_handed_out_is_free_again_once_its_page_has_had_time_to_join_and_left() {
        let live = Live::default();
        let t = Instant::now();
        let later = t + RESERVED + Duration::from_secs(1);
        assert!(live.take_seat("k3yid0ab", None, t).is_some());
        // Seat 1 is in the room; seat 2 was never handed out. Seat 1 stays taken beyond the
        // reservation because LiveKit lists it.
        let listed = vec!["monitor-k3yid0ab-1".to_owned()];
        assert_eq!(
            live.take_seat("k3yid0ab", Some(&listed), later).as_deref(),
            Some("monitor-k3yid0ab-2")
        );
        assert_eq!(
            live.take_seat("k3yid0ab", Some(&listed), later),
            None,
            "seat 1 is listed, seat 2 was just handed out"
        );
        // Nobody listens any more and the reservations have run out: seat 1 again.
        let much_later = later + RESERVED + Duration::from_secs(1);
        assert_eq!(
            live.take_seat("k3yid0ab", Some(&[]), much_later).as_deref(),
            Some("monitor-k3yid0ab-1")
        );
    }
}
