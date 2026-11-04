// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The profanity delay: the programme held back a few seconds on its way to listeners, so a
//! word that must not air can be dumped before it does (ADR-0003: after the desk, on the
//! listener path, never in the room).
//!
//! A dump throws away everything held, so listeners jump forward to the live programme, and
//! the delay then builds back up to its target. It rebuilds by playing what it holds slower
//! than it arrives: twice as slow through quiet passages (pauses between words, where nobody
//! hears it), and a few per cent slow otherwise (a pitch drop of a fraction of a semitone).
//! The desk starts the same way, from no delay, so a restarted desk is heard at once rather
//! than after the delay's length of silence.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, db_to_lin};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DelaySettings {
    /// How far behind the programme listeners are held; 0 turns the delay off.
    pub target_ms: u32,
    /// Below this level (RMS, dBFS) a passage is quiet, and is played at half speed while the
    /// delay rebuilds.
    pub quiet_db: f32,
    /// How much slower than it arrives anything else is played while the delay rebuilds.
    pub rebuild_slowdown: f32,
}

impl Default for DelaySettings {
    fn default() -> Self {
        Self {
            target_ms: 8_000,
            quiet_db: -50.0,
            rebuild_slowdown: 0.02,
        }
    }
}

impl DelaySettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.target_ms > 30_000 {
            return Err(SettingsError(format!(
                "target_ms {} must be at most 30000",
                self.target_ms
            )));
        }
        if !(0.005..=0.1).contains(&self.rebuild_slowdown) {
            return Err(SettingsError(format!(
                "rebuild_slowdown {} must be 0.005 to 0.1",
                self.rebuild_slowdown
            )));
        }
        if !(-90.0..=-20.0).contains(&self.quiet_db) {
            return Err(SettingsError(format!(
                "quiet_db {} must be -90 to -20",
                self.quiet_db
            )));
        }
        Ok(())
    }
}

/// The delay's state, for the desk's status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct DelayState {
    /// How far behind listeners are now.
    pub delay_ms: u32,
    pub target_ms: u32,
    /// Building back up after a dump (or a start); a dump throws away what is held so far.
    pub rebuilding: bool,
    /// Dumps since the desk started.
    pub dumps: u64,
}

pub struct DelayLine {
    channels: usize,
    target: usize,
    quiet: f32,
    slowdown: f64,
    /// Held frames, interleaved, oldest first.
    held: VecDeque<f32>,
    /// The read position's fraction of a frame past the front.
    frac: f64,
    rebuilding: bool,
    dumps: u64,
    /// Frames of fade-in left after a dump, so the jump is not a click.
    fade: usize,
    fade_len: usize,
}

impl DelayLine {
    pub fn new(settings: &DelaySettings, channels: usize) -> Self {
        let channels = channels.max(1);
        let target = settings.target_ms as usize * RATE as usize / 1000;
        Self {
            channels,
            target,
            quiet: db_to_lin(settings.quiet_db),
            slowdown: f64::from(settings.rebuild_slowdown),
            held: VecDeque::with_capacity((target + RATE as usize) * channels),
            frac: 0.0,
            rebuilding: target > 0,
            dumps: 0,
            fade: 0,
            fade_len: RATE as usize / 100,
        }
    }

    fn frames(&self) -> usize {
        self.held.len() / self.channels
    }

    pub fn state(&self) -> DelayState {
        let ms = |frames: usize| (frames as u64 * 1000 / u64::from(RATE)) as u32;
        DelayState {
            delay_ms: ms(self.frames()),
            target_ms: ms(self.target),
            rebuilding: self.rebuilding,
            dumps: self.dumps,
        }
    }

    /// Throw away everything held: listeners jump to the live programme. Allowed whenever
    /// anything is held, even while the delay rebuilds (ADR-0166): a short hold still catches a
    /// word, and the desk shows how much is held. Refused (`false`) with no delay, or with
    /// nothing held (right after a dump, or a start).
    pub fn dump(&mut self) -> bool {
        if self.target == 0 || self.held.is_empty() {
            return false;
        }
        self.held.clear();
        self.frac = 0.0;
        self.rebuilding = true;
        self.dumps += 1;
        self.fade = self.fade_len;
        true
    }

