// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The public API's shared state and its gate (ADR-0195): API tokens, rate limits, the events
//! out, and the one middleware that holds them together.
//!
//! The state lives in [`Api`], which [`crate::staff::Staff`] carries because `Staff` is the one
//! thing every route's state already shares (the studio's, the shows', steering's).
//!
//! **The gate** ([`gate`]) is layered once, over the whole router, in [`crate::serve`]. For each
//! request it
//!
//! 1. answers a browser's preflight (`OPTIONS`) for the open routes and adds
//!    `Access-Control-Allow-Origin: *` to their answers ([`is_open`]: now-playing and
//!    `/api/v1/public/`, and nothing else);
//! 2. limits the open routes per address and, for the now-playing stream, how many streams one
//!    address holds at once;
//! 3. if the bearer is an API token (`rsa_…`), finds it, limits it per minute, looks up the
//!    scope the route needs in [`crate::scopes::TABLE`] (by method and the route as registered,
//!    so `/shows/{show}` and not `/shows/kettle`), and records the verdict as a [`Grant`] for
//!    the rest of the request.
//!
//! **Why a task-local.** The checks handlers already make (`refusal`, `admin_refusal`, and
//! `staff::check` underneath) see headers, not routes. So `staff::check` accepts a token only
//! when the gate has granted *this* bearer *this* route; it then answers [`Actor::Token`], which
//! is never an admin, so `admin_refusal` refuses it wherever it is called. Without the gate (a
//! router assembled without it) no token is ever accepted: the check fails closed.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::tokens::{self as model, ApiToken, StoredToken};

use crate::apikey::ApiKey;
use crate::app::{problem, same};
use crate::events::Events;
use crate::hub::now_ms;
use crate::staff::Staff;

/// `last_used_at` is written at most this often per token.
const TOUCH_EVERY_MS: u64 = 60_000;
/// The window the per-minute limits count over.
const WINDOW_MS: u64 = 60_000;

/// The `[api]` settings.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct ApiConfig {
    /// Let webhooks point at private, loopback and link-local addresses (an integration on the
    /// station's own network). Off: a webhook must reach a public address over HTTPS.
    pub webhooks_allow_private: bool,
    /// Requests per minute from one address to the open routes (now-playing and
    /// `/api/v1/public/`). 0 turns the limit off.
    pub public_per_minute: u32,
    /// Requests per minute with one API token. 0 turns the limit off.
    pub token_per_minute: u32,
    /// Now-playing streams (server-sent events) one address may hold open at once. 0 turns the
    /// limit off.
    pub streams_per_address: u32,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            webhooks_allow_private: false,
            public_per_minute: 120,
            token_per_minute: 600,
            streams_per_address: 4,
        }
    }
}

/// A token as the gate holds it live.
struct Live {
    token: ApiToken,
    hash: [u8; 32],
    /// When `last_used_at` was last written.
    touched_ms: u64,
}

/// The public API's state: the key, the live tokens, the limits and the events.
#[derive(Default)]
pub struct Api {
    db: Option<sqlx::PgPool>,
    station: Option<StationId>,
    key: Option<ApiKey>,
    config: ApiConfig,
    /// `join.forwarded_for`: the proxy's header counts as the caller's address.
    forwarded_for: bool,
    tokens: Mutex<HashMap<[u8; 32], Live>>,
    /// Per address on the open routes.
    limiter: Limiter,
    /// Per token: its own map, so a flood of addresses never resets a token's count.
    token_limiter: Limiter,
    streams: Mutex<HashMap<IpAddr, u32>>,
    pub events: Events,
}

impl Api {
    pub fn new(
        db: Option<sqlx::PgPool>,
        station: StationId,
        key: Option<ApiKey>,
        config: ApiConfig,
        forwarded_for: bool,
    ) -> Self {
        Self {
            db,
            station: Some(station),
            key,
            config,
            forwarded_for,
            ..Default::default()
        }
    }

    pub fn config(&self) -> &ApiConfig {
        &self.config
    }

    pub fn key(&self) -> Option<&ApiKey> {
        self.key.as_ref()
    }

    pub fn db(&self) -> Option<&sqlx::PgPool> {
        self.db.as_ref()
    }

    pub fn station(&self) -> Option<StationId> {
        self.station
    }

