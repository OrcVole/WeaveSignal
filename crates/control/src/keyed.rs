// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows enforced (1.3.2, PLAN 19b; the spike `research/2026-11-24-keyed-spike.md` in the
//! project folder): the control plane's side of keeping a keyed show from the public.
//!
//! **Windows.** A keyed show's *window* is the part of an episode's air time its show's rule
//! keys: from the episode's start to its end, or to the show's `opens_at` when that comes first.
//! Around it go the **margins**: `keys.window_lead_s` before (opening early is harmless), and
//! after it `keys.window_tail_s` plus the desk's profanity delay (the desk sends playout what it
//! took that long ago, so the show's last words reach playout after its scheduled end). A
//! keyed show that overruns its slot is not followed: what the desk sends after the end and the
//! tail is the next show's, and public.
//!
//! **The driver** ([`Driver`]) runs a few times a second. While any margined window holds the
//! clock it keeps playout's public gate shut (`keyed.window on`; the spike's gate), writing the
//! window's state beside playout's socket first, so a playout that restarts mid-show starts
//! shut. Each show's shut time is recorded in the database (`keyed_interval`) *before* the gate
//! is told, so the public HLS can be judged by it after a restart. At a change of keyed show it
//! silences the keyed feed for a moment (`keyed.mute`, the spike's question 6), and at each
//! show's end it takes the listener process's keyed mounts down and up (`keyed.stop`,
//! `keyed.start`), which drops every keyed listener there. Every couple of seconds it asks
//! playout how the gate stands and says it again when they differ; a refusal is an alarm in the
//! studio, never skipped.
//!
//! **What the routes read** ([`Live`]): the windows (for a token's life and the show it is
//! for), the shows on now (for the origin's keyed streams), whether the public HLS and RTMP out
//! are held back, how an HLS segment is classified, and the keys revoked or paused lately.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use uuid::Uuid;
use weavesignal_keyed::{Segment, Shut};
use weavesignal_model::StationId;
use weavesignal_model::listener_keys_db as db;

use crate::hub::{Hub, now_ms};
use crate::playout::PlayoutClient;
use crate::studio::Tone;

/// How far ahead the windows are read: a key redeemed now gets a token for its show's next
/// window within this.
pub const AHEAD_MS: i64 = 14 * 86_400_000;
/// How far back: an episode that started up to a day ago may still be on.
const BEHIND_MS: i64 = 86_400_000;
/// The windows are read again this often (and at once after a rule changes): an episode put
/// on the schedule, moved or put on air is seen within this, which is less than the lead. A
/// station with no keyed show reads only its rules.
const RELOAD_MS: i64 = 2_000;
/// Keys revoked or paused within this long are on the revoked list: longer than any window a
/// whole-window token can cover (an episode lasts at most a day) and its margins.
pub const REVOKED_FOR_MS: i64 = 2 * 86_400_000;

/// What now playing says while a keyed show's window is open and the show's name cannot be read.
const UNNAMED_SHOW: &str = "A show for people with a key";

/// A keyed show's scheduled window, in milliseconds since the Unix epoch, `[start, end)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub show: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// The margins around every window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    /// Shut this long before a window starts.
    pub lead_ms: i64,
    /// Kept shut this long after it ends: the tail setting plus the desk's profanity delay.
    pub tail_ms: i64,
    /// The HLS written this long after a moment may still hold it (`keys.hls_lag_s`): the public
    /// HLS stays held back this long after a window, and a segment is judged by it.
    pub hls_lag_ms: i64,
    /// RTMP out stops this long before a window (`keys.rtmp_lead_s`).
    pub rtmp_lead_ms: i64,
}

impl Window {
    fn shut_from(&self, m: &Margins) -> i64 {
        self.start_ms - m.lead_ms
    }
    fn shut_until(&self, m: &Margins) -> i64 {
        self.end_ms + m.tail_ms
    }
    /// Whether the gate must be shut for this window at `now`.
    pub fn shut_at(&self, m: &Margins, now: i64) -> bool {
        self.shut_from(m) <= now && now < self.shut_until(m)
    }
}

