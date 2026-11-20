// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's mixing code, shared by the desk and (later) the presenter kit (ADR-0042).
//!
//! Plain Rust on interleaved `f32` samples, no GStreamer, so each processor is tested on its
//! own. The desk mixes in `f32`, so a sum above full scale survives until the output limiter
//! brings it back under the ceiling; mixing in 16-bit integers clipped the programme in the
//! spike's live run.
//!
//! - [`Compressor`]: a feed-forward compressor with a soft knee.
//! - [`Leveller`]: slow gain steering toward a target speech level (the spike's live run: the
//!   presenter at -30.7 dB against a guest's -25.1 dB per 10 ms of speech).
//! - [`Limiter`]: a look-ahead peak limiter whose output never exceeds its ceiling.
//! - [`Expander`]: a downward expander that keeps a quiet microphone's bleed out of the mix.
//! - [`Chain`]: expander, compressor, make-up gain with the leveller, limiter: a
//!   participant's input chain, set per role in the desk's settings.
//! - [`LoudnessMeter`]: EBU R128 short-term and momentary loudness, for the desk's meters.
//! - [`SourceLeveller`]: the slow ±12 dB gain on a host's own studio (ADR-0280), measured over
//!   ten seconds and frozen in silence.
//! - [`InputRing`]: where a voice's receiving thread leaves decoded audio for the mixing pass.
//! - [`DelayLine`]: the profanity delay, with its dump and rebuild.
//! - [`CartFeed`] and [`CartPlayer`]: a cart as a source of the pass, with its gain, its stop
//!   fade and a bed's duck under an open microphone (ADR-0117).
//! - [`Fade`] and [`ramp_toward`]: a voice's gate over seconds instead of one block, for the
//!   schedule's moves (ADR-0280).
//! - [`mix_into`]: one voice added into a bus with a gain ramped across the block.
//! - [`Resampler`]: any rate to 48 kHz (the ingest's workers), and a ratio steered while it
//!   runs (the desk's one clock, Phase 17g).
//! - [`RateFollower`]: steers that ratio so a source with a clock of its own stays a cushion
//!   ahead of the mixing pass.

mod cart;
mod chain;
mod compressor;
mod delay;
mod expander;
mod fade;
mod follow;
mod leveller;
mod limiter;
mod loudness;
mod resample;
mod ring;
mod source_leveller;
mod telephone;

pub use cart::{CartFeed, CartPlayer, DUCK_DB, DUCK_MS, Pulled, STOP_FADE_MS};
pub use chain::{Chain, ChainSettings};
pub use compressor::{Compressor, CompressorSettings};
pub use delay::{DelayLine, DelaySettings, DelayState};
pub use expander::{Expander, ExpanderSettings};
pub use fade::{Fade, ramp_toward};
pub use follow::{MAX_PPM, RateFollower};
pub use leveller::{Leveller, LevellerSettings};
pub use limiter::{Limiter, LimiterSettings};
pub use loudness::{FLOOR_LUFS, Loudness, LoudnessMeter};
pub use resample::Resampler;
pub use ring::{InputRing, RingStats};
pub use source_leveller::{SourceLeveller, SourceLevellerSettings};
pub use telephone::{Telephone, TelephoneSettings};

/// The sample rate everything at the desk runs at.
pub const RATE: u32 = 48_000;

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

/// A one-pole smoothing coefficient for a time constant in milliseconds.
pub(crate) fn coef(ms: f32, rate: u32) -> f32 {
    if ms <= 0.0 {
        1.0
    } else {
        1.0 - (-1.0 / (ms * 0.001 * rate as f32)).exp()
    }
}

/// Add `src` into `acc` (both interleaved, the same length) with a gain moving linearly from
/// `from` to `to` across the block, so a gate that opens or closes does not click. A voice
/// whose gain is 0 at both ends costs nothing.
pub fn mix_into(acc: &mut [f32], src: &[f32], channels: usize, from: f32, to: f32) {
    let channels = channels.max(1);
    if from == 0.0 && to == 0.0 {
        return;
    }
    if from == to {
        for (a, s) in acc.iter_mut().zip(src) {
            *a += s * to;
        }
        return;
    }
    let frames = (acc.len() / channels).max(1) as f32;
    for (i, (a, s)) in acc
        .chunks_exact_mut(channels)
        .zip(src.chunks_exact(channels))
        .enumerate()
    {
        let g = from + (to - from) * (i + 1) as f32 / frames;
        for (x, y) in a.iter_mut().zip(s) {
            *x += y * g;
        }
    }
}

/// Why a setting cannot be used.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("{0}")]
pub struct SettingsError(pub String);

#[cfg(test)]
mod mix_tests {
    use super::mix_into;

    #[test]
    fn a_gain_ramps_across_the_block_and_lands_on_its_target() {
        let src = vec![1.0f32; 8];
        let mut acc = vec![0.5f32; 8];
        mix_into(&mut acc, &src, 2, 0.0, 1.0);
        assert_eq!(acc[0], 0.75);
        assert_eq!(acc[6], 1.5);
        let mut acc = vec![0.0f32; 8];
        mix_into(&mut acc, &src, 2, 0.0, 0.0);
        assert!(acc.iter().all(|&s| s == 0.0));
        mix_into(&mut acc, &src, 2, 1.0, 1.0);
        assert!(acc.iter().all(|&s| s == 1.0));
    }
}

#[cfg(test)]
pub(crate) mod test_signal {
    /// An interleaved stereo sine at `freq` and `amp` (linear), `secs` long.
    pub fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (secs * super::RATE as f32) as usize;
        (0..n)
            .flat_map(|i| {
                let v =
                    amp * (2.0 * std::f32::consts::PI * freq * i as f32 / super::RATE as f32).sin();
                [v, v]
            })
            .collect()
    }

    pub fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    pub fn rms_db(s: &[f32]) -> f32 {
        let p = s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32;
        super::lin_to_db(p.sqrt())
    }
}
