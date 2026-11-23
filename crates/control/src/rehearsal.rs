// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio (1.3.0, #23): the control plane's second desk link and the rehearsal's
//! routes. The design is `docs/reference/design/rehearsal-mode-build-plan.md` in the project
//! folder (sections 1.2 to 1.7 and 1.12; steps R6 and R7).
//!
//! The rehearsal desk is a second desk process that mixes the rehearsal room and has no way to
//! the air. This side keeps it apart from the live studio as follows:
//!
//! - its own [`DeskClient`], which sends nothing to a desk that does not say it is a rehearsal
//!   desk (the live link refuses a rehearsal desk the same way);
//! - its own state, [`Rehearsal`], held beside the hub (`hub.rehearsal`) and read only by the
//!   `/api/v1/rehearsal…` routes: nothing here touches the hub's studio, its desk link or its
//!   cart wall, so the studio's view and events, webhooks, now playing, the per-play log and
//!   the chat see nothing of a rehearsal;
//! - its commands come through `POST /api/v1/rehearsal/commands` only, which takes the nine
//!   requests a rehearsal desk takes and no other;
//! - carts fire from the rehearsed show's banks and the station's, by the session's show, never
//!   the show on air; their plays are never read for the per-play log.
//!
//! One session (one show) at a time, kept in the database so a restart keeps it. Who may open,
//! drive and end it: whoever may change the show's step (its presenter, a producer, an admin,
//! the station key), from a window that is live or preparing. API tokens never (`scopes::NEVER`).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::carts::CartMode;
use weavesignal_model::rehearsal::{self as model, Ended, Opened, Opening, Session, ShowRef};
use weavesignal_proto::Role;

use crate::app::{AppState, cart_problem, desk_problem, problem};
use crate::config::RehearsalConfig;
use crate::desk::{DeskClient, DeskCommand, DeskError, DeskStatus};
use crate::hub::{Hub, now_ms};
use crate::staff::{Actor, Need};

/// The rehearsal's routes. Kept first in the file: the tests that check every route is
/// described and placed for API tokens read the sources up to the first test-only item.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/rehearsal", get(view))
        .route("/api/v1/rehearsal/events", get(events))
        .route("/api/v1/rehearsal/open", post(open))
        .route("/api/v1/rehearsal/commands", post(command))
        .route("/api/v1/rehearsal/end", post(end))
        .route("/api/v1/rehearsal/remove", post(remove))
        .route("/api/v1/rehearsal/move", post(move_people))
}

// --- the store ------------------------------------------------------------------------------

/// Where sessions are kept: the database, or (tests) memory.
pub enum Store {
    /// No database: the rehearsal studio cannot open (it keeps its sessions there).
    None,
    Db {
        pool: sqlx::PgPool,
        station: StationId,
    },
    #[cfg(test)]
    Memory(Mutex<Memory>),
}

/// The shows and sessions a test's store holds.
#[cfg(test)]
#[derive(Default)]
pub struct Memory {
    pub shows: Vec<ShowRef>,
    pub sessions: Vec<Session>,
    /// Each show's step by slug (`full` when not listed).
    pub steps: BTreeMap<String, weavesignal_model::shows::Step>,
    /// Airings: the show, its start and its end, in ms since the Unix epoch.
    pub airings: Vec<(Uuid, i64, i64)>,
    /// The rules' grace and fade.
    pub grace_s: i32,
    pub fade_ms: i32,
}

/// The rules of the post a move and an opening follow (ADR-0280): the grace after an airing's
/// start, and the crossfade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostRules {
    pub grace_s: i32,
    pub fade_ms: i32,
}

/// How far back and ahead the schedule is read for a show's airing: one under way, or one
/// within the rota's twelve hours (`rota::HORIZON_S`).
const AIRING_BACK_S: i64 = 30 * 3600;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the rehearsal studio keeps its sessions in the database, and none is configured")]
    NoDatabase,
    #[error("the database: {0}")]
    Db(#[from] sqlx::Error),
}

