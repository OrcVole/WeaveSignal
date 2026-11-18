// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Hosts in their own studios (ADR-0280, ADR-0284; design section 10): show keys, the sources
//! the ingest holds, and the routes the ingest asks before it reads any media.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/shows/{show}/keys` | admin, producer, the show's presenter | the show's keys (never a secret), where it comes from, each key's last refusal |
//! | `POST /api/v1/shows/{show}/keys` | the same | a key: the reply holds both secrets and the details, **once** |
//! | `POST /api/v1/shows/{show}/keys/{key}/renew` | the same | a new key for the same person; the old ends `now` or `on_disconnect` |
//! | `DELETE /api/v1/shows/{show}/keys/{key}` | the same | the key ends; a connection with it is closed |
//! | `GET /api/v1/sources` | staff; `sources:read` | every source connected or due, with its state |
//! | `POST`, `DELETE /api/v1/sources/{key}/disconnect` | staff at the desk; the first's undoing, as for keys | close it and turn the key away for the rest of the slot; let it back in |
//! | `GET`, `PUT /api/v1/settings/sources` | admin | the station's rules for sources |
//! | `/api/v1/internal/sources/…` | the ingest only | `config`, `admit`, `report`, `events` |
//!
//! **Secrets.** A key's secret is derived under the token key from its id and a salt
//! ([`ApiKey::show_key_secret`]); the monitor link's is kept as a keyed hash. Both are shown once,
//! in the reply that makes the key, and no route shows either again. The SRT listener needs the
//! key's secret in plain to answer the handshake, so `admit` gives it to the ingest for SRT, over
//! the internal route only.
//!
//! **The internal routes** need all three of: a loopback peer, no `X-Forwarded-For`, `Forwarded`
//! or `X-Real-IP` header (every proxy in the bundles adds one, so a request from outside never
//! passes), and the bearer in `WEAVESIGNAL_INGEST_TOKEN`. Without that variable they refuse: behind
//! a proxy every peer is loopback, so the address alone proves nothing.
//!
//! **Two phases for SRT.** A key id is public. `admit` for SRT decides without a proof and hands
//! back the passphrase; nothing is claimed for good until the ingest reports the handshake done
//! (`events`, `admitted`). A failed handshake comes back as `refused`, `wrong_key`, and counts
//! against the address as a wrong Icecast password does.

// Every helper here returns the refusal itself as its error, as the handlers send it.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::shows::{self, Show};
use weavesignal_model::sources::{
    self as model, Admission, AfterDoor, Airing, Door, LogEntry, NewShowKey, Refusal, ShowKey,
    SourceSettings, WrongKeys,
};
use weavesignal_proto::source_wire;

use crate::apikey::ApiKey;
use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::staff::{Actor, Need};

/// The environment variable that holds the ingest's bearer for the internal routes.
pub const INGEST_TOKEN_ENV: &str = "WEAVESIGNAL_INGEST_TOKEN";
/// The shortest ingest token taken.
pub const INGEST_TOKEN_MIN: usize = 32;
/// Keys a show may have that have not ended.
pub const MAX_LIVE_KEYS: usize = 8;
/// A report older than this means the ingest has gone: nothing it said is shown.
const REPORT_STALE_MS: u128 = 5_000;
/// How far ahead the schedule is read for a key's next door.
const LOOK_AHEAD_S: i64 = 62 * 24 * 3600;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/shows/{show}/keys", get(list_keys).post(make_key))
        .route(
            "/api/v1/shows/{show}/keys/{key}",
            axum::routing::delete(end_key),
        )
        .route("/api/v1/shows/{show}/keys/{key}/renew", post(renew_key))
        .route("/api/v1/sources", get(list_sources))
        .route(
            "/api/v1/sources/{key}/disconnect",
            post(disconnect).delete(let_back),
        )
        .route(
            "/api/v1/settings/sources",
            get(get_settings).put(put_settings),
        )
        .route("/api/v1/internal/sources/config", get(internal_config))
        .route("/api/v1/internal/sources/admit", post(internal_admit))
        .route("/api/v1/internal/sources/report", post(internal_report))
        .route("/api/v1/internal/sources/events", post(internal_events))
}

// ---- what the control plane holds in memory ----

/// A connection as the ingest reports it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestConn {
    pub key_id: String,
    /// `standby` (on the desk) or `test` (never on the desk).
    pub mode: String,
    /// `icecast` or `srt`.
    pub way: String,
    /// What the host's software sends, as the ingest read it (`mp3 44.1 kHz stereo`); empty
    /// until the first sound is decoded.
    #[serde(default)]
    pub format: String,
    /// When the connection was taken, on the station's clock.
    pub since_ms: u64,
    /// What arrives, in kbit/s, over the last few seconds.
    #[serde(default)]
    pub kbps: u32,
    /// Its level, dBFS RMS over the last few seconds, as decoded; `None` while silent.
    #[serde(default)]
    pub level_db: Option<f32>,
    /// What the host's software says is playing (Icecast's metadata), cleaned and cut.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Default)]
struct Report {
    at: Option<Instant>,
    conns: Vec<IngestConn>,
}

/// The sources' state in memory: the ingest's bearer, its last report, wrong keys by address,
/// and the connections waiting to be closed.
pub struct Live {
    ingest_token: Mutex<Option<String>>,
    wrong: Mutex<WrongKeys>,
    report: Mutex<Report>,
    close: Mutex<BTreeSet<String>>,
    /// The two ports the installation named (`ports.source_icecast`, `ports.source_srt`), which
    /// win over the rules' own: the installation is what publishes them. 0 is a way turned off.
    fixed: Mutex<(Option<u16>, Option<u16>)>,
}

impl Live {
    /// With the ingest's bearer from [`INGEST_TOKEN_ENV`]; one too short is ignored, and said.
    pub fn from_env() -> Self {
        let token = std::env::var(INGEST_TOKEN_ENV)
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
        let token = match token {
            Some(t) if t.chars().count() < INGEST_TOKEN_MIN => {
                tracing::error!(
                    env = INGEST_TOKEN_ENV,
                    "is shorter than {INGEST_TOKEN_MIN} characters: the ingest is refused"
                );
                None
            }
            t => t,
        };
        Self::with_token(token)
    }

    pub fn with_token(token: Option<String>) -> Self {
        Live {
            ingest_token: Mutex::new(token),
            wrong: Mutex::new(WrongKeys::default()),
            report: Mutex::new(Report::default()),
            close: Mutex::new(BTreeSet::new()),
            fixed: Mutex::new((None, None)),
        }
    }

    /// The ports the installation named for Icecast and SRT, which the rules cannot change.
    pub fn set_fixed_ports(&self, icecast: Option<u16>, srt: Option<u16>) {
        *self.fixed.lock().expect("sources lock") = (icecast, srt);
    }

    /// The ports the installation named, if it named them.
    pub fn fixed_ports(&self) -> (Option<u16>, Option<u16>) {
        *self.fixed.lock().expect("sources lock")
    }

