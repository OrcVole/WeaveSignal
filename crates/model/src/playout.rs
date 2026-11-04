// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Playout's rows (migration 0004): the station's playout settings, the music library, clocks and
//! the week's grid, the generated log, the per-play log and the anonymous listener counts.
//!
//! Times cross this boundary as milliseconds since the Unix epoch (the workspace's sqlx carries
//! no date type); SQL converts at each end.

use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

use crate::StationId;

/// `to_timestamp` for a bound millisecond count.
const TS: &str = "to_timestamp($ms::double precision / 1000.0)";

fn ts(n: usize) -> String {
    TS.replace("$ms", &format!("${n}"))
}

fn ms(col: &str) -> String {
    format!("(extract(epoch FROM {col}) * 1000)::bigint")
}

/// The station's playout settings (`playout_settings`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayoutSettings {
    /// An IANA zone; the clocks run in its local hours.
    pub time_zone: String,
    pub artist_separation_min: u32,
    pub title_separation_min: u32,
    pub album_separation_min: u32,
    /// `live` or `auto`.
    pub mode: String,
    pub default_clock: Option<Uuid>,
}

impl Default for PlayoutSettings {
    fn default() -> Self {
        Self {
            time_zone: "UTC".into(),
            artist_separation_min: 60,
            title_separation_min: 180,
            album_separation_min: 60,
            mode: "live".into(),
            default_clock: None,
        }
    }
}

/// Whether anyone has chosen the station's zone (`zone_set`), or it is still the configuration's
/// seed (ADR-0189). A station with no settings row yet has not chosen.
pub async fn zone_is_set(pool: &PgPool, station: StationId) -> Result<bool, sqlx::Error> {
    let r: Option<(bool,)> =
        sqlx::query_as("SELECT zone_set FROM playout_settings WHERE station_id = $1")
            .bind(station.as_uuid())
            .fetch_optional(pool)
            .await?;
    Ok(r.is_some_and(|(set,)| set))
}

pub async fn settings(pool: &PgPool, station: StationId) -> Result<PlayoutSettings, sqlx::Error> {
    sqlx::query("INSERT INTO playout_settings (station_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(station.as_uuid())
        .execute(pool)
        .await?;
    let r = sqlx::query(
        "SELECT time_zone, artist_separation_min, title_separation_min, album_separation_min,
                mode, default_clock
         FROM playout_settings WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_one(pool)
    .await?;
    Ok(PlayoutSettings {
        time_zone: r.get(0),
        artist_separation_min: r.get::<i32, _>(1) as u32,
        title_separation_min: r.get::<i32, _>(2) as u32,
        album_separation_min: r.get::<i32, _>(3) as u32,
        mode: r.get(4),
        default_clock: r.get(5),
    })
}

/// The station's time zone at start-up: the configuration's `time_zone` (`publishing.zone`)
/// unless someone has set one in the studio's settings (`zone_set`), in which case theirs.
/// Returns the zone in force (review 0025, S1).
pub async fn seed_time_zone(
    pool: &PgPool,
    station: StationId,
    time_zone: &str,
) -> Result<String, sqlx::Error> {
    sqlx::query(
        "INSERT INTO playout_settings (station_id, time_zone) VALUES ($1, $2)
         ON CONFLICT (station_id) DO UPDATE SET time_zone = EXCLUDED.time_zone
           WHERE NOT playout_settings.zone_set",
    )
    .bind(station.as_uuid())
    .bind(time_zone)
    .execute(pool)
    .await?;
    let (zone,): (String,) =
        sqlx::query_as("SELECT time_zone FROM playout_settings WHERE station_id = $1")
            .bind(station.as_uuid())
            .fetch_one(pool)
            .await?;
    Ok(zone)
}

pub async fn put_settings(
    pool: &PgPool,
    station: StationId,
    s: &PlayoutSettings,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO playout_settings (station_id, time_zone, artist_separation_min,
            title_separation_min, album_separation_min, mode, default_clock, updated_at, zone_set)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now(), true)
         ON CONFLICT (station_id) DO UPDATE SET time_zone = EXCLUDED.time_zone, zone_set = true,
            artist_separation_min = EXCLUDED.artist_separation_min,
            title_separation_min = EXCLUDED.title_separation_min,
            album_separation_min = EXCLUDED.album_separation_min,
            mode = EXCLUDED.mode, default_clock = EXCLUDED.default_clock,
            updated_at = now()",
    )
    .bind(station.as_uuid())
    .bind(&s.time_zone)
    .bind(s.artist_separation_min as i32)
    .bind(s.title_separation_min as i32)
    .bind(s.album_separation_min as i32)
    .bind(&s.mode)
    .bind(s.default_clock)
    .execute(pool)
    .await?;
    Ok(())
}

