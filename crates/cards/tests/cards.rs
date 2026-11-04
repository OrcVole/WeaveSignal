// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The card renderer against the design's specimens and stress cases.
//!
//! Golden images live in `tests/golden/`; a render may differ from its golden by a little
//! (tiny-skia's arithmetic differs between CPUs), not by a changed layout. After a deliberate
//! change, look at the new images and bless them:
//! `WEAVESIGNAL_BLESS=1 cargo test -p weavesignal-cards --test cards`.
//! `WEAVESIGNAL_CARDS_DUMP=<dir>` writes every output of every specimen there for review.

mod specimens;

use std::path::PathBuf;

use resvg::tiny_skia::Pixmap;
use weavesignal_cards::{CardData, CardError, Layout, Output, alt_text, ink_boxes, render};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn dump(name: &str, d: &CardData) {
    let Ok(dir) = std::env::var("WEAVESIGNAL_CARDS_DUMP") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    for o in Output::ALL {
        let r = render(d, o).unwrap_or_else(|e| panic!("{name} {o:?}: {e}"));
        let path = PathBuf::from(&dir).join(format!("{name}.{}.{}", o.as_str(), o.extension()));
        std::fs::write(&path, &r.bytes).unwrap();
        std::fs::write(path.with_extension("svg"), &r.svg).unwrap();
    }
}

#[test]
fn every_specimen_fits_the_safe_area_at_every_size() {
    for (name, d) in specimens::all() {
        dump(name, &d);
        for o in Output::ALL {
            let (l, t, r, b) = o.margin();
            let ink = ink_boxes(&d, o).unwrap_or_else(|e| panic!("{name} {o:?}: {e}"));
            assert!(ink.len() > 6, "{name} {o:?}: too little drawn");
            // The measure is live: things are where they belong, not only "nothing outside".
            let (w, h, _) = o.frame();
            assert!(
                ink.iter().any(|(_, i)| i.right > w - 200.0),
                "{name} {o:?}: the address at the right"
            );
            assert!(
                ink.iter().any(|(_, i)| i.bottom > h - 120.0),
                "{name} {o:?}: the foot"
            );
            assert!(
                ink.iter().any(|(_, i)| i.top < 140.0),
                "{name} {o:?}: the head"
            );
            for (id, i) in &ink {
                if id.starts_with("text-") {
                    assert!(
                        i.right - i.left > 1.0 && i.bottom - i.top > 1.0,
                        "{name} {o:?}: {id} has no area"
                    );
                }
            }
            for (id, i) in ink {
                assert!(
                    i.left >= l - 0.5
                        && i.top >= t - 0.5
                        && i.right <= r + 0.5
                        && i.bottom <= b + 0.5,
                    "{name} {o:?}: {id} at {i:?} is outside the margin {:?}",
                    (l, t, r, b)
                );
            }
        }
    }
}

#[test]
fn no_glyph_is_missing_from_the_bundled_fonts() {
    for (name, d) in specimens::all() {
        for o in Output::ALL {
            assert_eq!(
                weavesignal_cards::missing_glyphs(&d, o).unwrap(),
                0,
                "{name} {o:?} draws a blank box"
            );
        }
    }
}

#[test]
fn the_alt_text_reads_as_the_design_prints_it() {
    use specimens::*;
    assert_eq!(
        alt_text(&show(Layout::Poster)),
        "Saturday Night Kettle, Saturdays 20:00 on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan. kettle.example/saturday-night"
    );
    assert_eq!(
        alt_text(&series(Layout::Slate)),
        "Saturday Night Kettle, Series 3, 5 September to 7 November 2026, Saturdays 20:00 on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan. kettle.example/saturday-night/s3"
    );
    assert_eq!(
        alt_text(&episode(Layout::Poster)),
        "Saturday Night Kettle, Series 3 Episode 7, Ghost Train night, Saturday 24 October 2026 at 20:00 BST on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan, guests Firth Lights and Dr Una Robertson. kettle.example/saturday-night/s3/e7"
    );
    assert_eq!(
        alt_text(&five_guests(Layout::Slate)),
        "Saturday Night Kettle, Series 3 Episode 9, The big ceilidh, Saturday 7 November 2026 at 20:00 GMT on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan, guests Firth Lights, Dr Una Robertson, Rùnach, Lachlan Scott and نور الهدى حسن. kettle.example/saturday-night/s3/e9"
    );
    assert_eq!(
        alt_text(&long_name()),
        "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society, Sundays 23:00 on Kettle FM, with Hamish Wallace. kettle.example/shipping-forecast"
    );
}

