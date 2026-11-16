// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

use jiff::civil::{Date, DateTime, date};
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, ToSpan};
use uuid::Uuid;

use super::rrule::RRule;
use super::*;
use crate::rule::{End, Move, Repeat, Rule};

/// The tests' "now": Sunday 15 November 2026, noon in London.
fn now() -> Timestamp {
    "2026-11-15T12:00:00Z".parse().unwrap()
}

fn tz(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap()
}

fn rule(starts_on: Date, start: &str, minutes: u16, repeat: Repeat) -> Rule {
    let (h, m) = start.split_once(':').unwrap();
    Rule {
        starts_on: Some(starts_on),
        start: h.parse::<u16>().unwrap() * 60 + m.parse::<u16>().unwrap(),
        minutes: Some(minutes),
        repeat,
        end: End::Never,
        skip: vec![],
        moves: vec![],
    }
}

fn show(name: &str, rule: Rule) -> ExportShow {
    ExportShow {
        id: Uuid::from_u128(0x5eed_0000_0000_0000_0000_0000_0000_0001),
        name: name.into(),
        description: "Tunes, chat; and the Forth, after dark.\nEvery week.".into(),
        url: Some("https://kettle.example/the-late-kettle".into()),
        rule,
        anchor: None,
    }
}

fn export(zone: &str, shows: Vec<ExportShow>) -> String {
    write(&ExportCalendar {
        name: "Kettle FM".into(),
        zone_name: zone.into(),
        zone: tz(zone),
        shows,
        now: now(),
        version: "1.0.5".into(),
    })
}

fn import(text: &str, zone: &str) -> Proposed {
    propose(text.as_bytes(), &tz(zone), zone, now()).unwrap()
}

/// What a rule gives, as instants and lengths, for three years.
fn instants(r: &Rule, zone: &str) -> Vec<(Timestamp, u16)> {
    let end: Timestamp = "2029-11-15T00:00:00Z".parse().unwrap();
    r.occurrences(&tz(zone), r.starts_on.unwrap())
        .take_while(|o| o.start < end)
        .map(|o| (o.start, o.minutes))
        .collect()
}

/// The rule as an export and an import give it back: DTSTART is the first date the repeat
/// gives, lists sorted.
fn normal(mut r: Rule) -> Rule {
    let first = r.repeat_dates(r.starts_on.unwrap()).next().unwrap();
    r.starts_on = Some(first);
    r.skip.sort();
    r.moves.sort_by_key(|m| m.from);
    if let Repeat::Dates { dates } = &mut r.repeat {
        dates.sort();
    }
    r
}

/// Export one show and read it back: one proposal, exact, the same rule, the same instants.
fn round_trip(r: Rule, zone: &str) -> Proposal {
    assert_eq!(r.problems(), vec![], "the test's own rule stands");
    let text = export(zone, vec![show("The Late Kettle", r.clone())]);
    let got = import(&text, zone);
    assert_eq!(got.proposals.len(), 1, "{text}\n{got:#?}");
    let p = got.proposals[0].clone();
    assert_eq!(p.mapping, Mapping::Exact, "{text}\n{p:#?}");
    assert_eq!(p.name, "The Late Kettle");
    assert_eq!(
        p.description,
        "Tunes, chat; and the Forth, after dark.\nEvery week."
    );
    let back = p.rule.clone().unwrap();
    assert_eq!(back, normal(r.clone()), "{text}");
    assert_eq!(instants(&back, zone), instants(&r, zone));
    p
}

// ---- writing ----

#[test]
fn lines_fold_at_75_octets_without_splitting_a_character_and_text_is_escaped() {
    let mut out = text::Out::default();
    let name = "Ceòl agus Còmhradh — ".repeat(8);
    out.text("SUMMARY", &format!("{name};a,b\\c\nd"));
    let s = out.finish();
    assert!(s.ends_with("\r\n"));
    for line in s.split("\r\n").filter(|l| !l.is_empty()) {
        assert!(line.len() <= 75, "{} octets: {line}", line.len());
    }
    let unfolded = s.replace("\r\n ", "");
    assert_eq!(
        unfolded,
        format!("SUMMARY:{name}\\;a\\,b\\\\c\\nd\r\n"),
        "folding never splits a character"
    );
}

#[test]
fn a_calendar_has_its_header_one_event_per_rule_and_public_words_only() {
    let mut r = rule(
        date(2026, 12, 4),
        "22:00",
        120,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![5],
        },
    );
    r.skip = vec![date(2026, 12, 25)];
    r.moves = vec![Move {
        from: date(2027, 1, 1),
        to: "2027-01-02T21:00:00".parse().unwrap(),
        minutes: Some(90),
    }];
    let s = export("Europe/London", vec![show("The Late Kettle", r)]);
    for want in [
        "BEGIN:VCALENDAR\r\n",
        "VERSION:2.0\r\n",
        "PRODID:-//WeaveSignal//WeaveSignal 1.0.5//EN\r\n",
        "METHOD:PUBLISH\r\n",
        "X-WR-TIMEZONE:Europe/London\r\n",
        "BEGIN:VTIMEZONE\r\nTZID:Europe/London\r\n",
        "UID:5eed0000-0000-0000-0000-000000000001@weavesignal\r\n",
        "DTSTAMP:20261115T120000Z\r\n",
        "DTSTART;TZID=Europe/London:20261204T220000\r\n",
        "DTEND;TZID=Europe/London:20261205T000000\r\n",
        "RRULE:FREQ=WEEKLY;BYDAY=FR\r\n",
        "EXDATE;TZID=Europe/London:20261225T220000\r\n",
        "RECURRENCE-ID;TZID=Europe/London:20270101T220000\r\n",
        "DTSTART;TZID=Europe/London:20270102T210000\r\n",
        "DTEND;TZID=Europe/London:20270102T223000\r\n",
        "SUMMARY:The Late Kettle\r\n",
        "DESCRIPTION:Tunes\\, chat\\; and the Forth\\, after dark.\\nEvery week.\r\n",
        "URL:https://kettle.example/the-late-kettle\r\n",
        "END:VCALENDAR\r\n",
    ] {
        assert!(s.contains(want), "missing {want:?} in\n{s}");
    }
    assert_eq!(
        s.matches("BEGIN:VEVENT").count(),
        2,
        "the rule and its move"
    );
    // Every line is CRLF-terminated and at most 75 octets.
    assert!(s.split("\r\n").all(|l| l.len() <= 75 && !l.contains('\n')));
}

