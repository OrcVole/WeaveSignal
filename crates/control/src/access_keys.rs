// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows, the station's side (1.3.2; `docs/reference/design/keyed-shows.md`): making and
//! changing listener keys, key families, the leak report, and each show's access rule.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/listener-keys` | key manager | keys, filtered; never their text |
//! | `POST /api/v1/listener-keys` | key manager | make a key; its text is in the answer |
//! | `POST /api/v1/listener-keys/find` | key manager | which key a text belongs to (a leaked copy: whose was it) |
//! | `GET`, `PATCH /api/v1/listener-keys/{id}` | key manager | one key; change it in place |
//! | `DELETE /api/v1/listener-keys/{id}` | admin | erase it and everything kept about its use |
//! | `POST /api/v1/listener-keys/{id}/extend` | key manager | later end (by days, or to a date) |
//! | `POST /api/v1/listener-keys/{id}/pause`, `resume`, `revoke` | key manager | switch off for now, back on, or end for good |
//! | `POST /api/v1/listener-keys/{id}/replace` | key manager | revoke it and issue a new key to the same recipient, in one call |
//! | `POST /api/v1/listener-keys/{id}/text` | key manager | the key's text and link again (logged) |
//! | `GET /api/v1/listener-keys/{id}/log`, `devices` | key manager | what changed; who holds it |
//! | `DELETE /api/v1/listener-keys/{id}/devices/{device}` | key manager | sign a device out |
//! | `GET`, `POST /api/v1/listener-keys/families` | key manager | families; issue one key per recipient |
//! | `GET /api/v1/listener-keys/families/{id}`, `/export` | key manager | a family; its keys as CSV with links |
//! | `GET /api/v1/listener-keys/leaks` | key manager | the leak report, most likely passed on first |
//! | `GET /api/v1/listener-keys/signing` | key manager | how tokens are signed, and the public key an edge checks them with |
//! | `GET`, `PUT /api/v1/shows/{show}/access` | key manager (PUT: staff only) | a show's access rule |
//!
//! **Who may.** A *key manager* is a producer, an admin, the station key, or an API token holding
//! `keys:manage` ([`crate::scopes`]), which opens these routes and no others. Keys hold people's
//! names and emails and the means to listen, so a presenter or a screener is refused (403). Two
//! things are kept narrower: erasing a key is an admin's (`x-admin`, never a token's), and
//! changing a show's access rule is a person's, never a token's: a shop can make keys, it cannot
//! decide which shows are keyed.
//!
//! **Never in a list or a log:** a key's text. It is in the answer to making a key (once), to the
//! reveal route (logged as `text_shown`, by whom), to a family's export and to a replacement.

// The `Err` of these helpers is the response to send, built once per refused request.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::listener_keys::{Key, Kind, Media, State as KeyState};
use weavesignal_model::listener_keys_db::{
    self as db, Access, Change, Device, Family, Filter, Leak, LogEntry, Move,
};

use crate::access::{self, Spec, SpecError};
use crate::apikey::ApiKey;
use crate::app::{AppState, problem};
use crate::events::KEY_ENDED;
use crate::hub::now_ms;
use crate::staff::{Actor, Need};

/// The most keys one call to make a family issues.
const MAX_FAMILY: usize = 1_000;
/// The most keys one list returns.
const MAX_LIST: i64 = 500;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/listener-keys", get(list).post(create))
        .route("/api/v1/listener-keys/find", post(find))
        .route("/api/v1/listener-keys/leaks", get(leaks))
        .route("/api/v1/listener-keys/signing", get(signing))
        .route(
            "/api/v1/listener-keys/families",
            get(families).post(make_family),
        )
        .route("/api/v1/listener-keys/families/{id}", get(one_family))
        .route(
            "/api/v1/listener-keys/families/{id}/export",
            get(export_family),
        )
        .route(
            "/api/v1/listener-keys/{id}",
            get(one).patch(change).delete(erase),
        )
        .route("/api/v1/listener-keys/{id}/extend", post(extend))
        .route("/api/v1/listener-keys/{id}/pause", post(pause))
        .route("/api/v1/listener-keys/{id}/resume", post(resume))
        .route("/api/v1/listener-keys/{id}/revoke", post(revoke))
        .route("/api/v1/listener-keys/{id}/replace", post(replace))
        .route("/api/v1/listener-keys/{id}/text", post(reveal))
        .route("/api/v1/listener-keys/{id}/log", get(read_log))
        .route("/api/v1/listener-keys/{id}/devices", get(devices))
        .route(
            "/api/v1/listener-keys/{id}/devices/{device}",
            delete(end_device),
        )
        .route(
            "/api/v1/shows/{show}/access",
            get(get_access).put(put_access),
        )
}

// ---- the shared parts ----

struct Ctx<'a> {
    by: String,
    pool: &'a sqlx::PgPool,
    key: &'a ApiKey,
}

/// A producer, an admin, the station key, or a token the gate granted `keys:manage`; and what
/// keys need: the database and the station's key. A presenter or screener is refused.
fn ready<'a>(state: &'a AppState, headers: &HeaderMap, need: Need) -> Result<Ctx<'a>, Response> {
    let actor = state.check(headers, need)?;
    if matches!(actor, Actor::Staff(_)) && !actor.is_producer_or_admin() {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Only a producer or an admin can manage keys. Ask the station's admin.",
        ));
    }
    let Some(pool) = state.db.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Keys are kept in the database.",
        ));
    };
    let Some(key) = state.staff.api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "Keys need a station key to seal them under: set {} (or the station key).",
                ApiKey::ENV
            ),
        ));
    };
    Ok(Ctx {
        by: actor.label(),
        pool,
        key,
    })
}

fn db_problem(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "keyed shows: database error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "the database refused that",
    )
}

fn field(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": detail.into(), "field": field })),
    )
        .into_response()
}

fn spec_problem(e: SpecError) -> Response {
    field(e.field, e.message)
}

fn not_found() -> Response {
    problem(StatusCode::NOT_FOUND, "no such key")
}

/// A field that may be left out (keep it, or take the default), `null` (none), or a value.
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// What the answer to making a key carries.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyMade)]
pub struct Made {
    pub key: Key,
    /// The key's text, written `KETM-7QMP-2D4X`: read it out, print it, type it in.
    pub text: String,
    /// The link that opens the player with the key entered (the key is in the fragment).
    pub link: String,
}

fn made(state: &AppState, key: Key, text: &str) -> Made {
    Made {
        link: access::link(&state.join.public_url, text),
        text: weavesignal_model::listener_keys::format_text(text),
        key,
    }
}