#[test]
fn the_visible_names_use_a_non_breaking_hyphen_and_isolate_the_rtl_name() {
    let svg =
        weavesignal_cards::svg(&specimens::five_guests(Layout::Poster), Output::Link).unwrap();
    assert!(svg.contains("MacKenzie\u{2011}Buchanan"));
    assert!(!svg.contains("MacKenzie-Buchanan"));
    // The isolates around the name, the name in the Arabic face.
    let (fsi, pdi) = (svg.find('\u{2068}').unwrap(), svg.find('\u{2069}').unwrap());
    let name = svg.find("نور الهدى حسن").unwrap();
    assert!(fsi < name && name < pdi);
    assert!(svg.contains("'Noto Sans Arabic', 'Noto Sans'"));
}

#[test]
fn the_outputs_have_the_sizes_adr_0054_names() {
    let d = specimens::episode(Layout::Slate);
    let link = render(&d, Output::Link).unwrap();
    let png = Pixmap::decode_png(&link.bytes).unwrap();
    assert_eq!((png.width(), png.height()), (1200, 630));
    let post = render(&d, Output::Post).unwrap();
    let png = Pixmap::decode_png(&post.bytes).unwrap();
    assert_eq!((png.width(), png.height()), (1600, 900));
    let square = render(&d, Output::Square).unwrap();
    let png = Pixmap::decode_png(&square.bytes).unwrap();
    assert_eq!((png.width(), png.height()), (3000, 3000));
    assert!(
        square.bytes.len() < 2 * 1024 * 1024,
        "{} bytes",
        square.bytes.len()
    );
    assert!(link.file_name.starts_with("saturday-night-s3e7-link-"));
    assert!(square.file_name.ends_with(".png"));
}

#[test]
fn a_changed_card_gets_a_new_file_name() {
    let a = render(&specimens::episode(Layout::Poster), Output::Link).unwrap();
    let mut d = specimens::episode(Layout::Poster);
    d.episode.as_mut().unwrap().title = "Ghost Train night, part two".into();
    let b = render(&d, Output::Link).unwrap();
    assert_ne!(a.file_name, b.file_name);
    let again = render(&specimens::episode(Layout::Poster), Output::Link).unwrap();
    assert_eq!(a.file_name, again.file_name, "the same card, the same name");
}

#[test]
fn a_show_name_that_cannot_fit_is_refused_with_the_field_named() {
    let mut d = specimens::long_name();
    d.show.name = "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society, \
                   Extended Edition, With Added Weather For The Inshore Waters And The Coastal Stations"
        .into();
    for layout in [Layout::Poster, Layout::Slate] {
        d.layout = layout;
        assert_eq!(
            render(&d, Output::Link).unwrap_err(),
            CardError::DoesNotFit("show name"),
            "{layout:?}"
        );
    }
}

#[test]
fn many_people_become_and_n_more() {
    let mut d = specimens::five_guests(Layout::Poster);
    d.guests
        .extend(["Isla Murray".to_owned(), "Callum Fraser".to_owned()]);
    let svg = weavesignal_cards::svg(&d, Output::Link).unwrap();
    assert!(
        svg.contains("more"),
        "seven guests: some become \"and N more\""
    );
    render(&d, Output::Link).unwrap();
    d.layout = Layout::Slate;
    let svg = weavesignal_cards::svg(&d, Output::Link).unwrap();
    assert!(svg.contains("and 2 more"));
    render(&d, Output::Link).unwrap();
    render(&d, Output::Square).unwrap();
}

#[test]
fn fields_are_escaped_and_nothing_is_fetched() {
    let mut d = specimens::episode(Layout::Slate);
    d.episode.as_mut().unwrap().title =
        "Fish & \"chips\" <image href=\"file:///etc/passwd\"/>".into();
    d.guests = vec!["<script>alert(1)</script>".into()];
    let svg = weavesignal_cards::svg(&d, Output::Link).unwrap();
    assert!(!svg.contains("<image"));
    assert!(!svg.contains("<script"));
    assert!(svg.contains("&lt;image"));
    assert!(svg.contains("&amp;"));
    assert!(svg.contains("&quot;"));
    render(&d, Output::Link).unwrap();
}

