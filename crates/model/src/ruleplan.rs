// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Making a show with its first episode, and changing a rule later (release 1.0.4, ADR-0278;
//! design `new-show.md` sections 1, 3 and 9).
//!
//! A rule only makes dates; episodes are what air. Changing a rule never touches an episode
//! that has aired, and never moves one a person placed by hand. An episode drafted for a date to
//! come and still on its rule's date moves to the new rule's dates, in order, keeping its
//! number, title and guests, unless the request says to leave them.

use std::collections::{HashMap, HashSet};

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::StationId;
use crate::planner::{Clash, ClashRefusal, Clashes, clashes_for};
use crate::rule::Rule;
use crate::shows::{
    self, Episode, Maker, NewEpisodes, Series, SeriesInput, Show, ShowError, ShowInput, StepBy,
    episode_columns, episode_of, fill_next, lock_schedule, put_show_in, rule_write, series_columns,
    series_of, series_rule, show_columns, show_of,
};

/// What to do with the episodes drafted for dates to come when a rule changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Drafted {
    /// They move to the new rule's dates, in order.
    #[default]
    Move,
    /// They stay where they are.
    Leave,
}

impl Drafted {
    pub fn as_str(self) -> &'static str {
        match self {
            Drafted::Move => "move",
            Drafted::Leave => "leave",
        }
    }
}

/// Which rule a change is to: the show's, or one series'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Show,
    Series(u32),
}

/// An episode that stays where it is, and the date it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EpisodeAt {
    pub series: u32,
    pub number: u32,
    /// The rule date it fills, if it fills one.
    pub on: Option<Date>,
    pub start_ms: Option<i64>,
}

/// An episode that moves to another date.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Moved {
    pub series: u32,
    pub number: u32,
    pub from_on: Date,
    pub to_on: Date,
    pub from_ms: i64,
    pub to_ms: i64,
    /// Where the new time overlaps another show's.
    pub clashes: Vec<Clash>,
}

/// What a changed rule does to the episodes, before and after it is saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChangePlan {
    /// Episodes that have aired, or are on air: frozen, counted and not touched.
    pub aired: u32,
    pub moved: Vec<Moved>,
    /// Drafted by hand or moved by hand (or already past): they stay.
    pub by_hand: Vec<EpisodeAt>,
    /// Drafted on a date the new rule does not give: they stay.
    pub no_date: Vec<EpisodeAt>,
    /// `leave`: the drafted episodes left on their old dates.
    pub left: Vec<EpisodeAt>,
}

/// One series with its episodes, before and after the change.
pub struct SeriesState {
    pub old: Series,
    pub new: Series,
    pub episodes: Vec<Episode>,
}

struct Mover {
    id: Uuid,
    series: u32,
    number: u32,
    to_on: Date,
    to_start: i64,
    minutes: Option<u16>,
}

fn start_of(rule: &Rule, zone: &TimeZone, on: Date) -> Option<Timestamp> {
    rule.occurrences(zone, on)
        .next()
        .filter(|o| o.on == on)
        .map(|o| o.start)
}

