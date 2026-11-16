// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A calendar file read into events: the `icalendar` crate reads the content lines (unfolding,
//! parameters, nested components); this module reads what the properties mean.

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use super::rrule;

/// The most a file may hold.
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_EVENTS: usize = 200;
/// How deep components may nest.
const MAX_DEPTH: usize = 8;

/// Why a file cannot be read at all: a sentence for a person.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IcsError {
    #[error(
        "The file is larger than 1 MB. Export a shorter range of dates, or one calendar at a time."
    )]
    TooLarge,
    #[error(
        "The file holds {0} events; at most 200 can be read at once. Export a shorter range of dates."
    )]
    TooManyEvents(usize),
    #[error("{0}")]
    NotCalendar(String),
    #[error("The file holds no events.")]
    NoEvents,
}

/// A date or a date-time as a property gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    /// `VALUE=DATE`: a whole day.
    Date(Date),
    /// No zone: the station's wall clock.
    Floating(DateTime),
    /// `…Z`.
    Utc(Timestamp),
    /// `TZID=…`, the zone already found.
    Zoned(DateTime, Zone),
}

/// A zone named by `TZID`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone {
    /// The name as the file gave it.
    pub tzid: String,
    /// The IANA name it was read as.
    pub iana: String,
}

impl When {
    /// The wall clock it gives in its own zone; a whole day is its midnight.
    pub fn local(&self) -> DateTime {
        match self {
            When::Date(d) => d.at(0, 0, 0, 0),
            When::Floating(dt) | When::Zoned(dt, _) => *dt,
            When::Utc(t) => t.to_zoned(TimeZone::UTC).datetime(),
        }
    }
}

/// One VEVENT, read.
#[derive(Clone, Debug)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: String,
    pub description: String,
    pub start: When,
    pub end: Option<When>,
    pub duration: Option<SignedDuration>,
    pub rrule: Option<String>,
    pub rdates: Vec<When>,
    /// `RDATE;VALUE=PERIOD`: a date with its own length.
    pub rdate_lengths: Vec<(When, SignedDuration)>,
    pub exdates: Vec<When>,
    pub recurrence_id: Option<When>,
    pub cancelled: bool,
    /// A zone the file named that we could not read, if any.
    pub unknown_zone: Option<String>,
}

/// The events in a calendar file. `text` is the file as received.
pub fn events(bytes: &[u8]) -> Result<Vec<Event>, IcsError> {
    if bytes.len() > MAX_BYTES {
        return Err(IcsError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        IcsError::NotCalendar(
            "The file is not text in UTF-8, so it is not a calendar file we can read.".into(),
        )
    })?;
    let text = text.trim_start_matches('\u{feff}');
    if !text
        .trim_start()
        .get(..15)
        .is_some_and(|h| h.eq_ignore_ascii_case("BEGIN:VCALENDAR"))
    {
        return Err(IcsError::NotCalendar(
            "This is not a calendar file: it does not begin with BEGIN:VCALENDAR.".into(),
        ));
    }
    // Every line ends in CRLF for the parser, blank lines are dropped, and every property's
    // name is upper-cased: names are case-insensitive (RFC 5545 section 2), BEGIN and END must
    // match each other exactly for the parser, and it unescapes text properties only by their
    // upper-case names.
    let unfolded = icalendar::parser::unfold(&text.replace("\r\n", "\n").replace('\r', "\n"));
    let mut clean = String::with_capacity(unfolded.len() + 1024);
    // A calendar nests three deep (VCALENDAR, VEVENT, VALARM; VTIMEZONE, STANDARD); the parser
    // recurses once per level, so a file nested far deeper is refused before it is parsed.
    let mut depth = 0usize;
    for line in unfolded.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("BEGIN:") {
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(IcsError::NotCalendar(
                    "The calendar file cannot be read: its parts are nested far more deeply than a calendar's are.".into(),
                ));
            }
        } else if upper.starts_with("END:") {
            depth = depth.saturating_sub(1);
        }
        if upper.starts_with("BEGIN:") || upper.starts_with("END:") {
            clean.push_str(&upper);
        } else {
            let cut = line.find([';', ':']).unwrap_or(line.len());
            clean.push_str(&line[..cut].to_ascii_uppercase());
            clean.push_str(&line[cut..]);
        }
        clean.push_str("\r\n");
    }
    let calendar = icalendar::parser::read_calendar(&clean).map_err(|e| {
        // The parser's own message names the line in the unfolded text; a person needs the
        // line's number.
        let line = e
            .lines()
            .find_map(|l| l.trim().strip_prefix("at line ")?.split(':').next()?.trim().parse::<usize>().ok());
        IcsError::NotCalendar(match line {
            Some(n) => format!("The calendar file cannot be read: line {n} (counted after folded lines are joined) is not a content line we understand."),
            None => "The calendar file cannot be read: its lines are not in iCalendar's form.".into(),
        })
    })?;
    let vevents: Vec<&icalendar::parser::Component<'_>> = calendar
        .components
        .iter()
        .filter(|c| c.name.as_str().eq_ignore_ascii_case("VEVENT"))
        .collect();
    if vevents.len() > MAX_EVENTS {
        return Err(IcsError::TooManyEvents(vevents.len()));
    }
    if vevents.is_empty() {
        return Err(IcsError::NoEvents);
    }
    vevents.into_iter().map(event).collect()
}

