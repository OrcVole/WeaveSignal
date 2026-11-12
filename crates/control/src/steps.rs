// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Beginner steps (ADR-0271, ADR-0274; the design is `docs/reference/design/beginner-steps.md`,
//! section 6).
//!
//! A show runs at one of four steps, **solo**, **guests**, **callers** or **full**, and what a
//! step leaves out is switched off while that show is on air. The station's step is the step of
//! the show on air (the rule the consent words and the slate already use: the schedule's
//! episode whose air window holds now); between shows it is the step of the show last on air,
//! which is read from the schedule, so it is the same after a restart; with no show ever on
//! air it is `full`.
//!
//! One reconciler ([`start`]) keeps the studio's view in line with the schedule and the shows
//! every few seconds, and at once when a route changes a step. Everything that asks what the
//! step is reads the studio's view in memory, never the database:
//!
//! | Step | Call-in | Seats allowed | Desk commands |
//! |---|---|---|---|
//! | solo | closed | the show's presenter | no `dump`, no handover, picture held on the slate |
//! | guests | closed | and a guest, a second presenter | the same |
//! | callers | open | and a caller on the desk's strip for callers | `dump` allowed |
//! | full | open | and the next DJ, the screener's seat | the handover, the picture's modes |
//!
//! A person already in the room is never removed by a step. Callers who are only waiting are
//! released when the step falls below `callers`; a caller on the strip stays until dropped.
//!
//! The one route here is `PUT /api/v1/shows/{show}/step`.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::put;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
pub use weavesignal_model::shows::Step;
use weavesignal_model::shows::{self, Show, ShowError, StepBy};
use weavesignal_proto::Role;

use crate::app::{AppState, problem};
use crate::desk::DeskCommand;
use crate::staff::Actor;
use crate::studio::{StepFacts, StepNext, StepPolicy, StepShow, StepView};

/// How often the schedule and the shows are looked at for the step, when nothing wakes it.
const EVERY: Duration = Duration::from_secs(3);

/// What the player's call-in says, and what the call-in request itself answers, below Callers.
pub const LINES_CLOSED: &str = "The lines are closed for this show.";

/// What a waiting caller is told when the lines close under them.
pub const LINES_HAVE_CLOSED: &str = "The lines have closed. The show on air is not taking calls, so your place in the queue has gone. You were not on air.";

const GUESTS_LATER: &str = "The studio is not taking guests during this show. Your link still works later: ask whoever sent it when to join.";
const NEXT_DJ_LATER: &str = "The studio is not ready for the next DJ during this show. Your link still works later: ask whoever sent it when to join.";
const SCREENER_LATER: &str = "The screener's seat is not open during this show. Your link still works later: ask whoever sent it when to join.";

/// Why a waiting caller was let go: a show that takes no calls took the air, or the step of the
/// show on air was moved down. The call-in page picks its words by it (design section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseCause {
    ShowTookAir,
    StepMovedDown,
}

// ---- the rules, pure ----

/// A seat a step hides from someone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hidden {
    /// A guest, or a second presenter.
    Guest,
    /// A caller.
    Caller,
    /// The next DJ.
    NextDj,
    /// The screener's seat.
    Screener,
}

impl Hidden {
    /// What the join page says when someone comes to a seat the step has hidden. Their link
    /// stays good for later.
    pub fn at_join(self) -> &'static str {
        match self {
            Hidden::Guest => GUESTS_LATER,
            Hidden::Caller => LINES_HAVE_CLOSED,
            Hidden::NextDj => NEXT_DJ_LATER,
            Hidden::Screener => SCREENER_LATER,
        }
    }

    /// What the studio is told when it asks for an invitation or a token for a hidden seat.
    pub fn when_making(self, step: Step) -> String {
        match self {
            Hidden::Guest => format!(
                "This show runs at {}, so it has no seat for a guest or a second presenter. They come with Guests.",
                step.word()
            ),
            Hidden::Caller => format!(
                "This show runs at {}, so it takes no callers. Call-in comes with Callers.",
                step.word()
            ),
            Hidden::NextDj => "An invitation for the next show's DJ comes with Full.".to_owned(),
            Hidden::Screener => format!(
                "This show runs at {}, so the screener's seat is not open. It comes with Full.",
                step.word()
            ),
        }
    }
}