/// What the change does to one series' episodes (design section 3). Pure: it looks at nothing
/// but what it is given.
#[allow(clippy::too_many_arguments)] // the shows before and after, one series, the clock and what it fills in
fn plan_series(
    show_old: &Show,
    show_new: &Show,
    st: &SeriesState,
    mode: Drafted,
    zone: &TimeZone,
    now: Timestamp,
    plan: &mut ChangePlan,
    movers: &mut Vec<Mover>,
) {
    let old_rule = series_rule(show_old, Some(&st.old), &st.episodes);
    let at = |e: &Episode| EpisodeAt {
        series: st.old.number,
        number: e.number,
        on: e.occurrence,
        start_ms: e.air_at.map(|t| t * 1000),
    };
    let mut movable: Vec<&Episode> = Vec::new();
    for e in &st.episodes {
        if e.aired_at.is_some() {
            plan.aired += 1;
            continue;
        }
        // Still on its rule's date: it airs exactly when the old rule says that date airs, and
        // it has not started.
        let on_rule = match (e.occurrence, e.air_at, &old_rule) {
            (Some(d), Some(t), Some(rule)) => {
                t > now.as_second() && start_of(rule, zone, d).is_some_and(|s| s.as_second() == t)
            }
            _ => false,
        };
        if on_rule {
            movable.push(e);
        } else {
            plan.by_hand.push(at(e));
        }
    }
    if mode == Drafted::Leave {
        plan.left.extend(movable.iter().map(|e| at(e)));
        return;
    }
    if movable.is_empty() {
        return;
    }
    movable.sort_by_key(|e| (e.occurrence, e.number));
    let ids: HashSet<Uuid> = movable.iter().map(|e| e.id).collect();
    let rest: Vec<Episode> = st
        .episodes
        .iter()
        .filter(|e| !ids.contains(&e.id))
        .cloned()
        .collect();
    let filled: HashSet<Date> = rest.iter().filter_map(|e| e.occurrence).collect();
    let new_rule = series_rule(show_new, Some(&st.new), &rest);
    let from = now
        .checked_sub(48.hours())
        .unwrap_or(now)
        .to_zoned(zone.clone())
        .date();
    let mut targets: Box<dyn Iterator<Item = crate::rule::Occurrence> + '_> = match &new_rule {
        Some(rule) => Box::new(
            rule.occurrences(zone, from)
                .filter(|o| !filled.contains(&o.on))
                .filter(|o| o.start > now),
        ),
        None => Box::new(std::iter::empty()),
    };
    let length = new_rule.as_ref().map_or(60, Rule::length);
    for e in movable {
        match targets.next() {
            Some(o) => {
                let (from_on, from_ms) =
                    (e.occurrence.unwrap_or(o.on), e.air_at.unwrap_or(0) * 1000);
                if from_on == o.on && from_ms == o.start.as_millisecond() {
                    continue;
                }
                plan.moved.push(Moved {
                    series: st.old.number,
                    number: e.number,
                    from_on,
                    to_on: o.on,
                    from_ms,
                    to_ms: o.start.as_millisecond(),
                    clashes: Vec::new(),
                });
                movers.push(Mover {
                    id: e.id,
                    series: st.old.number,
                    number: e.number,
                    to_on: o.on,
                    to_start: o.start.as_second(),
                    minutes: (o.minutes != length).then_some(o.minutes),
                });
            }
            None => plan.no_date.push(at(e)),
        }
    }
}

/// The series a change reaches, with the show and the series as they would be after it.
fn after_change(
    show: &Show,
    series: Vec<(Series, Vec<Episode>)>,
    scope: Scope,
    rule: &Option<Rule>,
) -> Result<(Show, Vec<SeriesState>), ShowError> {
    let mut show_new = show.clone();
    let mut states = Vec::new();
    match scope {
        Scope::Show => {
            show_new.rule = rule.clone();
            show_new.slot = rule.as_ref().and_then(Rule::as_slot);
            for (s, episodes) in series {
                // A series with a rule of its own does not follow the show.
                if s.rule.is_none() {
                    states.push(SeriesState {
                        new: s.clone(),
                        old: s,
                        episodes,
                    });
                }
            }
        }
        Scope::Series(n) => {
            let Some((s, episodes)) = series.into_iter().find(|(s, _)| s.number == n) else {
                return Err(ShowError::Missing(format!("series {n} of `{}`", show.slug)));
            };
            let mut new = s.clone();
            new.rule = rule.clone();
            new.slot = rule.as_ref().and_then(Rule::as_slot);
            states.push(SeriesState {
                old: s,
                new,
                episodes,
            });
        }
    }
    Ok((show_new, states))
}

fn plan_all(
    show: &Show,
    show_new: &Show,
    states: &[SeriesState],
    mode: Drafted,
    zone: &TimeZone,
    now: Timestamp,
) -> (ChangePlan, Vec<Mover>) {
    let mut plan = ChangePlan::default();
    let mut movers = Vec::new();
    for st in states {
        plan_series(show, show_new, st, mode, zone, now, &mut plan, &mut movers);
    }
    (plan, movers)
}

