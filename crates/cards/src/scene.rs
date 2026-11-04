// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A card as a list of shapes and text lines in the template's own units, written out as SVG
//! for resvg, and the measurer that tells the layouts how wide a line of text will be.
//!
//! Every field reaches the SVG through [`escape`]; nothing in a card refers to a file or an
//! address, and the parser's image resolvers refuse everything (ADR-0053, D48).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use resvg::usvg;

/// A colour as `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub(crate) fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// The families bundled with the crate (see `fonts/`, OFL-1.1, recorded in NOTICES.md).
pub(crate) const SANS: &str = "'Noto Sans', 'Noto Sans Arabic'";
pub(crate) const MONO: &str = "'Noto Sans Mono', 'Noto Sans', 'Noto Sans Arabic'";
pub(crate) const ARABIC_FIRST: &str = "'Noto Sans Arabic', 'Noto Sans'";

/// Noto Sans's ascender and descender in ems (hhea 1069 and 293 of 1000): a CSS line box puts
/// half the leading above the content area, so the baseline sits at
/// `top + (line_height - 1.362 em) / 2 + 1.069 em`.
pub(crate) const ASCENT: f32 = 1.069;
pub(crate) const DESCENT: f32 = 0.293;

/// The baseline of a line of text whose line box starts at `top`.
pub(crate) fn baseline(top: f32, size: f32, line_height: f32) -> f32 {
    top + (line_height * size - (ASCENT + DESCENT) * size) / 2.0 + ASCENT * size
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Style {
    pub size: f32,
    pub weight: u16,
    pub fill: Rgb,
    pub mono: bool,
    /// Letter spacing in ems.
    pub tracking: f32,
}

impl Style {
    pub fn new(size: f32, weight: u16, fill: Rgb) -> Self {
        Self {
            size,
            weight,
            fill,
            mono: false,
            tracking: 0.0,
        }
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn tracking(mut self, em: f32) -> Self {
        self.tracking = em;
        self
    }

    pub fn sized(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

/// A piece of a line in one style.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub text: String,
    pub style: Style,
}

impl Run {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug)]
pub(crate) enum Shape {
    /// `decor` marks the show's colour band or rule, the one thing allowed outside the margin.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Rgb,
        decor: bool,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Rgb,
        ring: Option<(Rgb, f32)>,
    },
    Text {
        x: f32,
        baseline: f32,
        anchor: Anchor,
        runs: Vec<Run>,
    },
    /// The station's mark, Boxicons' `broadcast` (ADR-0094), `size` units square.
    Mark {
        x: f32,
        y: f32,
        size: f32,
        fill: Rgb,
    },
}

/// Boxicons `broadcast` (boxicons-core, MIT, Copyright (c) 2026 Boxicons), as kept at
/// `Library/design/assets/weavesignal-icon-broadcast.svg` and `packages/ui/icon/`.
const BROADCAST: [&str; 2] = [
    "M12 8c-1.1 0-2 .9-2 2c0 .85.53 1.57 1.28 1.86l-.53 10.13h2.5l-.53-10.13C13.47 11.57 14 10.85 14 10c0-1.1-.9-2-2-2m-2.83-.83L7.76 5.75A5.97 5.97 0 0 0 6 9.99c0 1.6.62 3.11 1.76 4.25l1.41-1.41A3.96 3.96 0 0 1 8 10c0-1.07.42-2.07 1.17-2.82Zm7.07-1.41l-1.41 1.41C15.59 7.93 16 8.93 16 10s-.42 2.07-1.17 2.83l1.41 1.41C17.37 13.11 18 11.6 18 10s-.62-3.11-1.76-4.24",
    "M6.34 4.34L4.93 2.92C3.04 4.81 2 7.32 2 9.99s1.04 5.18 2.93 7.07l1.41-1.41c-1.51-1.51-2.35-3.52-2.35-5.66s.83-4.15 2.34-5.65Zm11.32 0c3.12 3.12 3.12 8.2 0 11.31l1.41 1.41c3.9-3.9 3.9-10.24 0-14.14l-1.41 1.41Z",
];

/// Escape text for an SVG text node or attribute.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters have no place on a card (the model refuses them too).
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// Split text into pieces set in the Latin face or the Arabic face. resvg's per-character
/// fallback can leave a right-to-left name after a Latin word as blank boxes, so a piece with
/// right-to-left letters names the Arabic face first; spaces, punctuation and the bidi
/// isolates stay with the piece they are in.
fn by_script(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let rtl = crate::words::has_rtl(c.encode_utf8(&mut [0; 4]));
        let strong = rtl || c.is_alphanumeric();
        match out.last_mut() {
            Some((r, s)) if !strong || *r == rtl => s.push(c),
            _ => out.push((rtl, c.to_string())),
        }
    }
    out
}

