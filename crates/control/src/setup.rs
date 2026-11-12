// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station's setup page and its look (ADR-0237).
//!
//! **What a running station may change.** The settings file gives the station's name and its
//! takedown contact their first values. On Cloudron that file becomes `control.toml` again at
//! every start, and on Compose it is on the host where no container can write it; so what an
//! admin changes on the setup page is kept in the database (`station_setup`, migration 0028),
//! wins over the file, and takes effect at once: [`Live`] holds the two values in force and
//! every route that says the name or the contact reads it. The Jurisdiction is the existing
//! `/api/v1/retention`. The look (`station_branding`, migration 0029) is an accent colour, a
//! brand icon and a banner.
//!
//! One answer cannot live in the database: the listener stream's password, which only the
//! Cloudron package has and only its nginx enforces. With `[setup] overrides_file` set (the
//! package sets it), the page's answer is written there; the package's start script reads the
//! two keys it knows from it, never sources it, and so it applies at the next restart.
//!
//! **The lock.** Every route here that shows or changes anything needs an admin: the station
//! key or an admin's session (ADR-0127). None is in the scopes table, so no API token reaches
//! them. With no station key and no staff, nothing opens them. The three open routes
//! (`GET /api/v1/branding` and the two pictures) show only what the public pages show anyway.
//!
//! **When setup is offered.** While nobody has finished the page and something a listener or
//! caller would meet is missing: the name is still the demo's, or there is no takedown
//! contact. A station installed with both in its settings file (the website's wizard writes
//! them; so does a station upgraded from 1.0.0) is set up *by its settings*.
//!
//! **Pictures.** An upload is never stored or served: the media worker (ADR-0048 rule 2)
//! decodes a PNG or a JPEG and writes a clean PNG, which is what the database keeps. The body
//! is capped before it is read, and the caller is checked before the body is.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use weavesignal_media::brand::Kind;
use weavesignal_model::setup::{self as model, PictureKind};
use weavesignal_model::{Station, StationId};

use crate::app::problem;
use crate::media::WorkerError;
use crate::staff::{Actor, Need, Staff};

/// The demo station's name: a station still called this has not been named.
pub const DEMO_NAME: &str = "Kettle FM";

/// The name and takedown contact in force, shared by every route that says either.
#[derive(Debug)]
pub struct Live(RwLock<Inner>, StepWake);

/// What wakes the step reconciler (ADR-0271) when a route changes a show's step, and who
/// did it, for the notice every window gets. Kept here because the shows' routes and the studio's
/// hub already share this object.
#[derive(Debug, Default)]
struct StepWake {
    wake: tokio::sync::Notify,
    mover: Mutex<Option<(std::time::Instant, String)>>,
    /// One reconciliation at a time: a later look must not be undone by an earlier one.
    gate: tokio::sync::Mutex<()>,
}

#[derive(Debug)]
struct Inner {
    file_name: String,
    name: Option<String>,
    contact: Option<String>,
}

impl Live {
    /// A station whose name is the settings file's and which has set nothing on the page.
    pub fn new(file_name: &str) -> Self {
        Self(
            RwLock::new(Inner {
                file_name: file_name.to_owned(),
                name: None,
                contact: None,
            }),
            StepWake::default(),
        )
    }

    /// A show's step was changed by `who`: wake the step reconciler now, and keep the name for
    /// the notice.
    pub fn step_moved(&self, who: &str) {
        *self.1.mover.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((std::time::Instant::now(), who.to_owned()));
        self.1.wake.notify_one();
    }

    /// Look at the schedule again now (a show was made or changed).
    pub fn wake_steps(&self) {
        self.1.wake.notify_one();
    }

    /// Wait until a route says a step changed.
    pub async fn step_woken(&self) {
        self.1.wake.notified().await;
    }

    /// Hold this while the step is read and applied.
    pub async fn step_gate(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.1.gate.lock().await
    }

    /// Who last changed a step, once, if it was within the last half minute (a name left
    /// behind by a change that moved nothing must not be given to a later one).
    pub fn take_step_mover(&self) -> Option<String> {
        let (at, who) = self
            .1
            .mover
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()?;
        (at.elapsed() < std::time::Duration::from_secs(30)).then_some(who)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.0.read().unwrap_or_else(|p| p.into_inner())
    }

