// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! On air (M4): the control plane's side of ADR-0044's playout rows.
//!
//! - **Automation.** The scheduler's log (`schedule.rs`) is generated a day ahead and fed to
//!   playout's automation queue a few items ahead (`autodj.push`); the station's mode (`live`,
//!   the desk with automation on dead air, or `auto`) is held in playout.
//! - **The per-play log.** Playout reports each track, cart and fallback file as it starts
//!   (`events.since`); each becomes a `play_log` row with its words and the listeners connected.
//!   The wall's carts sound in the desk (ADR-0117): its play events are logged too, timed when
//!   listeners hear them, and marked as not aired when a dump threw them away.
//! - **Dead air.** Playout says when the desk went quiet while live and automation took over; it
//!   becomes a notice on the desk and a warning in the log.
//! - **Audience.** The listener process's `/status.json` (anonymous: a connection number and a
//!   duration) becomes listener seconds and sessions of at least a minute per hour (ADR-0046,
//!   ADR-0115).
//! - **Retention** (ADRs 0113-0116, the `retention` module). The legal logger records while the
//!   station's Jurisdiction keeps programme recordings (No retention, the default, keeps none);
//!   each hour's file is registered as a recording, so the retention sweeper deletes it at the
//!   end of its period, holds keep it, and callers' consent links to it. The per-play log is
//!   swept by its own period here, holds respected. Listener totals are anonymous and kept
//!   (ADR-0115); no access log with addresses is kept at all. A US statutory webcaster is one
//!   whose retention setting says so (or a US webcaster preset).
//! - **Now playing.** Pushed to listeners from here (`/api/v1/now-playing` and its events), not
//!   in the stream, from which metadata is dropped; what comes next is never shown for a US
//!   statutory webcaster.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::watch;
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::playout::{self as rows, LogItem, Play, PlayoutSettings};

use crate::desk::DeskCommand;
use crate::hub::{Hub, now_ms};
use crate::playout::PlayoutClient;
use crate::schedule::{self, Generator, HOUR_MS, Played, Rules, Slot};
use crate::studio::Tone;

/// How many log items playout holds ahead.
const QUEUE_AHEAD: usize = 2;
/// The log is generated this far ahead, and topped up when less than `LOG_LOW` remains.
const LOG_AHEAD_MS: i64 = 26 * HOUR_MS;
const LOG_LOW_MS: i64 = 6 * HOUR_MS;
/// An item that ended this long ago without playing is skipped: a live show ran over it.
const SKIP_GRACE_MS: i64 = 60_000;
/// A session counts once it has lasted this long (ADR-0046).
pub const SESSION_MIN_S: f64 = 60.0;

/// What listeners are told is playing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, utoipa::ToSchema)]
pub struct NowPlaying {
    /// `desk` (a live show), `automation`, `fallback`, `silence`, or `unknown` while playout is
    /// not answering. The default loop WeaveSignal ships (ADR-0130) is told as `fallback`: to a
    /// listener it is the station's fallback, and only the studio is told it is the default.
    pub on_air: String,
    pub now: Option<Current>,
    /// What comes next; left out for a US statutory webcaster (17 U.S.C. 114(d)(2)(C)(ii)).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<NextUp>>,
    pub up_next_shown: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Current {
    /// `track`, `cart`, `fallback` or `live`.
    pub kind: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub started_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct NextUp {
    pub title: String,
    pub artist: String,
}

/// Listener seconds and sessions of at least a minute, from repeated looks at who is
/// connected. Nothing about a listener is kept but their connection's number and how long it
/// has been credited, and only while it is connected.
#[derive(Debug, Default)]
pub struct Audience {
    seen: HashMap<u64, (f64, bool)>,
    carry: f64,
}

/// One look's additions to an hour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listening {
    pub hour_ms: i64,
    pub seconds: i64,
    pub sessions: i32,
    pub connected: i32,
}

impl Audience {
    /// `listeners`: each connection's number and how long it has lasted, in seconds.
    pub fn observe(&mut self, now_ms: i64, listeners: &[(u64, f64)]) -> Listening {
        let mut seconds = 0.0;
        let mut sessions = 0;
        let mut next = HashMap::with_capacity(listeners.len());
        for &(id, lasted) in listeners {
            let (mut credited, mut counted) = self.seen.get(&id).copied().unwrap_or((0.0, false));
            if lasted + 1.0 < credited {
                // A new connection with an old number (the listener process restarted).
                credited = 0.0;
                counted = false;
            }
            if lasted >= SESSION_MIN_S {
                if !counted {
                    counted = true;
                    sessions += 1;
                }
                seconds += lasted - credited;
                credited = lasted;
            }
            next.insert(id, (credited, counted));
        }
        self.seen = next;
        let total = seconds + self.carry;
        let whole = total.floor();
        self.carry = total - whole;
        Listening {
            hour_ms: now_ms - now_ms.rem_euclid(HOUR_MS),
            seconds: whole as i64,
            sessions,
            connected: listeners.len() as i32,
        }
    }
}

/// One event from playout (`events.since`).
#[derive(Clone, Debug, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Event {
    pub seq: u64,
    pub at: f64,
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
struct Events {
    boot: String,
    events: Vec<Event>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct ProgrammeStatus {
    pub boot: String,
    pub mode: String,
    /// `desk`, `automation`, `fallback` (the station's files), `default` (the shipped loop,
    /// ADR-0130) or `silence`.
    pub on_air: String,
    pub dead_air: bool,
    pub desk_connected: bool,
    pub desk_sounding: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct AutodjStatus {
    pub current: String,
    pub queued: Vec<String>,
    pub remaining: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct LoggerStatus {
    pub on: bool,
    #[serde(default)]
    pub dir: String,
}

/// What retention decides for the playout layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Retained {
    /// The logger records: programme recordings are kept for some time.
    pub logger: bool,
    /// Under the US statutory licence: the performance complement, and no up-next.
    pub us_statutory: bool,
    /// How long the per-play log is kept after a play, in days; `None` without limit.
    pub per_play_days: Option<u32>,
}

impl Retained {
    pub fn of(s: &weavesignal_model::retention::Setting) -> Self {
        use weavesignal_model::retention::Period;
        let p = s.periods();
        let days = |x: Period| match x {
            Period::NoLimit => None,
            other => Some(other.floor_days(p.programme_recordings)),
        };
        Self {
            logger: !matches!(
                p.programme_recordings,
                Period::Days { days: 0 } | Period::WithCall
            ),
            us_statutory: s
                .us_statutory_licence
                .unwrap_or_else(|| s.rules().key.starts_with("us_")),
            per_play_days: days(p.per_play_log),
        }
    }
}

/// The playout layer's state, for the studio app (`GET /api/v1/playout`).
#[derive(Clone, Debug, Default, Serialize, utoipa::ToSchema)]
pub struct Status {
    pub retained: Retained,
    pub playout: Option<ProgrammeStatus>,
    pub playout_error: Option<String>,
    pub automation: Option<AutodjStatus>,
    pub listeners: u32,
    /// Listeners hear the shipped default loop, not the station's own fallback files
    /// (ADR-0130), whether playout or the listener process is playing it.
    pub default_fallback: bool,
    pub logger: Option<LoggerStatus>,
    pub logger_error: Option<String>,
    pub settings: PlayoutSettings,
    /// What the last generation could not fill, in words.
    pub gaps: Vec<String>,
    pub now_playing: NowPlaying,
}

#[derive(Default)]
struct State {
    boot: String,
    seq: u64,
    status: Status,
    audience: Audience,
    current: Option<Current>,
    /// When automation last generated the log and made nothing (no clock, no tracks): it tries
    /// again at most once a minute, not every pass.
    empty_generate_ms: Option<i64>,
}

/// Where the playout layer's parts are.
pub struct Links {
    pub playout: PlayoutClient,
    pub listeners: PlayoutClient,
    /// The listener process's `/status.json` on loopback.
    pub status_url: String,
    pub library_dir: PathBuf,
    pub logger_dir: PathBuf,
    /// Now playing changes this long after playout says, when listeners hear it (ADR-0103).
    pub listener_delay: Duration,
    /// The library's loudness target (LUFS) and longest track (seconds), for imports.
    pub library_lufs: f64,
    pub library_max_seconds: f64,
}

/// Held while the log is generated (see `OnAir::generation_lock`); dropping it releases both locks.
struct GenerationLock<'a> {
    _local: tokio::sync::MutexGuard<'a, ()>,
    _tx: sqlx::Transaction<'static, sqlx::Postgres>,
}

pub struct OnAir {
    pub station: StationId,
    pub db: PgPool,
    pub links: Links,
    hub: Weak<Hub>,
    state: Mutex<State>,
    now_tx: watch::Sender<Arc<NowPlaying>>,
    /// One generation at a time in this process, taken before the database's lock so that at
    /// most one waiter holds a pool connection for it.
    generating: tokio::sync::Mutex<()>,
}

impl OnAir {
    pub fn new(station: StationId, db: PgPool, links: Links, hub: &Arc<Hub>) -> Arc<Self> {
        let (now_tx, _) = watch::channel(Arc::new(NowPlaying {
            on_air: "unknown".into(),
            ..NowPlaying::default()
        }));
        Arc::new(Self {
            station,
            db,
            links,
            hub: Arc::downgrade(hub),
            state: Mutex::new(State::default()),
            now_tx,
            generating: tokio::sync::Mutex::new(()),
        })
    }