impl Store {
    /// The step a show runs at and its presenter's account, read fresh (ADR-0271): invitations
    /// to a rehearsal follow the rehearsed show's step, never the show on air's.
    pub(crate) async fn policy(
        &self,
        show: Uuid,
    ) -> Result<Option<crate::studio::StepPolicy>, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                let all = weavesignal_model::shows::list_shows(pool, *station).await?;
                Ok(all
                    .iter()
                    .find(|s| s.id == show)
                    .map(|s| crate::studio::StepPolicy {
                        step: s.step,
                        presenter_account: s.presenter_account,
                    }))
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let m = m.lock().unwrap();
                Ok(m.shows
                    .iter()
                    .find(|s| s.id == show)
                    .map(|s| crate::studio::StepPolicy {
                        step: m
                            .steps
                            .get(&s.slug)
                            .copied()
                            .unwrap_or(weavesignal_model::shows::Step::Full),
                        presenter_account: s.presenter_account,
                    }))
            }
        }
    }

    /// The show's airing under way at `now_ms`, else its first within the rota's twelve hours:
    /// `(start, end)` in ms since the Unix epoch.
    pub(crate) async fn airing(
        &self,
        show: Uuid,
        now_ms: i64,
    ) -> Result<Option<(i64, i64)>, StoreError> {
        let horizon = crate::rota::HORIZON_S * 1000;
        let pick = |mut all: Vec<(i64, i64)>| {
            all.sort();
            all.into_iter()
                .find(|(start, end)| *end > now_ms && *start <= now_ms + horizon)
        };
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                let now_s = now_ms.div_euclid(1000);
                let eps = weavesignal_model::shows::episodes_between(
                    pool,
                    *station,
                    now_s - AIRING_BACK_S,
                    now_s + crate::rota::HORIZON_S,
                )
                .await?;
                Ok(pick(
                    eps.iter()
                        .filter(|(s, _, _)| s.id == show)
                        .filter_map(|(s, series, ep)| {
                            weavesignal_model::shows::air_window(s, series, ep)
                                .map(|(a, b)| (a * 1000, b * 1000))
                        })
                        .collect(),
                ))
            }
            #[cfg(test)]
            Store::Memory(m) => Ok(pick(
                m.lock()
                    .unwrap()
                    .airings
                    .iter()
                    .filter(|a| a.0 == show)
                    .map(|a| (a.1, a.2))
                    .collect(),
            )),
        }
    }

    /// The post's grace and fade in force.
    pub(crate) async fn rules(&self, hub: &Hub) -> Result<PostRules, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                let s = hub
                    .sources
                    .in_force(weavesignal_model::sources::get_settings(pool, *station).await?);
                Ok(PostRules {
                    grace_s: s.grace_s,
                    fade_ms: s.fade_ms,
                })
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let m = m.lock().unwrap();
                Ok(PostRules {
                    grace_s: m.grace_s,
                    fade_ms: m.fade_ms,
                })
            }
        }
    }

    /// A session by its id, open or ended.
    pub(crate) async fn session_by_id(&self, id: Uuid) -> Result<Option<Session>, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => Ok(model::session(pool, *station, id).await?),
            #[cfg(test)]
            Store::Memory(m) => Ok(m
                .lock()
                .unwrap()
                .sessions
                .iter()
                .find(|s| s.id == id)
                .cloned()),
        }
    }

    async fn record_move(
        &self,
        id: Uuid,
        moved: &[String],
        opening: &Opening,
    ) -> Result<bool, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                Ok(model::record_move(pool, *station, id, moved, opening).await?)
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let mut m = m.lock().unwrap();
                match m
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == id && s.ended.is_none())
                {
                    Some(s) => {
                        s.moved_at_ms = Some(now_ms() as i64);
                        s.moved = moved.to_vec();
                        s.opening = Some(opening.clone());
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
        }
    }

    /// The moves whose openings are not cleared, from the last day and a half, newest first.
    async fn moved_sessions(&self) -> Result<Vec<Session>, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                Ok(model::moved_sessions(pool, *station, MOVES_KEPT_MS).await?)
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let mut v: Vec<Session> = m
                    .lock()
                    .unwrap()
                    .sessions
                    .iter()
                    .filter(|s| {
                        s.ended == Some(Ended::Moved)
                            && s.opening
                                .as_ref()
                                .is_some_and(|o| o.cleared_at_ms.is_none())
                    })
                    .cloned()
                    .collect();
                v.sort_by_key(|s| std::cmp::Reverse(s.moved_at_ms));
                Ok(v)
            }
        }
    }

    async fn set_opening(&self, id: Uuid, opening: &Opening) -> Result<bool, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                Ok(model::set_opening(pool, *station, id, opening).await?)
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let mut m = m.lock().unwrap();
                match m
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == id && s.moved_at_ms.is_some())
                {
                    Some(s) => {
                        s.opening = Some(opening.clone());
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
        }
    }

    async fn clear_other_openings(
        &self,
        show: Uuid,
        except: Uuid,
        at_ms: i64,
    ) -> Result<u64, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                Ok(model::clear_other_openings(pool, *station, show, except, at_ms).await?)
            }
            #[cfg(test)]
            Store::Memory(m) => {
                let mut n = 0;
                for s in m.lock().unwrap().sessions.iter_mut() {
                    if s.show_id == show
                        && s.id != except
                        && let Some(o) = s.opening.as_mut()
                        && o.cleared_at_ms.is_none()
                    {
                        o.cleared_at_ms = Some(at_ms);
                        n += 1;
                    }
                }
                Ok(n)
            }
        }
    }

    async fn show(&self, slug: &str) -> Result<Option<ShowRef>, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => {
                Ok(weavesignal_model::shows::get_show(pool, *station, slug)
                    .await?
                    .map(|s| ShowRef {
                        id: s.id,
                        slug: s.slug,
                        name: s.name,
                        presenter_account: s.presenter_account,
                    }))
            }
            #[cfg(test)]
            Store::Memory(m) => Ok(m
                .lock()
                .unwrap()
                .shows
                .iter()
                .find(|s| s.slug == slug)
                .cloned()),
        }
    }

    async fn open_session(&self) -> Result<Option<Session>, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => Ok(model::open_session(pool, *station).await?),
            #[cfg(test)]
            Store::Memory(m) => Ok(m
                .lock()
                .unwrap()
                .sessions
                .iter()
                .find(|s| s.ended.is_none())
                .cloned()),
        }
    }

    async fn open(&self, show: &ShowRef, by: &str) -> Result<Opened, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => Ok(model::open(pool, *station, show, by).await?),
            #[cfg(test)]
            Store::Memory(m) => {
                let mut m = m.lock().unwrap();
                if let Some(open) = m.sessions.iter().find(|s| s.ended.is_none()) {
                    return Ok(if open.show_id == show.id {
                        Opened::Already(open.clone())
                    } else {
                        Opened::Busy(open.clone())
                    });
                }
                let s = Session {
                    id: Uuid::new_v4(),
                    show_id: show.id,
                    show_slug: show.slug.clone(),
                    show_name: show.name.clone(),
                    presenter_account: show.presenter_account,
                    opened_by: by.to_owned(),
                    opened_at_ms: now_ms() as i64,
                    ended_at_ms: None,
                    ended: None,
                    moved_at_ms: None,
                    moved: Vec::new(),
                    opening: None,
                };
                m.sessions.push(s.clone());
                Ok(Opened::New(s))
            }
        }
    }

    async fn end(&self, id: Uuid, how: Ended) -> Result<bool, StoreError> {
        match self {
            Store::None => Err(StoreError::NoDatabase),
            Store::Db { pool, station } => Ok(model::end(pool, *station, id, how).await?),
            #[cfg(test)]
            Store::Memory(m) => {
                let mut m = m.lock().unwrap();
                match m
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == id && s.ended.is_none())
                {
                    Some(s) => {
                        s.ended = Some(how);
                        s.ended_at_ms = Some(now_ms() as i64);
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
        }
    }
}

// --- the view -------------------------------------------------------------------------------

/// The rehearsal studio as the studio app shows it (`GET /api/v1/rehearsal`).
#[derive(Clone, Debug, Default, Serialize, PartialEq, utoipa::ToSchema)]
pub struct RehearsalView {
    /// Whether this station has a rehearsal studio (`REHEARSAL` in the installation's
    /// settings). Off, everything else is empty.
    pub enabled: bool,
    /// The rehearsal desk's link.
    pub desk: RehearsalDesk,
    /// The open session, if any: one show at a time.
    pub session: Option<SessionView>,
    /// The people at the rehearsal desk. `on` is in the rehearsal's mix, never on air.
    pub people: Vec<RehearsalPerson>,
    /// The carts sounding at the rehearsal desk.
    pub carts_sounding: Vec<RehearsalSounding>,
    /// The most people the rehearsal room takes at once (`rehearsal.max_people`).
    pub max_people: u16,
    /// The handover window of the session's show, when it is the window the live studio is
    /// showing (the rehearsal view says "On air at 14:05: move by then").
    pub window: Option<weavesignal_model::handover_window::WindowView>,
    /// Whether the person asking may run the open session (1.3.1): whoever may change its
    /// show's step. False with no session open. It is the asker's own, filled in for each
    /// request and each event.
    pub may_run: bool,
}

/// The rehearsal desk's link.
#[derive(Clone, Debug, Default, Serialize, PartialEq, utoipa::ToSchema)]
pub struct RehearsalDesk {
    /// The rehearsal desk answered the last time it was asked.
    pub linked: bool,
    /// Why not, when it did not (not answering, or a desk on its socket that is not a
    /// rehearsal desk).
    pub error: Option<String>,
}

/// The open session.
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct SessionView {
    pub id: Uuid,
    /// The rehearsed show's slug.
    pub show: String,
    pub show_name: String,
    /// Who opened it, as the studio names them ("Kirsty").
    pub opened_by: String,
    pub opened_at_ms: i64,
}

/// One person at the rehearsal desk.
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct RehearsalPerson {
    pub identity: String,
    pub name: String,
    pub role: Role,
    /// In the rehearsal's mix (the others hear them). The word is `on`, never `on_air`: nothing
    /// here is on air (ADR-0062).
    pub on: bool,
    pub mute: bool,
    pub cue: bool,
    pub talk: bool,
    pub talk_to: Option<String>,
    pub present: bool,
    pub linked: bool,
    pub lufs: Option<f32>,
    /// The person's strip, from 1 (1.3.1): the station gives each person at the rehearsal desk
    /// the lowest free number as they arrive and keeps it while the desk lists them, so every
    /// window draws the same person in the same strip.
    pub slot: u16,
}

/// A cart sounding at the rehearsal desk.
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct RehearsalSounding {
    pub cart: String,
    /// The cart's label, when the station still has the cart.
    pub label: String,
    pub mode: CartMode,
    /// In the presenters' ears only.
    pub cue: bool,
    pub elapsed_ms: u64,
    pub remaining_ms: Option<u64>,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub stopping: bool,
}

impl SessionView {
    fn of(s: &Session) -> Self {
        SessionView {
            id: s.id,
            show: s.show_slug.clone(),
            show_name: s.show_name.clone(),
            opened_by: s.opened_by.clone(),
            opened_at_ms: s.opened_at_ms,
        }
    }
}

// --- the rehearsal studio -------------------------------------------------------------------

struct Inner {
    session: Option<Session>,
    /// The live studio's handover window, when it is about the session's show.
    window: Option<weavesignal_model::handover_window::WindowView>,
    desk: Option<DeskStatus>,
    desk_error: Option<String>,
    /// Since when the open session has had nobody present at the desk.
    empty_since_ms: Option<u64>,
    labels: BTreeMap<String, String>,
    /// Each person's strip while the desk lists them (see [`RehearsalPerson::slot`]).
    slots: BTreeMap<String, u16>,
}

/// Give each person the desk lists a strip: those already placed keep theirs, those who have
/// gone give theirs up, and each new person takes the lowest number free, in the desk's order.
fn assign_slots<'a>(
    slots: &mut BTreeMap<String, u16>,
    identities: impl IntoIterator<Item = &'a str> + Clone,
) {
    slots.retain(|id, _| identities.clone().into_iter().any(|x| x == id));
    for id in identities {
        if slots.contains_key(id) {
            continue;
        }
        let free = (1..=u16::MAX)
            .find(|n| !slots.values().any(|v| v == n))
            .unwrap_or(u16::MAX);
        slots.insert(id.to_owned(), free);
    }
}

