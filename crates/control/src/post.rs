// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The post's rule at work (ADR-0280; design sections 3.3 to 3.6, 3.8 and 10): every second the
//! control plane gathers the facts (the schedule, the desk, the ingest's report, the source log,
//! the rules), asks [`weavesignal_model::post::plan`] what to do, does it through what exists (the
//! desk's On for a slot, the station's mode, the handover's two steps), logs each act as the
//! schedule's, and keeps the studio's `post` view up to date.
//!
//! Nothing is remembered between seconds except the log, so a restart changes nothing: the same
//! facts give the same acts.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `POST`, `DELETE /api/v1/air/post/hold` | admin or producer | hold this handover's grace, or let it go |
//! | `POST /api/v1/sources/{key}/go` | staff at the desk | the late count's *Go on air now* |
//! | `POST /api/v1/sources/{key}/not-yet` | staff at the desk | stop the late count for this airing |
//! | `POST /api/v1/sources/{key}/off-air` | staff at the desk | the toast's *Off air*: let a lost source's kept place go at once |
//!
//! **The crossfade.** The schedule's moves (the post, the late count, a grace's end, a return,
//! a drop's handover) and End show passing the air fade over the rules' `fade_ms` (2 s): the
//! desk's On is told how long to fade ([`DeskCommand::OnAir`]'s `fade_ms`) and the one coming on
//! rises as the one going off falls. A person's press (Go on air now, the handover's two presses,
//! a switch at the desk) stays instant: [`weavesignal_model::post::Cause::fades`] says which.
//! [`move_air`] is the one place that tells the desk.
//!
//! **The handover window** (Phase 17c, design note `handover-window.md`): after the rule, the same
//! facts and the rule's answer go to [`weavesignal_model::handover_window::for_post`], whose view
//! rides in `post.window`. When it says a heads-up is due, a `heads_up` line goes under the post's
//! key first (`airing <start> to <name>`), and only once it is down does the desk get its notice
//! (`handover.heads_up`) and the host's monitor link its line; a restart reads the line and does
//! not tell them twice. The window changes no act.

// Every helper that checks a request returns the refusal itself as its error, as the handlers send it.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::handover_window::{self as hw, Down, Origin};
use weavesignal_model::post::{
    self as rule, Act, AtDesk, Cause, Facts, Hold, Holder, Line, Plan, PostOpening, PostView,
    Ready, ShowFact, Subject,
};
use weavesignal_model::shows::{self, Show};
use weavesignal_model::sources::{self as model, Airing, LogEntry, ShowKey};
use weavesignal_proto::{Role, source_wire};

use crate::app::{AppState, problem};
use crate::desk::{DeskCommand, DeskPerson, StationMode};
use crate::hub::now_ms;
use crate::staff::{Actor, Need};

/// How far ahead the schedule is read for the next post.
const LOOK_AHEAD_S: i64 = 7 * 24 * 3600;
/// How far back the schedule is read for the show on air.
const LOOK_BACK_S: i64 = 30 * 3600;
/// How far back the log is read: a source on air since before this is not tracked.
const LOG_WINDOW_MS: i64 = 12 * 3600 * 1000;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/air/post/hold", post(hold).delete(unhold))
        .route("/api/v1/sources/{key}/go", post(go))
        .route("/api/v1/sources/{key}/not-yet", post(not_yet))
        .route("/api/v1/sources/{key}/off-air", post(off_air))
}

// ---- gathering the facts ----

/// Everything the rule needs, owned.
struct Gathered {
    settings: model::SourceSettings,
    airings: Vec<Airing>,
    shows: Vec<ShowFact>,
    subjects: Vec<Subject>,
    holders: Vec<Holder>,
    hold: Option<Hold>,
    automation: bool,
    auto_mode: bool,
    /// Who is down for each show, for the handover window.
    down: Vec<Down>,
    /// The heads-ups the log holds: `(airing start, when)`.
    told: Vec<(i64, i64)>,
}

impl Gathered {
    fn facts(&self, now_ms: i64) -> Facts<'_> {
        Facts {
            now_ms,
            settings: &self.settings,
            automation: self.automation,
            auto_mode: self.auto_mode,
            airings: &self.airings,
            shows: &self.shows,
            subjects: &self.subjects,
            holders: &self.holders,
            hold: self.hold.clone(),
        }
    }

    /// The handover window for the post `plan` answered with.
    fn window(&self, now_ms: i64, post: Option<&PostView>) -> hw::AtPost {
        hw::for_post(&hw::PostFacts {
            now_ms,
            window_s: i64::from(self.settings.window_s),
            airings: &self.airings,
            shows: &self.shows,
            subjects: &self.subjects,
            holders: &self.holders,
            hold: self.hold.as_ref(),
            post,
            down: &self.down,
            told: &self.told,
            steady_ms: u64::try_from(self.settings.steady_s).unwrap_or(0) * 1000,
        })
    }
}