    /// Start the loops, and hang this on the hub (for the cart wall and the routes).
    pub fn spawn(self: &Arc<Self>, hub: &Arc<Hub>) {
        let _ = hub.onair.set(self.clone());
        tokio::spawn(self.clone().log_desk_carts());
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                me.poll_events().await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = me.automate().await {
                    tracing::warn!(error = %e, "automation");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        let me = self.clone();
        tokio::spawn(async move { me.feed_slate().await });
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                me.count_listeners().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = me.keep_logger().await {
                    tracing::warn!(error = %e, "legal logger");
                    me.state.lock().unwrap().status.logger_error = Some(e.to_string());
                }
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        });
    }

    /// The video slate's words (ADR-0010): the desk is told what playout is playing, as playout
    /// says it, not after the listener delay: the video's sound is playout's own (ADR-0172),
    /// and the picture has the desk's delay to go through besides. A desk with video off
    /// refuses the words, which costs one cheap request every 15 s.
    async fn feed_slate(self: Arc<Self>) {
        let mut sent: Option<(DeskCommand, std::time::Instant)> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let Some(hub) = self.hub.upgrade() else {
                return;
            };
            if !hub.desk.configured() {
                return;
            }
            let (station, show) = {
                let v = hub.view();
                (v.station.clone(), v.show.title.clone())
            };
            let current = self.state.lock().unwrap().current.clone();
            let words = slate_words(&station, &show, current.as_ref());
            let due = match &sent {
                Some((w, at)) => *w != words || at.elapsed() >= SLATE_REPEAT,
                None => true,
            };
            if !due {
                continue;
            }
            // Whether it went or not, ask again in 15 s at the soonest unless the words change.
            match hub.desk.send(&words).await {
                Ok(()) => {}
                Err(e) => tracing::debug!(error = %e, "the slate's words did not reach the desk"),
            }
            sent = Some((words, std::time::Instant::now()));
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<NowPlaying>> {
        self.now_tx.subscribe()
    }

    pub fn now_playing(&self) -> Arc<NowPlaying> {
        self.now_tx.borrow().clone()
    }

    /// Tell listeners this is playing now, with no playout and no listener delay: for tests of
    /// what follows now-playing (the public stream, the events out).
    #[doc(hidden)]
    pub fn announce(&self, np: NowPlaying) {
        self.now_tx.send_if_modified(|v| {
            if **v != np {
                *v = Arc::new(np);
                true
            } else {
                false
            }
        });
    }

    pub fn status(&self) -> Status {
        let mut s = self.state.lock().unwrap().status.clone();
        s.now_playing = (*self.now_playing()).clone();
        s
    }

    fn notice(&self, tone: Tone, kind: &str, title: &str, message: String) {
        if let Some(hub) = self.hub.upgrade() {
            hub.with_studio(|s, now| s.playout_notice(now, tone, kind, title.to_owned(), message));
        }
    }

    // ---- The per-play log ----

    /// The wall's carts sound in the desk (ADR-0117): its play events become rows of the
    /// per-play log, timed when listeners hear them (the desk's time plus the profanity delay).
    /// A dump throws away what the delay held, so a cart fired in the dumped stretch is marked
    /// as not aired. Cued carts never reach listeners and are not logged.
    async fn log_desk_carts(self: Arc<Self>) {
        let Some(hub) = self.hub.upgrade() else {
            return;
        };
        let mut plays = hub.subscribe_plays();
        let mut dumps: Option<u64> = None;
        let mut open: HashMap<u64, (i64, i64)> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                ev = plays.recv() => {
                    let ev = match ev {
                        Ok(ev) => ev,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(missed = n, "the per-play log missed cart events");
                            continue;
                        }
                        Err(_) => return,
                    };
                    if ev.cue {
                        continue;
                    }
                    let delay = i64::from(
                        hub.desk_status().and_then(|d| d.delay).map(|d| d.delay_ms).unwrap_or(0),
                    );
                    if let Err(e) = self.log_desk_cart(&hub, &ev, delay, &mut open).await {
                        tracing::warn!(error = %e, "the per-play log could not keep a cart");
                    }
                }
                _ = tick.tick() => {
                    let Some(d) = hub.desk_status().and_then(|d| d.delay) else { continue };
                    let before = dumps.replace(d.dumps);
                    if let (Some(b), Some(ago)) = (before, d.dumped_ms_ago)
                        && d.dumps > b
                    {
                        let dump_at = now_ms() as i64 - ago as i64;
                        let held = i64::from(d.target_ms.max(d.delay_ms));
                        match rows::mark_not_aired(&self.db, self.station, dump_at, dump_at + held).await {
                            Ok(n) if n > 0 => tracing::info!(carts = n, "dumped carts marked as not aired"),
                            Ok(_) => {}
                            Err(e) => tracing::warn!(error = %e, "dumped carts were not marked"),
                        }
                    }
                }
            }
        }
    }

    async fn log_desk_cart(
        &self,
        hub: &Hub,
        ev: &crate::desk::PlayEvent,
        delay: i64,
        open: &mut HashMap<u64, (i64, i64)>,
    ) -> anyhow::Result<()> {
        let id: Uuid = ev.cart.parse()?;
        match ev.event.as_str() {
            "fired" => {
                let cart = hub.read_inner(|i| i.carts.carts.iter().find(|c| c.id == id).cloned());
                let listeners = self.state.lock().unwrap().status.listeners;
                let at = ev.at_ms as i64 + delay;
                let play = Play {
                    id: 0,
                    started_at_ms: at,
                    ended_at_ms: None,
                    kind: "cart".into(),
                    track: None,
                    cart: Some(id),
                    item: None,
                    title: cart
                        .as_ref()
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| "(a cart no longer on the wall)".into()),
                    artist: cart.as_ref().map(|c| c.detail.clone()).unwrap_or_default(),
                    album: String::new(),
                    label: cart.as_ref().map(|c| c.licence.clone()).unwrap_or_default(),
                    isrc: None,
                    duration_ms: cart.as_ref().map(|c| c.duration_ms).unwrap_or(0),
                    listeners,
                    aired: true,
                };
                let source = format!("desk:{}:{}", ev.at_ms, ev.play);
                if let Some(row) =
                    rows::record_play(&self.db, self.station, &play, Some(&source), false).await?
                {
                    open.insert(ev.play, (row, delay));
                }
            }
            "ended" => {
                if let Some((row, delay)) = open.remove(&ev.play) {
                    rows::end_play(
                        &self.db,
                        self.station,
                        row,
                        ev.at_ms as i64 + delay,
                        ev.played_ms,
                    )
                    .await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn poll_events(&self) {
        let since = self.state.lock().unwrap().seq;
        let mut answer = match self
            .links
            .playout
            .command(&format!("events.since {since}"))
            .await
        {
            Ok(a) => a,
            Err(_) => return,
        };
        let Ok(mut events) = serde_json::from_str::<Events>(&answer) else {
            tracing::warn!(%answer, "playout's events are not JSON");
            return;
        };
        let restarted = {
            let st = self.state.lock().unwrap();
            st.boot != events.boot
        };
        if restarted {
            // Playout started again: its queue and its counter are new.
            let first = self.state.lock().unwrap().boot.is_empty();
            {
                let mut st = self.state.lock().unwrap();
                st.boot = events.boot.clone();
                st.seq = 0;
            }
            if !first {
                tracing::warn!("playout started again");
                let _ = rows::end_open_plays(&self.db, self.station, now_ms() as i64).await;
            }
            let _ = sqlx::query(
                "UPDATE schedule_item SET status = 'planned' WHERE station_id = $1 AND status = 'queued'",
            )
            .bind(self.station.as_uuid())
            .execute(&self.db)
            .await;
            answer = match self.links.playout.command("events.since 0").await {
                Ok(a) => a,
                Err(_) => return,
            };
            events = match serde_json::from_str::<Events>(&answer) {
                Ok(e) => e,
                Err(_) => return,
            };
        }
        let boot = events.boot.clone();
        for ev in events.events {
            if let Err(e) = self.handle(&boot, &ev).await {
                tracing::warn!(error = %e, kind = %ev.kind, "a playout event was not recorded");
            }
            self.state.lock().unwrap().seq = ev.seq;
        }
    }

    async fn handle(&self, boot: &str, ev: &Event) -> anyhow::Result<()> {
        let at = (ev.at * 1000.0) as i64;
        let source = format!("{boot}:{}", ev.seq);
        let listeners = self.state.lock().unwrap().status.listeners;
        match ev.kind.as_str() {
            "track" | "cart" => {
                let item: Option<i64> = ev.item.parse().ok();
                let id: Uuid = ev.id.parse()?;
                if let Some(i) = item {
                    rows::update_item(&self.db, self.station, i, None, Some("played"), None)
                        .await?;
                }
                let mut play = Play {
                    id: 0,
                    started_at_ms: at,
                    ended_at_ms: None,
                    kind: ev.kind.clone(),
                    track: None,
                    cart: None,
                    item,
                    title: String::new(),
                    artist: String::new(),
                    album: String::new(),
                    label: String::new(),
                    isrc: None,
                    duration_ms: 0,
                    listeners,
                    aired: true,
                };
                if ev.kind == "track" {
                    let t = rows::track(&self.db, self.station, id).await?;
                    play.track = Some(id);
                    if let Some(t) = t {
                        play.title = t.title;
                        play.artist = t.artist;
                        play.album = t.album;
                        play.label = t.label;
                        play.isrc = t.isrc;
                        play.duration_ms = t.duration_ms;
                    } else {
                        play.title = "(a track no longer in the library)".into();
                    }
                } else {
                    play.cart = Some(id);
                    let cart = self.hub.upgrade().and_then(|h| {
                        h.with_inner(|i| i.carts.carts.iter().find(|c| c.id == id).cloned())
                    });
                    match cart {
                        Some(c) => {
                            play.title = c.label;
                            play.artist = c.detail;
                            play.label = c.licence;
                            play.duration_ms = c.duration_ms;
                        }
                        None => play.title = "(a cart no longer on the wall)".into(),
                    }
                }
                rows::record_play(&self.db, self.station, &play, Some(&source), true).await?;
                self.set_current(Some(Current {
                    kind: play.kind,
                    title: play.title,
                    artist: play.artist,
                    album: play.album,
                    started_at_ms: at,
                }))
                .await;
            }
            "fallback" => {
                let play = Play {
                    id: 0,
                    started_at_ms: at,
                    ended_at_ms: None,
                    kind: "fallback".into(),
                    track: None,
                    cart: None,
                    item: None,
                    title: ev.name.clone(),
                    artist: String::new(),
                    album: String::new(),
                    label: String::new(),
                    isrc: None,
                    duration_ms: 0,
                    listeners,
                    aired: true,
                };
                rows::record_play(&self.db, self.station, &play, Some(&source), true).await?;
                self.set_current(Some(Current {
                    kind: "fallback".into(),
                    title: ev.name.clone(),
                    artist: String::new(),
                    album: String::new(),
                    started_at_ms: at,
                }))
                .await;
            }
            "on_air" => {
                {
                    let mut st = self.state.lock().unwrap();
                    if let Some(p) = st.status.playout.as_mut() {
                        p.on_air = ev.name.clone();
                    }
                }
                // The default loop is not a play (it repeats every half minute and is not the
                // station's to report); like silence, it ends what was open and names nothing.
                let nameless = matches!(ev.name.as_str(), "silence" | "default");
                if ev.name == "desk" || nameless {
                    rows::end_open_plays(&self.db, self.station, at).await?;
                }
                let current = (ev.name == "desk").then(|| Current {
                    kind: "live".into(),
                    title: "Live".into(),
                    artist: String::new(),
                    album: String::new(),
                    started_at_ms: at,
                });
                if ev.name == "desk" || nameless {
                    self.set_current(current).await;
                } else {
                    self.publish().await;
                }
                self.set_default_fallback(ev.name == "default").await;
            }
            "dead_air" => {
                tracing::warn!(now = %ev.name, "dead air: the desk went quiet while live; {} took over", ev.name);
                self.notice(
                    Tone::Serious,
                    "playout.dead_air",
                    "Dead air",
                    format!(
                        "The desk was silent too long while live; {} is on air until it sounds again.",
                        ev.name
                    ),
                );
            }
            "dead_air_end" => {
                tracing::info!("the desk is back on air after dead air");
                self.notice(
                    Tone::Good,
                    "playout.dead_air_end",
                    "Back on air",
                    "The desk is on air again.".into(),
                );
            }
            _ => {}
        }
        Ok(())
    }

    async fn set_current(&self, current: Option<Current>) {
        self.state.lock().unwrap().current = current;
        self.publish().await;
    }

    /// Work out what listeners are told, and tell them when they hear it.
    async fn publish(&self) {
        let (on_air, current, us) = {
            let st = self.state.lock().unwrap();
            (
                st.status
                    .playout
                    .as_ref()
                    .map(|p| match p.on_air.as_str() {
                        // The shipped loop is the fallback to a listener (see `NowPlaying`).
                        "default" => "fallback".to_owned(),
                        other => other.to_owned(),
                    })
                    .unwrap_or_else(|| "unknown".into()),
                st.current.clone(),
                st.status.retained.us_statutory,
            )
        };
        let next = if us { None } else { Some(self.up_next().await) };
        let np = NowPlaying {
            on_air,
            now: current,
            up_next_shown: next.is_some(),
            next,
        };
        let tx = self.now_tx.clone();
        let delay = self.links.listener_delay;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            tx.send_if_modified(|v| {
                if **v != np {
                    *v = Arc::new(np);
                    true
                } else {
                    false
                }
            });
        });
    }

    /// The next two items of the log not yet played.
    async fn up_next(&self) -> Vec<NextUp> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT COALESCE(t.title, c.label, ''), COALESCE(t.artist, c.detail, '')
             FROM schedule_item i
             LEFT JOIN track t ON t.id = i.track_id
             LEFT JOIN cart c ON c.id = i.cart_id
             WHERE i.station_id = $1 AND i.status IN ('planned', 'queued')
             ORDER BY i.planned_at, i.id LIMIT 2",
        )
        .bind(self.station.as_uuid())
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();
        rows.into_iter()
            .map(|(title, artist)| NextUp { title, artist })
            .collect()
    }

    // ---- Automation ----

    async fn automate(&self) -> anyhow::Result<()> {
        let settings = rows::settings(&self.db, self.station).await?;
        // The station's mode, into the studio view and to the desk, whether playout answers or
        // not (ADR-0183, ADR-0172); the desk is told only when it says another.
        if let (Some(hub), Some(mode)) = (
            self.hub.upgrade(),
            crate::desk::StationMode::parse(&settings.mode),
        ) {
            hub.set_mode(mode).await;
        }
        let retained = self.retained().await?;
        let us_changed = {
            let mut st = self.state.lock().unwrap();
            let changed = st.status.retained.us_statutory != retained.us_statutory;
            st.status.settings = settings.clone();
            st.status.retained = retained;
            changed
        };
        if us_changed {
            self.publish().await;
        }
        let programme = match self.links.playout.command("programme.status").await {
            Ok(a) => serde_json::from_str::<ProgrammeStatus>(&a).ok(),
            Err(e) => {
                {
                    let mut st = self.state.lock().unwrap();
                    st.status.playout = None;
                    st.status.playout_error = Some(e.to_string());
                }
                // Playout is away: the listener process plays the fallback itself.
                let default = self.listener_process_default().await;
                self.set_default_fallback(default).await;
                return Ok(());
            }
        };
        if let Some(p) = &programme
            && p.mode != settings.mode
        {
            self.links
                .playout
                .command(&format!("programme.mode {}", settings.mode))
                .await?;
        }
        let default = programme.as_ref().is_some_and(|p| p.on_air == "default");
        {
            let mut st = self.state.lock().unwrap();
            st.status.playout = programme;
            st.status.playout_error = None;
        }
        self.set_default_fallback(default).await;
        self.top_up_log(now_ms() as i64).await?;
        self.feed().await
    }

    /// Whether the listener process, which plays the fallback when playout is away, is playing
    /// the shipped default loop (`fallback.status`); false when it does not answer.
    async fn listener_process_default(&self) -> bool {
        let Ok(a) = self.links.listeners.command("fallback.status").await else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(&a)
            .ok()
            .and_then(|v| v["default"].as_bool())
            .unwrap_or(false)
    }

    /// Tell the studio whether listeners hear the shipped default loop (ADR-0130): a flag the
    /// desk's playout panel shows for as long as it holds, and a notice when it changes, so no
    /// station goes on air with the default by mistake.
    async fn set_default_fallback(&self, on: bool) {
        {
            let mut st = self.state.lock().unwrap();
            if st.status.default_fallback == on {
                return;
            }
            st.status.default_fallback = on;
        }
        tracing::info!(on, "the default fallback loop");
        let Some(hub) = self.hub.upgrade() else {
            return;
        };
        hub.with_studio(|s, _| s.set_default_fallback(on));
        if on {
            self.notice(
                Tone::Warning,
                "playout.default_fallback",
                "Default loop on air",
                "Listeners hear the loop WeaveSignal ships: the station has no fallback files of its \
                 own yet. Add some to the fallback folder and they take over."
                    .into(),
            );
        } else {
            self.notice(
                Tone::Good,
                "playout.default_fallback_end",
                "Default loop off air",
                "Listeners no longer hear the default loop.".into(),
            );
        }
    }

    /// Extend the log when it runs low. The log's end is read under the generation lock, so a
    /// regeneration that finished first is seen, not generated over (ADR-0181).
    pub async fn top_up_log(&self, now: i64) -> anyhow::Result<()> {
        let resting = self
            .state
            .lock()
            .unwrap()
            .empty_generate_ms
            .is_some_and(|t| now - t < 60_000);
        if resting {
            return Ok(());
        }
        // Cheap look first, so a healthy log takes no lock.
        let low = |end: Option<i64>| end.is_none_or(|e| e < now + LOG_LOW_MS);
        if !low(rows::log_end(&self.db, self.station).await?) {
            return Ok(());
        }
        let _lock = self.generation_lock().await?;
        let end = rows::log_end(&self.db, self.station).await?;
        if low(end) {
            let from = end.unwrap_or(now).max(now);
            let (made, _) = self
                .generate_locked(from, now + LOG_AHEAD_MS, false)
                .await?;
            self.state.lock().unwrap().empty_generate_ms = (made == 0).then_some(now);
        }
        Ok(())
    }

    /// Take the station's generation lock: a PostgreSQL advisory lock held by a transaction of
    /// its own, which ends (and so releases the lock) when the guard is dropped, even if the
    /// task holding it is cancelled. The lock is the database's, so a second control plane over
    /// the same database waits too. The work itself uses the pool.
    async fn generation_lock(&self) -> anyhow::Result<GenerationLock<'_>> {
        let local = self.generating.lock().await;
        let mut tx = self.db.begin().await?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('weavesignal-log:' || $1::text, 0))",
        )
        .bind(self.station.as_uuid().to_string())
        .execute(&mut *tx)
        .await?;
        Ok(GenerationLock {
            _local: local,
            _tx: tx,
        })
    }

    /// Keep playout's queue a few items ahead.
    async fn feed(&self) -> anyhow::Result<()> {
        let a = self.links.playout.command("autodj.status").await?;
        let q: AutodjStatus = serde_json::from_str(&a)?;
        let want = QUEUE_AHEAD.saturating_sub(q.queued.len());
        self.state.lock().unwrap().status.automation = Some(q);
        if want == 0 {
            return Ok(());
        }
        let now = now_ms() as i64;
        let items =
            rows::next_to_queue(&self.db, self.station, now - SKIP_GRACE_MS, want as i64).await?;
        let mut pushed = false;
        for item in items {
            let target = match (item.kind.as_str(), item.track, item.cart) {
                ("track", Some(t), _) => rows::track(&self.db, self.station, t)
                    .await?
                    .map(|t| ("track", t.id, t.file, t.gain_db)),
                ("cart", _, Some(c)) => {
                    let r: Option<(Option<String>, f32)> = sqlx::query_as(
                        "SELECT file, gain_db FROM cart WHERE station_id = $1 AND id = $2",
                    )
                    .bind(self.station.as_uuid())
                    .bind(c)
                    .fetch_optional(&self.db)
                    .await?;
                    r.and_then(|(f, g)| f.map(|f| ("cart", c, f, g)))
                }
                _ => None,
            };
            let Some((kind, id, file, gain)) = target else {
                rows::update_item(
                    &self.db,
                    self.station,
                    item.id,
                    None,
                    Some("skipped"),
                    Some("its track or cart is gone"),
                )
                .await?;
                continue;
            };
            let answer = self
                .links
                .playout
                .command(&format!(
                    "autodj.push {} {kind} {id} {file} {gain:.2}",
                    item.id
                ))
                .await?;
            if answer.starts_with("ok") {
                rows::update_item(&self.db, self.station, item.id, None, Some("queued"), None)
                    .await?;
                pushed = true;
            } else {
                tracing::warn!(item = item.id, %answer, "playout refused a log item");
                rows::update_item(
                    &self.db,
                    self.station,
                    item.id,
                    None,
                    Some("skipped"),
                    Some(&answer.chars().take(200).collect::<String>()),
                )
                .await?;
            }
        }
        if pushed {
            self.publish().await;
        }
        Ok(())
    }

    /// Generate the log for `[from, to)`; `replace` first removes what is still only planned
    /// there. Returns how many items it made and the slots it left empty.
    ///
    /// Only one generation runs at a time for a station: a second waits for the first to finish
    /// (ADR-0181; review 0025, section 4).
    pub async fn generate(
        &self,
        from_ms: i64,
        to_ms: i64,
        replace: bool,
    ) -> anyhow::Result<(usize, Vec<String>)> {
        let _lock = self.generation_lock().await?;
        self.generate_locked(from_ms, to_ms, replace).await
    }

    /// `generate`, with the generation lock already held by the caller.
    async fn generate_locked(
        &self,
        from_ms: i64,
        to_ms: i64,
        replace: bool,
    ) -> anyhow::Result<(usize, Vec<String>)> {
        if replace {
            rows::delete_planned(&self.db, self.station, from_ms, to_ms).await?;
        }
        let settings = rows::settings(&self.db, self.station).await?;
        let tz = jiff::tz::TimeZone::get(&settings.time_zone)?;
        let rules = rules_of(&settings, self.retained().await?.us_statutory);
        let tracks: Vec<schedule::Track> = rows::tracks(&self.db, self.station)
            .await?
            .into_iter()
            .filter(|t| t.enabled)
            .map(|t| schedule::Track {
                id: t.id,
                title: t.title,
                artist: t.artist,
                album: t.album,
                category: t.category,
                duration_ms: t.duration_ms,
            })
            .collect();
        let clocks: HashMap<Uuid, schedule::Clock> = rows::clocks(&self.db, self.station)
            .await?
            .into_iter()
            .filter_map(|c| {
                let slots: Vec<Slot> = serde_json::from_str(&c.slots_json).ok()?;
                Some((
                    c.id,
                    schedule::Clock {
                        id: c.id,
                        name: c.name,
                        slots,
                    },
                ))
            })
            .collect();
        let mut week = schedule::Week {
            default_clock: settings.default_clock,
            ..schedule::Week::default()
        };
        for (d, h, c) in rows::week(&self.db, self.station).await? {
            week.hours.insert((d, h), c);
        }
        let carts: HashMap<Uuid, u32> = sqlx::query_as::<_, (Uuid, i32)>(
            "SELECT id, duration_ms FROM cart WHERE station_id = $1 AND file IS NOT NULL",
        )
        .bind(self.station.as_uuid())
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|(id, ms)| (id, ms.max(0) as u32))
        .collect();
        // Where the log before `from` ends, and what it played: the rules look back from there.
        let before = rows::items(&self.db, self.station, from_ms - 6 * HOUR_MS, from_ms).await?;
        let cursor = before
            .iter()
            .filter(|i| i.status != "skipped")
            .map(|i| i.planned_at_ms + i64::from(i.duration_ms))
            .max()
            .unwrap_or(from_ms)
            .max(from_ms);
        let horizon = rules.horizon_ms();
        let history: Vec<Played> =
            rows::planned_tracks(&self.db, self.station, cursor - horizon, cursor)
                .await?
                .into_iter()
                .map(|p| Played::new(p.track, &p.title, &p.artist, &p.album, p.at_ms))
                .collect();
        let last_used: HashMap<Uuid, i64> = rows::last_used(&self.db, self.station)
            .await?
            .into_iter()
            .collect();
        let mut ctx = schedule::Context {
            cursor_ms: cursor,
            history,
            last_used,
        };
        let g = Generator {
            tracks: &tracks,
            clocks: &clocks,
            carts: &carts,
            week: &week,
            rules,
            tz: tz.clone(),
        };
        let (start, _, _) = schedule::local_hour(&tz, cursor);
        let (made, gaps) = g.generate(start, to_ms, &mut ctx);
        let items: Vec<LogItem> = made
            .iter()
            .filter(|i| i.planned_at_ms >= from_ms)
            .map(|i| LogItem {
                id: 0,
                planned_at_ms: i.planned_at_ms,
                hour_ms: i.hour_ms,
                clock: Some(i.clock),
                slot: i.slot,
                kind: match i.kind {
                    schedule::ItemKind::Track(_) => "track".into(),
                    schedule::ItemKind::Cart(_) => "cart".into(),
                },
                track: match i.kind {
                    schedule::ItemKind::Track(t) => Some(t),
                    schedule::ItemKind::Cart(_) => None,
                },
                cart: match i.kind {
                    schedule::ItemKind::Cart(c) => Some(c),
                    schedule::ItemKind::Track(_) => None,
                },
                category: i.category.clone(),
                duration_ms: i.duration_ms,
                status: "planned".into(),
                note: i.note.clone(),
            })
            .collect();
        rows::insert_items(&self.db, self.station, &items).await?;
        let mut words: Vec<String> = gaps
            .iter()
            .take(20)
            .map(|g| {
                format!(
                    "{}: slot {} left empty: {}",
                    fmt_ms(g.at_ms),
                    g.slot + 1,
                    g.reason
                )
            })
            .collect();
        if gaps.len() > 20 {
            words.push(format!("and {} more", gaps.len() - 20));
        }
        {
            // Said once per change, not on every generation.
            let mut st = self.state.lock().unwrap();
            if !gaps.is_empty() && st.status.gaps != words {
                tracing::warn!(gaps = gaps.len(), first = %words[0], "the schedule left slots empty");
            }
            st.status.gaps = words.clone();
        }
        Ok((items.len(), words))
    }

    // ---- Audience ----

    /// Tell the studio's view how many are listening, when that has changed (the app's CDN
    /// warning, ADR-0017, and `desk.audience`).
    fn show_listeners(&self, n: Option<u32>) {
        if let Some(hub) = self.hub.upgrade() {
            hub.with_studio_if(|s| s.set_listeners(n));
        }
    }

    async fn count_listeners(&self) {
        let url = self.links.status_url.clone();
        let got = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<(u64, f64)>> {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(3)))
                .build()
                .into();
            let v: serde_json::Value = agent.get(&url).call()?.body_mut().read_json()?;
            Ok(listeners_of(&v))
        })
        .await;
        let Ok(Ok(listeners)) = got else {
            // Not counted: the studio's CDN warning must not read a stale figure (ADR-0017).
            self.show_listeners(None);
            return;
        };
        let l = {
            let mut st = self.state.lock().unwrap();
            st.status.listeners = listeners.len() as u32;
            st.audience.observe(now_ms() as i64, &listeners)
        };
        self.show_listeners(Some(listeners.len() as u32));
        if let Err(e) = rows::add_listening(
            &self.db,
            self.station,
            l.hour_ms,
            l.seconds,
            l.sessions,
            l.connected,
        )
        .await
        {
            tracing::warn!(error = %e, "the listener counts were not kept");
        }
    }

    // ---- Retention: the legal logger, the per-play log ----

    pub async fn retained(&self) -> anyhow::Result<Retained> {
        let s = weavesignal_model::retention::load_setting(&self.db, self.station).await?;
        Ok(Retained::of(&s))
    }

    async fn keep_logger(&self) -> anyhow::Result<()> {
        let want = self.retained().await?.logger;
        let st: LoggerStatus =
            serde_json::from_str(&self.links.listeners.command("logger.status").await?)?;
        let on = if st.on != want {
            let a = self
                .links
                .listeners
                .command(if want { "logger.on" } else { "logger.off" })
                .await?;
            anyhow::ensure!(a.starts_with("ok"), "the listener process refused: {a}");
            tracing::info!(on = want, "legal logger");
            want
        } else {
            st.on
        };
        {
            let mut s = self.state.lock().unwrap();
            s.status.logger = Some(LoggerStatus { on, dir: st.dir });
            s.status.logger_error = None;
        }
        self.register_recordings(on).await?;
        self.sweep_plays().await
    }

    /// Each hour's logger file becomes a recording for retention: open while it is written,
    /// ended when the next one starts, the hour is over, or the logger stops.
    async fn register_recordings(&self, on: bool) -> anyhow::Result<()> {
        use weavesignal_model::retention::{
            NewRecording, RecordingKind, add_recording, end_recording,
        };
        let dir = self.links.logger_dir.clone();
        let recs = tokio::task::spawn_blocking(move || recordings(&dir)).await?;
        let now = now_ms() as i64;
        for (i, r) in recs.iter().enumerate() {
            let file = format!("{}/{}", LOGGER_SUBDIR, r.name);
            let next = recs.get(i + 1).map(|n| n.start_ms);
            let ends = next
                .unwrap_or(r.start_ms + HOUR_MS)
                .min(r.start_ms + HOUR_MS);
            let done = next.is_some() || !on || now >= r.start_ms + HOUR_MS + 30_000;
            match rows::recording_by_file(&self.db, self.station, &file).await? {
                None => {
                    add_recording(
                        &self.db,
                        self.station,
                        &NewRecording {
                            kind: RecordingKind::Programme,
                            label: format!("Logger, the hour from {}", fmt_ms(r.start_ms)),
                            file: Some(file),
                            started_at_ms: r.start_ms as u64,
                            ended_at_ms: done.then_some(ends.min(now) as u64),
                        },
                    )
                    .await?;
                }
                Some((id, true)) if done => {
                    end_recording(&self.db, self.station, id, ends.min(now) as u64).await?;
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// The per-play log past its period goes (an hour after the play when it is 0 days).
    async fn sweep_plays(&self) -> anyhow::Result<()> {
        let Some(days) = self.retained().await?.per_play_days else {
            return Ok(());
        };
        let keep = if days == 0 {
            HOUR_MS
        } else {
            i64::from(days) * 24 * HOUR_MS
        };
        let n = rows::sweep_plays(&self.db, self.station, now_ms() as i64 - keep).await?;
        if n > 0 {
            tracing::info!(
                plays = n,
                days,
                "the per-play log past its retention period was deleted"
            );
        }
        Ok(())
    }
}

/// The logger's files sit in this directory of retention's recordings directory.
pub const LOGGER_SUBDIR: &str = "logger";

pub fn rules_of(s: &PlayoutSettings, us_statutory: bool) -> Rules {
    Rules {
        artist_separation_min: s.artist_separation_min,
        title_separation_min: s.title_separation_min,
        album_separation_min: s.album_separation_min,
        us_statutory,
    }
}

/// `/status.json` from the listener process: each listener's number and duration. A listener's
/// number counts from zero on each mount (the Opus mount and the MP3 compatibility mount, ADR-0002,
/// each have a listener 0), so the mount's name is folded into the number: the audience counts
/// them as the different people they are.
pub fn listeners_of(v: &serde_json::Value) -> Vec<(u64, f64)> {
    v["mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| {
            let key = mount_key(m["mount"].as_str().unwrap_or_default());
            m["listeners"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |l| Some((key | l["id"].as_u64()?, l["duration"].as_f64()?)))
        })
        .collect()
}

/// A mount's name as the high bits of a listener's number (FNV-1a, 24 bits, above bit 40): the
/// listener process numbers connections from zero on each mount and never gets near 2^40.
fn mount_key(mount: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in mount.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    (h & 0xFF_FFFF) << 40
}

pub fn fmt_ms(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_string())
        .unwrap_or_else(|_| ms.to_string())
}

/// A recording of the legal logger: the hour (UTC) it starts, its path and size.
#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Recording {
    pub start_ms: i64,
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
}

