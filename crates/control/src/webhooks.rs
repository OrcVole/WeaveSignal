// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Webhooks (ADR-0195): admin routes to make them, and the worker that delivers events
//! ([`crate::events`]) to them.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/webhooks` | admin | every webhook with its last delivery, and the events one can ask for |
//! | `POST /api/v1/webhooks` | admin | `name`, `url`, `events`: made, with its signing secret, shown once |
//! | `PUT /api/v1/webhooks/{id}` | admin | `name`, `url`, `events`, `enabled`: any of them |
//! | `POST /api/v1/webhooks/{id}/secret` | admin | a new signing secret (the old one stops at once), shown once |
//! | `DELETE /api/v1/webhooks/{id}` | admin | remove it |
//!
//! **A delivery** is a `POST` of the event as JSON with `X-WeaveSignal-Timestamp` (Unix seconds,
//! fresh on each attempt) and `X-WeaveSignal-Signature: sha256=<hex>`, the HMAC-SHA-256 under the
//! webhook's secret of the timestamp, a full stop and the exact body (so a receiver that refuses
//! an old timestamp cannot be replayed a delivery); `X-WeaveSignal-Event` and
//! `X-WeaveSignal-Delivery` (the event's id, the same on every retry). Any `2xx` is delivered. Else
//! it is tried again after 5 s, 30 s, 2 min and 10 min, and given up. Retries are kept in
//! memory: a restart forgets those waiting. Each attempt is recorded on the webhook (`last_status`,
//! `last_error`, `failures`). A slow or dead receiver holds up only its own deliveries
//! ([`spawn_worker`]).
//!
//! **The guard against reaching inside the station's network.** A webhook's address must be
//! `https`, and must resolve to public addresses only: loopback, private, link-local, shared
//! (carrier-grade NAT), unique-local, multicast and reserved ranges are refused when the webhook
//! is made *and again at every delivery*, by the resolver the delivery connects through (so a
//! name that changes its answer later reaches nothing new). Redirects are not followed and no
//! proxy is used. `api.webhooks_allow_private = true` lifts the guard for a station that wants
//! its integration on its own network, and then `http` is allowed to a loopback address too.
//! The secret is not stored: it is derived from the webhook's id and a salt under the token key
//! ([`crate::apikey`]).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Semaphore;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use uuid::Uuid;
use weavesignal_model::webhooks::{self as model, NewWebhook, Webhook};

use crate::api::Api as ApiState;
use crate::apikey::{ApiKey, delivery_signature};
use crate::app::{AppState, problem};
use crate::events::{self, Event};
use crate::hub::now_ms;
use crate::staff::{Need, Staff};

/// How long after an attempt that fails the next is made.
pub const BACKOFF: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(600),
];
/// One attempt gives up after this long.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Attempts in flight at once, across all webhooks (held for one attempt, not its back-off).
const IN_FLIGHT: usize = 32;
/// Deliveries one webhook may have waiting (in an attempt or a back-off); more are dropped
/// for it alone.
pub const PER_WEBHOOK: usize = 16;
/// How many webhooks a station may have.
const MAX_WEBHOOKS: usize = 20;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/webhooks", get(list).post(create))
        .route(
            "/api/v1/webhooks/{id}",
            axum::routing::put(change).delete(remove),
        )
        .route("/api/v1/webhooks/{id}/secret", post(new_secret))
}

// ---- addresses ----

/// Whether an address is one a webhook may reach with the guard on.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // shared address space, RFC 6598
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // documentation
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || o[0] >= 240) // reserved
}

fn public_v6(a: Ipv6Addr) -> bool {
    // IPv4-mapped (`::ffff:a.b.c.d`) and the deprecated IPv4-compatible (`::a.b.c.d`, which
    // takes `::` and `::1` to 0.0.0.0 and 0.0.0.1, neither public): judge the IPv4 address.
    if let Some(v4) = a.to_ipv4() {
        return public_v4(v4);
    }
    let s = a.segments();
    // IPv4-translated (`::ffff:0:a.b.c.d`, RFC 2765) and local-use NAT64 (64:ff9b:1::/48,
    // RFC 8215) reach whatever IPv4 network the translator sits in: never public.
    if (s[..4].iter().all(|x| *x == 0) && s[4] == 0xffff && s[5] == 0)
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1)
    {
        return false;
    }
    // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address: judge that one.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        return public_v4(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            s[6] as u8,
            (s[7] >> 8) as u8,
            s[7] as u8,
        ));
    }
    if s[0] == 0x2002 {
        return public_v4(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // site-local, deprecated
        || (s[0] == 0x2001 && s[1] == 0) // Teredo
        || (s[0] == 0x2001 && s[1] == 0xdb8) // documentation
        || (s[0] == 0x100 && s[1..4].iter().all(|x| *x == 0))) // discard-only
}

