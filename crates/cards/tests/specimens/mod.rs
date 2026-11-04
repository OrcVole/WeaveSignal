// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The design's specimens (`Library/design/weavesignal-design-show-cards.html`): Kettle FM's
//! Saturday Night Kettle at the show, series and episode levels in both layouts, and the
//! stress cases: a 78-character show name, five guests, and the one right-to-left name kept
//! outside the Scottish names list (ADR-0071).

#![allow(dead_code)]

use weavesignal_cards::{
    CardData, EpisodeCard, Layout, SeriesCard, ShowCard, Slot, StationCard, Weekday,
};

pub fn station() -> StationCard {
    StationCard {
        name: "Kettle FM".into(),
        place: "Dunfermline".into(),
        address: "kettle.example".into(),
    }
}

fn saturday_night() -> ShowCard {
    ShowCard {
        name: "Saturday Night Kettle".into(),
        slug: "saturday-night".into(),
        description: "Live music, requests and the Forth after dark, every Saturday from eight."
            .into(),
        slot: Some(Slot {
            weekday: Weekday::Saturday,
            start: "20:00".parse().unwrap(),
            end: Some("23:00".parse().unwrap()),
        }),
    }
}

fn hosts() -> Vec<String> {
    vec![
        "Shona Campbell".into(),
        "Catriona MacKenzie-Buchanan".into(),
    ]
}

fn series3() -> SeriesCard {
    SeriesCard {
        number: 3,
        title: None,
        first: Some("2026-09-05".parse().unwrap()),
        last: Some("2026-11-07".parse().unwrap()),
        episodes: Some(10),
    }
}

/// Saturday Night Kettle is bank 3 (its colour on the cart wall and the calendar, ADR-0066).
pub fn show(layout: Layout) -> CardData {
    CardData {
        layout,
        colour_slot: 3,
        station: station(),
        show: saturday_night(),
        series: None,
        episode: None,
        hosts: hosts(),
        guests: vec![],
    }
}

pub fn series(layout: Layout) -> CardData {
    CardData {
        series: Some(series3()),
        ..show(layout)
    }
}

pub fn episode(layout: Layout) -> CardData {
    CardData {
        episode: Some(EpisodeCard {
            number: 7,
            title: "Ghost Train night".into(),
            air: Some("2026-10-24T20:00[Europe/London]".parse().unwrap()),
        }),
        guests: vec!["Firth Lights".into(), "Dr Una Robertson".into()],
        ..series(layout)
    }
}

/// The stress case: a 78-character name, on the free bank 12.
pub fn long_name() -> CardData {
    CardData {
        layout: Layout::Poster,
        colour_slot: 12,
        station: station(),
        show: ShowCard {
            name: "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society"
                .into(),
            slug: "shipping-forecast".into(),
            description: String::new(),
            slot: Some(Slot {
                weekday: Weekday::Sunday,
                start: "23:00".parse().unwrap(),
                end: None,
            }),
        },
        series: None,
        episode: None,
        hosts: vec!["Hamish Wallace".into()],
        guests: vec![],
    }
}

/// The stress case: five guests, one of them the right-to-left test name.
pub fn five_guests(layout: Layout) -> CardData {
    CardData {
        episode: Some(EpisodeCard {
            number: 9,
            title: "The big ceilidh".into(),
            air: Some("2026-11-07T20:00[Europe/London]".parse().unwrap()),
        }),
        guests: vec![
            "Firth Lights".into(),
            "Dr Una Robertson".into(),
            "Rùnach".into(),
            "Lachlan Scott".into(),
            "نور الهدى حسن".into(),
        ],
        ..series(layout)
    }
}

/// The stress case: the right-to-left test name first in a list of two guests (the design's
/// `cards.square.rtl`).
pub fn rtl_first(layout: Layout) -> CardData {
    CardData {
        episode: Some(EpisodeCard {
            number: 8,
            title: "Songs for the clocks going back".into(),
            air: Some("2026-10-31T20:00[Europe/London]".parse().unwrap()),
        }),
        guests: vec!["نور الهدى حسن".into(), "Lachlan Scott".into()],
        ..series(layout)
    }
}

/// The 78-character name at the series level, with the hosts of the show.
pub fn long_name_series() -> CardData {
    CardData {
        series: Some(series3()),
        hosts: hosts(),
        ..long_name()
    }
}

/// The 78-character name at the episode level, with two hosts and five guests (one of them
/// right to left): every limit of the square at once.
pub fn long_name_episode() -> CardData {
    CardData {
        series: Some(series3()),
        episode: Some(EpisodeCard {
            number: 9,
            title: "The big ceilidh".into(),
            air: Some("2026-11-07T20:00[Europe/London]".parse().unwrap()),
        }),
        hosts: hosts(),
        guests: five_guests(Layout::Poster).guests,
        ..long_name()
    }
}

/// The Poster squares by their panel names in the design (`cards.square.*`, ADR-0070).
pub fn poster_squares() -> Vec<(&'static str, CardData)> {
    vec![
        ("cards.square.show", show(Layout::Poster)),
        ("cards.square.series", series(Layout::Poster)),
        ("cards.square.episode", episode(Layout::Poster)),
        ("cards.square.long-name", long_name()),
        ("cards.square.seven-people", five_guests(Layout::Poster)),
        ("cards.square.rtl", rtl_first(Layout::Poster)),
    ]
}

/// Every specimen by its panel name (ADR-0070) and the output it is drawn at.
pub fn all() -> Vec<(&'static str, CardData)> {
    vec![
        ("cards.poster.show", show(Layout::Poster)),
        ("cards.poster.series", series(Layout::Poster)),
        ("cards.poster.episode", episode(Layout::Poster)),
        ("cards.slate.show", show(Layout::Slate)),
        ("cards.slate.series", series(Layout::Slate)),
        ("cards.slate.episode", episode(Layout::Slate)),
        ("cards.stress.long-name", long_name()),
        ("cards.stress.five-guests", five_guests(Layout::Slate)),
        ("cards.stress.rtl-list", five_guests(Layout::Poster)),
    ]
}
