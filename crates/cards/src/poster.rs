// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Poster: the dark plane with a band in the show's colour at the left, the show name as large
//! as the measure allows, the level line, hosts and guests as text lines, discs with initials
//! at the right, and the station with its address in the foot (`docs/reference/design/
//! show-cards.md`, section 1). Units are the link card's pixels; the post image is the same
//! template 45 units taller, drawn at 4/3.
//!
//! The podcast square (ADR-0179, `show-cards.md` section 5a) is Poster's own: the band widened
//! to 40, the name as large as the full measure allows, the cast as a credits block standing
//! on the foot (the people lines, then the discs in a row), and the foot. Both templates share
//! the fitting loop; the numbers that differ are the two parameter blocks `LINK` and `SQUARE`.

use crate::scene::{Anchor, Measurer, Rgb, Run, Shape, Style, baseline};
use crate::words::{ELLIPSIS, initials};
use crate::wrap::{Token, clamp, join, whole, words, wrap, wrap_balanced};
use crate::{CardError, Level, MAX_GUESTS, MAX_HOSTS, Words};

pub(crate) const PLANE: Rgb = Rgb::hex(0x13141c);
const INK: Rgb = Rgb::hex(0xc0caf5);
const INK2: Rgb = Rgb::hex(0xa9b1d6);
const MUTED: Rgb = Rgb::hex(0x7982a9);
const SURFACE3: Rgb = Rgb::hex(0x292b3d);

const BAND: f32 = 28.0;
const PAD_TOP: f32 = 64.0;
const PAD_RIGHT: f32 = 72.0;
const PAD_BOTTOM: f32 = 52.0;
const PAD_LEFT: f32 = 60.0;
const DISCS_W: f32 = 104.0;

/// (level line, host names, guest names) sizes, largest first.
type Tier = (f32, f32, f32);

/// What differs between the link card (and the post image) and the square: the sizes, the
/// limits and the gaps the fitting loop works with. Each layout's own geometry (a column of
/// discs at the right, or a row at the foot) is in `link_layout` and `square_layout`.
struct Params {
    square: bool,
    /// The eyebrow's sizes, largest first; the last is its floor, where words are cut.
    eyebrow: &'static [f32],
    /// The name's largest size, its floor and the step between.
    name_start: f32,
    name_floor: f32,
    name_step: f32,
    /// Lines the name may take above its floor, and at it.
    name_lines: usize,
    name_floor_lines: usize,
    /// Between the blocks of the lower text; and between the name and that text.
    gap: f32,
    name_gap: f32,
    tiers: &'static [Tier],
    /// The foot's mark, its text and the gap between its parts.
    mark: f32,
    foot_text: f32,
    foot_gap: f32,
}

/// The link card and the post image: name 96 to 48, floors 32, 22 and 18.
const LINK: Params = Params {
    square: false,
    eyebrow: &[26.0, 24.0, 22.0, 20.0],
    name_start: 96.0,
    name_floor: 48.0,
    name_step: 4.0,
    name_lines: 2,
    name_floor_lines: 3,
    gap: 18.0,
    name_gap: 18.0,
    tiers: &[
        (40.0, 30.0, 30.0),
        (40.0, 28.0, 28.0),
        (40.0, 26.0, 26.0),
        (36.0, 26.0, 24.0),
        (36.0, 24.0, 22.0),
        (32.0, 22.0, 20.0),
        (32.0, 22.0, 18.0),
    ],
    mark: 40.0,
    foot_text: 26.0,
    foot_gap: 18.0,
};

/// The Poster square, in 1,200 × 1,200 template units (`show-cards.md` section 5a): the name
/// 136 to 64 in steps of 8 over up to three lines (four at the floor), the lower block's
/// lines 24 apart and 48 below the name, and the link card's tiers each at 4/3.
const SQUARE: Params = Params {
    square: true,
    eyebrow: &[36.0, 32.0, 28.0, 24.0],
    name_start: 136.0,
    name_floor: 64.0,
    name_step: 8.0,
    name_lines: 3,
    name_floor_lines: 4,
    gap: 24.0,
    name_gap: 48.0,
    tiers: &[
        (52.0, 40.0, 40.0),
        (52.0, 36.0, 36.0),
        (52.0, 34.0, 34.0),
        (48.0, 34.0, 32.0),
        (48.0, 32.0, 30.0),
        (44.0, 30.0, 26.0),
        (44.0, 30.0, 24.0),
    ],
    mark: 48.0,
    foot_text: 32.0,
    foot_gap: 24.0,
};

