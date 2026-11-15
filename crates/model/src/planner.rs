// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What a station's rules give beyond the episodes drafted: the dates still to draft, the next
//! date of a show, and the clashes between shows (release 1.0.4, ADR-0278).
//!
//! One answer serves the Schedule tab's dotted blocks (`GET /api/v1/schedule/episodes`), the
//! preview of a rule and the check that refuses a presenter an episode over another show's time.
//! A show's dates are those of its **current series**, the one with the highest number (the show
//! on its own when it has no series yet); older series are over.

use std::collections::HashMap;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::StationId;
use crate::rule::{End, Occurrence, Repeat, Rule, hhmm};
use crate::shows::{
    self, Episode, Series, Show, drafted_to, episodes_between, list_series, series_rule,
};

/// A show with its current series, that series' episodes, and the rule that governs it
/// ([`series_rule`]: the series' own or the show's, adjusted for the series' start, plan and
/// episodes made).
#[derive(Clone, Debug)]
pub struct Scheduled {
    pub show: Show,
    pub series: Option<Series>,
    pub episodes: Vec<Episode>,
    pub rule: Option<Rule>,
}

impl Scheduled {
    /// The show on its own, as a show that has not been given a series yet.
    pub fn bare(show: &Show) -> Self {
        Scheduled {
            rule: series_rule(show, None, &[]),
            show: show.clone(),
            series: None,
            episodes: Vec::new(),
        }
    }

    /// Every date the rule gives from `from` (a local date) on, in order, each marked: skipped,
    /// filled by an episode (its number), or still to draft with the number the episode would
    /// take when drafting reaches it. A date drafting has passed (before the latest date any
    /// episode fills, or already begun with nothing drafted) is a gap, and has no number.
    pub fn walk<'a>(
        &'a self,
        zone: &'a TimeZone,
        from: Date,
        now: Timestamp,
    ) -> Box<dyn Iterator<Item = Walked> + Send + 'a> {
        let Some(rule) = &self.rule else {
            return Box::new(std::iter::empty());
        };
        let filled: HashMap<Date, u32> = self
            .episodes
            .iter()
            .filter_map(|e| e.occurrence.map(|d| (d, e.number)))
            .collect();
        let latest = drafted_to(zone, &self.episodes);
        let under_way = now
            .checked_sub(i64::from(rule.length()).minutes())
            .unwrap_or(now);
        // Drafting goes on from the day after the latest date, or the date under way if that is
        // later: it is never into the past (as `fill_next`).
        let today = under_way.to_zoned(zone.clone()).date();
        let cursor = match latest {
            Some(d) => d.tomorrow().unwrap_or(d).max(today),
            None => today,
        };
        let mut number = self.episodes.iter().map(|e| e.number).max().unwrap_or(0);
        Box::new(
            rule.occurrences_with_skips(zone, from.min(cursor))
                .map(move |(occ, skipped)| {
                    let filled = filled.get(&occ.on).copied();
                    let reaches =
                        !skipped && filled.is_none() && occ.on >= cursor && occ.start > under_way;
                    let n = reaches.then(|| {
                        number += 1;
                        number
                    });
                    Walked {
                        occ,
                        skipped,
                        filled,
                        number: n,
                    }
                }),
        )
    }

    /// The dates the rule gives that no episode fills and the rule does not skip, ending after
    /// `from` and starting before `to`, in order, each with the number the episode would take.
    pub fn open_dates(
        &self,
        zone: &TimeZone,
        from: Timestamp,
        to: Timestamp,
        now: Timestamp,
    ) -> Vec<OpenDate> {
        let window_from = from
            .to_zoned(zone.clone())
            .date()
            .yesterday()
            .unwrap_or_default();
        self.walk(zone, window_from, now)
            .take_while(|w| w.occ.start < to)
            .filter(|w| !w.skipped && w.filled.is_none() && w.occ.end() > from)
            .map(|w| OpenDate {
                occ: w.occ,
                number: w.number,
            })
            .collect()
    }

    /// The effective rule in words: the short form, or the times alone with no rule.
    pub fn short(&self, zone: &TimeZone, today: Date) -> String {
        self.rule
            .as_ref()
            .map(|r| r.words(zone, today).short)
            .unwrap_or_default()
    }
}

/// A date of a rule, as [`Scheduled::walk`] gives it.
#[derive(Clone, Debug)]
pub struct Walked {
    pub occ: Occurrence,
    pub skipped: bool,
    /// The episode that fills it.
    pub filled: Option<u32>,
    pub number: Option<u32>,
}

/// A date still to draft.
#[derive(Clone, Debug)]
pub struct OpenDate {
    pub occ: Occurrence,
    pub number: Option<u32>,
}

