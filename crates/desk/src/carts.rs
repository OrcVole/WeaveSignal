// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Carts in the desk (ADR-0117): eight players for one-shots and sweepers and two for beds,
//! sources of the one mixing pass like any voice. So a cart is heard in every return as well
//! as the programme, sits inside the profanity delay and goes with a dump, passes the bus
//! limiters after the sum, and can be cued into the presenters' ears alone.
//!
//! **Decoding.** The control plane fires a cart by naming its file, a UUID and `.flac` that
//! the media worker wrote in the station's cart directory, never a path. Each sounding cart
//! is decoded by a small GStreamer pipeline of its own (`filesrc`, `flacparse`, `flacdec`,
//! conversion and resampling to the desk's 48 kHz stereo float) into a [`CartFeed`] about a
//! second ahead of the play head; its one streaming thread waits while the feed is full. The
//! pipeline is not the desk's, so a file that fails to decode ends that cart and nothing
//! else. A bed loops by seeking its decoder back to the start when it reaches the end, while
//! the second already in the feed plays, so the loop has no gap.
//!
//! **Ducking.** While any microphone is open on air (a presenter, DJ, guest or caller on air,
//! present and not muted) or a sweeper sounds on air, beds on air dip 12 dB over 250 ms and
//! come back the same way: computed by the pass from the switches it mixes with, not polled.
//!
//! **The per-play log.** Every fire and every end is an event with a sequence number the
//! desk keeps (the last 1,024): the control plane reads the ones after the last it saw
//! (`cart_plays`) and records them; the log line `cart fired` / `cart ended` says the same.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::{Deserialize, Serialize};
use weavesignal_mixer::{CartFeed, CartPlayer, DUCK_DB, RATE, STOP_FADE_MS, db_to_lin};

/// One-shot and sweeper players, and bed players.
pub const SHOT_PLAYERS: usize = 8;
pub const BED_PLAYERS: usize = 2;
/// Play events kept for the control plane to read.
const EVENTS_KEPT: usize = 1024;

/// How a cart plays (report 08's three modes; the cart's `mode` in the database).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CartMode {
    OneShot,
    /// Loops until stopped; dips under an open microphone.
    Bed,
    /// Plays once, and dips the beds while it sounds.
    Sweeper,
}

impl CartMode {
    pub fn pool(self) -> Pool {
        match self {
            CartMode::Bed => Pool::Bed,
            CartMode::OneShot | CartMode::Sweeper => Pool::Shot,
        }
    }
}

/// Which players a cart sounds in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pool {
    Shot,
    Bed,
}

impl Pool {
    pub fn word(self) -> &'static str {
        match self {
            Pool::Shot => "shot",
            Pool::Bed => "bed",
        }
    }
}

/// A fire, as the control plane asks for it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Fire {
    /// The cart's identifier (a UUID), reported back in the status and the play events.
    pub cart: String,
    /// The file's name in the cart directory: a UUID and `.flac`.
    pub file: String,
    /// The gain the media worker stored with the cart, -40 to 20 dB.
    pub gain_db: f32,
    pub mode: CartMode,
    /// Into the presenters' and DJs' ears only, never on air.
    pub cue: bool,
    /// The file's length, for the time left in the status; none for a bed.
    pub duration_ms: Option<u64>,
}

/// What a fire answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Fired {
    pub pool: Pool,
    /// 1 to 8 for one-shots and sweepers, 1 to 2 for beds.
    pub player: u8,
    /// This play's number, as its play events carry it.
    pub play: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CartError {
    #[error("not a cart identifier")]
    BadCart,
    #[error("not a cart file name (a UUID and .flac)")]
    BadFile,
    #[error("the gain must be -40 to 20 dB")]
    BadGain,
    #[error("the cart's file {0} is not in the cart directory")]
    Missing(String),
    #[error("every {0} player is sounding; stop one first")]
    Busy(&'static str),
    #[error("the cart's file {file} cannot be played: {why}")]
    Decode { file: String, why: String },
}

impl CartError {
    /// A word the control plane can act on.
    pub fn code(&self) -> &'static str {
        match self {
            CartError::BadCart | CartError::BadFile | CartError::BadGain => "invalid",
            CartError::Missing(_) => "missing",
            CartError::Busy(_) => "busy",
            CartError::Decode { .. } => "decode",
        }
    }
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || matches!(c, 'a'..='f'),
        })
}

