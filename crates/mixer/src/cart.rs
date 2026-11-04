// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A cart as a source of the mixing pass (ADR-0117): a decoder fills a [`CartFeed`] a little
//! ahead of the play head, and the pass takes one block a tick through a [`CartPlayer`], which
//! applies the cart's stored gain, its 300 ms stop fade and, for a bed, the dip under an open
//! microphone.
//!
//! The decoder runs on a thread of its own (GStreamer's, in the desk) and is the only side
//! that ever waits: it blocks while the feed is full. The pass never waits on a cart; a cart
//! whose decoder has fallen behind is silence for that block, as a late voice is.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::{RATE, coef, db_to_lin};

/// How far a bed dips while a microphone is open (report 08; the playout interim used the same).
pub const DUCK_DB: f32 = -12.0;
/// How fast a bed dips and comes back up (a one-pole time constant).
pub const DUCK_MS: f32 = 250.0;
/// How long a stopped cart takes to fade out (cart-wall.md section 4).
pub const STOP_FADE_MS: u32 = 300;

struct FeedInner {
    samples: VecDeque<f32>,
    /// The decoder has delivered the whole file (for a bed, it never does).
    eos: bool,
    /// The player let go: the decoder stops at its next push.
    closed: bool,
    /// Why the decoder failed, if it did.
    failed: Option<String>,
}

/// Decoded audio on its way from a decoder to the pass: interleaved `f32` at the desk's rate.
pub struct CartFeed {
    channels: usize,
    /// The most samples it holds before the decoder waits.
    capacity: usize,
    inner: Mutex<FeedInner>,
    space: Condvar,
    data: Condvar,
}

/// What a pull found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pulled {
    /// Frames of the cart in the block (the rest is silence).
    pub frames: usize,
    /// Nothing is left and nothing more will come: the cart has ended (or failed).
    pub drained: bool,
}

