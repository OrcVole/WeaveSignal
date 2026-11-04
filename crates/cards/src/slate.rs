// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Slate: the light surface with a rule in the show's colour on top, the station line, the
//! level eyebrow, the show name, the title and the date in a column, and the people at the
//! right as discs with their names under them (`docs/reference/design/show-cards.md`). The
//! podcast square is the same card stacked: the text above, a rule, the people in a row at the
//! foot.

use crate::scene::{Anchor, Measurer, Rgb, Run, Shape, Style, baseline};
use crate::words::initials;
use crate::wrap::{clamp, join, widest, words, wrap_balanced};
use crate::{CardError, MAX_GUESTS, MAX_HOSTS, Words};

pub(crate) const SURFACE: Rgb = Rgb::hex(0xfcfcfb);
const INK: Rgb = Rgb::hex(0x1a1b26);
const INK2: Rgb = Rgb::hex(0x4b4f66);
const MUTED: Rgb = Rgb::hex(0x6b7089);
const SURFACE3: Rgb = Rgb::hex(0xe3e6ef);
const GRID: Rgb = Rgb::hex(0xe1e0d9);

const PAD_X: f32 = 72.0;
const GAP: f32 = 18.0;

/// A size of person: the box a person takes, the box a long name may grow to, the disc, the
/// name's size, and the gap between people.
#[derive(Clone, Copy)]
struct Person {
    boxw: f32,
    wide: f32,
    disc: f32,
    name: f32,
    gap: f32,
}

struct Params {
    rule: f32,
    head_top: f32,
    head_h: f32,
    head_font: f32,
    mark: f32,
    body_gap: f32,
    bottom: f32,
    eyebrow: f32,
    name: f32,
    name_floor: f32,
    title: f32,
    title_floor: f32,
    when: f32,
    label: f32,
    hosts: [Person; 3],
    guests: [Person; 3],
}

const LINK: Params = Params {
    rule: 22.0,
    head_top: 52.0,
    head_h: 36.0,
    head_font: 28.0,
    mark: 36.0,
    body_gap: 36.0,
    bottom: 56.0,
    eyebrow: 26.0,
    name: 80.0,
    name_floor: 44.0,
    title: 44.0,
    title_floor: 32.0,
    when: 30.0,
    label: 20.0,
    hosts: [
        Person {
            boxw: 150.0,
            wide: 220.0,
            disc: 104.0,
            name: 22.0,
            gap: 18.0,
        },
        Person {
            boxw: 120.0,
            wide: 180.0,
            disc: 84.0,
            name: 20.0,
            gap: 14.0,
        },
        Person {
            boxw: 84.0,
            wide: 140.0,
            disc: 64.0,
            name: 18.0,
            gap: 8.0,
        },
    ],
    guests: [
        Person {
            boxw: 104.0,
            wide: 150.0,
            disc: 84.0,
            name: 22.0,
            gap: 18.0,
        },
        Person {
            boxw: 90.0,
            wide: 120.0,
            disc: 72.0,
            name: 20.0,
            gap: 12.0,
        },
        Person {
            boxw: 76.0,
            wide: 104.0,
            disc: 64.0,
            name: 18.0,
            gap: 8.0,
        },
    ],
};

const SQUARE: Params = Params {
    rule: 32.0,
    head_top: 58.0,
    head_h: 48.0,
    head_font: 26.0,
    mark: 48.0,
    body_gap: 64.0,
    bottom: 56.0,
    eyebrow: 36.0,
    name: 112.0,
    name_floor: 60.0,
    title: 56.0,
    title_floor: 40.0,
    when: 40.0,
    label: 28.0,
    hosts: [
        Person {
            boxw: 180.0,
            wide: 262.0,
            disc: 160.0,
            name: 30.0,
            gap: 18.0,
        },
        Person {
            boxw: 150.0,
            wide: 220.0,
            disc: 128.0,
            name: 26.0,
            gap: 14.0,
        },
        Person {
            boxw: 110.0,
            wide: 170.0,
            disc: 96.0,
            name: 22.0,
            gap: 10.0,
        },
    ],
    guests: [
        Person {
            boxw: 180.0,
            wide: 262.0,
            disc: 160.0,
            name: 30.0,
            gap: 18.0,
        },
        Person {
            boxw: 150.0,
            wide: 220.0,
            disc: 128.0,
            name: 26.0,
            gap: 14.0,
        },
        Person {
            boxw: 110.0,
            wide: 170.0,
            disc: 96.0,
            name: 22.0,
            gap: 10.0,
        },
    ],
};

