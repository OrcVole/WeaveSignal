// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The cart wall in the control plane (ADR-0020): the station's banks and carts, kept in
//! PostgreSQL and in memory; firing, cueing and stopping them at the desk; what is sounding,
//! from the desk's status; the play events the desk reports, for the per-play log; and
//! importing files through the media worker, the demo pack among them.
//!
//! Carts sound in the desk, as sources of its mixer (ADR-0117): the presenter and guests
//! hear them in their returns, `⇧` and a cart's key cues it into the presenters' ears, they
//! sit inside the profanity delay, and beds dip under an open microphone there, worked out
//! by the desk's mixing pass. They no longer go through playout.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::carts::{self, Bank, Cart, CartMode};

use crate::config::CartsConfig;
use crate::desk::{DeskCarts, DeskError, PlayEvent, Pool};
use crate::hub::{Hub, now_ms};
use crate::media::{self, WorkerError};

/// A cart fired in the last second may not be in the desk's status yet.
const FIRE_GRACE_MS: u64 = 1_000;
/// A one-shot whose end the desk puts this far from ours is corrected.
const DRIFT_MS: u64 = 500;
/// Play events kept in memory for the studio and the per-play log.
const PLAYS_KEPT: usize = 512;

/// What the cart wall needs beyond the hub (whose desk link fires the carts): the settings
/// and the database.
pub struct CartsLink {
    pub config: CartsConfig,
    pub db: Option<(sqlx::PgPool, StationId)>,
    /// Every play event the desk reports, as it is recorded (the per-play log listens here).
    pub plays: tokio::sync::broadcast::Sender<PlayEvent>,
}

impl CartsLink {
    pub fn new(config: CartsConfig, db: Option<(sqlx::PgPool, StationId)>) -> Self {
        Self {
            config,
            db,
            plays: tokio::sync::broadcast::channel(PLAYS_KEPT).0,
        }
    }

    /// No database: the wall shows itself as not set up.
    pub fn none() -> Self {
        Self::new(CartsConfig::default(), None)
    }
}

/// One cart sounding, as the control plane keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Sounding {
    pub cart: Uuid,
    pub pool: Pool,
    pub player: u8,
    pub mode: CartMode,
    /// Cued into the presenters' ears, not on air.
    pub cue: bool,
    /// The desk's number for this play.
    pub play: u64,
    /// When this play started and when it ends, in the station's epoch ms; a bed has no end.
    pub started_ms: u64,
    pub ends_ms: Option<u64>,
    /// Fading out (300 ms).
    pub stopping: bool,
    /// Higher is later: `Space` stops the highest.
    pub seq: u64,
}

/// The cart wall's state.
#[derive(Clone, Debug, Default)]
pub struct CartWall {
    pub banks: Vec<Bank>,
    pub carts: Vec<Cart>,
    /// Carts whose file is not on the disk: FILE MISSING.
    pub missing: BTreeSet<Uuid>,
    /// Bumped whenever banks or carts change, so the app fetches them again.
    pub version: u64,
    pub sounding: Vec<Sounding>,
    seq: u64,
    /// The desk answered the last time it was asked; else why not.
    pub desk_ok: bool,
    pub desk_error: Option<String>,
    /// Beds on air are dipped, as the desk last said.
    pub ducked: bool,
    /// A microphone is open on air at the desk.
    pub mic_open: bool,
    /// A pack is loading: carts done and to do.
    pub importing: Option<(usize, usize)>,
    /// The play events recorded, newest last, and the desk's number for the last one read.
    pub plays: VecDeque<PlayEvent>,
    pub play_seq: u64,
    /// When the desk whose events these are started.
    pub desk_started_ms: u64,
}

/// The cart wall in the studio's view (every change is an event); the banks and carts
/// themselves come from `GET /api/v1/carts` when `version` changes.
#[derive(Clone, Debug, Default, Serialize, PartialEq, utoipa::ToSchema)]
pub struct CartsView {
    pub version: u64,
    /// The desk, which plays the carts, answers; else why not.
    pub desk: bool,
    pub desk_error: Option<String>,
    pub sounding: Vec<Sounding>,
    pub ducked: bool,
    pub mic_open: bool,
    pub missing: Vec<Uuid>,
    /// Loading a pack: `[done, total]`.
    pub importing: Option<(usize, usize)>,
}

