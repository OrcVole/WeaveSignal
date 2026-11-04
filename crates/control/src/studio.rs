// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The studio as the studio app sees it: the desk's people in their slots, the screener's
//! callers, the chat's modes and the notices for each surface. Pure state, no I/O: the
//! control plane feeds it the desk's status and the people's commands, and sends its view to
//! the app.
//!
//! **Slots.** The desk's keys are bound to slots 1 to 6 (keymap section 3), and a source keeps
//! its slot, and so its keys, while it is at the desk (desk.md section 3, rule 4). No ADR says
//! who gets which slot; the rule here is: a presenter takes 1, guests 2 and 3, a caller sent to
//! the desk 4 (the keymap's `⌃D ⌃D` drops "the caller on strip 4"), a DJ 5, each falling back
//! to the first free slot; slot 6 is playout. Screeners and producers are not sources and get
//! no slot; a waiting caller is the screener's until sent to the desk.
//!
//! **Callers.** In arrival order, each waiting, being screened, or ready (sent to the desk);
//! on air and cued are the desk's own switches. A caller whose link drops keeps their place
//! for five minutes (the screener's caller card); the presenter can decline a caller sent to
//! the desk, which leaves them ready with DESK BUSY (ADR-0069) until the screener sends them
//! again or the presenter takes them. A caller whose identity was banned before carries
//! BANNED BEFORE with the date and reason (ADR-0069).
//!
//! **Consent to record.** A caller reaches the desk only with their consent on file: the
//! call-in route records it (who, when, which words) before it signs the caller's room token,
//! and the studio keeps it beside the name and topic they typed, for the screener. A caller
//! the desk lists without one (a token from before, or a test's) waits in the queue, cannot be
//! sent to the desk, and the control plane refuses to put them on air or in cue.
//!
//! **Chat modes.** The chat is its own service; its modes live here so the screener's keys
//! work: slow, hold new accounts, links for trusted chatters, and influx, which turns on five
//! settings at once and restores the earlier ones when turned off (ADR-0068, D35).

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use weavesignal_proto::Role;

use crate::desk::{DeskDelay, DeskPerson, DeskStatus, PlayoutLink, StationMode};

/// How long a dropped caller's place is held.
pub const PLACE_HELD_MS: u64 = 5 * 60 * 1000;
/// How long a call-in's record is kept after the caller has left the queue: a day, the
/// longest a pass lives. The consent log itself is kept by the control plane's log and the
/// database, not here; after a restart it is read back from the database
/// ([`crate::retention`]), and the retention sweeper deletes it when its period ends.
const CALL_IN_KEEP_MS: u64 = 24 * 3600 * 1000;
/// How many notices are kept.
const NOTICES: usize = 12;
/// The slots a source can take; slot 6 is playout's.
pub const SOURCE_SLOTS: usize = 5;

/// One row of the "coming up" rundown: a setting (`studio.rundown`); the schedule does not fill it yet.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RundownItem {
    /// The time it starts, `HH:MM` or `HH:MM:SS`, in the station's time.
    pub at: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// A hard post: the clock back-times to the first one still ahead.
    #[serde(default)]
    pub hard_post: bool,
    /// The cart key it needs, if any (`F1` to `F6`).
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallerState {
    Waiting,
    Screening,
    Ready,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Declined {
    pub at_ms: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Ban {
    /// The day it was made, as the screener reads it (`2026-09-12`).
    pub date: String,
    pub reason: String,
}

/// Who gave the consent: the caller on the player page, or the station on their behalf (an
/// admin's token request, for a test or a consent taken another way).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConsentBy {
    Caller,
    Station,
}

/// A caller's consent to record: when, to which words, and who gave it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Consent {
    pub at_ms: u64,
    /// The words' version: the first 16 hex digits of their SHA-256.
    pub version: String,
    pub by: ConsentBy,
    /// The words agreed to, as shown.
    #[serde(skip)]
    pub text: String,
}