#[test]
fn a_calendar_without_rules_has_no_events_and_no_zone() {
    let s = export("Europe/London", vec![]);
    assert!(s.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(!s.contains("VEVENT") && !s.contains("VTIMEZONE"));
}

/// The onsets a VTIMEZONE gives: each change's instant and the offset after it.
fn vtimezone_offsets(text: &str) -> Vec<(Timestamp, Offset)> {
    let unfolded = icalendar::parser::unfold(text);
    let cal = icalendar::parser::read_calendar(&unfolded).unwrap();
    let vtz = cal
        .components
        .iter()
        .find(|c| c.name.as_str() == "VTIMEZONE")
        .expect("a VTIMEZONE");
    let off = |s: &str| -> Offset {
        let sign = if s.starts_with('-') { -1 } else { 1 };
        let h: i32 = s[1..3].parse().unwrap();
        let m: i32 = s[3..5].parse().unwrap();
        Offset::from_seconds(sign * (h * 3600 + m * 60)).unwrap()
    };
    let mut onsets = Vec::new();
    for o in &vtz.components {
        let get = |n: &str| o.find_prop(n).map(|p| p.val.as_str().to_owned());
        let start = rrule::datetime(&get("DTSTART").unwrap()).unwrap();
        let from = off(&get("TZOFFSETFROM").unwrap());
        let to = off(&get("TZOFFSETTO").unwrap());
        let at = |d: Date| {
            d.to_datetime(start.time())
                .to_zoned(TimeZone::fixed(from))
                .unwrap()
                .timestamp()
        };
        match get("RRULE") {
            None => onsets.push((at(start.date()), to)),
            Some(r) => {
                let r = RRule::parse(&r).unwrap();
                for d in r
                    .days(start.date())
                    .unwrap()
                    .take_while(|d| d.year() < 2080)
                {
                    onsets.push((at(d), to));
                }
            }
        }
    }
    onsets.sort_by_key(|(t, _)| *t);
    onsets
}

/// The offset a VTIMEZONE gives at `t`: the TZOFFSETTO of the latest onset at or before it.
fn offset_at(onsets: &[(Timestamp, Offset)], t: Timestamp) -> Offset {
    onsets
        .iter()
        .rev()
        .find(|(at, _)| *at <= t)
        .map(|(_, o)| *o)
        .expect("an onset before")
}

#[test]
fn the_zone_written_gives_the_offsets_jiff_knows_for_decades() {
    for (zone, expect_rrule) in [
        ("Europe/London", true),
        ("America/New_York", true),
        ("Australia/Sydney", true),
        ("Pacific/Auckland", true),
        ("Asia/Kolkata", false),
        ("Europe/Dublin", true),
        ("America/Santiago", true),
        ("Asia/Jerusalem", true),
        ("America/Sao_Paulo", false),
        ("UTC", false),
    ] {
        let r = rule(
            date(2026, 12, 1),
            "20:00",
            60,
            Repeat::Weekly {
                every: 1,
                weekdays: vec![2],
            },
        );
        let s = export(zone, vec![show("Test", r)]);
        let block = &s[s.find("BEGIN:VTIMEZONE").unwrap()..s.find("END:VTIMEZONE").unwrap() + 15];
        if expect_rrule {
            assert!(block.contains("RRULE:FREQ=YEARLY"), "{zone}\n{block}");
            assert!(
                block.matches("BEGIN:").count() < 12,
                "{zone} is short\n{block}"
            );
        }
        let onsets = vtimezone_offsets(&s);
        let z = tz(zone);
        // Every change jiff knows from 2026 to 2070, an hour either side, and each month's
        // first.
        let mut probes = Vec::new();
        let start: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
        let end: Timestamp = "2070-01-01T00:00:00Z".parse().unwrap();
        for t in z.following(start).take_while(|t| t.timestamp() < end) {
            probes.push(t.timestamp() - 1.hour());
            probes.push(t.timestamp());
            probes.push(t.timestamp() + 1.hour());
        }
        let mut d = date(2026, 1, 1);
        while d.year() < 2070 {
            probes.push(d.to_zoned(TimeZone::UTC).unwrap().timestamp());
            d = d.checked_add(1.month()).unwrap();
        }
        for t in probes {
            assert_eq!(
                offset_at(&onsets, t),
                z.to_offset(t),
                "{zone} at {t}\n{block}"
            );
        }
    }
}

#[test]
fn london_is_written_as_two_yearly_rules() {
    let r = rule(
        date(2026, 12, 1),
        "20:00",
        60,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![2],
        },
    );
    let s = export("Europe/London", vec![show("Test", r)]);
    assert!(s.contains("BEGIN:DAYLIGHT\r\nDTSTART:20250330T010000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nTZOFFSETFROM:+0000\r\nTZOFFSETTO:+0100\r\nTZNAME:BST\r\nEND:DAYLIGHT\r\n"), "{s}");
    assert!(s.contains("BEGIN:STANDARD\r\nDTSTART:20251026T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0000\r\nTZNAME:GMT\r\nEND:STANDARD\r\n"), "{s}");
}

// ---- round trips ----

