// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Hosts in their own studios (ADR-0280, ADR-0284; migration 0040): show keys, the station's
//! rules for sources, the log of each connection, and the one rule that decides whether a
//! connection stands by, is a private test, or is refused: the slot's **door**.
//!
//! **The door** (design section 2): a show's slot is open from `door_before_min` (30) before
//! each of its airings until the show after it has taken the air, or two hours after its end,
//! whichever is first, and never before its own end. A skipped date has no airing and so no
//! door; a moved one has its door at its new time (ADR-0278), because the airings are the
//! episodes the schedule holds.
//!
//! **After the door closes** (the operator's answer, 2026-11-16): a source that is standing by
//! and not on air when its door closes becomes a private test ([`after_door`]): taken off the
//! desk, its connection kept, and its time limit starting then. A source on air is never touched.
//!
//! The control plane derives a key's secret from the salt kept here (`crates/control/src/
//! sources.rs`); nothing in this module sees a secret.

use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::StationId;

/// How long after its end a slot's door stays open at most.
pub const DOOR_AFTER_S: i64 = 2 * 3600;
/// How long `source_log` rows are kept.
pub const LOG_KEEP_MS: i64 = 30 * 24 * 3600 * 1000;

// ---- the door ----

/// One airing on the station's schedule, `[start, end)` in epoch seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Airing {
    pub show: Uuid,
    pub start: i64,
    pub end: i64,
}

/// Where a show's door stands at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// Open, for the airing `[start, end)`; it closes at `close`.
    Open { start: i64, end: i64, close: i64 },
    /// Closed; the show's next airing starts at `next` (it opens `door_before` earlier), if the
    /// schedule has one in the range it was given.
    Closed { next: Option<i64> },
}

/// The door of `show` at `now_s`, from every airing of the station in air order (the doors
/// close when the next show, any show, takes the air).
pub fn door(show: Uuid, airings: &[Airing], now_s: i64, door_before_s: i64) -> Door {
    let mut next = None;
    for (i, a) in airings.iter().enumerate() {
        if a.show != show {
            continue;
        }
        let after = airings
            .iter()
            .skip(i + 1)
            .filter(|b| b.show != show && b.start >= a.end)
            .map(|b| b.start)
            .min();
        let close = after
            .unwrap_or(i64::MAX)
            .min(a.end + DOOR_AFTER_S)
            .max(a.end);
        if a.start - door_before_s <= now_s && now_s < close {
            return Door::Open {
                start: a.start,
                end: a.end,
                close,
            };
        }
        if a.start > now_s && next.is_none_or(|n| a.start < n) {
            next = Some(a.start);
        }
    }
    Door::Closed { next }
}

// ---- admission ----

/// Why a connection was refused: a word for the log and the API, and the sentence the monitor
/// link and Settings show (design section 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// No such key, or the secret is wrong.
    WrongKey,
    /// The key has ended.
    Ended,
    /// Outside the slot, with tests off.
    NotYourSlot,
    /// Another connection is sending with this key.
    InUse,
    /// Too many wrong keys from this address lately.
    TooMany,
    /// Turned away from the desk for the rest of today's show (the tile's Disconnect).
    TurnedAway,
    /// What the software sends is not a format the station reads.
    Format,
    /// Every slot a source can take is in use.
    DeskFull,
    /// More than 2 Mbit/s.
    TooFast,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::WrongKey => "wrong_key",
            Refusal::Ended => "ended",
            Refusal::NotYourSlot => "not_your_slot",
            Refusal::InUse => "in_use",
            Refusal::TooMany => "too_many",
            Refusal::TurnedAway => "turned_away",
            Refusal::Format => "format",
            Refusal::DeskFull => "desk_full",
            Refusal::TooFast => "too_fast",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        [
            Refusal::WrongKey,
            Refusal::Ended,
            Refusal::NotYourSlot,
            Refusal::InUse,
            Refusal::TooMany,
            Refusal::TurnedAway,
            Refusal::Format,
            Refusal::DeskFull,
            Refusal::TooFast,
        ]
        .into_iter()
        .find(|r| r.as_str() == word)
    }

    /// The sentence a host reads.
    pub fn words(self) -> &'static str {
        match self {
            Refusal::WrongKey => "The key is wrong.",
            Refusal::Ended => "This key has ended. Ask the station for new details.",
            Refusal::NotYourSlot => "It is not your slot.",
            Refusal::InUse => {
                "Another connection is using this key. Close your other software first."
            }
            Refusal::TooMany => "Too many wrong keys from your address. Try again later.",
            Refusal::TurnedAway => {
                "The station has turned this key away for the rest of today's show."
            }
            Refusal::Format => {
                "The station cannot read what your software sends. Set it to MP3, AAC or Opus."
            }
            Refusal::DeskFull => "Every place on the station's desk is taken.",
            Refusal::TooFast => "Your software sends more than 2 Mbit/s. Lower its bit rate.",
        }
    }
}

