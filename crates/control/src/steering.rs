// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! HLS content steering (M7, ADR-0051): the steering manifest that tells each viewer's player
//! which edge to fetch from, as routes of their own, merged into the control plane's router in
//! [`crate::serve`].
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/steering` | public: the steering manifest (HLS Content Steering 1.2b1; RFC 8216bis section 7) |
//! | `GET /api/v1/steering/edges` | an admin (the station key or an admin's session): every edge with its health and how many viewers were sent to it first |
//!
//! The multivariant playlist names this route in `EXT-X-CONTENT-STEERING:SERVER-URI`. Each
//! answer orders the healthy edges for one viewer, by weight, and keeps that viewer on the same
//! edge from one reload to the next: the first answer gives the viewer a session in its
//! `RELOAD-URI`, and the order is a weighted rendezvous hash of that session, so an edge going
//! down moves only its own viewers. An edge whose health check fails `down_after` times in a
//! row leaves the order; one passing check brings it back. Edges of weight 0 (a CDN such as
//! Bunny, ADR-0017, or the origin) always come after the pool, in the order written.
//!
//! An edge with a `host` is sent as a pathway clone of `clone_from` (the pathway the
//! multivariant playlist declares), so the pool lives in these settings alone; an edge without
//! one must be declared in the playlist under its pathway. A clone replaces the host name
//! only (RFC 8216bis 7.3; hls.js 1.7.3 sets `url.hostname`): the scheme and the port stay the
//! playlist's, so every edge serves on the same port as the station.
//!
//! The query parameters players add (`_HLS_pathway`, `_HLS_throughput`) are accepted and not
//! used: the order depends on health and weight only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::app::problem;
use crate::config::ConfigError;

/// The steering settings (`[steering]`). With no edges, steering is off and its route
/// answers 404.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
#[serde(deny_unknown_fields, default)]
pub struct SteeringConfig {
    /// How long a player keeps a manifest before asking again, in seconds; the specification
    /// recommends 300.
    pub ttl_s: u64,
    /// How often each edge's health check runs, in seconds.
    pub check_every_s: u64,
    /// How long one check may take, in milliseconds.
    pub check_timeout_ms: u64,
    /// Failed checks in a row before an edge leaves the order.
    pub down_after: u32,
    /// The pathway the multivariant playlist declares, which edges with a `host` are cloned
    /// from; `.` when its variants carry no `PATHWAY-ID`. Empty: no clones are sent.
    pub clone_from: String,
    pub edges: Vec<EdgeConfig>,
}

impl Default for SteeringConfig {
    fn default() -> Self {
        Self {
            ttl_s: 300,
            check_every_s: 5,
            check_timeout_ms: 2000,
            down_after: 2,
            clone_from: ".".into(),
            edges: Vec::new(),
        }
    }
}

/// One edge, or one other pathway (a CDN, the origin).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeConfig {
    /// Its pathway ID: letters, digits, `.`, `-` and `_`.
    pub pathway: String,
    /// The host name that replaces the playlist's in this pathway's clone, such as
    /// `edge-1.edges.example`; never a port, which players keep from the playlist. Empty when
    /// the playlist declares the pathway itself.
    #[serde(default)]
    pub host: String,
    /// Its share of new viewers against the other edges; 0 puts it after the pool, as a last
    /// resort.
    #[serde(default = "one")]
    pub weight: u32,
    /// A plain-HTTP address answering 2xx while the edge is healthy (the edge image's
    /// `/health` on its HTTP port); empty, the edge is never checked and always counted up.
    #[serde(default)]
    pub check_url: String,
}

fn one() -> u32 {
    1
}

/// The steering route, as the multivariant playlist names it.
pub const STEERING_PATH: &str = "/api/v1/steering";

/// A pathway ID as the specification allows: non-empty, from `[A-Za-z0-9._-]`.
pub fn is_pathway_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl SteeringConfig {
    pub fn enabled(&self) -> bool {
        !self.edges.is_empty()
    }

    /// The line a multivariant playlist carries for players to ask this route which edge to
    /// use (ADR-0159); `None` while steering is off. `SERVER-URI` is a path on the host the
    /// playlist came from, so the route must be published beside `/hls/`. `PATHWAY-ID` is
    /// the pathway the edges are cloned from.
    pub fn playlist_line(&self) -> Option<String> {
        if !self.enabled() {
            return None;
        }
        let mut line = format!("#EXT-X-CONTENT-STEERING:SERVER-URI=\"{STEERING_PATH}\"");
        if !self.clone_from.is_empty() {
            line.push_str(&format!(",PATHWAY-ID=\"{}\"", self.clone_from));
        }
        Some(line)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |m: String| Err(ConfigError::Invalid(m));
        if !(1..=86_400).contains(&self.ttl_s) {
            return bad(format!(
                "`steering.ttl_s` is {}; a manifest lives 1 to 86400 s (300 recommended)",
                self.ttl_s
            ));
        }
        if !(1..=300).contains(&self.check_every_s) {
            return bad(format!(
                "`steering.check_every_s` is {}; check every 1 to 300 s",
                self.check_every_s
            ));
        }
        if !(100..=self.check_every_s * 1000).contains(&self.check_timeout_ms) {
            return bad(format!(
                "`steering.check_timeout_ms` is {}; a check takes 100 ms at least and no longer than `check_every_s`",
                self.check_timeout_ms
            ));
        }
        if !(1..=20).contains(&self.down_after) {
            return bad(format!(
                "`steering.down_after` is {}; 1 to 20 failed checks",
                self.down_after
            ));
        }
        if !self.clone_from.is_empty() && !is_pathway_id(&self.clone_from) {
            return bad(format!(
                "`steering.clone_from` is `{}`, not a pathway ID (letters, digits, `.`, `-`, `_`)",
                self.clone_from
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.edges {
            if !is_pathway_id(&e.pathway) {
                return bad(format!(
                    "`steering.edges` has pathway `{}`; a pathway ID is letters, digits, `.`, `-` and `_`",
                    e.pathway
                ));
            }
            if !seen.insert(e.pathway.as_str()) {
                return bad(format!(
                    "`steering.edges` names pathway `{}` twice",
                    e.pathway
                ));
            }
            if !e.host.is_empty() {
                if self.clone_from.is_empty() {
                    return bad(format!(
                        "edge `{}` has a `host`, which is sent as a clone of `steering.clone_from`; set `clone_from`, or declare the pathway in the playlist and leave `host` out",
                        e.pathway
                    ));
                }
                if e.pathway == self.clone_from {
                    return bad(format!(
                        "edge `{}` is the pathway it would be cloned from; leave its `host` out",
                        e.pathway
                    ));
                }
                // A name or an IPv4 address; an IPv6 address in brackets.
                let ok = match e.host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
                    Some(v6) => v6.bytes().all(|b| b.is_ascii_hexdigit() || b == b':'),
                    None => e
                        .host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')),
                };
                if !ok {
                    return bad(format!(
                        "edge `{}` has `host = \"{}\"`; write a host name only, such as `edge-1.edges.example`, without a scheme, port or path: players replace the host name and keep the playlist's port",
                        e.pathway, e.host
                    ));
                }
            }
            if !e.check_url.is_empty()
                && (!e.check_url.starts_with("http://")
                    || e.check_url.chars().any(char::is_whitespace))
            {
                return bad(format!(
                    "edge `{}` has `check_url = \"{}\"`; health checks speak plain HTTP to the edge's HTTP port, such as `http://edge-1.edges.example/health`",
                    e.pathway, e.check_url
                ));
            }
        }
        Ok(())
    }
}

