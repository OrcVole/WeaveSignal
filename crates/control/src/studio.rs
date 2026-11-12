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
use uuid::Uuid;
use weavesignal_model::shows::Step;
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
    /// Those listeners by mount, the busiest first; empty while not counted.
    mounts: Vec<MountCount>,
    /// Why the count is missing, in words, while `listeners` is `None`.
    audience_error: Option<String>,
    /// How much of playout's feed the listener process holds now, in ms: the part of the
    /// listener delay that is read live; `None` while it does not answer.
    feed_buffer_ms: Option<u64>,
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
    /// The station's step (ADR-0271) and where it comes from.
    step: StepState,
    /// What waits for a moderator in the chat (ADR-0274); `None` until counted.
    chat_counts: Option<weavesignal_model::chat::ModerationCounts>,
    /// The picture mode the desk had before the slate was held below Full (the desk's
    /// `desk.picture` mode as it reports it), to put back when the step returns to Full.
    held_picture: Option<serde_json::Value>,
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
    /// The station's beginner step (ADR-0271): what the studio shows and what is switched off.
    pub step: StepView,
    /// The station's mode (ADR-0045): `live`, the desk first with automation on dead air, or
    /// `auto`, automation holding the air, where the master lamp is to say AUTO (ADR-0183).
    /// The playout setting the control plane holds playout to; `None` until it has been read
    /// (and on a station without the playout layer).
    pub mode: Option<StationMode>,
}

/// How many listeners one mount of the listener process has now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct MountCount {
    /// The mount's path as the listener process names it, such as `/programme.opus`.
    pub mount: String,
    pub listeners: u32,
}

/// The audience as the app shows it, and the figures its CDN warning (ADR-0017) reads.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AudienceView {
    /// Listeners connected to the station's stream now, every mount together; `None` while
    /// not counted (see `error`).
    pub listeners: Option<u32>,
    /// The same by mount, most listeners first; empty while not counted.
    pub by_mount: Vec<MountCount>,
    /// Why there is no count, in words the studio can show: the listener process does not
    /// answer, or the control plane has no playout layer to ask it. `None` while counted.
    pub error: Option<String>,
    /// The station has edges or a CDN configured: steering has a pathway beside the origin, or
    /// `join.hls_url` points the player at another address.
    pub edges: bool,
}

/// The listener delay the surfaces show (ADR-0103, ADR-0165, ADR-0272): the parts that are read
/// live, and the rest as the station's setting.
///
/// What is read live is the profanity delay (the desk reports it) and what the listener process
/// holds of playout's feed (its own `feed.buffer_length`). What is not: the listener
/// process's mount holds back a fixed stretch before it serves listeners, and each listener's
/// player buffers on top. No marker travels in the stream (a metadata change would chain the
/// Ogg stream and drop the mount, AGENTS.md), so there is no measurement from the desk to a
/// listener's ear; `measured` says whether there is one.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ListenerDelay {
    /// The total: `desk_ms` plus `process_ms` plus `rest_ms`.
    pub ms: u64,
    /// True only when no part of `ms` is a setting. Nothing measures the mount's hold or a
    /// player's buffer, so it is false in this build; the app words the figure as an estimate
    /// and names the parts that are read.
    pub measured: bool,
    /// How much the desk's profanity delay holds now (read live from the desk), to 0.1 s.
    pub desk_ms: u64,
    /// How much of playout's feed the listener process holds now (read live from it, to
    /// 0.1 s); `None` while it does not answer.
    pub process_ms: Option<u64>,
    /// The rest of the path as the `studio.listener_delay_ms` setting says: the mount's own
    /// hold, playout and a listener's player. Never read.
    pub rest_ms: u64,
}

/// What the `studio.listener_delay_ms` setting is taken to include of the listener process's
/// feed buffer (the 0.5 s default of `FEED_BUFFER_S`): when the live reading exists it replaces
/// this share of the setting, so the buffer is counted once.
const NOMINAL_FEED_BUFFER_MS: u64 = 500;

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
    /// Messages held for a moderator (the chat service's queue); `None` when not counted. The
    /// desk's chat panel says when a moderator is needed (ADR-0274).
    pub held: Option<u32>,
    /// Open reports, notices and reviews waiting for a moderator; `None` when not counted.
    pub open_reports: Option<u32>,
}

