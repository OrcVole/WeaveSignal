// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! When a show airs: a small, typed subset of iCalendar (release 1.0.4; ADR-0278; design
//! `new-show.md` section 9).
//!
//! A [`Rule`] replaces the weekly [`Slot`] on a show and on a series. It is one start time and
//! one length a day, a [`Repeat`] (once, weekly on some weekdays every N weeks, monthly on the
//! nth weekday or on a day, yearly, or chosen dates), an [`End`] (never, until a date, after N
//! episodes), dates skipped and dates moved. All dates and times are the station's wall clock
//! in its one zone (ADR-0189); a rule is expanded to instants only here, with
//! [`Rule::occurrences`], which resolves a start the way [`Slot::next_after`] always has
//! (`Disambiguation::Compatible`: a start in the hour that does not exist airs an hour late, a
//! start in the hour that happens twice airs the first time), so a stored weekly slot airs at
//! exactly the instants it did before. `Count` counts episodes, not dates: a skipped date does
//! not use one up (the one place the subset knowingly differs from RFC 5545).

use std::collections::{HashMap, HashSet, VecDeque};

use jiff::civil::{Date, DateTime, Weekday};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::shows::{ShowError, Slot};

/// The longest a series can plan (`planned_episodes` has the same limit).
pub const MAX_COUNT: u32 = 520;
/// At most this many chosen dates, skipped dates or moved dates.
pub const MAX_LIST: usize = 120;
/// The studio's length for a rule that gives none.
pub const DEFAULT_MINUTES: u16 = 60;
/// A rule that gives no date for this many periods in a row is taken to give no more.
const EMPTY_PERIODS: u32 = 1000;

/// When a show airs. All dates and times are the station's wall clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The first date the rule may give (DTSTART's date). `None` only for a weekly rule of every
    /// week, which is what an old [`Slot`] was.
    #[serde(default)]
    pub starts_on: Option<Date>,
    /// Minutes after midnight.
    pub start: u16,
    /// The length in minutes; `None` is the studio's 60.
    #[serde(default)]
    pub minutes: Option<u16>,
    pub repeat: Repeat,
    #[serde(default)]
    pub end: End,
    /// Dates the rule gives that do not air (EXDATE).
    #[serde(default)]
    pub skip: Vec<Date>,
    /// A date aired at another time (an override with RECURRENCE-ID).
    #[serde(default)]
    pub moves: Vec<Move>,
}

/// How a rule repeats. Weekdays are 1 (Monday) to 7 (Sunday).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Repeat {
    /// The start date alone.
    Once,
    /// Every `every` weeks on these weekdays (weeks start on Monday).
    Weekly { every: u8, weekdays: Vec<u8> },
    /// Every `every` months on the `nth` weekday: 1 to 4, or -1 for the last.
    MonthlyNth { every: u8, nth: i8, weekday: u8 },
    /// Every `every` months on a day of the month: 1 to 31, or -1 for the last day. A month
    /// without the day is passed over.
    MonthlyDay { every: u8, day: i8 },
    /// Every year on a date; 29 February gives leap years only.
    Yearly { month: u8, day: u8 },
    /// Chosen dates.
    Dates { dates: Vec<Date> },
}

/// When a rule stops.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum End {
    #[default]
    Never,
    /// Up to and including this date (the station's local date of the start).
    Until { until: Date },
    /// After this many episodes: a skipped date does not count.
    Count { count: u32 },
}

/// A date that airs at another time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Move {
    /// The rule's date that is moved.
    pub from: Date,
    /// When it airs instead, on the station's wall clock (`2027-04-13T20:00:00`).
    #[schema(value_type = String, example = "2027-04-13T20:00:00")]
    pub to: DateTime,
    /// A length of its own for this date.
    #[serde(default)]
    pub minutes: Option<u16>,
}

/// A refused field: its path in the rule and a sentence for a person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RuleProblem {
    pub field: String,
    pub message: String,
}