/// A track in the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Track {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub label: String,
    pub isrc: Option<String>,
    pub category: String,
    /// The media worker's FLAC, `<uuid>.flac`, in the library directory.
    pub file: String,
    pub duration_ms: u32,
    pub gain_db: f32,
    pub loudness_lufs: Option<f32>,
    pub true_peak_dbtp: Option<f32>,
    pub enabled: bool,
}

const TRACK_COLUMNS: &str = "id, title, artist, album, label, isrc, category, file, duration_ms, \
     gain_db, loudness_lufs, true_peak_dbtp, enabled";

fn track_of(r: &sqlx::postgres::PgRow) -> Track {
    Track {
        id: r.get(0),
        title: r.get(1),
        artist: r.get(2),
        album: r.get(3),
        label: r.get(4),
        isrc: r.get(5),
        category: r.get(6),
        file: r.get(7),
        duration_ms: r.get::<i32, _>(8) as u32,
        gain_db: r.get(9),
        loudness_lufs: r.get(10),
        true_peak_dbtp: r.get(11),
        enabled: r.get(12),
    }
}

pub async fn put_track(pool: &PgPool, station: StationId, t: &Track) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO track (id, station_id, title, artist, album, label, isrc, category, file,
            duration_ms, gain_db, loudness_lufs, true_peak_dbtp, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, artist = EXCLUDED.artist,
            album = EXCLUDED.album, label = EXCLUDED.label, isrc = EXCLUDED.isrc,
            category = EXCLUDED.category, enabled = EXCLUDED.enabled
         WHERE track.station_id = EXCLUDED.station_id",
    )
    .bind(t.id)
    .bind(station.as_uuid())
    .bind(&t.title)
    .bind(&t.artist)
    .bind(&t.album)
    .bind(&t.label)
    .bind(&t.isrc)
    .bind(&t.category)
    .bind(&t.file)
    .bind(t.duration_ms as i32)
    .bind(t.gain_db)
    .bind(t.loudness_lufs)
    .bind(t.true_peak_dbtp)
    .bind(t.enabled)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn tracks(pool: &PgPool, station: StationId) -> Result<Vec<Track>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT {TRACK_COLUMNS} FROM track WHERE station_id = $1 ORDER BY artist, title"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(track_of).collect())
}

pub async fn track(
    pool: &PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Track>, sqlx::Error> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT {TRACK_COLUMNS} FROM track WHERE station_id = $1 AND id = $2"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(track_of))
}

/// Remove a track; returns its file, for the caller to delete.
pub async fn delete_track(
    pool: &PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("DELETE FROM track WHERE station_id = $1 AND id = $2 RETURNING file")
        .bind(station.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get(0)))
}

/// A clock, its slots as the JSON the scheduler reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ClockRow {
    pub id: Uuid,
    pub name: String,
    /// `[{"category": "a"}, {"cart": "<uuid>"}]`.
    pub slots_json: String,
}

