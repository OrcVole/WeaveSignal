// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station's zone as a VTIMEZONE (RFC 5545 section 3.6.5), from jiff's copy of the time
//! zone database.
//!
//! The changes of offset from the year before the first date the file needs are read from jiff
//! up to a horizon. The run of changes at the end that follows one yearly pattern per kind (the
//! last Sunday of March at 01:00, the second Sunday of March at 02:00, the first Sunday on or
//! after 2 April…) is written as two
//! observances with a yearly RRULE, so a calendar knows the clocks for good; every change before
//! that run is written as an observance of its own. The pattern is accepted only if it gives
//! every change jiff knows up to the horizon, and a zone whose changes follow no such pattern is
//! written change by change up to the horizon. Each observance's DTSTART is the local time of
//! the change on the clock before it (TZOFFSETFROM), as section 3.6.5 says.

use jiff::civil::{Date, DateTime, Weekday};
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, ToSpan};

use super::rrule::weekday_code;
use super::text::{Out, local, offset};

/// One change of offset.
#[derive(Clone, Debug)]
struct Change {
    at: Timestamp,
    from: Offset,
    to: Offset,
    dst: bool,
    name: String,
}

impl Change {
    /// The wall clock at the change, on the clock before it.
    fn local(&self) -> DateTime {
        self.at.to_zoned(TimeZone::fixed(self.from)).datetime()
    }
}

/// A yearly rule for one kind of change: a `weekday` of `month`, the `nth` (1 to 4, or -1 for
/// the last), or the first on or after a day (Chile's *first Sunday on or after the 2nd*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Yearly {
    month: i8,
    day: Day,
    weekday: Weekday,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    Nth(i8),
    OnOrAfter(i8),
}

impl Yearly {
    fn gives(&self, year: i16) -> Option<Date> {
        let first = Date::new(year, self.month, 1).ok()?;
        match self.day {
            Day::Nth(n) => first.nth_weekday_of_month(n, self.weekday).ok(),
            Day::OnOrAfter(d) => {
                let from = first.with().day(d).build().ok()?;
                let back = from.yesterday().ok()?;
                back.nth_weekday(1, self.weekday).ok()
            }
        }
    }

    fn rrule(&self) -> String {
        let wd = weekday_code(self.weekday);
        match self.day {
            Day::Nth(n) => format!("FREQ=YEARLY;BYMONTH={};BYDAY={n}{wd}", self.month),
            Day::OnOrAfter(d) => {
                let days: Vec<String> = (d..d + 7).map(|x| x.to_string()).collect();
                format!(
                    "FREQ=YEARLY;BYMONTH={};BYDAY={wd};BYMONTHDAY={}",
                    self.month,
                    days.join(",")
                )
            }
        }
    }
}

/// The ways a change's date can be said, simplest first: its nth weekday from the start of the
/// month, the last when it is in the last seven days, and the first on or after each of the
/// seven days before it (kept within the shortest month, so the week never runs over).
fn forms(d: Date) -> Vec<Yearly> {
    let (month, weekday) = (d.month(), d.weekday());
    let mut out = Vec::new();
    if d.day() <= 28 {
        out.push(Yearly {
            month,
            day: Day::Nth((d.day() - 1) / 7 + 1),
            weekday,
        });
    }
    if d.day() + 7 > d.days_in_month() {
        out.push(Yearly {
            month,
            day: Day::Nth(-1),
            weekday,
        });
    }
    // The month's length in a common year: a February week stays within the 28th.
    let shortest = Date::new(2023, month, 1).map_or(28, |m| m.days_in_month());
    for from in (d.day() - 6).max(2)..=d.day().min(shortest - 6) {
        if (from - 1) % 7 != 0 {
            out.push(Yearly {
                month,
                day: Day::OnOrAfter(from),
                weekday,
            });
        }
    }
    out
}

/// Whether `c` is what `y` gives in its year, with the same clock, offsets and name as `like`.
fn fits(c: &Change, like: &Change, y: &Yearly) -> bool {
    let l = c.local();
    c.dst == like.dst
        && c.from == like.from
        && c.to == like.to
        && c.name == like.name
        && l.time() == like.local().time()
        && y.gives(l.year()) == Some(l.date())
}