/// The keyed windows of these episodes (`(start_ms, end_ms, show)`) under these rules: an
/// episode whose show's rule is `key` is keyed from its start to its end, or to the show's
/// `opens_at` when that is sooner (from then it is public). Sorted by start.
pub fn windows_of(episodes: &[(i64, i64, Uuid)], access: &[db::Access]) -> Vec<Window> {
    let mut out: Vec<Window> = episodes
        .iter()
        .filter_map(|&(start, end, show)| {
            let a = access.iter().find(|a| a.show_id == show)?;
            if a.rule != "key" {
                return None;
            }
            let end = match a.opens_at_ms {
                Some(at) => end.min(at),
                None => end,
            };
            (end > start).then_some(Window {
                show,
                start_ms: start,
                end_ms: end,
            })
        })
        .collect();
    out.sort_by_key(|w| (w.start_ms, w.show));
    out
}

/// Read the keyed windows around `now` from the schedule and the rules.
pub async fn load_windows(
    pool: &sqlx::PgPool,
    station: StationId,
    now: i64,
) -> Result<Vec<Window>, sqlx::Error> {
    let access = db::list_access(pool, station).await?;
    if !access.iter().any(|a| a.rule == "key") {
        return Ok(Vec::new());
    }
    let eps = weavesignal_model::shows::episodes_between(
        pool,
        station,
        (now - BEHIND_MS).div_euclid(1000),
        (now + AHEAD_MS).div_euclid(1000),
    )
    .await?;
    let spans: Vec<(i64, i64, Uuid)> = eps
        .iter()
        .filter_map(|(show, series, ep)| {
            weavesignal_model::shows::air_window(show, series, ep)
                .map(|(s, e)| (s * 1000, e * 1000, show.id))
        })
        .collect();
    Ok(windows_of(&spans, &access))
}

/// What the routes read; kept by the [`Driver`] (and loaded on demand where none runs).
#[derive(Default)]
pub struct Live {
    s: Mutex<LiveState>,
}

#[derive(Default)]
struct LiveState {
    windows: Vec<Window>,
    loaded_ms: Option<i64>,
    margins: Margins,
    /// Each show's recorded shut time (the driver's, and those read back at start).
    intervals: Vec<Shut>,
    revoked: HashSet<String>,
}

impl Live {
    pub fn new(margins: Margins) -> Self {
        let live = Self::default();
        live.s.lock().unwrap().margins = margins;
        live
    }

    pub fn margins(&self) -> Margins {
        self.s.lock().unwrap().margins
    }

    /// The desk's profanity delay now joins the tail (the configured tail plus it).
    pub fn set_margins(&self, m: Margins) {
        self.s.lock().unwrap().margins = m;
    }

    pub fn set_windows(&self, w: Vec<Window>, now: i64) {
        let mut s = self.s.lock().unwrap();
        s.windows = w;
        s.loaded_ms = Some(now);
    }

    /// Read again at the next look (a rule or the schedule changed).
    pub fn stale(&self) {
        self.s.lock().unwrap().loaded_ms = None;
    }

    pub fn needs_load(&self, now: i64) -> bool {
        self.s
            .lock()
            .unwrap()
            .loaded_ms
            .is_none_or(|t| now - t >= RELOAD_MS)
    }

    /// Load the windows when they are older than a few seconds.
    pub async fn refresh(&self, pool: &sqlx::PgPool, station: StationId) {
        let now = now_ms() as i64;
        if !self.needs_load(now) {
            return;
        }
        match load_windows(pool, station, now).await {
            Ok(w) => self.set_windows(w, now),
            Err(e) => tracing::error!(error = %e, "keyed shows: cannot read the windows"),
        }
    }

    pub fn windows(&self) -> Vec<Window> {
        self.s.lock().unwrap().windows.clone()
    }

    /// The shows whose margined window holds `now`.
    pub fn active(&self, now: i64) -> Vec<Uuid> {
        let s = self.s.lock().unwrap();
        let mut v: Vec<Uuid> = s
            .windows
            .iter()
            .filter(|w| w.shut_at(&s.margins, now))
            .map(|w| w.show)
            .collect();
        v.dedup();
        v
    }

