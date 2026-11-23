// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio's rows (1.3.0, #23; migration 0070, provisional): a session, the move to
//! the live studio and its opening state, and bookings. The rules here are pure where they can
//! be: when people moved from rehearsal stop standing by, what the opening state holds, and
//! whether two bookings overlap.
//!
//! The design is `docs/reference/design/rehearsal-mode-build-plan.md` in the project folder
//! (sections 1.5, 1.6 and 1.11).

use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::StationId;

/// How a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    /// End rehearsal: everyone was taken out of the room.
    Ended,
    /// Move to the live studio: the people went on to the live room, standing by.
    Moved,
    /// Nobody was in the room for `rehearsal.idle_end_s`.
    Idle,
}

impl Ended {
    pub fn word(self) -> &'static str {
        match self {
            Ended::Ended => "ended",
            Ended::Moved => "moved",
            Ended::Idle => "idle",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            "ended" => Some(Ended::Ended),
            "moved" => Some(Ended::Moved),
            "idle" => Some(Ended::Idle),
            _ => None,
        }
    }
}

/// The opening state a move carries (section 1.6): who is on and who is muted, applied when the
/// show's host is put on air at the live desk, never on arrival. `cue`, `talk` and `talk_to` are
/// momentary and are not carried; there are no levels to carry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Opening {
    /// The airing the move is for: its start, in ms since the Unix epoch.
    pub airing_start_ms: i64,
    /// Until when the moved people arrive standing by: the airing's start plus the rules'
    /// grace ([`standby_until_ms`]).
    pub standby_until_ms: i64,
    /// The show's host among the people moved, whose going on air applies the opening state;
    /// `None` when no host was moved (it is then applied at the first handover for the show).
    #[serde(default)]
    pub host: Option<String>,
    /// Who goes on air when it is applied (sorted).
    #[serde(default)]
    pub on: Vec<String>,
    /// Who is muted when it is applied, before anyone is put on (sorted).
    #[serde(default)]
    pub mute: Vec<String>,
    /// When it was applied; kept until `standby_until_ms` has passed so that a live desk that
    /// starts again in between can be put right.
    #[serde(default)]
    pub applied_at_ms: Option<i64>,
    /// The airing's end, in ms since the Unix epoch: a page that missed the move is offered it
    /// again until then, and an opening never applied lapses then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airing_end_ms: Option<i64>,
    /// When it was cleared without being applied (its airing ended, or another move for the same
    /// show replaced it). A cleared opening is never applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared_at_ms: Option<i64>,
}

/// One person as the rehearsal desk last reported them, for the opening state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches<'a> {
    pub identity: &'a str,
    pub present: bool,
    pub on: bool,
    pub mute: bool,
}

impl Opening {
    /// The opening state from the rehearsal desk's people: `on` and `mute` for each person
    /// present; absent people are left out (nobody is put on air who is not there).
    pub fn from_people<'a>(
        airing_start_ms: i64,
        standby_until_ms: i64,
        host: Option<&str>,
        people: impl IntoIterator<Item = Switches<'a>>,
    ) -> Self {
        let mut on = Vec::new();
        let mut mute = Vec::new();
        for p in people.into_iter().filter(|p| p.present) {
            if p.on {
                on.push(p.identity.to_owned());
            }
            if p.mute {
                mute.push(p.identity.to_owned());
            }
        }
        on.sort();
        on.dedup();
        mute.sort();
        mute.dedup();
        Opening {
            airing_start_ms,
            standby_until_ms,
            host: host.map(str::to_owned),
            on,
            mute,
            applied_at_ms: None,
            airing_end_ms: None,
            cleared_at_ms: None,
        }
    }

    /// Whether it still waits to be applied at `now_ms`: neither applied nor cleared, and its
    /// airing not over.
    pub fn waiting(&self, now_ms: i64) -> bool {
        self.applied_at_ms.is_none()
            && self.cleared_at_ms.is_none()
            && self.airing_end_ms.is_none_or(|end| now_ms < end)
    }

    /// Whether a page that missed the move is still offered it at `now_ms`: not replaced by
    /// another move, and its airing not over (until the standing-by time when the end is not
    /// known).
    pub fn offered(&self, now_ms: i64) -> bool {
        self.cleared_at_ms.is_none() && now_ms < self.airing_end_ms.unwrap_or(self.standby_until_ms)
    }
}