/// A key as the rule sees it: at the desk or not, connected to the ingest or not.
pub(crate) fn key_subject(
    k: &ShowKey,
    people: &[DeskPerson],
    conns: &[crate::sources::IngestConn],
    log: Vec<Line>,
) -> Subject {
    let identity = source_wire::identity(&k.key_id);
    let person = people.iter().find(|p| p.identity == identity);
    Subject {
        id: k.key_id.clone(),
        name: k.label.clone(),
        show: k.show_id,
        in_room: false,
        since_ms: conns
            .iter()
            .find(|c| c.key_id == k.key_id && c.mode == "standby")
            .and_then(|c| i64::try_from(c.since_ms).ok()),
        at_desk: person.map(|p| AtDesk {
            on_air: p.on_air,
            here: p.present && p.linked,
            connected_ms: Some(p.source.as_ref().map_or(0, |s| s.connected_ms)),
        }),
        log,
        identity,
    }
}

/// A show's presenter in the room, signed in as themselves (only their exact account's own link
/// counts), if they are at the desk.
pub(crate) fn room_subject(
    show: &Show,
    people: &[DeskPerson],
    log: impl FnOnce(&str) -> Vec<Line>,
) -> Option<Subject> {
    let identity = crate::steps::own_identity(show.presenter_account?);
    let p = people
        .iter()
        .find(|p| p.identity == identity && matches!(p.role, Role::Dj | Role::Presenter))?;
    Some(Subject {
        id: identity.clone(),
        name: if p.name.is_empty() {
            identity.clone()
        } else {
            p.name.clone()
        },
        show: show.id,
        in_room: true,
        since_ms: None,
        at_desk: Some(AtDesk {
            on_air: p.on_air,
            here: p.present && p.linked,
            connected_ms: None,
        }),
        log: log(&identity),
        identity,
    })
}

