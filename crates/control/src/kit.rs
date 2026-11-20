// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The presenter kit's server half (Phase 17g; design note `2026-11-20-17g-bridge-and-kit-design`,
//! sections 4 and 5): VBAN in, each stream bound to a show key; VBAN out from the desk; radio
//! presets that set that routing in one press; and the clock's counts.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET`, `PUT /api/v1/kit` | admin | the kit's settings, checked |
//! | `POST /api/v1/kit/preset` | admin | a preset's routing applied to the settings |
//! | `GET /api/v1/kit/clock` | staff | each source's clock against the desk's, and VBAN's counts |
//!
//! VBAN is unauthenticated UDP for one network ("with any switch or router"), so:
//!
//! - it is **off** until the installation allows it (`[kit] vban = true` in the settings file),
//!   and **never on Cloudron** (an internet server: the package sets no UDP port for it, and the
//!   control plane refuses it whenever Cloudron's `CLOUDRON` is in its environment);
//! - it uses one **named address** of the station's, which is refused when it is public unless
//!   `allow_public` is set (with a warning in the log and on the screen);
//! - an input is a show key's source, so it only ever stands by until a person puts it on air;
//!   out goes to one named address each, never broadcast or multicast.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use weavesignal_model::sources::is_private_address;
use weavesignal_proto::vban::{DataType, StreamName};

use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::staff::Need;

/// The most VBAN inputs and outputs.
pub const MAX_INS: usize = 4;
pub const MAX_OUTS: usize = 4;
/// VBAN's usual port.
pub const VBAN_PORT: u16 = weavesignal_proto::vban::DEFAULT_PORT;
/// The environment variable Cloudron sets in every app.
pub const CLOUDRON_ENV: &str = "CLOUDRON";

/// What may leave the desk as VBAN.
pub const OUT_SOURCES: &str = "`programme` (after the profanity delay: what listeners hear), `programme_live` (before it), `monitor:<key id>` (the monitor mix the desk makes for a host's own studio) or `mix_minus:<identity>` (a person's return)";

/// The presets, by name: what each sets.
pub const PRESETS: [(&str, &str); 4] = [
    (
        "studio_lan",
        "Studio PC on the network: its VBAN stands by as a show key's source, and the live programme goes back to the studio's monitors (before the delay).",
    ),
    (
        "transmitter",
        "Transmitter feed: the programme after the profanity delay, what listeners hear, to a transmitter or a recorder.",
    ),
    (
        "local_guest",
        "Local guest: the first VBAN in stands by, and the desk sends it back the monitor mix it makes for that source (everyone but itself).",
    ),
    ("custom", "Custom: the rows as they are."),
];

fn default_port() -> u16 {
    VBAN_PORT
}
fn default_preset() -> String {
    "custom".into()
}
fn default_pair() -> [u16; 2] {
    [1, 2]
}
fn default_source() -> String {
    "programme".into()
}
fn default_format() -> String {
    "s16".into()
}
fn yes() -> bool {
    true
}

/// A VBAN stream the station takes: a sender's address and stream name, bound to a show key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VbanIn {
    /// The sender's address on the station's network (`192.168.1.20`).
    pub address: String,
    /// The stream's name as the sender sends it (Voicemeeter's `Stream1`), 1 to 16 characters.
    pub name: String,
    /// The show key whose source it is.
    pub key_id: String,
    /// The channels taken, left then right, counted from 1; mono is heard in both ears.
    #[serde(default = "default_pair")]
    pub channels: [u16; 2],
}

/// A VBAN stream the desk sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VbanOut {
    /// One receiver's address; never broadcast or multicast.
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// The stream's name the receiver listens for.
    pub name: String,
    /// What is sent: see [`OUT_SOURCES`].
    #[serde(default = "default_source")]
    pub source: String,
    /// `s16` (the default; 240 frames a packet), `s24` (160) or `f32` (120, by choice only:
    /// Voicemeeter's support is not stated).
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

