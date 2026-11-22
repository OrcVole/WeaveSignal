// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line's routes (Phase 17f): its state, the station's recording of the words, number
//! bans and the takedown search. Every one is a person's, never an API token's
//! (`scopes::NEVER`): a ban and a takedown act on callers' personal data. No route takes a
//! number in its path or echoes one in an answer; a typed number lives only in the request that
//! carries it.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::OpenApi;

use super::config::CONSENT_WORDS_HELP;
use super::limits::{BanError, PhoneBan};
use super::line::{Closed, Line, PhoneSettings};
use super::words::PhoneVoice;
use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::staff::Need;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/phone", get(status))
        .route(
            "/api/v1/phone/words",
            put(put_words)
                .delete(delete_words)
                .layer(DefaultBodyLimit::max(WORDS_MAX_BYTES)),
        )
        .route("/api/v1/phone/consent", put(put_consent))
        .route(
            "/api/v1/phone/consent/default/{file}",
            get(default_voice_file),
        )
        .route("/api/v1/phone/bans", get(list_bans).post(make_ban))
        .route("/api/v1/phone/bans/{ban_key}", delete(lift_ban))
        .route("/api/v1/phone/takedown", post(takedown))
}

#[derive(OpenApi)]
#[openapi(
    paths(
        status,
        put_words,
        delete_words,
        put_consent,
        default_voice_file,
        list_bans,
        make_ban,
        lift_ban,
        takedown
    ),
    components(schemas(
        PhoneStatus,
        PhoneConsentChange,
        PhoneWordsView,
        RecordingView,
        Closed,
        super::words::WordsProblem,
        PhoneBan,
        BanRequest,
        TakedownRequest,
        TakedownAnswer
    ))
)]
pub struct Api;

/// What the line's words are now.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PhoneWordsView {
    /// `off`, `default` (the recording that ships with WeaveSignal) or `station` (its own).
    pub kind: String,
    pub version: String,
    /// What the caller hears (with the words off, the sentence the records keep instead).
    pub text: String,
}

/// The phone line's state.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PhoneStatus {
    /// Whether the line takes calls now.
    pub open: bool,
    /// Why not, when it does not.
    pub closed: Option<Closed>,
    /// Calls at once (`phone.lines`); 0 with the line off.
    pub lines: u16,
    /// Calls hearing the words now.
    pub at_gate: usize,
    /// Phone callers in the queue.
    pub in_queue: usize,
    /// The words; absent with the line off.
    pub words: Option<PhoneWordsView>,
    /// What the station's own recording should say today (its takedown contact is the
    /// station's, not the show's, so it does not go out of date at each change of show);
    /// absent with the line off.
    pub words_to_record: Option<String>,
    /// The station's own recording, if it has one.
    pub recording: Option<RecordingView>,
    /// The description of the consent words setting (`phone.consent_words`, `PHONE_CONSENT_WORDS`
    /// on Compose and the Quadlets), for the studio to show beside its switch: what the words
    /// are, when to leave them on (a licensed broadcaster, calls recorded or kept) and when an
    /// internet-only station may turn them off. Plain words, not legal advice.
    #[schema(
        example = "Before a phone caller joins the queue, play them a few words: who they have reached, that the call may go on air and be recorded, and to press a key to carry on. …"
    )]
    pub consent_words_help: String,
    /// Whether the consent words play: the setting an admin changed in the studio
    /// (`PUT /api/v1/phone/consent`), else `phone.consent_words`. With the line off, false.
    pub consent_words: bool,
    /// Which shipped recording plays until the station records its own: the setting an admin
    /// changed in the studio (`PUT /api/v1/phone/consent`), else `phone.default_voice`
    /// (`PHONE_DEFAULT_VOICE` on every install route): `alba` (the default), `edinburgh` or `own`.
    /// The station's own recording, once made, plays instead of either. With the line off, the
    /// default.
    pub default_voice: super::words::PhoneVoice,
    /// The description of `default_voice`, for the studio beside its choice.
    pub default_voice_help: String,
    /// Where the line's inbound trunk and dispatch rule stand in LiveKit: a call reaches the
    /// line only once they are `made` (the station makes them at start and checks them every
    /// 30 s). Absent with the line off.
    pub trunk: Option<super::sync::TrunkStatus>,
}