/// Who is down for each show: the labels of its keys that have not ended (it comes from their
/// own studio), else its presenter (the room).
pub(crate) fn down_of(all_shows: &[Show], keys: &[ShowKey]) -> Vec<Down> {
    all_shows
        .iter()
        .map(|show| {
            let labels: Vec<String> = keys
                .iter()
                .filter(|k| k.show_id == show.id)
                .map(|k| k.label.clone())
                .collect();
            if labels.is_empty() {
                Down {
                    show: show.id,
                    names: [show.presenter.trim()]
                        .into_iter()
                        .filter(|n| !n.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    from: Origin::Room,
                }
            } else {
                Down {
                    show: show.id,
                    names: labels,
                    from: Origin::Push,
                }
            }
        })
        .collect()
}

fn line_of(e: &LogEntry) -> Line {
    Line {
        at_ms: e.at_ms,
        event: e.event.clone(),
        reason: e.reason.clone(),
        detail: e.detail.clone(),
    }
}

/// The hold the log last said, as `airing <start> by <who>`.
fn hold_of(post_lines: &[&LogEntry]) -> Option<Hold> {
    let last = post_lines
        .iter()
        .find(|l| l.event == "hold" || l.event == "hold_released")?;
    if last.event != "hold" {
        return None;
    }
    let rest = last.detail.strip_prefix("airing ")?;
    let (start, by) = rest.split_once(" by ")?;
    Some(Hold {
        airing_start_s: start.parse().ok()?,
        by: by.to_owned(),
    })
}

async fn gather(state: &AppState, now_ms: i64) -> Result<Option<Gathered>, sqlx::Error> {
    let Some(pool) = &state.db else {
        return Ok(None);
    };
    // Nothing is known of who is on air while the desk does not answer: nothing is done.
    let Some(desk) = state.hub.desk_status() else {
        return Ok(None);
    };
    let station = state.station.id;
    let now_s = now_ms.div_euclid(1000);
    let (settings, keys, all_shows, eps, log) = tokio::try_join!(
        model::get_settings(pool, station),
        model::live_keys(pool, station, now_ms),
        shows::list_shows(pool, station),
        shows::episodes_between(pool, station, now_s - LOOK_BACK_S, now_s + LOOK_AHEAD_S),
        model::post_log(pool, station, now_ms - LOG_WINDOW_MS),
    )?;
    let airings: Vec<Airing> = eps
        .iter()
        .filter_map(|(show, series, ep)| {
            shows::air_window(show, series, ep).map(|(start, end)| Airing {
                show: show.id,
                start,
                end,
            })
        })
        .collect();
    let by_key: BTreeMap<&str, Vec<&LogEntry>> = log.iter().fold(BTreeMap::new(), |mut m, l| {
        m.entry(l.key_id.as_str()).or_default().push(l);
        m
    });
    let lines = |id: &str| -> Vec<Line> {
        by_key
            .get(id)
            .map(|v| v.iter().map(|l| line_of(l)).collect())
            .unwrap_or_default()
    };
    let conns = state.hub.sources.connections();
    let mut subjects = Vec::new();
    for k in &keys {
        // A key of a show that was deleted has no show to bring on for.
        if !all_shows.iter().any(|s| s.id == k.show_id) {
            continue;
        }
        subjects.push(key_subject(k, &desk.people, &conns, lines(&k.key_id)));
    }
    // A show's presenter in the room, signed in as themselves, for the show on air and the next
    // (only their exact account's own link counts).
    let near: Vec<Uuid> = [
        rule::airing_now(&airings, now_s),
        rule::airing_next(&airings, now_s),
    ]
    .into_iter()
    .flatten()
    .map(|a| a.show)
    .collect();
    for show in all_shows.iter().filter(|s| near.contains(&s.id)) {
        if let Some(sub) = room_subject(show, &desk.people, lines) {
            subjects.push(sub);
        }
    }
    let holders = desk
        .people
        .iter()
        .filter(|p| p.on_air && matches!(p.role, Role::Presenter | Role::Dj | Role::Source))
        .map(|p| Holder {
            identity: p.identity.clone(),
            name: if p.name.is_empty() {
                p.identity.clone()
            } else {
                p.name.clone()
            },
        })
        .collect();
    let post_lines = by_key.get(rule::POST_KEY).cloned().unwrap_or_default();
    let told = post_lines
        .iter()
        .filter(|l| l.event == "heads_up")
        .filter_map(|l| hw::heads_up_airing(&l.detail).map(|start| (start, l.at_ms)))
        .collect();
    let (automation, auto_mode) = (
        state.hub.onair.get().is_some(),
        state.hub.read_studio(|s| s.station_mode()) == Some(StationMode::Auto),
    );
    Ok(Some(Gathered {
        settings: state.hub.sources.in_force(settings),
        airings,
        shows: all_shows
            .iter()
            .map(|s| ShowFact {
                id: s.id,
                slug: s.slug.clone(),
                name: s.name.clone(),
            })
            .collect(),
        subjects,
        holders,
        hold: hold_of(&post_lines),
        automation,
        auto_mode,
        down: down_of(&all_shows, &keys),
        told,
    }))
}

// ---- doing it ----

/// Write a line of the post's own, loudly: a rejected line would let the rule act twice.
pub(crate) async fn write(
    state: &AppState,
    show: Option<Uuid>,
    key_id: &str,
    at_ms: i64,
    event: &str,
    reason: &str,
    detail: String,
) -> Result<(), sqlx::Error> {
    let Some(pool) = &state.db else {
        return Ok(());
    };
    model::log(
        pool,
        state.station.id,
        show,
        &LogEntry {
            at_ms,
            key_id: key_id.to_owned(),
            event: event.to_owned(),
            reason: Some(reason.to_owned()),
            detail,
            way: None,
            address: None,
        },
    )
    .await
}

/// One On or Off for the desk, fading over `fade_ms` when it is more than 0. A desk from before
/// the fade refuses the field it does not know; the move is then made at once, which is what the
/// station did before, rather than not made.
pub(crate) async fn on_air_command(
    state: &AppState,
    identity: &str,
    on: bool,
    fade_ms: i32,
) -> Result<(), crate::desk::DeskError> {
    let fade_ms = u32::try_from(fade_ms).ok().filter(|ms| *ms > 0);
    let send = |fade_ms| async move {
        state
            .hub
            .command(&DeskCommand::OnAir {
                identity: identity.to_owned(),
                on,
                fade_ms,
            })
            .await
    };
    match send(fade_ms).await {
        Err(crate::desk::DeskError::Refused(_) | crate::desk::DeskError::Rejected { .. })
            if fade_ms.is_some() =>
        {
            tracing::warn!(%identity, "the desk would not fade; switching at once");
            send(None).await
        }
        other => other,
    }
}

/// The two presses of a handover: `to` on air, then everyone in `takeoff` off, both over
/// `fade_ms` when it is more than 0 (0 is at once).
pub(crate) async fn move_air(
    state: &AppState,
    to: &str,
    takeoff: &[String],
    fade_ms: i32,
) -> Result<(), crate::desk::DeskError> {
    on_air_command(state, to, true, fade_ms).await?;
    for t in takeoff {
        on_air_command(state, t, false, fade_ms).await?;
    }
    Ok(())
}

/// A key's id for a desk identity, if it is a source's.
fn key_of(identity: &str) -> Option<&str> {
    identity.strip_prefix("source-")
}

/// Put someone on air as the rule says, and write down what was done. `by` is the line's reason:
/// `schedule`, or who pressed.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn bring_on(
    state: &AppState,
    subject: &str,
    identity: &str,
    show: Uuid,
    cause: Cause,
    takeoff: &[String],
    set_live: bool,
    fade_ms: i32,
    by: &str,
) -> anyhow::Result<()> {
    let now = now_ms() as i64;
    // Only the schedule's moves and End show fade; a press is at once.
    let fade_ms = if cause.fades() { fade_ms } else { 0 };
    move_air(state, identity, takeoff, fade_ms).await?;
    if set_live && let Some(onair) = state.hub.onair.get() {
        onair.set_mode(StationMode::Live).await?;
    }
    // The show brought on: an opening from rehearsal for it is applied now (1.3).
    crate::rehearsal::apply_openings(
        state,
        weavesignal_model::rehearsal::Moment::BringOn,
        Some(show),
    )
    .await;
    let event = if cause == Cause::Back {
        "back"
    } else {
        "on_air"
    };
    write(
        state,
        Some(show),
        subject,
        now,
        event,
        by,
        format!("by {by}: {}", cause.words()),
    )
    .await?;
    for t in takeoff {
        if let Some(k) = key_of(t) {
            write(
                state,
                None,
                k,
                now,
                "off_air",
                by,
                format!("by {by}: {identity} took the air"),
            )
            .await?;
        }
    }
    tracing::info!(who = %identity, ?cause, %by, "the post: on air");
    Ok(())
}

