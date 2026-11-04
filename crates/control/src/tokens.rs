// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! API tokens (ADR-0195): the admin routes that make, list and revoke them. What a token may
//! do, and how it is checked on each request, is in [`crate::scopes`] and [`crate::api`].
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/tokens` | admin | every token (never its secret), and the scopes one can hold |
//! | `POST /api/v1/tokens` | admin | `name`, `scopes`, optional `expires_in_days`: made; the secret is in the answer, this once |
//! | `DELETE /api/v1/tokens/{id}` | admin | revoke: it stops working at once, and stays in the list as revoked |
//!
//! A token's secret is `rsa_` and 64 random hex digits (244 bits from the operating system),
//! sent as `Authorization: Bearer rsa_…`. It is kept only as a keyed hash ([`crate::apikey`]):
//! lost, it is made again, not read back.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::tokens::{self as model, ApiToken, NewToken, StoredToken, TOKEN_PREFIX};

use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::scopes::{SCOPES, is_scope};
use crate::staff::Need;

/// How many tokens a station may have live.
const MAX_LIVE: usize = 100;
/// The longest a token may be made to last.
const MAX_DAYS: u32 = 3650;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/tokens", get(list).post(create))
        .route("/api/v1/tokens/{id}", delete(revoke))
}

#[derive(Serialize, utoipa::ToSchema)]
struct Scope {
    name: &'static str,
    description: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Listed {
    tokens: Vec<ApiToken>,
    scopes: Vec<Scope>,
}

fn field(field: &str, detail: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": detail.into(), "field": field })),
    )
        .into_response()
}

fn db_problem(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "API tokens: database error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "the database refused that",
    )
}

/// The admin, and what tokens need: the database and a key, or the refusal. A token is never
/// an admin, so a token cannot make or revoke tokens.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn ready<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<(String, &'a sqlx::PgPool, &'a crate::apikey::ApiKey), Response> {
    let actor = state.check(headers, Need::Admin)?;
    let Some(pool) = state.db.as_ref() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "API tokens are kept in the database",
        ));
    };
    let Some(key) = state.staff.api.key() else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "API tokens need a key: set {} (or the station key)",
                crate::apikey::ApiKey::ENV
            ),
        ));
    };
    Ok((actor.label(), pool, key))
}

#[utoipa::path(
    get,
    path = "/api/v1/tokens",
    operation_id = "tokens_list",
    tag = "tokens",
    summary = "API tokens",
    description = "Admin only. Every token the station has made, live, expired and revoked, never its secret; and every scope a token can hold, with what it opens.",
    responses(
        (status = 200, description = "The tokens and the scopes.", body = Listed),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let (_, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match model::list(pool, state.station.id).await {
        Ok(all) => Json(Listed {
            tokens: all.into_iter().map(|s| s.token).collect(),
            scopes: SCOPES
                .iter()
                .map(|(name, description)| Scope { name, description })
                .collect(),
        })
        .into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct NewBody {
    name: String,
    scopes: Vec<String>,
    /// Days from now; left out, the token does not expire.
    #[serde(default)]
    expires_in_days: Option<u32>,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Made {
    token: ApiToken,
    /// The secret, shown this once.
    secret: String,
}

/// A fresh secret: `rsa_` and 244 random bits from the operating system, in hex.
fn new_secret() -> String {
    format!(
        "{TOKEN_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

#[utoipa::path(
    post,
    path = "/api/v1/tokens",
    operation_id = "tokens_create",
    tag = "tokens",
    summary = "Make an API token",
    description = "Admin only. `name` (up to 80 characters), `scopes` (at least one, from the list), and `expires_in_days` (1 to 3650; left out, it does not expire). The answer carries the secret (`rsa_` and 64 hex digits) this once; the station keeps only a keyed hash of it. At most 100 live tokens (409).",
    request_body = NewBody,
    responses(
        (status = 201, description = "The token, and its secret, shown once.", body = Made),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewBody>,
) -> Response {
    let (by, pool, key) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return field(
            "name",
            "Give it a name of at most 80 characters, so you can tell it apart.",
        );
    }
    let mut scopes: Vec<String> = Vec::new();
    for s in &req.scopes {
        if !is_scope(s) {
            return field("scopes", format!("`{s}` is not a scope."));
        }
        if !scopes.contains(s) {
            scopes.push(s.clone());
        }
    }
    if scopes.is_empty() {
        return field("scopes", "Choose at least one scope.");
    }
    scopes.sort();
    let now = now_ms() as i64;
    let expires_ms = match req.expires_in_days {
        None => None,
        Some(d) if (1..=MAX_DAYS).contains(&d) => Some(now + i64::from(d) * 86_400_000),
        Some(_) => {
            return field(
                "expires_in_days",
                format!("A token lasts 1 to {MAX_DAYS} days, or does not expire."),
            );
        }
    };
    match model::list(pool, state.station.id).await {
        Ok(all)
            if all
                .iter()
                .filter(|s| {
                    s.token.revoked_ms.is_none() && s.token.expires_ms.is_none_or(|e| e > now)
                })
                .count()
                >= MAX_LIVE =>
        {
            return problem(
                StatusCode::CONFLICT,
                format!("A station may have {MAX_LIVE} live tokens. Revoke one first."),
            );
        }
        Ok(_) => {}
        Err(e) => return db_problem(e),
    }
    let secret = new_secret();
    let hash = key.token_hash(&secret);
    let prefix: String = secret.chars().take(TOKEN_PREFIX.len() + 4).collect();
    let made = model::create(
        pool,
        state.station.id,
        &NewToken {
            id: Uuid::new_v4(),
            name,
            scopes: &scopes,
            prefix: &prefix,
            secret_hash: &hash,
            created_by: &by,
            expires_ms,
        },
    )
    .await;
    match made {
        Ok(token) => {
            tracing::info!(token = %token.id, name = %token.name, by, scopes = ?token.scopes, "API token made");
            state.staff.api.add(StoredToken {
                token: token.clone(),
                secret_hash: hash,
            });
            (StatusCode::CREATED, Json(Made { token, secret })).into_response()
        }
        Err(e) => db_problem(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/tokens/{id}",
    operation_id = "tokens_revoke",
    tag = "tokens",
    summary = "Revoke an API token",
    description = "Admin only. The token stops working at once and stays in the list, marked revoked.",
    params(("id" = Uuid, Path, description = "The token's id.")),
    responses(
        (status = 204, description = "Revoked."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let (by, pool, _) = match ready(&state, &headers) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match model::revoke(pool, state.station.id, id).await {
        Ok(true) => {
            state.staff.api.remove(id);
            tracing::info!(token = %id, by, "API token revoked");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such token"),
        Err(e) => db_problem(e),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(list, create, revoke))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_has_the_prefix_and_244_bits_of_hex() {
        let s = new_secret();
        assert!(s.starts_with("rsa_"));
        assert_eq!(s.len(), 4 + 64);
        assert!(s[4..].chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(s, new_secret());
    }
}