/// The square's geometry, in template units (section 5a).
mod sq {
    pub const BAND: f32 = 40.0;
    pub const X0: f32 = 120.0;
    pub const X1: f32 = 1120.0;
    pub const EYEBROW_TOP: f32 = 80.0;
    pub const NAME_TOP: f32 = 152.0;
    /// Where the discs row and the credits block stand.
    pub const STAND: f32 = 1022.0;
    /// Between the lower block and the discs row.
    pub const DISCS_GAP: f32 = 32.0;
    pub const DISCS_SPACING: f32 = 16.0;
    pub const RULE: f32 = 1046.0;
    pub const FOOT_TOP: f32 = 1080.0;
    /// (host disc, guest disc) sizes, largest first.
    pub const DISC_STEPS: [(f32, f32); 4] =
        [(160.0, 128.0), (160.0, 96.0), (128.0, 96.0), (96.0, 96.0)];
}

/// A people line: "WITH Shona Campbell and Catriona MacKenzie‑Buchanan", the label small and
/// muted, the names bold, the joining words in the paragraph's colour; past `max` names the
/// rest become "and N more".
pub(crate) fn people_tokens(label: &str, names: &[String], shown: usize, size: f32) -> Vec<Token> {
    let lab = Style::new(20.0 * size / 30.0, 600, MUTED).tracking(0.1);
    let name = Style::new(size, 600, INK);
    let plain = Style::new(size, 400, INK2);
    let mut out = vec![whole(label, lab, false)];
    let shown = shown.min(names.len());
    let more = names.len() - shown;
    for (i, n) in names[..shown].iter().enumerate() {
        let last = i + 1 == shown;
        let before_last = i + 2 == shown;
        let mut runs = vec![Run::new(n.clone(), name)];
        // "A, B and C"; with more to come, "A, B, C and 2 more".
        if (!last && !before_last) || (before_last && more > 0) {
            runs.push(Run::new(",", plain));
        }
        out.push(Token::new(runs, true));
        if (before_last && more == 0) || (last && more > 0) {
            out.push(whole("and", plain, true));
        }
    }
    if more > 0 {
        out.extend(
            words(&format!("{more} more"), plain)
                .into_iter()
                .map(|mut t| {
                    t.space_before = true;
                    t
                }),
        );
    }
    out
}

/// A block of lines at one size.
struct Block {
    lines: Vec<Vec<Run>>,
    size: f32,
    line_height: f32,
}

impl Block {
    fn height(&self) -> f32 {
        self.lines.len() as f32 * self.size * self.line_height
    }
}

/// The eyebrow at the largest size that fits `width`, and its size.
fn eyebrow(w: &Words, m: &Measurer, p: &Params, show: Rgb, width: f32) -> (f32, Vec<Run>) {
    let floor = *p.eyebrow.last().expect("the eyebrow has sizes");
    for &size in p.eyebrow {
        let st = Style::new(size, 700, show).tracking(0.14);
        let mut runs = vec![Run::new(w.station.to_uppercase(), st)];
        if let Some(when) = &w.poster_when {
            runs.push(Run::new(
                format!(" \u{b7} {}", when.to_uppercase()),
                Style { fill: MUTED, ..st },
            ));
        }
        if m.width(&runs) <= width {
            return (size, runs);
        }
        if size == floor {
            // Cut the words after the station, word by word.
            while runs.len() > 1 && m.width(&runs) > width {
                let rest = &mut runs[1].text;
                match rest.trim_end_matches(ELLIPSIS).rfind(' ') {
                    Some(i) if i > 3 => {
                        rest.truncate(i);
                        rest.push(ELLIPSIS);
                    }
                    _ => {
                        runs.truncate(1);
                    }
                }
            }
            return (size, runs);
        }
    }
    unreachable!("the loop returns at the floor")
}