#[allow(clippy::too_many_arguments)] // the station, the show, the plan and the clock
async fn attach_clashes(
    pool: &sqlx::PgPool,
    station: StationId,
    zone: &TimeZone,
    show: Uuid,
    plan: &mut ChangePlan,
    movers: &[Mover],
    rule_length: u16,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    let windows: Vec<(i64, i64)> = movers
        .iter()
        .map(|m| {
            (
                m.to_start,
                m.to_start + i64::from(m.minutes.unwrap_or(rule_length)) * 60,
            )
        })
        .collect();
    let found = clashes_for(pool, station, zone, Some(show), &windows, now).await?;
    for (moved, clashes) in plan.moved.iter_mut().zip(found) {
        moved.clashes = clashes;
    }
    Ok(())
}

fn refusal(plan: &ChangePlan, who: Clashes) -> Option<ShowError> {
    if who != Clashes::Refuse {
        return None;
    }
    let mut all: Vec<Clash> = plan.moved.iter().flat_map(|m| m.clashes.clone()).collect();
    if all.is_empty() {
        return None;
    }
    for c in &mut all {
        c.refused = true;
    }
    Some(ShowError::Clash(Box::new(ClashRefusal {
        message: all[0].words(),
        clashes: all,
    })))
}

async fn load_series(
    conn: &mut sqlx::PgConnection,
    show: Uuid,
) -> Result<Vec<(Series, Vec<Episode>)>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE show_id = $1 ORDER BY number FOR UPDATE"
    ))
    .bind(show)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for row in &rows {
        let s = series_of(row)?;
        let eps = sqlx::query(concat!(
            "SELECT ",
            episode_columns!(),
            " FROM episode WHERE series_id = $1 ORDER BY number FOR UPDATE"
        ))
        .bind(s.id)
        .fetch_all(&mut *conn)
        .await?;
        let eps = eps.iter().map(episode_of).collect::<Result<_, _>>()?;
        out.push((s, eps));
    }
    Ok(out)
}

async fn load_show(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    slug: &str,
) -> Result<Show, ShowError> {
    let row = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE station_id = $1 AND slug = $2 FOR UPDATE"
    ))
    .bind(station.as_uuid())
    .bind(slug)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some(r) => Ok(show_of(&r)?),
        None => Err(ShowError::Missing(format!("show `{slug}`"))),
    }
}

/// What saving `rule` would do to the episodes, without saving anything: the preview's
/// *Before you save*. `slug` names the show, and `scope` the show's rule or one series'.
#[allow(clippy::too_many_arguments)] // the station, the show, the change and the clock
pub async fn preview_change(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    scope: Scope,
    rule: &Option<Rule>,
    mode: Drafted,
    zone: &TimeZone,
    now: Timestamp,
) -> Result<ChangePlan, ShowError> {
    if let Some(r) = rule {
        r.check()?;
    }
    let show = shows::get_show(pool, station, slug)
        .await?
        .ok_or_else(|| ShowError::Missing(format!("show `{slug}`")))?;
    let mut series = Vec::new();
    for s in shows::list_series(pool, show.id).await? {
        let eps = shows::list_episodes(pool, s.id).await?;
        series.push((s, eps));
    }
    let (show_new, states) = after_change(&show, series, scope, rule)?;
    let (mut plan, movers) = plan_all(&show, &show_new, &states, mode, zone, now);
    let length = rule.as_ref().map_or(60, Rule::length);
    attach_clashes(
        pool, station, zone, show.id, &mut plan, &movers, length, now,
    )
    .await?;
    Ok(plan)
}

/// What a saved change did.
#[derive(Clone, Debug)]
pub struct RuleChanged {
    pub show: Show,
    pub series: Option<Series>,
    pub plan: ChangePlan,
    /// The episodes that moved, as (series number, episode number), for the cards to be drawn
    /// again.
    pub moved: Vec<(u32, u32)>,
    pub warnings: Vec<Clash>,
}