/// Taking the air back from automation (1.3). Invitations arrive standing by, so the press that
/// takes the air back also puts on air, at once, the presenter it is for: the show on air by the
/// schedule's own presenter (their own link), and the person who pressed (their own link), each
/// if they are at the desk standing by. Then an opening from rehearsal for the show on air is
/// applied: taking the air back while it is on air by the schedule is its moment.
pub(crate) async fn take_air_back(state: &AppState, actor: &Actor) {
    let now_s = (now_ms() as i64).div_euclid(1000);
    let on_air_show = match &state.db {
        Some(pool) => {
            match shows::episodes_between(pool, state.station.id, now_s - LOOK_BACK_S, now_s + 1)
                .await
            {
                Ok(eps) => {
                    let airings: Vec<Airing> = eps
                        .iter()
                        .filter_map(|(show, series, ep)| {
                            shows::air_window(show, series, ep).map(|(start, end)| Airing {
                                show: show.id,
                                start,
                                end,
                            })
                        })
                        .collect();
                    rule::airing_now(&airings, now_s).and_then(|a| {
                        eps.iter()
                            .find(|(s, _, _)| s.id == a.show)
                            .map(|(s, _, _)| s.clone())
                    })
                }
                Err(e) => {
                    tracing::warn!(error = %e, "taking the air back: the schedule could not be read");
                    None
                }
            }
        }
        None => None,
    };
    let mut wanted: Vec<String> = Vec::new();
    if let Some(account) = on_air_show.as_ref().and_then(|s| s.presenter_account) {
        wanted.push(crate::steps::own_identity(account));
    }
    if let Actor::Staff(a) = actor {
        let own = crate::steps::own_identity(a.id);
        if !wanted.contains(&own) {
            wanted.push(own);
        }
    }
    let people = state
        .hub
        .desk_status()
        .map(|d| d.people)
        .unwrap_or_default();
    for id in wanted {
        let standing_by = people.iter().any(|p| {
            p.identity == id
                && p.present
                && !p.on_air
                && matches!(p.role, Role::Presenter | Role::Dj)
        });
        if !standing_by {
            continue;
        }
        match on_air_command(state, &id, true, 0).await {
            Ok(()) => {
                tracing::info!(identity = %id, by = %actor.label(), "taking the air back put the presenter on air")
            }
            Err(e) => {
                tracing::warn!(identity = %id, error = %e, "taking the air back could not put the presenter on air")
            }
        }
    }
    crate::rehearsal::apply_openings(
        state,
        weavesignal_model::rehearsal::Moment::TakeAir,
        on_air_show.map(|s| s.id),
    )
    .await;
}

/// The handover's second press: everyone in `takeoff` off air, and the sources among them
/// written down as taken off by `by` (the schedule would otherwise read it as a press at the
/// desk and not know whose). Nobody is put on air: the one who holds the air already does.
pub(crate) async fn take_off(
    state: &AppState,
    takeoff: &[String],
    held_by: &str,
    by: &str,
) -> Result<(), crate::desk::DeskError> {
    let now = now_ms() as i64;
    for t in takeoff {
        state
            .hub
            .command(&DeskCommand::OnAir {
                identity: t.clone(),
                on: false,
                fade_ms: None,
            })
            .await?;
    }
    for t in takeoff {
        if let Some(k) = key_of(t) {
            let _ = write(
                state,
                None,
                k,
                now,
                "off_air",
                by,
                format!("by {by}: {held_by} took the air"),
            )
            .await;
        }
    }
    tracing::info!(who = %held_by, %by, "the post: the outgoing are off air");
    Ok(())
}