/// The text column at one set of sizes, or `None` when a field breaks its line limit.
#[allow(clippy::too_many_arguments)]
fn column(
    w: &Words,
    m: &Measurer,
    name: &Block,
    width: f32,
    level_size: f32,
    host_size: f32,
    guest_size: f32,
    hosts_shown: usize,
    guests_shown: usize,
    description: bool,
) -> Option<Vec<Block>> {
    let mut blocks = Vec::new();
    if let Some((label, rest)) = &w.level_line {
        let bold = Style::new(level_size, 600, INK).tracking(-0.01);
        let plain = Style::new(level_size, 400, INK2).tracking(-0.01);
        let mut toks = vec![whole(label, bold, false)];
        if let Some(rest) = rest {
            toks.push(whole("\u{b7}", plain, true));
            toks.extend(words(rest, plain));
            if let Some(t) = toks.get_mut(2) {
                t.space_before = true;
            }
        }
        let lines = wrap_balanced(m, &toks, width);
        blocks.push(Block {
            lines: clamp(m, lines, 2, width),
            size: level_size,
            line_height: 1.1,
        });
    }
    for (label, names, shown, size) in [
        ("With", &w.hosts, hosts_shown, host_size),
        (
            if w.guests.len() == 1 {
                "Guest"
            } else {
                "Guests"
            },
            &w.guests,
            guests_shown,
            guest_size,
        ),
    ] {
        if names.is_empty() {
            continue;
        }
        let toks = people_tokens(&label.to_uppercase(), names, shown, size);
        let lines = wrap(m, &toks, width);
        if lines.len() > 2 || crate::wrap::widest(m, &lines) > width {
            return None;
        }
        blocks.push(Block {
            lines: lines.iter().map(|l| join(l)).collect(),
            size,
            line_height: 1.3,
        });
    }
    if description && w.level == Level::Show && !w.description.is_empty() {
        let st = Style::new(host_size, 400, INK2);
        let lines = wrap(m, &words(&w.description, st), width);
        blocks.push(Block {
            lines: clamp(m, lines, 2, width),
            size: host_size,
            line_height: 1.3,
        });
    }
    let mut all = vec![Block {
        lines: name.lines.clone(),
        size: name.size,
        line_height: name.line_height,
    }];
    all.extend(blocks);
    Some(all)
}

/// The name, then `name_gap`, then the rest with `gap` between its blocks.
fn stack_height(blocks: &[Block], p: &Params) -> f32 {
    let lower = &blocks[1..];
    blocks[0].height()
        + if lower.is_empty() {
            0.0
        } else {
            p.name_gap
                + lower.iter().map(Block::height).sum::<f32>()
                + p.gap * (lower.len() - 1) as f32
        }
}

/// The name at each size that keeps it to its line limit (`name_lines`, or `name_floor_lines`
/// at the floor), largest first.
fn name_blocks(w: &Words, m: &Measurer, p: &Params, width: f32) -> Result<Vec<Block>, CardError> {
    let mut out = Vec::new();
    let mut size = p.name_start;
    while size >= p.name_floor {
        let floor = size <= p.name_floor;
        let st = Style::new(size, 700, INK).tracking(if floor { -0.015 } else { -0.025 });
        let lines = wrap_balanced(m, &words(&w.name, st), width);
        let fits = crate::wrap::widest(m, &lines) <= width;
        if fits && (lines.len() <= p.name_lines || (floor && lines.len() <= p.name_floor_lines)) {
            out.push(Block {
                lines: lines.iter().map(|l| join(l)).collect(),
                size,
                line_height: if floor { 1.06 } else { 0.98 },
            });
        }
        size -= p.name_step;
    }
    if out.is_empty() {
        return Err(CardError::DoesNotFit("show name"));
    }
    Ok(out)
}

/// The name and the lower text, fitted into `width` and `height`: for each cut (people by
/// name, fewest last), for each name size that keeps its line limit, the description on and
/// then off, for each tier, the first whose people lines are two or fewer and whose stack
/// fits. The first block is the name.
fn fit_column(
    w: &Words,
    m: &Measurer,
    p: &Params,
    width: f32,
    height: f32,
) -> Result<Vec<Block>, CardError> {
    let names = name_blocks(w, m, p, width)?;
    // Fewer people by name ("and N more") only once every size has been tried.
    let mut cuts = vec![(w.hosts.len().min(MAX_HOSTS), w.guests.len().min(MAX_GUESTS))];
    let (mut h, mut g) = cuts[0];
    while g > 1 {
        g -= 1;
        cuts.push((h, g));
    }
    while h > 1 {
        h -= 1;
        cuts.push((h, g));
    }
    for (hosts, guests) in cuts {
        for name in &names {
            for description in [true, false] {
                for &(lv, hs, gs) in p.tiers {
                    if let Some(blocks) =
                        column(w, m, name, width, lv, hs, gs, hosts, guests, description)
                        && stack_height(&blocks, p) <= height
                    {
                        return Ok(blocks);
                    }
                }
            }
        }
    }
    Err(CardError::DoesNotFit("people"))
}

