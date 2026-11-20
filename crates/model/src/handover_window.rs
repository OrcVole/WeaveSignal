// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The handover window's rule: how ready the incoming host's slot is, read from the clock, the
//! schedule's airings and a few facts. This is the decision only, a pure function of its inputs
//! — no I/O and no clock read here (the clock comes in as `now_s`), so the same facts always
//! give the same window. The control plane reads it every second and carries out whatever state
//! it returns.
//!
//! The **target** is the airing whose post has come: one under way when the incoming host is not
//! already on air, else the next airing to start. The window opens `window_s` seconds before the
//! target starts; as that time passes the incoming host moves from `Announced` to `Cueing` to
//! `Ready`. If the target is already on air the window reports `Overrun` while the outgoing host
//! still presents, or `Held` while a producer holds the handover.
//!
//! **At the post's side** ([`for_post`]): the control plane does not hand [`window`] the whole
//! schedule. It picks the target with the post's own rule ([`crate::post::post_airing`]), so the
//! window is always about the show `PostView` names, and hands [`window`] that one airing, the
//! readiness the post's rule reported for its source, and a hold only if it is on that airing.
//! (Given the whole schedule, the next host's readiness would also pick the target, and a show on
//! air with the next host not yet connected would read as `Overrun`.) It also says when the
//! incoming host is due their **heads-up**, the design's one cue an airing: the control plane
//! writes `heads_up` under the post's key first and tells the host only once the line is down,
//! so the log, not memory, says whether it was given.
//!
//! **The rota** ([`rota`]) is the same facts for every airing of the coming hours: who is down
//! for each, from where, and how far they have got. It decides nothing.

use crate::post::{self, Hold, Holder, PostView, ShowFact, SourceState, Subject};
use crate::sources::Airing;
use serde::Serialize;
use uuid::Uuid;

/// How ready the incoming host is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = HandoverReadiness)]
pub enum Readiness {
    /// Not connected to the desk yet.
    NotConnected,
    /// Connecting to the desk.
    Connecting,
    /// At the desk and ready to go on air.
    Ready,
    /// Already on air.
    OnAir,
}

/// The facts the handover window is read from.
pub struct Facts<'a> {
    /// The clock, in epoch seconds.
    pub now_s: i64,
    /// The window's length in seconds; 0 turns the window off (always `Quiet`).
    pub window_s: i64,
    /// The airings in air order.
    pub airings: &'a [Airing],
    /// The incoming host's readiness (the host of the target airing, see below).
    pub incoming: Readiness,
    /// Someone other than the incoming host is on air and presenting.
    pub outgoing_on_air: bool,
    /// A producer holds this handover.
    pub held: bool,
}

/// What the handover window is doing at `now_s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = HandoverWindowState)]
pub enum State {
    /// The window is idle — nothing to cue.
    Quiet,
    /// The target's post has come and the incoming host is told to get ready.
    Announced,
    /// The incoming host is connecting to the desk.
    Cueing,
    /// The incoming host is ready to go on air.
    Ready,
    /// The target's post is still running and the outgoing host has not handed over.
    Overrun,
    /// A producer is holding the handover.
    Held,
}

/// The handover window's state at `now_s`, with the target it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The window's state.
    pub state: State,
    /// The target airing's show, if the window has a target; `None` when it is off or has none.
    pub show: Option<Uuid>,
    /// The target airing's start, in epoch seconds, when `show` is set.
    pub start_s: Option<i64>,
    /// When the window opened for the target (`target.start - window_s`), when `show` is set.
    pub opens_s: Option<i64>,
}