/// The station's own recording, as `GET /api/v1/phone` shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RecordingView {
    pub duration_ms: i32,
    pub recorded_by: String,
    pub recorded_at_ms: i64,
    /// Made from other words than today's: the line is closed until it is recorded again.
    pub out_of_date: bool,
}

/// The largest recording taken (8 MB: two minutes of FLAC and more).
const WORDS_MAX_BYTES: usize = 8 * 1024 * 1024;
/// A recording's length, from the media worker's report: 1.5 s to 2 minutes.
const WORDS_MIN_MS: u64 = 1_500;
const WORDS_MAX_MS: u64 = 120_000;

fn line(state: &AppState) -> Option<&Arc<Line>> {
    state.hub.phone.get()
}

/// The browser's consent words with the station's own takedown contact (not the show's), as
/// the station's recording of the phone words must say them.
async fn station_consent_words(state: &AppState) -> String {
    let setting = match &state.db {
        Some(pool) => weavesignal_model::retention::load_setting(pool, state.station.id)
            .await
            .unwrap_or_default(),
        None => weavesignal_model::retention::Setting::default(),
    };
    let kept = crate::join::kept_phrase(setting.periods().programme_recordings);
    let station = state.station_name();
    let contact = crate::join::contact_phrase("", &station, &state.consent_contact());
    crate::join::consent_words(&state.join.consent_text, &station, &kept, &contact)
}

/// Read today's words and the station's recording into the line's context: at start, every
/// half minute, and after a recording is made or removed. Returns the recording's row.
pub async fn refresh(state: &AppState) -> Option<weavesignal_model::phone::PhoneWordsRecord> {
    let line = line(state)?;
    let consent_today = station_consent_words(state).await;
    let row = match &state.db {
        Some(pool) => match weavesignal_model::phone::words(pool, state.station.id).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read the station's phone words");
                return None;
            }
        },
        None => None,
    };
    let settings = match &state.db {
        Some(pool) => match weavesignal_model::phone::settings(pool, state.station.id).await {
            Ok(r) => Some(settings_of(&r)),
            Err(e) => {
                tracing::warn!(error = %e, "cannot read the phone line's settings");
                None
            }
        },
        // No database: what was changed stays in memory until a restart.
        None => None,
    };
    let mut ctx = line.context();
    ctx.consent_today = consent_today;
    if let Some(settings) = settings {
        ctx.settings = settings;
    }
    ctx.station_recording = row.as_ref().map(|r| super::words::StationRecording {
        text: r.words.clone(),
        file: r.file.clone(),
        gain_mb: (r.gain_db * 100.0).round() as i32,
    });
    line.set_context(ctx);
    row
}