    /// Read the station's live tokens back at start. Without a key there is nothing to match
    /// them against, and none is loaded.
    pub async fn restore(&self) -> usize {
        let (Some(pool), Some(station), Some(_)) = (&self.db, self.station, &self.key) else {
            return 0;
        };
        match model::list(pool, station).await {
            Ok(all) => {
                let now = now_ms() as i64;
                let mut live = self.tokens.lock().expect("api tokens");
                live.clear();
                for s in all.into_iter().filter(|s| usable(&s.token, now)) {
                    live.insert(
                        s.secret_hash,
                        Live {
                            token: s.token,
                            hash: s.secret_hash,
                            touched_ms: 0,
                        },
                    );
                }
                live.len()
            }
            Err(e) => {
                tracing::warn!(error = %e, "cannot read back API tokens; none will work until the database answers and the control plane restarts");
                0
            }
        }
    }

    /// A token just made joins the live set; any that have expired since leave it.
    pub fn add(&self, s: StoredToken) {
        let mut live = self.tokens.lock().expect("api tokens");
        let now = now_ms() as i64;
        live.retain(|_, l| usable(&l.token, now));
        live.insert(
            s.secret_hash,
            Live {
                token: s.token,
                hash: s.secret_hash,
                touched_ms: 0,
            },
        );
    }

    /// A revoked token leaves it at once.
    pub fn remove(&self, id: Uuid) {
        self.tokens
            .lock()
            .expect("api tokens")
            .retain(|_, l| l.token.id != id);
    }

    /// Find the live token a secret names: its keyed hash looked up, then compared in constant
    /// time. `None` for an unknown, revoked or expired token, and without a key.
    fn find(&self, secret: &str, now: u64) -> Option<(ApiToken, [u8; 32])> {
        let key = self.key.as_ref()?;
        let hash = key.token_hash(secret);
        let mut live = self.tokens.lock().expect("api tokens");
        let l = live.get_mut(&hash)?;
        if !same(&l.hash, &hash) || !usable(&l.token, now as i64) {
            return None;
        }
        if now.saturating_sub(l.touched_ms) >= TOUCH_EVERY_MS {
            l.touched_ms = now;
            l.token.last_used_ms = Some(now as i64);
            if let Some(pool) = self.db.clone()
                && let Ok(rt) = tokio::runtime::Handle::try_current()
            {
                let id = l.token.id;
                rt.spawn(async move {
                    if let Err(e) = model::touch(&pool, id, now as i64).await {
                        tracing::warn!(error = %e, "cannot write when an API token was last used");
                    }
                });
            }
        }
        Some((l.token.clone(), hash))
    }

    /// The address a request comes from, for the per-address limits: an IPv6 address counts
    /// as its /64, the least a single subscriber is given, so one machine cannot step past the
    /// limit by walking its own prefix.
    fn peer(&self, headers: &HeaderMap, conn: Option<SocketAddr>) -> Option<IpAddr> {
        crate::public::peer(self.forwarded_for, headers, conn).map(per_subscriber)
    }
}

/// An address as the limits count it: IPv4 as it is, IPv6 as its /64.
fn per_subscriber(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

/// A token that has not been revoked and has not expired.
fn usable(t: &ApiToken, now_ms: i64) -> bool {
    t.revoked_ms.is_none() && t.expires_ms.is_none_or(|e| e > now_ms)
}

// ---- the grant ----

/// What the gate decided about the request's bearer, if it is an API token.
#[derive(Clone, Debug)]
pub enum Grant {
    /// Not a live token: unknown, revoked, expired, or there is no key.
    Invalid,
    /// A live token. `need` is the scope this route needs (`None`: no scope opens it);
    /// `allowed` is whether the token holds it.
    Token {
        id: Uuid,
        name: String,
        hash: [u8; 32],
        need: Option<&'static str>,
        allowed: bool,
    },
}

tokio::task_local! {
    static GRANT: Grant;
}

/// Who a token is, once the gate has granted it the route: its id and name, for the logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenActor {
    pub id: Uuid,
    pub name: String,
}

