// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Calendar files (release 1.0.5, ADR-0281): shows' rules written as iCalendar (RFC 5545) for
//! a person's own calendar, and a calendar file read into proposed shows.
//!
//! A show's [`Rule`](crate::rule::Rule) is already a subset of RFC 5545's recurrence (design
//! `new-show.md` section 9), so both directions are a translation:
//!
//! | The rule | iCalendar |
//! |---|---|
//! | `starts_on`, `start` | DTSTART (with TZID, the station's zone): the first date the repeat gives |
//! | `minutes` | DTEND, that many minutes of elapsed time later |
//! | `weekly` | `FREQ=WEEKLY;INTERVAL=n;BYDAY=TU,FR` |
//! | `monthly_nth` | `FREQ=MONTHLY;BYDAY=1TU` or `-1FR` |
//! | `monthly_day` | `FREQ=MONTHLY;BYMONTHDAY=15` or `-1` |
//! | `yearly` | `FREQ=YEARLY;BYMONTH=12;BYMONTHDAY=31` |
//! | `dates` | RDATE |
//! | `skip` | EXDATE |
//! | `moves` | an override VEVENT with RECURRENCE-ID |
//! | `until` | `UNTIL`, the instant of the last date's start, in UTC |
//! | `count` | `COUNT`, with the skipped dates inside the run added: RFC 5545 counts dates before EXDATE takes them away, a show counts episodes |
//!
//! The content lines are read by the `icalendar` crate's parser; dates, zones and rules are
//! read here, with jiff.

mod export;
mod import;
pub mod read;
pub mod rrule;
mod text;
mod vtimezone;

pub use export::{ExportCalendar, ExportShow, uid, write};
pub use import::{Mapping, Proposal, Proposed, fold_name, propose, same_dates_to_come};
pub use read::{IcsError, MAX_BYTES, MAX_EVENTS};

#[cfg(test)]
mod tests;
