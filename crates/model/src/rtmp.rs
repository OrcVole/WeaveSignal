// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! RTMP out's destinations (ADR-0230, migration 0024): where the desk sends the
//! programme. The stream key is kept sealed; the control plane seals and opens it
//! (`crates/control/src/rtmp.rs`), and it is never part of what is shown.

use serde::Serialize;
use uuid::Uuid;

use crate::StationId;

/// A destination as it is kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RtmpDestination {
    pub id: Uuid,
    pub name: String,
    /// `youtube`, `owncast`, `x`, `gab`, `rumble`, `odysee`, `peertube` or `custom`: the form the URL came from.
    pub platform: String,
    /// The server URL, without the key.
    pub url: String,
    /// `compatible` (H.264 and AAC) or `open` (AV1 and Opus over Enhanced RTMP).
    pub profile: String,
    pub enabled: bool,
    pub created_by: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// The sealed key and its nonce; never shown.
    #[serde(skip)]
    pub key_nonce: Vec<u8>,
    #[serde(skip)]
    pub key_sealed: Vec<u8>,
}

pub struct NewRtmpDestination<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub platform: &'a str,
    pub url: &'a str,
    pub key_nonce: &'a [u8],
    pub key_sealed: &'a [u8],
    pub profile: &'a str,
    pub enabled: bool,
    pub created_by: &'a str,
}

/// What a change sets; `None` keeps the field.
#[derive(Default)]
pub struct RtmpChange<'a> {
    pub name: Option<&'a str>,
    pub platform: Option<&'a str>,
    pub url: Option<&'a str>,
    /// A new key: its nonce and sealed form.
    pub key: Option<(&'a [u8], &'a [u8])>,
    pub profile: Option<&'a str>,
    pub enabled: Option<bool>,
}

const COLUMNS: &str = "id, name, platform, url, profile, enabled, created_by,
    (extract(epoch FROM created_at) * 1000)::bigint,
    (extract(epoch FROM updated_at) * 1000)::bigint,
    key_nonce, key_sealed";

type Row = (
    Uuid,
    String,
    String,
    String,
    String,
    bool,
    String,
    i64,
    i64,
    Vec<u8>,
    Vec<u8>,
);

fn destination(r: Row) -> RtmpDestination {
    RtmpDestination {
        id: r.0,
        name: r.1,
        platform: r.2,
        url: r.3,
        profile: r.4,
        enabled: r.5,
        created_by: r.6,
        created_ms: r.7,
        updated_ms: r.8,
        key_nonce: r.9,
        key_sealed: r.10,
    }
}

/// A unique violation (a name already used) is `Ok(None)`.
pub async fn create(
    pool: &sqlx::PgPool,
    station: StationId,
    new: &NewRtmpDestination<'_>,
) -> Result<Option<RtmpDestination>, sqlx::Error> {
    let r: Result<Row, sqlx::Error> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO rtmp_destination
            (id, station_id, name, platform, url, key_nonce, key_sealed, profile, enabled, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(station.as_uuid())
    .bind(new.name)
    .bind(new.platform)
    .bind(new.url)
    .bind(new.key_nonce)
    .bind(new.key_sealed)
    .bind(new.profile)
    .bind(new.enabled)
    .bind(new.created_by)
    .fetch_one(pool)
    .await;
    match r {
        Ok(row) => Ok(Some(destination(row))),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn list(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<RtmpDestination>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM rtmp_destination WHERE station_id = $1 ORDER BY created_at, id"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(destination).collect())
}

/// Change a destination; `Ok(None)` when there is none, `Err` with a unique violation when the
/// new name is taken.
pub async fn update(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    c: &RtmpChange<'_>,
) -> Result<Option<RtmpDestination>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE rtmp_destination SET
            name = COALESCE($3, name), platform = COALESCE($4, platform),
            url = COALESCE($5, url), key_nonce = COALESCE($6, key_nonce),
            key_sealed = COALESCE($7, key_sealed), profile = COALESCE($8, profile),
            enabled = COALESCE($9, enabled), updated_at = now()
         WHERE station_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .bind(c.name)
    .bind(c.platform)
    .bind(c.url)
    .bind(c.key.map(|k| k.0))
    .bind(c.key.map(|k| k.1))
    .bind(c.profile)
    .bind(c.enabled)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(destination))
}

pub async fn delete(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query("DELETE FROM rtmp_destination WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n == 1)
}