fn runs_svg(runs: &[Run], out: &mut String) {
    for r in runs {
        let s = &r.style;
        for (rtl, text) in by_script(&r.text) {
            let family = match (rtl, s.mono) {
                (true, _) => ARABIC_FIRST,
                (false, true) => MONO,
                (false, false) => SANS,
            };
            let _ = write!(
                out,
                "<tspan font-family=\"{family}\" font-size=\"{}\" font-weight=\"{}\" fill=\"{}\"{}>{}</tspan>",
                num(s.size),
                s.weight,
                s.fill.css(),
                if s.tracking == 0.0 {
                    String::new()
                } else {
                    format!(" letter-spacing=\"{}\"", num(s.tracking * s.size))
                },
                escape(&text)
            );
        }
    }
}

/// The SVG for a card `w` by `h` units, its background `ground`.
pub(crate) fn to_svg(w: f32, h: f32, ground: Rgb, shapes: &[Shape]) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
         <rect id=\"ground\" width=\"{w}\" height=\"{h}\" fill=\"{}\"/>",
        ground.css()
    );
    for (i, shape) in shapes.iter().enumerate() {
        match shape {
            Shape::Rect {
                x,
                y,
                w,
                h,
                fill,
                decor,
            } => {
                let id = if *decor {
                    format!("decor-{i}")
                } else {
                    format!("rect-{i}")
                };
                let _ = write!(
                    out,
                    "<rect id=\"{id}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    num(*x),
                    num(*y),
                    num(*w),
                    num(*h),
                    fill.css()
                );
            }
            Shape::Circle {
                cx,
                cy,
                r,
                fill,
                ring,
            } => {
                let stroke = match ring {
                    // The ring is drawn inside the disc's edge, as a CSS border is.
                    Some((c, wd)) => format!(
                        "/><circle id=\"ring-{i}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"",
                        num(*cx),
                        num(*cy),
                        num(r - wd / 2.0),
                        c.css(),
                        num(*wd)
                    ),
                    None => String::new(),
                };
                let _ = write!(
                    out,
                    "<circle id=\"disc-{i}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"{stroke}/>",
                    num(*cx),
                    num(*cy),
                    num(*r),
                    fill.css()
                );
            }
            Shape::Text {
                x,
                baseline,
                anchor,
                runs,
            } => {
                let anchor = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let _ = write!(
                    out,
                    "<text id=\"text-{i}\" x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" xml:space=\"preserve\">",
                    num(*x),
                    num(*baseline)
                );
                runs_svg(runs, &mut out);
                out.push_str("</text>");
            }
            Shape::Mark { x, y, size, fill } => {
                let _ = write!(
                    out,
                    "<g id=\"mark-{i}\" transform=\"translate({} {}) scale({})\" fill=\"{}\">",
                    num(*x),
                    num(*y),
                    size / 24.0,
                    fill.css()
                );
                for d in BROADCAST {
                    let _ = write!(out, "<path d=\"{d}\"/>");
                }
                out.push_str("</g>");
            }
        }
    }
    out.push_str("</svg>");
    out
}

/// The bundled faces, loaded once.
fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for face in crate::fonts::FACES {
            db.load_font_data(face.to_vec());
        }
        db.set_sans_serif_family("Noto Sans");
        db.set_monospace_family("Noto Sans Mono");
        Arc::new(db)
    })
    .clone()
}

/// Parse options: only the bundled fonts, and no image may be loaded from anywhere.
pub(crate) fn options() -> usvg::Options<'static> {
    usvg::Options {
        font_family: "Noto Sans".to_owned(),
        fontdb: fontdb(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// Measures lines of text by laying them out as resvg will, with a cache per card.
pub(crate) struct Measurer {
    opts: usvg::Options<'static>,
    cache: RefCell<HashMap<String, f32>>,
}

impl Measurer {
    pub fn new() -> Self {
        Self {
            opts: options(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// The horizontal extent of a line from its start: the right edge of its ink.
    pub fn width(&self, runs: &[Run]) -> f32 {
        let mut body = String::new();
        runs_svg(runs, &mut body);
        if let Some(w) = self.cache.borrow().get(&body) {
            return *w;
        }
        let doc = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40000\" height=\"4000\"><text x=\"0\" y=\"2000\" xml:space=\"preserve\">{body}</text></svg>"
        );
        let w = usvg::Tree::from_str(&doc, &self.opts)
            .ok()
            .and_then(|tree| {
                tree.root().children().iter().find_map(|n| match n {
                    usvg::Node::Text(t) => Some(t.bounding_box().right()),
                    usvg::Node::Group(g) => Some(g.bounding_box().right()),
                    _ => None,
                })
            })
            .unwrap_or(0.0)
            .max(0.0);
        self.cache.borrow_mut().insert(body, w);
        w
    }
}