/// The resolver a delivery connects through: the system's, its answers filtered to the
/// addresses the guard allows. The connection goes to exactly what it returns.
#[derive(Debug)]
struct Guarded {
    allow_private: bool,
    inner: DefaultResolver,
}

impl Resolver for Guarded {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let all = self.inner.resolve(uri, config, timeout)?;
        if self.allow_private {
            return Ok(all);
        }
        let mut out = self.empty();
        for a in all.iter().filter(|a| is_public(a.ip())) {
            out.push(*a);
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

fn agent(allow_private: bool) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(ATTEMPT_TIMEOUT))
        .max_redirects(0)
        .proxy(None)
        .https_only(!allow_private)
        .http_status_as_error(false)
        .user_agent("WeaveSignal-Webhook")
        .build();
    ureq::Agent::with_parts(
        config,
        DefaultConnector::new(),
        Guarded {
            allow_private,
            inner: DefaultResolver::default(),
        },
    )
}

/// A webhook address checked as far as can be without the network: `Err` is a sentence for
/// the person who typed it. A host name is judged when it is resolved ([`check_resolved`]).
pub fn check_url(raw: &str, allow_private: bool) -> Result<url::Url, String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| "That is not a web address.".to_owned())?;
    if !u.username().is_empty() || u.password().is_some() {
        return Err("Leave the user name and password out of the address.".into());
    }
    let host = u.host().ok_or("The address needs a host name.")?;
    let loopback = match &host {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(a) => a.is_loopback(),
        url::Host::Ipv6(a) => a.is_loopback(),
    };
    match u.scheme() {
        "https" => {}
        "http" if allow_private && loopback => {}
        "http" => return Err("A webhook must use https.".into()),
        _ => return Err("A webhook must use https.".into()),
    }
    if !allow_private {
        match host {
            url::Host::Ipv4(a) if !public_v4(a) => {
                return Err("That address is on a private network; the station's settings do not allow webhooks there.".into());
            }
            url::Host::Ipv6(a) if !public_v6(a) => {
                return Err("That address is on a private network; the station's settings do not allow webhooks there.".into());
            }
            url::Host::Domain(d) if d.eq_ignore_ascii_case("localhost") => {
                return Err("That address is on a private network; the station's settings do not allow webhooks there.".into());
            }
            _ => {}
        }
    }
    Ok(u)
}

/// Resolve a host name and refuse it if any answer is on a private network.
async fn check_resolved(u: &url::Url, allow_private: bool) -> Result<(), String> {
    if allow_private {
        return Ok(());
    }
    let Some(url::Host::Domain(d)) = u.host() else {
        return Ok(());
    };
    let port = u.port_or_known_default().unwrap_or(443);
    let found = tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((d, port)))
        .await
        .map_err(|_| "That host did not answer when looked up.".to_owned())?
        .map_err(|_| "That host name cannot be found.".to_owned())?
        .collect::<Vec<SocketAddr>>();
    if found.is_empty() {
        return Err("That host name cannot be found.".into());
    }
    if found.iter().any(|a| !is_public(a.ip())) {
        return Err("That host name leads to a private network; the station's settings do not allow webhooks there.".into());
    }
    Ok(())
}

// ---- the routes ----

#[derive(Serialize, utoipa::ToSchema)]
struct Listed {
    webhooks: Vec<Webhook>,
    events: Vec<EventInfo>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct EventInfo {
    name: &'static str,
    description: &'static str,
}

fn event_infos() -> Vec<EventInfo> {
    events::KINDS
        .iter()
        .map(|(name, description)| EventInfo { name, description })
        .collect()
}

#[derive(Serialize, utoipa::ToSchema)]
struct Made {
    webhook: Webhook,
    /// The signing secret, shown this once (and again only by making a new one).
    secret: String,
}

fn field(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": detail.into(), "field": field })),
    )
        .into_response()
}

