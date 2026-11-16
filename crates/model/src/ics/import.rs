// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A calendar file read into proposed shows (ADR-0281): one per recurring event, and one per
//! run of single events with the same name.
//!
//! **How a translation is judged.** A recurring event's own dates are worked out from its
//! RRULE, RDATE, EXDATE and overrides by [`super::rrule`], as instants. Its RRULE is then
//! translated into a [`Rule`] on the station's clock where the subset can say it, and the
//! translation is `exact` only if that rule gives the same instants, with the same lengths, for
//! the next two years (or to its end). Anything else (a rule the subset cannot say, a zone whose
//! clocks change on other dates than the station's, a first date the rule itself would not give)
//! is offered as chosen dates for the next twelve months, marked `dates` and said why; or
//! refused with the reason, when even that cannot be done.

use std::collections::{BTreeMap, HashMap};

use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use super::read::{self, Event, IcsError, When};
use super::rrule::{Freq, RRule, Until};
use crate::rule::{End, MAX_LIST, Move, Repeat, Rule};

/// The most dates of one RRULE read (a daily rule from 1,000 years ago), whatever the rest.
const MAX_RAW: usize = 400_000;
/// How far back a recurring event's own dates are compared with a translation: older dates
/// are counted (for COUNT) but not worked out as instants.
const LOOK_BACK_DAYS: i64 = 400;
/// A show's name and description, at most (as a show takes them).
const NAME_MAX: usize = 160;
const DESCRIPTION_MAX: usize = 4000;

/// How a calendar's event became a show's rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mapping {
    /// The rule gives exactly the calendar's dates and times.
    Exact,
    /// The calendar's rule is not one a show can have: its dates for the next twelve months
    /// (at most 120), as chosen dates.
    Dates,
    /// It cannot be made into a show; `reason` says why.
    Refused,
}

/// A show the calendar file proposes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Proposal {
    /// The event's SUMMARY, as the show's name.
    pub name: String,
    /// The event's DESCRIPTION.
    pub description: String,
    pub mapping: Mapping,
    /// The rule, on the station's clock; `null` when refused.
    pub rule: Option<Rule>,
    /// Why it is `dates`, or why it is refused.
    pub reason: Option<String>,
    /// Things to say that change nothing: dates left out, a zone converted.
    pub notes: Vec<String>,
    /// The zone the calendar gave its times in (IANA), when it is not the station's.
    pub zone: Option<String>,
    /// The calendar's zone changes its clocks on other dates than the station's, so its times
    /// are not always the same on the station's clock.
    pub clock_differs: bool,
    /// How many of the file's VEVENTs went into it.
    pub events: u32,
    /// The calendar's UID for a recurring event.
    pub uid: Option<String>,
    /// The calendar's RRULE as written, for a recurring event.
    pub source_rule: Option<String>,
}

/// How far two rules' dates are compared to say they are one show's: two years from the first
/// date to come, as a translation is judged.
const SAME_DAYS: i64 = 731;
/// The most dates of each compared (a show every day for two years is 731).
const SAME_MAX: usize = 800;

/// A show's name as two are compared: case folded, every run of spaces one space, and none at
/// either end. *The  late kettle* is *The Late Kettle*.
pub fn fold_name(name: &str) -> String {
    name.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether two rules give the same dates still to end, at the same times and lengths, for two
/// years from the first of them (or to their ends). The rules are compared by what they give,
/// not as written: a calendar file's rule starts at the first date its repeat gives, a
/// station's may start on any day before it or have no start date at all, and both are the
/// same show. Two rules with no date to come are not the same.
pub fn same_dates_to_come(a: &Rule, b: &Rule, zone: &TimeZone, now: Timestamp) -> bool {
    let from = (now - SignedDuration::from_hours(48))
        .to_zoned(zone.clone())
        .date();
    let Some(first) = a.occurrences(zone, from).find(|o| o.end() > now) else {
        return false;
    };
    let horizon = first.start + SignedDuration::from_hours(24 * SAME_DAYS);
    let dates = |r: &Rule| -> Vec<(Timestamp, u16)> {
        r.occurrences(zone, from)
            .filter(|o| o.end() > now)
            .take_while(|o| o.start < horizon)
            .take(SAME_MAX)
            .map(|o| (o.start, o.minutes))
            .collect()
    };
    dates(a) == dates(b)
}

impl Proposal {
    /// Whether this is a show the station already has: the same name (case and spaces folded,
    /// [`fold_name`]) and the same dates and times still to come ([`same_dates_to_come`]).
    /// `rule` is the show's as its own calendar file writes it. A station's own calendar read
    /// back is then every show it has, and not a clash of each with itself.
    pub fn is_show(&self, name: &str, rule: &Rule, zone: &TimeZone, now: Timestamp) -> bool {
        self.rule.as_ref().is_some_and(|mine| {
            fold_name(&self.name) == fold_name(name) && same_dates_to_come(mine, rule, zone, now)
        })
    }
}

/// What a calendar file proposes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposed {
    /// The VEVENTs read.
    pub events: usize,
    pub proposals: Vec<Proposal>,
    /// About the file as a whole: cancelled events left out, changed dates that match nothing.
    pub notes: Vec<String>,
}

