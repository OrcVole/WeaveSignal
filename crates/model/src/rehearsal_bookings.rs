// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Bookings of the rehearsal studio, and the studio's past and open sessions over a stretch of
//! time (1.3.0, #24, step R14): what the schedule's Studios view draws in its Rehearsal column.
//!
//! The rehearsal studio is first come, with optional bookings (question 6, answer a): a booking
//! holds the studio for its show from its start to its end, and Open for another show is refused
//! while one holds it. Two bookings never overlap: a write locks the station's row and checks
//! the others ([`crate::rehearsal::clash`]) in the same transaction, so two writes at once cannot
//! both pass (the table has no exclusion constraint: that needs `btree_gist`, which the Cloudron
//! package's PostgreSQL may not offer). The table is `rehearsal_booking` (migration 0070,
//! provisional).
//!
//! The design is `docs/reference/design/rehearsal-mode-build-plan.md` in the project folder
//! (sections 1.11, 1.12 and 2).

use sqlx::Row;
use uuid::Uuid;

use crate::StationId;
use crate::rehearsal::{Booking, Session, clash};

/// The longest booking: a day. A rehearsal is hours, not days; a longer one is a slip of the
/// date.
pub const LONGEST_MS: i64 = 24 * 3_600_000;

/// The shortest booking: five minutes.
pub const SHORTEST_MS: i64 = 5 * 60_000;

/// A booking with its show's names, as the routes and the week give it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookingRow {
    pub booking: Booking,
    pub show_slug: String,
    pub show_name: String,
    pub colour_slot: i16,
    /// The show's presenter's account: who besides producers and admins may change it.
    pub presenter_account: Option<Uuid>,
}

/// What a booking's write did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Booked {
    /// Written.
    Done(BookingRow),
    /// It would share time with this booking: nothing written (409).
    Clash(BookingRow),
    /// No such booking (a change or a delete).
    Missing,
}

/// A booking's row with its show's names, for a `WHERE` on `b` (the booking) and `s` (the show).
macro_rules! booking_where {
    ($where:literal) => {
        concat!(
            "SELECT b.id, b.show_id, b.booked_by,
               (extract(epoch FROM b.starts_at) * 1000)::float8 AS starts_ms,
               (extract(epoch FROM b.ends_at) * 1000)::float8 AS ends_ms,
               s.slug, s.name, s.colour_slot, s.presenter_account
             FROM rehearsal_booking b JOIN show s ON s.id = b.show_id WHERE ",
            $where
        )
    };
}

fn booking_of(row: &sqlx::postgres::PgRow) -> Result<BookingRow, sqlx::Error> {
    let ms = |c: &str| -> Result<i64, sqlx::Error> { Ok(row.try_get::<f64, _>(c)?.round() as i64) };
    Ok(BookingRow {
        booking: Booking {
            id: row.try_get("id")?,
            show_id: row.try_get("show_id")?,
            starts_ms: ms("starts_ms")?,
            ends_ms: ms("ends_ms")?,
            booked_by: row.try_get("booked_by")?,
        },
        show_slug: row.try_get("slug")?,
        show_name: row.try_get("name")?,
        colour_slot: row.try_get("colour_slot")?,
        presenter_account: row.try_get("presenter_account")?,
    })
}

