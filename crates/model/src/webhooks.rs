// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Webhooks (ADR-0195, migration 0020): a URL told about events, and the record of the last
//! delivery. The signing secret is not stored; the control plane derives it from the webhook's
//! id and `salt` under its token key.

use serde::Serialize;
use uuid::Uuid;

use crate::StationId;

/// A webhook as it is kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Webhook {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_by: String,
    pub created_ms: i64,
    pub last_attempt_ms: Option<i64>,
    pub last_success_ms: Option<i64>,
    /// The HTTP status the receiver answered with; 0 when it did not answer.
    pub last_status: Option<i32>,
    pub last_error: Option<String>,
    /// Deliveries failed since the last good one.
    pub failures: i32,
    /// What the signing secret is derived from; never shown.
    #[serde(skip)]
    pub salt: [u8; 16],
}

pub struct NewWebhook<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub url: &'a str,
    pub events: &'a [String],
    pub salt: &'a [u8; 16],
    pub created_by: &'a str,
}

const COLUMNS: &str = "id, name, url, events, enabled, created_by,
    (extract(epoch FROM created_at) * 1000)::bigint,
    (extract(epoch FROM last_attempt_at) * 1000)::bigint,
    (extract(epoch FROM last_success_at) * 1000)::bigint,
    last_status, last_error, failures, salt";

type Row = (
    Uuid,
    String,
    String,
    Vec<String>,
    bool,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i32>,
    Option<String>,
    i32,
    Vec<u8>,
);

fn webhook(r: Row) -> Option<Webhook> {
    Some(Webhook {
        id: r.0,
        name: r.1,
        url: r.2,
        events: r.3,
        enabled: r.4,
        created_by: r.5,
        created_ms: r.6,
        last_attempt_ms: r.7,
        last_success_ms: r.8,
        last_status: r.9,
        last_error: r.10,
        failures: r.11,
        salt: r.12.try_into().ok()?,
    })
}

pub async fn create(
    pool: &sqlx::PgPool,
    station: StationId,
    new: &NewWebhook<'_>,
) -> Result<Webhook, sqlx::Error> {
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO webhook (id, station_id, name, url, events, salt, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(station.as_uuid())
    .bind(new.name)
    .bind(new.url)
    .bind(new.events)
    .bind(new.salt.to_vec())
    .bind(new.created_by)
    .fetch_one(pool)
    .await?;
    Ok(webhook(row).expect("a 16-byte salt"))
}

pub async fn list(pool: &sqlx::PgPool, station: StationId) -> Result<Vec<Webhook>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM webhook WHERE station_id = $1 ORDER BY created_at, id"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().filter_map(webhook).collect())
}

pub async fn get(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Webhook>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM webhook WHERE station_id = $1 AND id = $2"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(webhook))
}

/// Change a webhook's name, address, events or whether it is on; fields left `None` stay.
pub async fn update(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    name: Option<&str>,
    url: Option<&str>,
    events: Option<&[String]>,
    enabled: Option<bool>,
) -> Result<Option<Webhook>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE webhook SET
            name = COALESCE($3, name), url = COALESCE($4, url),
            events = COALESCE($5, events), enabled = COALESCE($6, enabled)
         WHERE station_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .bind(name)
    .bind(url)
    .bind(events)
    .bind(enabled)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(webhook))
}

/// A new salt, so the old secret stops signing and a new one starts.
pub async fn rotate(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    salt: &[u8; 16],
) -> Result<Option<Webhook>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE webhook SET salt = $3 WHERE station_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .bind(salt.to_vec())
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(webhook))
}

pub async fn delete(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query("DELETE FROM webhook WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n == 1)
}

/// Record a delivery's outcome: `status` is what the receiver answered (0 when nothing).
pub async fn record(
    pool: &sqlx::PgPool,
    id: Uuid,
    status: i32,
    ok: bool,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE webhook SET last_attempt_at = now(), last_status = $2, last_error = $4,
            last_success_at = CASE WHEN $3 THEN now() ELSE last_success_at END,
            failures = CASE WHEN $3 THEN 0 ELSE failures + 1 END
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(ok)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
