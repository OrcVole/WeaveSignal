// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Issue 16's measurement on real speech: what the presenter's gate does to the last 150 ms of
//! each word. Reads mono 48 kHz little-endian `f32` from a file, scales it so its loudest peak
//! sits at `PEAK_DB` dBFS, finds the words by their pauses, and prints, per word, the energy of
//! its last 150 ms before and after the expander, how far down the gate was (mean and deepest)
//! and the gain at the word's very end.
//!
//! `ffmpeg -i speech.wav -ar 48000 -ac 1 -f f32le speech.f32`, then
//! `cargo run -p weavesignal-mixer --example gate_words -- speech.f32 -12`; `OUT=gated.f32` also
//! writes the gate's output, `HOLD_MS` and `RELEASE_MS` try other settings.

use weavesignal_mixer::{Expander, ExpanderSettings, db_to_lin, lin_to_db};

const RATE: usize = 48_000;

fn rms_db(s: &[f32]) -> f32 {
    let p = s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32;
    lin_to_db(p.sqrt())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a raw f32le mono 48 kHz file");
    let peak_db: f32 = args
        .get(2)
        .map_or(-12.0, |v| v.parse().expect("a peak in dBFS"));
    let bytes = std::fs::read(path).expect("readable file");
    let mut x: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let top = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let k = db_to_lin(peak_db) / top;
    x.iter_mut().for_each(|v| *v *= k);

    // Words: runs of 5 ms frames above -60 dBFS, joined across gaps shorter than 120 ms.
    let frame = RATE / 200;
    let live: Vec<bool> = x
        .chunks(frame)
        .map(|f| lin_to_db(f.iter().fold(0.0f32, |m, v| m.max(v.abs()))) > -60.0)
        .collect();
    let mut words: Vec<(usize, usize)> = Vec::new();
    for (i, on) in live.iter().enumerate() {
        if !on {
            continue;
        }
        match words.last_mut() {
            Some(w) if i - w.1 < 24 => w.1 = i,
            _ => words.push((i, i)),
        }
    }

    let mut settings = ExpanderSettings::default();
    // `HOLD_MS` and `RELEASE_MS` try other values than the default.
    for (name, field) in [
        ("HOLD_MS", &mut settings.hold_ms),
        ("RELEASE_MS", &mut settings.release_ms),
    ] {
        if let Ok(v) = std::env::var(name) {
            *field = v.parse().expect("a number of milliseconds");
        }
    }
    let mut e = Expander::new(&settings, 1);
    let mut y = x.clone();
    let mut red = vec![0.0f32; x.len()];
    for (i, s) in y.iter_mut().enumerate() {
        let mut one = [*s];
        e.process(&mut one);
        *s = one[0];
        red[i] = e.reduction_db();
    }

    // `OUT=path` writes what came out of the gate (f32le mono 48 kHz), to listen to or transcribe.
    if let Ok(out) = std::env::var("OUT") {
        let raw: Vec<u8> = y.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(&out, raw).expect("writable OUT");
    }

    println!("expander {settings:?}, speech peak {peak_db} dBFS");
    println!(
        "word  end_s   in_dB  out_dB  lost_dB  mean_red  max_red  red_at_end  lost40_dB  (last 150 ms; lost40 is its last 40 ms)"
    );
    let mut worst = 0.0f32;
    let mut worst40 = 0.0f32;
    for (n, (_, last)) in words.iter().enumerate() {
        let end = ((last + 1) * frame).min(x.len());
        let from = end.saturating_sub(RATE * 150 / 1000);
        let (a, b) = (rms_db(&x[from..end]), rms_db(&y[from..end]));
        let t = end.saturating_sub(RATE * 40 / 1000);
        let tail = rms_db(&x[t..end]) - rms_db(&y[t..end]);
        worst40 = worst40.max(tail);
        let r = &red[from..end];
        let mean = r.iter().sum::<f32>() / r.len() as f32;
        let max = r.iter().fold(0.0f32, |m, v| m.max(*v));
        worst = worst.max(a - b);
        println!(
            "{:>4} {:>6.2} {:>7.1} {:>7.1} {:>8.1} {:>9.1} {:>8.1} {:>11.1} {:>10.1}",
            n + 1,
            end as f32 / RATE as f32,
            a,
            b,
            a - b,
            mean,
            max,
            red[end - 1],
            tail
        );
    }
    println!(
        "worst loss in a word's last 150 ms: {worst:.1} dB, in its last 40 ms: {worst40:.1} dB, over {} words",
        words.len()
    );
}