/// What the control plane knows of one edge's health.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[schema(as = EdgeHealth)]
pub struct Health {
    /// In the order: checked and passing, or never checked.
    pub up: bool,
    /// Failed checks in a row.
    pub failures: u32,
    pub last_check: Option<Timestamp>,
    /// The last failure's reason, cleared by a pass.
    pub last_error: Option<String>,
    /// How long the last check took, in milliseconds.
    pub last_ms: Option<u64>,
    /// When `up` last changed (or the control plane started).
    pub since: Timestamp,
}

pub struct SteeringState {
    pub config: SteeringConfig,
    pub admin_token: Option<String>,
    /// The station's staff, so an admin's session opens the edges' view as the station key
    /// does (issue 7; ADR-0127). Empty until [`Self::with_staff`].
    staff: Arc<crate::staff::Staff>,
    health: Vec<Mutex<Health>>,
    /// Manifests that put each edge first.
    first: Vec<AtomicU64>,
}

impl SteeringState {
    /// The station's staff, whose admins may read the edges' view.
    pub fn with_staff(mut self, staff: Arc<crate::staff::Staff>) -> Self {
        self.staff = staff;
        self
    }
}

impl SteeringState {
    pub fn new(config: SteeringConfig, admin_token: Option<String>) -> Self {
        let now = Timestamp::now();
        let n = config.edges.len();
        Self {
            config,
            admin_token,
            staff: Arc::new(crate::staff::Staff::new(None)),
            // Up until a check says otherwise, so viewers are steered from the first moment.
            health: (0..n)
                .map(|_| {
                    Mutex::new(Health {
                        up: true,
                        failures: 0,
                        last_check: None,
                        last_error: None,
                        last_ms: None,
                        since: now,
                    })
                })
                .collect(),
            first: (0..n).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    fn health(&self, i: usize) -> Health {
        self.health[i]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Record one check's result; returns the new `up` when it changed.
    pub fn record(&self, i: usize, result: Result<u64, String>) -> Option<bool> {
        let mut h = self.health[i]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Timestamp::now();
        h.last_check = Some(now);
        let was = h.up;
        match result {
            Ok(ms) => {
                h.failures = 0;
                h.last_error = None;
                h.last_ms = Some(ms);
                h.up = true;
            }
            Err(e) => {
                h.failures = h.failures.saturating_add(1);
                h.last_error = Some(e);
                h.last_ms = None;
                if h.failures >= self.config.down_after {
                    h.up = false;
                }
            }
        }
        (h.up != was).then(|| {
            h.since = now;
            h.up
        })
    }

    /// The pathways for one viewer, most preferred first: the healthy edges of the pool in
    /// weighted rendezvous order for `session`, then the last resorts (weight 0) in the
    /// order written. With nothing healthy, every edge in the pool, so the manifest is never
    /// empty and players fall back on their own penalties.
    pub fn order(&self, session: &str) -> Vec<usize> {
        let edges = &self.config.edges;
        let ranked = |up_only: bool| {
            let mut pool: Vec<(f64, usize)> = edges
                .iter()
                .enumerate()
                .filter(|(i, e)| e.weight > 0 && (!up_only || self.health(*i).up))
                .map(|(i, e)| (score(session, &e.pathway, e.weight), i))
                .collect();
            pool.sort_by(|a, b| b.0.total_cmp(&a.0));
            pool.into_iter().map(|(_, i)| i).collect::<Vec<_>>()
        };
        let mut order = ranked(true);
        order.extend(
            edges
                .iter()
                .enumerate()
                .filter(|(i, e)| e.weight == 0 && self.health(*i).up)
                .map(|(i, _)| i),
        );
        if order.is_empty() {
            order = ranked(false);
            order.extend(
                edges
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.weight == 0)
                    .map(|(i, _)| i),
            );
        }
        order
    }

    /// The steering manifest for one viewer.
    pub fn manifest(&self, session: &str) -> Value {
        let order = self.order(session);
        if let Some(&first) = order.first() {
            self.first[first].fetch_add(1, Ordering::Relaxed);
        }
        let edges = &self.config.edges;
        let priority: Vec<&str> = order.iter().map(|&i| edges[i].pathway.as_str()).collect();
        let clones: Vec<Value> = order
            .iter()
            .map(|&i| &edges[i])
            .filter(|e| !e.host.is_empty())
            .map(|e| {
                json!({
                    "BASE-ID": self.config.clone_from,
                    "ID": e.pathway,
                    "URI-REPLACEMENT": { "HOST": e.host },
                })
            })
            .collect();
        let mut m = json!({
            "VERSION": 1,
            "TTL": self.config.ttl_s,
            "RELOAD-URI": format!("steering?session={session}"),
            "PATHWAY-PRIORITY": priority,
        });
        if !clones.is_empty() {
            m["PATHWAY-CLONES"] = Value::Array(clones);
        }
        m
    }
}

/// A weighted rendezvous score in (0, inf): the highest wins. Stable for a session and an
/// edge, so a viewer keeps its edge, and removing one edge moves only the viewers it had.
fn score(session: &str, pathway: &str, weight: u32) -> f64 {
    let mut h = Sha256::new();
    h.update(session.as_bytes());
    h.update([0]);
    h.update(pathway.as_bytes());
    let d = h.finalize();
    let x = u64::from_be_bytes(d[..8].try_into().expect("eight bytes"));
    // A uniform draw in (0, 1), never 0 or 1.
    let u = ((x >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    f64::from(weight) / -u.ln()
}

/// A session a player sent back, if it is one this route could have made.
fn session_from(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "session")
        .map(|(_, v)| v)
        .filter(|v| {
            (1..=64).contains(&v.len())
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .map(str::to_owned)
}

pub fn router(state: Arc<SteeringState>) -> Router {
    Router::new()
        .route(STEERING_PATH, get(manifest))
        .route("/api/v1/steering/edges", get(edges))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/v1/steering",
    operation_id = "steering_manifest",
    tag = "delivery",
    summary = "The content-steering manifest",
    description = "HLS content steering (RFC 8216bis section 7): which edge a player takes first, per viewer. Never cached; CORS open to any origin. 404 while no edges are configured.",
    params(("session" = Option<String>, Query, description = "A session a player sent back from `RELOAD-URI`, so the viewer keeps its edge")),
    responses(
        (status = 200, description = "The manifest.", body = SteeringManifest),
        (status = 404, response = crate::openapi::NotFound),
    ),
    security(()),
)]
async fn manifest(State(state): State<Arc<SteeringState>>, RawQuery(q): RawQuery) -> Response {
    if !state.config.enabled() {
        return problem(
            StatusCode::NOT_FOUND,
            "content steering is off: no edges are configured (`steering.edges`)",
        );
    }
    let session =
        session_from(q.as_deref()).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let mut res = Json(state.manifest(&session)).into_response();
    let h = res.headers_mut();
    // Never cached on the way: each viewer's answer is its own, and players add a throughput
    // figure to the query that would split any cache (RFC 8216bis 7.4).
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    res
}

#[derive(Serialize, utoipa::ToSchema)]
struct EdgeView<'a> {
    pathway: &'a str,
    host: &'a str,
    weight: u32,
    checked: bool,
    /// Manifests since the control plane started that put this edge first.
    first: u64,
    #[serde(flatten)]
    health: Health,
}

#[utoipa::path(
    get,
    path = "/api/v1/steering/edges",
    operation_id = "steering_edges",
    tag = "delivery",
    summary = "The edges' health",
    description = "The station key only (not a staff session).",
    responses(
        (status = 200, description = "The edges.", body = EdgesBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn edges(State(state): State<Arc<SteeringState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let edges: Vec<EdgeView> = state
        .config
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| EdgeView {
            pathway: &e.pathway,
            host: &e.host,
            weight: e.weight,
            checked: !e.check_url.is_empty(),
            first: state.first[i].load(Ordering::Relaxed),
            health: state.health(i),
        })
        .collect();
    Json(json!({
        "enabled": state.config.enabled(),
        "ttl_s": state.config.ttl_s,
        "edges": edges,
    }))
    .into_response()
}

/// `None` when the request comes from an admin: the station key or an admin's session, as
/// `app::admin_refusal` (issue 7: this route took the station key only).
fn refusal(state: &SteeringState, headers: &HeaderMap) -> Option<Response> {
    crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        crate::staff::Need::Admin,
    )
    .err()
}

/// One health check: a GET that answers 2xx within the timeout. The time it took, or why not.
fn check(url: &str, timeout: Duration) -> Result<u64, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let t0 = Instant::now();
    let res = agent.get(url).call().map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    if (200..300).contains(&status) {
        Ok(u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX))
    } else {
        Err(format!("answered {status}"))
    }
}

/// Check every edge that has a `check_url`, every `check_every_s`, until the process ends.
pub fn spawn_checker(state: Arc<SteeringState>) {
    let checked: Vec<usize> = (0..state.config.edges.len())
        .filter(|&i| !state.config.edges[i].check_url.is_empty())
        .collect();
    if checked.is_empty() {
        return;
    }
    let every = Duration::from_secs(state.config.check_every_s);
    let timeout = Duration::from_millis(state.config.check_timeout_ms);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let mut set = tokio::task::JoinSet::new();
            for &i in &checked {
                let url = state.config.edges[i].check_url.clone();
                set.spawn_blocking(move || (i, check(&url, timeout)));
            }
            while let Some(done) = set.join_next().await {
                let Ok((i, result)) = done else { continue };
                let reason = result.as_ref().err().cloned();
                let pathway = &state.config.edges[i].pathway;
                match state.record(i, result) {
                    Some(true) => {
                        tracing::info!(pathway, "edge healthy again; back in the steering order")
                    }
                    Some(false) => tracing::warn!(
                        pathway,
                        reason = reason.as_deref().unwrap_or(""),
                        "edge failed its health checks; out of the steering order"
                    ),
                    None => {}
                }
            }
        }
    });
}

