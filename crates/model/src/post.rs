// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The post's rule (ADR-0280; design sections 3.3 to 3.6 and 10): who goes on air at a show's
//! post, who keeps the air while the next host is not ready, and what a dropped link does.
//!
//! This is the decision only, a pure function of facts: the schedule's airings, who is at the
//! desk and on air, what the log says each source last did, the rules and the clock
//! ([`plan`]). The control plane gathers the facts every second, carries out the acts
//! it returns and logs them, so nothing here is remembered: after a restart the same facts give
//! the same acts. It never presses anything that a person has already done: a source is brought
//! on by the post, the late count or the grace **at most once an airing** (the log's `on_air`,
//! `back` and `not_yet` lines since the post), and only a return after a lost link repeats.
//!
//! **A subject** is whatever the schedule can bring on for a show: a source admitted by one of
//! the show's keys, or the show's presenter in the room signed in as themselves (exactly the
//! presenter's account, never a link that merely looks like one). A room presenter takes part in
//! the grace, a dropped link's hand-over and End show; it is never brought on from automation,
//! where it would be a person put on air by the clock.
//!
//! **The post's show.** The show the rule is about is the one on air by the schedule when it has
//! a subject and none of its subjects is on air yet (its post is under way); otherwise the next
//! show to start. Its grace and its hold belong to that airing.
//!
//! **Who holds the air.** Automation holds it when the station's mode is `auto`, or nobody who
//! presents (a presenter, a DJ or a source) is on air; on a station without the playout layer
//! only the second.

use std::collections::BTreeMap;

use serde::Serialize;
use uuid::Uuid;

use crate::sources::{Airing, SourceSettings};

/// Three drops in this long make a source's steady time [`FLAPPING_STEADY_MS`].
pub const FLAPPING_WINDOW_MS: i64 = 10 * 60 * 1000;
/// How many drops in the window count as flapping.
pub const FLAPPING_DROPS: usize = 3;
/// The steady time of a source that keeps dropping.
pub const FLAPPING_STEADY_MS: u64 = 30_000;
/// The key id the post's own lines (a hold) are kept under in `source_log`.
pub const POST_KEY: &str = "post";
/// The `reason` of a line the schedule wrote itself.
pub const BY_SCHEDULE: &str = "schedule";
/// The `reason` of a press the schedule only saw at the desk.
pub const BY_DESK: &str = "desk";

/// The log lines that say where a source's place stands.
pub const STATE_EVENTS: [&str; 7] = [
    "on_air",
    "back",
    "off_air",
    "link_lost",
    "place_let_go",
    "disconnected",
    "let_back",
];
/// Every event the rule reads (the state ones, and the rest).
pub const POST_EVENTS: [&str; 11] = [
    "on_air",
    "back",
    "off_air",
    "link_lost",
    "place_let_go",
    "disconnected",
    "let_back",
    "covered",
    "go",
    "not_yet",
    "hold",
];

/// One line of the log, as the rule reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub at_ms: i64,
    pub event: String,
    pub reason: Option<String>,
    pub detail: String,
}

/// A show's name and slug, for the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowFact {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
}

/// A source (or a room presenter) as the desk holds it now.
#[derive(Clone, Debug, PartialEq)]
pub struct AtDesk {
    pub on_air: bool,
    /// Connected and linked (a source at the desk always is; a person in the room may have dropped).
    pub here: bool,
    /// How long the desk has had it unbroken; `None` for a person in the room.
    pub connected_ms: Option<u64>,
}

/// Whatever the schedule can bring on for a show.
#[derive(Clone, Debug, PartialEq)]
pub struct Subject {
    /// The key's id, or for a room presenter their desk identity.
    pub id: String,
    /// Its desk identity.
    pub identity: String,
    /// The name on the desk.
    pub name: String,
    pub show: Uuid,
    pub in_room: bool,
    /// When the ingest took the connection, on the station's clock; for the steady time's end.
    pub since_ms: Option<i64>,
    pub at_desk: Option<AtDesk>,
    /// The log's lines for it, newest first, within the rule's window.
    pub log: Vec<Line>,
}

/// Someone presenting who is on air now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub identity: String,
    pub name: String,
}

/// A hold on one handover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    /// The airing's start (epoch seconds) it holds.
    pub airing_start_s: i64,
    pub by: String,
}

pub struct Facts<'a> {
    pub now_ms: i64,
    pub settings: &'a SourceSettings,
    /// The station has the playout layer: automation can hold the air and the mode means something.
    pub automation: bool,
    /// The station's mode is `auto`.
    pub auto_mode: bool,
    /// The airings in air order.
    pub airings: &'a [Airing],
    pub shows: &'a [ShowFact],
    pub subjects: &'a [Subject],
    /// Who presents and is on air, subjects included.
    pub holders: &'a [Holder],
    pub hold: Option<Hold>,
}

/// Why a source is brought on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// Its post came with automation holding the air.
    AtPost,
    /// It arrived late, and the count ran out.
    LateCount,
    /// The grace ran out.
    Grace,
    /// It came back after a lost link, within its place's time.
    Back,
    /// The host on air dropped during the grace, and the next was standing by.
    DropHandover,
    /// End show, with the next host standing by.
    EndShow,
    /// The host's own Go on air now.
    Go,
    /// The host on air brought the next on, beside them, by a press (the handover's first press).
    Handover,
}

impl Cause {
    /// Whether the desk fades over the rules' `fade_ms` for this move (design section 10: *a
    /// fade on a source's On when the schedule or End show moves it; a person's press stays
    /// instant*). The schedule's moves and End show passing the air fade; the host's own Go on
    /// air now and the handover's first press are presses.
    pub fn fades(self) -> bool {
        !matches!(self, Cause::Go | Cause::Handover)
    }