/// What the clocks changing does to one start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClockChange {
    /// The start fell in the hour that does not exist, and airs an hour late.
    Forward,
    /// The start fell in the hour that happens twice, and airs the first time.
    Back,
}

/// One date a rule gives, as an instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// The rule date: what an episode's `occurrence` holds, even when the date is moved.
    pub on: Date,
    pub start: Timestamp,
    pub minutes: u16,
    /// The date's own nominal start, when it was moved.
    pub moved_from: Option<DateTime>,
    pub clock: Option<ClockChange>,
}

impl Occurrence {
    /// When it ends: its start plus its length in elapsed minutes, as `air_window` reckons.
    pub fn end(&self) -> Timestamp {
        self.start
            .checked_add(i64::from(self.minutes).minutes())
            .unwrap_or(self.start)
    }
}

/// A rule said back in words, by the control plane, in British English (design section 2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RuleWords {
    /// *Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes.*
    pub sentence: String,
    /// *First Tue monthly, 20:00–21:00*, for lists and tooltips.
    pub short: String,
    /// *The last is Tuesday 5 October 2027.* when the rule ends.
    pub last: Option<String>,
    pub last_on: Option<Date>,
    /// Dates the calendar cannot give, said rather than hidden.
    pub notes: Vec<String>,
}

impl From<Slot> for Rule {
    fn from(s: Slot) -> Self {
        Rule {
            starts_on: None,
            start: s.start,
            minutes: s.minutes,
            repeat: Repeat::Weekly {
                every: 1,
                weekdays: vec![s.weekday],
            },
            end: End::Never,
            skip: Vec::new(),
            moves: Vec::new(),
        }
    }
}

fn problem(field: &str, message: impl Into<String>) -> RuleProblem {
    RuleProblem {
        field: field.to_owned(),
        message: message.into(),
    }
}

fn has_repeats<T: Eq + std::hash::Hash>(items: impl IntoIterator<Item = T>) -> bool {
    let mut seen = HashSet::new();
    items.into_iter().any(|i| !seen.insert(i))
}

impl Rule {
    /// Whether the rule is exactly an old weekly slot: weekly, every week, one weekday, no start
    /// date, no end, nothing skipped or moved.
    pub fn as_slot(&self) -> Option<Slot> {
        match (&self.repeat, &self.end) {
            (Repeat::Weekly { every: 1, weekdays }, End::Never)
                if self.starts_on.is_none()
                    && weekdays.len() == 1
                    && self.skip.is_empty()
                    && self.moves.is_empty() =>
            {
                Some(Slot {
                    weekday: weekdays[0],
                    start: self.start,
                    minutes: self.minutes,
                })
            }
            _ => None,
        }
    }

    /// The length in minutes.
    pub fn length(&self) -> u16 {
        self.minutes.unwrap_or(DEFAULT_MINUTES)
    }