#[test]
fn bad_data_is_refused_before_drawing() {
    let mut d = specimens::show(Layout::Poster);
    d.colour_slot = 13;
    assert!(matches!(
        render(&d, Output::Link),
        Err(CardError::Invalid(_))
    ));
    let mut d = specimens::show(Layout::Poster);
    d.show.slug = "../etc".into();
    assert!(matches!(
        render(&d, Output::Link),
        Err(CardError::Invalid(_))
    ));
}

/// How far two images differ: the share of pixels whose channels differ by more than 8.
fn difference(a: &Pixmap, b: &Pixmap) -> f64 {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let differing = a
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0.iter())
        .filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    differing as f64 / (a.width() * a.height()) as f64
}

#[test]
fn the_specimens_match_their_golden_images() {
    let bless = std::env::var("WEAVESIGNAL_BLESS").is_ok();
    let dir = golden_dir();
    let mut cases: Vec<(String, CardData, Output)> = specimens::all()
        .into_iter()
        .map(|(n, d)| (n.to_owned(), d, Output::Link))
        .collect();
    cases.push((
        "cards.post".into(),
        specimens::episode(Layout::Poster),
        Output::Post,
    ));
    let mut failures = Vec::new();
    for (name, d, o) in cases {
        let r = render(&d, o).unwrap();
        let path = dir.join(format!("{name}.png"));
        if bless {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &r.bytes).unwrap();
            continue;
        }
        let golden = Pixmap::load_png(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; bless the goldens first", path.display()));
        let got = Pixmap::decode_png(&r.bytes).unwrap();
        let diff = difference(&got, &golden);
        if diff > 0.002 {
            failures.push(format!("{name}: {:.3} % of pixels differ", diff * 100.0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_square_matches_its_golden_image_at_a_fifth() {
    // The square is 3,000 pixels; its golden is the same template drawn at 600.
    let d = specimens::episode(Layout::Slate);
    let png = weavesignal_cards::preview(&d, Output::Square, 0.5).unwrap();
    let p = Pixmap::decode_png(&png).unwrap();
    let path = golden_dir().join("cards.square.png");
    if std::env::var("WEAVESIGNAL_BLESS").is_ok() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        std::fs::write(&path, &png).unwrap();
        return;
    }
    let golden = Pixmap::load_png(&path).unwrap();
    assert!(difference(&p, &golden) <= 0.002);
}

#[test]
fn text_the_fonts_cannot_draw_is_refused_with_the_field_named() {
    type Edit = fn(&mut CardData);
    let cases: [(&str, Edit); 5] = [
        ("show name", |d| {
            d.show.name = "Kettle \u{6c49}\u{8bed} Hour".into()
        }),
        ("episode title", |d| {
            d.episode.as_mut().unwrap().title = "\u{05e9}\u{05dc}\u{05d5}\u{05dd} night".into()
        }),
        ("guest name", |d| {
            d.guests[0] = "\u{0928}\u{092e}\u{0938}\u{094d}\u{0924}\u{0947}".into()
        }),
        ("host name", |d| d.hosts[0] = "\u{4f60}\u{597d}".into()),
        ("show description", |d| {
            d.show.description = "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}".into()
        }),
    ];
    for (field, edit) in cases {
        for layout in [Layout::Poster, Layout::Slate] {
            let mut d = specimens::episode(layout);
            edit(&mut d);
            for o in Output::ALL {
                assert_eq!(
                    render(&d, o).unwrap_err(),
                    CardError::MissingGlyphs(field),
                    "{field} {layout:?} {o:?}"
                );
            }
        }
    }
    let msg = CardError::MissingGlyphs("guest name").to_string();
    assert!(
        msg.contains("guest name") && msg.contains("cannot draw"),
        "{msg}"
    );
}

#[test]
fn scripts_the_fonts_do_cover_are_not_refused() {
    // Latin with accents, Greek, Cyrillic and Arabic are bundled.
    let mut d = specimens::episode(Layout::Slate);
    d.hosts = vec![
        "R\u{f9}nach".into(),
        "\u{39d}\u{3af}\u{3ba}\u{3bf}\u{3c2}".into(),
    ];
    d.guests = vec![
        "\u{412}\u{43b}\u{430}\u{434}\u{438}\u{43c}\u{438}\u{440}".into(),
        "\u{646}\u{648}\u{631} \u{627}\u{644}\u{647}\u{62f}\u{649}".into(),
    ];
    for o in Output::ALL {
        render(&d, o).unwrap();
    }
}

// ---- The Poster square (ADR-0179, show-cards.md section 5a) ----

use weavesignal_cards::Ink;

fn overlap(a: &Ink, b: &Ink) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Ink boxes by kind: the words, and the discs with their rings. A disc's own initials are
/// not words: they lie inside it.
fn split(ink: Vec<(String, Ink)>) -> (Vec<Ink>, Vec<Ink>) {
    let mut text = Vec::new();
    let mut discs = Vec::new();
    for (id, i) in ink {
        if id.starts_with("text-") {
            text.push(i);
        } else if id.starts_with("disc-") || id.starts_with("ring-") {
            discs.push(i);
        }
    }
    let inside = |t: &Ink, d: &Ink| {
        t.left >= d.left && t.right <= d.right && t.top >= d.top && t.bottom <= d.bottom
    };
    text.retain(|t| !discs.iter().any(|d| inside(t, d)));
    (text, discs)
}

/// The Poster squares and the three levels of the long-name stress case.
fn square_cases() -> Vec<(String, CardData)> {
    let mut cases: Vec<(String, CardData)> = specimens::poster_squares()
        .into_iter()
        .map(|(n, d)| (n.to_owned(), d))
        .collect();
    cases.push((
        "cards.square.long-name.series".into(),
        specimens::long_name_series(),
    ));
    cases.push((
        "cards.square.long-name.episode".into(),
        specimens::long_name_episode(),
    ));
    cases
}

#[test]
fn a_poster_show_gets_a_poster_square_not_the_slate_one() {
    for (name, d) in square_cases() {
        let svg = weavesignal_cards::svg(&d, Output::Square).unwrap();
        // Poster's plane and the band at 40, not Slate's light surface and its rule.
        assert!(svg.contains("fill=\"#13141c\""), "{name}: the dark plane");
        assert!(
            svg.contains("width=\"40\" height=\"1200\""),
            "{name}: the band is 40 wide and the full height"
        );
        let slate = weavesignal_cards::svg(
            &CardData {
                layout: Layout::Slate,
                ..d.clone()
            },
            Output::Square,
        )
        .unwrap();
        assert_ne!(svg, slate, "{name}: the layout is the show's");
        assert!(
            !slate.contains("fill=\"#13141c\""),
            "{name}: Slate is light"
        );
    }
}

#[test]
fn every_poster_square_is_inside_the_margin_and_stands_on_its_lines() {
    for (name, d) in square_cases() {
        let ink = ink_boxes(&d, Output::Square).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (id, i) in &ink {
            if id.starts_with("decor-") {
                continue;
            }
            assert!(
                i.left >= 40.0 - 0.5
                    && i.top >= 40.0 - 0.5
                    && i.right <= 1160.0 + 0.5
                    && i.bottom <= 1160.0 + 0.5,
                "{name}: {id} at {i:?} is outside the 40 margin"
            );
            // The measure: nothing starts left of x0 (the discs and words stand on it) or
            // runs past x1.
            if id.starts_with("text-") || id.starts_with("disc-") || id.starts_with("ring-") {
                assert!(
                    i.left >= 120.0 - 1.0 && i.right <= 1120.0 + 1.0,
                    "{name}: {id} at {i:?} is outside the measure 120 to 1,120"
                );
            }
        }
        let (text, discs) = split(ink);
        for t in &text {
            for d in &discs {
                assert!(!overlap(t, d), "{name}: words {t:?} over a disc {d:?}");
            }
        }
        // The discs are one row standing on 1,022, left to right from the measure.
        let mut row: Vec<&Ink> = discs.iter().collect();
        row.sort_by(|a, b| a.left.total_cmp(&b.left));
        for r in &row {
            assert!(
                (r.bottom - 1022.0).abs() < 1.0,
                "{name}: a disc stands on 1,022: {r:?}"
            );
        }
        if let Some(first) = row.first() {
            assert!(
                (first.left - 120.0).abs() < 1.0,
                "{name}: the row starts at x0"
            );
        }
        // The foot is under the rule and the discs are above it.
        assert!(
            text.iter().any(|t| t.top >= 1046.0),
            "{name}: the foot's words are below the rule"
        );
    }
}

#[test]
fn the_poster_squares_take_the_sizes_the_design_measures() {
    let disc_sizes = |d: &CardData| -> Vec<u32> {
        let (_, mut discs) = split(ink_boxes(d, Output::Square).unwrap());
        discs.sort_by(|a, b| a.left.total_cmp(&b.left));
        // Rings and discs share a box: keep one box per person.
        let mut sizes: Vec<u32> = discs
            .iter()
            .map(|i| (i.right - i.left).round() as u32)
            .collect();
        sizes.dedup();
        sizes
    };
    // Two hosts and two guests: hosts at 160, guests at 128.
    assert_eq!(disc_sizes(&specimens::episode(Layout::Poster)), [160, 128]);
    // Seven people: the guests drop to 96 so that one row holds all of them.
    assert_eq!(
        disc_sizes(&specimens::five_guests(Layout::Poster)),
        [160, 96]
    );
    // The show's hosts alone.
    assert_eq!(disc_sizes(&specimens::show(Layout::Poster)), [160]);
    // The name is as large as the measure allows: the show name takes 136 in two lines.
    let (text, _) = split(ink_boxes(&specimens::show(Layout::Poster), Output::Square).unwrap());
    let big: Vec<&Ink> = text.iter().filter(|t| t.bottom - t.top > 120.0).collect();
    assert_eq!(big.len(), 2, "the name takes two lines at the top size");
}

#[test]
fn the_poster_square_refuses_a_name_that_cannot_fit_with_the_field_named() {
    let mut d = specimens::long_name();
    d.show.name = "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society, \
                   Extended Edition, With Added Weather For The Inshore Waters And The Coastal Stations, \
                   And A Great Many More Words Besides, Read Slowly Over The Course Of A Long Night"
        .into();
    assert_eq!(
        render(&d, Output::Square).unwrap_err(),
        CardError::DoesNotFit("show name")
    );
}

#[test]
fn many_people_on_the_poster_square_become_and_n_more() {
    let mut d = specimens::five_guests(Layout::Poster);
    d.guests
        .extend(["Isla Murray".to_owned(), "Callum Fraser".to_owned()]);
    d.hosts.extend([
        "Hamish Wallace".to_owned(),
        "Morag Fraser".to_owned(),
        "Iain Stewart".to_owned(),
    ]);
    let svg = weavesignal_cards::svg(&d, Output::Square).unwrap();
    assert!(
        svg.contains("more"),
        "five hosts and seven guests: some are \"and N more\""
    );
    let ink = ink_boxes(&d, Output::Square).unwrap();
    let (text, discs) = split(ink);
    for t in &text {
        for d in &discs {
            assert!(!overlap(t, d));
        }
    }
    render(&d, Output::Square).unwrap();
}

#[test]
fn the_poster_squares_match_their_golden_images_at_a_fifth() {
    let bless = std::env::var("WEAVESIGNAL_BLESS").is_ok();
    let mut failures = Vec::new();
    for (name, d) in square_cases() {
        // The goldens are the template drawn at 600 pixels.
        let png = weavesignal_cards::preview(&d, Output::Square, 0.5).unwrap();
        let path = golden_dir().join(format!("{name}.png"));
        if bless {
            std::fs::write(&path, &png).unwrap();
            continue;
        }
        let golden = Pixmap::load_png(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; bless the goldens first", path.display()));
        let diff = difference(&Pixmap::decode_png(&png).unwrap(), &golden);
        if diff > 0.002 {
            failures.push(format!("{name}: {:.3} % of pixels differ", diff * 100.0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- The Poster link card and its disc column (the operator's report, 2026-11-02) ----

/// The disc column's left edge on the link card and the post image: the measure ends at
/// 1,128, the discs are 104 across.
const DISC_COLUMN_LEFT: f32 = 1128.0 - 104.0;

/// No word of the name, the level line or the people lines reaches the discs, and every word
/// and disc is inside the margin.
fn assert_clear_of_the_discs(label: &str, d: &CardData, o: Output) {
    let ink = ink_boxes(d, o).unwrap_or_else(|e| panic!("{label} {o:?}: {e}"));
    let (l, t, r, b) = o.margin();
    for (id, i) in &ink {
        if id.starts_with("decor-") {
            continue;
        }
        assert!(
            i.left >= l - 0.5 && i.top >= t - 0.5 && i.right <= r + 0.5 && i.bottom <= b + 0.5,
            "{label} {o:?}: {id} at {i:?} leaves the safe area {:?}",
            (l, t, r, b)
        );
    }
    let (text, discs) = split(ink);
    assert!(!discs.is_empty(), "{label} {o:?}: the discs are drawn");
    let column_top = discs.iter().map(|d| d.top).fold(f32::MAX, f32::min);
    let column_bottom = discs.iter().map(|d| d.bottom).fold(0.0, f32::max);
    for d in &discs {
        assert!(
            d.left >= DISC_COLUMN_LEFT - 0.5,
            "{label}: the discs are one column"
        );
    }
    for w in &text {
        for d in &discs {
            assert!(
                !overlap(w, d),
                "{label} {o:?}: words {w:?} overlap a disc {d:?}"
            );
        }
        // Whatever lies beside the column's height ends before the column starts.
        if w.bottom > column_top && w.top < column_bottom {
            assert!(
                w.right <= DISC_COLUMN_LEFT,
                "{label} {o:?}: words {w:?} run under the disc column at {DISC_COLUMN_LEFT}"
            );
        }
    }
}

#[test]
fn the_poster_link_card_keeps_a_long_title_clear_of_the_guest_discs() {
    // Exactly the episode the operator saw clipped on the design page: "Saturday Night Kettle",
    // Series 3 Episode 7 "Ghost Train night", two hosts and two guests.
    let d = specimens::episode(Layout::Poster);
    assert_eq!(d.show.name, "Saturday Night Kettle");
    assert_eq!(d.episode.as_ref().unwrap().title, "Ghost Train night");
    assert_eq!(d.guests, ["Firth Lights", "Dr Una Robertson"]);
    for o in [Output::Link, Output::Post] {
        assert_clear_of_the_discs("the operator's episode", &d, o);
        // The name is the two big lines: they stop well short of the discs too.
        let (text, _) = split(ink_boxes(&d, o).unwrap());
        let name: Vec<&Ink> = text.iter().filter(|t| t.bottom - t.top > 80.0).collect();
        assert_eq!(name.len(), 2, "{o:?}: the name takes two lines");
        for n in name {
            assert!(
                n.right <= DISC_COLUMN_LEFT - 40.0 + 0.5,
                "{o:?}: the name {n:?} keeps the 40 units between text and discs"
            );
        }
    }
}

#[test]
fn the_poster_link_card_fits_longer_names_and_titles_beside_the_discs() {
    // The measure is the text column, not the card: a longer name shrinks or wraps instead of
    // running under the discs, at every level.
    let names = [
        "Saturday Night Kettle",
        "Saturday Night Kettle Extra",
        "The Saturday Night Kettle Late Edition",
        "Saturday Night Kettle: The Ghost Train Years",
        "Extraordinarily Long Saturday Night Kettle Appreciation Hour",
        "The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society",
    ];
    let titles = [
        "Ghost Train night",
        "Ghost Train night, and the long way home from Kirkcaldy by the coast",
    ];
    for name in names {
        for title in titles {
            let mut d = specimens::episode(Layout::Poster);
            d.show.name = name.into();
            d.episode.as_mut().unwrap().title = title.into();
            for o in [Output::Link, Output::Post] {
                assert_clear_of_the_discs(&format!("{name} / {title}"), &d, o);
            }
        }
        for level in [
            specimens::show(Layout::Poster),
            specimens::series(Layout::Poster),
        ] {
            let mut d = level;
            d.show.name = name.into();
            d.guests = vec!["Firth Lights".into()];
            for o in [Output::Link, Output::Post] {
                assert_clear_of_the_discs(name, &d, o);
            }
        }
    }
}
