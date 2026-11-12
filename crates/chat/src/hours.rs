// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat's hours (ADR-0234): with `hours.open = "shows"`, chatters may post while a
//! scheduled episode is on air and the station's staff at any time; between shows the room is
//! read-only and the player says when the next show starts.
//!
//! "On air" is the schedule's, by the same rule as the slate's picture and the consent words
//! ([`weavesignal_model::shows::on_air_at`]): an episode from its air time for its length (its
//! own, else its series' or show's slot, else an hour). The desk does not open the chat: a show
//! put on air by hand without a scheduled episode leaves it read-only. The chat reads the
//! episodes straight from the station's database, as its retention sweep does (ADR-0149), so it
//! needs no link to the control plane for this and keeps its hours while that link is down.
//!
//! The windows ahead are kept, not only "on air now", so a show that starts between two readings
//! opens the chat on the second it starts; a reading that fails keeps the last windows read. Until
//! the first reading the chat is read-only (a station that chose `shows` never opens by
//! accident).

use weavesignal_model::StationId;
use weavesignal_model::shows::{Episode, Series, Show, air_window};

use crate::store::Db;

/// How far back an episode may have started and still be on air: the longest a slot may last
/// (24 hours) and then some, as the control plane looks.
const LOOK_BACK_S: i64 = 36 * 3600;
/// How far ahead the next show is looked for: two weeks covers a weekly show and a fortnight off.
const LOOK_AHEAD_S: i64 = 14 * 86_400;

/// The schedule as the chat's hours need it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schedule {
    /// Whether the schedule has been read at least once.
    pub read: bool,
    /// Each scheduled episode's air window, `[start, end)` in epoch seconds, in start order.
    pub windows: Vec<(i64, i64)>,
    /// The station's time zone (IANA), for saying when the chat opens; `UTC` until set.
    pub zone: String,
}

impl Schedule {
    /// From the episodes [`weavesignal_model::shows::episodes_between`] gives.
    pub fn of(episodes: &[(Show, Series, Episode)], zone: String) -> Self {
        let mut windows: Vec<(i64, i64)> = episodes
            .iter()
            .filter_map(|(show, series, ep)| air_window(show, series, ep))
            .collect();
        windows.sort_unstable();
        Schedule {
            read: true,
            windows,
            zone,
        }
    }

    /// A scheduled episode is on air at `now_s`.
    pub fn on_air(&self, now_s: i64) -> bool {
        self.windows
            .iter()
            .any(|&(start, end)| start <= now_s && now_s < end)
    }

    /// When the next scheduled episode starts after `now_s`, if one is in the schedule.
    pub fn next_start(&self, now_s: i64) -> Option<i64> {
        self.windows
            .iter()
            .map(|&(start, _)| start)
            .filter(|&start| start > now_s)
            .min()
    }
}

/// Read the schedule's windows around `now_s` and the station's zone.
pub async fn read(db: &Db, station: StationId, now_s: i64) -> sqlx::Result<Schedule> {
    let episodes = weavesignal_model::shows::episodes_between(
        db,
        station,
        now_s - LOOK_BACK_S,
        now_s + LOOK_AHEAD_S,
    )
    .await?;
    let zone: Option<String> =
        sqlx::query_scalar("SELECT time_zone FROM playout_settings WHERE station_id = $1")
            .bind(station.as_uuid())
            .fetch_optional(db)
            .await?;
    Ok(Schedule::of(
        &episodes,
        zone.unwrap_or_else(|| "UTC".into()),
    ))
}

/// Whether posting is open now, for the rules and the page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Hours {
    /// `shows` is in force (false: always open, as 1.0.0).
    pub shows_only: bool,
    /// Chatters may post now (staff always may).
    pub open: bool,
    /// While closed: when the next scheduled show starts, epoch ms; `None` when none is
    /// scheduled in the next two weeks.
    pub opens_at_ms: Option<i64>,
    /// The station's time zone, to say that time in.
    pub zone: String,
}

impl Hours {
    pub fn always() -> Self {
        Hours {
            shows_only: false,
            open: true,
            opens_at_ms: None,
            zone: String::new(),
        }
    }

    /// The hours at `now_s` under `shows`, from the schedule last read.
    pub fn shows(schedule: &Schedule, now_s: i64) -> Self {
        let open = schedule.read && schedule.on_air(now_s);
        Hours {
            shows_only: true,
            open,
            opens_at_ms: if open {
                None
            } else {
                schedule.next_start(now_s).map(|s| s * 1000)
            },
            zone: schedule.zone.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(windows: &[(i64, i64)]) -> Schedule {
        Schedule {
            read: true,
            windows: windows.to_vec(),
            zone: "Europe/London".into(),
        }
    }

    #[test]
    fn open_from_a_shows_start_until_its_end_and_closed_between() {
        let s = sched(&[(1_000, 4_600), (8_200, 11_800)]);
        let at = |t| Hours::shows(&s, t);
        assert!(!at(999).open);
        assert_eq!(at(999).opens_at_ms, Some(1_000_000));
        assert!(at(1_000).open && at(4_599).open);
        assert_eq!(at(1_000).opens_at_ms, None);
        assert!(!at(4_600).open, "the end is not on air");
        assert_eq!(at(4_600).opens_at_ms, Some(8_200_000));
        assert!(!at(12_000).open);
        assert_eq!(at(12_000).opens_at_ms, None, "nothing more is scheduled");
        assert_eq!(at(12_000).zone, "Europe/London");
    }

    #[test]
    fn overlapping_shows_keep_it_open_and_an_unread_schedule_keeps_it_closed() {
        let s = sched(&[(0, 100), (50, 300)]);
        assert!(Hours::shows(&s, 200).open);
        let unread = Schedule::default();
        assert!(!Hours::shows(&unread, 0).open);
        assert!(Hours::always().open && !Hours::always().shows_only);
    }
}