/// A property's parameter, by name (case-insensitive).
fn param<'a>(p: &'a icalendar::parser::Property<'_>, name: &str) -> Option<&'a str> {
    p.params
        .iter()
        .find(|x| x.key.as_str().eq_ignore_ascii_case(name))
        .and_then(|x| x.val.as_ref())
        .map(|v| v.as_str().trim_matches('"'))
}

fn bad(what: &str) -> IcsError {
    IcsError::NotCalendar(format!("The calendar file cannot be read: {what}."))
}

fn event(c: &icalendar::parser::Component<'_>) -> Result<Event, IcsError> {
    let mut e = Event {
        uid: None,
        summary: String::new(),
        description: String::new(),
        start: When::Date(Date::constant(1970, 1, 1)),
        end: None,
        duration: None,
        rrule: None,
        rdates: Vec::new(),
        rdate_lengths: Vec::new(),
        exdates: Vec::new(),
        recurrence_id: None,
        cancelled: false,
        unknown_zone: None,
    };
    let mut has_start = false;
    for p in &c.properties {
        let name = p.name.as_str().to_ascii_uppercase();
        let v = p.val.as_str();
        match name.as_str() {
            "UID" => e.uid = Some(v.trim().to_owned()),
            "SUMMARY" => e.summary = v.trim().to_owned(),
            "DESCRIPTION" => e.description = v.trim().to_owned(),
            "STATUS" => e.cancelled = v.trim().eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                e.start = when(p, v.trim(), &mut e.unknown_zone)
                    .ok_or_else(|| bad("an event's DTSTART is not a date or a time"))?;
                has_start = true;
            }
            "DTEND" => {
                e.end = Some(
                    when(p, v.trim(), &mut e.unknown_zone)
                        .ok_or_else(|| bad("an event's DTEND is not a date or a time"))?,
                )
            }
            "DURATION" => {
                e.duration = Some(
                    duration(v.trim()).ok_or_else(|| bad("an event's DURATION is not a length"))?,
                )
            }
            "RRULE" => e.rrule = Some(v.trim().to_owned()),
            "RDATE" => {
                let period = param(p, "VALUE").is_some_and(|x| x.eq_ignore_ascii_case("PERIOD"));
                for part in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    if period {
                        let (a, b) = part
                            .split_once('/')
                            .ok_or_else(|| bad("an RDATE period has no end"))?;
                        let start = when(p, a, &mut e.unknown_zone)
                            .ok_or_else(|| bad("an RDATE is not a time"))?;
                        let len = match duration(b) {
                            Some(d) => d,
                            None => {
                                let end = when(p, b, &mut e.unknown_zone)
                                    .ok_or_else(|| bad("an RDATE period's end is not a time"))?;
                                elapsed(&start, &end)
                                    .ok_or_else(|| bad("an RDATE period's end is not a time"))?
                            }
                        };
                        e.rdate_lengths.push((start, len));
                    } else {
                        e.rdates.push(
                            when(p, part, &mut e.unknown_zone)
                                .ok_or_else(|| bad("an RDATE is not a date or a time"))?,
                        );
                    }
                }
            }
            "EXDATE" => {
                for part in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    e.exdates.push(
                        when(p, part, &mut e.unknown_zone)
                            .ok_or_else(|| bad("an EXDATE is not a date or a time"))?,
                    );
                }
            }
            "RECURRENCE-ID" => {
                e.recurrence_id = Some(
                    when(p, v.trim(), &mut e.unknown_zone)
                        .ok_or_else(|| bad("a RECURRENCE-ID is not a date or a time"))?,
                )
            }
            _ => {}
        }
    }
    if !has_start {
        return Err(bad("an event has no DTSTART"));
    }
    Ok(e)
}

