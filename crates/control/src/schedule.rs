// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The scheduler (ADR-0045): clocks fill each hour of the week from the library's categories;
//! rotation picks the track rested longest; separation rules keep an artist, a song and an
//! album apart. For a US statutory webcaster the sound recording performance complement (17
//! U.S.C. 114(j)(13): in any three hours at most 3 tracks from one album, 2 of them in a row,
//! and at most 4 by one artist, 3 in a row) is a hard rule the scheduler never relaxes.
//!
//! Pure: no I/O and no clock. The control plane stores what it makes (`schedule_item`), lets
//! the log editor change it with the same rules, and feeds playout from it (`onair.rs`).
//!
//! Time is milliseconds since the Unix epoch; a clock runs in the station's local hour, so a
//! DST Sunday has one hour fewer or one more, each with the clock of its local hour.

use std::collections::HashMap;

use jiff::tz::TimeZone;
use jiff::{RoundMode, Timestamp, Unit, ZonedRound};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MINUTE_MS: i64 = 60_000;
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
/// The statutory licence's window (17 U.S.C. 114(j)(13)).
pub const US_WINDOW_MS: i64 = 3 * HOUR_MS;
/// The statutory caps: per album (phonorecord) and per featured artist in three hours, and how
/// many of each may run in a row.
pub const US_ALBUM_MAX: usize = 3;
pub const US_ALBUM_IN_A_ROW: usize = 2;
pub const US_ARTIST_MAX: usize = 4;
pub const US_ARTIST_IN_A_ROW: usize = 3;
/// The longest separation a station may set, in minutes (a day).
pub const SEPARATION_MAX_MIN: u32 = 24 * 60;

/// A track as the scheduler needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    /// Empty for a single with no album: it is never grouped with others by album.
    pub album: String,
    pub category: String,
    pub duration_ms: u32,
}

/// One position in a clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ClockSlot)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Slot {
    /// The next track of a library category (`a`, `gold`, ...), by rotation.
    Category(String),
    /// A cart from the wall: a station ident, a sweeper.
    Cart(Uuid),
}

/// An hour's template: its slots in order, repeated until the hour is full.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    pub id: Uuid,
    pub name: String,
    pub slots: Vec<Slot>,
}

/// Which clock runs each local hour of the week: Monday is weekday 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Week {
    pub hours: HashMap<(u8, u8), Uuid>,
    /// The clock for every hour the grid leaves empty; none leaves those hours to the fallback.
    pub default_clock: Option<Uuid>,
}

impl Week {
    pub fn clock_for(&self, weekday: u8, hour: u8) -> Option<Uuid> {
        self.hours
            .get(&(weekday, hour))
            .copied()
            .or(self.default_clock)
    }
}

/// The station's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Minutes between two tracks by one artist, start to start.
    pub artist_separation_min: u32,
    /// Minutes between two plays of one song (title and artist).
    pub title_separation_min: u32,
    /// Minutes between two tracks from one album.
    pub album_separation_min: u32,
    /// A US webcaster under the statutory licence: the performance complement is enforced and
    /// listeners are never told what comes next (17 U.S.C. 114(d)(2)(C)(ii)).
    pub us_statutory: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            artist_separation_min: 60,
            title_separation_min: 180,
            album_separation_min: 60,
            us_statutory: false,
        }
    }
}

/// A rule a play would break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Breach {
    Artist,
    Title,
    Album,
    UsAlbumInThreeHours,
    UsAlbumInARow,
    UsArtistInThreeHours,
    UsArtistInARow,
}

impl Breach {
    /// A breach of the statutory licence: never relaxed, never accepted from the log editor
    /// without saying so.
    pub fn legal(self) -> bool {
        matches!(
            self,
            Breach::UsAlbumInThreeHours
                | Breach::UsAlbumInARow
                | Breach::UsArtistInThreeHours
                | Breach::UsArtistInARow
        )
    }