/// What a key may do now, before the desk is asked for a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// In its slot: stand by on the desk.
    StandBy {
        door: Door,
    },
    /// Outside its slot: a private test, at most this long.
    Test {
        max_ms: i64,
    },
    Refused(Refusal),
}

/// The decision for a key that has already proved its secret.
pub fn admit(key: &ShowKey, door: Door, settings: &SourceSettings, now_ms: i64) -> Admission {
    if key.revoked_ms.is_some_and(|r| r <= now_ms) {
        return Admission::Refused(Refusal::Ended);
    }
    if key.turned_away_until_ms.is_some_and(|t| now_ms < t) {
        return Admission::Refused(Refusal::TurnedAway);
    }
    match door {
        Door::Open { .. } => Admission::StandBy { door },
        Door::Closed { .. } if settings.test_outside => Admission::Test {
            max_ms: i64::from(settings.test_max_min) * 60_000,
        },
        Door::Closed { .. } => Admission::Refused(Refusal::NotYourSlot),
    }
}

// ---- after the door ----

/// The log's word for why a standing-by source became a private test.
pub const DOOR_CLOSED_DETAIL: &str = "the slot's door closed";
/// The sentence the host reads (the monitor link and Settings show it; the link is not built yet).
pub const DOOR_CLOSED_WORDS: &str =
    "Your slot has ended, so you are now in a private test. The station cannot hear you.";

/// What happens to a connection standing by on the desk when its slot's door has closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterDoor {
    /// Nothing: the door is open, or the source is on air.
    Stay,
    /// A private test now, for at most this long.
    Test { max_ms: i64 },
    /// Tests outside the slot are off: the connection is closed.
    Close,
}

/// What to do with a source that stands by (`on_air` says whether the desk has it on the air).
/// One on air is never touched, and the next look finds it off the air if it is then.
pub fn after_door(on_air: bool, door: Door, settings: &SourceSettings) -> AfterDoor {
    match door {
        Door::Open { .. } => AfterDoor::Stay,
        Door::Closed { .. } if on_air => AfterDoor::Stay,
        Door::Closed { .. } if settings.test_outside => AfterDoor::Test {
            max_ms: i64::from(settings.test_max_min) * 60_000,
        },
        Door::Closed { .. } => AfterDoor::Close,
    }
}

// ---- wrong keys by address ----

/// Whether an address is one a host could not reach us from over the open internet: private
/// (10/8, 172.16/12, 192.168/16), loopback, link-local, unique-local (fc00::/7), carrier-grade
/// NAT (100.64/10) and the unspecified address. Docker's and Podman's ranges are within these.
/// Anything that does not parse as an address counts as public, so it can still be blocked.
pub fn is_private_address(addr: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let addr = addr.trim().trim_start_matches('[').trim_end_matches(']');
    let v4 = |a: Ipv4Addr| {
        let o = a.octets();
        a.is_private()
            || a.is_loopback()
            || a.is_link_local()
            || a.is_unspecified()
            || (o[0] == 100 && (64..128).contains(&o[1]))
    };
    let v6 = |a: Ipv6Addr| {
        if let Some(m) = a.to_ipv4_mapped() {
            return v4(m);
        }
        let first = a.segments()[0];
        a.is_loopback()
            || a.is_unspecified()
            || first & 0xfe00 == 0xfc00 // unique-local, fc00::/7
            || first & 0xffc0 == 0xfe80 // link-local, fe80::/10
    };
    match addr.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => v4(a),
        Ok(IpAddr::V6(a)) => v6(a),
        Err(_) => false,
    }
}