/// The rehearsal studio: the second desk link, the session, and the view.
pub struct Rehearsal {
    pub config: RehearsalConfig,
    /// The rehearsal room: `<room.name><rehearsal.room_suffix>`.
    pub room: String,
    pub desk: DeskClient,
    store: Store,
    poll: Duration,
    inner: Mutex<Inner>,
    tx: watch::Sender<Arc<RehearsalView>>,
    /// Held across each change of session with the store (open, end, the re-read), so a re-read
    /// never puts back what an open or an end has just changed.
    sessions: tokio::sync::Mutex<()>,
    /// The moves whose openings are not cleared, newest first (read back with the session): the
    /// openings still to apply at the live desk, and the moves a page that missed one is
    /// offered again.
    moves: Mutex<Vec<Session>>,
    /// Held while openings are applied, so the look every second and a press never apply one
    /// twice.
    applying: tokio::sync::Mutex<()>,
}

/// How often the session is read again from the database (a show deleted meanwhile, another
/// process's end).
const REREAD: Duration = Duration::from_secs(5);

/// How far back moves are read for their openings and for pages that missed them: longer than
/// the rota's twelve hours and the longest show after it.
const MOVES_KEPT_MS: i64 = 36 * 3600 * 1000;

impl Rehearsal {
    /// The rehearsal studio beside the live room `room`.
    pub fn new(config: RehearsalConfig, room: &str, store: Store, poll: Duration) -> Self {
        let desk = DeskClient::rehearsal(config.socket.as_deref());
        let error = (!desk.configured()).then(|| DeskError::NotConfigured.to_string());
        let view = RehearsalView {
            enabled: true,
            desk: RehearsalDesk {
                linked: false,
                error: error.clone(),
            },
            max_people: config.max_people,
            ..RehearsalView::default()
        };
        Rehearsal {
            room: config.room(room),
            config,
            desk,
            store,
            poll,
            inner: Mutex::new(Inner {
                session: None,
                window: None,
                desk: None,
                desk_error: error,
                empty_since_ms: None,
                labels: BTreeMap::new(),
                slots: BTreeMap::new(),
            }),
            tx: watch::channel(Arc::new(view)).0,
            sessions: tokio::sync::Mutex::new(()),
            moves: Mutex::new(Vec::new()),
            applying: tokio::sync::Mutex::new(()),
        }
    }

    /// The view when the station has no rehearsal studio.
    pub fn off_view() -> RehearsalView {
        RehearsalView::default()
    }

