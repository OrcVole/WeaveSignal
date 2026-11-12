// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A read of the chat's moderation queue for the studio's desk (ADR-0274: the desk's chat panel
//! says when a moderator is needed). The chat is its own service, and the tables are its own
//! (migration 0006); the control plane shares the database and only counts. The counts use the
//! chat's own queries: held messages as `held_messages` lists them (a held picture or document has its own queue), and open items as
//! `open_items` groups them (one per reported message, every notice and review on its own).

use crate::StationId;

/// What waits for a moderator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModerationCounts {
    /// Messages held until a moderator releases them.
    pub held: u32,
    /// Open reports, notices and reviews, with the reports of one message counted once.
    pub reports: u32,
}

/// Count what waits for a moderator.
pub async fn moderation_counts(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<ModerationCounts, sqlx::Error> {
    let (held, reports): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM chat_message WHERE station_id = $1 AND state = 'held'
                                          AND attachment IS NULL),
           (SELECT count(*) FROM chat_report r
             WHERE r.station_id = $1 AND r.status IN ('open', 'escalated')
               AND (r.kind <> 'report' OR r.id = (SELECT o.id FROM chat_report o
                 WHERE o.station_id = r.station_id AND o.kind = 'report'
                   AND o.status IN ('open', 'escalated')
                   AND o.message_id IS NOT DISTINCT FROM r.message_id
                   AND o.account_id IS NOT DISTINCT FROM r.account_id
                 ORDER BY o.created_at LIMIT 1)))",
    )
    .bind(station.as_uuid())
    .fetch_one(pool)
    .await?;
    Ok(ModerationCounts {
        held: held.clamp(0, i64::from(u32::MAX)) as u32,
        reports: reports.clamp(0, i64::from(u32::MAX)) as u32,
    })
}