async fn act(
    state: &AppState,
    settings: &model::SourceSettings,
    a: &Act,
    now: i64,
) -> anyhow::Result<()> {
    match a {
        Act::BringOn {
            subject,
            identity,
            show,
            cause,
            takeoff,
            set_live,
            ..
        } => {
            bring_on(
                state,
                subject,
                identity,
                *show,
                *cause,
                takeoff,
                *set_live,
                settings.fade_ms,
                rule::BY_SCHEDULE,
            )
            .await?;
        }
        Act::Observe { subject, show, on } => {
            write(
                state,
                Some(*show),
                subject,
                now,
                if *on { "on_air" } else { "off_air" },
                rule::BY_DESK,
                "seen at the desk".into(),
            )
            .await?;
        }
        Act::MarkLost {
            subject,
            show,
            at_ms,
        } => {
            write(
                state,
                Some(*show),
                subject,
                *at_ms,
                "link_lost",
                rule::BY_SCHEDULE,
                "the link of a source on air was lost".into(),
            )
            .await?;
            tracing::warn!(source = %subject, "the post: a source on air lost its link");
        }
        Act::Cover { subject, show } => {
            if let Some(onair) = state.hub.onair.get() {
                onair.set_mode(StationMode::Auto).await?;
            }
            write(
                state,
                Some(*show),
                subject,
                now,
                "covered",
                rule::BY_SCHEDULE,
                "automation took the air from a lost link".into(),
            )
            .await?;
            tracing::warn!(source = %subject, "the post: automation covers a lost link");
        }
        Act::LetGo { subject, show } => {
            write(
                state,
                Some(*show),
                subject,
                now,
                "place_let_go",
                rule::BY_SCHEDULE,
                "its place was let go".into(),
            )
            .await?;
        }
    }
    Ok(())
}

/// The opening state a move from the rehearsal studio left for the show `slug`, while it waits
/// to be applied (1.3.1).
pub(crate) fn opening_for(state: &AppState, slug: &str, now: i64) -> Option<PostOpening> {
    state
        .hub
        .rehearsal
        .get()?
        .waiting_moves(now)
        .into_iter()
        .find(|s| s.show_slug == slug)
        .and_then(|s| s.opening)
        .map(|o| PostOpening {
            on: o.on,
            mute: o.mute,
            host: o.host,
            standby_until_ms: o.standby_until_ms,
        })
}

/// Who was moved from the rehearsal studio and still waits for the opening state, with the time
/// they stand by until (1.3.1): what `/studio` marks "from rehearsal" on their source.
pub(crate) fn moved_standing(
    state: &AppState,
    now: i64,
) -> std::collections::BTreeMap<String, i64> {
    let mut moved = std::collections::BTreeMap::new();
    if let Some(r) = state.hub.rehearsal.get() {
        for s in r.waiting_moves(now) {
            let Some(o) = s.opening.as_ref() else {
                continue;
            };
            for id in &s.moved {
                let until = moved.entry(id.clone()).or_insert(o.standby_until_ms);
                *until = (*until).max(o.standby_until_ms);
            }
        }
    }
    moved
}

/// One look: gather, decide, do, and show. Called every second, and by tests.
pub async fn tick(state: &AppState) -> anyhow::Result<Plan> {
    let now = now_ms() as i64;
    let Some(g) = gather(state, now).await? else {
        let moved = moved_standing(state, now);
        state.hub.with_studio_if(|s| {
            let a = s.set_moved_standing(moved);
            let b = s.set_post(None);
            a || b
        });
        return Ok(Plan {
            acts: Vec::new(),
            post: None,
            ready: None,
        });
    };
    let plan = rule::plan(&g.facts(now));
    for a in &plan.acts {
        if let Err(e) = act(state, &g.settings, a, now).await {
            // Tried again at the next look; a refusal is not a reason to stop looking.
            tracing::warn!(error = %format!("{e:#}"), act = ?a, "the post's rule could not do what it meant to");
        }
    }
    let mut view = plan.post.clone();
    let at = g.window(now, plan.post.as_ref());
    let mut window = at.view;
    if let Some(h) = at.heads_up
        && heads_up(state, &h, now).await
        && let Some(w) = window.as_mut()
    {
        w.heads_up_ms = Some(now);
    }
    // The rehearsal studio (1.3): the window says when the show it is about is rehearsing, and
    // the rehearsal view shows the window of its own show.
    if let Some(r) = state.hub.rehearsal.get() {
        let rehearsing = r.rehearsing();
        if let Some(w) = window.as_mut() {
            w.rehearsing =
                rehearsing.is_some_and(|id| g.shows.iter().any(|s| s.id == id && s.slug == w.show));
        }
        r.set_window(window.as_ref());
    }
    if let Some(v) = view.as_mut() {
        v.window = window;
    }
    // The opening state moved from the rehearsal studio for the post's show (1.3.1).
    if let Some(v) = view.as_mut() {
        v.opening = v
            .show
            .as_ref()
            .and_then(|x| opening_for(state, &x.slug, now));
    }
    let moved = moved_standing(state, now);
    state.hub.with_studio_if(|s| {
        let a = s.set_moved_standing(moved);
        let b = s.set_post(view);
        a || b
    });
    // An opening from rehearsal whose show's host has just gone on air, by any route.
    crate::rehearsal::apply_openings(state, weavesignal_model::rehearsal::Moment::Look, None).await;
    Ok(plan)
}