/// Resolve show names (ids or slugs) to ids; an unknown one is a 422 naming it.
async fn resolve_shows(
    state: &AppState,
    c: &Ctx<'_>,
    names: &[String],
) -> Result<Vec<Uuid>, Response> {
    let mut ids = Vec::new();
    for n in names {
        let n = n.trim();
        if let Ok(id) = Uuid::parse_str(n) {
            ids.push(id);
        } else {
            match weavesignal_model::shows::get_show(c.pool, state.station.id, n).await {
                Ok(Some(s)) => ids.push(s.id),
                Ok(None) => return Err(field("shows", format!("There is no show `{n}`."))),
                Err(e) => return Err(db_problem(e)),
            }
        }
    }
    match access::missing_shows(c.pool, state.station.id, &ids).await {
        Ok(m) if m.is_empty() => Ok(ids),
        Ok(m) => Err(field("shows", format!("There is no show `{}`.", m[0]))),
        Err(e) => Err(db_problem(e)),
    }
}

// ---- making keys ----

/// A new key.
#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyNew)]
#[serde(deny_unknown_fields)]
struct NewBody {
    /// `personal` (the default), `group` or `machine`.
    #[serde(default = "personal")]
    kind: Kind,
    /// Who it is for: a name, a group, a partner, a ticket number (1 to 120 characters).
    label: String,
    /// A note for staff (up to 500 characters).
    #[serde(default)]
    note: String,
    /// The email the lost-key request matches and the key can be sent to; kept sealed.
    #[serde(default)]
    email: Option<String>,
    /// `sound`, `video` or `both` (the default).
    #[serde(default = "both")]
    media: Media,
    /// Opens every keyed show. Left out: true when no `shows` are named.
    #[serde(default)]
    all_shows: Option<bool>,
    /// The shows it opens, by id or slug (at most 32).
    #[serde(default)]
    shows: Vec<String>,
    #[serde(default)]
    valid_from_ms: Option<i64>,
    #[serde(default)]
    valid_until_ms: Option<i64>,
    /// Browsers or apps at once. Left out: 2 (a machine key: none). `null`: any number.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<u32>)]
    max_devices: Option<Option<u32>>,
    #[serde(default)]
    max_listens: Option<u32>,
    /// A group key's people at most.
    #[serde(default)]
    max_people: Option<u32>,
    /// A family to issue it in.
    #[serde(default)]
    family: Option<Uuid>,
}

fn personal() -> Kind {
    Kind::Personal
}

fn both() -> Media {
    Media::Both
}