    /// The words the log's `detail` carries.
    pub fn words(self) -> &'static str {
        match self {
            Cause::AtPost => "at the post",
            Cause::LateCount => "the late count ran out",
            Cause::Grace => "the grace ended",
            Cause::Back => "back after a lost link",
            Cause::DropHandover => "the host on air lost their link during the grace",
            Cause::EndShow => "End show passed the air on",
            Cause::Go => "Go on air now",
            Cause::Handover => "the host on air brought them on",
        }
    }
}

/// What the rule asks the control plane to do (and log).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Put `identity` on air and take `takeoff` off, as the handover's two steps; `set_live`
    /// first takes the air back from automation.
    BringOn {
        subject: String,
        identity: String,
        name: String,
        show: Uuid,
        cause: Cause,
        takeoff: Vec<String>,
        set_live: bool,
    },
    /// The desk shows a source on or off air that the log does not: write what the desk says.
    Observe {
        subject: String,
        show: Uuid,
        on: bool,
    },
    /// The source on air is gone: its place is kept from `at_ms`.
    MarkLost {
        subject: String,
        show: Uuid,
        at_ms: i64,
    },
    /// Nobody else is on air and the lost link's silence is over: automation takes the air.
    Cover { subject: String, show: Uuid },
    /// The place is let go (the time passed, or the next host took the air).
    LetGo { subject: String, show: Uuid },
}

/// The state of the post's source, in a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// A key of the show has not connected.
    NotConnected,
    /// Connected, not yet steady.
    Connecting,
    /// Steady and off air: ready.
    StandingBy,
    OnAir,
    /// It was on air and its link is gone.
    LinkLost,
}

/// A lost link and the place kept for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct LinkLostView {
    pub key_id: String,
    pub name: String,
    pub show: Option<String>,
    /// When the link was lost.
    pub lost_ms: i64,
    /// When automation covers it, if nobody else is on air (the rules' silence).
    pub covers_ms: i64,
    /// Until when the place is kept for it.
    pub place_kept_until_ms: i64,
    /// Whether automation has covered it already.
    pub covered: bool,
}

/// The next handover: the show whose post it is, its source and the clocks of the rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PostShow {
    pub slug: String,
    pub name: String,
    /// When its post is (its airing's start).
    pub start_ms: i64,
    /// Its post has come.
    pub started: bool,
}

/// What `/studio` and `GET /sources` say of the post (design section 10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PostView {
    /// The show whose post it is: the next to start, or the one on air whose host has not
    /// taken the air.
    pub show: Option<PostShow>,
    /// Its source: the key's id (or, for a presenter in the room, their identity).
    pub source: Option<String>,
    /// Its source's name.
    pub source_name: Option<String>,
    /// `not_connected`, `connecting`, `standing_by`, `on_air` or `link_lost`; `None` when the
    /// show has no host to bring on (it comes from the room).
    pub state: Option<SourceState>,
    /// When the source became steady (standing by).
    pub standing_since_ms: Option<i64>,
    /// When the schedule puts it on air, from automation: at the post, or the late count's end.
    pub on_air_at_ms: Option<i64>,
    /// When the grace ends and the schedule hands over from whoever holds the air.
    pub grace_ends_ms: Option<i64>,
    /// Who holds this handover (a producer's Hold); the grace does not run.
    pub held_by: Option<String>,
    /// When the late count ends and the source goes on air.
    pub late_ends_ms: Option<i64>,
    /// Who presents now, on air at the desk.
    pub holders: Vec<String>,
    /// A lost link and the place kept for it, whichever show it belonged to.
    pub link_lost: Option<LinkLostView>,
}

/// The rule's answer: what to do, what to show, and who End show would pass the air to.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub acts: Vec<Act>,
    pub post: Option<PostView>,
    /// The post's source if it is ready (standing by, steady): End show passes the air to it.
    pub ready: Option<Ready>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub subject: String,
    pub identity: String,
    pub name: String,
    pub show: Uuid,
}

// ---- reading a subject's log ----

impl Subject {
    /// The latest line that says where the place stands.
    fn state_line(&self) -> Option<&Line> {
        self.log
            .iter()
            .find(|l| STATE_EVENTS.contains(&l.event.as_str()))
    }

    /// Whether the log says it is, or was last, on air.
    fn logged_on(&self) -> bool {
        self.state_line()
            .is_some_and(|l| l.event == "on_air" || l.event == "back")
    }

    fn drops(&self, now_ms: i64) -> usize {
        self.log
            .iter()
            .filter(|l| l.event == "link_lost" && now_ms - l.at_ms < FLAPPING_WINDOW_MS)
            .count()
    }

    /// How long it must be steady to stand by.
    fn needed_ms(&self, settings: &SourceSettings, now_ms: i64) -> u64 {
        if self.drops(now_ms) >= FLAPPING_DROPS {
            FLAPPING_STEADY_MS.max(settings.steady_s as u64 * 1000)
        } else {
            settings.steady_s as u64 * 1000
        }
    }

    /// Whether it went on air or off air, or was told to wait, since `since_ms`: the post's
    /// airing has been dealt with, by the schedule or by a person, and the schedule does not
    /// bring it on again. (A source taken off air by a press, or by the handover, stays off.)
    fn acted_since(&self, since_ms: i64) -> bool {
        self.log.iter().any(|l| {
            l.at_ms >= since_ms
                && matches!(l.event.as_str(), "on_air" | "back" | "off_air" | "not_yet")
        })
    }

