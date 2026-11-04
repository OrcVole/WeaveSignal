// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The words a card prints, built from its fields: dates, slots, lists of people, initials.
//! The alternative text uses the same words with the plain hyphen and no bidi marks.

use jiff::Zoned;
use jiff::civil::{Date, Time};

use crate::Weekday;

/// U+2011, the non-breaking hyphen: a hyphenated surname never breaks at its hyphen on a card
/// (the design's fix round); the alt text keeps the plain one.
pub(crate) const NB_HYPHEN: char = '\u{2011}';
/// First strong isolate and pop directional isolate: a right-to-left name in an English list
/// keeps the commas and "and" in order (the design's `bdi`).
pub(crate) const FSI: char = '\u{2068}';
pub(crate) const PDI: char = '\u{2069}';

/// A name as printed: hyphens made non-breaking, and isolated when it holds right-to-left text.
pub(crate) fn visible_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c == '-' { NB_HYPHEN } else { c })
        .collect();
    if has_rtl(&name) {
        format!("{FSI}{name}{PDI}")
    } else {
        name
    }
}

/// Whether a string holds a right-to-left letter (Hebrew, Arabic, Syriac, Thaana, N'Ko and
/// their presentation forms).
pub(crate) fn has_rtl(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c as u32,
            0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
    })
}

/// Titles that do not give an initial: "Dr Una Robertson" is UR.
const HONORIFICS: &[&str] = &[
    "dr", "dr.", "prof", "prof.", "rev", "rev.", "sir", "dame", "mr", "mr.", "mrs", "mrs.", "ms",
    "ms.", "mx", "mx.", "lord", "lady",
];

/// The initials on a placeholder disc: the first letter of the first and last words, after any
/// title; one word gives one letter ("Rùnach" is R).
pub(crate) fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| !HONORIFICS.contains(&w.to_lowercase().as_str()))
        .collect();
    let first = |w: &str| {
        w.chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default()
    };
    match words.as_slice() {
        [] => String::new(),
        [one] => first(one),
        [a, .., z] => format!("{}{}", first(a), first(z)),
    }
}

/// "A", "A and B", "A, B and C".
pub(crate) fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

pub(crate) fn weekday_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

pub(crate) fn hhmm(t: Time) -> String {
    format!("{:02}:{:02}", t.hour(), t.minute())
}

/// "8 pm", "8.30 am", "noon", "midnight": the Slate show card's prose.
pub(crate) fn clock_words(t: Time) -> String {
    let (h, m) = (t.hour(), t.minute());
    match (h, m) {
        (0, 0) => "midnight".to_owned(),
        (12, 0) => "noon".to_owned(),
        _ => {
            let (h12, half) = match h {
                0 => (12, "am"),
                1..=11 => (h, "am"),
                12 => (12, "pm"),
                _ => (h - 12, "pm"),
            };
            if m == 0 {
                format!("{h12} {half}")
            } else {
                format!("{h12}.{m:02} {half}")
            }
        }
    }
}

/// "Saturday 24 October 2026".
pub(crate) fn long_date(d: Date) -> String {
    d.strftime("%A %-d %B %Y").to_string()
}

/// "24 October 2026", or "24 October" when the year is understood.
fn day_month(d: Date, year: bool) -> String {
    if year {
        d.strftime("%-d %B %Y").to_string()
    } else {
        d.strftime("%-d %B").to_string()
    }
}

/// "5 September to 7 November 2026"; "28 December 2026 to 3 January 2027".
pub(crate) fn date_run(first: Date, last: Date) -> String {
    if first == last {
        return day_month(first, true);
    }
    let same_year = first.year() == last.year();
    format!(
        "{} to {}",
        day_month(first, !same_year),
        day_month(last, true)
    )
}

/// "20:00 BST": the time with the zone's abbreviation where the zone has one, else its offset.
pub(crate) fn time_zone(at: &Zoned) -> String {
    let abbrev = at.strftime("%Z").to_string();
    format!("{} {abbrev}", hhmm(at.time()))
}

/// A date run as printed: no-break spaces inside each date, so a line breaks only at "to".
pub(crate) fn unbroken_dates(run: &str) -> String {
    run.split(" to ")
        .map(|d| d.replace(' ', "\u{a0}"))
        .collect::<Vec<_>>()
        .join(" to ")
}

/// Ellipsis for a title cut at two lines.
pub(crate) const ELLIPSIS: char = '\u{2026}';

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_skip_titles_and_take_first_and_last_words() {
        assert_eq!(initials("Shona Campbell"), "SC");
        assert_eq!(initials("Catriona MacKenzie-Buchanan"), "CM");
        assert_eq!(initials("Dr Una Robertson"), "UR");
        assert_eq!(initials("Rùnach"), "R");
        assert_eq!(initials("نور الهدى حسن"), "نح");
        assert_eq!(initials("  "), "");
    }

    #[test]
    fn lists_read_as_english() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(list(&n(&["A"])), "A");
        assert_eq!(list(&n(&["A", "B"])), "A and B");
        assert_eq!(list(&n(&["A", "B", "C"])), "A, B and C");
    }

    #[test]
    fn visible_names_keep_hyphens_whole_and_isolate_rtl() {
        assert_eq!(
            visible_name("Catriona MacKenzie-Buchanan"),
            "Catriona MacKenzie\u{2011}Buchanan"
        );
        assert_eq!(
            visible_name("نور الهدى حسن"),
            "\u{2068}نور الهدى حسن\u{2069}"
        );
        assert!(!has_rtl("Rùnach"));
    }

    #[test]
    fn dates_and_times_read_as_the_design_prints_them() {
        let d = |y, m, day| Date::new(y, m, day).unwrap();
        assert_eq!(long_date(d(2026, 10, 24)), "Saturday 24 October 2026");
        assert_eq!(
            date_run(d(2026, 9, 5), d(2026, 11, 7)),
            "5 September to 7 November 2026"
        );
        assert_eq!(
            date_run(d(2026, 12, 28), d(2027, 1, 3)),
            "28 December 2026 to 3 January 2027"
        );
        let at: Zoned = "2026-10-24T20:00[Europe/London]".parse().unwrap();
        assert_eq!(time_zone(&at), "20:00 BST");
        let at: Zoned = "2026-11-07T20:00[Europe/London]".parse().unwrap();
        assert_eq!(time_zone(&at), "20:00 GMT");
        assert_eq!(clock_words(Time::new(20, 0, 0, 0).unwrap()), "8 pm");
        assert_eq!(clock_words(Time::new(8, 30, 0, 0).unwrap()), "8.30 am");
        assert_eq!(clock_words(Time::new(0, 0, 0, 0).unwrap()), "midnight");
    }
}
