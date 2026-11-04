// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The telephone sound: a band-pass from about 300 to 3,400 Hz, the band of a phone line, so a
//! caller sounds like a call on air and in the presenters' ears (operator, 2026-11-01: on by
//! default for callers while we run diagnostics). Two cascaded second-order Butterworth
//! sections at each edge (24 dB an octave), from the RBJ audio-EQ cookbook.

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelephoneSettings {
    /// Below this the voice is cut, in Hz.
    pub low_hz: f32,
    /// Above this the voice is cut, in Hz.
    pub high_hz: f32,
}

impl Default for TelephoneSettings {
    /// The narrowband phone line: 300 to 3,400 Hz.
    fn default() -> Self {
        Self {
            low_hz: 300.0,
            high_hz: 3400.0,
        }
    }
}

impl TelephoneSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(50.0..=1000.0).contains(&self.low_hz) {
            return Err(SettingsError(format!(
                "telephone low_hz {} must be 50 to 1000",
                self.low_hz
            )));
        }
        if !(1500.0..=8000.0).contains(&self.high_hz) {
            return Err(SettingsError(format!(
                "telephone high_hz {} must be 1500 to 8000",
                self.high_hz
            )));
        }
        Ok(())
    }
}

/// One second-order section (direct form I), with a state per channel.
#[derive(Clone)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    /// Per channel: x[n-1], x[n-2], y[n-1], y[n-2].
    state: Vec<[f32; 4]>,
}

impl Biquad {
    /// A Butterworth (Q = 1/sqrt 2) high-pass or low-pass at `hz`.
    fn new(hz: f32, high_pass: bool, channels: usize) -> Self {
        let w = 2.0 * std::f32::consts::PI * hz / RATE as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b = if high_pass {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self {
            b: [b[0] / a0, b[1] / a0, b[2] / a0],
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            state: vec![[0.0; 4]; channels],
        }
    }

    fn step(&mut self, ch: usize, x: f32) -> f32 {
        let s = &mut self.state[ch];
        let y = self.b[0] * x + self.b[1] * s[0] + self.b[2] * s[1]
            - self.a[0] * s[2]
            - self.a[1] * s[3];
        // A NaN or an infinity must not stay in the state (the limiter's lesson, staging 2026-11-01).
        let y = if y.is_finite() { y } else { 0.0 };
        *s = [x, s[0], y, s[2]];
        y
    }
}

pub struct Telephone {
    sections: Vec<Biquad>,
    channels: usize,
}

impl Telephone {
    pub fn new(settings: &TelephoneSettings, channels: usize) -> Self {
        Self {
            sections: vec![
                Biquad::new(settings.low_hz, true, channels),
                Biquad::new(settings.low_hz, true, channels),
                Biquad::new(settings.high_hz, false, channels),
                Biquad::new(settings.high_hz, false, channels),
            ],
            channels,
        }
    }

    /// Filter interleaved samples in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(self.channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                let x = if s.is_finite() { *s } else { 0.0 };
                *s = self.sections.iter_mut().fold(x, |v, b| b.step(ch, v));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::{rms_db, sine};

    /// How much a tone at `hz` is turned down, in dB, after the filter has settled.
    fn loss_db(hz: f32) -> f32 {
        let mut t = Telephone::new(&TelephoneSettings::default(), 2);
        let mut s = sine(hz, 0.5, 1.0);
        let before = rms_db(&s);
        t.process(&mut s);
        let settled = &s[s.len() / 2..];
        before - rms_db(settled)
    }

    #[test]
    fn the_voice_band_passes_and_the_rest_is_cut() {
        assert!(loss_db(1000.0) < 0.5, "1 kHz: {} dB", loss_db(1000.0));
        // Two stacked sections droop a little before the edge (about 1 dB at 2 kHz), as a line does.
        assert!(loss_db(2000.0) < 1.5, "2 kHz: {} dB", loss_db(2000.0));
        // At the edges, 3 dB per section, about 6 dB for the pair.
        assert!(
            (4.0..8.0).contains(&loss_db(300.0)),
            "300 Hz: {} dB",
            loss_db(300.0)
        );
        assert!(
            (4.0..8.0).contains(&loss_db(3400.0)),
            "3.4 kHz: {} dB",
            loss_db(3400.0)
        );
        // An octave and more outside: well down.
        assert!(loss_db(100.0) > 30.0, "100 Hz: {} dB", loss_db(100.0));
        assert!(loss_db(8000.0) > 25.0, "8 kHz: {} dB", loss_db(8000.0));
    }

    #[test]
    fn nan_does_not_stay_in_the_filter() {
        let mut t = Telephone::new(&TelephoneSettings::default(), 2);
        let mut s = vec![f32::NAN; 960];
        t.process(&mut s);
        let mut tone = sine(1000.0, 0.5, 0.5);
        t.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()));
        assert!(rms_db(&tone[tone.len() / 2..]) > -12.0);
    }

    #[test]
    fn settings_out_of_range_are_refused() {
        assert!(TelephoneSettings::default().validate().is_ok());
        assert!(
            TelephoneSettings {
                low_hz: 20.0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            TelephoneSettings {
                high_hz: 12000.0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