    /// Ready: steady and off air.
    fn ready(&self, settings: &SourceSettings, now_ms: i64) -> bool {
        let Some(d) = &self.at_desk else {
            return false;
        };
        d.here
            && !d.on_air
            && match d.connected_ms {
                Some(ms) => ms >= self.needed_ms(settings, now_ms),
                None => true,
            }
    }

    /// When it became ready, on the station's clock: the connection's start (the ingest's)
    /// and the steady time. A room presenter is ready from the post.
    fn ready_since_ms(&self, settings: &SourceSettings, now_ms: i64, post_ms: i64) -> i64 {
        let d = self.at_desk.as_ref();
        match (d.and_then(|d| d.connected_ms), self.since_ms) {
            (Some(ms), since) => {
                let start = since.unwrap_or_else(|| (now_ms - ms as i64) / 1000 * 1000);
                start + self.needed_ms(settings, now_ms) as i64
            }
            (None, _) => post_ms,
        }
    }
}

/// The airing on air by the schedule at `now_s`: the latest-starting one that holds it.
pub fn airing_now(airings: &[Airing], now_s: i64) -> Option<&Airing> {
    airings
        .iter()
        .filter(|a| a.start <= now_s && now_s < a.end)
        .max_by_key(|a| a.start)
}

/// The next airing to start after `now_s`.
pub fn airing_next(airings: &[Airing], now_s: i64) -> Option<&Airing> {
    airings
        .iter()
        .filter(|a| a.start > now_s)
        .min_by_key(|a| a.start)
}

/// The airing the rule is about (see the module's note on the post's show).
pub fn post_airing<'a>(
    airings: &'a [Airing],
    subjects: &[Subject],
    now_s: i64,
) -> Option<&'a Airing> {
    let has = |show: Uuid| subjects.iter().any(|s| s.show == show);
    let on_air = |show: Uuid| {
        subjects
            .iter()
            .any(|s| s.show == show && s.at_desk.as_ref().is_some_and(|d| d.on_air))
    };
    match airing_now(airings, now_s) {
        Some(cur) if has(cur.show) && !on_air(cur.show) => Some(cur),
        _ => airing_next(airings, now_s),
    }
}

// ---- the rule ----