/// The seat a step hides from `role` (design section 6). `own_presenter` is the show's
/// presenter on their own link, who is allowed at every step.
pub fn hidden_seat(step: Step, role: Role, own_presenter: bool) -> Option<Hidden> {
    match role {
        Role::Presenter if own_presenter || step >= Step::Guests => None,
        Role::Presenter | Role::Guest if step < Step::Guests => Some(Hidden::Guest),
        Role::Caller if step < Step::Callers => Some(Hidden::Caller),
        Role::Dj if step < Step::Full => Some(Hidden::NextDj),
        Role::Screener if step < Step::Full => Some(Hidden::Screener),
        _ => None,
    }
}

/// The room identity of a presenter's own link (the Solo DJ's "Join with my microphone…"): it
/// names the staff account, so the pass can tell the show's presenter from a second presenter
/// when it is used. A pass is signed by the control plane, so the name cannot be forged; a
/// random identity (`presenter-` and 12 hex digits) never begins this way.
pub fn own_identity(account: Uuid) -> String {
    format!("presenter-me-{}", account.simple())
}

/// Whether `identity` is the show's presenter's own link. A show that names no presenter account
/// has no one to tell it from, so any account's own link counts.
pub fn is_own_link(identity: &str, presenter_account: Option<Uuid>) -> bool {
    let Some(rest) = identity.strip_prefix("presenter-me-") else {
        return false;
    };
    match presenter_account {
        Some(a) => rest == a.simple().to_string(),
        None => true,
    }
}

/// Whether this account may make its own presenter link now: at Guests and above any member of
/// staff may (a presenter's invitation is theirs to make); at Solo only the show's presenter
/// (or anyone, when the show names none).
pub fn own_link_allowed(policy: StepPolicy, account: Uuid) -> bool {
    policy.step >= Step::Guests || policy.presenter_account.is_none_or(|p| p == account)
}

/// The seat a step refuses someone who uses a pass (or opens the join page). `in_room` is
/// whether the desk lists them now: a person already in the room is never removed by a step,
/// and a rejoin within the desk's grace is not a new arrival.
pub fn pass_refusal(
    policy: StepPolicy,
    role: Role,
    identity: &str,
    in_room: bool,
) -> Option<Hidden> {
    if in_room {
        return None;
    }
    let own = role == Role::Presenter && is_own_link(identity, policy.presenter_account);
    hidden_seat(policy.step, role, own)
}

/// Whether call-in is open at this step.
pub fn call_in_open(step: Step) -> bool {
    step >= Step::Callers
}

/// Who may change a step: the station key, an admin, a producer, or the show's presenter. An API
/// token never.
pub fn may_change(actor: &Actor, presenter_account: Option<Uuid>) -> bool {
    use weavesignal_model::staff::Role as Staff;
    match actor {
        Actor::Station => true,
        Actor::Staff(a) => {
            a.role.is_admin() || a.role == Staff::Producer || presenter_account == Some(a.id)
        }
        Actor::Token(_) => false,
    }
}

/// The refusal for a desk command the step switches off, if it does.
pub fn command_refusal(step: Step, cmd: &DeskCommand, target_is_dj: bool) -> Option<String> {
    match cmd {
        DeskCommand::Dump if step < Step::Callers => Some(format!(
            "Dump is off at {}: there is nothing for it to do. It comes on at Callers.",
            step.word()
        )),
        // The handover is the next DJ going on air (ADR-0231).
        DeskCommand::OnAir { on: true, .. } if target_is_dj && step < Step::Full => Some(format!(
            "The handover is off at {}: it comes on at Full.",
            step.word()
        )),
        DeskCommand::Picture { .. } if step < Step::Full => Some(format!(
            "The picture is held on the slate at {}: switching it comes with Full.",
            step.word()
        )),
        _ => None,
    }
}