/// Wrong keys counted by address (design section 10: five a minute, then ten minutes refused).
/// An address that is not public is counted and logged but never blocked: behind a proxy or on
/// a home network it stands for the host's own people.
#[derive(Debug, Default)]
pub struct WrongKeys {
    by_addr: std::collections::HashMap<String, (Vec<i64>, Option<i64>)>,
}

impl WrongKeys {
    pub const PER_MINUTE: usize = 5;
    pub const BLOCK_MS: i64 = 10 * 60 * 1000;

    /// Until when `addr` is refused, if it is.
    pub fn blocked_until(&self, addr: &str, now_ms: i64) -> Option<i64> {
        self.by_addr
            .get(addr)
            .and_then(|(_, until)| *until)
            .filter(|&u| now_ms < u)
    }

    /// Count a wrong key from `addr`; true when that blocks it (never for a private address).
    pub fn wrong(&mut self, addr: &str, now_ms: i64) -> bool {
        if self.by_addr.len() > 10_000 {
            self.by_addr
                .retain(|_, (times, until)| until.is_some_and(|u| now_ms < u) || !times.is_empty());
        }
        let (times, until) = self.by_addr.entry(addr.to_owned()).or_default();
        times.retain(|t| now_ms - t < 60_000);
        times.push(now_ms);
        if times.len() >= Self::PER_MINUTE {
            times.clear();
            if is_private_address(addr) {
                return false;
            }
            *until = Some(now_ms + Self::BLOCK_MS);
            return true;
        }
        false
    }
}

// ---- keys ----

/// A show key as it is kept (never its secret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ShowKey {
    pub id: Uuid,
    pub show_id: Uuid,
    /// Who it is for: their name on the desk.
    pub label: String,
    /// The member of staff it is for, if one.
    pub staff_id: Option<Uuid>,
    /// The public part: the SRT stream id, and the Icecast password's first part.
    pub key_id: String,
    /// How the host means to send: `icecast` or `srt` (a hint, not a lock).
    pub hint: String,
    pub created_by: String,
    pub created_ms: i64,
    pub last_used_ms: Option<i64>,
    /// When it ended, if it has.
    pub revoked_ms: Option<i64>,
    /// It ends when its connection closes (made again with `old: on-disconnect`).
    pub ends_on_disconnect: bool,
    /// Turned away from the desk until then (the tile's Disconnect).
    pub turned_away_until_ms: Option<i64>,
    pub turned_away_by: Option<String>,
    #[serde(skip)]
    pub key_salt: Vec<u8>,
    #[serde(skip)]
    pub monitor_hash: Vec<u8>,
}

pub struct NewShowKey<'a> {
    pub id: Uuid,
    pub show_id: Uuid,
    pub label: &'a str,
    pub staff_id: Option<Uuid>,
    pub key_id: &'a str,
    pub key_salt: &'a [u8; 16],
    pub monitor_hash: &'a [u8; 32],
    pub hint: &'a str,
    pub created_by: &'a str,
    pub created_ms: i64,
}

macro_rules! key_columns {
    () => {
        "id, show_id, label, staff_id, key_id, hint, created_by, created_ms, last_used_ms, revoked_ms, ends_on_disconnect, turned_away_until_ms, turned_away_by, key_salt, monitor_hash"
    };
}

fn key_of(r: &sqlx::postgres::PgRow) -> Result<ShowKey, sqlx::Error> {
    Ok(ShowKey {
        id: r.try_get("id")?,
        show_id: r.try_get("show_id")?,
        label: r.try_get("label")?,
        staff_id: r.try_get("staff_id")?,
        key_id: r.try_get("key_id")?,
        hint: r.try_get("hint")?,
        created_by: r.try_get("created_by")?,
        created_ms: r.try_get("created_ms")?,
        last_used_ms: r.try_get("last_used_ms")?,
        revoked_ms: r.try_get("revoked_ms")?,
        ends_on_disconnect: r.try_get("ends_on_disconnect")?,
        turned_away_until_ms: r.try_get("turned_away_until_ms")?,
        turned_away_by: r.try_get("turned_away_by")?,
        key_salt: r.try_get("key_salt")?,
        monitor_hash: r.try_get("monitor_hash")?,
    })
}