/// The station's beginner step as the studio's windows see it (ADR-0271, ADR-0274).
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct StepView {
    /// The step in force: the step of the show on air; between shows, of the show last on air;
    /// `full` when no show has ever been on air.
    pub step: Step,
    /// The show the step comes from; `None` when no show has ever been on air.
    pub show: Option<StepShow>,
    /// That show is on air now (false: it is the show last on air).
    pub on_air: bool,
    /// The next show to start by the schedule, with its step.
    pub next: Option<StepNext>,
    /// Whether the account asking may change the step: an admin, a producer or the show's
    /// presenter. Filled in for each request (the shared view says `false`).
    pub may_change: bool,
}

impl Default for StepView {
    fn default() -> Self {
        StepView {
            step: Step::Full,
            show: None,
            on_air: false,
            next: None,
            may_change: false,
        }
    }
}

/// The show a step comes from.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct StepShow {
    pub slug: String,
    pub name: String,
}

/// The next show to start.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct StepNext {
    pub slug: String,
    pub name: String,
    /// When it starts, in epoch milliseconds.
    pub start_ms: u64,
    pub step: Step,
}

/// What the control plane reads from the schedule and the shows to know the step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepFacts {
    pub view: StepView,
    /// The id of the show the step comes from.
    pub show_id: Option<Uuid>,
    /// The staff account that presents that show, if it names one.
    pub presenter_account: Option<Uuid>,
}

/// What the studio holds of the step: the view, and what the seat checks need beside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StepState {
    view: StepView,
    show_id: Option<Uuid>,
    presenter_account: Option<Uuid>,
    /// Why the last fall below Callers happened, for callers let go at later looks.
    release_cause: Option<crate::steps::ReleaseCause>,
}

/// How the step moved when new facts came in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepMove {
    pub from: Step,
    pub to: Step,
    /// Another show now holds the step (a show took the air, or its time ended).
    pub other_show: bool,
    /// Anything the view carries changed.
    pub changed: bool,
}