/// The step facts for a show on air, last on air, or none, and the next show.
pub fn facts_from(show: Option<&Show>, on_air: bool, next: Option<(&Show, i64)>) -> StepFacts {
    StepFacts {
        view: StepView {
            step: show.map_or(Step::Full, |s| s.step),
            show: show.map(|s| StepShow {
                slug: s.slug.clone(),
                name: s.name.clone(),
            }),
            on_air: show.is_some() && on_air,
            next: next.map(|(s, at)| StepNext {
                slug: s.slug.clone(),
                name: s.name.clone(),
                start_ms: u64::try_from(at).unwrap_or(0) * 1000,
                step: s.step,
            }),
            may_change: false,
        },
        show_id: show.map(|s| s.id),
        presenter_account: show.and_then(|s| s.presenter_account),
    }
}

// ---- reading the schedule, and keeping the studio in line ----

async fn read(
    pool: &sqlx::PgPool,
    station: weavesignal_model::StationId,
) -> Result<StepFacts, sqlx::Error> {
    let now_s = jiff::Timestamp::now().as_second();
    let eps = shows::episodes_between(pool, station, now_s - 36 * 3600, now_s + 1).await?;
    let (show, on_air) = match shows::on_air_at(&eps, now_s) {
        Some((show, _, _)) => (Some(show.clone()), true),
        None => (
            shows::last_started(pool, station, now_s)
                .await?
                .map(|(s, _)| s),
            false,
        ),
    };
    let next = shows::next_start(pool, station, now_s).await?;
    Ok(facts_from(
        show.as_ref(),
        on_air,
        next.as_ref().map(|(s, at)| (s, *at)),
    ))
}

/// Read the step from the schedule and the shows and bring everything in line: the studio's
/// view, the callers who must be let go, the picture, the chat's counts. Without a database the
/// station stays at `full` and nothing is hidden.
pub async fn refresh(state: &AppState) {
    let Some(pool) = &state.db else { return };
    let _one_at_a_time = state.hub.live.step_gate().await;
    let facts = match read(pool, state.station.id).await {
        Ok(f) => f,
        Err(e) => {
            // The step stays as it was: a database that blinks must not open the lines.
            tracing::debug!(error = %e, "the step: the schedule is not readable");
            return;
        }
    };
    apply(state, facts).await;
    let counts = weavesignal_model::chat::moderation_counts(pool, state.station.id)
        .await
        .ok();
    state.hub.with_studio_if(|s| s.set_chat_counts(counts));
}

/// Take the step the schedule gave and bring the station in line with it. Called at every look,
/// so what a look misses the next one puts right.
pub(crate) async fn apply(state: &AppState, facts: StepFacts) {
    let show_name = facts.view.show.as_ref().map(|s| s.name.clone());
    let mv = state.hub.with_studio(|s, _| s.set_step(facts));
    if mv.changed {
        tracing::info!(
            from = %mv.from,
            to = %mv.to,
            show = ?show_name,
            "the station's step"
        );
    }
    let mover = state.hub.live.take_step_mover();
    if mv.from != mv.to
        && !mv.other_show
        && let Some(name) = &show_name
    {
        state
            .hub
            .with_studio(|s, now| s.step_moved(now, mover.as_deref(), name, mv.to));
    }
    if mv.to < Step::Callers {
        // Below Callers no one waits: on the way down every waiting caller is let go, and at
        // each look after, anyone who got past that (a call accepted a moment before the lines
        // closed, or a caller LiveKit did not remove) goes the same way, with the same cause.
        if mv.from >= Step::Callers {
            let cause = if mv.other_show {
                ReleaseCause::ShowTookAir
            } else {
                ReleaseCause::StepMovedDown
            };
            state.hub.with_studio(|s, _| s.note_release_cause(cause));
        }
        let cause = state.hub.read_studio(|s| s.release_cause());
        release_waiting(state, cause).await;
    }
    hold_picture(&state.hub, mv.to).await;
}

