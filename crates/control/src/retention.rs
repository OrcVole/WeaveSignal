// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Retention's routes, the sweeper on its schedule, and callers' consent read back from the
//! database (ADRs 0107, 0113-0116). The rules and the database work are in
//! [`weavesignal_model::retention`]; this module is the control plane's side.
//!
//! The studio app's Settings use these routes (admin bearer, until accounts arrive). There are
//! no accounts yet, so whoever changes the setting or places a hold types their name, and the
//! log keeps it.
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/retention` | the setting, each period in force with its source and floor, the fourteen presets, warnings, the complaint-hold rule, the last sweep |
//! | `PUT /api/v1/retention` | choose a preset, or Custom with every period; `by` says who; answers as `GET` |
//! | `POST /api/v1/retention/preview` | a proposed setting's warnings, and what the next sweep would delete under it (a dry run) |
//! | `POST /api/v1/retention/sweep` | sweep now; answers with what went |
//! | `GET /api/v1/retention/holds` | every hold, open first, with who, when and why |
//! | `POST /api/v1/retention/holds` | place a hold on named records or a stretch of broadcast time |
//! | `POST /api/v1/retention/holds/{id}/release` | release it, with who and why |
//! | `POST /api/v1/retention/holds/{id}/extend` | keep it until a later date (ADR-0156), with who and why; the "past its date" flag clears until then |
//! | `POST /api/v1/retention/recordings` | register a recording (the logger's hour, an archive); it takes the consent of each caller who was on air during it (ADR-0153) |
//! | `POST /api/v1/retention/recordings/{id}/end` | close an open recording; its period starts |
//! | `POST /api/v1/retention/consents/{id}/recordings` | link a consent to a recording it covers (a podcast episode) |
//! | `GET /api/v1/retention/consents?name=` | callers' consents whose name holds the words, newest first: to find one to erase |
//! | `POST /api/v1/retention/consents/{id}/erase` | erase a caller's consent at their request (ADR-0122): name and topic redacted, date and terms kept; `by` says who |
//!
//! **Consent across a restart.** The screener's queue is in memory; consent is in the database.
//! At start the control plane reads back the consent of every call that may still be going,
//! and a caller's rejoin looks there before refusing, so a restart no longer turns a waiting
//! caller away (M3's known fault).

// The `Err` of these helpers is the response to send.
#![allow(clippy::result_large_err)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use weavesignal_model::retention::{
    self as model, CUSTOM, Cell, ComplaintHold, HoldError, Kind, LastSweep, NewHold, NewRecording,
    PRESETS, PRESETS_AS_OF, Period, Preset, Setting, SettingError, StoreError, Warning,
};
use weavesignal_model::{ConsentRecord, StationId};

use crate::app::{AppState, admin_refusal, problem};
use crate::studio::{CallIn, Consent, ConsentBy};

/// What the sweeper needs that the routes do not: where recordings live and how often to run.
/// Layered over the router as an extension by `serve`, so `AppState` does not change.
#[derive(Clone, Debug)]
pub struct RetentionLink {
    pub recordings_dir: PathBuf,
    pub sweep_every: Duration,
}

/// Retention's routes, merged into the control plane's router.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/retention", get(view).put(choose))
        .route("/api/v1/retention/preview", post(preview))
        .route("/api/v1/retention/sweep", post(sweep_now))
        .route("/api/v1/retention/holds", get(holds).post(place_hold))
        .route("/api/v1/retention/holds/{id}/release", post(release_hold))
        .route("/api/v1/retention/holds/{id}/extend", post(extend_hold))
        .route("/api/v1/retention/recordings", post(add_recording))
        .route("/api/v1/retention/recordings/{id}/end", post(end_recording))
        .route(
            "/api/v1/retention/consents/{id}/recordings",
            post(link_consent),
        )
        .route("/api/v1/retention/consents", get(find_consents))
        .route("/api/v1/retention/consents/{id}/erase", post(erase_consent))
}

/// The admin check and the database, or the refusal to send. Retention, holds, recordings,
/// consents and erasure are an admin's (ADR-0127; review 0025, T1).
/// Who a retention change is recorded as (review 0028, C4): a signed-in member of staff is
/// named by their own username, whatever the request's `by` says; only the station key, which
/// is no one in particular, names the person in `by`.
fn signed_in(state: &AppState, headers: &HeaderMap) -> Option<String> {
    match state.check(headers, crate::staff::Need::Admin) {
        Ok(crate::staff::Actor::Staff(a)) => Some(a.username),
        _ => None,
    }
}

fn guard<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<(&'a sqlx::PgPool, StationId), Response> {
    if let Some(r) = admin_refusal(state, headers) {
        return Err(r);
    }
    match &state.db {
        Some(pool) => Ok((pool, state.station.id)),
        None => Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Retention is kept in the database, and this control plane has none (set WEAVESIGNAL_DATABASE_URL).",
        )),
    }
}

fn db_problem(e: impl std::fmt::Display) -> Response {
    tracing::error!(error = %e, "retention: database error");
    problem(
        StatusCode::SERVICE_UNAVAILABLE,
        "The station's database did not answer. Try again in a minute.",
    )
}

// ---- the view ----

/// One period as the screen shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Row {
    pub kind: Kind,
    pub label: &'static str,
    pub period: Period,
    pub source: &'static str,
    pub floor: Option<model::Floor>,
    /// Whether this release keeps records of the kind (and so deletes them).
    pub swept: bool,
}

fn rows(cells: impl Fn(Kind) -> (Period, &'static str), floors: &Preset) -> Vec<Row> {
    Kind::ALL
        .iter()
        .map(|&k| {
            let (period, source) = cells(k);
            Row {
                kind: k,
                label: k.label(),
                period,
                source,
                floor: floors.cell(k).floor,
                swept: k.swept(),
            }
        })
        .collect()
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PresetView {
    #[serde(flatten)]
    pub preset: &'static Preset,
    pub rows: Vec<Row>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SettingView {
    pub preset: String,
    pub based_on: Option<String>,
    pub broadcast_licence: Option<bool>,
    pub us_statutory_licence: Option<bool>,
    pub updated_by: Option<String>,
    pub updated_at_ms: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RetentionView {
    pub setting: SettingView,
    /// Each period in force, with its source and the floor of the rules that apply.
    pub rows: Vec<Row>,
    pub warnings: Vec<Warning>,
    /// What the rules that apply say about a complaint (ADR-0116).
    pub complaint_hold: Option<ComplaintHold>,
    pub presets: Vec<PresetView>,
    pub presets_as_of: &'static str,
    pub last_sweep: Option<LastSweep>,
    /// How often the sweeper runs; `null` when it is not running.
    pub sweep_every_s: Option<u64>,
}

fn in_force(s: &Setting) -> Vec<Row> {
    let rules = s.rules();
    if s.preset == CUSTOM {
        let p = s.periods();
        rows(|k| (p.get(k), "Set by the station (Custom)."), rules)
    } else {
        let preset = model::preset(&s.preset).unwrap_or(&PRESETS[0]);
        rows(
            |k| {
                let c: &Cell = preset.cell(k);
                (c.period, c.source)
            },
            preset,
        )
    }
}

async fn view_of(
    pool: &sqlx::PgPool,
    station: StationId,
    link: Option<&RetentionLink>,
) -> Result<RetentionView, sqlx::Error> {
    let s = model::load_setting(pool, station).await?;
    Ok(RetentionView {
        rows: in_force(&s),
        warnings: s.warnings(),
        complaint_hold: s.rules().complaint_hold,
        presets: PRESETS
            .iter()
            .map(|p| PresetView {
                preset: p,
                rows: rows(
                    |k| {
                        let c = p.cell(k);
                        (c.period, c.source)
                    },
                    p,
                ),
            })
            .collect(),
        presets_as_of: PRESETS_AS_OF,
        last_sweep: model::last_sweep(pool, station).await?,
        sweep_every_s: link.map(|l| l.sweep_every.as_secs()),
        setting: SettingView {
            preset: s.preset,
            based_on: s.based_on,
            broadcast_licence: s.broadcast_licence,
            us_statutory_licence: s.us_statutory_licence,
            updated_by: s.updated_by,
            updated_at_ms: s.updated_at_ms,
        },
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/retention",
    operation_id = "retention_view",
    tag = "retention",
    summary = "The Jurisdiction setting",
    description = "Admin only. The setting, each period in force with its source and floor, warnings, the presets, the last sweep.",
    responses(
        (status = 200, description = "The setting.", body = RetentionView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn view(
    State(state): State<Arc<AppState>>,
    link: Option<Extension<Arc<RetentionLink>>>,
    headers: HeaderMap,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match view_of(pool, station, link.as_deref().map(Arc::as_ref)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => db_problem(e),
    }
}

// ---- choosing ----

/// A setting as the screen sends it.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub preset: String,
    /// Under Custom: the preset whose floors become warnings.
    #[serde(default)]
    pub based_on: Option<String>,
    /// Under Custom: every period.
    #[serde(default)]
    pub periods: Option<model::Periods>,
    #[serde(default)]
    pub broadcast_licence: Option<bool>,
    #[serde(default)]
    pub us_statutory_licence: Option<bool>,
    /// Who is making the change (needed to save; a preview leaves it out).
    #[serde(default)]
    pub by: String,
}

impl Choice {
    fn setting(&self) -> Setting {
        Setting {
            preset: self.preset.clone(),
            based_on: if self.preset == CUSTOM {
                self.based_on.clone()
            } else {
                None
            },
            custom: self.periods,
            broadcast_licence: self.broadcast_licence,
            us_statutory_licence: self.us_statutory_licence,
            updated_by: None,
            updated_at_ms: None,
        }
    }
}

fn setting_problem(e: &SettingError) -> Response {
    let detail = match e {
        SettingError::UnknownPreset(k) => format!("“{k}” is not one of the Jurisdiction presets."),
        SettingError::CustomWithoutPeriods => {
            "Custom needs a period for every kind of record.".to_owned()
        }
        SettingError::PeriodsWithPreset => {
            "A preset sets its own periods. To change one, choose Custom.".to_owned()
        }
        SettingError::BadPeriod(why) => why.clone(),
        SettingError::NoWho => "Add your name, so the log says who made the change.".to_owned(),
    };
    problem(StatusCode::UNPROCESSABLE_ENTITY, detail)
}

#[utoipa::path(
    put,
    path = "/api/v1/retention",
    operation_id = "retention_choose",
    tag = "retention",
    summary = "Choose the Jurisdiction setting",
    description = "Admin only. A preset, or Custom with a period for every kind of record. Answers with the view as it now is.",
    request_body = Choice,
    responses(
        (status = 200, description = "The view as it now is.", body = RetentionView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn choose(
    State(state): State<Arc<AppState>>,
    link: Option<Extension<Arc<RetentionLink>>>,
    headers: HeaderMap,
    Json(req): Json<Choice>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let mut req = req;
    if let Some(name) = signed_in(&state, &headers) {
        req.by = name;
    }
    let setting = req.setting();
    match model::save_setting(pool, station, &setting, &req.by).await {
        Ok(saved) => {
            tracing::info!(
                target: "weavesignal::retention",
                preset = %saved.preset,
                based_on = ?saved.based_on,
                by = %req.by.trim(),
                periods = %serde_json::to_string(&saved.periods()).unwrap_or_default(),
                "retention setting changed"
            );
        }
        Err(StoreError::Setting(e)) => return setting_problem(&e),
        Err(StoreError::Db(e)) => return db_problem(e),
    }
    match view_of(pool, station, link.as_deref().map(Arc::as_ref)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => db_problem(e),
    }
}

/// A proposed setting's warnings, and what the next sweep would delete under it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Preview {
    pub warnings: Vec<Warning>,
    pub sweep: model::SweepReport,
}

/// What the next sweep would delete if the proposed setting were chosen: the screen asks for
/// confirmation with these numbers before a shorter preset is saved.
#[utoipa::path(
    post,
    path = "/api/v1/retention/preview",
    operation_id = "retention_preview",
    tag = "retention",
    summary = "What a setting would delete",
    description = "Admin only. A dry run: the warnings and what a sweep would do under the setting, nothing changed.",
    request_body = Choice,
    responses(
        (status = 200, description = "The warnings and the dry sweep.", body = Preview),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn preview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<Choice>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let setting = req.setting();
    if let Err(e) = setting.check() {
        return setting_problem(&e);
    }
    match model::sweep(pool, station, &setting, None, true).await {
        Ok(sweep) => Json(Preview {
            warnings: setting.warnings(),
            sweep,
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

// ---- sweeping ----

/// Sweep the station once under its setting, logging what went.
pub async fn sweep_once(
    pool: &sqlx::PgPool,
    station: StationId,
    link: &RetentionLink,
) -> Result<model::SweepReport, sqlx::Error> {
    let setting = model::load_setting(pool, station).await?;
    let report = model::sweep(
        pool,
        station,
        &setting,
        Some(link.recordings_dir.as_path()),
        false,
    )
    .await?;
    if report.recordings_deleted + report.consents_deleted + report.holds_deleted + report.held > 0
        || !report.errors.is_empty()
    {
        tracing::info!(
            target: "weavesignal::retention",
            preset = %report.preset,
            recordings_deleted = report.recordings_deleted,
            files_missing = report.files_missing,
            consents_deleted = report.consents_deleted,
            holds_deleted = report.holds_deleted,
            held = report.held,
            errors = report.errors.len(),
            "retention sweep"
        );
    }
    for e in &report.errors {
        tracing::warn!(target: "weavesignal::retention", "{e}");
    }
    Ok(report)
}

/// The sweeper: once a minute after start, then every `sweep_every`.
pub fn spawn_sweeper(pool: sqlx::PgPool, station: StationId, link: Arc<RetentionLink>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut every = tokio::time::interval(link.sweep_every);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            if let Err(e) = sweep_once(&pool, station, &link).await {
                tracing::warn!(target: "weavesignal::retention", error = %e, "retention sweep failed; trying again at the next");
            }
        }
    });
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/sweep",
    operation_id = "retention_sweep_now",
    tag = "retention",
    summary = "Sweep now",
    description = "Admin only. Run the sweeper once. 503 also when the sweeper is not running in this control plane.",
    responses(
        (status = 200, description = "What the sweep did.", body = weavesignal_model::retention::SweepReport),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn sweep_now(
    State(state): State<Arc<AppState>>,
    link: Option<Extension<Arc<RetentionLink>>>,
    headers: HeaderMap,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let Some(Extension(link)) = link else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The sweeper is not running in this control plane.",
        );
    };
    match sweep_once(pool, station, &link).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => db_problem(e),
    }
}

// ---- holds ----

fn hold_problem(e: HoldError) -> Response {
    match e {
        HoldError::Invalid(why) => problem(StatusCode::UNPROCESSABLE_ENTITY, why),
        HoldError::NotFound(id) => problem(
            StatusCode::NOT_FOUND,
            format!("There is no hold {id} at this station."),
        ),
        HoldError::Released(id) => problem(
            StatusCode::CONFLICT,
            format!("Hold {id} was already released."),
        ),
        HoldError::Db(e) => db_problem(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/retention/holds",
    operation_id = "retention_holds",
    tag = "retention",
    summary = "Holds",
    description = "Admin only. Evidence holds, placed and released.",
    responses(
        (status = 200, description = "The holds.", body = Vec<weavesignal_model::retention::Hold>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn holds(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match model::holds(pool, station).await {
        Ok(h) => Json(h).into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/holds",
    operation_id = "retention_place_hold",
    tag = "retention",
    summary = "Place a hold",
    description = "Admin only. Keep records past their period for a stated reason.",
    request_body = weavesignal_model::retention::NewHold,
    responses(
        (status = 201, description = "The hold.", body = weavesignal_model::retention::Hold),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn place_hold(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewHold>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let mut req = req;
    if let Some(name) = signed_in(&state, &headers) {
        req.by = name;
    }
    match model::place_hold(pool, station, &req).await {
        Ok(h) => {
            tracing::info!(
                target: "weavesignal::retention",
                hold = h.id,
                by = %h.placed_by,
                reason = %h.reason,
                from_ms = ?h.from_ms,
                to_ms = ?h.to_ms,
                records = h.records.len(),
                "hold placed"
            );
            (StatusCode::CREATED, Json(h)).into_response()
        }
        Err(e) => hold_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub by: String,
    pub reason: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/holds/{id}/release",
    operation_id = "retention_release_hold",
    tag = "retention",
    summary = "Release a hold",
    description = "Admin only.",
    params(("id" = i64, Path, description = "The hold")),
    request_body = Release,
    responses(
        (status = 200, description = "The hold, released.", body = weavesignal_model::retention::Hold),
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
async fn release_hold(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<Release>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let mut req = req;
    if let Some(name) = signed_in(&state, &headers) {
        req.by = name;
    }
    match model::release_hold(pool, station, id, &req.by, &req.reason).await {
        Ok(h) => {
            tracing::info!(
                target: "weavesignal::retention",
                hold = h.id,
                by = %req.by.trim(),
                reason = %req.reason.trim(),
                "hold released"
            );
            Json(h).into_response()
        }
        Err(e) => hold_problem(e),
    }
}

/// Extend an open hold to a later date (ADR-0156). An admin's, like placing and releasing one:
/// it keeps personal data longer.
#[utoipa::path(
    post,
    path = "/api/v1/retention/holds/{id}/extend",
    operation_id = "retention_extend_hold",
    tag = "retention",
    summary = "Extend a hold",
    description = "Admin only.",
    params(("id" = i64, Path, description = "The hold")),
    request_body = weavesignal_model::retention::Extension,
    responses(
        (status = 200, description = "The hold, extended.", body = weavesignal_model::retention::Hold),
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
async fn extend_hold(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<model::Extension>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let mut req = req;
    if let Some(name) = signed_in(&state, &headers) {
        req.by = name;
    }
    let rule = match model::load_setting(pool, station).await {
        Ok(s) => s.rules().complaint_hold,
        Err(e) => return db_problem(e),
    };
    match model::extend_hold(pool, station, id, &req, rule.as_ref(), crate::hub::now_ms()).await {
        Ok(h) => {
            tracing::info!(
                target: "weavesignal::retention",
                hold = h.id,
                by = %req.by.trim(),
                reason = %req.reason.trim(),
                until_ms = req.until_ms,
                "hold extended"
            );
            Json(h).into_response()
        }
        Err(e) => hold_problem(e),
    }
}

// ---- erasure (ADR-0122) ----

#[derive(Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct FindQuery {
    #[serde(default)]
    name: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/retention/consents",
    operation_id = "retention_find_consents",
    tag = "retention",
    summary = "Find callers' consents",
    description = "Admin only. By at least two letters of the caller's name.",
    params(FindQuery),
    responses(
        (status = 200, description = "The consents found.", body = Vec<weavesignal_model::retention::ConsentFound>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn find_consents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<FindQuery>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    if q.name.trim().chars().count() < 2 {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "give at least two letters of the caller's name",
        );
    }
    match model::find_consents(pool, station, &q.name).await {
        Ok(found) => Json(found).into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct EraseConsent {
    by: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ConsentErased {
    erased: u64,
}

/// Erase a caller's consent at their request: every consent of that call loses its name and
/// topic; its date and the words agreed stay as evidence. A caller still in the studio is
/// forgotten there too.
#[utoipa::path(
    post,
    path = "/api/v1/retention/consents/{id}/erase",
    operation_id = "retention_erase_consent",
    tag = "retention",
    summary = "Erase a caller's consent",
    description = "Admin only. Erases the consent record and forgets the caller in the studio.",
    params(("id" = i64, Path, description = "The consent")),
    request_body = EraseConsent,
    responses(
        (status = 200, description = "How many rows were erased.", body = ConsentErased),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn erase_consent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<EraseConsent>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let mut req = req;
    if let Some(name) = signed_in(&state, &headers) {
        req.by = name;
    }
    let by = req.by.trim();
    if by.is_empty() || by.chars().count() > 120 || by.chars().any(char::is_control) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "`by` names who erased it: 1 to 120 characters",
        );
    }
    let identity: Option<(String,)> = match sqlx::query_as(
        "SELECT identity FROM caller_consent WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return db_problem(e),
    };
    let Some((identity,)) = identity else {
        return problem(StatusCode::NOT_FOUND, "no such consent");
    };
    match model::erase_consent(pool, station, id, by).await {
        Ok(n) => {
            state.hub.with_studio(|s, now| s.forget(&identity, now));
            tracing::info!(consent = id, rows = n, %by, "caller's consent erased");
            Json(ConsentErased { erased: n }).into_response()
        }
        Err(e) => db_problem(e),
    }
}

// ---- recordings and consent ----

#[derive(Serialize, utoipa::ToSchema)]
struct Created {
    id: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/recordings",
    operation_id = "retention_add_recording",
    tag = "retention",
    summary = "Note a recording",
    description = "Admin only. Record that a programme recording exists, so retention can keep and delete it.",
    request_body = weavesignal_model::retention::NewRecording,
    responses(
        (status = 201, description = "The recording's number.", body = Created),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn add_recording(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewRecording>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    if let Some(f) = &req.file
        && !model::safe_relative(f)
    {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("“{f}” is not a path inside the recordings directory."),
        );
    }
    if req.ended_at_ms.is_some_and(|e| e < req.started_at_ms) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "A recording cannot end before it starts.",
        );
    }
    match model::add_recording(pool, station, &req).await {
        Ok(id) => (StatusCode::CREATED, Json(Created { id })).into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct End {
    /// When it ended; now when left out.
    #[serde(default)]
    pub ended_at_ms: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/recordings/{id}/end",
    operation_id = "retention_end_recording",
    tag = "retention",
    summary = "End a recording",
    description = "Admin only.",
    params(("id" = i64, Path, description = "The recording")),
    request_body = End,
    responses(
        (status = 204, description = "Ended."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn end_recording(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<End>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let at = req.ended_at_ms.unwrap_or_else(crate::hub::now_ms);
    match model::end_recording(pool, station, id, at).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => problem(
            StatusCode::NOT_FOUND,
            format!("There is no open recording {id} at this station."),
        ),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub recording_id: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/retention/consents/{id}/recordings",
    operation_id = "retention_link_consent",
    tag = "retention",
    summary = "Link a consent to a recording",
    description = "Admin only.",
    params(("id" = i64, Path, description = "The consent")),
    request_body = Link,
    responses(
        (status = 204, description = "Linked."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn link_consent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<Link>,
) -> Response {
    let (pool, station) = match guard(&state, &headers) {
        Ok(g) => g,
        Err(r) => return r,
    };
    match model::link_consent(pool, station, id, req.recording_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => problem(
            StatusCode::NOT_FOUND,
            "That consent or recording is not this station's, or the recording is deleted.",
        ),
        Err(e) => db_problem(e),
    }
}

/// A consent as the studio keeps it for the screener.
pub fn call_in_of(r: ConsentRecord) -> CallIn {
    CallIn {
        identity: r.identity,
        name: r.name,
        topic: r.topic,
        consent: Consent {
            at_ms: r.accepted_at_ms,
            version: r.words_version,
            by: if r.given_by == "station" {
                ConsentBy::Station
            } else {
                ConsentBy::Caller
            },
            text: r.words,
        },
    }
}

/// At start: put the consent of every call that may still be going back in the studio.
pub async fn restore_consents(state: &AppState) -> usize {
    let Some(pool) = &state.db else { return 0 };
    match model::open_consents(pool, state.station.id).await {
        Ok(list) => {
            let n = list.len();
            state.hub.with_studio(|s, _| {
                for r in list {
                    s.call_in(call_in_of(r));
                }
            });
            n
        }
        Err(e) => {
            tracing::warn!(error = %e, "cannot read back callers' consent; a waiting caller's rejoin will look again");
            0
        }
    }
}

/// A caller's rejoin found no consent in memory: look in the database, and put it back if their
/// call has not ended. Returns whether the consent is on file now.
pub async fn restore_consent(state: &AppState, identity: &str) -> bool {
    let Some(pool) = &state.db else { return false };
    match model::open_consent_of(pool, state.station.id, identity).await {
        Ok(Some(r)) => {
            tracing::info!(caller = %identity, "consent read back from the database");
            state.hub.with_studio(|s, _| s.call_in(call_in_of(r)));
            true
        }
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(error = %e, caller = %identity, "cannot read the caller's consent");
            false
        }
    }
}

/// A call ended (hang-up or drop): its consent's period can start (ADR-0107, ADR-0113).
pub async fn call_ended(state: &AppState, identity: &str) {
    let Some(pool) = &state.db else { return };
    if let Err(e) = model::mark_call_ended(pool, state.station.id, identity).await {
        tracing::warn!(error = %e, caller = %identity, "cannot mark the call ended; its consent is kept until a day after it was given");
    }
}

/// A line at start when the station is on No retention without having chosen it, as setup must
/// say plainly that a licensed broadcaster has to choose its jurisdiction (ADR-0113). Then a
/// line for each open hold past its "keep until" date (ADR-0156): nothing releases it but a
/// person, so the log says it is still there.
pub async fn warn_at_start(pool: &sqlx::PgPool, station: StationId) {
    let setting = match model::load_setting(pool, station).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "cannot read the retention setting");
            return;
        }
    };
    if setting.updated_by.is_none() {
        tracing::warn!(
            target: "weavesignal::retention",
            "retention is No retention (the default): nothing is kept after a show except anonymous counts. A licensed broadcaster or a US statutory licensee must choose its jurisdiction in Settings"
        );
    }
    match model::holds_past_date(pool, station, &setting, crate::hub::now_ms()).await {
        Ok(late) => {
            for (h, until) in late {
                tracing::warn!(
                    target: "weavesignal::retention",
                    hold = h.id,
                    placed_by = %h.placed_by,
                    reason = %h.reason,
                    keep_until_ms = until,
                    "hold {} is past its keep-until date and still open: release or extend it in Settings",
                    h.id
                );
            }
        }
        Err(e) => tracing::warn!(error = %e, "cannot read the holds"),
    }
}

// ---- the description (ADR-0195) ----

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    view,
    choose,
    preview,
    sweep_now,
    holds,
    place_hold,
    release_hold,
    extend_hold,
    add_recording,
    end_recording,
    link_consent,
    find_consents,
    erase_consent
))]
pub(crate) struct Api;