/// What the seat checks and the commands need to know of the step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepPolicy {
    pub step: Step,
    pub presenter_account: Option<Uuid>,
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
            listener_delay_ms: 13_000,
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

    /// The count of listeners the listener process reports, by mount, or why there is none.
    /// Returns whether anything the view shows changed.
    pub fn set_audience(
        &mut self,
        counted: Option<Vec<MountCount>>,
        error: Option<String>,
    ) -> bool {
        let (total, mut mounts) = match counted {
            Some(m) => (Some(m.iter().map(|c| c.listeners).sum()), m),
            None => (None, Vec::new()),
        };
        mounts.sort_by(|a, b| b.listeners.cmp(&a.listeners).then(a.mount.cmp(&b.mount)));
        let error = if total.is_some() { None } else { error };
        let changed =
            self.listeners != total || self.mounts != mounts || self.audience_error != error;
        self.listeners = total;
        self.mounts = mounts;
        self.audience_error = error;
        changed
    }

    /// How much of playout's feed the listener process holds, in ms (`None`: it does not answer).
    /// Shown to 0.1 s, and a move of under 0.2 s is not a change: the reading wobbles every poll,
    /// and a change wakes every app that follows the view.
    pub fn set_feed_buffer(&mut self, ms: Option<u64>) -> bool {
        let ms = ms.map(|m| m / 100 * 100);
        if let (Some(old), Some(new)) = (self.feed_buffer_ms, ms)
            && old.abs_diff(new) < 200
        {
            return false;
        }
        std::mem::replace(&mut self.feed_buffer_ms, ms) != ms
    }

    /// Take the step the schedule and the shows give (ADR-0271); says how it moved.
    pub fn set_step(&mut self, facts: StepFacts) -> StepMove {
        let before = &self.step;
        let mut view = facts.view;
        // `may_change` is for each request to fill in.
        view.may_change = false;
        let mv = StepMove {
            from: before.view.step,
            to: view.step,
            other_show: before.show_id != facts.show_id,
            changed: before.view != view
                || before.show_id != facts.show_id
                || before.presenter_account != facts.presenter_account,
        };
        self.step = StepState {
            view,
            show_id: facts.show_id,
            presenter_account: facts.presenter_account,
            release_cause: self.step.release_cause,
        };
        mv
    }

    /// Keep why the step fell below Callers (for callers let go at later looks).
    pub fn note_release_cause(&mut self, cause: crate::steps::ReleaseCause) {
        self.step.release_cause = Some(cause);
    }

    /// Why the step last fell below Callers; `StepMovedDown` if it never did in this run.
    pub fn release_cause(&self) -> crate::steps::ReleaseCause {
        self.step
            .release_cause
            .unwrap_or(crate::steps::ReleaseCause::StepMovedDown)
    }

    /// The show the station's step comes from: the show on air, or between shows the show last
    /// on air. Its own cart banks are the live studio's (1.0.2).
    pub fn step_show(&self) -> Option<&StepShow> {
        self.step.view.show.as_ref()
    }

    /// The step in force and who presents the show it comes from.
    pub fn step_policy(&self) -> StepPolicy {
        StepPolicy {
            step: self.step.view.step,
            presenter_account: self.step.presenter_account,
        }
    }

    /// The callers a falling step lets go: everyone in the queue except those who hold the desk's
    /// strip for callers or are in cue or on air (ADR-0271: a caller on strip 4 stays until
    /// dropped). Includes callers whose link dropped and whose place is held.
    pub fn releasable(&self, desk: Option<&DeskStatus>) -> Vec<String> {
        let person = |id: &str| desk.and_then(|d| d.people.iter().find(|p| p.identity == id));
        self.callers
            .iter()
            .filter(|c| {
                self.slot_of(&c.identity).is_none()
                    && !person(&c.identity).is_some_and(|p| p.on_air || p.cue)
            })
            .map(|c| c.identity.clone())
            .collect()
    }

    /// What waits for a moderator, as counted; returns whether it changed.
    pub fn set_chat_counts(
        &mut self,
        counts: Option<weavesignal_model::chat::ModerationCounts>,
    ) -> bool {
        std::mem::replace(&mut self.chat_counts, counts) != counts
    }

    /// Keep the picture mode the desk had when the slate was taken to hold it.
    pub fn hold_picture(&mut self, mode: Option<serde_json::Value>) {
        self.held_picture = mode;
    }

    /// The picture mode held for Full, if the slate is being held.
    pub fn held_picture(&self) -> Option<&serde_json::Value> {
        self.held_picture.as_ref()
    }

    /// Say that a show's step moved, and who moved it if that is known, for every window
    /// (ADR-0271).
    pub fn step_moved(&mut self, now: u64, who: Option<&str>, show: &str, step: Step) {
        let title = match who {
            Some(who) => format!("{who} moved {show} to {}.", step.word()),
            None => format!("{show} moved to {}.", step.word()),
        };
        self.notice(
            now,
            (Surface::Desk, Tone::Good, "step.moved"),
            None,
            title,
            String::new(),
        );
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
        // At Callers the desk has one strip for callers (ADR-0271): a second caller waits until
        // the first is dropped or sent back. A caller the desk declined has left the strip.
        if self.step.view.step == crate::steps::Step::Callers
            && let Some(other) = self
                .callers
                .iter()
                .find(|o| o.identity != id && o.state == CallerState::Ready && o.declined.is_none())
        {
            let name = self.short_name(&other.identity);
            return Err(StudioError::NotNow(format!(
                "At Callers the desk takes one caller at a time, and {name} is on the callers' strip. Drop them or send them back first, or move the show to Full."
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

    /// The listener delay: the desk's profanity delay and the listener process's feed buffer as
    /// read, and the rest of the path as the setting. When the buffer is read it replaces its
    /// nominal share of the setting, so it is not counted twice.
    fn listener_delay(&self, delay: Option<&crate::desk::DeskDelay>) -> ListenerDelay {
        let desk_ms = delay.map_or(0, |x| u64::from(x.delay_ms / 100 * 100));
        let process_ms = self.feed_buffer_ms;
        let rest_ms = match process_ms {
            Some(_) => self
                .listener_delay_ms
                .saturating_sub(NOMINAL_FEED_BUFFER_MS),
            None => self.listener_delay_ms,
        };
        ListenerDelay {
            ms: desk_ms + process_ms.unwrap_or(0) + rest_ms,
            measured: false,
            desk_ms,
            process_ms,
            rest_ms,
        }
    }

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
                // The presenter's seat (ADR-0074, ADR-0231): as in 1.0.0, the first presenter,
                // while any presenter is on air; with no presenter on air, the first DJ on air
                // (after a handover, or a DJ presenting alone). So once the outgoing presenter
                // takes themselves off, the talk keys, ⌃T and the join page's name pass to the
                // DJ, but a second presenter never takes the seat from the first.
                let on_air = |role: Role| {
                    self.slots.iter().flatten().find(|id| {
                        person(id).is_some_and(|p| p.role == role && p.on_air && p.present)
                    })
                };
                let presenter = if on_air(Role::Presenter).is_some() {
                    seat(Role::Presenter)
                } else {
                    on_air(Role::Dj).cloned().or_else(|| seat(Role::Presenter))
                };
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
                held: self.chat_counts.map(|c| c.held),
                open_reports: self.chat_counts.map(|c| c.reports),
            },
            notices: self.notices.iter().cloned().collect(),
            carts: crate::carts::CartsView::default(),
            // The setting is the path beyond the desk; the profanity delay's current length is
            // added (ADR-0165), so a dump or a rebuild moves the figure too.
            listener_delay: self.listener_delay(d.delay.as_ref()),
            default_fallback: self.default_fallback,
            audience: AudienceView {
                listeners: self.listeners,
                by_mount: self.mounts.clone(),
                error: self.audience_error.clone(),
                edges: self.edges,
            },
            step: self.step.view.clone(),
            mode: self.mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(name: &str, n: u32) -> MountCount {
        MountCount {
            mount: name.into(),
            listeners: n,
        }
    }

    #[test]
    fn the_view_carries_the_audience_for_the_apps_cdn_warning() {
        let mut s = Studio::default();
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.audience,
            AudienceView {
                listeners: None,
                by_mount: vec![],
                error: None,
                edges: false
            }
        );
        let counted = vec![mount("/programme.mp3", 112), mount("/programme.opus", 500)];
        assert!(
            s.set_audience(Some(counted.clone()), None),
            "a new figure is a change"
        );
        assert!(
            !s.set_audience(Some(counted), None),
            "the same figure is not"
        );
        s.set_edges(true);
        let v = s.view("", None, None, 0);
        assert_eq!(
            v.audience,
            AudienceView {
                listeners: Some(612),
                by_mount: vec![mount("/programme.opus", 500), mount("/programme.mp3", 112)],
                error: None,
                edges: true
            },
            "the total adds the mounts, the busiest first"
        );
        assert!(
            s.set_audience(None, Some("the listener process does not answer".into())),
            "not counted is a change too"
        );
        let v = s.view("", None, None, 0);
        assert_eq!(v.audience.listeners, None);
        assert!(v.audience.by_mount.is_empty(), "no stale mounts");
        assert_eq!(
            v.audience.error.as_deref(),
            Some("the listener process does not answer")
        );
        let json = serde_json::to_value(s.view("", None, None, 0)).unwrap();
        assert_eq!(
            json["audience"],
            serde_json::json!({
                "listeners": null,
                "by_mount": [],
                "error": "the listener process does not answer",
                "edges": true
            })
        );
        // A count wipes the reason.
        s.set_audience(Some(vec![mount("/programme.opus", 0)]), Some("old".into()));
        assert_eq!(s.view("", None, None, 0).audience.error, None);
        assert_eq!(s.view("", None, None, 0).audience.listeners, Some(0));
    }

    #[test]
    fn the_listener_delay_names_what_is_read_and_what_is_set() {
        let mut s = Studio::new("Show", vec![]);
        let delay = |ms| crate::desk::DeskDelay {
            delay_ms: ms,
            target_ms: 8_000,
            rebuilding: false,
            dumps: 0,
            dumped_ms_ago: None,
        };
        // Nothing read from the listener process: the setting stands as it did, plus the desk.
        let none = s.listener_delay(Some(&delay(8_040)));
        assert_eq!(
            none,
            ListenerDelay {
                ms: 21_000,
                measured: false,
                desk_ms: 8_000,
                process_ms: None,
                rest_ms: 13_000
            }
        );
        // Its buffer read: it replaces the 0.5 s the setting is taken to hold, once.
        assert!(s.set_feed_buffer(Some(3_270)));
        assert!(
            !s.set_feed_buffer(Some(3_310)),
            "a wobble of a tenth is no change"
        );
        assert!(s.set_feed_buffer(Some(3_600)), "a move of 0.4 s is");
        let read = s.listener_delay(Some(&delay(8_040)));
        assert_eq!(read.process_ms, Some(3_600));
        assert_eq!(read.rest_ms, 12_500);
        assert_eq!(read.ms, 8_000 + 3_600 + 12_500);
        assert_eq!(
            read.ms,
            read.desk_ms + read.process_ms.unwrap() + read.rest_ms
        );
        assert!(
            !read.measured,
            "the mount's hold and the player are never read"
        );
        // No desk: no profanity delay.
        assert_eq!(s.listener_delay(None).desk_ms, 0);
        assert!(s.set_feed_buffer(None));
        assert_eq!(s.listener_delay(None).ms, 13_000);
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
    fn the_presenters_seat_follows_the_air_through_a_handover() {
        let mut s = Studio::new("", vec![]);
        let on = |id: &str, role: Role, on_air: bool| DeskPerson {
            on_air,
            ..person(id, role)
        };
        let mut seat_at = |people: Vec<DeskPerson>, now: u64| {
            let room = desk(people);
            s.sync(&room, now);
            s.view("Kettle FM", Some(&room), None, now).seats.presenter
        };
        // Finlay joins as a DJ, off air: the seat stays Eilidh's.
        let eilidh = || person("eilidh", Role::Presenter);
        assert_eq!(
            seat_at(vec![eilidh(), on("finlay", Role::Dj, false)], 0).as_deref(),
            Some("eilidh")
        );
        // The overlap: both on air; the presenter keeps the seat.
        assert_eq!(
            seat_at(vec![eilidh(), on("finlay", Role::Dj, true)], 1).as_deref(),
            Some("eilidh")
        );
        // Eilidh takes herself off: the seat passes to the DJ on air, before she leaves...
        let eilidh_off = || on("eilidh", Role::Presenter, false);
        assert_eq!(
            seat_at(vec![eilidh_off(), on("finlay", Role::Dj, true)], 2).as_deref(),
            Some("finlay")
        );
        // ...and after.
        assert_eq!(
            seat_at(vec![on("finlay", Role::Dj, true)], 3).as_deref(),
            Some("finlay")
        );
        // A DJ off air with no presenter holds no seat: there is nobody to speak as.
        assert_eq!(seat_at(vec![on("finlay", Role::Dj, false)], 4), None);
        // A presenter off for a moment, with no DJ on air, keeps it.
        assert_eq!(
            seat_at(vec![eilidh_off(), on("finlay", Role::Dj, false)], 5).as_deref(),
            Some("eilidh")
        );
        // Two presenters: the first off air keeps the seat while the second is on (as in 1.0.0),
        // so the talk keys never speak with the second one's microphone.
        assert_eq!(
            seat_at(vec![eilidh_off(), person("rory", Role::Presenter)], 6).as_deref(),
            Some("eilidh")
        );
        // ...and with a DJ on air too, a presenter on air still holds it for the presenters.
        assert_eq!(
            seat_at(
                vec![
                    eilidh_off(),
                    person("rory", Role::Presenter),
                    on("finlay", Role::Dj, true)
                ],
                7
            )
            .as_deref(),
            Some("eilidh")
        );
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
    fn at_callers_the_desk_takes_one_caller_at_a_time() {
        use crate::steps::{Step, facts_from, test_show};
        let mut s = Studio::new("", vec![]);
        s.set_step(facts_from(
            Some(&test_show(Step::Callers, None)),
            true,
            None,
        ));
        consented(&mut s, "niall");
        consented(&mut s, "jessie");
        let room = desk(vec![
            person("eilidh", Role::Presenter),
            person("niall", Role::Caller),
            person("jessie", Role::Caller),
        ]);
        s.sync(&room, 0);
        s.send("niall", 1).unwrap();
        let err = s.send("jessie", 2).unwrap_err();
        assert!(
            matches!(&err, StudioError::NotNow(m) if m.contains("one caller at a time")),
            "{err}"
        );
        s.decline("niall", "", 3).unwrap();
        s.send("jessie", 4)
            .expect("a declined caller has left the strip");
        s.set_step(facts_from(Some(&test_show(Step::Full, None)), true, None));
        s.send("niall", 5)
            .expect("at Full the screener may send more");
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

    #[test]
    fn the_view_carries_the_step_and_the_chats_counts() {
        use crate::steps::{facts_from, test_show};
        let mut s = Studio::default();
        let v = s.view("", None, None, 0);
        assert_eq!(v.step.step, Step::Full, "no show has been on air: full");
        assert!(v.step.show.is_none() && !v.step.on_air && v.step.next.is_none());
        assert_eq!((v.chat.held, v.chat.open_reports), (None, None));

        let kettle = test_show(Step::Solo, None);
        let steeped = crate::steps::test_show(Step::Full, None);
        let mv = s.set_step(facts_from(Some(&kettle), true, Some((&steeped, 1_000))));
        assert_eq!(
            (mv.from, mv.to, mv.other_show, mv.changed),
            (Step::Full, Step::Solo, true, true)
        );
        let again = s.set_step(facts_from(Some(&kettle), true, Some((&steeped, 1_000))));
        assert!(!again.changed && !again.other_show, "nothing moved");
        // The same show, another step: not another show.
        let moved = s.set_step(facts_from(
            Some(&test_show(Step::Guests, None)),
            true,
            Some((&steeped, 1_000)),
        ));
        assert_eq!(
            (moved.from, moved.to, moved.other_show),
            (Step::Solo, Step::Guests, false)
        );
        assert_eq!(s.step_policy().step, Step::Guests);

        assert!(
            s.set_chat_counts(Some(weavesignal_model::chat::ModerationCounts {
                held: 2,
                reports: 1
            }))
        );
        let json = serde_json::to_value(s.view("", None, None, 0)).unwrap();
        assert_eq!(json["step"]["step"], "guests");
        assert_eq!(json["step"]["show"]["slug"], "morning-kettle");
        assert_eq!(json["step"]["on_air"], true);
        assert_eq!(json["step"]["may_change"], false);
        assert_eq!(json["step"]["next"]["start_ms"], 1_000_000);
        assert_eq!(json["step"]["next"]["step"], "full");
        assert_eq!(json["chat"]["held"], 2);
        assert_eq!(json["chat"]["open_reports"], 1);
        assert!(
            !s.set_chat_counts(Some(weavesignal_model::chat::ModerationCounts {
                held: 2,
                reports: 1
            }))
        );
    }

    #[test]
    fn a_falling_step_lets_waiting_callers_go_but_not_the_one_on_the_desks_strip() {
        let mut s = Studio::default();
        let desk = DeskStatus {
            people: vec![
                {
                    let mut p = person("caller-niall", Role::Caller);
                    p.name = "Niall".into();
                    p
                },
                {
                    let mut p = person("caller-jessie", Role::Caller);
                    p.name = "Jessie".into();
                    p
                },
                person("caller-dougal", Role::Caller),
            ],
            ..DeskStatus::default()
        };
        for id in ["caller-niall", "caller-jessie", "caller-dougal"] {
            s.call_in(CallIn {
                identity: id.into(),
                name: id.into(),
                topic: "No bus".into(),
                consent: Consent {
                    at_ms: 0,
                    version: "v".into(),
                    by: ConsentBy::Caller,
                    text: "words".into(),
                },
            });
        }
        s.sync(&desk, 1);
        // Jessie is being screened, Dougal was sent to the desk and holds its strip.
        s.screen("caller-jessie", 2).unwrap();
        s.send("caller-dougal", 3).unwrap();
        s.sync(&desk, 4);
        assert_eq!(s.slot_of("caller-dougal"), Some(4));
        assert_eq!(
            s.releasable(Some(&desk)),
            vec!["caller-niall".to_owned(), "caller-jessie".to_owned()]
        );
        // One in cue stays too, even without the strip.
        let mut cued = desk.clone();
        cued.people[0].cue = true;
        assert_eq!(s.releasable(Some(&cued)), vec!["caller-jessie".to_owned()]);
    }
}