    /// The station's name in force: the setup page's, else the settings file's.
    pub fn name(&self) -> String {
        let i = self.read();
        i.name.clone().unwrap_or_else(|| i.file_name.clone())
    }

    /// The settings file's name.
    pub fn file_name(&self) -> String {
        self.read().file_name.clone()
    }

    /// The name set on the setup page, if any.
    pub fn name_set(&self) -> Option<String> {
        self.read().name.clone()
    }

    /// The takedown contact set on the setup page, if any; else the settings file's applies
    /// ([`crate::app::AppState::consent_contact`]).
    pub fn contact_set(&self) -> Option<String> {
        self.read().contact.clone()
    }

    /// The settings file's name, when the control plane learns it after the hub was made.
    pub fn set_file_name(&self, file_name: &str) {
        self.0.write().unwrap_or_else(|p| p.into_inner()).file_name = file_name.to_owned();
    }

    /// What the setup page stored.
    pub fn set(&self, name: Option<String>, contact: Option<String>) {
        let mut i = self.0.write().unwrap_or_else(|p| p.into_inner());
        i.name = name;
        i.contact = contact;
    }
}

/// What the page explains but cannot change: the install's own settings, as this control plane
/// was started with them.
#[derive(Clone, Debug, Default, Serialize, utoipa::ToSchema)]
pub struct InstallFacts {
    /// The address cards and feeds print; empty on a station for this machine only.
    pub public_url: String,
    /// The listener stream as the player is given it.
    pub stream_url: String,
    /// LiveKit as browsers reach it.
    pub room_url: String,
    /// The live room's media port (UDP).
    pub media_port: u16,
    /// TURN over TLS's port (TCP).
    pub turn_tls_port: u16,
    /// How sign-in codes go out: `command`, `outbox` or `off`.
    pub mail: String,
    /// Whether search engines may list the public pages.
    pub listed: bool,
}

/// The setup routes' state: its own, like the shows' and steering's.
pub struct SetupState {
    pub station: StationId,
    pub db: Option<sqlx::PgPool>,
    pub admin_token: Option<String>,
    pub staff: Arc<Staff>,
    /// The hub: it holds the name and contact in force ([`Live`]) and republishes the studio's
    /// view when the name changes.
    pub hub: Arc<crate::hub::Hub>,
    /// The settings file's takedown contact (`join.consent_contact`).
    pub file_contact: String,
    /// Where the stream password's answer is written for the start script; `None` on a route
    /// with no stream password (Compose, Quadlets).
    pub overrides_file: Option<PathBuf>,
    /// Whether the stream asks for a password as this station was started; `None` when the
    /// route has no such setting.
    pub stream_auth: Option<bool>,
    pub facts: InstallFacts,
    /// The media worker's program and timeout.
    pub carts: crate::config::CartsConfig,
    /// When this control plane started, to tell an answer waiting for a restart.
    pub started: std::time::SystemTime,
}

impl SetupState {
    fn contact(&self) -> String {
        self.hub
            .live
            .contact_set()
            .unwrap_or_else(|| self.file_contact.clone())
    }
}

type St = State<Arc<SetupState>>;

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn admin(state: &SetupState, headers: &HeaderMap) -> Result<Actor, Response> {
    crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        Need::Admin,
    )
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn guard<'a>(
    state: &'a SetupState,
    headers: &HeaderMap,
) -> Result<(&'a sqlx::PgPool, Actor), Response> {
    let actor = admin(state, headers)?;
    match &state.db {
        Some(pool) => Ok((pool, actor)),
        None => Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Setup is kept in the database, and this control plane has none (set WEAVESIGNAL_DATABASE_URL).",
        )),
    }
}

fn db_problem(e: impl std::fmt::Display) -> Response {
    tracing::error!(error = %e, "setup: database error");
    problem(
        StatusCode::SERVICE_UNAVAILABLE,
        "The station's database did not answer. Try again in a minute.",
    )
}

/// The admin check as middleware, for the picture uploads: the handler's own check would run
/// only after the body had been read (review 0025, P1). The pictures' `GET` shares the route
/// and stays open.
async fn admin_first(
    State(state): St,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    // Reading a picture is open; setting or removing one is an admin's.
    let reading = matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    );
    if !reading && let Err(r) = admin(&state, req.headers()) {
        return r;
    }
    next.run(req).await
}