/// One person laid out: box width, the name's lines and size.
struct Placed {
    name: String,
    boxw: f32,
    lines: Vec<Vec<Run>>,
    size: f32,
}

/// A group (hosts or guests) at one size: its people, and the "and N more" line if any.
struct Group {
    label: String,
    people: Vec<Placed>,
    more: Option<String>,
    p: Person,
    label_size: f32,
}

impl Group {
    fn width(&self) -> f32 {
        self.people.iter().map(|p| p.boxw).sum::<f32>()
            + self.p.gap * (self.people.len().saturating_sub(1)) as f32
    }

    fn name_height(&self) -> f32 {
        self.people
            .iter()
            .map(|p| p.lines.len() as f32 * p.size * 1.15)
            .fold(0.0, f32::max)
    }

    fn height(&self) -> f32 {
        let more = if self.more.is_some() {
            12.0 + self.label_size * 1.2
        } else {
            0.0
        };
        self.label_size + 12.0 + self.p.disc + 8.0 + self.name_height() + more
    }
}

fn place(m: &Measurer, name: &str, p: Person) -> Placed {
    let mut size = p.name;
    loop {
        let st = Style::new(size, 600, INK);
        let toks = words(name, st);
        // The box grows for a long word before the type shrinks (the design's 150 to 220).
        let longest = toks
            .iter()
            .map(|t| m.width(&join(std::slice::from_ref(t))))
            .fold(0.0, f32::max);
        let boxw = p.boxw.max(longest.min(p.wide));
        let lines = wrap_balanced(m, &toks, boxw);
        if (lines.len() <= 2 && widest(m, &lines) <= boxw) || size <= p.name - 6.0 {
            return Placed {
                name: name.to_owned(),
                boxw,
                lines: clamp(m, lines, 2, boxw),
                size,
            };
        }
        size -= 1.0;
    }
}

fn group(
    m: &Measurer,
    label: &str,
    names: &[String],
    shown: usize,
    p: Person,
    label_size: f32,
) -> Group {
    let shown = shown.min(names.len());
    let more = names.len() - shown;
    Group {
        label: label.to_uppercase(),
        people: names[..shown].iter().map(|n| place(m, n, p)).collect(),
        more: (more > 0).then(|| format!("and {more} more")),
        p,
        label_size,
    }
}

/// The people at the largest sizes that fit `width` by `height`; side by side in the square.
fn people(
    w: &Words,
    m: &Measurer,
    pr: &Params,
    width: f32,
    height: f32,
    side_by_side: bool,
) -> Result<Vec<Group>, CardError> {
    let host_label = if w.hosts.len() == 1 { "Host" } else { "Hosts" };
    let guest_label = if w.guests.len() == 1 {
        "Guest"
    } else {
        "Guests"
    };
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
    for (hs, gs) in cuts {
        for (hi, gi) in [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)] {
            let mut groups = Vec::new();
            if !w.hosts.is_empty() {
                groups.push(group(m, host_label, &w.hosts, hs, pr.hosts[hi], pr.label));
            }
            if !w.guests.is_empty() {
                groups.push(group(
                    m,
                    guest_label,
                    &w.guests,
                    gs,
                    pr.guests[gi],
                    pr.label,
                ));
            }
            let fits = if side_by_side {
                let total_w = groups.iter().map(Group::width).sum::<f32>()
                    + 56.0 * (groups.len().saturating_sub(1)) as f32;
                let total_h = groups.iter().map(Group::height).fold(0.0, f32::max);
                total_w <= width && total_h <= height
            } else {
                let total_h = groups.iter().map(Group::height).sum::<f32>()
                    + 22.0 * (groups.len().saturating_sub(1)) as f32;
                groups.iter().all(|g| g.width() <= width) && total_h <= height
            };
            if fits {
                return Ok(groups);
            }
        }
    }
    Err(CardError::DoesNotFit("people"))
}

