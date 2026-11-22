// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What a host sees on their monitor link, and what they may press (ADR-0280; design sections
//! 3.3, 3.5, 3.7, 3.8, 4.5, 8 and 10): the one view that `GET /api/v1/monitor`, its events and
//! every control's answer carry.
//!
//! It is built from facts in two steps. [`gather`] reads them (the schedule, the sources, the
//! desk, the post's rule's own view, the log) and [`build`] is a pure function of them, so the
//! words and the rules are tested without a database. The controls' rules live in [`controls`]
//! alone: the view says what may be pressed from them, and every `POST /monitor/...` asks them
//! again before it acts, so the page and the routes cannot disagree.
//!
//! **What it never holds:** another host's key, secret or key id; the post's view is projected
//! down to what this host needs of it, and the names that appear are the ones the design shows
//! on the page (the next show's host, whoever holds the air, the show before and after).

use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;
use weavesignal_model::post::{PostView, SourceState};
use weavesignal_model::shows::{Show, Step};
use weavesignal_model::sources::{Airing, LogEntry, Refusal, ShowKey, SourceSettings};
use weavesignal_proto::{Role, source_wire};

use crate::app::AppState;
use crate::desk::DeskPerson;
use crate::hub::now_ms;
use crate::sources::SourceView;

/// How far ahead the log is read for the next insert.
const INSERT_LOOK_MS: i64 = 2 * 3600 * 1000;
/// How long a lost place's gone-words stay up, after the place was let go.
const PLACE_GONE_FOR_MS: i64 = 30 * 60 * 1000;
/// How long DUMPED stays up after a Dump (design section 3.7: "for a few seconds").
const DUMPED_FOR_MS: i64 = 10_000;

/// The station's clock, as the monitor page shows it (never the device's).
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct StationClock {
    /// The station's time now, milliseconds since the Unix epoch.
    pub now_ms: u64,
    /// The station's time zone (an IANA name).
    pub zone: String,
}

/// A refusal of the host's software, in words.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct LastRefusal {
    pub at_ms: i64,
    /// `wrong_key`, `ended`, `not_your_slot`, `in_use`, `too_many`, `turned_away`, `format`,
    /// `desk_full` or `too_fast`.
    pub reason: String,
    /// What the page says.
    pub words: String,
}

/// What happens next to this host, in one sentence, with its time.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct Then {
    /// `at_post`, `late_count`, `grace`, `held`, `waits`, `press_only`, `keeps_air`, `slot`,
    /// `link_lost`, `place_gone` or `test`: which sentence this is, for a page that picks by it.
    pub kind: String,
    /// The sentence, in the design's words, without its count and without clock times: the
    /// page adds the count it draws from `at_ms` and the station's clock.
    pub words: String,
    /// The instant the sentence counts to, on the station's clock (`station.now_ms`), when it
    /// counts to one.
    pub at_ms: Option<i64>,
}

/// The post, as far as it is this host's: the show whose post it is, and its clocks.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct MonitorPost {
    /// The show whose post it is.
    pub show_name: String,
    /// When its post is.
    pub start_ms: i64,
    /// Its post has come.
    pub started: bool,
    /// The post's show is this host's own: they are who takes the air (or one of their show's
    /// hosts is).
    pub mine: bool,
    /// The host who takes the air at this post, by name, and where they have got to:
    /// `standing_by`, `connecting`, `not_connected`, `link_lost` or `on_air`.
    pub host: Option<String>,
    pub host_state: Option<String>,
    /// When the schedule puts this host on air by itself, from automation: at the post, or the
    /// late count's end.
    pub on_air_at_ms: Option<i64>,
    /// When the late count ends (this host's own count; `go` and `not_yet` act on it).
    pub late_ends_ms: Option<i64>,
    /// When the grace ends and the schedule hands the air over from whoever holds it.
    pub grace_ends_ms: Option<i64>,
    /// Who holds this handover (a producer's Hold): the grace does not run.
    pub held_by: Option<String>,
    /// Who presents now, on air, by name.
    pub on_air: Vec<String>,
    /// The handover window's heads-up, when this host is the one coming on next and the station
    /// has given it (once an airing; design note `handover-window.md`).
    pub heads_up: Option<HeadsUpLine>,
}

/// The station's one line to the incoming host before their post.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct HeadsUpLine {
    /// When the station gave it.
    pub at_ms: i64,
    /// What the page says.
    pub words: String,
}

/// One show in the rundown around this host's own.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct RundownRow {
    pub show_name: String,
    /// Its host's name: the key's label for a host's own studio, the presenter for a show from
    /// the room; absent when it names none.
    pub host: Option<String>,
    /// `on_air`, `open`, `standing_by`, `connecting`, `not_connected`, `link_lost`, `test`,
    /// `in_room` (a show from the room) or `standby` (for this host's own row, as `state`).
    pub state: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Before you, you and after you (`monitor.next`).
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct Rundown {
    pub before: Option<RundownRow>,
    pub you: Option<RundownRow>,
    /// Absent when nothing is scheduled after: automation follows.
    pub after: Option<RundownRow>,
}

/// The station's next insert, as far as the log can say (ADR-0045).
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct Insert {
    /// The cart's label.
    pub label: String,
    pub at_ms: i64,
    pub duration_ms: u32,
    /// Whether it plays over a live show or instead of it is not in the log's model: always
    /// `unknown` today.
    pub plays: String,
}

/// A Dump just taken, for the host who was on air (design sections 3.7 and 8).
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct Dumped {
    /// When it was taken, on the station's clock.
    pub at_ms: i64,
    /// What listeners lost, in milliseconds: what the delay held.
    pub lost_ms: u64,
    /// The same in whole seconds, as the words say it.
    pub seconds: u64,
    /// What the page says: *Dumped. Listeners lost 8 s. Carry on, and do not repeat the words.*
    pub words: String,
}

/// The show's step, as the controls follow it.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct StepInfo {
    /// `solo`, `guests`, `callers` or `full`: the station's, which is the step of the show on
    /// air, and the next show's during a grace (ADR-0271).
    pub step: Step,
    /// `Solo`, `Guests`, `Callers` or `Full`.
    pub word: String,
    /// Dump is there (Callers and up).
    pub dump: bool,
    /// The overlap, and so the handover's two presses, is there (Full).
    pub handover: bool,
}

/// The next host, for Cue, Bring and Hand over.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct NextHostView {
    pub name: String,
    /// `standing_by`, `connecting`, `not_connected`, `link_lost` or `on_air`.
    pub state: String,
    /// In this host's headphones now.
    pub cued: bool,
}

/// One button: whether it may be pressed now, and if not, why, in words.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Control {
    pub may: bool,
    pub why_not: Option<String>,
}

impl Control {
    fn yes() -> Self {
        Control {
            may: true,
            why_not: None,
        }
    }

    fn no(why: impl Into<String>) -> Self {
        Control {
            may: false,
            why_not: Some(why.into()),
        }
    }
}

/// Everything this host may do now. A page draws only what has `may`, and says `why_not` for
/// the one it keeps in sight (Dump below Callers).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Controls {
    /// *Go on air now*: skips the late count, or takes the air at the show's time.
    pub go: Control,
    /// *Not yet*: stops the late count, so the host goes on air by their own press.
    pub not_yet: Control,
    /// *Cue [next]*: the next host in this host's headphones.
    pub cue: Control,
    /// *Bring [next] on air*: the handover's first press, the overlap (Full).
    pub bring: Control,
    /// *Hand over to [next]*: the second press; this host comes off air (Full).
    pub hand_over: Control,
    /// *End my show*, which asks once; `end_show_says` is what follows.
    pub end_show: Control,
    /// What follows End show, for the question: *Heather is standing by: the air passes to
    /// them as you leave.* or *Automation takes the air.*
    pub end_show_says: Option<String>,
    /// *Dump*, two presses on the page (Callers and up).
    pub dump: Control,
}

