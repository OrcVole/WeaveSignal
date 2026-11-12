// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video path's routes (M5, ADR-0010): the slate's still, and the cameras' thumbnails.
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/video/still` | the slate's still: whether there is one, its size and when it was set |
//! | `PUT /api/v1/video/still` | a PNG or JPEG as the body: the media worker reads it in its sandbox and writes a clean PNG for the desk to show |
//! | `DELETE /api/v1/video/still` | take it away; the slate is text again |
//! | `GET /api/v1/video/still/image` | the PNG the desk shows, for the studio's preview |
//! | `GET /api/v1/video/thumbs` | a small JPEG of each camera the desk sees, as base64, refreshed by the desk about once a second |
//!
//! | `GET`, `PUT`, `DELETE /api/v1/shows/{show}/still` and `GET …/still/image` | a show's own still (ADR-0200), the same way |
//!
//! All of them are for staff (the same bearers as the cart wall). The desk reads the still from
//! a file ([`VideoConfig::still_file`]) and notices when it changes, so a desk that restarts
//! shows it again without being told.
//!
//! **Which still the slate shows** (ADR-0200): the show on air's, else the station's. They are
//! kept beside the desk's file, `station.png` and `show-<id>.png`; [`spawn_slate_still`] looks
//! every few seconds at the schedule (the episode whose air time holds now) and copies the one
//! that should show into the desk's file, all or nothing, or removes it when there is none.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use uuid::Uuid;

use crate::app::{AppState, on_air_refusal, problem, refusal, staff_first};
use crate::config::VideoConfig;
use crate::desk::DeskError;
use crate::media::WorkerError;

pub fn routes(state: &Arc<AppState>) -> Router<Arc<AppState>> {
    let limit =
        usize::try_from(state.video.still_max_upload_mb * 1024 * 1024).unwrap_or(usize::MAX);
    Router::new()
        .route(
            "/api/v1/video/still",
            get(still_info)
                .put(put_still)
                .delete(delete_still)
                .layer(DefaultBodyLimit::max(limit))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    staff_first,
                )),
        )
        .route("/api/v1/video/still/image", get(still_image))
        .route("/api/v1/video/thumbs", get(thumbs))
        .route(
            "/api/v1/shows/{show}/still",
            get(show_still_info)
                .put(put_show_still)
                .delete(delete_show_still)
                .layer(DefaultBodyLimit::max(limit))
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    staff_first,
                )),
        )
        .route("/api/v1/shows/{show}/still/image", get(show_still_image))
}

/// The slate's still as it is now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct StillInfo {
    pub present: bool,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// When it was set, in milliseconds since the Unix epoch.
    pub set_at_ms: u64,
}

/// What the file says about itself; `None` when there is no still (or it is no PNG).
pub async fn info(cfg: &VideoConfig) -> Option<StillInfo> {
    info_at(station_still(cfg)).await
}

async fn info_at(path: PathBuf) -> Option<StillInfo> {
    tokio::task::spawn_blocking(move || info_blocking(&path))
        .await
        .ok()
        .flatten()
}

/// The folder the stills share with the desk's file.
fn stills_dir(cfg: &VideoConfig) -> PathBuf {
    cfg.still_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// The station's own still (ADR-0200): what the slate shows when the show on air has none.
pub fn station_still(cfg: &VideoConfig) -> PathBuf {
    stills_dir(cfg).join("station.png")
}

/// A show's still (ADR-0200).
pub fn show_still(cfg: &VideoConfig, show: Uuid) -> PathBuf {
    stills_dir(cfg).join(format!("show-{show}.png"))
}

fn info_blocking(path: &Path) -> Option<StillInfo> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    if &head[..8] != b"\x89PNG\r\n\x1a\n" || &head[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    Some(StillInfo {
        present: true,
        width: be(16),
        height: be(20),
        bytes: meta.len(),
        set_at_ms: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64),
    })
}

/// Make the upload the station's still: the worker's clean PNG, written all or nothing.
pub async fn set_still(
    state: &AppState,
    bytes: Vec<u8>,
) -> Result<StillInfo, (StatusCode, String)> {
    let target = station_still(&state.video);
    set_still_at(state, bytes, target).await
}

/// The worker's clean PNG of an upload, written all or nothing at `path`; then the slate is
/// brought up to date at once rather than at the next look.
async fn set_still_at(
    state: &AppState,
    bytes: Vec<u8>,
    path: PathBuf,
) -> Result<StillInfo, (StatusCode, String)> {
    let cfg = &state.video;
    let (png, _report) = crate::media::still(
        &state.hub.carts.config,
        bytes,
        cfg.still_max_width,
        cfg.still_max_height,
    )
    .await
    .map_err(|e| match e {
        WorkerError::Refused(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
        WorkerError::Failed(m) => {
            tracing::warn!(error = %m, "the media worker failed on a still");
            (StatusCode::INTERNAL_SERVER_ERROR, e_text(&m))
        }
    })?;
    let io = |e: std::io::Error| {
        tracing::warn!(error = %e, path = %path.display(), "cannot write the still");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot write the still: {e}"),
        )
    };
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io)?;
    }
    let tmp = path.with_extension("png.part");
    tokio::fs::write(&tmp, &png).await.map_err(io)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io)?;
    apply(state, &mut None).await;
    info_at(path).await.ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "the still was written and cannot be read back".to_owned(),
    ))
}

fn e_text(m: &str) -> String {
    format!("the media worker failed: {m}")
}

#[utoipa::path(
    get,
    path = "/api/v1/video/still",
    operation_id = "video_still_info",
    tag = "video",
    summary = "The slate's picture",
    description = "Whether there is one, its size in pixels and bytes, and when it was set.",
    responses(
        (status = 200, description = "The still as it is now.", body = StillInfo),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn still_info(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    Json(info(&state.video).await.unwrap_or(StillInfo {
        present: false,
        width: 0,
        height: 0,
        bytes: 0,
        set_at_ms: 0,
    }))
    .into_response()
}

#[utoipa::path(
    put,
    path = "/api/v1/video/still",
    operation_id = "video_put_still",
    tag = "video",
    summary = "Set the station's slate picture",
    description = "The slate shows it while the show on air has no picture of its own (ADR-0200). A PNG or JPEG as the body (at most `video.still_max_upload_mb`, 24 MB by default). The media worker reads it in its sandbox and writes a clean PNG, fitted and never enlarged.",
    request_body(content = crate::openapi::File, content_type = "image/*", description = "A PNG or JPEG."),
    responses(
        (status = 201, description = "The still.", body = StillInfo),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 413, description = "The picture is larger than `video.still_max_upload_mb`.", body = crate::openapi::Problem),
        (status = 500, description = "The media worker or the disk failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_still(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = on_air_refusal(&state, &headers) {
        return r;
    }
    match set_still(&state, body.to_vec()).await {
        Ok(info) => (StatusCode::CREATED, Json(info)).into_response(),
        Err((code, m)) => problem(code, m),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/video/still",
    operation_id = "video_delete_still",
    tag = "video",
    summary = "Take the station's picture away",
    description = "The slate shows the show on air's picture if it has one, else words. 204 also when there was none.",
    responses(
        (status = 204, description = "Gone."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 500, description = "The file could not be removed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_still(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = on_air_refusal(&state, &headers) {
        return r;
    }
    match remove(&state, station_still(&state.video)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot take the still away: {e}"),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/video/still/image",
    operation_id = "video_still_image",
    tag = "video",
    summary = "The slate's picture",
    description = "The PNG the desk shows, never cached.",
    responses(
        (status = 200, description = "The picture.", body = crate::openapi::File, content_type = "image/png"),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn still_image(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    match tokio::fs::read(station_still(&state.video)).await {
        Ok(bytes) if info(&state.video).await.is_some() => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        _ => problem(StatusCode::NOT_FOUND, "the station has no still"),
    }
}

/// The cameras the desk sees now, each as a small JPEG in base64.
#[utoipa::path(
    get,
    path = "/api/v1/video/thumbs",
    operation_id = "video_thumbs",
    tag = "video",
    summary = "Camera thumbnails",
    description = "A small picture of each camera the desk sees now, made by the desk about once a second. 409 with video off, 503 while the desk does not answer.",
    responses(
        (status = 200, description = "The thumbnails.", body = ThumbsBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn thumbs(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    match state.hub.desk.thumbs().await {
        Ok(thumbs) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "thumbs": thumbs })),
        )
            .into_response(),
        Err(DeskError::NotConfigured | DeskError::Unreachable { .. }) => {
            problem(StatusCode::SERVICE_UNAVAILABLE, "the desk is not answering")
        }
        // The desk refuses with video off.
        Err(e) => problem(StatusCode::CONFLICT, e.to_string()),
    }
}

// ---- the slate's still: the show on air's, else the station's (ADR-0200) ----

/// How often the schedule is looked at for the show on air.
const SLATE_STILL_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Remove a still and bring the slate up to date; a still that was not there is no error.
async fn remove(state: &AppState, path: PathBuf) -> std::io::Result<()> {
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    apply(state, &mut None).await;
    Ok(())
}

/// The show whose episode is on air at `now_s` by the schedule: aired from its air time for its
/// length (the episode's, else its series' or show's slot, else an hour). The latest wins.
#[cfg(test)]
fn show_on_air(
    episodes: &[(
        weavesignal_model::shows::Show,
        weavesignal_model::shows::Series,
        weavesignal_model::shows::Episode,
    )],
    now_s: i64,
) -> Option<Uuid> {
    on_air_entry(episodes, now_s).map(|(show, _, _)| show.id)
}

fn on_air_entry(
    episodes: &[(
        weavesignal_model::shows::Show,
        weavesignal_model::shows::Series,
        weavesignal_model::shows::Episode,
    )],
    now_s: i64,
) -> Option<&(
    weavesignal_model::shows::Show,
    weavesignal_model::shows::Series,
    weavesignal_model::shows::Episode,
)> {
    weavesignal_model::shows::on_air_at(episodes, now_s)
}

/// The show on air now by the schedule (ADR-0200's rule, shared with the consent words,
/// ADR-0214): `None` without a database, when no episode holds now, or when the schedule
/// cannot be read (logged at debug, as the slate does).
pub(crate) async fn on_air_show(state: &AppState) -> Option<weavesignal_model::shows::Show> {
    let pool = state.db.as_ref()?;
    let now_s = jiff::Timestamp::now().as_second();
    match weavesignal_model::shows::episodes_between(
        pool,
        state.station.id,
        now_s - 36 * 3600,
        now_s + 1,
    )
    .await
    {
        Ok(eps) => on_air_entry(&eps, now_s).map(|(show, _, _)| show.clone()),
        Err(e) => {
            tracing::debug!(error = %e, "the show on air: the schedule is not readable");
            None
        }
    }
}

/// The still the slate should show now: the show on air's, else the station's, else none.
async fn wanted(state: &AppState) -> Option<PathBuf> {
    let cfg = &state.video;
    if let Some(show) = on_air_show(state).await {
        let p = show_still(cfg, show.id);
        if tokio::fs::try_exists(&p).await.unwrap_or(false) {
            return Some(p);
        }
    }
    let p = station_still(cfg);
    tokio::fs::try_exists(&p)
        .await
        .unwrap_or(false)
        .then_some(p)
}

/// What a still file is, to tell when it changed: its path, length and time.
type Seen = Option<(PathBuf, u64, std::time::SystemTime)>;

/// Copy the still the slate should show into the desk's file, all or nothing, when it differs
/// from what was copied last (`last`; `None` forces a look); remove the desk's file when there
/// is none to show.
async fn apply(state: &AppState, last: &mut Option<Seen>) {
    let dest = state.video.still_file.clone();
    let want = wanted(state).await;
    let now: Seen = match &want {
        Some(p) => match tokio::fs::metadata(p).await {
            Ok(m) => Some((
                p.clone(),
                m.len(),
                m.modified().unwrap_or(std::time::UNIX_EPOCH),
            )),
            Err(_) => None,
        },
        None => None,
    };
    if last.as_ref() == Some(&now) {
        return;
    }
    let done = match &now {
        Some((src, _, _)) => {
            let tmp = dest.with_extension("png.part");
            match tokio::fs::copy(src, &tmp).await {
                Ok(_) => tokio::fs::rename(&tmp, &dest).await,
                Err(e) => Err(e),
            }
        }
        None => match tokio::fs::remove_file(&dest).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    };
    match done {
        Ok(()) => *last = Some(now),
        Err(e) => tracing::warn!(error = %e, "the slate's still could not be brought up to date"),
    }
}

/// Keep the desk's still the show on air's, else the station's (ADR-0200). A station that had
/// one still before keeps it as the station's.
pub fn spawn_slate_still(state: Arc<AppState>) {
    tokio::spawn(async move {
        let (desk_file, station) = (state.video.still_file.clone(), station_still(&state.video));
        if !tokio::fs::try_exists(&station).await.unwrap_or(true)
            && tokio::fs::try_exists(&desk_file).await.unwrap_or(false)
        {
            match tokio::fs::copy(&desk_file, &station).await {
                Ok(_) => tracing::info!("the slate's still is kept as the station's (ADR-0200)"),
                Err(e) => {
                    tracing::warn!(error = %e, "the slate's still could not be kept as the station's")
                }
            }
        }
        adopt_demo_still(&state.video).await;
        let mut last = None;
        loop {
            apply(&state, &mut last).await;
            tokio::time::sleep(SLATE_STILL_EVERY).await;
        }
    });
}

/// Beside the stills: the demo still has been given to this station once, so a station that
/// takes it away is not given it again at its next start.
const DEMO_GIVEN: &str = ".demo-still-given";

/// The demo station's still (ADR-0200, `video.demo_still`): made the station's when the station
/// has none and has never been given it. It is our own PNG at the slate's size (the image's
/// `assets/stills-demo`), so it is copied as it is; a file that is no PNG is skipped.
async fn adopt_demo_still(cfg: &VideoConfig) {
    let Some(demo) = cfg.demo_still.clone().filter(|p| !p.as_os_str().is_empty()) else {
        return;
    };
    let (station, given) = (station_still(cfg), stills_dir(cfg).join(DEMO_GIVEN));
    if tokio::fs::try_exists(&station).await.unwrap_or(true)
        || tokio::fs::try_exists(&given).await.unwrap_or(true)
    {
        return;
    }
    if info_at(demo.clone()).await.is_none() {
        tracing::warn!(path = %demo.display(), "the demo still is not a PNG; the station has none");
        return;
    }
    let copied = async {
        tokio::fs::create_dir_all(stills_dir(cfg)).await?;
        let tmp = station.with_extension("png.part");
        tokio::fs::copy(&demo, &tmp).await?;
        tokio::fs::rename(&tmp, &station).await?;
        tokio::fs::write(&given, demo.display().to_string()).await
    };
    match copied.await {
        Ok(()) => {
            tracing::info!(path = %demo.display(), "the demo still is the station's (ADR-0200)")
        }
        Err(e) => tracing::warn!(error = %e, "the demo still could not be made the station's"),
    }
}

/// A show by its slug, or the status and words to answer with.
async fn find_show(
    state: &AppState,
    slug: &str,
) -> Result<weavesignal_model::shows::Show, (StatusCode, String)> {
    let Some(pool) = &state.db else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "the station has no database".into(),
        ));
    };
    match weavesignal_model::shows::get_show(pool, state.station.id, slug).await {
        Ok(Some(show)) => Ok(show),
        Ok(None) => Err((StatusCode::NOT_FOUND, format!("no show `{slug}`"))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the database: {e}"),
        )),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/still",
    operation_id = "shows_still_info",
    tag = "video",
    summary = "A show's slate picture",
    description = "Whether the show has a picture of its own for the slate (ADR-0200), its size and when it was set. The slate shows it while the show is on air by the schedule, else the station's.",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "The still as it is now.", body = StillInfo),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn show_still_info(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let show = match find_show(&state, &slug).await {
        Ok(s) => s,
        Err((code, m)) => return problem(code, m),
    };
    Json(
        info_at(show_still(&state.video, show.id))
            .await
            .unwrap_or(StillInfo {
                present: false,
                width: 0,
                height: 0,
                bytes: 0,
                set_at_ms: 0,
            }),
    )
    .into_response()
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/still",
    operation_id = "shows_put_still",
    tag = "video",
    summary = "Set a show's slate picture",
    description = "A PNG or JPEG as the body, as for the station's (ADR-0200). The slate shows it within a few seconds whenever the show is on air by the schedule.",
    params(("show" = String, Path, description = "The show's slug")),
    request_body(content = crate::openapi::File, content_type = "image/*", description = "A PNG or JPEG."),
    responses(
        (status = 201, description = "The still.", body = StillInfo),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 413, description = "The picture is larger than `video.still_max_upload_mb`.", body = crate::openapi::Problem),
        (status = 500, description = "The media worker or the disk failed.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_show_still(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
    body: Bytes,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let show = match find_show(&state, &slug).await {
        Ok(s) => s,
        Err((code, m)) => return problem(code, m),
    };
    let target = show_still(&state.video, show.id);
    match set_still_at(&state, body.to_vec(), target).await {
        Ok(info) => (StatusCode::CREATED, Json(info)).into_response(),
        Err((code, m)) => problem(code, m),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/shows/{show}/still",
    operation_id = "shows_delete_still",
    tag = "video",
    summary = "Take a show's slate picture away",
    description = "The slate shows the station's picture while the show is on air. 204 also when there was none.",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 204, description = "Gone."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 500, description = "The file could not be removed.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_show_still(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let show = match find_show(&state, &slug).await {
        Ok(s) => s,
        Err((code, m)) => return problem(code, m),
    };
    match remove(&state, show_still(&state.video, show.id)).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot take the still away: {e}"),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/still/image",
    operation_id = "shows_still_image",
    tag = "video",
    summary = "A show's slate picture",
    description = "The show's PNG, never cached.",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "The picture.", body = crate::openapi::File, content_type = "image/png"),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn show_still_image(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
) -> Response {
    if let Some(r) = refusal(&state, &headers) {
        return r;
    }
    let show = match find_show(&state, &slug).await {
        Ok(s) => s,
        Err((code, m)) => return problem(code, m),
    };
    let path = show_still(&state.video, show.id);
    match tokio::fs::read(&path).await {
        Ok(bytes) if info_at(path).await.is_some() => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        _ => problem(StatusCode::NOT_FOUND, "the show has no still"),
    }
}

// ---- the description (ADR-0195) ----

#[derive(Serialize, utoipa::ToSchema)]
struct ThumbsBody {
    /// Each camera's identity and its picture: a JPEG in base64.
    thumbs: std::collections::BTreeMap<String, String>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    still_info,
    put_still,
    delete_still,
    still_image,
    thumbs,
    show_still_info,
    put_show_still,
    delete_show_still,
    show_still_image
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(air_at: Option<i64>, minutes: Option<u16>) -> weavesignal_model::shows::Episode {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "series_id": Uuid::new_v4(), "number": 1, "title": "",
            "description": "", "kind": "live", "air_at": air_at, "minutes": minutes,
            "hosts": null, "guests": [], "tags": null, "aired_at": null, "frozen": null,
            "files": null, "cards": null
        }))
        .unwrap()
    }

    fn show(slot_minutes: Option<u16>) -> weavesignal_model::shows::Show {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "slug": "kettle-talk", "name": "Kettle Talk", "description": "",
            "colour_slot": 0, "layout": "poster",
            "slot": slot_minutes.map(|m| serde_json::json!({"weekday": 1, "start": 480, "minutes": m})),
            "hosts": [], "tags": [], "explicit": false, "feed_guid": null, "cards": null
        }))
        .unwrap()
    }

    fn series() -> weavesignal_model::shows::Series {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "show_id": Uuid::new_v4(), "number": 1, "title": "",
            "slot": null, "starts_on": null, "cards": null
        }))
        .unwrap()
    }

    #[test]
    fn the_show_on_air_is_the_one_whose_episode_holds_now() {
        let (morning, night) = (show(Some(120)), show(None));
        let eps = vec![
            (morning.clone(), series(), episode(Some(1_000), None)),
            (night.clone(), series(), episode(Some(10_000), Some(30))),
        ];
        // The morning show's slot gives it two hours; the night show's episode, 30 minutes.
        assert_eq!(show_on_air(&eps, 999), None);
        assert_eq!(show_on_air(&eps, 1_000), Some(morning.id));
        assert_eq!(show_on_air(&eps, 1_000 + 7_199), Some(morning.id));
        assert_eq!(show_on_air(&eps, 1_000 + 7_200), None);
        assert_eq!(show_on_air(&eps, 10_000 + 1_799), Some(night.id));
        assert_eq!(show_on_air(&eps, 10_000 + 1_800), None);
        // With no length anywhere, an hour.
        let bare = vec![(night.clone(), series(), episode(Some(0), None))];
        assert_eq!(show_on_air(&bare, 3_599), Some(night.id));
        assert_eq!(show_on_air(&bare, 3_600), None);
        // An episode with no air time is never on air.
        assert_eq!(
            show_on_air(&[(night, series(), episode(None, None))], 0),
            None
        );
    }

    #[tokio::test]
    async fn the_demo_still_is_given_once_and_only_to_a_station_without_one() {
        let dir = std::env::temp_dir().join(format!("weavesignal-demo-still-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let demo = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/stills-demo/kettle-fm.png"
        );
        let cfg = VideoConfig {
            still_file: dir.join("stills").join("slate.png"),
            demo_still: Some(demo.into()),
            ..VideoConfig::default()
        };
        let station = station_still(&cfg);

        adopt_demo_still(&cfg).await;
        assert_eq!(
            std::fs::read(&station).unwrap(),
            std::fs::read(demo).unwrap()
        );
        assert_eq!(
            info_blocking(&station).map(|i| (i.width, i.height)),
            Some((1280, 720))
        );

        // Taken away by the station: not given again at the next start.
        std::fs::remove_file(&station).unwrap();
        adopt_demo_still(&cfg).await;
        assert!(!station.exists());

        // A station with its own still, or no demo set, is left alone.
        let fresh = VideoConfig {
            still_file: dir.join("own").join("slate.png"),
            ..cfg.clone()
        };
        std::fs::create_dir_all(dir.join("own")).unwrap();
        std::fs::write(station_still(&fresh), b"ours").unwrap();
        adopt_demo_still(&fresh).await;
        assert_eq!(std::fs::read(station_still(&fresh)).unwrap(), b"ours");
        let none = VideoConfig {
            still_file: dir.join("none").join("slate.png"),
            demo_still: Some("".into()),
            ..cfg
        };
        adopt_demo_still(&none).await;
        assert!(!station_still(&none).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_that_is_no_png_is_no_still() {
        let dir = std::env::temp_dir().join(format!("weavesignal-still-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slate.png");
        assert_eq!(info_blocking(&path), None);
        std::fs::write(&path, b"not a picture at all, not at all").unwrap();
        assert_eq!(info_blocking(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