/// The kit's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Kit {
    /// The preset last applied: `studio_lan`, `transmitter`, `local_guest` or `custom`.
    #[serde(default = "default_preset")]
    pub preset: String,
    /// The station's own address on the network VBAN uses: VBAN in listens there, VBAN out
    /// sends from it. Needed for either.
    #[serde(default)]
    pub bind: Option<String>,
    /// VBAN in's port.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Allow a public address (VBAN has no authentication or encryption: carry it through
    /// WireGuard instead).
    #[serde(default)]
    pub allow_public: bool,
    #[serde(default)]
    pub vban_in: Vec<VbanIn>,
    #[serde(default)]
    pub vban_out: Vec<VbanOut>,
}

impl Default for Kit {
    fn default() -> Self {
        Kit {
            preset: default_preset(),
            bind: None,
            port: VBAN_PORT,
            allow_public: false,
            vban_in: Vec::new(),
            vban_out: Vec::new(),
        }
    }
}

fn key_id_ok(k: &str) -> bool {
    k.len() == 8
        && k.chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
}

/// Whether `source` names something the desk can send.
pub fn out_source_ok(source: &str) -> bool {
    match source.split_once(':') {
        None => matches!(source, "programme" | "programme_live"),
        Some(("monitor", k)) => key_id_ok(k),
        Some(("mix_minus", id)) => {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        _ => false,
    }
}

impl Kit {
    /// Whether it asks for anything: a VBAN in or an enabled VBAN out.
    pub fn active(&self) -> bool {
        !self.vban_in.is_empty() || self.vban_out.iter().any(|o| o.enabled)
    }

    /// The named address, parsed.
    pub fn bind_addr(&self) -> Option<IpAddr> {
        self.bind.as_deref().and_then(|b| b.trim().parse().ok())
    }

    /// Check every field: `Err((field, why))` for the first that cannot be used.
    pub fn check(&self) -> Result<(), (String, String)> {
        let bad = |f: &str, w: &str| Err((f.to_owned(), w.to_owned()));
        if !PRESETS.iter().any(|(p, _)| *p == self.preset) {
            return bad(
                "preset",
                "`studio_lan`, `transmitter`, `local_guest` or `custom`",
            );
        }
        if self.port == 0 {
            return bad("port", "a port from 1 to 65535");
        }
        if self.vban_in.len() > MAX_INS {
            return bad("vban_in", "at most 4 inputs");
        }
        if self.vban_out.len() > MAX_OUTS {
            return bad("vban_out", "at most 4 outputs");
        }
        let public = |a: &str| !is_private_address(a);
        match (&self.bind, self.bind_addr()) {
            (None, _) if self.active() => {
                return bad(
                    "bind",
                    "the station's own address on the network VBAN uses (192.168.1.10, say)",
                );
            }
            (Some(_), None) => return bad("bind", "an IP address"),
            (Some(b), Some(a)) if a.is_unspecified() || a.is_multicast() => {
                let _ = b;
                return bad(
                    "bind",
                    "one address of the station's, not every address or a group",
                );
            }
            (Some(b), Some(_)) if public(b) && !self.allow_public => {
                return bad(
                    "bind",
                    "a public address: VBAN has no password and no encryption. Use the station's address on its own network, carry VBAN through WireGuard, or set allow_public",
                );
            }
            _ => {}
        }
        let mut seen = Vec::new();
        for (i, v) in self.vban_in.iter().enumerate() {
            let f = |name: &str| format!("vban_in[{i}].{name}");
            let Ok(a) = v.address.trim().parse::<IpAddr>() else {
                return Err((f("address"), "an IP address".into()));
            };
            if a.is_unspecified() || a.is_multicast() {
                return Err((f("address"), "one sender's address".into()));
            }
            if public(&v.address) && !self.allow_public {
                return Err((
                    f("address"),
                    "a public address: VBAN is for the station's own network".into(),
                ));
            }
            if StreamName::new(&v.name).is_none() {
                return Err((f("name"), "1 to 16 characters of plain ASCII".into()));
            }
            if !key_id_ok(&v.key_id) {
                return Err((f("key_id"), "a show key's id (8 characters)".into()));
            }
            if v.channels.iter().any(|c| !(1..=8).contains(c)) {
                return Err((f("channels"), "two channels from 1 to 8".into()));
            }
            if seen.contains(&(a, v.name.as_str())) {
                return Err((f("name"), "that sender and name are already bound".into()));
            }
            seen.push((a, v.name.as_str()));
        }
        for (i, o) in self.vban_out.iter().enumerate() {
            let f = |name: &str| format!("vban_out[{i}].{name}");
            let Ok(a) = o.address.trim().parse::<IpAddr>() else {
                return Err((f("address"), "an IP address".into()));
            };
            let broadcast = match a {
                IpAddr::V4(v4) => v4.is_broadcast() || v4.octets()[3] == 255,
                IpAddr::V6(_) => false,
            };
            if a.is_unspecified() || a.is_multicast() || broadcast {
                return Err((
                    f("address"),
                    "one receiver's address: never broadcast or multicast".into(),
                ));
            }
            if public(&o.address) && !self.allow_public {
                return Err((
                    f("address"),
                    "a public address: VBAN is for the station's own network".into(),
                ));
            }
            if o.port == 0 {
                return Err((f("port"), "a port from 1 to 65535".into()));
            }
            if StreamName::new(&o.name).is_none() {
                return Err((f("name"), "1 to 16 characters of plain ASCII".into()));
            }
            if !out_source_ok(&o.source) {
                return Err((f("source"), OUT_SOURCES.into()));
            }
            if !matches!(o.format.as_str(), "s16" | "s24" | "f32") {
                return Err((f("format"), "`s16`, `s24` or `f32`".into()));
            }
        }
        Ok(())
    }

    /// The kit with `preset`'s routing applied, toward `address` (the studio PC, transmitter or
    /// guest machine); `Err` with why when it cannot be.
    pub fn with_preset(&self, preset: &str, address: Option<&str>) -> Result<Kit, String> {
        let mut k = self.clone();
        k.preset = preset.to_owned();
        let out = |address: &str, name: &str, source: String, format: &str| VbanOut {
            address: address.to_owned(),
            port: VBAN_PORT,
            name: name.to_owned(),
            source,
            format: format.to_owned(),
            enabled: true,
        };
        match preset {
            "custom" => {}
            "studio_lan" | "transmitter" => {
                let a = address.ok_or("the address to send to")?;
                let (name, source, format) = if preset == "studio_lan" {
                    ("Programme", "programme_live", "s16")
                } else {
                    ("Programme", "programme", "s24")
                };
                k.vban_out = vec![out(a, name, source.into(), format)];
            }
            "local_guest" => {
                let first = k
                    .vban_in
                    .first()
                    .ok_or("a VBAN in bound to the guest's show key first")?;
                let a = address.unwrap_or(&first.address).to_owned();
                k.vban_out = vec![out(
                    &a,
                    "Return",
                    format!("monitor:{}", first.key_id),
                    "s16",
                )];
            }
            _ => return Err("not a preset".into()),
        }
        Ok(k)
    }
}

// ---- the installation's word ----

/// Whether this installation may use VBAN at all: `Err` with why not.
pub fn allowed(file_allows: bool, cloudron: bool) -> Result<(), String> {
    if cloudron {
        return Err("VBAN is never offered on Cloudron: it is a protocol for one local network, and a Cloudron server is on the internet.".into());
    }
    if !file_allows {
        return Err("VBAN is off for this installation: set `[kit] vban = true` in the settings file (Compose: KIT_VBAN=true) and restart.".into());
    }
    Ok(())
}

/// The installation's word on VBAN, as the control plane started with it.
pub fn install_allows(state: &AppState) -> Result<(), String> {
    state.hub.sources.vban_allowed()
}

// ---- storage ----

/// The kit as stored; the defaults when there is no row or it cannot be read.
pub async fn load(
    pool: &sqlx::PgPool,
    station: weavesignal_model::StationId,
) -> Result<Kit, sqlx::Error> {
    let row = sqlx::query("SELECT body FROM kit_settings WHERE station_id = $1")
        .bind(station.as_uuid())
        .fetch_optional(pool)
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<serde_json::Value, _>("body").ok())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

async fn store(
    pool: &sqlx::PgPool,
    station: weavesignal_model::StationId,
    kit: &Kit,
    by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO kit_settings (station_id, body, updated_by, updated_ms) VALUES ($1, $2, $3, $4)
         ON CONFLICT (station_id) DO UPDATE SET body = $2, updated_by = $3, updated_ms = $4",
    )
    .bind(station.as_uuid())
    .bind(serde_json::to_value(kit).unwrap_or_default())
    .bind(by.chars().take(120).collect::<String>())
    .bind(now_ms() as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether a VBAN stream from `address` named `stream` is bound to `key_id`, and VBAN is
/// allowed here (the ingest's admission asks).
pub async fn binds(
    state: &AppState,
    pool: &sqlx::PgPool,
    key_id: &str,
    address: &str,
    stream: Option<&str>,
) -> bool {
    if install_allows(state).is_err() {
        return false;
    }
    let (Some(stream), Ok(addr)) = (stream, address.trim().parse::<IpAddr>()) else {
        return false;
    };
    let Ok(kit) = load(pool, state.station.id).await else {
        return false;
    };
    kit.vban_in.iter().any(|v| {
        v.key_id == key_id
            && v.name == stream
            && v.address.trim().parse::<IpAddr>().ok() == Some(addr)
    })
}

/// What the ingest needs for VBAN in: `None` when it should not listen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct IngestVban {
    pub bind: String,
    pub port: u16,
    pub rules: Vec<VbanIn>,
}

/// The ingest's VBAN settings now.
pub async fn for_ingest(state: &AppState, pool: &sqlx::PgPool) -> Option<IngestVban> {
    install_allows(state).ok()?;
    let kit = load(pool, state.station.id).await.ok()?;
    if kit.vban_in.is_empty() || kit.check().is_err() {
        return None;
    }
    Some(IngestVban {
        bind: kit.bind_addr()?.to_string(),
        port: kit.port,
        rules: kit.vban_in,
    })
}

/// The desk's request for VBAN out: `(set, request)`, the set a short digest of what it holds.
pub fn desk_request(kit: Option<&Kit>) -> (String, serde_json::Value) {
    let (bind, outs): (Option<String>, Vec<&VbanOut>) = match kit {
        Some(k) if k.check().is_ok() => (
            k.bind_addr().map(|a| a.to_string()),
            k.vban_out.iter().filter(|o| o.enabled).collect(),
        ),
        _ => (None, Vec::new()),
    };
    let outs: Vec<_> = outs
        .iter()
        .map(|o| {
            json!({"address": o.address.trim(), "port": o.port, "name": o.name, "source": o.source,
                   "format": DataType::from_name(&o.format).map_or("s16", DataType::name)})
        })
        .collect();
    let body = json!({"bind": bind, "outs": outs});
    let set = {
        use sha2::Digest;
        let d = sha2::Sha256::digest(body.to_string().as_bytes());
        d.iter()
            .take(6)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    (
        set.clone(),
        json!({"op": "kit", "set": set, "bind": body["bind"], "outs": body["outs"]}),
    )
}

/// Send the desk its VBAN outs if the set it holds is not the settings'.
pub async fn sync(state: &AppState) {
    let Some(pool) = state.db.as_ref() else {
        return;
    };
    let Some(desk) = state.hub.desk_status() else {
        return;
    };
    let kit = if install_allows(state).is_ok() {
        load(pool, state.station.id).await.ok()
    } else {
        None
    };
    let (set, request) = desk_request(kit.as_ref());
    let has = desk.kit.as_ref().and_then(|k| k.set.clone());
    if has.as_deref() == Some(set.as_str()) {
        return;
    }
    // A desk that has never been sent a set and has no outs to send needs nothing; one too
    // old to report a kit is not asked.
    if desk.kit.is_none() {
        return;
    }
    if let Err(e) = state.hub.desk.rtmp_set(request).await {
        tracing::warn!(error = %e, "kit: the desk did not take its VBAN outs; trying again shortly");
    } else {
        tracing::info!(set, "kit: the desk has its VBAN outs");
    }
}

/// Keep the desk's VBAN outs the settings', every few seconds.
pub fn spawn_sync(state: Arc<AppState>) {
    if state.db.is_none() || !state.hub.desk.configured() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            sync(&state).await;
        }
    });
}