/// The monitor link's view: this host's own, and nothing of anyone else's.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct MonitorView {
    /// Who the key is for.
    pub label: String,
    pub show: String,
    pub show_name: String,
    /// The host's lamp: `on_air` (listeners hear them), `open` (the station and the room hear
    /// them; listeners do not: automation holds the air, or playout is down), `standby`,
    /// `link_lost`, `test`, or `not_connected`.
    pub state: String,
    /// The lamp's sentence, in the design's words (*Standing by. The station hears you.
    /// Listeners do not.*).
    pub says: String,
    /// What happens next, in one sentence with its time; absent when nothing does.
    pub then: Option<Then>,
    /// `icecast`, `srt`, `whip`, `bridge` or `vban`, while connected; `pull` while the station holds a pull open.
    pub way: Option<String>,
    /// What their software sends, while connected.
    pub format: Option<String>,
    /// Their level at the desk (short-term LUFS after the leveller); for a test, the ingest's
    /// reading (dBFS RMS).
    pub level: Option<f32>,
    /// What the leveller does to bring them to the station's level, in dB.
    pub leveller_db: Option<f32>,
    /// How long they have been connected, in milliseconds.
    pub steady_ms: Option<u64>,
    /// What their software says is playing.
    pub title: Option<String>,
    /// Whether their monitor mix is open at the desk (someone is listening).
    pub monitor_open: bool,
    /// When their slot's door closes, while it is open (the station's clock).
    pub door_closes_ms: Option<i64>,
    /// The last time the station refused their software, in words.
    pub last_refusal: Option<LastRefusal>,
    /// The post, as far as it is this host's; absent when it is not.
    pub post: Option<MonitorPost>,
    pub rundown: Rundown,
    /// The next insert, as far as the log can say.
    pub insert: Option<Insert>,
    pub step: StepInfo,
    /// The next host: whoever this host would cue and bring on; absent when none is in sight.
    pub next_host: Option<NextHostView>,
    /// How far behind the programme listeners hear, in ms, for *Listeners hear you, about 23 s
    /// behind.*
    pub listeners_behind_ms: Option<u64>,
    /// The host's monitor mix carries what listeners hear now: someone else is on air at the
    /// desk and listeners hear the desk, not automation. False while automation (or the
    /// fallback) holds the air, which the desk does not make, so the mix is silent but for cue
    /// and talk addressed to the host. The page then plays the listeners' stream, a few
    /// seconds behind, while the host stands by or is OPEN (in OPEN the room's voices are not
    /// in it, and the page says so).
    pub monitor_has_programme: bool,
    pub controls: Controls,
    /// A Dump was taken in the last few seconds (from this link or from the desk) and this host
    /// is on air: the page says DUMPED. Absent otherwise.
    pub dumped: Option<Dumped>,
    pub station: StationClock,
}

/// The facts a view is built from.
pub(crate) struct Facts {
    pub now_ms: i64,
    pub key: ShowKey,
    pub show: Show,
    /// This host's own row in the sources' list; absent when the list leaves them out (not
    /// connected, and the door is shut).
    pub mine: Option<SourceView>,
    pub people: Vec<DeskPerson>,
    /// The desk slot of each DJ in the room, for the order the handover's key takes them in.
    pub slots: HashMap<String, usize>,
    pub post: Option<PostView>,
    pub step: Step,
    /// The station has the playout layer.
    pub automation: bool,
    pub auto_mode: bool,
    pub playout_unknown: bool,
    pub settings: SourceSettings,
    pub airings: Vec<Airing>,
    pub shows: Vec<Show>,
    pub keys: Vec<ShowKey>,
    pub sources: Vec<SourceView>,
    pub log: Vec<LogEntry>,
    pub insert: Option<Insert>,
    pub behind_ms: Option<u64>,
    /// The last Dump the desk took.
    pub dump: Option<crate::hub::DumpRecord>,
    pub zone: String,
}

// ---- reading the facts ----

/// Read the facts for `key` and `show`.
pub(crate) async fn gather(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &ShowKey,
    show: &Show,
) -> Result<Facts, sqlx::Error> {
    use weavesignal_model::{shows, sources as model};
    let now = now_ms() as i64;
    let sources = crate::sources::sources_now(state, pool).await?;
    let (settings, keys, all_shows, airings, log) = tokio::try_join!(
        model::get_settings(pool, state.station.id),
        model::live_keys(pool, state.station.id, now),
        shows::list_shows(pool, state.station.id),
        crate::sources::airings(state, pool, now / 1000),
        model::key_log(pool, state.station.id, &key.key_id, 40),
    )?;
    let mine = sources
        .sources
        .iter()
        .find(|s| s.key_id == key.key_id)
        .cloned();
    let people = state
        .hub
        .desk_status()
        .map(|d| d.people)
        .unwrap_or_default();
    let slots = people
        .iter()
        .filter(|p| p.role == Role::Dj)
        .filter_map(|p| {
            state
                .hub
                .read_studio(|s| s.slot_of(&p.identity))
                .map(|n| (p.identity.clone(), n))
        })
        .collect();
    let insert = next_insert(pool, state, now).await?;
    Ok(Facts {
        now_ms: now,
        key: key.clone(),
        show: show.clone(),
        mine,
        people,
        slots,
        post: sources.post,
        step: state.hub.read_studio(|s| s.step_policy().step),
        automation: state.hub.onair.get().is_some(),
        auto_mode: state.hub.read_studio(|s| s.station_mode())
            == Some(crate::desk::StationMode::Auto),
        playout_unknown: state
            .hub
            .onair
            .get()
            .is_some_and(|o| o.now_playing().on_air == "unknown"),
        settings: state.hub.sources.in_force(settings),
        airings,
        shows: all_shows,
        keys,
        sources: sources.sources,
        log,
        insert,
        behind_ms: Some(state.hub.view().listener_delay.ms).filter(|ms| *ms > 0),
        dump: state.hub.last_dump(),
        zone: state.hub.zone.name(),
    })
}

/// The next cart the log plans within a couple of hours: all it can say of an insert.
async fn next_insert(
    pool: &sqlx::PgPool,
    state: &AppState,
    now: i64,
) -> Result<Option<Insert>, sqlx::Error> {
    use weavesignal_model::{carts, playout};
    let items = playout::items(pool, state.station.id, now, now + INSERT_LOOK_MS).await?;
    let Some(item) = items
        .into_iter()
        .find(|i| i.kind == "cart" && matches!(i.status.as_str(), "planned" | "queued"))
    else {
        return Ok(None);
    };
    let label = match item.cart {
        Some(id) => carts::load(pool, state.station.id)
            .await?
            .1
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.label),
        None => None,
    };
    Ok(Some(Insert {
        label: label.unwrap_or_else(|| "Station insert".to_owned()),
        at_ms: item.planned_at_ms,
        duration_ms: item.duration_ms,
        plays: "unknown".to_owned(),
    }))
}

// ---- the pieces of the view ----

impl Facts {
    fn identity(&self) -> String {
        source_wire::identity(&self.key.key_id)
    }

    fn me(&self) -> Option<&DeskPerson> {
        let id = self.identity();
        self.people.iter().find(|p| p.identity == id)
    }

    /// This host's source is on air at the desk (ON AIR or OPEN).
    pub(crate) fn on_air(&self) -> bool {
        self.me().is_some_and(|p| p.on_air)
    }

    /// Whether automation holds the air or playout is down, so an open source is not heard by
    /// listeners.
    fn open_not_heard(&self) -> bool {
        (self.automation && self.auto_mode) || self.playout_unknown
    }