#[test]
fn weekly_rules_come_back_the_same() {
    let mut r = rule(
        date(2026, 11, 20),
        "20:00",
        60,
        Repeat::Weekly {
            every: 2,
            weekdays: vec![2, 5],
        },
    );
    r.skip = vec![date(2026, 12, 18)];
    r.moves = vec![Move {
        from: date(2027, 1, 1),
        to: "2027-01-01T21:30:00".parse().unwrap(),
        minutes: None,
    }];
    round_trip(r.clone(), "Europe/London");
    round_trip(r, "America/New_York");
    // Every day, until a date.
    let mut r = rule(
        date(2026, 11, 16),
        "07:00",
        180,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![1, 2, 3, 4, 5, 6, 7],
        },
    );
    r.end = End::Until {
        until: date(2027, 6, 30),
    };
    round_trip(r, "Europe/London");
}

#[test]
fn monthly_last_friday_and_the_third_tuesday_with_a_count_come_back_the_same() {
    let r = rule(
        date(2026, 11, 1),
        "19:00",
        90,
        Repeat::MonthlyNth {
            every: 1,
            nth: -1,
            weekday: 5,
        },
    );
    round_trip(r, "Europe/London");
    let mut r = rule(
        date(2026, 12, 1),
        "20:00",
        60,
        Repeat::MonthlyNth {
            every: 2,
            nth: 3,
            weekday: 2,
        },
    );
    r.end = End::Count { count: 6 };
    r.skip = vec![date(2027, 2, 16)];
    let text = export("Europe/London", vec![show("The Late Kettle", r.clone())]);
    // Six episodes and one skipped date: RFC 5545 counts seven dates, then takes one away.
    assert!(
        text.contains("RRULE:FREQ=MONTHLY;INTERVAL=2;BYDAY=3TU;COUNT=7\r\n"),
        "{text}"
    );
    round_trip(r, "Europe/London");
}

#[test]
fn monthly_on_a_day_yearly_and_leap_days_come_back_the_same() {
    round_trip(
        rule(
            date(2026, 12, 1),
            "18:00",
            60,
            Repeat::MonthlyDay { every: 1, day: 31 },
        ),
        "Europe/London",
    );
    round_trip(
        rule(
            date(2026, 12, 1),
            "18:00",
            60,
            Repeat::MonthlyDay { every: 3, day: -1 },
        ),
        "Europe/London",
    );
    round_trip(
        rule(
            date(2026, 12, 31),
            "23:00",
            120,
            Repeat::Yearly { month: 12, day: 31 },
        ),
        "Europe/London",
    );
    round_trip(
        rule(
            date(2026, 11, 20),
            "12:00",
            60,
            Repeat::Yearly { month: 2, day: 29 },
        ),
        "America/New_York",
    );
}

#[test]
fn once_and_chosen_dates_with_a_move_come_back_the_same() {
    round_trip(
        rule(date(2027, 3, 6), "14:00", 45, Repeat::Once),
        "Europe/London",
    );
    let mut r = rule(
        date(2026, 12, 3),
        "20:00",
        60,
        Repeat::Dates {
            dates: vec![date(2027, 1, 14), date(2026, 12, 3), date(2027, 2, 11)],
        },
    );
    r.moves = vec![Move {
        from: date(2027, 1, 14),
        to: "2027-01-14T21:00:00".parse().unwrap(),
        minutes: Some(30),
    }];
    round_trip(r, "Europe/London");
}

#[test]
fn starts_in_the_hour_the_clocks_skip_or_repeat_come_back_the_same() {
    // 01:30 on Sundays in London: on 28 March 2027 it does not exist (airs at 02:30 BST); on
    // 31 October 2027 it happens twice (airs the first time).
    round_trip(
        rule(
            date(2026, 11, 15),
            "01:30",
            60,
            Repeat::Weekly {
                every: 1,
                weekdays: vec![7],
            },
        ),
        "Europe/London",
    );
    // 02:30 on the second Sunday of the month in New York: 14 March 2027 skips it.
    round_trip(
        rule(
            date(2026, 11, 1),
            "02:30",
            60,
            Repeat::MonthlyNth {
                every: 1,
                nth: 2,
                weekday: 7,
            },
        ),
        "America/New_York",
    );
}

#[test]
fn a_whole_station_comes_back_as_one_proposal_per_show() {
    let a = rule(
        date(2026, 11, 20),
        "20:00",
        60,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![5],
        },
    );
    let b = rule(
        date(2026, 12, 1),
        "09:00",
        120,
        Repeat::MonthlyDay { every: 1, day: 1 },
    );
    let mut sb = show("Morning Brew", b);
    sb.id = Uuid::from_u128(2);
    let text = export("Europe/London", vec![show("Friday Night", a), sb]);
    let got = import(&text, "Europe/London");
    assert_eq!(got.events, 2);
    let names: Vec<_> = got.proposals.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Friday Night", "Morning Brew"]);
    assert!(got.proposals.iter().all(|p| p.mapping == Mapping::Exact));
}

// ---- reading other calendars ----

fn cal(body: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n{}END:VCALENDAR\r\n",
        body.replace('\n', "\r\n")
    )
}

fn one(text: &str, zone: &str) -> Proposal {
    let got = import(text, zone);
    assert_eq!(got.proposals.len(), 1, "{got:#?}");
    got.proposals[0].clone()
}

#[test]
fn a_count_with_an_exdate_counts_dates_then_takes_the_skipped_one_away() {
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Ten Tuesdays\nDTSTART;TZID=Europe/London:20261201T200000\nDTEND;TZID=Europe/London:20261201T210000\nRRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=10\nEXDATE;TZID=Europe/London:20261222T200000\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    let r = p.rule.unwrap();
    assert_eq!(r.end, End::Count { count: 9 });
    assert_eq!(r.skip, vec![date(2026, 12, 22)]);
    // Nine episodes, the last on 2 February.
    let last = r
        .occurrences(&tz("Europe/London"), date(2026, 12, 1))
        .last()
        .unwrap();
    assert_eq!(last.on, date(2027, 2, 2));
}