// ---- routes ----

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/kit", get(get_kit).put(put_kit))
        .route("/api/v1/kit/preset", post(apply_preset))
        .route("/api/v1/kit/clock", get(clock))
}

/// A preset as the screen offers it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PresetInfo {
    pub name: String,
    pub words: String,
}

/// The kit as the screen shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct KitView {
    /// Whether this installation may use VBAN.
    pub allowed: bool,
    /// Why not, when it may not.
    pub why_not: Option<String>,
    /// A warning to show: the address is public.
    pub warning: Option<String>,
    pub kit: Kit,
    pub presets: Vec<PresetInfo>,
    /// What VBAN out may send.
    pub out_sources: String,
}

fn view(state: &AppState, kit: Kit) -> KitView {
    let allowed = install_allows(state);
    let warning = (kit.allow_public
        && kit
            .bind
            .as_deref()
            .is_some_and(|b| !is_private_address(b)))
    .then(|| {
        "VBAN uses a public address: anyone on the path can send to it or read what it sends. Carry it through WireGuard instead.".to_owned()
    });
    KitView {
        allowed: allowed.is_ok(),
        why_not: allowed.err(),
        warning,
        kit,
        presets: PRESETS
            .iter()
            .map(|(n, w)| PresetInfo {
                name: (*n).into(),
                words: (*w).into(),
            })
            .collect(),
        out_sources: OUT_SOURCES.into(),
    }
}