/// Refresh the line's context every 30 seconds, while the line is on.
pub fn spawn_refresher(state: Arc<AppState>) {
    if line(&state).is_none() {
        return;
    }
    tokio::spawn(async move {
        loop {
            refresh(&state).await;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

#[utoipa::path(
    get,
    path = "/api/v1/phone",
    operation_id = "phone_status",
    tag = "phone",
    summary = "The phone line's state",
    description = "Whether the line takes calls and why not, the calls at the gate and in the queue, the words a caller hears first, and whether the line's trunk and dispatch rule are made in LiveKit (`trunk`). For staff.",
    responses(
        (status = 200, description = "The line.", body = PhoneStatus),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    phone_status(&state).await.into_response()
}

async fn phone_status(state: &AppState) -> Json<PhoneStatus> {
    let in_queue = state.hub.read_studio(|s| s.phone_in_queue());
    let Some(line) = line(state) else {
        return Json(PhoneStatus {
            open: false,
            closed: Some(Closed::Off),
            lines: 0,
            at_gate: 0,
            in_queue,
            words: None,
            words_to_record: None,
            recording: None,
            consent_words_help: CONSENT_WORDS_HELP.to_owned(),
            consent_words: false,
            default_voice: super::words::PhoneVoice::default(),
            default_voice_help: super::words::DEFAULT_VOICE_HELP.to_owned(),
            trunk: None,
        });
    };
    let row = refresh(state).await;
    let ctx = line.context();
    let (closed, words) = state.hub.read_studio(|s| {
        let m = Line::moment(&ctx, s.phone_bans(), in_queue);
        (line.closed(&m), line.words(&m))
    });
    let kind = match words {
        super::words::PhoneWords::Off => "off",
        super::words::PhoneWords::Default(_) => "default",
        super::words::PhoneWords::Station(_) => "station",
    };
    let to_record = super::words::station_words_today(&ctx.consent_today);
    let recording = row.map(|r| RecordingView {
        duration_ms: r.duration_ms,
        recorded_by: r.recorded_by,
        recorded_at_ms: r.recorded_at_ms,
        out_of_date: crate::join::consent_version(&r.words)
            != crate::join::consent_version(&to_record),
    });
    Json(PhoneStatus {
        open: closed.is_none() && ctx.call_in_open,
        closed,
        lines: line.config.lines,
        at_gate: line.at_gate(),
        in_queue,
        words: Some(PhoneWordsView {
            kind: kind.to_owned(),
            version: words.version(),
            text: words.text().to_owned(),
        }),
        words_to_record: Some(to_record),
        recording,
        consent_words_help: CONSENT_WORDS_HELP.to_owned(),
        consent_words: line.consent_words(ctx.settings),
        default_voice: line.default_voice(ctx.settings),
        default_voice_help: super::words::DEFAULT_VOICE_HELP.to_owned(),
        trunk: Some(line.trunk.get()),
    })
}

#[utoipa::path(
    put,
    path = "/api/v1/phone/words",
    operation_id = "phone_words_record",
    tag = "phone",
    summary = "Keep the station's recording of the phone words",
    description = "The body is the recording, in any format the media worker reads (at most 8 MB, 1.5 s to 2 minutes). It must say `words_to_record` from `GET /api/v1/phone`; the station keeps those words beside it, and the line closes when they change until the words are recorded again. It replaces the recording before, and takes the place of the shipped default. An admin's.",
    request_body(content = crate::openapi::File, content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "The line with its new words.", body = PhoneStatus),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_words(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(line) = line(&state) else {
        return problem(StatusCode::CONFLICT, "The phone line is off.");
    };
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The station has no database.",
        );
    };
    let (flac, report) = match crate::media::normalise(&state.hub.carts.config, body.to_vec()).await
    {
        Ok(r) => r,
        Err(crate::media::WorkerError::Refused(e)) => {
            return problem(StatusCode::UNPROCESSABLE_ENTITY, e);
        }
        Err(e) => {
            tracing::error!(error = %e, "the media worker failed on the phone words");
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not read the recording.",
            );
        }
    };
    if !(WORDS_MIN_MS..=WORDS_MAX_MS).contains(&report.duration_ms) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The recording should be between a second and a half and two minutes long.",
        );
    }
    let file = match crate::media::store(&line.config.words_dir, &flac).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(error = %e, "cannot keep the phone words");
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not keep the recording.",
            );
        }
    };
    let words = super::words::station_words_today(&station_consent_words(&state).await);
    let rec = weavesignal_model::phone::PhoneWordsRecord {
        words_version: crate::join::consent_version(&words),
        words,
        file,
        duration_ms: i32::try_from(report.duration_ms).unwrap_or(i32::MAX),
        gain_db: report.play_gain_db as f32,
        recorded_by: actor.label(),
        recorded_at_ms: now_ms() as i64,
    };
    match weavesignal_model::phone::put_words(pool, state.station.id, &rec).await {
        Ok(old) => {
            if let Some(old) = old {
                // A call at the gate may still be playing it; the desk has it open, so removing
                // the name does not cut it short.
                let _ = tokio::fs::remove_file(line.config.words_dir.join(old)).await;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "cannot keep the phone words");
            let _ = tokio::fs::remove_file(line.config.words_dir.join(&rec.file)).await;
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not keep the recording.",
            );
        }
    }
    tracing::info!(by = %actor.label(), version = %rec.words_version, "the phone words recorded");
    phone_status(&state).await.into_response()
}