#[test]
fn every_day_is_a_rule_and_every_other_day_is_chosen_dates() {
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Breakfast\nDTSTART;TZID=Europe/London:20261116T070000\nDURATION:PT3H\nRRULE:FREQ=DAILY\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(
        p.rule.unwrap().repeat,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![1, 2, 3, 4, 5, 6, 7]
        }
    );
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:b\nSUMMARY:Alternate\nDTSTART;TZID=Europe/London:20261116T070000\nDURATION:PT1H\nRRULE:FREQ=DAILY;INTERVAL=2\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Dates, "{p:#?}");
    assert!(
        p.reason.as_deref().unwrap().contains("every 2 days"),
        "{p:#?}"
    );
    let r = p.rule.clone().unwrap();
    let Repeat::Dates { dates } = &r.repeat else {
        panic!("{r:?}")
    };
    // Every other day for twelve months is 183 dates: the first 120 are kept, and it says so.
    assert_eq!(dates.len(), 120);
    assert_eq!(dates[0], date(2026, 11, 16));
    assert_eq!(dates[1], date(2026, 11, 18));
    assert!(p.notes.iter().any(|n| n.contains("at most 120")), "{p:#?}");
    assert!(r.problems().is_empty());
}

#[test]
fn monthly_by_setpos_and_by_weekday_ordinal_say_the_same_last_friday() {
    for rr in [
        "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1",
        "FREQ=MONTHLY;BYDAY=-1FR",
    ] {
        let p = one(
            &cal(&format!(
                "BEGIN:VEVENT\nUID:a\nSUMMARY:Last Friday\nDTSTART;TZID=Europe/London:20261127T190000\nDTEND;TZID=Europe/London:20261127T203000\nRRULE:{rr}\nEND:VEVENT\n"
            )),
            "Europe/London",
        );
        assert_eq!(p.mapping, Mapping::Exact, "{rr}: {p:#?}");
        assert_eq!(
            p.rule.unwrap().repeat,
            Repeat::MonthlyNth {
                every: 1,
                nth: -1,
                weekday: 5
            }
        );
    }
    // The fifth Friday is not in the subset: chosen dates.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Fifth Friday\nDTSTART;TZID=Europe/London:20270129T190000\nRRULE:FREQ=MONTHLY;BYDAY=5FR\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Dates, "{p:#?}");
    let Repeat::Dates { dates } = p.rule.unwrap().repeat else {
        panic!()
    };
    assert_eq!(
        dates,
        vec![
            date(2027, 1, 29),
            date(2027, 4, 30),
            date(2027, 7, 30),
            date(2027, 10, 29),
            date(2027, 12, 31)
        ]
    );
}

#[test]
fn until_in_utc_ends_on_the_local_date_of_the_last_start() {
    // 20:00 in New York on Mondays is 01:00 UTC on Tuesdays: an UNTIL of Tuesday 01:00 UTC
    // takes Monday's in New York.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Monday Night\nDTSTART;TZID=America/New_York:20261116T200000\nDTEND;TZID=America/New_York:20261116T210000\nRRULE:FREQ=WEEKLY;BYDAY=MO;UNTIL=20261208T010000Z\nEND:VEVENT\n",
        ),
        "America/New_York",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(
        p.rule.unwrap().end,
        End::Until {
            until: date(2026, 12, 7)
        }
    );
}

#[test]
fn a_moved_and_a_cancelled_date_are_a_move_and_a_skip() {
    let text = cal(
        "BEGIN:VEVENT\nUID:late\nSUMMARY:Late Kettle\nDTSTART;TZID=Europe/London:20261120T220000\nDTEND;TZID=Europe/London:20261120T235900\nRRULE:FREQ=WEEKLY;BYDAY=FR\nEND:VEVENT\nBEGIN:VEVENT\nUID:late\nRECURRENCE-ID;TZID=Europe/London:20261204T220000\nSUMMARY:Late Kettle\nDTSTART;TZID=Europe/London:20261205T210000\nDTEND;TZID=Europe/London:20261205T220000\nEND:VEVENT\nBEGIN:VEVENT\nUID:late\nRECURRENCE-ID:20261218T220000Z\nSTATUS:CANCELLED\nSUMMARY:Late Kettle\nDTSTART;TZID=Europe/London:20261218T220000\nEND:VEVENT\n",
    );
    let p = one(&text, "Europe/London");
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.events, 3);
    let r = p.rule.unwrap();
    assert_eq!(r.skip, vec![date(2026, 12, 18)]);
    assert_eq!(
        r.moves,
        vec![Move {
            from: date(2026, 12, 4),
            to: "2026-12-05T21:00:00".parse().unwrap(),
            minutes: Some(60)
        }]
    );
    assert_eq!(r.minutes, Some(119));
}

#[test]
fn an_event_from_a_zone_with_the_same_clock_changes_is_exact_and_converted() {
    // 21:00 in Paris is 20:00 in London all year: the two change their clocks together.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Radio Paris\nDTSTART;TZID=Europe/Paris:20261118T210000\nDTEND;TZID=Europe/Paris:20261118T220000\nRRULE:FREQ=WEEKLY;BYDAY=WE\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.zone.as_deref(), Some("Europe/Paris"));
    assert!(!p.clock_differs);
    let r = p.rule.unwrap();
    assert_eq!(r.start, 20 * 60);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("converted from Europe/Paris"))
    );
}