fn db_problem(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "webhooks: database error");
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
) -> Result<(String, &'a ApiState, &'a sqlx::PgPool, &'a ApiKey), Response> {
    let actor = state.check(headers, Need::Admin)?;
    let api = &state.staff.api;
    let Some(pool) = state.db.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "webhooks are kept in the database",
        ));
    };
    let Some(key) = api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "webhooks need a key: set {} (or the station key)",
                ApiKey::ENV
            ),
        ));
    };
    Ok((actor.label(), api, pool, key))
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn clean_name(name: &str) -> Result<String, Response> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(field(
            "name",
            "Give it a name of at most 80 characters, so you can tell it apart.",
        ));
    }
    Ok(name.to_owned())
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn clean_events(wanted: &[String]) -> Result<Vec<String>, Response> {
    let mut out: Vec<String> = Vec::new();
    for e in wanted {
        if !events::is_kind(e) {
            return Err(field("events", format!("`{e}` is not an event.")));
        }
        if !out.contains(e) {
            out.push(e.clone());
        }
    }
    if out.is_empty() {
        return Err(field("events", "Choose at least one event."));
    }
    Ok(out)
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn clean_url(raw: &str, api: &ApiState) -> Result<String, Response> {
    let allow = api.config().webhooks_allow_private;
    let u = check_url(raw, allow).map_err(|e| field("url", e))?;
    check_resolved(&u, allow)
        .await
        .map_err(|e| field("url", e))?;
    Ok(u.to_string())
}

#[utoipa::path(
    get,
    path = "/api/v1/webhooks",
    operation_id = "webhooks_list",
    tag = "webhooks",
    summary = "Webhooks",
    description = "Admin only. Every webhook with the record of its last delivery (never its secret), and every event one can ask for.",
    responses(
        (status = 200, description = "The webhooks and the events.", body = Listed),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, _, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match model::list(pool, state.station.id).await {
        Ok(webhooks) => Json(Listed {
            webhooks,
            events: event_infos(),
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct NewBody {
    name: String,
    url: String,
    events: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/webhooks",
    operation_id = "webhooks_create",
    tag = "webhooks",
    summary = "Make a webhook",
    description = "Admin only. `name`, `url` and `events`. The address must be `https` and reach only public addresses, unless `[api] webhooks_allow_private` is on. The answer carries the signing secret (`whsec_…`) this once. At most 20 webhooks (409).",
    request_body = NewBody,
    responses(
        (status = 201, description = "The webhook, and its signing secret, shown once.", body = Made),
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
    Json(req): Json<NewBody>,
) -> Response {
    let (by, api, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let name = match clean_name(&req.name) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let events = match clean_events(&req.events) {
        Ok(e) => e,
        Err(r) => return r,
    };
    let url = match clean_url(&req.url, api).await {
        Ok(u) => u,
        Err(r) => return r,
    };
    match model::list(pool, state.station.id).await {
        Ok(all) if all.len() >= MAX_WEBHOOKS => {
            return problem(
                StatusCode::CONFLICT,
                format!("A station may have {MAX_WEBHOOKS} webhooks. Remove one first."),
            );
        }
        Ok(_) => {}
        Err(e) => return db_problem(e),
    }
    let id = Uuid::new_v4();
    let salt = *Uuid::new_v4().as_bytes();
    match model::create(
        pool,
        state.station.id,
        &NewWebhook {
            id,
            name: &name,
            url: &url,
            events: &events,
            salt: &salt,
            created_by: &by,
        },
    )
    .await
    {
        Ok(webhook) => {
            tracing::info!(webhook = %id, by, "webhook made");
            (
                StatusCode::CREATED,
                Json(Made {
                    secret: key.webhook_secret(id, &salt),
                    webhook,
                }),
            )
                .into_response()
        }
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    events: Option<Vec<String>>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[utoipa::path(
    put,
    path = "/api/v1/webhooks/{id}",
    operation_id = "webhooks_change",
    tag = "webhooks",
    summary = "Change a webhook",
    description = "Admin only. Any of `name`, `url`, `events` and `enabled`; a field left out is kept. A new address is checked as when the webhook is made.",
    params(("id" = Uuid, Path, description = "The webhook's id.")),
    request_body = ChangeBody,
    responses(
        (status = 200, description = "The webhook as it is now.", body = weavesignal_model::webhooks::Webhook),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
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
    Json(req): Json<ChangeBody>,
) -> Response {
    let (by, api, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let name = match req.name.as_deref().map(clean_name).transpose() {
        Ok(n) => n,
        Err(r) => return r,
    };
    let events = match req.events.as_deref().map(clean_events).transpose() {
        Ok(e) => e,
        Err(r) => return r,
    };
    let url = match &req.url {
        Some(u) => match clean_url(u, api).await {
            Ok(u) => Some(u),
            Err(r) => return r,
        },
        None => None,
    };
    match model::update(
        pool,
        state.station.id,
        id,
        name.as_deref(),
        url.as_deref(),
        events.as_deref(),
        req.enabled,
    )
    .await
    {
        Ok(Some(w)) => {
            tracing::info!(webhook = %id, by, "webhook changed");
            Json(w).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such webhook"),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/webhooks/{id}/secret",
    operation_id = "webhooks_new_secret",
    tag = "webhooks",
    summary = "A new signing secret",
    description = "Admin only. A new signing secret, shown this once; the old one stops signing at once.",
    params(("id" = Uuid, Path, description = "The webhook's id.")),
    responses(
        (status = 200, description = "The webhook, and its new signing secret, shown once.", body = Made),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn new_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let (by, _, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let salt = *Uuid::new_v4().as_bytes();
    match model::rotate(pool, state.station.id, id, &salt).await {
        Ok(Some(webhook)) => {
            tracing::info!(webhook = %id, by, "webhook secret replaced");
            Json(Made {
                secret: key.webhook_secret(id, &salt),
                webhook,
            })
            .into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such webhook"),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/webhooks/{id}",
    operation_id = "webhooks_remove",
    tag = "webhooks",
    summary = "Remove a webhook",
    description = "Admin only. It is told about nothing more; deliveries waiting to be retried are given up.",
    params(("id" = Uuid, Path, description = "The webhook's id.")),
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
    let (by, _, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match model::delete(pool, state.station.id, id).await {
        Ok(true) => {
            tracing::info!(webhook = %id, by, "webhook removed");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such webhook"),
        Err(e) => db_problem(e),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(list, create, change, new_secret, remove))]
pub(crate) struct Api;

// ---- delivery ----

/// Start the worker that delivers events to webhooks. `backoff` is the wait before each retry
/// ([`BACKOFF`] in service).
///
/// **A dead receiver blocks only itself.** Each webhook has its own [`PER_WEBHOOK`] places for
/// deliveries waiting (in an attempt or in back-off); an event that finds its webhook's places
/// full is dropped for that webhook alone, and recorded as a failure on it. The [`IN_FLIGHT`]
/// slots shared by every webhook are held only for the length of one attempt, never through
/// a back-off, so a receiver that never answers costs the others at most a wait of one
/// attempt's timeout.
pub fn spawn_worker(staff: Arc<Staff>, backoff: Vec<Duration>) {
    let api = &staff.api;
    let (Some(pool), Some(station), Some(_)) = (api.db(), api.station(), api.key()) else {
        tracing::info!("webhooks are off: they need the database and a token key");
        return;
    };
    let (pool, station) = (pool.clone(), station);
    let mut rx = api.events.subscribe();
    let slots = Arc::new(Semaphore::new(IN_FLIGHT));
    let mut places: HashMap<Uuid, Arc<Semaphore>> = HashMap::new();
    tokio::spawn(async move {
        loop {
            let event = match rx.recv().await {
                Ok(e) => e,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        missed = n,
                        "webhooks: events came faster than they were taken; the oldest were dropped"
                    );
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let hooks = match model::list(&pool, station).await {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(error = %e, "webhooks: cannot read them");
                    continue;
                }
            };
            // A webhook removed is forgotten here too, so the map is never larger than the list.
            places.retain(|id, _| hooks.iter().any(|h| h.id == *id));
            for hook in hooks
                .into_iter()
                .filter(|h| h.enabled && h.events.iter().any(|e| e == event.kind))
            {
                let place = places
                    .entry(hook.id)
                    .or_insert_with(|| Arc::new(Semaphore::new(PER_WEBHOOK)))
                    .clone();
                let Ok(place) = place.try_acquire_owned() else {
                    tracing::warn!(webhook = %hook.id, "webhooks: this receiver has too many deliveries waiting; one was dropped");
                    let error = format!(
                        "dropped a delivery: {PER_WEBHOOK} were already waiting for this receiver"
                    );
                    if let Err(e) = model::record(&pool, hook.id, 0, false, Some(&error)).await {
                        tracing::warn!(error = %e, "webhooks: cannot record a delivery");
                    }
                    continue;
                };
                let (staff, pool, event, backoff, slots) = (
                    staff.clone(),
                    pool.clone(),
                    event.clone(),
                    backoff.clone(),
                    slots.clone(),
                );
                tokio::spawn(async move {
                    deliver(&staff, &pool, station, hook, &event, &backoff, &slots).await;
                    drop(place);
                });
            }
        }
    });
}

/// One event to one webhook, with its retries, each attempt recorded. Before a retry the
/// webhook is read again: one removed or turned off is given up, and one whose secret or
/// address changed is retried with the new.
async fn deliver(
    staff: &Staff,
    pool: &sqlx::PgPool,
    station: weavesignal_model::StationId,
    mut hook: Webhook,
    event: &Event,
    backoff: &[Duration],
    slots: &Semaphore,
) {
    let Some(key) = staff.api.key() else { return };
    let body = match serde_json::to_vec(event) {
        Ok(b) => Arc::new(b),
        Err(_) => return,
    };
    let allow = staff.api.config().webhooks_allow_private;
    for attempt in 0..=backoff.len() {
        if attempt > 0 {
            tokio::time::sleep(backoff[attempt - 1]).await;
            match model::get(pool, station, hook.id).await {
                Ok(Some(now)) if now.enabled && now.events.iter().any(|e| e == event.kind) => {
                    hook = now;
                }
                Ok(_) => return,
                Err(e) => {
                    tracing::warn!(error = %e, "webhooks: cannot read one back before a retry");
                }
            }
        }
        let secret = key.webhook_secret(hook.id, &hook.salt);
        let sent = {
            // A shared slot for the attempt only: never held through a back-off.
            let Ok(_slot) = slots.acquire().await else {
                return;
            };
            let timestamp = now_ms() / 1000;
            let sig = delivery_signature(&secret, timestamp, &body);
            let (url, kind, id, body) = (hook.url.clone(), event.kind, event.id, body.clone());
            tokio::task::spawn_blocking(move || send(allow, &url, kind, id, timestamp, &sig, &body))
                .await
        };
        let (status, ok, error) = match sent {
            Ok(Ok(code)) if (200..300).contains(&code) => (i32::from(code), true, None),
            Ok(Ok(code)) => (i32::from(code), false, Some(format!("answered {code}"))),
            Ok(Err(e)) => (0, false, Some(e)),
            Err(e) => (0, false, Some(e.to_string())),
        };
        if let Err(e) = model::record(pool, hook.id, status, ok, error.as_deref()).await {
            tracing::warn!(error = %e, "webhooks: cannot record a delivery");
        }
        if ok {
            return;
        }
        tracing::info!(webhook = %hook.id, attempt = attempt + 1, status, "webhook delivery failed");
    }
}

/// One attempt: the status the receiver answered with, or what went wrong.
fn send(
    allow_private: bool,
    url: &str,
    kind: &str,
    id: Uuid,
    timestamp: u64,
    sig: &str,
    body: &[u8],
) -> Result<u16, String> {
    let mut res = agent(allow_private)
        .post(url)
        .header("Content-Type", "application/json")
        .header("X-WeaveSignal-Event", kind)
        .header("X-WeaveSignal-Delivery", &id.to_string())
        .header("X-WeaveSignal-Timestamp", &timestamp.to_string())
        .header(crate::api::SIGNATURE_HEADER.as_str(), sig)
        .send(body)
        .map_err(|e| e.to_string())?;
    // The answer's body is of no interest; read it so the connection ends cleanly.
    let _ = res.body_mut().read_to_vec();
    Ok(res.status().as_u16())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> bool {
        is_public(s.parse().unwrap())
    }

    #[test]
    fn private_loopback_and_reserved_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "127.9.9.9",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.5",
            "198.51.100.5",
            "203.0.113.5",
            "198.18.0.1",
            "192.0.0.8",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "2002:7f00:1::1",
            "2001::1",
            "2001:db8::1",
            "fec0::1",
            "::7f00:1",
            "::a00:1",
            "::a9fe:a9fe",
            "::ffff:169.254.169.254",
            "::ffff:0:7f00:1",
            "::ffff:0:808:808",
            "64:ff9b:1::808:808",
        ] {
            assert!(!v4(ip), "{ip} should not be public");
        }
    }

    #[test]
    fn ordinary_public_addresses_are_public() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "172.15.0.1",
            "172.32.0.1",
            "100.63.0.1",
            "100.128.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4009::200e",
            "::ffff:8.8.8.8",
        ] {
            assert!(v4(ip), "{ip} should be public");
        }
    }

    #[test]
    fn a_webhook_must_use_https_to_a_public_address() {
        assert!(check_url("https://hooks.example/weavesignal", false).is_ok());
        assert!(check_url("https://8.8.8.8/x", false).is_ok());
        for bad in [
            "http://hooks.example/x",
            "ftp://hooks.example/x",
            "not a url",
            "https://user:pass@hooks.example/x",
            "https://127.0.0.1/x",
            "https://10.0.0.5:8443/x",
            "https://[::1]/x",
            "https://localhost/x",
            "https://169.254.169.254/latest/meta-data",
            "http://127.0.0.1:8080/x",
        ] {
            assert!(check_url(bad, false).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn with_the_guard_lifted_loopback_may_use_http_and_nothing_else_may() {
        assert!(check_url("http://127.0.0.1:23400/hook", true).is_ok());
        assert!(check_url("http://localhost:23400/hook", true).is_ok());
        assert!(check_url("https://10.0.0.5/hook", true).is_ok());
        assert!(check_url("http://10.0.0.5/hook", true).is_err());
        assert!(check_url("http://hooks.example/hook", true).is_err());
    }

    /// The guard holds at delivery, not only when the webhook is made: a webhook that points at
    /// a private address (a name that changed its answer, a row written before the setting
    /// changed) connects to nothing.
    #[tokio::test]
    async fn a_delivery_to_a_private_address_connects_to_nothing() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        for url in [
            format!("https://127.0.0.1:{port}/hook"),
            format!("http://127.0.0.1:{port}/hook"),
            format!("https://localhost:{port}/hook"),
        ] {
            let sent = tokio::task::spawn_blocking({
                let url = url.clone();
                move || {
                    send(
                        false,
                        &url,
                        "on_air.changed",
                        Uuid::nil(),
                        0,
                        "sha256=00",
                        b"{}",
                    )
                }
            })
            .await
            .unwrap();
            assert!(sent.is_err(), "{url} was reached: {sent:?}");
        }
        let accepted = tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
        assert!(
            accepted.is_err(),
            "something connected to the private address"
        );
        // With the guard lifted, the same address is reached (the receiver answers nothing,
        // so the attempt fails, but it got there).
        let reached = tokio::task::spawn_blocking(move || {
            send(
                true,
                &format!("http://127.0.0.1:{port}/hook"),
                "on_air.changed",
                Uuid::nil(),
                0,
                "sha256=00",
                b"{}",
            )
        });
        let accepted = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await;
        assert!(accepted.is_ok(), "the lifted guard still refused");
        drop(accepted);
        let _ = reached.await;
    }

    #[test]
    fn a_resolver_guard_drops_private_answers() {
        // The guard is the same function the resolver uses.
        let all: Vec<SocketAddr> = vec![
            "10.0.0.1:443".parse().unwrap(),
            "93.184.216.34:443".parse().unwrap(),
        ];
        let kept: Vec<_> = all.iter().filter(|a| is_public(a.ip())).collect();
        assert_eq!(kept.len(), 1);
    }
}