#[utoipa::path(
    delete,
    path = "/api/v1/phone/words",
    operation_id = "phone_words_remove",
    tag = "phone",
    summary = "Remove the station's recording of the phone words",
    description = "The shipped default plays again. An admin's.",
    responses(
        (status = 200, description = "The line with the default words.", body = PhoneStatus),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_words(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let Some(line) = line(&state) else {
        return problem(StatusCode::CONFLICT, "The phone line is off.");
    };
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The station has no database.",
        );
    };
    match weavesignal_model::phone::delete_words(pool, state.station.id).await {
        Ok(Some(old)) => {
            let _ = tokio::fs::remove_file(line.config.words_dir.join(old)).await;
        }
        Ok(None) => {}
        Err(e) => {
            tracing::error!(error = %e, "cannot remove the phone words");
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not remove the recording.",
            );
        }
    }
    phone_status(&state).await.into_response()
}

/// The settings as the database keeps them, read into the line's; a voice the database should
/// never hold is left to the installation's setting.
fn settings_of(r: &weavesignal_model::phone::PhoneSettingsRecord) -> PhoneSettings {
    PhoneSettings {
        consent_words: r.consent_words,
        default_voice: r
            .default_voice
            .as_deref()
            .and_then(|v| serde_json::from_value(serde_json::Value::String(v.to_owned())).ok()),
    }
}

fn voice_name(v: PhoneVoice) -> &'static str {
    match v {
        PhoneVoice::Alba => "alba",
        PhoneVoice::Edinburgh => "edinburgh",
        PhoneVoice::Own => "own",
    }
}

/// A change to the phone line's consent settings; a field left out stays as it is.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PhoneConsentChange {
    /// Play the consent words before a caller joins the queue (`phone.consent_words`).
    #[serde(default)]
    pub consent_words: Option<bool>,
    /// Which shipped recording plays until the station records its own
    /// (`phone.default_voice`): `alba`, `edinburgh` or `own`.
    #[serde(default)]
    pub default_voice: Option<PhoneVoice>,
}

