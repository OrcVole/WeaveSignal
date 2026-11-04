// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The playout layer's routes (M4). Admin bearer, until accounts arrive:
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/playout` | mode, what is on air, dead air, whether the default loop plays (ADR-0130), automation's queue, listeners, the logger, now playing |
//! | `GET`, `PUT /api/v1/playout/settings` | time zone, separations, US statutory licence, mode, logger days, default clock |
//! | `GET /api/v1/playout/zone` | the station's zone and whether anyone has chosen it (ADR-0189) |
//! | `GET /api/v1/library/tracks` | the music library |
//! | `PUT /api/v1/library/tracks?title=&artist=&category=&album=&label=&isrc=` | an audio file as the body: the media worker normalises it into the library |
//! | `DELETE /api/v1/library/tracks/{id}` | remove a track |
//! | `GET /api/v1/schedule/clocks`, `PUT`, `DELETE /api/v1/schedule/clocks/{id}` | clocks: `name`, `slots` |
//! | `GET`, `PUT /api/v1/schedule/week` | which clock runs each local hour |
//! | `POST /api/v1/schedule/generate` | (re)generate the log for `from_ms`..`to_ms` (default: a week from now) |
//! | `GET /api/v1/schedule/log?from_ms=&to_ms=` | the log, with each item's words and any rule it breaks |
//! | `PUT`, `DELETE /api/v1/schedule/log/{id}` | the log editor: another track for an item (checked against the rules), or none |
//! | `GET /api/v1/reports/plays?from_ms=&to_ms=&format=json\|csv\|soundexchange` | the per-play log |
//! | `GET /api/v1/reports/audience?from_ms=&to_ms=` | listener hours, sessions, tracks per hour (PPL) |
//! | `GET /api/v1/logger` | the legal logger's recordings, and the hours missing while it was on |
//!
//! Public (no bearer): `GET /api/v1/now-playing` and `GET /api/v1/now-playing/events` (server-sent
//! events), offset by the listener delay; never what comes next for a US statutory webcaster.

use std::collections::{BTreeMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;
use uuid::Uuid;
use weavesignal_model::playout::{self as rows, ClockRow, PlayoutSettings, Track};

use crate::app::{AppState, problem, refusal};
use crate::hub::now_ms;
use crate::onair::{OnAir, fmt_ms, recordings, rules_of};
use crate::schedule::{HOUR_MS, Played, SEPARATION_MAX_MIN, Slot};

type S = State<Arc<AppState>>;

/// The largest track upload, in megabytes (`playout.library_max_upload_mb`, set at start).
pub static LIBRARY_UPLOAD_MB: AtomicU64 = AtomicU64::new(200);

/// The routes, merged into the app's router.
pub fn routes(state: &Arc<AppState>) -> Router<Arc<AppState>> {
    let upload_mb = LIBRARY_UPLOAD_MB.load(Ordering::Relaxed);
    let upload = usize::try_from(upload_mb * 1024 * 1024).unwrap_or(usize::MAX);
    Router::new()
        .route("/api/v1/playout", get(status))
        .route(
            "/api/v1/playout/settings",
            get(get_settings).put(put_settings),
        )
        .route("/api/v1/playout/zone", get(get_zone))
        .route(
            "/api/v1/library/tracks",
            get(list_tracks)
                .put(import_track)
                .layer(DefaultBodyLimit::max(upload))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    crate::app::admin_first,
                )),
        )
        .route(
            "/api/v1/library/tracks/{id}",
            axum::routing::delete(delete_track),
        )
        .route("/api/v1/schedule/clocks", get(list_clocks))
        .route(
            "/api/v1/schedule/clocks/{id}",
            put(put_clock).delete(delete_clock),
        )
        .route("/api/v1/schedule/week", get(get_week).put(put_week))
        .route("/api/v1/schedule/generate", post(generate))
        .route("/api/v1/schedule/log", get(log))
        .route(
            "/api/v1/schedule/log/{id}",
            put(edit_item).delete(delete_item),
        )
        .route("/api/v1/reports/plays", get(report_plays))
        .route("/api/v1/reports/audience", get(report_audience))
        .route("/api/v1/logger", get(logger))
        .route("/api/v1/now-playing", get(now_playing))
        .route("/api/v1/now-playing/events", get(now_playing_events))
}

/// The admin check and the playout layer, or the refusal to send.
#[allow(clippy::result_large_err)]
fn admin<'a>(state: &'a AppState, headers: &HeaderMap) -> Result<&'a Arc<OnAir>, Response> {
    if let Some(r) = refusal(state, headers) {
        return Err(r);
    }
    onair(state)
}

#[allow(clippy::result_large_err)]
fn onair(state: &AppState) -> Result<&Arc<OnAir>, Response> {
    state.hub.onair.get().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "playout's schedule and logs need the database and playout's socket",
        )
    })
}

fn db_error(e: impl std::fmt::Display) -> Response {
    tracing::warn!(error = %e, "playout route");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("the database refused: {e}"),
    )
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
struct Range {
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    format: Option<String>,
}

impl Range {
    /// The range asked for, or the last day.
    fn past_day(&self) -> (i64, i64) {
        let to = self.to_ms.unwrap_or(now_ms() as i64);
        (self.from_ms.unwrap_or(to - 24 * HOUR_MS), to)
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/playout",
    operation_id = "playout_status",
    tag = "playout",
    summary = "The playout layer",
    description = "Mode, what is on air, dead air, whether the default loop plays (ADR-0130), automation's queue, listeners, the logger and now playing.",
    responses(
        (status = 200, description = "The playout layer's state.", body = crate::onair::Status),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn status(State(state): S, headers: HeaderMap) -> Response {
    match admin(&state, &headers) {
        Ok(o) => Json(o.status()).into_response(),
        Err(r) => r,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/playout/settings",
    operation_id = "get_settings",
    tag = "playout",
    summary = "Playout's settings",
    description = "Time zone, separations, US statutory licence, mode, logger days, default clock.",
    responses(
        (status = 200, description = "The settings.", body = weavesignal_model::playout::PlayoutSettings),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_settings(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::settings(&o.db, o.station).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => db_error(e),
    }
}

/// The station's zone and whether anyone has chosen it (ADR-0189). The studio asks an admin to
/// choose once, with the configuration's zone offered; any signed-in staff may read it.
#[utoipa::path(
    get,
    path = "/api/v1/playout/zone",
    operation_id = "get_zone",
    tag = "playout",
    summary = "The station's time zone",
    description = "The station's zone and whether anyone has chosen it (ADR-0189). The studio asks an admin to choose once, with the configuration's zone offered; any signed-in staff may read it.",
    responses(
        (status = 200, description = "The zone.", body = ZoneBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_zone(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::zone_is_set(&o.db, o.station).await {
        Ok(set) => {
            Json(serde_json::json!({ "zone": state.hub.zone.name(), "set": set })).into_response()
        }
        Err(e) => db_error(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/playout/settings",
    operation_id = "put_settings",
    tag = "playout",
    summary = "Change playout's settings",
    description = "Admin only. Reading the settings is any staff member's; changing them, the US statutory licence among them, is an admin's. A zone change reaches the schedule's screens at once, and `mode` (`live` or `auto`) the studio's view and the desk.",
    request_body = weavesignal_model::playout::PlayoutSettings,
    responses(
        (status = 200, description = "The settings as saved.", body = weavesignal_model::playout::PlayoutSettings),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn put_settings(
    State(state): S,
    headers: HeaderMap,
    Json(s): Json<PlayoutSettings>,
) -> Response {
    // Reading the settings is anyone's (the schedule's screens need the zone); changing them,
    // the US statutory licence among them, is an admin's.
    if let Some(r) = crate::app::admin_refusal(&state, &headers) {
        return r;
    }
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    if jiff::tz::TimeZone::get(&s.time_zone).is_err() {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "`{}` is not a time zone (an IANA name such as Europe/London)",
                s.time_zone
            ),
        );
    }
    if s.mode != "live" && s.mode != "auto" {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "mode is `live` or `auto`");
    }
    if [
        s.artist_separation_min,
        s.title_separation_min,
        s.album_separation_min,
    ]
    .iter()
    .any(|m| *m > SEPARATION_MAX_MIN)
    {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a separation is at most 1440 minutes",
        );
    }
    match rows::put_settings(&o.db, o.station, &s).await {
        Ok(()) => {
            // Show slots and the Schedule tab follow at once (review 0025, S1).
            state.hub.zone.set(&s.time_zone);
            // The studio's view and the desk follow the mode at once too (ADR-0183, ADR-0172).
            if let Some(mode) = crate::desk::StationMode::parse(&s.mode) {
                state.hub.set_mode(mode).await;
            }
            Json(s).into_response()
        }
        Err(e) => db_error(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/library/tracks",
    operation_id = "list_tracks",
    tag = "library",
    summary = "The music library",
    responses(
        (status = 200, description = "The tracks.", body = TracksBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_tracks(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::tracks(&o.db, o.station).await {
        Ok(t) => Json(serde_json::json!({ "tracks": t })).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct TrackMeta {
    title: String,
    artist: String,
    category: String,
    #[serde(default)]
    album: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    isrc: Option<String>,
}

fn valid_isrc(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..5]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && b[5..].iter().all(u8::is_ascii_digit)
}

fn valid_category(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

#[utoipa::path(
    put,
    path = "/api/v1/library/tracks",
    operation_id = "import_track",
    tag = "library",
    summary = "Add a track to the library",
    description = "An audio file as the body, the track's words in the query: the media worker normalises it into the library.",
    params(TrackMeta),
    request_body(content = crate::openapi::File, content_type = "application/octet-stream", description = "The audio file, at most `playout.library_max_upload_mb`."),
    responses(
        (status = 201, description = "The track.", body = weavesignal_model::playout::Track),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 413, description = "The file is larger than `playout.library_max_upload_mb`.", body = crate::openapi::Problem),
        (status = 500, description = "The media worker or the library failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn import_track(
    State(state): S,
    headers: HeaderMap,
    Query(m): Query<TrackMeta>,
    body: Bytes,
) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let title = m.title.trim().to_owned();
    let artist = m.artist.trim().to_owned();
    if title.is_empty()
        || title.chars().count() > 300
        || artist.is_empty()
        || artist.chars().count() > 300
    {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a track needs a title and an artist (at most 300 characters each)",
        );
    }
    if !valid_category(&m.category) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a category is 1 to 32 of a-z, 0-9, _ and -",
        );
    }
    let isrc = m
        .isrc
        .map(|i| i.trim().replace('-', "").to_uppercase())
        .filter(|i| !i.is_empty());
    if isrc.as_deref().is_some_and(|i| !valid_isrc(i)) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "an ISRC is 12 characters: CC-XXX-YY-NNNNN",
        );
    }
    if m.album.chars().count() > 300 || m.label.chars().count() > 200 {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the album or label is too long",
        );
    }
    let mut cfg = state.hub.carts.config.clone();
    cfg.loudness_lufs = o.links.library_lufs;
    cfg.max_seconds = o.links.library_max_seconds;
    let (flac, report) = match crate::media::normalise(&cfg, body.to_vec()).await {
        Ok(r) => r,
        Err(crate::media::WorkerError::Refused(e)) => {
            return problem(StatusCode::UNPROCESSABLE_ENTITY, e);
        }
        Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let file = match crate::media::store(&o.links.library_dir, &flac).await {
        Ok(f) => f,
        Err(e) => {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot write the library: {e}"),
            );
        }
    };
    let t = Track {
        id: Uuid::new_v4(),
        title,
        artist,
        album: m.album.trim().to_owned(),
        label: m.label.trim().to_owned(),
        isrc,
        category: m.category,
        file,
        duration_ms: u32::try_from(report.duration_ms).unwrap_or(u32::MAX).max(1),
        gain_db: report.play_gain_db as f32,
        loudness_lufs: Some(report.integrated_lufs as f32),
        true_peak_dbtp: Some(report.true_peak_dbtp as f32),
        enabled: true,
    };
    match rows::put_track(&o.db, o.station, &t).await {
        Ok(()) => (StatusCode::CREATED, Json(t)).into_response(),
        Err(e) => {
            let _ = tokio::fs::remove_file(o.links.library_dir.join(&t.file)).await;
            db_error(e)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/library/tracks/{id}",
    operation_id = "delete_track",
    tag = "library",
    summary = "Remove a track",
    params(("id" = Uuid, Path, description = "The track")),
    responses(
        (status = 204, description = "Removed."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_track(State(state): S, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::delete_track(&o.db, o.station, id).await {
        Ok(Some(file)) => {
            let _ = tokio::fs::remove_file(o.links.library_dir.join(file)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such track"),
        Err(e) => db_error(e),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
struct ClockView {
    id: Uuid,
    name: String,
    slots: Vec<Slot>,
}

#[utoipa::path(
    get,
    path = "/api/v1/schedule/clocks",
    operation_id = "list_clocks",
    tag = "schedule",
    summary = "The hour clocks",
    responses(
        (status = 200, description = "The clocks.", body = ClocksBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_clocks(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::clocks(&o.db, o.station).await {
        Ok(cs) => Json(serde_json::json!({
            "clocks": cs.into_iter().map(|c| ClockView {
                id: c.id,
                name: c.name,
                slots: serde_json::from_str(&c.slots_json).unwrap_or_default(),
            }).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct ClockBody {
    name: String,
    slots: Vec<Slot>,
}

#[utoipa::path(
    put,
    path = "/api/v1/schedule/clocks/{id}",
    operation_id = "put_clock",
    tag = "schedule",
    summary = "Make or change a clock",
    params(("id" = Uuid, Path, description = "The clock")),
    request_body = ClockBody,
    responses(
        (status = 200, description = "The clock as saved.", body = ClockView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_clock(
    State(state): S,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<ClockBody>,
) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a clock's name is 1 to 80 characters",
        );
    }
    if b.slots.is_empty() || b.slots.len() > 60 {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a clock has 1 to 60 slots",
        );
    }
    if let Some(Slot::Category(c)) = b
        .slots
        .iter()
        .find(|s| matches!(s, Slot::Category(c) if !valid_category(c)))
    {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("`{c}` is not a category"),
        );
    }
    let row = ClockRow {
        id,
        name: name.clone(),
        slots_json: serde_json::to_string(&b.slots).expect("slots serialise"),
    };
    match rows::put_clock(&o.db, o.station, &row).await {
        Ok(()) => Json(ClockView {
            id,
            name,
            slots: b.slots,
        })
        .into_response(),
        Err(e) => db_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/schedule/clocks/{id}",
    operation_id = "delete_clock",
    tag = "schedule",
    summary = "Remove a clock",
    params(("id" = Uuid, Path, description = "The clock")),
    responses(
        (status = 204, description = "Removed."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_clock(State(state): S, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::delete_clock(&o.db, o.station, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such clock"),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct WeekHour {
    /// Monday is 0.
    weekday: u8,
    hour: u8,
    clock: Uuid,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct WeekBody {
    hours: Vec<WeekHour>,
}

#[utoipa::path(
    get,
    path = "/api/v1/schedule/week",
    operation_id = "get_week",
    tag = "schedule",
    summary = "Which clock runs each hour",
    responses(
        (status = 200, description = "The week.", body = WeekBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_week(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::week(&o.db, o.station).await {
        Ok(w) => Json(WeekBody {
            hours: w
                .into_iter()
                .map(|(weekday, hour, clock)| WeekHour {
                    weekday,
                    hour,
                    clock,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => db_error(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/schedule/week",
    operation_id = "put_week",
    tag = "schedule",
    summary = "Set which clock runs each hour",
    description = "Each hour is a weekday 0-6 (Monday 0) and an hour 0-23, once; every clock must be known.",
    request_body = WeekBody,
    responses(
        (status = 200, description = "The week as saved.", body = WeekBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_week(State(state): S, headers: HeaderMap, Json(b): Json<WeekBody>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let mut seen = HashSet::new();
    for h in &b.hours {
        if h.weekday > 6 || h.hour > 23 || !seen.insert((h.weekday, h.hour)) {
            return problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "each hour is a weekday 0-6 (Monday 0) and an hour 0-23, once",
            );
        }
    }
    let hours: Vec<(u8, u8, Uuid)> = b
        .hours
        .iter()
        .map(|h| (h.weekday, h.hour, h.clock))
        .collect();
    match rows::put_week(&o.db, o.station, &hours).await {
        Ok(()) => Json(b).into_response(),
        Err(e) => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("the week was refused (is every clock known?): {e}"),
        ),
    }
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct GenerateBody {
    from_ms: Option<i64>,
    to_ms: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/v1/schedule/generate",
    operation_id = "schedule_generate",
    tag = "schedule",
    summary = "Generate the log",
    description = "(Re)generate the log for `from_ms` to `to_ms` (default: a week from now; at most 31 days ahead). The body may be left out.",
    request_body = GenerateBody,
    responses(
        (status = 200, description = "How many items were made, and what could not be filled.", body = GeneratedBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn generate(
    State(state): S,
    headers: HeaderMap,
    body: Option<Json<GenerateBody>>,
) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let now = now_ms() as i64;
    let from = b.from_ms.unwrap_or(now).max(now);
    let to = b.to_ms.unwrap_or(from + 7 * 24 * HOUR_MS);
    if to <= from || to - from > 31 * 24 * HOUR_MS {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "generate up to 31 days ahead, `to_ms` after `from_ms`",
        );
    }
    match o.generate(from, to, true).await {
        Ok((items, gaps)) => {
            Json(serde_json::json!({ "items": items, "gaps": gaps })).into_response()
        }
        Err(e) => problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
struct LogEntry {
    #[serde(flatten)]
    item: rows::LogItem,
    title: String,
    artist: String,
    breaches: Vec<&'static str>,
}

#[utoipa::path(
    get,
    path = "/api/v1/schedule/log",
    operation_id = "schedule_log",
    tag = "schedule",
    summary = "The log",
    description = "The log between `from_ms` and `to_ms` (default: from an hour ago to a day ahead; at most 8 days), with each item's words and any rule it breaks.",
    params(("from_ms" = Option<i64>, Query, description = "From, in milliseconds since the Unix epoch"), ("to_ms" = Option<i64>, Query, description = "To (exclusive), in milliseconds since the Unix epoch")),
    responses(
        (status = 200, description = "The log.", body = LogBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn log(State(state): S, headers: HeaderMap, Query(r): Query<Range>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let now = now_ms() as i64;
    let from = r.from_ms.unwrap_or(now - HOUR_MS);
    let to = r.to_ms.unwrap_or(now + 24 * HOUR_MS);
    if to <= from || to - from > 8 * 24 * HOUR_MS {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "ask for at most 8 days");
    }
    let settings = match rows::settings(&o.db, o.station).await {
        Ok(s) => s,
        Err(e) => return db_error(e),
    };
    let us = o.retained().await.map(|r| r.us_statutory).unwrap_or(true);
    let rules = rules_of(&settings, us);
    let horizon = rules.horizon_ms();
    let (items, plays, tracks) = match tokio::try_join!(
        rows::items(&o.db, o.station, from, to),
        rows::planned_tracks(&o.db, o.station, from - horizon, to),
        rows::tracks(&o.db, o.station),
    ) {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let names: BTreeMap<Uuid, (String, String)> = tracks
        .into_iter()
        .map(|t| (t.id, (t.title, t.artist)))
        .collect();
    let played: Vec<Played> = plays
        .iter()
        .map(|p| Played::new(p.track, &p.title, &p.artist, &p.album, p.at_ms))
        .collect();
    let mut breaches: BTreeMap<(i64, Uuid), Vec<&'static str>> = BTreeMap::new();
    for (i, b) in rules.verify(&played) {
        breaches
            .entry((played[i].at_ms, played[i].track))
            .or_default()
            .push(b.describe());
    }
    let carts: BTreeMap<Uuid, String> = state
        .hub
        .catalogue()
        .carts
        .iter()
        .map(|c| (c.cart.id, c.cart.label.clone()))
        .collect();
    let entries: Vec<LogEntry> = items
        .into_iter()
        .map(|i| {
            let (title, artist) = match (i.track, i.cart) {
                (Some(t), _) => names.get(&t).cloned().unwrap_or_default(),
                (_, Some(c)) => (carts.get(&c).cloned().unwrap_or_default(), String::new()),
                _ => Default::default(),
            };
            let b = i
                .track
                .and_then(|t| breaches.get(&(i.planned_at_ms, t)).cloned())
                .unwrap_or_default();
            LogEntry {
                item: i,
                title,
                artist,
                breaches: b,
            }
        })
        .collect();
    Json(serde_json::json!({ "items": entries })).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct EditBody {
    track: Uuid,
    /// Accept a break of the station's own separations (never of the US statutory licence).
    #[serde(default)]
    force: bool,
}

#[utoipa::path(
    put,
    path = "/api/v1/schedule/log/{id}",
    operation_id = "edit_item",
    tag = "schedule",
    summary = "Another track for a log item",
    description = "The log editor: another track for an item, checked against the rules; `force` accepts a rule it breaks.",
    params(("id" = i64, Path, description = "The log item")),
    request_body = EditBody,
    responses(
        (status = 200, description = "The item as edited, with a note of the rules it now breaks.", body = EditedBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn edit_item(
    State(state): S,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<EditBody>,
) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (item, track, settings) = match tokio::try_join!(
        rows::item(&o.db, o.station, id),
        rows::track(&o.db, o.station, b.track),
        rows::settings(&o.db, o.station),
    ) {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let Some(item) = item else {
        return problem(StatusCode::NOT_FOUND, "no such log item");
    };
    let Some(track) = track else {
        return problem(StatusCode::NOT_FOUND, "no such track");
    };
    if item.status != "planned" {
        return problem(
            StatusCode::CONFLICT,
            format!("the item is {} already", item.status),
        );
    }
    if item.kind != "track" {
        return problem(
            StatusCode::CONFLICT,
            "only a track item takes another track",
        );
    }
    let us = o.retained().await.map(|r| r.us_statutory).unwrap_or(true);
    let rules = rules_of(&settings, us);
    let h = rules.horizon_ms();
    let at = item.planned_at_ms;
    let around = match rows::planned_tracks(&o.db, o.station, at - h, at + h + 1).await {
        Ok(p) => p,
        Err(e) => return db_error(e),
    };
    let to_played =
        |p: &rows::RecentPlay| Played::new(p.track, &p.title, &p.artist, &p.album, p.at_ms);
    let before: Vec<Played> = around.iter().map(to_played).collect();
    let mut after: Vec<Played> = around
        .iter()
        .filter(|p| !(p.at_ms == at && Some(p.track) == item.track))
        .map(to_played)
        .collect();
    let pos = after.partition_point(|p| p.at_ms <= at);
    after.insert(
        pos,
        Played::new(track.id, &track.title, &track.artist, &track.album, at),
    );
    let key =
        |log: &[Played], (i, b): (usize, crate::schedule::Breach)| (log[i].at_ms, log[i].track, b);
    let was: HashSet<_> = rules
        .verify(&before)
        .into_iter()
        .map(|x| key(&before, x))
        .collect();
    let new: Vec<crate::schedule::Breach> = rules
        .verify(&after)
        .into_iter()
        .map(|x| key(&after, x))
        .filter(|k| !was.contains(k))
        .map(|k| k.2)
        .collect();
    if new.iter().any(|b| b.legal()) {
        return problem(
            StatusCode::CONFLICT,
            format!(
                "refused: {} (the US statutory licence)",
                new.iter()
                    .filter(|b| b.legal())
                    .map(|b| b.describe())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    if !new.is_empty() && !b.force {
        return problem(
            StatusCode::CONFLICT,
            format!(
                "it would break {}; send `force` to accept",
                new.iter()
                    .map(|b| b.describe())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    let note = if new.is_empty() {
        "edited".to_owned()
    } else {
        format!(
            "edited over: {}",
            new.iter()
                .map(|b| b.describe())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    match rows::update_item(
        &o.db,
        o.station,
        id,
        Some((track.id, track.duration_ms)),
        None,
        Some(&note),
    )
    .await
    {
        Ok(_) => {
            Json(serde_json::json!({ "id": id, "track": track.id, "note": note })).into_response()
        }
        Err(e) => db_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/schedule/log/{id}",
    operation_id = "delete_item",
    tag = "schedule",
    summary = "Take a planned item out of the log",
    params(("id" = i64, Path, description = "The log item")),
    responses(
        (status = 204, description = "Removed."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_item(State(state): S, headers: HeaderMap, Path(id): Path<i64>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match rows::delete_item(&o.db, o.station, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => problem(StatusCode::NOT_FOUND, "no planned item with that number"),
        Err(e) => db_error(e),
    }
}

fn csv_field(s: &str) -> String {
    // A leading =, +, - or @ is quoted with an apostrophe so a spreadsheet never runs it.
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/reports/plays",
    operation_id = "report_plays",
    tag = "reports",
    summary = "The per-play log",
    description = "What aired between `from_ms` and `to_ms` (default: the last day), as JSON, CSV, or in SoundExchange's layout.",
    params(("from_ms" = Option<i64>, Query, description = "From, in milliseconds since the Unix epoch"), ("to_ms" = Option<i64>, Query, description = "To (exclusive), in milliseconds since the Unix epoch"), ("format" = Option<String>, Query, description = "`json` (the default), `csv` or `soundexchange`")),
    responses(
        (status = 200, description = "The plays. `json` is the body shown; `csv` and `soundexchange` answer `text/csv`.", body = PlaysBody),
        (status = 200, description = "The plays as CSV.", body = String, content_type = "text/csv"),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn report_plays(State(state): S, headers: HeaderMap, Query(r): Query<Range>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (from, to) = r.past_day();
    let plays = match rows::plays(&o.db, o.station, from, to).await {
        // A cart a dump threw away never reached listeners: it is no play.
        Ok(p) => p.into_iter().filter(|p| p.aired).collect::<Vec<_>>(),
        Err(e) => return db_error(e),
    };
    match r.format.as_deref().unwrap_or("json") {
        "json" => Json(serde_json::json!({ "from_ms": from, "to_ms": to, "plays": plays }))
            .into_response(),
        "csv" => {
            let mut out = String::from(
                "started_at,ended_at,kind,title,artist,album,label,isrc,duration_s,listeners\n",
            );
            for p in &plays {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{:.1},{}\n",
                    fmt_ms(p.started_at_ms),
                    p.ended_at_ms.map(fmt_ms).unwrap_or_default(),
                    p.kind,
                    csv_field(&p.title),
                    csv_field(&p.artist),
                    csv_field(&p.album),
                    csv_field(&p.label),
                    p.isrc.as_deref().unwrap_or(""),
                    f64::from(p.duration_ms) / 1000.0,
                    p.listeners
                ));
            }
            ([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out).into_response()
        }
        // SoundExchange's Report of Use: one row a sound recording, its performances being the
        // listeners at each play, summed (carts and fallback files are not sound recordings
        // the log can name; they are left out).
        "soundexchange" => {
            let mut rows_by: BTreeMap<(String, String, String, String, String), u64> =
                BTreeMap::new();
            for p in plays.iter().filter(|p| p.kind == "track") {
                *rows_by
                    .entry((
                        p.artist.clone(),
                        p.title.clone(),
                        p.isrc.clone().unwrap_or_default(),
                        p.album.clone(),
                        p.label.clone(),
                    ))
                    .or_default() += u64::from(p.listeners);
            }
            let mut out = String::from(
                "FEATURED_ARTIST,SOUND_RECORDING_TITLE,ISRC,ALBUM_TITLE,MARKETING_LABEL,ACTUAL_TOTAL_PERFORMANCES\n",
            );
            for ((artist, title, isrc, album, label), n) in rows_by {
                out.push_str(&format!(
                    "{},{},{},{},{},{n}\n",
                    csv_field(&artist),
                    csv_field(&title),
                    isrc,
                    csv_field(&album),
                    csv_field(&label)
                ));
            }
            ([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out).into_response()
        }
        other => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("format `{other}`: json, csv or soundexchange"),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/reports/audience",
    operation_id = "report_audience",
    tag = "reports",
    summary = "Audience",
    description = "Listener hours, sessions, peak and tracks per hour between `from_ms` and `to_ms` (default: the last day).",
    params(("from_ms" = Option<i64>, Query, description = "From, in milliseconds since the Unix epoch"), ("to_ms" = Option<i64>, Query, description = "To (exclusive), in milliseconds since the Unix epoch")),
    responses(
        (status = 200, description = "The audience.", body = AudienceBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database refused.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn report_audience(State(state): S, headers: HeaderMap, Query(r): Query<Range>) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (from, to) = r.past_day();
    let (hours, plays) = match tokio::try_join!(
        rows::listening(&o.db, o.station, from, to),
        rows::plays(&o.db, o.station, from, to),
    ) {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let seconds: i64 = hours.iter().map(|h| h.listener_seconds).sum();
    let sessions: u32 = hours.iter().map(|h| h.sessions).sum();
    let peak = hours.iter().map(|h| h.peak).max().unwrap_or(0);
    let tracks = plays.iter().filter(|p| p.kind == "track").count();
    let span_h = ((to - from) as f64 / HOUR_MS as f64).max(1.0 / 60.0);
    Json(serde_json::json!({
        "from_ms": from,
        "to_ms": to,
        "listener_hours": seconds as f64 / 3600.0,
        "sessions": sessions,
        "peak": peak,
        "tracks": tracks,
        "tracks_per_hour": tracks as f64 / span_h,
        "hours": hours,
    }))
    .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/logger",
    operation_id = "reports_logger",
    tag = "reports",
    summary = "The legal logger's recordings",
    description = "The recordings, and the hours of the last day with none while the logger was on.",
    responses(
        (status = 200, description = "The logger.", body = LoggerBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn logger(State(state): S, headers: HeaderMap) -> Response {
    let o = match admin(&state, &headers) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let dir = o.links.logger_dir.clone();
    let recs = tokio::task::spawn_blocking(move || recordings(&dir))
        .await
        .unwrap_or_default();
    let status = o.status();
    // The hours of the last day with no recording while the logger is on.
    let now = now_ms() as i64;
    let this_hour = now - now.rem_euclid(HOUR_MS);
    let have: HashSet<i64> = recs
        .iter()
        .map(|r| r.start_ms - r.start_ms.rem_euclid(HOUR_MS))
        .collect();
    let first = recs
        .first()
        .map(|r| r.start_ms - r.start_ms.rem_euclid(HOUR_MS))
        .unwrap_or(this_hour);
    let missing: Vec<String> = (0..24)
        .map(|k| this_hour - k * HOUR_MS)
        .filter(|h| *h >= first && !have.contains(h))
        .map(fmt_ms)
        .collect();
    Json(serde_json::json!({
        "on": status.logger.as_ref().map(|l| l.on),
        "records": status.retained.logger,
        "error": status.logger_error,
        "recordings": recs,
        "missing_hours": missing,
    }))
    .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/now-playing",
    operation_id = "now_playing",
    tag = "public",
    summary = "What is playing now",
    description = "What listeners are told is playing, offset by the listener delay. Never what comes next for a US statutory webcaster.",
    responses(
        (status = 200, description = "Now playing.", body = crate::onair::NowPlaying),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn now_playing(State(state): S) -> Response {
    match onair(&state) {
        Ok(o) => Json((*o.now_playing()).clone()).into_response(),
        Err(r) => r,
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/now-playing/events",
    operation_id = "now_playing_events",
    tag = "public",
    summary = "What is playing now, as events",
    description = "Server-sent events: a `now_playing` event whose data is the same JSON as `GET /api/v1/now-playing`, at once and on every change.",
    responses(
        (status = 200, description = "An event stream (`text/event-stream`).", body = String, content_type = "text/event-stream"),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn now_playing_events(State(state): S) -> Response {
    let o = match onair(&state) {
        Ok(o) => o,
        Err(r) => return r,
    };
    let stream = WatchStream::new(o.subscribe()).map(|np| {
        Ok::<_, Infallible>(
            Event::default()
                .event("now_playing")
                .data(serde_json::to_string(&*np).unwrap_or_default()),
        )
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---- the description (ADR-0195) ----

/// Documentation of the answers the handlers build with `json!`; the handlers send these shapes.
#[derive(Serialize, utoipa::ToSchema)]
struct ZoneBody {
    /// An IANA zone name.
    zone: String,
    /// Whether an admin has chosen it.
    set: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
struct TracksBody {
    tracks: Vec<Track>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct ClocksBody {
    clocks: Vec<ClockView>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct GeneratedBody {
    /// How many items the log now holds for the span.
    items: usize,
    /// What could not be filled, in words.
    gaps: Vec<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct LogBody {
    items: Vec<LogEntry>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct EditedBody {
    id: i64,
    track: Uuid,
    note: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct PlaysBody {
    from_ms: i64,
    to_ms: i64,
    plays: Vec<rows::Play>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct AudienceBody {
    from_ms: i64,
    to_ms: i64,
    listener_hours: f64,
    sessions: u32,
    peak: u32,
    tracks: usize,
    tracks_per_hour: f64,
    hours: Vec<rows::ListenerHour>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct LoggerBody {
    /// Whether the logger is on; `null` while playout does not say.
    on: Option<bool>,
    /// Whether the retention setting lets the logger record.
    records: bool,
    error: Option<String>,
    recordings: Vec<crate::onair::Recording>,
    /// The hours of the last day with no recording while the logger was on.
    missing_hours: Vec<String>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    status,
    get_settings,
    get_zone,
    put_settings,
    list_tracks,
    import_track,
    delete_track,
    list_clocks,
    put_clock,
    delete_clock,
    get_week,
    put_week,
    generate,
    log,
    edit_item,
    delete_item,
    report_plays,
    report_audience,
    logger,
    now_playing,
    now_playing_events
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isrcs_and_categories_are_checked() {
        assert!(valid_isrc("GBAYE6800011"));
        assert!(!valid_isrc("gbaye6800011"));
        assert!(!valid_isrc("GB-AYE-68-00011"));
        assert!(valid_category("gold"));
        assert!(!valid_category("Gold"));
        assert!(!valid_category(""));
    }

    #[test]
    fn csv_fields_are_quoted_and_never_formulas() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a, b"), "\"a, b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
    }
}