    pub fn view(&self) -> Arc<RehearsalView> {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<RehearsalView>> {
        self.tx.subscribe()
    }

    /// The open session, if any.
    pub fn session(&self) -> Option<Session> {
        self.lock().session.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn publish(&self, inner: &Inner) {
        let people = inner
            .desk
            .as_ref()
            .map(|d| {
                d.people
                    .iter()
                    .map(|p| RehearsalPerson {
                        identity: p.identity.clone(),
                        name: p.name.clone(),
                        role: p.role,
                        on: p.on_air,
                        mute: p.mute,
                        cue: p.cue,
                        talk: p.talk,
                        talk_to: p.talk_to.clone(),
                        present: p.present,
                        linked: p.linked,
                        lufs: p.lufs,
                        slot: inner.slots.get(&p.identity).copied().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let carts_sounding = inner
            .desk
            .as_ref()
            .and_then(|d| d.carts.as_ref())
            .map(|c| {
                c.sounding
                    .iter()
                    .map(|s| RehearsalSounding {
                        cart: s.cart.clone(),
                        label: inner.labels.get(&s.cart).cloned().unwrap_or_default(),
                        mode: s.mode,
                        cue: s.cue,
                        elapsed_ms: s.elapsed_ms,
                        remaining_ms: s.remaining_ms,
                        looping: s.looping,
                        stopping: s.stopping,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let view = RehearsalView {
            enabled: true,
            desk: RehearsalDesk {
                linked: inner.desk.is_some(),
                error: inner.desk_error.clone(),
            },
            session: inner.session.as_ref().map(SessionView::of),
            people,
            carts_sounding,
            max_people: self.config.max_people,
            window: inner.window.clone(),
            may_run: false,
        };
        self.tx.send_if_modified(|old| {
            if **old == view {
                false
            } else {
                *old = Arc::new(view);
                true
            }
        });
    }

    fn change<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.lock();
        let r = f(&mut inner);
        self.publish(&inner);
        r
    }

    /// The live studio's handover window, shown in the rehearsal view when it is about the
    /// session's show (the post's look every second sets it).
    pub fn set_window(&self, window: Option<&weavesignal_model::handover_window::WindowView>) {
        self.change(|i| {
            i.window = window
                .filter(|w| i.session.as_ref().is_some_and(|s| s.show_slug == w.show))
                .cloned();
        });
    }

    /// The show of the open session, if any.
    pub fn rehearsing(&self) -> Option<Uuid> {
        self.lock().session.as_ref().map(|s| s.show_id)
    }

    /// The moves whose openings are not cleared, newest first.
    pub fn moves(&self) -> Vec<Session> {
        self.moves.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The moves whose opening state still waits to be applied at `now`, newest first.
    pub fn waiting_moves(&self, now: i64) -> Vec<Session> {
        self.moves()
            .into_iter()
            .filter(|s| s.opening.as_ref().is_some_and(|o| o.waiting(now)))
            .collect()
    }

    /// The move of `session` that took `identity` to the live studio, if it is still offered.
    pub fn move_of(&self, session: Uuid, identity: &str, now: i64) -> Option<Session> {
        self.moves()
            .into_iter()
            .find(|s| s.id == session && s.moved.iter().any(|m| m == identity))
            .filter(|s| s.opening.as_ref().is_some_and(|o| o.offered(now)))
    }

    fn keep_move(&self, s: Session) {
        let mut moves = self.moves.lock().unwrap_or_else(|p| p.into_inner());
        moves.retain(|m| m.id != s.id && m.show_id != s.show_id);
        moves.insert(0, s);
    }

    fn keep_opening(&self, id: Uuid, opening: Opening) {
        let mut moves = self.moves.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(m) = moves.iter_mut().find(|m| m.id == id) {
            m.opening = Some(opening);
        }
    }

    /// Read the open session back from the store (at start-up, and every few seconds), with
    /// the moves whose openings are not cleared.
    pub async fn restore(&self) -> Result<(), StoreError> {
        let _held = self.sessions.lock().await;
        {
            // Not while an opening is being applied: the read could put back one just applied.
            let _applying = self.applying.lock().await;
            let moves = self.store.moved_sessions().await?;
            *self.moves.lock().unwrap_or_else(|p| p.into_inner()) = moves;
        }
        let open = self.store.open_session().await?;
        self.change(|i| {
            let same = i.session.as_ref().map(|s| s.id) == open.as_ref().map(|s| s.id);
            if !same {
                if let Some(s) = &open {
                    tracing::info!(show = %s.show_slug, by = %s.opened_by, "a rehearsal session is open");
                }
                i.empty_since_ms = open.as_ref().map(|_| now_ms());
            }
            i.session = open;
        });
        Ok(())
    }

    /// Fold in the rehearsal desk's answer to a status request, with the carts' labels.
    pub fn take_status(
        &self,
        answer: Result<DeskStatus, DeskError>,
        labels: BTreeMap<String, String>,
        now: u64,
    ) {
        self.change(|i| {
            i.labels = labels;
            match answer {
                Ok(st) => {
                    if i.desk_error.take().is_some() {
                        tracing::info!("the rehearsal desk answers");
                    }
                    if st.people.iter().any(|p| p.present) {
                        i.empty_since_ms = None;
                    } else if i.session.is_some() && i.empty_since_ms.is_none() {
                        i.empty_since_ms = Some(now);
                    }
                    assign_slots(&mut i.slots, st.people.iter().map(|p| p.identity.as_str()));
                    i.desk = Some(st);
                }
                Err(e) => {
                    let text = e.to_string();
                    if i.desk_error.as_deref() != Some(&text) {
                        tracing::warn!(error = %text, "the rehearsal desk is not answering");
                    }
                    i.desk = None;
                    i.desk_error = Some(text);
                }
            }
        });
    }

    /// The open session that has had nobody in it for `rehearsal.idle_end_s`, if any.
    fn idle(&self, now: u64) -> Option<Session> {
        let i = self.lock();
        let since = i.empty_since_ms?;
        (now.saturating_sub(since) >= self.config.idle_end_s * 1000)
            .then(|| i.session.clone())
            .flatten()
    }

    /// Ask the rehearsal desk once and fold the answer in; end a session left empty too long.
    pub async fn poll_once(&self, labels: BTreeMap<String, String>) {
        let answer = self.desk.status().await;
        let now = now_ms();
        self.take_status(answer, labels, now);
        if let Some(s) = self.idle(now) {
            tracing::info!(show = %s.show_slug, "a rehearsal session with nobody in it ends by itself");
            if let Err(e) = self.finish(&s, Ended::Idle).await {
                tracing::warn!(error = %e, "cannot end an empty rehearsal session; trying again in a minute");
                // Not at every poll: the count runs again for a minute.
                let again = now.saturating_sub(self.config.idle_end_s * 1000) + 60_000;
                self.lock().empty_since_ms = Some(again);
            }
        }
    }

    /// Open the studio for a show (or find it open).
    pub async fn open(&self, show: &ShowRef, by: &str) -> Result<Opened, StoreError> {
        let _held = self.sessions.lock().await;
        let opened = self.store.open(show, by).await?;
        if let Opened::New(s) | Opened::Already(s) = &opened {
            let s = s.clone();
            self.change(|i| {
                if i.session.as_ref().map(|x| x.id) != Some(s.id) {
                    i.empty_since_ms = Some(now_ms());
                }
                i.session = Some(s);
            });
        }
        if let Opened::New(s) = &opened {
            tracing::info!(show = %s.show_slug, by = %s.opened_by, "the rehearsal studio is open");
        }
        Ok(opened)
    }

    /// End a session: written first (its passes buy nothing from here on), then every cart at
    /// the rehearsal desk faded. Returns the people the desk had, to take out of the room.
    async fn finish(&self, s: &Session, how: Ended) -> Result<Vec<String>, StoreError> {
        let held = self.sessions.lock().await;
        self.store.end(s.id, how).await?;
        let people = self.change(|i| {
            if i.session.as_ref().map(|x| x.id) == Some(s.id) {
                i.session = None;
                i.empty_since_ms = None;
            }
            i.desk
                .as_ref()
                .map(|d| d.people.iter().map(|p| p.identity.clone()).collect())
                .unwrap_or_default()
        });
        drop(held);
        if let Err(e) = self.desk.cart_stop_all(None).await {
            tracing::debug!(error = %e, "no carts faded at the rehearsal desk as the session ended");
        }
        Ok(people)
    }
}

impl Rehearsal {
    /// A show by its slug (`None` when there is none, or it cannot be read).
    pub async fn show_ref(&self, slug: &str) -> Option<ShowRef> {
        self.store.show(slug).await.ok().flatten()
    }

    /// The rehearsal desk's last status, if it answered.
    pub fn desk_status(&self) -> Option<DeskStatus> {
        self.lock().desk.clone()
    }

    /// Whether the rehearsal desk lists this person now.
    pub fn has_person(&self, identity: &str) -> bool {
        self.lock()
            .desk
            .as_ref()
            .is_some_and(|d| d.people.iter().any(|p| p.identity == identity))
    }

    /// A session by its id, open or ended (`None` when it cannot be read).
    pub async fn store_session(&self, id: Uuid) -> Option<Session> {
        match self.store.session_by_id(id).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read a rehearsal session");
                None
            }
        }
    }

    /// The step a show runs at and its presenter, read fresh (`None` when it cannot be read).
    pub async fn policy_of(&self, show: Uuid) -> Option<crate::studio::StepPolicy> {
        match self.store.policy(show).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read a rehearsed show's step");
                None
            }
        }
    }

    /// Whether an identity in the rehearsal room is the rehearsal desk's own (it, or one of its
    /// returns): never taken out, never counted.
    pub fn is_desk(&self, identity: &str) -> bool {
        identity
            .strip_prefix(self.config.desk_identity.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
    }

    /// How many people are in the rehearsal room now, besides `except`: LiveKit's list when it
    /// can be read, else the rehearsal desk's last status.
    pub async fn people_in_room(
        &self,
        room: Option<&crate::hub::RoomAdmin>,
        except: &str,
    ) -> usize {
        if let Some(room) = room {
            match room.list_in(&self.room).await {
                Ok(listed) => {
                    return listed
                        .iter()
                        .filter(|id| !self.is_desk(id) && id.as_str() != except)
                        .count();
                }
                Err(e) => {
                    tracing::debug!(error = %format!("{e:#}"), "cannot list the rehearsal room; counting its desk's people")
                }
            }
        }
        self.lock().desk.as_ref().map_or(0, |d| {
            d.people
                .iter()
                .filter(|p| p.present && p.identity != except)
                .count()
        })
    }
}

/// The carts' labels by id, as the desk names them.
fn labels(hub: &Hub) -> BTreeMap<String, String> {
    hub.cart_labels()
        .into_iter()
        .map(|(id, label)| (id.hyphenated().to_string(), label))
        .collect()
}

/// Follow the rehearsal desk until the process ends, if the station has one.
pub fn spawn_poller(hub: Arc<Hub>) {
    let Some(r) = hub.rehearsal.get().cloned() else {
        return;
    };
    if !r.desk.configured() {
        tracing::warn!("the rehearsal studio is on but `rehearsal.socket` is not set");
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(r.poll);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_read = tokio::time::Instant::now();
        loop {
            tick.tick().await;
            if last_read.elapsed() >= REREAD && !matches!(r.store, Store::None) {
                last_read = tokio::time::Instant::now();
                if let Err(e) = r.restore().await {
                    tracing::warn!(error = %e, "cannot read the rehearsal session back");
                }
            }
            r.poll_once(labels(&hub)).await;
        }
    });
}

/// The rehearsal studio for the control plane's settings, its session read back; set on the
/// hub and followed. Nothing when the station has none.
pub async fn start(hub: &Arc<Hub>, config: &crate::config::Config, store: Store) {
    if !config.rehearsal.enabled {
        return;
    }
    let r = Rehearsal::new(
        config.rehearsal.clone(),
        &config.room.name,
        store,
        Duration::from_millis(config.desk.poll_ms),
    );
    if let Err(e) = r.restore().await {
        tracing::error!(error = %e, "cannot read the rehearsal session back");
    }
    tracing::info!(room = %r.room, "the rehearsal studio is on");
    let _ = hub.rehearsal.set(Arc::new(r));
    spawn_poller(hub.clone());
}

// --- the routes -----------------------------------------------------------------------------

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(view, events, open, command, end, remove, move_people),
    components(schemas(
        MoveResult,
        RehearsalView,
        RehearsalDesk,
        SessionView,
        RehearsalPerson,
        RehearsalSounding,
        OpenRequest,
        RemoveRequest,
        RehearsalCommand
    ))
)]
pub struct Api;

/// What a station without a rehearsal studio says.
pub(crate) const OFF: &str = "The rehearsal studio is off at this station. An admin turns it on with the installation's REHEARSAL setting.";

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn rehearsal(state: &AppState) -> Result<&Arc<Rehearsal>, Response> {
    state
        .hub
        .rehearsal
        .get()
        .ok_or_else(|| problem(StatusCode::SERVICE_UNAVAILABLE, OFF))
}

/// The rehearsal view as `actor` sees it: the shared view with their own `may_run`.
fn seen_by(r: &Rehearsal, actor: &Actor) -> RehearsalView {
    let mut view = (*r.view()).clone();
    view.may_run = r
        .session()
        .is_some_and(|s| crate::steps::may_change(actor, s.presenter_account));
    view
}

fn current(state: &AppState, actor: &Actor) -> RehearsalView {
    state
        .hub
        .rehearsal
        .get()
        .map_or_else(Rehearsal::off_view, |r| seen_by(r, actor))
}

fn store_problem(e: &StoreError) -> Response {
    match e {
        StoreError::NoDatabase => problem(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        StoreError::Db(err) => {
            tracing::error!(error = %err, "rehearsal: database");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
        }
    }
}

/// The open session, and that `actor` may run it: whoever may change its show's step.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn session_for(r: &Rehearsal, actor: &Actor) -> Result<Session, Response> {
    let Some(s) = r.session() else {
        return Err(problem(
            StatusCode::CONFLICT,
            "The rehearsal studio is not open. Open it for a show first.",
        ));
    };
    if !crate::steps::may_change(actor, s.presenter_account) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            format!(
                "Only an admin, a producer or the presenter of {} runs its rehearsal.",
                s.show_name
            ),
        ));
    }
    Ok(s)
}

#[utoipa::path(
    get,
    path = "/api/v1/rehearsal",
    operation_id = "rehearsal_view",
    tag = "rehearsal",
    summary = "The rehearsal studio",
    description = "Whether this station has a rehearsal studio, its desk's link, the open session (one show at a time), the people at the rehearsal desk and the carts sounding there. Nothing in the rehearsal studio reaches the air. `may_run` says whether the asker may run the open session. The studio app fetches it once and then follows `GET /api/v1/rehearsal/events`.",
    responses(
        (status = 200, description = "The rehearsal studio.", body = RehearsalView),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn view(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    Json(current(&state, &actor)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/rehearsal/events",
    operation_id = "rehearsal_events",
    tag = "rehearsal",
    summary = "The rehearsal studio as events",
    description = "Server-sent events: one `state` event at once whose data is the rehearsal view as JSON, then one per change; a keep-alive comment every 10 seconds. As `GET /api/v1/studio/events` is for the live studio.",
    responses(
        (status = 200, description = "An event stream (`text/event-stream`).", body = String, content_type = "text/event-stream"),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn events(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let event = |view: &RehearsalView| {
        Ok::<_, Infallible>(
            Event::default()
                .event("state")
                .json_data(view)
                .expect("the view serialises"),
        )
    };
    let keep = KeepAlive::new().interval(Duration::from_secs(10));
    match state.hub.rehearsal.get().cloned() {
        Some(r) => {
            Sse::new(WatchStream::new(r.subscribe()).map(move |_| event(&seen_by(&r, &actor))))
                .keep_alive(keep)
                .into_response()
        }
        None => {
            let off =
                tokio_stream::once(event(&Rehearsal::off_view())).chain(tokio_stream::pending());
            Sse::new(off).keep_alive(keep).into_response()
        }
    }
}

/// The show to open the rehearsal studio for.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    /// The show's slug.
    pub show: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/open",
    operation_id = "rehearsal_open",
    tag = "rehearsal",
    summary = "Open the rehearsal studio for a show",
    description = "Opens the rehearsal studio for one show: 201 and the view. Whoever may change the show's step may (its presenter, a producer, an admin, the station key), from a window that is live or preparing (ADR-0276); 403 otherwise. The same show's session already open answers 200 with it. Another show's session open is 409, naming who is rehearsing what: one show at a time. A booking for another show holding the studio now is 409 too, naming the show and when it ends (`GET /api/v1/rehearsal/bookings`); with no booking, the studio is first come. 503 when the station has no rehearsal studio or no database.",
    request_body = OpenRequest,
    responses(
        (status = 201, description = "The rehearsal studio, open for the show.", body = RehearsalView),
        (status = 200, description = "It was open for this show already.", body = RehearsalView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn open(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<OpenRequest>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let r = match rehearsal(&state) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let show = match r.store.show(req.show.trim()).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return problem(
                StatusCode::NOT_FOUND,
                format!("show `{}` does not exist", req.show.trim()),
            );
        }
        Err(e) => return store_problem(&e),
    };
    if !crate::steps::may_change(&actor, show.presenter_account) {
        return problem(
            StatusCode::FORBIDDEN,
            format!(
                "Only an admin, a producer or the presenter of {} opens its rehearsal.",
                show.name
            ),
        );
    }
    // A booking for another show holds the studio now (R14): refused, unless this show's session
    // is open already.
    if r.session().map(|s| s.show_id) != Some(show.id)
        && let Err(resp) = crate::rehearsal_bookings::refuse_if_booked(&state, show.id).await
    {
        return resp;
    }
    match r.open(&show, &crate::steps::mover_name(&actor)).await {
        Ok(Opened::New(_)) => (StatusCode::CREATED, Json(seen_by(r, &actor))).into_response(),
        Ok(Opened::Already(_)) => Json(seen_by(r, &actor)).into_response(),
        Ok(Opened::Busy(other)) => problem(
            StatusCode::CONFLICT,
            format!(
                "{} is rehearsing {}. The rehearsal studio takes one show at a time: it is free once they move to the live studio or end the rehearsal.",
                other.opened_by, other.show_name
            ),
        ),
        Err(e) => store_problem(&e),
    }
}

/// A request for the rehearsal desk: the nine a rehearsal desk takes, but `status`, which is
/// `GET /api/v1/rehearsal`. A person's switches are the desk's own words; a cart is named by
/// its id and fired from the rehearsed show's banks or the station's.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RehearsalCommand {
    /// Into the rehearsal's mix, or out of it (never the air: the rehearsal desk has none).
    OnAir {
        identity: String,
        on: bool,
    },
    Mute {
        identity: String,
        on: bool,
    },
    Cue {
        identity: String,
        on: bool,
    },
    Talk {
        identity: String,
        on: bool,
    },
    TalkTo {
        identity: String,
        to: String,
        on: bool,
    },
    /// Fire a cart; `cue` into the presenters' ears only.
    CartFire {
        cart: Uuid,
        #[serde(default)]
        cue: bool,
    },
    /// Fade out a cart.
    CartStop {
        cart: Uuid,
    },
    /// Fade out every cart (`cue`: only the cued, or only the others).
    CartStopAll {
        #[serde(default)]
        cue: Option<bool>,
    },
}

/// The ops the route takes, as the refusal names them.
const OPS: [&str; 8] = [
    "on_air",
    "mute",
    "cue",
    "talk",
    "talk_to",
    "cart_fire",
    "cart_stop",
    "cart_stop_all",
];

/// A body as a command, or the 422 that names what is wrong with it.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn parse_command(body: serde_json::Value) -> Result<RehearsalCommand, Response> {
    let op = body
        .get("op")
        .and_then(|o| o.as_str())
        .unwrap_or_default()
        .to_owned();
    let op = op.as_str();
    if !OPS.contains(&op) {
        let named = if op.is_empty() {
            "A request with no `op`".to_owned()
        } else {
            format!("`{op}`")
        };
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "{named} is not done on the rehearsal desk; it takes {}.",
                OPS.join(", ")
            ),
        ));
    }
    serde_json::from_value(body).map_err(|e| {
        problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("`{op}` is not well formed: {e}"),
        )
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/commands",
    operation_id = "rehearsal_command",
    tag = "rehearsal",
    summary = "A switch or a cart at the rehearsal desk",
    description = "The rehearsal desk's switches (`on_air`: into the rehearsal's mix, never the air; `mute`, `cue`, `talk`, `talk_to`) and its carts (`cart_fire`, `cart_stop`, `cart_stop_all`). Nothing else: any other op, `dump` and `picture` among them, is 422 naming it. These never reach the live desk. A cart fires from the rehearsed show's own banks or the station's, whatever show is on air; another show's is 409, and its plays are not in the per-play log. Whoever may run the session's show (403 otherwise), from a live or a preparing window. 409 with no session open. Answers with the rehearsal view.",
    request_body = RehearsalCommand,
    responses(
        (status = 200, description = "The rehearsal view after the command.", body = RehearsalView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let r = match rehearsal(&state) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let cmd = match parse_command(body) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let session = match session_for(r, &actor) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let desk = &r.desk;
    let sent = match cmd {
        RehearsalCommand::OnAir { identity, on } => {
            desk.send(&DeskCommand::OnAir {
                identity,
                on,
                fade_ms: None,
            })
            .await
        }
        RehearsalCommand::Mute { identity, on } => {
            desk.send(&DeskCommand::Mute { identity, on }).await
        }
        RehearsalCommand::Cue { identity, on } => {
            desk.send(&DeskCommand::Cue { identity, on }).await
        }
        RehearsalCommand::Talk { identity, on } => {
            desk.send(&DeskCommand::Talk { identity, on }).await
        }
        RehearsalCommand::TalkTo { identity, to, on } => {
            desk.send(&DeskCommand::TalkTo { identity, to, on }).await
        }
        RehearsalCommand::CartFire { cart, cue } => {
            let (c, file) = match state.hub.rehearsal_cart(cart, &session.show_slug) {
                Ok(found) => found,
                Err(e) => return cart_problem(&e),
            };
            desk.cart_fire(cart, &file, c.gain_db, c.mode, cue, c.duration_ms)
                .await
                .map(|_| ())
        }
        RehearsalCommand::CartStop { cart } => desk.cart_stop(cart).await.map(|_| ()),
        RehearsalCommand::CartStopAll { cue } => desk.cart_stop_all(cue).await.map(|_| ()),
    };
    if let Err(e) = sent {
        return desk_problem(&e);
    }
    r.poll_once(labels(&state.hub)).await;
    Json(seen_by(r, &actor)).into_response()
}

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/end",
    operation_id = "rehearsal_end",
    tag = "rehearsal",
    summary = "End the rehearsal",
    description = "Ends the open session: its rehearsal links buy nothing from now on, every cart at the rehearsal desk fades, and everyone is taken out of the rehearsal room. Whoever may run the session's show (403 otherwise), from a live or a preparing window; 409 with no session open. Answers with the rehearsal view.",
    responses(
        (status = 200, description = "The rehearsal view, with no session.", body = RehearsalView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn end(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let r = match rehearsal(&state) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let session = match session_for(r, &actor) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut people = match r.finish(&session, Ended::Ended).await {
        Ok(p) => p,
        Err(e) => return store_problem(&e),
    };
    // Everyone LiveKit has in the room, too: the rehearsal desk may be down, or not have seen
    // someone yet, and they must still be taken out. Never the rehearsal desk itself.
    if let Some(room) = &state.hub.room {
        match room.list_in(&r.room).await {
            Ok(listed) => {
                for id in listed {
                    if !r.is_desk(&id) && !people.contains(&id) {
                        people.push(id);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "cannot list the rehearsal room; taking out the people its desk knows")
            }
        }
    }
    tracing::info!(show = %session.show_slug, by = %actor.label(), people = people.len(), "rehearsal ended");
    match &state.hub.room {
        Some(room) => {
            for identity in &people {
                if let Err(e) = room.remove_in(&r.room, identity).await {
                    tracing::warn!(%identity, error = %format!("{e:#}"), "cannot take them out of the rehearsal room; their link no longer joins it");
                }
            }
        }
        None if !people.is_empty() => {
            tracing::warn!(
                "no LiveKit secret: the rehearsal is ended, but its people are still in the rehearsal room"
            );
        }
        None => {}
    }
    r.poll_once(labels(&state.hub)).await;
    Json(seen_by(r, &actor)).into_response()
}

/// The person to take out of the rehearsal.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveRequest {
    /// Their identity, as the rehearsal view's `people` give it.
    pub identity: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/remove",
    operation_id = "rehearsal_remove",
    tag = "rehearsal",
    summary = "Take one person out of the rehearsal",
    description = "Takes one person out of the rehearsal room and ends their rehearsal link for this session: it buys no token for it again, and their page says it ended. The session carries on with everyone else, and they can be invited again. Whoever may run the session's show (403 otherwise); 409 with no session open; 404 when they are not in the rehearsal studio; 422 for the rehearsal desk's own identity. Answers with the rehearsal view.",
    request_body = RemoveRequest,
    responses(
        (status = 200, description = "The rehearsal view after the removal.", body = RehearsalView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RemoveRequest>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let r = match rehearsal(&state) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let session = match session_for(r, &actor) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let identity = req.identity.trim().to_owned();
    if identity.is_empty() || r.is_desk(&identity) {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "That is not a person in the rehearsal studio.",
        );
    }
    // In the room as the rehearsal desk or LiveKit has them.
    let in_room = match &state.hub.room {
        Some(room) => room
            .list_in(&r.room)
            .await
            .map(|l| l.iter().any(|id| id == &identity))
            .unwrap_or(false),
        None => false,
    };
    if !r.has_person(&identity) && !in_room {
        return problem(
            StatusCode::NOT_FOUND,
            format!(
                "`{identity}` is not in the rehearsal studio for {}.",
                session.show_name
            ),
        );
    }
    // Written first: their link buys nothing from here on, then out of the room.
    state
        .joins
        .remove_from_rehearsal(&session.id.to_string(), &identity);
    match &state.hub.room {
        Some(room) => {
            if let Err(e) = room.remove_in(&r.room, &identity).await {
                tracing::warn!(%identity, error = %format!("{e:#}"), "cannot take them out of the rehearsal room; their link no longer joins it");
            }
        }
        None => {
            tracing::warn!(
                "no LiveKit secret: their rehearsal link is ended, but they are still in the rehearsal room"
            );
        }
    }
    tracing::info!(show = %session.show_slug, by = %actor.label(), %identity, "taken out of the rehearsal");
    r.poll_once(labels(&state.hub)).await;
    Json(seen_by(r, &actor)).into_response()
}

/// What a move did (`POST /api/v1/rehearsal/move`).
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct MoveResult {
    /// Everyone moved, by identity: each page is sent its live pass on its event stream.
    pub moved: Vec<String>,
    /// The opening state, applied when the show's host is put on air at the live desk.
    pub opening: Opening,
    /// The airing they stand by for: its start, in ms since the Unix epoch.
    pub airing_start_ms: i64,
}

/// How long after a move anyone still in the rehearsal room is taken out of it: their page has
/// had the move and left, or it never will (their rehearsal link buys nothing any more).
const MOVED_OUT_AFTER: Duration = Duration::from_secs(30);

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/move",
    operation_id = "rehearsal_move",
    tag = "rehearsal",
    summary = "Move to the live studio",
    description = "Moves everyone in the rehearsal studio to the live studio, on the links they already have, and closes the session. In order: the move is written on the session (who moved, and the opening state: who is on and who is muted at the rehearsal desk, among the people present); each person's page is sent a live pass on `GET /api/v1/pass/events` (`move`), and joins the live room **standing by**: nobody goes on air until the handover; then the session ends, so its rehearsal links buy nothing more. The opening state is applied when the show's host is put on air at the live desk, by whichever route (the schedule, the handover's press, taking the air from automation, their own On key): the muted are muted, then the others who were on are put on air with the rules' fade. 409 with no session open, with nobody in the rehearsal studio, or when the show has no airing under way or within twelve hours (the sentence says so). Whoever may run the session's show (403 otherwise), from a live or a preparing window.",
    responses(
        (status = 200, description = "Who moved, and the opening state.", body = MoveResult),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn move_people(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let r = match rehearsal(&state) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let session = match session_for(r, &actor) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let now = now_ms() as i64;
    let airing = match r.store.airing(session.show_id, now).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return problem(
                StatusCode::CONFLICT,
                format!(
                    "{} has no airing under way or in the next twelve hours, so there is nothing to move to. Keep rehearsing, or end the rehearsal.",
                    session.show_name
                ),
            );
        }
        Err(e) => return store_problem(&e),
    };
    let rules = match r.store.rules(&state.hub).await {
        Ok(x) => x,
        Err(e) => return store_problem(&e),
    };
    // The people as the rehearsal desk last reported them, read fresh.
    r.poll_once(labels(&state.hub)).await;
    let people: Vec<crate::desk::DeskPerson> = r
        .lock()
        .desk
        .as_ref()
        .map(|d| d.people.clone())
        .unwrap_or_default();
    if people.is_empty() {
        return problem(
            StatusCode::CONFLICT,
            "Nobody is in the rehearsal studio to move. Invite them to the live studio instead, or wait for them to join.",
        );
    }
    let host = session
        .presenter_account
        .map(crate::steps::own_identity)
        .filter(|h| people.iter().any(|p| &p.identity == h));
    let standby = model::standby_until_ms(airing.0, rules.grace_s);
    let mut opening = Opening::from_people(
        airing.0,
        standby,
        host.as_deref(),
        people.iter().map(|p| model::Switches {
            identity: &p.identity,
            present: p.present,
            on: p.on_air,
            mute: p.mute,
        }),
    );
    opening.airing_end_ms = Some(airing.1);
    let moved: Vec<String> = people.iter().map(|p| p.identity.clone()).collect();
    // 1. Written before anything else, so a restart in the middle cannot lose it.
    match r.store.record_move(session.id, &moved, &opening).await {
        Ok(true) => {}
        Ok(false) => {
            return problem(
                StatusCode::CONFLICT,
                "The rehearsal studio is not open. Open it for a show first.",
            );
        }
        Err(e) => return store_problem(&e),
    }
    // A move for the same show replaces an earlier one's opening.
    if let Err(e) = r
        .store
        .clear_other_openings(session.show_id, session.id, now)
        .await
    {
        tracing::warn!(error = %e, "cannot clear an earlier move's opening for the same show");
    }
    // 2 and 3: each page is sent its live pass on its event stream, from this move.
    r.keep_move(Session {
        moved_at_ms: Some(now),
        moved: moved.clone(),
        opening: Some(opening.clone()),
        ended: Some(Ended::Moved),
        ended_at_ms: Some(now),
        ..session.clone()
    });
    // 4. The session ends: its rehearsal links buy nothing more. Never by identity (a live pass
    // made in the same second for the same identity would be refused too).
    if let Err(e) = r.finish(&session, Ended::Moved).await {
        return store_problem(&e);
    }
    tracing::info!(show = %session.show_slug, by = %actor.label(), people = moved.len(), host = ?host, airing_start_ms = airing.0, "moved to the live studio, standing by");
    // Anyone whose page has not left the rehearsal room after a while is taken out of it.
    if let Some(room) = state
        .hub
        .room
        .as_ref()
        .map(|r| (r.api_url.clone(), r.keys.clone()))
    {
        let (api_url, keys) = room;
        let rehearsal_room = r.room.clone();
        let ids = moved.clone();
        tokio::spawn(async move {
            tokio::time::sleep(MOVED_OUT_AFTER).await;
            let admin = crate::hub::RoomAdmin {
                api_url,
                room: rehearsal_room.clone(),
                keys,
            };
            let still = admin.list_in(&rehearsal_room).await.unwrap_or_default();
            for id in ids.iter().filter(|id| still.contains(id)) {
                if let Err(e) = admin.remove_in(&rehearsal_room, id).await {
                    tracing::warn!(identity = %id, error = %format!("{e:#}"), "cannot take a moved person out of the rehearsal room");
                }
            }
        });
    }
    Json(MoveResult {
        moved,
        opening,
        airing_start_ms: airing.0,
    })
    .into_response()
}

// --- the opening state at the live desk -------------------------------------------------------

/// Apply the openings that are due (section 1.6, *The handover*): every second from the post's
/// look, at once after a press puts someone on air, when the air is taken back from automation
/// (`moment` [`model::Moment::TakeAir`] for the show on air by the schedule, `show`), and when
/// the schedule brings a show on ([`model::Moment::BringOn`]). Each is applied once: the muted
/// first, then the others who were on, with the rules' fade; people not present are skipped.
/// Returns how many were applied.
pub async fn apply_openings(state: &AppState, moment: model::Moment, show: Option<Uuid>) -> usize {
    let Some(r) = state.hub.rehearsal.get() else {
        return 0;
    };
    if r.moves().is_empty() {
        return 0;
    }
    let _held = r.applying.lock().await;
    let mut applied = 0;
    for s in r.moves() {
        let Some(mut opening) = s.opening.clone() else {
            continue;
        };
        let now = now_ms() as i64;
        let Some(desk) = state.hub.desk_status() else {
            return applied;
        };
        let people: Vec<model::AtLiveDesk<'_>> = desk
            .people
            .iter()
            .map(|p| model::AtLiveDesk {
                identity: &p.identity,
                present: p.present,
                on_air: p.on_air,
            })
            .collect();
        let m = if show == Some(s.show_id) {
            moment
        } else {
            model::Moment::Look
        };
        if !model::opening_due(&opening, &s.moved, &people, m, now) {
            continue;
        }
        let acts = model::opening_acts(&opening, &people);
        drop(people);
        let fade_ms = r.store.rules(&state.hub).await.map_or(0, |x| x.fade_ms);
        // Written first: a restart in the middle applies it at most once.
        opening.applied_at_ms = Some(now);
        if let Err(e) = r.store.set_opening(s.id, &opening).await {
            tracing::warn!(error = %e, show = %s.show_slug, "cannot write that the opening is applied; trying again at the next look");
            continue;
        }
        r.keep_opening(s.id, opening.clone());
        for id in &acts.mute {
            let cmd = DeskCommand::Mute {
                identity: id.clone(),
                on: true,
            };
            if let Err(e) = state.hub.command(&cmd).await {
                tracing::warn!(identity = %id, error = %e, "the opening: cannot mute them");
            }
        }
        for id in &acts.on {
            if let Err(e) = crate::post::on_air_command(state, id, true, fade_ms).await {
                tracing::warn!(identity = %id, error = %e, "the opening: cannot put them on air");
            }
        }
        tracing::info!(show = %s.show_slug, ?moment, on = ?acts.on, mute = ?acts.mute, "the opening state from rehearsal is applied");
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests;