/// Read a calendar file and propose its shows, on the clock of `station` (its IANA name).
pub fn propose(
    bytes: &[u8],
    station: &TimeZone,
    station_name: &str,
    now: Timestamp,
) -> Result<Proposed, IcsError> {
    let events = read::events(bytes)?;
    let count = events.len();
    let ctx = Ctx {
        station,
        station_name,
        now,
        today: now.to_zoned(station.clone()).date(),
        early: now - SignedDuration::from_hours(24 * LOOK_BACK_DAYS),
    };
    let mut notes = Vec::new();
    // Masters and their overrides, by UID; an override with no master stands alone.
    let mut overrides: HashMap<String, Vec<Event>> = HashMap::new();
    let mut masters: Vec<Event> = Vec::new();
    for e in events {
        match (&e.recurrence_id, &e.uid) {
            (Some(_), Some(uid)) => overrides.entry(uid.clone()).or_default().push(e),
            _ => masters.push(e),
        }
    }
    let master_uids: std::collections::HashSet<String> =
        masters.iter().filter_map(|m| m.uid.clone()).collect();
    let mut orphans = Vec::new();
    overrides.retain(|uid, list| {
        if master_uids.contains(uid) {
            true
        } else {
            orphans.append(list);
            false
        }
    });
    let mut cancelled = 0;
    // Items in the file's order: a recurring event where it stands, a run of singles where its
    // first stands.
    let mut items: Vec<(usize, Item)> = Vec::new();
    let mut groups: BTreeMap<String, (usize, Vec<Event>)> = BTreeMap::new();
    for (i, e) in masters.into_iter().chain(orphans).enumerate() {
        if e.cancelled {
            cancelled += 1;
            continue;
        }
        if e.rrule.is_some() || !e.rdates.is_empty() || !e.rdate_lengths.is_empty() {
            let own = e
                .uid
                .as_ref()
                .and_then(|u| overrides.remove(u))
                .unwrap_or_default();
            items.push((i, Item::Recurring(Box::new(e), own)));
        } else {
            let key = e.summary.trim().to_lowercase();
            groups.entry(key).or_insert((i, Vec::new())).1.push(e);
        }
    }
    for (_, (i, list)) in groups {
        items.push((i, Item::Singles(list)));
    }
    items.sort_by_key(|(i, _)| *i);
    if cancelled > 0 {
        notes.push(format!(
            "{cancelled} cancelled event{} left out.",
            if cancelled == 1 { " was" } else { "s were" }
        ));
    }
    let proposals = items
        .into_iter()
        .map(|(_, item)| match item {
            Item::Recurring(e, own) => ctx.recurring(&e, &own),
            Item::Singles(list) => ctx.singles(&list),
        })
        .collect();
    Ok(Proposed {
        events: count,
        proposals,
        notes,
    })
}

enum Item {
    Recurring(Box<Event>, Vec<Event>),
    Singles(Vec<Event>),
}

struct Ctx<'a> {
    station: &'a TimeZone,
    station_name: &'a str,
    now: Timestamp,
    today: Date,
    /// Dates before this are counted but not compared.
    early: Timestamp,
}

/// One date an event gives: its own date in its zone, its instant and its length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inst {
    date: Date,
    start: Timestamp,
    minutes: u16,
}

fn name_of(summary: &str) -> String {
    let n = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = if n.is_empty() {
        "Untitled".to_owned()
    } else {
        n
    };
    n.chars().take(NAME_MAX).collect()
}

fn description_of(d: &str) -> String {
    d.trim().chars().take(DESCRIPTION_MAX).collect()
}

fn refused(e: &Event, events: u32, reason: impl Into<String>) -> Proposal {
    Proposal {
        name: name_of(&e.summary),
        description: description_of(&e.description),
        mapping: Mapping::Refused,
        rule: None,
        reason: Some(reason.into()),
        notes: Vec::new(),
        zone: None,
        clock_differs: false,
        events,
        uid: e.uid.clone(),
        source_rule: e.rrule.clone(),
    }
}

/// An instant for a value, a floating time (and a whole day's midnight) read on `floating`.
fn instant(w: &When, floating: &TimeZone) -> Option<Timestamp> {
    match w {
        When::Utc(t) => Some(*t),
        When::Zoned(dt, z) => TimeZone::get(&z.iana)
            .ok()?
            .to_ambiguous_zoned(*dt)
            .compatible()
            .ok()
            .map(|z| z.timestamp()),
        When::Date(_) | When::Floating(_) => floating
            .to_ambiguous_zoned(w.local())
            .compatible()
            .ok()
            .map(|z| z.timestamp()),
    }
}

/// A value's date in `zone` (a whole day is that day).
fn date_in(w: &When, zone: &TimeZone) -> Option<Date> {
    match w {
        When::Date(d) => Some(*d),
        _ => instant(w, zone).map(|t| t.to_zoned(zone.clone()).date()),
    }
}

