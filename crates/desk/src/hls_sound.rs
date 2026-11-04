// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video stream's sound (ADR-0172): the desk's delayed programme while someone is live,
//! playout's output (the feed back, [`crate::feed`]) otherwise, so viewers hear what
//! listeners hear at all times, and lip sync holds where there are lips.
//!
//! **Live** is the desk's own state: someone in the room whose role can air has their strip
//! open on air (ADR-0062's OPEN or ON AIR). Muting does not count as going off: a cough key
//! or a muted microphone under a record would otherwise flip the sound to playout and back.
//!
//! **Through the delay.** What the HLS carries is the programme after the profanity delay,
//! so live is judged there too: a strip opened now is live for viewers once the delay has
//! played out what came before it, and a dump brings it forward with the sound. Until then
//! viewers hear playout, which itself goes on with automation until the desk's sound reaches
//! it, as listeners do.
//!
//! **Off air.** Playout's copy of the desk arrives a little after the desk's own, so the desk
//! stays for `off_air_hold_ms` after the last strip closes (as heard after the delay), and
//! the show's last words are not heard twice.
//!
//! **The switch** is an equal-power crossfade, as is any switch to the desk when the feed goes
//! away (playout stopped or restarting: the desk's programme, never nothing).
//!
//! **The station's mode** (ADR-0045, ADR-0183), which the control plane tells the desk: in
//! auto mode automation holds the air and playout never takes the desk, so the sound is
//! playout's feed whoever is live. Playout switches the moment its mode changes, so the
//! mode is not judged through the delay, and leaving live for auto needs no hold: playout
//! never carried the desk's last words. Back in live mode the desk's own rule applies at once:
//! a strip open (as heard after the delay) is the desk's sound. Without a feed the sound is the
//! desk's programme in either mode. The desk starts in live mode until it is told otherwise.

use std::collections::VecDeque;

/// What the video stream's sound is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The desk's delayed programme.
    Desk,
    /// Playout's output, fed back.
    Playout,
}

/// The station's mode, as playout runs it (`programme.mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StationMode {
    /// The desk first, automation on dead air.
    #[default]
    Live,
    /// Automation only: the desk never airs.
    Auto,
}

impl StationMode {
    /// The socket's word for it: `live` or `auto`.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "live" => Some(StationMode::Live),
            "auto" => Some(StationMode::Auto),
            _ => None,
        }
    }
}

pub struct HlsSound {
    channels: usize,
    mode: StationMode,
    /// The crossfade's step per frame.
    step: f32,
    hold: u64,
    /// When live changed, by the frame of the programme it changed at, entering the delay.
    changes: VecDeque<(u64, bool)>,
    /// Frames that have entered the delay.
    fed: u64,
    last_in: bool,
    /// Live, as heard after the delay.
    live_out: bool,
    /// Frames since live (after the delay) last ended.
    off_for: u64,
    /// Where the crossfade is: 0 the desk, 1 playout.
    x: f32,
}

impl HlsSound {
    /// The crossfade's length and the hold after off air, in frames. The sound starts on the
    /// desk, and moves to playout's feed as soon as there is one and nobody is live.
    pub fn new(channels: usize, crossfade: usize, hold: usize) -> Self {
        HlsSound {
            channels: channels.max(1),
            mode: StationMode::Live,
            step: 1.0 / crossfade.max(1) as f32,
            hold: hold as u64,
            changes: VecDeque::new(),
            fed: 0,
            last_in: false,
            live_out: false,
            off_for: hold as u64,
            x: 0.0,
        }
    }

