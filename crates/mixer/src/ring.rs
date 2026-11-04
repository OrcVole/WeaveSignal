// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A voice's input ring: the thread that receives a voice (a jitter buffer's, for a person in
//! the room) writes decoded samples in as they arrive, at the place on the desk's timeline
//! their timestamps give them, and the mixing pass reads one block every tick, a fixed time
//! behind the clock. The two meet only here, under a lock held for a copy.
//!
//! Reading by timestamp rather than by arrival is what GStreamer's live mixers do: a voice's
//! delay through the desk is the read's distance behind the clock whatever the phase of its
//! packets against the tick, every voice is in step with every other, and a burst (a jitter
//! buffer releasing what it held) lands where it belongs instead of piling up. A packet that
//! arrives after its place was read is dropped, as a live mixer drops it; a gap is silence.
//!
//! Positions are in frames on the desk's timeline (running time at 48 kHz).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counters, for the status and the logs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RingStats {
    /// Blocks that found part of their place empty while the voice was flowing.
    pub underruns: u64,
    /// Frames that arrived after their place had been read, and were dropped.
    pub late_frames: u64,
    /// Frames waiting ahead of the read position now.
    pub frames: usize,
}

struct Inner {
    /// The position of the front of `samples`; `None` before anything arrived.
    start: Option<i64>,
    samples: VecDeque<f32>,
    /// When the latest audio arrived, on the timeline.
    arrived: Option<i64>,
}

pub struct InputRing {
    channels: usize,
    /// Arrivals within this many frames of the end are taken as following on (timestamps
    /// rounded or dejittered by a frame or two).
    tolerance: i64,
    /// The most the ring holds ahead of its front; more is dropped from the front.
    capacity: usize,
    inner: Mutex<Inner>,
    underruns: AtomicU64,
    late: AtomicU64,
}

impl InputRing {
    /// A ring of interleaved `channels`-channel audio holding at most `capacity` frames.
    pub fn new(channels: usize, capacity: usize) -> Self {
        let channels = channels.max(1);
        Self {
            channels,
            tolerance: 96,
            capacity: capacity.max(1),
            inner: Mutex::new(Inner {
                start: None,
                samples: VecDeque::new(),
                arrived: None,
            }),
            underruns: AtomicU64::new(0),
            late: AtomicU64::new(0),
        }
    }

    /// Write interleaved samples that begin at `pos` on the timeline, arriving at `now`.
    pub fn push(&self, pos: i64, samples: &[f32], now: i64) {
        let ch = self.channels;
        let n = (samples.len() / ch) as i64;
        let mut g = self.inner.lock().expect("ring lock");
        g.arrived = Some(now);
        let start = *g.start.get_or_insert(pos);
        let end = start + (g.samples.len() / ch) as i64;
        let mut pos = pos;
        let mut samples = samples;
        if (pos - end).abs() <= self.tolerance {
            pos = end;
        }
        if pos < end {
            // Already there or already read: keep only what is new.
            let skip = (end - pos).min(n);
            if pos < start {
                self.late
                    .fetch_add((start - pos).min(n) as u64, Ordering::Relaxed);
            }
            samples = &samples[skip as usize * ch..];
        } else if pos > end {
            let gap = (pos - end) as usize;
            if gap > self.capacity {
                g.samples.clear();
                g.start = Some(pos);
            } else {
                g.samples.extend(std::iter::repeat_n(0.0, gap * ch));
            }
        }
        g.samples.extend(samples.iter().copied());
        let over = (g.samples.len() / ch).saturating_sub(self.capacity);
        if over > 0 {
            g.samples.drain(..over * ch);
            g.start = g.start.map(|s| s + over as i64);
        }
    }