/// The link card's discs: hosts ringed in the show's colour, then guests, shrinking toward the
/// 64 px floor, then guests left to the text, then as many hosts as fit.
fn discs(w: &Words, height: f32) -> Vec<(String, f32, bool)> {
    let hosts: Vec<&String> = w.hosts.iter().take(MAX_HOSTS).collect();
    let guests: Vec<&String> = w.guests.iter().take(MAX_GUESTS).collect();
    let total =
        |sizes: &[f32]| sizes.iter().sum::<f32>() + 14.0 * (sizes.len().saturating_sub(1)) as f32;
    for (hs, gs) in [(104.0, 84.0), (104.0, 64.0), (84.0, 64.0), (64.0, 64.0)] {
        let mut sizes: Vec<f32> = hosts.iter().map(|_| hs).collect();
        sizes.extend(guests.iter().map(|_| gs));
        if total(&sizes) <= height {
            let mut out: Vec<(String, f32, bool)> =
                hosts.iter().map(|n| (n.to_string(), hs, true)).collect();
            out.extend(guests.iter().map(|n| (n.to_string(), gs, false)));
            return out;
        }
    }
    for hs in [104.0, 84.0, 64.0] {
        let mut n = hosts.len();
        while n > 0 && total(&vec![hs; n]) > height {
            n -= 1;
        }
        if n == hosts.len() || hs == 64.0 {
            return hosts[..n]
                .iter()
                .map(|h| (h.to_string(), hs, true))
                .collect();
        }
    }
    Vec::new()
}

/// The square's discs, in one row of at most `width`: hosts ringed in the show's colour, then
/// guests, through the size steps; then the guests leave the row (they stay in the text line);
/// then as many hosts as fit.
fn disc_row(w: &Words, width: f32) -> Vec<(String, f32, bool)> {
    let hosts: Vec<&String> = w.hosts.iter().take(MAX_HOSTS).collect();
    let guests: Vec<&String> = w.guests.iter().take(MAX_GUESTS).collect();
    let total = |sizes: &[f32]| {
        sizes.iter().sum::<f32>() + sq::DISCS_SPACING * (sizes.len().saturating_sub(1)) as f32
    };
    for (hs, gs) in sq::DISC_STEPS {
        let mut sizes: Vec<f32> = hosts.iter().map(|_| hs).collect();
        sizes.extend(guests.iter().map(|_| gs));
        if total(&sizes) <= width {
            let mut out: Vec<(String, f32, bool)> =
                hosts.iter().map(|n| (n.to_string(), hs, true)).collect();
            out.extend(guests.iter().map(|n| (n.to_string(), gs, false)));
            return out;
        }
    }
    let last = sq::DISC_STEPS.len() - 1;
    for (i, (hs, _)) in sq::DISC_STEPS.iter().enumerate() {
        let mut n = hosts.len();
        while n > 0 && total(&vec![*hs; n]) > width {
            n -= 1;
        }
        if n == hosts.len() || i == last {
            return hosts[..n]
                .iter()
                .map(|h| (h.to_string(), *hs, true))
                .collect();
        }
    }
    Vec::new()
}