/// An event's length: from DURATION, else DTEND; `None` when it gives none (or none above
/// zero). `Err` when it lasts more than a day.
fn length(e: &Event, zone: &TimeZone) -> Result<Option<u16>, String> {
    let d: Option<SignedDuration> = match (&e.duration, &e.end) {
        (Some(d), _) => Some(*d),
        (None, Some(end)) => {
            let (a, b) = (instant(&e.start, zone), instant(end, zone));
            match (a, b) {
                (Some(a), Some(b)) => Some(b.duration_since(a)),
                _ => None,
            }
        }
        _ => None,
    };
    let Some(d) = d else { return Ok(None) };
    let minutes = (d.as_secs() + 30).div_euclid(60);
    if minutes <= 0 {
        return Ok(None);
    }
    if minutes > 1440 {
        return Err("It lasts more than a day: a show lasts at most 24 hours.".into());
    }
    Ok(Some(minutes as u16))
}

/// The zone an event's start is in: `None` for a floating time (the station's clock).
fn zone_of(w: &When) -> Option<(TimeZone, String)> {
    match w {
        When::Utc(_) => Some((TimeZone::UTC, "UTC".into())),
        When::Zoned(_, z) => TimeZone::get(&z.iana).ok().map(|tz| (tz, z.iana.clone())),
        _ => None,
    }
}

fn minutes_of(t: Time) -> u16 {
    (t.hour() as u16) * 60 + t.minute() as u16
}