impl Api {
    /// What `staff::check` asks when the bearer starts with `rsa_`: the token, if the gate
    /// granted this bearer this route; otherwise the refusal.
    #[allow(clippy::result_large_err)] // the refusal is the response itself
    pub fn authorise(&self, given: &str) -> Result<TokenActor, Response> {
        let invalid = || {
            problem(
                StatusCode::UNAUTHORIZED,
                "That API token is not valid: it is unknown, revoked or expired.",
            )
        };
        let Some(key) = &self.key else {
            return Err(problem(
                StatusCode::UNAUTHORIZED,
                "API tokens are off here: the station has no token key (see WEAVESIGNAL_CONTROL_TOKEN_KEY).",
            ));
        };
        match GRANT.try_with(Clone::clone) {
            Ok(Grant::Token {
                id,
                name,
                hash,
                need,
                allowed,
            }) if same(&hash, &key.token_hash(given)) => {
                if allowed {
                    Ok(TokenActor { id, name })
                } else {
                    Err(match need {
                        Some(scope) => problem(
                            StatusCode::FORBIDDEN,
                            format!("This API token does not hold the `{scope}` scope."),
                        ),
                        None => problem(
                            StatusCode::FORBIDDEN,
                            "An API token cannot do that: no scope opens this route.",
                        ),
                    })
                }
            }
            _ => Err(invalid()),
        }
    }
}

// ---- the open routes ----

/// The routes open to any website: no bearer, and CORS to any origin. Nothing else is.
pub fn is_open(path: &str) -> bool {
    path == "/api/v1/now-playing"
        || path == "/api/v1/now-playing/events"
        || path.starts_with("/api/v1/public/")
}

const STREAM: &str = "/api/v1/now-playing/events";

fn cors(res: &mut Response) {
    let h = res.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("retry-after"),
    );
}

fn preflight() -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    let h = res.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("accept, cache-control, last-event-id"),
    );
    h.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    cors(&mut res);
    res
}

fn too_many(retry_s: u64, open: bool) -> Response {
    let mut res = problem(
        StatusCode::TOO_MANY_REQUESTS,
        "Too many requests just now. Wait a little and try again.",
    );
    res.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&retry_s.max(1).to_string()).expect("digits"),
    );
    if open {
        cors(&mut res);
    }
    res
}

// ---- the limits ----

/// The most keys one limiter holds. Past it, windows that have turned over are dropped; if
/// every key is still in its window (a flood from more addresses than this in one minute) the
/// counts start again, so memory stays bounded at the cost of that minute's limit.
const MAX_KEYS: usize = 16_384;

/// Per-minute counts by key, in fixed windows.
#[derive(Default)]
struct Limiter {
    windows: Mutex<HashMap<String, (u64, u32)>>,
}

impl Limiter {
    /// Count one request under `key`; `Err` is how many seconds until the window turns over.
    /// A `limit` of 0 is no limit.
    fn hit(&self, key: String, now: u64, limit: u32) -> Result<(), u64> {
        if limit == 0 {
            return Ok(());
        }
        let mut w = self.windows.lock().expect("rate limits");
        if w.len() >= MAX_KEYS && !w.contains_key(&key) {
            w.retain(|_, (start, _)| now.saturating_sub(*start) < WINDOW_MS);
            if w.len() >= MAX_KEYS {
                tracing::warn!(
                    keys = w.len(),
                    "rate limits: more addresses than can be counted in one minute; counting starts again"
                );
                w.clear();
            }
        }
        let e = w.entry(key).or_insert((now, 0));
        if now.saturating_sub(e.0) >= WINDOW_MS {
            *e = (now, 0);
        }
        if e.1 >= limit {
            return Err((WINDOW_MS - now.saturating_sub(e.0)).div_ceil(1000));
        }
        e.1 += 1;
        Ok(())
    }
}

/// An open now-playing stream, counted against its address until the response ends.
struct StreamGuard {
    staff: Arc<Staff>,
    addr: IpAddr,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let mut s = self.staff.api.streams.lock().expect("streams");
        if let Some(n) = s.get_mut(&self.addr) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                s.remove(&self.addr);
            }
        }
    }
}

// ---- the gate ----