/// The foot's words: the station, its "when" line and the address, as (x, anchor, runs). The
/// address keeps the right end; the other parts give way in a fixed order (the link card:
/// the address, the address smaller, the show's address, the show's address smaller, the
/// "when" line dropped; the square: the address with the "when" line, the address smaller,
/// the "when" line dropped, the address smaller still, the show's address).
fn foot(w: &Words, m: &Measurer, p: &Params, x0: f32, x1: f32) -> Vec<(f32, Anchor, Vec<Run>)> {
    let gap = p.foot_gap;
    let t = p.foot_text;
    let station_x = x0 + p.mark + gap;
    let station = || Run::new(w.station.clone(), Style::new(t, 700, INK));
    let when = Run::new(w.foot_when.clone(), Style::new(t, 400, MUTED));
    let addr =
        |text: &str, size: f32| Run::new(text.to_owned(), Style::new(size, 400, INK2).mono());
    let attempts: [(bool, &str, f32); 5] = if p.square {
        [
            (true, &w.address, 32.0),
            (true, &w.address, 28.0),
            (false, &w.address, 32.0),
            (false, &w.address, 28.0),
            (false, &w.show_address, 28.0),
        ]
    } else {
        [
            (true, &w.address, 26.0),
            (true, &w.address, 22.0),
            (true, &w.show_address, 26.0),
            (true, &w.show_address, 22.0),
            (false, &w.show_address, 22.0),
        ]
    };
    let floor = attempts[4].2;
    for (with_when, address, size) in attempts {
        let sw = m.width(&[station()]);
        let has_when = with_when && !w.foot_when.is_empty();
        let ww = m.width(std::slice::from_ref(&when));
        let aw = m.width(&[addr(address, size)]);
        let need = if p.square {
            // Mark, station, when and address, each `gap` apart.
            sw + gap + if has_when { ww + gap } else { 0.0 } + aw
        } else {
            sw + gap + if has_when { ww + gap } else { 0.0 } + 24.0 + aw
        };
        if station_x + need <= x1 {
            let mut out = vec![(station_x, Anchor::Start, vec![station()])];
            if has_when {
                out.push((station_x + sw + gap, Anchor::Start, vec![when.clone()]));
            }
            out.push((x1, Anchor::End, vec![addr(address, size)]));
            return out;
        }
    }
    // A station name too long for the foot: the address keeps its place, the name is cut.
    let aw = m.width(&[addr(&w.show_address, floor)]);
    let room = x1 - station_x - 24.0 - aw;
    let mut name = w.station.clone();
    while !name.is_empty()
        && m.width(&[Run::new(format!("{name}{ELLIPSIS}"), station().style)]) > room
    {
        name.pop();
    }
    vec![
        (
            station_x,
            Anchor::Start,
            vec![Run::new(
                format!("{}{ELLIPSIS}", name.trim_end()),
                station().style,
            )],
        ),
        (x1, Anchor::End, vec![addr(&w.show_address, floor)]),
    ]
}

/// One disc with its initials, centred on `cx`, its top at `top`.
fn disc_shapes(
    out: &mut Vec<Shape>,
    name: &str,
    size: f32,
    host: bool,
    show: Rgb,
    cx: f32,
    top: f32,
) {
    let r = size / 2.0;
    out.push(Shape::Circle {
        cx,
        cy: top + r,
        r,
        fill: SURFACE3,
        ring: host.then_some((show, 3.0 * size / 104.0)),
    });
    let fs = size * 32.0 / 104.0;
    out.push(Shape::Text {
        x: cx,
        baseline: baseline(top, fs, size / fs),
        anchor: Anchor::Middle,
        runs: vec![Run::new(
            initials(name),
            Style::new(fs, 700, INK2).tracking(0.02),
        )],
    });
}

/// The foot: a rule, the mark, the station, the address at the right.
fn foot_shapes(
    out: &mut Vec<Shape>,
    w: &Words,
    m: &Measurer,
    p: &Params,
    show: Rgb,
    (x0, x1): (f32, f32),
    (rule_y, foot_top): (f32, f32),
) {
    out.push(Shape::Rect {
        x: x0,
        y: rule_y,
        w: x1 - x0,
        h: 2.0,
        fill: SURFACE3,
        decor: false,
    });
    out.push(Shape::Mark {
        x: x0,
        y: foot_top,
        size: p.mark,
        fill: show,
    });
    for (x, anchor, runs) in foot(w, m, p, x0, x1) {
        out.push(Shape::Text {
            x,
            baseline: baseline(foot_top, p.foot_text, p.mark / p.foot_text),
            anchor,
            runs,
        });
    }
}

/// The lines of the blocks from `top` down, `gap` between blocks.
fn lines_at(out: &mut Vec<Shape>, blocks: &[Block], x: f32, top: f32, gap: f32) {
    let mut y = top;
    for b in blocks {
        for line in &b.lines {
            out.push(Shape::Text {
                x,
                baseline: baseline(y, b.size, b.line_height),
                anchor: Anchor::Start,
                runs: line.clone(),
            });
            y += b.size * b.line_height;
        }
        y += gap;
    }
}