/// Read the step now, then keep it in line every few seconds and whenever a route changes a
/// step. The first look is made before the routes are served, so a restart does not open the
/// lines at a Solo show.
pub async fn start(state: Arc<AppState>) {
    refresh(&state).await;
    if state.db.is_none() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tokio::time::sleep(EVERY) => {}
                () = state.hub.live.step_woken() => {}
            }
            refresh(&state).await;
        }
    });
}

/// Let go every caller who is only waiting (design section 6): pass ended, out of the queue,
/// out of the room, with the cause for their page. A caller on the desk's strip, in cue or on
/// air stays until dropped.
pub(crate) async fn release_waiting(state: &AppState, cause: ReleaseCause) {
    let desk = state.hub.desk_status();
    let ids = state.hub.read_studio(|s| s.releasable(desk.as_ref()));
    for id in ids {
        state.joins.release(&id, cause);
        crate::public::end_pass(state, &id).await;
        // A caller the desk lists without a consent on file is not forgotten by that.
        state.hub.with_studio(|s, now| s.forget(&id, now));
        if let Some(room) = &state.hub.room
            && let Err(e) = room.remove(&id).await
        {
            tracing::warn!(caller = %id, error = %format!("{e:#}"), "cannot remove the released caller from the room");
        }
        tracing::info!(caller = %id, ?cause, "a waiting caller is released: the lines have closed");
    }
}

/// Below Full the picture is held on the slate, and the mode in force before returns at Full.
/// Asserted at each look, because a desk that restarts comes back in its own default mode.
pub(crate) async fn hold_picture(hub: &crate::hub::Hub, step: Step) {
    let Some(video) = hub.desk_status().and_then(|d| d.video) else {
        return;
    };
    let in_force = video.mode.get("mode").and_then(|m| m.as_str());
    if step < Step::Full {
        if in_force == Some("slate") {
            return;
        }
        // Keep what was in force only on the way down, not when a restarted desk is put back.
        if hub.read_studio(|s| s.held_picture().is_none()) {
            let before = video.mode.clone();
            hub.with_studio(|s, _| s.hold_picture(Some(before)));
        }
        if let Err(e) = hub
            .command(&DeskCommand::Picture {
                mode: "slate".into(),
                identity: None,
            })
            .await
        {
            tracing::warn!(error = %e, "the desk did not take the slate to hold the picture below Full");
        }
    } else if let Some(before) = hub.read_studio(|s| s.held_picture().cloned()) {
        let mode = before
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or("auto")
            .to_owned();
        let identity = before
            .get("identity")
            .and_then(|i| i.as_str())
            .map(str::to_owned);
        hub.with_studio(|s, _| s.hold_picture(None));
        if let Err(e) = hub.command(&DeskCommand::Picture { mode, identity }).await {
            tracing::warn!(error = %e, "the desk did not take the picture's mode back at Full");
        }
    }
}

// ---- the route ----

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/shows/{show}/step", put(put_step))
}

/// A show's new step.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StepBody {
    pub step: Step,
}

