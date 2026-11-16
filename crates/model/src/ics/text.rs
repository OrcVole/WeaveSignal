// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Writing content lines (RFC 5545 section 3.1): escaped text, lines folded at 75 octets without
//! splitting a character, CRLF; and the date and time forms.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::Offset;

/// A calendar file being written, one content line at a time.
#[derive(Default)]
pub(crate) struct Out {
    buf: String,
}

impl Out {
    /// One content line, folded: no line longer than 75 octets, the break never inside a
    /// character, each continuation starting with a space.
    pub(crate) fn line(&mut self, line: &str) {
        let mut used = 0usize;
        for c in line.chars() {
            let n = c.len_utf8();
            if used + n > 75 {
                self.buf.push_str("\r\n ");
                used = 1;
            }
            self.buf.push(c);
            used += n;
        }
        self.buf.push_str("\r\n");
    }

    /// `NAME:value`, the value written as given.
    pub(crate) fn prop(&mut self, name: &str, value: &str) {
        self.line(&format!("{name}:{value}"));
    }

    /// `NAME:text`, the text escaped.
    pub(crate) fn text(&mut self, name: &str, value: &str) {
        self.line(&format!("{name}:{}", escape(value)));
    }

    pub(crate) fn finish(self) -> String {
        self.buf
    }
}

/// TEXT as RFC 5545 section 3.3.11 has it: backslash, semicolon and comma escaped, every line
/// break as `\n`, other control characters left out.
pub(crate) fn escape(s: &str) -> String {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// `20270105T200000`: a local date and time.
pub(crate) fn local(dt: DateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

/// `20270105T200000Z`.
pub(crate) fn utc(t: Timestamp) -> String {
    format!("{}Z", local(t.to_zoned(jiff::tz::TimeZone::UTC).datetime()))
}

/// `+0100`, `-0430`; seconds only when the offset has them.
pub(crate) fn offset(o: Offset) -> String {
    let s = o.seconds();
    let sign = if s < 0 { '-' } else { '+' };
    let a = s.unsigned_abs();
    let (h, m, sec) = (a / 3600, (a / 60) % 60, a % 60);
    if sec == 0 {
        format!("{sign}{h:02}{m:02}")
    } else {
        format!("{sign}{h:02}{m:02}{sec:02}")
    }
}
