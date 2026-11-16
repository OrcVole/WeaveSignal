// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! An RRULE read and expanded, one date a day (RFC 5545 section 3.3.10 and 3.8.5.3).
//!
//! A show airs at most once a day, so the expander works in days: every part that picks days
//! (`BYMONTH`, `BYWEEKNO` aside, `BYYEARDAY`, `BYMONTHDAY`, `BYDAY` with or without an ordinal,
//! `BYSETPOS`) is honoured; a rule that gives more than one time a day (`BYHOUR` or `BYMINUTE`
//! with several values, `FREQ=HOURLY` and finer) is refused before it is expanded. The import
//! uses it to read what a calendar's rule really gives, and judges a translation by comparing
//! those dates with the ones [`crate::rule::Rule::occurrences`] gives.

use jiff::civil::{Date, DateTime, Weekday};
use jiff::{Timestamp, ToSpan};

/// How often.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// `UNTIL`, in the form it was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    Date(Date),
    /// A local date and time, in the event's own zone (or floating).
    Local(DateTime),
    Utc(Timestamp),
}

/// An RRULE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    /// `BYDAY`: an optional ordinal and a weekday.
    pub by_day: Vec<(Option<i32>, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<i8>,
    pub by_year_day: Vec<i32>,
    pub by_week_no: Vec<i32>,
    pub by_set_pos: Vec<i32>,
    pub by_hour: Vec<i8>,
    pub by_minute: Vec<i8>,
    pub by_second: Vec<i8>,
    pub wkst: Weekday,
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// `MO`…`SU`.
pub(crate) fn weekday_code(w: Weekday) -> &'static str {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][w.to_monday_zero_offset() as usize]
}

fn ints(name: &str, v: &str, lo: i32, hi: i32, zero: bool) -> Result<Vec<i32>, String> {
    v.split(',')
        .map(|x| {
            let n: i32 = x
                .trim()
                .parse()
                .map_err(|_| format!("its RRULE's {name} is not a number list"))?;
            let bad = n.abs() > hi || (n == 0 && !zero) || (n != 0 && n.abs() < lo);
            if bad {
                return Err(format!("its RRULE's {name} is out of range"));
            }
            Ok(n)
        })
        .collect()
}

/// Read `BYMINUTE=0` and the like: small non-negative numbers.
fn small(name: &str, v: &str, hi: i32) -> Result<Vec<i8>, String> {
    let all = ints(name, v, 0, hi, true)?;
    if all.iter().any(|n| *n < 0) {
        return Err(format!("its RRULE's {name} is out of range"));
    }
    Ok(all.into_iter().map(|n| n as i8).collect())
}

/// Read a date or date-time value as `UNTIL` gives it.
pub(crate) fn until_value(v: &str) -> Result<Until, String> {
    let v = v.trim();
    if let Some(t) = v.strip_suffix('Z') {
        let dt = datetime(t).ok_or("its RRULE's UNTIL is not a time")?;
        let ts = dt
            .to_zoned(jiff::tz::TimeZone::UTC)
            .map_err(|_| "its RRULE's UNTIL is not a time")?
            .timestamp();
        return Ok(Until::Utc(ts));
    }
    if v.len() == 8 {
        return date(v)
            .map(Until::Date)
            .ok_or_else(|| "its RRULE's UNTIL is not a date".into());
    }
    datetime(v)
        .map(Until::Local)
        .ok_or_else(|| "its RRULE's UNTIL is not a time".into())
}

/// `YYYYMMDD`.
pub(crate) fn date(v: &str) -> Option<Date> {
    if v.len() != 8 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i16 = v[0..4].parse().ok()?;
    let m: i8 = v[4..6].parse().ok()?;
    let d: i8 = v[6..8].parse().ok()?;
    Date::new(y, m, d).ok()
}

/// `YYYYMMDDTHHMMSS` (a leap second is read as 59).
pub(crate) fn datetime(v: &str) -> Option<DateTime> {
    let (d, t) = v.split_once(['T', 't'])?;
    let d = date(d)?;
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i8 = t[0..2].parse().ok()?;
    let m: i8 = t[2..4].parse().ok()?;
    let s: i8 = t[4..6].parse().ok()?;
    if s > 60 {
        return None;
    }
    DateTime::new(d.year(), d.month(), d.day(), h, m, s.min(59), 0).ok()
}