/// Make a key, and mark its show as one that comes from a host's own studio.
pub async fn create_key(
    pool: &sqlx::PgPool,
    station: StationId,
    k: &NewShowKey<'_>,
) -> Result<ShowKey, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(concat!("INSERT INTO show_key (id, station_id, show_id, label, staff_id, key_id, key_salt, monitor_hash, hint, created_by, created_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING ",
        key_columns!(),
        ""))
    .bind(k.id)
    .bind(station.as_uuid())
    .bind(k.show_id)
    .bind(k.label)
    .bind(k.staff_id)
    .bind(k.key_id)
    .bind(&k.key_salt[..])
    .bind(&k.monitor_hash[..])
    .bind(k.hint)
    .bind(k.created_by)
    .bind(k.created_ms)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE show SET source = 'push' WHERE id = $1 AND station_id = $2")
        .bind(k.show_id)
        .bind(station.as_uuid())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    key_of(&row)
}

/// A show's keys, newest first, ended ones included.
pub async fn list_keys(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
) -> Result<Vec<ShowKey>, sqlx::Error> {
    sqlx::query(concat!(
        "SELECT ",
        key_columns!(),
        " FROM show_key WHERE station_id = $1 AND show_id = $2 ORDER BY created_ms DESC, key_id"
    ))
    .bind(station.as_uuid())
    .bind(show)
    .fetch_all(pool)
    .await?
    .iter()
    .map(key_of)
    .collect()
}

/// Every key the station has that has not ended.
pub async fn live_keys(
    pool: &sqlx::PgPool,
    station: StationId,
    now_ms: i64,
) -> Result<Vec<ShowKey>, sqlx::Error> {
    sqlx::query(concat!("SELECT ",
        key_columns!(),
        " FROM show_key WHERE station_id = $1 AND (revoked_ms IS NULL OR revoked_ms > $2) ORDER BY created_ms"))
    .bind(station.as_uuid())
    .bind(now_ms)
    .fetch_all(pool)
    .await?
    .iter()
    .map(key_of)
    .collect()
}

/// A key by its public id.
pub async fn key_by_key_id(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
) -> Result<Option<ShowKey>, sqlx::Error> {
    sqlx::query(concat!(
        "SELECT ",
        key_columns!(),
        " FROM show_key WHERE station_id = $1 AND key_id = $2"
    ))
    .bind(station.as_uuid())
    .bind(key_id)
    .fetch_optional(pool)
    .await?
    .as_ref()
    .map(key_of)
    .transpose()
}

/// End a key now; when it was the show's last live key, the show comes from the room again.
/// `false` when it had already ended.
pub async fn end_key(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
    now_ms: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let show: Option<Uuid> = sqlx::query_scalar(
        "UPDATE show_key SET revoked_ms = $3 WHERE station_id = $1 AND key_id = $2
         AND (revoked_ms IS NULL OR revoked_ms > $3) RETURNING show_id",
    )
    .bind(station.as_uuid())
    .bind(key_id)
    .bind(now_ms)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(show) = show {
        sqlx::query(
            "UPDATE show SET source = 'room' WHERE id = $1 AND NOT EXISTS (
               SELECT 1 FROM show_key WHERE show_id = $1 AND (revoked_ms IS NULL OR revoked_ms > $2))",
        )
        .bind(show)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(show.is_some())
}