pub async fn put_clock(pool: &PgPool, station: StationId, c: &ClockRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO clock (id, station_id, name, slots) VALUES ($1, $2, $3, $4::jsonb)
         ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, slots = EXCLUDED.slots
         WHERE clock.station_id = EXCLUDED.station_id",
    )
    .bind(c.id)
    .bind(station.as_uuid())
    .bind(&c.name)
    .bind(&c.slots_json)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clocks(pool: &PgPool, station: StationId) -> Result<Vec<ClockRow>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT id, name, slots::text FROM clock WHERE station_id = $1 ORDER BY name")
            .bind(station.as_uuid())
            .fetch_all(pool)
            .await?;
    Ok(rows
        .iter()
        .map(|r| ClockRow {
            id: r.get(0),
            name: r.get(1),
            slots_json: r.get(2),
        })
        .collect())
}

pub async fn delete_clock(
    pool: &PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query("DELETE FROM clock WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// The week's grid: (weekday, Monday 0; hour; clock).
pub async fn week(pool: &PgPool, station: StationId) -> Result<Vec<(u8, u8, Uuid)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT weekday, hour, clock_id FROM clock_hour WHERE station_id = $1 ORDER BY weekday, hour",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<i16, _>(0) as u8, r.get::<i16, _>(1) as u8, r.get(2)))
        .collect())
}

/// Replace the week's grid.
pub async fn put_week(
    pool: &PgPool,
    station: StationId,
    hours: &[(u8, u8, Uuid)],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM clock_hour WHERE station_id = $1")
        .bind(station.as_uuid())
        .execute(&mut *tx)
        .await?;
    for (d, h, c) in hours {
        sqlx::query(
            "INSERT INTO clock_hour (station_id, weekday, hour, clock_id) VALUES ($1, $2, $3, $4)",
        )
        .bind(station.as_uuid())
        .bind(i16::from(*d))
        .bind(i16::from(*h))
        .bind(c)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// An item of the generated log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LogItem {
    pub id: i64,
    pub planned_at_ms: i64,
    pub hour_ms: i64,
    pub clock: Option<Uuid>,
    pub slot: u16,
    /// `track` or `cart`.
    pub kind: String,
    pub track: Option<Uuid>,
    pub cart: Option<Uuid>,
    pub category: String,
    pub duration_ms: u32,
    /// `planned`, `queued`, `played` or `skipped`.
    pub status: String,
    pub note: String,
}

const ITEM_COLUMNS: &str = "id, (extract(epoch FROM planned_at) * 1000)::bigint, \
     (extract(epoch FROM hour_start) * 1000)::bigint, clock_id, slot, kind, track_id, cart_id, \
     category, duration_ms, status, note";

fn item_of(r: &sqlx::postgres::PgRow) -> LogItem {
    LogItem {
        id: r.get(0),
        planned_at_ms: r.get(1),
        hour_ms: r.get(2),
        clock: r.get(3),
        slot: r.get::<i16, _>(4) as u16,
        kind: r.get(5),
        track: r.get(6),
        cart: r.get(7),
        category: r.get(8),
        duration_ms: r.get::<i32, _>(9) as u32,
        status: r.get(10),
        note: r.get(11),
    }
}

/// Add items (their `id` and `status` are ignored); returns their new identifiers in order.
pub async fn insert_items(
    pool: &PgPool,
    station: StationId,
    items: &[LogItem],
) -> Result<Vec<i64>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut ids = Vec::with_capacity(items.len());
    let sql = format!(
        "INSERT INTO schedule_item (station_id, planned_at, hour_start, clock_id, slot, kind,
            track_id, cart_id, category, duration_ms, note)
         VALUES ($1, {}, {}, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
        ts(2),
        ts(3)
    );
    for i in items {
        let (id,): (i64,) = sqlx::query_as(AssertSqlSafe(sql.as_str()))
            .bind(station.as_uuid())
            .bind(i.planned_at_ms as f64)
            .bind(i.hour_ms as f64)
            .bind(i.clock)
            .bind(i.slot as i16)
            .bind(&i.kind)
            .bind(i.track)
            .bind(i.cart)
            .bind(&i.category)
            .bind(i.duration_ms as i32)
            .bind(&i.note)
            .fetch_one(&mut *tx)
            .await?;
        ids.push(id);
    }
    tx.commit().await?;
    Ok(ids)
}