/// The state of the handover window at `f.now_s`, for the schedule's next target airing.
pub fn window(f: &Facts<'_>) -> Window {
    if f.window_s <= 0 {
        return Window {
            state: State::Quiet,
            show: None,
            start_s: None,
            opens_s: None,
        };
    }
    let now = f.now_s;
    // The target is an airing under way whose post has come (the incoming host is not on air),
    // else the first airing still to start.
    let target = f
        .airings
        .iter()
        .find(|a| now >= a.start && now < a.end && f.incoming != Readiness::OnAir)
        .or_else(|| f.airings.iter().find(|a| a.start > now));
    let Some(target) = target else {
        return Window {
            state: State::Quiet,
            show: None,
            start_s: None,
            opens_s: None,
        };
    };
    let opens = target.start - f.window_s;
    let carry = Window {
        state: State::Quiet,
        show: Some(target.show),
        start_s: Some(target.start),
        opens_s: Some(opens),
    };
    if now >= target.start && now < target.end {
        // The target's post has come.
        if f.held {
            Window {
                state: State::Held,
                ..carry
            }
        } else if f.outgoing_on_air {
            Window {
                state: State::Overrun,
                ..carry
            }
        } else {
            Window {
                state: State::Quiet,
                ..carry
            }
        }
    } else if now < opens {
        // The target is still to come and the window has not opened yet.
        carry
    } else if f.held {
        Window {
            state: State::Held,
            ..carry
        }
    } else {
        match f.incoming {
            Readiness::NotConnected => Window {
                state: State::Announced,
                ..carry
            },
            Readiness::Connecting => Window {
                state: State::Cueing,
                ..carry
            },
            Readiness::Ready | Readiness::OnAir => Window {
                state: State::Ready,
                ..carry
            },
        }
    }
}

// ---- at the post's side ----

/// Where a show's host comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = HostOrigin)]
pub enum Origin {
    /// The room: a presenter or DJ at the desk.
    Room,
    /// Their own studio, sending with a show key (ADR-0280).
    Push,
}

/// Who is down for a show: its hosts by name (a key's label each, or the show's presenter) and
/// where they come from. A show with no names has nobody to announce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Down {
    pub show: Uuid,
    pub names: Vec<String>,
    pub from: Origin,
}

/// The handover window as `GET /studio` and `GET /sources` carry it, inside `post`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = HandoverWindow)]
pub struct WindowView {
    /// `quiet` (before the window opens), `announced` (not here yet), `cueing` (connecting),
    /// `ready`, `overrun` (the post has come and the outgoing host still presents) or `held`.
    pub state: State,
    /// The show it is about: the post's show (`post.show`), or the next show once that one's
    /// window has opened while the post's show is still waiting for a host who never came.
    pub show: String,
    pub show_name: String,
    /// The post: the show's start.
    pub start_ms: i64,
    /// When the window opens (`start_ms` less the rules' `window_s`).
    pub opens_ms: i64,
    /// Who is down for it, by name: the source the post's rule names, else every name down.
    pub host: String,
    /// `room` or `push`.
    pub from: Origin,
    /// When the incoming host was given their heads-up for this airing (once an airing).
    pub heads_up_ms: Option<i64>,
}

/// The heads-up the control plane is to give now, once it has written it down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadsUp {
    pub show: Uuid,
    /// The airing's start, in epoch seconds: the line's `airing <start>`, which keeps it once.
    pub airing_start_s: i64,
    /// The post's source (a key's id, or a room presenter's identity), if one is known.
    pub subject: Option<String>,
    /// Its desk identity, for the notice on its seat.
    pub identity: Option<String>,
    /// Who it is for, by name.
    pub name: String,
    /// The desk notice's title and message.
    pub title: String,
    pub message: String,
}

/// The facts the window at the post is read from: the post rule's own facts and answer, who is
/// down for each show, and the heads-ups the log holds.
pub struct PostFacts<'a> {
    pub now_ms: i64,
    /// The rules' `window_s`; 0 is off.
    pub window_s: i64,
    pub airings: &'a [Airing],
    pub shows: &'a [ShowFact],
    pub subjects: &'a [Subject],
    pub holders: &'a [Holder],
    pub hold: Option<&'a Hold>,
    /// The post's view, as [`crate::post::plan`] gave it.
    pub post: Option<&'a PostView>,
    pub down: &'a [Down],
    /// The heads-ups written down: `(airing start in epoch seconds, when)`.
    pub told: &'a [(i64, i64)],
    /// The rules' steady time, in ms: how a source of a show the post does not name is read.
    pub steady_ms: u64,
}

/// The window at the post, and a heads-up when one is due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtPost {
    pub view: Option<WindowView>,
    pub heads_up: Option<HeadsUp>,
}

/// How far the post's source has got, as the window reads it.
pub fn readiness(state: Option<SourceState>) -> Readiness {
    match state {
        Some(SourceState::OnAir) => Readiness::OnAir,
        Some(SourceState::StandingBy) => Readiness::Ready,
        Some(SourceState::Connecting) => Readiness::Connecting,
        Some(SourceState::NotConnected | SourceState::LinkLost) | None => Readiness::NotConnected,
    }
}