impl CartFeed {
    /// A feed of `channels`-channel audio holding at most `ahead` of it.
    pub fn new(channels: usize, ahead: Duration) -> Self {
        let channels = channels.max(1);
        let frames = (ahead.as_secs_f64() * f64::from(RATE)).ceil() as usize;
        Self {
            channels,
            capacity: frames.max(1) * channels,
            inner: Mutex::new(FeedInner {
                samples: VecDeque::new(),
                eos: false,
                closed: false,
                failed: None,
            }),
            space: Condvar::new(),
            data: Condvar::new(),
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FeedInner> {
        self.inner.lock().expect("cart feed lock")
    }

    /// The decoder's side: add samples, waiting while the feed is full. `false` once the
    /// player has let go, so the decoder stops.
    pub fn push(&self, mut samples: &[f32]) -> bool {
        let mut g = self.lock();
        while !samples.is_empty() {
            if g.closed {
                return false;
            }
            let room = self.capacity.saturating_sub(g.samples.len());
            if room == 0 {
                g = self.space.wait(g).expect("cart feed lock");
                continue;
            }
            let take = room.min(samples.len());
            g.samples.extend(&samples[..take]);
            samples = &samples[take..];
            self.data.notify_all();
        }
        !g.closed
    }

    /// The decoder has delivered everything.
    pub fn finish(&self) {
        self.lock().eos = true;
        self.data.notify_all();
    }

    /// The decoder failed: what it delivered still plays, then the cart ends.
    pub fn fail(&self, why: &str) {
        let mut g = self.lock();
        g.eos = true;
        g.failed.get_or_insert_with(|| why.to_owned());
        self.data.notify_all();
    }

    pub fn failed(&self) -> Option<String> {
        self.lock().failed.clone()
    }

    /// The player's side: let go. A decoder waiting for room wakes and stops.
    pub fn close(&self) {
        let mut g = self.lock();
        g.closed = true;
        g.samples.clear();
        self.space.notify_all();
        self.data.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Wait up to `limit` until at least `frames` are there, or the file ended or failed
    /// before them. Whether anything is there to play.
    pub fn wait_for(&self, frames: usize, limit: Duration) -> bool {
        let want = frames * self.channels;
        let g = self.lock();
        let (g, _) = self
            .data
            .wait_timeout_while(g, limit, |g| g.samples.len() < want && !g.eos && !g.closed)
            .expect("cart feed lock");
        !g.samples.is_empty()
    }

    /// The pass's side, never waiting: fill `out` (interleaved, whole frames) from the front;
    /// silence where nothing has arrived.
    pub fn pull(&self, out: &mut [f32]) -> Pulled {
        let mut g = self.lock();
        let take = out.len().min(g.samples.len());
        let take = take - take % self.channels;
        for (o, s) in out.iter_mut().zip(g.samples.drain(..take)) {
            *o = s;
        }
        out[take..].fill(0.0);
        let drained = (g.eos || g.closed) && g.samples.is_empty();
        if take > 0 {
            self.space.notify_all();
        }
        Pulled {
            frames: take / self.channels,
            drained,
        }
    }

    /// Frames waiting now.
    pub fn frames(&self) -> usize {
        self.lock().samples.len() / self.channels
    }
}

/// One cart sounding: its feed, its gain, its fade and its duck.
pub struct CartPlayer {
    channels: usize,
    gain: f32,
    /// The stop fade: frames left of it, out of how many, from the gain it started at; `None`
    /// until stopped.
    fade: Option<(u64, u64)>,
    /// The bed's duck gain now, moving toward its target a frame at a time.
    duck: f32,
    duck_coef: f32,
    played: u64,
    ended: bool,
    buf: Vec<f32>,
}

impl CartPlayer {
    /// A player at `gain_db`, the gain the media worker stored with the cart.
    pub fn new(channels: usize, gain_db: f32) -> Self {
        Self {
            channels: channels.max(1),
            gain: db_to_lin(gain_db),
            fade: None,
            duck: 1.0,
            duck_coef: coef(DUCK_MS, RATE),
            played: 0,
            ended: false,
            buf: Vec::new(),
        }
    }

    /// Fade out over `ms`, then end; 0 ends at once.
    pub fn stop(&mut self, ms: u32) {
        if self.fade.is_none() {
            let frames = (u64::from(ms) * u64::from(RATE) / 1000).max(1);
            self.fade = Some((frames, frames));
        }
    }

    pub fn stopping(&self) -> bool {
        self.fade.is_some()
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Frames played so far.
    pub fn played(&self) -> u64 {
        self.played
    }

    /// The duck gain now (1 when not ducked).
    pub fn duck(&self) -> f32 {
        self.duck
    }

    /// Take the next block of `frames` from `feed`, with the gain, the fade and the duck
    /// (`duck`: the gain a bed moves toward, 1 for none). Returns the block; after the block
    /// that ends it (its fade ran out, or its file did), [`CartPlayer::ended`] is true.
    pub fn render(&mut self, feed: &CartFeed, frames: usize, duck: f32) -> &[f32] {
        let ch = self.channels;
        self.buf.resize(frames * ch, 0.0);
        if self.ended {
            self.buf.fill(0.0);
            return &self.buf;
        }
        let got = feed.pull(&mut self.buf);
        self.played += got.frames as u64;
        for frame in self.buf.chunks_exact_mut(ch) {
            self.duck += (duck - self.duck) * self.duck_coef;
            let fade = match &mut self.fade {
                Some((left, total)) => {
                    *left = left.saturating_sub(1);
                    *left as f32 / *total as f32
                }
                None => 1.0,
            };
            let g = self.gain * fade * self.duck;
            for s in frame {
                *s *= g;
            }
        }
        if got.drained || self.fade.is_some_and(|(left, _)| left == 0) {
            self.ended = true;
            feed.close();
        }
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lin_to_db;
    use crate::test_signal::{peak, sine};
    use std::sync::Arc;

    const BLOCK: usize = 960;

    fn feed_of(samples: &[f32], eos: bool) -> CartFeed {
        let f = CartFeed::new(2, Duration::from_secs(10));
        assert!(f.push(samples));
        if eos {
            f.finish();
        }
        f
    }

    #[test]
    fn a_cart_plays_at_its_gain_and_ends_with_its_file() {
        let s = sine(1000.0, 0.5, 0.05); // 2,400 frames: two and a half blocks
        let f = feed_of(&s, true);
        let mut p = CartPlayer::new(2, -6.0);
        let b = p.render(&f, BLOCK, 1.0);
        assert!((peak(b) - 0.5 * db_to_lin(-6.0)).abs() < 0.01);
        assert!(!p.ended());
        p.render(&f, BLOCK, 1.0);
        let last = p.render(&f, BLOCK, 1.0).to_vec();
        assert!(p.ended(), "the file ran out");
        assert_eq!(p.played(), 2_400);
        assert!(
            last[480 * 2..].iter().all(|&s| s == 0.0),
            "silence after the end"
        );
        assert!(f.is_closed());
    }

    #[test]
    fn a_stopped_cart_fades_to_nothing_in_300_ms_and_ends() {
        let f = feed_of(&sine(1000.0, 0.5, 2.0), false);
        let mut p = CartPlayer::new(2, 0.0);
        p.render(&f, BLOCK, 1.0);
        p.stop(STOP_FADE_MS);
        assert!(p.stopping());
        let mut blocks = 0;
        let mut last_peak = 1.0;
        while !p.ended() {
            let b = p.render(&f, BLOCK, 1.0);
            let pk = peak(b);
            assert!(pk <= last_peak + 1e-6, "the fade never rises");
            last_peak = pk;
            blocks += 1;
            assert!(blocks < 50);
        }
        assert_eq!(blocks, 15, "300 ms is 15 blocks of 20 ms");
        assert!(p.render(&f, BLOCK, 1.0).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_bed_dips_12_db_under_an_open_microphone_and_comes_back() {
        let f = feed_of(&sine(1000.0, 0.5, 4.0), false);
        let mut p = CartPlayer::new(2, 0.0);
        let level = |p: &mut CartPlayer, duck: f32| lin_to_db(peak(p.render(&f, BLOCK, duck)));
        let open = level(&mut p, 1.0);
        let mut ducked = 0.0;
        for _ in 0..75 {
            ducked = level(&mut p, db_to_lin(DUCK_DB));
        }
        assert!(
            (ducked - open - DUCK_DB).abs() < 0.2,
            "{ducked} against {open}"
        );
        let mut back = 0.0;
        for _ in 0..50 {
            back = level(&mut p, 1.0);
        }
        assert!((back - open).abs() < 0.2);
    }

    #[test]
    fn a_decoder_waits_while_the_feed_is_full_and_stops_when_let_go() {
        let f = Arc::new(CartFeed::new(2, Duration::from_millis(40)));
        let block = sine(500.0, 0.3, 0.02);
        let decoder = {
            let f = f.clone();
            std::thread::spawn(move || {
                let mut pushed = 0;
                while f.push(&block) {
                    pushed += 1;
                }
                pushed
            })
        };
        assert!(f.wait_for(BLOCK, Duration::from_secs(2)));
        std::thread::sleep(Duration::from_millis(50));
        assert!(f.frames() <= 1_920, "never more than 40 ms ahead");
        let mut p = CartPlayer::new(2, 0.0);
        for _ in 0..10 {
            // Each block taken makes room, and the waiting decoder fills it again. Waiting for
            // the refill (rather than racing a 2 ms sleep against the scheduler) keeps what is
            // proved, that a full feed's decoder resumes, without asking a loaded test machine
            // to be real time.
            assert!(
                f.wait_for(BLOCK, Duration::from_secs(2)),
                "the decoder refills the feed"
            );
            assert!(peak(p.render(&f, BLOCK, 1.0)) > 0.2, "the decoder keeps up");
            assert!(f.frames() <= 1_920, "never more than 40 ms ahead");
        }
        p.stop(0);
        p.render(&f, BLOCK, 1.0);
        assert!(p.ended());
        let pushed = decoder.join().unwrap();
        assert!(pushed >= 10);
    }

    #[test]
    fn a_feed_with_nothing_yet_is_silence_not_an_end() {
        let f = CartFeed::new(2, Duration::from_secs(1));
        let mut p = CartPlayer::new(2, 0.0);
        assert!(p.render(&f, BLOCK, 1.0).iter().all(|&s| s == 0.0));
        assert!(!p.ended());
        f.fail("broken");
        p.render(&f, BLOCK, 1.0);
        assert!(p.ended());
        assert_eq!(f.failed().as_deref(), Some("broken"));
    }
}