    pub fn describe(self) -> &'static str {
        match self {
            Breach::Artist => "artist separation",
            Breach::Title => "song separation",
            Breach::Album => "album separation",
            Breach::UsAlbumInThreeHours => "more than 3 tracks from one album in 3 hours (US)",
            Breach::UsAlbumInARow => "more than 2 tracks from one album in a row (US)",
            Breach::UsArtistInThreeHours => "more than 4 tracks by one artist in 3 hours (US)",
            Breach::UsArtistInARow => "more than 3 tracks by one artist in a row (US)",
        }
    }
}

/// Names compare without case and runs of spaces: "The  Blue Nile" is "the blue nile".
pub fn key(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A track's play, past or planned, as the rules see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Played {
    pub at_ms: i64,
    pub track: Uuid,
    artist: String,
    album: String,
    song: String,
}

impl Played {
    pub fn of(t: &Track, at_ms: i64) -> Self {
        Self::new(t.id, &t.title, &t.artist, &t.album, at_ms)
    }

    pub fn new(track: Uuid, title: &str, artist: &str, album: &str, at_ms: i64) -> Self {
        let artist = key(artist);
        Self {
            at_ms,
            track,
            song: format!("{}\u{1f}{artist}", key(title)),
            album: key(album),
            artist,
        }
    }
}

impl Rules {
    /// The longest look back any rule needs.
    pub fn horizon_ms(&self) -> i64 {
        let sep = self
            .artist_separation_min
            .max(self.title_separation_min)
            .max(self.album_separation_min);
        (i64::from(sep) * MINUTE_MS).max(if self.us_statutory { US_WINDOW_MS } else { 0 })
    }

    /// What playing `p` would break after `before`: the tracks played or planned before it, in
    /// time order (carts are not tracks and are not in it).
    pub fn breaches(&self, p: &Played, before: &[Played]) -> Vec<Breach> {
        let mut out = Vec::new();
        let within = |min: u32, q: &Played| q.at_ms > p.at_ms - i64::from(min) * MINUTE_MS;
        let earlier = before.iter().filter(|q| q.at_ms <= p.at_ms);
        let mut artist = false;
        let mut song = false;
        let mut album = false;
        for q in earlier.clone() {
            artist |= !p.artist.is_empty()
                && q.artist == p.artist
                && within(self.artist_separation_min, q);
            song |= q.song == p.song && within(self.title_separation_min, q);
            album |=
                !p.album.is_empty() && q.album == p.album && within(self.album_separation_min, q);
        }
        if artist {
            out.push(Breach::Artist);
        }
        if song {
            out.push(Breach::Title);
        }
        if album {
            out.push(Breach::Album);
        }
        if self.us_statutory {
            let window: Vec<&Played> = earlier
                .filter(|q| q.at_ms > p.at_ms - US_WINDOW_MS)
                .collect();
            if !p.album.is_empty()
                && window.iter().filter(|q| q.album == p.album).count() >= US_ALBUM_MAX
            {
                out.push(Breach::UsAlbumInThreeHours);
            }
            if window.iter().filter(|q| q.artist == p.artist).count() >= US_ARTIST_MAX {
                out.push(Breach::UsArtistInThreeHours);
            }
            let run = |n: usize, same: &dyn Fn(&Played) -> bool| {
                window.len() >= n && window[window.len() - n..].iter().all(|q| same(q))
            };
            if !p.album.is_empty() && run(US_ALBUM_IN_A_ROW, &|q| q.album == p.album) {
                out.push(Breach::UsAlbumInARow);
            }
            if !p.artist.is_empty() && run(US_ARTIST_IN_A_ROW, &|q| q.artist == p.artist) {
                out.push(Breach::UsArtistInARow);
            }
        }
        out
    }