/// The change, and the station's step now.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StepChanged {
    /// The show's path segment.
    pub show: String,
    pub name: String,
    /// The show's step now.
    pub step: Step,
    /// What it was.
    pub previous: Step,
    /// The station's step now: it moves with the show only when this show is the one the step
    /// comes from.
    pub station: StepView,
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/step",
    operation_id = "shows_put_step",
    tag = "shows",
    summary = "Change a show's step",
    description = "Changes only the show's step (`solo`, `guests`, `callers` or `full`, ADR-0271), for the studio's step dialog. An admin, a producer or the show's presenter may; an API token never. The change is logged with who made it and when, every studio window is told, and if the show is the one the station's step comes from, the change takes effect at once: below `callers` call-in closes and waiting callers are released; when it rises the lines open. A person already in the room is never removed. `station` is the station's step after the change.",
    params(("show" = String, Path, description = "The show's slug")),
    request_body = StepBody,
    responses(
        (status = 200, description = "The change.", body = StepChanged),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_step(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<StepBody>,
) -> Response {
    let actor = match state.check(&headers, crate::staff::Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "shows are kept in the database: set {}",
                crate::config::DATABASE_URL_ENV
            ),
        );
    };
    let show = match shows::get_show(pool, state.station.id, &slug).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return problem(
                StatusCode::NOT_FOUND,
                format!("show `{slug}` does not exist"),
            );
        }
        Err(e) => return db_failed(&e),
    };
    if !may_change(&actor, show.presenter_account) {
        return problem(
            StatusCode::FORBIDDEN,
            "Only an admin, a producer or the show's presenter changes its step.",
        );
    }
    let by = StepBy {
        label: actor.label(),
        account: match &actor {
            Actor::Staff(a) => Some(a.id),
            _ => None,
        },
    };
    let (show, previous) =
        match shows::set_step(pool, state.station.id, &slug, body.step, &by).await {
            Ok(done) => done,
            Err(ShowError::Missing(what)) => {
                return problem(StatusCode::NOT_FOUND, format!("{what} does not exist"));
            }
            Err(ShowError::Db(e)) => return db_failed(&e),
            Err(e) => return problem(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        };
    if previous != show.step {
        tracing::info!(show = %show.slug, from = %previous, to = %show.step, by = %by.label, "a show's step changed");
        state.hub.live.step_moved(&mover_name(&actor));
        // At once: the next look, here and now, not in a few seconds.
        refresh(&state).await;
    }
    let station = state.view_for(&actor).step;
    Json(StepChanged {
        show: show.slug,
        name: show.name,
        step: show.step,
        previous,
        station,
    })
    .into_response()
}