#[allow(clippy::result_large_err)]
fn pool(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the presenter kit needs the database",
        )
    })
}

fn field_problem(field: &str, why: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": why, "field": field })),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/kit",
    operation_id = "kit_get",
    tag = "kit",
    summary = "The presenter kit's settings",
    description = "VBAN in (each bound to a show key) and out, the address they use, the preset, and whether this installation may use VBAN at all (never on Cloudron; elsewhere only with `[kit] vban = true` in the settings file). Admin only.",
    responses(
        (status = 200, description = "The kit.", body = KitView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_kit(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match load(pool, state.station.id).await {
        Ok(k) => Json(view(&state, k)).into_response(),
        Err(e) => problem(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
    }
}

async fn save(state: &Arc<AppState>, headers: &HeaderMap, kit: Kit) -> Response {
    let actor = match state.check(headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match pool(state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    if let Err((field, why)) = kit.check() {
        return field_problem(&field, &why);
    }
    if kit.active()
        && let Err(why) = install_allows(state)
    {
        return problem(StatusCode::CONFLICT, why);
    }
    if kit.allow_public && kit.bind.as_deref().is_some_and(|b| !is_private_address(b)) {
        tracing::warn!(bind = ?kit.bind, "kit: VBAN on a public address (allow_public): unauthenticated and unencrypted");
    }
    if let Err(e) = store(pool, state.station.id, &kit, &actor.label()).await {
        return problem(StatusCode::SERVICE_UNAVAILABLE, e.to_string());
    }
    let s = state.clone();
    tokio::spawn(async move { sync(&s).await });
    Json(view(state, kit)).into_response()
}

#[utoipa::path(
    put,
    path = "/api/v1/kit",
    operation_id = "kit_put",
    tag = "kit",
    summary = "Change the presenter kit's settings",
    description = "The whole kit at once, checked: at most four VBAN ins and four outs; a named address of the station's, refused when public unless `allow_public`; each input a sender's address and stream name bound to a show key; each output one receiver (never broadcast or multicast). The ingest listens afresh and the desk sends afresh within seconds. 409 when the kit asks for VBAN and this installation does not allow it. Admin only.",
    request_body = Kit,
    responses(
        (status = 200, description = "The kit now.", body = KitView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_kit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(kit): Json<Kit>,
) -> Response {
    save(&state, &headers, kit).await
}

/// A preset to apply.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetRequest {
    /// `studio_lan`, `transmitter`, `local_guest` or `custom`.
    pub preset: String,
    /// Where its output goes (the studio PC, the transmitter); for `local_guest`, the first
    /// VBAN in's sender when left out.
    #[serde(default)]
    pub address: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/kit/preset",
    operation_id = "kit_preset",
    tag = "kit",
    summary = "Apply a preset",
    description = "A radio preset's routing in one press: `studio_lan` sends the live programme (before the delay) back to the studio PC; `transmitter` sends the programme after the delay, what listeners hear; `local_guest` sends the first VBAN in's sender the monitor mix the desk makes for it; `custom` changes nothing but the name. VBAN ins are kept (each needs its show key). Checked and saved as `PUT` is. Admin only.",
    request_body = PresetRequest,
    responses(
        (status = 200, description = "The kit now.", body = KitView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn apply_preset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PresetRequest>,
) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let kit = match load(pool, state.station.id).await {
        Ok(k) => k,
        Err(e) => return problem(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
    };
    match kit.with_preset(&req.preset, req.address.as_deref()) {
        Ok(k) => save(&state, &headers, k).await,
        Err(why) => field_problem("preset", &why),
    }
}

/// One source's clock, as the desk follows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SourceClock {
    pub identity: String,
    pub name: String,
    pub way: String,
    /// Its clock against the desk's, in parts per million; `None` before the first estimate
    /// (about 20 s), or from a desk that does not follow.
    pub ppm: Option<f32>,
    /// Times it was placed afresh (a gap, or drift past the cushion) since it connected.
    pub replaced: u64,
    /// For VBAN: packets lost on the way and filled with silence.
    pub lost: u64,
}

/// The clock badge's figures.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Clock {
    /// The desk's rate: everything is resampled to it once, at the desk's edge.
    pub rate: u32,
    pub sources: Vec<SourceClock>,
    /// VBAN packets the ingest dropped, by reason (`address`, `name`, `late`, `too_fast`, …).
    pub vban_dropped: std::collections::BTreeMap<String, u64>,
    /// VBAN out, as the desk reports it.
    pub vban_out: Vec<crate::desk::DeskKitOut>,
}

#[utoipa::path(
    get,
    path = "/api/v1/kit/clock",
    operation_id = "kit_clock",
    tag = "kit",
    summary = "One clock: each source against the desk's",
    description = "For the clock badge: every host's own studio at the desk with its clock against the desk's in ppm (the desk resamples it once to follow it), the times it was placed afresh, VBAN's lost and dropped packets, and VBAN out's counts. Any staff member.",
    responses(
        (status = 200, description = "The figures now.", body = Clock),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn clock(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    let desk = state.hub.desk_status();
    let sources = desk
        .as_ref()
        .map(|d| source_clocks(&d.people, &state.hub.sources.connections()))
        .unwrap_or_default();
    Json(Clock {
        rate: 48_000,
        sources,
        vban_dropped: state.hub.sources.vban_dropped(),
        vban_out: desk.and_then(|d| d.kit.map(|k| k.outs)).unwrap_or_default(),
    })
    .into_response()
}

/// Each source at the desk with its clock. VBAN's lost packets are counted where they arrive, in
/// the ingest, and joined on the show key; a desk's own count (none yet) is kept if larger.
fn source_clocks(
    people: &[crate::desk::DeskPerson],
    conns: &[crate::sources::IngestConn],
) -> Vec<SourceClock> {
    people
        .iter()
        .filter_map(|p| {
            let s = p.source.as_ref()?;
            let ingest_lost = conns
                .iter()
                .find(|c| c.key_id == s.key)
                .and_then(|c| c.lost)
                .unwrap_or(0);
            Some(SourceClock {
                identity: p.identity.clone(),
                name: p.name.clone(),
                way: s.way.clone(),
                ppm: s.clock_ppm,
                replaced: s.replaced,
                lost: s.lost.max(ingest_lost),
            })
        })
        .collect()
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(get_kit, put_kit, apply_preset, clock))]
pub struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    fn lan() -> Kit {
        Kit {
            bind: Some("192.168.1.10".into()),
            vban_in: vec![VbanIn {
                address: "192.168.1.20".into(),
                name: "Stream1".into(),
                key_id: "abcd2345".into(),
                channels: [1, 2],
            }],
            ..Kit::default()
        }
    }

    fn field(k: &Kit) -> String {
        k.check().unwrap_err().0
    }

    #[test]
    fn the_default_is_off_and_checks() {
        let k = Kit::default();
        assert!(!k.active());
        assert!(k.check().is_ok());
        assert_eq!(k.port, 6980);
        let parsed: Kit = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, k);
    }

    #[test]
    fn a_kit_on_the_lan_checks_and_a_public_one_needs_allow_public() {
        assert!(lan().check().is_ok());
        let mut k = lan();
        k.bind = None;
        assert_eq!(field(&k), "bind");
        k.bind = Some("0.0.0.0".into());
        assert_eq!(field(&k), "bind");
        k.bind = Some("203.0.113.5".into());
        assert_eq!(field(&k), "bind");
        k.allow_public = true;
        assert!(k.check().is_ok());
        let mut k = lan();
        k.vban_in[0].address = "198.51.100.7".into();
        assert_eq!(field(&k), "vban_in[0].address");
    }

    #[test]
    fn every_field_is_checked() {
        let mut k = lan();
        k.vban_in[0].name = "A name far too long".into();
        assert_eq!(field(&k), "vban_in[0].name");
        let mut k = lan();
        k.vban_in[0].key_id = "ABCD2345".into();
        assert_eq!(field(&k), "vban_in[0].key_id");
        let mut k = lan();
        k.vban_in[0].channels = [1, 9];
        assert_eq!(field(&k), "vban_in[0].channels");
        let mut k = lan();
        k.vban_in.push(k.vban_in[0].clone());
        assert_eq!(field(&k), "vban_in[1].name");
        let mut k = lan();
        k.vban_in = vec![k.vban_in[0].clone(); 5];
        assert_eq!(field(&k), "vban_in");
        let out = |address: &str, source: &str, format: &str| VbanOut {
            address: address.into(),
            port: 6980,
            name: "Programme".into(),
            source: source.into(),
            format: format.into(),
            enabled: true,
        };
        for (o, f) in [
            (
                out("192.168.1.255", "programme", "s16"),
                "vban_out[0].address",
            ),
            (out("239.1.1.1", "programme", "s16"), "vban_out[0].address"),
            (
                out("192.168.1.30", "everything", "s16"),
                "vban_out[0].source",
            ),
            (
                out("192.168.1.30", "monitor:nope", "s16"),
                "vban_out[0].source",
            ),
            (out("192.168.1.30", "programme", "u8"), "vban_out[0].format"),
        ] {
            let mut k = lan();
            k.vban_out = vec![o];
            assert_eq!(field(&k), f);
        }
        for s in [
            "programme",
            "programme_live",
            "monitor:abcd2345",
            "mix_minus:hamish",
        ] {
            assert!(out_source_ok(s), "{s}");
        }
        let mut k = lan();
        k.preset = "loud".into();
        assert_eq!(field(&k), "preset");
    }

    #[test]
    fn presets_set_their_routing() {
        let k = lan()
            .with_preset("transmitter", Some("192.168.1.40"))
            .unwrap();
        assert_eq!(k.vban_out[0].source, "programme");
        assert_eq!(k.vban_out[0].format, "s24");
        assert!(k.check().is_ok());
        let k = lan()
            .with_preset("studio_lan", Some("192.168.1.20"))
            .unwrap();
        assert_eq!(k.vban_out[0].source, "programme_live");
        assert_eq!(k.vban_in.len(), 1, "inputs are kept");
        let k = lan().with_preset("local_guest", None).unwrap();
        assert_eq!(k.vban_out[0].address, "192.168.1.20");
        assert_eq!(k.vban_out[0].source, "monitor:abcd2345");
        assert!(Kit::default().with_preset("local_guest", None).is_err());
        assert!(lan().with_preset("transmitter", None).is_err());
        assert_eq!(lan().with_preset("custom", None).unwrap().vban_out, vec![]);
    }

    #[test]
    fn never_on_cloudron_and_off_unless_the_file_says() {
        assert!(allowed(true, true).unwrap_err().contains("Cloudron"));
        assert!(allowed(false, false).unwrap_err().contains("[kit] vban"));
        assert!(allowed(true, false).is_ok());
    }

    #[test]
    fn the_desk_is_sent_nothing_for_a_kit_that_does_not_check() {
        let (_, none) = desk_request(None);
        assert_eq!(none["outs"], json!([]));
        let mut k = lan()
            .with_preset("transmitter", Some("192.168.1.40"))
            .unwrap();
        let (set, r) = desk_request(Some(&k));
        assert_eq!(r["op"], "kit");
        assert_eq!(r["bind"], "192.168.1.10");
        assert_eq!(r["outs"][0]["format"], "s24");
        k.vban_out[0].enabled = false;
        let (set2, r) = desk_request(Some(&k));
        assert_ne!(set, set2);
        assert_eq!(r["outs"], json!([]));
        k.bind = Some("203.0.113.5".into());
        assert_eq!(desk_request(Some(&k)).1["bind"], json!(null));
    }

    #[test]
    fn the_clock_readout_takes_vbans_lost_packets_from_the_ingest_by_key() {
        let person = |id: &str, way: &str, key: &str| -> crate::desk::DeskPerson {
            serde_json::from_value(json!({
                "identity": id, "name": "Isla", "role": "source", "on_air": false,
                "talk": false, "present": true, "linked": true,
                "source": {"way": way, "format": "vban 24-bit 48 kHz stereo", "key": key,
                    "show": "the-blether", "connected_ms": 1000, "leveller_db": 0.0,
                    "replaced": 1}
            }))
            .unwrap()
        };
        let conn = |key: &str, way: &str, lost: Option<u64>| -> crate::sources::IngestConn {
            serde_json::from_value(json!({
                "key_id": key, "mode": "standby", "way": way, "since_ms": 1, "lost": lost,
            }))
            .unwrap()
        };
        let people = [
            person("source-abcd2345", "vban", "abcd2345"),
            person("source-efgh6789", "srt", "efgh6789"),
        ];
        let conns = [
            conn("efgh6789", "srt", None),
            conn("abcd2345", "vban", Some(7)),
        ];
        let c = source_clocks(&people, &conns);
        assert_eq!(
            (c[0].way.as_str(), c[0].lost, c[0].replaced),
            ("vban", 7, 1)
        );
        assert_eq!((c[1].way.as_str(), c[1].lost), ("srt", 0));
        // The ingest's report gone stale: nothing to join, nothing invented.
        assert_eq!(source_clocks(&people, &[])[0].lost, 0);
    }
}