impl RRule {
    /// Read an RRULE's value (`FREQ=WEEKLY;BYDAY=TU`). A refusal is a sentence's tail: *its
    /// RRULE has no FREQ*.
    pub fn parse(value: &str) -> Result<RRule, String> {
        let mut r = RRule {
            freq: Freq::Daily,
            interval: 1,
            count: None,
            until: None,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
            by_year_day: Vec::new(),
            by_week_no: Vec::new(),
            by_set_pos: Vec::new(),
            by_hour: Vec::new(),
            by_minute: Vec::new(),
            by_second: Vec::new(),
            wkst: Weekday::Monday,
        };
        let mut freq = None;
        for part in value.trim().split(';').filter(|p| !p.trim().is_empty()) {
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| format!("its RRULE has a part without a value ({part})"))?;
            let k = k.trim().to_ascii_uppercase();
            let v = v.trim();
            match k.as_str() {
                "FREQ" => {
                    freq = Some(match v.to_ascii_uppercase().as_str() {
                        "SECONDLY" => Freq::Secondly,
                        "MINUTELY" => Freq::Minutely,
                        "HOURLY" => Freq::Hourly,
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        _ => return Err(format!("its RRULE's FREQ ({v}) is not one we know")),
                    })
                }
                "INTERVAL" => {
                    r.interval = v
                        .parse()
                        .ok()
                        .filter(|n| (1..=10_000).contains(n))
                        .ok_or("its RRULE's INTERVAL is not a whole number from 1")?;
                }
                "COUNT" => {
                    r.count = Some(
                        v.parse()
                            .ok()
                            .filter(|n| (1..=100_000).contains(n))
                            .ok_or("its RRULE's COUNT is not a whole number from 1")?,
                    );
                }
                "UNTIL" => r.until = Some(until_value(v)?),
                "BYDAY" => {
                    for d in v.split(',') {
                        let d = d.trim().to_ascii_uppercase();
                        if d.len() < 2 {
                            return Err("its RRULE's BYDAY is not a list of weekdays".into());
                        }
                        let (n, w) = d.split_at(d.len() - 2);
                        let w = weekday(w).ok_or("its RRULE's BYDAY is not a list of weekdays")?;
                        let n = if n.is_empty() {
                            None
                        } else {
                            let n: i32 = n.trim_start_matches('+').parse().map_err(
                                |_| "its RRULE's BYDAY has an ordinal that is not a number",
                            )?;
                            if n == 0 || n.abs() > 53 {
                                return Err("its RRULE's BYDAY has an ordinal out of range".into());
                            }
                            Some(n)
                        };
                        r.by_day.push((n, w));
                    }
                }
                "BYMONTHDAY" => r.by_month_day = ints("BYMONTHDAY", v, 1, 31, false)?,
                "BYMONTH" => {
                    r.by_month = ints("BYMONTH", v, 1, 12, false)?
                        .into_iter()
                        .map(|m| {
                            if m < 0 {
                                Err("its RRULE's BYMONTH is out of range".to_owned())
                            } else {
                                Ok(m as i8)
                            }
                        })
                        .collect::<Result<_, _>>()?
                }
                "BYYEARDAY" => r.by_year_day = ints("BYYEARDAY", v, 1, 366, false)?,
                "BYWEEKNO" => r.by_week_no = ints("BYWEEKNO", v, 1, 53, false)?,
                "BYSETPOS" => r.by_set_pos = ints("BYSETPOS", v, 1, 366, false)?,
                "BYHOUR" => r.by_hour = small("BYHOUR", v, 23)?,
                "BYMINUTE" => r.by_minute = small("BYMINUTE", v, 59)?,
                "BYSECOND" => r.by_second = small("BYSECOND", v, 60)?,
                "WKST" => {
                    r.wkst = weekday(&v.to_ascii_uppercase())
                        .ok_or("its RRULE's WKST is not a weekday")?
                }
                // An extension part (RSCALE, SKIP and the like) changes the dates; refuse
                // rather than guess.
                other => return Err(format!("its RRULE has a part we do not read ({other})")),
            }
        }
        r.freq = freq.ok_or("its RRULE has no FREQ")?;
        if r.count.is_some() && r.until.is_some() {
            return Err("its RRULE has both COUNT and UNTIL".into());
        }
        Ok(r)
    }

    /// Why the rule cannot be read one date a day, if it cannot: more than one start a day.
    pub fn more_than_daily(&self) -> Option<&'static str> {
        if matches!(self.freq, Freq::Secondly | Freq::Minutely | Freq::Hourly) {
            return Some("It repeats within a day: a show airs at most once a day.");
        }
        if self.by_hour.len() > 1 || self.by_minute.len() > 1 || self.by_second.len() > 1 {
            return Some("It gives more than one time a day: a show airs at most once a day.");
        }
        None
    }

    /// The days the rule gives from `start` (DTSTART's date), in order, before `COUNT`,
    /// `UNTIL` and EXDATE: DTSTART's date first, as RFC 5545 counts it, whether or not the rule
    /// would give it. Ends when a period is out of range, or after 1,000 empty periods in a row.
    /// `Err` for what is not read (`BYWEEKNO`).
    pub fn days(&self, start: Date) -> Result<impl Iterator<Item = Date> + '_, String> {
        if !self.by_week_no.is_empty() {
            return Err(
                "its RRULE picks weeks of the year (BYWEEKNO), which we do not read".into(),
            );
        }
        let mut k: i64 = 0;
        let mut buffered: std::collections::VecDeque<Date> = std::collections::VecDeque::new();
        let mut empty = 0u32;
        let mut done = false;
        let mut first = Some(start);
        let interval = i64::from(self.interval);
        Ok(std::iter::from_fn(move || {
            if let Some(f) = first.take() {
                // DTSTART is the first instance; the expansion below skips it.
                return Some(f);
            }
            loop {
                if let Some(d) = buffered.pop_front() {
                    return Some(d);
                }
                if done {
                    return None;
                }
                match self.period(start, k * interval) {
                    None => done = true,
                    Some(v) => {
                        let v: Vec<Date> = v.into_iter().filter(|d| *d > start).collect();
                        if v.is_empty() {
                            empty += 1;
                            if empty >= 1000 {
                                done = true;
                            }
                        } else {
                            empty = 0;
                            buffered.extend(v);
                        }
                    }
                }
                k += 1;
            }
        }))
    }

    /// The days of the period `steps` periods after DTSTART's, sorted, with `BYSETPOS` applied.
    fn period(&self, start: Date, steps: i64) -> Option<Vec<Date>> {
        let mut days: Vec<Date> = match self.freq {
            Freq::Daily => {
                let d = start.checked_add(steps.days()).ok()?;
                vec![d]
            }
            Freq::Weekly => {
                let back = (7 + start.weekday().to_monday_zero_offset()
                    - self.wkst.to_monday_zero_offset())
                    % 7;
                let week = start
                    .checked_sub(i64::from(back).days())
                    .ok()?
                    .checked_add((steps * 7).days())
                    .ok()?;
                let all: Vec<Date> = (0..7)
                    .filter_map(|i| week.checked_add(i.days()).ok())
                    .collect();
                if self.by_day.is_empty() {
                    all.into_iter()
                        .filter(|d| d.weekday() == start.weekday())
                        .collect()
                } else {
                    all
                }
            }
            Freq::Monthly => {
                let first = month_after(start, steps)?;
                let all = month_days(first);
                if self.by_day.is_empty()
                    && self.by_month_day.is_empty()
                    && self.by_year_day.is_empty()
                {
                    all.into_iter().filter(|d| d.day() == start.day()).collect()
                } else {
                    all
                }
            }
            Freq::Yearly => {
                let year = i64::from(start.year()) + steps;
                if !(1..=9999).contains(&year) {
                    return None;
                }
                let year = year as i16;
                let months: Vec<i8> = if self.by_month.is_empty() {
                    (1..=12).collect()
                } else {
                    self.by_month.clone()
                };
                let no_days = self.by_day.is_empty()
                    && self.by_month_day.is_empty()
                    && self.by_year_day.is_empty();
                if no_days {
                    // The day of DTSTART, in DTSTART's month or in each BYMONTH.
                    let months = if self.by_month.is_empty() {
                        vec![start.month()]
                    } else {
                        months
                    };
                    months
                        .into_iter()
                        .filter_map(|m| Date::new(year, m, start.day()).ok())
                        .collect()
                } else if self.by_month.is_empty() {
                    let jan = Date::new(year, 1, 1).ok()?;
                    (0..jan.days_in_year())
                        .filter_map(|i| jan.checked_add(i64::from(i).days()).ok())
                        .collect()
                } else {
                    months
                        .into_iter()
                        .filter_map(|m| Date::new(year, m, 1).ok())
                        .flat_map(month_days)
                        .collect()
                }
            }
            Freq::Hourly | Freq::Minutely | Freq::Secondly => return None,
        };
        if days.is_empty() && matches!(self.freq, Freq::Daily) {
            return Some(days);
        }
        let monthly_scope = matches!(self.freq, Freq::Monthly)
            || (matches!(self.freq, Freq::Yearly) && !self.by_month.is_empty());
        days.retain(|d| self.keeps(*d, monthly_scope));
        days.sort();
        days.dedup();
        if !self.by_set_pos.is_empty() {
            let n = days.len() as i32;
            let mut picked: Vec<Date> = self
                .by_set_pos
                .iter()
                .filter_map(|p| {
                    let i = if *p > 0 { p - 1 } else { n + p };
                    (0..n).contains(&i).then(|| days[i as usize])
                })
                .collect();
            picked.sort();
            picked.dedup();
            days = picked;
        }
        Some(days)
    }

    /// Whether a day of the period passes every BYxxx part that picks days.
    fn keeps(&self, d: Date, monthly_scope: bool) -> bool {
        if !self.by_month.is_empty() && !self.by_month.contains(&d.month()) {
            return false;
        }
        if !self.by_year_day.is_empty() {
            let doy = i32::from(d.day_of_year());
            let len = i32::from(d.days_in_year());
            if !self
                .by_year_day
                .iter()
                .any(|n| *n == doy || *n == doy - len - 1)
            {
                return false;
            }
        }
        if !self.by_month_day.is_empty() {
            let day = i32::from(d.day());
            let len = i32::from(d.days_in_month());
            if !self
                .by_month_day
                .iter()
                .any(|n| *n == day || *n == day - len - 1)
            {
                return false;
            }
        }
        if !self.by_day.is_empty() {
            let (pos, neg) = if monthly_scope {
                let day = i32::from(d.day());
                let len = i32::from(d.days_in_month());
                ((day - 1) / 7 + 1, -((len - day) / 7 + 1))
            } else {
                let doy = i32::from(d.day_of_year());
                let len = i32::from(d.days_in_year());
                ((doy - 1) / 7 + 1, -((len - doy) / 7 + 1))
            };
            let ordinals_count = matches!(self.freq, Freq::Monthly | Freq::Yearly);
            if !self.by_day.iter().any(|(n, w)| {
                *w == d.weekday()
                    && match n {
                        None => true,
                        Some(n) if ordinals_count => *n == pos || *n == neg,
                        // An ordinal means nothing in a daily or weekly rule; RFC 5545 says
                        // it is not given there, and we read it as the bare weekday.
                        Some(_) => true,
                    }
            }) {
                return false;
            }
        }
        true
    }
}

/// The first of the month `months` after `d`'s.
fn month_after(d: Date, months: i64) -> Option<Date> {
    let index = i64::from(d.year()) * 12 + i64::from(d.month() - 1) + months;
    let (y, m) = (index.div_euclid(12), index.rem_euclid(12) + 1);
    if !(1..=9999).contains(&y) {
        return None;
    }
    Date::new(y as i16, m as i8, 1).ok()
}

fn month_days(first: Date) -> Vec<Date> {
    (0..first.days_in_month())
        .filter_map(|i| first.checked_add(i64::from(i).days()).ok())
        .collect()
}