impl Ctx<'_> {
    fn local(&self, t: Timestamp) -> DateTime {
        t.to_zoned(self.station.clone()).datetime()
    }

    /// A recurring event (an RRULE, or RDATEs) with its overrides.
    fn recurring(&self, e: &Event, own: &[Event]) -> Proposal {
        let events = 1 + own.len() as u32;
        if let Some(z) = &e.unknown_zone {
            return refused(
                e,
                events,
                format!("Its time zone, “{z}”, is not one we know."),
            );
        }
        if matches!(e.start, When::Date(_)) {
            return refused(
                e,
                events,
                "It is an all-day event: a show needs a time to start.",
            );
        }
        let (ez, zone_name) = zone_of(&e.start)
            .map(|(z, n)| (z, Some(n)))
            .unwrap_or((self.station.clone(), None));
        let same_zone = zone_name.as_deref().is_none_or(|n| n == self.station_name);
        let minutes = match length(e, &ez) {
            Ok(m) => m,
            Err(r) => return refused(e, events, r),
        };
        let mut notes = Vec::new();
        if minutes.is_none() {
            notes.push("It gives no length, so it is an hour.".to_owned());
        }
        let length_or = minutes.unwrap_or(crate::rule::DEFAULT_MINUTES);
        let rrule = match e.rrule.as_deref().map(RRule::parse).transpose() {
            Ok(r) => r,
            Err(r) => return refused(e, events, format!("It cannot be read: {r}.")),
        };
        if let Some(r) = rrule.as_ref().and_then(RRule::more_than_daily) {
            return refused(e, events, r);
        }
        let start_dt = e.start.local();
        let Some(start_at) = instant(&e.start, &ez) else {
            return refused(e, events, "Its start is not a time we can read.");
        };
        // Work out the event's own dates far enough for the comparison and the twelve months.
        let horizon = start_at.max(self.now) + SignedDuration::from_hours(24 * 731);
        let src = match self.expand(e, rrule.as_ref(), own, &ez, length_or, horizon) {
            Ok(s) => s,
            Err(r) => return refused(e, events, r),
        };
        notes.extend(src.notes.iter().cloned());
        let zone_note = |notes: &mut Vec<String>| {
            if let Some(n) = &zone_name
                && !same_zone
            {
                notes.push(format!(
                    "Its times are converted from {n} to the station's clock."
                ));
            }
        };
        let base = Proposal {
            name: name_of(&e.summary),
            description: description_of(&e.description),
            mapping: Mapping::Exact,
            rule: None,
            reason: None,
            notes: Vec::new(),
            zone: zone_name.clone().filter(|_| !same_zone),
            clock_differs: false,
            events,
            uid: e.uid.clone(),
            source_rule: e.rrule.clone(),
        };
        if src.insts.is_empty() {
            let reason = if src.raw_dates.is_empty() {
                "It gives no dates."
            } else {
                "Its dates have all passed."
            };
            return Proposal {
                mapping: Mapping::Refused,
                reason: Some(reason.into()),
                notes,
                ..base
            };
        }
        // Chosen dates alone (RDATE without an RRULE): exact while all of them fit.
        if rrule.is_none() {
            zone_note(&mut notes);
            return self.as_dates(base, &src.insts, notes, None, true);
        }
        let rrule = rrule.expect("checked");
        let station_times: std::collections::BTreeSet<u16> = src
            .insts
            .iter()
            .filter(|i| !src.moved.contains(&i.date))
            .map(|i| minutes_of(self.local(i.start).time()))
            .collect();
        let clock_differs = !same_zone && station_times.len() > 1;
        let candidate = self
            .candidate(e, &rrule, own, &ez, start_dt, minutes, &src)
            .and_then(|r| {
                if same_zone {
                    Ok(r)
                } else {
                    self.convert(r, &ez)
                }
            });
        let why = match candidate {
            Ok(rule) => {
                let problems = rule.problems();
                if problems.is_empty() && self.same_dates(&rule, &src.insts, horizon) {
                    zone_note(&mut notes);
                    let from = self.today.yesterday().unwrap_or(self.today);
                    let passed = !rule
                        .occurrences(self.station, from)
                        .any(|o| o.end() > self.now);
                    if passed {
                        return Proposal {
                            mapping: Mapping::Refused,
                            reason: Some("Its dates have all passed.".into()),
                            notes,
                            ..base
                        };
                    }
                    return Proposal {
                        rule: Some(rule),
                        notes,
                        ..base
                    };
                }
                match problems.into_iter().next() {
                    Some(p) => format!("As a show's rule it would not stand ({}).", p.message),
                    None if clock_differs => format!(
                        "The clocks change on other dates in {}, so its time on the station's clock is not always the same.",
                        zone_name.as_deref().unwrap_or("its zone")
                    ),
                    None => "Its dates are not quite what any show's rule gives.".to_owned(),
                }
            }
            Err(why) => why,
        };
        zone_note(&mut notes);
        // Its dates for the next twelve months, from today or from its first date.
        let first = self.local(src.insts[0].start).date();
        let from = first.max(self.today);
        let until = from.checked_add(1.year()).unwrap_or(from);
        let window: Vec<Inst> = src
            .insts
            .iter()
            .copied()
            .filter(|i| {
                let d = self.local(i.start).date();
                d >= from && d < until
            })
            .collect();
        let p = Proposal {
            clock_differs,
            ..base
        };
        if window.is_empty() {
            return Proposal {
                mapping: Mapping::Refused,
                reason: Some(format!("{why} It has no dates in the next twelve months.")),
                notes,
                ..p
            };
        }
        self.as_dates(
            p,
            &window,
            notes,
            Some(format!(
                "{why} These are its dates for the next twelve months."
            )),
            false,
        )
    }

    /// Single events with one name: one show on chosen dates (or once).
    fn singles(&self, list: &[Event]) -> Proposal {
        let first = &list[0];
        let events = list.len() as u32;
        let mut notes = Vec::new();
        let mut insts = Vec::new();
        let (mut all_day, mut unknown, mut too_long) = (0, 0, 0);
        let mut zones: std::collections::BTreeSet<String> = Default::default();
        for e in list {
            if e.unknown_zone.is_some() {
                unknown += 1;
                continue;
            }
            if matches!(e.start, When::Date(_)) {
                all_day += 1;
                continue;
            }
            let (ez, zn) = zone_of(&e.start)
                .map(|(z, n)| (z, Some(n)))
                .unwrap_or((self.station.clone(), None));
            if let Some(n) = zn.filter(|n| n != self.station_name) {
                zones.insert(n);
            }
            let minutes = match length(e, &ez) {
                Ok(m) => m.unwrap_or(crate::rule::DEFAULT_MINUTES),
                Err(_) => {
                    too_long += 1;
                    continue;
                }
            };
            let Some(start) = instant(&e.start, &ez) else {
                continue;
            };
            insts.push(Inst {
                date: e.start.local().date(),
                start,
                minutes,
            });
        }
        let base = Proposal {
            name: name_of(&first.summary),
            description: description_of(&first.description),
            mapping: Mapping::Exact,
            rule: None,
            reason: None,
            notes: Vec::new(),
            zone: (zones.len() == 1)
                .then(|| zones.iter().next().cloned())
                .flatten(),
            clock_differs: false,
            events,
            uid: None,
            source_rule: None,
        };
        let left = |n: usize, what: &str, notes: &mut Vec<String>| {
            if n > 0 {
                notes.push(format!(
                    "{n} {what} {} left out.",
                    if n == 1 { "was" } else { "were" }
                ));
            }
        };
        left(all_day, "all-day event", &mut notes);
        left(unknown, "event in a time zone we do not know", &mut notes);
        left(too_long, "event longer than a day", &mut notes);
        for z in &zones {
            notes.push(format!(
                "Times are converted from {z} to the station's clock."
            ));
        }
        if insts.is_empty() {
            let reason = if all_day == list.len() {
                "It is an all-day event: a show needs a time to start."
            } else if unknown == list.len() {
                "Its time zone is not one we know."
            } else if too_long == list.len() {
                "It lasts more than a day: a show lasts at most 24 hours."
            } else {
                "It has no time we can read."
            };
            return Proposal {
                mapping: Mapping::Refused,
                reason: Some(reason.into()),
                notes,
                ..base
            };
        }
        insts.sort_by_key(|i| i.start);
        self.as_dates(base, &insts, notes, None, true)
    }

    /// Instants as a rule of chosen dates (one date alone is `once`), on the station's clock:
    /// the most usual time and length are the rule's, and a date at another time or of another
    /// length is moved. Past dates are left out; at most 120 are kept. `exact` stays exact only
    /// if nothing beyond the past had to be left out.
    fn as_dates(
        &self,
        base: Proposal,
        insts: &[Inst],
        mut notes: Vec<String>,
        reason: Option<String>,
        exact: bool,
    ) -> Proposal {
        let future: Vec<&Inst> = insts
            .iter()
            .filter(|i| i.start + SignedDuration::from_mins(i64::from(i.minutes)) > self.now)
            .collect();
        let past = insts.len() - future.len();
        if past > 0 && !future.is_empty() {
            notes.push(format!(
                "{past} date{} before today {} left out.",
                if past == 1 { "" } else { "s" },
                if past == 1 { "was" } else { "were" }
            ));
        }
        if future.is_empty() {
            return Proposal {
                mapping: Mapping::Refused,
                reason: Some("Its dates have all passed.".into()),
                notes,
                ..base
            };
        }
        let mut seen: BTreeMap<Date, (DateTime, u16)> = BTreeMap::new();
        let mut doubled = Vec::new();
        for i in future {
            let dt = self.local(i.start);
            match seen.entry(dt.date()) {
                std::collections::btree_map::Entry::Occupied(_) => doubled.push(dt.date()),
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert((dt, i.minutes));
                }
            }
        }
        doubled.dedup();
        for d in &doubled {
            notes.push(format!(
                "It has two times on {d}: a show airs once a day, so only the first is kept."
            ));
        }
        let mut truncated = false;
        if seen.len() > MAX_LIST {
            truncated = true;
            let last = *seen.keys().nth(MAX_LIST - 1).expect("long enough");
            seen.retain(|d, _| *d <= last);
            notes.push(format!(
                "A show has at most 120 chosen dates, so only those to {last} are kept."
            ));
        }
        let mode = |values: Vec<u16>| -> u16 {
            let mut count: BTreeMap<u16, usize> = BTreeMap::new();
            for v in &values {
                *count.entry(*v).or_default() += 1;
            }
            let best = count.values().copied().max().unwrap_or(0);
            // A tie goes to the first date's.
            values
                .iter()
                .copied()
                .find(|v| count[v] == best)
                .unwrap_or(0)
        };
        let start = mode(seen.values().map(|(dt, _)| minutes_of(dt.time())).collect());
        let minutes = mode(seen.values().map(|(_, m)| *m).collect());
        let dates: Vec<Date> = seen.keys().copied().collect();
        let moves: Vec<Move> = seen
            .iter()
            .filter(|(_, (dt, m))| minutes_of(dt.time()) != start || *m != minutes)
            .map(|(d, (dt, m))| Move {
                from: *d,
                to: *dt,
                minutes: (*m != minutes).then_some(*m),
            })
            .collect();
        let rule = if dates.len() == 1 {
            let (dt, m) = seen.values().next().expect("one");
            Rule {
                starts_on: Some(dt.date()),
                start: minutes_of(dt.time()),
                minutes: Some(*m),
                repeat: Repeat::Once,
                end: End::Never,
                skip: Vec::new(),
                moves: Vec::new(),
            }
        } else {
            Rule {
                starts_on: dates.first().copied(),
                start,
                minutes: Some(minutes),
                repeat: Repeat::Dates { dates },
                end: End::Never,
                skip: Vec::new(),
                moves,
            }
        };
        if let Some(p) = rule.problems().into_iter().next() {
            return Proposal {
                mapping: Mapping::Refused,
                reason: Some(format!(
                    "Its dates cannot be made into a show's rule: {}.",
                    p.message
                )),
                notes,
                ..base
            };
        }
        Proposal {
            mapping: if exact && !truncated && doubled.is_empty() {
                Mapping::Exact
            } else {
                Mapping::Dates
            },
            rule: Some(rule),
            reason: if exact && truncated {
                Some("It has more than 120 dates to come.".into())
            } else if exact && !doubled.is_empty() {
                Some("It has more than one time on a day.".into())
            } else {
                reason
            },
            notes,
            ..base
        }
    }

    /// Whether `rule` gives exactly `insts` (instants and lengths) from `early` to `horizon`.
    /// A count is counted from the rule's own start, however far back that is.
    fn same_dates(&self, rule: &Rule, insts: &[Inst], horizon: Timestamp) -> bool {
        let Some(start) = rule.starts_on else {
            return false;
        };
        let early_date = self.early.to_zoned(self.station.clone()).date();
        let from = early_date.yesterday().unwrap_or(early_date).max(start);
        let ours: Vec<(Timestamp, u16)> = rule
            .occurrences(self.station, from)
            .filter(|o| o.start >= self.early)
            .take_while(|o| o.start < horizon)
            .map(|o| (o.start, o.minutes))
            .collect();
        let theirs: Vec<(Timestamp, u16)> = insts
            .iter()
            .filter(|i| i.start >= self.early && i.start < horizon)
            .map(|i| (i.start, i.minutes))
            .collect();
        ours == theirs
    }

    /// The event's own dates up to `horizon`, as RFC 5545 gives them: the RRULE (COUNT and
    /// UNTIL applied) and RDATEs, less EXDATEs, with each override's time and length. The
    /// RRULE's dates before `early` are only counted (in `raw_dates`), not worked out as
    /// instants. `Err` is a refusal's reason.
    fn expand(
        &self,
        e: &Event,
        rrule: Option<&RRule>,
        own: &[Event],
        ez: &TimeZone,
        minutes: u16,
        horizon: Timestamp,
    ) -> Result<Source, String> {
        let start = e.start.local();
        let mut notes = Vec::new();
        let at = |d: Date, t: Time| -> Option<Timestamp> {
            ez.to_ambiguous_zoned(d.to_datetime(t))
                .compatible()
                .ok()
                .map(|z| z.timestamp())
        };
        let mut insts: Vec<Inst> = Vec::new();
        let mut raw_dates: Vec<Date> = Vec::new();
        let early = self.early.to_zoned(ez.clone()).date();
        match rrule {
            Some(r) => {
                let time = Time::new(
                    r.by_hour.first().copied().unwrap_or(start.hour()),
                    r.by_minute.first().copied().unwrap_or(start.minute()),
                    0,
                    0,
                )
                .unwrap_or(start.time());
                let days = r
                    .days(start.date())
                    .map_err(|w| format!("It cannot be read: {w}."))?;
                for (n, d) in days.enumerate() {
                    if n >= MAX_RAW || r.count.is_some_and(|c| n as u32 >= c) {
                        break;
                    }
                    let t = if n == 0 { start.time() } else { time };
                    // An old date is only counted: its instant is worked out only when UNTIL
                    // is one.
                    let old = d < early;
                    let ts = if old && !matches!(r.until, Some(Until::Utc(_))) {
                        None
                    } else {
                        match at(d, t) {
                            Some(ts) => Some(ts),
                            None => continue,
                        }
                    };
                    let ended = match r.until {
                        None => false,
                        Some(Until::Date(u)) => d > u,
                        Some(Until::Local(u)) => d.to_datetime(t) > u,
                        Some(Until::Utc(u)) => ts.is_some_and(|ts| ts > u),
                    };
                    if ended || ts.is_some_and(|ts| ts >= horizon) {
                        break;
                    }
                    raw_dates.push(d);
                    if let (false, Some(ts)) = (old, ts) {
                        insts.push(Inst {
                            date: d,
                            start: ts,
                            minutes,
                        });
                    }
                }
            }
            None => {
                let ts =
                    at(start.date(), start.time()).ok_or("Its start is not a time we can read.")?;
                insts.push(Inst {
                    date: start.date(),
                    start: ts,
                    minutes,
                });
            }
        }
        for w in &e.rdates {
            if matches!(w, When::Date(_)) {
                return Err("It has a date without a time (an all-day RDATE).".into());
            }
            let ts = instant(w, ez).ok_or("An RDATE is not a time we can read.")?;
            insts.push(Inst {
                date: ts.to_zoned(ez.clone()).date(),
                start: ts,
                minutes,
            });
        }
        for (w, len) in &e.rdate_lengths {
            let ts = instant(w, ez).ok_or("An RDATE is not a time we can read.")?;
            let m = (len.as_secs() / 60).clamp(1, 1440) as u16;
            insts.push(Inst {
                date: ts.to_zoned(ez.clone()).date(),
                start: ts,
                minutes: m,
            });
        }
        let ex: std::collections::HashSet<Date> =
            e.exdates.iter().filter_map(|w| date_in(w, ez)).collect();
        insts.retain(|i| !ex.contains(&i.date));
        let mut moved = std::collections::HashSet::new();
        let mut unmatched = 0;
        for o in own {
            let Some(rid) = o.recurrence_id.as_ref().and_then(|w| date_in(w, ez)) else {
                unmatched += 1;
                continue;
            };
            let Some(pos) = insts.iter().position(|i| i.date == rid) else {
                // Before the dates compared, past the horizon, or a date the event does not
                // give: only the last is worth a note.
                if rid >= early
                    && instant(o.recurrence_id.as_ref().expect("read"), ez)
                        .is_some_and(|t| t < horizon)
                {
                    unmatched += 1;
                }
                continue;
            };
            if o.cancelled {
                insts.remove(pos);
                continue;
            }
            if matches!(o.start, When::Date(_)) {
                return Err("One of its dates is changed to a whole day.".into());
            }
            let ts = instant(&o.start, ez).ok_or("A changed date is not a time we can read.")?;
            let m = length(o, ez)?.unwrap_or(minutes);
            insts[pos].start = ts;
            insts[pos].minutes = m;
            moved.insert(rid);
        }
        if unmatched > 0 {
            notes.push(format!(
                "{unmatched} changed date{} that the event does not give {} left out.",
                if unmatched == 1 { "" } else { "s" },
                if unmatched == 1 { "was" } else { "were" }
            ));
        }
        insts.sort_by_key(|i| i.start);
        let raw_set = raw_dates.iter().copied().collect();
        Ok(Source {
            insts,
            raw_dates,
            raw_set,
            moved,
            notes,
        })
    }

    /// The event's rule in the subset, on its own clock, or why the subset cannot say it.
    #[allow(clippy::too_many_arguments)] // the event and what is already read of it
    fn candidate(
        &self,
        e: &Event,
        r: &RRule,
        own: &[Event],
        ez: &TimeZone,
        start: DateTime,
        minutes: Option<u16>,
        src: &Source,
    ) -> Result<Rule, String> {
        let cannot = |what: &str| Err(format!("{what}, which a show's rule cannot say."));
        if !e.rdates.is_empty() || !e.rdate_lengths.is_empty() {
            return cannot("It has extra dates (RDATE) beside its rule");
        }
        if r.by_hour.first().is_some_and(|h| *h != start.hour())
            || r.by_minute.first().is_some_and(|m| *m != start.minute())
        {
            return cannot("Its rule sets a time of day other than its start");
        }
        if !r.by_year_day.is_empty() {
            return cannot("It picks days of the year (BYYEARDAY)");
        }
        let wd = |w: Weekday| w.to_monday_one_offset() as u8;
        let every = |n: u32, unit: &str| -> Result<u8, String> {
            if n > 12 {
                Err(format!(
                    "It repeats every {n} {unit}, which a show's rule cannot say."
                ))
            } else {
                Ok(n as u8)
            }
        };
        let plain_days = || -> Result<Vec<u8>, String> {
            if r.by_day.iter().any(|(n, _)| n.is_some()) {
                return Err("Its weekdays have numbers, which a show's rule cannot say.".into());
            }
            let mut d: Vec<u8> = r.by_day.iter().map(|(_, w)| wd(*w)).collect();
            d.sort_unstable();
            d.dedup();
            Ok(d)
        };
        let repeat = match r.freq {
            Freq::Daily => {
                if !r.by_month.is_empty() || !r.by_month_day.is_empty() || !r.by_set_pos.is_empty()
                {
                    return cannot("Its daily rule picks months or days");
                }
                if r.interval != 1 {
                    return Err(format!(
                        "It repeats every {} days, which a show's rule cannot say.",
                        r.interval
                    ));
                }
                let days = plain_days()?;
                Repeat::Weekly {
                    every: 1,
                    weekdays: if days.is_empty() {
                        (1..=7).collect()
                    } else {
                        days
                    },
                }
            }
            Freq::Weekly => {
                if !r.by_month.is_empty() || !r.by_month_day.is_empty() || !r.by_set_pos.is_empty()
                {
                    return cannot("Its weekly rule picks months or days of the month");
                }
                let days = plain_days()?;
                Repeat::Weekly {
                    every: every(r.interval, "weeks")?,
                    weekdays: if days.is_empty() {
                        vec![wd(start.weekday())]
                    } else {
                        days
                    },
                }
            }
            Freq::Monthly => {
                let every = every(r.interval, "months")?;
                if !r.by_month.is_empty() {
                    return cannot("Its monthly rule picks some months only");
                }
                match (
                    r.by_day.as_slice(),
                    r.by_month_day.as_slice(),
                    r.by_set_pos.as_slice(),
                ) {
                    ([], [], []) => Repeat::MonthlyDay {
                        every,
                        day: start.day(),
                    },
                    ([], [d], []) if (1..=31).contains(d) || *d == -1 => Repeat::MonthlyDay {
                        every,
                        day: *d as i8,
                    },
                    ([(Some(n), w)], [], []) if (1..=4).contains(n) || *n == -1 => {
                        Repeat::MonthlyNth {
                            every,
                            nth: *n as i8,
                            weekday: wd(*w),
                        }
                    }
                    ([(None, w)], [], [n]) if (1..=4).contains(n) || *n == -1 => {
                        Repeat::MonthlyNth {
                            every,
                            nth: *n as i8,
                            weekday: wd(*w),
                        }
                    }
                    ([(Some(5) | Some(-5), _)], [], []) => {
                        return cannot("It is on the fifth weekday of a month");
                    }
                    _ => return cannot("It picks several days of the month"),
                }
            }
            Freq::Yearly => {
                if r.interval != 1 {
                    return Err(format!(
                        "It repeats every {} years, which a show's rule cannot say.",
                        r.interval
                    ));
                }
                if !r.by_day.is_empty() || !r.by_set_pos.is_empty() {
                    return cannot("It is on a weekday of a month each year");
                }
                let (month, day) = match (r.by_month.as_slice(), r.by_month_day.as_slice()) {
                    ([], []) => (start.month(), start.day()),
                    ([m], []) => (*m, start.day()),
                    ([], [d]) if *d > 0 => (start.month(), *d as i8),
                    ([m], [d]) if *d > 0 => (*m, *d as i8),
                    _ => return cannot("It picks several dates a year"),
                };
                Repeat::Yearly {
                    month: month as u8,
                    day: day as u8,
                }
            }
            Freq::Hourly | Freq::Minutely | Freq::Secondly => {
                return cannot("It repeats within a day");
            }
        };
        let mut rule = Rule {
            starts_on: Some(start.date()),
            start: minutes_of(start.time()),
            minutes,
            repeat,
            end: End::Never,
            skip: Vec::new(),
            moves: Vec::new(),
        };
        // Skipped: the EXDATEs and the cancelled dates the rule gives.
        let mut skip: Vec<Date> = e
            .exdates
            .iter()
            .filter_map(|w| date_in(w, ez))
            .chain(
                own.iter()
                    .filter(|o| o.cancelled)
                    .filter_map(|o| o.recurrence_id.as_ref().and_then(|w| date_in(w, ez))),
            )
            .filter(|d| src.raw_set.contains(d))
            .collect();
        skip.sort();
        skip.dedup();
        // The end. COUNT counts the dates before EXDATE; a show's count counts episodes.
        if let Some(c) = r.count {
            let skipped = src
                .raw_dates
                .iter()
                .take(c as usize)
                .filter(|d| skip.contains(d))
                .count() as u32;
            let n = c.saturating_sub(skipped);
            if n == 0 {
                return Err("Every one of its dates is skipped.".into());
            }
            rule.end = End::Count { count: n };
        }
        if let Some(u) = r.until {
            let t = start.time();
            let until = match u {
                Until::Date(d) => d,
                Until::Local(dt) => {
                    if dt.time() >= t {
                        dt.date()
                    } else {
                        dt.date().yesterday().unwrap_or(dt.date())
                    }
                }
                Until::Utc(ts) => {
                    let dt = ts.to_zoned(ez.clone()).datetime();
                    if dt.time() >= t {
                        dt.date()
                    } else {
                        dt.date().yesterday().unwrap_or(dt.date())
                    }
                }
            };
            rule.end = End::Until { until };
        }
        rule.skip = skip;
        // Moved: each override that is not cancelled, at its own time on the event's clock.
        let length = minutes.unwrap_or(crate::rule::DEFAULT_MINUTES);
        for o in own.iter().filter(|o| !o.cancelled) {
            let Some(from) = o.recurrence_id.as_ref().and_then(|w| date_in(w, ez)) else {
                continue;
            };
            if !src.raw_set.contains(&from) || rule.skip.contains(&from) {
                continue;
            }
            let Some(ts) = instant(&o.start, ez) else {
                continue;
            };
            let to = ts.to_zoned(ez.clone()).datetime();
            let m = length_of(o, ez).unwrap_or(length);
            if to == from.to_datetime(start.time()) && m == length {
                continue;
            }
            rule.moves.push(Move {
                from,
                to,
                minutes: (m != length).then_some(m),
            });
        }
        rule.moves.sort_by_key(|m| m.from);
        Ok(rule)
    }

    /// A rule on another zone's clock, moved onto the station's: its start is the station's
    /// time of its first date, and a date that falls on the day before or after there moves its
    /// weekdays with it. Whether that is right on every date is for the comparison to say.
    fn convert(&self, mut r: Rule, ez: &TimeZone) -> Result<Rule, String> {
        let first = r.starts_on.expect("a candidate has a start");
        let here = |dt: DateTime| -> Option<DateTime> {
            ez.to_ambiguous_zoned(dt)
                .compatible()
                .ok()
                .map(|z| z.timestamp().to_zoned(self.station.clone()).datetime())
        };
        let nominal = first.at((r.start / 60) as i8, (r.start % 60) as i8, 0, 0);
        let there = here(nominal).ok_or("Its start is not a time we can read.")?;
        let shift = (there.date() - first).get_days() as i64;
        let by = |d: Date| d.checked_add(shift.days()).unwrap_or(d);
        r.start = minutes_of(there.time());
        r.starts_on = Some(there.date());
        if shift != 0 {
            match &mut r.repeat {
                Repeat::Weekly { weekdays, .. } => {
                    for w in weekdays.iter_mut() {
                        *w = ((i64::from(*w) - 1 + shift).rem_euclid(7) + 1) as u8;
                    }
                    weekdays.sort_unstable();
                }
                Repeat::Once => {}
                _ => {
                    return Err(
                        "On the station's clock it falls on another day, which moves a monthly or yearly rule off its pattern.".into(),
                    );
                }
            }
        }
        r.skip = r.skip.into_iter().map(by).collect();
        if let End::Until { until } = r.end {
            r.end = End::Until { until: by(until) };
        }
        for m in &mut r.moves {
            m.from = by(m.from);
            m.to = here(m.to).ok_or("A changed date is not a time we can read.")?;
        }
        Ok(r)
    }
}

/// An override's own length, when it gives one.
fn length_of(e: &Event, zone: &TimeZone) -> Option<u16> {
    length(e, zone).ok().flatten()
}

/// An event's dates as worked out.
struct Source {
    insts: Vec<Inst>,
    /// The RRULE's dates (before EXDATE), on the event's clock.
    raw_dates: Vec<Date>,
    raw_set: std::collections::HashSet<Date>,
    /// Dates an override moved.
    moved: std::collections::HashSet<Date>,
    notes: Vec<String>,
}
