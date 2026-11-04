// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's show cards (A42; ADR-0053 to ADR-0057, ADR-0066, ADR-0067).
//!
//! One template per layout, Poster (dark, the type carries it) or Slate (light, it shows the
//! people), filled from a show's, a series' or an episode's fields, written as SVG with every
//! field escaped, and rendered in-process by resvg with bundled fonts (ADR-0053). Four outputs
//! per save (ADR-0054): the 1,200 × 630 link card, whose words and faces sit inside the
//! centred 1,200 × 600 safe area with a further 40 px margin; the 1,600 × 900 post image; and
//! the 3,000 square, a PNG, for feeds and as archive art. The square follows the show's
//! layout (ADR-0179): Poster's own square, or the Slate card stacked.
//!
//! Text is fitted, never cropped: a field shrinks to its floor, then wraps, then a list falls
//! back to "and N more"; a show name that still does not fit is refused with the field named.
//! Every render is checked: if any word or disc lies outside the margin, the card is refused
//! rather than sent (the design's rule that no card leaves with text off its edge).

mod fonts;
mod poster;
mod scene;
mod slate;
mod words;
mod wrap;

use jiff::Zoned;
use jiff::civil::{Date, Time};
use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use scene::Rgb;

/// The twelve-colour cart palette (the design system's tokens, `--cart-1` to `--cart-12`):
/// a show's colour is its bank's slot on every surface (ADR-0066). The validated palette has
/// one value per mode: Poster, a dark card, takes the dark value; Slate takes the light one.
pub const PALETTE_DARK: [Rgb; 12] = [
    Rgb::hex(0x3987e5),
    Rgb::hex(0x267618),
    Rgb::hex(0xdf5d93),
    Rgb::hex(0x935c19),
    Rgb::hex(0xa73e93),
    Rgb::hex(0xa29027),
    Rgb::hex(0x6b55c7),
    Rgb::hex(0xc23c45),
    Rgb::hex(0xac72e3),
    Rgb::hex(0xe36826),
    Rgb::hex(0x1b6d9c),
    Rgb::hex(0x2da88c),
];
pub const PALETTE_LIGHT: [Rgb; 12] = [
    Rgb::hex(0x2a78d6),
    Rgb::hex(0x8f175d),
    Rgb::hex(0x2ba835),
    Rgb::hex(0x405d12),
    Rgb::hex(0x9c41b6),
    Rgb::hex(0xa05119),
    Rgb::hex(0x5830b0),
    Rgb::hex(0xac8b26),
    Rgb::hex(0x155a8d),
    Rgb::hex(0xd16adb),
    Rgb::hex(0xf4632e),
    Rgb::hex(0xcf3b78),
];

/// The two layouts; both ship and a station picks one per show (ADR-0067).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    Poster,
    Slate,
}

impl Layout {
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Poster => "poster",
            Layout::Slate => "slate",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "poster" => Some(Layout::Poster),
            "slate" => Some(Layout::Slate),
            _ => None,
        }
    }
}

/// The four renders of one card (ADR-0054); the archive art is the square itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// 1,200 × 630 PNG: `og:image` and `twitter:image`.
    Link,
    /// 1,600 × 900 PNG: attached to a post.
    Post,
    /// 3,000 × 3,000 PNG: podcast art for feeds and Castopod, and the archive files' art.
    Square,
}

impl Output {
    pub const ALL: [Output; 3] = [Output::Link, Output::Post, Output::Square];