fn readiness_words(r: Readiness) -> &'static str {
    match r {
        Readiness::NotConnected => "not here yet",
        Readiness::Connecting => "connecting",
        Readiness::Ready => "ready",
        Readiness::OnAir => "on air",
    }
}

/// The handover window for the post's show, and the heads-up due now (see the module's note).
pub fn for_post(f: &PostFacts<'_>) -> AtPost {
    let none = AtPost {
        view: None,
        heads_up: None,
    };
    if f.window_s <= 0 {
        return none;
    }
    let now_s = f.now_ms.div_euclid(1000);
    let Some(mut target) = post::post_airing(f.airings, f.subjects, now_s) else {
        return none;
    };
    // The post's show is under way and its host has not taken the air. The window moves on to
    // the next show once that one's window has opened (a host who never came must not hide the
    // next), or once the airing has been dealt with since its start (its host went on air and
    // came off again: nobody is running over into it).
    if (target.start..target.end).contains(&now_s) {
        let dealt_with = f.subjects.iter().any(|s| {
            s.show == target.show
                && s.log.iter().any(|l| {
                    l.at_ms >= target.start * 1000
                        && matches!(l.event.as_str(), "on_air" | "back" | "off_air" | "not_yet")
                })
        });
        let next = post::airing_next(f.airings, now_s);
        let next_open = next.is_some_and(|n| now_s >= n.start - f.window_s);
        if dealt_with || next_open {
            let Some(n) = next else {
                return none;
            };
            target = n;
        }
    }
    let Some(show) = f.shows.iter().find(|s| s.id == target.show) else {
        return none;
    };
    // The post's view is about this airing (it picks it the same way); its source is the host.
    let post = f.post.filter(|p| {
        p.show
            .as_ref()
            .is_some_and(|s| s.start_ms == target.start * 1000 && s.slug == show.slug)
    });
    let down = f.down.iter().find(|d| d.show == target.show);
    // The target's own hosts, furthest first: the one to tell when the post names none.
    let mut theirs_now: Vec<&Subject> = f
        .subjects
        .iter()
        .filter(|s| s.show == target.show)
        .collect();
    theirs_now.sort_by_key(|s| {
        (
            std::cmp::Reverse(rank(show_readiness(&[*s], f.steady_ms))),
            s.id.clone(),
        )
    });
    let lead = theirs_now.first().copied();
    let host = post
        .and_then(|p| p.source_name.clone())
        .or_else(|| lead.map(|s| s.name.clone()))
        .filter(|n| !n.is_empty())
        .or_else(|| down.map(|d| d.names.join(", ")).filter(|n| !n.is_empty()));
    let Some(host) = host else {
        // Nobody is down for the show: nothing to announce, nobody to tell.
        return none;
    };
    let from = down.map_or_else(
        || {
            if f.subjects
                .iter()
                .any(|s| s.show == target.show && !s.in_room)
            {
                Origin::Push
            } else {
                Origin::Room
            }
        },
        |d| d.from,
    );
    // The post's own reading when it is about this airing; else the furthest of its hosts.
    let incoming = match post {
        Some(p) => readiness(p.state),
        None => show_readiness(&theirs_now, f.steady_ms),
    };
    let theirs = |identity: &str| {
        f.subjects
            .iter()
            .any(|s| s.show == target.show && s.identity == identity)
    };
    let w = window(&Facts {
        now_s,
        window_s: f.window_s,
        airings: std::slice::from_ref(target),
        incoming,
        outgoing_on_air: f.holders.iter().any(|h| !theirs(&h.identity)),
        held: f.hold.is_some_and(|h| h.airing_start_s == target.start),
    });
    let (Some(start_s), Some(opens_s)) = (w.start_s, w.opens_s) else {
        return none;
    };
    let told = f
        .told
        .iter()
        .filter(|(start, _)| *start == start_s)
        .map(|(_, at)| *at)
        .min();
    let view = WindowView {
        state: w.state,
        show: show.slug.clone(),
        show_name: show.name.clone(),
        start_ms: start_s * 1000,
        opens_ms: opens_s * 1000,
        host: host.clone(),
        from,
        heads_up_ms: told,
    };
    // Due once the window has opened and before the post (a hold before the post does not stop
    // it: the host still has to get ready), unless they are on air already or have been told.
    let due = w.state != State::Quiet
        && (opens_s..start_s).contains(&now_s)
        && incoming != Readiness::OnAir
        && told.is_none();
    let heads_up = due.then(|| {
        let subject = post
            .and_then(|p| p.source.clone())
            .or_else(|| lead.map(|s| s.id.clone()));
        let identity = subject.as_ref().and_then(|id| {
            f.subjects
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.identity.clone())
        });
        let minutes = ((start_s * 1000 - f.now_ms).max(0) + 59_999) / 60_000;
        HeadsUp {
            show: target.show,
            airing_start_s: start_s,
            subject,
            identity,
            name: host.clone(),
            title: format!("Next: {host}, {}", show.name),
            message: format!(
                "{} starts in {minutes} min; {host} is {}.",
                show.name,
                readiness_words(incoming)
            ),
        }
    });
    AtPost {
        view: Some(view),
        heads_up,
    }
}