/// The logger's files, `<dir>/YYYY-MM-DD/HH-MM-SS.opus`, oldest first.
pub fn recordings(dir: &Path) -> Vec<Recording> {
    let mut out = Vec::new();
    let Ok(days) = std::fs::read_dir(dir) else {
        return out;
    };
    for day in days.flatten() {
        let dname = day.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in files.flatten() {
            let fname = f.file_name().to_string_lossy().into_owned();
            let Some(stem) = fname.strip_suffix(".opus") else {
                continue;
            };
            let text = format!("{dname}T{}Z", stem.replace('-', ":"));
            let Ok(ts) = text.parse::<jiff::Timestamp>() else {
                continue;
            };
            out.push(Recording {
                start_ms: ts.as_millisecond(),
                path: f.path(),
                name: format!("{dname}/{fname}"),
                bytes: f.metadata().map(|m| m.len()).unwrap_or(0),
            });
        }
    }
    out.sort_by_key(|r| r.start_ms);
    out
}

/// How often the desk is told the slate's words again when they have not changed: a desk that
/// restarted has forgotten them.
const SLATE_REPEAT: Duration = Duration::from_secs(15);

/// What the slate says (ADR-0010): the station, the show, and the track playing: not a cart or
/// a fallback loop (they come and go in seconds, or have no name worth showing), and not a live
/// show (the people are the picture).
fn slate_words(station: &str, show: &str, current: Option<&Current>) -> DeskCommand {
    let track = current.filter(|c| c.kind == "track" && !c.title.is_empty());
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    DeskCommand::NowPlaying {
        station: some(station),
        show: some(show),
        title: track.map(|c| c.title.clone()),
        artist: track.and_then(|c| some(&c.artist)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current(kind: &str, title: &str, artist: &str) -> Current {
        Current {
            kind: kind.into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            started_at_ms: 0,
        }
    }

    #[test]
    fn the_slate_names_a_playing_track_and_nothing_that_passes_or_is_live() {
        let words = |c: Option<&Current>| slate_words("Kettle FM", "The Morning Kettle", c);
        let np = |title: Option<&str>, artist: Option<&str>| DeskCommand::NowPlaying {
            station: Some("Kettle FM".into()),
            show: Some("The Morning Kettle".into()),
            title: title.map(Into::into),
            artist: artist.map(Into::into),
        };
        assert_eq!(
            words(Some(&current("track", "Skye Boat Song", "The Corries"))),
            np(Some("Skye Boat Song"), Some("The Corries"))
        );
        assert_eq!(
            words(Some(&current("track", "Untitled", ""))),
            np(Some("Untitled"), None)
        );
        // A cart, a fallback loop, a live show and nothing at all: the station and the show.
        for c in [
            Some(current("cart", "Station ident", "")),
            Some(current("fallback", "loop.opus", "")),
            Some(current("live", "Live", "")),
            Some(current("track", "", "Nobody")),
            None,
        ] {
            assert_eq!(words(c.as_ref()), np(None, None), "{c:?}");
        }
        // No show on the schedule: no show line.
        assert_eq!(
            slate_words("Kettle FM", "", None),
            DeskCommand::NowPlaying {
                station: Some("Kettle FM".into()),
                show: None,
                title: None,
                artist: None
            }
        );
    }

    #[test]
    fn sessions_count_after_a_minute_and_hours_add_up() {
        let mut a = Audience::default();
        let h = 10 * HOUR_MS;
        // Two listeners: one 30 s in, one 59 s in. Nothing counts yet.
        let l = a.observe(h, &[(1, 30.0), (2, 59.0)]);
        assert_eq!((l.seconds, l.sessions, l.connected), (0, 0, 2));
        // Five seconds later the second passes a minute: one session, all its 64 s.
        let l = a.observe(h + 5_000, &[(1, 35.0), (2, 64.0)]);
        assert_eq!((l.seconds, l.sessions), (64, 1));
        // Then only the time since.
        let l = a.observe(h + 10_000, &[(1, 40.0), (2, 69.5)]);
        assert_eq!((l.seconds, l.sessions), (5, 0));
        let l = a.observe(h + 15_000, &[(2, 75.0)]);
        assert_eq!((l.seconds, l.sessions, l.connected), (6, 0, 1));
        // Listener 1 left at 40 s: never a session, never any seconds. A new connection with an
        // old number starts again.
        let l = a.observe(h + 20_000, &[(2, 3.0)]);
        assert_eq!((l.seconds, l.sessions), (0, 0));
        assert_eq!(l.hour_ms, h);
    }

    #[test]
    fn the_status_page_gives_numbers_and_durations_only() {
        let v = serde_json::json!({"mounts": [{"mount": "/programme.opus",
            "listeners": [{"id": 3, "duration": 61.5}, {"id": 4, "duration": 2.0}]}]});
        let key = mount_key("/programme.opus");
        assert_eq!(listeners_of(&v), vec![(key | 3, 61.5), (key | 4, 2.0)]);
        assert!(listeners_of(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn listeners_with_the_same_number_on_two_mounts_are_two_listeners() {
        // The Opus mount and the MP3 compatibility mount each number from zero.
        let v = serde_json::json!({"mounts": [
            {"mount": "/programme.mp3", "listeners": [{"id": 0, "duration": 70.0}]},
            {"mount": "/programme.opus", "listeners": [{"id": 0, "duration": 90.0}]}]});
        let l = listeners_of(&v);
        assert_eq!(l.len(), 2);
        assert_ne!(l[0].0, l[1].0);
        let mut a = Audience::default();
        let h = a.observe(1_000_000, &l);
        assert_eq!((h.sessions, h.connected), (2, 2));
        assert_eq!(h.seconds, 160);
    }

    #[test]
    fn logger_files_are_found_by_their_hour() {
        let dir = crate::desk::tests::tempdir();
        for (d, f) in [
            ("2026-10-25", "23-00-00.opus"),
            ("2026-10-01", "10-00-00.opus"),
            ("2026-10-26", "notes.txt"),
        ] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
            std::fs::write(dir.join(d).join(f), b"x").unwrap();
        }
        let r = recordings(&dir);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "2026-10-01/10-00-00.opus");
        assert_eq!(
            r[1].start_ms,
            "2026-10-25T23:00:00Z"
                .parse::<jiff::Timestamp>()
                .unwrap()
                .as_millisecond()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_decides_the_logger_and_the_us_rule() {
        use weavesignal_model::retention::Setting;
        let none = Retained::of(&Setting::default());
        assert!(!none.logger, "No retention records nothing");
        assert!(!none.us_statutory);
        let uk = Retained::of(&Setting {
            preset: "uk_ofcom".into(),
            ..Setting::default()
        });
        assert!(uk.logger);
        let us = Retained::of(&Setting {
            preset: "us_webcaster".into(),
            ..Setting::default()
        });
        assert!(us.us_statutory);
        let said_no = Retained::of(&Setting {
            preset: "us_webcaster".into(),
            us_statutory_licence: Some(false),
            ..Setting::default()
        });
        assert!(!said_no.us_statutory);
    }
}