    /// The lamp: `on_air`, `open`, `standby`, `link_lost`, `test` or `not_connected`.
    fn lamp(&self) -> &'static str {
        match self.mine.as_ref().map(|s| s.state.as_str()) {
            Some("on_air") if self.open_not_heard() => "open",
            Some("on_air") => "on_air",
            Some("standby") => "standby",
            Some("link_lost") => "link_lost",
            Some("test") => "test",
            _ => "not_connected",
        }
    }

    /// Whether the desk's mix for this host carries what listeners hear: another voice that can
    /// reach the programme is on air at the desk (not away, not muted), and listeners hear the
    /// desk. Automation is playout's, downstream of the desk, so with none of them it is silent.
    fn monitor_has_programme(&self) -> bool {
        let me = self.identity();
        !self.open_not_heard()
            && self.people.iter().any(|p| {
                p.identity != me
                    && p.role.can_air()
                    && p.on_air
                    && !p.mute
                    && p.talk_to.is_none()
                    && p.away_ms.is_none()
            })
    }

    /// Who else presents and is on air (names), this host left out.
    fn others_on_air(&self) -> Vec<String> {
        let me = self.identity();
        self.people
            .iter()
            .filter(|p| {
                p.on_air
                    && p.identity != me
                    && matches!(p.role, Role::Presenter | Role::Dj | Role::Source)
            })
            .map(|p| {
                if p.name.is_empty() {
                    p.identity.clone()
                } else {
                    p.name.clone()
                }
            })
            .collect()
    }

    /// This host's show's airing on now, or else the next to come.
    fn my_airing(&self) -> Option<&Airing> {
        let now_s = self.now_ms.div_euclid(1000);
        let mine = |a: &&Airing| a.show == self.show.id;
        self.airings
            .iter()
            .filter(mine)
            .find(|a| a.start <= now_s && now_s < a.end)
            .or_else(|| {
                self.airings
                    .iter()
                    .filter(mine)
                    .filter(|a| a.start > now_s)
                    .min_by_key(|a| a.start)
            })
    }

    fn my_airing_on_now(&self) -> bool {
        let now_s = self.now_ms.div_euclid(1000);
        self.my_airing()
            .is_some_and(|a| a.start <= now_s && now_s < a.end)
    }
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `is` or `are` for a list of names.
fn is_are(n: usize) -> &'static str {
    if n == 1 { "is" } else { "are" }
}

/// The next host: who this host would cue, bring on and hand over to.
struct Next {
    identity: String,
    /// The post's source (its key id) when it came from the post, else the identity.
    name: String,
    state: &'static str,
    /// At the desk now (it can be cued and brought on).
    at_desk: bool,
    on_air: bool,
    cued: bool,
    /// End show would pass the air to them (the post's own source, standing by).
    passes: bool,
}

fn next_host(f: &Facts) -> Option<Next> {
    let person_of = |ids: &[String]| {
        f.people
            .iter()
            .find(|p| ids.contains(&p.identity) && p.present && p.linked)
    };
    // The next show's host the post knows (a source, or the show's presenter in the room).
    if let Some(post) = &f.post
        && let Some(sh) = &post.show
        && sh.slug != f.show.slug
        && let Some(src) = &post.source
    {
        let ids = [source_wire::identity(src), src.clone()];
        let p = person_of(&ids);
        let name = post
            .source_name
            .clone()
            .or_else(|| p.map(|p| p.name.clone()))
            .filter(|n| !n.is_empty())?;
        let state = match (p, post.state) {
            (Some(p), _) if p.on_air => "on_air",
            (Some(_), Some(SourceState::StandingBy)) => "standing_by",
            (Some(_), Some(SourceState::OnAir)) => "on_air",
            (Some(_), _) => "connecting",
            (None, Some(SourceState::LinkLost)) => "link_lost",
            (None, Some(SourceState::Connecting)) => "connecting",
            (None, _) => "not_connected",
        };
        return Some(Next {
            identity: p.map_or_else(|| ids[0].clone(), |p| p.identity.clone()),
            name,
            state,
            at_desk: p.is_some(),
            on_air: p.is_some_and(|p| p.on_air),
            cued: p.is_some_and(|p| p.cue),
            passes: p.is_some_and(|p| !p.on_air) && post.state == Some(SourceState::StandingBy),
        });
    }
    // Otherwise a DJ in the room, the lowest slot first, as the desk's own handover key takes them.
    let me = f.identity();
    let dj = f
        .people
        .iter()
        .filter(|p| p.role == Role::Dj && p.present && p.linked && p.identity != me)
        .min_by_key(|p| f.slots.get(&p.identity).copied().unwrap_or(usize::MAX))?;
    Some(Next {
        identity: dj.identity.clone(),
        name: if dj.name.is_empty() {
            "the next DJ".to_owned()
        } else {
            dj.name.clone()
        },
        state: if dj.on_air { "on_air" } else { "standing_by" },
        at_desk: true,
        on_air: dj.on_air,
        cued: dj.cue,
        passes: false,
    })
}

/// Who a handover or a cue is for: the next host, as the station works them out.
pub(crate) struct Target {
    /// Their desk identity.
    pub identity: String,
    /// What the log writes them under: their key's id, or their desk identity.
    pub subject: String,
    /// Their show, when the post knows it.
    pub show: Option<Uuid>,
}

/// The target of a handover or a cue; absent when none is in sight.
pub(crate) fn next_target(f: &Facts) -> Option<Target> {
    let n = next_host(f)?;
    let subject = f
        .post
        .as_ref()
        .and_then(|p| p.source.clone())
        .filter(|s| source_wire::identity(s) == n.identity)
        .unwrap_or_else(|| n.identity.clone());
    let show = f
        .keys
        .iter()
        .find(|k| source_wire::identity(&k.key_id) == n.identity)
        .map(|k| k.show_id);
    Some(Target {
        identity: n.identity,
        subject,
        show,
    })
}

/// Who the first press brought on (the next host on air beside this one): the handover's
/// second press takes everyone else presenting off.
pub(crate) fn handover_takeoff(f: &Facts) -> Vec<String> {
    let Some(n) = next_host(f) else {
        return Vec::new();
    };
    f.people
        .iter()
        .filter(|p| {
            p.on_air
                && p.identity != n.identity
                && matches!(p.role, Role::Presenter | Role::Dj | Role::Source)
        })
        .map(|p| p.identity.clone())
        .collect()
}

/// What this host may press now (the one rule: [`build`] shows it, every route asks it).
pub(crate) fn controls(f: &Facts) -> Controls {
    let lamp = f.lamp();
    let on_air = f.on_air();
    let step = f.step;
    let next = next_host(f);
    let others = f.others_on_air();
    let not_on_air = "You are not on air.";

    // The late count's own press.
    let counting = f.post.as_ref().is_some_and(|p| {
        p.source.as_deref() == Some(f.key.key_id.as_str()) && p.late_ends_ms.is_some()
    });
    let steady_ms = f
        .me()
        .and_then(|p| p.source.as_ref())
        .map_or(0, |s| s.connected_ms);
    let go = if on_air {
        Control::no("You are on air.")
    } else if lamp != "standby" {
        Control::no(match lamp {
            "test" => "This is a test: your slot's door is not open yet.",
            _ => "Your software has not reached the station.",
        })
    } else if !f.my_airing_on_now() {
        Control::no("Your show has not started. You go on air at its time.")
    } else if !others.is_empty() {
        Control::no(format!(
            "{} {} still on air. You take the air when they hand over, or when the schedule does.",
            join_names(&others),
            is_are(others.len())
        ))
    } else if steady_ms < f.settings.steady_s.max(0) as u64 * 1000 {
        Control::no(
            "You are not steady yet. You can go on air after a few seconds of steady sound.",
        )
    } else {
        Control::yes()
    };
    let not_yet = if counting && lamp == "standby" {
        Control::yes()
    } else if on_air {
        Control::no("You are on air.")
    } else {
        Control::no("There is no count to stop.")
    };

    let step_no = |what: &str, from: Step| {
        format!(
            "{what} is off at {}. It comes on at {}.",
            step.word(),
            from.word()
        )
    };
    let next_gone = |n: &Option<Next>| -> Option<String> {
        match n {
            None => Some("Nobody is standing by after you.".to_owned()),
            Some(n) if !n.at_desk => Some(format!("{} has not connected yet.", n.name)),
            _ => None,
        }
    };

    let cue = if !on_air {
        Control::no(not_on_air)
    } else if let Some(why) = next_gone(&next) {
        Control::no(why)
    } else if next.as_ref().is_some_and(|n| n.on_air) {
        Control::no(format!(
            "{} is on air already.",
            next.as_ref().expect("is some").name
        ))
    } else {
        Control::yes()
    };
    let bring = if !on_air {
        Control::no(not_on_air)
    } else if step < Step::Full {
        Control::no(step_no("The overlap", Step::Full))
    } else if let Some(why) = next_gone(&next) {
        Control::no(why)
    } else if let Some(n) = next.as_ref().filter(|n| n.on_air) {
        Control::no(format!("{} is on air already.", n.name))
    } else {
        Control::yes()
    };
    let hand_over = if !on_air {
        Control::no(not_on_air)
    } else if step < Step::Full {
        Control::no(step_no("The overlap", Step::Full))
    } else if let Some(why) = next_gone(&next) {
        Control::no(why)
    } else if let Some(n) = next.as_ref().filter(|n| !n.on_air) {
        Control::no(format!("Bring {} on air first.", n.name))
    } else {
        Control::yes()
    };
    let (end_show, end_show_says) = if on_air {
        let says = if !others.is_empty() {
            format!(
                "{} stay{} on air.",
                join_names(&others),
                if others.len() == 1 { "s" } else { "" }
            )
        } else if let Some(n) = next.as_ref().filter(|n| n.passes) {
            format!(
                "{} is standing by: the air passes to them as you leave.",
                n.name
            )
        } else if f.automation {
            "Automation takes the air.".to_owned()
        } else {
            "The fallback takes the air.".to_owned()
        };
        (Control::yes(), Some(says))
    } else {
        (Control::no(not_on_air), None)
    };
    let dump = if !on_air {
        Control::no(not_on_air)
    } else if step < Step::Callers {
        Control::no(step_no("Dump", Step::Callers))
    } else {
        Control::yes()
    };
    Controls {
        go,
        not_yet,
        cue,
        bring,
        hand_over,
        end_show,
        end_show_says,
        dump,
    }
}

