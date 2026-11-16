// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Shows' rules written as a calendar file: one VEVENT per rule, with its RRULE or RDATE, its
//! EXDATE and an override VEVENT (RECURRENCE-ID) for each moved date, in the station's zone with
//! its VTIMEZONE, so a calendar works out the dates itself.

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use uuid::Uuid;

use super::rrule::weekday_code;
use super::text::{self, Out};
use super::vtimezone;
use crate::rule::{End, Repeat, Rule};

/// One show to write: only what the public already sees.
#[derive(Clone, Debug)]
pub struct ExportShow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    /// The show's page, when the station has a public address.
    pub url: Option<String>,
    pub rule: Rule,
    /// Where a weekly rule with no start date begins (an older station's weekly slot): its
    /// first episode's date, say. `None` is the date of `now`.
    pub anchor: Option<Date>,
}

/// A calendar to write.
#[derive(Clone, Debug)]
pub struct ExportCalendar {
    /// The calendar's name in a person's list of calendars.
    pub name: String,
    /// The station's zone, by its IANA name, and the zone itself.
    pub zone_name: String,
    pub zone: TimeZone,
    pub shows: Vec<ExportShow>,
    /// DTSTAMP, and the date a rule without a start begins from.
    pub now: Timestamp,
    /// The product's version, for PRODID.
    pub version: String,
}

/// A show's UID: stable for the show's life, and the same for its overrides.
pub fn uid(show: Uuid) -> String {
    format!("{show}@weavesignal")
}

/// The calendar file.
pub fn write(cal: &ExportCalendar) -> String {
    let mut out = Out::default();
    out.prop("BEGIN", "VCALENDAR");
    out.prop("VERSION", "2.0");
    out.text(
        "PRODID",
        &format!("-//WeaveSignal//WeaveSignal {}//EN", cal.version),
    );
    out.prop("CALSCALE", "GREGORIAN");
    out.prop("METHOD", "PUBLISH");
    out.text("NAME", &cal.name);
    out.text("X-WR-CALNAME", &cal.name);
    out.text("X-WR-TIMEZONE", &cal.zone_name);
    // How often a subscribed calendar looks again (RFC 7986, and the older name).
    out.prop("REFRESH-INTERVAL;VALUE=DURATION", "PT1H");
    out.prop("X-PUBLISHED-TTL", "PT1H");
    let today = cal.now.to_zoned(cal.zone.clone()).date();
    let events: Vec<(&ExportShow, Date)> = cal
        .shows
        .iter()
        .filter_map(|s| first(s, s.anchor.unwrap_or(today)).map(|d| (s, d)))
        .collect();
    if let Some(earliest) = events
        .iter()
        .map(|(s, d)| s.rule.moves.iter().map(|m| m.to.date()).fold(*d, Date::min))
        .min()
    {
        let last_listed = events
            .iter()
            .flat_map(|(s, _)| last_date(&s.rule, s.anchor.unwrap_or(today)))
            .max()
            .unwrap_or(today);
        let horizon = today.year().max(last_listed.year()).saturating_add(10);
        vtimezone::write(&mut out, &cal.zone, &cal.zone_name, earliest, horizon);
    }
    let stamp = text::utc(cal.now);
    for (show, first) in events {
        event(&mut out, cal, show, first, &stamp);
    }
    out.prop("END", "VCALENDAR");
    out.finish()
}

/// DTSTART's date: the first date the repeat gives, which must be a date of the set (RFC 5545
/// section 3.8.2.4 asks that DTSTART agree with the RRULE). `None` when the rule gives none.
fn first(show: &ExportShow, from: Date) -> Option<Date> {
    let rule = &show.rule;
    let d = rule.repeat_dates(from).next()?;
    if let End::Until { until } = rule.end
        && d > until
    {
        return None;
    }
    Some(d)
}

/// The last date a rule that ends gives (for the zone's range); `None` for one that goes on.
fn last_date(rule: &Rule, from: Date) -> Option<Date> {
    match (&rule.repeat, rule.end.clone()) {
        (Repeat::Dates { dates }, _) => dates.iter().max().copied(),
        (Repeat::Once, _) => rule.starts_on,
        (_, End::Until { until }) => Some(until),
        (_, End::Count { .. }) => counted(rule, from).map(|(_, last)| last),
        (_, End::Never) => None,
    }
}

/// For a rule that ends after a count of episodes: how many dates of the repeat that takes
/// (skipped dates included, as COUNT counts them before EXDATE), and the last.
fn counted(rule: &Rule, from: Date) -> Option<(u32, Date)> {
    let End::Count { count } = rule.end else {
        return None;
    };
    let mut aired = 0u32;
    let mut taken = 0u32;
    for d in rule.repeat_dates(from).take(100_000) {
        taken += 1;
        if !rule.skip.contains(&d) {
            aired += 1;
        }
        if aired == count {
            return Some((taken, d));
        }
    }
    None
}

fn at(rule: &Rule, d: Date) -> DateTime {
    d.at((rule.start / 60) as i8, (rule.start % 60) as i8, 0, 0)
}

