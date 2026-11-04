// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Line breaking for cards: SVG has no wrapping, so the layouts break lines here, at spaces
//! only, measuring each candidate line as resvg will draw it. A name is one token and never
//! breaks inside, unless it alone is wider than the line.

use crate::scene::{Measurer, Run, Style};
use crate::words::ELLIPSIS;

/// A piece of text that does not break inside, and whether a breakable space comes before it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub runs: Vec<Run>,
    pub space_before: bool,
}

impl Token {
    pub fn new(runs: Vec<Run>, space_before: bool) -> Self {
        Self { runs, space_before }
    }
}

/// Plain text split at its spaces, one token a word.
pub(crate) fn words(text: &str, style: Style) -> Vec<Token> {
    // Breakable spaces only: a no-break space keeps "7 November 2026" on one line.
    text.split(|c: char| c.is_whitespace() && c != '\u{a0}')
        .filter(|w| !w.is_empty())
        .enumerate()
        .map(|(i, w)| Token::new(vec![Run::new(w, style)], i > 0))
        .collect()
}

/// A token for a whole name (it never breaks inside); the name may contain spaces.
pub(crate) fn whole(text: &str, style: Style, space_before: bool) -> Token {
    Token::new(vec![Run::new(text, style)], space_before)
}

/// The runs of one line made of these tokens: spaces between them, adjacent runs of the same
/// style merged.
pub(crate) fn join(tokens: &[Token]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        for (j, r) in t.runs.iter().enumerate() {
            let mut text = r.text.clone();
            if i > 0 && j == 0 && t.space_before {
                text.insert(0, ' ');
            }
            match out.last_mut() {
                Some(last) if last.style == r.style => last.text.push_str(&text),
                _ => out.push(Run::new(text, r.style)),
            }
        }
    }
    out
}

/// Split a token that is wider than a line at its own spaces, so a very long name can still
/// wrap rather than run off the card.
fn split_token(t: &Token) -> Vec<Token> {
    let mut out = Vec::new();
    for (j, r) in t.runs.iter().enumerate() {
        for (k, w) in r.text.split(' ').filter(|w| !w.is_empty()).enumerate() {
            let space = if j == 0 && k == 0 {
                t.space_before
            } else {
                k > 0 || r.text.starts_with(' ')
            };
            out.push(Token::new(vec![Run::new(w, r.style)], space));
        }
    }
    out
}

/// Greedy wrapping into lines no wider than `width`.
pub(crate) fn wrap(m: &Measurer, tokens: &[Token], width: f32) -> Vec<Vec<Token>> {
    let mut expanded: Vec<Token> = Vec::new();
    for t in tokens {
        if m.width(&join(std::slice::from_ref(t))) > width && split_token(t).len() > 1 {
            expanded.extend(split_token(t));
        } else {
            expanded.push(t.clone());
        }
    }
    let mut lines: Vec<Vec<Token>> = Vec::new();
    let mut line: Vec<Token> = Vec::new();
    for t in expanded {
        line.push(t);
        if line.len() > 1 && m.width(&join(&line)) > width {
            let t = line.pop().expect("just pushed");
            lines.push(std::mem::take(&mut line));
            line.push(t);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Wrapping with the lines evened out (CSS `text-wrap: balance`): the narrowest measure that
/// still gives the same number of lines.
pub(crate) fn wrap_balanced(m: &Measurer, tokens: &[Token], width: f32) -> Vec<Vec<Token>> {
    let greedy = wrap(m, tokens, width);
    let n = greedy.len();
    if n < 2 {
        return greedy;
    }
    let (mut lo, mut hi) = (width / n as f32, width);
    let mut best = greedy;
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        let attempt = wrap(m, tokens, mid);
        if attempt.len() <= n && widest(m, &attempt) <= mid + 0.5 {
            hi = mid;
            best = attempt;
        } else {
            lo = mid;
        }
    }
    best
}

/// The widest line.
pub(crate) fn widest(m: &Measurer, lines: &[Vec<Token>]) -> f32 {
    lines.iter().map(|l| m.width(&join(l))).fold(0.0, f32::max)
}

/// The first `max` lines, the last ending in an ellipsis when anything was cut.
pub(crate) fn clamp(
    m: &Measurer,
    mut lines: Vec<Vec<Token>>,
    max: usize,
    width: f32,
) -> Vec<Vec<Run>> {
    if lines.len() <= max {
        return lines.iter().map(|l| join(l)).collect();
    }
    lines.truncate(max);
    let mut last = lines.pop().expect("max is at least one");
    loop {
        let mut runs = join(&last);
        if let Some(r) = runs.last_mut() {
            r.text.push(ELLIPSIS);
        }
        if m.width(&runs) <= width || last.len() <= 1 {
            let mut out: Vec<Vec<Run>> = lines.iter().map(|l| join(l)).collect();
            out.push(runs);
            return out;
        }
        last.pop();
    }
}