pub fn router(state: Arc<SetupState>) -> Router {
    // The body is the picture itself, so the cap is the worker's own.
    let upload = |kind: Kind| DefaultBodyLimit::max(kind.max_bytes());
    Router::new()
        .route("/api/v1/setup", get(view).put(save))
        .route("/api/v1/setup/finish", post(finish))
        .route("/api/v1/setup/stream-password", put(stream_password))
        .route("/api/v1/branding", get(branding))
        .route("/api/v1/branding/accent", put(put_accent))
        .route(
            "/api/v1/branding/icon",
            get(icon)
                .put(put_icon)
                .delete(delete_icon)
                .layer(upload(Kind::Icon))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    admin_first,
                )),
        )
        .route(
            "/api/v1/branding/banner",
            get(banner)
                .put(put_banner)
                .delete(delete_banner)
                .layer(upload(Kind::Banner))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    admin_first,
                )),
        )
        .with_state(state)
}

// ---- the view ----

/// One answer that the settings file starts and the page may override.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Answer {
    /// The value in force.
    pub value: String,
    /// `page`: set on the setup page. `settings`: the settings file's.
    pub source: &'static str,
    /// The settings file's value, which comes back if the page's is cleared.
    pub settings_value: String,
}

/// The listener stream's password, where the install route has one.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StreamPassword {
    /// Whether this install route has a stream password the page can set (the Cloudron package).
    pub available: bool,
    /// Whether the stream asks for a password now; `null` when not available.
    pub enabled: Option<bool>,
    /// What the page last asked for, if it has; it applies when the station restarts.
    pub asked: Option<bool>,
    /// Whether an answer is waiting for a restart.
    pub restart_needed: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SetupView {
    /// `pending`: setup is offered. `done`: it is not.
    pub status: &'static str,
    /// `page`: an admin finished the setup page. `settings`: the settings file gave a name and
    /// a contact. `null` while pending.
    pub done_by: Option<&'static str>,
    /// What is still missing, in words, while pending.
    pub missing: Vec<&'static str>,
    pub completed_at_ms: Option<u64>,
    pub name: Answer,
    pub contact: Answer,
    pub stream_password: StreamPassword,
    pub install: InstallFacts,
    pub branding: BrandingView,
    pub updated_by: Option<String>,
    pub updated_at_ms: Option<u64>,
}

/// What a station still lacks before a listener or a caller meets it (the name in force and
/// the contact in force).
pub fn missing(name: &str, contact: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if name.trim() == DEMO_NAME {
        out.push("The station still has the demo's name, Kettle FM.");
    }
    if contact.trim().is_empty() {
        out.push("There is no takedown contact, so call-in is closed.");
    }
    out
}

/// What the overrides file asks for: `(LISTENERS_AUTH, whether it holds a password)`.
fn read_overrides(path: &Path) -> (Option<bool>, bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, false);
    };
    let last = |key: &str| {
        text.lines()
            .rev()
            .find_map(|l| l.strip_prefix(key))
            .map(str::to_owned)
    };
    let auth = match last("LISTENERS_AUTH=").as_deref() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        _ => None,
    };
    (auth, last("LISTENERS_PASSWORD=").is_some())
}

fn stream_view(state: &SetupState) -> StreamPassword {
    let Some(path) = &state.overrides_file else {
        return StreamPassword {
            available: false,
            enabled: None,
            asked: None,
            restart_needed: false,
        };
    };
    let (asked, _) = read_overrides(path);
    let written_since_start = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|at| at > state.started);
    StreamPassword {
        available: true,
        enabled: state.stream_auth,
        asked,
        restart_needed: written_since_start,
    }
}

async fn view_of(state: &SetupState, pool: &sqlx::PgPool) -> Result<SetupView, sqlx::Error> {
    let setup = model::load_setup(pool, state.station).await?;
    let look = model::load_branding(pool, state.station).await?;
    let name = state.hub.live.name();
    let contact = state.contact();
    let missing = if setup.completed_at_ms.is_some() {
        Vec::new()
    } else {
        missing(&name, &contact)
    };
    let (status, done_by) = match (setup.completed_at_ms, missing.is_empty()) {
        (Some(_), _) => ("done", Some("page")),
        (None, true) => ("done", Some("settings")),
        (None, false) => ("pending", None),
    };
    Ok(SetupView {
        status,
        done_by,
        missing,
        completed_at_ms: setup.completed_at_ms,
        name: Answer {
            source: if state.hub.live.name_set().is_some() {
                "page"
            } else {
                "settings"
            },
            value: name,
            settings_value: state.hub.live.file_name(),
        },
        contact: Answer {
            source: if state.hub.live.contact_set().is_some() {
                "page"
            } else {
                "settings"
            },
            value: contact,
            settings_value: state.file_contact.clone(),
        },
        stream_password: stream_view(state),
        install: state.facts.clone(),
        branding: branding_view(&look),
        updated_by: setup.updated_by,
        updated_at_ms: setup.updated_at_ms,
    })
}

