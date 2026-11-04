// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A look-ahead peak limiter.
//!
//! Each frame's needed gain (ceiling over the frame's peak) enters a window as long as the
//! look-ahead; the gain applied to the frame leaving the delay line is the smoothed minimum
//! of that window, and never above that frame's own needed gain, so no output sample exceeds
//! the ceiling. The look-ahead is the limiter's whole latency.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{RATE, SettingsError, coef, db_to_lin};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimiterSettings {
    /// The highest sample peak out, in dBFS.
    pub ceiling_db: f32,
    /// How far ahead the limiter looks, and so its latency.
    pub lookahead_ms: f32,
    /// How fast the gain recovers after a peak.
    pub release_ms: f32,
}

impl Default for LimiterSettings {
    fn default() -> Self {
        Self {
            ceiling_db: -1.0,
            lookahead_ms: 1.5,
            release_ms: 120.0,
        }
    }
}

impl LimiterSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(-24.0..=0.0).contains(&self.ceiling_db) {
            return Err(SettingsError(format!(
                "limiter ceiling_db {} must be -24 to 0",
                self.ceiling_db
            )));
        }
        if !(0.1..=10.0).contains(&self.lookahead_ms) {
            return Err(SettingsError(format!(
                "limiter lookahead_ms {} must be 0.1 to 10",
                self.lookahead_ms
            )));
        }
        if !(1.0..=5000.0).contains(&self.release_ms) {
            return Err(SettingsError(format!(
                "limiter release_ms {} must be 1 to 5000",
                self.release_ms
            )));
        }
        Ok(())
    }
}

pub struct Limiter {
    channels: usize,
    ceiling: f32,
    lookahead: usize,
    attack: f32,
    release: f32,
    /// The delayed frames, interleaved, oldest first.
    delay: VecDeque<f32>,
    /// Each delayed frame's needed gain, oldest first.
    needed: VecDeque<f32>,
    /// A monotonic queue of (frame number, needed gain) for the window minimum.
    window: VecDeque<(u64, f32)>,
    frame: u64,
    env: f32,
}

impl Limiter {
    pub fn new(settings: &LimiterSettings, channels: usize) -> Self {
        let lookahead = ((settings.lookahead_ms * 0.001 * RATE as f32).round() as usize).max(1);
        let mut l = Limiter {
            channels: channels.max(1),
            ceiling: db_to_lin(settings.ceiling_db),
            lookahead,
            // Reach the window's minimum well within the look-ahead.
            attack: coef(settings.lookahead_ms / 5.0, RATE),
            release: coef(settings.release_ms, RATE),
            delay: VecDeque::new(),
            needed: VecDeque::new(),
            window: VecDeque::new(),
            frame: 0,
            env: 1.0,
        };
        // Start with a delay line of silence, so the latency is fixed from the first sample.
        for _ in 0..lookahead {
            l.delay.extend(std::iter::repeat_n(0.0, l.channels));
            l.needed.push_back(1.0);
            l.push_needed(1.0);
        }
        l
    }

    /// The latency in frames.
    pub fn latency_frames(&self) -> usize {
        self.lookahead
    }

    /// The gain applied to the last frame out (1.0 when not limiting).
    pub fn gain(&self) -> f32 {
        self.env
    }

    fn push_needed(&mut self, g: f32) {
        while self.window.back().is_some_and(|&(_, v)| v >= g) {
            self.window.pop_back();
        }
        self.window.push_back((self.frame, g));
        self.frame += 1;
        // The window covers the delayed frames and the new one: lookahead + 1 frames.
        let oldest = self.frame.saturating_sub(self.lookahead as u64 + 1);
        while self.window.front().is_some_and(|&(n, _)| n < oldest) {
            self.window.pop_front();
        }
    }

    /// Limit interleaved samples in place; the output is the input delayed by the look-ahead.
    pub fn process(&mut self, samples: &mut [f32]) {
        let ch = self.channels;
        for frame in samples.chunks_exact_mut(ch) {
            let peak = frame.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let needed = if peak > self.ceiling {
                self.ceiling / peak
            } else {
                1.0
            };
            self.delay.extend(frame.iter().copied());
            self.needed.push_back(needed);
            self.push_needed(needed);
            let target = self.window.front().map_or(1.0, |&(_, v)| v);
            let k = if target < self.env {
                self.attack
            } else {
                self.release
            };
            self.env += (target - self.env) * k;
            let own = self.needed.pop_front().unwrap_or(1.0);
            let g = self.env.min(own);
            for s in frame.iter_mut() {
                let v = self.delay.pop_front().unwrap_or(0.0) * g;
                *s = v.clamp(-self.ceiling, self.ceiling);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::{peak, sine};

    #[test]
    fn nothing_exceeds_the_ceiling() {
        let s = LimiterSettings::default();
        let mut l = Limiter::new(&s, 2);
        // A sum of three loud voices: up to 2.4 times full scale.
        let mut x: Vec<f32> = sine(220.0, 0.8, 1.0)
            .iter()
            .zip(sine(1000.0, 0.8, 1.0))
            .zip(sine(3100.0, 0.8, 1.0))
            .map(|((a, b), c)| a + b + c)
            .collect();
        assert!(peak(&x) > 2.0);
        for chunk in x.chunks_mut(960) {
            l.process(chunk);
        }
        assert!(peak(&x) <= db_to_lin(-1.0) + 1e-6, "peak {}", peak(&x));
        assert!(peak(&x) > db_to_lin(-3.0), "the limiter over-reduced");
    }

    #[test]
    fn a_quiet_signal_passes_unchanged_after_the_lookahead() {
        let s = LimiterSettings::default();
        let mut l = Limiter::new(&s, 2);
        let x = sine(1000.0, 0.25, 0.2);
        let mut y = x.clone();
        l.process(&mut y);
        let d = l.latency_frames() * 2;
        assert_eq!(d, 144, "1.5 ms at 48 kHz, two channels");
        assert!(y[..d].iter().all(|v| *v == 0.0));
        for (a, b) in x.iter().zip(&y[d..]) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn a_single_full_scale_click_is_caught_before_it_arrives() {
        let s = LimiterSettings::default();
        let mut l = Limiter::new(&s, 1);
        let mut x = vec![0.1f32; 48_000];
        x[2400] = 1.0;
        x[2401] = -1.0;
        l.process(&mut x);
        assert!(peak(&x) <= db_to_lin(-1.0) + 1e-6);
        // The gain comes back within a second of the click.
        assert!(l.gain() > 0.99, "gain {}", l.gain());
    }

    #[test]
    fn bad_settings_are_refused() {
        let bad = LimiterSettings {
            ceiling_db: 3.0,
            ..LimiterSettings::default()
        };
        assert!(bad.validate().is_err());
        assert!(LimiterSettings::default().validate().is_ok());
    }
}