/// The log's items planned in `[from, to)`, in order.
pub async fn items(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<LogItem>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT {ITEM_COLUMNS} FROM schedule_item
         WHERE station_id = $1 AND planned_at >= {} AND planned_at < {}
         ORDER BY planned_at, id",
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(item_of).collect())
}

pub async fn item(
    pool: &PgPool,
    station: StationId,
    id: i64,
) -> Result<Option<LogItem>, sqlx::Error> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT {ITEM_COLUMNS} FROM schedule_item WHERE station_id = $1 AND id = $2"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(item_of))
}

/// Remove the items still `planned` in `[from, to)` (a regeneration starts there).
pub async fn delete_planned(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(AssertSqlSafe(format!(
        "DELETE FROM schedule_item WHERE station_id = $1 AND status = 'planned'
           AND planned_at >= {} AND planned_at < {}",
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .execute(pool)
    .await?
    .rows_affected())
}

/// The end of the log: when its last item ends, if it has any.
pub async fn log_end(pool: &PgPool, station: StationId) -> Result<Option<i64>, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        "SELECT max((extract(epoch FROM planned_at) * 1000)::bigint + duration_ms)
         FROM schedule_item WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Change one item: its track (the log editor), its status, its note.
pub async fn update_item(
    pool: &PgPool,
    station: StationId,
    id: i64,
    track: Option<(Uuid, u32)>,
    status: Option<&str>,
    note: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE schedule_item SET
            track_id = COALESCE($3, track_id),
            duration_ms = COALESCE($4, duration_ms),
            status = COALESCE($5, status),
            note = COALESCE($6, note)
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(track.map(|t| t.0))
    .bind(track.map(|t| t.1 as i32))
    .bind(status)
    .bind(note)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

pub async fn delete_item(pool: &PgPool, station: StationId, id: i64) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "DELETE FROM schedule_item WHERE station_id = $1 AND id = $2 AND status = 'planned'",
    )
    .bind(station.as_uuid())
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// The next items automation should queue: `planned`, in order, not ending before `since`.
/// Items that ended before `since` without playing (a live show ran over them) are marked
/// `skipped` first.
pub async fn next_to_queue(
    pool: &PgPool,
    station: StationId,
    since_ms: i64,
    limit: i64,
) -> Result<Vec<LogItem>, sqlx::Error> {
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE schedule_item SET status = 'skipped'
         WHERE station_id = $1 AND status IN ('planned', 'queued')
           AND planned_at + make_interval(secs => duration_ms / 1000.0) < {}",
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(since_ms as f64)
    .execute(pool)
    .await?;
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT {ITEM_COLUMNS} FROM schedule_item
         WHERE station_id = $1 AND status = 'planned' ORDER BY planned_at, id LIMIT $2"
    )))
    .bind(station.as_uuid())
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(item_of).collect())
}

/// When each track was last planned or played, for rotation.
pub async fn last_used(pool: &PgPool, station: StationId) -> Result<Vec<(Uuid, i64)>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT track_id, max(t) FROM (
            SELECT track_id, {} AS t FROM schedule_item
              WHERE station_id = $1 AND track_id IS NOT NULL AND status <> 'skipped'
            UNION ALL
            SELECT track_id, {} AS t FROM play_log
              WHERE station_id = $1 AND track_id IS NOT NULL
         ) u GROUP BY track_id",
        ms("planned_at"),
        ms("started_at")
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// A track's play as the separation rules see it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentPlay {
    pub at_ms: i64,
    pub track: Uuid,
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// The tracks planned (not skipped) in `[from, to)`, with their names, in order.
pub async fn planned_tracks(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<RecentPlay>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT {}, t.id, t.title, t.artist, t.album FROM schedule_item i
         JOIN track t ON t.id = i.track_id
         WHERE i.station_id = $1 AND i.status <> 'skipped'
           AND i.planned_at >= {} AND i.planned_at < {}
         ORDER BY i.planned_at, i.id",
        ms("i.planned_at"),
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| RecentPlay {
            at_ms: r.get(0),
            track: r.get(1),
            title: r.get(2),
            artist: r.get(3),
            album: r.get(4),
        })
        .collect())
}

