// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Measuring the desk: taps that record a bus with arrival times, tone levels, and click
//! onsets, as spike 01's harness measured them (every latency on one monotonic clock).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::pipeline::{RATE, RAW_CAPS};

/// One recorded buffer: its arrival time and its samples, mixed to mono.
pub type Chunk = (Instant, Vec<i16>);

/// A recording of one stream.
#[derive(Clone, Default)]
pub struct Tap {
    chunks: Arc<Mutex<Vec<Chunk>>>,
}

impl Tap {
    /// A recording fed by the desk's mixing pass ([`crate::pipeline::Desk::tap`]).
    pub fn new() -> Tap {
        Tap::default()
    }

    /// Record a block of interleaved stereo `f32`, as it leaves a bus, mixed to mono 16-bit.
    pub fn push_f32(&self, at: Instant, samples: &[f32]) {
        let mono: Vec<i16> = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| (((f[0] + f[1]) * 0.5).clamp(-1.0, 1.0) * 32767.0).round() as i16)
            .collect();
        self.chunks.lock().expect("tap lock").push((at, mono));
    }

    /// Record from `upstream` (an element with a free or request src pad) into an appsink.
    pub fn attach(pipeline: &gst::Pipeline, upstream: &gst::Element, name: &str) -> Result<Tap> {
        let (head, sink) = Self::chain(pipeline, name)?;
        upstream.link(&head)?;
        Ok(Self::finish(sink, pipeline))
    }

    /// Record from a pad (a `livekitwebrtcsrc` pad, say).
    pub fn attach_pad(pipeline: &gst::Pipeline, pad: &gst::Pad, name: &str) -> Result<Tap> {
        let (head, sink) = Self::chain(pipeline, name)?;
        let tap = Self::finish(sink, pipeline);
        pad.link(&head.static_pad("sink").expect("queue sink"))?;
        Ok(tap)
    }

    fn chain(pipeline: &gst::Pipeline, name: &str) -> Result<(gst::Element, gst_app::AppSink)> {
        let q = gst::ElementFactory::make("queue").build()?;
        let conv = gst::ElementFactory::make("audioconvert").build()?;
        let res = gst::ElementFactory::make("audioresample").build()?;
        let sink = gst_app::AppSink::builder()
            .name(name)
            .caps(&RAW_CAPS.parse::<gst::Caps>()?)
            .sync(false)
            .build();
        pipeline.add_many([&q, &conv, &res, sink.upcast_ref()])?;
        gst::Element::link_many([&q, &conv, &res, sink.upcast_ref()])?;
        for e in [sink.upcast_ref::<gst::Element>(), &res, &conv, &q] {
            e.sync_state_with_parent()?;
        }
        Ok((q, sink))
    }

    fn finish(sink: gst_app::AppSink, _pipeline: &gst::Pipeline) -> Tap {
        let tap = Tap::default();
        let chunks = tap.chunks.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let t = Instant::now();
                    if let Some(buf) = sample.buffer()
                        && let Ok(map) = buf.map_readable()
                    {
                        let mono: Vec<i16> = map
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|f| {
                                let l = i16::from_le_bytes([f[0], f[1]]) as i32;
                                let r = i16::from_le_bytes([f[2], f[3]]) as i32;
                                ((l + r) / 2) as i16
                            })
                            .collect();
                        chunks.lock().expect("tap lock").push((t, mono));
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        tap
    }

    pub fn buffers(&self) -> usize {
        self.chunks.lock().expect("tap lock").len()
    }

    pub fn chunks(&self) -> Vec<Chunk> {
        self.chunks.lock().expect("tap lock").clone()
    }

    pub fn samples(&self) -> Vec<i16> {
        self.chunks().into_iter().flat_map(|(_, s)| s).collect()
    }

    pub fn seconds(&self) -> f64 {
        self.chunks().iter().map(|(_, s)| s.len()).sum::<usize>() as f64 / f64::from(RATE)
    }
}

/// The power of one frequency, summed over 100 ms Hann-windowed blocks (Goertzel). The block
/// is 4,800 samples, so every multiple of 10 Hz falls on a bin.
pub fn tone_power(samples: &[i16], freq: f64) -> f64 {
    const N: usize = 4800;
    let w = 2.0 * std::f64::consts::PI * freq / f64::from(RATE);
    let coeff = 2.0 * w.cos();
    let hann: Vec<f64> = (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos())
        .collect();
    samples
        .as_chunks::<N>()
        .0
        .iter()
        .map(|block| {
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for (x, h) in block.iter().zip(&hann) {
                let s0 = f64::from(*x) * h + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            s1 * s1 + s2 * s2 - coeff * s1 * s2
        })
        .sum()
}

/// `a` against `b` in dB; a floor keeps silence finite.
pub fn ratio_db(a: f64, b: f64) -> f64 {
    10.0 * ((a + 1e-3) / (b + 1e-3)).log10()
}

/// Peak level in dBFS.
pub fn peak_dbfs(samples: &[i16]) -> f64 {
    let peak = samples
        .iter()
        .map(|s| i32::from(*s).abs())
        .max()
        .unwrap_or(0);
    20.0 * (f64::from(peak.max(1)) / 32768.0).log10()
}

/// A click: when it began, and whether it was low (under 2 kHz) or high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    pub at: Instant,
    pub high: bool,
}