fn draw_group(out: &mut Vec<Shape>, g: &Group, x: f32, y: f32) {
    let lab = Style::new(g.label_size, 700, MUTED).tracking(0.12);
    out.push(Shape::Text {
        x,
        baseline: baseline(y, g.label_size, 1.0),
        anchor: Anchor::Start,
        runs: vec![Run::new(g.label.clone(), lab)],
    });
    let disc_top = y + g.label_size + 12.0;
    let mut px = x;
    for p in &g.people {
        let cx = px + p.boxw / 2.0;
        let r = g.p.disc / 2.0;
        out.push(Shape::Circle {
            cx,
            cy: disc_top + r,
            r,
            fill: SURFACE3,
            ring: None,
        });
        let fs = g.p.disc * 32.0 / 104.0;
        out.push(Shape::Text {
            x: cx,
            baseline: baseline(disc_top, fs, g.p.disc / fs),
            anchor: Anchor::Middle,
            runs: vec![Run::new(
                initials(&p.name),
                Style::new(fs, 700, INK2).tracking(0.02),
            )],
        });
        let mut ny = disc_top + g.p.disc + 8.0;
        for line in &p.lines {
            out.push(Shape::Text {
                x: cx,
                baseline: baseline(ny, p.size, 1.15),
                anchor: Anchor::Middle,
                runs: line.clone(),
            });
            ny += p.size * 1.15;
        }
        px += p.boxw + g.p.gap;
    }
    if let Some(more) = &g.more {
        let top = disc_top + g.p.disc + 8.0 + g.name_height() + 12.0;
        out.push(Shape::Text {
            x,
            baseline: baseline(top, g.label_size, 1.2),
            anchor: Anchor::Start,
            runs: vec![Run::new(more.clone(), Style::new(g.label_size, 600, INK2))],
        });
    }
}

/// One line of text shrunk toward `floor`, then cut with an ellipsis.
fn one_line(m: &Measurer, text: &str, st: Style, floor: f32, width: f32) -> Vec<Run> {
    let mut size = st.size;
    loop {
        let runs = vec![Run::new(text, st.sized(size))];
        if m.width(&runs) <= width {
            return runs;
        }
        if size <= floor {
            let lines = wrap_balanced(m, &words(text, st.sized(size)), width);
            return clamp(m, lines, 1, width).remove(0);
        }
        size -= 2.0;
    }
}

struct TextBlock {
    eyebrow: Vec<Run>,
    name: (Vec<Vec<Run>>, f32, f32),
    title: Option<(Vec<Vec<Run>>, f32)>,
    when: Vec<Vec<Run>>,
    when_size: f32,
}

impl TextBlock {
    fn height(&self, pr: &Params) -> f32 {
        let mut h = pr.eyebrow + GAP + self.name.0.len() as f32 * self.name.1 * self.name.2;
        if let Some((lines, size)) = &self.title {
            h += GAP + lines.len() as f32 * size * 1.12;
        }
        if !self.when.is_empty() {
            h += GAP + self.when.len() as f32 * self.when_size * 1.25;
        }
        h
    }
}