/// The `detail` of a `heads_up` line: `airing <start> to <name>`.
pub fn heads_up_detail(h: &HeadsUp) -> String {
    format!("airing {} to {}", h.airing_start_s, h.name)
}

/// The airing a `heads_up` line's `detail` names, in epoch seconds.
pub fn heads_up_airing(detail: &str) -> Option<i64> {
    let rest = detail.strip_prefix("airing ")?;
    rest.split(' ').next()?.parse().ok()
}

// ---- the rota ----

/// One airing on the rota.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RotaRow {
    pub show: String,
    pub show_name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Who is down for it, by name: a key's label each, or the show's presenter. Empty when
    /// nobody is (automation, or a show nobody has been put down for).
    pub hosts: Vec<String>,
    /// `room` or `push`.
    pub from: Origin,
    /// How far the host has got now: `not_connected`, `connecting`, `ready` or `on_air`.
    pub state: Readiness,
    /// Ready or on air.
    pub ready: bool,
    /// On air by the schedule now.
    pub now: bool,
}

fn rank(r: Readiness) -> u8 {
    match r {
        Readiness::OnAir => 3,
        Readiness::Ready => 2,
        Readiness::Connecting => 1,
        Readiness::NotConnected => 0,
    }
}

/// How far a show's hosts have got: the furthest of them. A source is ready once steady for
/// `steady_ms`; a person in the room once at the desk.
pub fn show_readiness(subjects: &[&Subject], steady_ms: u64) -> Readiness {
    let one = |s: &Subject| match &s.at_desk {
        Some(d) if d.on_air => Readiness::OnAir,
        Some(d) if d.here && d.connected_ms.is_none_or(|ms| ms >= steady_ms) => Readiness::Ready,
        Some(_) => Readiness::Connecting,
        None if s.since_ms.is_some() => Readiness::Connecting,
        None => Readiness::NotConnected,
    };
    subjects
        .iter()
        .map(|s| one(s))
        .max_by_key(|r| rank(*r))
        .unwrap_or(Readiness::NotConnected)
}