/// One row of the per-play log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Play {
    pub id: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    /// `track`, `cart` or `fallback`.
    pub kind: String,
    pub track: Option<Uuid>,
    pub cart: Option<Uuid>,
    pub item: Option<i64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub label: String,
    pub isrc: Option<String>,
    pub duration_ms: u32,
    pub listeners: u32,
    /// False for a desk cart a dump threw away before listeners heard it.
    pub aired: bool,
}

/// Keep a play; the open plays before it end as it starts (`ends_open`). An event already
/// kept (`source`) is not kept twice: returns `None` then.
pub async fn record_play(
    pool: &PgPool,
    station: StationId,
    p: &Play,
    source: Option<&str>,
    ends_open: bool,
) -> Result<Option<i64>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO play_log (station_id, started_at, kind, track_id, cart_id, item_id, title,
            artist, album, label, isrc, duration_ms, listeners, source)
         VALUES ($1, {}, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (station_id, source) DO NOTHING RETURNING id",
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(p.started_at_ms as f64)
    .bind(&p.kind)
    .bind(p.track)
    .bind(p.cart)
    .bind(p.item)
    .bind(&p.title)
    .bind(&p.artist)
    .bind(&p.album)
    .bind(&p.label)
    .bind(&p.isrc)
    .bind(p.duration_ms as i32)
    .bind(p.listeners as i32)
    .bind(source)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(None);
    };
    let id: i64 = row.get(0);
    if ends_open {
        end_open_in(
            &mut tx,
            station,
            p.started_at_ms,
            Some(id),
            p.kind != "cart",
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Some(id))
}