    /// Every breach in a log of plays in time order, by index. Each window and each separation
    /// is checked from the play that closes it, so looking back from every play finds them all.
    pub fn verify(&self, log: &[Played]) -> Vec<(usize, Breach)> {
        let horizon = self.horizon_ms();
        let mut out = Vec::new();
        let mut start = 0;
        for (i, p) in log.iter().enumerate() {
            while start < i && log[start].at_ms <= p.at_ms - horizon {
                start += 1;
            }
            for b in self.breaches(p, &log[start..i]) {
                out.push((i, b));
            }
        }
        out
    }
}

/// What a planned item plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Track(Uuid),
    Cart(Uuid),
}

/// One entry of the generated log.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub planned_at_ms: i64,
    /// The start of the local hour whose clock placed it.
    pub hour_ms: i64,
    pub clock: Uuid,
    /// Its slot's index in the clock.
    pub slot: u16,
    pub kind: ItemKind,
    /// The category it filled; empty for a cart.
    pub category: String,
    pub duration_ms: u32,
    /// A rule relaxed to fill it, in words; empty when none was.
    pub note: String,
}

/// What a generation run could not do: a slot left empty, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    pub at_ms: i64,
    pub slot: u16,
    pub reason: String,
}

/// The library, the clocks and the grid, with the station's rules and time zone.
pub struct Generator<'a> {
    /// The enabled tracks.
    pub tracks: &'a [Track],
    pub clocks: &'a HashMap<Uuid, Clock>,
    /// Each cart a clock may name, with its length.
    pub carts: &'a HashMap<Uuid, u32>,
    pub week: &'a Week,
    pub rules: Rules,
    pub tz: TimeZone,
}

/// Where a generation run starts from: the log so far.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Where the log ends: the next item starts here (or at the hour, if later).
    pub cursor_ms: i64,
    /// Tracks played or planned before the cursor, in time order; only the rules' horizon is
    /// needed.
    pub history: Vec<Played>,
    /// When each track last played or is planned; a track never used rests longest.
    pub last_used: HashMap<Uuid, i64>,
}