/// `GET /api/v1/carts`.
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct Catalogue {
    pub version: u64,
    pub banks: Vec<Bank>,
    pub carts: Vec<CatalogueCart>,
    /// The loudness every cart is brought to, in LUFS.
    pub loudness_lufs: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct CatalogueCart {
    #[serde(flatten)]
    pub cart: Cart,
    pub missing: bool,
}

/// What the studio app asks of the wall (`POST /api/v1/carts/commands`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CartCommand {
    /// A cart's key: fire it, or fade it out if it is sounding (keymap section 4).
    Toggle {
        cart: Uuid,
    },
    Fire {
        cart: Uuid,
    },
    Stop {
        cart: Uuid,
    },
    /// `Space`: fade out the cart started last.
    StopLast,
    /// `⇧Space ⇧Space`: fade out everything sounding.
    FadeAll,
    /// `⇧` and a cart's key: cue it into the presenters' ears, or take it out of the cue.
    Cue {
        cart: Uuid,
    },
    /// `Esc`: take every cued cart out of the cue.
    ClearCue,
}

#[derive(Debug, thiserror::Error)]
pub enum CartProblem {
    #[error("no cart `{0}`")]
    NoCart(Uuid),
    #[error("FILE MISSING: “{0}” has no file. Re-link it in the library.")]
    Missing(String),
    #[error("“{0}” is already sounding")]
    AlreadySounding(String),
    #[error("“{0}” is on air; it cannot be cued as well")]
    OnAir(String),
    #[error("nothing is sounding")]
    NothingSounding,
    #[error("every {0} player is sounding; stop one first")]
    Busy(&'static str),
    #[error(transparent)]
    Desk(#[from] DeskError),
    #[error("carts are kept in the database, and none is configured")]
    NoDatabase,
    #[error("{0}")]
    Invalid(String),
    #[error("the file was refused: {0}")]
    Refused(String),
    #[error("{0}")]
    Failed(String),
}

impl From<WorkerError> for CartProblem {
    fn from(e: WorkerError) -> Self {
        match e {
            WorkerError::Refused(m) => CartProblem::Refused(m),
            WorkerError::Failed(m) => CartProblem::Failed(m),
        }
    }
}

impl From<sqlx::Error> for CartProblem {
    fn from(e: sqlx::Error) -> Self {
        CartProblem::Failed(format!("the database: {e}"))
    }
}

impl From<carts::CartError> for CartProblem {
    fn from(e: carts::CartError) -> Self {
        CartProblem::Invalid(e.to_string())
    }
}

impl CartWall {
    /// A wall that says why the desk cannot be reached, before it is first asked.
    pub fn with_desk_error(error: Option<String>) -> Self {
        Self {
            desk_error: error,
            ..Self::default()
        }
    }

    pub fn view(&self) -> CartsView {
        CartsView {
            version: self.version,
            desk: self.desk_ok,
            desk_error: self.desk_error.clone(),
            sounding: self.sounding.clone(),
            ducked: self.ducked,
            mic_open: self.mic_open,
            missing: self.missing.iter().copied().collect(),
            importing: self.importing,
        }
    }

    pub fn catalogue(&self, loudness_lufs: f64) -> Catalogue {
        Catalogue {
            version: self.version,
            banks: self.banks.clone(),
            carts: self
                .carts
                .iter()
                .map(|c| CatalogueCart {
                    cart: c.clone(),
                    missing: self.missing.contains(&c.id),
                })
                .collect(),
            loudness_lufs,
        }
    }

    /// Take a fresh set of banks and carts (from the database), and check their files.
    pub fn replace(&mut self, banks: Vec<Bank>, carts: Vec<Cart>, dir: &Path) {
        self.banks = banks;
        self.carts = carts;
        self.missing.clear();
        self.refresh_missing(dir);
        self.version += 1;
    }

    /// Look at the disk again: a file that went is FILE MISSING; one that came back is not.
    /// True when anything changed.
    pub fn refresh_missing(&mut self, dir: &Path) -> bool {
        let now: BTreeSet<Uuid> = self
            .carts
            .iter()
            .filter(|c| match &c.file {
                None => true,
                Some(f) => !dir.join(f).is_file(),
            })
            .map(|c| c.id)
            .collect();
        if now == self.missing {
            return false;
        }
        self.missing = now;
        true
    }

    /// Sounding on air (not cued, not fading).
    fn on_air_now(&self, cart: Uuid) -> bool {
        self.sounding
            .iter()
            .any(|s| s.cart == cart && !s.cue && !s.stopping)
    }

    /// Sounding in the cue (not fading).
    fn cued_now(&self, cart: Uuid) -> bool {
        self.sounding
            .iter()
            .any(|s| s.cart == cart && s.cue && !s.stopping)
    }

    /// Fold in what the desk says is sounding.
    pub fn take_status(&mut self, st: &DeskCarts, now: u64) {
        let mut next = Vec::new();
        for p in &st.sounding {
            let Ok(cart) = Uuid::parse_str(&p.cart) else {
                continue;
            };
            let s = match self.sounding.iter().find(|s| s.play == p.play) {
                Some(m) => {
                    let mut s = m.clone();
                    s.stopping = s.stopping || p.stopping;
                    if let Some(remaining) = p.remaining_ms.filter(|_| !p.looping) {
                        let ends = now + remaining;
                        if s.ends_ms.is_none_or(|e| e.abs_diff(ends) > DRIFT_MS) {
                            s.ends_ms = Some(ends);
                        }
                    }
                    if p.looping {
                        s.ends_ms = None;
                    }
                    s
                }
                None => {
                    self.seq += 1;
                    Sounding {
                        cart,
                        pool: p.pool,
                        player: p.player,
                        mode: p.mode,
                        cue: p.cue,
                        play: p.play,
                        started_ms: now.saturating_sub(p.elapsed_ms),
                        ends_ms: p.remaining_ms.filter(|_| !p.looping).map(|r| now + r),
                        stopping: p.stopping,
                        seq: self.seq,
                    }
                }
            };
            next.push(s);
        }
        // Keep a cart fired a moment ago that the desk has not reported yet.
        for s in &self.sounding {
            let reported = next.iter().any(|n| n.play == s.play);
            if !reported && now.saturating_sub(s.started_ms) < FIRE_GRACE_MS && !s.stopping {
                next.push(s.clone());
            }
        }
        next.sort_by_key(|s| s.seq);
        self.sounding = next;
        self.ducked = st.ducked;
        self.mic_open = st.mic_open;
    }

    /// Follow the desk's event numbers: a desk that started again (its start time changed, or
    /// its numbers went back) numbers from 1 again. Whether there are events to read.
    pub fn follow_plays(&mut self, c: &DeskCarts) -> bool {
        if c.started_ms != self.desk_started_ms || c.play_seq < self.play_seq {
            if self.desk_started_ms != 0 {
                tracing::info!("the desk started again; its play events are numbered afresh");
            }
            self.desk_started_ms = c.started_ms;
            self.play_seq = 0;
        }
        c.play_seq > self.play_seq
    }

    /// Record a play event from the desk; `false` if it was already recorded.
    pub fn record(&mut self, e: &PlayEvent) -> bool {
        if e.seq <= self.play_seq {
            return false;
        }
        self.play_seq = e.seq;
        if self.plays.len() >= PLAYS_KEPT {
            self.plays.pop_front();
        }
        self.plays.push_back(e.clone());
        true
    }
}

/// A demo or station pack: `carts.json` beside its files (`assets/carts-demo`).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct Pack {
    banks: std::collections::BTreeMap<String, String>,
    carts: Vec<PackCart>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct PackCart {
    id: String,
    label: String,
    file: String,
    category: String,
    suggested: PackSlot,
    #[serde(rename = "loop")]
    looping: bool,
    licence: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct PackSlot {
    bank: u8,
    slot: u8,
}

impl PackCart {
    /// A loop is a bed; a whoosh is a sweeper; the rest play once.
    fn mode(&self) -> CartMode {
        if self.looping {
            CartMode::Bed
        } else if self.category == "whoosh" {
            CartMode::Sweeper
        } else {
            CartMode::OneShot
        }
    }
}

/// What an upload says about its cart.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct CartMeta {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    pub mode: CartMode,
    #[serde(default)]
    pub ramp_ms: Option<u32>,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub licence: String,
}

impl Hub {
    fn wall<R>(&self, f: impl FnOnce(&mut CartWall) -> R) -> R {
        self.with_inner(|inner| f(&mut inner.carts))
    }

    pub fn carts_view(&self) -> CartsView {
        self.with_inner(|i| i.carts.view())
    }

    pub fn catalogue(&self) -> Catalogue {
        let lufs = self.carts.config.loudness_lufs;
        self.with_inner(|i| i.carts.catalogue(lufs))
    }

    /// Read the banks and carts again from the database.
    pub async fn reload_carts(&self) -> Result<(), CartProblem> {
        let Some((pool, station)) = &self.carts.db else {
            return Err(CartProblem::NoDatabase);
        };
        let (banks, list) = carts::load(pool, *station).await?;
        let dir = self.carts.config.media_dir.clone();
        self.wall(|w| w.replace(banks, list, &dir));
        Ok(())
    }

    pub async fn cart_command(&self, cmd: &CartCommand) -> Result<(), CartProblem> {
        match cmd {
            CartCommand::Toggle { cart } => {
                if self.with_inner(|i| i.carts.on_air_now(*cart)) {
                    self.stop_cart(*cart).await
                } else {
                    self.fire(*cart, false).await
                }
            }
            CartCommand::Fire { cart } => self.fire(*cart, false).await,
            CartCommand::Stop { cart } => self.stop_cart(*cart).await,
            CartCommand::Cue { cart } => {
                if self.with_inner(|i| i.carts.on_air_now(*cart)) {
                    let label = self.with_inner(|i| {
                        i.carts
                            .carts
                            .iter()
                            .find(|c| c.id == *cart)
                            .map(|c| c.label.clone())
                            .unwrap_or_default()
                    });
                    Err(CartProblem::OnAir(label))
                } else if self.with_inner(|i| i.carts.cued_now(*cart)) {
                    self.stop_cart(*cart).await
                } else {
                    self.fire(*cart, true).await
                }
            }
            CartCommand::ClearCue => {
                self.desk.cart_stop_all(Some(true)).await?;
                self.wall(|w| {
                    w.sounding
                        .iter_mut()
                        .filter(|s| s.cue)
                        .for_each(|s| s.stopping = true)
                });
                Ok(())
            }
            CartCommand::StopLast => {
                // The last cart started on air: a cue is not what `Space` means.
                let last = self.with_inner(|i| {
                    i.carts
                        .sounding
                        .iter()
                        .filter(|s| !s.stopping && !s.cue)
                        .max_by_key(|s| s.seq)
                        .map(|s| s.cart)
                });
                match last {
                    Some(c) => self.stop_cart(c).await,
                    None => Err(CartProblem::NothingSounding),
                }
            }
            CartCommand::FadeAll => {
                self.desk.cart_stop_all(None).await?;
                self.wall(|w| w.sounding.iter_mut().for_each(|s| s.stopping = true));
                Ok(())
            }
        }
    }

    /// Fire a cart at the desk, on air or (`cue`) into the presenters' ears. Firing a cart
    /// that is cued takes it out of the cue and puts it on air from its start.
    async fn fire(&self, id: Uuid, cue: bool) -> Result<(), CartProblem> {
        let dir = self.carts.config.media_dir.clone();
        let cart = self.with_inner(|i| i.carts.carts.iter().find(|c| c.id == id).cloned());
        let cart = cart.ok_or(CartProblem::NoCart(id))?;
        if self.with_inner(|i| i.carts.on_air_now(id)) {
            return Err(if cue {
                CartProblem::OnAir(cart.label)
            } else {
                CartProblem::AlreadySounding(cart.label)
            });
        }
        let file = match &cart.file {
            Some(f) if dir.join(f).is_file() => f.clone(),
            _ => {
                self.wall(|w| {
                    if w.missing.insert(id) {
                        tracing::warn!(cart = %id, label = %cart.label, "a cart's file is missing");
                    }
                });
                return Err(CartProblem::Missing(cart.label));
            }
        };
        if !cue && self.with_inner(|i| i.carts.cued_now(id)) {
            self.desk.cart_stop(id).await?;
            self.wall(|w| {
                w.sounding
                    .iter_mut()
                    .filter(|s| s.cart == id && s.cue)
                    .for_each(|s| s.stopping = true)
            });
        }
        let fired = self
            .desk
            .cart_fire(id, &file, cart.gain_db, cart.mode, cue, cart.duration_ms)
            .await;
        let fired = match fired {
            Ok(f) => f,
            Err(DeskError::Rejected { code, message }) => {
                return Err(match code.as_str() {
                    "missing" => {
                        self.wall(|w| w.missing.insert(id));
                        CartProblem::Missing(cart.label)
                    }
                    "busy" => CartProblem::Busy(if cart.mode == CartMode::Bed {
                        "bed"
                    } else {
                        "one-shot"
                    }),
                    _ => CartProblem::Desk(DeskError::Rejected { code, message }),
                });
            }
            Err(e) => {
                if matches!(e, DeskError::Unreachable { .. }) {
                    let text = e.to_string();
                    self.wall(|w| {
                        w.desk_ok = false;
                        w.desk_error = Some(text);
                    });
                }
                return Err(e.into());
            }
        };
        let now = now_ms();
        tracing::info!(cart = %id, label = %cart.label, player = fired.player, play = fired.play, cue, "cart fired");
        self.wall(|w| {
            w.seq += 1;
            let seq = w.seq;
            w.sounding
                .retain(|s| !(s.pool == fired.pool && s.player == fired.player));
            w.sounding.push(Sounding {
                cart: id,
                pool: fired.pool,
                player: fired.player,
                mode: cart.mode,
                cue,
                play: fired.play,
                started_ms: now,
                ends_ms: (cart.mode != CartMode::Bed).then_some(now + u64::from(cart.duration_ms)),
                stopping: false,
                seq,
            });
        });
        Ok(())
    }

    async fn stop_cart(&self, id: Uuid) -> Result<(), CartProblem> {
        let n = self.desk.cart_stop(id).await?;
        self.wall(|w| {
            w.sounding
                .iter_mut()
                .filter(|s| s.cart == id)
                .for_each(|s| s.stopping = true);
            if n == 0 {
                w.sounding.retain(|s| s.cart != id);
            }
        });
        Ok(())
    }

    /// Fold the carts of a desk status in; the play events after the last one read are
    /// fetched separately ([`Hub::read_plays`]). Returns whether there are new ones.
    pub(crate) fn take_carts(&self, carts: Option<&DeskCarts>) -> bool {
        self.wall(|w| {
            w.desk_ok = true;
            w.desk_error = None;
            let Some(c) = carts else {
                w.sounding.clear();
                return false;
            };
            w.take_status(c, now_ms());
            w.follow_plays(c)
        })
    }

    /// The desk is not answering: nothing is known to sound.
    pub(crate) fn carts_lost(&self, error: &str) {
        self.wall(|w| {
            w.desk_ok = false;
            w.desk_error = Some(error.to_owned());
            w.sounding.clear();
            w.mic_open = false;
            w.ducked = false;
        });
    }

    /// Read and record the desk's play events after the last one recorded.
    pub(crate) async fn read_plays(&self) {
        let after = self.with_inner(|i| i.carts.play_seq);
        match self.desk.cart_plays(after).await {
            Ok(events) => {
                for e in events {
                    if self.wall(|w| w.record(&e)) {
                        tracing::info!(
                            target: "weavesignal_control::plays",
                            play = e.play, event = %e.event, cart = %e.cart, mode = e.mode.as_str(),
                            cue = e.cue, at_ms = e.at_ms, played_ms = ?e.played_ms, end = ?e.end,
                            "cart play"
                        );
                        let _ = self.carts.plays.send(e);
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "cannot read the desk's play events"),
        }
    }

    /// The play events recorded, oldest first (the last 512).
    pub fn cart_plays(&self) -> Vec<PlayEvent> {
        self.read_inner(|i| i.carts.plays.iter().cloned().collect())
    }

    /// Every play event from now on, as it is recorded: for the per-play log.
    pub fn subscribe_plays(&self) -> tokio::sync::broadcast::Receiver<PlayEvent> {
        self.carts.plays.subscribe()
    }

    /// Look at the disk now: a cart whose file went is FILE MISSING; one that came back is not.
    pub fn check_cart_files(&self) {
        let dir = self.carts.config.media_dir.clone();
        self.wall(|w| {
            w.refresh_missing(&dir);
        });
    }

    /// Look at the disk for missing files every 5 s, until the process ends.
    pub fn spawn_file_watch(self: &Arc<Self>) {
        let hub = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                hub.check_cart_files();
            }
        });
    }

    fn db(&self) -> Result<(&sqlx::PgPool, StationId), CartProblem> {
        self.carts
            .db
            .as_ref()
            .map(|(p, s)| (p, *s))
            .ok_or(CartProblem::NoDatabase)
    }

    /// Make or rename a bank.
    pub async fn put_bank(&self, bank: Bank) -> Result<(), CartProblem> {
        bank.check()?;
        let (pool, station) = self.db()?;
        carts::put_bank(pool, station, &bank).await?;
        self.reload_carts().await
    }

    /// Run an upload through the media worker and put it in a slot.
    pub async fn import_cart(
        &self,
        bank: u8,
        slot: u8,
        meta: CartMeta,
        bytes: Vec<u8>,
    ) -> Result<Cart, CartProblem> {
        let (pool, station) = self.db()?;
        let mut cart = Cart {
            id: Uuid::new_v4(),
            bank,
            slot,
            label: meta.label.trim().to_owned(),
            detail: meta.detail.trim().to_owned(),
            mode: meta.mode,
            file: None,
            duration_ms: 0,
            gain_db: 0.0,
            loudness_lufs: None,
            true_peak_dbtp: None,
            ramp_ms: meta.ramp_ms,
            origin: meta.origin.chars().take(300).collect(),
            licence: meta.licence.chars().take(80).collect(),
        };
        // Check before the worker runs, so a bad slot costs nothing.
        cart.check()?;
        if !self.with_inner(|i| i.carts.banks.iter().any(|b| b.number == bank)) {
            return Err(CartProblem::Invalid(format!(
                "bank {bank} has not been made; make it first"
            )));
        }
        let (flac, report) = media::normalise(&self.carts.config, bytes).await?;
        let dir = &self.carts.config.media_dir;
        let name = media::store(dir, &flac)
            .await
            .map_err(|e| CartProblem::Failed(format!("cannot write the cart file: {e}")))?;
        cart.file = Some(name.clone());
        cart.duration_ms = u32::try_from(report.duration_ms).unwrap_or(u32::MAX);
        cart.gain_db = report.play_gain_db as f32;
        cart.loudness_lufs = Some(report.integrated_lufs as f32);
        cart.true_peak_dbtp = Some(report.true_peak_dbtp as f32);
        match carts::put_cart(pool, station, &cart).await {
            Ok(old) => {
                if let Some(old) = old
                    && carts::is_cart_file(&old)
                {
                    let _ = tokio::fs::remove_file(dir.join(old)).await;
                }
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(dir.join(&name)).await;
                return Err(e.into());
            }
        }
        tracing::info!(
            cart = %cart.id, bank, slot, label = %cart.label, format = %report.format,
            lufs = report.integrated_lufs, gain_db = report.play_gain_db, "cart imported"
        );
        self.reload_carts().await?;
        Ok(cart)
    }

    /// Empty a slot (its file goes too).
    pub async fn delete_cart(&self, bank: u8, slot: u8) -> Result<(), CartProblem> {
        let (pool, station) = self.db()?;
        let file = self.with_inner(|i| {
            i.carts
                .carts
                .iter()
                .find(|c| c.bank == bank && c.slot == slot)
                .and_then(|c| c.file.clone())
        });
        let Some(id) = carts::delete_cart(pool, station, bank, slot).await? else {
            return Err(CartProblem::Invalid(format!(
                "bank {bank} slot {slot} is empty"
            )));
        };
        let _ = self.desk.cart_stop(id).await;
        if let Some(f) = file.filter(|f| carts::is_cart_file(f)) {
            let _ = tokio::fs::remove_file(self.carts.config.media_dir.join(f)).await;
        }
        self.reload_carts().await
    }

    /// Load a pack (`carts.json` and its files) into the station's banks: each file goes
    /// through the media worker as an upload would. Returns how many carts it loaded.
    pub async fn load_pack(&self, dir: &Path) -> Result<usize, CartProblem> {
        self.db()?;
        let text = tokio::fs::read_to_string(dir.join("carts.json"))
            .await
            .map_err(|e| CartProblem::Invalid(format!("cannot read the pack's carts.json: {e}")))?;
        let pack: Pack = serde_json::from_str(&text)
            .map_err(|e| CartProblem::Invalid(format!("the pack's carts.json: {e}")))?;
        for (number, name) in &pack.banks {
            let number: u8 = number
                .parse()
                .map_err(|_| CartProblem::Invalid(format!("bank `{number}` is not a number")))?;
            self.put_bank(Bank {
                number,
                name: name.clone(),
                // Banks take palette slots in order (ADR-0066).
                colour_slot: number,
            })
            .await?;
        }
        let total = pack.carts.len();
        self.wall(|w| w.importing = Some((0, total)));
        self.publish_now();
        let mut done = 0;
        let mut failed = Vec::new();
        for c in &pack.carts {
            let path: PathBuf = dir.join(&c.file);
            // The pack names files beside it; nothing reaches outside it.
            if c.file.contains('/') || c.file.starts_with('.') {
                failed.push(format!("{}: not a file beside carts.json", c.id));
                continue;
            }
            let meta = CartMeta {
                label: c.label.clone(),
                detail: String::new(),
                mode: c.mode(),
                ramp_ms: None,
                origin: format!("demo:{}", c.id),
                licence: c.licence.clone(),
            };
            let result = match tokio::fs::read(&path).await {
                Ok(bytes) => self
                    .import_cart(c.suggested.bank, c.suggested.slot, meta, bytes)
                    .await
                    .map(|_| ()),
                Err(e) => Err(CartProblem::Invalid(format!("cannot read {}: {e}", c.file))),
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => {
                    tracing::warn!(cart = %c.id, error = %e, "a pack cart was not loaded");
                    failed.push(format!("{}: {e}", c.id));
                }
            }
            self.wall(|w| w.importing = Some((done + failed.len(), total)));
            self.publish_now();
        }
        self.wall(|w| w.importing = None);
        self.publish_now();
        tracing::info!(loaded = done, failed = failed.len(), pack = %dir.display(), "cart pack loaded");
        if done == 0 && !failed.is_empty() {
            return Err(CartProblem::Failed(failed.join("; ")));
        }
        Ok(done)
    }

    /// At start: read the carts, and load the demo pack if the station has no banks yet.
    pub async fn start_carts(self: &Arc<Self>) {
        if self.carts.db.is_none() {
            return;
        }
        if let Err(e) = tokio::fs::create_dir_all(&self.carts.config.media_dir).await {
            tracing::warn!(dir = %self.carts.config.media_dir.display(), error = %e, "cannot make the cart directory");
        }
        if let Err(e) = self.reload_carts().await {
            tracing::warn!(error = %e, "cannot read the carts");
            return;
        }
        self.spawn_file_watch();
        let empty = self.with_inner(|i| i.carts.banks.is_empty());
        if let (true, Some(pack)) = (empty, self.carts.config.demo_pack.clone()) {
            let hub = self.clone();
            tokio::spawn(async move {
                if let Err(e) = hub.load_pack(&pack).await {
                    tracing::warn!(pack = %pack.display(), error = %e, "the demo pack was not loaded");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::DeskSounding;

    fn cart(bank: u8, slot: u8, mode: CartMode, file: Option<String>) -> Cart {
        Cart {
            id: Uuid::new_v4(),
            bank,
            slot,
            label: format!("Cart {bank}.{slot}"),
            detail: String::new(),
            mode,
            file,
            duration_ms: 4_000,
            gain_db: -6.0,
            loudness_lufs: Some(-17.0),
            true_peak_dbtp: Some(-2.0),
            ramp_ms: None,
            origin: String::new(),
            licence: String::new(),
        }
    }

    #[test]
    fn a_cart_without_its_file_is_missing() {
        let dir = crate::desk::tests::tempdir();
        let present = format!("{}.flac", Uuid::new_v4());
        std::fs::write(dir.join(&present), b"fLaC").unwrap();
        let a = cart(1, 1, CartMode::OneShot, Some(present.clone()));
        let b = cart(
            1,
            2,
            CartMode::OneShot,
            Some(format!("{}.flac", Uuid::new_v4())),
        );
        let c = cart(1, 3, CartMode::OneShot, None);
        let mut w = CartWall::default();
        w.replace(vec![], vec![a.clone(), b.clone(), c.clone()], &dir);
        assert_eq!(w.missing, BTreeSet::from([b.id, c.id]));
        let cat = w.catalogue(-23.0);
        assert!(!cat.carts[0].missing && cat.carts[1].missing);
        std::fs::remove_file(dir.join(&present)).unwrap();
        assert!(w.refresh_missing(&dir));
        assert!(w.missing.contains(&a.id));
        assert!(!w.refresh_missing(&dir), "no change the second time");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_desks_status_is_folded_in() {
        let bed = cart(1, 1, CartMode::Bed, None);
        let sweep = cart(1, 2, CartMode::Sweeper, None);
        let mut w = CartWall {
            carts: vec![bed.clone(), sweep.clone()],
            ..CartWall::default()
        };
        let sounding = |cart: &Cart, pool, player, play, remaining: Option<u64>| DeskSounding {
            cart: cart.id.to_string(),
            pool,
            player,
            mode: cart.mode,
            cue: false,
            play,
            elapsed_ms: if pool == Pool::Bed { 2_000 } else { 1_000 },
            remaining_ms: remaining,
            looping: pool == Pool::Bed,
            stopping: false,
            ducked: false,
        };
        let st = |remaining: u64| DeskCarts {
            ducked: true,
            mic_open: false,
            sounding: vec![
                sounding(&bed, Pool::Bed, 1, 1, None),
                sounding(&sweep, Pool::Shot, 3, 2, Some(remaining)),
            ],
            play_seq: 2,
            started_ms: 1,
        };
        w.take_status(&st(4_000), 100_000);
        assert_eq!(w.sounding.len(), 2);
        assert_eq!(w.sounding[0].ends_ms, None, "a bed loops");
        assert_eq!(w.sounding[0].started_ms, 98_000);
        assert_eq!(w.sounding[1].ends_ms, Some(104_000));
        assert!(w.ducked, "the desk says the beds are dipped");
        // Small drift is not news; a big one is corrected.
        w.take_status(&st(3_600), 100_200);
        assert_eq!(w.sounding[1].ends_ms, Some(104_000));
        w.take_status(&st(1_000), 100_300);
        assert_eq!(w.sounding[1].ends_ms, Some(101_300));
        // Gone from the desk, gone from the wall.
        let mut only_bed = st(0);
        only_bed.sounding.truncate(1);
        only_bed.ducked = false;
        w.take_status(&only_bed, 110_000);
        assert_eq!(w.sounding.len(), 1);
        assert!(!w.ducked);
    }

    #[test]
    fn each_play_event_is_recorded_once_and_a_restarted_desk_counts_again() {
        let mut w = CartWall::default();
        let e = |seq: u64| PlayEvent {
            seq,
            play: seq,
            event: "fired".into(),
            cart: Uuid::new_v4().to_string(),
            mode: CartMode::OneShot,
            pool: Pool::Shot,
            player: 1,
            cue: false,
            at_ms: 0,
            played_ms: None,
            end: None,
            error: None,
        };
        assert!(w.record(&e(1)) && w.record(&e(2)));
        assert!(!w.record(&e(2)));
        assert_eq!(w.plays.len(), 2);
        for n in 3..600 {
            w.record(&e(n));
        }
        assert_eq!(w.plays.len(), PLAYS_KEPT);
        assert_eq!(w.play_seq, 599);
        // A desk that started again, already past the old count: its events are read afresh.
        let restarted = DeskCarts {
            play_seq: 700,
            started_ms: 2,
            ..DeskCarts::default()
        };
        w.desk_started_ms = 1;
        assert!(w.follow_plays(&restarted));
        assert_eq!(w.play_seq, 0);
        assert!(w.record(&e(1)), "event 1 of the new desk is new");
        let same = DeskCarts {
            play_seq: 1,
            started_ms: 2,
            ..DeskCarts::default()
        };
        assert!(!w.follow_plays(&same));
    }

    #[test]
    fn the_demo_pack_reads_and_its_modes_follow_its_words() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/carts-demo");
        let pack: Pack =
            serde_json::from_str(&std::fs::read_to_string(format!("{dir}/carts.json")).unwrap())
                .unwrap();
        assert_eq!(pack.carts.len(), 50);
        assert_eq!(pack.banks.len(), 4);
        let mode = |id: &str| pack.carts.iter().find(|c| c.id == id).unwrap().mode();
        assert_eq!(mode("bed-1"), CartMode::Bed);
        assert_eq!(mode("whoosh-1"), CartMode::Sweeper);
        assert_eq!(mode("applause-1"), CartMode::OneShot);
        for c in &pack.carts {
            assert!(
                std::path::Path::new(dir).join(&c.file).is_file(),
                "{}",
                c.file
            );
            assert!((1..=24).contains(&c.suggested.slot) && (1..=5).contains(&c.suggested.bank));
        }
    }
}
