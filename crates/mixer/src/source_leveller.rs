// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The leveller on a host's own studio (ADR-0280; design section 10, *The desk*).
//!
//! A host's software sends a finished mix, music and voice together, at whatever level its own
//! faders give. The desk brings it to its working level with a slow gain: the level is the
//! mean square of the last ten seconds that were not silent, and the gain moves toward
//! `target − level` by at most a decibel a second, within ±12 dB, and only while the source
//! sounds: a silent 100 ms leaves the gain where it is, so a quiet passage or a pause is not
//! pumped up into noise. It has no gate and no phone band: what the host sends is what they
//! mean listeners to hear, only louder or quieter.
//!
//! The level is measured unweighted, so its target is in dBFS RMS; for broadband programme it
//! sits within about a decibel of the same figure in LUFS, which is what the desk's meter
//! shows after it.

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, db_to_lin, lin_to_db};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SourceLevellerSettings {
    /// The level to steer toward, dBFS RMS over the window (the desk's working level).
    pub target_db: f32,
    /// The most the gain may be, either way, in dB.
    pub range_db: f32,
    /// A 100 ms block quieter than this (before the gain) is silence, and is not measured.
    pub silence_db: f32,
    /// How fast the gain may move, dB a second.
    pub rate_db_s: f32,
    /// The window the level is measured over, in seconds.
    pub window_s: f32,
}

impl Default for SourceLevellerSettings {
    fn default() -> Self {
        Self {
            target_db: -23.0,
            range_db: 12.0,
            silence_db: -50.0,
            rate_db_s: 1.0,
            window_s: 10.0,
        }
    }
}

impl SourceLevellerSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(-40.0..=-6.0).contains(&self.target_db) {
            return Err(SettingsError(
                "the source leveller's target_db must be -40 to -6".into(),
            ));
        }
        if !(0.0..=24.0).contains(&self.range_db) {
            return Err(SettingsError(
                "the source leveller's range_db must be 0 to 24".into(),
            ));
        }
        if !(-80.0..=-20.0).contains(&self.silence_db) {
            return Err(SettingsError(
                "the source leveller's silence_db must be -80 to -20".into(),
            ));
        }
        if !(0.1..=12.0).contains(&self.rate_db_s) || !(1.0..=60.0).contains(&self.window_s) {
            return Err(SettingsError(
                "the source leveller's rate_db_s must be 0.1 to 12 and window_s 1 to 60".into(),
            ));
        }
        Ok(())
    }
}

/// A measuring block: 100 ms.
const BLOCK: usize = (RATE / 10) as usize;
/// The shortest window: a second.
const MIN_WINDOW: usize = 10;

pub struct SourceLeveller {
    settings: SourceLevellerSettings,
    channels: usize,
    gain_db: f32,
    applied: f32,
    acc: f32,
    frames: usize,
    /// The mean square of each of the window's blocks, `None` for a silent one; oldest first.
    blocks: std::collections::VecDeque<Option<f32>>,
    window: usize,
}

impl SourceLeveller {
    pub fn new(settings: &SourceLevellerSettings, channels: usize) -> Self {
        let window = ((settings.window_s * 10.0).round() as usize).max(MIN_WINDOW);
        Self {
            settings: *settings,
            channels: channels.max(1),
            gain_db: 0.0,
            applied: 1.0,
            acc: 0.0,
            frames: 0,
            blocks: std::collections::VecDeque::with_capacity(window),
            window,
        }
    }

    /// The gain applied now, in dB: what the station is doing to bring the source to the desk's
    /// level (the tile's *leveller −7.4 dB*).
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// The source's own level over the window, before the gain; `None` until it has sounded.
    pub fn level_db(&self) -> Option<f32> {
        let (sum, n) = self
            .blocks
            .iter()
            .flatten()
            .fold((0.0f32, 0usize), |(s, n), ms| (s + ms, n + 1));
        (n > 0).then(|| lin_to_db((sum / n as f32).sqrt()))
    }