    /// Take `input` in, and fill `out` (the same length) with what listeners hear now.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let ch = self.channels;
        if self.target == 0 {
            out.copy_from_slice(input);
            return;
        }
        if self.rebuilding && self.frames() >= self.target {
            self.rebuilding = false;
        }
        self.held.extend(input.iter().copied());
        let n = out.len() / ch;
        let rate = if self.rebuilding {
            if self.quiet_ahead(n) {
                0.5
            } else {
                1.0 - self.slowdown
            }
        } else {
            1.0
        };
        // Linear interpolation between the frames either side of the read position.
        let mut pos = self.frac;
        let avail = self.frames();
        for i in 0..n {
            let a = (pos.floor() as usize).min(avail.saturating_sub(1));
            let b = (a + 1).min(avail.saturating_sub(1));
            let t = (pos - a as f64).clamp(0.0, 1.0) as f32;
            for c in 0..ch {
                let x = self.held[a * ch + c];
                let y = self.held[b * ch + c];
                out[i * ch + c] = x + (y - x) * t;
            }
            pos += rate;
        }
        let whole = (pos.floor() as usize).min(avail);
        self.held.drain(..whole * ch);
        self.frac = pos - whole as f64;
        if self.fade > 0 {
            for (i, frame) in out.chunks_exact_mut(ch).enumerate() {
                let done = self.fade_len - self.fade.saturating_sub(i);
                let g = done as f32 / self.fade_len as f32;
                frame.iter_mut().for_each(|s| *s *= g.min(1.0));
            }
            self.fade = self.fade.saturating_sub(n);
        }
    }

    /// Whether the next `n` frames to be played are quiet.
    fn quiet_ahead(&self, n: usize) -> bool {
        let len = (n * self.channels).min(self.held.len());
        if len == 0 {
            return true;
        }
        let power = self.held.range(..len).map(|s| s * s).sum::<f32>() / len as f32;
        power.sqrt() < self.quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::sine;

    const BLOCK: usize = 960 * 2;

    fn settings(target_ms: u32) -> DelaySettings {
        DelaySettings {
            target_ms,
            ..DelaySettings::default()
        }
    }

    /// Run `input` through in blocks; what came out.
    fn run(d: &mut DelayLine, input: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; input.len()];
        for (i, o) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            d.process(i, o);
        }
        out
    }

    #[test]
    fn no_target_passes_the_programme_straight_through() {
        let mut d = DelayLine::new(&settings(0), 2);
        let x = sine(440.0, 0.5, 0.1);
        assert_eq!(run(&mut d, &x[..BLOCK * 4]), x[..BLOCK * 4]);
        assert!(!d.dump(), "nothing to dump");
    }

    #[test]
    fn silence_builds_the_delay_at_half_speed_then_it_holds() {
        let mut d = DelayLine::new(&settings(1000), 2);
        assert!(d.state().rebuilding && d.state().delay_ms == 0);
        // 2 s of silence: the delay grows by half of what passes, so it reaches 1 s.
        run(&mut d, &vec![0.0; 100_800 * 2]);
        let s = d.state();
        assert!(!s.rebuilding, "{s:?}");
        assert!((990..=1020).contains(&s.delay_ms), "{s:?}");
        // Then it holds: a tone comes out exactly 1 s late.
        let mut x = sine(1000.0, 0.5, 3.0);
        x[..48_000 * 2].fill(0.0);
        let before = d.state().delay_ms;
        let y = run(&mut d, &x);
        assert_eq!(d.state().delay_ms, before);
        let first = |v: &[f32]| v.iter().position(|s| s.abs() > 0.01).unwrap() / 2;
        let lag = first(&y) as i64 - first(&x) as i64;
        assert!((lag - 48 * i64::from(before)).abs() < 48 * 21, "lag {lag}");
    }

    #[test]
    fn speech_rebuilds_the_delay_a_few_per_cent_slow() {
        let mut d = DelayLine::new(&settings(8000), 2);
        let x = sine(300.0, 0.3, 10.0);
        run(&mut d, &x);
        let ms = d.state().delay_ms;
        // 10 s at 2 % slow is 200 ms of delay.
        assert!((180..=220).contains(&ms), "{ms}");
        assert!(d.state().rebuilding);
    }

    #[test]
    fn a_dump_while_the_delay_rebuilds_throws_away_what_is_held() {
        let mut d = DelayLine::new(&settings(8000), 2);
        // Nothing is held at the start, so there is nothing to dump.
        assert!(!d.dump(), "a dump with nothing held");
        run(&mut d, &sine(300.0, 0.3, 10.0));
        let s = d.state();
        assert!(s.rebuilding && s.delay_ms > 100, "{s:?}");
        // Still rebuilding, with about 200 ms held: the dump is taken and empties it.
        assert!(d.dump(), "a dump while rebuilding was refused");
        let s = d.state();
        assert_eq!(s.dumps, 1);
        assert!(s.rebuilding && s.delay_ms == 0, "{s:?}");
        // Straight after, nothing is held again, so a second dump is refused until audio has
        // been taken in.
        assert!(!d.dump());
        run(&mut d, &sine(300.0, 0.3, 1.0));
        assert!(d.dump(), "audio held again, so dumping is allowed again");
        assert_eq!(d.state().dumps, 2);
    }

    #[test]
    fn a_dump_throws_away_everything_held() {
        let mut d = DelayLine::new(&settings(1000), 2);
        run(&mut d, &vec![0.0; 100_800 * 2]);
        assert!(!d.state().rebuilding);
        // A marker goes in, and is dumped before it can come out.
        let marker = sine(1000.0, 0.5, 0.5);
        let out = run(&mut d, &marker);
        assert!(
            out.iter().all(|s| s.abs() < 1e-6),
            "the marker came out early"
        );
        assert!(d.dump());
        assert_eq!(d.state().dumps, 1);
        assert!(d.state().rebuilding && d.state().delay_ms < 30);
        // What follows is live programme (quiet here), never the marker.
        let after = run(&mut d, &vec![0.0; 48_000 * 2 * 3]);
        assert!(
            after.iter().all(|s| s.abs() < 1e-6),
            "dumped audio reached listeners"
        );
        // And a tone after the dump is heard at once, not a delay later.
        let mut d2 = DelayLine::new(&settings(1000), 2);
        run(&mut d2, &vec![0.0; 100_800 * 2]);
        assert!(d2.dump());
        let y = run(&mut d2, &sine(1000.0, 0.5, 0.2));
        let at = y.iter().position(|s| s.abs() > 0.01).unwrap() / 2;
        assert!(
            at < 960,
            "after a dump the programme came out {at} frames late"
        );
    }
}
