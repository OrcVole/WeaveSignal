// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! One clock at the desk (Phase 17g): a source with a clock of its own (a host's software, a
//! VBAN sender) is resampled once at the desk's edge, at a ratio steered so that it stays a
//! cushion ahead of the mixing pass, instead of being placed afresh each time it has drifted by
//! the cushion (a jump of up to the cushion about every 100 minutes at 50 ppm).
//!
//! The follower sees, at each arrival, how far ahead of the mixing pass the source's samples
//! reach (`ahead`, in frames). Arrivals are late by the network's and the decoder's jitter but
//! never early, so it reads the *upper envelope*: the most `ahead` over each 10 s block. From
//! two blocks it knows how fast `ahead` moves, so the ratio that would hold it still (the
//! source's clock against the desk's); it moves its estimate half way there each block. A
//! slow pull (a minute's time constant, at most 200 ppm) brings `ahead` back to the cushion.
//! The ratio stays within ±500 ppm and moves at most 250 ppm a block, so it never jumps: 250
//! ppm is under half a cent of pitch.
//!
//! A block whose envelope stepped by more than the drift could explain (a pause the source
//! never caught up, a burst after a stall) is not believed: the estimate holds, and the step
//! is left to the pull. [`RateFollower::gap`] (the source was placed afresh) forgets the blocks
//! and keeps the estimate: the clock is the same clock.

/// The most the ratio may be steered from 1.
pub const MAX_PPM: f64 = 500.0;
/// The most the ratio moves at one block.
const MAX_STEP_PPM: f64 = 250.0;
/// The most the pull back to the cushion adds.
const MAX_PULL_PPM: f64 = 200.0;
/// A block, in frames at 48 kHz.
const BLOCK: i64 = 10 * 48_000;
/// The pull's time constant, in frames.
const PULL: f64 = 60.0 * 48_000.0;
/// How far toward each new estimate the follower moves.
const GAIN: f64 = 0.5;

#[derive(Clone, Copy, Debug)]
struct Block {
    /// The most `ahead` in the block.
    top: f64,
    /// The block's middle, in frames of the desk.
    at: f64,
    /// The ratio's mean over the block.
    ratio: f64,
}

/// Steers a [`crate::Resampler`]'s ratio to hold a source a cushion ahead of the mix.
#[derive(Clone, Debug)]
pub struct RateFollower {
    target: f64,
    ratio: f64,
    /// The ratio that would hold `ahead` still: the source's clock against the desk's.
    clock: f64,
    /// The block being filled: when it began, its top, the ratio's sum and count.
    start: Option<i64>,
    top: f64,
    ratio_sum: f64,
    n: u64,
    last: Option<Block>,
    /// Estimates taken.
    estimates: u64,
    /// Blocks not believed (a step), and gaps.
    steps: u64,
    gaps: u64,
}

impl RateFollower {
    /// Hold the source `cushion` frames ahead of the mixing pass.
    pub fn new(cushion: i64) -> Self {
        RateFollower {
            target: cushion as f64,
            ratio: 1.0,
            clock: 1.0,
            start: None,
            top: f64::MIN,
            ratio_sum: 0.0,
            n: 0,
            last: None,
            estimates: 0,
            steps: 0,
            gaps: 0,
        }
    }

    /// The ratio to resample at now (above 1: the source runs fast and is read faster).
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// The source's clock against the desk's, in parts per million (positive: it runs fast).
    pub fn ppm(&self) -> f64 {
        (self.clock - 1.0) * 1e6
    }