/// Give the incoming host their heads-up: written down first, under the post's key, and only
/// then the desk's notice (the host's monitor link reads it from `post.window`). A line that
/// cannot be written tells nobody, and the next look tries again. Returns whether it was given.
async fn heads_up(state: &AppState, h: &hw::HeadsUp, now: i64) -> bool {
    if let Err(e) = write(
        state,
        Some(h.show),
        rule::POST_KEY,
        now,
        "heads_up",
        rule::BY_SCHEDULE,
        hw::heads_up_detail(h),
    )
    .await
    {
        tracing::warn!(error = %e, "the handover window: the heads-up could not be written down");
        return false;
    }
    let (title, message) = (h.title.clone(), h.message.clone());
    state
        .hub
        .with_studio(|s, at| s.heads_up(at, h.identity.as_deref(), title, message));
    tracing::info!(to = %h.name, airing = h.airing_start_s, "the handover window: heads-up given");
    true
}

/// Look every second until the process ends.
pub fn start(state: Arc<AppState>) {
    if state.db.is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut tick_every = tokio::time::interval(Duration::from_secs(1));
        tick_every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick_every.tick().await;
            if let Err(e) = tick(&state).await {
                tracing::debug!(error = %format!("{e:#}"), "the post's rule: a look failed");
            }
        }
    });
}

/// The source End show would pass the air to: the post's, if it is standing by.
pub async fn ready_for_end_show(state: &AppState) -> Option<(Ready, model::SourceSettings, bool)> {
    let now = now_ms() as i64;
    let g = gather(state, now).await.ok().flatten()?;
    let plan = rule::plan(&g.facts(now));
    plan.ready
        .map(|r| (r, g.settings, g.automation && g.auto_mode))
}

// ---- routes ----

fn may_hold(actor: &Actor) -> bool {
    use weavesignal_model::staff::Role as Staff;
    match actor {
        Actor::Station => true,
        Actor::Staff(a) => a.role.is_admin() || a.role == Staff::Producer,
        Actor::Token(_) => false,
    }
}