/// Every show with its current series and that series' episodes: three queries.
pub async fn scheduled(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Scheduled>, sqlx::Error> {
    let shows = shows::list_shows(pool, station).await?;
    let mut current: HashMap<Uuid, Series> = HashMap::new();
    for s in shows::all_series(pool, station).await? {
        // In series order, so the newest is the one kept.
        current.insert(s.show_id, s);
    }
    let ids: Vec<Uuid> = current.values().map(|s| s.id).collect();
    let mut episodes: HashMap<Uuid, Vec<Episode>> = HashMap::new();
    for e in shows::episodes_of(pool, &ids).await? {
        episodes.entry(e.series_id).or_default().push(e);
    }
    Ok(shows
        .into_iter()
        .map(|show| {
            let series = current.remove(&show.id);
            let eps = series
                .as_ref()
                .and_then(|s| episodes.remove(&s.id))
                .unwrap_or_default();
            let rule = series_rule(&show, series.as_ref(), &eps);
            Scheduled {
                show,
                series,
                episodes: eps,
                rule,
            }
        })
        .collect())
}

/// One show with its current series, as [`scheduled`] gives every show.
pub async fn scheduled_show(pool: &sqlx::PgPool, show: &Show) -> Result<Scheduled, sqlx::Error> {
    let series = list_series(pool, show.id).await?.pop();
    let episodes = match &series {
        Some(s) => shows::list_episodes(pool, s.id).await?,
        None => Vec::new(),
    };
    Ok(Scheduled {
        rule: series_rule(show, series.as_ref(), &episodes),
        show: show.clone(),
        series,
        episodes,
    })
}

/// The newest series of a show.
pub async fn current_series(
    pool: &sqlx::PgPool,
    show: Uuid,
) -> Result<Option<Series>, sqlx::Error> {
    Ok(list_series(pool, show).await?.pop())
}

/// Why a show has no next date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoNext {
    /// The show has no rule.
    None,
    /// A one-off, or chosen dates, all in the past.
    OncePassed,
    /// The rule's `until` has passed.
    Ended,
    /// The count of episodes is made.
    Complete,
}

/// A show's next date, or why it has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NextOn {
    pub on: Option<Date>,
    pub start_ms: Option<i64>,
    pub reason: Option<NoNext>,
}

/// The first date of a rule that starts after `now`.
pub fn next_on(rule: Option<&Rule>, zone: &TimeZone, now: Timestamp) -> NextOn {
    let Some(rule) = rule else {
        return NextOn {
            on: None,
            start_ms: None,
            reason: Some(NoNext::None),
        };
    };
    let from = (now - 48.hours()).to_zoned(zone.clone()).date();
    match rule.occurrences(zone, from).find(|o| o.start > now) {
        Some(o) => NextOn {
            on: Some(o.on),
            start_ms: Some(o.start.as_millisecond()),
            reason: None,
        },
        None => NextOn {
            on: None,
            start_ms: None,
            reason: Some(match (&rule.repeat, &rule.end) {
                (Repeat::Once | Repeat::Dates { .. }, _) => NoNext::OncePassed,
                (_, End::Count { .. }) => NoNext::Complete,
                _ => NoNext::Ended,
            }),
        },
    }
}

/// An overlap between a date a show would take and another show's time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Clash {
    /// The other show's name and path.
    pub show: String,
    pub slug: String,
    /// `episode`: it has an episode drafted there; `rule`: its rule gives that date.
    pub kind: ClashKind,
    pub series: Option<u32>,
    pub episode: Option<u32>,
    /// The other show's date.
    pub on: Date,
    pub start_ms: i64,
    pub end_ms: i64,
    pub overlap_minutes: u32,
    /// The other show's rule in short (*Sat, 20:00–23:00*), else its times.
    pub short: String,
    pub hosts: Vec<String>,
    pub presenter: String,
    /// This session may not go on (a presenter drafting over it).
    pub refused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClashKind {
    Episode,
    Rule,
}

impl Clash {
    /// *Saturday Night Kettle has that time. Sat, 20:00–23:00, with Shona Campbell.*
    pub fn words(&self) -> String {
        let mut s = format!("{} has that time. {}", self.show, self.short);
        if !self.hosts.is_empty() {
            let (last, rest) = self.hosts.split_last().expect("not empty");
            let who = if rest.is_empty() {
                last.clone()
            } else {
                format!("{} and {last}", rest.join(", "))
            };
            s.push_str(&format!(", with {who}"));
        }
        s.push('.');
        s
    }
}

/// A draft refused for the time it would take.
#[derive(Clone, Debug, PartialEq)]
pub struct ClashRefusal {
    /// *Saturday Night Kettle has that time. Sat, 20:00–23:00, with Shona Campbell.*
    pub message: String,
    pub clashes: Vec<Clash>,
}