/// One person at the live desk, as the opening's rule reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtLiveDesk<'a> {
    pub identity: &'a str,
    /// In the room with their link up (the desk's `present`).
    pub present: bool,
    pub on_air: bool,
}

/// What, besides the desk's own status, says the moment has come for an opening (section 1.6,
/// *The handover*). The host going on air by any route is read from the desk's status, so it
/// needs no trigger of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// Nothing but the desk's status (the control plane's look every second).
    Look,
    /// The air was taken back from automation while the opening's show is on air by the
    /// schedule.
    TakeAir,
    /// The schedule brought someone on for the opening's show (the post's bring-on, or a press
    /// of Go on air now).
    BringOn,
}

/// Whether an opening is due now (section 1.6): it is still waiting, and either its host is
/// present and on air at the live desk (by whichever route: the schedule's bring-on, the first
/// press of a handover, taking the air from automation, their own On key), or, with no host
/// among the people moved, one of the people moved has been put on air. Taking the air from
/// automation, and the schedule bringing the show on, are the moment as well, host or not.
pub fn opening_due(
    o: &Opening,
    moved: &[String],
    people: &[AtLiveDesk<'_>],
    moment: Moment,
    now_ms: i64,
) -> bool {
    if !o.waiting(now_ms) {
        return false;
    }
    if matches!(moment, Moment::TakeAir | Moment::BringOn) {
        return true;
    }
    let on = |id: &str| {
        people
            .iter()
            .any(|p| p.identity == id && p.present && p.on_air)
    };
    match &o.host {
        Some(host) => on(host),
        None => moved.iter().any(|id| on(id)),
    }
}

/// What applying an opening does at the live desk: mute these first, then put these on air (with
/// the rules' fade). Only people present are touched, and nobody already on air is put on again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpeningActs {
    pub mute: Vec<String>,
    pub on: Vec<String>,
}

/// The acts an opening comes to among the people at the live desk now.
pub fn opening_acts(o: &Opening, people: &[AtLiveDesk<'_>]) -> OpeningActs {
    let here = |id: &str| people.iter().find(|p| p.identity == id && p.present);
    OpeningActs {
        mute: o
            .mute
            .iter()
            .filter(|id| here(id).is_some())
            .cloned()
            .collect(),
        on: o
            .on
            .iter()
            .filter(|id| here(id).is_some_and(|p| !p.on_air))
            .cloned()
            .collect(),
    }
}

/// Until when people moved from rehearsal stand by (section 1.6): the airing's start plus the
/// rules' `grace_s`, the last moment the post can still hand over. A grace below zero counts as
/// none.
pub fn standby_until_ms(airing_start_ms: i64, grace_s: i32) -> i64 {
    airing_start_ms.saturating_add(i64::from(grace_s.max(0)) * 1000)
}

/// A rehearsal session as the control plane holds it, with its show's names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub show_id: Uuid,
    pub show_slug: String,
    pub show_name: String,
    /// The show's presenter's account: who besides producers and admins may run its rehearsal.
    pub presenter_account: Option<Uuid>,
    pub opened_by: String,
    pub opened_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub ended: Option<Ended>,
    pub moved_at_ms: Option<i64>,
    pub moved: Vec<String>,
    pub opening: Option<Opening>,
}

/// The show a session is opened for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowRef {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub presenter_account: Option<Uuid>,
}

/// What Open did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// A new session.
    New(Session),
    /// This show's session was open already: it is the one.
    Already(Session),
    /// Another show's session is open: the studio is busy (409).
    Busy(Session),
}

/// The longest `opened_by` the table keeps.
pub const BY_MAX: usize = 120;

fn by(name: &str) -> String {
    let t = name.trim();
    let t = if t.is_empty() { "someone" } else { t };
    t.chars().take(BY_MAX).collect()
}