    /// The shows whose keyed listeners are still being served at `now`: from a window's lead to
    /// its tail and the time its listeners take to hear it to the end (they are
    /// [`MOUNT_LAG_MS`] behind) and the keyed mounts to be taken down and up again. While two
    /// are, the keyed mounts' buffer may hold the first one's end: no new keyed stream starts.
    pub fn serving(&self, now: i64) -> Vec<Uuid> {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        let mut v: Vec<Uuid> = s
            .windows
            .iter()
            .filter(|w| w.shut_from(&m) <= now && now < w.shut_until(&m) + DROP_AFTER_MS + DOWN_MS)
            .map(|w| w.show)
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The window of `show` that a token made at `now` is for: the one whose margins hold
    /// `now`, else the next. `(nbf_ms, exp_ms)`: from its lead to its tail and the HLS lag.
    pub fn token_window(&self, show: Uuid, now: i64) -> Option<(i64, i64)> {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        s.windows
            .iter()
            .filter(|w| w.show == show && now < w.shut_until(&m))
            .min_by_key(|w| w.start_ms)
            .map(|w| (w.shut_from(&m), w.shut_until(&m) + m.hls_lag_ms))
    }

    /// The first show `opens` says yes to: on now, else the soonest to start.
    pub fn first_for(&self, opens: impl Fn(Uuid) -> bool, now: i64) -> Option<Uuid> {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        s.windows
            .iter()
            .filter(|w| now < w.shut_until(&m) && opens(w.show))
            .min_by_key(|w| (!w.shut_at(&m, now), w.start_ms))
            .map(|w| w.show)
    }

    /// Whether the public HLS is held back at `now`: a window's margins hold it, or one ended
    /// less than the HLS lag ago, or a recorded shut time is open or that recent.
    pub fn public_hls_held(&self, now: i64) -> bool {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        s.windows
            .iter()
            .any(|w| w.shut_from(&m) <= now && now < w.shut_until(&m) + m.hls_lag_ms)
            || s.intervals
                .iter()
                .any(|i| i.start <= now && i.end.is_none_or(|e| now < e + m.hls_lag_ms))
    }

    /// Whether RTMP out is held back at `now`: from `keys.rtmp_lead_s` before a window to its
    /// tail and the HLS lag after (the desk's RTMP carries the same picture and sound).
    pub fn rtmp_held(&self, now: i64) -> bool {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        s.windows.iter().any(|w| {
            w.shut_from(&m) - m.rtmp_lead_ms <= now && now < w.shut_until(&m) + m.hls_lag_ms
        }) || s
            .intervals
            .iter()
            .any(|i| i.start <= now && i.end.is_none_or(|e| now < e + m.hls_lag_ms))
    }

    /// What an HLS file written at `written_ms` may be served as: judged against every
    /// recorded shut time and every margined window, with the picture and sound it holds taken
    /// to be from up to the HLS lag before it was written (an unmeasured bound: see the docs).
    pub fn classify(&self, written_ms: i64) -> Segment {
        let s = self.s.lock().unwrap();
        let m = s.margins;
        let mut shut: Vec<Shut> = s.intervals.clone();
        shut.extend(s.windows.iter().map(|w| Shut {
            show: w.show.to_string(),
            start: w.shut_from(&m),
            end: Some(w.shut_until(&m)),
        }));
        weavesignal_keyed::classify(written_ms, 0, 0, m.hls_lag_ms, &shut)
    }

    pub fn is_revoked(&self, key: &str) -> bool {
        self.s.lock().unwrap().revoked.contains(key)
    }

    pub fn revoked(&self) -> Vec<String> {
        let mut v: Vec<String> = self.s.lock().unwrap().revoked.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn set_revoked(&self, ids: Vec<String>) {
        self.s.lock().unwrap().revoked = ids.into_iter().collect();
    }

    pub fn set_intervals(&self, v: Vec<Shut>) {
        self.s.lock().unwrap().intervals = v;
    }

    fn open_interval(&self, show: Uuid, at: i64) {
        self.s.lock().unwrap().intervals.push(Shut {
            show: show.to_string(),
            start: at,
            end: None,
        });
    }

    fn close_interval(&self, show: Uuid, at: i64) {
        let id = show.to_string();
        let mut s = self.s.lock().unwrap();
        for i in s.intervals.iter_mut() {
            if i.show == id && i.end.is_none() {
                i.end = Some(at);
            }
        }
        // Keep a day of them.
        s.intervals
            .retain(|i| i.end.is_none_or(|e| at - e < BEHIND_MS));
    }
}

// ---- the recorded shut times ----

/// Record that `show`'s gate is shut from `at`.
async fn record_open(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO keyed_interval (station_id, show_id, start_at) VALUES ($1, $2, to_timestamp($3::float8 / 1000))",
    )
    .bind(station.as_uuid())
    .bind(show)
    .bind(at as f64)
    .execute(pool)
    .await?;
    Ok(())
}

async fn record_close(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE keyed_interval SET end_at = to_timestamp($3::float8 / 1000)
         WHERE station_id = $1 AND show_id = $2 AND end_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(show)
    .bind(at as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// The shut times of the last day, the open ones included (a control plane that stopped with
/// a window open left it open: it is judged shut until the driver closes it).
pub async fn read_intervals(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Shut>, sqlx::Error> {
    let rows: Vec<(Uuid, i64, Option<i64>)> = sqlx::query_as(
        "SELECT show_id, (extract(epoch FROM start_at) * 1000)::bigint,
                (extract(epoch FROM end_at) * 1000)::bigint
         FROM keyed_interval
         WHERE station_id = $1 AND (end_at IS NULL OR end_at > now() - interval '1 day')
         ORDER BY start_at",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(show, start, end)| Shut {
            show: show.to_string(),
            start,
            end,
        })
        .collect())
}

/// The keys revoked or paused within [`REVOKED_FOR_MS`], and those whose own window ended within
/// it (a key shortened after its token was made): what edges refuse at once, and the origin's
/// keyed streams end on.
pub async fn read_revoked(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM listener_key
         WHERE station_id = $1
           AND ((state <> 'active'
                 AND coalesce(state_at, updated_at) > now() - make_interval(secs => $2::float8 / 1000))
             OR (valid_until <= now()
                 AND valid_until > now() - make_interval(secs => $2::float8 / 1000)))
         ORDER BY id",
    )
    .bind(station.as_uuid())
    .bind(REVOKED_FOR_MS as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id.to_string()).collect())
}