/// How a clash is treated by whoever is drafting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clashes {
    /// Not looked for.
    Ignore,
    /// Looked for and returned as warnings.
    Warn,
    /// Looked for, and any refuses the draft (a presenter's).
    Refuse,
}

/// For each of `windows` (epoch seconds `[start, end)`), the other shows' times it overlaps by a
/// minute or more: their drafted episodes and the dates their rules give that no episode fills.
/// `own` is left out (a show cannot clash with itself); `None` leaves nothing out. Automation
/// never clashes: it yields.
pub async fn clashes_for(
    pool: &sqlx::PgPool,
    station: StationId,
    zone: &TimeZone,
    own: Option<Uuid>,
    windows: &[(i64, i64)],
    now: Timestamp,
) -> Result<Vec<Vec<Clash>>, sqlx::Error> {
    let mut out = vec![Vec::new(); windows.len()];
    let (Some(lo), Some(hi)) = (
        windows.iter().map(|w| w.0).min(),
        windows.iter().map(|w| w.1).max(),
    ) else {
        return Ok(out);
    };
    let people = shows::people(pool, station).await?;
    let today = now.to_zoned(zone.clone()).date();
    let names = |show: &Show, series: Option<&Series>, ep: Option<&Episode>| -> Vec<String> {
        let ids = ep
            .and_then(|e| e.hosts.clone())
            .or_else(|| series.and_then(|s| s.hosts.clone()))
            .unwrap_or_else(|| show.hosts.clone());
        ids.iter().filter_map(|i| people.get(i).cloned()).collect()
    };
    let overlap = |a: (i64, i64), b: (i64, i64)| (a.1.min(b.1) - a.0.max(b.0)).max(0);
    let schedule = scheduled(pool, station).await?;
    let short_of: HashMap<Uuid, String> = schedule
        .iter()
        .map(|s| (s.show.id, s.short(zone, today)))
        .collect();
    let times = |start: i64, end: i64| {
        let z = |t: i64| {
            Timestamp::from_second(t)
                .unwrap_or_default()
                .to_zoned(zone.clone())
        };
        let (a, b) = (z(start), z(end));
        let m = |z: &jiff::Zoned| hhmm((z.hour() as u16) * 60 + z.minute() as u16);
        format!("{}–{}", m(&a), m(&b))
    };
    let short = |show: &Show, start: i64, end: i64| match short_of.get(&show.id) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => times(start, end),
    };
    // Other shows' drafted episodes. One that started up to a day before `lo` can still be on.
    let found = episodes_between(pool, station, lo - 86_400, hi).await?;
    for (show, series, ep) in &found {
        if Some(show.id) == own {
            continue;
        }
        let Some((start, end)) = shows::air_window(show, series, ep) else {
            continue;
        };
        for (i, w) in windows.iter().enumerate() {
            let secs = overlap((start, end), *w);
            if secs >= 60 {
                out[i].push(Clash {
                    show: show.name.clone(),
                    slug: show.slug.clone(),
                    kind: ClashKind::Episode,
                    series: Some(series.number),
                    episode: Some(ep.number),
                    on: Timestamp::from_second(start)
                        .unwrap_or_default()
                        .to_zoned(zone.clone())
                        .date(),
                    start_ms: start * 1000,
                    end_ms: end * 1000,
                    overlap_minutes: (secs / 60) as u32,
                    short: short(show, start, end),
                    hosts: names(show, Some(series), Some(ep)),
                    presenter: show.presenter.clone(),
                    refused: false,
                });
            }
        }
    }
    // The dates other shows' rules give, which no episode fills yet.
    let from = Timestamp::from_second(lo - 86_400).unwrap_or(now);
    let to = Timestamp::from_second(hi).unwrap_or(now);
    for s in &schedule {
        if Some(s.show.id) == own {
            continue;
        }
        for open in s.open_dates(zone, from, to, now) {
            let o = &open.occ;
            let (start, end) = (o.start.as_second(), o.end().as_second());
            for (i, w) in windows.iter().enumerate() {
                let secs = overlap((start, end), *w);
                if secs >= 60 {
                    out[i].push(Clash {
                        show: s.show.name.clone(),
                        slug: s.show.slug.clone(),
                        kind: ClashKind::Rule,
                        series: s.series.as_ref().map(|x| x.number),
                        episode: None,
                        on: o.on,
                        start_ms: start * 1000,
                        end_ms: end * 1000,
                        overlap_minutes: (secs / 60) as u32,
                        short: short(&s.show, start, end),
                        hosts: names(&s.show, s.series.as_ref(), None),
                        presenter: s.show.presenter.clone(),
                        refused: false,
                    });
                }
            }
        }
    }
    for list in &mut out {
        list.sort_by_key(|c| (c.start_ms, c.slug.clone()));
    }
    Ok(out)
}