    /// One block. `live`: whether someone is live at the desk now, for the block that has just
    /// entered the delay; `held`: the frames the delay holds after it; `desk`: the delay's
    /// output; `feed`: playout's output for the same block, when it is there. `out` gets the
    /// video stream's sound.
    pub fn render(
        &mut self,
        live: bool,
        held: u64,
        desk: &[f32],
        feed: Option<&[f32]>,
        out: &mut Vec<f32>,
    ) {
        let ch = self.channels;
        let n = desk.len() / ch;
        if live != self.last_in {
            self.changes.push_back((self.fed, live));
            self.last_in = live;
        }
        self.fed += n as u64;
        // What has left the delay by the end of this block.
        let out_to = self.fed.saturating_sub(held);
        while let Some(&(at, v)) = self.changes.front() {
            if at >= out_to {
                break;
            }
            self.live_out = v;
            self.off_for = 0;
            self.changes.pop_front();
        }
        if !self.live_out {
            self.off_for = self.off_for.saturating_add(n as u64);
        }
        let to_feed = feed.is_some()
            && (self.mode == StationMode::Auto || (!self.live_out && self.off_for >= self.hold));
        let target = if to_feed { 1.0 } else { 0.0 };
        out.clear();
        out.resize(desk.len(), 0.0);
        for i in 0..n {
            if self.x < target {
                self.x = (self.x + self.step).min(1.0);
            } else if self.x > target {
                self.x = (self.x - self.step).max(0.0);
            }
            let (g_feed, g_desk) = if self.x <= 0.0 {
                (0.0, 1.0)
            } else if self.x >= 1.0 {
                (1.0, 0.0)
            } else {
                let a = self.x * std::f32::consts::FRAC_PI_2;
                (a.sin(), a.cos())
            };
            for c in 0..ch {
                let j = i * ch + c;
                let f = feed.map_or(0.0, |f| f.get(j).copied().unwrap_or(0.0));
                out[j] = desk[j] * g_desk + f * g_feed;
            }
        }
    }