async fn end_open_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    station: StationId,
    at_ms: i64,
    except: Option<i64>,
    programme_only: bool,
) -> Result<(), sqlx::Error> {
    // A programme item (a track, a fallback file) ends the one before it; carts sound over the
    // programme and end by their own length.
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE play_log SET ended_at = LEAST({}, started_at + make_interval(secs => GREATEST(duration_ms, 0) / 1000.0))
         WHERE station_id = $1 AND ended_at IS NULL AND id IS DISTINCT FROM $3
           AND started_at <= {} AND (NOT $4 OR kind <> 'cart')",
        ts(2),
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(at_ms as f64)
    .bind(except)
    .bind(programme_only)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// End one play (a desk cart's end): when listeners stopped hearing it, and for how long.
pub async fn end_play(
    pool: &PgPool,
    station: StationId,
    id: i64,
    at_ms: i64,
    played_ms: Option<u64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE play_log SET ended_at = {}, duration_ms = COALESCE($4, duration_ms)
         WHERE station_id = $1 AND id = $3",
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(at_ms as f64)
    .bind(id)
    .bind(played_ms.map(|v| v.min(i32::MAX as u64) as i32))
    .execute(pool)
    .await?;
    Ok(())
}

/// Carts listeners would have heard in `(from, to]` were thrown away by a dump: not aired.
pub async fn mark_not_aired(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(AssertSqlSafe(format!(
        "UPDATE play_log SET aired = false
         WHERE station_id = $1 AND kind = 'cart' AND started_at > {} AND started_at <= {}",
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .execute(pool)
    .await?
    .rows_affected())
}

/// End every open programme play at `at` (the desk took the air, or playout stopped).
pub async fn end_open_plays(
    pool: &PgPool,
    station: StationId,
    at_ms: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    end_open_in(&mut tx, station, at_ms, None, true).await?;
    tx.commit().await
}

/// The plays that started in `[from, to)`, in order.
pub async fn plays(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<Play>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT id, {}, {}, kind, track_id, cart_id, item_id, title, artist, album, label, isrc,
                duration_ms, listeners, aired
         FROM play_log WHERE station_id = $1 AND started_at >= {} AND started_at < {}
         ORDER BY started_at, id",
        ms("started_at"),
        ms("ended_at"),
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Play {
            id: r.get(0),
            started_at_ms: r.get(1),
            ended_at_ms: r.get(2),
            kind: r.get(3),
            track: r.get(4),
            cart: r.get(5),
            item: r.get(6),
            title: r.get(7),
            artist: r.get(8),
            album: r.get(9),
            label: r.get(10),
            isrc: r.get(11),
            duration_ms: r.get::<i32, _>(12) as u32,
            listeners: r.get::<i32, _>(13) as u32,
            aired: r.get(14),
        })
        .collect())
}

/// Delete the plays that ended before `before_ms` (retention's per-play period, ADR-0113),
/// except those a hold covers (ADR-0116: `per_play` by number, or a held stretch of broadcast
/// time). Returns how many went.
pub async fn sweep_plays(
    pool: &PgPool,
    station: StationId,
    before_ms: i64,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(AssertSqlSafe(format!(
        "DELETE FROM play_log p WHERE p.station_id = $1
           AND coalesce(p.ended_at, p.started_at) < {}
           AND NOT retention_held(p.station_id, 'per_play', p.id, p.started_at, p.ended_at)",
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(before_ms as f64)
    .execute(pool)
    .await?
    .rows_affected())
}

/// A registered recording by its file (relative to the recordings directory): its number and
/// whether it is still open.
pub async fn recording_by_file(
    pool: &PgPool,
    station: StationId,
    file: &str,
) -> Result<Option<(i64, bool)>, sqlx::Error> {
    let row: Option<(i64, bool)> = sqlx::query_as(
        "SELECT id, ended_at IS NULL FROM recording WHERE station_id = $1 AND file = $2
         ORDER BY id DESC LIMIT 1",
    )
    .bind(station.as_uuid())
    .bind(file)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Add to an hour's anonymous totals; the peak only rises.
pub async fn add_listening(
    pool: &PgPool,
    station: StationId,
    hour_ms: i64,
    seconds: i64,
    sessions: i32,
    peak: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO listener_hour (station_id, hour, listener_seconds, sessions, peak)
         VALUES ($1, {}, $3, $4, $5)
         ON CONFLICT (station_id, hour) DO UPDATE SET
            listener_seconds = listener_hour.listener_seconds + EXCLUDED.listener_seconds,
            sessions = listener_hour.sessions + EXCLUDED.sessions,
            peak = GREATEST(listener_hour.peak, EXCLUDED.peak)",
        ts(2)
    )))
    .bind(station.as_uuid())
    .bind(hour_ms as f64)
    .bind(seconds.max(0))
    .bind(sessions.max(0))
    .bind(peak.max(0))
    .execute(pool)
    .await?;
    Ok(())
}

/// An hour's anonymous totals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ListenerHour {
    pub hour_ms: i64,
    pub listener_seconds: i64,
    pub sessions: u32,
    pub peak: u32,
}

pub async fn listening(
    pool: &PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<ListenerHour>, sqlx::Error> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT {}, listener_seconds, sessions, peak FROM listener_hour
         WHERE station_id = $1 AND hour >= {} AND hour < {} ORDER BY hour",
        ms("hour"),
        ts(2),
        ts(3)
    )))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ListenerHour {
            hour_ms: r.get(0),
            listener_seconds: r.get(1),
            sessions: r.get::<i32, _>(2) as u32,
            peak: r.get::<i32, _>(3) as u32,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamps_bind_by_position() {
        assert_eq!(super::ts(3), "to_timestamp($3::double precision / 1000.0)");
        assert!(super::ms("x").contains("extract(epoch FROM x)"));
    }
}