/// The airings from `now_s` to `now_s + horizon_s` (the one on air included), in start order,
/// each with who is down for it and how far they have got.
pub fn rota(
    now_s: i64,
    horizon_s: i64,
    airings: &[Airing],
    shows: &[ShowFact],
    down: &[Down],
    subjects: &[Subject],
    steady_ms: u64,
) -> Vec<RotaRow> {
    let mut coming: Vec<&Airing> = airings
        .iter()
        .filter(|a| a.end > now_s && a.start < now_s + horizon_s)
        .collect();
    coming.sort_by_key(|a| (a.start, a.end));
    coming
        .into_iter()
        .filter_map(|a| {
            let show = shows.iter().find(|s| s.id == a.show)?;
            let d = down.iter().find(|d| d.show == a.show);
            let theirs: Vec<&Subject> = subjects.iter().filter(|s| s.show == a.show).collect();
            let state = show_readiness(&theirs, steady_ms);
            Some(RotaRow {
                show: show.slug.clone(),
                show_name: show.name.clone(),
                start_ms: a.start * 1000,
                end_ms: a.end * 1000,
                hosts: d.map(|d| d.names.clone()).unwrap_or_default(),
                from: d.map_or(Origin::Room, |d| d.from),
                state,
                ready: matches!(state, Readiness::Ready | Readiness::OnAir),
                now: a.start <= now_s && now_s < a.end,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Airing;

    const A: Uuid = Uuid::from_u128(0xa);
    const B: Uuid = Uuid::from_u128(0xb);

    const AIRINGS: [Airing; 2] = [
        Airing {
            show: A,
            start: 1000,
            end: 2000,
        },
        Airing {
            show: B,
            start: 2000,
            end: 3000,
        },
    ];

    fn facts(now_s: i64, make: impl FnOnce(&mut Facts<'static>)) -> Facts<'static> {
        let mut f = Facts {
            now_s,
            window_s: 300,
            airings: &AIRINGS,
            incoming: Readiness::Ready,
            outgoing_on_air: false,
            held: false,
        };
        make(&mut f);
        f
    }

    #[test]
    fn off_when_the_window_is_zero() {
        let w = window(&facts(750, |f| {
            f.window_s = 0;
            f.incoming = Readiness::NotConnected;
        }));
        assert_eq!(w.state, State::Quiet);
        assert_eq!(w.show, None);
    }

    #[test]
    fn empty_schedule_is_quiet() {
        let f = Facts {
            now_s: 750,
            window_s: 300,
            airings: &[],
            incoming: Readiness::NotConnected,
            outgoing_on_air: false,
            held: false,
        };
        let w = window(&f);
        assert_eq!(w.state, State::Quiet);
        assert_eq!(w.show, None);
    }

    #[test]
    fn before_the_target_is_quiet_but_carries_it() {
        // now 600: the target is A (B is not for another 1400s), window opens at 700.
        let w = window(&facts(600, |f| f.incoming = Readiness::NotConnected));
        assert_eq!(w.state, State::Quiet);
        assert_eq!(w.show, Some(A));
        assert_eq!(w.start_s, Some(1000));
        assert_eq!(w.opens_s, Some(700));
    }

    #[test]
    fn the_window_opens_into_announced_cueing_and_ready() {
        assert_eq!(
            window(&facts(750, |f| f.incoming = Readiness::NotConnected)).state,
            State::Announced
        );
        assert_eq!(
            window(&facts(750, |f| f.incoming = Readiness::Connecting)).state,
            State::Cueing
        );
        assert_eq!(
            window(&facts(750, |f| f.incoming = Readiness::Ready)).state,
            State::Ready
        );
    }

    #[test]
    fn a_hold_holds_the_window() {
        let w = window(&facts(750, |f| f.held = true));
        assert_eq!(w.state, State::Held);
        assert_eq!(w.show, Some(A));
    }

    #[test]
    fn an_airing_under_way_is_overrun_quiet_or_held() {
        // now 1500: A is under way and its post has come.
        let overrun = window(&facts(1500, |f| {
            f.incoming = Readiness::Ready;
            f.outgoing_on_air = true;
        }));
        assert_eq!(overrun.state, State::Overrun);
        assert_eq!(overrun.show, Some(A));

        let held = window(&facts(1500, |f| {
            f.incoming = Readiness::Ready;
            f.outgoing_on_air = true;
            f.held = true;
        }));
        assert_eq!(held.state, State::Held);

        let quiet = window(&facts(1500, |f| f.outgoing_on_air = false));
        assert_eq!(quiet.state, State::Quiet);
        assert_eq!(quiet.show, Some(A));
    }

    #[test]
    fn an_airing_under_way_with_the_incoming_on_air_is_the_next() {
        // now 1500 and the incoming host is already on air: the target is B, not A.
        let w = window(&facts(1500, |f| f.incoming = Readiness::OnAir));
        assert_eq!(w.state, State::Quiet);
        assert_eq!(w.show, Some(B));
        assert_eq!(w.opens_s, Some(1700));
    }

    // ---- at the post's side, with the post's own rule ----

    use crate::post::{AtDesk, Facts as PostRuleFacts, Holder, ShowFact, Subject, plan};
    use crate::sources::SourceSettings;

    /// Steeped (A) airs [1000 s, 2000 s) with Heather on air in the room; Kettle Drive (B)
    /// follows with Fiona sending from her own studio.
    fn shows_ab() -> Vec<ShowFact> {
        vec![
            ShowFact {
                id: A,
                slug: "steeped".into(),
                name: "Steeped".into(),
            },
            ShowFact {
                id: B,
                slug: "kettle".into(),
                name: "Kettle Drive".into(),
            },
        ]
    }

    fn down_ab() -> Vec<Down> {
        vec![
            Down {
                show: A,
                names: vec!["Heather".into()],
                from: Origin::Room,
            },
            Down {
                show: B,
                names: vec!["Fiona".into()],
                from: Origin::Push,
            },
        ]
    }

    fn heather_on_air() -> Subject {
        Subject {
            id: "presenter-heather".into(),
            identity: "presenter-heather".into(),
            name: "Heather".into(),
            show: A,
            in_room: true,
            since_ms: None,
            at_desk: Some(AtDesk {
                on_air: true,
                here: true,
                connected_ms: None,
            }),
            log: Vec::new(),
        }
    }

    /// Fiona's key: `None` not connected, else connected that many seconds.
    fn fiona(connected_s: Option<u64>, now_ms: i64) -> Subject {
        Subject {
            id: "fionakey".into(),
            identity: "source-fionakey".into(),
            name: "Fiona".into(),
            show: B,
            in_room: false,
            since_ms: connected_s.map(|c| now_ms - c as i64 * 1000),
            at_desk: connected_s.map(|c| AtDesk {
                on_air: false,
                here: true,
                connected_ms: Some(c * 1000),
            }),
            log: Vec::new(),
        }
    }

    fn heather() -> Holder {
        Holder {
            identity: "presenter-heather".into(),
            name: "Heather".into(),
        }
    }

    /// The window at `now_ms`, from the post's own rule's answer.
    fn at_post(
        now_ms: i64,
        subjects: &[Subject],
        holders: &[Holder],
        hold: Option<&Hold>,
        told: &[(i64, i64)],
    ) -> AtPost {
        let settings = SourceSettings::default();
        let shows = shows_ab();
        let plan = plan(&PostRuleFacts {
            now_ms,
            settings: &settings,
            automation: true,
            auto_mode: false,
            airings: &AIRINGS,
            shows: &shows,
            subjects,
            holders,
            hold: hold.cloned(),
        });
        for_post(&PostFacts {
            now_ms,
            window_s: i64::from(settings.window_s),
            airings: &AIRINGS,
            shows: &shows,
            subjects,
            holders,
            hold,
            post: plan.post.as_ref(),
            down: &down_ab(),
            told,
            steady_ms: 5000,
        })
    }

    #[test]
    fn back_to_back_the_window_is_the_next_shows_while_the_one_on_air_runs() {
        // 1800 s: Heather is on air in Steeped; Kettle Drive's window opened at 1700 s.
        let now = 1_800_000;
        let not_here = at_post(
            now,
            &[heather_on_air(), fiona(None, now)],
            &[heather()],
            None,
            &[],
        );
        let v = not_here.view.unwrap();
        assert_eq!(
            (v.state, v.show.as_str(), v.host.as_str(), v.from),
            (State::Announced, "kettle", "Fiona", Origin::Push)
        );
        assert_eq!((v.start_ms, v.opens_ms), (2_000_000, 1_700_000));

        let connecting = at_post(
            now,
            &[heather_on_air(), fiona(Some(2), now)],
            &[heather()],
            None,
            &[],
        );
        assert_eq!(connecting.view.unwrap().state, State::Cueing);

        let ready = at_post(
            now,
            &[heather_on_air(), fiona(Some(60), now)],
            &[heather()],
            None,
            &[],
        );
        assert_eq!(ready.view.unwrap().state, State::Ready);
    }

    #[test]
    fn before_the_window_it_is_quiet_and_nobody_is_told() {
        let now = 1_500_000;
        let p = at_post(
            now,
            &[heather_on_air(), fiona(None, now)],
            &[heather()],
            None,
            &[],
        );
        assert_eq!(p.view.unwrap().state, State::Quiet);
        assert!(p.heads_up.is_none());
    }

    #[test]
    fn past_the_post_with_the_outgoing_still_on_air_is_overrun_and_no_act() {
        // 2030 s: Kettle Drive's post has come, Fiona stands by, Heather still presents.
        let now = 2_030_000;
        let subjects = [heather_on_air(), fiona(Some(600), now)];
        let p = at_post(now, &subjects, &[heather()], None, &[]);
        let v = p.view.unwrap();
        assert_eq!((v.state, v.show.as_str()), (State::Overrun, "kettle"));
        assert!(p.heads_up.is_none(), "the heads-up is for before the post");
    }

    #[test]
    fn the_heads_up_is_due_once_an_airing() {
        let now = 1_750_000;
        let subjects = [heather_on_air(), fiona(Some(60), now)];
        let p = at_post(now, &subjects, &[heather()], None, &[]);
        let h = p.heads_up.unwrap();
        assert_eq!(h.airing_start_s, 2000);
        assert_eq!(h.subject.as_deref(), Some("fionakey"));
        assert_eq!(h.identity.as_deref(), Some("source-fionakey"));
        assert_eq!(h.title, "Next: Fiona, Kettle Drive");
        assert_eq!(h.message, "Kettle Drive starts in 5 min; Fiona is ready.");
        // The line the control plane writes names the airing; read back, it is not due again.
        let detail = heads_up_detail(&h);
        assert_eq!(heads_up_airing(&detail), Some(2000));
        let again = at_post(now + 1000, &subjects, &[heather()], None, &[(2000, now)]);
        assert!(again.heads_up.is_none());
        assert_eq!(again.view.unwrap().heads_up_ms, Some(now));
        // A heads-up for another airing does not count for this one.
        let other = at_post(now + 1000, &subjects, &[heather()], None, &[(1000, 1)]);
        assert!(other.heads_up.is_some());
    }

    #[test]
    fn a_hold_on_this_airing_holds_the_window_and_the_host_is_still_told() {
        let now = 1_800_000;
        let subjects = [heather_on_air(), fiona(None, now)];
        let hold = Hold {
            airing_start_s: 2000,
            by: "Shona".into(),
        };
        let p = at_post(now, &subjects, &[heather()], Some(&hold), &[]);
        assert_eq!(p.view.unwrap().state, State::Held);
        assert!(p.heads_up.is_some());
        // A hold on another airing is no hold on this one.
        let elsewhere = Hold {
            airing_start_s: 1000,
            ..hold
        };
        let p = at_post(now, &subjects, &[heather()], Some(&elsewhere), &[]);
        assert_eq!(p.view.unwrap().state, State::Announced);
    }

    #[test]
    fn off_at_zero_and_silent_when_nobody_is_down() {
        let now = 1_800_000;
        let with_key = [heather_on_air(), fiona(None, now)];
        let without = [heather_on_air()];
        let shows = shows_ab();
        let base = |window_s, down: &[Down], subjects: &[Subject]| {
            for_post(&PostFacts {
                now_ms: now,
                window_s,
                airings: &AIRINGS,
                shows: &shows,
                subjects,
                holders: &[heather()],
                hold: None,
                post: None,
                down,
                told: &[],
                steady_ms: 5000,
            })
        };
        assert_eq!(base(0, &down_ab(), &with_key).view, None);
        // Without the post's view the host is read from their own key: not yet here.
        let v = base(300, &down_ab(), &with_key).view.unwrap();
        assert_eq!((v.state, v.host.as_str()), (State::Announced, "Fiona"));
        // A room show whose presenter is not at the desk is announced by the name down.
        let room = [Down {
            show: B,
            names: vec!["Fiona".into()],
            from: Origin::Room,
        }];
        let v = base(300, &room, &without).view.unwrap();
        assert_eq!(
            (v.state, v.host.as_str(), v.from),
            (State::Announced, "Fiona", Origin::Room)
        );
        // Nobody down for Kettle Drive and no key of its own: nothing to say.
        let nobody = [Down {
            show: B,
            names: Vec::new(),
            from: Origin::Room,
        }];
        let p = base(300, &nobody, &without);
        assert_eq!((p.view, p.heads_up), (None, None));
    }

    /// Kettle Drive (B) under way from 2000 s with Fiona's key; Steeped (A) again from 3000 s
    /// with Heather in the room.
    const LATER: [Airing; 2] = [
        Airing {
            show: B,
            start: 2000,
            end: 3000,
        },
        Airing {
            show: A,
            start: 3000,
            end: 4000,
        },
    ];

    fn later(now_ms: i64, subjects: &[Subject], holders: &[Holder]) -> AtPost {
        let shows = shows_ab();
        let settings = SourceSettings::default();
        let plan = plan(&PostRuleFacts {
            now_ms,
            settings: &settings,
            automation: true,
            auto_mode: false,
            airings: &LATER,
            shows: &shows,
            subjects,
            holders,
            hold: None,
        });
        for_post(&PostFacts {
            now_ms,
            window_s: 300,
            airings: &LATER,
            shows: &shows,
            subjects,
            holders,
            hold: None,
            post: plan.post.as_ref(),
            down: &down_ab(),
            told: &[],
            steady_ms: 5000,
        })
    }

    fn heather_off_air() -> Subject {
        let mut h = heather_on_air();
        h.at_desk.as_mut().unwrap().on_air = false;
        h
    }

    #[test]
    fn a_host_who_never_came_does_not_hide_the_next_show() {
        // 2800 s: Fiona never connected for Kettle Drive, so its post is still pending; Steeped
        // is next at 3000 s with Heather at the desk. The window is Steeped's, and she is told.
        let now = 2_800_000;
        let p = later(now, &[fiona(None, now), heather_off_air()], &[]);
        let v = p.view.expect("Steeped's window");
        assert_eq!(
            (v.show.as_str(), v.state, v.host.as_str(), v.from),
            ("steeped", State::Ready, "Heather", Origin::Room)
        );
        let h = p.heads_up.expect("Heather's heads-up");
        assert_eq!(
            (h.airing_start_s, h.identity.as_deref()),
            (3000, Some("presenter-heather"))
        );
        // Before Steeped's window opens, the pending post stays Kettle Drive's.
        let p = later(2_500_000, &[fiona(None, 2_500_000), heather_off_air()], &[]);
        assert_eq!(p.view.unwrap().show, "kettle");
    }

    #[test]
    fn a_host_who_went_on_air_and_came_off_is_not_overrun() {
        // 2500 s: Fiona went on air at the post and came off at 2400 s; a DJ in the room presents.
        let now = 2_500_000;
        let mut f = fiona(Some(600), now);
        f.log = vec![
            crate::post::Line {
                at_ms: 2_400_000,
                event: "off_air".into(),
                reason: None,
                detail: String::new(),
            },
            crate::post::Line {
                at_ms: 2_000_000,
                event: "on_air".into(),
                reason: Some("schedule".into()),
                detail: String::new(),
            },
        ];
        let dj = Holder {
            identity: "dj-eilidh".into(),
            name: "Eilidh".into(),
        };
        let p = later(now, &[f, heather_off_air()], std::slice::from_ref(&dj));
        let v = p.view.expect("the next show's");
        assert_eq!((v.show.as_str(), v.state), ("steeped", State::Quiet));
        assert!(p.heads_up.is_none());
    }

    #[test]
    fn readiness_follows_the_posts_source_state() {
        assert_eq!(readiness(None), Readiness::NotConnected);
        assert_eq!(
            readiness(Some(SourceState::LinkLost)),
            Readiness::NotConnected
        );
        assert_eq!(
            readiness(Some(SourceState::Connecting)),
            Readiness::Connecting
        );
        assert_eq!(readiness(Some(SourceState::StandingBy)), Readiness::Ready);
        assert_eq!(readiness(Some(SourceState::OnAir)), Readiness::OnAir);
    }

    // ---- the rota ----

    #[test]
    fn the_rota_lists_the_coming_hours_with_who_and_how_far() {
        let now = 1_500_000;
        let shows = shows_ab();
        let later = [
            AIRINGS[0],
            AIRINGS[1],
            Airing {
                show: A,
                start: 50_000,
                end: 51_000,
            },
        ];
        let subjects = [heather_on_air(), fiona(Some(2), now)];
        let rows = rota(1500, 12 * 3600, &later, &shows, &down_ab(), &subjects, 5000);
        assert_eq!(
            rows.len(),
            2,
            "the airing past the twelve hours is left out"
        );
        assert_eq!(
            (
                rows[0].show.as_str(),
                rows[0].state,
                rows[0].ready,
                rows[0].now
            ),
            ("steeped", Readiness::OnAir, true, true)
        );
        assert_eq!(rows[0].hosts, ["Heather"]);
        assert_eq!(
            (
                rows[1].show.as_str(),
                rows[1].from,
                rows[1].state,
                rows[1].ready
            ),
            ("kettle", Origin::Push, Readiness::Connecting, false)
        );
        // An airing that has ended is not on it; one with nobody down says so.
        let rows = rota(2500, 3600, &later, &shows, &[], &[], 5000);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].hosts.is_empty());
        assert_eq!(rows[0].state, Readiness::NotConnected);
    }
}