// ---- the description (ADR-0195) ----

/// The steering manifest (HLS content steering): the keys are the specification's.
#[derive(Serialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct SteeringManifest {
    #[serde(rename = "VERSION")]
    version: u32,
    #[serde(rename = "TTL")]
    ttl: u32,
    #[serde(rename = "RELOAD-URI")]
    reload_uri: String,
    #[serde(rename = "PATHWAY-PRIORITY")]
    pathway_priority: Vec<String>,
    /// Present while an edge has a host: `BASE-ID`, `ID` and `URI-REPLACEMENT` (`HOST`).
    #[serde(rename = "PATHWAY-CLONES", skip_serializing_if = "Option::is_none")]
    pathway_clones: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct EdgesBody<'a> {
    enabled: bool,
    ttl_s: u32,
    edges: Vec<EdgeView<'a>>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(manifest, edges))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_view_takes_an_admins_session_as_well_as_the_station_key() {
        use weavesignal_model::staff::Role;
        let staff = Arc::new(crate::staff::Staff::new(None));
        let admin = staff.signed_in_for_test(Role::Admin);
        let presenter = staff.signed_in_for_test(Role::Presenter);
        let s = SteeringState::new(pool(), Some("key".into())).with_staff(staff);
        let with = |b: &str| {
            let mut h = HeaderMap::new();
            h.insert(
                header::AUTHORIZATION,
                format!("Bearer {b}").parse().unwrap(),
            );
            refusal(&s, &h).map(|r| r.status())
        };
        assert_eq!(with("key"), None);
        assert_eq!(with(&admin), None);
        assert_eq!(with(&presenter), Some(StatusCode::FORBIDDEN));
        assert_eq!(with("nope"), Some(StatusCode::UNAUTHORIZED));
    }

    fn edge(pathway: &str, weight: u32, host: &str, check: bool) -> EdgeConfig {
        EdgeConfig {
            pathway: pathway.into(),
            host: host.into(),
            weight,
            check_url: if check {
                format!("http://127.0.0.1:9/{pathway}")
            } else {
                String::new()
            },
        }
    }

    fn pool() -> SteeringConfig {
        SteeringConfig {
            edges: vec![
                edge("edge-1", 1, "edge-1.edges.example", true),
                edge("edge-2", 1, "edge-2.edges.example", true),
                edge("bunny", 0, "kettle.b-cdn.example", false),
            ],
            ..SteeringConfig::default()
        }
    }

    #[test]
    fn the_defaults_keep_steering_off_and_validate() {
        let c = SteeringConfig::default();
        assert!(!c.enabled());
        assert_eq!(c.ttl_s, 300, "the specification's recommendation");
        c.validate().unwrap();
        pool().validate().unwrap();
    }

    #[test]
    fn a_manifest_has_the_required_fields_and_clones() {
        let s = SteeringState::new(pool(), None);
        let m = s.manifest("abc");
        assert_eq!(m["VERSION"], 1);
        assert_eq!(m["TTL"], 300);
        assert_eq!(m["RELOAD-URI"], "steering?session=abc");
        let p: Vec<&str> = m["PATHWAY-PRIORITY"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(p.len(), 3);
        assert_eq!(p[2], "bunny", "a weight of 0 comes after the pool");
        let clones = m["PATHWAY-CLONES"].as_array().unwrap();
        assert_eq!(clones.len(), 3);
        assert_eq!(clones[0]["BASE-ID"], ".");
        assert!(
            clones
                .iter()
                .any(|c| c["ID"] == "bunny"
                    && c["URI-REPLACEMENT"]["HOST"] == "kettle.b-cdn.example")
        );
    }

    #[test]
    fn a_session_keeps_its_edge_and_the_pool_is_shared_by_weight() {
        let mut c = pool();
        c.edges[1].weight = 3;
        let s = SteeringState::new(c, None);
        assert_eq!(s.order("viewer-7"), s.order("viewer-7"));
        let n = 4000;
        let second = (0..n).filter(|i| s.order(&format!("v{i}"))[0] == 1).count();
        // Three quarters, give or take sampling.
        assert!((2800..3200).contains(&second), "{second} of {n}");
    }

    #[test]
    fn an_unhealthy_edge_leaves_the_order_after_down_after_failures_and_only_its_viewers_move() {
        let s = SteeringState::new(pool(), None);
        let before: Vec<Vec<usize>> = (0..200).map(|i| s.order(&format!("v{i}"))).collect();
        assert_eq!(
            s.record(0, Err("refused".into())),
            None,
            "one failure is not enough"
        );
        assert!(s.order("v1").contains(&0));
        assert_eq!(s.record(0, Err("refused".into())), Some(false));
        for (i, was) in before.iter().enumerate() {
            let now = s.order(&format!("v{i}"));
            assert!(!now.contains(&0), "edge-1 is out");
            if was[0] == 1 {
                assert_eq!(now[0], 1, "a viewer of edge-2 stays on edge-2");
            }
        }
        assert_eq!(s.record(0, Ok(3)), Some(true), "one pass brings it back");
        assert!(s.order("v1").contains(&0));
    }

    #[test]
    fn with_nothing_healthy_the_manifest_still_names_every_edge() {
        let mut c = pool();
        c.edges.pop();
        let s = SteeringState::new(c, None);
        for i in 0..2 {
            s.record(i, Err("x".into()));
            s.record(i, Err("x".into()));
        }
        let mut order = s.order("v");
        order.sort_unstable();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn a_checked_last_resort_is_dropped_when_down() {
        let mut c = pool();
        c.edges[2].check_url = "http://127.0.0.1:9/".into();
        let s = SteeringState::new(c, None);
        s.record(2, Err("x".into()));
        s.record(2, Err("x".into()));
        assert!(!s.order("v").contains(&2));
    }

    #[test]
    fn bad_settings_are_refused_with_the_key_named() {
        type Change = Box<dyn Fn(&mut SteeringConfig)>;
        let cases: Vec<(Change, &str)> = vec![
            (Box::new(|c| c.ttl_s = 0), "steering.ttl_s"),
            (
                Box::new(|c| c.edges[0].pathway = "edge 1".into()),
                "pathway ID",
            ),
            (Box::new(|c| c.edges[1].pathway = "edge-1".into()), "twice"),
            (
                Box::new(|c| c.edges[0].host = "https://edge-1.example/".into()),
                "without a scheme",
            ),
            (
                Box::new(|c| c.edges[0].host = "edge-1.example:8443".into()),
                "keep the playlist's port",
            ),
            (
                Box::new(|c| c.edges[0].check_url = "https://edge-1.example/health".into()),
                "plain HTTP",
            ),
            (
                Box::new(|c| c.clone_from = String::new()),
                "set `clone_from`",
            ),
            (
                Box::new(|c| c.check_timeout_ms = 10_000),
                "check_timeout_ms",
            ),
            (Box::new(|c| c.down_after = 0), "down_after"),
        ];
        for (change, says) in cases {
            let mut c = pool();
            change(&mut c);
            let err = c.validate().unwrap_err().to_string();
            assert!(err.contains(says), "{err}");
        }
    }

    #[test]
    fn only_sessions_this_route_could_make_are_taken_back() {
        assert_eq!(
            session_from(Some(
                "session=ab_c-1&_HLS_pathway=edge-1&_HLS_throughput=5000000"
            )),
            Some("ab_c-1".into())
        );
        assert_eq!(session_from(Some("_HLS_pathway=edge-1")), None);
        assert_eq!(session_from(Some("session=%3Cx%3E")), None);
        assert_eq!(
            session_from(Some(&format!("session={}", "a".repeat(65)))),
            None
        );
        assert_eq!(session_from(None), None);
    }

    #[tokio::test]
    async fn the_route_answers_404_when_off_and_never_caches() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let off = router(Arc::new(SteeringState::new(
            SteeringConfig::default(),
            None,
        )));
        let res = off
            .oneshot(
                Request::get("/api/v1/steering")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        let on = router(Arc::new(SteeringState::new(pool(), Some("t".into()))));
        let res = on
            .clone()
            .oneshot(
                Request::get("/api/v1/steering?_HLS_pathway=.&_HLS_throughput=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
        let res = on
            .oneshot(
                Request::get("/api/v1/steering/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