fn db_failed(e: &sqlx::Error) -> Response {
    tracing::error!(error = %e, "steps: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

/// How the notice names whoever changed a step: an account's first name, else its username.
pub fn mover_name(actor: &Actor) -> String {
    match actor {
        Actor::Staff(a) => a
            .name
            .split_whitespace()
            .next()
            .unwrap_or(&a.username)
            .to_owned(),
        Actor::Station => "The station key".to_owned(),
        Actor::Token(t) => format!("API token {}", t.name),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(put_step))]
pub(crate) struct Api;

/// A show at `step` presented by `presenter`, for tests.
#[cfg(test)]
pub(crate) fn test_show(step: Step, presenter: Option<Uuid>) -> Show {
    Show {
        id: Uuid::from_u128(1),
        slug: "morning-kettle".into(),
        name: "The Morning Kettle".into(),
        description: String::new(),
        colour_slot: 1,
        layout: "poster".into(),
        slot: None,
        hosts: vec![],
        tags: vec![],
        explicit: false,
        feed_guid: None,
        cards: None,
        presenter: String::new(),
        contact: String::new(),
        links: vec![],
        commercial_music: false,
        presenter_account: presenter,
        step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(step: Step) -> StepPolicy {
        StepPolicy {
            step,
            presenter_account: None,
        }
    }

    #[test]
    fn the_seat_table_is_the_designs() {
        use Hidden::*;
        // (role, own presenter link) against Solo, Guests, Callers, Full.
        let row = |role, own| -> Vec<Option<Hidden>> {
            Step::ALL
                .into_iter()
                .map(|s| hidden_seat(s, role, own))
                .collect()
        };
        assert_eq!(row(Role::Presenter, true), vec![None; 4]);
        assert_eq!(
            row(Role::Presenter, false),
            vec![Some(Guest), None, None, None]
        );
        assert_eq!(row(Role::Guest, false), vec![Some(Guest), None, None, None]);
        assert_eq!(
            row(Role::Caller, false),
            vec![Some(Caller), Some(Caller), None, None]
        );
        assert_eq!(
            row(Role::Dj, false),
            vec![Some(NextDj), Some(NextDj), Some(NextDj), None]
        );
        assert_eq!(
            row(Role::Screener, false),
            vec![Some(Screener), Some(Screener), Some(Screener), None]
        );
        // A producer and a monitor have no seat a step closes.
        assert_eq!(row(Role::Producer, false), vec![None; 4]);
        assert_eq!(row(Role::Listener, false), vec![None; 4]);
    }

    #[test]
    fn the_presenters_own_link_is_the_one_a_second_presenter_lacks() {
        let me = Uuid::from_u128(7);
        let other = Uuid::from_u128(8);
        let mine = own_identity(me);
        assert!(mine.starts_with("presenter-me-") && mine.len() <= 64);
        assert!(is_own_link(&mine, Some(me)));
        assert!(!is_own_link(&mine, Some(other)), "someone else's own link");
        assert!(
            is_own_link(&mine, None),
            "a show that names no presenter has no one to tell it from"
        );
        // A random invitation's identity is never an own link.
        assert!(!is_own_link("presenter-0a1b2c3d4e5f", Some(me)));
        assert!(!is_own_link("presenter-0a1b2c3d4e5f", None));
        let at_solo = StepPolicy {
            step: Step::Solo,
            presenter_account: Some(me),
        };
        assert_eq!(pass_refusal(at_solo, Role::Presenter, &mine, false), None);
        assert_eq!(
            pass_refusal(at_solo, Role::Presenter, "presenter-0a1b2c3d4e5f", false),
            Some(Hidden::Guest),
            "a second presenter is a guest's seat"
        );
        assert_eq!(
            pass_refusal(at_solo, Role::Presenter, &own_identity(other), false),
            Some(Hidden::Guest)
        );
        // Anyone the desk lists is never sent away.
        assert_eq!(
            pass_refusal(at_solo, Role::Guest, "guest-0a1b2c3d4e5f", true),
            None
        );
        assert!(own_link_allowed(at_solo, me));
        assert!(!own_link_allowed(at_solo, other));
        assert!(own_link_allowed(policy(Step::Guests), other));
        assert!(
            own_link_allowed(policy(Step::Solo), other),
            "no presenter named"
        );
    }

    #[test]
    fn call_in_opens_at_callers() {
        assert_eq!(
            Step::ALL.map(call_in_open),
            [false, false, true, true],
            "closed at Solo and Guests"
        );
    }

    #[test]
    fn dump_the_handover_and_the_picture_follow_their_steps() {
        let dj_on = DeskCommand::OnAir {
            identity: "dj-1".into(),
            on: true,
        };
        let dj_off = DeskCommand::OnAir {
            identity: "dj-1".into(),
            on: false,
        };
        let picture = DeskCommand::Picture {
            mode: "auto".into(),
            identity: None,
        };
        for (step, dump, handover, pic) in [
            (Step::Solo, true, true, true),
            (Step::Guests, true, true, true),
            (Step::Callers, false, true, true),
            (Step::Full, false, false, false),
        ] {
            assert_eq!(
                command_refusal(step, &DeskCommand::Dump, false).is_some(),
                dump,
                "dump at {step}"
            );
            assert_eq!(
                command_refusal(step, &dj_on, true).is_some(),
                handover,
                "handover at {step}"
            );
            assert_eq!(
                command_refusal(step, &picture, false).is_some(),
                pic,
                "picture at {step}"
            );
            // Taking a DJ off air, or putting anyone else on, is not the handover.
            assert!(command_refusal(step, &dj_off, true).is_none());
            assert!(command_refusal(step, &dj_on, false).is_none());
        }
        let words = command_refusal(Step::Solo, &DeskCommand::Dump, false).unwrap();
        assert!(words.starts_with("Dump is off at Solo"), "{words}");
    }

    #[test]
    fn the_step_comes_from_the_show_and_is_full_without_one() {
        let none = facts_from(None, false, None);
        assert_eq!(none.view.step, Step::Full);
        assert!(none.view.show.is_none() && !none.view.on_air && none.show_id.is_none());
        let show = test_show(Step::Solo, Some(Uuid::from_u128(9)));
        let next = Show {
            slug: "steeped".into(),
            name: "Steeped".into(),
            step: Step::Full,
            ..show.clone()
        };
        let f = facts_from(Some(&show), false, Some((&next, 1_700_000_000)));
        assert_eq!(
            f.view.step,
            Step::Solo,
            "between shows: the show last on air"
        );
        assert!(!f.view.on_air);
        assert_eq!(f.view.show.as_ref().unwrap().name, "The Morning Kettle");
        let n = f.view.next.as_ref().unwrap();
        assert_eq!(
            (n.slug.as_str(), n.start_ms, n.step),
            ("steeped", 1_700_000_000_000, Step::Full)
        );
        assert_eq!(f.presenter_account, Some(Uuid::from_u128(9)));
        assert!(facts_from(Some(&show), true, None).view.on_air);
        assert!(!f.view.may_change, "that is for each request to say");
    }

    #[test]
    fn who_may_change_a_step() {
        use weavesignal_model::staff::{Account, Role as Staff};
        let account = |role, n| {
            Actor::Staff(Account {
                id: Uuid::from_u128(n),
                username: format!("u{n}"),
                name: format!("Person {n}"),
                role,
                disabled_ms: None,
                created_ms: 0,
                has_email: false,
            })
        };
        let presenter = Some(Uuid::from_u128(3));
        assert!(may_change(&Actor::Station, None));
        assert!(may_change(&account(Staff::Admin, 1), presenter));
        assert!(may_change(&account(Staff::Producer, 2), presenter));
        assert!(
            may_change(&account(Staff::Presenter, 3), presenter),
            "the show's own presenter"
        );
        assert!(
            !may_change(&account(Staff::Presenter, 4), presenter),
            "another presenter"
        );
        assert!(!may_change(&account(Staff::Screener, 5), presenter));
        assert!(!may_change(&account(Staff::Presenter, 3), None));
        assert_eq!(mover_name(&account(Staff::Admin, 1)), "Person");
    }

    fn picture_ops(fake: &std::sync::Mutex<crate::desk::fake::FakeDesk>) -> Vec<serde_json::Value> {
        fake.lock()
            .unwrap()
            .lines
            .iter()
            .filter(|l| l["op"] == "picture")
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn below_full_the_picture_is_held_on_the_slate_and_its_mode_returns_at_full() {
        use crate::carts::CartsLink;
        use crate::desk::DeskClient;
        use crate::hub::Hub;
        use crate::studio::Studio;
        let dir = crate::desk::tests::tempdir();
        let fake = crate::desk::fake::spawn(&dir.join("desk.sock"));
        fake.lock().unwrap().video =
            Some(serde_json::json!({"mode": "person", "identity": "eilidh"}));
        let hub = Hub::new(
            "Kettle FM",
            Studio::default(),
            DeskClient::new(Some(&dir.join("desk.sock"))),
            None,
            Duration::from_millis(250),
            CartsLink::new(crate::config::CartsConfig::default(), None),
        );
        hub.poll_once().await;
        // At Full nothing is held and nothing is sent.
        hold_picture(&hub, Step::Full).await;
        assert!(picture_ops(&fake).is_empty());
        // A Solo show takes the air: the slate holds the picture.
        hold_picture(&hub, Step::Solo).await;
        assert_eq!(
            hub.desk_status().unwrap().video.unwrap().mode["mode"],
            "slate"
        );
        assert_eq!(picture_ops(&fake).len(), 1);
        // Held: the next looks send nothing more.
        hold_picture(&hub, Step::Guests).await;
        hold_picture(&hub, Step::Callers).await;
        assert_eq!(picture_ops(&fake).len(), 1);
        // A desk that restarted comes back in auto: put on the slate again, and the mode
        // remembered is still the one from before, not auto.
        fake.lock().unwrap().video = Some(serde_json::json!({"mode": "auto"}));
        hub.poll_once().await;
        hold_picture(&hub, Step::Solo).await;
        assert_eq!(picture_ops(&fake).len(), 2);
        // Full again: the mode in force before returns.
        hold_picture(&hub, Step::Full).await;
        assert_eq!(
            fake.lock().unwrap().video,
            Some(serde_json::json!({"mode": "person", "identity": "eilidh"}))
        );
        // And once back, nothing more is sent.
        hold_picture(&hub, Step::Full).await;
        assert_eq!(picture_ops(&fake).len(), 3);
        // A station without video sends nothing, whatever the step.
        fake.lock().unwrap().video = None;
        hub.poll_once().await;
        hold_picture(&hub, Step::Solo).await;
        assert_eq!(picture_ops(&fake).len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }
}