/// Mark a key to end when its connection closes (a new key made with `old: on-disconnect`).
pub async fn end_on_disconnect(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE show_key SET ends_on_disconnect = true WHERE station_id = $1 AND key_id = $2",
    )
    .bind(station.as_uuid())
    .bind(key_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Turn a key away from the desk until `until_ms` (`None` lets it back in).
pub async fn turn_away(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
    until_ms: Option<i64>,
    by: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE show_key SET turned_away_until_ms = $3, turned_away_by = $4 WHERE station_id = $1 AND key_id = $2",
    )
    .bind(station.as_uuid())
    .bind(key_id)
    .bind(until_ms)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Note that a key was used now.
pub async fn touch_key(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
    now_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE show_key SET last_used_ms = $3 WHERE station_id = $1 AND key_id = $2")
        .bind(station.as_uuid())
        .bind(key_id)
        .bind(now_ms)
        .execute(pool)
        .await?;
    Ok(())
}

/// Where a show comes from: `room` or `push`.
pub async fn show_source(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT source FROM show WHERE id = $1 AND station_id = $2")
        .bind(show)
        .bind(station.as_uuid())
        .fetch_one(pool)
        .await
}

// ---- the log ----

/// One line of what happened to a source connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = SourceLogEntry)]
pub struct LogEntry {
    pub at_ms: i64,
    pub key_id: String,
    /// `admitted`, `test`, `refused`, `ended`, `disconnected`, `let_back`, or one the post's rule
    /// writes (`on_air`, `off_air`, `link_lost`, `back`, `place_let_go`, `covered`, `go`, `not_yet`,
    /// `hold`, `hold_released`).
    pub event: String,
    /// For a refusal: its word ([`Refusal::as_str`]).
    pub reason: Option<String>,
    #[serde(default)]
    pub detail: String,
    /// `icecast` or `srt`.
    pub way: Option<String>,
    /// The address it came from, as the ingest saw it (the real one, not a proxy's).
    #[serde(skip_serializing)]
    pub address: Option<String>,
}

/// Write a line, and drop lines older than [`LOG_KEEP_MS`].
pub async fn log(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Option<Uuid>,
    e: &LogEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO source_log (station_id, at_ms, key_id, show_id, event, reason, detail, way, address)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(station.as_uuid())
    .bind(e.at_ms)
    .bind(e.key_id.chars().take(80).collect::<String>())
    .bind(show)
    .bind(&e.event)
    .bind(&e.reason)
    .bind(e.detail.chars().take(400).collect::<String>())
    .bind(&e.way)
    .bind(&e.address)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM source_log WHERE station_id = $1 AND at_ms < $2")
        .bind(station.as_uuid())
        .bind(e.at_ms - LOG_KEEP_MS)
        .execute(pool)
        .await?;
    Ok(())
}

/// A key's latest lines, newest first.
pub async fn key_log(
    pool: &sqlx::PgPool,
    station: StationId,
    key_id: &str,
    limit: i64,
) -> Result<Vec<LogEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT at_ms, key_id, event, reason, detail, way, address FROM source_log
         WHERE station_id = $1 AND key_id = $2 ORDER BY at_ms DESC, id DESC LIMIT $3",
    )
    .bind(station.as_uuid())
    .bind(key_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(LogEntry {
                at_ms: r.try_get("at_ms")?,
                key_id: r.try_get("key_id")?,
                event: r.try_get("event")?,
                reason: r.try_get("reason")?,
                detail: r.try_get("detail")?,
                way: r.try_get("way")?,
                address: r.try_get("address")?,
            })
        })
        .collect()
}

/// The lines the post's rule reads (`crate::post::POST_EVENTS` and a hold's release) since
/// `since_ms`, newest first.
pub async fn post_log(
    pool: &sqlx::PgPool,
    station: StationId,
    since_ms: i64,
) -> Result<Vec<LogEntry>, sqlx::Error> {
    let mut events: Vec<&str> = crate::post::POST_EVENTS.to_vec();
    events.push("hold_released");
    let rows = sqlx::query(
        "SELECT at_ms, key_id, event, reason, detail, way, address FROM source_log
         WHERE station_id = $1 AND at_ms >= $2 AND event = ANY($3) ORDER BY at_ms DESC, id DESC",
    )
    .bind(station.as_uuid())
    .bind(since_ms)
    .bind(&events)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(LogEntry {
                at_ms: r.try_get("at_ms")?,
                key_id: r.try_get("key_id")?,
                event: r.try_get("event")?,
                reason: r.try_get("reason")?,
                detail: r.try_get("detail")?,
                way: r.try_get("way")?,
                address: r.try_get("address")?,
            })
        })
        .collect()
}

// ---- the rules ----