fn row_state(state: &str) -> &'static str {
    match state {
        "on_air" => "on_air",
        "open" => "open",
        "standby" => "standing_by",
        "link_lost" => "link_lost",
        "test" => "test",
        _ => "not_connected",
    }
}

/// The row for a show: its host and where they are.
fn row_of(f: &Facts, a: &Airing) -> Option<RundownRow> {
    let sh = f.shows.iter().find(|s| s.id == a.show)?;
    let keys: Vec<&ShowKey> = f.keys.iter().filter(|k| k.show_id == sh.id).collect();
    let (host, state) = if keys.is_empty() {
        let host = Some(sh.presenter.clone()).filter(|p| !p.is_empty());
        (host, "in_room".to_owned())
    } else {
        // The best of the show's keys: on air, then standing by, then a lost link.
        let rank = |s: &SourceView| match s.state.as_str() {
            "on_air" => 0,
            "standby" => 1,
            "link_lost" => 2,
            "test" => 3,
            _ => 4,
        };
        let best = f
            .sources
            .iter()
            .filter(|s| keys.iter().any(|k| k.key_id == s.key_id))
            .min_by_key(|s| (rank(s), s.key_id.clone()));
        match best {
            Some(s) => {
                let mut st = row_state(&s.state);
                // How long it has been steady: the ingest's reckoning, else the desk's.
                let steady = s.steady_ms.or_else(|| {
                    f.people
                        .iter()
                        .find(|p| p.identity == s.identity)
                        .and_then(|p| p.source.as_ref())
                        .map(|d| d.connected_ms)
                });
                if st == "standing_by"
                    && steady.unwrap_or(0) < f.settings.steady_s.max(0) as u64 * 1000
                {
                    st = "connecting";
                }
                (Some(s.label.clone()), st.to_owned())
            }
            None => (Some(keys[0].label.clone()), "not_connected".to_owned()),
        }
    };
    Some(RundownRow {
        show_name: sh.name.clone(),
        host,
        state,
        start_ms: a.start * 1000,
        end_ms: a.end * 1000,
    })
}

fn rundown(f: &Facts, lamp: &str) -> Rundown {
    let Some(mine) = f.my_airing() else {
        return Rundown {
            before: None,
            you: None,
            after: None,
        };
    };
    let before = f
        .airings
        .iter()
        .filter(|a| a.show != mine.show && a.start < mine.start)
        .max_by_key(|a| (a.start, a.show));
    let after = f
        .airings
        .iter()
        .filter(|a| a.show != mine.show && a.start > mine.start)
        .min_by_key(|a| (a.start, a.show));
    Rundown {
        before: before.and_then(|a| row_of(f, a)),
        you: Some(RundownRow {
            show_name: f.show.name.clone(),
            host: Some(f.key.label.clone()),
            state: lamp.to_owned(),
            start_ms: mine.start * 1000,
            end_ms: mine.end * 1000,
        }),
        after: after.and_then(|a| row_of(f, a)),
    }
}

/// The post, projected to what is this host's.
fn project_post(f: &Facts) -> Option<MonitorPost> {
    let p = f.post.as_ref()?;
    let show = p.show.as_ref()?;
    let mine = show.slug == f.show.slug;
    // Only the post that concerns this host: their own show's, or the one they hold the air at.
    if !mine && !f.on_air() {
        return None;
    }
    let me = f.me().map(|p| p.name.clone()).unwrap_or_default();
    let incoming = p.source.as_deref() == Some(f.key.key_id.as_str());
    Some(MonitorPost {
        show_name: show.name.clone(),
        start_ms: show.start_ms,
        started: show.started,
        mine,
        host: p.source_name.clone(),
        host_state: p.state.map(|s| {
            match s {
                SourceState::NotConnected => "not_connected",
                SourceState::Connecting => "connecting",
                SourceState::StandingBy => "standing_by",
                SourceState::OnAir => "on_air",
                SourceState::LinkLost => "link_lost",
            }
            .to_owned()
        }),
        on_air_at_ms: p.on_air_at_ms.filter(|_| incoming || !mine),
        late_ends_ms: p.late_ends_ms.filter(|_| incoming),
        grace_ends_ms: p.grace_ends_ms,
        held_by: p.held_by.clone(),
        on_air: p.holders.iter().filter(|h| **h != me).cloned().collect(),
        heads_up: p
            .window
            .as_ref()
            .filter(|w| incoming && w.show == show.slug && w.start_ms == show.start_ms)
            .and_then(|w| {
                w.heads_up_ms.map(|at_ms| HeadsUpLine {
                    at_ms,
                    words: format!("You are on next, at the start of {}.", w.show_name),
                })
            }),
    })
}

/// The lamp's sentence and what happens next.
fn says(f: &Facts, lamp: &str, post: Option<&MonitorPost>) -> (String, Option<Then>) {
    let behind = f.behind_ms.map(|ms| (ms as f64 / 1000.0).round() as u64);
    match lamp {
        "on_air" => {
            let says = match behind {
                Some(s) => format!("On air. Listeners hear you, about {s} s behind."),
                None => "On air. Listeners hear you.".to_owned(),
            };
            (says, outgoing_then(f, post))
        }
        "open" => {
            let why = if f.playout_unknown {
                "Listeners hear the fallback."
            } else {
                "Automation holds the air, so listeners do not hear you."
            };
            (
                format!("Open. The station hears you. {why}"),
                outgoing_then(f, post),
            )
        }
        "standby" => (
            "Standing by. The station hears you. Listeners do not.".to_owned(),
            standing_then(f, post),
        ),
        "link_lost" => lost_says(f),
        "test" => (
            "Test. The station hears you. Nobody else does.".to_owned(),
            Some(Then {
                kind: "test".into(),
                words: "Your slot's door is not open yet.".into(),
                at_ms: None,
            }),
        ),
        _ => {
            let gone = f
                .log
                .iter()
                .find(|l| weavesignal_model::post::STATE_EVENTS.contains(&l.event.as_str()))
                .filter(|l| l.event == "place_let_go" && f.now_ms - l.at_ms < PLACE_GONE_FOR_MS);
            if let Some(gone) = gone {
                let mins = (f.settings.place_kept_s.max(0) + 59) / 60;
                // The schedule lets a place go when its time is over; a person who pressed
                // *Off air* let it go sooner, so the time kept is not the setting.
                let by_a_press = gone
                    .reason
                    .as_deref()
                    .is_some_and(|r| r != weavesignal_model::post::BY_SCHEDULE);
                return (
                    if by_a_press {
                        "Your place was let go by the station. Connect again and you will be standing by."
                            .to_owned()
                    } else {
                        format!(
                            "Your place was kept for {mins} minute{}. Connect again and you will be standing by.",
                            if mins == 1 { "" } else { "s" }
                        )
                    },
                    Some(Then {
                        kind: "place_gone".into(),
                        words: "Connect again and you will be standing by.".into(),
                        at_ms: None,
                    }),
                );
            }
            (
                "Not connected. Your software has not reached the station.".to_owned(),
                f.my_airing().map(|a| Then {
                    kind: "slot".into(),
                    words: format!("{} is next.", f.show.name),
                    at_ms: Some(a.start * 1000),
                }),
            )
        }
    }
}