/// What to do now, and what to say, from the facts.
pub fn plan(f: &Facts<'_>) -> Plan {
    let now = f.now_ms;
    let now_s = now.div_euclid(1000);
    let s = f.settings;
    let name_of = |id: Uuid| f.shows.iter().find(|x| x.id == id);
    let mut acts: Vec<Act> = Vec::new();

    let auto_holds = if f.automation {
        f.auto_mode || f.holders.is_empty()
    } else {
        f.holders.is_empty()
    };
    let post = post_airing(f.airings, f.subjects, now_s);

    // What the log says against what the desk shows, for every subject.
    let mut lost: Vec<(&Subject, i64)> = Vec::new();
    for sub in f.subjects {
        let logged_on = sub.logged_on();
        match (&sub.at_desk, logged_on) {
            (Some(d), false) if d.on_air => acts.push(Act::Observe {
                subject: sub.id.clone(),
                show: sub.show,
                on: true,
            }),
            (Some(d), true) if !d.on_air => {
                // On air by the log, off air at the desk: a press, or a connection that came back
                // (the desk puts a new connection off air).
                let on_since = sub.state_line().map_or(now, |l| l.at_ms);
                let came_back = d.connected_ms.is_some_and(|ms| {
                    i64::try_from(ms).unwrap_or(i64::MAX) + 2_000 < now - on_since
                });
                if came_back {
                    let at = now - d.connected_ms.unwrap_or(0) as i64;
                    acts.push(Act::MarkLost {
                        subject: sub.id.clone(),
                        show: sub.show,
                        at_ms: at,
                    });
                    lost.push((sub, at));
                } else {
                    acts.push(Act::Observe {
                        subject: sub.id.clone(),
                        show: sub.show,
                        on: false,
                    });
                }
            }
            (None, true) if !sub.in_room => {
                acts.push(Act::MarkLost {
                    subject: sub.id.clone(),
                    show: sub.show,
                    at_ms: now,
                });
                lost.push((sub, now));
            }
            _ => {}
        }
        if let Some(l) = sub.state_line()
            && l.event == "link_lost"
        {
            lost.push((sub, l.at_ms));
        }
    }

    // The post's subject, and who could go on air for it.
    let post_subjects: Vec<&Subject> = post
        .map(|p| f.subjects.iter().filter(|x| x.show == p.show).collect())
        .unwrap_or_default();
    let mut ready: Vec<(&Subject, i64)> = post_subjects
        .iter()
        .copied()
        .filter(|x| x.ready(s, now))
        .filter(|x| post.is_none_or(|p| !x.acted_since(p.start * 1000)))
        // What the desk shows is written down first: a change seen now is not a reason to act yet.
        .filter(|x| {
            !acts.iter().any(|a| {
                matches!(a, Act::Observe { subject, .. } | Act::MarkLost { subject, .. } if *subject == x.id)
            })
        })
        .map(|x| {
            (
                x,
                x.ready_since_ms(s, now, post.map_or(now, |p| p.start * 1000)),
            )
        })
        .collect();
    ready.sort_by(|a, b| (a.1, &a.0.id).cmp(&(b.1, &b.0.id)));
    let best = ready.first().copied();

    let mut view = PostView {
        show: post.and_then(|p| {
            name_of(p.show).map(|sh| PostShow {
                slug: sh.slug.clone(),
                name: sh.name.clone(),
                start_ms: p.start * 1000,
                started: now_s >= p.start,
            })
        }),
        source: None,
        source_name: None,
        state: None,
        standing_since_ms: None,
        on_air_at_ms: None,
        grace_ends_ms: None,
        held_by: None,
        late_ends_ms: None,
        holders: f.holders.iter().map(|h| h.name.clone()).collect(),
        link_lost: None,
    };

    if let (Some(p), Some(h)) = (post, &f.hold)
        && h.airing_start_s == p.start
    {
        view.held_by = Some(h.by.clone());
    }

    // The post's source, by how far it has got.
    let rank = |x: &Subject| -> (u8, SourceState) {
        let lost_now = x.state_line().is_some_and(|l| l.event == "link_lost");
        match &x.at_desk {
            Some(d) if d.on_air => (0, SourceState::OnAir),
            Some(_) if x.ready(s, now) => (1, SourceState::StandingBy),
            Some(_) => (2, SourceState::Connecting),
            None if lost_now => (3, SourceState::LinkLost),
            None => (4, SourceState::NotConnected),
        }
    };
    let shown = best.map(|(x, _)| x).or_else(|| {
        post_subjects
            .iter()
            .copied()
            .min_by_key(|x| (rank(x).0, x.id.clone()))
    });
    if let Some(x) = shown {
        view.source = Some(x.id.clone());
        view.source_name = Some(x.name.clone());
        view.state = Some(rank(x).1);
        if best.is_some() {
            view.standing_since_ms = best.map(|b| b.1);
        }
    }

    // A source just lost is still the air's, for the silence; and once the silence is over, a next
    // host standing by is brought on by the loss itself (below), not by the post's own clock.
    let silence_ms = i64::from(s.silence_s) * 1000;
    let kept_ms = i64::from(s.place_kept_s) * 1000;
    let recent_loss = lost.iter().any(|(_, at)| now - at < silence_ms);
    let drop_handover = lost.iter().any(|(l, at)| {
        let age = now - at;
        age >= silence_ms
            && age < kept_ms
            && f.holders.is_empty()
            && !l.at_desk.as_ref().is_some_and(|d| d.here)
            && post.is_some_and(|p| now_s >= p.start)
            && best.is_some_and(|(c, _)| c.id != l.id)
    });

    // The post, the late count and the grace.
    if let (Some(p), Some((c, since))) = (post, best) {
        let post_ms = p.start * 1000;
        let started = now_s >= p.start;
        let acted = c.acted_since(post_ms);
        let held = f
            .hold
            .as_ref()
            .filter(|h| h.airing_start_s == p.start)
            .cloned();
        let others: Vec<String> = f
            .holders
            .iter()
            .filter(|h| h.identity != c.identity)
            .map(|h| h.identity.clone())
            .collect();
        let bring = |cause: Cause, takeoff: Vec<String>| Act::BringOn {
            subject: c.id.clone(),
            identity: c.identity.clone(),
            name: c.name.clone(),
            show: c.show,
            cause,
            takeoff,
            set_live: f.automation && f.auto_mode,
        };
        // A source that comes back after a lost link has its own rule (below).
        let c_lost = lost.iter().any(|(l, _)| l.id == c.id);
        let may_act = started && !acted && !recent_loss && !drop_handover;
        if auto_holds && !c.in_room {
            // Automation holds the air: at the post, to the second, if the source was standing by
            // before it; else the late count from the moment it stood by (0 is off).
            if since <= post_ms {
                view.on_air_at_ms = Some(post_ms);
                if may_act && !c_lost {
                    acts.push(bring(Cause::AtPost, Vec::new()));
                }
            } else if s.late_count_s > 0 {
                let due = since + i64::from(s.late_count_s) * 1000;
                view.late_ends_ms = Some(due);
                view.on_air_at_ms = Some(due);
                if may_act && !c_lost && now >= due {
                    acts.push(bring(Cause::LateCount, Vec::new()));
                }
            }
            if acted {
                view.on_air_at_ms = None;
                view.late_ends_ms = None;
            }
        } else if !auto_holds && s.grace_s > 0 {
            // Someone presents: the grace starts when the next is standing by and the post has
            // come, and at its end the schedule hands over (0 is off: a press only).
            let from = since.max(post_ms);
            let ends = from + i64::from(s.grace_s) * 1000;
            if held.is_none() && !acted && started {
                view.grace_ends_ms = Some(ends);
            }
            if may_act && held.is_none() && now >= ends {
                acts.push(bring(Cause::Grace, others));
            }
        }
    }

    // A lost link and the place kept for it.
    for (sub, at) in &lost {
        let age = now - at;
        let kept = kept_ms;
        let silence = silence_ms;
        let covered = sub
            .log
            .iter()
            .any(|l| l.event == "covered" && l.at_ms >= *at);
        if view.link_lost.is_none() && age < kept.max(silence) {
            view.link_lost = Some(LinkLostView {
                key_id: sub.id.clone(),
                name: sub.name.clone(),
                show: name_of(sub.show).map(|x| x.slug.clone()),
                lost_ms: *at,
                covers_ms: at + silence,
                place_kept_until_ms: at + kept,
                covered,
            });
        }
        if age >= kept {
            acts.push(Act::LetGo {
                subject: sub.id.clone(),
                show: sub.show,
            });
            continue;
        }
        let here = sub.at_desk.as_ref().is_some_and(|d| d.here);
        if here {
            // It reconnected. Within its place's time it goes back on air once steady, if nobody
            // else is on air to be taken off.
            if sub.ready(s, now) && auto_holds {
                acts.push(Act::BringOn {
                    subject: sub.id.clone(),
                    identity: sub.identity.clone(),
                    name: sub.name.clone(),
                    show: sub.show,
                    cause: Cause::Back,
                    takeoff: Vec::new(),
                    set_live: f.automation && f.auto_mode,
                });
            }
            continue;
        }
        if age >= silence && f.holders.is_empty() {
            // A drop during a grace: the next host is standing by and it is their time.
            if drop_handover
                && let Some((c, _)) = best
                && c.id != sub.id
            {
                acts.push(Act::BringOn {
                    subject: c.id.clone(),
                    identity: c.identity.clone(),
                    name: c.name.clone(),
                    show: c.show,
                    cause: Cause::DropHandover,
                    takeoff: Vec::new(),
                    set_live: f.automation && f.auto_mode,
                });
                acts.push(Act::LetGo {
                    subject: sub.id.clone(),
                    show: sub.show,
                });
            } else if f.automation && !f.auto_mode && !covered {
                acts.push(Act::Cover {
                    subject: sub.id.clone(),
                    show: sub.show,
                });
            }
        }
    }

    let ready_out = best.map(|(c, _)| Ready {
        subject: c.id.clone(),
        identity: c.identity.clone(),
        name: c.name.clone(),
        show: c.show,
    });
    let any_post = view.show.is_some() || view.link_lost.is_some();
    Plan {
        acts: dedupe(acts),
        post: any_post.then_some(view),
        ready: ready_out,
    }
}