impl Fire {
    /// Every word checked before a file is opened: an identifier, a bare file name the media
    /// worker could have written, and a gain in range.
    pub fn check(&self) -> Result<(), CartError> {
        if !is_uuid(&self.cart) {
            return Err(CartError::BadCart);
        }
        if !self.file.strip_suffix(".flac").is_some_and(is_uuid) {
            return Err(CartError::BadFile);
        }
        if !(-40.0..=20.0).contains(&self.gain_db) {
            return Err(CartError::BadGain);
        }
        Ok(())
    }
}

/// One cart sounding, as the status reports it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SoundingStatus {
    pub cart: String,
    pub pool: Pool,
    pub player: u8,
    pub mode: CartMode,
    pub cue: bool,
    pub play: u64,
    /// How long it has sounded (a bed: across every pass).
    pub elapsed_ms: u64,
    /// What is left of a cart that plays once, if its length was given.
    pub remaining_ms: Option<u64>,
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Fading out.
    pub stopping: bool,
    /// Beds only: dipped under an open microphone now.
    pub ducked: bool,
}

/// The carts in the desk's status.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CartsStatus {
    /// Beds on air are dipped now (a microphone open, or a sweeper sounding).
    pub ducked: bool,
    /// A microphone is open on air.
    pub mic_open: bool,
    pub sounding: Vec<SoundingStatus>,
    /// The last play event's number: more to read with `cart_plays` when it moves.
    pub play_seq: u64,
    /// When this desk started (ms since the Unix epoch): play events are numbered from 1
    /// again by a desk that started again.
    pub started_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayEventKind {
    Fired,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndKind {
    /// The file ran out.
    RanOut,
    /// Stopped by a key, faded over 300 ms.
    Stopped,
    /// The file could not be decoded to its end.
    Failed,
}

/// A cart fired or ended, for the per-play log.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlayEvent {
    /// The event's number; `cart_plays` returns those after a number.
    pub seq: u64,
    /// The play it belongs to: a fire and its end share it.
    pub play: u64,
    pub event: PlayEventKind,
    pub cart: String,
    pub mode: CartMode,
    pub pool: Pool,
    pub player: u8,
    /// Cued (the presenters' ears), not aired.
    pub cue: bool,
    /// When, in milliseconds since the Unix epoch, at the desk: listeners hear it the delay
    /// later.
    pub at_ms: u64,
    /// How long it sounded (an end only).
    pub played_ms: Option<u64>,
    pub end: Option<EndKind>,
    pub error: Option<String>,
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A sounding cart's decoder: a pipeline of its own feeding a [`CartFeed`].
pub struct Decoder {
    pipeline: gst::Pipeline,
    feed: Arc<CartFeed>,
}

static DECODERS: AtomicU64 = AtomicU64::new(1);

impl Decoder {
    /// Start decoding `path` (a bed with `looping`) into a feed `ahead` deep.
    pub fn start(
        path: &Path,
        looping: bool,
        channels: usize,
        ahead: Duration,
    ) -> Result<(Decoder, Arc<CartFeed>)> {
        let n = DECODERS.fetch_add(1, Ordering::Relaxed);
        let pipeline = gst::Pipeline::with_name(&format!("cart-{n}"));
        let src = gst::ElementFactory::make("filesrc")
            .property("location", path.to_string_lossy().as_ref())
            .build()?;
        let mut elements = vec![src];
        for f in ["flacparse", "flacdec", "audioconvert", "audioresample"] {
            elements.push(gst::ElementFactory::make(f).build().with_context(|| {
                format!("GStreamer element `{f}` is missing (is its plugin installed?)")
            })?);
        }
        let caps: gst::Caps = crate::pipeline::MIX_CAPS.parse().expect("valid caps");
        elements.push(
            gst::ElementFactory::make("capsfilter")
                .property("caps", &caps)
                .build()?,
        );
        let sink = gst_app::AppSink::builder()
            .caps(&caps)
            .sync(false)
            .max_buffers(4)
            .build();
        sink.set_property("async", false);
        let feed = Arc::new(CartFeed::new(channels, ahead));
        {
            let feed = feed.clone();
            let eos_feed = feed.clone();
            let weak = pipeline.downgrade();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(buf) = sample.buffer()
                            && let Ok(map) = buf.map_readable()
                        {
                            let samples: Vec<f32> = map
                                .as_slice()
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|b| f32::from_le_bytes(*b))
                                .collect();
                            // Waits while the feed is full; false once the player let go.
                            if !feed.push(&samples) {
                                return Err(gst::FlowError::Flushing);
                            }
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .eos(move |s| {
                        if looping && !eos_feed.is_closed() {
                            // Back to the start for the next pass, off the streaming thread:
                            // the feed still holds about a second, so the loop has no gap.
                            let weak = weak.clone();
                            let feed = eos_feed.clone();
                            s.call_async(move |_| {
                                let Some(p) = weak.upgrade() else { return };
                                if !feed.is_closed()
                                    && p.seek_simple(gst::SeekFlags::FLUSH, gst::ClockTime::ZERO)
                                        .is_err()
                                {
                                    feed.fail("the bed could not loop");
                                }
                            });
                        } else {
                            eos_feed.finish();
                        }
                    })
                    .build(),
            );
        }
        elements.push(sink.upcast());
        pipeline.add_many(&elements)?;
        gst::Element::link_many(&elements)?;
        {
            let feed = feed.clone();
            pipeline
                .bus()
                .expect("a pipeline has a bus")
                .set_sync_handler(move |_, msg| {
                    if let gst::MessageView::Error(e) = msg.view() {
                        feed.fail(&e.error().to_string());
                    }
                    gst::BusSyncReply::Drop
                });
        }
        let dec = Decoder { pipeline, feed };
        if let Err(e) = dec.pipeline.set_state(gst::State::Playing) {
            dec.feed.fail(&e.to_string());
        }
        let feed = dec.feed.clone();
        Ok((dec, feed))
    }

    /// Let go and stop the pipeline; waits for its thread.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.feed.close();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Where decoders go to be stopped, off the mixing pass: stopping waits for a thread.
pub fn reaper() -> mpsc::Sender<Decoder> {
    let (tx, rx) = mpsc::channel::<Decoder>();
    let spawned = std::thread::Builder::new()
        .name("desk-cart-stop".into())
        .spawn(move || {
            for d in rx {
                d.stop();
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "no thread to stop cart decoders on");
    }
    tx
}

struct Slot {
    cart: String,
    mode: CartMode,
    cue: bool,
    play: u64,
    duration_ms: Option<u64>,
    feed: Arc<CartFeed>,
    player: CartPlayer,
    decoder: Option<Decoder>,
}

/// The players and what they sound this block.
pub struct Deck {
    channels: usize,
    shots: Vec<Option<Slot>>,
    beds: Vec<Option<Slot>>,
    plays: u64,
    seq: u64,
    events: VecDeque<PlayEvent>,
    air: Vec<f32>,
    cue: Vec<f32>,
    has_air: bool,
    has_cue: bool,
    ducked: bool,
    mic_open: bool,
    started_ms: u64,
    reaper: Option<mpsc::Sender<Decoder>>,
}

impl Deck {
    /// `reaper`: where finished decoders are stopped (none: they are stopped where they end,
    /// for tests without a pipeline).
    pub fn new(channels: usize, reaper: Option<mpsc::Sender<Decoder>>) -> Self {
        Self {
            channels,
            shots: (0..SHOT_PLAYERS).map(|_| None).collect(),
            beds: (0..BED_PLAYERS).map(|_| None).collect(),
            plays: 0,
            seq: 0,
            events: VecDeque::new(),
            air: Vec::new(),
            cue: Vec::new(),
            has_air: false,
            has_cue: false,
            ducked: false,
            mic_open: false,
            started_ms: epoch_ms(),
            reaper,
        }
    }

    fn pool(&mut self, pool: Pool) -> &mut Vec<Option<Slot>> {
        match pool {
            Pool::Shot => &mut self.shots,
            Pool::Bed => &mut self.beds,
        }
    }

    /// The first free player of a pool (0-based).
    pub fn free(&mut self, pool: Pool) -> Option<usize> {
        self.pool(pool).iter().position(Option::is_none)
    }

    fn event(&mut self, mut e: PlayEvent) {
        self.seq += 1;
        e.seq = self.seq;
        if self.events.len() >= EVENTS_KEPT {
            self.events.pop_front();
        }
        self.events.push_back(e);
    }

    /// Put a fired cart in player `index` of its pool (from [`Deck::free`]).
    pub fn insert(
        &mut self,
        fire: &Fire,
        index: usize,
        feed: Arc<CartFeed>,
        decoder: Option<Decoder>,
    ) -> Fired {
        let pool = fire.mode.pool();
        self.plays += 1;
        let play = self.plays;
        let slot = Slot {
            cart: fire.cart.clone(),
            mode: fire.mode,
            cue: fire.cue,
            play,
            duration_ms: fire.duration_ms.filter(|_| fire.mode != CartMode::Bed),
            feed,
            player: CartPlayer::new(self.channels, fire.gain_db),
            decoder,
        };
        self.pool(pool)[index] = Some(slot);
        let player = index as u8 + 1;
        tracing::info!(cart = %fire.cart, play, pool = pool.word(), player, cue = fire.cue, mode = ?fire.mode, "cart fired");
        self.event(PlayEvent {
            seq: 0,
            play,
            event: PlayEventKind::Fired,
            cart: fire.cart.clone(),
            mode: fire.mode,
            pool,
            player,
            cue: fire.cue,
            at_ms: epoch_ms(),
            played_ms: None,
            end: None,
            error: None,
        });
        Fired { pool, player, play }
    }

    fn slots_mut(&mut self) -> impl Iterator<Item = &mut Slot> {
        self.shots.iter_mut().chain(self.beds.iter_mut()).flatten()
    }

    fn slots(&self) -> impl Iterator<Item = (Pool, usize, &Slot)> {
        let shots = self
            .shots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (Pool::Shot, i, s)));
        let beds = self
            .beds
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (Pool::Bed, i, s)));
        shots.chain(beds)
    }

    /// Fade out every player sounding `cart`. How many.
    pub fn stop(&mut self, cart: &str) -> usize {
        let mut n = 0;
        for s in self.slots_mut().filter(|s| s.cart == cart) {
            s.player.stop(STOP_FADE_MS);
            n += 1;
        }
        n
    }

    /// Fade out everything sounding (`cue`: only what is cued, or only what is on air).
    pub fn stop_all(&mut self, cue: Option<bool>) -> usize {
        let mut n = 0;
        for s in self
            .slots_mut()
            .filter(|s| cue.is_none_or(|c| s.cue == c) && !s.player.stopping())
        {
            s.player.stop(STOP_FADE_MS);
            n += 1;
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        self.slots().next().is_none()
    }

    /// Mix every player's next block of `frames`: what is on air into one sum, what is cued
    /// into another. `mic_open`: a microphone is open on air, so beds on air dip.
    pub fn render(&mut self, frames: usize, mic_open: bool) {
        let n = frames * self.channels;
        self.mic_open = mic_open;
        self.has_air = false;
        self.has_cue = false;
        let sweeping = self
            .slots()
            .any(|(_, _, s)| s.mode == CartMode::Sweeper && !s.cue && !s.player.stopping());
        self.ducked = mic_open || sweeping;
        if self.is_empty() {
            return;
        }
        self.air.resize(n, 0.0);
        self.air.fill(0.0);
        self.cue.resize(n, 0.0);
        self.cue.fill(0.0);
        let dip = db_to_lin(DUCK_DB);
        let mut ended = Vec::new();
        for pool in [Pool::Shot, Pool::Bed] {
            let ducked = self.ducked;
            let slots = match pool {
                Pool::Shot => &mut self.shots,
                Pool::Bed => &mut self.beds,
            };
            for (i, slot) in slots.iter_mut().enumerate() {
                let Some(s) = slot else { continue };
                let duck = if s.mode == CartMode::Bed && !s.cue && ducked {
                    dip
                } else {
                    1.0
                };
                let block = s.player.render(&s.feed, frames, duck);
                let (sum, flag) = if s.cue {
                    (&mut self.cue, &mut self.has_cue)
                } else {
                    (&mut self.air, &mut self.has_air)
                };
                for (a, b) in sum.iter_mut().zip(block) {
                    *a += b;
                }
                *flag = true;
                if s.player.ended() {
                    ended.push((pool, i, slot.take().expect("sounding")));
                }
            }
        }
        for (pool, i, mut s) in ended {
            let failed = s.feed.failed();
            let end = if failed.is_some() {
                EndKind::Failed
            } else if s.player.stopping() {
                EndKind::Stopped
            } else {
                EndKind::RanOut
            };
            let played_ms = s.player.played() * 1000 / u64::from(RATE);
            tracing::info!(cart = %s.cart, play = s.play, pool = pool.word(), player = i + 1, played_ms, end = ?end, error = ?failed, "cart ended");
            self.event(PlayEvent {
                seq: 0,
                play: s.play,
                event: PlayEventKind::Ended,
                cart: s.cart.clone(),
                mode: s.mode,
                pool,
                player: i as u8 + 1,
                cue: s.cue,
                at_ms: epoch_ms(),
                played_ms: Some(played_ms),
                end: Some(end),
                error: failed,
            });
            if let Some(d) = s.decoder.take() {
                match &self.reaper {
                    Some(tx) => {
                        if let Err(mpsc::SendError(d)) = tx.send(d) {
                            // No stopping thread: stop it on one of its own.
                            let _ = std::thread::Builder::new()
                                .name("desk-cart-stop".into())
                                .spawn(move || d.stop());
                        }
                    }
                    None => d.stop(),
                }
            }
        }
    }

    /// This block's carts on air, if any sounded.
    pub fn air(&self) -> Option<&[f32]> {
        self.has_air.then_some(&self.air[..])
    }

    /// This block's cued carts, if any sounded.
    pub fn cue(&self) -> Option<&[f32]> {
        self.has_cue.then_some(&self.cue[..])
    }

    pub fn status(&self) -> CartsStatus {
        let sounding = self
            .slots()
            .map(|(pool, i, s)| {
                let elapsed_ms = s.player.played() * 1000 / u64::from(RATE);
                SoundingStatus {
                    cart: s.cart.clone(),
                    pool,
                    player: i as u8 + 1,
                    mode: s.mode,
                    cue: s.cue,
                    play: s.play,
                    elapsed_ms,
                    remaining_ms: s.duration_ms.map(|d| d.saturating_sub(elapsed_ms)),
                    looping: s.mode == CartMode::Bed,
                    stopping: s.player.stopping(),
                    ducked: s.player.duck() < 0.99,
                }
            })
            .collect();
        CartsStatus {
            ducked: self.ducked,
            mic_open: self.mic_open,
            sounding,
            play_seq: self.seq,
            started_ms: self.started_ms,
        }
    }

    /// The play events after `after`, oldest first.
    pub fn plays_after(&self, after: u64) -> Vec<PlayEvent> {
        self.events
            .iter()
            .filter(|e| e.seq > after)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b5c3a52-1f7e-4a8b-9a51-0d2b6a4e9c11";

    fn fire(mode: CartMode, cue: bool) -> Fire {
        Fire {
            cart: ID.into(),
            file: format!("{ID}.flac"),
            gain_db: 0.0,
            mode,
            cue,
            duration_ms: Some(1_000),
        }
    }

    fn tone(secs: f32, amp: f32) -> Arc<CartFeed> {
        let f = Arc::new(CartFeed::new(2, Duration::from_secs(30)));
        let n = (secs * RATE as f32) as usize;
        let s: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = amp * (i as f32 * 0.1).sin();
                [v, v]
            })
            .collect();
        assert!(f.push(&s));
        f.finish();
        f
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn a_fire_is_checked_before_any_file_is_opened() {
        assert_eq!(fire(CartMode::OneShot, false).check(), Ok(()));
        let mut f = fire(CartMode::OneShot, false);
        f.file = "../../etc/passwd".into();
        assert_eq!(f.check(), Err(CartError::BadFile));
        f.file = format!("{ID}.wav");
        assert_eq!(f.check(), Err(CartError::BadFile));
        f = fire(CartMode::OneShot, false);
        f.cart = "x".into();
        assert_eq!(f.check(), Err(CartError::BadCart));
        f = fire(CartMode::OneShot, false);
        f.gain_db = 30.0;
        assert_eq!(f.check(), Err(CartError::BadGain));
        assert!(!is_uuid(&ID.to_uppercase()));
    }

    #[test]
    fn eight_one_shots_and_two_beds_then_busy() {
        let mut d = Deck::new(2, None);
        for _ in 0..SHOT_PLAYERS {
            let i = d.free(Pool::Shot).unwrap();
            d.insert(&fire(CartMode::OneShot, false), i, tone(1.0, 0.1), None);
        }
        assert_eq!(d.free(Pool::Shot), None);
        for _ in 0..BED_PLAYERS {
            let i = d.free(Pool::Bed).unwrap();
            d.insert(&fire(CartMode::Bed, false), i, tone(1.0, 0.1), None);
        }
        assert_eq!(d.free(Pool::Bed), None);
        assert_eq!(d.status().sounding.len(), 10);
    }

    #[test]
    fn air_and_cue_are_summed_apart_and_an_ended_cart_is_reported() {
        let mut d = Deck::new(2, None);
        let i = d.free(Pool::Shot).unwrap();
        let fired = d.insert(&fire(CartMode::OneShot, false), i, tone(0.03, 0.5), None);
        assert_eq!((fired.pool, fired.player, fired.play), (Pool::Shot, 1, 1));
        let j = d.free(Pool::Shot).unwrap();
        d.insert(&fire(CartMode::Sweeper, true), j, tone(1.0, 0.25), None);
        d.render(960, false);
        assert!((peak(d.air().unwrap()) - 0.5).abs() < 0.01);
        assert!((peak(d.cue().unwrap()) - 0.25).abs() < 0.01);
        assert!(!d.ducked, "a cued sweeper ducks nothing");
        d.render(960, false);
        assert_eq!(d.status().sounding.len(), 1, "the 30 ms one-shot ended");
        let plays = d.plays_after(0);
        assert_eq!(plays.len(), 3);
        assert_eq!(plays[2].event, PlayEventKind::Ended);
        assert_eq!(plays[2].end, Some(EndKind::RanOut));
        assert_eq!(plays[2].played_ms, Some(30));
        assert_eq!(d.plays_after(2).len(), 1);
        assert_eq!(d.status().play_seq, 3);
    }

    #[test]
    fn beds_on_air_dip_under_an_open_microphone_or_a_sweeper() {
        let mut d = Deck::new(2, None);
        let i = d.free(Pool::Bed).unwrap();
        d.insert(&fire(CartMode::Bed, false), i, tone(20.0, 0.5), None);
        d.render(960, false);
        let open = peak(d.air().unwrap());
        for _ in 0..100 {
            d.render(960, true);
        }
        assert!(d.status().ducked && d.status().sounding[0].ducked);
        let dipped = peak(d.air().unwrap());
        assert!(
            (weavesignal_mixer::lin_to_db(dipped / open) - DUCK_DB).abs() < 0.3,
            "{dipped} against {open}"
        );
        for _ in 0..100 {
            d.render(960, false);
        }
        assert!(!d.status().ducked);
        let k = d.free(Pool::Shot).unwrap();
        d.insert(&fire(CartMode::Sweeper, false), k, tone(20.0, 0.0), None);
        d.render(960, false);
        assert!(d.status().ducked, "a sweeper on air dips the beds");
    }

    #[test]
    fn stopping_fades_and_ends_and_says_so() {
        let mut d = Deck::new(2, None);
        let i = d.free(Pool::Shot).unwrap();
        d.insert(&fire(CartMode::OneShot, true), i, tone(10.0, 0.5), None);
        let j = d.free(Pool::Shot).unwrap();
        d.insert(&fire(CartMode::OneShot, false), j, tone(10.0, 0.5), None);
        assert_eq!(d.stop_all(Some(true)), 1, "Esc: only the cue");
        for _ in 0..16 {
            d.render(960, false);
        }
        let s = d.status();
        assert_eq!(s.sounding.len(), 1);
        assert!(!s.sounding[0].cue);
        assert_eq!(d.stop(ID), 1);
        assert!(d.status().sounding[0].stopping);
        for _ in 0..16 {
            d.render(960, false);
        }
        assert!(d.is_empty());
        let ends: Vec<_> = d.plays_after(0).into_iter().filter_map(|e| e.end).collect();
        assert_eq!(ends, vec![EndKind::Stopped, EndKind::Stopped]);
    }
}