#[test]
fn an_event_from_new_york_is_chosen_dates_on_the_station_clock_and_flagged() {
    // 20:00 on Fridays in New York is 01:00 on Saturdays in London, but midnight in the weeks
    // when one has changed its clocks and the other has not.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Coast to Coast\nDTSTART;TZID=America/New_York:20261120T200000\nDTEND;TZID=America/New_York:20261120T210000\nRRULE:FREQ=WEEKLY;BYDAY=FR\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Dates, "{p:#?}");
    assert!(p.clock_differs);
    assert_eq!(p.zone.as_deref(), Some("America/New_York"));
    assert!(
        p.reason
            .as_deref()
            .unwrap()
            .contains("clocks change on other dates")
    );
    let r = p.rule.unwrap();
    assert_eq!(r.start, 60, "most weeks it is 01:00 here");
    // Fridays 19 and 26 March 2027 (New York on summer time, London not yet) and 5 November
    // 2027 (London back on winter time, New York not yet) are at midnight here.
    let moved: Vec<(Date, DateTime)> = r.moves.iter().map(|m| (m.from, m.to)).collect();
    assert_eq!(
        moved,
        vec![
            (date(2027, 3, 20), "2027-03-20T00:00:00".parse().unwrap()),
            (date(2027, 3, 27), "2027-03-27T00:00:00".parse().unwrap()),
            (date(2027, 11, 6), "2027-11-06T00:00:00".parse().unwrap()),
        ]
    );
    // And every date is the instant New York gives.
    let ny = tz("America/New_York");
    for o in r.occurrences(&tz("Europe/London"), date(2026, 11, 1)) {
        let there = o.start.to_zoned(ny.clone());
        assert_eq!((there.hour(), there.minute()), (20, 0), "{}", o.on);
        assert_eq!(there.weekday(), jiff::civil::Weekday::Friday);
    }
}

#[test]
fn single_events_with_one_name_are_one_show_on_chosen_dates() {
    let ev = |d: &str, t: &str| {
        format!(
            "BEGIN:VEVENT\nUID:{d}{t}\nSUMMARY:Ceilidh Night \nDTSTART;TZID=Europe/London:{d}T{t}\nDURATION:PT2H\nEND:VEVENT\n"
        )
    };
    let mut body = String::new();
    for (d, t) in [
        ("20261205", "190000"),
        ("20270109", "190000"),
        ("20270206", "200000"),
        ("20260905", "190000"),
    ] {
        body.push_str(&ev(d, t));
    }
    body.push_str("BEGIN:VEVENT\nUID:x\nSUMMARY:Burns Supper\nDTSTART;TZID=Europe/London:20270125T193000\nDTEND;TZID=Europe/London:20270125T223000\nEND:VEVENT\n");
    let got = import(&cal(&body), "Europe/London");
    assert_eq!(got.proposals.len(), 2, "{got:#?}");
    let ceilidh = &got.proposals[0];
    assert_eq!(ceilidh.name, "Ceilidh Night");
    assert_eq!(ceilidh.mapping, Mapping::Exact, "{ceilidh:#?}");
    assert_eq!(ceilidh.events, 4);
    let r = ceilidh.rule.clone().unwrap();
    assert_eq!(
        r.repeat,
        Repeat::Dates {
            dates: vec![date(2026, 12, 5), date(2027, 1, 9), date(2027, 2, 6)]
        }
    );
    assert_eq!(r.start, 19 * 60);
    assert_eq!(
        r.moves,
        vec![Move {
            from: date(2027, 2, 6),
            to: "2027-02-06T20:00:00".parse().unwrap(),
            minutes: None
        }]
    );
    assert!(
        ceilidh
            .notes
            .iter()
            .any(|n| n.contains("1 date before today"))
    );
    let burns = &got.proposals[1];
    assert_eq!(burns.mapping, Mapping::Exact);
    let r = burns.rule.clone().unwrap();
    assert_eq!(r.repeat, Repeat::Once);
    assert_eq!((r.start, r.minutes), (19 * 60 + 30, Some(180)));
}

#[test]
fn outlook_and_google_files_are_read() {
    // Outlook: a Windows zone name with its own VTIMEZONE, folded lines, a quoted parameter.
    let outlook = "BEGIN:VCALENDAR\r\nPRODID:-//Microsoft Corporation//Outlook 16.0 MIMEDIR//EN\r\nVERSION:2.0\r\nMETHOD:PUBLISH\r\nBEGIN:VTIMEZONE\r\nTZID:GMT Standard Time\r\nBEGIN:STANDARD\r\nDTSTART:16011028T020000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:-0000\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010325T010000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\nTZOFFSETFROM:-0000\r\nTZOFFSETTO:+0100\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nCLASS:PUBLIC\r\nDESCRIPTION:The week's papers\\, read aloud.\\n\r\nRRULE:FREQ=WEEKLY;UNTIL=20270630T090000Z;INTERVAL=1;BYDAY=SU;WKST=SU\r\nUID:040000008200E00074C5B7101A82E00800000000\r\n SAMPLE\r\nSUMMARY;LANGUAGE=en-gb:Papers Review\r\nDTSTART;TZID=\"GMT Standard Time\":20261122T090000\r\nDTEND;TZID=\"GMT Standard Time\":20261122T100000\r\nDTSTAMP:20261101T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let p = one(outlook, "Europe/London");
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.name, "Papers Review");
    assert_eq!(p.description, "The week's papers, read aloud.");
    assert_eq!(p.zone, None, "GMT Standard Time is the station's own zone");
    assert_eq!(
        p.uid.as_deref(),
        Some("040000008200E00074C5B7101A82E00800000000SAMPLE")
    );
    let r = p.rule.unwrap();
    // UNTIL is 10:00 on Wednesday 30 June here; the last Sunday it takes is the 27th.
    assert_eq!(
        r.end,
        End::Until {
            until: date(2027, 6, 30)
        }
    );
    // Google: lower-case property names are allowed too, and a blank line or two.
    let google = "BEGIN:VCALENDAR\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\nVERSION:2.0\nX-WR-TIMEZONE:Europe/London\n\nBEGIN:VEVENT\nDTSTART;TZID=Europe/London:20261201T200000\nDTEND;TZID=Europe/London:20261201T210000\nrrule:FREQ=MONTHLY;BYDAY=1TU\nUID:abc@google.com\nSUMMARY:First Tuesday Folk\nEND:VEVENT\nEND:VCALENDAR\n";
    let p = one(google, "Europe/London");
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(
        p.rule.unwrap().repeat,
        Repeat::MonthlyNth {
            every: 1,
            nth: 1,
            weekday: 2
        }
    );
}