pub(crate) fn layout(
    w: &Words,
    m: &Measurer,
    fw: f32,
    fh: f32,
    show: Rgb,
    square: bool,
) -> Result<Vec<Shape>, CardError> {
    if square {
        square_layout(w, m, fh, show)
    } else {
        link_layout(w, m, fw, fh, show)
    }
}

/// The link card and the post image.
fn link_layout(
    w: &Words,
    m: &Measurer,
    fw: f32,
    fh: f32,
    show: Rgb,
) -> Result<Vec<Shape>, CardError> {
    let p = &LINK;
    let mut out = vec![Shape::Rect {
        x: 0.0,
        y: 0.0,
        w: BAND,
        h: fh,
        fill: show,
        decor: true,
    }];
    let x0 = BAND + PAD_LEFT;
    let x1 = fw - PAD_RIGHT;
    let width = x1 - x0;

    // The eyebrow: the station in the show's colour, then the slot or the date.
    let (eyebrow_size, runs) = eyebrow(w, m, p, show, width);
    out.push(Shape::Text {
        x: x0,
        baseline: baseline(PAD_TOP, eyebrow_size, 1.0),
        anchor: Anchor::Start,
        runs,
    });

    let foot_bottom = fh - PAD_BOTTOM;
    let foot_top = foot_bottom - 40.0;
    let rule_y = foot_top - 24.0 - 2.0;
    foot_shapes(&mut out, w, m, p, show, (x0, x1), (rule_y, foot_top));

    // The row: the text column, and the discs at the right. The text column stops short of
    // the discs: the name, the level line and the people lines are all fitted to `text_w`.
    let row_top = PAD_TOP + 26.0 + 28.0;
    let row_bottom = rule_y - 12.0;
    let people = discs(w, row_bottom - row_top);
    let text_w = if people.is_empty() {
        width
    } else {
        width - 40.0 - DISCS_W
    };
    let blocks = fit_column(w, m, p, text_w, row_bottom - row_top)?;
    lines_at(&mut out, &blocks, x0, row_top, p.gap);
    let cx = x1 - DISCS_W / 2.0;
    let mut y = row_top;
    for (name, size, host) in people {
        disc_shapes(&mut out, &name, size, host, show, cx, y);
        y += size + 14.0;
    }
    Ok(out)
}

/// The Poster square (`show-cards.md` section 5a): the eyebrow and the name hang from the
/// top; the lower block and the discs row stand on 1,022; the foot sits below its rule.
fn square_layout(w: &Words, m: &Measurer, side: f32, show: Rgb) -> Result<Vec<Shape>, CardError> {
    let p = &SQUARE;
    let (x0, x1) = (sq::X0, sq::X1);
    let width = x1 - x0;
    let mut out = vec![Shape::Rect {
        x: 0.0,
        y: 0.0,
        w: sq::BAND,
        h: side,
        fill: show,
        decor: true,
    }];

    let (eyebrow_size, runs) = eyebrow(w, m, p, show, width);
    out.push(Shape::Text {
        x: x0,
        baseline: baseline(sq::EYEBROW_TOP, eyebrow_size, 1.0),
        anchor: Anchor::Start,
        runs,
    });

    foot_shapes(&mut out, w, m, p, show, (x0, x1), (sq::RULE, sq::FOOT_TOP));

    // The discs stand on the credits block's line; what the stack may take is what lies
    // between the name's top and the discs' bottom, less the discs and their gap.
    let people = disc_row(w, width);
    let disc_h = people.iter().map(|(_, s, _)| *s).fold(0.0, f32::max);
    let below = if people.is_empty() {
        0.0
    } else {
        sq::DISCS_GAP + disc_h
    };
    let blocks = fit_column(w, m, p, width, sq::STAND - sq::NAME_TOP - below)?;
    let (name, lower) = blocks.split_at(1);
    lines_at(&mut out, name, x0, sq::NAME_TOP, p.gap);
    if !lower.is_empty() {
        let height =
            lower.iter().map(Block::height).sum::<f32>() + p.gap * (lower.len() - 1) as f32;
        lines_at(&mut out, lower, x0, sq::STAND - below - height, p.gap);
    }
    let mut x = x0;
    for (name, size, host) in people {
        disc_shapes(
            &mut out,
            &name,
            size,
            host,
            show,
            x + size / 2.0,
            sq::STAND - size,
        );
        x += size + sq::DISCS_SPACING;
    }
    Ok(out)
}