    /// Every field that cannot stand, with its path. Empty when the rule is good.
    pub fn problems(&self) -> Vec<RuleProblem> {
        let mut out = Vec::new();
        if self.start >= 1440 {
            out.push(problem("start", "a rule starts before 24:00"));
        }
        if self.minutes.is_some_and(|m| m == 0 || m > 1440) {
            out.push(problem("minutes", "a rule lasts 1 to 1,440 minutes"));
        }
        match &self.repeat {
            Repeat::Once => {}
            Repeat::Weekly { every, weekdays } => {
                if !(1..=12).contains(every) {
                    out.push(problem("repeat.every", "repeat every 1 to 12 weeks"));
                }
                if weekdays.is_empty()
                    || has_repeats(weekdays.iter())
                    || weekdays.iter().any(|w| !(1..=7).contains(w))
                {
                    out.push(problem(
                        "repeat.weekdays",
                        "pick one or more different weekdays, 1 (Monday) to 7 (Sunday)",
                    ));
                }
            }
            Repeat::MonthlyNth {
                every,
                nth,
                weekday,
            } => {
                if !(1..=12).contains(every) {
                    out.push(problem("repeat.every", "repeat every 1 to 12 months"));
                }
                if !matches!(nth, 1..=4 | -1) {
                    out.push(problem(
                        "repeat.nth",
                        "the weekday is the first to fourth of the month, or the last",
                    ));
                }
                if !(1..=7).contains(weekday) {
                    out.push(problem(
                        "repeat.weekday",
                        "a weekday is 1 (Monday) to 7 (Sunday)",
                    ));
                }
            }
            Repeat::MonthlyDay { every, day } => {
                if !(1..=12).contains(every) {
                    out.push(problem("repeat.every", "repeat every 1 to 12 months"));
                }
                if !(matches!(day, 1..=31 | -1)) {
                    out.push(problem(
                        "repeat.day",
                        "the day is 1 to 31, or -1 for the last day",
                    ));
                }
            }
            Repeat::Yearly { month, day } => {
                if !(1..=12).contains(month) {
                    out.push(problem("repeat.month", "a month is 1 to 12"));
                } else if !(1..=31).contains(day) {
                    out.push(problem("repeat.day", "a day is 1 to 31"));
                } else if Date::new(2024, *month as i8, *day as i8).is_err() {
                    out.push(problem("repeat.day", "that date never happens"));
                }
            }
            Repeat::Dates { dates } => {
                if dates.len() > MAX_LIST {
                    out.push(problem("repeat.dates", "at most 120 chosen dates"));
                }
                if has_repeats(dates.iter()) {
                    out.push(problem("repeat.dates", "each chosen date is different"));
                }
            }
        }
        if let End::Count { count } = self.end
            && !(1..=MAX_COUNT).contains(&count)
        {
            out.push(problem("end.count", "a series plans 1 to 520 episodes"));
        }
        if self.skip.len() > MAX_LIST {
            out.push(problem("skip", "at most 120 skipped dates"));
        }
        if has_repeats(self.skip.iter()) {
            out.push(problem("skip", "each skipped date is different"));
        }
        if self.moves.len() > MAX_LIST {
            out.push(problem("moves", "at most 120 moved dates"));
        }
        if has_repeats(self.moves.iter().map(|m| m.from)) {
            out.push(problem("moves", "a date is moved once"));
        }
        if self
            .moves
            .iter()
            .any(|m| m.minutes.is_some_and(|n| n == 0 || n > 1440))
        {
            out.push(problem("moves", "a moved date lasts 1 to 1,440 minutes"));
        }
        let weekly_always = matches!(self.repeat, Repeat::Weekly { every: 1, .. });
        if self.starts_on.is_none() && !weekly_always {
            out.push(problem(
                "starts_on",
                "give the first date: only a weekly rule of every week can do without",
            ));
        }
        if matches!(self.repeat, Repeat::Once | Repeat::Dates { .. }) && self.end != End::Never {
            out.push(problem(
                "end",
                "one date, or chosen dates, have no end of their own",
            ));
        }
        // What follows reads the dates the rule gives, so only for a rule whose shape stands.
        if out.is_empty() {
            for d in &self.skip {
                if !self.gives(*d) {
                    out.push(problem("skip", format!("the rule gives no date on {d}")));
                }
            }
            for m in &self.moves {
                if !self.gives(m.from) {
                    out.push(problem(
                        "moves",
                        format!("the rule gives no date on {}", m.from),
                    ));
                }
            }
        }
        if out.is_empty() && !self.moves.is_empty() {
            self.check_moves(&mut out);
        }
        out
    }

    /// The first refused field as an error, for the routes that save.
    pub fn check(&self) -> Result<(), ShowError> {
        match self.problems().into_iter().next() {
            None => Ok(()),
            Some(p) => Err(ShowError::Invalid(p.message)),
        }
    }