/// A session's row with its show's names, for a `WHERE` on `r` (the session) and `s` (the show).
macro_rules! session_where {
    ($where:literal) => {
        concat!(
            "SELECT r.id, r.show_id, s.slug, s.name, s.presenter_account, r.opened_by,
               (extract(epoch FROM r.opened_at) * 1000)::float8 AS opened_ms,
               (extract(epoch FROM r.ended_at) * 1000)::float8 AS ended_ms, r.ended_how,
               (extract(epoch FROM r.moved_at) * 1000)::float8 AS moved_ms, r.moved, r.opening
             FROM rehearsal_session r JOIN show s ON s.id = r.show_id WHERE ",
            $where
        )
    };
}

fn session_of(row: &sqlx::postgres::PgRow) -> Result<Session, sqlx::Error> {
    let ms = |c: &str| -> Result<Option<i64>, sqlx::Error> {
        Ok(row.try_get::<Option<f64>, _>(c)?.map(|v| v.round() as i64))
    };
    let moved: Option<serde_json::Value> = row.try_get("moved")?;
    let opening: Option<serde_json::Value> = row.try_get("opening")?;
    let decode = |e: serde_json::Error| sqlx::Error::Decode(Box::new(e));
    Ok(Session {
        id: row.try_get("id")?,
        show_id: row.try_get("show_id")?,
        show_slug: row.try_get("slug")?,
        show_name: row.try_get("name")?,
        presenter_account: row.try_get("presenter_account")?,
        opened_by: row.try_get("opened_by")?,
        opened_at_ms: ms("opened_ms")?.unwrap_or(0),
        ended_at_ms: ms("ended_ms")?,
        ended: row
            .try_get::<Option<String>, _>("ended_how")?
            .as_deref()
            .and_then(Ended::parse),
        moved_at_ms: ms("moved_ms")?,
        moved: moved
            .map(serde_json::from_value)
            .transpose()
            .map_err(decode)?
            .unwrap_or_default(),
        opening: opening
            .map(serde_json::from_value)
            .transpose()
            .map_err(decode)?,
    })
}

