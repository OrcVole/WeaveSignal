// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A feed-forward compressor with a soft knee, on the frame peak, in decibels.

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, coef, db_to_lin, lin_to_db};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CompressorSettings {
    /// Where compression starts, in dBFS.
    pub threshold_db: f32,
    /// Input decibels over the threshold per output decibel (2.5 means 2.5:1).
    pub ratio: f32,
    /// The width of the soft knee around the threshold.
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl Default for CompressorSettings {
    /// The spike's presenter compressor: about -14 dBFS at 2.5:1 with a soft knee.
    fn default() -> Self {
        Self {
            threshold_db: -14.0,
            ratio: 2.5,
            knee_db: 6.0,
            attack_ms: 5.0,
            release_ms: 120.0,
        }
    }
}

impl CompressorSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        let bad = |m: String| Err(SettingsError(m));
        if !(-60.0..=0.0).contains(&self.threshold_db) {
            return bad(format!(
                "compressor threshold_db {} must be -60 to 0",
                self.threshold_db
            ));
        }
        if !(1.0..=20.0).contains(&self.ratio) {
            return bad(format!("compressor ratio {} must be 1 to 20", self.ratio));
        }
        if !(0.0..=24.0).contains(&self.knee_db) {
            return bad(format!(
                "compressor knee_db {} must be 0 to 24",
                self.knee_db
            ));
        }
        if !(0.1..=500.0).contains(&self.attack_ms) || !(1.0..=5000.0).contains(&self.release_ms) {
            return bad("compressor attack_ms must be 0.1 to 500 and release_ms 1 to 5000".into());
        }
        Ok(())
    }

    /// The static curve: gain reduction in dB (zero or positive) for an input level in dB.
    pub fn reduction_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 - 1.0 / self.ratio;
        let half = self.knee_db / 2.0;
        if over <= -half {
            0.0
        } else if self.knee_db > 0.0 && over < half {
            slope * (over + half).powi(2) / (2.0 * self.knee_db)
        } else {
            slope * over
        }
    }
}

pub struct Compressor {
    settings: CompressorSettings,
    channels: usize,
    attack: f32,
    release: f32,
    /// The smoothed gain reduction, in dB.
    reduction: f32,
}

impl Compressor {
    pub fn new(settings: &CompressorSettings, channels: usize) -> Self {
        Self {
            settings: *settings,
            channels: channels.max(1),
            attack: coef(settings.attack_ms, RATE),
            release: coef(settings.release_ms, RATE),
            reduction: 0.0,
        }
    }

    /// The gain reduction applied to the last frame, in dB.
    pub fn reduction_db(&self) -> f32 {
        self.reduction
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(self.channels) {
            let peak = frame.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let wanted = self.settings.reduction_db(lin_to_db(peak));
            let k = if wanted > self.reduction {
                self.attack
            } else {
                self.release
            };
            self.reduction += (wanted - self.reduction) * k;
            let g = db_to_lin(-self.reduction);
            for s in frame.iter_mut() {
                *s *= g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::{rms_db, sine};

    #[test]
    fn the_curve_is_flat_below_and_divides_above() {
        let s = CompressorSettings::default();
        assert_eq!(s.reduction_db(-40.0), 0.0);
        // 10 dB over at 2.5:1 leaves 4 dB over: 6 dB of reduction.
        assert!((s.reduction_db(-4.0) - 6.0).abs() < 1e-4);
        // Continuous through the knee.
        for edge in [-17.0f32, -11.0] {
            let a = s.reduction_db(edge - 1e-4);
            let b = s.reduction_db(edge + 1e-4);
            assert!((a - b).abs() < 1e-3, "a step at {edge} dB");
        }
    }

    #[test]
    fn a_loud_tone_is_turned_down_and_a_quiet_one_left_alone() {
        let s = CompressorSettings::default();
        let mut c = Compressor::new(&s, 2);
        let mut loud = sine(1000.0, db_to_lin(-4.0), 1.0);
        c.process(&mut loud);
        let tail = &loud[loud.len() / 2..];
        // A sine's peak is 3 dB over its RMS; -4 dBFS peak should come out near -10 dBFS peak.
        let out_rms = rms_db(tail);
        assert!(
            (out_rms - (-10.0 - 3.01)).abs() < 1.0,
            "rms out {out_rms:.2} dB"
        );
        let mut c = Compressor::new(&s, 2);
        let quiet = sine(1000.0, db_to_lin(-30.0), 0.5);
        let mut q = quiet.clone();
        c.process(&mut q);
        assert_eq!(q, quiet);
    }
}