    fn end_block(&mut self) {
        let ms = self.acc / (self.frames * self.channels) as f32;
        self.acc = 0.0;
        self.frames = 0;
        let silent = lin_to_db(ms.sqrt()) < self.settings.silence_db;
        if self.blocks.len() == self.window {
            self.blocks.pop_front();
        }
        self.blocks.push_back((!silent).then_some(ms));
        if silent {
            return;
        }
        let Some(level) = self.level_db() else { return };
        let want = (self.settings.target_db - level)
            .clamp(-self.settings.range_db, self.settings.range_db);
        let step = self.settings.rate_db_s * BLOCK as f32 / RATE as f32;
        self.gain_db += (want - self.gain_db).clamp(-step, step);
    }

    /// Apply the gain in place, ramping across each block to its new value.
    pub fn process(&mut self, samples: &mut [f32]) {
        let ch = self.channels;
        let mut target = db_to_lin(self.gain_db);
        let mut step = (target - self.applied) / BLOCK as f32;
        for frame in samples.chunks_exact_mut(ch) {
            for s in frame.iter() {
                self.acc += s * s;
            }
            if (self.applied - target).abs() > 1e-7 {
                self.applied += step;
                if (step > 0.0 && self.applied > target) || (step < 0.0 && self.applied < target) {
                    self.applied = target;
                }
            }
            for s in frame.iter_mut() {
                *s *= self.applied;
            }
            self.frames += 1;
            if self.frames == BLOCK {
                self.end_block();
                target = db_to_lin(self.gain_db);
                step = (target - self.applied) / BLOCK as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::sine;

    fn run(l: &mut SourceLeveller, peak_db: f32, seconds: f32) {
        let mut x = sine(440.0, db_to_lin(peak_db), seconds);
        l.process(&mut x);
    }

    #[test]
    fn a_quiet_source_is_brought_up_slowly_toward_the_desks_level() {
        let s = SourceLevellerSettings::default();
        let mut l = SourceLeveller::new(&s, 2);
        // A sine at -32 dB peak is -35 dB RMS: 12 dB under the target, the range's edge.
        run(&mut l, -32.0, 3.0);
        let after_3s = l.gain_db();
        assert!(
            after_3s > 2.0 && after_3s <= 3.0 + 1e-3,
            "a decibel a second: {after_3s}"
        );
        run(&mut l, -32.0, 15.0);
        assert!((l.gain_db() - 12.0).abs() < 0.05, "{}", l.gain_db());
        assert!((l.level_db().unwrap() + 35.0).abs() < 0.2);
    }

    #[test]
    fn the_gain_never_goes_past_twelve_decibels() {
        let s = SourceLevellerSettings::default();
        let mut l = SourceLeveller::new(&s, 2);
        run(&mut l, -45.0, 30.0);
        assert!((l.gain_db() - 12.0).abs() < 1e-3, "{}", l.gain_db());
        let mut l = SourceLeveller::new(&s, 2);
        run(&mut l, -1.0, 30.0);
        // -4 dB RMS against -23: down 19 dB wanted, 12 allowed.
        assert!((l.gain_db() + 12.0).abs() < 1e-3, "{}", l.gain_db());
    }

    #[test]
    fn silence_freezes_the_gain() {
        let s = SourceLevellerSettings::default();
        let mut l = SourceLeveller::new(&s, 2);
        run(&mut l, -29.0, 5.0);
        let held = l.gain_db();
        assert!(held > 2.0);
        let mut quiet = vec![0.0f32; 2 * RATE as usize * 20];
        l.process(&mut quiet);
        assert_eq!(l.gain_db(), held, "a pause is not pumped up");
        // Hiss under the silence floor is silence too.
        run(&mut l, -60.0, 20.0);
        assert_eq!(l.gain_db(), held);
    }

    #[test]
    fn a_short_loud_burst_in_a_level_programme_moves_the_gain_little() {
        let s = SourceLevellerSettings::default();
        let mut l = SourceLeveller::new(&s, 2);
        run(&mut l, -20.0, 20.0); // -23 dB RMS: on target
        assert!(l.gain_db().abs() < 0.05);
        run(&mut l, -2.0, 0.5);
        assert!(
            l.gain_db() > -0.6,
            "half a second at most half a decibel: {}",
            l.gain_db()
        );
    }

    #[test]
    fn the_settings_are_checked() {
        assert!(SourceLevellerSettings::default().validate().is_ok());
        let bad = SourceLevellerSettings {
            range_db: 30.0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }
}