    /// Blocks whose step was left to the pull, and gaps, since it began.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    pub fn gaps(&self) -> u64 {
        self.gaps
    }

    /// Whether the clock has been estimated yet (two blocks, about 20 s).
    pub fn estimated(&self) -> bool {
        self.estimates > 0
    }

    /// The source was placed afresh (a gap, a restart): forget the blocks, keep the clock.
    pub fn gap(&mut self) {
        self.start = None;
        self.top = f64::MIN;
        self.ratio_sum = 0.0;
        self.n = 0;
        self.last = None;
        self.gaps += 1;
    }

    /// A piece arrived at `now` (the mixing pass's position, in frames) and the source now
    /// reaches `ahead` frames past it; the ratio to resample the next piece at.
    pub fn update(&mut self, now: i64, ahead: i64) -> f64 {
        let start = *self.start.get_or_insert(now);
        self.top = self.top.max(ahead as f64);
        self.ratio_sum += self.ratio;
        self.n += 1;
        if now - start < BLOCK {
            return self.ratio;
        }
        let block = Block {
            top: self.top,
            at: (start + now) as f64 / 2.0,
            ratio: self.ratio_sum / self.n as f64,
        };
        self.start = Some(now);
        self.top = f64::MIN;
        self.ratio_sum = 0.0;
        self.n = 0;
        if let Some(prev) = self.last {
            // `ahead` moves at (source / ratio − 1) frames per frame, so the source runs at the
            // ratio between the blocks times (1 + that slope).
            let slope = (block.top - prev.top) / (block.at - prev.at).max(1.0);
            let seen = (prev.ratio + block.ratio) / 2.0 * (1.0 + slope);
            if ((seen - 1.0) * 1e6).abs() <= 2.0 * MAX_PPM {
                self.clock += GAIN * (seen - self.clock);
                self.clock = self.clock.clamp(1.0 - MAX_PPM * 1e-6, 1.0 + MAX_PPM * 1e-6);
                self.estimates += 1;
            } else {
                self.steps += 1;
            }
        }
        self.last = Some(block);
        // Ahead of the cushion: read a little faster.
        let pull =
            ((block.top - self.target) / PULL).clamp(-MAX_PULL_PPM * 1e-6, MAX_PULL_PPM * 1e-6);
        let want = (self.clock * (1.0 + pull)).clamp(1.0 - MAX_PPM * 1e-6, 1.0 + MAX_PPM * 1e-6);
        let step = MAX_STEP_PPM * 1e-6;
        self.ratio = want.clamp(self.ratio - step, self.ratio + step);
        self.ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Resampler;

    const CUSHION: i64 = 300 * 48;

    /// A source whose clock runs `ppm` fast, sending pieces of 1,152 frames that arrive late by
    /// up to `jitter_ms`, through the follower, a resampler (`real`) or its arithmetic, and
    /// placement back to back as the desk's `Placer` does, re-placing when `ahead` has moved
    /// past the cushion. `stall` is a pause of the network (arrivals held, then all at once).
    struct Run {
        ratios: Vec<(f64, f64)>,
        clocks: Vec<(f64, f64)>,
        aheads: Vec<(f64, i64)>,
        replaced: u32,
        follower: RateFollower,
    }

    fn run(ppm: f64, seconds: f64, jitter_ms: f64, stall: Option<(f64, f64)>, real: bool) -> Run {
        let piece = 1152usize;
        let source_rate = 48_000.0 * (1.0 + ppm * 1e-6);
        let mut f = RateFollower::new(CUSHION);
        let mut r = Resampler::steered(48_000);
        let mut next: Option<i64> = None;
        let mut frac_out = 0.0f64;
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut out = Vec::new();
        let input = vec![0.0f32; 2 * piece];
        let mut run = Run {
            ratios: Vec::new(),
            clocks: Vec::new(),
            aheads: Vec::new(),
            replaced: 0,
            follower: f.clone(),
        };
        let mut last_arrival = 0.0f64;
        let mut k = 0u64;
        loop {
            // When the piece was made, in desk seconds, and when it arrives.
            let made = (k + 1) as f64 * piece as f64 / source_rate;
            if made > seconds {
                break;
            }
            let mut arrive = made + rand() * jitter_ms / 1000.0;
            if let Some((at, len)) = stall
                && arrive >= at
                && arrive < at + len
            {
                arrive = at + len;
            }
            arrive = arrive.max(last_arrival);
            last_arrival = arrive;
            let now = (arrive * 48_000.0) as i64;
            let frames = if real {
                out.clear();
                r.set_ratio(f.ratio());
                r.process(&input, &mut out);
                (out.len() / 2) as i64
            } else {
                frac_out += piece as f64 / f.ratio();
                let n = frac_out.floor();
                frac_out -= n;
                n as i64
            };
            let fresh = now + CUSHION - frames;
            let pos = match next {
                Some(p) if (p - fresh).abs() <= CUSHION => p,
                _ => {
                    if next.is_some() {
                        run.replaced += 1;
                        f.gap();
                    }
                    fresh
                }
            };
            next = Some(pos + frames);
            let ahead = pos + frames - now;
            let before = f.ratio();
            let ratio = f.update(now, ahead);
            assert!(
                (ratio - before).abs() <= MAX_STEP_PPM * 1e-6 + 1e-12,
                "a jump of {} ppm",
                (ratio - before) * 1e6
            );
            run.ratios.push((arrive, ratio));
            run.clocks.push((arrive, f.ppm()));
            run.aheads.push((arrive, ahead));
            k += 1;
        }
        run.follower = f;
        run
    }

    fn ratio_at(r: &Run, t: f64) -> f64 {
        r.ratios.iter().find(|(a, _)| *a >= t).map_or(1.0, |x| x.1)
    }

    fn clock_at(r: &Run, t: f64) -> f64 {
        r.clocks.iter().find(|(a, _)| *a >= t).map_or(0.0, |x| x.1)
    }

    fn worst_ahead_after(r: &Run, t: f64) -> i64 {
        r.aheads
            .iter()
            .filter(|(a, _)| *a >= t)
            .map(|(_, x)| (x - CUSHION).abs())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn clocks_up_to_500_ppm_apart_settle_within_a_minute_and_never_jump() {
        for ppm in [-500.0, -200.0, -50.0, 50.0, 200.0, 500.0] {
            let r = run(ppm, 600.0, 30.0, None, false);
            assert_eq!(r.replaced, 0, "{ppm} ppm: placed afresh");
            // The clock is known within a tenth of the drift (and 10 ppm) by 60 s, and from
            // then on; the ratio is that and the pull back to the cushion, which has died away
            // by five minutes.
            for t in [60.0, 120.0, 300.0, 590.0] {
                let got = clock_at(&r, t);
                assert!(
                    (got - ppm).abs() <= ppm.abs() * 0.1 + 10.0,
                    "{ppm} ppm: the clock read {got:.1} ppm at {t} s"
                );
                let steered = (ratio_at(&r, t) - 1.0) * 1e6;
                let slack = if t >= 300.0 { 20.0 } else { MAX_PULL_PPM };
                assert!(
                    (steered - ppm).abs() <= ppm.abs() * 0.1 + slack,
                    "{ppm} ppm: steered {steered:.1} ppm at {t} s"
                );
            }
            // `ahead` stays within 20 ms of the cushion, under 30 ms of arrival jitter, and
            // within 40 ms (30 ms of it jitter) once settled.
            assert!(worst_ahead_after(&r, 0.0) < 50 * 48, "{ppm} ppm");
            assert!(worst_ahead_after(&r, 300.0) < 40 * 48, "{ppm} ppm");
            assert!((r.follower.ppm() - ppm).abs() < ppm.abs() * 0.1 + 10.0);
        }
    }

    #[test]
    fn two_hours_at_200_ppm_are_never_placed_afresh() {
        let r = run(200.0, 7200.0, 30.0, None, false);
        assert_eq!(r.replaced, 0);
        assert!(worst_ahead_after(&r, 300.0) < 40 * 48);
        let got = (ratio_at(&r, 7100.0) - 1.0) * 1e6;
        assert!((got - 200.0).abs() < 15.0, "{got} ppm");
    }

    #[test]
    fn without_the_follower_the_same_source_would_jump() {
        // The control: at 200 ppm a 300 ms cushion is used up in 25 minutes.
        let drift_s = 0.3 / 200e-6;
        assert!(drift_s < 7200.0);
    }

    #[test]
    fn the_real_resampler_is_steered_the_right_way() {
        // Through the real kernel (slower, so 60 s): a fast source is read faster, and `ahead`
        // stays near the cushion; the wrong sign would run to the clamp and re-place.
        for ppm in [400.0, -400.0] {
            let r = run(ppm, 60.0, 10.0, None, true);
            assert_eq!(r.replaced, 0, "{ppm} ppm");
            let got = (ratio_at(&r, 55.0) - 1.0) * 1e6;
            assert!(
                got.signum() == ppm.signum() && got.abs() > 200.0,
                "{ppm}: {got}"
            );
            assert!(worst_ahead_after(&r, 0.0) < 30 * 48);
        }
    }

    #[test]
    fn a_stall_then_a_burst_is_not_taken_for_a_clock() {
        // 250 ms with nothing, then everything at once, at 100 ppm.
        let r = run(100.0, 300.0, 20.0, Some((95.0, 0.25)), false);
        assert_eq!(r.replaced, 0);
        for t in [60.0, 100.0, 110.0, 120.0, 200.0, 290.0] {
            let got = clock_at(&r, t);
            assert!((got - 100.0).abs() < 20.0, "{got} ppm at {t} s");
        }
        assert!(worst_ahead_after(&r, 120.0) < 40 * 48);
    }

    #[test]
    fn a_pause_never_caught_up_is_left_to_the_pull_and_a_gap_keeps_the_clock() {
        // A source on the desk's own clock, 20 ms pieces, its `ahead` moved by the ratio.
        let mut f = RateFollower::new(CUSHION);
        let (mut ahead, mut now, mut frac) = (CUSHION as f64, 0i64, 0.0f64);
        let mut step = |f: &mut RateFollower, ahead: &mut f64, now: &mut i64| {
            *now += 960;
            frac += 960.0 / f.ratio();
            let out = frac.floor();
            frac -= out;
            *ahead += out - 960.0;
            f.update(*now, *ahead as i64);
        };
        for _ in 0..60 * 50 {
            step(&mut f, &mut ahead, &mut now);
        }
        assert!(f.ppm().abs() < 1.0);
        // It loses 100 ms for good: a step, not a clock.
        ahead -= 4_800.0;
        let mut lowest = 1.0f64;
        // At the pull's 200 ppm, 100 ms takes about seven and a half minutes to win back.
        for _ in 0..600 * 50 {
            step(&mut f, &mut ahead, &mut now);
            lowest = lowest.min(f.ratio());
        }
        assert_eq!(f.steps(), 1);
        assert!(f.ppm().abs() < 5.0, "{}", f.ppm());
        // The pull read it slower, within its bound, and won the cushion back.
        let lowest_ppm = (lowest - 1.0) * 1e6;
        assert!(
            (-MAX_PULL_PPM - 10.0..-100.0).contains(&lowest_ppm),
            "{lowest_ppm}"
        );
        assert!((ahead - CUSHION as f64).abs() < 480.0, "{ahead}");
        let clock = f.ppm();
        f.gap();
        assert_eq!(f.gaps(), 1);
        assert_eq!(f.ppm(), clock);
    }
}