fn text_block(
    w: &Words,
    m: &Measurer,
    pr: &Params,
    square: bool,
    show: Rgb,
    width: f32,
    height: f32,
) -> Result<TextBlock, CardError> {
    let eyebrow = one_line(
        m,
        &w.slate_eyebrow.to_uppercase(),
        Style::new(pr.eyebrow, 700, show).tracking(0.14),
        pr.eyebrow * 0.77,
        width,
    );
    let when: Vec<Vec<Run>> = if square {
        w.square_when
            .iter()
            .map(|s| one_line(m, s, Style::new(pr.when, 600, INK), pr.when * 0.75, width))
            .collect()
    } else {
        match &w.slate_when {
            Some((bold, plain)) => vec![
                one_line(
                    m,
                    bold,
                    Style::new(pr.when, 600, INK),
                    pr.when * 0.75,
                    width,
                ),
                one_line(
                    m,
                    plain,
                    Style::new(pr.when, 400, INK2),
                    pr.when * 0.75,
                    width,
                ),
            ],
            None => Vec::new(),
        }
    };
    let mut size = pr.name;
    while size >= pr.name_floor {
        let floor = size - 4.0 < pr.name_floor;
        let lh = if floor { 1.06 } else { 1.0 };
        let st = Style::new(size, 700, INK).tracking(-0.025);
        let lines = wrap_balanced(m, &words(&w.name, st), width);
        if widest(m, &lines) <= width && (lines.len() <= 2 || (floor && lines.len() <= 3)) {
            let name: Vec<Vec<Run>> = lines.iter().map(|l| join(l)).collect();
            let mut ts = pr.title;
            loop {
                let title = w.slate_title.as_ref().map(|t| {
                    let st = Style::new(ts, 400, INK);
                    (
                        clamp(m, wrap_balanced(m, &words(t, st), width), 2, width),
                        ts,
                    )
                });
                let block = TextBlock {
                    eyebrow: eyebrow.clone(),
                    name: (name.clone(), size, lh),
                    title,
                    when: when.clone(),
                    when_size: pr.when,
                };
                if block.height(pr) <= height {
                    return Ok(block);
                }
                if ts <= pr.title_floor {
                    break;
                }
                ts -= 4.0;
            }
        }
        size -= 4.0;
    }
    if w.slate_title.is_some() {
        // Last: the title gives way to the name.
        let without = Words {
            slate_title: None,
            ..w.clone()
        };
        return text_block(&without, m, pr, square, show, width, height);
    }
    Err(CardError::DoesNotFit("show name"))
}

fn draw_text(out: &mut Vec<Shape>, t: &TextBlock, pr: &Params, x: f32, top: f32, bottom: f32) {
    let mut y = top;
    out.push(Shape::Text {
        x,
        baseline: baseline(y, pr.eyebrow, 1.0),
        anchor: Anchor::Start,
        runs: t.eyebrow.clone(),
    });
    y += pr.eyebrow + GAP;
    for line in &t.name.0 {
        out.push(Shape::Text {
            x,
            baseline: baseline(y, t.name.1, t.name.2),
            anchor: Anchor::Start,
            runs: line.clone(),
        });
        y += t.name.1 * t.name.2;
    }
    if let Some((lines, size)) = &t.title {
        y += GAP;
        for line in lines {
            out.push(Shape::Text {
                x,
                baseline: baseline(y, *size, 1.12),
                anchor: Anchor::Start,
                runs: line.clone(),
            });
            y += size * 1.12;
        }
    }
    // The date sits at the foot of the column.
    let mut y = bottom - t.when.len() as f32 * t.when_size * 1.25;
    for line in &t.when {
        out.push(Shape::Text {
            x,
            baseline: baseline(y, t.when_size, 1.25),
            anchor: Anchor::Start,
            runs: line.clone(),
        });
        y += t.when_size * 1.25;
    }
}

