// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Passes the studio has ended (ADR-0119): kept so that an ended pass buys no room token after
//! a restart, and only for as long as a pass can live.

use crate::StationId;

/// The longest a pass can live (ADR-0108), and so how long an ended pass is remembered.
pub const PASS_LONGEST_S: i64 = 24 * 3600;

/// End an identity's passes now, forgetting ends older than a pass can live.
pub async fn end_pass(
    pool: &sqlx::PgPool,
    station: StationId,
    identity: &str,
) -> Result<(), sqlx::Error> {
    forget_old(pool, station).await?;
    sqlx::query(
        "INSERT INTO pass_ended (station_id, identity, ended_at) VALUES ($1, $2, now())
         ON CONFLICT (station_id, identity) DO UPDATE SET ended_at = excluded.ended_at",
    )
    .bind(station.as_uuid())
    .bind(identity)
    .execute(pool)
    .await?;
    Ok(())
}

/// The passes ended within a pass's longest life: each identity with when, in Unix ms.
pub async fn ended_passes(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<(String, u64)>, sqlx::Error> {
    forget_old(pool, station).await?;
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT identity, extract(epoch FROM ended_at)::float8 * 1000 FROM pass_ended
         WHERE station_id = $1 ORDER BY identity",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, ms)| (id, ms.max(0.0) as u64))
        .collect())
}

async fn forget_old(pool: &sqlx::PgPool, station: StationId) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM pass_ended WHERE station_id = $1 AND ended_at < now() - make_interval(secs => $2)",
    )
    .bind(station.as_uuid())
    .bind(PASS_LONGEST_S as f64)
    .execute(pool)
    .await?
    .rows_affected())
}