// ---- the driver ----

/// The file beside playout's socket that says where the window stands, for a playout that
/// starts again (its data's `keyed.window_file`).
pub fn window_file(playout_socket: &Path) -> PathBuf {
    playout_socket.with_file_name("keyed-window")
}

fn write_window_file(path: &Path, on: bool) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, if on { "on\n" } else { "off\n" })?;
    std::fs::rename(&tmp, path)
}

/// How far behind playout a listener of the listener process is: icecast.server sends each new
/// listener the last 12 s or so and then keeps them that far behind, plus the feed's cushion
/// (measured by the keyed stack test, 2026-11-23: a key holder heard the show about 13 s after
/// playout played it).
pub const MOUNT_LAG_MS: i64 = 13_000;
/// The listener process's keyed mounts go down this long after a show's shut time ends, so its
/// key holders hear the show to its end before they are dropped.
pub const DROP_AFTER_MS: i64 = MOUNT_LAG_MS + 500;
/// Down this long before they come back up.
pub const DOWN_MS: i64 = 1_000;
/// Playout is asked how the gate stands this often.
const RECONCILE_MS: i64 = 2_000;
/// The revoked list is read again this often.
const REVOKED_EVERY_MS: i64 = 2_000;

pub struct Driver {
    pub station: StationId,
    pub db: sqlx::PgPool,
    pub playout: PlayoutClient,
    pub listeners: PlayoutClient,
    pub window_file: Option<PathBuf>,
    pub hub: Weak<Hub>,
    pub live: Arc<Live>,
    /// The configured margins; the desk's delay is added to the tail as it changes.
    pub base: Margins,
}

#[derive(Default)]
struct Run {
    /// The shows whose shut time is open.
    open: Vec<Uuid>,
    /// What playout was last told and acknowledged: `Some(true)` shut.
    told: Option<bool>,
    reconciled_ms: i64,
    revoked_ms: i64,
    /// When to take the keyed mounts down, and when to put them up.
    drops: Vec<(i64, bool)>,
    alarm: bool,
    /// The keyed show now playing names, with its name (looked up once per show).
    named: Option<(Uuid, String)>,
}