#[utoipa::path(
    put,
    path = "/api/v1/phone/consent",
    operation_id = "phone_consent_set",
    tag = "phone",
    summary = "Change whether the consent words play, and which shipped voice",
    description = "Change `consent_words` (play the words before a caller joins the queue) or `default_voice` (`alba`, `edinburgh` or `own`: which shipped recording plays until the station records its own), or both; a field left out stays as it is. Each wins over the installation's setting (`PHONE_CONSENT_WORDS`, `PHONE_DEFAULT_VOICE`) from the next call, and is kept in the database. The station's own recording, once made, plays instead of either voice; `own` with no recording closes the line while the words are on. An admin's.",
    request_body = PhoneConsentChange,
    responses(
        (status = 200, description = "The line with its new settings.", body = PhoneStatus),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_consent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PhoneConsentChange>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(line) = line(&state) else {
        return problem(StatusCode::CONFLICT, "The phone line is off.");
    };
    if req.consent_words.is_none() && req.default_voice.is_none() {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Say what to change: `consent_words`, `default_voice`, or both.",
        );
    }
    // The settings as kept; the line's context is read after the database answers, so a
    // refresh meanwhile is not written over.
    let settings = if let Some(pool) = &state.db {
        let change = weavesignal_model::phone::PhoneSettingsRecord {
            consent_words: req.consent_words,
            default_voice: req.default_voice.map(|v| voice_name(v).to_owned()),
        };
        match weavesignal_model::phone::put_settings(
            pool,
            state.station.id,
            &change,
            &actor.label(),
        )
        .await
        {
            Ok(kept) => settings_of(&kept),
            Err(e) => {
                tracing::error!(error = %e, "cannot keep the phone line's settings");
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The station could not save the setting.",
                );
            }
        }
    } else {
        let mut s = line.context().settings;
        if let Some(w) = req.consent_words {
            s.consent_words = Some(w);
        }
        if let Some(v) = req.default_voice {
            s.default_voice = Some(v);
        }
        s
    };
    let mut ctx = line.context();
    ctx.settings = settings;
    line.set_context(ctx.clone());
    tracing::info!(
        by = %actor.label(),
        consent_words = line.consent_words(ctx.settings),
        default_voice = voice_name(line.default_voice(ctx.settings)),
        "the phone line's consent settings changed"
    );
    phone_status(&state).await.into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/phone/consent/default/{file}",
    operation_id = "phone_consent_default_voice",
    tag = "phone",
    summary = "Hear a shipped voice",
    description = "One of the two recordings of the phone words that ship with WeaveSignal, for the studio's Play: `alba.opus` (a Scottish woman, the default) or `edinburgh.opus` (a man from Edinburgh), Ogg Opus, CC BY 4.0 (NOTICES.md). For staff.",
    params(("file" = String, Path, description = "`alba.opus` or `edinburgh.opus`.")),
    responses(
        (status = 200, description = "The recording.", content_type = "audio/ogg", body = crate::openapi::File),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn default_voice_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(file): Path<String>,
) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    let Some(line) = line(&state) else {
        return problem(StatusCode::CONFLICT, "The phone line is off.");
    };
    let shipped = match file.as_str() {
        "alba.opus" => super::words::ALBA_FILE,
        "edinburgh.opus" => super::words::EDINBURGH_FILE,
        _ => return problem(StatusCode::NOT_FOUND, "There is no such voice."),
    };
    // The copy the desk plays, else the image's own.
    for dir in [&line.config.words_dir, &line.config.defaults_dir] {
        if let Ok(bytes) = tokio::fs::read(dir.join(shipped)).await {
            return (
                [
                    (axum::http::header::CONTENT_TYPE, "audio/ogg"),
                    (axum::http::header::CACHE_CONTROL, "private, max-age=3600"),
                ],
                bytes,
            )
                .into_response();
        }
    }
    problem(
        StatusCode::NOT_FOUND,
        "This station's image has no copy of that voice.",
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/phone/bans",
    operation_id = "phone_bans",
    tag = "phone",
    summary = "Number bans",
    description = "Every number ban that has not lapsed: its ban key, the last digits as the screener saw them, the day and the reason. Never a number. For staff.",
    responses(
        (status = 200, description = "The bans.", body = [PhoneBan]),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_bans(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    Json(state.hub.read_studio(|s| s.phone_bans().list())).into_response()
}

/// A ban: of a phone caller in the queue (the screener's), or of a number typed (an admin's).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BanRequest {
    /// A phone caller's person key (`phone-…`) in the queue now.
    #[serde(default)]
    pub identity: Option<String>,
    /// A number as a caller or the provider gave it; an admin's only. Never kept.
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub reason: String,
    /// Days until it lapses; absent, it does not.
    #[serde(default)]
    pub days: Option<u32>,
}

#[utoipa::path(
    post,
    path = "/api/v1/phone/bans",
    operation_id = "phone_ban",
    tag = "phone",
    summary = "Ban a number",
    description = "Ban a phone caller in the queue by `identity` (staff), or a number by `number` (an admin; every form it may arrive in is banned). The station keeps the ban key and the last digits, never the number. A withheld caller cannot be banned by number (409: \"Cannot ban a withheld number\"). Caller ID can be spoofed: a ban is advisory.",
    request_body = BanRequest,
    responses(
        (status = 200, description = "The bans made.", body = [PhoneBan]),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn make_ban(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BanRequest>,
) -> Response {
    let need = if req.number.is_some() {
        Need::Admin
    } else {
        Need::OnAir
    };
    let actor = match state.check(&headers, need) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let reason = match crate::join::clean_line(&req.reason, 280, "The reason") {
        Ok(r) => r,
        Err(e) => return problem(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    let now = now_ms();
    let date = jiff::Timestamp::from_millisecond(now as i64)
        .map(|t| t.strftime("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let expires_ms = req
        .days
        .map(|d| now.saturating_add(u64::from(d) * 86_400_000));
    // (ban key, last digits, withheld)
    let targets: Vec<(String, String, bool)> = match (&req.identity, &req.number) {
        (Some(id), None) => match state.hub.read_studio(|s| s.phone_caller(id).cloned()) {
            None => {
                return problem(
                    StatusCode::NOT_FOUND,
                    "There is no phone caller by that identity.",
                );
            }
            Some(c) => vec![(c.ban_key.unwrap_or_default(), c.tail, c.withheld)],
        },
        (None, Some(number)) => {
            let Some(line) = line(&state) else {
                return problem(StatusCode::CONFLICT, "The phone line is off.");
            };
            let digits: String = number.chars().filter(char::is_ascii_digit).collect();
            if digits.len() < 5 {
                return problem(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "That is not a phone number.",
                );
            }
            let tail = digits[digits.len() - 4..].to_owned();
            line.ban_keys_of_typed(number)
                .into_iter()
                .map(|k| (k, tail.clone(), false))
                .collect()
        }
        _ => {
            return problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Name a phone caller (`identity`) or a number (`number`), one of them.",
            );
        }
    };
    let mut made = Vec::new();
    for (ban_key, tail, withheld) in targets {
        let ban = PhoneBan {
            ban_key,
            tail,
            date: date.clone(),
            reason: reason.clone(),
            expires_ms,
        };
        let r = state
            .hub
            .with_studio(|s, _| s.phone_bans_mut().ban(ban.clone(), withheld));
        match r {
            Ok(()) => {}
            Err(BanError::Withheld) => {
                return problem(StatusCode::CONFLICT, BanError::Withheld.to_string());
            }
            Err(BanError::NotABanKey) => {
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The phone line has no key to make ban keys with.",
                );
            }
        }
        if let Some(pool) = &state.db {
            let rec = weavesignal_model::phone::PhoneBanRecord {
                ban_key: ban.ban_key.clone(),
                number_tail: ban.tail.clone(),
                reason: ban.reason.clone(),
                made_by: actor.label(),
                made_at_ms: now as i64,
                expires_at_ms: ban.expires_ms.map(|e| e as i64),
            };
            if let Err(e) = weavesignal_model::phone::put_ban(pool, state.station.id, &rec).await {
                tracing::error!(error = %e, "cannot keep a number ban");
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The station could not save the ban.",
                );
            }
        }
        made.push(ban);
    }
    tracing::info!(by = %actor.label(), bans = made.len(), "number banned");
    Json(made).into_response()
}

#[utoipa::path(
    delete,
    path = "/api/v1/phone/bans/{ban_key}",
    operation_id = "phone_unban",
    tag = "phone",
    summary = "Lift a number ban",
    description = "Lift the ban with this ban key. An admin's.",
    params(("ban_key" = String, Path, description = "`pb_` and 32 hex digits.")),
    responses(
        (status = 204, description = "Lifted."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn lift_ban(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(ban_key): Path<String>,
) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let had = state
        .hub
        .with_studio(|s, _| s.phone_bans_mut().lift(&ban_key));
    let mut stored = false;
    if let Some(pool) = &state.db {
        match weavesignal_model::phone::lift_ban(pool, state.station.id, &ban_key).await {
            Ok(b) => stored = b,
            Err(e) => {
                tracing::error!(error = %e, "cannot lift a number ban");
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The station could not lift the ban.",
                );
            }
        }
    }
    if had || stored {
        StatusCode::NO_CONTENT.into_response()
    } else {
        problem(StatusCode::NOT_FOUND, "There is no ban with that key.")
    }
}

/// A takedown's number.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TakedownRequest {
    /// The number as the caller gives it. Never kept or logged.
    pub number: String,
}

/// The calls a number may have made.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TakedownAnswer {
    /// Their consent records, newest first: erase each with `POST /api/v1/retention/consents/{id}/erase`.
    pub consents: Vec<i64>,
}

#[utoipa::path(
    post,
    path = "/api/v1/phone/takedown",
    operation_id = "phone_takedown",
    tag = "phone",
    summary = "Find a number's calls",
    description = "The consent records of the calls a number may have made, found by its ban keys (every form the number may have arrived in). The number is used for this request only. A withheld call cannot be found by number: find it by its date, time and show. An admin's.",
    request_body = TakedownRequest,
    responses(
        (status = 200, description = "The calls.", body = TakedownAnswer),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn takedown(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TakedownRequest>,
) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let Some(line) = line(&state) else {
        return problem(StatusCode::CONFLICT, "The phone line is off.");
    };
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The station has no database.",
        );
    };
    let keys = line.ban_keys_of_typed(&req.number);
    match weavesignal_model::phone::consents_by_ban_keys(pool, state.station.id, &keys).await {
        Ok(consents) => Json(TakedownAnswer { consents }).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "cannot search the phone calls");
            problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "The station could not search its calls.",
            )
        }
    }
}