/// Change a show's rule, or one series', and move the drafted episodes in the same
/// transaction (or leave them). Logged with who made it. `clashes` says what to do with a moved
/// episode that now overlaps another show: refuse the change (a presenter's), or carry on and
/// return it as a warning.
#[allow(clippy::too_many_arguments)] // the station, the show, the change, the clock and who
pub async fn change_rule(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    scope: Scope,
    rule: Option<Rule>,
    mode: Drafted,
    zone: &TimeZone,
    now: Timestamp,
    by: &StepBy,
    clashes: Clashes,
) -> Result<RuleChanged, ShowError> {
    if let Some(r) = &rule {
        r.check()?;
    }
    let mut tx = pool.begin().await?;
    lock_schedule(&mut tx, station).await?;
    let show = load_show(&mut tx, station, slug).await?;
    let series = load_series(&mut tx, show.id).await?;
    let (show_new, states) = after_change(&show, series, scope, &rule)?;
    let (mut plan, movers) = plan_all(&show, &show_new, &states, mode, zone, now);
    let length = rule.as_ref().map_or(60, Rule::length);
    if clashes != Clashes::Ignore {
        attach_clashes(
            pool, station, zone, show.id, &mut plan, &movers, length, now,
        )
        .await?;
        if let Some(e) = refusal(&plan, clashes) {
            return Err(e);
        }
    }
    let warnings: Vec<Clash> = plan.moved.iter().flat_map(|m| m.clashes.clone()).collect();
    let json = serde_json::to_string(&rule)
        .map_err(|e| ShowError::Invalid(format!("the rule cannot be stored: {e}")))?;
    let (weekday, start, minutes) = {
        let slot = rule.as_ref().and_then(Rule::as_slot);
        (
            slot.map(|s| i16::from(s.weekday)),
            slot.map(|s| s.start as i16),
            slot.and_then(|s| s.minutes).map(|m| m as i16),
        )
    };
    let json = if rule.is_some() { Some(json) } else { None };
    let series_changed = match scope {
        Scope::Show => {
            sqlx::query(
                "UPDATE show SET rule = $2::text::jsonb, slot_weekday = $3, slot_start = $4,
                                 slot_minutes = $5, updated_at = now()
                 WHERE id = $1",
            )
            .bind(show.id)
            .bind(&json)
            .bind(weekday)
            .bind(start)
            .bind(minutes)
            .execute(&mut *tx)
            .await?;
            None
        }
        Scope::Series(_) => {
            let st = &states[0];
            sqlx::query(
                "UPDATE series SET rule = $2::text::jsonb, slot_weekday = $3, slot_start = $4,
                                   slot_minutes = $5, updated_at = now()
                 WHERE id = $1",
            )
            .bind(st.old.id)
            .bind(&json)
            .bind(weekday)
            .bind(start)
            .bind(minutes)
            .execute(&mut *tx)
            .await?;
            Some(st.old.number)
        }
    };
    // Several episodes may swap dates: the check waits for the end of the transaction.
    sqlx::query("SET CONSTRAINTS episode_occurrence_key DEFERRED")
        .execute(&mut *tx)
        .await?;
    for m in &movers {
        sqlx::query(
            "UPDATE episode SET air_at = to_timestamp($2::double precision),
                                occurrence = $3::text::date,
                                minutes = COALESCE($4, minutes), updated_at = now()
             WHERE id = $1 AND aired_at IS NULL",
        )
        .bind(m.id)
        .bind(m.to_start as f64)
        .bind(m.to_on.to_string())
        .bind(m.minutes.map(|x| x as i16))
        .execute(&mut *tx)
        .await?;
    }
    let from_rule = match scope {
        Scope::Show => show.rule.clone(),
        Scope::Series(_) => states[0].old.rule.clone(),
    };
    sqlx::query(
        "INSERT INTO show_rule_change (station_id, show_id, series_id, from_rule, to_rule,
                                       drafted, moved, by_label, by_account)
         VALUES ($1, $2, $3, $4::text::jsonb, $5::text::jsonb, $6, $7, $8, $9)",
    )
    .bind(station.as_uuid())
    .bind(show.id)
    .bind(series_changed.and(states.first().map(|s| s.old.id)))
    .bind(
        from_rule
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default()),
    )
    .bind(&json)
    .bind(mode.as_str())
    .bind(movers.len() as i32)
    .bind(by.label.chars().take(160).collect::<String>())
    .bind(by.account)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let show_after = shows::get_show(pool, station, slug)
        .await?
        .ok_or_else(|| ShowError::Missing(format!("show `{slug}`")))?;
    let series_after = match series_changed {
        Some(n) => shows::get_series(pool, show.id, n).await?,
        None => None,
    };
    Ok(RuleChanged {
        show: show_after,
        series: series_after,
        moved: movers.iter().map(|m| (m.series, m.number)).collect(),
        plan,
        warnings,
    })
}