/// What a caller gave on the player page before they could call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallIn {
    pub identity: String,
    pub name: String,
    /// The one line they typed: what they want to say.
    pub topic: String,
    pub consent: Consent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Caller {
    identity: String,
    name: String,
    arrived_ms: u64,
    state: CallerState,
    since_ms: u64,
    line: String,
    note: String,
    name_on_air: String,
    declined: Option<Declined>,
    /// When their link dropped, while it is down.
    dropped_ms: Option<u64>,
    /// The presenter has taken them (cued or on air) since they were last sent.
    taken: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ChatModes {
    pub slow: bool,
    pub hold_new: bool,
    pub links_trusted: bool,
    pub accounts_only: bool,
    pub min_age_24h: bool,
}

// ---- M6, the chat: the modes start as ADR-0036 says ----
// The chat service (crates/chat) follows these modes from the studio view; links for trusted
// chatters only are on from the start, slow mode and the hold off but one key away.
impl Default for ChatModes {
    fn default() -> Self {
        ChatModes {
            slow: false,
            hold_new: false,
            links_trusted: true,
            accounts_only: false,
            min_age_24h: false,
        }
    }
}
// ---- end M6 ----

impl ChatModes {
    /// Influx: accounts only, a 24-hour age, slow mode, the hold for new chatters, links for
    /// trusted chatters only (D35).
    pub const INFLUX: ChatModes = ChatModes {
        slow: true,
        hold_new: true,
        links_trusted: true,
        accounts_only: true,
        min_age_24h: true,
    };
}

/// A room-wide chat mode the screener switches with one key.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Slow,
    HoldNew,
    LinksTrusted,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Desk,
    Screener,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Good,
    Warning,
    Serious,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Notice {
    pub id: u64,
    pub at_ms: u64,
    pub surface: Surface,
    pub tone: Tone,
    /// What it is about (`caller.ready`, `caller.declined`, ...), for the app to pick keys.
    pub kind: String,
    /// The person it is about, if any.
    pub subject: Option<String>,
    pub title: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StudioError {
    #[error("`{0}` is not a caller in the queue")]
    NoCaller(String),
    #[error("{0}")]
    NotNow(String),
}

/// The studio's own state.
#[derive(Clone, Debug, Default)]
pub struct Studio {
    show: String,
    rundown: Vec<RundownItem>,
    slots: [Option<String>; SOURCE_SLOTS],
    callers: Vec<Caller>,
    bans: BTreeMap<String, Ban>,
    modes: ChatModes,
    before_influx: Option<ChatModes>,
    influx_since: Option<u64>,
    notices: VecDeque<Notice>,
    next_notice: u64,
    /// What callers gave before calling, by identity: name, topic, consent.
    lobby: BTreeMap<String, CallIn>,
    /// How far behind listeners hear, as set (ADR-0103), without the profanity delay.
    listener_delay_ms: u64,
    /// Listeners connected now, from the listener process; `None` until counted, and while it
    /// does not answer (ADR-0046).
    listeners: Option<u32>,
    /// Whether the station has edges or a CDN configured (`steering.edges`, ADR-0017).
    edges: bool,
    /// The station's mode, from the playout settings (ADR-0045); `None` until read.
    mode: Option<StationMode>,
    /// Callers the screener dropped, and when: the desk still lists them (away) for its grace,
    /// and they must not come back into the queue from that; a caller who calls in again later
    /// joins at the back.
    dropped: BTreeMap<String, u64>,
    /// The listener stream is playing WeaveSignal's shipped default loop, because the station has
    /// no fallback files of its own yet (ADR-0130).
    default_fallback: bool,
}

/// How long a dropped caller is not taken back from the desk's list, whatever it says (its
/// status can be a poll behind the removal).
const DROP_SETTLE_MS: u64 = 5_000;

// ---- the view the app gets ----

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct View {
    pub station: String,
    pub now_ms: u64,
    pub show: ShowView,
    pub desk: DeskView,
    pub seats: Seats,
    /// Slots 1 to 5 that hold a source, in slot order.
    pub sources: Vec<Source>,
    pub callers: Vec<CallerView>,
    /// No caller sits declined: the screener's status bar says the desk takes callers.
    pub desk_taking_callers: bool,
    pub chat: ChatView,
    pub notices: Vec<Notice>,
    /// The cart wall: what is sounding, playout's link, FILE MISSING, the catalogue's version.
    pub carts: crate::carts::CartsView,
    /// How far behind the studio listeners hear.
    pub listener_delay: ListenerDelay,
    /// Listeners are hearing the default loop WeaveSignal ships, not the station's own fallback
    /// (ADR-0130): the desk's playout panel says so until the station adds fallback files.
    pub default_fallback: bool,
    /// Who is listening and what carries them (the desk's `desk.audience`).
    pub audience: AudienceView,
    /// The station's mode (ADR-0045): `live`, the desk first with automation on dead air, or
    /// `auto`, automation holding the air, where the master lamp is to say AUTO (ADR-0183).
    /// The playout setting the control plane holds playout to; `None` until it has been read
    /// (and on a station without the playout layer).
    pub mode: Option<StationMode>,
}

/// The audience as the app shows it, and the figures its CDN warning (ADR-0017) reads.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AudienceView {
    /// Listeners connected to the station's stream now; `None` while not counted.
    pub listeners: Option<u32>,
    /// The station has edges or a CDN configured (steering has a pathway beside the origin).
    pub edges: bool,
}

/// The listener delay the surfaces show: a setting until it is measured (ADR-0103), plus the
/// profanity delay's current length (ADR-0165).
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ListenerDelay {
    /// The `studio.listener_delay_ms` setting plus how much the desk's profanity delay holds now.
    pub ms: u64,
    /// Always false in this build: nothing measures it yet, so the app labels it a setting.
    pub measured: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
#[schema(as = StudioShow)]
pub struct ShowView {
    pub title: String,
    pub rundown: Vec<RundownItem>,
}

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskView {
    /// The desk answers on its control socket.
    pub reachable: bool,
    pub error: Option<String>,
    pub playout: PlayoutLink,
    /// The profanity delay (the desk's `desk.delay`); `None` while the desk does not answer.
    pub delay: Option<DelayView>,
    /// The picture (the desk's `desk.picture`, F7): `None` with video off.
    pub picture: Option<crate::desk::DeskVideo>,
}

/// The profanity delay as the desk's delay panel shows it.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct DelayView {
    pub delay_ms: u32,
    pub target_ms: u32,
    pub rebuilding: bool,
    pub dumps: u64,
    /// When the last dump was, on the station's clock, to the second (so the view does not
    /// change with every look at the desk).
    pub dumped_at_ms: Option<u64>,
}

impl DelayView {
    fn from_desk(d: &DeskDelay, now_ms: u64) -> Self {
        DelayView {
            delay_ms: d.delay_ms / 100 * 100,
            target_ms: d.target_ms,
            rebuilding: d.rebuilding,
            dumps: d.dumps,
            dumped_at_ms: d
                .dumped_ms_ago
                .map(|ago| now_ms.saturating_sub(ago) / 1000 * 1000),
        }
    }
}

/// Who sits where: the identities the studio app acts as on each surface, until accounts.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Seats {
    pub presenter: Option<String>,
    pub screener: Option<String>,
    /// Their names, as the room gives them (may be empty).
    pub presenter_name: String,
    pub screener_name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct Source {
    pub slot: usize,
    #[serde(flatten)]
    pub person: DeskPerson,
    /// For a caller: what the screener sent with them.
    pub caller: Option<CallerNote>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct CallerNote {
    pub line: String,
    pub note: String,
    pub name_on_air: String,
    pub sent: bool,
    pub taken: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct CallerView {
    pub identity: String,
    pub name: String,
    /// 1, 2, 3...: their place in the queue, the screener's jump key.
    pub ordinal: usize,
    pub state: CallerState,
    pub since_ms: u64,
    pub arrived_ms: u64,
    pub line: String,
    pub note: String,
    pub name_on_air: String,
    pub declined: Option<Declined>,
    pub banned_before: Option<Ban>,
    /// What they typed on the player page.
    pub topic: String,
    /// Their consent to record; `None` means they cannot go to the desk.
    pub consent: Option<Consent>,
    pub dropped_ms: Option<u64>,
    pub slot: Option<usize>,
    pub on_air: bool,
    pub cue: bool,
    pub linked: bool,
    pub present: bool,
    pub lufs: Option<f32>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ChatView {
    #[serde(flatten)]
    pub modes: ChatModes,
    pub influx: bool,
    pub influx_since_ms: Option<u64>,
}

fn preferred(role: Role) -> &'static [usize] {
    match role {
        Role::Presenter => &[1, 2, 3, 4, 5],
        Role::Guest => &[2, 3, 1, 4, 5],
        Role::Caller => &[4, 3, 2, 5, 1],
        Role::Dj => &[5, 1, 2, 3, 4],
        Role::Screener | Role::Producer | Role::Listener => &[],
    }
}

impl Studio {
    pub fn new(show: &str, rundown: Vec<RundownItem>) -> Self {
        Studio {
            show: show.to_owned(),
            rundown,
            listener_delay_ms: 15_000,
            ..Studio::default()
        }
    }

    /// Set how far behind listeners hear (the `studio.listener_delay_ms` setting).
    pub fn set_listener_delay(&mut self, ms: u64) {
        self.listener_delay_ms = ms;
    }

    /// Whether the default loop is on air; true when this changed it.
    pub fn set_default_fallback(&mut self, on: bool) -> bool {
        let changed = self.default_fallback != on;
        self.default_fallback = on;
        changed
    }

    /// Whether the station has edges or a CDN configured.
    pub fn set_edges(&mut self, on: bool) {
        self.edges = on;
    }

    /// The station's mode, as the playout settings say. Returns whether it changed.
    pub fn set_station_mode(&mut self, mode: StationMode) -> bool {
        self.mode.replace(mode) != Some(mode)
    }

    /// The station's mode; `None` until the settings have been read.
    pub fn station_mode(&self) -> Option<StationMode> {
        self.mode
    }

    /// Listeners connected now (`None`: not counted). Returns whether the figure changed.
    pub fn set_listeners(&mut self, n: Option<u32>) -> bool {
        std::mem::replace(&mut self.listeners, n) != n
    }

    /// A notice from playout (dead air, the logger) for the desk.
    pub fn playout_notice(
        &mut self,
        now: u64,
        tone: Tone,
        kind: &str,
        title: String,
        message: String,
    ) {
        self.notice(now, (Surface::Desk, tone, kind), None, title, message);
    }

    /// Keep a notice for a surface: `what` is its surface, tone and kind.
    fn notice(
        &mut self,
        now: u64,
        what: (Surface, Tone, &str),
        subject: Option<&str>,
        title: String,
        message: String,
    ) {
        let (surface, tone, kind) = what;
        self.next_notice += 1;
        self.notices.push_back(Notice {
            id: self.next_notice,
            at_ms: now,
            surface,
            tone,
            kind: kind.to_owned(),
            subject: subject.map(str::to_owned),
            title,
            message,
        });
        while self.notices.len() > NOTICES {
            self.notices.pop_front();
        }
    }

    fn caller_mut(&mut self, id: &str) -> Result<&mut Caller, StudioError> {
        self.callers
            .iter_mut()
            .find(|c| c.identity == id)
            .ok_or_else(|| StudioError::NoCaller(id.to_owned()))
    }

    fn short_name(&self, id: &str) -> String {
        self.callers
            .iter()
            .find(|c| c.identity == id)
            .map(|c| {
                if !c.name_on_air.is_empty() {
                    c.name_on_air.clone()
                } else if !c.name.is_empty() {
                    c.name
                        .split_whitespace()
                        .next()
                        .unwrap_or(&c.name)
                        .to_owned()
                } else {
                    id.to_owned()
                }
            })
            .unwrap_or_else(|| id.to_owned())
    }

    /// Bring the studio in line with what the desk reports at `now` (epoch ms).
    pub fn sync(&mut self, desk: &DeskStatus, now: u64) {
        let by_id: BTreeMap<&str, &DeskPerson> = desk
            .people
            .iter()
            .map(|p| (p.identity.as_str(), p))
            .collect();

        // Callers: newcomers join the queue; a dropped link holds the place; a return clears it.
        // One the screener dropped stays out until they are in the room again, later.
        self.dropped.retain(|id, at| {
            by_id.contains_key(id.as_str()) || now.saturating_sub(*at) < DROP_SETTLE_MS
        });
        for p in desk.people.iter().filter(|p| p.role == Role::Caller) {
            if let Some(&at) = self.dropped.get(&p.identity) {
                if !p.present || now.saturating_sub(at) < DROP_SETTLE_MS {
                    continue;
                }
                self.dropped.remove(&p.identity);
            }
            let given = self.lobby.get(&p.identity).map(|c| c.name.clone());
            match self.callers.iter_mut().find(|c| c.identity == p.identity) {
                None => self.callers.push(Caller {
                    identity: p.identity.clone(),
                    name: if p.name.is_empty() {
                        given.unwrap_or_default()
                    } else {
                        p.name.clone()
                    },
                    arrived_ms: now,
                    state: CallerState::Waiting,
                    since_ms: now,
                    line: String::new(),
                    note: String::new(),
                    name_on_air: String::new(),
                    declined: None,
                    dropped_ms: if p.present { None } else { Some(now) },
                    taken: false,
                }),
                Some(c) => {
                    if c.name != p.name && !p.name.is_empty() {
                        c.name = p.name.clone();
                    }
                    if p.present {
                        c.dropped_ms = None;
                    }
                }
            }
        }
        let mut dropped_now = Vec::new();
        for c in &mut self.callers {
            let here = by_id.get(c.identity.as_str());
            let present = here.is_some_and(|p| p.present);
            if !present && c.dropped_ms.is_none() {
                c.dropped_ms = Some(now);
                dropped_now.push(c.identity.clone());
            }
        }
        for id in dropped_now {
            let name = self.short_name(&id);
            self.notice(
                now,
                (Surface::Screener, Tone::Warning, "caller.dropped"),
                Some(&id),
                format!("{name}'s link dropped"),
                "Their place in the queue is held for 5 min; if they call in again they come back where they were.".to_owned(),
            );
        }
        self.callers.retain(|c| {
            c.dropped_ms
                .is_none_or(|t| now.saturating_sub(t) < PLACE_HELD_MS)
        });
        let queued: Vec<String> = self.callers.iter().map(|c| c.identity.clone()).collect();
        self.lobby.retain(|id, c| {
            queued.contains(id) || now.saturating_sub(c.consent.at_ms) < CALL_IN_KEEP_MS
        });

        // A ready caller the presenter has cued or put on air has been taken.
        let mut taken_now = Vec::new();
        for c in &mut self.callers {
            let live = by_id
                .get(c.identity.as_str())
                .is_some_and(|p| p.cue || p.on_air);
            if c.state == CallerState::Ready && live && !c.taken {
                c.taken = true;
                c.declined = None;
                taken_now.push(c.identity.clone());
            }
        }

        // Slots: release those whose source has gone or is no longer the desk's.
        for slot in &mut self.slots {
            let keep = slot.as_deref().is_some_and(|id| {
                by_id.get(id).is_some_and(|p| match p.role {
                    Role::Caller => {
                        self.callers.iter().any(|c| {
                            c.identity == id
                                && c.state == CallerState::Ready
                                && c.declined.is_none()
                                && c.dropped_ms.is_none()
                        }) || p.on_air
                    }
                    r => !preferred(r).is_empty(),
                })
            });
            if !keep {
                *slot = None;
            }
        }
        // ...and seat newcomers by the rule above, in the desk's order.
        for p in &desk.people {
            if self.slots.iter().flatten().any(|id| *id == p.identity) {
                continue;
            }
            let eligible = match p.role {
                Role::Caller => {
                    p.on_air
                        || self.callers.iter().any(|c| {
                            c.identity == p.identity
                                && c.state == CallerState::Ready
                                && c.declined.is_none()
                                && c.dropped_ms.is_none()
                        })
                }
                r => !preferred(r).is_empty(),
            };
            if !eligible {
                continue;
            }
            if let Some(&n) = preferred(p.role)
                .iter()
                .find(|&&n| self.slots[n - 1].is_none())
            {
                self.slots[n - 1] = Some(p.identity.clone());
            }
        }

        for id in taken_now {
            let name = self.short_name(&id);
            let slot = self.slot_of(&id);
            self.notice(
                now,
                (Surface::Screener, Tone::Good, "caller.taken"),
                Some(&id),
                format!("{name} is with the desk"),
                match slot {
                    Some(n) => {
                        format!("The presenter took them on strip {n}. Your note went with them.")
                    }
                    None => "The presenter took them. Your note went with them.".to_owned(),
                },
            );
        }
    }

    pub fn slot_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_deref() == Some(id))
            .map(|i| i + 1)
    }

    /// The identity in a slot.
    pub fn in_slot(&self, slot: usize) -> Option<&str> {
        self.slots.get(slot.checked_sub(1)?)?.as_deref()
    }

    // ---- the screener's actions ----

    /// Open the screening conversation with a caller; whoever was being screened waits again.
    pub fn screen(&mut self, id: &str, now: u64) -> Result<(), StudioError> {
        self.caller_mut(id)?;
        for c in &mut self.callers {
            if c.state == CallerState::Screening && c.identity != id {
                c.state = CallerState::Waiting;
                c.since_ms = now;
            }
        }
        let c = self.caller_mut(id)?;
        if c.state == CallerState::Waiting {
            c.state = CallerState::Screening;
            c.since_ms = now;
        }
        Ok(())
    }

    /// Keep what a caller gave before calling: their consent, name and topic.
    pub fn call_in(&mut self, call: CallIn) {
        self.lobby.insert(call.identity.clone(), call);
    }

    /// What a caller gave before calling, if anything is on file.
    pub fn call_in_of(&self, id: &str) -> Option<&CallIn> {
        self.lobby.get(id)
    }

    /// Whether the person may be put on air or in cue: anyone but a caller, and a caller with
    /// their consent to record on file.
    pub fn may_reach_desk(&self, id: &str, role: Role) -> bool {
        role != Role::Caller || self.lobby.contains_key(id)
    }

    /// Send a caller to the desk (READY), or send them again after a decline.
    pub fn send(&mut self, id: &str, now: u64) -> Result<(), StudioError> {
        self.caller_mut(id)?;
        if !self.lobby.contains_key(id) {
            return Err(StudioError::NotNow(format!(
                "`{id}` has no consent to record on file; a caller cannot go to the desk without it"
            )));
        }
        let c = self.caller_mut(id)?;
        if c.dropped_ms.is_some() {
            return Err(StudioError::NotNow(format!(
                "`{id}`'s link is down; they cannot go to the desk"
            )));
        }
        c.state = CallerState::Ready;
        c.since_ms = now;
        c.declined = None;
        c.taken = false;
        let line = c.line.clone();
        let name = self.short_name(id);
        self.notice(
            now,
            (Surface::Desk, Tone::Good, "caller.ready"),
            Some(id),
            format!("{name} is ready for you"),
            if line.is_empty() {
                "Sent by the screener.".to_owned()
            } else {
                line
            },
        );
        Ok(())
    }

    /// Back to the queue from screening or the desk.
    pub fn back(&mut self, id: &str, on_air: bool, now: u64) -> Result<(), StudioError> {
        if on_air {
            return Err(StudioError::NotNow(format!(
                "`{id}` is on air; the presenter takes them off first"
            )));
        }
        let c = self.caller_mut(id)?;
        c.state = CallerState::Waiting;
        c.since_ms = now;
        c.declined = None;
        c.taken = false;
        Ok(())
    }

    /// The presenter declines a caller sent to the desk: they stay ready, with DESK BUSY.
    pub fn decline(&mut self, id: &str, reason: &str, now: u64) -> Result<(), StudioError> {
        let c = self.caller_mut(id)?;
        if c.state != CallerState::Ready {
            return Err(StudioError::NotNow(format!(
                "`{id}` was not sent to the desk"
            )));
        }
        c.declined = Some(Declined {
            at_ms: now,
            reason: reason.to_owned(),
        });
        c.taken = false;
        let name = self.short_name(id);
        self.notice(
            now,
            (Surface::Screener, Tone::Warning, "caller.declined"),
            Some(id),
            "The desk is not taking callers".to_owned(),
            format!(
                "The presenter declined {name}{}. {name} stays ready; send again when the desk is free.",
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(": \u{201c}{reason}\u{201d}")
                }
            ),
        );
        Ok(())
    }

    /// What the screener wrote for the presenter.
    pub fn notes(
        &mut self,
        id: &str,
        line: Option<&str>,
        note: Option<&str>,
        name_on_air: Option<&str>,
    ) -> Result<(), StudioError> {
        let c = self.caller_mut(id)?;
        for (field, value) in [
            (&mut c.line, line),
            (&mut c.note, note),
            (&mut c.name_on_air, name_on_air),
        ] {
            if let Some(v) = value {
                *field = v.chars().take(500).collect();
            }
        }
        Ok(())
    }

    /// Let a dropped caller's held place go.
    pub fn release(&mut self, id: &str) -> Result<(), StudioError> {
        let c = self.caller_mut(id)?;
        if c.dropped_ms.is_none() {
            return Err(StudioError::NotNow(format!(
                "`{id}` is still on the line; drop them instead"
            )));
        }
        self.callers.retain(|c| c.identity != id);
        Ok(())
    }

    /// The screener dropped the caller: they leave the queue and their slot.
    pub fn forget(&mut self, id: &str, now: u64) {
        self.dropped.insert(id.to_owned(), now);
        self.callers.retain(|c| c.identity != id);
        for s in &mut self.slots {
            if s.as_deref() == Some(id) {
                *s = None;
            }
        }
    }

    pub fn ban(&mut self, id: &str, ban: Ban) {
        self.bans.insert(id.to_owned(), ban);
    }

    // ---- chat modes ----

    pub fn set_mode(&mut self, mode: Mode, on: bool) {
        let m = &mut self.modes;
        match mode {
            Mode::Slow => m.slow = on,
            Mode::HoldNew => m.hold_new = on,
            Mode::LinksTrusted => m.links_trusted = on,
        }
    }

    /// Influx on: five settings at once, the earlier ones kept for the undo; off: restored.
    pub fn influx(&mut self, on: bool, now: u64) {
        match (on, self.before_influx) {
            (true, None) => {
                self.before_influx = Some(self.modes);
                self.modes = ChatModes::INFLUX;
                self.influx_since = Some(now);
                self.notice(
                    now,
                    (Surface::Screener, Tone::Warning, "chat.influx"),
                    None,
                    "Influx mode on".to_owned(),
                    "One press, five settings: accounts only, 24 h old, slow mode 10 s, every new chatter held, links for trusted chatters only. The undo is the same key.".to_owned(),
                );
            }
            (false, Some(before)) => {
                self.modes = before;
                self.before_influx = None;
                self.influx_since = None;
                self.notice(
                    now,
                    (Surface::Screener, Tone::Good, "chat.influx-off"),
                    None,
                    "Influx mode off".to_owned(),
                    "The chat's settings are back as they were before.".to_owned(),
                );
            }
            _ => {}
        }
    }

    // ---- the view ----

    pub fn view(
        &self,
        station: &str,
        desk: Option<&DeskStatus>,
        desk_error: Option<String>,
        now: u64,
    ) -> View {
        let empty = DeskStatus::default();
        let d = desk.unwrap_or(&empty);
        let person = |id: &str| d.people.iter().find(|p| p.identity == id);
        let sources = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let id = id.as_deref()?;
                let p = person(id)?.clone();
                let caller = self
                    .callers
                    .iter()
                    .find(|c| c.identity == id)
                    .map(|c| CallerNote {
                        line: c.line.clone(),
                        note: c.note.clone(),
                        name_on_air: c.name_on_air.clone(),
                        sent: c.state == CallerState::Ready,
                        taken: c.taken,
                    });
                Some(Source {
                    slot: i + 1,
                    person: p,
                    caller,
                })
            })
            .collect();
        let callers = self
            .callers
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let p = person(&c.identity);
                CallerView {
                    identity: c.identity.clone(),
                    name: c.name.clone(),
                    ordinal: i + 1,
                    state: c.state,
                    since_ms: c.since_ms,
                    arrived_ms: c.arrived_ms,
                    line: c.line.clone(),
                    note: c.note.clone(),
                    name_on_air: c.name_on_air.clone(),
                    declined: c.declined.clone(),
                    banned_before: self.bans.get(&c.identity).cloned(),
                    topic: self
                        .lobby
                        .get(&c.identity)
                        .map(|l| l.topic.clone())
                        .unwrap_or_default(),
                    consent: self.lobby.get(&c.identity).map(|l| l.consent.clone()),
                    dropped_ms: c.dropped_ms,
                    slot: self.slot_of(&c.identity),
                    on_air: p.is_some_and(|p| p.on_air),
                    cue: p.is_some_and(|p| p.cue),
                    linked: p.is_some_and(|p| p.linked),
                    present: p.is_some_and(|p| p.present),
                    lufs: p.and_then(|p| p.lufs),
                }
            })
            .collect();
        let seat = |role: Role| {
            let first_slot = self
                .slots
                .iter()
                .flatten()
                .find(|id| person(id).is_some_and(|p| p.role == role));
            first_slot.cloned().or_else(|| {
                d.people
                    .iter()
                    .find(|p| p.role == role && p.present)
                    .map(|p| p.identity.clone())
            })
        };
        View {
            station: station.to_owned(),
            now_ms: now,
            show: ShowView {
                title: self.show.clone(),
                rundown: self.rundown.clone(),
            },
            desk: DeskView {
                reachable: desk.is_some(),
                error: desk_error,
                playout: d.playout,
                delay: d.delay.as_ref().map(|x| DelayView::from_desk(x, now)),
                picture: d.video.clone(),
            },
            seats: {
                let presenter = seat(Role::Presenter);
                let screener = seat(Role::Screener);
                let name_of = |id: &Option<String>| {
                    id.as_deref()
                        .and_then(person)
                        .map(|p| p.name.clone())
                        .unwrap_or_default()
                };
                Seats {
                    presenter_name: name_of(&presenter),
                    screener_name: name_of(&screener),
                    presenter,
                    screener,
                }
            },
            sources,
            callers,
            desk_taking_callers: !self.callers.iter().any(|c| c.declined.is_some()),
            chat: ChatView {
                modes: self.modes,
                influx: self.before_influx.is_some(),
                influx_since_ms: self.influx_since,
            },
            notices: self.notices.iter().cloned().collect(),
            carts: crate::carts::CartsView::default(),
            // The setting is the path beyond the desk; the profanity delay's current length is
            // added (ADR-0165), so a dump or a rebuild moves the figure too.
            listener_delay: ListenerDelay {
                ms: self.listener_delay_ms
                    + d.delay
                        .as_ref()
                        .map_or(0, |x| u64::from(x.delay_ms / 100 * 100)),
                measured: false,
            },
            default_fallback: self.default_fallback,
            audience: AudienceView {
                listeners: self.listeners,
                edges: self.edges,
            },
            mode: self.mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_carries_the_audience_for_the_apps_cdn_warning() {
        let mut s = Studio::default();
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.audience,
            AudienceView {
                listeners: None,
                edges: false
            }
        );
        assert!(s.set_listeners(Some(612)), "a new figure is a change");
        assert!(!s.set_listeners(Some(612)), "the same figure is not");
        s.set_edges(true);
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.audience,
            AudienceView {
                listeners: Some(612),
                edges: true
            }
        );
        assert!(s.set_listeners(None), "not counted is a change too");
        assert_eq!(s.view("", None, None, 0).audience.listeners, None);
        let json = serde_json::to_value(s.view("", None, None, 0)).unwrap();
        assert_eq!(
            json["audience"],
            serde_json::json!({ "listeners": null, "edges": true })
        );
    }

    #[test]
    fn the_view_carries_the_stations_mode_for_the_auto_lamp() {
        let mut s = Studio::default();
        assert_eq!(s.view("", None, None, 0).mode, None, "not read yet");
        assert_eq!(
            serde_json::to_value(s.view("", None, None, 0)).unwrap()["mode"],
            serde_json::Value::Null
        );
        assert!(
            s.set_station_mode(StationMode::Live),
            "the first reading is a change"
        );
        assert!(
            !s.set_station_mode(StationMode::Live),
            "the same mode is not"
        );
        assert!(s.set_station_mode(StationMode::Auto));
        assert_eq!(s.station_mode(), Some(StationMode::Auto));
        let v = s.view("", None, None, 0);
        assert_eq!(v.mode, Some(StationMode::Auto));
        assert_eq!(serde_json::to_value(v).unwrap()["mode"], "auto");
    }

    fn person(id: &str, role: Role) -> DeskPerson {
        DeskPerson {
            identity: id.into(),
            name: String::new(),
            role,
            on_air: role.on_air_at_join(),
            mute: false,
            cue: false,
            talk: false,
            talk_to: None,
            present: true,
            away_ms: None,
            linked: true,
            lufs: None,
            lufs_peak: None,
        }
    }

    fn desk(people: Vec<DeskPerson>) -> DeskStatus {
        DeskStatus {
            people,
            playout: PlayoutLink {
                connected: true,
                for_ms: Some(1),
            },
            mode: None,
            delay: None,
            carts: None,
            video: None,
        }
    }

    /// A caller who called from the player page and agreed to be recorded.
    fn consented(s: &mut Studio, id: &str) {
        s.call_in(CallIn {
            identity: id.into(),
            name: format!("{id} from the player page"),
            topic: "the bins".into(),
            consent: Consent {
                at_ms: 0,
                version: "0123456789abcdef".into(),
                by: ConsentBy::Caller,
                text: "Your call may be broadcast".into(),
            },
        });
    }

    fn slots(s: &Studio) -> Vec<Option<&str>> {
        (1..=5).map(|n| s.in_slot(n)).collect()
    }

    #[test]
    fn sources_take_their_slots_and_keep_them() {
        let mut s = Studio::new("The Morning Kettle", vec![]);
        let room = desk(vec![
            person("gregor", Role::Guest),
            person("eilidh", Role::Presenter),
            person("finlay", Role::Dj),
            person("kirsty", Role::Guest),
            person("moira", Role::Screener),
        ]);
        s.sync(&room, 0);
        assert_eq!(
            slots(&s),
            [
                Some("eilidh"),
                Some("gregor"),
                Some("kirsty"),
                None,
                Some("finlay")
            ]
        );
        // Gregor leaves for good; Kirsty keeps slot 3 and her keys.
        s.sync(
            &desk(vec![
                person("eilidh", Role::Presenter),
                person("kirsty", Role::Guest),
                person("finlay", Role::Dj),
            ]),
            1,
        );
        assert_eq!(s.slot_of("kirsty"), Some(3));
        assert_eq!(s.in_slot(2), None);
        let v = s.view("Kettle FM", Some(&room), None, 2);
        assert_eq!(v.seats.presenter.as_deref(), Some("eilidh"));
        assert_eq!(v.seats.screener.as_deref(), Some("moira"));
    }

    #[test]
    fn a_caller_waits_is_screened_sent_and_taken_on_strip_four() {
        let mut s = Studio::new("", vec![]);
        consented(&mut s, "dougal");
        let mut room = vec![
            person("eilidh", Role::Presenter),
            person("dougal", Role::Caller),
            person("iona", Role::Caller),
        ];
        s.sync(&desk(room.clone()), 0);
        let v = s.view("", Some(&desk(room.clone())), None, 0);
        assert_eq!(v.callers.len(), 2);
        assert!(v.callers.iter().all(|c| c.state == CallerState::Waiting));
        assert_eq!(s.slot_of("dougal"), None, "a waiting caller has no slot");

        s.screen("dougal", 10).unwrap();
        s.screen("iona", 11).unwrap();
        let v = s.view("", None, None, 12);
        assert_eq!(
            v.callers[0].state,
            CallerState::Waiting,
            "one screening at a time"
        );
        assert_eq!(v.callers[1].state, CallerState::Screening);

        s.notes(
            "dougal",
            Some("Bin strike"),
            Some("Nervous; go gentle"),
            Some("Dougal"),
        )
        .unwrap();
        s.send("dougal", 20).unwrap();
        s.sync(&desk(room.clone()), 21);
        assert_eq!(s.slot_of("dougal"), Some(4));
        assert!(
            s.notices
                .iter()
                .any(|n| n.kind == "caller.ready" && n.surface == Surface::Desk)
        );

        room[1].cue = true;
        s.sync(&desk(room.clone()), 30);
        let n = s.notices.back().unwrap();
        assert_eq!(
            (n.kind.as_str(), n.surface),
            ("caller.taken", Surface::Screener)
        );
        assert!(n.message.contains("strip 4"));
        let v = s.view("", Some(&desk(room)), None, 31);
        let src = v.sources.iter().find(|x| x.slot == 4).unwrap();
        assert_eq!(src.caller.as_ref().unwrap().note, "Nervous; go gentle");
        assert!(v.callers[0].cue && v.desk_taking_callers);
    }

    #[test]
    fn the_view_says_when_the_default_loop_is_on_the_stream() {
        let mut s = Studio::new("", vec![]);
        assert!(!s.view("", None, None, 0).default_fallback);
        assert!(s.set_default_fallback(true), "turning it on is a change");
        assert!(s.view("", None, None, 1).default_fallback);
        assert!(!s.set_default_fallback(true), "saying it again is not");
        assert!(s.set_default_fallback(false));
        assert!(!s.view("", None, None, 2).default_fallback);
    }

    #[test]
    fn a_decline_is_desk_busy_until_sent_again() {
        let mut s = Studio::new("", vec![]);
        consented(&mut s, "niall");
        let room = desk(vec![
            person("eilidh", Role::Presenter),
            person("niall", Role::Caller),
        ]);
        s.sync(&room, 0);
        assert!(matches!(
            s.decline("niall", "", 1),
            Err(StudioError::NotNow(_))
        ));
        s.send("niall", 1).unwrap();
        s.sync(&room, 2);
        assert_eq!(s.slot_of("niall"), Some(4));
        s.decline("niall", "in a two-way, give me 2 min", 3)
            .unwrap();
        s.sync(&room, 4);
        let v = s.view("", Some(&room), None, 4);
        assert!(!v.desk_taking_callers);
        assert_eq!(
            v.callers[0].state,
            CallerState::Ready,
            "a declined caller stays ready"
        );
        assert!(v.callers[0].declined.is_some());
        assert_eq!(
            s.slot_of("niall"),
            None,
            "a declined caller leaves the desk's strip"
        );
        assert!(s.notices.back().unwrap().message.contains("two-way"));
        s.send("niall", 5).unwrap();
        s.sync(&room, 6);
        assert!(s.view("", Some(&room), None, 6).desk_taking_callers);
        assert_eq!(s.slot_of("niall"), Some(4));
    }

    #[test]
    fn a_dropped_link_holds_the_place_for_five_minutes() {
        let mut s = Studio::new("", vec![]);
        let mut room = vec![person("jessie", Role::Caller), person("iona", Role::Caller)];
        s.sync(&desk(room.clone()), 0);
        room[1].present = false;
        s.sync(&desk(room.clone()), 1_000);
        let v = s.view("", Some(&desk(room.clone())), None, 1_000);
        assert_eq!(v.callers[1].dropped_ms, Some(1_000));
        assert_eq!(s.notices.back().unwrap().kind, "caller.dropped");
        // Back within the hold: same place.
        room[1].present = true;
        s.sync(&desk(room.clone()), 60_000);
        let v = s.view("", None, None, 60_000);
        assert_eq!(
            (v.callers[1].identity.as_str(), v.callers[1].dropped_ms),
            ("iona", None)
        );
        // Gone for good: the place goes after 5 min.
        room.pop();
        s.sync(&desk(room.clone()), 70_000);
        s.sync(&desk(room.clone()), 70_000 + PLACE_HELD_MS);
        assert_eq!(s.view("", None, None, 0).callers.len(), 1);
    }

    #[test]
    fn a_held_place_can_be_let_go_and_a_line_on_air_cannot_go_back() {
        let mut s = Studio::new("", vec![]);
        let mut room = vec![person("iona", Role::Caller)];
        s.sync(&desk(room.clone()), 0);
        assert!(matches!(s.release("iona"), Err(StudioError::NotNow(_))));
        room[0].present = false;
        s.sync(&desk(room.clone()), 1);
        s.release("iona").unwrap();
        assert!(s.view("", None, None, 2).callers.is_empty());
        assert!(matches!(
            s.back("nobody", false, 0),
            Err(StudioError::NoCaller(_))
        ));
        let mut s = Studio::new("", vec![]);
        s.sync(&desk(vec![person("dougal", Role::Caller)]), 0);
        assert!(matches!(
            s.back("dougal", true, 1),
            Err(StudioError::NotNow(_))
        ));
    }

    #[test]
    fn a_dropped_caller_stays_out_while_the_desk_still_lists_them() {
        let mut s = Studio::new("", vec![]);
        let mut room = vec![
            person("tavish", Role::Caller),
            person("jessie", Role::Caller),
        ];
        s.sync(&desk(room.clone()), 0);
        s.forget("tavish", 1_000);
        // The desk's next status is from before the removal: still present.
        s.sync(&desk(room.clone()), 1_200);
        assert_eq!(s.view("", None, None, 0).callers.len(), 1);
        // Then away for the desk's grace: not back in the queue as a dropped link either.
        room[0].present = false;
        s.sync(&desk(room.clone()), 3_000);
        s.sync(&desk(room.clone()), 20_000);
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.callers
                .iter()
                .map(|c| c.identity.as_str())
                .collect::<Vec<_>>(),
            ["jessie"]
        );
        // Calling in again later: at the back of the queue.
        room[0].present = true;
        s.sync(&desk(room), 40_000);
        let v = s.view("", None, None, 0);
        assert_eq!(v.callers[1].identity, "tavish");
        assert_eq!(v.callers[1].arrived_ms, 40_000);
    }

    #[test]
    fn a_caller_without_consent_cannot_go_to_the_desk() {
        let mut s = Studio::new("", vec![]);
        consented(&mut s, "jessie");
        let room = desk(vec![
            person("eilidh", Role::Presenter),
            person("jessie", Role::Caller),
            person("tavish", Role::Caller),
        ]);
        s.sync(&room, 0);
        let v = s.view("", Some(&room), None, 1);
        let jessie = &v.callers[0];
        assert_eq!(jessie.topic, "the bins");
        assert_eq!(
            jessie.consent.as_ref().map(|c| c.by),
            Some(ConsentBy::Caller)
        );
        assert_eq!(jessie.name, "jessie from the player page");
        assert!(v.callers[1].consent.is_none());
        // Waits and is screened like anyone, but is never sent, never seated.
        s.screen("tavish", 2).unwrap();
        let err = s.send("tavish", 3).unwrap_err();
        assert!(err.to_string().contains("consent"), "{err}");
        s.sync(&room, 4);
        assert_eq!(s.slot_of("tavish"), None);
        assert!(!s.may_reach_desk("tavish", Role::Caller));
        assert!(s.may_reach_desk("jessie", Role::Caller));
        assert!(
            s.may_reach_desk("eilidh", Role::Presenter),
            "only callers need it"
        );
        s.send("jessie", 5).unwrap();
        s.sync(&room, 6);
        assert_eq!(s.slot_of("jessie"), Some(4));
    }

    #[test]
    fn banned_before_is_shown_with_its_date_and_reason() {
        let mut s = Studio::new("", vec![]);
        s.ban(
            "tavish",
            Ban {
                date: "2026-09-12".into(),
                reason: "Abuse on air".into(),
            },
        );
        s.sync(&desk(vec![person("tavish", Role::Caller)]), 0);
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.callers[0].banned_before.as_ref().unwrap().date,
            "2026-09-12"
        );
    }

    #[test]
    fn influx_is_one_press_and_its_undo_restores_what_was_there() {
        let mut s = Studio::new("", vec![]);
        s.set_mode(Mode::LinksTrusted, true);
        s.influx(true, 5);
        let v = s.view("", None, None, 6);
        assert!(v.chat.influx && v.chat.modes == ChatModes::INFLUX);
        assert_eq!(v.chat.influx_since_ms, Some(5));
        s.influx(true, 7); // a second "on" keeps the first undo
        s.influx(false, 8);
        let v = s.view("", None, None, 9);
        assert!(!v.chat.influx);
        assert_eq!(
            v.chat.modes,
            ChatModes {
                links_trusted: true,
                ..ChatModes::default()
            }
        );
    }

    #[test]
    fn an_unreachable_desk_is_said() {
        let s = Studio::new("", vec![]);
        let v = s.view("", None, Some("no answer".into()), 0);
        assert!(!v.desk.reachable && !v.desk.playout.connected);
        assert_eq!(v.desk.error.as_deref(), Some("no answer"));
    }
}