/// The changes of offset in `[from, to)`. A change of name or of the daylight flag alone is not
/// a change a calendar needs.
fn changes(tz: &TimeZone, from: Timestamp, to: Timestamp) -> Vec<Change> {
    let mut out = Vec::new();
    for t in tz.following(from) {
        let at = t.timestamp();
        if at >= to {
            break;
        }
        let before = tz.to_offset(at - 1.second());
        if before == t.offset() {
            continue;
        }
        out.push(Change {
            at,
            from: before,
            to: t.offset(),
            dst: t.dst().is_dst(),
            name: t.abbreviation().to_owned(),
        });
    }
    out
}

/// The run of changes at the end that two yearly rules give, one per kind, and the rules: the
/// index of the run's first change. `None` when there is no such run of at least three years.
fn tail(all: &[Change]) -> Option<(usize, [(usize, Yearly); 2])> {
    let n = all.len();
    if n < 6 {
        return None;
    }
    // The last two changes are one of each kind, or there is no yearly pair.
    let (a, b) = (&all[n - 2], &all[n - 1]);
    if a.dst == b.dst {
        return None;
    }
    let mut best: Option<(usize, [(usize, Yearly); 2])> = None;
    for ya in forms(a.local().date()) {
        for yb in forms(b.local().date()) {
            // Walk back while each change fits the rule of its kind and the kinds alternate.
            let mut i = n;
            while i > 0 {
                let c = &all[i - 1];
                let (like, y) = if (n - i).is_multiple_of(2) {
                    (b, &yb)
                } else {
                    (a, &ya)
                };
                if !fits(c, like, y) {
                    break;
                }
                // Each rule gives one change a year: the years must follow on.
                if i + 1 < n {
                    let next_same = &all[i + 1];
                    if next_same.local().year() != c.local().year() + 1 {
                        break;
                    }
                }
                i -= 1;
            }
            if n - i >= 6 && best.as_ref().is_none_or(|(s, _)| i < *s) {
                // The rules' first changes: the first of each kind in the run.
                let first_a = if (n - i).is_multiple_of(2) { i } else { i + 1 };
                let first_b = if (n - i).is_multiple_of(2) { i + 1 } else { i };
                best = Some((i, [(first_a, ya), (first_b, yb)]));
            }
        }
    }
    best
}

/// Write the zone as a VTIMEZONE named `tzid`, good for dates from `first` and, change by
/// change, at least up to the start of `horizon_year`.
pub(crate) fn write(out: &mut Out, tz: &TimeZone, tzid: &str, first: Date, horizon_year: i16) {
    let begin_year = first.year().saturating_sub(1).max(1);
    let begin = Date::new(begin_year, 1, 1)
        .ok()
        .and_then(|d| d.to_zoned(TimeZone::UTC).ok())
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    let end_year = horizon_year.max(first.year()).saturating_add(1).min(9999);
    let end = Date::new(end_year, 1, 1)
        .ok()
        .and_then(|d| d.to_zoned(TimeZone::UTC).ok())
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::MAX);
    let all = changes(tz, begin, end);
    out.prop("BEGIN", "VTIMEZONE");
    out.text("TZID", tzid);
    // The clock in force at the start of the range, so a date before the first change listed
    // has an offset too.
    let info = tz.to_offset_info(begin);
    observance(
        out,
        info.dst().is_dst(),
        begin.to_zoned(TimeZone::fixed(info.offset())).datetime(),
        info.offset(),
        info.offset(),
        info.abbreviation(),
        None,
    );
    let (explicit, rules) = match tail(&all) {
        Some((start, rules)) => (&all[..start], Some(rules)),
        None => (&all[..], None),
    };
    for c in explicit {
        observance(out, c.dst, c.local(), c.from, c.to, &c.name, None);
    }
    if let Some(rules) = rules {
        for (i, y) in rules {
            let c = &all[i];
            observance(
                out,
                c.dst,
                c.local(),
                c.from,
                c.to,
                &c.name,
                Some(&y.rrule()),
            );
        }
    }
    out.prop("END", "VTIMEZONE");
}

fn observance(
    out: &mut Out,
    dst: bool,
    start: DateTime,
    from: Offset,
    to: Offset,
    name: &str,
    rrule: Option<&str>,
) {
    let kind = if dst { "DAYLIGHT" } else { "STANDARD" };
    out.prop("BEGIN", kind);
    out.prop("DTSTART", &local(start));
    if let Some(r) = rrule {
        out.prop("RRULE", r);
    }
    out.prop("TZOFFSETFROM", &offset(from));
    out.prop("TZOFFSETTO", &offset(to));
    if !name.is_empty() {
        out.text("TZNAME", name);
    }
    out.prop("END", kind);
}