/// A show made with its first episode.
#[derive(Clone, Debug)]
pub struct ShowMade {
    pub show: Show,
    pub drafted: Option<(Series, Episode)>,
    pub warnings: Vec<Clash>,
}

/// Make or change a show and, in the same transaction, make series 1 if it has none and draft
/// its first episode (`draft_first`): the date under way if one is, else the next. Nothing is
/// saved if anything is refused, a presenter's clash among it.
#[allow(clippy::too_many_arguments)] // the station, the show, the clock, who and the clash policy
pub async fn put_show_drafting(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    input: &ShowInput,
    zone: &TimeZone,
    now: Timestamp,
    maker: Maker<'_>,
    clashes: Clashes,
) -> Result<ShowMade, ShowError> {
    if !shows::is_slug(slug) {
        return Err(ShowError::Invalid(format!(
            "`{slug}` is not a show path: lower-case letters and digits, joined by hyphens"
        )));
    }
    input.check()?;
    let hosts = shows::people_ids(pool, station, &input.hosts).await?;
    let mut tx = pool.begin().await?;
    lock_schedule(&mut tx, station).await?;
    let show = put_show_in(&mut tx, station, slug, input, &hosts).await?;
    if !input.draft_first {
        tx.commit().await?;
        return Ok(ShowMade {
            show,
            drafted: None,
            warnings: Vec::new(),
        });
    }
    let existing = load_series(&mut tx, show.id).await?;
    // The first episode is drafted once: a save that sends `draft_first` again (a retry, the
    // setup run again) finds the show has episodes and drafts nothing more.
    if existing.iter().any(|(_, eps)| !eps.is_empty()) {
        tx.commit().await?;
        return Ok(ShowMade {
            show,
            drafted: None,
            warnings: Vec::new(),
        });
    }
    let (series, made) = match existing.into_iter().last() {
        Some((s, _)) => (s, false),
        None => {
            let row = sqlx::query(concat!(
                "INSERT INTO series (id, station_id, show_id, number)
                 VALUES ($1, $2, $3, 1) RETURNING ",
                series_columns!()
            ))
            .bind(Uuid::new_v4())
            .bind(station.as_uuid())
            .bind(show.id)
            .fetch_one(&mut *tx)
            .await?;
            (series_of(&row)?, true)
        }
    };
    let result = shows::create_episodes_in(
        &mut tx,
        pool,
        station,
        &show,
        &series,
        zone,
        &NewEpisodes {
            count: 1,
            title: String::new(),
            guests: Vec::new(),
        },
        &[],
        now,
        maker,
        clashes,
    )
    .await;
    let (numbers, warnings) = match result {
        Ok(r) => r,
        // A show with no rule, or no date left, is made all the same: nothing to draft.
        Err(ShowError::Nothing(_)) => (Vec::new(), Vec::new()),
        Err(e) => return Err(e),
    };
    if numbers.is_empty() && made {
        // Nothing drafted: no series was needed.
        sqlx::query("DELETE FROM series WHERE id = $1")
            .bind(series.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let drafted = match numbers.first() {
        Some(n) => shows::get_episode(pool, series.id, *n)
            .await?
            .map(|e| (series, e)),
        None => None,
    };
    Ok(ShowMade {
        show,
        drafted,
        warnings,
    })
}

/// Give the episodes made before rules the date they fill: for a series with a weekly slot, the
/// slot's date in the Monday-to-Sunday week of the episode's air time (the reading the MOVED
/// chip always made). Done once, at the first start after migration 0036, because the date needs
/// the station's zone, which the control plane settles at start and the database does not know.
/// Returns how many episodes were given a date.
pub async fn backfill_occurrences(
    pool: &sqlx::PgPool,
    zone: &TimeZone,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let first = sqlx::query(
        "INSERT INTO schema_backfill (name) VALUES ('episode_occurrence') ON CONFLICT DO NOTHING",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if first == 0 {
        return Ok(0);
    }
    let n = backfill_in(&mut tx, zone).await?;
    tx.commit().await?;
    Ok(n)
}

/// [`backfill_occurrences`] without the once-only gate.
pub async fn backfill_in(
    conn: &mut sqlx::PgConnection,
    zone: &TimeZone,
) -> Result<u64, sqlx::Error> {
    let rows: Vec<(Uuid, Uuid, i64, Option<String>)> = sqlx::query_as(
        "SELECT e.id, e.series_id, extract(epoch FROM e.air_at)::bigint,
                COALESCE(se.rule, sh.rule)::text
           FROM episode e
           JOIN series se ON se.id = e.series_id
           JOIN show sh ON sh.id = se.show_id
          WHERE e.air_at IS NOT NULL AND e.occurrence IS NULL
          ORDER BY e.series_id, e.number",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut taken: HashSet<(Uuid, Date)> = HashSet::new();
    let mut given: HashMap<Uuid, Date> = HashMap::new();
    for (id, series, air, rule) in rows {
        let Some(slot) = rule
            .and_then(|r| serde_json::from_str::<Rule>(&r).ok())
            .and_then(|r| r.as_slot())
        else {
            continue;
        };
        let Some(date) = Timestamp::from_second(air)
            .ok()
            .map(|t| t.to_zoned(zone.clone()).date())
        else {
            continue;
        };
        let monday = date
            .checked_sub(i64::from(date.weekday().to_monday_zero_offset()).days())
            .ok();
        let Some(on) =
            monday.and_then(|m| m.checked_add((i64::from(slot.weekday) - 1).days()).ok())
        else {
            continue;
        };
        // Two episodes in one week: the first keeps the date; the other was made by hand.
        if taken.insert((series, on)) {
            given.insert(id, on);
        }
    }
    let mut n = 0;
    for (id, on) in given {
        n += sqlx::query("UPDATE episode SET occurrence = $2::text::date WHERE id = $1")
            .bind(id)
            .bind(on.to_string())
            .execute(&mut *conn)
            .await?
            .rows_affected();
    }
    Ok(n)
}

/// What a save does to a series' rule fields, for a series made by [`SeriesInput`]: the rule the
/// series would hold after it, given what is stored.
pub fn series_rule_after(input: &SeriesInput, stored: Option<&Rule>) -> Option<Rule> {
    rule_write(&input.rule, input.slot, stored)
}

/// The drafts the next episode would take, for the preview's *first* mark: the same answer as
/// drafting.
pub fn first_draft(
    rule: Option<&Rule>,
    zone: &TimeZone,
    episodes: &[Episode],
    now: Timestamp,
) -> Option<shows::Draft> {
    fill_next(rule, zone, episodes, 1, now)
        .into_iter()
        .next()
        .filter(|d| d.air_at.is_some())
}

/// Log a change of rule made some other way (a show's or series' save): `leave` records that the
/// drafted episodes were not touched.
pub async fn log_rule_change(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
    series: Option<Uuid>,
    from: Option<&Rule>,
    to: Option<&Rule>,
    by: &StepBy,
) -> Result<(), sqlx::Error> {
    let json = |r: Option<&Rule>| r.and_then(|r| serde_json::to_string(r).ok());
    sqlx::query(
        "INSERT INTO show_rule_change (station_id, show_id, series_id, from_rule, to_rule,
                                       drafted, moved, by_label, by_account)
         VALUES ($1, $2, $3, $4::text::jsonb, $5::text::jsonb, 'leave', 0, $6, $7)",
    )
    .bind(station.as_uuid())
    .bind(show)
    .bind(series)
    .bind(json(from))
    .bind(json(to))
    .bind(by.label.chars().take(160).collect::<String>())
    .bind(by.account)
    .execute(pool)
    .await?;
    Ok(())
}