    /// A moved date must stay strictly between the dates either side of it, so episode numbers
    /// keep their order.
    fn check_moves(&self, out: &mut Vec<RuleProblem>) {
        let Some(last) = self.moves.iter().map(|m| m.from).max() else {
            return;
        };
        let effective = self.effective_until(last);
        let at = |d: Date| effective.iter().position(|(on, _)| *on == d);
        for m in &self.moves {
            // A skipped date does not air, so its move is moot.
            let Some(i) = at(m.from) else { continue };
            let before = i.checked_sub(1).map(|j| effective[j].1);
            let after = effective.get(i + 1).map(|(_, dt)| *dt);
            let late = before.is_some_and(|b| m.to <= b);
            let early = after.is_some_and(|a| m.to >= a);
            if late || early {
                let (lo, hi) = (
                    before.map(|b| b.date().to_string()),
                    after.map(|a| a.date().to_string()),
                );
                let range = match (lo, hi) {
                    (Some(lo), Some(hi)) => format!("between {lo} and {hi}"),
                    (Some(lo), None) => format!("after {lo}"),
                    (None, Some(hi)) => format!("before {hi}"),
                    (None, None) => "in its place".to_owned(),
                };
                out.push(problem(
                    "moves",
                    format!(
                        "keep the date moved from {} {range}, so the episodes stay in order",
                        m.from
                    ),
                ));
            }
        }
    }

    /// The dates that air (skips removed, the end applied), each with its effective local start
    /// (a move applied), up to the first one after `last`.
    fn effective_until(&self, last: Date) -> Vec<(Date, DateTime)> {
        let moves: HashMap<Date, &Move> = self.moves.iter().map(|m| (m.from, m)).collect();
        let mut out = Vec::new();
        for d in self.dates_from(self.starts_on.unwrap_or(last)) {
            let nominal = self.nominal(d);
            out.push((d, moves.get(&d).map_or(nominal, |m| m.to)));
            if d > last {
                break;
            }
        }
        out
    }

    fn nominal(&self, d: Date) -> DateTime {
        d.at((self.start / 60) as i8, (self.start % 60) as i8, 0, 0)
    }

    /// Whether the repeat gives this date (before skips and the count; up to `Until`).
    pub fn gives(&self, date: Date) -> bool {
        if let End::Until { until } = self.end
            && date > until
        {
            return false;
        }
        self.raw(self.starts_on.unwrap_or(date))
            .take_while(|d| *d <= date)
            .any(|d| d == date)
    }

