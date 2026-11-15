// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

use jiff::civil::date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

use super::*;

fn london() -> TimeZone {
    TimeZone::get("Europe/London").unwrap()
}

/// The design's own "today" (2026-11-13), for the sentences.
fn today() -> Date {
    date(2026, 11, 13)
}

fn rule(starts_on: Option<Date>, start: &str, minutes: u16, repeat: Repeat) -> Rule {
    let (h, m) = start.split_once(':').unwrap();
    Rule {
        starts_on,
        start: h.parse::<u16>().unwrap() * 60 + m.parse::<u16>().unwrap(),
        minutes: Some(minutes),
        repeat,
        end: End::Never,
        skip: vec![],
        moves: vec![],
    }
}

fn local(o: &Occurrence) -> String {
    o.start
        .to_zoned(london())
        .strftime("%a %Y-%m-%d %H:%M %Z")
        .to_string()
}

fn dates(r: &Rule, from: Date, n: usize) -> Vec<String> {
    r.occurrences(&london(), from)
        .take(n)
        .map(|o| o.on.to_string())
        .collect()
}

#[test]
fn the_designs_sample_round_trips_and_unknown_fields_are_refused() {
    let json = r#"{
      "starts_on": "2026-12-01",
      "start": 1200,
      "minutes": 60,
      "repeat": { "kind": "monthly_nth", "every": 1, "nth": 1, "weekday": 2 },
      "end": { "kind": "count", "count": 10 },
      "skip": ["2027-01-05"],
      "moves": [{ "from": "2027-04-06", "to": "2027-04-13T20:00:00", "minutes": null }]
    }"#;
    let r: Rule = serde_json::from_str(json).unwrap();
    assert_eq!(r.end, End::Count { count: 10 });
    assert_eq!(
        r.moves[0].to,
        date(2027, 4, 13).at(20, 0, 0, 0),
        "a civil date-time in ISO form"
    );
    let back: Rule = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back, r);
    assert_eq!(
        serde_json::to_value(&r.end).unwrap(),
        serde_json::json!({"kind": "count", "count": 10})
    );
    assert_eq!(
        serde_json::to_value(End::Until {
            until: date(2027, 2, 26)
        })
        .unwrap(),
        serde_json::json!({"kind": "until", "until": "2027-02-26"})
    );
    assert_eq!(
        serde_json::to_value(Repeat::Once).unwrap(),
        serde_json::json!({"kind": "once"})
    );
    assert!(
        serde_json::from_str::<Rule>(r#"{"start":0,"repeat":{"kind":"once"},"every":2}"#).is_err()
    );
    assert!(
        serde_json::from_str::<Rule>(
            r#"{"start":0,"repeat":{"kind":"weekly","every":2,"weekdays":[1],"x":1}}"#
        )
        .is_err()
    );
    // Only `start` and `repeat` are required.
    let min: Rule = serde_json::from_str(r#"{"start":600,"repeat":{"kind":"once"}}"#).unwrap();
    assert_eq!(min.end, End::Never);
    assert!(min.skip.is_empty() && min.moves.is_empty() && min.minutes.is_none());
}

/// The design's section 2 table, verbatim, with 13 November 2026 as today.
#[test]
fn the_sentences_and_short_forms_are_the_designs() {
    let z = london();
    let cases: Vec<(Rule, &str, &str)> = vec![
        (
            rule(Some(date(2026, 11, 30)), "19:00", 120, Repeat::Once),
            "Once, on Monday 30 November, 19:00 to 21:00.",
            "Once, Mon 30 Nov, 19:00–21:00",
        ),
        (
            Rule {
                end: End::Until {
                    until: date(2027, 2, 26),
                },
                ..rule(
                    Some(date(2026, 11, 18)),
                    "21:00",
                    120,
                    Repeat::Weekly {
                        every: 1,
                        weekdays: vec![3, 5],
                    },
                )
            },
            "Every week on Wednesday and Friday, 21:00 to 23:00, from 18 November, until 26 February 2027.",
            "Wed and Fri, 21:00–23:00",
        ),
        (
            rule(
                None,
                "07:00",
                180,
                Repeat::Weekly {
                    every: 1,
                    weekdays: vec![1, 2, 3, 4, 5],
                },
            ),
            "Every week on Monday to Friday, 07:00 to 10:00.",
            "Mon–Fri, 07:00–10:00",
        ),
        (
            rule(
                Some(date(2026, 11, 15)),
                "18:00",
                60,
                Repeat::Weekly {
                    every: 2,
                    weekdays: vec![7],
                },
            ),
            "Every 2 weeks on Sunday, 18:00 to 19:00, from 15 November.",
            "Every 2 weeks, Sun, 18:00–19:00",
        ),
        (
            Rule {
                end: End::Count { count: 10 },
                skip: vec![date(2027, 1, 5)],
                ..rule(
                    Some(date(2026, 12, 1)),
                    "20:00",
                    60,
                    Repeat::MonthlyNth {
                        every: 1,
                        nth: 1,
                        weekday: 2,
                    },
                )
            },
            "Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes. Not on 5 January 2027.",
            "First Tue monthly, 20:00–21:00",
        ),
        (
            rule(
                Some(date(2026, 11, 27)),
                "22:00",
                120,
                Repeat::MonthlyNth {
                    every: 1,
                    nth: -1,
                    weekday: 5,
                },
            ),
            "Every month on the last Friday, 22:00 to 00:00 the next day, from 27 November.",
            "Last Fri monthly, 22:00–00:00",
        ),
        (
            rule(
                Some(date(2026, 12, 31)),
                "22:00",
                150,
                Repeat::Yearly { month: 12, day: 31 },
            ),
            "Every year on 31 December, 22:00 to 00:30 the next day.",
            "Yearly, 31 Dec, 22:00–00:30",
        ),
        (
            rule(
                Some(date(2026, 12, 5)),
                "14:00",
                120,
                Repeat::Dates {
                    dates: vec![
                        date(2026, 12, 5),
                        date(2026, 12, 12),
                        date(2026, 12, 19),
                        date(2027, 1, 9),
                    ],
                },
            ),
            "On 4 dates you chose, 14:00 to 16:00, from Saturday 5 December.",
            "4 chosen dates, 14:00–16:00",
        ),
    ];
    for (r, sentence, short) in cases {
        assert_eq!(r.problems(), vec![], "{sentence}");
        let w = r.words(&z, today());
        assert_eq!(w.sentence, sentence);
        assert_eq!(w.short, short, "{sentence}");
    }
    // The old weekly slot: no start, nothing to say but the day and the times.
    let slot = Rule::from(Slot {
        weekday: 1,
        start: 20 * 60,
        minutes: Some(180),
    });
    assert_eq!(
        slot.words(&z, today()).sentence,
        "Every week on Monday, 20:00 to 23:00."
    );
    assert_eq!(slot.words(&z, today()).short, "Mon, 20:00–23:00");
    let daily = Rule {
        repeat: Repeat::Weekly {
            every: 1,
            weekdays: vec![1, 2, 3, 4, 5, 6, 7],
        },
        ..Rule::from(Slot {
            weekday: 1,
            start: 60,
            minutes: None,
        })
    };
    assert_eq!(
        daily.words(&z, today()).sentence,
        "Every day, 01:00 to 02:00."
    );
    // The end line, the leap year note, the month note.
    let ends = Rule {
        end: End::Count { count: 10 },
        skip: vec![date(2027, 1, 5)],
        ..rule(
            Some(date(2026, 12, 1)),
            "20:00",
            60,
            Repeat::MonthlyNth {
                every: 1,
                nth: 1,
                weekday: 2,
            },
        )
    };
    let w = ends.words(&z, today());
    assert_eq!(
        w.last.as_deref(),
        Some("The last is Tuesday 5 October 2027.")
    );
    assert_eq!(w.last_on, Some(date(2027, 10, 5)));
    let leap = rule(
        Some(date(2028, 2, 29)),
        "12:00",
        60,
        Repeat::Yearly { month: 2, day: 29 },
    );
    assert_eq!(
        leap.words(&z, today()).notes,
        vec!["Only in leap years. The next is 29 February 2028.".to_owned()]
    );
    let thirty_first = rule(
        Some(date(2026, 12, 31)),
        "12:00",
        60,
        Repeat::MonthlyDay { every: 1, day: 31 },
    );
    assert_eq!(
        thirty_first.words(&z, today()).notes,
        vec!["Months without a 31st are skipped.".to_owned()]
    );
    assert_eq!(
        thirty_first.words(&z, today()).sentence,
        "Every month on the 31st, 12:00 to 13:00, from 31 December."
    );
    // A moved date and three skipped dates.
    let moved = Rule {
        skip: vec![date(2027, 1, 5), date(2027, 2, 2), date(2027, 3, 2)],
        moves: vec![Move {
            from: date(2027, 4, 6),
            to: date(2027, 4, 13).at(19, 0, 0, 0),
            minutes: None,
        }],
        ..rule(
            Some(date(2026, 12, 1)),
            "20:00",
            60,
            Repeat::MonthlyNth {
                every: 1,
                nth: 1,
                weekday: 2,
            },
        )
    };
    assert_eq!(
        moved.words(&z, today()).sentence,
        "Every month on the first Tuesday, 20:00 to 21:00, from 1 December. 3 dates skipped. 6 April 2027 moves to 13 April 2027 at 19:00."
    );
}

fn slot_zones() -> Vec<TimeZone> {
    [
        "Europe/London",
        "America/New_York",
        "Australia/Lord_Howe",
        "UTC",
    ]
    .iter()
    .map(|n| TimeZone::get(n).unwrap())
    .collect()
}

/// Every slot gives the same instants through the rule as through `Slot::next_after`, over two
/// years across both of Europe/London's clock changes (and three other zones, one of which
/// moves its clocks by half an hour).
#[test]
fn every_slot_gives_the_same_instants_through_the_rule() {
    let from = date(2026, 1, 1);
    let weeks = 104;
    for zone in slot_zones() {
        // Every start through the small hours (where the clocks change) at the minute, and
        // every quarter hour all day.
        let starts: Vec<u16> = (0..300).chain((300..1440).step_by(15)).collect();
        for weekday in 1..=7u8 {
            for &start in &starts {
                let slot = Slot {
                    weekday,
                    start,
                    minutes: Some(60),
                };
                let rule = Rule::from(slot);
                assert_eq!(rule.as_slot(), Some(slot));
                let mut after = from
                    .at(0, 0, 0, 0)
                    .to_zoned(zone.clone())
                    .unwrap()
                    .timestamp()
                    - 1.second();
                let mut old = Vec::with_capacity(weeks);
                for _ in 0..weeks {
                    let next = slot.next_after(&zone, after).unwrap();
                    old.push(next);
                    after = next;
                }
                let new: Vec<Timestamp> = rule
                    .occurrences(&zone, from)
                    .take(weeks)
                    .map(|o| o.start)
                    .collect();
                assert_eq!(new, old, "{zone:?} weekday {weekday} start {start}");
            }
        }
    }
}

#[test]
fn the_clocks_changing_are_said_and_the_show_stays_on_air() {
    let z = london();
    // 01:30 for an hour, on Sundays: the two change nights of 2027.
    let r = rule(
        Some(date(2027, 3, 21)),
        "01:30",
        60,
        Repeat::Weekly {
            every: 1,
            weekdays: vec![7],
        },
    );
    let march: Vec<Occurrence> = r.occurrences(&z, date(2027, 3, 21)).take(2).collect();
    assert_eq!(march[0].clock, None);
    assert_eq!(march[1].on, date(2027, 3, 28));
    assert_eq!(march[1].clock, Some(ClockChange::Forward));
    assert_eq!(
        local(&march[1]),
        "Sun 2027-03-28 02:30 BST",
        "02:30 BST = 01:30 UTC"
    );
    assert_eq!(
        march[1].start,
        "2027-03-28T01:30:00Z".parse::<Timestamp>().unwrap()
    );
    assert_eq!(
        march[1]
            .end()
            .to_zoned(z.clone())
            .strftime("%H:%M %Z")
            .to_string(),
        "03:30 BST"
    );
    let oct: Vec<Occurrence> = r.occurrences(&z, date(2027, 10, 24)).take(2).collect();
    assert_eq!(oct[1].on, date(2027, 10, 31));
    assert_eq!(oct[1].clock, Some(ClockChange::Back));
    assert_eq!(
        oct[1].start,
        "2027-10-31T00:30:00Z".parse::<Timestamp>().unwrap()
    );
    assert_eq!(
        local(&oct[1]),
        "Sun 2027-10-31 01:30 BST",
        "the first 01:30"
    );
    // The end is an hour on: the second 01:30, GMT.
    assert_eq!(
        oct[1]
            .end()
            .to_zoned(z.clone())
            .strftime("%H:%M %Z")
            .to_string(),
        "01:30 GMT"
    );
    // A show that only spans the change keeps its length.
    let spans = rule(Some(date(2027, 3, 28)), "00:30", 120, Repeat::Once);
    let o = spans.occurrences(&z, date(2027, 3, 28)).next().unwrap();
    assert_eq!(o.clock, None);
    assert_eq!(
        o.end().to_zoned(z.clone()).strftime("%H:%M %Z").to_string(),
        "03:30 BST"
    );
    // The occurrence is the date of the nominal start, and a show past midnight belongs to the
    // date it starts on.
    let nye = rule(
        Some(date(2026, 12, 31)),
        "22:00",
        150,
        Repeat::Yearly { month: 12, day: 31 },
    );
    let o = nye.occurrences(&z, date(2026, 12, 31)).next().unwrap();
    assert_eq!(o.on, date(2026, 12, 31));
    assert_eq!(
        o.end().to_zoned(z).strftime("%Y-%m-%d %H:%M").to_string(),
        "2027-01-01 00:30"
    );
}

#[test]
fn the_first_tuesdays_are_the_designs() {
    let r = rule(
        Some(date(2026, 12, 1)),
        "20:00",
        60,
        Repeat::MonthlyNth {
            every: 1,
            nth: 1,
            weekday: 2,
        },
    );
    assert_eq!(
        dates(&r, date(2026, 12, 1), 11),
        [
            "2026-12-01",
            "2027-01-05",
            "2027-02-02",
            "2027-03-02",
            "2027-04-06",
            "2027-05-04",
            "2027-06-01",
            "2027-07-06",
            "2027-08-03",
            "2027-09-07",
            "2027-10-05"
        ]
    );
    // From a later date, the same dates.
    assert_eq!(dates(&r, date(2027, 9, 1), 2), ["2027-09-07", "2027-10-05"]);
    // The last Friday, and every second month.
    let last = rule(
        Some(date(2026, 11, 27)),
        "22:00",
        120,
        Repeat::MonthlyNth {
            every: 2,
            nth: -1,
            weekday: 5,
        },
    );
    assert_eq!(
        dates(&last, date(2026, 11, 1), 4),
        ["2026-11-27", "2027-01-29", "2027-03-26", "2027-05-28"]
    );
}

#[test]
fn a_count_counts_episodes_so_a_skipped_date_does_not_use_one_up() {
    let mut r = rule(
        Some(date(2026, 12, 1)),
        "20:00",
        60,
        Repeat::MonthlyNth {
            every: 1,
            nth: 1,
            weekday: 2,
        },
    );
    r.end = End::Count { count: 10 };
    let plain = dates(&r, date(2026, 12, 1), 20);
    assert_eq!(plain.len(), 10);
    assert_eq!(plain.last().unwrap(), "2027-09-07");
    r.skip = vec![date(2027, 1, 5)];
    let skipped = dates(&r, date(2026, 12, 1), 20);
    assert_eq!(skipped.len(), 10, "ten episodes are still made");
    assert!(!skipped.contains(&"2027-01-05".to_owned()));
    assert_eq!(skipped.last().unwrap(), "2027-10-05", "one date later");
    // Counted from the rule's own start, whatever the caller starts from.
    assert_eq!(
        dates(&r, date(2027, 6, 1), 20).last().unwrap(),
        "2027-10-05"
    );
    // The preview's view keeps the skipped date, marked.
    let all: Vec<(String, bool)> = r
        .occurrences_with_skips(&london(), date(2026, 12, 1))
        .map(|(o, s)| (o.on.to_string(), s))
        .collect();
    assert_eq!(all.len(), 11);
    assert_eq!(all[1], ("2027-01-05".to_owned(), true));
    assert_eq!(all.iter().filter(|(_, s)| !s).count(), 10);
}

#[test]
fn until_is_inclusive_and_an_every_second_week_keeps_its_week() {
    let mut r = rule(
        Some(date(2026, 11, 15)),
        "18:00",
        60,
        Repeat::Weekly {
            every: 2,
            weekdays: vec![7, 3],
        },
    );
    r.end = End::Until {
        until: date(2026, 12, 13),
    };
    // Week zero holds Sunday 15 November (its Monday is the 9th); that week's Wednesday, the
    // 11th, is before the start and is not given.
    assert_eq!(
        dates(&r, date(2026, 11, 15), 20),
        [
            "2026-11-15",
            "2026-11-25",
            "2026-11-29",
            "2026-12-09",
            "2026-12-13"
        ]
    );
    assert!(r.gives(date(2026, 12, 13)));
    assert!(!r.gives(date(2026, 12, 20)));
    assert!(!r.gives(date(2026, 11, 22)), "the off week");
}

#[test]
fn months_without_the_day_are_passed_over_and_leap_days_come_every_fourth_year() {
    let r = rule(
        Some(date(2026, 12, 31)),
        "12:00",
        60,
        Repeat::MonthlyDay { every: 1, day: 31 },
    );
    assert_eq!(
        dates(&r, date(2026, 12, 31), 6),
        [
            "2026-12-31",
            "2027-01-31",
            "2027-03-31",
            "2027-05-31",
            "2027-07-31",
            "2027-08-31"
        ]
    );
    let last = rule(
        Some(date(2027, 1, 1)),
        "12:00",
        60,
        Repeat::MonthlyDay { every: 1, day: -1 },
    );
    assert_eq!(
        dates(&last, date(2027, 1, 1), 3),
        ["2027-01-31", "2027-02-28", "2027-03-31"]
    );
    let leap = rule(
        Some(date(2026, 3, 1)),
        "12:00",
        60,
        Repeat::Yearly { month: 2, day: 29 },
    );
    assert_eq!(
        dates(&leap, date(2026, 3, 1), 3),
        ["2028-02-29", "2032-02-29", "2036-02-29"]
    );
    let every_other = rule(
        Some(date(2026, 12, 15)),
        "12:00",
        60,
        Repeat::MonthlyDay { every: 2, day: 15 },
    );
    assert_eq!(
        dates(&every_other, date(2026, 12, 15), 3),
        ["2026-12-15", "2027-02-15", "2027-04-15"]
    );
}

#[test]
fn chosen_dates_and_a_move_in_the_rule() {
    let mut r = rule(
        Some(date(2026, 12, 5)),
        "14:00",
        120,
        Repeat::Dates {
            dates: vec![date(2026, 12, 19), date(2026, 12, 5), date(2026, 12, 12)],
        },
    );
    assert_eq!(
        dates(&r, date(2026, 12, 1), 5),
        ["2026-12-05", "2026-12-12", "2026-12-19"]
    );
    r.moves = vec![Move {
        from: date(2026, 12, 12),
        to: date(2026, 12, 13).at(10, 0, 0, 0),
        minutes: Some(30),
    }];
    assert_eq!(r.problems(), vec![]);
    let o: Vec<Occurrence> = r.occurrences(&london(), date(2026, 12, 1)).collect();
    assert_eq!(
        o[1].on,
        date(2026, 12, 12),
        "it still fills the date it was moved from"
    );
    assert_eq!(o[1].moved_from, Some(date(2026, 12, 12).at(14, 0, 0, 0)));
    assert_eq!(o[1].minutes, 30);
    assert_eq!(local(&o[1]), "Sun 2026-12-13 10:00 GMT");
    // A moved date stays between its neighbours.
    r.moves[0].to = date(2026, 12, 20).at(10, 0, 0, 0);
    let p = r.problems();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].field, "moves");
    r.moves[0].to = date(2026, 12, 4).at(10, 0, 0, 0);
    assert_eq!(r.problems().len(), 1, "not before the date before it");
}