fn spec_of(body: &NewBody, shows: Vec<Uuid>) -> Spec {
    let all = body.all_shows.unwrap_or(shows.is_empty());
    Spec {
        kind: body.kind,
        label: body.label.clone(),
        note: body.note.clone(),
        email: body.email.clone(),
        media: body.media,
        all_shows: all,
        shows,
        valid_from_ms: body.valid_from_ms,
        valid_until_ms: body.valid_until_ms,
        max_devices: match body.max_devices {
            Some(v) => v,
            None if body.kind == Kind::Machine => None,
            None => Some(2),
        },
        max_listens: body.max_listens,
        max_people: body.max_people,
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys",
    operation_id = "listener_keys_create",
    tag = "keys",
    summary = "Make a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. `label` names who it is for. `kind` is `personal` (default), `group` or `machine`; `media` `sound`, `video` or `both` (default); `shows` names the shows it opens by id or slug (at most 32), or `all_shows` opens every keyed show (the default when no show is named). `valid_from_ms` and `valid_until_ms` are the window (inclusive, exclusive). Limits: `max_devices` (default 2; null for any), `max_listens`, and for a group key `max_people`. The answer carries the key's text this once; the station keeps it sealed, and the reveal route shows it again. Make the key for a family with `family`.",
    request_body = NewBody,
    responses(
        (status = 201, description = "The key, its text and its link.", body = Made),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<NewBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let shows = match resolve_shows(&state, &c, &body.shows).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let mut spec = spec_of(&body, shows);
    if let Err(e) = spec.check() {
        return spec_problem(e);
    }
    if let Some(f) = body.family {
        match db::get_family(c.pool, state.station.id, f).await {
            Ok(Some(_)) => {}
            Ok(None) => return field("family", "There is no such family."),
            Err(e) => return db_problem(e),
        }
    }
    let result = async {
        let mut tx = c.pool.begin().await?;
        let r = access::make_key(
            &mut tx,
            state.station.id,
            c.key,
            &spec,
            body.family,
            None,
            &c.by,
        )
        .await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(r)
    }
    .await;
    match result {
        Ok((key, text)) => {
            tracing::info!(key = %key.id, by = c.by, kind = key.kind.as_str(), "listener key made");
            (StatusCode::CREATED, Json(made(&state, key, &text))).into_response()
        }
        Err(e) => db_problem(e),
    }
}

// ---- listing and reading ----

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct ListQuery {
    /// Only this family's keys.
    family: Option<Uuid>,
    /// `active`, `paused` or `revoked`.
    state: Option<KeyState>,
    /// `personal`, `group` or `machine`.
    kind: Option<Kind>,
    /// Only keys that open this show (id or slug).
    show: Option<String>,
    /// A part of the label or note, case ignored.
    q: Option<String>,
    /// How many, 1 to 500 (default 100).
    limit: Option<i64>,
    /// How many to skip.
    offset: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyList)]
struct Listed {
    keys: Vec<Key>,
    /// How many keys match, before `limit`.
    total: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys",
    operation_id = "listener_keys_list",
    tag = "keys",
    summary = "Listener keys",
    description = "A producer, an admin, or an API token with `keys:manage`. Keys, newest first, narrowed by family, state, kind, a show it opens, or a part of the label or note. Never a key's text.",
    params(ListQuery),
    responses(
        (status = 200, description = "The keys, and how many match.", body = Listed),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let show = match &q.show {
        None => None,
        Some(s) => match resolve_shows(&state, &c, std::slice::from_ref(s)).await {
            Ok(ids) => ids.first().copied(),
            Err(r) => return r,
        },
    };
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    match db::list_keys(
        c.pool,
        state.station.id,
        &Filter {
            family: q.family,
            state: q.state,
            kind: q.kind,
            show,
            text,
            limit: q.limit.unwrap_or(100).clamp(1, MAX_LIST),
            offset: q.offset.unwrap_or(0).max(0),
        },
    )
    .await
    {
        Ok((keys, total)) => Json(Listed { keys, total }).into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/{id}",
    operation_id = "listener_keys_get",
    tag = "keys",
    summary = "One listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. The key as the list shows it; never its text.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 200, description = "The key.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn one(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::get_key(c.pool, state.station.id, id).await {
        Ok(Some(k)) => Json(k).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyFind)]
#[serde(deny_unknown_fields)]
struct FindBody {
    /// A key's text as it was found: any case, with or without dashes.
    text: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/find",
    operation_id = "listener_keys_find",
    tag = "keys",
    summary = "Which key is this text?",
    description = "A producer, an admin, or an API token with `keys:manage`. For a key found where it should not be (a post, a forum, a leaked list): whose key it was, and which family issued it. 404 when it is no key of this station.",
    request_body = FindBody,
    responses(
        (status = 200, description = "The key.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn find(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<FindBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let norm = weavesignal_model::listener_keys::normalise_text(&body.text);
    match db::find_by_text(c.pool, state.station.id, &c.key.listener_text_hash(&norm)).await {
        Ok(Some(k)) => Json(k).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_problem(e),
    }
}

// ---- changing keys ----

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyChange)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    label: Option<String>,
    note: Option<String>,
    /// A new email; `null` takes it away.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    email: Option<Option<String>>,
    media: Option<Media>,
    all_shows: Option<bool>,
    /// The shows it opens, by id or slug; replaces the set.
    shows: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<i64>)]
    valid_from_ms: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<i64>)]
    valid_until_ms: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<u32>)]
    max_devices: Option<Option<u32>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<u32>)]
    max_listens: Option<Option<u32>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<u32>)]
    max_people: Option<Option<u32>>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/listener-keys/{id}",
    operation_id = "listener_keys_change",
    tag = "keys",
    summary = "Change a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. Only the fields sent change; a window or limit sent as `null` is taken away. The key's text stays the same, so nothing needs sending again: the shows it opens, its window and its limits take effect for the holder at once (within a token's life). A revoked key cannot be changed (409).",
    params(("id" = Uuid, Path, description = "The key's id.")),
    request_body = ChangeBody,
    responses(
        (status = 200, description = "The key as changed.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn change(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<ChangeBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let cur = match db::get_key(c.pool, station, id).await {
        Ok(Some(k)) => k,
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    };
    if cur.state == KeyState::Revoked {
        return problem(
            StatusCode::CONFLICT,
            "That key was ended for good. Replace it to give the person a new one.",
        );
    }
    // The change as a spec on top of the key, so one set of rules checks both.
    let mut spec = Spec::of_key(&cur, None);
    if let Some(l) = &body.label {
        spec.label = l.clone();
    }
    if let Some(n) = &body.note {
        spec.note = n.clone();
    }
    if let Some(Some(e)) = &body.email {
        spec.email = Some(e.clone());
    }
    if let Some(m) = body.media {
        spec.media = m;
    }
    let shows = match &body.shows {
        Some(names) => match resolve_shows(&state, &c, names).await {
            Ok(s) => Some(s),
            Err(r) => return r,
        },
        None => None,
    };
    if let Some(s) = &shows {
        spec.shows = s.clone();
        spec.all_shows = body.all_shows.unwrap_or(false);
    } else if let Some(a) = body.all_shows {
        spec.all_shows = a;
        if a {
            spec.shows.clear();
        }
    }
    if let Some(v) = body.valid_from_ms {
        spec.valid_from_ms = v;
    }
    if let Some(v) = body.valid_until_ms {
        spec.valid_until_ms = v;
    }
    if let Some(v) = body.max_devices {
        spec.max_devices = v;
    }
    if let Some(v) = body.max_listens {
        spec.max_listens = v;
    }
    if let Some(v) = body.max_people {
        spec.max_people = v;
    }
    if let Err(e) = spec.check() {
        return spec_problem(e);
    }
    let email = match &body.email {
        None => None,
        Some(None) => Some(None),
        Some(Some(_)) => spec.email.as_deref().map(|e| {
            let (n, s) = access::seal_email(c.key, id, e);
            Some((c.key.listener_email_hash(e), n, s))
        }),
    };
    let change = Change {
        label: body.label.as_ref().map(|_| spec.label.as_str()),
        note: body.note.as_ref().map(|_| spec.note.as_str()),
        email: email
            .as_ref()
            .map(|e| e.as_ref().map(|(h, n, s)| (h, &n[..], &s[..]))),
        media: body.media,
        all_shows: (shows.is_some() || body.all_shows.is_some()).then_some(spec.all_shows),
        shows: (shows.is_some() || body.all_shows == Some(true)).then_some(&spec.shows[..]),
        valid_from_ms: body.valid_from_ms,
        valid_until_ms: body.valid_until_ms,
        max_devices: body.max_devices,
        max_listens: body.max_listens,
        max_people: body.max_people,
    };
    let detail = changed_fields(&body);
    let result = async {
        let mut tx = c.pool.begin().await?;
        let k = db::change_key(&mut tx, station, id, &change, &c.by, detail, "changed").await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(k)
    }
    .await;
    match result {
        Ok(Some(k)) => Json(k).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_problem(e),
    }
}