/// The same act twice in one pass is done once (a lost source's place can be let go by two rules).
fn dedupe(acts: Vec<Act>) -> Vec<Act> {
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    acts.into_iter()
        .filter(|a| seen.insert(format!("{a:?}"), ()).is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn show(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn shows() -> Vec<ShowFact> {
        vec![
            ShowFact {
                id: show(1),
                slug: "steeped".into(),
                name: "Steeped".into(),
            },
            ShowFact {
                id: show(2),
                slug: "kettle".into(),
                name: "Kettle Drive".into(),
            },
        ]
    }

    /// Steeped airs for an hour up to the post; Kettle Drive airs an hour from it.
    fn airings(post_s: i64) -> Vec<Airing> {
        vec![
            Airing {
                show: show(1),
                start: post_s - 3600,
                end: post_s,
            },
            Airing {
                show: show(2),
                start: post_s,
                end: post_s + 3600,
            },
        ]
    }

    fn line(at_ms: i64, event: &str) -> Line {
        Line {
            at_ms,
            event: event.into(),
            reason: None,
            detail: String::new(),
        }
    }

    fn source(id: &str, show_n: u128, connected_s: u64, since_ms: i64) -> Subject {
        Subject {
            id: id.into(),
            identity: format!("source-{id}"),
            name: id.to_uppercase(),
            show: show(show_n),
            in_room: false,
            since_ms: Some(since_ms),
            at_desk: Some(AtDesk {
                on_air: false,
                here: true,
                connected_ms: Some(connected_s * 1000),
            }),
            log: Vec::new(),
        }
    }

    fn dj() -> Holder {
        Holder {
            identity: "dj-1".into(),
            name: "Heather".into(),
        }
    }

    fn run(
        now: i64,
        post_s: i64,
        subjects: &[Subject],
        holders: &[Holder],
        settings: &SourceSettings,
        auto: bool,
        hold: Option<Hold>,
    ) -> Plan {
        let a = airings(post_s);
        let sh = shows();
        plan(&Facts {
            now_ms: now,
            settings,
            automation: true,
            auto_mode: auto,
            airings: &a,
            shows: &sh,
            subjects,
            holders,
            hold,
        })
    }

    fn brings(p: &Plan) -> Vec<(&str, Cause)> {
        p.acts
            .iter()
            .filter_map(|a| match a {
                Act::BringOn { subject, cause, .. } => Some((subject.as_str(), *cause)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn automation_holds_the_air_and_the_next_host_is_standing_by_at_the_post() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        // Steady for two minutes before the post: on air at the post, to the second, and the
        // mode goes live as part of it.
        let subj = [source("fin", 2, 120, NOW - 120_000)];
        let before = run(NOW - 1000, post_s, &subj, &[], &s, true, None);
        assert!(brings(&before).is_empty(), "not before the post");
        let p = before.post.as_ref().unwrap();
        assert_eq!(p.on_air_at_ms, Some(NOW));
        assert_eq!(p.state, Some(SourceState::StandingBy));
        assert_eq!(p.show.as_ref().unwrap().slug, "kettle");
        assert!(!p.show.as_ref().unwrap().started);
        let at = run(NOW, post_s, &subj, &[], &s, true, None);
        assert_eq!(brings(&at), [("fin", Cause::AtPost)]);
        match &at.acts[0] {
            Act::BringOn {
                set_live, takeoff, ..
            } => assert!(*set_live && takeoff.is_empty()),
            a => panic!("{a:?}"),
        }
    }

    #[test]
    fn a_source_not_yet_steady_is_not_standing_by() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let subj = [source("fin", 2, 3, NOW - 3000)];
        let p = run(NOW + 1000, post_s, &subj, &[], &s, true, None);
        assert!(brings(&p).is_empty());
        assert_eq!(p.post.unwrap().state, Some(SourceState::Connecting));
    }

    #[test]
    fn a_dj_on_air_keeps_it_until_the_grace_ends_and_then_the_schedule_hands_over() {
        let s = SourceSettings::default(); // 120 s
        let post_s = NOW / 1000;
        let subj = [source("fin", 2, 600, NOW - 600_000)];
        // Just after the post: the grace counts from the post.
        let p = run(NOW + 1000, post_s, &subj, &[dj()], &s, false, None);
        assert!(brings(&p).is_empty());
        assert_eq!(p.post.as_ref().unwrap().grace_ends_ms, Some(NOW + 120_000));
        assert_eq!(p.post.as_ref().unwrap().holders, ["Heather"]);
        // At its end: the handover, with the DJ taken off.
        let p = run(NOW + 120_000, post_s, &subj, &[dj()], &s, false, None);
        match &p.acts[..] {
            [Act::BringOn { cause, takeoff, .. }] => {
                assert_eq!(*cause, Cause::Grace);
                assert_eq!(takeoff, &["dj-1".to_owned()]);
            }
            a => panic!("{a:?}"),
        }
        // Grace off: a press only.
        let off = SourceSettings { grace_s: 0, ..s };
        let p = run(NOW + 999_000, post_s, &subj, &[dj()], &off, false, None);
        assert!(brings(&p).is_empty() && p.post.unwrap().grace_ends_ms.is_none());
    }

    #[test]
    fn the_grace_starts_when_the_next_host_is_standing_by_if_they_are_late() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        // Connected five minutes after the post (steady 5 s later).
        let since = NOW + 300_000;
        let subj = [source("fin", 2, 10, since)];
        let p = run(since + 10_000, post_s, &subj, &[dj()], &s, false, None);
        assert_eq!(
            p.post.unwrap().grace_ends_ms,
            Some(since + 5_000 + 120_000),
            "from the moment it stood by"
        );
    }

    #[test]
    fn a_hold_stops_the_grace_for_this_handover_only() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let subj = [source("fin", 2, 600, NOW - 600_000)];
        let hold = Hold {
            airing_start_s: post_s,
            by: "Shona".into(),
        };
        let p = run(
            NOW + 500_000,
            post_s,
            &subj,
            &[dj()],
            &s,
            false,
            Some(hold.clone()),
        );
        assert!(brings(&p).is_empty());
        let v = p.post.unwrap();
        assert_eq!(
            (v.held_by.as_deref(), v.grace_ends_ms),
            (Some("Shona"), None)
        );
        // A hold on another airing is no hold on this one.
        let other = Hold {
            airing_start_s: post_s - 99,
            ..hold
        };
        let p = run(
            NOW + 500_000,
            post_s,
            &subj,
            &[dj()],
            &s,
            false,
            Some(other),
        );
        assert_eq!(brings(&p), [("fin", Cause::Grace)]);
    }

    #[test]
    fn with_nobody_standing_by_nothing_happens() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let p = run(NOW + 5000, post_s, &[], &[dj()], &s, false, None);
        assert!(p.acts.is_empty());
        let nobody = [Subject {
            at_desk: None,
            ..source("fin", 2, 0, NOW)
        }];
        let p = run(NOW + 5000, post_s, &nobody, &[dj()], &s, false, None);
        assert!(p.acts.is_empty());
        assert_eq!(p.post.unwrap().state, Some(SourceState::NotConnected));
    }

    #[test]
    fn a_late_host_goes_on_air_after_the_count_unless_told_to_wait() {
        let s = SourceSettings::default(); // 20 s
        let post_s = NOW / 1000;
        // Connected ten minutes after the post; steady at since + 5 s.
        let since = NOW + 600_000;
        let subj = [source("fin", 2, 6, since)];
        let stood = since + 5_000;
        let p = run(since + 6_000, post_s, &subj, &[], &s, true, None);
        assert!(brings(&p).is_empty());
        assert_eq!(p.post.as_ref().unwrap().late_ends_ms, Some(stood + 20_000));
        let p = run(stood + 20_000, post_s, &subj, &[], &s, true, None);
        assert_eq!(brings(&p), [("fin", Cause::LateCount)]);
        // Not yet: it waits for the host's own press.
        let mut waited = subj.clone();
        waited[0].log = vec![line(stood + 10_000, "not_yet")];
        let p = run(stood + 60_000, post_s, &waited, &[], &s, true, None);
        assert!(brings(&p).is_empty());
        assert_eq!(p.post.unwrap().late_ends_ms, None);
        // The count off: a press only.
        let off = SourceSettings {
            late_count_s: 0,
            ..s
        };
        let p = run(stood + 60_000, post_s, &subj, &[], &off, true, None);
        assert!(brings(&p).is_empty());
    }

    #[test]
    fn a_source_is_brought_on_at_most_once_an_airing() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let mut subj = [source("fin", 2, 600, NOW - 600_000)];
        // Put on air at the post, then taken off by a producer: the schedule leaves it.
        subj[0].log = vec![line(NOW + 30_000, "off_air"), line(NOW + 1_000, "on_air")];
        let p = run(NOW + 60_000, post_s, &subj, &[], &s, true, None);
        assert!(brings(&p).is_empty(), "{p:?}");
    }

    /// A room presenter on their own link, as the show's presenter.
    fn presenter(on_air: bool) -> Subject {
        Subject {
            id: "presenter-me-abc".into(),
            identity: "presenter-me-abc".into(),
            name: "Shona".into(),
            show: show(2),
            in_room: true,
            since_ms: None,
            at_desk: Some(AtDesk {
                on_air,
                here: true,
                connected_ms: None,
            }),
            log: Vec::new(),
        }
    }

    #[test]
    fn someone_on_air_before_the_post_and_taken_off_after_it_is_not_put_back() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        // A presenter on air from joining, before the post, goes off mid-show while a second
        // presenter holds the air and the grace is long over.
        let mut me = presenter(false);
        me.log = vec![
            line(NOW + 600_000, "off_air"),
            line(NOW - 300_000, "on_air"),
        ];
        let co = Holder {
            identity: "presenter-co".into(),
            name: "Ewan".into(),
        };
        let p = run(
            NOW + 700_000,
            post_s,
            std::slice::from_ref(&me),
            &[co],
            &s,
            false,
            None,
        );
        assert!(brings(&p).is_empty(), "{p:?}");
        assert!(p.ready.is_none(), "End show does not put them back either");
        // The same for a source put on air before the post (a press, or End show's pass) and
        // taken off after it with nobody else on air.
        let mut src = source("fin", 2, 600, NOW - 600_000);
        src.log = vec![line(NOW + 30_000, "off_air"), line(NOW - 60_000, "on_air")];
        let p = run(NOW + 40_000, post_s, &[src], &[], &s, true, None);
        assert!(brings(&p).is_empty(), "{p:?}");
        assert!(p.ready.is_none());
    }

    #[test]
    fn a_press_seen_now_is_not_acted_on_in_the_same_look() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        // On air by the log from before the post; off air at the desk now, after it: written
        // down, and nothing is brought on in the same look.
        let mut src = source("fin", 2, 600, NOW - 600_000);
        src.log = vec![line(NOW - 60_000, "on_air")];
        let p = run(NOW + 40_000, post_s, &[src], &[], &s, true, None);
        assert!(
            matches!(&p.acts[..], [Act::Observe { on: false, .. }]),
            "{p:?}"
        );
    }

    #[test]
    fn the_desks_own_presses_are_written_down() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let mut subj = [source("fin", 2, 600, NOW - 600_000)];
        subj[0].at_desk.as_mut().unwrap().on_air = true;
        let p = run(NOW + 60_000, post_s, &subj, &[], &s, true, None);
        assert!(
            matches!(&p.acts[..], [Act::Observe { on: true, .. }]),
            "{p:?}"
        );
        // Logged on, now off at the desk (a press): off air.
        subj[0].at_desk.as_mut().unwrap().on_air = false;
        subj[0].log = vec![line(NOW + 59_000, "on_air")];
        let p = run(NOW + 61_000, post_s, &subj, &[], &s, true, None);
        assert!(
            p.acts
                .iter()
                .any(|a| matches!(a, Act::Observe { on: false, .. })),
            "{p:?}"
        );
    }

    #[test]
    fn a_link_lost_is_marked_covered_after_the_silence_and_the_place_is_kept() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 - 1800; // mid-show of Kettle; the host was on air
        let lost_at = NOW;
        let mut gone = source("fin", 2, 0, NOW - 99_999);
        gone.at_desk = None;
        gone.log = vec![line(NOW - 60_000, "on_air")];
        // Nothing yet marked: the loss is marked now.
        let p = run(NOW, post_s, &[gone.clone()], &[], &s, false, None);
        assert!(
            matches!(&p.acts[..], [Act::MarkLost { at_ms, .. }] if *at_ms == NOW),
            "{p:?}"
        );
        // Marked: inside the silence nothing else; the view counts the place.
        gone.log.insert(0, line(lost_at, "link_lost"));
        let p = run(
            lost_at + 2_000,
            post_s,
            &[gone.clone()],
            &[],
            &s,
            false,
            None,
        );
        assert!(p.acts.is_empty(), "{p:?}");
        let v = p.post.unwrap().link_lost.unwrap();
        assert_eq!(
            (v.lost_ms, v.covers_ms, v.place_kept_until_ms, v.covered),
            (lost_at, lost_at + 4_000, lost_at + 300_000, false)
        );
        // After the silence with nobody on air: automation covers.
        let p = run(
            lost_at + 4_000,
            post_s,
            &[gone.clone()],
            &[],
            &s,
            false,
            None,
        );
        assert!(matches!(&p.acts[..], [Act::Cover { .. }]), "{p:?}");
        // Already covered: not again, even if the station was handed back to live meanwhile.
        gone.log.insert(0, line(lost_at + 4_100, "covered"));
        let p = run(
            lost_at + 9_000,
            post_s,
            &[gone.clone()],
            &[],
            &s,
            false,
            None,
        );
        assert!(p.acts.is_empty(), "{p:?}");
        // Someone else on air: the loss changes nothing.
        let mut other = gone.clone();
        other.log.remove(0);
        let p = run(lost_at + 9_000, post_s, &[other], &[dj()], &s, false, None);
        assert!(p.acts.is_empty(), "{p:?}");
        // Five minutes on: the place is let go.
        let p = run(lost_at + 300_000, post_s, &[gone], &[], &s, false, None);
        assert!(matches!(&p.acts[..], [Act::LetGo { .. }]), "{p:?}");
    }

    #[test]
    fn a_return_steady_for_the_time_goes_back_on_air_within_the_place_kept() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 - 1800;
        let lost_at = NOW;
        let mut back = source("fin", 2, 3, lost_at + 20_000);
        back.log = vec![line(lost_at, "link_lost"), line(lost_at - 1000, "on_air")];
        // Three seconds in: not steady.
        let p = run(
            lost_at + 23_000,
            post_s,
            &[back.clone()],
            &[],
            &s,
            true,
            None,
        );
        assert!(brings(&p).is_empty(), "{p:?}");
        // Steady five seconds, automation holds: back on air, mode live.
        back.at_desk.as_mut().unwrap().connected_ms = Some(5_000);
        let p = run(
            lost_at + 25_000,
            post_s,
            &[back.clone()],
            &[],
            &s,
            true,
            None,
        );
        assert_eq!(brings(&p), [("fin", Cause::Back)]);
        // Someone else on air: it stands by like anyone.
        let p = run(
            lost_at + 25_000,
            post_s,
            &[back.clone()],
            &[dj()],
            &s,
            false,
            None,
        );
        assert!(brings(&p).is_empty());
        // Past the place kept: let go, no return.
        let p = run(lost_at + 301_000, post_s, &[back], &[], &s, true, None);
        assert!(brings(&p).is_empty());
        assert!(
            p.acts.iter().any(|a| matches!(a, Act::LetGo { .. })),
            "{p:?}"
        );
    }

    #[test]
    fn three_drops_in_ten_minutes_make_the_steady_time_thirty_seconds() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 - 1800;
        let mut back = source("fin", 2, 10, NOW);
        back.log = vec![
            line(NOW - 1_000, "link_lost"),
            line(NOW - 100_000, "link_lost"),
            line(NOW - 200_000, "link_lost"),
        ];
        let p = run(NOW + 10_000, post_s, &[back.clone()], &[], &s, true, None);
        assert!(brings(&p).is_empty(), "ten seconds is not thirty: {p:?}");
        back.at_desk.as_mut().unwrap().connected_ms = Some(30_000);
        let p = run(NOW + 30_000, post_s, &[back.clone()], &[], &s, true, None);
        assert_eq!(brings(&p), [("fin", Cause::Back)]);
        // The drops fall out of the window: five seconds again.
        let mut calm = source("fin", 2, 5, NOW);
        calm.log = vec![line(NOW - 700_000, "link_lost")];
        assert!(calm.ready(&s, NOW));
    }

    #[test]
    fn a_drop_during_a_grace_hands_over_at_once() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 - 30; // Kettle's post was 30 s ago; Steeped's host was on air
        let mut old = source("old", 1, 0, NOW - 99_999);
        old.at_desk = None;
        old.log = vec![
            line(NOW - 5_000, "link_lost"),
            line(NOW - 900_000, "on_air"),
        ];
        let next = source("fin", 2, 600, NOW - 600_000);
        // Four seconds after the loss, nobody on air, the next standing by: hand over.
        let p = run(
            NOW - 1_000,
            post_s,
            &[old.clone(), next.clone()],
            &[],
            &s,
            false,
            None,
        );
        assert_eq!(brings(&p), [("fin", Cause::DropHandover)]);
        assert!(
            p.acts
                .iter()
                .any(|a| matches!(a, Act::LetGo { subject, .. } if subject == "old")),
            "the dropped host's place is let go: {p:?}"
        );
        assert!(
            !p.acts.iter().any(|a| matches!(a, Act::Cover { .. })),
            "automation does not cover"
        );
        // Inside the silence: nothing yet.
        let p = run(NOW - 3_000, post_s, &[old, next], &[], &s, false, None);
        assert!(p.acts.is_empty(), "{p:?}");
    }

    #[test]
    fn a_reconnection_the_desk_replaced_in_place_is_a_lost_link() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 - 1800;
        // On air by the log for a minute; the desk's connection is two seconds old and off air.
        let mut fin = source("fin", 2, 2, NOW - 2_000);
        fin.log = vec![line(NOW - 60_000, "on_air")];
        let p = run(NOW, post_s, &[fin], &[], &s, true, None);
        assert!(
            matches!(&p.acts[..], [Act::MarkLost { at_ms, .. }] if *at_ms == NOW - 2_000),
            "{p:?}"
        );
    }

    #[test]
    fn a_room_presenter_is_brought_on_only_by_the_grace_never_from_automation() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000;
        let me = Subject {
            id: "presenter-me-abc".into(),
            identity: "presenter-me-abc".into(),
            name: "Shona".into(),
            show: show(2),
            in_room: true,
            since_ms: None,
            at_desk: Some(AtDesk {
                on_air: false,
                here: true,
                connected_ms: None,
            }),
            log: Vec::new(),
        };
        let p = run(
            NOW + 10_000,
            post_s,
            std::slice::from_ref(&me),
            &[],
            &s,
            true,
            None,
        );
        assert!(
            brings(&p).is_empty(),
            "no one is put on air from automation by the clock"
        );
        let p = run(NOW + 130_000, post_s, &[me], &[dj()], &s, false, None);
        assert_eq!(brings(&p), [("presenter-me-abc", Cause::Grace)]);
    }

    #[test]
    fn without_the_playout_layer_only_an_empty_air_is_automations() {
        let s = SourceSettings::default();
        let a = airings(NOW / 1000);
        let sh = shows();
        let subj = [source("fin", 2, 600, NOW - 600_000)];
        let facts = |holders: &'static [Holder]| Facts {
            now_ms: NOW,
            settings: &s,
            automation: false,
            auto_mode: false,
            airings: &a,
            shows: &sh,
            subjects: &subj,
            holders,
            hold: None,
        };
        match &plan(&facts(&[])).acts[..] {
            [Act::BringOn { set_live, .. }] => assert!(!set_live, "no mode to set"),
            x => panic!("{x:?}"),
        }
    }

    #[test]
    fn end_show_would_pass_the_air_to_the_ready_source() {
        let s = SourceSettings::default();
        let post_s = NOW / 1000 + 600; // the next show is ten minutes away
        let subj = [source("fin", 2, 600, NOW - 600_000)];
        let p = run(NOW, post_s, &subj, &[dj()], &s, false, None);
        assert_eq!(p.ready.unwrap().subject, "fin");
        // Not steady: not ready.
        let subj = [source("fin", 2, 2, NOW - 2_000)];
        assert!(
            run(NOW, post_s, &subj, &[dj()], &s, false, None)
                .ready
                .is_none()
        );
    }

    #[test]
    fn the_schedules_moves_and_end_show_fade_and_a_hosts_own_presses_do_not() {
        for c in [
            Cause::AtPost,
            Cause::LateCount,
            Cause::Grace,
            Cause::Back,
            Cause::DropHandover,
            Cause::EndShow,
        ] {
            assert!(c.fades(), "{c:?}");
        }
        for c in [Cause::Go, Cause::Handover] {
            assert!(!c.fades(), "{c:?}");
        }
    }
}
