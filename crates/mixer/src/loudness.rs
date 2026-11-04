// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Loudness as EBU R128 measures it (ITU-R BS.1770): the K-weighting filter, then the mean
//! square over sliding windows, in LUFS. The desk's tile meters show the short-term figure
//! (3 s) against the −23 LUFS target, with the highest momentary figure (400 ms) of the same
//! 3 s as the peak mark.
//!
//! The coefficients are BS.1770's for 48 kHz, the rate everything at the desk runs at. The
//! window is kept as 100 ms blocks, so a reading costs nothing between blocks.

use std::collections::VecDeque;

use crate::RATE;

/// The block the windows are built from.
const BLOCK_MS: usize = 100;
/// Momentary: 400 ms.
const MOMENTARY_BLOCKS: usize = 4;
/// Short-term: 3 s.
const SHORT_TERM_BLOCKS: usize = 30;
/// Below this a reading is silence, not a number (BS.1770's absolute gate).
pub const FLOOR_LUFS: f32 = -70.0;

#[derive(Clone, Copy, Debug)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    const fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Biquad { b, a, z: [0.0; 2] }
    }

    fn run(&mut self, x: f64) -> f64 {
        // Transposed direct form II.
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// BS.1770's K-weighting at 48 kHz: a high shelf (the head), then a high pass.
fn k_weighting() -> [Biquad; 2] {
    [
        Biquad::new(
            [
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85,
            ],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
        ),
        Biquad::new(
            [1.0, -2.0, 1.0],
            [-1.990_047_454_833_98, 0.990_072_250_366_21],
        ),
    ]
}

/// A loudness meter over interleaved `f32` audio at 48 kHz.
#[derive(Clone, Debug)]
pub struct LoudnessMeter {
    channels: usize,
    filters: Vec<[Biquad; 2]>,
    /// The weighted sum of squares in the block being filled, over every channel.
    sum: f64,
    frames: usize,
    block_frames: usize,
    /// Mean square per finished block (channels summed, as BS.1770 sums them), newest last.
    blocks: VecDeque<f64>,
}

/// One reading: short-term loudness and the highest momentary loudness within it, in LUFS;
/// `None` is silence (under [`FLOOR_LUFS`]) or not yet measured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loudness {
    pub short_term: Option<f32>,
    pub momentary_max: Option<f32>,
}

fn lufs(mean_square: f64) -> Option<f32> {
    if mean_square <= 0.0 {
        return None;
    }
    let l = (-0.691 + 10.0 * mean_square.log10()) as f32;
    (l >= FLOOR_LUFS).then_some(l)
}

impl LoudnessMeter {
    pub fn new(channels: usize) -> Self {
        let channels = channels.max(1);
        LoudnessMeter {
            channels,
            filters: (0..channels).map(|_| k_weighting()).collect(),
            sum: 0.0,
            frames: 0,
            block_frames: RATE as usize * BLOCK_MS / 1000,
            blocks: VecDeque::with_capacity(SHORT_TERM_BLOCKS + 1),
        }
    }

    /// Measure interleaved samples; they are not changed.
    pub fn process(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.channels) {
            for (x, f) in frame.iter().zip(self.filters.iter_mut()) {
                let shelved = f[0].run(f64::from(*x));
                let y = f[1].run(shelved);
                self.sum += y * y;
            }
            self.frames += 1;
            if self.frames == self.block_frames {
                self.blocks.push_back(self.sum / self.frames as f64);
                if self.blocks.len() > SHORT_TERM_BLOCKS {
                    self.blocks.pop_front();
                }
                self.sum = 0.0;
                self.frames = 0;
            }
        }
    }

    /// The momentary loudness (the last 400 ms) in LUFS; `None` for silence or nothing yet.
    /// What the vision mixer follows: who is speaking now, not over the last 3 s.
    pub fn momentary(&self) -> Option<f32> {
        let n = self.blocks.len();
        if n == 0 {
            return None;
        }
        let w = MOMENTARY_BLOCKS.min(n);
        lufs(self.blocks.range(n - w..).sum::<f64>() / w as f64)
    }

    /// The latest reading.
    pub fn reading(&self) -> Loudness {
        if self.blocks.is_empty() {
            return Loudness::default();
        }
        let n = self.blocks.len();
        let short_term = lufs(self.blocks.iter().sum::<f64>() / n as f64);
        let w = MOMENTARY_BLOCKS.min(n);
        let momentary_max = (0..=n - w)
            .map(|i| self.blocks.range(i..i + w).sum::<f64>() / w as f64)
            .fold(0.0f64, f64::max);
        Loudness {
            short_term,
            momentary_max: lufs(momentary_max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::sine;

    fn measure(signal: &[f32]) -> Loudness {
        let mut m = LoudnessMeter::new(2);
        m.process(signal);
        m.reading()
    }

    #[test]
    fn a_stereo_1k_sine_at_minus_23_dbfs_reads_minus_23_lufs() {
        // EBU Tech 3341, case 1: a 1 kHz sine at −23 dBFS in each channel is −23.0 LUFS.
        let r = measure(&sine(1000.0, 10f32.powf(-23.0 / 20.0), 4.0));
        let st = r.short_term.unwrap();
        assert!((st + 23.0).abs() < 0.1, "short-term {st}");
        let m = r.momentary_max.unwrap();
        assert!((m + 23.0).abs() < 0.1, "momentary {m}");
    }

    #[test]
    fn momentary_follows_the_last_400_ms_only() {
        let mut m = LoudnessMeter::new(2);
        assert_eq!(m.momentary(), None);
        m.process(&sine(1000.0, 10f32.powf(-23.0 / 20.0), 2.0));
        let loud = m.momentary().unwrap();
        assert!((loud + 23.0).abs() < 0.2, "{loud}");
        m.process(&vec![0.0; 48_000 * 2 / 2]);
        // Half a second of silence: the speaker has stopped, though the 3 s reading has not.
        assert_eq!(m.momentary(), None);
        assert!(m.reading().short_term.is_some());
    }

    #[test]
    fn ten_db_down_reads_ten_lu_lower() {
        let st = measure(&sine(1000.0, 10f32.powf(-33.0 / 20.0), 4.0))
            .short_term
            .unwrap();
        assert!((st + 33.0).abs() < 0.1, "{st}");
    }

    #[test]
    fn low_frequencies_weigh_less() {
        // The high pass of K-weighting: 30 Hz reads well under 1 kHz at the same level.
        let a = 10f32.powf(-23.0 / 20.0);
        let low = measure(&sine(30.0, a, 4.0)).short_term.unwrap();
        assert!(low < -25.0, "{low}");
    }

    #[test]
    fn silence_and_nothing_read_as_none() {
        assert_eq!(LoudnessMeter::new(2).reading(), Loudness::default());
        assert_eq!(measure(&vec![0.0; 48_000 * 2]).short_term, None);
    }

    #[test]
    fn the_peak_mark_holds_a_burst_the_short_term_averages_away() {
        let mut s = vec![0.0f32; 48_000 * 2 * 2];
        s.extend(sine(1000.0, 10f32.powf(-20.0 / 20.0), 0.5));
        s.extend(vec![0.0f32; 48_000]);
        let r = measure(&s);
        let (st, m) = (r.short_term.unwrap(), r.momentary_max.unwrap());
        assert!(m > -21.0 && m < -19.5, "momentary {m}");
        assert!(st < m - 5.0, "short-term {st} against momentary {m}");
    }
}