/// A deterministic shuffle among tracks rested equally long, varied by the hour.
fn tiebreak(id: Uuid, hour_ms: i64) -> u64 {
    let mut z = (id.as_u128() as u64) ^ ((id.as_u128() >> 64) as u64) ^ (hour_ms as u64);
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The start of the local hour `at_ms` falls in, and its weekday (Monday 0) and hour.
pub fn local_hour(tz: &TimeZone, at_ms: i64) -> (i64, u8, u8) {
    let ts = Timestamp::from_millisecond(at_ms).unwrap_or(Timestamp::UNIX_EPOCH);
    let z = ts.to_zoned(tz.clone());
    let floor = z
        .round(
            ZonedRound::new()
                .smallest(Unit::Hour)
                .mode(RoundMode::Trunc),
        )
        .unwrap_or_else(|_| z.clone());
    (
        floor.timestamp().as_millisecond(),
        floor.weekday().to_monday_zero_offset() as u8,
        floor.hour() as u8,
    )
}

impl Generator<'_> {
    /// Fill every local hour that starts in `[from_ms, to_ms)` from the context; returns the new
    /// items in order, and the slots it had to leave empty. The context moves on with them, so
    /// the next run continues where this one stopped.
    pub fn generate(&self, from_ms: i64, to_ms: i64, ctx: &mut Context) -> (Vec<Item>, Vec<Gap>) {
        let mut items = Vec::new();
        let mut gaps = Vec::new();
        let mut by_category: HashMap<&str, Vec<&Track>> = HashMap::new();
        for t in self.tracks {
            by_category.entry(t.category.as_str()).or_default().push(t);
        }
        let horizon = self.rules.horizon_ms();
        let (mut hour, _, _) = local_hour(&self.tz, from_ms);
        if hour < from_ms {
            hour += HOUR_MS;
        }
        while hour < to_ms {
            let (_, weekday, h) = local_hour(&self.tz, hour);
            let end = hour + HOUR_MS;
            let clock = self
                .week
                .clock_for(weekday, h)
                .and_then(|id| self.clocks.get(&id));
            if let Some(clock) = clock.filter(|c| !c.slots.is_empty()) {
                let mut at = ctx.cursor_ms.max(hour);
                let mut i = 0usize;
                let mut placed_in_pass = false;
                while at < end {
                    let slot_index = i % clock.slots.len();
                    if slot_index == 0 && i > 0 {
                        if !placed_in_pass {
                            break;
                        }
                        placed_in_pass = false;
                    }
                    i += 1;
                    let slot = slot_index as u16;
                    match &clock.slots[slot_index] {
                        Slot::Cart(id) => match self.carts.get(id) {
                            Some(&ms) if ms > 0 => {
                                items.push(Item {
                                    planned_at_ms: at,
                                    hour_ms: hour,
                                    clock: clock.id,
                                    slot,
                                    kind: ItemKind::Cart(*id),
                                    category: String::new(),
                                    duration_ms: ms,
                                    note: String::new(),
                                });
                                at += i64::from(ms);
                                placed_in_pass = true;
                            }
                            _ => gaps.push(Gap {
                                at_ms: at,
                                slot,
                                reason: format!("cart {id} is not on the wall or has no file"),
                            }),
                        },
                        Slot::Category(cat) => {
                            let pool = by_category
                                .get(cat.as_str())
                                .map(Vec::as_slice)
                                .unwrap_or(&[]);
                            let first = ctx.history.partition_point(|p| p.at_ms <= at - horizon);
                            let recent = &ctx.history[first..];
                            match self.pick(pool, at, hour, recent, &ctx.last_used) {
                                Ok((t, note)) => {
                                    items.push(Item {
                                        planned_at_ms: at,
                                        hour_ms: hour,
                                        clock: clock.id,
                                        slot,
                                        kind: ItemKind::Track(t.id),
                                        category: cat.clone(),
                                        duration_ms: t.duration_ms,
                                        note,
                                    });
                                    ctx.history.push(Played::of(t, at));
                                    ctx.last_used.insert(t.id, at);
                                    at += i64::from(t.duration_ms.max(1000));
                                    placed_in_pass = true;
                                }
                                Err(reason) => gaps.push(Gap {
                                    at_ms: at,
                                    slot,
                                    reason,
                                }),
                            }
                        }
                    }
                }
                ctx.cursor_ms = at;
            }
            hour = end;
        }
        // Keep only what the rules can still see.
        let keep_from = ctx.cursor_ms - horizon;
        let first = ctx.history.partition_point(|p| p.at_ms <= keep_from);
        ctx.history.drain(..first);
        (items, gaps)
    }

    /// Rotation: the track rested longest that breaks no rule. When every track breaks one, the
    /// station's own separations are relaxed (the item says which), but never the statutory
    /// licence's.
    fn pick<'t>(
        &self,
        pool: &[&'t Track],
        at: i64,
        hour: i64,
        recent: &[Played],
        last_used: &HashMap<Uuid, i64>,
    ) -> Result<(&'t Track, String), String> {
        if pool.is_empty() {
            return Err("the category has no tracks".into());
        }
        let mut ranked: Vec<&&Track> = pool.iter().collect();
        ranked.sort_by_key(|t| {
            (
                last_used.get(&t.id).copied().unwrap_or(i64::MIN),
                tiebreak(t.id, hour),
            )
        });
        let mut relaxed: Option<(&Track, Vec<Breach>)> = None;
        for t in ranked {
            let b = self.rules.breaches(&Played::of(t, at), recent);
            if b.is_empty() {
                return Ok((t, String::new()));
            }
            let better = match &relaxed {
                None => true,
                Some((_, rb)) => b.len() < rb.len(),
            };
            if !b.iter().any(|x| x.legal()) && better {
                relaxed = Some((t, b));
            }
        }
        match relaxed {
            Some((t, b)) => Ok((
                t,
                format!(
                    "relaxed: {}",
                    b.iter()
                        .map(|x| x.describe())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
            None => Err("every track in the category would break the US statutory licence".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// A library of `artists` artists with `albums` albums of `tracks` tracks each, of 3:30.
    fn library(
        artists: u128,
        albums: u128,
        tracks: u128,
        category: &str,
        base: u128,
    ) -> Vec<Track> {
        let mut v = Vec::new();
        for a in 0..artists {
            for b in 0..albums {
                for t in 0..tracks {
                    v.push(Track {
                        id: id(base + a * 10_000 + b * 100 + t),
                        title: format!("Song {a}-{b}-{t}"),
                        artist: format!("Artist {category}{a}"),
                        album: format!("Album {category}{a}-{b}"),
                        category: category.to_owned(),
                        duration_ms: 180_000 + ((a * 7 + b * 3 + t) % 5) as u32 * 15_000,
                    });
                }
            }
        }
        v
    }

    fn ms(s: &str) -> i64 {
        s.parse::<Timestamp>().unwrap().as_millisecond()
    }

    #[test]
    fn separation_is_start_to_start_and_names_ignore_case() {
        let r = Rules::default();
        let a = Played::new(id(1), "Song", "The Blue  Nile", "Hats", 0);
        let later = |m: i64, artist: &str, album: &str, title: &str| {
            Played::new(id(2), title, artist, album, m * MINUTE_MS)
        };
        assert_eq!(
            r.breaches(
                &later(59, "the blue nile", "", "Other"),
                std::slice::from_ref(&a)
            ),
            vec![Breach::Artist]
        );
        assert!(
            r.breaches(
                &later(60, "the blue nile", "", "Other"),
                std::slice::from_ref(&a)
            )
            .is_empty()
        );
        assert_eq!(
            r.breaches(&later(100, "Someone", "", "Song"), std::slice::from_ref(&a)),
            vec![]
        );
        assert_eq!(
            r.breaches(
                &later(100, "The Blue Nile", "", "song"),
                std::slice::from_ref(&a)
            ),
            vec![Breach::Title]
        );
        assert_eq!(
            r.breaches(&later(30, "Another", "hats", "X"), &[a]),
            vec![Breach::Album]
        );
    }

    #[test]
    fn the_us_complement_counts_three_hours_and_runs() {
        let r = Rules {
            artist_separation_min: 0,
            title_separation_min: 0,
            album_separation_min: 0,
            us_statutory: true,
        };
        let p = |i: i64, artist: &str, album: &str| {
            Played::new(
                id(i as u128),
                &format!("t{i}"),
                artist,
                album,
                i * 10 * MINUTE_MS,
            )
        };
        // Two from one album in a row: a third in a row breaks it.
        let log = vec![p(0, "A", "X"), p(1, "A", "X")];
        assert!(
            r.breaches(&p(2, "A", "X"), &log)
                .contains(&Breach::UsAlbumInARow)
        );
        // Three from one album in three hours, not in a row: a fourth breaks it.
        let log = vec![
            p(0, "A", "X"),
            p(1, "B", "Y"),
            p(2, "A", "X"),
            p(3, "B", "Y"),
            p(4, "A", "X"),
        ];
        let b = r.breaches(&p(5, "C", "X"), &log);
        assert_eq!(b, vec![Breach::UsAlbumInThreeHours]);
        // After three hours the window has moved on.
        assert!(r.breaches(&p(19, "C", "X"), &log).is_empty());
        // Three by one artist in a row; four in three hours.
        let log = vec![p(0, "A", "1"), p(1, "A", "2"), p(2, "A", "3")];
        assert!(
            r.breaches(&p(3, "A", "4"), &log)
                .contains(&Breach::UsArtistInARow)
        );
        let log = vec![
            p(0, "A", "1"),
            p(1, "A", "2"),
            p(2, "B", "3"),
            p(3, "A", "4"),
            p(4, "A", "5"),
        ];
        assert!(
            r.breaches(&p(5, "A", "6"), &log)
                .contains(&Breach::UsArtistInThreeHours)
        );
        // Off for a station that is not a US statutory webcaster.
        let off = Rules {
            us_statutory: false,
            ..r
        };
        assert!(off.breaches(&p(5, "A", "6"), &log).is_empty());
    }

    fn station() -> (Vec<Track>, HashMap<Uuid, Clock>, HashMap<Uuid, u32>, Week) {
        let mut tracks = library(30, 2, 5, "a", 1_000_000);
        tracks.extend(library(20, 3, 4, "b", 2_000_000));
        tracks.extend(library(25, 2, 4, "gold", 3_000_000));
        let ident = id(77);
        let music = Clock {
            id: id(10),
            name: "Music".into(),
            slots: vec![
                Slot::Cart(ident),
                Slot::Category("a".into()),
                Slot::Category("b".into()),
                Slot::Category("a".into()),
                Slot::Category("gold".into()),
            ],
        };
        let gold = Clock {
            id: id(11),
            name: "Gold".into(),
            slots: vec![Slot::Category("gold".into()), Slot::Category("a".into())],
        };
        let mut week = Week {
            default_clock: Some(music.id),
            ..Week::default()
        };
        for h in 0..6 {
            week.hours.insert((6, h), gold.id);
        }
        let clocks = HashMap::from([(music.id, music), (gold.id, gold)]);
        let carts = HashMap::from([(ident, 8_000u32)]);
        (tracks, clocks, carts, week)
    }

    #[test]
    fn a_generated_week_breaks_no_rule() {
        let (tracks, clocks, carts, week) = station();
        let tz = TimeZone::get("America/New_York").unwrap();
        let rules = Rules {
            us_statutory: true,
            ..Rules::default()
        };
        let g = Generator {
            tracks: &tracks,
            clocks: &clocks,
            carts: &carts,
            week: &week,
            rules,
            tz,
        };
        let from = ms("2026-11-02T05:00:00Z");
        let mut ctx = Context {
            cursor_ms: from,
            ..Context::default()
        };
        let (items, gaps) = g.generate(from, from + 7 * 24 * HOUR_MS, &mut ctx);
        assert!(gaps.is_empty(), "gaps: {gaps:?}");
        assert!(
            items.iter().all(|i| i.note.is_empty()),
            "a rule was relaxed"
        );
        // Every hour of the week is filled, and the log has no holes or overlaps.
        assert!(items.len() > 7 * 24 * 12, "only {} items", items.len());
        for w in items.windows(2) {
            assert_eq!(
                w[1].planned_at_ms,
                w[0].planned_at_ms + i64::from(w[0].duration_ms)
            );
        }
        let by_id: HashMap<Uuid, &Track> = tracks.iter().map(|t| (t.id, t)).collect();
        let plays: Vec<Played> = items
            .iter()
            .filter_map(|i| match i.kind {
                ItemKind::Track(t) => Some(Played::of(by_id[&t], i.planned_at_ms)),
                ItemKind::Cart(_) => None,
            })
            .collect();
        assert_eq!(rules.verify(&plays), vec![]);
        // Sunday's small hours (local) ran the gold clock.
        let sunday_3am = ms("2026-11-08T08:00:00Z");
        let first = items.iter().find(|i| i.hour_ms == sunday_3am).unwrap();
        assert_eq!(first.clock, id(11));
        // Rotation spread the plays: no track of category a played more than twice as often
        // as the least played.
        let mut counts: HashMap<Uuid, usize> = HashMap::new();
        for i in &items {
            if let ItemKind::Track(t) = i.kind
                && i.category == "a"
            {
                *counts.entry(t).or_default() += 1;
            }
        }
        let (lo, hi) = (
            counts.values().min().unwrap(),
            counts.values().max().unwrap(),
        );
        assert_eq!(counts.len(), 300, "every track in category a played");
        assert!(*hi <= lo * 2 + 1, "rotation uneven: {lo} to {hi}");
    }

    #[test]
    fn a_dst_sunday_runs_each_local_hours_clock() {
        // Europe/London, 2026-03-29: 01:00 GMT becomes 02:00 BST, so local 01:00 never happens.
        let (tracks, clocks, carts, mut week) = station();
        week.hours.clear();
        week.default_clock = None;
        for h in 0..24u8 {
            week.hours
                .insert((6, h), if h == 2 { id(11) } else { id(10) });
        }
        let g = Generator {
            tracks: &tracks,
            clocks: &clocks,
            carts: &carts,
            week: &week,
            rules: Rules::default(),
            tz: TimeZone::get("Europe/London").unwrap(),
        };
        let from = ms("2026-03-29T00:00:00Z");
        let mut ctx = Context {
            cursor_ms: from,
            ..Context::default()
        };
        let (items, _) = g.generate(from, from + 3 * HOUR_MS, &mut ctx);
        let hours: Vec<i64> = {
            let mut h: Vec<i64> = items.iter().map(|i| i.hour_ms).collect();
            h.dedup();
            h
        };
        // 00:00 GMT (local 00:00), 01:00 GMT (local 02:00 BST), 02:00 GMT (local 03:00 BST).
        assert_eq!(hours, vec![from, from + HOUR_MS, from + 2 * HOUR_MS]);
        let clock_of = |h: i64| items.iter().find(|i| i.hour_ms == h).unwrap().clock;
        assert_eq!(clock_of(from), id(10));
        assert_eq!(
            clock_of(from + HOUR_MS),
            id(11),
            "local 02:00 runs the 02:00 clock"
        );
        assert_eq!(clock_of(from + 2 * HOUR_MS), id(10));
        // The autumn change repeats local 01:00: both hours run its clock.
        let autumn = ms("2026-10-25T00:00:00Z");
        assert_eq!(local_hour(&g.tz, autumn).2, 1);
        assert_eq!(local_hour(&g.tz, autumn + HOUR_MS).2, 1);
    }

    #[test]
    fn rotation_rests_the_longest_played_and_relaxes_only_the_stations_rules() {
        let tracks = vec![
            Track {
                id: id(1),
                title: "One".into(),
                artist: "Same".into(),
                album: String::new(),
                category: "a".into(),
                duration_ms: 200_000,
            },
            Track {
                id: id(2),
                title: "Two".into(),
                artist: "Same".into(),
                album: String::new(),
                category: "a".into(),
                duration_ms: 200_000,
            },
        ];
        let clock = Clock {
            id: id(10),
            name: "a".into(),
            slots: vec![Slot::Category("a".into())],
        };
        let clocks = HashMap::from([(clock.id, clock)]);
        let week = Week {
            default_clock: Some(id(10)),
            ..Week::default()
        };
        let carts = HashMap::new();
        let mut g = Generator {
            tracks: &tracks,
            clocks: &clocks,
            carts: &carts,
            week: &week,
            rules: Rules::default(),
            tz: TimeZone::UTC,
        };
        let mut ctx = Context {
            cursor_ms: 0,
            last_used: HashMap::from([(id(1), -10 * HOUR_MS), (id(2), -20 * HOUR_MS)]),
            ..Context::default()
        };
        let (items, gaps) = g.generate(0, HOUR_MS, &mut ctx);
        assert!(gaps.is_empty());
        assert_eq!(
            items[0].kind,
            ItemKind::Track(id(2)),
            "the one rested longest first"
        );
        assert!(items[0].note.is_empty());
        assert!(
            items[1].note.contains("artist separation"),
            "{:?}",
            items[1]
        );
        // Under the statutory licence four in a row by one artist is never placed.
        g.rules.us_statutory = true;
        let mut ctx = Context::default();
        let (items, gaps) = g.generate(0, HOUR_MS, &mut ctx);
        assert_eq!(items.len(), 3, "three in a row, then the slot stays empty");
        assert!(!gaps.is_empty());
    }
}