#[test]
fn what_cannot_be_a_show_is_refused_with_a_reason() {
    let cases = [
        (
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Hourly News\nDTSTART;TZID=Europe/London:20261201T070000\nRRULE:FREQ=HOURLY\nEND:VEVENT\n",
            "within a day",
        ),
        (
            "BEGIN:VEVENT\nUID:b\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261225\nDTEND;VALUE=DATE:20261226\nRRULE:FREQ=YEARLY\nEND:VEVENT\n",
            "all-day",
        ),
        (
            "BEGIN:VEVENT\nUID:c\nSUMMARY:Elsewhere\nDTSTART;TZID=Mars/Olympus_Mons:20261201T070000\nRRULE:FREQ=WEEKLY\nEND:VEVENT\n",
            "Mars/Olympus_Mons",
        ),
        (
            "BEGIN:VEVENT\nUID:d\nSUMMARY:Marathon\nDTSTART;TZID=Europe/London:20261201T070000\nDURATION:P2D\nRRULE:FREQ=WEEKLY\nEND:VEVENT\n",
            "more than a day",
        ),
        (
            "BEGIN:VEVENT\nUID:e\nSUMMARY:Gone\nDTSTART;TZID=Europe/London:20201201T070000\nRRULE:FREQ=WEEKLY;COUNT=3\nEND:VEVENT\n",
            "all passed",
        ),
        (
            "BEGIN:VEVENT\nUID:f\nSUMMARY:Twice a day\nDTSTART;TZID=Europe/London:20261201T070000\nRRULE:FREQ=DAILY;BYHOUR=7,19\nEND:VEVENT\n",
            "more than one time a day",
        ),
    ];
    for (body, why) in cases {
        let p = one(&cal(body), "Europe/London");
        assert_eq!(p.mapping, Mapping::Refused, "{body}: {p:#?}");
        assert!(p.rule.is_none());
        assert!(
            p.reason.as_deref().unwrap().contains(why),
            "{why}: {:?}",
            p.reason
        );
    }
}

#[test]
fn a_file_that_is_not_a_calendar_is_refused_with_a_sentence_and_never_panics() {
    let tz = tz("Europe/London");
    let bad: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"hello".to_vec(),
        b"BEGIN:VCALENDAR\r\n".to_vec(),
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:2026".to_vec(),
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:x\r\nEND:VTODO\r\nEND:VCALENDAR\r\n".to_vec(),
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:no start\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            .to_vec(),
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261301T250000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            .to_vec(),
        vec![0xff, 0xfe, 0x00, 0x42],
        b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n".to_vec(),
    ];
    for b in bad {
        let e = propose(&b, &tz, "Europe/London", now()).unwrap_err();
        let s = e.to_string();
        assert!(s.ends_with('.') && s.len() > 15, "{s:?}");
    }
    // Every prefix of a good file is either read or refused, never a panic.
    let good = export(
        "Europe/London",
        vec![show(
            "The Late Kettle",
            rule(
                date(2026, 12, 4),
                "22:00",
                120,
                Repeat::Weekly {
                    every: 1,
                    weekdays: vec![5],
                },
            ),
        )],
    );
    for n in (0..good.len()).step_by(7) {
        let _ = propose(&good.as_bytes()[..n], &tz, "Europe/London", now());
    }
    // An RRULE it cannot read: refused for that event, with the reason.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Odd\nDTSTART;TZID=Europe/London:20261201T070000\nRRULE:FREQ=FORTNIGHTLY\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Refused);
    assert!(p.reason.unwrap().contains("FORTNIGHTLY"));
}

#[test]
fn the_limits_are_one_megabyte_and_200_events() {
    let tz = tz("Europe/London");
    let big = vec![b'x'; MAX_BYTES + 1];
    assert_eq!(
        propose(&big, &tz, "Europe/London", now()).unwrap_err(),
        IcsError::TooLarge
    );
    let mut body = String::new();
    for i in 0..201 {
        body.push_str(&format!(
            "BEGIN:VEVENT\nUID:{i}\nSUMMARY:S{i}\nDTSTART:20270101T120000Z\nEND:VEVENT\n"
        ));
    }
    assert_eq!(
        propose(cal(&body).as_bytes(), &tz, "Europe/London", now()).unwrap_err(),
        IcsError::TooManyEvents(201)
    );
}

#[test]
fn floating_times_are_the_station_clock_and_a_byte_order_mark_is_fine() {
    let text = format!(
        "\u{feff}{}",
        cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Local\nDTSTART:20261201T200000\nDTEND:20261201T210000\nRRULE:FREQ=WEEKLY\nEND:VEVENT\n"
        )
    );
    let p = one(&text, "America/New_York");
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.zone, None);
    let r = p.rule.unwrap();
    assert_eq!(r.start, 20 * 60);
    assert_eq!(
        r.repeat,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![2]
        }
    );
}

#[test]
fn the_expander_reads_the_rfc_examples() {
    let days = |rr: &str, start: Date, n: usize| -> Vec<Date> {
        let r = RRule::parse(rr).unwrap();
        let v: Vec<Date> = r.days(start).unwrap().take(n).collect();
        v
    };
    // RFC 5545 3.8.5.3: every other week on Monday, Wednesday and Friday, starting Tuesday
    // 2 September 1997 (DTSTART is the first instance).
    assert_eq!(
        days(
            "FREQ=WEEKLY;INTERVAL=2;WKST=SU;BYDAY=MO,WE,FR",
            date(1997, 9, 2),
            7
        ),
        vec![
            date(1997, 9, 2),
            date(1997, 9, 3),
            date(1997, 9, 5),
            date(1997, 9, 15),
            date(1997, 9, 17),
            date(1997, 9, 19),
            date(1997, 9, 29)
        ]
    );
    // The last weekday of the month.
    assert_eq!(
        days(
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            date(1997, 9, 30),
            3
        ),
        vec![date(1997, 9, 30), date(1997, 10, 31), date(1997, 11, 28)]
    );
    // Every Thursday in March.
    assert_eq!(
        days("FREQ=YEARLY;BYMONTH=3;BYDAY=TH", date(1997, 3, 13), 4),
        vec![
            date(1997, 3, 13),
            date(1997, 3, 20),
            date(1997, 3, 27),
            date(1998, 3, 5)
        ]
    );
    // The 20th Monday of the year.
    assert_eq!(
        days("FREQ=YEARLY;BYDAY=20MO", date(1997, 5, 19), 3),
        vec![date(1997, 5, 19), date(1998, 5, 18), date(1999, 5, 17)]
    );
    // Monthly on the 31st passes over shorter months.
    assert_eq!(
        days("FREQ=MONTHLY;BYMONTHDAY=31", date(2027, 1, 31), 3),
        vec![date(2027, 1, 31), date(2027, 3, 31), date(2027, 5, 31)]
    );
    assert!(RRule::parse("FREQ=WEEKLY;COUNT=3;UNTIL=20270101").is_err());
    assert!(RRule::parse("INTERVAL=2").is_err());
}