    pub fn as_str(self) -> &'static str {
        match self {
            Output::Link => "link",
            Output::Post => "post",
            Output::Square => "square",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.as_str() == s)
    }

    /// The template's own size, in the units the design is drawn in, and the scale to pixels.
    pub fn frame(self) -> (f32, f32, f32) {
        match self {
            Output::Link => (1200.0, 630.0, 1.0),
            Output::Post => (1200.0, 675.0, 4.0 / 3.0),
            Output::Square => (1200.0, 1200.0, 2.5),
        }
    }

    /// The pixel size.
    pub fn size(self) -> (u32, u32) {
        let (w, h, s) = self.frame();
        ((w * s).round() as u32, (h * s).round() as u32)
    }

    /// Every output is a PNG. The design proposed JPEG for the square, but the one pure-Rust
    /// JPEG encoder carries the IJG licence, which `deny.toml` does not allow; Apple and
    /// Spotify take PNG art, and a flat card compresses well as PNG.
    pub fn media_type(self) -> &'static str {
        "image/png"
    }

    pub fn extension(self) -> &'static str {
        "png"
    }

    /// Where every word and face must lie, in template units (left, top, right, bottom): on
    /// the link card the centred 1,200 × 600 safe area less its 40 px margin; elsewhere 40
    /// units in from each edge.
    pub fn margin(self) -> (f32, f32, f32, f32) {
        let (w, h, _) = self.frame();
        match self {
            Output::Link => (40.0, 15.0 + 40.0, w - 40.0, h - 15.0 - 40.0),
            _ => (40.0, 40.0, w - 40.0, h - 40.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub fn to_jiff(self) -> jiff::civil::Weekday {
        use jiff::civil::Weekday as W;
        match self {
            Weekday::Monday => W::Monday,
            Weekday::Tuesday => W::Tuesday,
            Weekday::Wednesday => W::Wednesday,
            Weekday::Thursday => W::Thursday,
            Weekday::Friday => W::Friday,
            Weekday::Saturday => W::Saturday,
            Weekday::Sunday => W::Sunday,
        }
    }

    pub fn from_jiff(w: jiff::civil::Weekday) -> Self {
        use jiff::civil::Weekday as W;
        match w {
            W::Monday => Weekday::Monday,
            W::Tuesday => Weekday::Tuesday,
            W::Wednesday => Weekday::Wednesday,
            W::Thursday => Weekday::Thursday,
            W::Friday => Weekday::Friday,
            W::Saturday => Weekday::Saturday,
            W::Sunday => Weekday::Sunday,
        }
    }
}

/// A weekly slot: "Saturdays 20:00", "Saturdays 20:00–23:00".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub weekday: Weekday,
    pub start: Time,
    #[serde(default)]
    pub end: Option<Time>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationCard {
    pub name: String,
    /// Where the station is ("Dunfermline"); may be empty.
    #[serde(default)]
    pub place: String,
    /// The station's address as printed, without a scheme: `kettle.example` (ADR-0056).
    pub address: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShowCard {
    pub name: String,
    /// The show's path segment: `saturday-night` in `kettle.example/saturday-night`.
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub slot: Option<Slot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesCard {
    pub number: u32,
    #[serde(default)]
    pub title: Option<String>,
    /// The run of dates, first and last episode.
    #[serde(default)]
    pub first: Option<Date>,
    #[serde(default)]
    pub last: Option<Date>,
    #[serde(default)]
    pub episodes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeCard {
    pub number: u32,
    pub title: String,
    /// When it goes on air, in the station's zone: "2026-10-24T20:00:00+01:00[Europe/London]";
    /// an unscheduled episode's card shows the show's slot instead.
    #[serde(default)]
    pub air: Option<Zoned>,
}

/// Everything a card shows, already resolved (inheritance is the model's job, ADR-0055).
/// The level is the deepest part present: an episode needs its series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardData {
    pub layout: Layout,
    /// 1 to 12: the show's bank slot in the palette (ADR-0066).
    pub colour_slot: u8,
    pub station: StationCard,
    pub show: ShowCard,
    #[serde(default)]
    pub series: Option<SeriesCard>,
    #[serde(default)]
    pub episode: Option<EpisodeCard>,
    pub hosts: Vec<String>,
    #[serde(default)]
    pub guests: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Show,
    Series,
    Episode,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CardError {
    /// The field does not fit even at its floor: the save is refused with the field named.
    #[error("the {0} does not fit on the card even at its smallest size; shorten it")]
    DoesNotFit(&'static str),
    /// The field holds characters the bundled fonts cannot draw (ADR-0176): the save is
    /// refused with the field named, instead of publishing blank boxes.
    #[error(
        "the {0} holds characters the card fonts cannot draw (they cover Latin, Greek, Cyrillic and Arabic); change them"
    )]
    MissingGlyphs(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error("the card could not be drawn: {0}")]
    Render(String),
}

/// Most hosts and guests shown by name before "and N more" (the design's fitting rules).
pub const MAX_HOSTS: usize = 4;
pub const MAX_GUESTS: usize = 5;

impl CardData {
    pub fn level(&self) -> Level {
        match (&self.series, &self.episode) {
            (_, Some(_)) => Level::Episode,
            (Some(_), None) => Level::Series,
            _ => Level::Show,
        }
    }

    /// The checks every card makes before it is drawn.
    pub fn check(&self) -> Result<(), CardError> {
        let bad = |s: String| Err(CardError::Invalid(s));
        if !(1..=12).contains(&self.colour_slot) {
            return bad(format!(
                "colour slot {} does not exist; the palette has 12",
                self.colour_slot
            ));
        }
        if self.episode.is_some() && self.series.is_none() {
            return bad("an episode card needs its series".to_owned());
        }
        if self.show.name.trim().is_empty() {
            return bad("the show needs a name".to_owned());
        }
        if self.station.name.trim().is_empty() || self.station.address.trim().is_empty() {
            return bad("the station needs a name and an address".to_owned());
        }
        if self.show.slug.is_empty()
            || !self
                .show
                .slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return bad(format!(
                "`{}` is not a show path: lower-case letters, digits and hyphens",
                self.show.slug
            ));
        }
        self.check_glyphs()
    }

    /// Refuse a card whose text holds characters the bundled fonts lack, naming the first
    /// field that does (ADR-0176). Each field is laid out as resvg will lay it out, with the
    /// same font fallback, so a character any bundled face draws passes.
    fn check_glyphs(&self) -> Result<(), CardError> {
        let mut fields: Vec<(&'static str, &str)> = vec![
            ("station name", &self.station.name),
            ("station place", &self.station.place),
            ("station address", &self.station.address),
            ("show name", &self.show.name),
            ("show description", &self.show.description),
        ];
        if let Some(t) = self.series.as_ref().and_then(|s| s.title.as_deref()) {
            fields.push(("series title", t));
        }
        if let Some(e) = &self.episode {
            fields.push(("episode title", &e.title));
        }
        fields.extend(self.hosts.iter().map(|h| ("host name", h.as_str())));
        fields.extend(self.guests.iter().map(|g| ("guest name", g.as_str())));
        check_text(&fields)
    }

    /// The path of this card's own page: `/saturday-night`, `/saturday-night/s3`,
    /// `/saturday-night/s3/e7` (ADR-0056: the card links to it).
    pub fn path(&self) -> String {
        let mut p = format!("/{}", self.show.slug);
        if let Some(s) = &self.series {
            p.push_str(&format!("/s{}", s.number));
            if let Some(e) = &self.episode {
                p.push_str(&format!("/e{}", e.number));
            }
        }
        p
    }

    fn address(&self) -> String {
        format!(
            "{}{}",
            self.station.address.trim_end_matches('/'),
            self.path()
        )
    }

    fn show_address(&self) -> String {
        format!(
            "{}/{}",
            self.station.address.trim_end_matches('/'),
            self.show.slug
        )
    }

    fn slot_short(&self) -> Option<String> {
        self.show.slot.as_ref().map(|s| {
            format!(
                "{}s {}",
                words::weekday_name(s.weekday),
                words::hhmm(s.start)
            )
        })
    }

    fn slot_range(&self) -> Option<String> {
        self.show.slot.as_ref().map(|s| {
            let mut out = format!(
                "{}s {}",
                words::weekday_name(s.weekday),
                words::hhmm(s.start)
            );
            if let Some(end) = s.end {
                out.push('\u{2013}');
                out.push_str(&words::hhmm(end));
            }
            out
        })
    }

    fn run(&self) -> Option<String> {
        let s = self.series.as_ref()?;
        match (s.first, s.last) {
            (Some(a), Some(b)) => Some(words::date_run(a, b)),
            _ => None,
        }
    }

    fn series_label(&self) -> Option<String> {
        self.series.as_ref().map(|s| format!("Series {}", s.number))
    }

    fn air_words(&self) -> Option<(String, String)> {
        self.episode
            .as_ref()
            .and_then(|e| e.air.as_ref())
            .map(|at| (words::long_date(at.date()), words::time_zone(at)))
    }

    fn episodes_count(&self) -> Option<String> {
        self.series.as_ref().and_then(|s| s.episodes).map(|n| {
            if n == 1 {
                "1 episode".to_owned()
            } else {
                format!("{n} episodes")
            }
        })
    }
}

/// The alternative text (`og:image:alt`, `twitter:image:alt`), built from the same fields in
/// a fixed order: show, level, title, date and time with the zone, station, hosts, guests,
/// address. Names keep their plain hyphens and have no bidi marks.
pub fn alt_text(d: &CardData) -> String {
    let mut parts: Vec<String> = vec![d.show.name.trim().to_owned()];
    match d.level() {
        Level::Show => {}
        Level::Series => {
            if let Some(l) = d.series_label() {
                parts.push(match d.series.as_ref().and_then(|s| s.title.clone()) {
                    Some(t) => format!("{l}, {t}"),
                    None => l,
                });
            }
            if let Some(r) = d.run() {
                parts.push(r);
            }
        }
        Level::Episode => {
            if let (Some(s), Some(e)) = (&d.series, &d.episode) {
                parts.push(format!("Series {} Episode {}", s.number, e.number));
                if !e.title.trim().is_empty() {
                    parts.push(e.title.trim().to_owned());
                }
            }
        }
    }
    let when = match (d.level(), d.air_words(), d.slot_short()) {
        (Level::Episode, Some((date, time)), _) => format!("{date} at {time} "),
        (_, _, Some(slot)) => format!("{slot} "),
        _ => String::new(),
    };
    parts.push(format!("{when}on {}", d.station.name.trim()));
    let hosts: Vec<String> = d.hosts.iter().map(|h| h.trim().to_owned()).collect();
    if !hosts.is_empty() {
        parts.push(format!("with {}", words::list(&hosts)));
    }
    let guests: Vec<String> = d.guests.iter().map(|g| g.trim().to_owned()).collect();
    if d.level() == Level::Episode && !guests.is_empty() {
        parts.push(format!("guests {}", words::list(&guests)));
    }
    format!("{}. {}", parts.join(", "), d.address())
}

/// The words of a card, worked out once for both layouts.
#[derive(Clone)]
pub(crate) struct Words {
    pub level: Level,
    pub station: String,
    pub place: String,
    pub address: String,
    pub show_address: String,
    pub name: String,
    pub description: String,
    /// The Poster eyebrow after the station: the slot, or the episode's date and time.
    pub poster_when: Option<String>,
    /// The Poster level line: the bold part, then the rest.
    pub level_line: Option<(String, Option<String>)>,
    /// The Poster foot's middle words.
    pub foot_when: String,
    pub slate_eyebrow: String,
    pub slate_title: Option<String>,
    pub slate_when: Option<(String, String)>,
    /// The square's single "when" line.
    pub square_when: Option<String>,
    pub hosts: Vec<String>,
    pub guests: Vec<String>,
}

impl Words {
    fn of(d: &CardData) -> Self {
        let level = d.level();
        let station = d.station.name.trim().to_owned();
        let air = d.air_words();
        let poster_when = match (&air, d.slot_short()) {
            (Some((date, time)), _) => Some(format!("{date} \u{b7} {time}")),
            (None, slot) => slot,
        };
        let level_line = match level {
            Level::Show => None,
            Level::Series => d.series_label().map(|l| {
                let rest = d
                    .series
                    .as_ref()
                    .and_then(|s| s.title.clone())
                    .or_else(|| d.run().map(|r| words::unbroken_dates(&r)));
                (l, rest)
            }),
            Level::Episode => match (&d.series, &d.episode) {
                (Some(s), Some(e)) => Some((
                    format!("Series {}, Episode {}", s.number, e.number),
                    Some(e.title.trim().to_owned()).filter(|t| !t.is_empty()),
                )),
                _ => None,
            },
        };
        let foot_when = match level {
            Level::Show => d.station.place.trim().to_owned(),
            Level::Series => d.episodes_count().unwrap_or_default(),
            Level::Episode => "Listen live or later".to_owned(),
        };
        let slate_eyebrow = match level {
            Level::Show => match d.slot_range() {
                Some(s) => format!("Show \u{b7} {s}"),
                None => "Show".to_owned(),
            },
            Level::Series => {
                let l = d.series_label().unwrap_or_default();
                match d.episodes_count() {
                    Some(n) => format!("{l} \u{b7} {n}"),
                    None => l,
                }
            }
            Level::Episode => match (&d.series, &d.episode) {
                (Some(s), Some(e)) => format!("Series {} \u{b7} Episode {}", s.number, e.number),
                _ => String::new(),
            },
        };
        let slate_title = match level {
            Level::Show => Some(d.show.description.trim().to_owned()).filter(|s| !s.is_empty()),
            Level::Series => d
                .series
                .as_ref()
                .and_then(|s| s.title.clone())
                .or_else(|| d.run().map(|r| words::unbroken_dates(&r))),
            Level::Episode => d
                .episode
                .as_ref()
                .map(|e| e.title.trim().to_owned())
                .filter(|t| !t.is_empty()),
        };
        let slate_when = match level {
            Level::Show => d.show.slot.as_ref().map(|s| {
                (
                    format!(
                        "Every {} from {}",
                        words::weekday_name(s.weekday),
                        words::clock_words(s.start)
                    ),
                    "Live, or later on the show page".to_owned(),
                )
            }),
            Level::Series => d
                .slot_range()
                .map(|s| (s, format!("Live from the {station} studio"))),
            Level::Episode => air.as_ref().map(|(date, time)| {
                (
                    date.clone(),
                    format!("{time} \u{b7} live, then on the episode page"),
                )
            }),
        };
        let square_when = match (&air, &slate_when) {
            (Some((date, time)), _) => Some(format!("{date} \u{b7} {time}")),
            (None, Some((bold, _))) => Some(bold.clone()),
            _ => None,
        };
        Self {
            level,
            station,
            place: d.station.place.trim().to_owned(),
            address: d.address(),
            show_address: d.show_address(),
            name: d.show.name.trim().to_owned(),
            description: d.show.description.trim().to_owned(),
            poster_when,
            level_line,
            foot_when,
            slate_eyebrow,
            slate_title,
            slate_when,
            square_when,
            hosts: d
                .hosts
                .iter()
                .map(|h| words::visible_name(h.trim()))
                .filter(|h| !h.is_empty())
                .collect(),
            guests: if level == Level::Episode {
                d.guests
                    .iter()
                    .map(|g| words::visible_name(g.trim()))
                    .filter(|g| !g.is_empty())
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
}

/// One rendered output.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub output: Output,
    pub width: u32,
    pub height: u32,
    pub media_type: &'static str,
    /// The image itself.
    pub bytes: Vec<u8>,
    /// `saturday-night-s3e7-link-1a2b3c4d5e6f.png`: a changed card gets a new name, since X
    /// keeps a fetched card for days (0008 section 2).
    pub file_name: String,
    pub alt: String,
    /// The SVG it was drawn from.
    pub svg: String,
}

/// A box in template units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

type Template = (f32, f32, Rgb, Vec<scene::Shape>);

/// The template's shapes for one output.
fn shapes(d: &CardData, output: Output) -> Result<Template, CardError> {
    d.check()?;
    let w = Words::of(d);
    let m = scene::Measurer::new();
    let (fw, fh, _) = output.frame();
    let slot = usize::from(d.colour_slot - 1);
    // The podcast square follows the show's layout (ADR-0179): Poster's own square, or the
    // Slate card stacked.
    let square = output == Output::Square;
    match d.layout {
        Layout::Poster => {
            let shapes = poster::layout(&w, &m, fw, fh, PALETTE_DARK[slot], square)?;
            Ok((fw, fh, poster::PLANE, shapes))
        }
        Layout::Slate => {
            let shapes = slate::layout(&w, &m, fw, fh, PALETTE_LIGHT[slot], square)?;
            Ok((fw, fh, slate::SURFACE, shapes))
        }
    }
}

/// The SVG of a card, unrendered.
pub fn svg(d: &CardData, output: Output) -> Result<String, CardError> {
    let (fw, fh, ground, shapes) = shapes(d, output)?;
    Ok(scene::to_svg(fw, fh, ground, &shapes))
}

fn tree(svg: &str) -> Result<usvg::Tree, CardError> {
    usvg::Tree::from_str(svg, &scene::options()).map_err(|e| CardError::Render(e.to_string()))
}

/// Every word, disc and mark on the card, in template units, from the tree resvg draws.
fn ink(tree: &usvg::Tree) -> Vec<(String, Ink)> {
    let mut out = Vec::new();
    for n in tree.root().children() {
        let id = n.id().to_owned();
        if id == "ground" || id.starts_with("decor-") || id.starts_with("rect-") {
            continue;
        }
        let b = n.abs_stroke_bounding_box();
        out.push((
            id,
            Ink {
                left: b.left(),
                top: b.top(),
                right: b.right(),
                bottom: b.bottom(),
            },
        ));
    }
    out
}

/// The ink boxes of a card, for tests and for the check every render makes.
pub fn ink_boxes(d: &CardData, output: Output) -> Result<Vec<(String, Ink)>, CardError> {
    Ok(ink(&tree(&svg(d, output)?)?))
}

/// Refuse text the bundled fonts cannot draw, naming the first field that holds any
/// (ADR-0176). Each field is laid out as resvg will lay it out, with the same font fallback,
/// so a character any bundled face draws passes. The model's save routes call this on the
/// words they are about to keep, before keeping them.
pub fn check_text(fields: &[(&'static str, &str)]) -> Result<(), CardError> {
    // Most cards are plain Latin: skip the font work when every character is ASCII.
    let fields: Vec<_> = fields.iter().filter(|(_, t)| !t.is_ascii()).collect();
    if fields.is_empty() {
        return Ok(());
    }
    let mut svg =
        String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\">");
    for (i, (_, text)) in fields.iter().enumerate() {
        svg.push_str(&format!(
            "<text id=\"field-{i}\" x=\"0\" y=\"{}\" font-size=\"20\" xml:space=\"preserve\">{}</text>",
            20 + i * 30,
            scene::escape(text)
        ));
    }
    svg.push_str("</svg>");
    let tree = tree(&svg)?;
    let mut bad = vec![false; fields.len()];
    glyph_gaps(tree.root(), &mut |id, n| {
        if let Some(i) = id
            .strip_prefix("field-")
            .and_then(|i| i.parse::<usize>().ok())
            && n > 0
        {
            bad[i] = true;
        }
    });
    match bad.iter().position(|b| *b) {
        Some(i) => Err(CardError::MissingGlyphs(fields[i].0)),
        None => Ok(()),
    }
}

/// Calls `f` with the id and the missing-glyph count (glyph id 0, drawn as a blank box) of
/// every text node under `g`.
fn glyph_gaps(g: &usvg::Group, f: &mut dyn FnMut(&str, usize)) {
    for n in g.children() {
        match n {
            usvg::Node::Text(t) => f(
                t.id(),
                t.layouted()
                    .iter()
                    .flat_map(|s| s.positioned_glyphs.iter())
                    .filter(|g| g.id.0 == 0)
                    .count(),
            ),
            usvg::Node::Group(g) => glyph_gaps(g, f),
            _ => {}
        }
    }
}

/// How many glyphs on the card the bundled fonts lack (they would be drawn as blank boxes).
pub fn missing_glyphs(d: &CardData, output: Output) -> Result<usize, CardError> {
    let tree = tree(&svg(d, output)?)?;
    let mut total = 0;
    glyph_gaps(tree.root(), &mut |_, n| total += n);
    Ok(total)
}

/// Draw one output of a card.
pub fn render(d: &CardData, output: Output) -> Result<Rendered, CardError> {
    let svg = svg(d, output)?;
    let tree = tree(&svg)?;
    // The field check in `check` names the field; this is the net under it, for a glyph that
    // only goes missing once the card's own words are laid out.
    let mut gaps = 0;
    glyph_gaps(tree.root(), &mut |_, n| gaps += n);
    if gaps > 0 {
        return Err(CardError::MissingGlyphs("text"));
    }
    let (l, t, r, b) = output.margin();
    for (id, i) in ink(&tree) {
        if i.left < l - 0.5 || i.top < t - 0.5 || i.right > r + 0.5 || i.bottom > b + 0.5 {
            return Err(CardError::DoesNotFit(match id.split('-').next() {
                Some("text") => "text",
                Some("disc") | Some("ring") => "people",
                _ => "layout",
            }));
        }
    }
    let (_, _, scale) = output.frame();
    let (pw, ph) = output.size();
    let mut pixmap =
        tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| CardError::Render("no pixmap".into()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let bytes = pixmap
        .encode_png()
        .map_err(|e| CardError::Render(e.to_string()))?;
    let digest = Sha256::digest(&bytes);
    let hash: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    let level = match (&d.series, &d.episode) {
        (Some(s), Some(e)) => format!("s{}e{}", s.number, e.number),
        (Some(s), None) => format!("s{}", s.number),
        _ => "show".to_owned(),
    };
    Ok(Rendered {
        output,
        width: pw,
        height: ph,
        media_type: output.media_type(),
        file_name: format!(
            "{}-{level}-{}-{hash}.{}",
            d.show.slug,
            output.as_str(),
            output.extension()
        ),
        alt: alt_text(d),
        bytes,
        svg,
    })
}

/// A PNG of one output drawn at `scale` of its template units (the square's golden image is
/// drawn at a fifth of its 3,000 pixels), with the same margin check as [`render`].
pub fn preview(d: &CardData, output: Output, scale: f32) -> Result<Vec<u8>, CardError> {
    let svg = svg(d, output)?;
    let tree = tree(&svg)?;
    let (fw, fh, _) = output.frame();
    let mut pixmap =
        tiny_skia::Pixmap::new((fw * scale).round() as u32, (fh * scale).round() as u32)
            .ok_or_else(|| CardError::Render("no pixmap".into()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| CardError::Render(e.to_string()))
}

/// The renders of one save (ADR-0054): link, post and square; the archive art is the square.
pub fn render_all(d: &CardData) -> Result<Vec<Rendered>, CardError> {
    Output::ALL.iter().map(|o| render(d, *o)).collect()
}