    /// The dates the repeat gives from `anchor`, in order, with no end and no skips.
    fn raw(&self, anchor: Date) -> Box<dyn Iterator<Item = Date> + Send + '_> {
        match &self.repeat {
            Repeat::Once => Box::new(self.starts_on.into_iter()),
            Repeat::Dates { dates } => {
                let mut all = dates.clone();
                all.sort();
                all.dedup();
                Box::new(all.into_iter())
            }
            Repeat::Weekly { every, weekdays } => {
                let mut days = weekdays.clone();
                days.sort_unstable();
                days.dedup();
                let step = i64::from((*every).max(1)) * 7;
                let monday = anchor
                    .checked_sub(i64::from(anchor.weekday().to_monday_zero_offset()).days())
                    .ok();
                periodic(move |k| {
                    let week = monday?.checked_add((step * k).days()).ok()?;
                    Some(
                        days.iter()
                            .filter_map(|w| week.checked_add((i64::from(*w) - 1).days()).ok())
                            .filter(|d| *d >= anchor)
                            .collect(),
                    )
                })
            }
            Repeat::MonthlyNth {
                every,
                nth,
                weekday,
            } => {
                let (every, nth) = (i64::from((*every).max(1)), *nth);
                let weekday =
                    Weekday::from_monday_one_offset(*weekday as i8).unwrap_or(Weekday::Monday);
                periodic(move |k| {
                    let first = first_of_month(anchor, every * k)?;
                    Some(
                        first
                            .nth_weekday_of_month(nth, weekday)
                            .ok()
                            .filter(|d| *d >= anchor)
                            .into_iter()
                            .collect(),
                    )
                })
            }
            Repeat::MonthlyDay { every, day } => {
                let (every, day) = (i64::from((*every).max(1)), *day);
                periodic(move |k| {
                    let first = first_of_month(anchor, every * k)?;
                    let d = if day == -1 {
                        Some(first.last_of_month())
                    } else if day >= 1 && day <= first.days_in_month() {
                        first.with().day(day).build().ok()
                    } else {
                        None
                    };
                    Some(d.filter(|d| *d >= anchor).into_iter().collect())
                })
            }
            Repeat::Yearly { month, day } => {
                let (month, day) = (*month as i8, *day as i8);
                periodic(move |k| {
                    let year = i64::from(anchor.year()) + k;
                    if year > 9999 {
                        return None;
                    }
                    Some(
                        Date::new(year as i16, month, day)
                            .ok()
                            .filter(|d| *d >= anchor)
                            .into_iter()
                            .collect(),
                    )
                })
            }
        }
    }

    /// The dates that air from `anchor` on, before the instant is worked out: the repeat, up to
    /// `Until`, without the skipped dates, at most `Count` of them counted from the start.
    /// (`Count` counts from the rule's own start, so `anchor` is that start; callers pass the
    /// rule's `starts_on` and filter later dates themselves.)
    fn dates_from(&self, anchor: Date) -> impl Iterator<Item = Date> + '_ {
        let until = match self.end {
            End::Until { until } => Some(until),
            _ => None,
        };
        let limit = match self.end {
            End::Count { count } => count as usize,
            _ => usize::MAX,
        };
        let skip: HashSet<Date> = self.skip.iter().copied().collect();
        self.raw(anchor)
            .take_while(move |d| until.is_none_or(|u| *d <= u))
            .filter(move |d| !skip.contains(d))
            .take(limit)
    }

    /// Every date the rule gives from `from` (a local date), in order, as instants in `zone`:
    /// skips removed, moves applied, the end honoured. Lazy; a rule without an end goes on.
    /// `Count` is counted from the rule's own start, whatever `from` is.
    pub fn occurrences<'a>(
        &'a self,
        zone: &'a TimeZone,
        from: Date,
    ) -> impl Iterator<Item = Occurrence> + 'a {
        let moves: HashMap<Date, &Move> = self.moves.iter().map(|m| (m.from, m)).collect();
        self.dates_from(self.starts_on.unwrap_or(from))
            .filter(move |d| *d >= from)
            .filter_map(move |d| self.occurrence(zone, d, moves.get(&d).copied()))
    }

    fn occurrence(&self, zone: &TimeZone, on: Date, moved: Option<&Move>) -> Option<Occurrence> {
        let nominal = self.nominal(on);
        let (local, minutes, moved_from) = match moved {
            Some(m) => (
                m.to,
                m.minutes.unwrap_or_else(|| self.length()),
                Some(nominal),
            ),
            None => (nominal, self.length(), None),
        };
        let amb = zone.to_ambiguous_zoned(local);
        let clock = match amb.offset() {
            AmbiguousOffset::Gap { .. } => Some(ClockChange::Forward),
            AmbiguousOffset::Fold { .. } => Some(ClockChange::Back),
            AmbiguousOffset::Unambiguous { .. } => None,
        };
        let start = amb.compatible().ok()?.timestamp();
        Some(Occurrence {
            on,
            start,
            minutes,
            moved_from,
            clock,
        })
    }

    /// The same, with the skipped dates kept and marked: for the preview, which shows them
    /// struck through. `true` is a skipped date.
    pub fn occurrences_with_skips<'a>(
        &'a self,
        zone: &'a TimeZone,
        from: Date,
    ) -> impl Iterator<Item = (Occurrence, bool)> + 'a {
        let moves: HashMap<Date, &Move> = self.moves.iter().map(|m| (m.from, m)).collect();
        let skip: HashSet<Date> = self.skip.iter().copied().collect();
        let until = match self.end {
            End::Until { until } => Some(until),
            _ => None,
        };
        let limit = match self.end {
            End::Count { count } => count as usize,
            _ => usize::MAX,
        };
        let mut counted = 0usize;
        self.raw(self.starts_on.unwrap_or(from))
            .take_while(move |d| until.is_none_or(|u| *d <= u))
            .take_while(move |d| {
                // The count is used up by dates that air; stop at the date after the last one.
                let skipped = skip.contains(d);
                if !skipped {
                    counted += 1;
                }
                counted <= limit
            })
            .filter(move |d| *d >= from)
            .filter_map(move |d| {
                let skipped = self.skip.contains(&d);
                self.occurrence(zone, d, moves.get(&d).copied().filter(|_| !skipped))
                    .map(|o| (o, skipped))
            })
    }

    /// Whether the rule gives any date at all, from its start.
    pub fn gives_any(&self, zone: &TimeZone, from: Date) -> bool {
        self.occurrences(zone, from).next().is_some()
    }

    /// Words for the rule: the sentence and the short form (design section 2). `today` is the
    /// station's date, which decides where a year is added or a start is left out.
    pub fn words(&self, zone: &TimeZone, today: Date) -> RuleWords {
        let times = self.times_words();
        let mut sentence = format!("{}, {times}", self.pattern_words(today));
        if let Some(start) = self.start_words(today) {
            sentence.push_str(", ");
            sentence.push_str(&start);
        }
        match self.end {
            End::Never => {}
            End::Until { until } => {
                sentence.push_str(&format!(", until {}", long_date(until, false, today)));
            }
            End::Count { count } => {
                sentence.push_str(&format!(
                    ", for {count} episode{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
        }
        sentence.push('.');
        if let Some(e) = self.exceptions_words(today) {
            sentence.push(' ');
            sentence.push_str(&e);
        }
        let last_on = match self.end {
            End::Never => None,
            _ => self
                .occurrences(zone, self.starts_on.unwrap_or(today))
                .take(20_000)
                .last()
                .map(|o| o.on),
        };
        RuleWords {
            sentence,
            short: self.short_words(today),
            last: last_on.map(|d| format!("The last is {}.", long_date(d, true, today))),
            last_on,
            notes: self.notes(today),
        }
    }

    fn pattern_words(&self, today: Date) -> String {
        match &self.repeat {
            Repeat::Once => match self.starts_on {
                Some(d) => format!("Once, on {}", long_date(d, true, today)),
                None => "Once".to_owned(),
            },
            Repeat::Weekly { every, weekdays } => {
                let days = sorted(weekdays);
                if *every == 1 && days.len() == 7 {
                    "Every day".to_owned()
                } else if *every == 1 {
                    format!("Every week on {}", days_words(&days, false))
                } else {
                    format!("Every {every} weeks on {}", days_words(&days, false))
                }
            }
            Repeat::MonthlyNth {
                every,
                nth,
                weekday,
            } => format!(
                "{} on the {} {}",
                every_months(*every),
                nth_word(*nth),
                weekday_name(*weekday)
            ),
            Repeat::MonthlyDay { every, day } => {
                if *day == -1 {
                    format!("{} on the last day", every_months(*every))
                } else {
                    format!("{} on the {}", every_months(*every), ordinal(*day as u32))
                }
            }
            Repeat::Yearly { month, day } => {
                format!("Every year on {day} {}", month_name(*month))
            }
            Repeat::Dates { dates } => format!(
                "On {} date{} you chose",
                dates.len(),
                if dates.len() == 1 { "" } else { "s" }
            ),
        }
    }

    fn times_words(&self) -> String {
        let end = u32::from(self.start) + u32::from(self.length());
        format!(
            "{} to {}{}",
            hhmm(self.start),
            hhmm((end % 1440) as u16),
            if end >= 1440 { " the next day" } else { "" }
        )
    }

    /// *from 18 November*: left out for a one-off, and once the first date has come.
    fn start_words(&self, today: Date) -> Option<String> {
        let first = self.starts_on?;
        if matches!(self.repeat, Repeat::Once) || first <= today {
            return None;
        }
        Some(match self.repeat {
            Repeat::Yearly { .. } => {
                if first.year() == today.year() {
                    return None;
                }
                format!("from {}", first.year())
            }
            Repeat::Dates { .. } => format!("from {}", long_date(first, true, today)),
            _ => format!("from {}", long_date(first, false, today)),
        })
    }

    fn exceptions_words(&self, today: Date) -> Option<String> {
        let mut parts = Vec::new();
        if !self.skip.is_empty() {
            let mut dates = self.skip.clone();
            dates.sort();
            if dates.len() <= 2 {
                let named: Vec<String> =
                    dates.iter().map(|d| long_date(*d, false, today)).collect();
                parts.push(format!("Not on {}.", named.join(" and ")));
            } else {
                parts.push(format!("{} dates skipped.", dates.len()));
            }
        }
        if !self.moves.is_empty() {
            let mut moves: Vec<&Move> = self.moves.iter().collect();
            moves.sort_by_key(|m| m.from);
            if moves.len() <= 2 {
                for m in moves {
                    let at = if m.to.time() == self.nominal(m.from).time() {
                        String::new()
                    } else {
                        format!(" at {:02}:{:02}", m.to.hour(), m.to.minute())
                    };
                    parts.push(format!(
                        "{} moves to {}{at}.",
                        long_date(m.from, false, today),
                        long_date(m.to.date(), false, today)
                    ));
                }
            } else {
                parts.push(format!("{} dates moved.", moves.len()));
            }
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    fn short_words(&self, today: Date) -> String {
        let end = u32::from(self.start) + u32::from(self.length());
        let times = format!("{}–{}", hhmm(self.start), hhmm((end % 1440) as u16));
        let head = match &self.repeat {
            Repeat::Once => match self.starts_on {
                Some(d) => format!("Once, {}", short_date(d, today)),
                None => "Once".to_owned(),
            },
            Repeat::Weekly { every, weekdays } => {
                let days = sorted(weekdays);
                if *every == 1 && days.len() == 7 {
                    "Every day".to_owned()
                } else if *every == 1 {
                    days_words(&days, true)
                } else {
                    format!("Every {every} weeks, {}", days_words(&days, true))
                }
            }
            Repeat::MonthlyNth {
                every,
                nth,
                weekday,
            } => {
                let what = format!("{} {}", capital(nth_word(*nth)), weekday_abbr(*weekday));
                if *every == 1 {
                    format!("{what} monthly")
                } else {
                    format!("{what} every {every} months")
                }
            }
            Repeat::MonthlyDay { every, day } => {
                let what = if *day == -1 {
                    "Last day".to_owned()
                } else {
                    ordinal(*day as u32)
                };
                if *every == 1 {
                    format!("{what} monthly")
                } else {
                    format!("{what} every {every} months")
                }
            }
            Repeat::Yearly { month, day } => {
                format!("Yearly, {day} {}", &month_name(*month)[..3])
            }
            Repeat::Dates { dates } => format!(
                "{} chosen date{}",
                dates.len(),
                if dates.len() == 1 { "" } else { "s" }
            ),
        };
        format!("{head}, {times}")
    }

    /// Dates the calendar cannot give, said rather than hidden.
    fn notes(&self, today: Date) -> Vec<String> {
        let mut notes = Vec::new();
        match &self.repeat {
            Repeat::MonthlyDay { day, .. } if (29..=31).contains(day) => {
                notes.push(format!(
                    "Months without a {} are skipped.",
                    ordinal(*day as u32)
                ));
            }
            Repeat::Yearly { month: 2, day: 29 } => {
                let next = (today.year()..today.year() + 9)
                    .filter_map(|y| Date::new(y, 2, 29).ok())
                    .find(|d| *d >= today);
                notes.push(match next {
                    Some(d) => format!(
                        "Only in leap years. The next is {}.",
                        long_date(d, false, today)
                    ),
                    None => "Only in leap years.".to_owned(),
                });
            }
            _ => {}
        }
        notes
    }
}

/// Run a per-period generator: period `k` gives its dates (already in order); the iterator stops
/// when the generator says it is out of range, or after a thousand empty periods in a row.
fn periodic<'a>(
    mut period: impl FnMut(i64) -> Option<Vec<Date>> + Send + 'a,
) -> Box<dyn Iterator<Item = Date> + Send + 'a> {
    let mut k = 0i64;
    let mut buffered: VecDeque<Date> = VecDeque::new();
    let mut empty = 0u32;
    let mut done = false;
    Box::new(std::iter::from_fn(move || {
        loop {
            if let Some(d) = buffered.pop_front() {
                return Some(d);
            }
            if done {
                return None;
            }
            match period(k) {
                None => done = true,
                Some(v) if v.is_empty() => {
                    empty += 1;
                    if empty >= EMPTY_PERIODS {
                        done = true;
                    }
                }
                Some(v) => {
                    empty = 0;
                    buffered.extend(v);
                }
            }
            k += 1;
        }
    }))
}

/// The first of the month `months` after `anchor`'s.
fn first_of_month(anchor: Date, months: i64) -> Option<Date> {
    let index = i64::from(anchor.year()) * 12 + i64::from(anchor.month() - 1) + months;
    let (y, m) = (index.div_euclid(12), index.rem_euclid(12) + 1);
    if !(-9999..=9999).contains(&y) {
        return None;
    }
    Date::new(y as i16, m as i8, 1).ok()
}

fn sorted(weekdays: &[u8]) -> Vec<u8> {
    let mut d = weekdays.to_vec();
    d.sort_unstable();
    d.dedup();
    d
}

/// *Wednesday and Friday*, *Monday to Friday*, *Monday, Wednesday and Friday*; abbreviated for
/// the short form (*Mon–Fri*).
fn days_words(days: &[u8], short: bool) -> String {
    let name = |d: u8| {
        if short {
            weekday_abbr(d)
        } else {
            weekday_name(d)
        }
    };
    let to = if short { "–" } else { " to " };
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < days.len() {
        let mut j = i;
        while j + 1 < days.len() && days[j + 1] == days[j] + 1 {
            j += 1;
        }
        if j - i >= 2 {
            parts.push(format!("{}{to}{}", name(days[i]), name(days[j])));
            i = j + 1;
        } else {
            parts.push(name(days[i]).to_owned());
            i += 1;
        }
    }
    join_and(&parts, short)
}

/// *a and b*, *a, b and c*; the short form's list has the same words.
fn join_and(parts: &[String], _short: bool) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn every_months(every: u8) -> String {
    if every == 1 {
        "Every month".to_owned()
    } else {
        format!("Every {every} months")
    }
}

fn nth_word(nth: i8) -> &'static str {
    match nth {
        1 => "first",
        2 => "second",
        3 => "third",
        4 => "fourth",
        _ => "last",
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

pub fn weekday_name(d: u8) -> &'static str {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ][(d.clamp(1, 7) - 1) as usize]
}

fn weekday_abbr(d: u8) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(d.clamp(1, 7) - 1) as usize]
}

pub fn month_name(m: u8) -> &'static str {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][(m.clamp(1, 12) - 1) as usize]
}

/// *07:00*, from minutes after midnight.
pub fn hhmm(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// *Monday 30 November* or *30 November*; the year is added only when it is not this year's.
pub fn long_date(d: Date, weekday: bool, today: Date) -> String {
    let wd = if weekday {
        format!(
            "{} ",
            weekday_name(d.weekday().to_monday_one_offset() as u8)
        )
    } else {
        String::new()
    };
    let year = if d.year() == today.year() {
        String::new()
    } else {
        format!(" {}", d.year())
    };
    format!("{wd}{} {}{year}", d.day(), month_name(d.month() as u8))
}

/// *Mon 30 Nov*; the year is added only when it is not this year's.
pub fn short_date(d: Date, today: Date) -> String {
    let year = if d.year() == today.year() {
        String::new()
    } else {
        format!(" {}", d.year())
    };
    format!(
        "{} {} {}{year}",
        weekday_abbr(d.weekday().to_monday_one_offset() as u8),
        d.day(),
        &month_name(d.month() as u8)[..3]
    )
}

#[cfg(test)]
mod tests;