/// Click onsets in a recording: a sample over `threshold` after at least 50 ms under it. A
/// sample's time is its buffer's arrival time less the samples after it in that buffer.
pub fn onsets(chunks: &[Chunk], threshold: i16) -> Vec<Onset> {
    let mut times = Vec::new();
    let mut all = Vec::new();
    for (t, s) in chunks {
        let n = s.len();
        for (i, x) in s.iter().enumerate() {
            let back = (n - i) as f64 / f64::from(RATE);
            times.push(
                t.checked_sub(std::time::Duration::from_secs_f64(back))
                    .unwrap_or(*t),
            );
            all.push(*x);
        }
    }
    let gap = (RATE / 20) as usize;
    let mut out = Vec::new();
    let mut last_loud: Option<usize> = None;
    for i in 0..all.len() {
        if all[i].unsigned_abs() > threshold.unsigned_abs() {
            if last_loud.is_none_or(|j| i - j > gap) {
                let seg = &all[i..(i + 240).min(all.len())];
                let crossings = seg.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
                let hz = crossings as f64 / 2.0 / (seg.len() as f64 / f64::from(RATE));
                out.push(Onset {
                    at: times[i],
                    high: hz >= 2000.0,
                });
            }
            last_loud = Some(i);
        }
    }
    out
}

/// Delays from each sent click to the same kind of click received, in ms: each received click
/// is matched to the latest sent one before it, within `window_s`.
pub fn delays_ms(sent: &[Onset], received: &[Onset], high: bool, window_s: f64) -> Vec<f64> {
    received
        .iter()
        .filter(|r| r.high == high)
        .filter_map(|r| {
            sent.iter()
                .filter(|s| s.high == high && s.at <= r.at)
                .map(|s| (r.at - s.at).as_secs_f64())
                .filter(|d| *d < window_s)
                .reduce(f64::min)
                .map(|d| d * 1000.0)
        })
        .collect()
}

pub fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, secs: f64, amp: f64) -> Vec<i16> {
        let n = (secs * f64::from(RATE)) as usize;
        (0..n)
            .map(|i| {
                (amp * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(RATE)).sin())
                    as i16
            })
            .collect()
    }

    #[test]
    fn a_tone_is_found_and_another_is_not() {
        let s = sine(1000.0, 1.0, 10_000.0);
        let r = ratio_db(tone_power(&s, 3000.0), tone_power(&s, 1000.0));
        assert!(r < -80.0, "{r}");
    }

    #[test]
    fn a_tone_40_db_down_measures_40_db_down() {
        let a = sine(1000.0, 1.0, 10_000.0);
        let b = sine(3000.0, 1.0, 100.0);
        let mix: Vec<i16> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let r = ratio_db(tone_power(&mix, 3000.0), tone_power(&mix, 1000.0));
        assert!((r + 40.0).abs() < 0.5, "{r}");
    }

    #[test]
    fn onsets_and_their_pitch_are_found() {
        let t0 = Instant::now();
        let mut s = vec![0i16; 4800];
        s.extend(sine(1000.0, 0.01, 20_000.0));
        s.extend(vec![0i16; 9600]);
        s.extend(sine(3000.0, 0.01, 20_000.0));
        s.extend(vec![0i16; 4800]);
        let end = t0 + std::time::Duration::from_secs_f64(s.len() as f64 / f64::from(RATE));
        let found = onsets(&[(end, s)], 3000);
        assert_eq!(found.len(), 2);
        assert!(!found[0].high && found[1].high);
        let gap = (found[1].at - found[0].at).as_secs_f64();
        assert!((gap - 0.21).abs() < 0.002, "{gap}");
    }

    #[test]
    fn delays_match_the_latest_earlier_click() {
        let t = Instant::now();
        let at = |ms: u64| t + std::time::Duration::from_millis(ms);
        let sent = [
            Onset {
                at: at(0),
                high: false,
            },
            Onset {
                at: at(2000),
                high: false,
            },
        ];
        let recv = [
            Onset {
                at: at(150),
                high: false,
            },
            Onset {
                at: at(2160),
                high: false,
            },
            Onset {
                at: at(2170),
                high: true,
            },
        ];
        let d = delays_ms(&sent, &recv, false, 1.9);
        assert_eq!(d.len(), 2);
        assert!((d[0] - 150.0).abs() < 1.0 && (d[1] - 160.0).abs() < 1.0);
        assert_eq!(median(&d), Some(d[1]));
    }
}
