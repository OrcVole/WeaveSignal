// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Any rate to the desk's 48 kHz: a windowed-sinc resampler, streaming, for interleaved stereo.
//!
//! Each output sample is the input around its place weighted by a sinc low-passed at the lower
//! of the two Nyquist frequencies (a little under it, so nothing folds back), under a Blackman
//! window 64 input samples wide. The kernel is tabled at 256 phases and read between two of
//! them, so a sample costs 128 multiplies and no trigonometry. Nothing in the workspace did this
//! before: the media worker keeps each import at its own rate.

/// The desk's rate.
pub const OUT_RATE: u32 = 48_000;
/// Half the kernel's width, in input samples.
const HALF: usize = 32;
const PHASES: usize = 256;

pub struct Resampler {
    rate: u32,
    /// The input still needed, interleaved stereo; its first frame is input frame `base`.
    buf: Vec<f32>,
    /// Where the next output frame falls, in input frames from the start of `buf`.
    pos: f64,
    step: f64,
    /// `PHASES + 1` rows of `2 * HALF` taps.
    table: Vec<f32>,
}

impl Resampler {
    /// From `rate` to 48 kHz; `rate` 8 kHz to 192 kHz.
    pub fn new(rate: u32) -> Self {
        let step = f64::from(rate) / f64::from(OUT_RATE);
        // The cutoff, as a fraction of the input's Nyquist frequency.
        let cut = (1.0 / step).min(1.0) * 0.94;
        let mut table = Vec::with_capacity((PHASES + 1) * 2 * HALF);
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            for j in 0..2 * HALF {
                // The tap for input sample `i + j - HALF + 1` of an output at `i + frac`.
                let x = j as f64 - (HALF as f64 - 1.0) - frac;
                let s = if x.abs() < 1e-9 {
                    cut
                } else {
                    let a = std::f64::consts::PI * cut * x;
                    cut * a.sin() / a
                };
                let w = {
                    let t = (x / HALF as f64).clamp(-1.0, 1.0);
                    let c = std::f64::consts::PI * t;
                    0.42 + 0.5 * c.cos() + 0.08 * (2.0 * c).cos()
                };
                table.push((s * w) as f32);
            }
        }
        Self {
            rate,
            buf: vec![0.0; 2 * (HALF - 1)],
            pos: 0.0,
            step,
            table,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Take interleaved stereo at the input rate; add what can be made to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.rate == OUT_RATE {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        let frames = self.buf.len() / 2;
        let taps = 2 * HALF;
        loop {
            let i = self.pos.floor() as usize;
            if i + taps > frames {
                break;
            }
            let frac = self.pos - i as f64;
            let ph = frac * PHASES as f64;
            let p = (ph.floor() as usize).min(PHASES - 1);
            let t = (ph - p as f64) as f32;
            let (row0, row1) = (
                &self.table[p * taps..(p + 1) * taps],
                &self.table[(p + 1) * taps..(p + 2) * taps],
            );
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for j in 0..taps {
                let k = row0[j] + (row1[j] - row0[j]) * t;
                l += self.buf[2 * (i + j)] * k;
                r += self.buf[2 * (i + j) + 1] * k;
            }
            out.push(l);
            out.push(r);
            self.pos += self.step;
        }
        // Keep what the next output still reads.
        let keep_from = (self.pos.floor() as usize).min(frames);
        self.buf.drain(..2 * keep_from);
        self.pos -= keep_from as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, freq: f64, seconds: f64, amp: f32) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let v = amp
                    * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() as f32;
                [v, -v]
            })
            .collect()
    }

    /// The power of `freq` in the left channel (Goertzel over the whole of it).
    fn power(x: &[f32], rate: u32, freq: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / f64::from(rate);
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let n = x.len() / 2;
        for i in 0..n {
            let s0 = f64::from(x[2 * i]) + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - c * s1 * s2) / (n as f64 * n as f64)
    }

    #[test]
    fn a_tone_from_44_1_khz_keeps_its_pitch_level_and_length() {
        let input = tone(44_100, 1000.0, 2.0, 0.5);
        let mut r = Resampler::new(44_100);
        let mut out = Vec::new();
        // In uneven pieces, as a decoder gives them.
        for chunk in input.chunks(2 * 1152) {
            r.process(chunk, &mut out);
        }
        let frames = out.len() / 2;
        assert!(
            (95_900..=96_000).contains(&frames),
            "{frames} frames for 2 s"
        );
        // Skip the kernel's start; the tone is at 1 kHz at the new rate, not shifted.
        let body = &out[2 * 4_800..2 * 90_000];
        let at = power(body, OUT_RATE, 1000.0);
        let off = power(body, OUT_RATE, 1088.4); // where 1 kHz would land at the wrong rate
        assert!(at > 1000.0 * off, "pitch kept: {at} against {off}");
        let peak = body.iter().step_by(2).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "level kept: {peak}");
        // The right channel is the left's negative still.
        assert!((body[1] + body[0]).abs() < 1e-4);
    }

    #[test]
    fn what_cannot_be_heard_at_48_khz_does_not_fold_back_from_96_khz() {
        // 30 kHz at 96 kHz would alias to 18 kHz at 48 kHz without the low pass.
        let input = tone(96_000, 30_000.0, 1.0, 0.5);
        let mut r = Resampler::new(96_000);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let body = &out[2 * 2_000..];
        let peak = body.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.01, "folded back at {peak}");
    }

    #[test]
    fn at_48_khz_it_passes_through() {
        let input = tone(48_000, 440.0, 0.1, 0.3);
        let mut r = Resampler::new(48_000);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out, input);
    }
}