#[test]
fn a_show_running_since_long_ago_is_still_exact() {
    // Every day since 2010, and every weekday since 2005: thousands of dates before today,
    // counted but not compared.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Breakfast\nDTSTART;TZID=Europe/London:20100104T060000\nDURATION:PT3H\nRRULE:FREQ=DAILY\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.rule.unwrap().starts_on, Some(date(2010, 1, 4)));
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:b\nSUMMARY:Drive Time\nDTSTART;TZID=Europe/London:20050103T160000\nDURATION:PT2H\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\nEXDATE;TZID=Europe/London:20051226T160000\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    // A count reached long ago has passed, however many dates it took.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:c\nSUMMARY:Old Series\nDTSTART;TZID=Europe/London:20050103T160000\nRRULE:FREQ=DAILY;COUNT=3000\nEND:VEVENT\n",
        ),
        "Europe/London",
    );
    assert_eq!(p.mapping, Mapping::Refused, "{p:#?}");
    assert!(p.reason.unwrap().contains("all passed"));
}

#[test]
fn a_file_nested_far_too_deep_is_refused_before_it_is_parsed() {
    let mut text = String::from("BEGIN:VCALENDAR\r\n");
    for _ in 0..50_000 {
        text.push_str("BEGIN:X\r\n");
    }
    let e = propose(
        text.as_bytes(),
        &tz("Europe/London"),
        "Europe/London",
        now(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("nested"), "{e}");
}

#[test]
fn an_event_that_falls_on_the_day_before_here_keeps_its_weekly_rule() {
    // 02:00 on Mondays in Tokyo is 22:30 on Sundays in Kolkata; neither changes its clocks.
    let p = one(
        &cal(
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Night Owls\nDTSTART;TZID=Asia/Tokyo:20261130T020000\nDTEND;TZID=Asia/Tokyo:20261130T030000\nRRULE:FREQ=WEEKLY;BYDAY=MO,TH;UNTIL=20270301T000000Z\nEXDATE;TZID=Asia/Tokyo:20261228T020000\nEND:VEVENT\n",
        ),
        "Asia/Kolkata",
    );
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    assert_eq!(p.zone.as_deref(), Some("Asia/Tokyo"));
    let r = p.rule.unwrap();
    assert_eq!(r.start, 22 * 60 + 30);
    assert_eq!(r.starts_on, Some(date(2026, 11, 29)));
    assert_eq!(
        r.repeat,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![3, 7]
        }
    );
    assert_eq!(r.skip, vec![date(2026, 12, 27)]);
    assert_eq!(
        r.end,
        End::Until {
            until: date(2027, 2, 28)
        }
    );
}

#[test]
fn a_weekly_slot_with_no_start_date_begins_at_its_anchor() {
    let r = Rule {
        starts_on: None,
        start: 20 * 60,
        minutes: None,
        repeat: Repeat::Weekly {
            every: 1,
            weekdays: vec![2],
        },
        end: End::Never,
        skip: vec![],
        moves: vec![],
    };
    let mut s = show("Old Slot", r.clone());
    s.anchor = Some(date(2026, 11, 2));
    let text = export("Europe/London", vec![s]);
    assert!(
        text.contains("DTSTART;TZID=Europe/London:20261103T200000\r\n"),
        "{text}"
    );
    assert!(text.contains("DTEND;TZID=Europe/London:20261103T210000\r\n"));
    let p = one(&text, "Europe/London");
    assert_eq!(p.mapping, Mapping::Exact, "{p:#?}");
    let back = p.rule.unwrap();
    assert_eq!(back.starts_on, Some(date(2026, 11, 3)));
    assert_eq!(back.minutes, Some(60));
    let z = tz("Europe/London");
    let next = |r: &Rule| -> Vec<Timestamp> {
        r.occurrences(&z, date(2026, 11, 15))
            .take(60)
            .map(|o| o.start)
            .collect()
    };
    assert_eq!(next(&back), next(&r));
    // With no anchor, it begins at today's week.
    let text = export("Europe/London", vec![show("Old Slot", r)]);
    assert!(
        text.contains("DTSTART;TZID=Europe/London:20261117T200000\r\n"),
        "{text}"
    );
}

// ---- a show the station already has ----