/// What the desk's Playout panel is told about a keyed show's window (`KeyedOnAir`).
///
/// `no_problem`: playout accepted the gate as it should stand. `away`: playout did not answer
/// at the last look. `told_shut`: playout acknowledged a shut gate. `slate`: what playout says
/// the public hears meanwhile.
///
/// `confirmed` is whether playout itself answered: with it away, what the public hears is the
/// listener process's fallback by design, not something playout said, and the studio words it
/// as "not confirmed".
fn on_air_view(
    show: Uuid,
    no_problem: bool,
    away: bool,
    told_shut: bool,
    slate: Option<String>,
) -> crate::studio::KeyedOnAir {
    crate::studio::KeyedOnAir {
        show,
        kept: no_problem && (away || told_shut),
        confirmed: !away,
        public_hears: if away {
            Some("listener_fallback".to_owned())
        } else if told_shut {
            slate
        } else {
            None
        },
    }
}

impl Driver {
    pub fn spawn(self) {
        tokio::spawn(async move {
            let mut run = Run::default();
            match read_intervals(&self.db, self.station).await {
                Ok(v) => {
                    // A shut time left open by a control plane that stopped: carried on.
                    run.open = v
                        .iter()
                        .filter(|i| i.end.is_none())
                        .filter_map(|i| i.show.parse().ok())
                        .collect();
                    self.live.set_intervals(v);
                }
                Err(e) => tracing::error!(error = %e, "keyed shows: cannot read the shut times"),
            }
            loop {
                self.tick(&mut run).await;
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
    }

    fn notice(&self, tone: Tone, kind: &str, title: &str, message: String) {
        if let Some(hub) = self.hub.upgrade() {
            hub.with_studio(|s, now| s.playout_notice(now, tone, kind, title.to_owned(), message));
        }
    }

    async fn tick(&self, run: &mut Run) {
        let now = now_ms() as i64;
        // The desk's profanity delay joins the tail.
        let delay = self
            .hub
            .upgrade()
            .and_then(|h| h.desk_status())
            .and_then(|d| d.delay)
            .map_or(0, |d| i64::from(d.target_ms.max(d.delay_ms)));
        let mut m = self.base;
        m.tail_ms += delay;
        self.live.set_margins(m);
        self.live.refresh(&self.db, self.station).await;
        if now - run.revoked_ms >= REVOKED_EVERY_MS {
            run.revoked_ms = now;
            match read_revoked(&self.db, self.station).await {
                Ok(v) => self.live.set_revoked(v),
                Err(e) => tracing::warn!(error = %e, "keyed shows: cannot read the revoked keys"),
            }
        }

        let want = self.live.active(now);
        // What listeners are told is playing names only the show while its window is open. Set
        // here, ahead of any call to playout or the gate's record, so that no track is told
        // after the lead has begun (the lead is three seconds, and every look is a quarter of
        // one).
        let name = self.name_of(run, want.first().copied()).await;
        if let Some(onair) = self.hub.upgrade().and_then(|h| h.onair.get().cloned()) {
            onair.set_keyed(name).await;
        }
        let newly: Vec<Uuid> = want
            .iter()
            .filter(|s| !run.open.contains(s))
            .copied()
            .collect();
        let gone: Vec<Uuid> = run
            .open
            .iter()
            .filter(|s| !want.contains(s))
            .copied()
            .collect();

        // A show starts: its shut time is recorded first, then the gate is told.
        for s in &newly {
            if let Err(e) = record_open(&self.db, self.station, *s, now).await {
                // Not recorded is not a reason to leave the gate open: carry on and say so.
                tracing::error!(error = %e, show = %s, "keyed shows: cannot record a shut time");
            }
            self.live.open_interval(*s, now);
            tracing::info!(show = %s, "keyed shows: a keyed show's window opens");
        }
        // A change of keyed show (the next one's margins begin while the last one's run): the
        // keyed feed is silent until the last one's listeners have been dropped.
        if !newly.is_empty() && run.open.iter().any(|s| !newly.contains(s)) {
            let last_end = self
                .live
                .windows()
                .iter()
                .filter(|w| run.open.contains(&w.show) && w.shut_at(&m, now))
                .map(|w| w.end_ms + m.tail_ms)
                .max()
                .unwrap_or(now);
            let secs = ((last_end - now + DROP_AFTER_MS + DOWN_MS + 1_000) as f64 / 1000.0)
                .clamp(1.0, 60.0);
            match self.playout.command(&format!("keyed.mute {secs:.1}")).await {
                Ok(a) if a.starts_with("ok") => {
                    tracing::info!(
                        secs,
                        "keyed shows: the keyed feed is silent for a change of show"
                    )
                }
                Ok(a) => tracing::error!(answer = %a, "keyed shows: playout refused keyed.mute"),
                Err(e) => tracing::error!(error = %e, "keyed shows: keyed.mute not sent"),
            }
        }

        let shut = !want.is_empty();
        let due = now - run.reconciled_ms >= RECONCILE_MS;
        if run.told != Some(shut) || due {
            run.reconciled_ms = now;
            self.tell(run, want.first().copied()).await;
        }

        // A show ends: after the gate (if nothing else holds it shut), its shut time is closed,
        // and its keyed listeners are dropped once the listener process has played it out.
        for s in &gone {
            if let Err(e) = record_close(&self.db, self.station, *s, now).await {
                tracing::error!(error = %e, show = %s, "keyed shows: cannot close a shut time");
            }
            self.live.close_interval(*s, now);
            run.drops.push((now + DROP_AFTER_MS, false));
            run.drops.push((now + DROP_AFTER_MS + DOWN_MS, true));
            tracing::info!(show = %s, "keyed shows: a keyed show's window closes");
        }
        run.open = want;

        // The keyed mounts down and up, in order.
        run.drops.sort_by_key(|d| d.0);
        while let Some(&(at, up)) = run.drops.first() {
            if at > now {
                break;
            }
            run.drops.remove(0);
            let cmd = if up { "keyed.start" } else { "keyed.stop" };
            match self.listeners.command(cmd).await {
                Ok(a) if a.starts_with("ok") => tracing::info!(cmd, "keyed shows: keyed mounts"),
                Ok(a) => {
                    tracing::warn!(cmd, answer = %a, "keyed shows: the listener process refused")
                }
                Err(e) => {
                    tracing::warn!(cmd, error = %e, "keyed shows: the listener process did not answer")
                }
            }
        }
    }

    /// The keyed show's name for now playing: from the database once per show, and a generic
    /// name when it cannot be read (never a track, never the station's other title).
    async fn name_of(&self, run: &mut Run, show: Option<Uuid>) -> Option<(Uuid, String)> {
        let id = show?;
        if let Some((n, name)) = &run.named
            && *n == id
        {
            return Some((id, name.clone()));
        }
        let name = match weavesignal_model::shows::get_show_by_id(&self.db, id).await {
            Ok(Some(s)) => s.name,
            _ => return Some((id, UNNAMED_SHOW.to_owned())),
        };
        run.named = Some((id, name.clone()));
        Some((id, name))
    }

    /// Tell playout whether the gate is shut, after writing it beside its socket; then ask how
    /// it stands. A refusal or no answer while it must be shut is an alarm.
    async fn tell(&self, run: &mut Run, show: Option<Uuid>) {
        let shut = show.is_some();
        if let Some(f) = &self.window_file
            && let Err(e) = write_window_file(f, shut)
        {
            tracing::warn!(error = %e, file = %f.display(), "keyed shows: cannot write the window's file");
        }
        let answer = self.playout.command("keyed.status").await;
        let status: Option<serde_json::Value> = answer
            .as_ref()
            .ok()
            .and_then(|a| serde_json::from_str(a).ok());
        let enabled = status.as_ref().and_then(|v| v["enabled"].as_bool());
        let window = status.as_ref().and_then(|v| v["window"].as_bool());
        // What the public hears while the gate is shut, as playout says (1.3.2's scripts say).
        let slate = status
            .as_ref()
            .and_then(|v| v["slate"].as_str())
            .map(str::to_owned);
        let away = answer.is_err();
        let problem = match (enabled, window) {
            (Some(true), Some(w)) if w == shut => {
                run.told = Some(shut);
                None
            }
            (Some(true), _) => {
                let cmd = if shut { "keyed.window on" } else { "keyed.window off" };
                match self.playout.command(cmd).await {
                    Ok(a) if a.starts_with("ok") => {
                        run.told = Some(shut);
                        tracing::info!(cmd, "keyed shows: playout's gate");
                        None
                    }
                    Ok(a) => Some(format!("playout refused `{cmd}`: {a}")),
                    Err(e) => Some(e.to_string()),
                }
            }
            // Keyed shows off and nothing to shut: nothing to say (a station without them).
            (Some(false), _) if !shut => {
                run.told = Some(false);
                None
            }
            (Some(false), _) => Some(
                "playout has keyed shows off in its data (`keyed.enabled`): nothing keeps this show from the public"
                    .to_owned(),
            ),
            (None, _) => match &answer {
                // Playout away: the listener process plays its own fallback, which holds
                // nothing keyed. Said in the log, not as an alarm.
                Err(_) => {
                    run.told = None;
                    if shut {
                        tracing::warn!("keyed shows: playout is not answering during a keyed window");
                    }
                    None
                }
                Ok(_) if !shut => {
                    run.told = Some(false);
                    None
                }
                Ok(_) => Some(
                    "playout does not know keyed shows (an older script?): nothing keeps this show from the public"
                        .to_owned(),
                ),
            },
        };
        // The desk's Playout panel: which keyed show, and whether the public is kept from it.
        let on_air = show.map(|show| {
            on_air_view(
                show,
                problem.is_none(),
                away,
                run.told == Some(true),
                slate.clone(),
            )
        });
        if let Some(hub) = self.hub.upgrade() {
            hub.with_studio_if(|s| s.set_keyed(on_air));
        }

        match problem {
            Some(p) if shut => {
                tracing::error!(problem = %p, "keyed shows: the public gate is not shut");
                if !run.alarm {
                    run.alarm = true;
                    self.notice(
                        Tone::Serious,
                        "keyed.gate_open",
                        "Keyed show not kept from the public",
                        format!("A keyed show's window is open and {p}."),
                    );
                }
            }
            Some(p) => tracing::warn!(problem = %p, "keyed shows: playout's gate"),
            None => {
                if run.alarm && run.told == Some(shut) {
                    run.alarm = false;
                    self.notice(
                        Tone::Good,
                        "keyed.gate_shut",
                        "Keyed show kept from the public",
                        "Playout's public gate follows the keyed show again.".into(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(show: Uuid, rule: &str, opens_at_ms: Option<i64>) -> db::Access {
        db::Access {
            show_id: show,
            rule: rule.into(),
            opens_at_ms,
            slate: "announcement".into(),
            slate_note: String::new(),
            updated_ms: 0,
            updated_by: "test".into(),
        }
    }

    const M: Margins = Margins {
        lead_ms: 3_000,
        tail_ms: 5_000,
        hls_lag_ms: 30_000,
        rtmp_lead_ms: 15_000,
    };

    #[test]
    fn a_keyed_show_is_keyed_until_it_opens_to_everyone() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let eps = [
            (100_000, 200_000, a),
            (200_000, 300_000, b),
            (300_000, 400_000, c),
            (400_000, 500_000, c),
        ];
        let w = windows_of(
            &eps,
            &[
                access(a, "key", None),
                access(b, "public", None),
                access(c, "key", Some(350_000)),
            ],
        );
        assert_eq!(
            w,
            vec![
                Window {
                    show: a,
                    start_ms: 100_000,
                    end_ms: 200_000
                },
                Window {
                    show: c,
                    start_ms: 300_000,
                    end_ms: 350_000
                },
            ],
            "b is public; c opens at 350 s, so its second episode is public"
        );
    }

    #[test]
    fn the_margins_shut_early_and_late_and_the_hls_after() {
        let a = Uuid::from_u128(1);
        let live = Live::new(M);
        live.set_windows(
            vec![Window {
                show: a,
                start_ms: 100_000,
                end_ms: 200_000,
            }],
            0,
        );
        assert!(live.active(96_999).is_empty());
        assert_eq!(live.active(97_000), vec![a], "the lead");
        assert_eq!(live.active(204_999), vec![a], "the tail");
        assert!(live.active(205_000).is_empty());
        assert!(!live.rtmp_held(81_999));
        assert!(live.rtmp_held(82_000), "RTMP out stops before the lead");
        assert!(live.public_hls_held(234_999), "the HLS lag after the tail");
        assert!(!live.public_hls_held(235_000));
        assert_eq!(live.token_window(a, 50_000), Some((97_000, 235_000)));
        assert_eq!(live.token_window(a, 205_000), None, "it has ended");
        // A segment written 20 s after the tail may hold the show's end.
        assert_eq!(live.classify(225_000), Segment::Keyed(a.to_string()));
        assert_eq!(live.classify(235_001), Segment::Public);
        assert_eq!(live.classify(96_999), Segment::Public);
    }

    #[test]
    fn key_holders_are_served_to_the_end_and_no_new_stream_starts_while_two_shows_are() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let live = Live::new(M);
        live.set_windows(
            vec![
                Window {
                    show: a,
                    start_ms: 100_000,
                    end_ms: 200_000,
                },
                Window {
                    show: b,
                    start_ms: 200_000,
                    end_ms: 300_000,
                },
            ],
            0,
        );
        assert_eq!(live.serving(150_000), vec![a]);
        // b's lead begins at 197 s; a's holders are 13 s behind, so a is served to its tail
        // (205 s), the drop after it (13.5 s) and a second: both until 219.5 s, so no new stream.
        assert_eq!(live.serving(198_000), vec![a, b]);
        assert_eq!(live.serving(219_499), vec![a, b]);
        assert_eq!(live.serving(219_500), vec![b]);
    }

    #[test]
    fn a_recorded_shut_time_counts_after_the_schedule_forgets_it() {
        let a = Uuid::from_u128(1);
        let live = Live::new(M);
        live.open_interval(a, 1_000_000);
        assert!(live.public_hls_held(1_500_000), "still open");
        live.close_interval(a, 1_600_000);
        assert!(live.public_hls_held(1_629_999));
        assert!(!live.public_hls_held(1_630_000));
        assert_eq!(live.classify(1_620_000), Segment::Keyed(a.to_string()));
    }

    #[test]
    fn a_token_is_for_the_show_on_now_else_the_next_one_it_opens() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let live = Live::new(M);
        live.set_windows(
            vec![
                Window {
                    show: a,
                    start_ms: 100_000,
                    end_ms: 200_000,
                },
                Window {
                    show: b,
                    start_ms: 300_000,
                    end_ms: 400_000,
                },
            ],
            0,
        );
        assert_eq!(live.first_for(|_| true, 150_000), Some(a));
        assert_eq!(live.first_for(|s| s == b, 150_000), Some(b));
        assert_eq!(live.first_for(|_| true, 250_000), Some(b));
        assert_eq!(live.first_for(|s| s == a, 250_000), None);
    }

    #[test]
    fn what_the_public_hears_is_confirmed_only_when_playout_answered() {
        let show = Uuid::from_u128(7);
        // Playout answered, the gate is shut and it names the slate.
        let v = on_air_view(show, true, false, true, Some("slate_file".into()));
        assert!(v.kept && v.confirmed);
        assert_eq!(v.public_hears.as_deref(), Some("slate_file"));
        // Playout did not answer: the listener process's fallback holds nothing keyed, which
        // is the design and not playout's word, so it is kept but not confirmed.
        let v = on_air_view(show, true, true, false, None);
        assert!(v.kept && !v.confirmed);
        assert_eq!(v.public_hears.as_deref(), Some("listener_fallback"));
        // Playout answered and refused: the studio's alarm, and that is confirmed.
        let v = on_air_view(show, false, false, false, Some("fallback".into()));
        assert!(!v.kept && v.confirmed);
        assert_eq!(v.public_hears, None);
        // Playout answered but the gate is not yet told shut (the next look says): nothing
        // is claimed about what is heard.
        let v = on_air_view(show, true, false, false, Some("fallback".into()));
        assert!(!v.kept && v.confirmed && v.public_hears.is_none());
    }
}