/// The station's rules for sources (design section 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSettings {
    /// How early a host may connect before their show, in minutes (0 to 240).
    pub door_before_min: i32,
    /// The grace at the post, in seconds: 0 (off) to 600; the studio offers 60, 120 and 300.
    pub grace_s: i32,
    /// A late host's count onto automation, in seconds; 0 is off.
    pub late_count_s: i32,
    /// Outside the slot, take a connection as a private test.
    pub test_outside: bool,
    /// How long a test lasts at most, in minutes.
    pub test_max_min: i32,
    /// How long a source must be steady to stand by, in seconds.
    pub steady_s: i32,
    /// How long a link may be lost before automation covers, in seconds.
    pub silence_s: i32,
    /// How long a lost source's place is kept, in seconds.
    pub place_kept_s: i32,
    /// The crossfade when the schedule or End show moves a source, in ms.
    pub fade_ms: i32,
    /// The ingest's Icecast port (TCP).
    pub port_icecast: i32,
    /// The ingest's SRT port (UDP).
    pub port_srt: i32,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            door_before_min: 30,
            grace_s: 120,
            late_count_s: 20,
            test_outside: true,
            test_max_min: 15,
            steady_s: 5,
            silence_s: 4,
            place_kept_s: 300,
            fade_ms: 2000,
            port_icecast: 8005,
            port_srt: 9710,
        }
    }
}

impl SourceSettings {
    /// Which field is wrong and why, if one is.
    pub fn check(&self) -> Result<(), (&'static str, &'static str)> {
        let within = |v: i32, lo: i32, hi: i32| (lo..=hi).contains(&v);
        if !within(self.door_before_min, 0, 240) {
            return Err(("door_before_min", "minutes, 0 to 240"));
        }
        if !within(self.grace_s, 0, 600) {
            return Err((
                "grace_s",
                "seconds, 0 (off) to 600; the studio offers 60, 120 and 300",
            ));
        }
        if !within(self.late_count_s, 0, 120) {
            return Err(("late_count_s", "seconds, 0 (off) to 120"));
        }
        if !within(self.test_max_min, 1, 60) {
            return Err(("test_max_min", "minutes, 1 to 60"));
        }
        if !within(self.steady_s, 1, 60) || !within(self.silence_s, 1, 30) {
            return Err(("steady_s", "steady_s 1 to 60 and silence_s 1 to 30 seconds"));
        }
        if !within(self.place_kept_s, 0, 3600) {
            return Err(("place_kept_s", "seconds, 0 to 3600"));
        }
        if !within(self.fade_ms, 0, 10_000) {
            return Err(("fade_ms", "ms, 0 to 10000"));
        }
        if !within(self.port_icecast, 1, 65_535) || !within(self.port_srt, 1, 65_535) {
            return Err(("port_icecast", "ports are 1 to 65535"));
        }
        Ok(())
    }
}

