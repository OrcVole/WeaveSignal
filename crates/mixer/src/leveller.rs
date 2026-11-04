// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Slow gain steering toward a target speech level.
//!
//! From the spike's live run (2026-10-25): the presenter spoke at -30.7 dB against a guest's
//! -25.1 dB per 10 ms of speech, and the operator heard himself "much too quiet". The
//! leveller measures each 10 ms block; while the output is speech (above the gate) it follows
//! the level slowly, and every half second moves the gain by at most half a decibel toward the
//! target, within bounds. Silence and breaths leave the gain where it is.

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, db_to_lin, lin_to_db};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LevellerSettings {
    /// The speech level to steer toward, per 10 ms of speech, in dBFS RMS.
    pub target_db: f32,
    /// Blocks quieter than this (after the gain) are not speech.
    pub gate_db: f32,
    pub min_db: f32,
    pub max_db: f32,
    /// The most the gain moves per half second.
    pub step_db: f32,
}

impl Default for LevellerSettings {
    fn default() -> Self {
        Self {
            target_db: -25.0,
            gate_db: -38.0,
            min_db: -6.0,
            max_db: 18.0,
            step_db: 0.5,
        }
    }
}

impl LevellerSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(self.min_db <= 0.0
            && self.max_db >= 0.0
            && self.max_db <= 30.0
            && self.min_db >= -30.0)
        {
            return Err(SettingsError(
                "leveller min_db must be -30 to 0 and max_db 0 to 30".into(),
            ));
        }
        if !(-60.0..=0.0).contains(&self.target_db) || !(-80.0..=0.0).contains(&self.gate_db) {
            return Err(SettingsError(
                "leveller target_db must be -60 to 0 and gate_db -80 to 0".into(),
            ));
        }
        if !(0.01..=6.0).contains(&self.step_db) {
            return Err(SettingsError("leveller step_db must be 0.01 to 6".into()));
        }
        Ok(())
    }
}

const BLOCK: usize = (RATE / 100) as usize; // 10 ms
const BLOCKS_PER_STEP: u32 = 50; // half a second

pub struct Leveller {
    settings: LevellerSettings,
    channels: usize,
    /// The gain now, in dB, and the linear gain applied at the end of the last block.
    gain_db: f32,
    applied: f32,
    /// The running mean square of the current block, and its frame count.
    acc: f32,
    frames: usize,
    /// The followed speech level of the input (before the gain).
    ema: Option<f32>,
    blocks: u32,
}

impl Leveller {
    /// `start_db` is the gain to begin from (the make-up gain).
    pub fn new(settings: &LevellerSettings, channels: usize, start_db: f32) -> Self {
        let start = start_db.clamp(settings.min_db, settings.max_db);
        Self {
            settings: *settings,
            channels: channels.max(1),
            gain_db: start,
            applied: db_to_lin(start),
            acc: 0.0,
            frames: 0,
            ema: None,
            blocks: 0,
        }
    }

    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    fn end_block(&mut self) {
        let level_in = lin_to_db((self.acc / (self.frames * self.channels) as f32).sqrt());
        self.acc = 0.0;
        self.frames = 0;
        if level_in + self.gain_db > self.settings.gate_db {
            self.ema = Some(match self.ema {
                None => level_in,
                Some(e) => 0.99 * e + 0.01 * level_in,
            });
        }
        self.blocks += 1;
        if self.blocks >= BLOCKS_PER_STEP {
            self.blocks = 0;
            if let Some(e) = self.ema {
                let error = self.settings.target_db - (e + self.gain_db);
                let step = (0.3 * error).clamp(-self.settings.step_db, self.settings.step_db);
                self.gain_db =
                    (self.gain_db + step).clamp(self.settings.min_db, self.settings.max_db);
            }
        }
    }

    /// Apply the gain in place, ramping smoothly to each new value over a block.
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

    #[test]
    fn a_quiet_presenter_is_brought_up_slowly_and_within_bounds() {
        let s = LevellerSettings::default();
        // Speech at -33 dB RMS (a sine at -30 dB peak), starting at +5.6 dB as the spike did.
        let mut l = Leveller::new(&s, 2, 5.6);
        let mut x = sine(300.0, db_to_lin(-30.0), 10.0);
        l.process(&mut x[..2 * RATE as usize]); // two seconds
        let after_2s = l.gain_db();
        assert!(
            after_2s > 5.6 && after_2s <= 5.6 + 4.0 * 0.5 + 1e-4,
            "{after_2s}"
        );
        l.process(&mut x[2 * RATE as usize..]);
        // -33 dB in, target -25 dB: about +8 dB, reached within ten seconds.
        assert!((l.gain_db() - 8.0).abs() < 0.6, "gain {}", l.gain_db());
    }

    #[test]
    fn silence_leaves_the_gain_alone() {
        let s = LevellerSettings::default();
        let mut l = Leveller::new(&s, 2, 5.6);
        let mut x = vec![0.0f32; 2 * RATE as usize * 3];
        l.process(&mut x);
        assert_eq!(l.gain_db(), 5.6);
    }

    #[test]
    fn a_loud_presenter_is_brought_down_no_further_than_the_floor() {
        let s = LevellerSettings::default();
        let mut l = Leveller::new(&s, 1, 0.0);
        let mut x: Vec<f32> = sine(300.0, 0.9, 40.0).into_iter().step_by(2).collect();
        l.process(&mut x);
        assert_eq!(l.gain_db(), -6.0);
    }
}