    /// Fill `out` (interleaved, a whole number of frames) with the block that begins at `pos`
    /// on the timeline: silence where nothing arrived. Returns whether any of the voice was
    /// in it.
    pub fn pull(&self, pos: i64, out: &mut [f32]) -> bool {
        let ch = self.channels;
        let n = (out.len() / ch) as i64;
        let mut g = self.inner.lock().expect("ring lock");
        out.fill(0.0);
        let Some(start) = g.start else {
            return false;
        };
        let have = (g.samples.len() / ch) as i64;
        let end = start + have;
        // Drop what lies before the block: its time has passed.
        if pos > start {
            let drop = (pos - start).min(have) as usize;
            g.samples.drain(..drop * ch);
        }
        let front = pos.max(start);
        let stop = (pos + n).min(end);
        let take = if stop > front {
            let from = (front - pos) as usize;
            let take = (stop - front) as usize;
            for (o, s) in out[from * ch..(from + take) * ch]
                .iter_mut()
                .zip(g.samples.drain(..take * ch))
            {
                *o = s;
            }
            take
        } else {
            0
        };
        g.start = Some((pos + n).max(start));
        if take > 0 && (take as i64) < n && end < pos + n {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        take > 0
    }

    /// Whether the ring holds the voice up to `until`, or need not be waited for: nothing
    /// has arrived since `flowing_since` (a voice gone quiet, or never started).
    pub fn ready(&self, until: i64, flowing_since: i64) -> bool {
        let g = self.inner.lock().expect("ring lock");
        let end = g
            .start
            .map(|s| s + (g.samples.len() / self.channels) as i64);
        end.is_some_and(|e| e >= until) || g.arrived.is_none_or(|a| a < flowing_since)
    }

    /// When the latest audio arrived, on the timeline; `None` before any did.
    pub fn arrived(&self) -> Option<i64> {
        self.inner.lock().expect("ring lock").arrived
    }

    pub fn stats(&self) -> RingStats {
        let g = self.inner.lock().expect("ring lock");
        RingStats {
            underruns: self.underruns.load(Ordering::Relaxed),
            late_frames: self.late.load(Ordering::Relaxed),
            frames: g.samples.len() / self.channels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(from: usize, frames: usize) -> Vec<f32> {
        (from..from + frames)
            .flat_map(|i| [i as f32, i as f32])
            .collect()
    }

    #[test]
    fn a_block_is_read_from_its_place_on_the_timeline() {
        let r = InputRing::new(2, 48_000);
        let mut out = vec![0.0; 960 * 2];
        r.push(10_000, &ramp(0, 960), 0);
        r.push(10_960, &ramp(960, 960), 0);
        // Read 480 frames before the voice began: half silence, half voice.
        assert!(r.pull(9_520, &mut out));
        assert_eq!(out[0], 0.0);
        assert_eq!(out[2 * 479], 0.0);
        assert_eq!(
            out[2 * 480],
            0.0,
            "the voice's first frame is 0.0 by the ramp"
        );
        assert_eq!(out[2 * 481], 1.0);
        assert!(r.pull(10_480, &mut out));
        assert_eq!(out[0], 480.0);
        assert_eq!(out[2 * 959], 1439.0);
        assert_eq!(r.stats().frames, 480);
    }

    #[test]
    fn a_burst_lands_in_place_and_the_delay_does_not_grow() {
        let r = InputRing::new(2, 48_000);
        let mut out = vec![0.0; 960 * 2];
        // Five packets at once, each in its place.
        for k in 0..5 {
            r.push(k * 960, &ramp(k as usize * 960, 960), 0);
        }
        for k in 0..5 {
            assert!(r.pull(k * 960, &mut out));
            assert_eq!(out[0], (k * 960) as f32);
        }
        assert!(!r.pull(5 * 960, &mut out), "nothing more arrived");
        assert_eq!(r.stats().frames, 0);
    }

    #[test]
    fn a_late_packet_is_dropped_and_a_gap_is_silence() {
        let r = InputRing::new(2, 48_000);
        let mut out = vec![0.0; 960 * 2];
        r.push(0, &ramp(0, 960), 0);
        assert!(r.pull(0, &mut out));
        assert!(!r.pull(960, &mut out), "a gap");
        // The packet for 960 arrives after its place was read.
        r.push(960, &ramp(960, 960), 0);
        assert_eq!(r.stats().late_frames, 960);
        r.push(1920, &ramp(1920, 960), 0);
        assert!(r.pull(1920, &mut out));
        assert_eq!(out[0], 1920.0);
    }

    #[test]
    fn a_block_wholly_before_the_voice_is_silence() {
        let r = InputRing::new(2, 48_000);
        let mut out = vec![1.0; 960 * 2];
        r.push(10_000, &ramp(0, 960), 0);
        assert!(!r.pull(0, &mut out));
        assert!(out.iter().all(|&s| s == 0.0));
        assert!(r.pull(9_960, &mut out));
        assert_eq!(out[2 * 40], 0.0);
        assert_eq!(out[2 * 41], 1.0);
    }

    #[test]
    fn a_voice_is_waited_for_only_while_it_flows() {
        let r = InputRing::new(2, 48_000);
        assert!(
            r.ready(960, 0),
            "a voice that never arrived is not waited for"
        );
        r.push(0, &ramp(0, 480), 1_000);
        assert!(!r.ready(960, 0), "half a block is there");
        assert!(r.ready(480, 0));
        assert!(r.ready(960, 2_000), "nothing since: gone quiet");
    }

    #[test]
    fn timestamps_a_frame_or_two_off_are_taken_as_following_on() {
        let r = InputRing::new(2, 48_000);
        let mut out = vec![0.0; 960 * 2];
        r.push(0, &ramp(0, 960), 0);
        r.push(962, &ramp(960, 960), 0);
        r.push(1918, &ramp(1920, 960), 0);
        assert!(r.pull(0, &mut out));
        assert!(r.pull(960, &mut out));
        assert_eq!(out[0], 960.0);
        assert!(r.pull(1920, &mut out));
        assert_eq!(out[2 * 959], 2879.0);
        assert_eq!(r.stats().underruns, 0);
    }
}