/// The wall clock `minutes` after `start`, counted in elapsed time, as the station reckons a
/// show's end.
fn end_of(zone: &TimeZone, start: DateTime, minutes: u16) -> DateTime {
    match zone.to_ambiguous_zoned(start).compatible() {
        Ok(z) => z
            .timestamp()
            .checked_add(i64::from(minutes).minutes())
            .map(|t| t.to_zoned(zone.clone()).datetime())
            .unwrap_or(start),
        Err(_) => start,
    }
}

fn rrule(rule: &Rule, from: Date) -> Option<String> {
    let mut r = match &rule.repeat {
        Repeat::Once | Repeat::Dates { .. } => return None,
        Repeat::Weekly { every, weekdays } => {
            let mut days = weekdays.clone();
            days.sort_unstable();
            days.dedup();
            let days: Vec<&str> = days
                .iter()
                .filter_map(|d| jiff::civil::Weekday::from_monday_one_offset(*d as i8).ok())
                .map(weekday_code)
                .collect();
            format!("FREQ=WEEKLY{};BYDAY={}", interval(*every), days.join(","))
        }
        Repeat::MonthlyNth {
            every,
            nth,
            weekday,
        } => format!(
            "FREQ=MONTHLY{};BYDAY={nth}{}",
            interval(*every),
            jiff::civil::Weekday::from_monday_one_offset(*weekday as i8)
                .map(weekday_code)
                .unwrap_or("MO")
        ),
        Repeat::MonthlyDay { every, day } => {
            format!("FREQ=MONTHLY{};BYMONTHDAY={day}", interval(*every))
        }
        Repeat::Yearly { month, day } => format!("FREQ=YEARLY;BYMONTH={month};BYMONTHDAY={day}"),
    };
    match rule.end {
        End::Never => {}
        End::Until { .. } => {}
        End::Count { .. } => {
            if let Some((n, _)) = counted(rule, from) {
                r.push_str(&format!(";COUNT={n}"));
            }
        }
    }
    Some(r)
}

fn interval(every: u8) -> String {
    if every > 1 {
        format!(";INTERVAL={every}")
    } else {
        String::new()
    }
}

fn event(out: &mut Out, cal: &ExportCalendar, show: &ExportShow, first: Date, stamp: &str) {
    let rule = &show.rule;
    let from = show
        .anchor
        .unwrap_or_else(|| cal.now.to_zoned(cal.zone.clone()).date());
    let z = &cal.zone_name;
    let tzid =
        |name: &str, dt: DateTime| format!("{name};TZID={}:{}", tzid_param(z), text::local(dt));
    let uid = uid(show.id);
    let start = at(rule, first);
    let common = |out: &mut Out| {
        out.text("SUMMARY", &show.name);
        if !show.description.trim().is_empty() {
            out.text("DESCRIPTION", show.description.trim());
        }
        if let Some(u) = &show.url {
            out.prop("URL", u);
        }
    };
    out.prop("BEGIN", "VEVENT");
    out.text("UID", &uid);
    out.prop("DTSTAMP", stamp);
    out.line(&tzid("DTSTART", start));
    out.line(&tzid("DTEND", end_of(&cal.zone, start, rule.length())));
    common(out);
    if let Some(mut r) = rrule(rule, from) {
        if let End::Until { until } = rule.end {
            // UNTIL is inclusive, and in UTC when DTSTART has a zone (section 3.3.10): the
            // instant of the last date's start.
            let last = at(rule, until);
            if let Ok(t) = cal.zone.to_ambiguous_zoned(last).compatible() {
                r.push_str(&format!(";UNTIL={}", text::utc(t.timestamp())));
            }
        }
        out.prop("RRULE", &r);
    }
    if let Repeat::Dates { dates } = &rule.repeat {
        let mut rest: Vec<Date> = dates.iter().copied().filter(|d| *d != first).collect();
        rest.sort();
        rest.dedup();
        if !rest.is_empty() {
            let list: Vec<String> = rest.iter().map(|d| text::local(at(rule, *d))).collect();
            out.line(&format!("RDATE;TZID={}:{}", tzid_param(z), list.join(",")));
        }
    }
    let mut skip = rule.skip.clone();
    skip.sort();
    skip.dedup();
    if !skip.is_empty() {
        let list: Vec<String> = skip.iter().map(|d| text::local(at(rule, *d))).collect();
        out.line(&format!("EXDATE;TZID={}:{}", tzid_param(z), list.join(",")));
    }
    out.prop("END", "VEVENT");
    let mut moves = rule.moves.clone();
    moves.sort_by_key(|m| m.from);
    for m in moves.iter().filter(|m| !rule.skip.contains(&m.from)) {
        out.prop("BEGIN", "VEVENT");
        out.text("UID", &uid);
        out.prop("DTSTAMP", stamp);
        out.line(&tzid("RECURRENCE-ID", at(rule, m.from)));
        out.line(&tzid("DTSTART", m.to));
        out.line(&tzid(
            "DTEND",
            end_of(&cal.zone, m.to, m.minutes.unwrap_or_else(|| rule.length())),
        ));
        common(out);
        out.prop("END", "VEVENT");
    }
}

/// A TZID parameter's value, quoted when it holds a character a parameter cannot.
fn tzid_param(name: &str) -> String {
    if name.contains([':', ';', ',']) {
        format!("\"{}\"", name.replace('"', ""))
    } else {
        name.to_owned()
    }
}