/// Every booking that shares any time with `[from_ms, to_ms)`, in start order.
pub async fn bookings_between<'e, E: sqlx::PgExecutor<'e>>(
    db: E,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<BookingRow>, sqlx::Error> {
    sqlx::query(booking_where!(
        "b.station_id = $1
           AND b.starts_at < to_timestamp($3::float8 / 1000)
           AND b.ends_at > to_timestamp($2::float8 / 1000)
         ORDER BY b.starts_at, b.id"
    ))
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(db)
    .await?
    .iter()
    .map(booking_of)
    .collect()
}

/// One booking by its id.
pub async fn booking<'e, E: sqlx::PgExecutor<'e>>(
    db: E,
    station: StationId,
    id: Uuid,
) -> Result<Option<BookingRow>, sqlx::Error> {
    sqlx::query(booking_where!("b.station_id = $1 AND b.id = $2"))
        .bind(station.as_uuid())
        .bind(id)
        .fetch_optional(db)
        .await?
        .as_ref()
        .map(booking_of)
        .transpose()
}

/// The booking that holds the studio at `now_ms`, if any.
pub async fn holding_now(
    pool: &sqlx::PgPool,
    station: StationId,
    now_ms: i64,
) -> Result<Option<BookingRow>, sqlx::Error> {
    Ok(bookings_between(pool, station, now_ms, now_ms + 1)
        .await?
        .into_iter()
        .next())
}

/// Lock the station's row for the rest of the transaction: every booking write takes it first,
/// so the overlap check and the write are one step for each station. `NO KEY UPDATE` queues
/// booking writes behind each other without holding up the rows that refer to the station
/// (their foreign keys take only `KEY SHARE`).
async fn lock_station(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    station: StationId,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT id FROM station WHERE id = $1 FOR NO KEY UPDATE")
        .bind(station.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;
    Ok(())
}

/// The booking among the others that `[starts_ms, ends_ms)` would share time with.
async fn clash_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    station: StationId,
    starts_ms: i64,
    ends_ms: i64,
    except: Option<Uuid>,
) -> Result<Option<BookingRow>, sqlx::Error> {
    let near = bookings_between(&mut **tx, station, starts_ms, ends_ms).await?;
    let plain: Vec<Booking> = near.iter().map(|b| b.booking.clone()).collect();
    Ok(clash(&plain, starts_ms, ends_ms, except)
        .and_then(|hit| near.iter().find(|b| b.booking.id == hit.id).cloned()))
}

/// Book the studio for `show_id` from `starts_ms` until `ends_ms`, unless that shares time with
/// another booking. The times are checked by the caller ([`check_times`]).
pub async fn create(
    pool: &sqlx::PgPool,
    station: StationId,
    show_id: Uuid,
    starts_ms: i64,
    ends_ms: i64,
    booked_by: &str,
) -> Result<Booked, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_station(&mut tx, station).await?;
    if let Some(hit) = clash_in(&mut tx, station, starts_ms, ends_ms, None).await? {
        return Ok(Booked::Clash(hit));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO rehearsal_booking (id, station_id, show_id, starts_at, ends_at, booked_by)
         VALUES ($1, $2, $3, to_timestamp($4::float8 / 1000), to_timestamp($5::float8 / 1000), $6)",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(show_id)
    .bind(starts_ms as f64)
    .bind(ends_ms as f64)
    .bind(by(booked_by))
    .execute(&mut *tx)
    .await?;
    let made = booking(&mut *tx, station, id).await?;
    tx.commit().await?;
    Ok(made.map_or(Booked::Missing, Booked::Done))
}

/// Change a booking's show and times, unless they share time with another booking.
pub async fn change(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    show_id: Uuid,
    starts_ms: i64,
    ends_ms: i64,
) -> Result<Booked, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_station(&mut tx, station).await?;
    if booking(&mut *tx, station, id).await?.is_none() {
        return Ok(Booked::Missing);
    }
    if let Some(hit) = clash_in(&mut tx, station, starts_ms, ends_ms, Some(id)).await? {
        return Ok(Booked::Clash(hit));
    }
    sqlx::query(
        "UPDATE rehearsal_booking SET show_id = $3,
            starts_at = to_timestamp($4::float8 / 1000), ends_at = to_timestamp($5::float8 / 1000)
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(show_id)
    .bind(starts_ms as f64)
    .bind(ends_ms as f64)
    .execute(&mut *tx)
    .await?;
    let changed = booking(&mut *tx, station, id).await?;
    tx.commit().await?;
    Ok(changed.map_or(Booked::Missing, Booked::Done))
}

/// Delete a booking: `false` when there was none.
pub async fn delete(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM rehearsal_booking WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Why a booking's times are refused, if they are: the end after the start, at least five
/// minutes and at most a day, and not over already. A booking may start a little before `now_ms`
/// (a minute: the time it took to fill in the editor), never further back.
pub fn check_times(starts_ms: i64, ends_ms: i64, now_ms: i64) -> Option<&'static str> {
    if ends_ms <= starts_ms {
        Some("A booking ends after it starts.")
    } else if ends_ms - starts_ms < SHORTEST_MS {
        Some("A booking is at least five minutes long.")
    } else if ends_ms - starts_ms > LONGEST_MS {
        Some("A booking is at most a day long.")
    } else if starts_ms < now_ms - 60_000 {
        Some("A booking starts from now on: the past cannot be booked.")
    } else {
        None
    }
}

fn by(name: &str) -> String {
    let t = name.trim();
    let t = if t.is_empty() { "someone" } else { t };
    t.chars().take(crate::rehearsal::BY_MAX).collect()
}

/// A session over the time the week asks for, with its show's colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    pub session: Session,
    pub colour_slot: i16,
}

/// Every session, open or ended, that shares any time with `[from_ms, to_ms)`, in the order they
/// opened. An open session runs until now.
pub async fn sessions_between(
    pool: &sqlx::PgPool,
    station: StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<SessionRow>, sqlx::Error> {
    let ids: Vec<(Uuid, i16)> = sqlx::query_as(
        "SELECT r.id, s.colour_slot FROM rehearsal_session r JOIN show s ON s.id = r.show_id
         WHERE r.station_id = $1
           AND r.opened_at < to_timestamp($3::float8 / 1000)
           AND coalesce(r.ended_at, now()) > to_timestamp($2::float8 / 1000)
         ORDER BY r.opened_at, r.id",
    )
    .bind(station.as_uuid())
    .bind(from_ms as f64)
    .bind(to_ms as f64)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(ids.len());
    for (id, colour_slot) in ids {
        if let Some(session) = crate::rehearsal::session(pool, station, id).await? {
            out.push(SessionRow {
                session,
                colour_slot,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_booking_is_five_minutes_to_a_day_and_never_in_the_past() {
        let now = 10 * LONGEST_MS;
        let h = 3_600_000;
        assert_eq!(check_times(now, now + h, now), None);
        assert_eq!(
            check_times(now - 30_000, now + h, now),
            None,
            "a minute's slack"
        );
        assert!(check_times(now - 2 * 60_000, now + h, now).is_some());
        assert!(check_times(now + h, now + h, now).is_some());
        assert!(check_times(now + h, now, now).is_some());
        assert!(check_times(now, now + SHORTEST_MS - 1, now).is_some());
        assert_eq!(check_times(now, now + SHORTEST_MS, now), None);
        assert_eq!(check_times(now, now + LONGEST_MS, now), None);
        assert!(check_times(now, now + LONGEST_MS + 1, now).is_some());
    }

    #[test]
    fn booked_by_is_kept_short_and_never_blank() {
        assert_eq!(by(" Finlay "), "Finlay");
        assert_eq!(by(""), "someone");
        assert_eq!(
            by(&"y".repeat(500)).chars().count(),
            crate::rehearsal::BY_MAX
        );
    }
}
