// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The fade of a source's On when the schedule moves it (ADR-0280, design section 10): the gate
//! that opens or closes for a voice ramps over `fade_ms` instead of over one block.
//!
//! The count is of blocks mixed, one per pass, never of the clock: a pass that catches up mixes
//! several blocks, and the fade is that many blocks shorter in time, as the sound is. Every bus
//! a voice goes to steps its own gain by the same [`Fade::step`] each block, so the programme
//! and every mix-minus rise and fall together, and a bus made in the middle of a fade joins it
//! where it stands and ends with it.

/// A fade in progress for one voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    step: f32,
    blocks_left: u32,
}

impl Fade {
    /// A fade over `ms` milliseconds of `block_frames`-frame blocks at `rate`. `None` when it
    /// would be no longer than one block (the gate's own ramp is that already) or `ms` is 0.
    pub fn new(ms: u32, block_frames: usize, rate: u32) -> Option<Fade> {
        if ms == 0 || block_frames == 0 {
            return None;
        }
        let frames = u64::from(ms) * u64::from(rate) / 1000;
        let blocks = frames.div_ceil(block_frames as u64);
        if blocks <= 1 {
            return None;
        }
        let blocks = u32::try_from(blocks).unwrap_or(u32::MAX);
        Some(Fade {
            step: 1.0 / blocks as f32,
            blocks_left: blocks,
        })
    }

    /// How far a gain may move toward its target in this block (1.0 is the whole way).
    pub fn step(&self) -> f32 {
        self.step
    }

    /// One block has been mixed; whether the fade goes on.
    pub fn tick(&mut self) -> bool {
        self.blocks_left = self.blocks_left.saturating_sub(1);
        self.blocks_left > 0
    }
}

/// `gain` moved toward `target` by at most `step`, landing on it exactly rather than past it. A
/// `step` of 1.0 or more is the jump the gate always made.
pub fn ramp_toward(gain: f32, target: f32, step: f32) -> f32 {
    if (target - gain).abs() <= step {
        target
    } else if target > gain {
        gain + step
    } else {
        gain - step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RATE, mix_into};

    const BLOCK: usize = 240; // 5 ms

    #[test]
    fn two_seconds_of_five_millisecond_blocks_is_four_hundred_steps() {
        let mut f = Fade::new(2000, BLOCK, RATE).expect("a fade");
        assert_eq!(f.blocks_left, 400);
        assert!((f.step() - 1.0 / 400.0).abs() < 1e-9);
        for _ in 0..399 {
            assert!(f.tick());
        }
        assert!(!f.tick(), "the four hundredth block ends it");
    }

    #[test]
    fn no_fade_for_nothing_or_for_no_longer_than_a_block() {
        assert_eq!(Fade::new(0, BLOCK, RATE), None);
        assert_eq!(Fade::new(4, BLOCK, RATE), None);
        assert_eq!(Fade::new(5, BLOCK, RATE), None);
        assert!(Fade::new(6, BLOCK, RATE).is_some());
        assert_eq!(Fade::new(2000, 0, RATE), None);
    }

    #[test]
    fn a_step_of_one_is_the_old_jump_and_a_fade_lands_exactly() {
        assert_eq!(ramp_toward(0.0, 1.0, 1.0), 1.0);
        assert_eq!(ramp_toward(1.0, 0.0, 1.0), 0.0);
        assert_eq!(ramp_toward(0.3, 0.3, 0.1), 0.3);
        assert_eq!(ramp_toward(0.95, 1.0, 0.1), 1.0, "never past it");
        assert!((ramp_toward(0.5, 1.0, 0.1) - 0.6).abs() < 1e-6);
        assert!((ramp_toward(0.5, 0.0, 0.1) - 0.4).abs() < 1e-6);
    }

    /// Mix a constant voice for `blocks` blocks the way the desk's pass does, and return the
    /// gain each block ended on.
    fn gains(from: f32, target: f32, step: f32, blocks: usize) -> Vec<f32> {
        let mut g = from;
        let src = vec![1.0f32; BLOCK * 2];
        (0..blocks)
            .map(|_| {
                let next = ramp_toward(g, target, step);
                let mut acc = vec![0.0f32; BLOCK * 2];
                mix_into(&mut acc, &src, 2, g, next);
                g = next;
                g
            })
            .collect()
    }

    #[test]
    fn a_two_second_fade_rises_evenly_and_a_crossfade_keeps_the_sum_in_step() {
        let mut f = Fade::new(2000, BLOCK, RATE).unwrap();
        let (mut on, mut off) = (0.0f32, 1.0f32);
        let mut worst = 0.0f32;
        for block in 0..400 {
            on = ramp_toward(on, 1.0, f.step());
            off = ramp_toward(off, 0.0, f.step());
            // Halfway the two are about equal, and at every point they sum to about one.
            if block == 199 {
                assert!(
                    (on - 0.5).abs() < 0.01 && (off - 0.5).abs() < 0.01,
                    "{on} {off}"
                );
            }
            worst = worst.max((on + off - 1.0).abs());
            f.tick();
        }
        assert!(worst < 1e-3, "linear gains sum to one: {worst}");
        // Both end within float error of where they were going; once the fade is over the
        // plain gate lands them exactly (a step of 8e-7, which no one hears).
        assert!((on - 1.0).abs() < 1e-5 && off < 1e-5, "{on} {off}");
        assert_eq!(
            (ramp_toward(on, 1.0, 1.0), ramp_toward(off, 0.0, 1.0)),
            (1.0, 0.0)
        );
    }

    #[test]
    fn the_gain_never_steps_more_than_the_fade_asks_so_there_is_no_click() {
        let step = Fade::new(2000, BLOCK, RATE).unwrap().step();
        let g = gains(0.0, 1.0, step, 400);
        let mut prev = 0.0;
        for x in &g {
            assert!(x - prev <= step + 1e-6);
            assert!(*x >= prev);
            prev = *x;
        }
        assert!((g.last().unwrap() - 1.0).abs() < 1e-5);
        // A bus made late joins a fade already running from zero and ends with the others.
        let late = gains(0.0, 1.0, step, 400 - 150);
        assert!(
            *late.last().unwrap() < 1.0,
            "it does not arrive before its time"
        );
    }
}
