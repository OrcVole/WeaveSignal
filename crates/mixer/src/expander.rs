// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A downward expander: below its threshold a microphone is turned down, so what it picks up
//! while its owner is quiet (headphone bleed, the room) stays out of the mix.
//!
//! Why: in the second live run a guest and a caller came back into the programme 0.3 s late,
//! their voices leaking from the presenter's headphones into the presenter's microphone and
//! lifted by the browser's gain control while the presenter was quiet. The presenter's chain
//! now starts here, before the compressor.
//!
//! GStreamer's `audiodynamic mode=expander`, which the spike's harness used, was measured on
//! 2026-10-25 (GStreamer 1.28.8, a 440 Hz sine): with `ratio=0.3` it turns quiet signals *up*
//! (−63 dBFS RMS came out at −46 dBFS soft-knee, −37 hard-knee), and with a ratio over 1 it
//! gates them to silence sample by sample, with no attack or release. This one works on a peak
//! envelope with a soft knee, a fast attack, a slow release and a floor (`range_db`), so speech
//! opens it at once and it closes without chopping the ends of words.

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, coef, db_to_lin, lin_to_db};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExpanderSettings {
    /// Below this level, in dBFS (peak), the input is turned down.
    pub threshold_db: f32,
    /// Output decibels per input decibel below the threshold (3.0 means 1:3, downward).
    pub ratio: f32,
    /// The width of the soft knee around the threshold.
    pub knee_db: f32,
    /// The most it ever turns down, in dB.
    pub range_db: f32,
    /// How fast it opens when the level rises.
    pub attack_ms: f32,
    /// How slowly it closes when the level falls.
    pub release_ms: f32,
}

impl Default for ExpanderSettings {
    /// The presenter's expander: −34 dBFS (the live run's 0.02), 1:3 below it, at most 18 dB.
    fn default() -> Self {
        Self {
            threshold_db: -34.0,
            ratio: 3.0,
            knee_db: 6.0,
            range_db: 18.0,
            attack_ms: 1.0,
            release_ms: 150.0,
        }
    }
}

impl ExpanderSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        let bad = |m: String| Err(SettingsError(m));
        if !(-80.0..=0.0).contains(&self.threshold_db) {
            return bad(format!(
                "expander threshold_db {} must be -80 to 0",
                self.threshold_db
            ));
        }
        if !(1.0..=10.0).contains(&self.ratio) {
            return bad(format!("expander ratio {} must be 1 to 10", self.ratio));
        }
        if !(0.0..=24.0).contains(&self.knee_db) || !(0.0..=60.0).contains(&self.range_db) {
            return bad("expander knee_db must be 0 to 24 and range_db 0 to 60".into());
        }
        if !(0.1..=100.0).contains(&self.attack_ms) || !(10.0..=5000.0).contains(&self.release_ms) {
            return bad("expander attack_ms must be 0.1 to 100 and release_ms 10 to 5000".into());
        }
        Ok(())
    }

    /// The static curve: how far to turn down (zero or positive dB) at an input level.
    pub fn reduction_db(&self, level_db: f32) -> f32 {
        let under = self.threshold_db - level_db;
        let slope = self.ratio - 1.0;
        let half = self.knee_db / 2.0;
        let r = if under <= -half {
            0.0
        } else if self.knee_db > 0.0 && under < half {
            slope * (under + half).powi(2) / (2.0 * self.knee_db)
        } else {
            slope * under
        };
        r.min(self.range_db)
    }
}

/// How fast the level detector falls between peaks: it holds a voice's level through its
/// zero crossings and the gaps between syllables' cycles.
const DETECTOR_MS: f32 = 50.0;

pub struct Expander {
    settings: ExpanderSettings,
    channels: usize,
    attack: f32,
    release: f32,
    fall: f32,
    envelope: f32,
    /// The smoothed reduction, in dB.
    reduction: f32,
}

impl Expander {
    pub fn new(settings: &ExpanderSettings, channels: usize) -> Self {
        Self {
            settings: *settings,
            channels: channels.max(1),
            attack: coef(settings.attack_ms, RATE),
            release: coef(settings.release_ms, RATE),
            fall: 1.0 - coef(DETECTOR_MS, RATE),
            envelope: 0.0,
            // Closed at the start: a microphone is quiet until its owner speaks.
            reduction: settings.range_db,
        }
    }

    /// The reduction applied to the last frame, in dB.
    pub fn reduction_db(&self) -> f32 {
        self.reduction
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(self.channels) {
            let peak = frame.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            self.envelope = peak.max(self.envelope * self.fall);
            let wanted = self.settings.reduction_db(lin_to_db(self.envelope));
            // Opening (less reduction) is the attack; closing is the release.
            let k = if wanted < self.reduction {
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

    fn through(level_db: f32, secs: f32) -> f32 {
        let mut e = Expander::new(&ExpanderSettings::default(), 2);
        let mut x = sine(440.0, db_to_lin(level_db), secs);
        e.process(&mut x);
        rms_db(&x[x.len() / 2..]) - (level_db - 3.01)
    }

    #[test]
    fn the_curve_turns_down_only_below_the_threshold_and_never_past_its_range() {
        let s = ExpanderSettings::default();
        assert_eq!(s.reduction_db(-20.0), 0.0);
        assert_eq!(s.reduction_db(-30.0), 0.0);
        // 6 dB under at 1:3 is 12 dB down.
        assert!((s.reduction_db(-40.0) - 12.0).abs() < 1e-4);
        assert_eq!(s.reduction_db(-70.0), 18.0, "the range is the floor");
        for edge in [-37.0f32, -31.0] {
            let (a, b) = (s.reduction_db(edge - 1e-4), s.reduction_db(edge + 1e-4));
            assert!((a - b).abs() < 1e-3, "a step at {edge} dB");
        }
    }

    #[test]
    fn speech_passes_and_bleed_is_turned_down() {
        // A presenter speaking at −20 dBFS peak passes untouched.
        let speech = through(-20.0, 1.0);
        assert!(speech.abs() < 0.1, "speech changed by {speech:.2} dB");
        // Headphone bleed at −50 dBFS peak comes out 18 dB lower (the floor).
        let bleed = through(-50.0, 1.0);
        assert!((bleed + 18.0).abs() < 0.5, "bleed changed by {bleed:.2} dB");
        // Unlike audiodynamic's ratio 0.3, nothing quiet ever comes out louder.
        for level in [-80.0f32, -60.0, -45.0, -36.0] {
            assert!(through(level, 0.5) <= 0.01, "{level} dBFS came out louder");
        }
    }

    #[test]
    fn it_opens_within_a_few_milliseconds_of_a_word() {
        let mut e = Expander::new(&ExpanderSettings::default(), 2);
        let mut quiet = sine(440.0, db_to_lin(-60.0), 0.5);
        e.process(&mut quiet);
        assert!(e.reduction_db() > 17.0);
        let mut word = sine(440.0, db_to_lin(-20.0), 0.01);
        e.process(&mut word);
        assert!(
            e.reduction_db() < 1.0,
            "still {:.1} dB down after 10 ms",
            e.reduction_db()
        );
    }
}