fn fields(r: &Rule) -> Vec<String> {
    r.problems().into_iter().map(|p| p.field).collect()
}

#[test]
fn check_refuses_exactly_what_the_design_lists() {
    let good = rule(
        Some(date(2026, 12, 1)),
        "20:00",
        60,
        Repeat::MonthlyNth {
            every: 1,
            nth: 1,
            weekday: 2,
        },
    );
    assert!(good.check().is_ok());
    assert_eq!(
        fields(&Rule {
            start: 1440,
            ..good.clone()
        }),
        ["start"]
    );
    for minutes in [0, 1441] {
        assert_eq!(
            fields(&Rule {
                minutes: Some(minutes),
                ..good.clone()
            }),
            ["minutes"]
        );
    }
    let with = |repeat: Repeat| Rule {
        repeat,
        ..good.clone()
    };
    assert_eq!(
        fields(&with(Repeat::MonthlyNth {
            every: 13,
            nth: 1,
            weekday: 2
        })),
        ["repeat.every"]
    );
    for nth in [5, -2, 0] {
        assert_eq!(
            fields(&with(Repeat::MonthlyNth {
                every: 1,
                nth,
                weekday: 2
            })),
            ["repeat.nth"]
        );
    }
    for day in [0, 32, -2] {
        assert_eq!(
            fields(&with(Repeat::MonthlyDay { every: 1, day })),
            ["repeat.day"]
        );
    }
    for weekdays in [vec![], vec![2, 2], vec![0], vec![8]] {
        assert_eq!(
            fields(&with(Repeat::Weekly { every: 1, weekdays })),
            ["repeat.weekdays"]
        );
    }
    for count in [0, 521] {
        assert_eq!(
            fields(&Rule {
                end: End::Count { count },
                ..good.clone()
            }),
            ["end.count"]
        );
    }
    assert!(
        Rule {
            end: End::Count { count: 520 },
            ..good.clone()
        }
        .check()
        .is_ok()
    );
    let many: Vec<Date> = (0..121).map(|i| date(2027, 1, 1) + i.days()).collect();
    assert_eq!(
        fields(&Rule {
            skip: many.clone(),
            ..good.clone()
        })
        .first()
        .map(String::as_str),
        Some("skip")
    );
    assert_eq!(
        fields(&with(Repeat::Dates { dates: many })),
        ["repeat.dates"]
    );
    assert_eq!(
        fields(&Rule {
            skip: vec![date(2027, 1, 5), date(2027, 1, 5)],
            ..good.clone()
        }),
        ["skip"]
    );
    // `starts_on` missing: only a weekly rule of every week can do without.
    assert_eq!(
        fields(&Rule {
            starts_on: None,
            ..good.clone()
        }),
        ["starts_on"]
    );
    assert_eq!(
        fields(&Rule {
            starts_on: None,
            repeat: Repeat::Weekly {
                every: 2,
                weekdays: vec![2]
            },
            ..good.clone()
        }),
        ["starts_on"]
    );
    assert!(
        Rule {
            starts_on: None,
            repeat: Repeat::Weekly {
                every: 1,
                weekdays: vec![2]
            },
            ..good.clone()
        }
        .check()
        .is_ok()
    );
    // Once and chosen dates have no end.
    assert_eq!(
        fields(&Rule {
            repeat: Repeat::Once,
            end: End::Count { count: 3 },
            ..good.clone()
        }),
        ["end"]
    );
    // A skip or a move that the rule does not give.
    assert_eq!(
        fields(&Rule {
            skip: vec![date(2027, 1, 6)],
            ..good.clone()
        }),
        ["skip"]
    );
    assert_eq!(
        fields(&Rule {
            moves: vec![Move {
                from: date(2027, 1, 6),
                to: date(2027, 1, 6).at(20, 0, 0, 0),
                minutes: None
            }],
            ..good.clone()
        }),
        ["moves"]
    );
    assert!(
        Rule {
            skip: vec![date(2027, 1, 5)],
            ..good.clone()
        }
        .check()
        .is_ok()
    );
}

#[test]
fn a_rule_is_slot_shaped_only_when_it_is_exactly_that() {
    let slot = Slot {
        weekday: 3,
        start: 600,
        minutes: None,
    };
    let r = Rule::from(slot);
    assert_eq!(r.as_slot(), Some(slot));
    assert_eq!(
        Rule {
            starts_on: Some(date(2026, 12, 2)),
            ..r.clone()
        }
        .as_slot(),
        None
    );
    assert_eq!(
        Rule {
            skip: vec![date(2026, 12, 2)],
            ..r.clone()
        }
        .as_slot(),
        None
    );
    assert_eq!(
        Rule {
            end: End::Count { count: 5 },
            ..r.clone()
        }
        .as_slot(),
        None
    );
    assert_eq!(
        Rule {
            repeat: Repeat::Weekly {
                every: 2,
                weekdays: vec![3]
            },
            ..r.clone()
        }
        .as_slot(),
        None
    );
    assert_eq!(
        Rule {
            repeat: Repeat::Weekly {
                every: 1,
                weekdays: vec![3, 4]
            },
            ..r.clone()
        }
        .as_slot(),
        None
    );
}