async fn respond(state: &SetupState, pool: &sqlx::PgPool) -> Response {
    match view_of(state, pool).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/setup",
    operation_id = "setup_view",
    tag = "setup",
    summary = "The station's setup",
    description = "Admin only. Whether setup is still offered and what is missing; the station's name and takedown contact in force, each with where it comes from (the setup page or the settings file); the stream password where the install route has one; the install's own settings, which the page explains but cannot change; and the look.",
    responses(
        (status = 200, description = "The setup.", body = SetupView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn view(State(state): St, headers: HeaderMap) -> Response {
    match guard(&state, &headers) {
        Ok((pool, _)) => respond(&state, pool).await,
        Err(r) => r,
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupChange {
    /// The station's name: at most 200 characters, one line. Empty or left out hands the name
    /// back to the settings file.
    #[serde(default)]
    pub name: Option<String>,
    /// The takedown contact, an email address or a link: at most 200 characters, one line.
    /// Empty or left out hands it back to the settings file.
    #[serde(default)]
    pub contact: Option<String>,
}

fn refused(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(crate::openapi::Problem {
            error: detail.into(),
            field: Some(field.to_owned()),
        }),
    )
        .into_response()
}

/// A field as given: trimmed, and `None` when empty.
fn given(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

#[utoipa::path(
    put,
    path = "/api/v1/setup",
    operation_id = "setup_save",
    tag = "setup",
    summary = "Set the station's name and takedown contact",
    description = "Admin only. Stores both in the database, where they win over the settings file, and puts them in force at once: the player, the consent words (whose version changes, so callers are asked again), feeds, cards and the studio's view. The chat reads the name when it next starts. A field left empty goes back to the settings file's value.",
    request_body = SetupChange,
    responses(
        (status = 200, description = "The setup as it is now.", body = SetupView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn save(State(state): St, headers: HeaderMap, Json(req): Json<SetupChange>) -> Response {
    let (pool, actor) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let name = match given(req.name) {
        None => None,
        // The model's own rule for a station's name, and one line.
        Some(n) => match Station::new(state.station, n.as_str()) {
            Ok(s) if !s.name.chars().any(char::is_control) => Some(s.name),
            Ok(_) => return refused("name", "A station's name is one line."),
            Err(e) => return refused("name", e.to_string()),
        },
    };
    let contact = given(req.contact);
    if let Some(c) = &contact
        && (c.chars().count() > model::CONTACT_MAX || c.chars().any(char::is_control))
    {
        return refused(
            "contact",
            "A takedown contact is an email address or a link: at most 200 characters, on one line.",
        );
    }
    let by = actor.label();
    let file_name = state.hub.live.file_name();
    match model::save_setup(
        pool,
        state.station,
        name.as_deref(),
        contact.as_deref(),
        &file_name,
        &by,
    )
    .await
    {
        Ok(_) => {
            state.hub.live.set(name, contact);
            state.hub.republish();
            tracing::info!(
                target: "weavesignal::setup",
                name = %state.hub.live.name(),
                contact_set = !state.contact().is_empty(),
                by = %by,
                "the station's name and takedown contact were set on the setup page"
            );
            respond(&state, pool).await
        }
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/setup/finish",
    operation_id = "setup_finish",
    tag = "setup",
    summary = "Finish setup",
    description = "Admin only. Marks setup finished: it is no longer offered, whatever the name and contact are. The page stays for admins. Done once; a second call changes nothing.",
    responses(
        (status = 200, description = "The setup as it is now.", body = SetupView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn finish(State(state): St, headers: HeaderMap) -> Response {
    let (pool, actor) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let by = actor.label();
    match model::complete_setup(pool, state.station, &by).await {
        Ok(_) => {
            tracing::info!(target: "weavesignal::setup", by = %by, "setup finished");
            respond(&state, pool).await
        }
        Err(e) => db_problem(e),
    }
}

// ---- the stream password (the Cloudron package) ----

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamPasswordChange {
    /// Whether the listener stream asks for a password.
    pub enabled: bool,
    /// A new password: 8 to 64 of letters, digits, `.`, `_`, `~` and `-`. Left out, the
    /// password already set is kept.
    #[serde(default)]
    pub password: Option<String>,
}

/// The stream password's rule, which the start script checks again: it goes into a file a
/// shell script reads and into an `htpasswd` line.
pub fn password_ok(p: &str) -> bool {
    (8..=64).contains(&p.len())
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
}

/// The overrides file's text. Only ever these two keys, each checked.
fn overrides_text(enabled: bool, password: Option<&str>) -> String {
    let mut out = String::from(
        "# Written by the station's setup page (ADR-0237). start.sh reads LISTENERS_AUTH and\n# LISTENERS_PASSWORD from here at each start, over weavesignal.env and secrets.env; it reads\n# nothing else, and never runs this file. Delete it to go back to those files.\n",
    );
    out.push_str(if enabled {
        "LISTENERS_AUTH=on\n"
    } else {
        "LISTENERS_AUTH=off\n"
    });
    if let Some(p) = password {
        out.push_str(&format!("LISTENERS_PASSWORD={p}\n"));
    }
    out
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let tmp = path.with_extension("env.part");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[utoipa::path(
    put,
    path = "/api/v1/setup/stream-password",
    operation_id = "setup_stream_password",
    tag = "setup",
    summary = "Turn the stream's password on or off",
    description = "Admin only. Only where the install route has a stream password (the Cloudron package; elsewhere `409`). The answer is written to a file the package's start script reads, so it applies when the station next restarts. The password is never read back. A password-protected stream is silent in the player page: a browser does not ask for a password for audio from another name.",
    request_body = StreamPasswordChange,
    responses(
        (status = 200, description = "The setup as it is now.", body = SetupView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn stream_password(
    State(state): St,
    headers: HeaderMap,
    Json(req): Json<StreamPasswordChange>,
) -> Response {
    let (pool, actor) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let Some(path) = state.overrides_file.clone() else {
        return problem(
            StatusCode::CONFLICT,
            "This install has no stream password: the stream is open to anyone with its address. Put a password in the proxy in front of the station if it needs one.",
        );
    };
    if let Some(p) = &req.password
        && !password_ok(p)
    {
        return refused(
            "password",
            "A stream password is 8 to 64 letters, digits, dots, underscores, tildes or hyphens.",
        );
    }
    // Keep the password the page set before, unless a new one is given.
    let kept = std::fs::read_to_string(&path).ok().and_then(|t| {
        t.lines()
            .rev()
            .find_map(|l| l.strip_prefix("LISTENERS_PASSWORD="))
            .filter(|p| password_ok(p))
            .map(str::to_owned)
    });
    let password = req.password.or(kept);
    let text = overrides_text(req.enabled, password.as_deref());
    let written = tokio::task::spawn_blocking(move || write_private(&path, &text)).await;
    match written {
        Ok(Ok(())) => {
            tracing::info!(
                target: "weavesignal::setup",
                enabled = req.enabled,
                by = %actor.label(),
                "the stream password's answer was written; it applies at the next restart"
            );
            respond(&state, pool).await
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "setup: cannot write the overrides file");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The station could not write the setting. Check the app's data folder and try again.",
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "setup: the writer stopped");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The station could not write the setting. Try again.",
            )
        }
    }
}

// ---- the look ----

/// One brand picture as the pages are given it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PictureView {
    /// Its address on this station, with its version: it can be kept for good.
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u32,
}

/// The station's look on its public pages; every part is `null` until it is set.
#[derive(Serialize, utoipa::ToSchema)]
pub struct BrandingView {
    /// The accent colour as the station chose it, `#rrggbb`. The pages derive each theme's
    /// accent from it so that it stays legible (WCAG AA); they never use it as it is.
    pub accent: Option<String>,
    /// The brand icon, also the favicon: a PNG up to 256 pixels square.
    pub icon: Option<PictureView>,
    /// The banner across the top of the player: a PNG up to 1200 by 400, three to one.
    pub banner: Option<PictureView>,
}

fn picture_view(kind: &str, p: &model::Picture) -> PictureView {
    PictureView {
        url: format!("/api/v1/branding/{kind}?v={}", p.version),
        width: p.width,
        height: p.height,
        bytes: p.bytes,
    }
}

fn branding_view(b: &model::Branding) -> BrandingView {
    BrandingView {
        accent: b.accent.clone(),
        icon: b.icon.as_ref().map(|p| picture_view("icon", p)),
        banner: b.banner.as_ref().map(|p| picture_view("banner", p)),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/branding",
    operation_id = "branding",
    tag = "setup",
    summary = "The station's look",
    description = "Open: what the public pages show anyway. The accent colour and the addresses of the brand icon and the banner, each `null` until set. A station without a database answers with all three `null`.",
    responses(
        (status = 200, description = "The look.", body = BrandingView),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn branding(State(state): St) -> Response {
    let look = match &state.db {
        None => model::Branding::default(),
        Some(pool) => match model::load_branding(pool, state.station).await {
            Ok(b) => b,
            Err(e) => return db_problem(e),
        },
    };
    (
        [(header::CACHE_CONTROL, "no-cache")],
        Json(branding_view(&look)),
    )
        .into_response()
}

/// `#rrggbb` in lower case, from six hex digits with or without the hash.
pub fn accent_of(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches('#');
    (s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", s.to_ascii_lowercase()))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccentChange {
    /// Six hex digits, such as `#b4532a`. Empty or left out: the product's own accent.
    #[serde(default)]
    pub accent: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/v1/branding/accent",
    operation_id = "branding_accent",
    tag = "setup",
    summary = "Set the accent colour",
    description = "Admin only. One sRGB colour as six hex digits. The pages derive each theme's accent from it (its lightness moved until it reads at 4.5:1), so any colour is accepted. Empty goes back to the product's own.",
    request_body = AccentChange,
    responses(
        (status = 200, description = "The look as it is now.", body = BrandingView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn put_accent(
    State(state): St,
    headers: HeaderMap,
    Json(req): Json<AccentChange>,
) -> Response {
    let (pool, actor) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let accent = match given(req.accent) {
        None => None,
        Some(a) => match accent_of(&a) {
            Some(hex) => Some(hex),
            None => {
                return refused("accent", "A colour is six hex digits, such as #b4532a.");
            }
        },
    };
    match model::set_accent(pool, state.station, accent.as_deref(), &actor.label()).await {
        Ok(()) => look(&state, pool).await,
        Err(e) => db_problem(e),
    }
}

async fn look(state: &SetupState, pool: &sqlx::PgPool) -> Response {
    match model::load_branding(pool, state.station).await {
        Ok(b) => Json(branding_view(&b)).into_response(),
        Err(e) => db_problem(e),
    }
}

/// The first 16 hex digits of the SHA-256 of what is served: the picture's version.
fn version_of(png: &[u8]) -> String {
    Sha256::digest(png)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn model_kind(kind: Kind) -> PictureKind {
    match kind {
        Kind::Icon => PictureKind::Icon,
        Kind::Banner => PictureKind::Banner,
    }
}

async fn put_picture(state: &SetupState, headers: &HeaderMap, kind: Kind, body: Bytes) -> Response {
    let (pool, actor) = match guard(state, headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    if body.is_empty() {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Send the picture as the request's body: a PNG or a JPEG.",
        );
    }
    let (png, report) = match crate::media::brand(&state.carts, body.to_vec(), kind).await {
        Ok(done) => done,
        Err(WorkerError::Refused(m)) => return problem(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(WorkerError::Failed(m)) => {
            tracing::warn!(error = %m, "the media worker failed on a brand picture");
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The station could not read the picture just now. Try again.",
            );
        }
    };
    let version = version_of(&png);
    match model::set_picture(
        pool,
        state.station,
        model_kind(kind),
        model::NewPicture {
            png: &png,
            version: &version,
            width: report.width,
            height: report.height,
        },
        &actor.label(),
    )
    .await
    {
        Ok(()) => look(state, pool).await,
        Err(e) => db_problem(e),
    }
}

async fn delete_picture(state: &SetupState, headers: &HeaderMap, kind: Kind) -> Response {
    let (pool, actor) = match guard(state, headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match model::clear_picture(pool, state.station, model_kind(kind), &actor.label()).await {
        Ok(_) => look(state, pool).await,
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize)]
struct Version {
    v: Option<String>,
}

async fn serve_picture(
    state: &SetupState,
    headers: &HeaderMap,
    kind: Kind,
    asked: Option<String>,
) -> Response {
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::NOT_FOUND,
            "This station has set no such picture.",
        );
    };
    let (png, version) = match model::picture_bytes(pool, state.station, model_kind(kind)).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            return problem(
                StatusCode::NOT_FOUND,
                "This station has set no such picture.",
            );
        }
        Err(e) => return db_problem(e),
    };
    let etag = format!("\"{version}\"");
    // Under its own version the picture never changes; under any other address it is checked
    // each time, so an old link shows the new picture.
    let cache = if asked.as_deref() == Some(version.as_str()) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut h = HeaderMap::new();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
    {
        return (StatusCode::NOT_MODIFIED, h).into_response();
    }
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    (h, png).into_response()
}

macro_rules! picture_routes {
    ($kind:expr, $word:literal, $shape:literal, $get:ident, $put:ident, $delete:ident, $get_id:literal, $put_id:literal, $delete_id:literal) => {
        #[utoipa::path(
            get,
            path = concat!("/api/v1/branding/", $word),
            operation_id = $get_id,
            tag = "setup",
            summary = concat!("The station's ", $word),
            description = "Open. The PNG the station made from the upload. Under the version its address carries (`?v=`, from `GET /api/v1/branding`) it may be kept for good; `404` when the station has set none.",
            params(("v" = Option<String>, Query, description = "The picture's version.")),
            responses(
                (status = 200, description = "The picture.", content_type = "image/png", body = crate::openapi::File),
                (status = 304, description = "Not changed (`If-None-Match`)."),
                (status = 404, response = crate::openapi::NotFound),
                (status = 503, response = crate::openapi::Unavailable),
            ),
            security(()),
        )]
        async fn $get(State(state): St, headers: HeaderMap, Query(q): Query<Version>) -> Response {
            serve_picture(&state, &headers, $kind, q.v).await
        }

        #[utoipa::path(
            put,
            path = concat!("/api/v1/branding/", $word),
            operation_id = $put_id,
            tag = "setup",
            summary = concat!("Set the station's ", $word),
            description = concat!("Admin only. The body is the picture itself, a PNG or a JPEG (recognised by its first bytes; nothing else is read, and never SVG). ", $shape, " The station stores and serves the PNG its media worker writes, never the upload, so no metadata survives."),
            request_body(content = crate::openapi::File, content_type = "application/octet-stream"),
            responses(
                (status = 200, description = "The look as it is now.", body = BrandingView),
                (status = 401, response = crate::openapi::Unauthorized),
                (status = 403, response = crate::openapi::Forbidden),
                (status = 413, description = "The body is larger than this picture may be."),
                (status = 422, response = crate::openapi::Unprocessable),
                (status = 503, response = crate::openapi::Unavailable),
            ),
            security(("staff_session" = []), ("station_key" = [])),
            extensions(("x-admin" = json!(true))),
        )]
        async fn $put(State(state): St, headers: HeaderMap, body: Bytes) -> Response {
            put_picture(&state, &headers, $kind, body).await
        }

        #[utoipa::path(
            delete,
            path = concat!("/api/v1/branding/", $word),
            operation_id = $delete_id,
            tag = "setup",
            summary = concat!("Remove the station's ", $word),
            description = "Admin only. The pages go back to the product's own.",
            responses(
                (status = 200, description = "The look as it is now.", body = BrandingView),
                (status = 401, response = crate::openapi::Unauthorized),
                (status = 403, response = crate::openapi::Forbidden),
                (status = 503, response = crate::openapi::Unavailable),
            ),
            security(("staff_session" = []), ("station_key" = [])),
            extensions(("x-admin" = json!(true))),
        )]
        async fn $delete(State(state): St, headers: HeaderMap) -> Response {
            delete_picture(&state, &headers, $kind).await
        }
    };
}

picture_routes!(
    Kind::Icon,
    "icon",
    "At most 1 MB, each side 64 to 4,096 pixels; it is fitted inside 256 pixels square with its transparency kept.",
    icon,
    put_icon,
    delete_icon,
    "branding_icon",
    "branding_icon_set",
    "branding_icon_remove"
);
picture_routes!(
    Kind::Banner,
    "banner",
    "At most 4 MB, at least 600 by 200 pixels; it is cropped about its middle to three to one and fitted inside 1,200 by 400.",
    banner,
    put_banner,
    delete_banner,
    "branding_banner",
    "branding_banner_set",
    "branding_banner_remove"
);

/// Read what the setup page stored into [`Live`] at start, and say in the log whether setup is
/// still offered.
pub async fn restore(state: &SetupState) {
    let Some(pool) = &state.db else { return };
    match model::load_setup(pool, state.station).await {
        Ok(setup) => {
            if let Some(name) = &setup.name
                && *name != state.hub.live.file_name()
            {
                tracing::info!(
                    name = %name,
                    settings = %state.hub.live.file_name(),
                    "the station's name is the one set on the setup page, not the settings file's"
                );
            }
            state.hub.live.set(setup.name, setup.consent_contact);
            if setup.completed_at_ms.is_none() {
                let missing = missing(&state.hub.live.name(), &state.contact());
                if !missing.is_empty() {
                    tracing::warn!(
                        missing = %missing.join(" "),
                        "this station is not set up: open /setup/ on the station's address and sign in with the station key"
                    );
                }
            }
        }
        Err(e) => tracing::error!(error = %e, "cannot read the station's setup"),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        view,
        save,
        finish,
        stream_password,
        branding,
        put_accent,
        icon,
        put_icon,
        delete_icon,
        banner,
        put_banner,
        delete_banner,
    ),
    components(schemas(
        SetupView,
        Answer,
        StreamPassword,
        InstallFacts,
        BrandingView,
        PictureView,
        SetupChange,
        StreamPasswordChange,
        AccentChange,
    ))
)]
pub struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_wins_over_the_settings_file_and_hands_back() {
        let live = Live::new("Kettle FM");
        assert_eq!(live.name(), "Kettle FM");
        assert_eq!(live.contact_set(), None);
        live.set(
            Some("Radio Ceilidh".into()),
            Some("calls@station.example".into()),
        );
        assert_eq!(live.name(), "Radio Ceilidh");
        assert_eq!(live.file_name(), "Kettle FM");
        assert_eq!(live.contact_set().as_deref(), Some("calls@station.example"));
        live.set(None, None);
        assert_eq!(live.name(), "Kettle FM");
    }

    #[test]
    fn setup_is_offered_for_the_demo_name_or_no_contact() {
        assert_eq!(missing("Kettle FM", "").len(), 2);
        assert_eq!(missing("Kettle FM", "calls@station.example").len(), 1);
        assert_eq!(missing("Radio Ceilidh", "  ").len(), 1);
        assert!(missing("Radio Ceilidh", "calls@station.example").is_empty());
    }

    #[test]
    fn an_accent_is_six_hex_digits() {
        assert_eq!(accent_of("#B4532A").as_deref(), Some("#b4532a"));
        assert_eq!(accent_of(" b4532a ").as_deref(), Some("#b4532a"));
        for bad in [
            "", "#fa0", "red", "#b4532a;", "#b4532g", "url(x)", "#b4532a0",
        ] {
            assert_eq!(accent_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_stream_password_is_what_a_shell_and_htpasswd_take_unquoted() {
        for ok in ["kettle-fm-2026", "aB3.dE_f~g", &"x".repeat(64)] {
            assert!(password_ok(ok), "{ok}");
        }
        for bad in [
            "short",
            &"x".repeat(65),
            "has space1",
            "semi;colon1",
            "$(reboot)12",
            "back`tick`12",
            "quote\"12345",
            "new\nline123",
            "colon:12345",
            "päßwörter1",
            "hash#12345",
        ] {
            assert!(!password_ok(bad), "{bad}");
        }
    }

    #[test]
    fn the_overrides_file_holds_two_keys_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("ws-setup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("setup.env");
        assert_eq!(read_overrides(&path), (None, false));
        write_private(&path, &overrides_text(true, Some("kettle-fm-2026"))).unwrap();
        assert_eq!(read_overrides(&path), (Some(true), true));
        let text = std::fs::read_to_string(&path).unwrap();
        let keys: Vec<&str> = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .map(|l| l.split('=').next().unwrap())
            .collect();
        assert_eq!(keys, ["LISTENERS_AUTH", "LISTENERS_PASSWORD"]);
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write_private(&path, &overrides_text(false, None)).unwrap();
        assert_eq!(read_overrides(&path), (Some(false), false));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