/// The gate, layered once over the whole router (see the module's words).
pub async fn gate(State(staff): State<Arc<Staff>>, req: Request, next: Next) -> Response {
    let api = &staff.api;
    let now = now_ms();
    let method = req.method().clone();
    let open = is_open(req.uri().path());
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned());
    let addr = api.peer(
        req.headers(),
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0),
    );

    if open {
        if method == Method::OPTIONS {
            return preflight();
        }
        if let Some(a) = addr
            && let Err(wait) = api
                .limiter
                .hit(format!("a:{a}"), now, api.config.public_per_minute)
        {
            return too_many(wait, true);
        }
    }

    // A stream is counted from here to the end of its response.
    let mut guard = None;
    if open
        && req.uri().path() == STREAM
        && api.config.streams_per_address > 0
        && let Some(a) = addr
    {
        {
            let mut s = api.streams.lock().expect("streams");
            let n = s.entry(a).or_insert(0);
            if *n >= api.config.streams_per_address {
                return too_many(5, true);
            }
            *n += 1;
        }
        guard = Some(StreamGuard {
            staff: staff.clone(),
            addr: a,
        });
    }

    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|b| b.starts_with(model::TOKEN_PREFIX))
        .map(str::to_owned);
    let grant = match given {
        None => None,
        Some(secret) => Some(match api.find(&secret, now) {
            None => Grant::Invalid,
            Some((token, hash)) => {
                if let Err(wait) = api.token_limiter.hit(
                    format!("t:{}", token.id),
                    now,
                    api.config.token_per_minute,
                ) {
                    return too_many(wait, open);
                }
                let need = route
                    .as_deref()
                    .and_then(|r| crate::scopes::scope_for(&method, r));
                let allowed = need.is_some_and(|n| token.scopes.iter().any(|s| s == n));
                Grant::Token {
                    id: token.id,
                    name: token.name,
                    hash,
                    need,
                    allowed,
                }
            }
        }),
    };

    let mut res = match grant {
        Some(g) => GRANT.scope(g, next.run(req)).await,
        None => next.run(req).await,
    };
    if open && matches!(method, Method::GET | Method::HEAD) {
        cors(&mut res);
    }
    if let Some(guard) = guard {
        // The stream's body owns the guard: it is released when the client goes.
        let (parts, body) = res.into_parts();
        let stream = body.into_data_stream().map(move |chunk| {
            let _held = &guard;
            chunk
        });
        res = Response::from_parts(parts, axum::body::Body::from_stream(stream));
    }
    res
}

/// Layer the gate over a router: once, over everything (see the module's words).
pub fn layer(router: axum::Router, staff: Arc<Staff>) -> axum::Router {
    router.layer(axum::middleware::from_fn_with_state(staff, gate))
}

/// The header a delivery's signature is in.
pub const SIGNATURE_HEADER: HeaderName = HeaderName::from_static("x-weavesignal-signature");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_now_playing_and_the_public_prefix_are_open() {
        assert!(is_open("/api/v1/now-playing"));
        assert!(is_open("/api/v1/now-playing/events"));
        assert!(is_open("/api/v1/public/schedule"));
        assert!(!is_open("/api/v1/studio"));
        assert!(!is_open("/api/v1/player"));
        assert!(!is_open("/api/v1/publicity"));
        assert!(!is_open("/api/v1/now-playing-secret"));
        assert!(!is_open("/api/v1/tokens"));
    }

    #[test]
    fn the_limiter_counts_per_key_and_per_window() {
        let l = Limiter::default();
        assert!(l.hit("a".into(), 1_000, 2).is_ok());
        assert!(l.hit("a".into(), 1_100, 2).is_ok());
        assert_eq!(l.hit("a".into(), 1_200, 2), Err(60));
        assert!(l.hit("b".into(), 1_200, 2).is_ok());
        // The window turns over after a minute.
        assert!(l.hit("a".into(), 61_100, 2).is_ok());
        // 0 is no limit.
        for _ in 0..1000 {
            assert!(l.hit("c".into(), 1_000, 0).is_ok());
        }
    }

    #[test]
    fn the_limiter_stays_bounded_under_a_flood_of_addresses() {
        let l = Limiter::default();
        for i in 0..(MAX_KEYS * 2) {
            let _ = l.hit(format!("a:{i}"), 1_000, 5);
        }
        assert!(l.windows.lock().unwrap().len() <= MAX_KEYS);
    }

    #[test]
    fn an_ipv6_address_counts_as_its_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(per_subscriber(a), per_subscriber(b));
        assert_ne!(per_subscriber(a), per_subscriber(c));
        let v4: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(per_subscriber(v4), v4);
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        assert_eq!(per_subscriber(mapped), v4);
    }

    #[test]
    fn a_token_is_usable_until_revoked_or_expired() {
        let t = |expires, revoked| ApiToken {
            id: Uuid::nil(),
            name: "n".into(),
            scopes: vec![],
            prefix: "rsa_".into(),
            created_by: "x".into(),
            created_ms: 0,
            expires_ms: expires,
            last_used_ms: None,
            revoked_ms: revoked,
        };
        assert!(usable(&t(None, None), 5));
        assert!(usable(&t(Some(10), None), 5));
        assert!(!usable(&t(Some(5), None), 5));
        assert!(!usable(&t(None, Some(1)), 5));
    }
}
