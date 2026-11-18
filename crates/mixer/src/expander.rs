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
    /// How long it stays as open as speech left it after the level falls, before the release
    /// begins: the last consonant of a word (a `t`, `d`, `p`, `s` or `f` after the vowel) can
    /// come 100 to 200 ms after the vowel's energy and sits 25 dB or more below it.
    pub hold_ms: f32,
}

/// The default hold (issue 16); see [`ExpanderSettings::hold_ms`].
const HOLD_DEFAULT_MS: f32 = 250.0;

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
            hold_ms: HOLD_DEFAULT_MS,
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
        if !(0.0..=2000.0).contains(&self.hold_ms) {
            return bad(format!(
                "expander hold_ms {} must be 0 to 2000",
                self.hold_ms
            ));
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

/// Below this reduction the gate counts as open for speech, and the hold starts again.
const HELD_OPEN_DB: f32 = 1.0;

pub struct Expander {
    settings: ExpanderSettings,
    channels: usize,
    attack: f32,
    release: f32,
    fall: f32,
    /// Frames the gate is held open for after speech last held it open.
    hold_frames: u32,
    held: u32,
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
            hold_frames: (settings.hold_ms * 0.001 * RATE as f32).round() as u32,
            held: 0,
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
            let mut wanted = self.settings.reduction_db(lin_to_db(self.envelope));
            // Speech (less than `HELD_OPEN_DB` down) restarts the hold; while it runs the gate
            // may open further but not close.
            if wanted < HELD_OPEN_DB {
                self.held = self.hold_frames;
            } else if self.held > 0 {
                self.held -= 1;
                wanted = wanted.min(self.reduction);
            }
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

    /// A word as a microphone hears it: a vowel, a stop's closure, then the soft burst of a
    /// final `t` or `d`, 30 dB or more under the vowel (issue 16, "Steeped" heard as "Steep").
    /// Returns the samples and where the burst is.
    fn word(vowel_db: f32, closure_ms: f32, burst_db: f32) -> (Vec<f32>, std::ops::Range<usize>) {
        let ms = |t: f32| (t * 0.001 * RATE as f32) as usize;
        let mut x = sine(180.0, db_to_lin(vowel_db), 0.25);
        let burst_from = x.len() / 2 + ms(closure_ms);
        x.resize(burst_from * 2, 0.0);
        let mut seed = 12345u32;
        let burst = ms(25.0);
        for _ in 0..burst {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = ((seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * db_to_lin(burst_db);
            x.extend([v, v]);
        }
        // Room tone under the word's end, as the 250 ms after it, for the gate to close in.
        x.resize(x.len() + ms(250.0) * 2, 0.0);
        (x, burst_from..burst_from + burst)
    }

    /// How far down the gate leaves the burst, in dB (RMS before against after).
    fn burst_loss_db(settings: &ExpanderSettings, closure_ms: f32) -> f32 {
        let (x, burst) = word(-20.0, closure_ms, -50.0);
        let mut y = x.clone();
        Expander::new(settings, 2).process(&mut y);
        let at = burst.start * 2..burst.end * 2;
        rms_db(&x[at.clone()]) - rms_db(&y[at])
    }

    #[test]
    fn a_words_soft_last_consonant_is_not_gated_away() {
        // The word ends in a stop: a closure of up to 200 ms, then a burst at -50 dBFS under a -20 dBFS
        // vowel. Measured on the gate without a hold: 3 dB lost at 120 ms of closure, 9 dB at 200 ms.
        let none = ExpanderSettings {
            hold_ms: 0.0,
            ..ExpanderSettings::default()
        };
        let old = burst_loss_db(&none, 200.0);
        assert!(old > 6.0, "without a hold the burst lost only {old:.1} dB");
        for closure in [80.0, 160.0, 200.0] {
            let kept = burst_loss_db(&ExpanderSettings::default(), closure);
            assert!(
                kept < 1.5,
                "{closure} ms of closure: the burst lost {kept:.1} dB"
            );
        }
    }

    #[test]
    fn the_hold_does_not_let_bleed_back_in_for_long() {
        // After speech stops the gate closes: the hold plus the release must still take bleed
        // 12 dB down within 0.8 s (the presenter's echo protection; see the live run's echo).
        let mut e = Expander::new(&ExpanderSettings::default(), 2);
        let mut speech = sine(180.0, db_to_lin(-20.0), 0.5);
        e.process(&mut speech);
        assert!(e.reduction_db() < 1.0);
        let mut bleed = sine(440.0, db_to_lin(-50.0), 0.8);
        e.process(&mut bleed);
        assert!(
            e.reduction_db() > 12.0,
            "only {:.1} dB down 0.8 s after speech",
            e.reduction_db()
        );
    }

    #[test]
    fn the_hold_is_a_setting_with_a_range() {
        let s: ExpanderSettings = toml::from_str("hold_ms = 100.0").unwrap();
        assert_eq!(s.hold_ms, 100.0);
        assert_eq!(s.release_ms, 150.0, "unset fields default");
        assert!(s.validate().is_ok());
        assert!(ExpanderSettings { hold_ms: -1.0, ..s }.validate().is_err());
        assert!(
            ExpanderSettings {
                hold_ms: 2001.0,
                ..s
            }
            .validate()
            .is_err()
        );
    }
}