    /// The rules with the installation's ports in place of their own.
    pub fn in_force(&self, mut settings: SourceSettings) -> SourceSettings {
        let (ice, srt) = self.fixed_ports();
        if let Some(p) = ice {
            settings.port_icecast = i32::from(p);
        }
        if let Some(p) = srt {
            settings.port_srt = i32::from(p);
        }
        settings
    }

    /// Set the ingest's bearer (tests, and a station that sets it after starting).
    pub fn set_ingest_token(&self, token: Option<String>) {
        *self.ingest_token.lock().expect("sources lock") = token;
    }

    /// The ingest's connections, while its last report is fresh.
    pub fn connections(&self) -> Vec<IngestConn> {
        let r = self.report.lock().expect("sources lock");
        match r.at {
            Some(at) if at.elapsed().as_millis() < REPORT_STALE_MS => r.conns.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether the ingest has reported lately.
    pub fn ingest_alive(&self) -> bool {
        self.report
            .lock()
            .expect("sources lock")
            .at
            .is_some_and(|at| at.elapsed().as_millis() < REPORT_STALE_MS)
    }

    pub(crate) fn close_later(&self, key_id: &str) {
        self.close
            .lock()
            .expect("sources lock")
            .insert(key_id.to_owned());
    }
}

impl Default for Live {
    fn default() -> Self {
        Self::with_token(None)
    }
}

// ---- permissions and helpers ----

/// Who may make, renew and end a show's keys: the station key, an admin, a producer, or the
/// show's presenter. An API token never (ADR-0280).
pub fn may_manage(actor: &Actor, presenter_account: Option<Uuid>) -> bool {
    use weavesignal_model::staff::Role as Staff;
    match actor {
        Actor::Station => true,
        Actor::Staff(a) => {
            a.role.is_admin() || a.role == Staff::Producer || presenter_account == Some(a.id)
        }
        Actor::Token(_) => false,
    }
}

fn unavailable(what: &str) -> Response {
    problem(StatusCode::SERVICE_UNAVAILABLE, what.to_owned())
}

fn db_failed(e: &sqlx::Error) -> Response {
    tracing::error!(error = %e, "sources: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

fn field_problem(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": detail.into(), "field": field })),
    )
        .into_response()
}

fn pool(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    state.db.as_ref().ok_or_else(|| {
        unavailable(&format!(
            "show keys are kept in the database: set {}",
            crate::config::DATABASE_URL_ENV
        ))
    })
}

fn token_key(state: &AppState) -> Result<&ApiKey, Response> {
    state.staff.api.key().ok_or_else(|| {
        unavailable(&format!(
            "show keys need the token key: set {} (or the station key)",
            ApiKey::ENV
        ))
    })
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new()
        .fill(&mut b)
        .expect("the system's random source");
    b
}

/// The station's airings around now, in air order.
pub(crate) async fn airings(
    state: &AppState,
    pool: &sqlx::PgPool,
    now_s: i64,
) -> Result<Vec<Airing>, sqlx::Error> {
    let eps = shows::episodes_between(
        pool,
        state.station.id,
        now_s - 30 * 3600,
        now_s + LOOK_AHEAD_S,
    )
    .await?;
    Ok(eps
        .iter()
        .filter_map(|(show, series, ep)| {
            shows::air_window(show, series, ep).map(|(start, end)| Airing {
                show: show.id,
                start,
                end,
            })
        })
        .collect())
}

async fn show_by_slug(state: &AppState, slug: &str) -> Result<Show, Response> {
    let pool = pool(state)?;
    match shows::get_show(pool, state.station.id, slug).await {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(problem(
            StatusCode::NOT_FOUND,
            format!("show `{slug}` does not exist"),
        )),
        Err(e) => Err(db_failed(&e)),
    }
}

/// The actor and the show, if the actor may manage its keys.
async fn manager(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
) -> Result<(Actor, Show), Response> {
    let actor = state.check(headers, Need::Staff)?;
    let show = show_by_slug(state, slug).await?;
    if !may_manage(&actor, show.presenter_account) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Only an admin, a producer or the show's presenter makes or ends its keys. An API token never does.",
        ));
    }
    Ok((actor, show))
}

// ---- keys ----