/// A session by its id, open or ended.
pub async fn session(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query(session_where!("r.station_id = $1 AND r.id = $2"))
        .bind(station.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?
        .as_ref()
        .map(session_of)
        .transpose()
}

/// The station's open session, if any (read back after a restart).
pub async fn open_session(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query(session_where!("r.station_id = $1 AND r.ended_at IS NULL"))
        .bind(station.as_uuid())
        .fetch_optional(pool)
        .await?
        .as_ref()
        .map(session_of)
        .transpose()
}

/// Open the rehearsal studio for `show`: a new session, unless one is open already (this show's:
/// [`Opened::Already`]; another's: [`Opened::Busy`]). Two Opens at once cannot both win: the
/// table's partial unique index refuses the second, which is then answered as here.
pub async fn open(
    pool: &sqlx::PgPool,
    station: StationId,
    show: &ShowRef,
    opened_by: &str,
) -> Result<Opened, sqlx::Error> {
    // Twice at most: a session that ends between the insert's refusal and the read is gone, and
    // the next insert takes the studio.
    for _ in 0..2 {
        if let Some(open) = open_session(pool, station).await? {
            return Ok(if open.show_id == show.id {
                Opened::Already(open)
            } else {
                Opened::Busy(open)
            });
        }
        let id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO rehearsal_session (id, station_id, show_id, opened_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(station.as_uuid())
        .bind(show.id)
        .bind(by(opened_by))
        .execute(pool)
        .await;
        match inserted {
            Ok(_) => {
                return session(pool, station, id)
                    .await?
                    .map(Opened::New)
                    .ok_or(sqlx::Error::RowNotFound);
            }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e),
        }
    }
    open_session(pool, station)
        .await?
        .map(|s| {
            if s.show_id == show.id {
                Opened::Already(s)
            } else {
                Opened::Busy(s)
            }
        })
        .ok_or(sqlx::Error::RowNotFound)
}

/// End a session (once): `false` when it had ended already or is not this station's.
pub async fn end(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    how: Ended,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE rehearsal_session SET ended_at = greatest(now(), opened_at), ended_how = $3
         WHERE station_id = $1 AND id = $2 AND ended_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(how.word())
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Write a move on an open session (section 1.6, step 1): who moved and the opening state,
/// before anything else is done, so a restart in the middle cannot lose it. A later move for
/// the same session replaces it. `false` when the session is not open.
pub async fn record_move(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    moved: &[String],
    opening: &Opening,
) -> Result<bool, sqlx::Error> {
    let moved = serde_json::to_value(moved).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    let opening = serde_json::to_value(opening).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    let done = sqlx::query(
        "UPDATE rehearsal_session SET moved_at = now(), moved = $3, opening = $4
         WHERE station_id = $1 AND id = $2 AND ended_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(moved)
    .bind(opening)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// The sessions ended by a move whose opening is not cleared, moved within the last
/// `within_ms`, newest first: read back after a restart, to apply an opening still waiting and
/// to offer the move again to a page that missed it.
pub async fn moved_sessions(
    pool: &sqlx::PgPool,
    station: StationId,
    within_ms: i64,
) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query(session_where!(
        "r.station_id = $1 AND r.ended_how = 'moved' AND r.opening IS NOT NULL
           AND (r.opening->>'cleared_at_ms') IS NULL
           AND r.moved_at > now() - make_interval(secs => $2::float8 / 1000)
         ORDER BY r.moved_at DESC"
    ))
    .bind(station.as_uuid())
    .bind(within_ms as f64)
    .fetch_all(pool)
    .await?
    .iter()
    .map(session_of)
    .collect()
}

/// Write a moved session's opening as it now stands (applied, or cleared). `false` when the
/// session has no move.
pub async fn set_opening(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    opening: &Opening,
) -> Result<bool, sqlx::Error> {
    let opening = serde_json::to_value(opening).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    let done = sqlx::query(
        "UPDATE rehearsal_session SET opening = $3
         WHERE station_id = $1 AND id = $2 AND moved_at IS NOT NULL",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(opening)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Clear the openings of a show's earlier moves that are neither applied nor cleared: a new move
/// for the same show replaces them (section 1.6). Returns how many.
pub async fn clear_other_openings(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
    except: Uuid,
    at_ms: i64,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE rehearsal_session
            SET opening = jsonb_set(opening, '{cleared_at_ms}', to_jsonb($4::bigint))
          WHERE station_id = $1 AND show_id = $2 AND id <> $3 AND opening IS NOT NULL
            AND (opening->>'cleared_at_ms') IS NULL",
    )
    .bind(station.as_uuid())
    .bind(show)
    .bind(except)
    .bind(at_ms)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// A booking: the rehearsal studio held for one show from `starts_ms` until `ends_ms`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Booking {
    pub id: Uuid,
    pub show_id: Uuid,
    pub starts_ms: i64,
    pub ends_ms: i64,
    pub booked_by: String,
}

/// Whether two stretches `[start, end)` share any time: one that ends as the other starts does
/// not.
pub fn overlaps(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// The first booking that a new or moved one, `[starts_ms, ends_ms)`, would overlap; `except`
/// is the booking being moved.
pub fn clash(
    bookings: &[Booking],
    starts_ms: i64,
    ends_ms: i64,
    except: Option<Uuid>,
) -> Option<&Booking> {
    bookings
        .iter()
        .filter(|b| Some(b.id) != except)
        .find(|b| overlaps((b.starts_ms, b.ends_ms), (starts_ms, ends_ms)))
}

/// The booking holding the studio at `now_ms`, if any.
pub fn holding(bookings: &[Booking], now_ms: i64) -> Option<&Booking> {
    bookings
        .iter()
        .find(|b| b.starts_ms <= now_ms && now_ms < b.ends_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_moved_stand_by_until_the_post_and_its_grace() {
        assert_eq!(standby_until_ms(1_000_000, 120), 1_120_000);
        assert_eq!(standby_until_ms(1_000_000, 0), 1_000_000);
        assert_eq!(standby_until_ms(1_000_000, -5), 1_000_000);
        assert_eq!(standby_until_ms(i64::MAX - 10, 600), i64::MAX);
    }

    #[test]
    fn the_opening_holds_who_is_on_and_muted_among_the_people_present() {
        let p = |identity, present, on, mute| Switches {
            identity,
            present,
            on,
            mute,
        };
        let o = Opening::from_people(
            5_000,
            125_000,
            Some("eilidh"),
            [
                p("kirsty", true, true, true),
                p("eilidh", true, true, false),
                p("gregor", true, false, false),
                // Gone from the room: never put on, even though their switch was on.
                p("finlay", false, true, false),
            ],
        );
        assert_eq!(o.on, ["eilidh", "kirsty"]);
        assert_eq!(o.mute, ["kirsty"]);
        assert_eq!(o.host.as_deref(), Some("eilidh"));
        assert_eq!((o.airing_start_ms, o.standby_until_ms), (5_000, 125_000));
        assert_eq!(o.applied_at_ms, None);
        // As the row keeps it, and back.
        let v = serde_json::to_value(&o).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"airing_start_ms": 5_000, "standby_until_ms": 125_000,
                "host": "eilidh", "on": ["eilidh", "kirsty"], "mute": ["kirsty"],
                "applied_at_ms": null})
        );
        assert_eq!(serde_json::from_value::<Opening>(v).unwrap(), o);
    }

    fn here(identity: &str, present: bool, on_air: bool) -> AtLiveDesk<'_> {
        AtLiveDesk {
            identity,
            present,
            on_air,
        }
    }

    fn opening(host: Option<&str>) -> Opening {
        Opening {
            airing_start_ms: 1_000_000,
            standby_until_ms: 1_120_000,
            host: host.map(str::to_owned),
            on: vec!["eilidh".into(), "finlay".into(), "kirsty".into()],
            mute: vec!["kirsty".into()],
            applied_at_ms: None,
            airing_end_ms: Some(4_600_000),
            cleared_at_ms: None,
        }
    }

    #[test]
    fn the_opening_is_due_when_its_host_is_on_air_by_any_route() {
        let o = opening(Some("eilidh"));
        let moved = [
            "eilidh".to_owned(),
            "kirsty".to_owned(),
            "finlay".to_owned(),
        ];
        let now = 1_000_500;
        // Standing by: not yet.
        let standing = [here("eilidh", true, false), here("kirsty", true, false)];
        assert!(!opening_due(&o, &moved, &standing, Moment::Look, now));
        // A guest put on by a press is not the moment while the host is moved.
        let guest_on = [here("eilidh", true, false), here("kirsty", true, true)];
        assert!(!opening_due(&o, &moved, &guest_on, Moment::Look, now));
        // The host on air, whoever pressed: the moment.
        let host_on = [here("eilidh", true, true), here("kirsty", true, false)];
        assert!(opening_due(&o, &moved, &host_on, Moment::Look, now));
        // On air but their link down: not yet.
        let host_away = [here("eilidh", false, true)];
        assert!(!opening_due(&o, &moved, &host_away, Moment::Look, now));
        // Taking the air from automation, and the schedule's bring-on: the moment, host or not.
        assert!(opening_due(&o, &moved, &standing, Moment::TakeAir, now));
        assert!(opening_due(&o, &moved, &standing, Moment::BringOn, now));
        // Applied once, never again; cleared or over, never.
        let mut applied = o.clone();
        applied.applied_at_ms = Some(now);
        assert!(!opening_due(
            &applied,
            &moved,
            &host_on,
            Moment::TakeAir,
            now
        ));
        let mut cleared = o.clone();
        cleared.cleared_at_ms = Some(now);
        assert!(!opening_due(
            &cleared,
            &moved,
            &host_on,
            Moment::BringOn,
            now
        ));
        assert!(!opening_due(&o, &moved, &host_on, Moment::Look, 4_600_000));
    }

    #[test]
    fn with_no_host_moved_the_first_moved_person_put_on_air_is_the_moment() {
        let o = opening(None);
        let moved = ["kirsty".to_owned(), "finlay".to_owned()];
        let now = 1_000_500;
        let standing = [here("kirsty", true, false), here("finlay", true, false)];
        assert!(!opening_due(&o, &moved, &standing, Moment::Look, now));
        // Someone who was not moved going on air is not the moment.
        let other = [here("kirsty", true, false), here("hamish", true, true)];
        assert!(!opening_due(&o, &moved, &other, Moment::Look, now));
        let pressed = [here("kirsty", true, false), here("finlay", true, true)];
        assert!(opening_due(&o, &moved, &pressed, Moment::Look, now));
        assert!(opening_due(&o, &moved, &standing, Moment::BringOn, now));
    }

    #[test]
    fn applying_mutes_first_then_puts_on_only_the_people_present_and_off_air() {
        let o = opening(Some("eilidh"));
        let people = [
            here("eilidh", true, true),
            here("kirsty", true, false),
            // Finlay's link is down: skipped, never put on air.
            here("finlay", false, false),
        ];
        let acts = opening_acts(&o, &people);
        assert_eq!(acts.mute, ["kirsty"]);
        assert_eq!(
            acts.on,
            ["kirsty"],
            "the host is on already; Finlay is not here"
        );
        // Nobody at the desk: nothing.
        assert_eq!(opening_acts(&o, &[]), OpeningActs::default());
    }

    #[test]
    fn a_missed_move_is_offered_until_the_airing_ends_and_never_once_replaced() {
        let mut o = opening(Some("eilidh"));
        assert!(o.offered(4_599_999));
        assert!(!o.offered(4_600_000));
        o.airing_end_ms = None;
        assert!(o.offered(1_119_999) && !o.offered(1_120_000));
        o.cleared_at_ms = Some(1);
        assert!(!o.offered(0));
        // The new fields are left out of the row while unset, as the first moves wrote it.
        let v = serde_json::to_value(opening(None)).unwrap();
        assert!(v.get("cleared_at_ms").is_none());
        assert_eq!(v["airing_end_ms"], 4_600_000);
    }

    fn booking(n: u128, starts_ms: i64, ends_ms: i64) -> Booking {
        Booking {
            id: Uuid::from_u128(n),
            show_id: Uuid::from_u128(100 + n),
            starts_ms,
            ends_ms,
            booked_by: "Finlay".into(),
        }
    }

    #[test]
    fn bookings_that_share_time_clash_and_ones_that_touch_do_not() {
        let list = [booking(1, 1_000, 2_000), booking(2, 3_000, 4_000)];
        assert_eq!(
            clash(&list, 1_500, 2_500, None).map(|b| b.id.as_u128()),
            Some(1)
        );
        assert_eq!(
            clash(&list, 2_000, 3_000, None),
            None,
            "touching is not a clash"
        );
        assert_eq!(
            clash(&list, 500, 5_000, None).map(|b| b.id.as_u128()),
            Some(1)
        );
        assert_eq!(
            clash(&list, 3_500, 3_600, None).map(|b| b.id.as_u128()),
            Some(2)
        );
        // A booking moved within its own time does not clash with itself.
        assert_eq!(clash(&list, 1_200, 1_800, Some(Uuid::from_u128(1))), None);
        assert!(overlaps((0, 10), (9, 20)));
        assert!(!overlaps((0, 10), (10, 20)));
    }

    #[test]
    fn a_booking_holds_the_studio_from_its_start_until_its_end() {
        let list = [booking(1, 1_000, 2_000)];
        assert_eq!(holding(&list, 999), None);
        assert_eq!(holding(&list, 1_000).map(|b| b.id.as_u128()), Some(1));
        assert_eq!(holding(&list, 1_999).map(|b| b.id.as_u128()), Some(1));
        assert_eq!(holding(&list, 2_000), None);
    }

    #[test]
    fn opened_by_is_kept_short_and_never_blank() {
        assert_eq!(by("  Kirsty "), "Kirsty");
        assert_eq!(by(" "), "someone");
        assert_eq!(by(&"x".repeat(300)).chars().count(), BY_MAX);
    }

    #[test]
    fn the_migration_makes_one_open_session_per_station_and_bookings() {
        let m = crate::MIGRATOR
            .iter()
            .find(|m| m.description.contains("rehearsal"))
            .expect("the rehearsal migration");
        let sql = m.sql.as_str();
        assert!(sql.contains("CREATE TABLE rehearsal_session"));
        assert!(sql.contains("CREATE UNIQUE INDEX rehearsal_session_one_open"));
        assert!(sql.contains("WHERE ended_at IS NULL"));
        assert!(sql.contains("CREATE TABLE rehearsal_booking"));
        assert!(sql.contains("CHECK (ends_at > starts_at)"));
        assert!(
            !sql.contains("CREATE EXTENSION"),
            "no extension the addon may lack"
        );
        for word in [Ended::Ended, Ended::Moved, Ended::Idle] {
            assert!(sql.contains(&format!("'{}'", word.word())));
            assert_eq!(Ended::parse(word.word()), Some(word));
        }
    }
}