fn db_failed(e: &sqlx::Error) -> Response {
    tracing::error!(error = %e, "post: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

/// The airing a hold would hold: the post's show's start, as the studio's view says.
fn held_airing(state: &AppState) -> Option<i64> {
    state
        .hub
        .view()
        .post
        .as_ref()
        .and_then(|p| p.show.as_ref())
        .map(|s| s.start_ms.div_euclid(1000))
}

#[utoipa::path(
    post,
    path = "/api/v1/air/post/hold",
    operation_id = "post_hold",
    tag = "sources",
    summary = "Hold this handover",
    description = "Stops the grace for the handover now at the post (the next show's, as `post` in `GET /studio` names it): the schedule never takes the air from whoever holds it, and the handover is by a press only. It holds this handover alone; the next one is not held. For an admin or a producer; an API token never. 409 when there is no handover to hold.",
    responses(
        (status = 200, description = "Held."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn hold(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if !may_hold(&actor) {
        return problem(
            StatusCode::FORBIDDEN,
            "Only an admin or a producer holds a handover.",
        );
    }
    let Some(start) = held_airing(&state) else {
        return problem(StatusCode::CONFLICT, "there is no handover to hold");
    };
    let who = actor.label();
    match write(
        &state,
        None,
        rule::POST_KEY,
        now_ms() as i64,
        "hold",
        &who,
        format!("airing {start} by {who}"),
    )
    .await
    {
        Ok(()) => Json(json!({ "held": true, "by": who, "airing_start_ms": start * 1000 }))
            .into_response(),
        Err(e) => db_failed(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/air/post/hold",
    operation_id = "post_unhold",
    tag = "sources",
    summary = "Let this handover go",
    description = "Lets the held handover go: the grace runs again, counted from where the post and the next host's standing by put it (a grace that was already over hands over at the next look). For an admin or a producer.",
    responses(
        (status = 200, description = "Let go."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn unhold(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if !may_hold(&actor) {
        return problem(
            StatusCode::FORBIDDEN,
            "Only an admin or a producer holds a handover.",
        );
    }
    let who = actor.label();
    match write(
        &state,
        None,
        rule::POST_KEY,
        now_ms() as i64,
        "hold_released",
        &who,
        format!("by {who}"),
    )
    .await
    {
        Ok(()) => Json(json!({ "held": false })).into_response(),
        Err(e) => db_failed(&e),
    }
}

/// The source with this key, at the desk, and its show.
async fn source_at_desk(
    state: &AppState,
    key_id: &str,
) -> Result<(model::ShowKey, crate::desk::DeskPerson), Response> {
    let pool = state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "show keys are kept in the database",
        )
    })?;
    let key = match model::key_by_key_id(pool, state.station.id, key_id).await {
        Ok(Some(k)) if k.revoked_ms.is_none() => k,
        Ok(_) => return Err(problem(StatusCode::NOT_FOUND, format!("no key `{key_id}`"))),
        Err(e) => return Err(db_failed(&e)),
    };
    let identity = source_wire::identity(&key.key_id);
    let person = state
        .hub
        .desk_status()
        .and_then(|d| d.people.into_iter().find(|p| p.identity == identity));
    match person {
        Some(p) => Ok((key, p)),
        None => Err(problem(
            StatusCode::CONFLICT,
            "that source is not at the desk: it is not connected, or is only a test",
        )),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/sources/{key}/go",
    operation_id = "sources_go",
    tag = "sources",
    summary = "Go on air now",
    description = "Skips the late count: puts a source standing by on air at once, taking the air back from automation as the count would have. Refused (409) when someone else presents (the handover is by the post or a press then), when the source is not at the desk or not steady yet. For staff at the desk (a window preparing a show is refused, 409); an API token never.",
    params(("key" = String, Path, description = "The key's id")),
    responses(
        (status = 200, description = "On air."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn go(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Response {
    let actor = match state.check(&headers, Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if matches!(actor, Actor::Token(_)) {
        return problem(StatusCode::FORBIDDEN, "An API token cannot do that.");
    }
    match go_now(&state, &key_id, &actor.label()).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// *Go on air now*, whoever presses it (staff at the desk, or the host on their monitor link):
/// the one path. `by` is written down as who did it.
pub(crate) async fn go_now(
    state: &AppState,
    key_id: &str,
    by: &str,
) -> Result<serde_json::Value, Response> {
    let (key, person) = source_at_desk(state, key_id).await?;
    if person.on_air {
        return Ok(json!({ "on_air": true, "already": true }));
    }
    let now = now_ms() as i64;
    let settings =
        match model::get_settings(state.db.as_ref().expect("checked"), state.station.id).await {
            Ok(s) => state.hub.sources.in_force(s),
            Err(e) => return Err(db_failed(&e)),
        };
    let steady = person.source.as_ref().map_or(0, |s| s.connected_ms);
    if steady < settings.steady_s as u64 * 1000 {
        return Err(problem(
            StatusCode::CONFLICT,
            "that source is not steady yet: it stands by after a few seconds of steady sound",
        ));
    }
    let desk = state.hub.desk_status().unwrap_or_default();
    let others: Vec<String> = desk
        .people
        .iter()
        .filter(|p| {
            p.on_air
                && p.identity != person.identity
                && matches!(p.role, Role::Presenter | Role::Dj | Role::Source)
        })
        .map(|p| p.identity.clone())
        .collect();
    if !others.is_empty() {
        return Err(problem(
            StatusCode::CONFLICT,
            "someone else is presenting: the air passes at the post, after the grace, or by a press",
        ));
    }
    let auto = state.hub.onair.get().is_some()
        && state.hub.read_studio(|s| s.station_mode()) == Some(StationMode::Auto);
    match bring_on(
        state,
        &key.key_id,
        &person.identity,
        key.show_id,
        Cause::Go,
        &[],
        auto,
        settings.fade_ms,
        by,
    )
    .await
    {
        Ok(()) => {
            let _ = write(
                state,
                Some(key.show_id),
                &key.key_id,
                now,
                "go",
                by,
                "Go on air now".into(),
            )
            .await;
            Ok(json!({ "on_air": true }))
        }
        Err(e) => Err(problem(StatusCode::CONFLICT, format!("{e:#}"))),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/sources/{key}/not-yet",
    operation_id = "sources_not_yet",
    tag = "sources",
    summary = "Not yet: stop the late count",
    description = "Stops the late count for this airing: the schedule will not put the source on air by itself, and it goes on air by its own press (`go`, or the handover's key at the desk). For staff at the desk; an API token never.",
    params(("key" = String, Path, description = "The key's id")),
    responses(
        (status = 200, description = "The count is stopped."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn not_yet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Response {
    let actor = match state.check(&headers, Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if matches!(actor, Actor::Token(_)) {
        return problem(StatusCode::FORBIDDEN, "An API token cannot do that.");
    }
    match not_yet_now(&state, &key_id, &actor.label()).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// *Not yet*, whoever presses it: the one path.
pub(crate) async fn not_yet_now(
    state: &AppState,
    key_id: &str,
    by: &str,
) -> Result<serde_json::Value, Response> {
    let (key, _) = source_at_desk(state, key_id).await?;
    match write(
        state,
        Some(key.show_id),
        &key.key_id,
        now_ms() as i64,
        "not_yet",
        by,
        format!("by {by}: not yet"),
    )
    .await
    {
        Ok(()) => Ok(json!({ "waiting": true })),
        Err(e) => Err(db_failed(&e)),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/sources/{key}/off-air",
    operation_id = "sources_off_air",
    tag = "sources",
    summary = "Off air: let a lost source's place go",
    description = "The desk toast's *Off air* for a source whose link was lost while it was on air (design section 3.6): its place is kept for the rules' time (`place_kept_s`), and this lets it go at once. The log says `place_let_go`, by whom. The tile leaves the stage, and if the source reconnects it stands by instead of going back on air by itself. If nobody else is on air and the station is not on automation yet, automation takes the air first, so listeners are not left with nothing. Refused (409) unless the key's place is being kept now. For staff at the desk; an API token never.",
    params(("key" = String, Path, description = "The key's id")),
    responses(
        (status = 200, description = "The place is let go; `covered` says whether automation took the air with it."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn off_air(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Response {
    let actor = match state.check(&headers, Need::OnAir) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if matches!(actor, Actor::Token(_)) {
        return problem(StatusCode::FORBIDDEN, "An API token cannot do that.");
    }
    let by: String = actor.label().chars().take(60).collect();
    match let_go_now(&state, &key_id, &by).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// *Off air* on a kept place, whoever presses it: the one path. The key's latest state line
/// must be `link_lost`; the place is let go the way the rule lets it go when its time is over
/// (`place_let_go`), with `by` as who did it.
pub(crate) async fn let_go_now(
    state: &AppState,
    key_id: &str,
    by: &str,
) -> Result<serde_json::Value, Response> {
    let pool = state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "show keys are kept in the database",
        )
    })?;
    let key = match model::key_by_key_id(pool, state.station.id, key_id).await {
        Ok(Some(k)) if k.revoked_ms.is_none() => k,
        Ok(_) => return Err(problem(StatusCode::NOT_FOUND, format!("no key `{key_id}`"))),
        Err(e) => return Err(db_failed(&e)),
    };
    let log = model::key_log(pool, state.station.id, &key.key_id, 40)
        .await
        .map_err(|e| db_failed(&e))?;
    let kept = log
        .iter()
        .find(|l| rule::STATE_EVENTS.contains(&l.event.as_str()))
        .is_some_and(|l| l.event == "link_lost");
    if !kept {
        return Err(problem(
            StatusCode::CONFLICT,
            "no place is being kept for that source: its link has not been lost, or its place is let go already",
        ));
    }
    let now = now_ms() as i64;
    // Letting the place go before automation has covered it must not leave listeners with
    // nothing: if nobody presents and the station is not on automation, automation takes the air
    // first, as the rule would have done once the silence was over.
    let someone_on_air = state.hub.desk_status().is_some_and(|d| {
        d.people
            .iter()
            .any(|p| p.on_air && matches!(p.role, Role::Presenter | Role::Dj | Role::Source))
    });
    let mut covered = false;
    if !someone_on_air
        && let Some(onair) = state.hub.onair.get()
        && state.hub.read_studio(|s| s.station_mode()) != Some(StationMode::Auto)
    {
        if let Err(e) = onair.set_mode(StationMode::Auto).await {
            return Err(problem(StatusCode::CONFLICT, format!("{e:#}")));
        }
        covered = true;
        write(
            state,
            Some(key.show_id),
            &key.key_id,
            now,
            "covered",
            by,
            format!("by {by}: automation took the air as the place was let go"),
        )
        .await
        .map_err(|e| db_failed(&e))?;
    }
    write(
        state,
        Some(key.show_id),
        &key.key_id,
        now,
        "place_let_go",
        by,
        format!("by {by}: Off air"),
    )
    .await
    .map_err(|e| db_failed(&e))?;
    tracing::info!(key = %key.key_id, %by, covered, "the post: a lost place was let go by a press");
    Ok(json!({ "let_go": true, "covered": covered }))
}

/// The routes' OpenAPI paths.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(hold, unhold, go, not_yet, off_air),
    components(schemas(PostView))
)]
pub struct Api;