/// The names of the fields a change sent, for the log (never their values: a label and an
/// email are personal data and the log outlives the key's holder).
fn changed_fields(b: &ChangeBody) -> serde_json::Value {
    let mut f = Vec::new();
    for (name, set) in [
        ("label", b.label.is_some()),
        ("note", b.note.is_some()),
        ("email", b.email.is_some()),
        ("media", b.media.is_some()),
        ("all_shows", b.all_shows.is_some()),
        ("shows", b.shows.is_some()),
        ("valid_from", b.valid_from_ms.is_some()),
        ("valid_until", b.valid_until_ms.is_some()),
        ("max_devices", b.max_devices.is_some()),
        ("max_listens", b.max_listens.is_some()),
        ("max_people", b.max_people.is_some()),
    ] {
        if set {
            f.push(name);
        }
    }
    json!({ "fields": f })
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyExtend)]
#[serde(deny_unknown_fields)]
struct ExtendBody {
    /// Days to add to the end (or to now, if it has already ended), 1 to 3650.
    #[serde(default)]
    by_days: Option<u32>,
    /// Or the new end, as Unix milliseconds.
    #[serde(default)]
    until_ms: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/extend",
    operation_id = "listener_keys_extend",
    tag = "keys",
    summary = "Extend a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. Send `by_days` (added to the end, or to now if it has ended) or `until_ms`, not both. A key with no end has nothing to extend (422); a revoked key cannot be extended (409). An ended key that is extended works again and may send `key.ended` again later.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    request_body = ExtendBody,
    responses(
        (status = 200, description = "The key with its new end.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn extend(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<ExtendBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let cur = match db::get_key(c.pool, station, id).await {
        Ok(Some(k)) => k,
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    };
    if cur.state == KeyState::Revoked {
        return problem(StatusCode::CONFLICT, "That key was ended for good.");
    }
    let now = now_ms() as i64;
    let until = match (body.by_days, body.until_ms) {
        (Some(d), None) if (1..=3650).contains(&d) => {
            let Some(end) = cur.valid_until_ms else {
                return field(
                    "by_days",
                    "That key has no end, so there is nothing to extend.",
                );
            };
            end.max(now) + i64::from(d) * 86_400_000
        }
        (Some(_), None) => return field("by_days", "Extend by 1 to 3650 days."),
        (None, Some(u)) => u,
        _ => return field("until_ms", "Send `by_days` or `until_ms`, one of them."),
    };
    if cur.valid_from_ms.is_some_and(|f| until <= f) {
        return field("until_ms", "A key must end after it starts.");
    }
    let change = Change {
        valid_until_ms: Some(Some(until)),
        ..Default::default()
    };
    let result = async {
        let mut tx = c.pool.begin().await?;
        let k = db::change_key(
            &mut tx,
            station,
            id,
            &change,
            &c.by,
            json!({ "from_ms": cur.valid_until_ms, "to_ms": until }),
            "extended",
        )
        .await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(k)
    }
    .await;
    match result {
        Ok(Some(k)) => Json(k).into_response(),
        Ok(None) => not_found(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema, Default)]
#[schema(as = ListenerKeyReason)]
#[serde(deny_unknown_fields, default)]
struct ReasonBody {
    /// Why, for the log and (for a revoked key) the holder's page: at most 200 characters.
    reason: String,
}

/// Pause, resume or revoke, with the event a revoke sends.
async fn do_move(
    state: Arc<AppState>,
    headers: HeaderMap,
    id: Uuid,
    mv: Move,
    body: axum::body::Bytes,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    // No body at all is no reason; a body is the JSON the route says.
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        String::new()
    } else {
        match serde_json::from_slice::<ReasonBody>(&body) {
            Ok(b) => b.reason,
            Err(_) => return field("reason", "Send `{\"reason\": \"…\"}` or nothing."),
        }
    };
    let Some(reason) = access::clean_line(&reason, 200) else {
        return field("reason", "A reason is at most 200 characters on one line.");
    };
    let station = state.station.id;
    let result = async {
        let mut tx = c.pool.begin().await?;
        let m = db::move_key(&mut tx, station, id, mv, &reason, &c.by).await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(m)
    }
    .await;
    match result {
        Ok(Some(m)) if m.unchanged && m.key.state == KeyState::Revoked && mv != Move::Revoke => {
            problem(StatusCode::CONFLICT, "That key was ended for good.")
        }
        Ok(Some(m)) => {
            if m.ended_now {
                state.staff.api.events.publish(
                    KEY_ENDED,
                    json!({ "key": m.key.id, "label": m.key.label, "family": m.key.family_id,
                            "kind": m.key.kind.as_str(), "reason": "revoked" }),
                );
            }
            tracing::info!(key = %id, by = c.by, action = ?mv, "listener key moved");
            Json(m.key).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/pause",
    operation_id = "listener_keys_pause",
    tag = "keys",
    summary = "Pause a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. Switches it off for now and keeps it (a late payment): the holder is cut off within a token's life and told it is switched off. Resume brings it back. Pausing a key already paused changes nothing; a revoked key cannot be paused (409).",
    params(("id" = Uuid, Path, description = "The key's id.")),
    request_body(content = ReasonBody, description = "Optionally why."),
    responses(
        (status = 200, description = "The key.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn pause(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    do_move(state, headers, id, Move::Pause, body).await
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/resume",
    operation_id = "listener_keys_resume",
    tag = "keys",
    summary = "Resume a paused listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. Brings a paused key back; its holder can listen again at their next renewal. A revoked key cannot be resumed (409).",
    params(("id" = Uuid, Path, description = "The key's id.")),
    request_body(content = ReasonBody, description = "Optionally why."),
    responses(
        (status = 200, description = "The key.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn resume(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    do_move(state, headers, id, Move::Resume, body).await
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/revoke",
    operation_id = "listener_keys_revoke",
    tag = "keys",
    summary = "Revoke a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. Ends it for good: the holder is cut off within a token's life and told it was ended by the station. Sends `key.ended` to webhooks (once). Revoking a key already revoked changes nothing.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    request_body(content = ReasonBody, description = "Optionally why."),
    responses(
        (status = 200, description = "The key.", body = Key),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    do_move(state, headers, id, Move::Revoke, body).await
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyReplaced)]
struct Replaced {
    /// The key that was revoked.
    old: Key,
    /// The new key, for the same recipient.
    new: Key,
    text: String,
    link: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/replace",
    operation_id = "listener_keys_replace",
    tag = "keys",
    summary = "Replace a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. For a leak, a lost key or a rotation: in one step the key is revoked (reason `replaced`, `key.ended` sent) and a new key with a new text is issued to the same recipient: the same name, note, email, family, shows, window and limits, and the listens already used. Every other key stays alive. A revoked key cannot be replaced (409).",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 200, description = "The old key, the new key and its text.", body = Replaced),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn replace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let cur = match db::get_key(c.pool, station, id).await {
        Ok(Some(k)) => k,
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    };
    if cur.state == KeyState::Revoked {
        return problem(
            StatusCode::CONFLICT,
            "That key was ended for good, so it cannot be replaced; make a new key.",
        );
    }
    let email = match db::sealed_email(c.pool, station, id).await {
        Ok(Some((n, s))) => access::open_email(c.key, id, &n, &s),
        Ok(None) => None,
        Err(e) => return db_problem(e),
    };
    let spec = Spec::of_key(&cur, email);
    let result = async {
        let mut tx = c.pool.begin().await?;
        let moved = db::move_key(&mut tx, station, id, Move::Revoke, "replaced", &c.by)
            .await?
            .expect("the key was just read");
        let (new, text) = access::make_key(
            &mut tx,
            station,
            c.key,
            &spec,
            cur.family_id,
            Some(id),
            &c.by,
        )
        .await?;
        db::link_replacement(&mut tx, station, id, new.id).await?;
        db::carry_listens(&mut tx, station, id, new.id).await?;
        db::log(
            &mut tx,
            station,
            id,
            &c.by,
            "replaced",
            json!({ "by_key": new.id }),
        )
        .await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>((moved, new.id, text))
    }
    .await;
    match result {
        Ok((moved, new_id, text)) => {
            if moved.ended_now {
                state.staff.api.events.publish(
                    KEY_ENDED,
                    json!({ "key": id, "label": moved.key.label, "family": moved.key.family_id,
                            "kind": moved.key.kind.as_str(), "reason": "replaced" }),
                );
            }
            let (old, new) = match (
                db::get_key(c.pool, station, id).await,
                db::get_key(c.pool, station, new_id).await,
            ) {
                (Ok(Some(o)), Ok(Some(n))) => (o, n),
                (Err(e), _) | (_, Err(e)) => return db_problem(e),
                _ => return not_found(),
            };
            tracing::info!(old = %id, new = %new_id, by = c.by, "listener key replaced");
            let m = made(&state, new, &text);
            Json(Replaced {
                old,
                new: m.key,
                text: m.text,
                link: m.link,
            })
            .into_response()
        }
        Err(e) => db_problem(e),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyText)]
struct Text {
    text: String,
    link: String,
    /// The email address kept with the key, if any: to send the key to.
    email: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/{id}/text",
    operation_id = "listener_keys_text",
    tag = "keys",
    summary = "Show a listener key's text again",
    description = "A producer, an admin, or an API token with `keys:manage`. The key's text, its link and the email kept with it, to send it again to someone who lost it. It is a POST and is logged (`text_shown`, by whom) so that who saw a key is known. 409 when the station's token key changed since the key was made: the text can no longer be read; replace the key.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 200, description = "The text, its link and the email.", body = Text),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn reveal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    match db::get_key(c.pool, station, id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    }
    let text = match db::sealed_text(c.pool, station, id).await {
        Ok(Some((n, s))) => access::open_text(c.key, id, &n, &s),
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    };
    let Some(text) = text else {
        return problem(
            StatusCode::CONFLICT,
            "That key's text can no longer be read: the station's token key changed after it was made. Replace the key.",
        );
    };
    let email = match db::sealed_email(c.pool, station, id).await {
        Ok(Some((n, s))) => access::open_email(c.key, id, &n, &s),
        Ok(None) => None,
        Err(e) => return db_problem(e),
    };
    let mut conn = match c.pool.acquire().await {
        Ok(c) => c,
        Err(e) => return db_problem(e),
    };
    if let Err(e) = db::log(&mut conn, station, id, &c.by, "text_shown", json!({})).await {
        return db_problem(e);
    }
    Json(Text {
        link: access::link(&state.join.public_url, &text),
        text: weavesignal_model::listener_keys::format_text(&text),
        email,
    })
    .into_response()
}

#[utoipa::path(
    delete,
    path = "/api/v1/listener-keys/{id}",
    operation_id = "listener_keys_erase",
    tag = "keys",
    summary = "Erase a listener key",
    description = "Admin only. Removes the key and everything kept about it: its log, devices and use records (the person's right to erasure, ADR-0122). Not an undo of a leak: to stop a key working, revoke it; an erased key is gone from the leak report too. Not available to an API token.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 204, description = "Erased."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn erase(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Admin) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::erase_key(c.pool, state.station.id, id).await {
        Ok(true) => {
            tracing::info!(key = %id, by = c.by, "listener key erased");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => not_found(),
        Err(e) => db_problem(e),
    }
}

// ---- the log and devices ----

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyLog)]
struct LogList {
    entries: Vec<LogEntry>,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/{id}/log",
    operation_id = "listener_keys_log",
    tag = "keys",
    summary = "A listener key's change log",
    description = "A producer, an admin, or an API token with `keys:manage`. Who changed what and when, newest first (the last 200): made, changed (field names only), extended, paused, resumed, revoked, replaced, text shown, lost-key asked, device signed out. Never a key's text.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 200, description = "The entries.", body = LogList),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn read_log(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::get_key(c.pool, state.station.id, id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    }
    match db::read_log(c.pool, state.station.id, id, 200).await {
        Ok(entries) => Json(LogList { entries }).into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyDevices)]
struct DeviceList {
    devices: Vec<Device>,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/{id}/devices",
    operation_id = "listener_keys_devices",
    tag = "keys",
    summary = "The devices that hold a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. The browsers and apps that have used the key (the last 200), the live ones first, each with what it called itself, when it was first and last seen and the show it last listened to.",
    params(("id" = Uuid, Path, description = "The key's id.")),
    responses(
        (status = 200, description = "The devices.", body = DeviceList),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn devices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::get_key(c.pool, state.station.id, id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => return db_problem(e),
    }
    match db::devices(c.pool, state.station.id, id).await {
        Ok(devices) => Json(DeviceList { devices }).into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/listener-keys/{id}/devices/{device}",
    operation_id = "listener_keys_end_device",
    tag = "keys",
    summary = "Sign a device out of a listener key",
    description = "A producer, an admin, or an API token with `keys:manage`. The device is refused at its next renewal (within a token's life) and its place is free at once.",
    params(
        ("id" = Uuid, Path, description = "The key's id."),
        ("device" = Uuid, Path, description = "The device's id."),
    ),
    responses(
        (status = 204, description = "Signed out."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn end_device(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    match db::end_device(c.pool, station, id, device).await {
        Ok(true) => {
            if let Ok(mut conn) = c.pool.acquire().await {
                let _ = db::log(
                    &mut conn,
                    station,
                    id,
                    &c.by,
                    "device_ended",
                    json!({ "device": device }),
                )
                .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such device on that key"),
        Err(e) => db_problem(e),
    }
}

// ---- families ----

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyFamilyList)]
struct FamilyList {
    families: Vec<Family>,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/families",
    operation_id = "listener_key_families_list",
    tag = "keys",
    summary = "Key families",
    description = "A producer, an admin, or an API token with `keys:manage`. The station's families, newest first, each with how many keys it has issued.",
    responses(
        (status = 200, description = "The families.", body = FamilyList),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn families(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::list_families(c.pool, state.station.id).await {
        Ok(families) => Json(FamilyList { families }).into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/families/{id}",
    operation_id = "listener_key_families_get",
    tag = "keys",
    summary = "One key family",
    description = "A producer, an admin, or an API token with `keys:manage`. The family; list its keys with `GET /api/v1/listener-keys?family=…`.",
    params(("id" = Uuid, Path, description = "The family's id.")),
    responses(
        (status = 200, description = "The family.", body = Family),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn one_family(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match db::get_family(c.pool, state.station.id, id).await {
        Ok(Some(f)) => Json(f).into_response(),
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such family"),
        Err(e) => db_problem(e),
    }
}

/// One recipient of a family: a name, and optionally an email and a note.
#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyRecipient)]
#[serde(deny_unknown_fields)]
struct Recipient {
    label: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyFamilyNew)]
#[serde(deny_unknown_fields)]
struct FamilyBody {
    /// The family's name, unique at the station: "Autumn patrons", "Charity quiz tickets".
    label: String,
    #[serde(default)]
    note: String,
    /// One key per recipient, named...
    #[serde(default)]
    recipients: Vec<Recipient>,
    /// ...or this many anonymous keys named `<prefix> 001`, `<prefix> 002`, and so on.
    #[serde(default)]
    count: Option<u32>,
    /// The anonymous keys' name before the number (default: the family's label).
    #[serde(default)]
    prefix: Option<String>,
    /// What every key in the family is: kind, media, shows, window, limits (the same words as
    /// making a key; its `label`, `email` and `note` are the recipient's).
    template: Template,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyTemplate)]
#[serde(deny_unknown_fields)]
struct Template {
    #[serde(default = "personal")]
    kind: Kind,
    #[serde(default = "both")]
    media: Media,
    #[serde(default)]
    all_shows: Option<bool>,
    #[serde(default)]
    shows: Vec<String>,
    #[serde(default)]
    valid_from_ms: Option<i64>,
    #[serde(default)]
    valid_until_ms: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<u32>)]
    max_devices: Option<Option<u32>>,
    #[serde(default)]
    max_listens: Option<u32>,
    #[serde(default)]
    max_people: Option<u32>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyFamilyMade)]
struct FamilyMade {
    family: Family,
    /// The keys, each with its text and link, in the order of the recipients.
    keys: Vec<Made>,
}

#[utoipa::path(
    post,
    path = "/api/v1/listener-keys/families",
    operation_id = "listener_key_families_create",
    tag = "keys",
    summary = "Issue a family of listener keys",
    description = "A producer, an admin, or an API token with `keys:manage`. Makes a family and one key per recipient in one go (named `recipients`, or `count` anonymous ones), all from one `template`, so that every copy is its own key and a leak can be traced to the key it came from. At most 1000 keys a call. The answer carries every key's text this once; the family's export and the reveal route give them again. The family's label is unique (409).",
    request_body = FamilyBody,
    responses(
        (status = 201, description = "The family and its keys.", body = FamilyMade),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn make_family(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<FamilyBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let Some(label) = access::clean_line(&body.label, 120).filter(|l| !l.is_empty()) else {
        return field("label", "Name the family in at most 120 characters.");
    };
    let Some(note) = access::clean_line(&body.note, 500) else {
        return field("note", "A note is at most 500 characters on one line.");
    };
    let mut people: Vec<Recipient> = body.recipients;
    match (people.is_empty(), body.count) {
        (false, None) => {}
        (true, Some(n)) if n >= 1 => {
            if n as usize > MAX_FAMILY {
                return field("count", format!("At most {MAX_FAMILY} keys a call."));
            }
            let prefix = body.prefix.as_deref().unwrap_or(&label);
            let Some(prefix) = access::clean_line(prefix, 100).filter(|p| !p.is_empty()) else {
                return field("prefix", "A prefix is at most 100 characters.");
            };
            for i in 1..=n {
                people.push(Recipient {
                    label: format!("{prefix} {i:03}"),
                    email: None,
                    note: String::new(),
                });
            }
        }
        _ => {
            return field(
                "recipients",
                "Name the recipients, or ask for a `count` of anonymous keys, one of the two.",
            );
        }
    }
    if people.len() > MAX_FAMILY {
        return field("recipients", format!("At most {MAX_FAMILY} keys a call."));
    }
    let t = &body.template;
    let shows = match resolve_shows(&state, &c, &t.shows).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let mut specs = Vec::with_capacity(people.len());
    for (i, r) in people.iter().enumerate() {
        let mut s = Spec {
            kind: t.kind,
            label: r.label.clone(),
            note: r.note.clone(),
            email: r.email.clone(),
            media: t.media,
            all_shows: t.all_shows.unwrap_or(shows.is_empty()),
            shows: shows.clone(),
            valid_from_ms: t.valid_from_ms,
            valid_until_ms: t.valid_until_ms,
            max_devices: match t.max_devices {
                Some(v) => v,
                None if t.kind == Kind::Machine => None,
                None => Some(2),
            },
            max_listens: t.max_listens,
            max_people: t.max_people,
        };
        if let Err(e) = s.check() {
            return field(
                e.field,
                format!("Recipient {} ({}): {}", i + 1, r.label.trim(), e.message),
            );
        }
        specs.push(s);
    }
    let family_id = Uuid::new_v4();
    let result = async {
        let mut tx = c.pool.begin().await?;
        db::create_family(&mut tx, station, family_id, &label, &note, &c.by).await?;
        let mut out = Vec::with_capacity(specs.len());
        for s in &specs {
            out.push(
                access::make_key(&mut tx, station, c.key, s, Some(family_id), None, &c.by).await?,
            );
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(out)
    }
    .await;
    match result {
        Ok(keys) => {
            let family = match db::get_family(c.pool, station, family_id).await {
                Ok(Some(f)) => f,
                Ok(None) => return not_found(),
                Err(e) => return db_problem(e),
            };
            tracing::info!(family = %family_id, keys = keys.len(), by = c.by, "listener key family issued");
            (
                StatusCode::CREATED,
                Json(FamilyMade {
                    family,
                    keys: keys.into_iter().map(|(k, t)| made(&state, k, &t)).collect(),
                }),
            )
                .into_response()
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => problem(
            StatusCode::CONFLICT,
            "A family with that name exists already. Give this one another.",
        ),
        Err(e) => db_problem(e),
    }
}

/// A spreadsheet cell that cannot run as a formula: one that starts with `=`, `+`, `-`, `@`, a
/// tab or a return is written with a quote in front, and every cell is quoted.
fn csv_cell(s: &str) -> String {
    let safe = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/families/{id}/export",
    operation_id = "listener_key_families_export",
    tag = "keys",
    summary = "Export a family's keys as CSV",
    description = "A producer, an admin, or an API token with `keys:manage`. One row per key: `name`, `email`, `note`, `key`, `link`, `state`: for a mail merge or a ticket platform. It holds every key's text, so it is logged on each key (`text_shown`). A cell that could run as a spreadsheet formula is written with a quote in front of it.",
    params(("id" = Uuid, Path, description = "The family's id.")),
    responses(
        (status = 200, description = "The CSV.", content_type = "text/csv", body = String),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn export_family(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let fam = match db::get_family(c.pool, station, id).await {
        Ok(Some(f)) => f,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "no such family"),
        Err(e) => return db_problem(e),
    };
    let (keys, _) = match db::list_keys(
        c.pool,
        station,
        &Filter {
            family: Some(id),
            limit: 5_000,
            ..Default::default()
        },
    )
    .await
    {
        Ok(k) => k,
        Err(e) => return db_problem(e),
    };
    let mut out = String::from("name,email,note,key,link,state\r\n");
    let mut conn = match c.pool.acquire().await {
        Ok(c) => c,
        Err(e) => return db_problem(e),
    };
    // Oldest first, so the rows follow the order the recipients were given in.
    for k in keys.iter().rev() {
        let text = match db::sealed_text(c.pool, station, k.id).await {
            Ok(Some((n, s))) => access::open_text(c.key, k.id, &n, &s),
            Ok(None) => None,
            Err(e) => return db_problem(e),
        };
        let Some(text) = text else {
            return problem(
                StatusCode::CONFLICT,
                "Some keys' text can no longer be read: the station's token key changed after they were made.",
            );
        };
        let email = match db::sealed_email(c.pool, station, k.id).await {
            Ok(Some((n, s))) => access::open_email(c.key, k.id, &n, &s).unwrap_or_default(),
            Ok(None) => String::new(),
            Err(e) => return db_problem(e),
        };
        out.push_str(&format!(
            "{},{},{},{},{},{}\r\n",
            csv_cell(&k.label),
            csv_cell(&email),
            csv_cell(&k.note),
            csv_cell(&weavesignal_model::listener_keys::format_text(&text)),
            csv_cell(&access::link(&state.join.public_url, &text)),
            csv_cell(k.state.as_str()),
        ));
        if let Err(e) = db::log(
            &mut conn,
            station,
            k.id,
            &c.by,
            "text_shown",
            json!({ "export": fam.id }),
        )
        .await
        {
            return db_problem(e);
        }
    }
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"keys.csv\"".to_owned(),
            ),
        ],
        out,
    )
        .into_response()
}

// ---- the leak report ----

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct LeakQuery {
    /// Only this family's keys; left out, every key.
    family: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyLeakReport)]
struct LeakReport {
    generated_ms: i64,
    /// Every key, the most likely passed on first (then by addresses seen in the last seven
    /// days, then the most recently seen); keys never used are last.
    entries: Vec<Leak>,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/leaks",
    operation_id = "listener_keys_leaks",
    tag = "keys",
    summary = "The leak report",
    description = "A producer, an admin, or an API token with `keys:manage`. For each key (of a family, or all): first and last seen, listens, devices, distinct addresses (ever, in 24 hours, in seven days), the most new addresses in one hour (`spike` from 3), and a `score` with its `reasons` for \"most likely passed on\", sorted highest first. The score is a hint: a phone on mobile data changes address, which is why it sums several signs and says which. An address is never shown, only counted (the station keeps a keyed hash of it for `keys.use_records_days`).",
    params(LeakQuery),
    responses(
        (status = 200, description = "The report.", body = LeakReport),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn leaks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<LeakQuery>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let now = now_ms() as i64;
    match db::leak_report(c.pool, state.station.id, q.family, now).await {
        Ok(entries) => Json(LeakReport {
            generated_ms: now,
            entries,
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

// ---- how tokens are signed ----

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeySigning)]
struct Signing {
    /// `p256`: ECDSA P-256 with SHA-256 (the one format; 1.3.2).
    alg: &'static str,
    /// The token's prefix: `ks1`.
    prefix: String,
    /// The public key an edge checks tokens with (`EDGE_KEYED_PUBLIC_KEY`): 65 bytes,
    /// uncompressed, in URL-safe base 64 without padding (87 characters). It checks tokens and
    /// can make none. Always present.
    public_key: Option<String>,
    /// The renewal's deadline (a token itself lasts its show's window).
    token_ttl_s: u64,
    /// A player renews about this often: a change to a key other than a pause, a revoke or a
    /// replace reaches someone already listening within it.
    renew_every_s: u64,
    clock_skew_s: u64,
    /// A key ends "soon" this many days before its end: the player then says when it ends.
    ends_soon_days: u32,
    /// A device holds its place on a key while it has renewed within this many seconds.
    device_hold_s: u64,
}

#[utoipa::path(
    get,
    path = "/api/v1/listener-keys/signing",
    operation_id = "listener_keys_signing",
    tag = "keys",
    summary = "How keyed shows' tokens are signed",
    description = "A producer, an admin, or an API token with `keys:manage`. The signing algorithm (`p256`) and the public key an edge checks tokens with (it cannot make them), for `EDGE_KEYED_PUBLIC_KEY`; and the renewal's deadline and pace and the clock leeway. The token's format is in the reference.",
    responses(
        (status = 200, description = "The signing facts.", body = Signing),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn signing(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    use base64::Engine as _;
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let rt = &state.staff.api.access;
    let s = rt.signer(c.key);
    Json(Signing {
        alg: s.alg(),
        prefix: weavesignal_keyed::PREFIX.to_owned(),
        public_key: Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.public_key())),
        token_ttl_s: rt.config.token_ttl_s,
        renew_every_s: rt.config.renew_every_s,
        clock_skew_s: rt.config.clock_skew_s,
        ends_soon_days: rt.config.ends_soon_days,
        device_hold_s: rt.config.device_hold_s,
    })
    .into_response()
}

// ---- a show's access rule ----

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ShowAccessSet)]
#[serde(deny_unknown_fields)]
struct AccessBody {
    /// `public` or `key`.
    rule: String,
    /// From this moment the show is public whatever the rule says: early access, or the free
    /// part of a show (Unix milliseconds); `null` for never.
    #[serde(default)]
    opens_at_ms: Option<i64>,
    /// What the public hears while the show is keyed: `announcement` (default), `fallback` or
    /// `teaser`.
    #[serde(default = "announcement")]
    slate: String,
    /// A line for the announcement slate: where to get a key, when the public programme
    /// returns (at most 500 characters).
    #[serde(default)]
    slate_note: String,
}

fn announcement() -> String {
    "announcement".to_owned()
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/access",
    operation_id = "show_access_get",
    tag = "keys",
    summary = "A show's access rule",
    description = "A producer, an admin, or an API token with `keys:manage`. Whether the show is `public` or needs a `key`, when it opens to everyone (`opens_at_ms`), and what the public hears meanwhile (`slate`). A show with no rule set is public.",
    params(("show" = String, Path, description = "The show's slug.")),
    responses(
        (status = 200, description = "The rule.", body = Access),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_access(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Response {
    let c = match ready(&state, &headers, Need::Staff) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let station = state.station.id;
    let show = match weavesignal_model::shows::get_show(c.pool, station, &slug).await {
        Ok(Some(s)) => s,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "no such show"),
        Err(e) => return db_problem(e),
    };
    match db::get_access(c.pool, station, show.id).await {
        Ok(Some(a)) => Json(a).into_response(),
        Ok(None) => Json(Access {
            show_id: show.id,
            rule: "public".into(),
            opens_at_ms: None,
            slate: "announcement".into(),
            slate_note: String::new(),
            updated_ms: 0,
            updated_by: String::new(),
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/access",
    operation_id = "show_access_put",
    tag = "keys",
    summary = "Set a show's access rule",
    description = "A producer or an admin (a staff session; never an API token: which shows are keyed is a person's decision). `rule` is `public` or `key`; `opens_at_ms` makes the show public from then on; `slate` is what the public hears meanwhile. Changing the rule changes who may hear the show at once for new listeners.",
    params(("show" = String, Path, description = "The show's slug.")),
    request_body = AccessBody,
    responses(
        (status = 200, description = "The rule as set.", body = Access),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, description = "`key` on a station whose playout has keyed shows off: nothing would keep the show from the public."),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_access(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<AccessBody>,
) -> Response {
    let c = match ready(&state, &headers, Need::Producer) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if !matches!(body.rule.as_str(), "public" | "key") {
        return field("rule", "The rule is `public` or `key`.");
    }
    if !matches!(body.slate.as_str(), "announcement" | "fallback" | "teaser") {
        return field(
            "slate",
            "The public hears an `announcement`, the `fallback` loop or a `teaser`.",
        );
    }
    let Some(note) = access::clean_line(&body.slate_note, 500) else {
        return field(
            "slate_note",
            "A slate note is at most 500 characters on one line.",
        );
    };
    let station = state.station.id;
    let show = match weavesignal_model::shows::get_show(c.pool, station, &slug).await {
        Ok(Some(s)) => s,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "no such show"),
        Err(e) => return db_problem(e),
    };
    // Fail closed (1.3.2): a show is keyed only where playout can keep it from the public. A
    // playout that answers without keyed shows (its data's `keyed.enabled` off, or an older
    // script) is refused; one that does not answer is asked again later. A station with no
    // playout at all has no programme to keep anything from.
    if body.rule == "key"
        && let Some(onair) = state.hub.onair.get()
        && onair.links.playout.configured()
    {
        match onair.links.playout.command("keyed.status").await {
            Ok(a) => {
                let on = serde_json::from_str::<serde_json::Value>(&a)
                    .ok()
                    .and_then(|v| v["enabled"].as_bool())
                    == Some(true);
                if !on {
                    return problem(
                        StatusCode::CONFLICT,
                        "Playout has keyed shows off (its data's `keyed.enabled`), so nothing would keep this show from the public. Turn them on and restart playout first.",
                    );
                }
            }
            Err(e) => {
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Playout is not answering, so a keyed show cannot be checked: {e}"),
                );
            }
        }
    }
    let a = Access {
        show_id: show.id,
        rule: body.rule,
        opens_at_ms: body.opens_at_ms,
        slate: body.slate,
        slate_note: note,
        updated_ms: 0,
        updated_by: String::new(),
    };
    match db::set_access(c.pool, station, &a, &c.by).await {
        Ok(a) => {
            // The windows are read again at once: the gate follows the new rule within a tick.
            state.staff.api.access.live.stale();
            tracing::info!(show = %slug, rule = a.rule, by = c.by, "show access set");
            Json(a).into_response()
        }
        Err(e) => db_problem(e),
    }
}

/// First use, as webhooks hear of it: ids and the label, never the text.
pub(crate) fn first_used_data(k: &Key) -> serde_json::Value {
    json!({ "key": k.id, "label": k.label, "family": k.family_id, "kind": k.kind.as_str() })
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    list,
    create,
    find,
    one,
    change,
    erase,
    extend,
    pause,
    resume,
    revoke,
    replace,
    reveal,
    read_log,
    devices,
    end_device,
    families,
    make_family,
    one_family,
    export_family,
    leaks,
    signing,
    get_access,
    put_access
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spreadsheet_cell_cannot_run_as_a_formula() {
        assert_eq!(csv_cell("Ishbel"), "\"Ishbel\"");
        assert_eq!(csv_cell("=1+1"), "\"'=1+1\"");
        assert_eq!(csv_cell("+44 7700"), "\"'+44 7700\"");
        assert_eq!(csv_cell("@sum"), "\"'@sum\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
    }

    #[test]
    fn a_change_logs_the_names_of_its_fields_and_no_values() {
        let b: ChangeBody = serde_json::from_str(
            r#"{"label":"Secret Name","email":"a@b.co","valid_until_ms":null}"#,
        )
        .unwrap();
        let d = changed_fields(&b);
        assert_eq!(d["fields"], json!(["label", "email", "valid_until"]));
        assert!(!d.to_string().contains("Secret"));
        assert!(!d.to_string().contains("a@b.co"));
    }

    #[test]
    fn a_limit_left_out_differs_from_a_limit_sent_as_null() {
        let b: NewBody = serde_json::from_str(r#"{"label":"x"}"#).unwrap();
        assert_eq!(b.max_devices, None);
        let b: NewBody = serde_json::from_str(r#"{"label":"x","max_devices":null}"#).unwrap();
        assert_eq!(b.max_devices, Some(None));
        let b: NewBody = serde_json::from_str(r#"{"label":"x","max_devices":3}"#).unwrap();
        assert_eq!(b.max_devices, Some(Some(3)));
        // The defaults: a personal key for every keyed show, two devices.
        let spec = spec_of(&serde_json::from_str(r#"{"label":"x"}"#).unwrap(), vec![]);
        assert!(spec.all_shows && spec.max_devices == Some(2) && spec.kind == Kind::Personal);
        let spec = spec_of(
            &serde_json::from_str(r#"{"label":"x","kind":"machine"}"#).unwrap(),
            vec![],
        );
        assert_eq!(spec.max_devices, None);
    }
}