fn lost_says(f: &Facts) -> (String, Option<Then>) {
    let kept = f
        .mine
        .as_ref()
        .and_then(|s| s.place_kept_until_ms)
        .or_else(|| {
            f.post
                .as_ref()
                .and_then(|p| p.link_lost.as_ref())
                .filter(|l| l.key_id == f.key.key_id)
                .map(|l| l.place_kept_until_ms)
        });
    let steady = f.settings.steady_s.max(0);
    let heard = if !f.others_on_air().is_empty() {
        ""
    } else if f.automation {
        "Listeners hear automation. "
    } else {
        "Listeners hear the fallback. "
    };
    (
        "The station has lost your stream.".to_owned(),
        Some(Then {
            kind: "link_lost".into(),
            words: format!(
                "{heard}Reconnect your software: you are back on air {steady} s after it is steady."
            ),
            at_ms: kept,
        }),
    )
}

/// What happens next to a host who is on air: the grace, a hold, a next host who has not come.
fn outgoing_then(f: &Facts, post: Option<&MonitorPost>) -> Option<Then> {
    let p = post.filter(|p| !p.mine)?;
    let next = p.host.clone();
    if let Some(by) = &p.held_by {
        return Some(Then {
            kind: "held".into(),
            words: format!("Handover held by {by}. It is by a press only."),
            at_ms: None,
        });
    }
    if let Some(ends) = p.grace_ends_ms {
        let who = next
            .map(|n| format!(" {n} is standing by."))
            .unwrap_or_default();
        return Some(Then {
            kind: "grace".into(),
            words: format!("{} takes the air by the schedule.{who}", p.show_name),
            at_ms: Some(ends),
        });
    }
    let name = next?;
    let standing = matches!(p.host_state.as_deref(), Some("standing_by" | "on_air"));
    if !standing {
        return Some(Then {
            kind: "keeps_air".into(),
            words: format!("{name} has not connected. You keep the air."),
            at_ms: None,
        });
    }
    // Standing by with the grace off: a press only.
    (f.settings.grace_s == 0 && p.started).then(|| Then {
        kind: "press_only".into(),
        words: format!("{name} is standing by. The handover is by a press only."),
        at_ms: None,
    })
}

/// What happens next to a host who is standing by.
fn standing_then(f: &Facts, post: Option<&MonitorPost>) -> Option<Then> {
    let start = f.my_airing().map(|a| a.start * 1000);
    let Some(p) = post.filter(|p| p.mine) else {
        return start.map(|at| Then {
            kind: "at_post".into(),
            words: "You go on air at your show's time, by the schedule.".into(),
            at_ms: Some(at),
        });
    };
    if let Some(by) = &p.held_by {
        return Some(Then {
            kind: "held".into(),
            words: format!("Handover held by {by}. It is by a press only."),
            at_ms: None,
        });
    }
    if let Some(at) = p.late_ends_ms {
        return Some(Then {
            kind: "late_count".into(),
            words: "Automation holds your air.".into(),
            at_ms: Some(at),
        });
    }
    if let Some(at) = p.grace_ends_ms {
        let who = join_names(&p.on_air);
        return Some(Then {
            kind: "grace".into(),
            words: format!(
                "{who} {} still on air. The handover is by a press or by the schedule.",
                is_are(p.on_air.len().max(1))
            ),
            at_ms: Some(at),
        });
    }
    if !p.on_air.is_empty() {
        let who = join_names(&p.on_air);
        return Some(if f.settings.grace_s == 0 && p.started {
            Then {
                kind: "press_only".into(),
                words: format!(
                    "{who} {} on air. The handover is by a press only.",
                    is_are(p.on_air.len())
                ),
                at_ms: None,
            }
        } else {
            Then {
                kind: "waits".into(),
                words: format!("{who} {} still on air.", is_are(p.on_air.len())),
                at_ms: Some(p.start_ms),
            }
        });
    }
    if let Some(at) = p.on_air_at_ms {
        return Some(Then {
            kind: "at_post".into(),
            words: "You go on air at the post, by the schedule.".into(),
            at_ms: Some(at),
        });
    }
    start.map(|at| Then {
        kind: "at_post".into(),
        words: "You go on air at your show's time, by the schedule.".into(),
        at_ms: Some(at),
    })
}

/// The view, from the facts: pure.
pub(crate) fn build(f: &Facts, refusal: Option<LastRefusal>) -> MonitorView {
    let lamp = f.lamp();
    let post = project_post(f);
    let (says, then) = says(f, lamp, post.as_ref());
    let at_desk = f.me().and_then(|p| p.source.as_ref());
    let next = next_host(f).map(|n| NextHostView {
        name: n.name,
        state: n.state.to_owned(),
        cued: n.cued,
    });
    let mine = f.mine.as_ref();
    let rundown = rundown(f, lamp);
    MonitorView {
        label: mine.map_or_else(|| f.key.label.clone(), |s| s.label.clone()),
        show: f.show.slug.clone(),
        show_name: f.show.name.clone(),
        state: lamp.to_owned(),
        says,
        then,
        way: mine
            .and_then(|s| s.way.clone())
            .or_else(|| at_desk.map(|d| d.way.clone())),
        format: mine
            .and_then(|s| s.format.clone())
            .or_else(|| at_desk.map(|d| d.format.clone())),
        level: mine.and_then(|s| s.level),
        leveller_db: mine.and_then(|s| s.leveller_db),
        steady_ms: mine
            .and_then(|s| s.steady_ms)
            .or_else(|| at_desk.map(|d| d.connected_ms)),
        title: mine.and_then(|s| s.title.clone()),
        monitor_open: at_desk.is_some_and(|s| s.monitor),
        door_closes_ms: mine.and_then(|s| s.door_closes_ms),
        last_refusal: refusal,
        post,
        rundown,
        insert: f.insert.clone(),
        step: StepInfo {
            step: f.step,
            word: f.step.word().to_owned(),
            dump: f.step >= Step::Callers,
            handover: f.step >= Step::Full,
        },
        next_host: next,
        listeners_behind_ms: f.behind_ms,
        monitor_has_programme: f.monitor_has_programme(),
        controls: controls(f),
        dumped: dumped(f),
        station: StationClock {
            now_ms: f.now_ms.max(0) as u64,
            zone: f.zone.clone(),
        },
    }
}

/// DUMPED, for a host who is on air in the seconds after a Dump (design section 3.7): how many
/// seconds listeners lost, and to carry on without repeating the words. A host who is not on air
/// said nothing that was cut.
fn dumped(f: &Facts) -> Option<Dumped> {
    let d = f.dump?;
    let age = f.now_ms - d.at_ms;
    if !f.on_air() || !(0..DUMPED_FOR_MS).contains(&age) {
        return None;
    }
    let seconds = (d.lost_ms + 500) / 1000;
    let words = if seconds == 0 {
        "Dumped. Carry on, and do not repeat the words.".to_owned()
    } else {
        format!("Dumped. Listeners lost {seconds} s. Carry on, and do not repeat the words.")
    };
    Some(Dumped {
        at_ms: d.at_ms,
        lost_ms: d.lost_ms,
        seconds,
        words,
    })
}