/// A value of DTSTART, DTEND, RDATE, EXDATE or RECURRENCE-ID. A zone that cannot be read is
/// kept in `unknown` and the time read as floating, so the caller can refuse the event by name.
fn when(
    p: &icalendar::parser::Property<'_>,
    v: &str,
    unknown: &mut Option<String>,
) -> Option<When> {
    let is_date = param(p, "VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE"));
    if is_date || (v.len() == 8 && !v.contains('T')) {
        return rrule::date(v).map(When::Date);
    }
    if let Some(t) = v.strip_suffix(['Z', 'z']) {
        let dt = rrule::datetime(t)?;
        return Some(When::Utc(dt.to_zoned(TimeZone::UTC).ok()?.timestamp()));
    }
    let dt = rrule::datetime(v)?;
    match param(p, "TZID") {
        None => Some(When::Floating(dt)),
        Some(tzid) => match zone(tzid) {
            Some(iana) => Some(When::Zoned(
                dt,
                Zone {
                    tzid: tzid.to_owned(),
                    iana,
                },
            )),
            None => {
                *unknown = Some(tzid.to_owned());
                Some(When::Floating(dt))
            }
        },
    }
}

/// The IANA zone a TZID names: the name itself, the name at the end of a path
/// (`/mozilla.org/20070129_1/Europe/London`, `/citadel.org/…/Europe/London`), or a Windows name
/// (`GMT Standard Time`), which Outlook writes.
pub fn zone(tzid: &str) -> Option<String> {
    let tzid = tzid.trim();
    if TimeZone::get(tzid).is_ok() {
        return Some(tzid.to_owned());
    }
    let parts: Vec<&str> = tzid.split('/').filter(|p| !p.is_empty()).collect();
    for n in [3, 2] {
        if parts.len() >= n {
            let tail = parts[parts.len() - n..].join("/");
            if TimeZone::get(&tail).is_ok() {
                return Some(tail);
            }
        }
    }
    let windows = windows_zone(tzid)?;
    TimeZone::get(windows).is_ok().then(|| windows.to_owned())
}

/// Common Windows zone names, as CLDR's `windowsZones.xml` maps them (territory 001).
fn windows_zone(name: &str) -> Option<&'static str> {
    Some(match name {
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "Romance Standard Time" => "Europe/Paris",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Russian Standard Time" => "Europe/Moscow",
        "Turkey Standard Time" => "Europe/Istanbul",
        "Israel Standard Time" => "Asia/Jerusalem",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "Arabian Standard Time" => "Asia/Dubai",
        "India Standard Time" => "Asia/Calcutta",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "E. Australia Standard Time" => "Australia/Brisbane",
        "Cen. Australia Standard Time" => "Australia/Adelaide",
        "W. Australia Standard Time" => "Australia/Perth",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "US Mountain Standard Time" => "America/Phoenix",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Alaskan Standard Time" => "America/Anchorage",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Atlantic Standard Time" => "America/Halifax",
        "Newfoundland Standard Time" => "America/St_Johns",
        "Canada Central Standard Time" => "America/Regina",
        "SA Pacific Standard Time" => "America/Bogota",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Argentina Standard Time" => "America/Buenos_Aires",
        "Pacific SA Standard Time" => "America/Santiago",
        "Central Standard Time (Mexico)" => "America/Mexico_City",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    })
}

/// `PT1H30M`, `P1D`, `-PT15M`, `P1W`: a length. Days and weeks are taken as 24 hours each.
pub(crate) fn duration(v: &str) -> Option<SignedDuration> {
    let (neg, rest) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let rest = rest.strip_prefix(['P', 'p'])?;
    let mut secs: i64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            u => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                any = true;
                secs += n * match (u, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    if !num.is_empty() || !any {
        return None;
    }
    Some(SignedDuration::from_secs(if neg { -secs } else { secs }))
}

/// The time between two values, each read in its own zone (a floating time in UTC, which is
/// right when both are floating).
pub(crate) fn elapsed(a: &When, b: &When) -> Option<SignedDuration> {
    let t = |w: &When| -> Option<Timestamp> {
        Some(match w {
            When::Utc(t) => *t,
            When::Zoned(dt, z) => TimeZone::get(&z.iana)
                .ok()?
                .to_ambiguous_zoned(*dt)
                .compatible()
                .ok()?
                .timestamp(),
            other => other.local().to_zoned(TimeZone::UTC).ok()?.timestamp(),
        })
    };
    Some(t(b)?.duration_since(t(a)?))
}