fn head(out: &mut Vec<Shape>, w: &Words, m: &Measurer, pr: &Params, show: Rgb, fw: f32) {
    let x0 = PAD_X;
    let x1 = fw - PAD_X;
    let top = pr.head_top;
    out.push(Shape::Mark {
        x: x0,
        y: top,
        size: pr.mark,
        fill: show,
    });
    let base = baseline(top, pr.head_font, pr.head_h / pr.head_font);
    let sx = x0 + pr.mark + 16.0;
    let station = vec![Run::new(
        w.station.clone(),
        Style::new(pr.head_font, 700, INK),
    )];
    let sw = m.width(&station);
    let place = vec![Run::new(
        w.place.clone(),
        Style::new(pr.head_font, 400, INK2),
    )];
    let pw = if w.place.is_empty() {
        0.0
    } else {
        m.width(&place) + 16.0
    };
    let addr = |a: &str, size: f32| vec![Run::new(a, Style::new(size, 400, INK2).mono())];
    let floor = pr.head_font * 22.0 / 28.0;
    let choice = [
        (&w.address, pr.head_font, true),
        (&w.address, floor, true),
        (&w.show_address, pr.head_font, true),
        (&w.show_address, floor, true),
        (&w.show_address, floor, false),
    ]
    .into_iter()
    .find(|(a, size, with_place)| {
        let pw = if *with_place { pw } else { 0.0 };
        sx + sw + pw + 24.0 + m.width(&addr(a, *size)) <= x1
    })
    .unwrap_or((&w.show_address, floor, false));
    out.push(Shape::Text {
        x: sx,
        baseline: base,
        anchor: Anchor::Start,
        runs: if sx + sw + 24.0 + m.width(&addr(choice.0, choice.1)) <= x1 {
            station
        } else {
            let room = x1 - sx - 24.0 - m.width(&addr(choice.0, choice.1));
            one_line(
                m,
                &w.station,
                Style::new(pr.head_font, 700, INK),
                pr.head_font,
                room,
            )
        },
    });
    if choice.2 && pw > 0.0 {
        out.push(Shape::Text {
            x: sx + sw + 16.0,
            baseline: base,
            anchor: Anchor::Start,
            runs: place,
        });
    }
    out.push(Shape::Text {
        x: x1,
        baseline: base,
        anchor: Anchor::End,
        runs: addr(choice.0, choice.1),
    });
}

pub(crate) fn layout(
    w: &Words,
    m: &Measurer,
    fw: f32,
    fh: f32,
    show: Rgb,
    square: bool,
) -> Result<Vec<Shape>, CardError> {
    let pr = if square { &SQUARE } else { &LINK };
    let mut out = vec![Shape::Rect {
        x: 0.0,
        y: 0.0,
        w: fw,
        h: pr.rule,
        fill: show,
        decor: true,
    }];
    head(&mut out, w, m, pr, show, fw);
    let x0 = PAD_X;
    let x1 = fw - PAD_X;
    let body_top = pr.head_top + pr.head_h + pr.body_gap;
    let body_bottom = fh - pr.bottom;
    let has_people = !w.hosts.is_empty() || !w.guests.is_empty();

    if square {
        // The people in a row at the foot, a rule above them, the text above that.
        let mut text_bottom = body_bottom;
        if has_people {
            let groups = people(w, m, pr, x1 - x0, (body_bottom - body_top) * 0.45, true)?;
            let h = groups.iter().map(Group::height).fold(0.0, f32::max);
            let top = body_bottom - h;
            out.push(Shape::Rect {
                x: x0,
                y: top - 48.0 - 2.0,
                w: x1 - x0,
                h: 2.0,
                fill: GRID,
                decor: false,
            });
            let mut x = x0;
            for g in &groups {
                draw_group(&mut out, g, x, top);
                x += g.width() + 56.0;
            }
            text_bottom = top - 48.0 - 2.0 - 56.0;
        }
        let t = text_block(w, m, pr, true, show, x1 - x0, text_bottom - body_top)?;
        draw_text(&mut out, &t, pr, x0, body_top, text_bottom);
        return Ok(out);
    }

    // The link card: the text at the left, 13 parts to the people's 11, 40 apart.
    let inner = x1 - x0 - 40.0;
    let text_w = if has_people {
        inner * 13.0 / 24.0
    } else {
        x1 - x0
    };
    let t = text_block(w, m, pr, false, show, text_w, body_bottom - body_top)?;
    draw_text(&mut out, &t, pr, x0, body_top, body_bottom);
    if has_people {
        let col = x0 + text_w + 40.0;
        out.push(Shape::Rect {
            x: col,
            y: body_top,
            w: 2.0,
            h: body_bottom - body_top,
            fill: GRID,
            decor: false,
        });
        let px = col + 2.0 + 32.0;
        let groups = people(w, m, pr, x1 - px, body_bottom - body_top, false)?;
        let mut y = body_top;
        for g in &groups {
            draw_group(&mut out, g, px, y);
            y += g.height() + 22.0;
        }
    }
    Ok(out)
}