/// The last refusal of the host's software, from its log (newest first).
pub(crate) fn last_refusal(log: &[LogEntry]) -> Option<LastRefusal> {
    log.iter().find(|e| e.event == "refused").map(|e| {
        let words = e
            .reason
            .as_deref()
            .and_then(Refusal::parse)
            .map_or(String::new(), |r| r.words().to_owned());
        LastRefusal {
            at_ms: e.at_ms,
            reason: e.reason.clone().unwrap_or_default(),
            words,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use weavesignal_model::post::PostShow;

    const NOW: i64 = 1_800_000_000_000;

    fn show(name: &str, slug: &str) -> Show {
        let mut s: Show = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "slug": slug, "name": name, "description": "",
            "colour_slot": 1, "layout": "poster", "slot": null, "hosts": [], "tags": [],
            "explicit": false, "feed_guid": null, "cards": null
        }))
        .expect("a show");
        s.step = Step::Full;
        s
    }

    fn key(show: &Show, label: &str, id: &str) -> ShowKey {
        ShowKey {
            id: Uuid::new_v4(),
            show_id: show.id,
            label: label.into(),
            staff_id: None,
            key_id: id.into(),
            hint: "srt".into(),
            pull_url: None,
            created_by: "t".into(),
            created_ms: 0,
            last_used_ms: None,
            revoked_ms: None,
            ends_on_disconnect: false,
            turned_away_until_ms: None,
            turned_away_by: None,
            key_salt: vec![],
            monitor_hash: vec![],
        }
    }

    fn person(identity: &str, name: &str, role: Role, on_air: bool, ms: Option<u64>) -> DeskPerson {
        let mut v = serde_json::json!({
            "identity": identity, "name": name, "role": role, "on_air": on_air, "talk": false,
            "present": true, "linked": true
        });
        if let Some(ms) = ms {
            v["source"] = serde_json::json!({"way": "srt", "format": "mp3", "key": "k", "show": "x",
                "connected_ms": ms, "leveller_db": 0.0});
        }
        serde_json::from_value(v).expect("a person")
    }

    fn row(key: &ShowKey, show: &Show, state: &str) -> SourceView {
        SourceView {
            key_id: key.key_id.clone(),
            identity: source_wire::identity(&key.key_id),
            label: key.label.clone(),
            show: show.slug.clone(),
            show_name: show.name.clone(),
            state: state.into(),
            slot: Some(5),
            way: Some("srt".into()),
            pull: false,
            format: Some("mp3".into()),
            level: None,
            leveller_db: None,
            steady_ms: Some(600_000),
            monitor_open: false,
            door_closes_ms: None,
            title: None,
            turned_away_until_ms: None,
            place_kept_until_ms: None,
        }
    }

    /// Steeped (this host, Finlay) airs `[0, 1h)` from `NOW - start_ago`; Kettle Drive follows.
    struct World {
        f: Facts,
    }

    fn world(lamp: &str, on_air_people: &[(&str, &str, Role)], step: Step) -> World {
        let steeped = show("Steeped", "steeped");
        let kettle = show("Kettle Drive", "kettle");
        let k = key(&steeped, "Finlay Grant", "finlay0001");
        let k2 = key(&kettle, "Heather Douglas", "heather001");
        let now_s = NOW / 1000;
        let mut people = vec![person(
            &source_wire::identity(&k.key_id),
            "Finlay Grant",
            Role::Source,
            matches!(lamp, "on_air"),
            Some(600_000),
        )];
        for (id, name, role) in on_air_people {
            people.push(person(id, name, *role, true, None));
        }
        let f = Facts {
            now_ms: NOW,
            mine: Some(row(
                &k,
                &steeped,
                if lamp == "on_air" { "on_air" } else { lamp },
            )),
            key: k.clone(),
            show: steeped.clone(),
            people,
            slots: HashMap::new(),
            post: None,
            step,
            automation: true,
            auto_mode: false,
            playout_unknown: false,
            settings: SourceSettings::default(),
            airings: vec![
                Airing {
                    show: steeped.id,
                    start: now_s - 600,
                    end: now_s + 3000,
                },
                Airing {
                    show: kettle.id,
                    start: now_s + 3000,
                    end: now_s + 6600,
                },
            ],
            shows: vec![steeped, kettle],
            keys: vec![k, k2],
            sources: vec![],
            log: vec![],
            insert: None,
            behind_ms: Some(23_000),
            dump: None,
            zone: "Europe/London".into(),
        };
        World { f }
    }

    fn kettle_post(f: &mut Facts, state: SourceState, grace: Option<i64>, held: Option<&str>) {
        f.post = Some(PostView {
            show: Some(PostShow {
                slug: "kettle".into(),
                name: "Kettle Drive".into(),
                start_ms: NOW + 3_000_000,
                started: false,
            }),
            source: Some("heather001".into()),
            source_name: Some("Heather Douglas".into()),
            state: Some(state),
            standing_since_ms: None,
            on_air_at_ms: None,
            grace_ends_ms: grace,
            held_by: held.map(str::to_owned),
            late_ends_ms: None,
            holders: vec!["Finlay Grant".into()],
            link_lost: None,
            window: None,
        });
        f.sources.push(row(&f.keys[1], &f.shows[1], "standby"));
        f.people.push(person(
            "source-heather001",
            "Heather Douglas",
            Role::Source,
            false,
            Some(600_000),
        ));
    }

    #[test]
    fn the_monitor_carries_the_programme_only_when_someone_is_on_air_at_the_desk() {
        // Standing by with nobody on air: automation holds the air, which the desk does not
        // make, so the mix is silent and the page plays the listeners' stream.
        let mut w = world("standby", &[], Step::Full);
        assert!(!build(&w.f, None).monitor_has_programme);
        // A presenter on air in the room: their voice is in the mix, live.
        w.f.people
            .push(person("hamish", "Hamish", Role::Presenter, true, None));
        assert!(build(&w.f, None).monitor_has_programme);
        // Muted, away, or only cued: not in the programme, so not in the mix.
        w.f.people.last_mut().unwrap().mute = true;
        assert!(!build(&w.f, None).monitor_has_programme);
        w.f.people.last_mut().unwrap().mute = false;
        w.f.people.last_mut().unwrap().away_ms = Some(5000);
        assert!(!build(&w.f, None).monitor_has_programme);
        // On air at the desk while automation holds the air: listeners do not hear them.
        w.f.people.last_mut().unwrap().away_ms = None;
        w.f.auto_mode = true;
        assert!(!build(&w.f, None).monitor_has_programme);
        // Another host's own studio on air counts as a person in the room does.
        w.f.auto_mode = false;
        w.f.people.pop();
        w.f.people.push(person(
            "source-heather001",
            "Heather Douglas",
            Role::Source,
            true,
            Some(1000),
        ));
        assert!(build(&w.f, None).monitor_has_programme);
    }

    #[test]
    fn an_open_host_has_no_programme_in_the_monitor_so_the_page_plays_the_stream() {
        // OPEN by automation: someone in the room is on air at the desk, which listeners do not
        // hear, so the desk's mix for this host is not the programme, whoever is in it.
        let mut w = world(
            "on_air",
            &[("hamish", "Hamish", Role::Presenter)],
            Step::Full,
        );
        w.f.auto_mode = true;
        let v = build(&w.f, None);
        assert_eq!(v.state, "open");
        assert!(!v.monitor_has_programme);
        // OPEN because playout is down (listeners hear the fallback): the same.
        let mut w = world(
            "on_air",
            &[("hamish", "Hamish", Role::Presenter)],
            Step::Full,
        );
        w.f.playout_unknown = true;
        let v = build(&w.f, None);
        assert_eq!(v.state, "open");
        assert!(!v.monitor_has_programme);
        // On air for real, with someone else on air too: the desk's mix is the programme.
        let w = world(
            "on_air",
            &[("hamish", "Hamish", Role::Presenter)],
            Step::Full,
        );
        let v = build(&w.f, None);
        assert_eq!(v.state, "on_air");
        assert!(v.monitor_has_programme);
    }

    #[test]
    fn standing_by_the_station_hears_you_and_listeners_do_not() {
        let w = world("standby", &[], Step::Full);
        let v = build(&w.f, None);
        assert_eq!(v.state, "standby");
        assert_eq!(
            v.says,
            "Standing by. The station hears you. Listeners do not."
        );
        let then = v.then.expect("a count to the show's time");
        assert_eq!(then.kind, "at_post");
        // Not on air: nothing but Go (the show has started and nobody else is on air).
        assert!(v.controls.go.may, "{:?}", v.controls.go);
        for c in [
            &v.controls.cue,
            &v.controls.bring,
            &v.controls.hand_over,
            &v.controls.end_show,
            &v.controls.dump,
        ] {
            assert_eq!(c.why_not.as_deref(), Some("You are not on air."));
        }
        assert!(!v.controls.not_yet.may, "no count is running");
    }

    #[test]
    fn go_waits_for_the_shows_time_and_for_whoever_is_on_air() {
        let mut w = world("standby", &[], Step::Full);
        // The show is not on yet.
        w.f.airings[0] = Airing {
            show: w.f.show.id,
            start: NOW / 1000 + 1800,
            end: NOW / 1000 + 5400,
        };
        let c = controls(&w.f);
        assert!(!c.go.may);
        assert_eq!(
            c.go.why_not.as_deref(),
            Some("Your show has not started. You go on air at its time.")
        );
        // The show's time has come but a DJ holds the air.
        let mut w = world("standby", &[("dj-1", "Duncan Boyd", Role::Dj)], Step::Full);
        let c = controls(&w.f);
        assert!(!c.go.may);
        assert_eq!(
            c.go.why_not.as_deref(),
            Some(
                "Duncan Boyd is still on air. You take the air when they hand over, or when the schedule does."
            )
        );
        // Not steady yet.
        w.f.people.retain(|p| p.role != Role::Dj);
        w.f.people[0] = person(
            "source-finlay0001",
            "Finlay Grant",
            Role::Source,
            false,
            Some(2_000),
        );
        assert!(!controls(&w.f).go.may);
    }

    #[test]
    fn the_late_count_is_the_hosts_to_stop_and_only_theirs() {
        let mut w = world("standby", &[], Step::Full);
        let mut p = PostView {
            show: Some(PostShow {
                slug: "steeped".into(),
                name: "Steeped".into(),
                start_ms: NOW - 600_000,
                started: true,
            }),
            source: Some("finlay0001".into()),
            source_name: Some("Finlay Grant".into()),
            state: Some(SourceState::StandingBy),
            standing_since_ms: None,
            on_air_at_ms: Some(NOW + 12_000),
            grace_ends_ms: None,
            held_by: None,
            late_ends_ms: Some(NOW + 12_000),
            holders: vec![],
            link_lost: None,
            window: None,
        };
        w.f.post = Some(p.clone());
        let v = build(&w.f, None);
        assert!(v.controls.not_yet.may && v.controls.go.may);
        let then = v.then.expect("the count");
        assert_eq!(
            (then.kind.as_str(), then.at_ms),
            ("late_count", Some(NOW + 12_000))
        );
        assert_eq!(then.words, "Automation holds your air.");
        // The count is another host's: this one has nothing to stop.
        p.source = Some("someone-else".into());
        w.f.post = Some(p);
        assert!(!controls(&w.f).not_yet.may);
    }

    #[test]
    fn on_air_with_the_grace_running_says_who_takes_the_air_and_when() {
        let mut w = world("on_air", &[], Step::Full);
        kettle_post(&mut w.f, SourceState::StandingBy, Some(NOW + 42_000), None);
        let v = build(&w.f, None);
        assert_eq!(v.state, "on_air");
        assert_eq!(v.says, "On air. Listeners hear you, about 23 s behind.");
        let then = v.then.expect("the grace");
        assert_eq!(then.kind, "grace");
        assert_eq!(
            then.words,
            "Kettle Drive takes the air by the schedule. Heather Douglas is standing by."
        );
        assert_eq!(then.at_ms, Some(NOW + 42_000));
        let next = v.next_host.expect("the next host");
        assert_eq!(
            (next.name.as_str(), next.state.as_str(), next.cued),
            ("Heather Douglas", "standing_by", false)
        );
        assert!(
            v.controls.cue.may
                && v.controls.bring.may
                && v.controls.end_show.may
                && v.controls.dump.may
        );
        assert!(!v.controls.hand_over.may, "bring them on first");
        assert_eq!(
            v.controls.hand_over.why_not.as_deref(),
            Some("Bring Heather Douglas on air first.")
        );
        assert_eq!(
            v.controls.end_show_says.as_deref(),
            Some("Heather Douglas is standing by: the air passes to them as you leave.")
        );
        // Held: a press only.
        kettle_post(&mut w.f, SourceState::StandingBy, None, Some("Shona"));
        w.f.sources.pop();
        w.f.people.pop();
        let v = build(&w.f, None);
        assert_eq!(
            v.then.expect("held").words,
            "Handover held by Shona. It is by a press only."
        );
        // Heather has not connected: this host keeps the air.
        w.f.people.pop();
        kettle_post(&mut w.f, SourceState::NotConnected, None, None);
        w.f.people.retain(|p| p.identity != "source-heather001");
        let v = build(&w.f, None);
        assert_eq!(
            v.then.expect("keeps").words,
            "Heather Douglas has not connected. You keep the air."
        );
        assert!(!v.controls.cue.may && !v.controls.bring.may);
        assert_eq!(
            v.controls.bring.why_not.as_deref(),
            Some("Heather Douglas has not connected yet.")
        );
    }

    #[test]
    fn the_heads_up_is_on_the_incoming_hosts_link_only() {
        use weavesignal_model::handover_window::{Origin, State, WindowView};
        let mut w = world("on_air", &[], Step::Full);
        kettle_post(&mut w.f, SourceState::StandingBy, None, None);
        w.f.post.as_mut().unwrap().window = Some(WindowView {
            state: State::Ready,
            show: "kettle".into(),
            show_name: "Kettle Drive".into(),
            start_ms: NOW + 3_000_000,
            opens_ms: NOW + 2_700_000,
            host: "Heather Douglas".into(),
            from: Origin::Push,
            heads_up_ms: Some(NOW),
            late: None,
        });
        // Finlay holds the air: the line is not his.
        let his = project_post(&w.f).expect("the post he holds the air at");
        assert!(his.heads_up.is_none());
        // Heather's own link carries it.
        w.f.key = w.f.keys[1].clone();
        w.f.show = w.f.shows[1].clone();
        let hers = project_post(&w.f)
            .expect("her post")
            .heads_up
            .expect("the line");
        assert_eq!(
            (hers.at_ms, hers.words.as_str()),
            (NOW, "You are on next, at the start of Kettle Drive.")
        );
        // Not given yet: no line.
        w.f.post
            .as_mut()
            .unwrap()
            .window
            .as_mut()
            .unwrap()
            .heads_up_ms = None;
        assert!(project_post(&w.f).unwrap().heads_up.is_none());
    }

    #[test]
    fn below_full_there_is_no_overlap_and_below_callers_no_dump() {
        let mut w = world("on_air", &[], Step::Solo);
        kettle_post(&mut w.f, SourceState::StandingBy, None, None);
        let c = controls(&w.f);
        assert_eq!(
            c.bring.why_not.as_deref(),
            Some("The overlap is off at Solo. It comes on at Full.")
        );
        assert_eq!(
            c.hand_over.why_not.as_deref(),
            Some("The overlap is off at Solo. It comes on at Full.")
        );
        assert_eq!(
            c.dump.why_not.as_deref(),
            Some("Dump is off at Solo. It comes on at Callers.")
        );
        assert!(c.end_show.may, "End my show is there at every step");
        assert!(c.cue.may, "and so is Cue");
        let v = build(&w.f, None);
        assert_eq!((v.step.dump, v.step.handover), (false, false));
        // Callers has Dump and no overlap; Full has both.
        let mut w2 = world("on_air", &[], Step::Callers);
        kettle_post(&mut w2.f, SourceState::StandingBy, None, None);
        let c = controls(&w2.f);
        assert!(c.dump.may && !c.bring.may);
        let mut w3 = world("on_air", &[], Step::Full);
        kettle_post(&mut w3.f, SourceState::StandingBy, None, None);
        let c = controls(&w3.f);
        assert!(c.dump.may && c.bring.may);
    }

    #[test]
    fn the_second_press_wants_the_next_on_air_beside_them() {
        let mut w = world("on_air", &[], Step::Full);
        kettle_post(&mut w.f, SourceState::StandingBy, None, None);
        w.f.people.last_mut().expect("heather").on_air = true;
        let c = controls(&w.f);
        assert!(c.hand_over.may);
        assert_eq!(
            c.bring.why_not.as_deref(),
            Some("Heather Douglas is on air already.")
        );
        assert!(!c.cue.may);
        // The second press takes this host off, and not the one brought on.
        assert_eq!(handover_takeoff(&w.f), ["source-finlay0001"]);
    }

    #[test]
    fn a_dj_in_the_room_is_the_next_when_the_post_names_no_one() {
        let mut w = world("on_air", &[("dj-9", "Duncan Boyd", Role::Dj)], Step::Full);
        // The DJ in the room is on air with the host (the overlap): the second press is on offer.
        let c = controls(&w.f);
        assert!(c.hand_over.may && !c.bring.may, "{c:?}");
        // Waiting off air: Bring is on offer.
        w.f.people.last_mut().expect("dj").on_air = false;
        let c = controls(&w.f);
        assert!(c.bring.may && c.cue.may, "{c:?}");
        assert_eq!(
            c.end_show_says.as_deref(),
            Some("Automation takes the air."),
            "End show passes the air to the post's host only"
        );
        let t = next_target(&w.f).expect("a target");
        assert_eq!((t.identity.as_str(), t.subject.as_str()), ("dj-9", "dj-9"));
    }

    #[test]
    fn open_is_not_on_air_while_automation_holds_the_air() {
        let mut w = world("on_air", &[], Step::Full);
        w.f.auto_mode = true;
        let v = build(&w.f, None);
        assert_eq!(v.state, "open");
        assert!(
            v.says.starts_with("Open. The station hears you."),
            "{}",
            v.says
        );
        // Playout down: the fallback.
        let mut w = world("on_air", &[], Step::Full);
        w.f.playout_unknown = true;
        assert_eq!(build(&w.f, None).state, "open");
        // No playout layer, never OPEN by the mode.
        let mut w = world("on_air", &[], Step::Full);
        w.f.automation = false;
        w.f.auto_mode = true;
        assert_eq!(build(&w.f, None).state, "on_air");
    }

    #[test]
    fn a_lost_link_says_what_listeners_hear_and_until_when() {
        let mut w = world("link_lost", &[], Step::Full);
        w.f.people.clear();
        w.f.mine.as_mut().expect("row").place_kept_until_ms = Some(NOW + 290_000);
        let v = build(&w.f, None);
        assert_eq!(v.state, "link_lost");
        assert_eq!(v.says, "The station has lost your stream.");
        let then = v.then.expect("what happens");
        assert_eq!(
            then.words,
            "Listeners hear automation. Reconnect your software: you are back on air 5 s after it is steady."
        );
        assert_eq!(then.at_ms, Some(NOW + 290_000));
        // Someone else on air: no line on automation.
        let mut w = world(
            "link_lost",
            &[("dj-1", "Duncan Boyd", Role::Dj)],
            Step::Full,
        );
        w.f.mine.as_mut().expect("row").place_kept_until_ms = Some(NOW + 290_000);
        let then = build(&w.f, None).then.expect("what happens");
        assert_eq!(
            then.words,
            "Reconnect your software: you are back on air 5 s after it is steady."
        );
        assert!(!controls(&w.f).end_show.may);
    }

    #[test]
    fn the_rundown_is_before_you_after_you_and_leaves_out_nobody_it_does_not_know() {
        let mut w = world("standby", &[], Step::Full);
        kettle_post(&mut w.f, SourceState::StandingBy, None, None);
        let v = build(&w.f, None);
        assert!(v.rundown.before.is_none(), "nothing before");
        assert_eq!(v.rundown.you.as_ref().expect("you").state, "standby");
        let after = v.rundown.after.expect("after");
        assert_eq!(
            (
                after.show_name.as_str(),
                after.host.as_deref(),
                after.state.as_str()
            ),
            ("Kettle Drive", Some("Heather Douglas"), "standing_by")
        );
        // Not connected yet.
        let mut w = world("standby", &[], Step::Full);
        let after = build(&w.f, None).rundown.after.expect("after");
        assert_eq!(after.state, "not_connected");
        // Nothing after: automation follows.
        w.f.airings.truncate(1);
        assert!(build(&w.f, None).rundown.after.is_none());
        // A show from the room.
        let mut w = world("standby", &[], Step::Full);
        w.f.keys.pop();
        w.f.shows[1].presenter = "Duncan Boyd".into();
        let after = build(&w.f, None).rundown.after.expect("after");
        assert_eq!(
            (after.state.as_str(), after.host.as_deref()),
            ("in_room", Some("Duncan Boyd"))
        );
    }

    #[test]
    fn nothing_of_another_hosts_keys_is_in_the_view() {
        let mut w = world("on_air", &[], Step::Full);
        kettle_post(&mut w.f, SourceState::StandingBy, Some(NOW + 42_000), None);
        let text = serde_json::to_string(&build(&w.f, None)).expect("json");
        for forbidden in ["heather001", "source-heather", "monitor_hash", "\"key_id\""] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
        // The names the design shows on the page are there.
        assert!(text.contains("Heather Douglas") && text.contains("Kettle Drive"));
    }

    #[test]
    fn dumped_is_said_for_a_few_seconds_to_the_host_on_air_and_to_no_one_else() {
        use crate::hub::DumpRecord;
        let mut w = world("on_air", &[], Step::Full);
        assert!(build(&w.f, None).dumped.is_none(), "no dump yet");
        w.f.dump = Some(DumpRecord {
            at_ms: NOW - 2_000,
            lost_ms: 8_200,
        });
        let d = build(&w.f, None).dumped.expect("dumped");
        assert_eq!((d.seconds, d.lost_ms), (8, 8_200));
        assert_eq!(
            d.words,
            "Dumped. Listeners lost 8 s. Carry on, and do not repeat the words."
        );
        // Under a second lost: no figure.
        w.f.dump = Some(DumpRecord {
            at_ms: NOW - 1,
            lost_ms: 300,
        });
        assert_eq!(
            build(&w.f, None).dumped.expect("dumped").words,
            "Dumped. Carry on, and do not repeat the words."
        );
        // Over by ten seconds.
        w.f.dump = Some(DumpRecord {
            at_ms: NOW - DUMPED_FOR_MS,
            lost_ms: 8_200,
        });
        assert!(build(&w.f, None).dumped.is_none());
        // A host who is standing by said nothing that was cut.
        let mut w = world("standby", &[], Step::Full);
        w.f.dump = Some(DumpRecord {
            at_ms: NOW - 2_000,
            lost_ms: 8_200,
        });
        assert!(build(&w.f, None).dumped.is_none());
    }

    #[test]
    fn a_place_let_go_by_a_press_is_not_said_to_have_been_kept_for_the_whole_time() {
        let line = |reason: &str| LogEntry {
            at_ms: NOW - 60_000,
            key_id: "finlay0001".into(),
            event: "place_let_go".into(),
            reason: Some(reason.into()),
            detail: String::new(),
            way: None,
            address: None,
        };
        let mut w = world("not_connected", &[], Step::Full);
        w.f.mine = None;
        w.f.log = vec![line(weavesignal_model::post::BY_SCHEDULE)];
        let by_time = build(&w.f, None);
        assert_eq!(
            by_time.says,
            "Your place was kept for 5 minutes. Connect again and you will be standing by."
        );
        w.f.log = vec![line("shona")];
        let by_press = build(&w.f, None);
        assert_eq!(
            by_press.says,
            "Your place was let go by the station. Connect again and you will be standing by."
        );
        assert_eq!(by_press.then.expect("then").kind, "place_gone");
    }
}