    /// The station's mode; from the next block on.
    pub fn set_mode(&mut self, mode: StationMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> StationMode {
        self.mode
    }

    /// Whether someone is live at the desk, as heard after the delay (in either mode).
    pub fn live(&self) -> bool {
        self.live_out
    }

    /// What the sound is now (the nearer, mid-crossfade).
    pub fn source(&self) -> Source {
        if self.x >= 0.5 {
            Source::Playout
        } else {
            Source::Desk
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 960;

    /// `blocks` blocks: the desk at 0.5, the feed at 0.25 (when there), one channel.
    fn run(
        s: &mut HlsSound,
        blocks: usize,
        live: bool,
        held: u64,
        feed: bool,
    ) -> Vec<(Source, f32)> {
        let desk = vec![0.5; BLOCK];
        let fed = vec![0.25; BLOCK];
        let mut out = Vec::new();
        (0..blocks)
            .map(|_| {
                s.render(live, held, &desk, feed.then_some(&fed[..]), &mut out);
                (s.source(), *out.last().unwrap())
            })
            .collect()
    }

    #[test]
    fn nobody_live_is_playout_and_without_a_feed_the_desk() {
        let mut s = HlsSound::new(1, 4_800, 48_000);
        let r = run(&mut s, 1, false, 0, false);
        assert_eq!(
            r[0],
            (Source::Desk, 0.5),
            "no feed: the desk, never nothing"
        );
        // The feed arrives: a crossfade of 100 ms (5 blocks), then playout alone.
        let r = run(&mut s, 6, false, 0, true);
        assert!(r[0].1 > 0.25 && r[0].1 < 0.75);
        assert_eq!(r[5], (Source::Playout, 0.25));
        // The feed goes away: back to the desk.
        let r = run(&mut s, 6, false, 0, false);
        assert_eq!(r[5], (Source::Desk, 0.5));
    }

    #[test]
    fn going_live_reaches_the_sound_when_the_delay_has_played_out_what_came_before() {
        // A delay of 1 s (50 blocks of 20 ms) that holds steady.
        let mut s = HlsSound::new(1, 4_800, 48_000);
        let held = 48_000;
        run(&mut s, 100, false, held, true);
        assert_eq!(s.source(), Source::Playout);
        // Live now; for viewers, after the second the delay holds.
        let r = run(&mut s, 49, true, held, true);
        assert!(
            r.iter().all(|(src, _)| *src == Source::Playout),
            "too early"
        );
        assert!(!s.live());
        let r = run(&mut s, 7, true, held, true);
        assert!(s.live());
        assert_eq!(r[6], (Source::Desk, 0.5));
    }

    #[test]
    fn off_air_holds_the_desk_then_crossfades_to_playout() {
        let mut s = HlsSound::new(1, 4_800, 48_000);
        run(&mut s, 10, true, 0, true);
        assert_eq!(s.source(), Source::Desk);
        // Off air with no delay: the desk for the 1 s hold, then playout.
        let r = run(&mut s, 50, false, 0, true);
        assert!(r.iter().all(|(src, _)| *src == Source::Desk), "the hold");
        let r = run(&mut s, 6, false, 0, true);
        assert_eq!(r[5], (Source::Playout, 0.25));
    }

    #[test]
    fn a_dump_brings_live_forward_with_the_sound() {
        let mut s = HlsSound::new(1, 960, 0);
        let held = 96_000;
        run(&mut s, 10, false, held, true);
        run(&mut s, 10, true, held, true);
        assert!(!s.live(), "still behind the delay");
        // Dumped: the delay holds nothing, and what is live now is heard now.
        run(&mut s, 1, true, 0, true);
        assert!(s.live());
    }

    #[test]
    fn in_auto_mode_the_sound_is_playout_whoever_is_live() {
        let mut s = HlsSound::new(1, 4_800, 48_000);
        run(&mut s, 10, true, 0, true);
        assert_eq!(s.source(), Source::Desk);
        // Automation takes the air: playout at once, no hold, and live is still the desk's fact.
        s.set_mode(StationMode::Auto);
        let r = run(&mut s, 6, true, 0, true);
        assert!(s.live());
        assert_eq!(r[5], (Source::Playout, 0.25), "no hold in auto mode");
        // Without a feed: the desk's programme, never nothing.
        let r = run(&mut s, 6, true, 0, false);
        assert_eq!(r[5], (Source::Desk, 0.5));
        let r = run(&mut s, 6, false, 0, true);
        assert_eq!(r[5], (Source::Playout, 0.25));
    }

    #[test]
    fn back_in_live_mode_an_open_strip_is_the_desk_at_once() {
        let mut s = HlsSound::new(1, 4_800, 48_000);
        let held = 48_000;
        s.set_mode(StationMode::Auto);
        run(&mut s, 100, true, held, true);
        assert!(s.live(), "live judged through the delay meanwhile");
        assert_eq!(s.source(), Source::Playout);
        s.set_mode(StationMode::Live);
        let r = run(&mut s, 6, true, held, true);
        assert_eq!(
            r[5],
            (Source::Desk, 0.5),
            "not held back by the delay again"
        );
        // Nobody live, back in live mode: playout after the hold, as before.
        let mut s = HlsSound::new(1, 4_800, 48_000);
        s.set_mode(StationMode::Auto);
        run(&mut s, 10, false, 0, true);
        s.set_mode(StationMode::Live);
        let r = run(&mut s, 6, false, 0, true);
        assert!(r.iter().all(|(src, _)| *src == Source::Playout));
    }

    #[test]
    fn modes_use_the_sockets_words() {
        assert_eq!(StationMode::parse("live"), Some(StationMode::Live));
        assert_eq!(StationMode::parse("auto"), Some(StationMode::Auto));
        assert_eq!(StationMode::parse("Auto"), None);
        assert_eq!(StationMode::default(), StationMode::Live);
        assert_eq!(
            serde_json::to_string(&StationMode::Auto).unwrap(),
            r#""auto""#
        );
    }

    #[test]
    fn the_crossfade_keeps_the_power_of_two_equal_sources() {
        let mut s = HlsSound::new(1, 960 * 4, 0);
        let desk = vec![0.5; BLOCK];
        let mut out = Vec::new();
        // A feed in the desk's level: equal power, so never more than 3 dB up mid-way.
        for _ in 0..4 {
            s.render(false, 0, &desk, Some(&desk), &mut out);
            assert!(out.iter().all(|v| (0.499..=0.5 * 1.4143).contains(v)));
        }
    }
}