/// How a host's software reaches the station (design section 3.1). The secret is in these
/// only in the reply that makes the key.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Details {
    /// The station's address as listeners reach it, when the settings give one; else the
    /// studio fills in the address it knows.
    pub host: Option<String>,
    pub icecast: IcecastDetails,
    pub srt: SrtDetails,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct IcecastDetails {
    pub port: u16,
    /// Any mount is taken; this one is suggested.
    pub mount: String,
    /// Any user name is taken; most software insists on one.
    pub user: String,
    /// The key id and the secret, joined by a hyphen.
    pub password: String,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct SrtDetails {
    pub port: u16,
    /// The key id.
    pub streamid: String,
    /// The secret: the media is encrypted with it, so it never crosses in clear.
    pub passphrase: String,
    /// AES-128.
    pub pbkeylen: u8,
    /// `srt://host:port?streamid=…&passphrase=…&pbkeylen=16`, when the host is known.
    pub url: Option<String>,
}

/// A key just made: its row, its two secrets and the details, shown this once.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MadeKey {
    pub key: ShowKey,
    /// The key's secret, 24 characters. Shown once.
    pub secret: String,
    /// The monitor link's secret, 24 characters, a second secret made and ended with the key.
    /// Shown once.
    pub monitor_secret: String,
    pub details: Details,
    /// The same as plain text, to paste into a message.
    pub as_text: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewKey {
    /// Who it is for: their name on the desk, 1 to 120 characters.
    pub label: String,
    /// The member of staff it is for, if one.
    #[serde(default)]
    pub staff_id: Option<Uuid>,
    /// `icecast` or `srt`: how they will send (a hint, not a lock).
    pub hint: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Renew {
    /// When the old key ends: `now` (a connection with it is closed) or `on_disconnect`.
    pub old: String,
}

/// A key with what the studio's row shows.
#[derive(Serialize, utoipa::ToSchema)]
pub struct KeyRow {
    #[serde(flatten)]
    pub key: ShowKey,
    /// The last refusal, if there has been one.
    pub last_refusal: Option<LogEntry>,
    /// The details without the secrets (the ports and the key id).
    pub details: Details,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct KeyList {
    pub show: String,
    /// `room` or `push`: where the show comes from.
    pub source: String,
    pub keys: Vec<KeyRow>,
}

fn details(
    state: &AppState,
    settings: &SourceSettings,
    key_id: &str,
    secret: Option<&str>,
) -> Details {
    let host = url::Url::parse(&state.join.stream_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    let shown = secret.unwrap_or("(shown once, when the key was made)");
    let (ice, srt) = (settings.port_icecast as u16, settings.port_srt as u16);
    Details {
        icecast: IcecastDetails {
            port: ice,
            mount: "/live".into(),
            user: "source".into(),
            password: format!("{key_id}-{shown}"),
        },
        srt: SrtDetails {
            port: srt,
            streamid: key_id.to_owned(),
            passphrase: shown.to_owned(),
            pbkeylen: 16,
            url: match (&host, secret) {
                (Some(h), Some(s)) if srt != 0 => Some(format!(
                    "srt://{h}:{srt}?streamid={key_id}&passphrase={s}&pbkeylen=16"
                )),
                _ => None,
            },
        },
        host,
    }
}

fn as_text(label: &str, show: &str, d: &Details, monitor: &str) -> String {
    let host = d.host.as_deref().unwrap_or("(the station's address)");
    // A way the installation turned off (port 0) is left out, rather than offered on port 0.
    let icecast = if d.icecast.port == 0 {
        String::new()
    } else {
        format!(
            "Icecast (most radio software)\n  Server: {host}\n  Port: {}\n  Mount: {}\n  User: {}\n  Password: {}\n\n",
            d.icecast.port, d.icecast.mount, d.icecast.user, d.icecast.password
        )
    };
    let srt = if d.srt.port == 0 {
        String::new()
    } else {
        format!(
            "SRT (OBS and others; needed for callers or a co-host)\n  Server: {host}\n  Port: {}\n  Stream ID: {}\n  Passphrase: {}\n  Key length: 16 (AES-128)\n\n",
            d.srt.port, d.srt.streamid, d.srt.passphrase
        )
    };
    let none = if icecast.is_empty() && srt.is_empty() {
        "The station has not turned on its ports for hosts' own studios yet: ask its administrator.\n\n"
    } else {
        ""
    };
    format!(
        "{label}'s details for {show}\n\n{icecast}{srt}{none}Monitor link secret: {monitor}\n\nPut these into your software, open the monitor link, and press connect. You can test at any time. You go on air at your show's time, not when you connect.\n"
    )
}

/// The rules, with the installation's ports in force where it named them.
async fn settings_of(state: &AppState, pool: &sqlx::PgPool) -> Result<SourceSettings, Response> {
    model::get_settings(pool, state.station.id)
        .await
        .map(|s| state.hub.sources.in_force(s))
        .map_err(|e| db_failed(&e))
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/keys",
    operation_id = "sources_keys_list",
    tag = "sources",
    summary = "A show's keys",
    description = "Every key the show has, live and ended, newest first, never a secret; where the show comes from (`room` or `push`); each key's last refusal. For an admin, a producer or the show's presenter; an API token never.",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "The keys.", body = KeyList),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Response {
    let (_, show) = match manager(&state, &headers, &slug).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let settings = match settings_of(&state, pool).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let (keys, source) = match tokio::try_join!(
        model::list_keys(pool, state.station.id, show.id),
        model::show_source(pool, state.station.id, show.id)
    ) {
        Ok(x) => x,
        Err(e) => return db_failed(&e),
    };
    let mut rows = Vec::with_capacity(keys.len());
    for key in keys {
        let last_refusal = match model::key_log(pool, state.station.id, &key.key_id, 20).await {
            Ok(log) => log.into_iter().find(|e| e.event == "refused"),
            Err(e) => return db_failed(&e),
        };
        rows.push(KeyRow {
            details: details(&state, &settings, &key.key_id, None),
            key,
            last_refusal,
        });
    }
    Json(KeyList {
        show: show.slug,
        source,
        keys: rows,
    })
    .into_response()
}

/// Make a key for `show`, for `label`; the reply with its secrets.
async fn make(
    state: &AppState,
    actor: &Actor,
    show: &Show,
    label: &str,
    staff_id: Option<Uuid>,
    hint: &str,
) -> Result<MadeKey, Response> {
    let pool = pool(state)?;
    let key = token_key(state)?;
    let settings = settings_of(state, pool).await?;
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 120 || label.chars().any(char::is_control) {
        return Err(field_problem(
            "label",
            "who the key is for: 1 to 120 characters, no control characters",
        ));
    }
    if !matches!(hint, "icecast" | "srt") {
        return Err(field_problem("hint", "`icecast` or `srt`"));
    }
    let now = now_ms() as i64;
    let live = model::live_keys(pool, state.station.id, now)
        .await
        .map_err(|e| db_failed(&e))?;
    if live.iter().filter(|k| k.show_id == show.id).count() >= MAX_LIVE_KEYS {
        return Err(problem(
            StatusCode::CONFLICT,
            format!("a show has at most {MAX_LIVE_KEYS} keys that have not ended"),
        ));
    }
    let salt: [u8; 16] = random();
    let monitor_secret = crate::apikey::base32(&random::<15>());
    let monitor_hash = key.monitor_hash(&monitor_secret);
    // A key id is 8 base-32 characters (40 bits), unique; a clash is tried again.
    for _ in 0..5 {
        let key_id = crate::apikey::base32(&random::<5>());
        let made = model::create_key(
            pool,
            state.station.id,
            &NewShowKey {
                id: Uuid::new_v4(),
                show_id: show.id,
                label,
                staff_id,
                key_id: &key_id,
                key_salt: &salt,
                monitor_hash: &monitor_hash,
                hint,
                created_by: &actor.label(),
                created_ms: now,
            },
        )
        .await;
        match made {
            Ok(k) => {
                let secret = key.show_key_secret(&k.key_id, &k.key_salt);
                let d = details(state, &settings, &k.key_id, Some(&secret));
                tracing::info!(show = %show.slug, key = %k.key_id, by = %actor.label(), "a show key was made");
                return Ok(MadeKey {
                    as_text: as_text(&k.label, &show.name, &d, &monitor_secret),
                    key: k,
                    secret,
                    monitor_secret,
                    details: d,
                });
            }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation() => {
                return Err(field_problem("staff_id", "no such member of staff"));
            }
            Err(e) => return Err(db_failed(&e)),
        }
    }
    Err(problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "could not find a free key id",
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/keys",
    operation_id = "sources_keys_make",
    tag = "sources",
    summary = "Make a show key",
    description = "A key for one person to send to this show from their own software (ADR-0284). The reply holds the key's secret, the monitor link's secret and the details for Icecast and SRT: **shown this once**; the station keeps neither in a readable form. The show then comes from the host's own studio (`source: push`). For an admin, a producer or the show's presenter; an API token never.",
    params(("show" = String, Path, description = "The show's slug")),
    request_body = NewKey,
    responses(
        (status = 201, description = "The key, with its secrets, once.", body = MadeKey),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn make_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<NewKey>,
) -> Response {
    let (actor, show) = match manager(&state, &headers, &slug).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    match make(
        &state,
        &actor,
        &show,
        &body.label,
        body.staff_id,
        &body.hint,
    )
    .await
    {
        Ok(made) => (StatusCode::CREATED, Json(made)).into_response(),
        Err(r) => r,
    }
}

/// The show's key with this id, if it is the show's.
async fn key_of_show(state: &AppState, show: &Show, key_id: &str) -> Result<ShowKey, Response> {
    let pool = pool(state)?;
    match model::key_by_key_id(pool, state.station.id, key_id).await {
        Ok(Some(k)) if k.show_id == show.id => Ok(k),
        Ok(_) => Err(problem(
            StatusCode::NOT_FOUND,
            format!("show `{}` has no key `{key_id}`", show.slug),
        )),
        Err(e) => Err(db_failed(&e)),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/keys/{key}/renew",
    operation_id = "sources_keys_renew",
    tag = "sources",
    summary = "Make a new key in place of one",
    description = "A new key for the same person and show (its secrets shown once, as when a key is made). The old key, and its monitor link, end `now` (a connection with it is closed: if the host is on air, listeners hear what the dropped-link rule gives) or `on_disconnect` (when its connection closes, at once if it has none). For an admin, a producer or the show's presenter; an API token never.",
    params(
        ("show" = String, Path, description = "The show's slug"),
        ("key" = String, Path, description = "The old key's id"),
    ),
    request_body = Renew,
    responses(
        (status = 201, description = "The new key, with its secrets, once.", body = MadeKey),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn renew_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((slug, key_id)): Path<(String, String)>,
    Json(body): Json<Renew>,
) -> Response {
    let (actor, show) = match manager(&state, &headers, &slug).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !matches!(body.old.as_str(), "now" | "on_disconnect") {
        return field_problem("old", "`now` or `on_disconnect`");
    }
    let old = match key_of_show(&state, &show, &key_id).await {
        Ok(k) => k,
        Err(r) => return r,
    };
    if old.revoked_ms.is_some() {
        return problem(StatusCode::CONFLICT, "that key has ended already");
    }
    let made = match make(&state, &actor, &show, &old.label, old.staff_id, &old.hint).await {
        Ok(m) => m,
        Err(r) => return r,
    };
    let pool = state.db.as_ref().expect("checked by make");
    let connected = state
        .hub
        .sources
        .connections()
        .iter()
        .any(|c| c.key_id == key_id);
    let ended = if body.old == "now" || !connected {
        state.hub.sources.close_later(&key_id);
        crate::monitor::remove_seats(&state.hub, &key_id);
        model::end_key(pool, state.station.id, &key_id, now_ms() as i64).await
    } else {
        model::end_on_disconnect(pool, state.station.id, &key_id)
            .await
            .map(|()| true)
    };
    if let Err(e) = ended {
        return db_failed(&e);
    }
    (StatusCode::CREATED, Json(made)).into_response()
}

#[utoipa::path(
    delete,
    path = "/api/v1/shows/{show}/keys/{key}",
    operation_id = "sources_keys_end",
    tag = "sources",
    summary = "End a show key",
    description = "The key and its monitor link end for good: the host's software is refused from now on, and a connection with the key is closed. When it was the show's last key, the show comes from the room again. For an admin, a producer or the show's presenter; an API token never.",
    params(
        ("show" = String, Path, description = "The show's slug"),
        ("key" = String, Path, description = "The key's id"),
    ),
    responses(
        (status = 200, description = "Ended (`ended` is false when it had ended already)."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn end_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((slug, key_id)): Path<(String, String)>,
) -> Response {
    let (actor, show) = match manager(&state, &headers, &slug).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = key_of_show(&state, &show, &key_id).await {
        return r;
    }
    let pool = state.db.as_ref().expect("checked");
    match model::end_key(pool, state.station.id, &key_id, now_ms() as i64).await {
        Ok(ended) => {
            state.hub.sources.close_later(&key_id);
            crate::monitor::remove_seats(&state.hub, &key_id);
            tracing::info!(show = %show.slug, key = %key_id, by = %actor.label(), "a show key was ended");
            Json(json!({ "ended": ended })).into_response()
        }
        Err(e) => db_failed(&e),
    }
}

// ---- the sources, live ----

/// A source connected or due, as the studio sees it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct SourceView {
    pub key_id: String,
    /// The desk identity it has or would have (`source-` and the key id).
    pub identity: String,
    /// Who it is for.
    pub label: String,
    pub show: String,
    pub show_name: String,
    /// `on_air`, `standby` (at the desk, not on air), `test` (connected outside its slot,
    /// never on the desk), `link_lost` (it was on air and its link is gone; its place is kept
    /// until `place_kept_until_ms`), or `not_connected` (its slot's door is open and it has not
    /// connected).
    pub state: String,
    /// The desk slot it holds, while it is at the desk.
    pub slot: Option<usize>,
    /// `icecast` or `srt`, while connected.
    pub way: Option<String>,
    /// What the host's software sends, while connected.
    pub format: Option<String>,
    /// Its level at the desk (short-term LUFS, after the leveller); for a test, the ingest's
    /// reading (dBFS RMS).
    pub level: Option<f32>,
    /// What the leveller does, in dB, while it is at the desk.
    pub leveller_db: Option<f32>,
    /// How long it has been connected, in ms.
    pub steady_ms: Option<u64>,
    /// Whether the host's monitor link is open: someone listens, and the desk makes their monitor mix.
    pub monitor_open: bool,
    /// When its slot's door closes, while it is open.
    pub door_closes_ms: Option<i64>,
    /// What the host's software says is playing (Icecast's metadata), while connected.
    pub title: Option<String>,
    /// Turned away from the desk until then (the tile's Disconnect).
    pub turned_away_until_ms: Option<i64>,
    /// While `link_lost`: until when its place is kept.
    pub place_kept_until_ms: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SourcesView {
    /// Whether the ingest is running and reporting.
    pub listening: bool,
    pub sources: Vec<SourceView>,
    /// The next handover, as in `GET /studio`: the show whose post it is, its source's state, the
    /// grace's and the late count's ends, who holds it, and a lost link with its place kept
    /// (ADR-0280). Absent when there is none.
    pub post: Option<weavesignal_model::post::PostView>,
}

#[utoipa::path(
    get,
    path = "/api/v1/sources",
    operation_id = "sources_list",
    tag = "sources",
    summary = "Sources connected or due",
    description = "Every host's own studio that is connected (`on_air`, `standby`, `test`) or due (`not_connected`: its slot's door is open). `listening` says whether the ingest is running. For staff; an API token with `sources:read`.",
    responses(
        (status = 200, description = "The sources.", body = SourcesView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_sources(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match sources_now(&state, pool).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => db_failed(&e),
    }
}

/// The sources as they stand: the keys, the ingest's report and the desk's status.
pub async fn sources_now(
    state: &AppState,
    pool: &sqlx::PgPool,
) -> Result<SourcesView, sqlx::Error> {
    let now = now_ms() as i64;
    let settings = model::get_settings(pool, state.station.id).await?;
    let keys = model::live_keys(pool, state.station.id, now).await?;
    let all_shows: BTreeMap<Uuid, Show> = shows::list_shows(pool, state.station.id)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let air = airings(state, pool, now / 1000).await?;
    let conns = state.hub.sources.connections();
    let desk = state.hub.desk_status();
    let post = state.hub.view().post.clone();
    let mut out = Vec::new();
    for k in keys {
        let Some(show) = all_shows.get(&k.show_id) else {
            continue;
        };
        let identity = source_wire::identity(&k.key_id);
        let door = model::door(
            show.id,
            &air,
            now / 1000,
            i64::from(settings.door_before_min) * 60,
        );
        let conn = conns.iter().find(|c| c.key_id == k.key_id);
        let at_desk = desk
            .as_ref()
            .and_then(|d| d.people.iter().find(|p| p.identity == identity));
        let lost = post
            .as_ref()
            .and_then(|p| p.link_lost.as_ref())
            .filter(|l| l.key_id == k.key_id);
        let state_word = match (conn, at_desk) {
            (_, Some(p)) if p.on_air => "on_air",
            (_, Some(_)) => "standby",
            (Some(c), None) if c.mode == "test" => "test",
            (Some(_), None) => "standby",
            (None, None) if lost.is_some() => "link_lost",
            (None, None) if matches!(door, Door::Open { .. }) => "not_connected",
            (None, None) => continue,
        };
        out.push(SourceView {
            slot: at_desk.and_then(|_| state.hub.read_studio(|s| s.slot_of(&identity))),
            identity,
            key_id: k.key_id.clone(),
            label: k.label.clone(),
            show: show.slug.clone(),
            show_name: show.name.clone(),
            state: state_word.to_owned(),
            way: conn.map(|c| c.way.clone()),
            format: conn.map(|c| c.format.clone()).filter(|f| !f.is_empty()),
            level: at_desk
                .and_then(|p| p.lufs)
                .or_else(|| conn.and_then(|c| c.level_db)),
            leveller_db: at_desk
                .and_then(|p| p.source.as_ref())
                .map(|s| s.leveller_db),
            steady_ms: conn.map(|c| (now as u64).saturating_sub(c.since_ms)),
            monitor_open: at_desk
                .and_then(|p| p.source.as_ref())
                .is_some_and(|s| s.monitor),
            door_closes_ms: match door {
                Door::Open { close, .. } => Some(close * 1000),
                Door::Closed { .. } => None,
            },
            title: conn.and_then(|c| c.title.clone()),
            turned_away_until_ms: k.turned_away_until_ms.filter(|&t| t > now),
            place_kept_until_ms: lost.map(|l| l.place_kept_until_ms),
        });
    }
    Ok(SourcesView {
        listening: state.hub.sources.ingest_alive(),
        sources: out,
        post,
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/sources/{key}/disconnect",
    operation_id = "sources_disconnect",
    tag = "sources",
    summary = "Disconnect a source",
    description = "The tile's Disconnect: closes the source's connection, takes it off the desk, and turns its key away until its slot's door closes (most software reconnects by itself). `DELETE` lets it back in. For staff at the desk (a window preparing a show is refused, 409); an API token never.",
    params(("key" = String, Path, description = "The key's id")),
    responses(
        (status = 200, description = "Disconnected; `until_ms` is when the key is let back in by itself."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn disconnect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Response {
    let actor = match state.check(&headers, Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let key = match model::key_by_key_id(pool, state.station.id, &key_id).await {
        Ok(Some(k)) => k,
        Ok(None) => return problem(StatusCode::NOT_FOUND, format!("no key `{key_id}`")),
        Err(e) => return db_failed(&e),
    };
    let now = now_ms() as i64;
    let settings = match settings_of(&state, pool).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let air = match airings(&state, pool, now / 1000).await {
        Ok(a) => a,
        Err(e) => return db_failed(&e),
    };
    let until = match model::door(
        key.show_id,
        &air,
        now / 1000,
        i64::from(settings.door_before_min) * 60,
    ) {
        Door::Open { close, .. } => close * 1000,
        Door::Closed { .. } => now + model::DOOR_AFTER_S * 1000,
    };
    if let Err(e) = model::turn_away(
        pool,
        state.station.id,
        &key_id,
        Some(until),
        Some(&actor.label()),
    )
    .await
    {
        return db_failed(&e);
    }
    state.hub.sources.close_later(&key_id);
    let _ = model::log(
        pool,
        state.station.id,
        Some(key.show_id),
        &LogEntry {
            at_ms: now,
            key_id: key_id.clone(),
            event: "disconnected".into(),
            reason: None,
            detail: format!("by {}", actor.label()),
            way: None,
            address: None,
        },
    )
    .await;
    Json(json!({ "disconnected": true, "until_ms": until })).into_response()
}

#[utoipa::path(
    delete,
    path = "/api/v1/sources/{key}/disconnect",
    operation_id = "sources_let_back",
    tag = "sources",
    summary = "Let a disconnected source back in",
    description = "Settings' Let back in: the key is taken again at once. For an admin, a producer or the show's presenter; an API token never.",
    params(("key" = String, Path, description = "The key's id")),
    responses(
        (status = 200, description = "Let back in."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn let_back(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let key = match model::key_by_key_id(pool, state.station.id, &key_id).await {
        Ok(Some(k)) => k,
        Ok(None) => return problem(StatusCode::NOT_FOUND, format!("no key `{key_id}`")),
        Err(e) => return db_failed(&e),
    };
    let presenter = shows::list_shows(pool, state.station.id)
        .await
        .ok()
        .and_then(|all| all.into_iter().find(|s| s.id == key.show_id))
        .and_then(|s| s.presenter_account);
    if !may_manage(&actor, presenter) {
        return problem(
            StatusCode::FORBIDDEN,
            "Only an admin, a producer or the show's presenter lets a key back in.",
        );
    }
    if let Err(e) = model::turn_away(pool, state.station.id, &key_id, None, None).await {
        return db_failed(&e);
    }
    let _ = model::log(
        pool,
        state.station.id,
        Some(key.show_id),
        &LogEntry {
            at_ms: now_ms() as i64,
            key_id,
            event: "let_back".into(),
            reason: None,
            detail: format!("by {}", actor.label()),
            way: None,
            address: None,
        },
    )
    .await;
    Json(json!({ "let_back": true })).into_response()
}

// ---- the rules ----

#[utoipa::path(
    get,
    path = "/api/v1/settings/sources",
    operation_id = "sources_settings_get",
    tag = "sources",
    summary = "The rules for sources",
    description = "The station's rules for hosts in their own studios: the door, the grace, tests outside the slot, the late count, the dropped-link times, the fade, and the ingest's two ports. Admin only.",
    responses(
        (status = 200, description = "The rules.", body = SourceSettings),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_settings(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match settings_of(&state, pool).await {
        Ok(s) => Json(s).into_response(),
        Err(r) => r,
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/settings/sources",
    operation_id = "sources_settings_put",
    tag = "sources",
    summary = "Change the rules for sources",
    description = "Every rule at once. The ingest takes new ports within seconds (it listens afresh). A port the installation names (`ports.source_icecast`, `ports.source_srt`: the Compose settings, Cloudron's dashboard) is shown here and cannot be changed here: send it as it is, or the change is refused (422). 0 is a way the installation turned off. Admin only.",
    request_body = SourceSettings,
    responses(
        (status = 200, description = "The rules now.", body = SourceSettings),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<SourceSettings>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    // A port the installation names stays as it is: the same number is taken (and the stored
    // rule kept under it), another is refused.
    let mut body = body;
    let (fixed_ice, fixed_srt) = state.hub.sources.fixed_ports();
    if fixed_ice.is_some() || fixed_srt.is_some() {
        let stored = match model::get_settings(pool, state.station.id).await {
            Ok(s) => s,
            Err(e) => return db_failed(&e),
        };
        for (fixed, field, given, kept, setting) in [
            (
                fixed_ice,
                "port_icecast",
                &mut body.port_icecast,
                stored.port_icecast,
                "ports.source_icecast",
            ),
            (
                fixed_srt,
                "port_srt",
                &mut body.port_srt,
                stored.port_srt,
                "ports.source_srt",
            ),
        ] {
            if let Some(p) = fixed {
                if *given != i32::from(p) {
                    return field_problem(
                        field,
                        format!(
                            "this port is the installation's ({setting}, {p}): change it where the station is installed"
                        ),
                    );
                }
                *given = kept;
            }
        }
    }
    if let Err((field, why)) = body.check() {
        return field_problem(field, why);
    }
    let shown = state.hub.sources.in_force(body);
    if shown.port_icecast == shown.port_srt && shown.port_icecast != 0 {
        // One is TCP and the other UDP, so the same number would work; it is refused only
        // because a host who types one into the wrong field then gets no answer at all.
        return field_problem("port_srt", "use a different number from the Icecast port");
    }
    match model::put_settings(
        pool,
        state.station.id,
        &body,
        &actor.label(),
        now_ms() as i64,
    )
    .await
    {
        Ok(()) => Json(shown).into_response(),
        Err(e) => db_failed(&e),
    }
}

// ---- the ingest's routes ----

/// `None` when the request is the ingest's: a loopback peer, no proxy's header, and its bearer.
#[allow(clippy::result_large_err)]
fn ingest_only(
    state: &AppState,
    headers: &HeaderMap,
    conn: Option<&ConnectInfo<SocketAddr>>,
) -> Option<Response> {
    let Some(want) = state
        .hub
        .sources
        .ingest_token
        .lock()
        .expect("sources lock")
        .clone()
    else {
        return Some(unavailable(&format!(
            "the ingest's routes are off: set {INGEST_TOKEN_ENV}"
        )));
    };
    let forwarded = ["x-forwarded-for", "forwarded", "x-real-ip"]
        .iter()
        .any(|h| headers.contains_key(*h));
    let loopback = conn.is_some_and(|ConnectInfo(a)| a.ip().is_loopback());
    if forwarded || !loopback {
        return Some(problem(
            StatusCode::FORBIDDEN,
            "for the ingest on this machine only",
        ));
    }
    let given = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !crate::app::same(given.as_bytes(), want.as_bytes()) {
        return Some(problem(StatusCode::UNAUTHORIZED, "not the ingest's bearer"));
    }
    None
}

/// What the ingest needs to listen.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct IngestConfig {
    /// Whether to listen at all: only once a key exists that has not ended.
    pub listen: bool,
    /// 0: this way is off (the installation turned it off).
    pub port_icecast: u16,
    /// 0: this way is off.
    pub port_srt: u16,
    /// A test's longest life, in ms.
    pub test_max_ms: u64,
}

#[utoipa::path(
    get,
    path = "/api/v1/internal/sources/config",
    operation_id = "internal_sources_config",
    tag = "internal",
    summary = "The ingest's settings",
    description = "For the ingest on this machine only (a loopback peer, no proxy's header, the bearer in `WEAVESIGNAL_INGEST_TOKEN`): whether to listen, and on which ports. The ingest asks every few seconds.",
    responses(
        (status = 200, description = "The ingest's settings.", body = IngestConfig),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("ingest" = [])),
)]
async fn internal_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Response {
    if let Some(r) = ingest_only(&state, &headers, conn.as_ref().map(|Extension(c)| c)) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let now = now_ms() as i64;
    let (settings, keys) = match tokio::try_join!(
        model::get_settings(pool, state.station.id),
        model::live_keys(pool, state.station.id, now)
    ) {
        Ok(x) => x,
        Err(e) => return db_failed(&e),
    };
    let settings = state.hub.sources.in_force(settings);
    Json(IngestConfig {
        listen: !keys.is_empty(),
        port_icecast: settings.port_icecast as u16,
        port_srt: settings.port_srt as u16,
        test_max_ms: settings.test_max_min as u64 * 60_000,
    })
    .into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdmitRequest {
    /// The key id the host's software gave (the SRT stream id, or the Icecast password's first
    /// part).
    pub key_id: String,
    /// The secret it gave, for Icecast; left out for SRT, whose handshake proves it.
    #[serde(default)]
    pub secret: Option<String>,
    /// `icecast` or `srt`.
    pub way: String,
    /// The address the connection came from, as the ingest saw it.
    pub address: String,
    /// The ingest asks again for a connection it already holds (a test, to see whether the
    /// slot's door has opened); nothing is counted or logged.
    #[serde(default)]
    pub again: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, utoipa::ToSchema)]
pub struct AdmitReply {
    /// `standby`, `test` or `refused`.
    pub decision: String,
    /// For a refusal: its word.
    pub reason: Option<Refusal>,
    /// For a refusal: the sentence for the host.
    pub message: Option<String>,
    /// When a refused address may try again.
    pub retry_after_ms: Option<i64>,
    /// For `standby`: the desk slot it is promised.
    pub slot: Option<usize>,
    /// Its desk identity (`source-` and the key id).
    pub identity: Option<String>,
    /// Who the key is for: the name on the desk.
    pub name: Option<String>,
    pub show: Option<String>,
    /// For SRT: the passphrase to answer the handshake with.
    pub passphrase: Option<String>,
    /// For `test`: how long it may last.
    pub max_ms: Option<i64>,
    /// For `standby`: when the slot's door closes.
    pub door_closes_ms: Option<i64>,
}

impl AdmitReply {
    fn refused(r: Refusal, message: String, retry_after_ms: Option<i64>) -> Self {
        AdmitReply {
            decision: "refused".into(),
            reason: Some(r),
            message: Some(message),
            retry_after_ms,
            slot: None,
            identity: None,
            name: None,
            show: None,
            passphrase: None,
            max_ms: None,
            door_closes_ms: None,
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/internal/sources/admit",
    operation_id = "internal_sources_admit",
    tag = "internal",
    summary = "May this connection stand by?",
    description = "For the ingest on this machine only, before it reads any media. With the key id (and, for Icecast, the secret) and the address: `standby` with the desk slot promised to it, `test` (outside its slot, never on the desk) with how long, or `refused` with its reason. For SRT the reply carries the passphrase for the handshake; the handshake's outcome comes back on `events`. Five wrong keys from a public address in a minute refuse it for ten minutes; an address that is private, loopback, link-local or carrier-grade NAT is counted and logged but never refused.",
    request_body = AdmitRequest,
    responses(
        (status = 200, description = "The decision.", body = AdmitReply),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("ingest" = [])),
)]
async fn internal_admit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(req): Json<AdmitRequest>,
) -> Response {
    if let Some(r) = ingest_only(&state, &headers, conn.as_ref().map(|Extension(c)| c)) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let key = match token_key(&state) {
        Ok(k) => k,
        Err(r) => return r,
    };
    match admit(&state, pool, key, &req).await {
        Ok(reply) => Json(reply).into_response(),
        Err(e) => db_failed(&e),
    }
}

async fn admit(
    state: &AppState,
    pool: &sqlx::PgPool,
    tk: &ApiKey,
    req: &AdmitRequest,
) -> Result<AdmitReply, sqlx::Error> {
    let now = now_ms() as i64;
    let live = &state.hub.sources;
    let addr = req.address.chars().take(64).collect::<String>();
    let blocked_until = live
        .wrong
        .lock()
        .expect("sources lock")
        .blocked_until(&addr, now);
    if let Some(until) = blocked_until {
        let r = Refusal::TooMany;
        if !req.again {
            log_refusal(state, pool, None, &req.key_id, r, &req.way, &addr).await;
        }
        return Ok(AdmitReply::refused(
            r,
            format!(
                "Too many wrong keys from your address. Try again in {} minutes.",
                (until - now + 59_999) / 60_000
            ),
            Some(until - now),
        ));
    }
    let addr_ref = addr.as_str();
    let wrong = |show: Option<Uuid>| async move {
        let blocked = live
            .wrong
            .lock()
            .expect("sources lock")
            .wrong(addr_ref, now);
        log_refusal(
            state,
            pool,
            show,
            &req.key_id,
            Refusal::WrongKey,
            &req.way,
            addr_ref,
        )
        .await;
        if blocked {
            tracing::warn!(address = %addr_ref, "five wrong show keys in a minute: refused for ten minutes");
        }
        Ok(AdmitReply::refused(
            Refusal::WrongKey,
            Refusal::WrongKey.words().into(),
            None,
        ))
    };
    let well_formed = req.key_id.len() == 8
        && req
            .key_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c));
    let found = if well_formed {
        model::key_by_key_id(pool, state.station.id, &req.key_id).await?
    } else {
        None
    };
    let Some(key) = found else {
        return wrong(None).await;
    };
    let secret = tk.show_key_secret(&key.key_id, &key.key_salt);
    match (req.way.as_str(), &req.secret) {
        ("icecast", Some(given)) if crate::app::same(given.as_bytes(), secret.as_bytes()) => {}
        ("srt", None) => {}
        _ => return wrong(Some(key.show_id)).await,
    }
    let settings = model::get_settings(pool, state.station.id).await?;
    let air = airings(state, pool, now / 1000).await?;
    let door = model::door(
        key.show_id,
        &air,
        now / 1000,
        i64::from(settings.door_before_min) * 60,
    );
    let show = shows::list_shows(pool, state.station.id)
        .await?
        .into_iter()
        .find(|s| s.id == key.show_id);
    let (slug, show_name) = show.map_or_else(Default::default, |s| (s.slug, s.name));
    let identity = source_wire::identity(&key.key_id);
    let srt = req.way == "srt";
    let mut reply = match model::admit(&key, door, &settings, now) {
        Admission::Refused(r) => {
            let message = match (r, door) {
                (Refusal::NotYourSlot, Door::Closed { next: Some(next) }) => format!(
                    "It is not your slot. {show_name} is next at {}; you can connect from {} minutes before.",
                    crate::sources::when(state, next),
                    settings.door_before_min
                ),
                _ => r.words().to_owned(),
            };
            if !req.again {
                log_refusal(
                    state,
                    pool,
                    Some(key.show_id),
                    &key.key_id,
                    r,
                    &req.way,
                    &addr,
                )
                .await;
            }
            return Ok(AdmitReply::refused(r, message, None));
        }
        Admission::Test { max_ms } => AdmitReply {
            decision: "test".into(),
            max_ms: Some(max_ms),
            ..AdmitReply::refused(Refusal::WrongKey, String::new(), None)
        },
        Admission::StandBy { door } => {
            // For SRT the slot is promised only once the handshake has proved the passphrase
            // (the `admitted` event): a key id is public.
            let slot = if srt {
                state
                    .hub
                    .read_studio(|s| s.free_source_slot(&identity, now_ms()))
            } else {
                state
                    .hub
                    .with_studio(|s, now| s.reserve_source_slot(&identity, now))
            };
            let Some(slot) = slot else {
                let r = Refusal::DeskFull;
                if !req.again {
                    log_refusal(
                        state,
                        pool,
                        Some(key.show_id),
                        &key.key_id,
                        r,
                        &req.way,
                        &addr,
                    )
                    .await;
                }
                return Ok(AdmitReply::refused(r, r.words().into(), None));
            };
            AdmitReply {
                decision: "standby".into(),
                slot: Some(slot),
                door_closes_ms: match door {
                    Door::Open { close, .. } => Some(close * 1000),
                    Door::Closed { .. } => None,
                },
                ..AdmitReply::refused(Refusal::WrongKey, String::new(), None)
            }
        }
    };
    reply.reason = None;
    reply.message = None;
    reply.identity = Some(identity);
    reply.name = Some(key.label.clone());
    reply.show = Some(slug);
    reply.passphrase = srt.then_some(secret);
    Ok(reply)
}

/// A moment on the station's clock in words: `Friday 27 November at 14:00`.
pub fn when(state: &AppState, epoch_s: i64) -> String {
    let zone = state.hub.zone.get();
    jiff::Timestamp::from_second(epoch_s)
        .map(|t| t.to_zoned(zone).strftime("%A %-d %B at %H:%M").to_string())
        .unwrap_or_default()
}

async fn log_refusal(
    state: &AppState,
    pool: &sqlx::PgPool,
    show: Option<Uuid>,
    key_id: &str,
    r: Refusal,
    way: &str,
    addr: &str,
) {
    let entry = LogEntry {
        at_ms: now_ms() as i64,
        key_id: key_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect(),
        event: "refused".into(),
        reason: Some(r.as_str().into()),
        detail: String::new(),
        way: matches!(way, "icecast" | "srt").then(|| way.to_owned()),
        address: Some(addr.to_owned()),
    };
    if let Err(e) = model::log(pool, state.station.id, show, &entry).await {
        tracing::warn!(error = %e, "sources: cannot log a refusal");
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestReport {
    /// Every connection the ingest holds now, standing by or in a test.
    pub connections: Vec<IngestConn>,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReportReply {
    /// Key ids whose connections the ingest is to close now: a key that ended, a source
    /// disconnected from the desk.
    pub close: Vec<String>,
    /// Connections standing by whose slot's door has closed and that are not on air: the ingest
    /// makes each a private test, its time running from then.
    #[serde(default)]
    pub to_test: Vec<ToTest>,
}

/// A connection to turn into a private test.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct ToTest {
    pub key_id: String,
    /// How long the test may last from now, in ms (the rules' `test_max_min`).
    pub max_ms: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/internal/sources/report",
    operation_id = "internal_sources_report",
    tag = "internal",
    summary = "The ingest's connections",
    description = "For the ingest on this machine only, every second: what it holds. The reply names the connections to close, and those standing by whose slot's door has closed and that are not on air, which become private tests (their time running from then; one on air is never touched).",
    request_body = IngestReport,
    responses(
        (status = 200, description = "What to close.", body = ReportReply),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("ingest" = [])),
)]
async fn internal_report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<IngestReport>,
) -> Response {
    if let Some(r) = ingest_only(&state, &headers, conn.as_ref().map(|Extension(c)| c)) {
        return r;
    }
    let live = &state.hub.sources;
    let now = now_ms() as i64;
    let mut to_test: Vec<ToTest> = Vec::new();
    let mut close: BTreeSet<String> = {
        let mut pending = live.close.lock().expect("sources lock");
        // A key to close stays asked for until the ingest no longer holds it.
        pending.retain(|k| body.connections.iter().any(|c| &c.key_id == k));
        pending.clone()
    };
    // Keys that ended, or were turned away, since they connected.
    if let Some(pool) = &state.db
        && let Ok(keys) = model::live_keys(pool, state.station.id, now).await
    {
        for c in &body.connections {
            match keys.iter().find(|k| k.key_id == c.key_id) {
                None => {
                    close.insert(c.key_id.clone());
                }
                Some(k) if k.turned_away_until_ms.is_some_and(|t| now < t) => {
                    close.insert(c.key_id.clone());
                }
                Some(_) => {}
            }
        }
        if let Err(e) = after_the_door(
            &state,
            pool,
            &body.connections,
            &keys,
            now,
            &mut close,
            &mut to_test,
        )
        .await
        {
            tracing::warn!(error = %e, "sources: cannot look at the doors of sources standing by");
        }
    }
    let mut conns = body.connections;
    for c in &mut conns {
        c.title = c
            .title
            .as_deref()
            .map(source_wire::clean)
            .filter(|t| !t.is_empty());
        c.format = source_wire::clean(&c.format);
    }
    *live.report.lock().expect("sources lock") = Report {
        at: Some(Instant::now()),
        conns,
    };
    Json(ReportReply {
        close: close.into_iter().collect(),
        to_test,
    })
    .into_response()
}

/// The sources standing by whose slot's door has closed (the operator's answer, 2026-11-16: a
/// source standing by and not on air then becomes a private test, so it leaves the desk; one on
/// air is left alone, and the next report finds it off the air). Without a report from the desk
/// nothing is known of who is on air, and nothing is touched. With tests outside the slot off
/// there is no test to become, so the connection is closed.
async fn after_the_door(
    state: &AppState,
    pool: &sqlx::PgPool,
    conns: &[IngestConn],
    keys: &[ShowKey],
    now: i64,
    close: &mut BTreeSet<String>,
    to_test: &mut Vec<ToTest>,
) -> Result<(), sqlx::Error> {
    if !conns.iter().any(|c| c.mode == "standby") {
        return Ok(());
    }
    let Some(desk) = state.hub.desk_status() else {
        return Ok(());
    };
    let settings = model::get_settings(pool, state.station.id).await?;
    let air = airings(state, pool, now / 1000).await?;
    for c in conns.iter().filter(|c| c.mode == "standby") {
        let Some(k) = keys.iter().find(|k| k.key_id == c.key_id) else {
            continue;
        };
        let identity = source_wire::identity(&k.key_id);
        let on_air = desk
            .people
            .iter()
            .any(|p| p.identity == identity && p.on_air);
        let door = model::door(
            k.show_id,
            &air,
            now / 1000,
            i64::from(settings.door_before_min) * 60,
        );
        match model::after_door(on_air, door, &settings) {
            AfterDoor::Stay => {}
            AfterDoor::Test { max_ms } => to_test.push(ToTest {
                key_id: c.key_id.clone(),
                max_ms,
            }),
            AfterDoor::Close => {
                close.insert(c.key_id.clone());
            }
        }
    }
    Ok(())
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestEvent {
    pub key_id: String,
    /// `admitted` (standing by), `test`, `refused` (with `reason`) or `ended`.
    pub event: String,
    #[serde(default)]
    pub reason: Option<Refusal>,
    #[serde(default)]
    pub detail: String,
    pub way: String,
    #[serde(default)]
    pub address: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/internal/sources/events",
    operation_id = "internal_sources_events",
    tag = "internal",
    summary = "What happened to a connection",
    description = "For the ingest on this machine only: a connection admitted (after SRT's handshake), a test begun, a refusal the ingest made (a failed SRT handshake counts as a wrong key; a format it cannot read; a key in use; too fast), or an end. Written to the key's log.",
    request_body = IngestEvent,
    responses(
        (status = 204, description = "Written."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("ingest" = [])),
)]
async fn internal_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(e): Json<IngestEvent>,
) -> Response {
    if let Some(r) = ingest_only(&state, &headers, conn.as_ref().map(|Extension(c)| c)) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !matches!(e.event.as_str(), "admitted" | "test" | "refused" | "ended") {
        return field_problem("event", "`admitted`, `test`, `refused` or `ended`");
    }
    let now = now_ms() as i64;
    let addr = e
        .address
        .as_deref()
        .map(|a| a.chars().take(64).collect::<String>());
    let key = match model::key_by_key_id(pool, state.station.id, &e.key_id).await {
        Ok(k) => k,
        Err(err) => return db_failed(&err),
    };
    if e.event == "refused"
        && e.reason == Some(Refusal::WrongKey)
        && let Some(a) = &addr
    {
        state
            .hub
            .sources
            .wrong
            .lock()
            .expect("sources lock")
            .wrong(a, now);
    }
    let entry = LogEntry {
        at_ms: now,
        key_id: e
            .key_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect(),
        event: e.event.clone(),
        reason: e.reason.map(|r| r.as_str().to_owned()),
        detail: source_wire::clean(&e.detail),
        way: matches!(e.way.as_str(), "icecast" | "srt").then(|| e.way.clone()),
        address: addr,
    };
    let show = key.as_ref().map(|k| k.show_id);
    if let Err(err) = model::log(pool, state.station.id, show, &entry).await {
        return db_failed(&err);
    }
    if let Some(k) = &key {
        if matches!(e.event.as_str(), "admitted" | "test") {
            let _ = model::touch_key(pool, state.station.id, &k.key_id, now).await;
        }
        if e.event == "admitted" {
            let identity = source_wire::identity(&k.key_id);
            state
                .hub
                .with_studio(|s, now| s.reserve_source_slot(&identity, now));
        }
        if e.event == "ended" && k.ends_on_disconnect {
            let _ = model::end_key(pool, state.station.id, &k.key_id, now).await;
            crate::monitor::remove_seats(&state.hub, &k.key_id);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_keys,
        make_key,
        renew_key,
        end_key,
        list_sources,
        disconnect,
        let_back,
        get_settings,
        put_settings,
        internal_config,
        internal_admit,
        internal_report,
        internal_events
    ),
    components(schemas(IngestConn))
)]
pub struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_staff_who_run_the_show_manage_its_keys() {
        assert!(may_manage(&Actor::Station, None));
    }
}
