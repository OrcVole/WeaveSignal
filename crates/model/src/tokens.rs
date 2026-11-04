// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! API tokens (ADR-0195, migration 0019): what the database keeps of a token.
//!
//! The secret is never here: `secret_hash` is the keyed hash the control plane made of it
//! (`crates/control/src/apikey.rs`), and the model only stores and finds it. The control plane
//! holds the live tokens in memory, reads them back at start and writes `last_used_at` here.

use serde::Serialize;
use uuid::Uuid;

use crate::StationId;

/// What an API token's secret starts with, so it is never mistaken for a staff session
/// (`rss_`) or the station key.
pub const TOKEN_PREFIX: &str = "rsa_";

/// A token as it is listed: everything but the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ApiToken {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    /// The secret's first characters, to tell tokens apart.
    pub prefix: String,
    pub created_by: String,
    pub created_ms: i64,
    pub expires_ms: Option<i64>,
    pub last_used_ms: Option<i64>,
    pub revoked_ms: Option<i64>,
}

/// A token and its hash, as the control plane keeps the live ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredToken {
    pub token: ApiToken,
    pub secret_hash: [u8; 32],
}

/// What a new token is made from.
pub struct NewToken<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub scopes: &'a [String],
    pub prefix: &'a str,
    pub secret_hash: &'a [u8; 32],
    pub created_by: &'a str,
    pub expires_ms: Option<i64>,
}

const COLUMNS: &str = "id, name, scopes, prefix, created_by,
    (extract(epoch FROM created_at) * 1000)::bigint,
    (extract(epoch FROM expires_at) * 1000)::bigint,
    (extract(epoch FROM last_used_at) * 1000)::bigint,
    (extract(epoch FROM revoked_at) * 1000)::bigint,
    secret_hash";

type Row = (
    Uuid,
    String,
    Vec<String>,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Vec<u8>,
);

fn stored(r: Row) -> Option<StoredToken> {
    Some(StoredToken {
        secret_hash: r.9.try_into().ok()?,
        token: ApiToken {
            id: r.0,
            name: r.1,
            scopes: r.2,
            prefix: r.3,
            created_by: r.4,
            created_ms: r.5,
            expires_ms: r.6,
            last_used_ms: r.7,
            revoked_ms: r.8,
        },
    })
}

/// Keep a new token and answer it as listed.
pub async fn create(
    pool: &sqlx::PgPool,
    station: StationId,
    new: &NewToken<'_>,
) -> Result<ApiToken, sqlx::Error> {
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO api_token (id, station_id, name, scopes, prefix, secret_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $8::float8 IS NULL THEN NULL ELSE to_timestamp($8::float8 / 1000) END)
         RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(station.as_uuid())
    .bind(new.name)
    .bind(new.scopes)
    .bind(new.prefix)
    .bind(new.secret_hash.to_vec())
    .bind(new.created_by)
    .bind(new.expires_ms.map(|m| m as f64))
    .fetch_one(pool)
    .await?;
    Ok(stored(row).expect("a 32-byte hash").token)
}

/// Every token of the station, newest first, revoked and expired ones included.
pub async fn list(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<StoredToken>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM api_token WHERE station_id = $1 ORDER BY created_at DESC, id"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().filter_map(stored).collect())
}

/// Revoke a token: it never works again. `false` when the station has no such token; a token
/// already revoked keeps its first revocation.
pub async fn revoke(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE api_token SET revoked_at = COALESCE(revoked_at, now())
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// Write when a token was last used.
pub async fn touch(pool: &sqlx::PgPool, id: Uuid, at_ms: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE api_token SET last_used_at = to_timestamp($2::float8 / 1000)
         WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < to_timestamp($2::float8 / 1000))",
    )
    .bind(id)
    .bind(at_ms as f64)
    .execute(pool)
    .await?;
    Ok(())
}