/// Five shows as a station has them, each written a different way: a start long past, an old
/// weekly slot with no start date, a start on a day the repeat does not give with a skip and a
/// move, one date, and a count part of the way through.
fn kettle_fm() -> Vec<ExportShow> {
    let weekdays = || vec![1, 2, 3, 4, 5];
    let breakfast = rule(
        date(2026, 9, 7),
        "07:00",
        180,
        Repeat::Weekly {
            every: 1,
            weekdays: weekdays(),
        },
    );
    let drive = Rule {
        starts_on: None,
        start: 16 * 60,
        minutes: None,
        repeat: Repeat::Weekly {
            every: 1,
            weekdays: weekdays(),
        },
        end: End::Never,
        skip: vec![],
        moves: vec![],
    };
    let mut bothy = rule(
        date(2026, 11, 10),
        "21:00",
        120,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![3, 5],
        },
    );
    bothy.end = End::Until {
        until: date(2027, 2, 26),
    };
    bothy.skip = vec![date(2026, 12, 9)];
    bothy.moves = vec![Move {
        from: date(2026, 12, 11),
        to: DateTime::constant(2026, 12, 12, 20, 0, 0, 0),
        minutes: None,
    }];
    let budget = rule(date(2026, 11, 19), "18:00", 90, Repeat::Once);
    let mut saturday = rule(
        date(2026, 10, 31),
        "20:00",
        180,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![6],
        },
    );
    saturday.end = End::Count { count: 10 };
    [
        ("Kettle Breakfast", breakfast, None),
        ("Kettle Drive", drive, Some(date(2026, 10, 5))),
        ("The Bothy Sessions", bothy, None),
        ("Budget Night Special", budget, None),
        ("Saturday Night Kettle", saturday, None),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, rule, anchor))| {
        assert_eq!(
            rule.problems(),
            vec![],
            "{name}: the test's own rule stands"
        );
        let mut s = show(name, rule);
        s.id = Uuid::from_u128(0x5eed_0000_0000_0000_0000_0000_0000_0010 + i as u128);
        s.anchor = anchor;
        s
    })
    .collect()
}

#[test]
fn a_stations_own_calendar_read_back_is_the_shows_it_already_has() {
    // The test station's case (2026-11-15): its own schedule.ics imported into itself proposed
    // all five shows, each a clash with itself.
    let zone = "Europe/London";
    let z = tz(zone);
    let shows = kettle_fm();
    let text = export(zone, shows.clone());
    let got = import(&text, zone);
    assert_eq!(got.proposals.len(), 5, "{text}\n{got:#?}");
    for p in &got.proposals {
        assert_ne!(p.mapping, Mapping::Refused, "{p:#?}");
        let own: Vec<&str> = shows
            .iter()
            .filter(|s| p.is_show(&s.name, &s.rule, &z, now()))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(own, [p.name.as_str()], "{p:#?}");
    }
    let by = |name: &str| {
        got.proposals
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .clone()
    };
    let of = |name: &str| shows.iter().find(|s| s.name == name).unwrap().rule.clone();

    // The rules are not the same as written (a start date moved to the first date, a slot
    // given one), and are the same show all the same.
    let drive = by("Kettle Drive");
    assert_ne!(drive.rule, Some(of("Kettle Drive")));
    let bothy = by("The Bothy Sessions");
    assert_ne!(bothy.rule, Some(of("The Bothy Sessions")));

    // The name: case and spaces folded, and nothing more.
    assert_eq!(fold_name("  The  LATE\tKettle "), "the late kettle");
    assert!(drive.is_show("  kettle   DRIVE ", &of("Kettle Drive"), &z, now()));
    assert!(!drive.is_show("Kettle Drive Time", &of("Kettle Drive"), &z, now()));
    assert!(!drive.is_show("KettleDrive", &of("Kettle Drive"), &z, now()));

    // Another name at the same times is another show; so is the same name at another time, on
    // other days, at another length, or with one date skipped or moved.
    assert!(!drive.is_show("Kettle Breakfast", &of("Kettle Drive"), &z, now()));
    assert!(!drive.is_show("Kettle Drive", &of("Kettle Breakfast"), &z, now()));
    let mut later = of("Kettle Drive");
    later.start += 30;
    assert!(!drive.is_show("Kettle Drive", &later, &z, now()));
    let mut longer = of("Kettle Drive");
    longer.minutes = Some(90);
    assert!(!drive.is_show("Kettle Drive", &longer, &z, now()));
    let mut fewer = of("Kettle Drive");
    fewer.repeat = Repeat::Weekly {
        every: 1,
        weekdays: vec![1, 2, 3, 4],
    };
    assert!(!drive.is_show("Kettle Drive", &fewer, &z, now()));
    let mut skipped = of("Kettle Drive");
    skipped.skip = vec![date(2027, 3, 1)];
    assert!(!drive.is_show("Kettle Drive", &skipped, &z, now()));
    let mut unmoved = of("The Bothy Sessions");
    unmoved.moves.clear();
    assert!(!bothy.is_show("The Bothy Sessions", &unmoved, &z, now()));
    let mut ended = of("Saturday Night Kettle");
    ended.end = End::Count { count: 11 };
    assert!(!by("Saturday Night Kettle").is_show("Saturday Night Kettle", &ended, &z, now()));

    // A date that has passed tells no two shows apart: what is compared is still to come.
    let mut past = of("Kettle Breakfast");
    past.skip = vec![date(2026, 10, 1)];
    assert!(by("Kettle Breakfast").is_show("Kettle Breakfast", &past, &z, now()));

    // A show whose first date is years away is compared from that date, not from today.
    let far = rule(
        date(2036, 9, 6),
        "20:00",
        180,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![6],
        },
    );
    let mut far_fortnightly = far.clone();
    far_fortnightly.repeat = Repeat::Weekly {
        every: 2,
        weekdays: vec![6],
    };
    assert!(same_dates_to_come(&far, &far, &z, now()));
    assert!(!same_dates_to_come(&far, &far_fortnightly, &z, now()));
    assert!(!same_dates_to_come(&far_fortnightly, &far, &z, now()));

    // A show with nothing to come is not there to be the same as; nor is what was refused.
    let over = rule(date(2026, 10, 1), "18:00", 90, Repeat::Once);
    assert!(!same_dates_to_come(&over, &over, &z, now()));
    let fair = one(
        &cal(
            "BEGIN:VEVENT\r\nUID:fair\r\nSUMMARY:Record Fair\r\nDTSTART;VALUE=DATE:20261205\r\nDTEND;VALUE=DATE:20261206\r\nEND:VEVENT\r\n",
        ),
        zone,
    );
    assert_eq!(fair.mapping, Mapping::Refused);
    assert!(!fair.is_show("Record Fair", &of("Budget Night Special"), &z, now()));
}