pub async fn get_settings(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<SourceSettings, sqlx::Error> {
    let row = sqlx::query(
        "SELECT door_before_min, grace_s, late_count_s, test_outside, test_max_min, steady_s, silence_s, place_kept_s, fade_ms, port_icecast, port_srt
         FROM source_settings WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(SourceSettings::default());
    };
    Ok(SourceSettings {
        door_before_min: r.try_get("door_before_min")?,
        grace_s: r.try_get("grace_s")?,
        late_count_s: r.try_get("late_count_s")?,
        test_outside: r.try_get("test_outside")?,
        test_max_min: r.try_get("test_max_min")?,
        steady_s: r.try_get("steady_s")?,
        silence_s: r.try_get("silence_s")?,
        place_kept_s: r.try_get("place_kept_s")?,
        fade_ms: r.try_get("fade_ms")?,
        port_icecast: r.try_get("port_icecast")?,
        port_srt: r.try_get("port_srt")?,
    })
}

pub async fn put_settings(
    pool: &sqlx::PgPool,
    station: StationId,
    s: &SourceSettings,
    by: &str,
    now_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO source_settings (station_id, door_before_min, grace_s, late_count_s, test_outside, test_max_min, steady_s, silence_s, place_kept_s, fade_ms, port_icecast, port_srt, updated_by, updated_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (station_id) DO UPDATE SET door_before_min = $2, grace_s = $3, late_count_s = $4, test_outside = $5,
           test_max_min = $6, steady_s = $7, silence_s = $8, place_kept_s = $9, fade_ms = $10, port_icecast = $11,
           port_srt = $12, updated_by = $13, updated_ms = $14",
    )
    .bind(station.as_uuid())
    .bind(s.door_before_min)
    .bind(s.grace_s)
    .bind(s.late_count_s)
    .bind(s.test_outside)
    .bind(s.test_max_min)
    .bind(s.steady_s)
    .bind(s.silence_s)
    .bind(s.place_kept_s)
    .bind(s.fade_ms)
    .bind(s.port_icecast)
    .bind(s.port_srt)
    .bind(by)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;

    fn airing(show: Uuid, start: i64, hours: i64) -> Airing {
        Airing {
            show,
            start,
            end: start + hours * H,
        }
    }

    #[test]
    fn the_door_opens_thirty_minutes_before_and_shuts_when_the_next_show_airs() {
        let (lunch, steeped, drive) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        // 12:00 to 14:00, 14:00 to 16:00, 16:00 to 18:00 (hours from 0).
        let a = [
            airing(lunch, 12 * H, 2),
            airing(steeped, 14 * H, 2),
            airing(drive, 16 * H, 2),
        ];
        let before = 30 * 60;
        assert_eq!(
            door(steeped, &a, 13 * H + 29 * 60, before),
            Door::Closed { next: Some(14 * H) }
        );
        let open = Door::Open {
            start: 14 * H,
            end: 16 * H,
            close: 16 * H,
        };
        assert_eq!(door(steeped, &a, 13 * H + 30 * 60, before), open);
        assert_eq!(door(steeped, &a, 15 * H, before), open);
        // Drive takes the air at 16:00: Steeped's door shuts then.
        assert_eq!(
            door(steeped, &a, 16 * H, before),
            Door::Closed { next: None }
        );
        // The last show of the day keeps its door two hours past its end.
        let late = door(drive, &a, 19 * H, before);
        assert!(matches!(late, Door::Open { close, .. } if close == 20 * H));
        assert_eq!(door(drive, &a, 20 * H, before), Door::Closed { next: None });
    }

    #[test]
    fn a_show_with_no_airing_that_day_has_no_door() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let airings = [airing(a, 10 * H, 1), airing(a, 34 * H, 1)];
        assert_eq!(door(b, &airings, 10 * H, 1800), Door::Closed { next: None });
        assert_eq!(
            door(a, &airings, 20 * H, 1800),
            Door::Closed { next: Some(34 * H) }
        );
    }

    fn key() -> ShowKey {
        ShowKey {
            id: Uuid::new_v4(),
            show_id: Uuid::new_v4(),
            label: "Finlay Grant".into(),
            staff_id: None,
            key_id: "abcd2345".into(),
            hint: "srt".into(),
            created_by: "test".into(),
            created_ms: 0,
            last_used_ms: None,
            revoked_ms: None,
            ends_on_disconnect: false,
            turned_away_until_ms: None,
            turned_away_by: None,
            key_salt: vec![0; 16],
            monitor_hash: vec![0; 32],
        }
    }

    #[test]
    fn in_the_slot_a_key_stands_by_outside_it_tests_or_is_refused() {
        let open = Door::Open {
            start: 100,
            end: 200,
            close: 200,
        };
        let shut = Door::Closed { next: Some(500) };
        let s = SourceSettings::default();
        assert_eq!(
            admit(&key(), open, &s, 150_000),
            Admission::StandBy { door: open }
        );
        assert_eq!(
            admit(&key(), shut, &s, 0),
            Admission::Test {
                max_ms: 15 * 60_000
            }
        );
        let no_tests = SourceSettings {
            test_outside: false,
            ..s
        };
        assert_eq!(
            admit(&key(), shut, &no_tests, 0),
            Admission::Refused(Refusal::NotYourSlot)
        );
        let ended = ShowKey {
            revoked_ms: Some(10),
            ..key()
        };
        assert_eq!(
            admit(&ended, open, &s, 11),
            Admission::Refused(Refusal::Ended)
        );
        let away = ShowKey {
            turned_away_until_ms: Some(1_000),
            ..key()
        };
        assert_eq!(
            admit(&away, open, &s, 999),
            Admission::Refused(Refusal::TurnedAway)
        );
        assert_eq!(
            admit(&away, open, &s, 1_000),
            Admission::StandBy { door: open }
        );
    }

    #[test]
    fn five_wrong_keys_in_a_minute_block_an_address_for_ten_minutes() {
        let mut w = WrongKeys::default();
        for i in 0..4 {
            assert!(!w.wrong("192.0.2.7", i * 1000));
        }
        assert_eq!(w.blocked_until("192.0.2.7", 4_000), None);
        assert!(w.wrong("192.0.2.7", 5_000));
        assert_eq!(w.blocked_until("192.0.2.7", 6_000), Some(605_000));
        assert_eq!(w.blocked_until("192.0.2.8", 6_000), None);
        assert_eq!(w.blocked_until("192.0.2.7", 605_000), None);
        // Spread out, they never block.
        let mut w = WrongKeys::default();
        for i in 0..20 {
            assert!(!w.wrong("192.0.2.9", i * 20_000));
        }
    }

    #[test]
    fn a_private_address_is_counted_but_never_blocked() {
        for a in [
            "10.0.0.5",
            "172.17.0.1",
            "192.168.1.20",
            "127.0.0.1",
            "169.254.3.4",
            "100.64.0.9",
            "100.127.255.1",
            "::1",
            "fe80::1",
            "fd12:3456::7",
            "fc00::1",
            "::ffff:10.1.2.3",
        ] {
            assert!(is_private_address(a), "{a}");
            let mut w = WrongKeys::default();
            for i in 0..20 {
                assert!(!w.wrong(a, i * 1000), "{a}");
            }
            assert_eq!(w.blocked_until(a, 20_000), None, "{a}");
        }
        for a in [
            "192.0.2.7",
            "8.8.8.8",
            "100.128.0.1",
            "172.32.0.1",
            "2001:db8::1",
            "2606:4700::1111",
            "not an address",
        ] {
            assert!(!is_private_address(a), "{a}");
        }
        // A public one still blocks on the fifth.
        let mut w = WrongKeys::default();
        for i in 0..4 {
            assert!(!w.wrong("2606:4700::1111", i));
        }
        assert!(w.wrong("2606:4700::1111", 5));
        assert_eq!(w.blocked_until("2606:4700::1111", 6), Some(5 + 600_000));
    }

    #[test]
    fn a_closed_door_turns_a_standing_by_source_into_a_test_but_never_one_on_air() {
        let open = Door::Open {
            start: 100,
            end: 200,
            close: 200,
        };
        let shut = Door::Closed { next: Some(500) };
        let s = SourceSettings::default();
        assert_eq!(after_door(false, open, &s), AfterDoor::Stay);
        assert_eq!(after_door(true, open, &s), AfterDoor::Stay);
        assert_eq!(
            after_door(false, shut, &s),
            AfterDoor::Test {
                max_ms: 15 * 60_000
            }
        );
        assert_eq!(after_door(true, shut, &s), AfterDoor::Stay);
        // The limit is the station's own.
        let long = SourceSettings {
            test_max_min: 40,
            ..s
        };
        assert_eq!(
            after_door(false, Door::Closed { next: None }, &long),
            AfterDoor::Test {
                max_ms: 40 * 60_000
            }
        );
        // With tests outside the slot off there is no test to become: the connection closes.
        let no_tests = SourceSettings {
            test_outside: false,
            ..s
        };
        assert_eq!(after_door(false, shut, &no_tests), AfterDoor::Close);
        assert_eq!(after_door(true, shut, &no_tests), AfterDoor::Stay);
    }

    #[test]
    fn refusals_round_trip_and_settings_are_checked() {
        for r in [Refusal::WrongKey, Refusal::InUse, Refusal::TooFast] {
            assert_eq!(Refusal::parse(r.as_str()), Some(r));
        }
        assert!(SourceSettings::default().check().is_ok());
        let bad = SourceSettings {
            grace_s: 700,
            ..Default::default()
        };
        assert_eq!(bad.check().unwrap_err().0, "grace_s");
    }
}
