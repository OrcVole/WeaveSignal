// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A show's recurrence rule over HTTP (release 1.0.4, ADR-0278; design `new-show.md`
//! section 9, *Routes*): changing a rule with what happens to the drafted episodes, the preview
//! that writes nothing, and the dates the Schedule tab draws as dotted blocks.
//!
//! | Route | What |
//! |---|---|
//! | `PUT /api/v1/shows/{show}/rule` | the show's rule alone, with `drafted`: `move` (the default) or `leave`; moves the drafted episodes in the same transaction and replies with what moved |
//! | `PUT /api/v1/shows/{show}/series/{series}/rule` | the same for one series; `null` follows the show again |
//! | `POST /api/v1/schedule/rule/preview` | a rule's sentence, its next dates with their clashes and what saving it would do to the episodes; nothing is written |
//!
//! The studio works out no dates, writes no sentence and judges no clash: it draws what these
//! routes answer.

use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{post, put};
use axum::{Json, Router};
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use weavesignal_model::planner::{self, Clash, NextOn, Scheduled};
use weavesignal_model::rule::{ClockChange, Rule, RuleProblem, RuleWords, hhmm, long_date};
use weavesignal_model::ruleplan::{self, ChangePlan, Drafted, EpisodeAt, Moved, Scope};
use weavesignal_model::shows::{self, Episode, Level, Series, Show, ShowError};

use crate::shows::{
    Answer, Fail, SeriesOut, ShowOut, ShowsState, clash_policy, db, find_series, find_show,
    series_out, show_out, tell_windows,
};

pub fn routes() -> Router<Arc<ShowsState>> {
    Router::new()
        .route("/api/v1/shows/{show}/rule", put(put_show_rule))
        .route(
            "/api/v1/shows/{show}/series/{series}/rule",
            put(put_series_rule),
        )
        .route("/api/v1/schedule/rule/preview", post(preview))
}

/// `rule` must be sent, as a rule or as `null`: leaving it out is a mistake, not a clear.
fn required<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d)
}

/// A rule to set, and what to do with the episodes already drafted for dates to come.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleInput {
    /// The new rule; `null` clears a show's, or makes a series follow its show again.
    #[serde(deserialize_with = "required")]
    #[schema(required = true, nullable = true)]
    rule: Option<Rule>,
    /// `move` (the default): the drafted episodes still on their old rule dates move to the new
    /// rule's dates, in order, keeping their numbers, titles and guests; `leave`: they stay.
    /// Aired episodes, and episodes a person moved or made by hand, never move.
    #[serde(default)]
    drafted: Drafted,
    /// A token the saving window makes for this save (1.0.5): the `rule.changed` notice every
    /// window gets carries it back, so the window that saved knows the notice for its own. Up to
    /// 64 letters, digits, `-` and `_`.
    #[serde(default)]
    #[schema(max_length = 64, pattern = "^[A-Za-z0-9_-]{1,64}$")]
    change: Option<String>,
}

/// Whether a save's token is one the notice may carry back.
fn change_token(t: &str) -> bool {
    (1..=64).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// What a saved rule did.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RuleChangedOut {
    pub show: ShowOut,
    /// The series whose rule was set, for the series route.
    pub series: Option<SeriesOut>,
    /// The rule now set on the show (or on the series).
    pub rule: Option<Rule>,
    pub rule_words: Option<RuleWords>,
    pub next_on: NextOn,
    pub drafted: Drafted,
    /// Episodes that have aired, or are on air: not touched.
    pub aired: u32,
    /// Episodes moved to the new rule's dates.
    pub moved: Vec<Moved>,
    /// Episodes a person placed (or already past): they stay.
    pub by_hand: Vec<EpisodeAt>,
    /// Drafted episodes the new rule has no date for: they stay.
    pub no_date: Vec<EpisodeAt>,
    /// `leave`: the drafted episodes left on their old dates.
    pub left: Vec<EpisodeAt>,
    /// Moved episodes that now overlap another show's time (an admin's, a producer's or the
    /// key's warning).
    pub warnings: Vec<Clash>,
}

async fn put_rule(
    state: &Arc<ShowsState>,
    headers: &HeaderMap,
    slug: &str,
    scope: Scope,
    input: RuleInput,
) -> Answer {
    let actor = match crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        crate::staff::Need::Staff,
    ) {
        Ok(a) => a,
        Err(r) => return Ok(r),
    };
    if input.change.as_deref().is_some_and(|t| !change_token(t)) {
        return Err(Fail::Status(
            StatusCode::UNPROCESSABLE_ENTITY,
            "`change` is up to 64 letters, digits, `-` and `_`".to_owned(),
        ));
    }
    let pool = db(state)?;
    let by = shows::StepBy {
        label: actor.label(),
        account: match &actor {
            crate::staff::Actor::Staff(a) => Some(a.id),
            _ => None,
        },
    };
    let rule = input.rule.clone();
    let changed = ruleplan::change_rule(
        pool,
        state.station.id,
        slug,
        scope,
        input.rule,
        input.drafted,
        &state.zone(),
        Timestamp::now(),
        &by,
        clash_policy(&actor),
    )
    .await?;
    let show = changed.show;
    // What moved is drawn again, and so is the show: a card prints a weekly slot.
    if let Some(live) = &state.live {
        live.wake_steps();
    }
    tell_windows(state, &actor, &show, input.change.as_deref());
    let people = shows::people(pool, state.station.id).await?;
    let zone = state.zone();
    for (series_number, episode_number) in &changed.moved {
        let Ok(series) = find_series(state, &show, *series_number).await else {
            continue;
        };
        let Ok(ep) = shows::get_episode(pool, series.id, *episode_number).await else {
            continue;
        };
        let Some(ep) = ep else { continue };
        let siblings = shows::list_episodes(pool, series.id).await?;
        let r = shows::resolve_episode(&show, &series, &ep, &people, &zone, &siblings);
        if let Err(Fail::Status(_, m)) =
            crate::shows::draw_and_keep(state, Level::Episode, ep.id, &r).await
        {
            tracing::warn!(episode = %ep.id, problem = %m, "a moved episode's cards were not drawn again");
        }
    }
    let r = shows::resolve_show(&show, &people);
    if let Err(Fail::Status(_, m)) =
        crate::shows::draw_and_keep(state, Level::Show, show.id, &r).await
    {
        tracing::warn!(show = %show.slug, problem = %m, "a show's cards were not drawn again");
    }
    let series_out_ = match &changed.series {
        Some(s) => {
            let eps = shows::list_episodes(pool, s.id).await?;
            Some(series_out(state, &show, s.clone(), &eps))
        }
        None => None,
    };
    let scheduled = planner::scheduled_show(pool, &show).await?;
    let (words, next) = match &series_out_ {
        Some(s) => (s.rule_words.clone(), s.next_on.clone()),
        None => {
            let today = Timestamp::now().to_zoned(zone.clone()).date();
            (
                show.rule.as_ref().map(|r| r.words(&zone, today)),
                planner::next_on(scheduled.rule.as_ref(), &zone, Timestamp::now()),
            )
        }
    };
    let plan = changed.plan;
    Ok(Json(RuleChangedOut {
        show: show_out(state, show, Some(&scheduled)),
        series: series_out_,
        rule,
        rule_words: words,
        next_on: next,
        drafted: input.drafted,
        aired: plan.aired,
        moved: plan.moved,
        by_hand: plan.by_hand,
        no_date: plan.no_date,
        left: plan.left,
        warnings: changed.warnings,
    })
    .into_response())
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/rule",
    operation_id = "shows_put_show_rule",
    tag = "shows",
    summary = "Change when a show airs",
    description = "Sets the show's rule alone (`null` clears it) and says what to do with the episodes already drafted for dates to come (`drafted`): `move` (the default) moves those still on their old rule dates to the new rule's dates, in order, keeping their numbers, titles and guests; `leave` leaves them. Aired episodes are never touched, and neither is an episode a person moved or made by hand. Every series that follows the show's rule is moved; one with a rule of its own is not. All in one transaction; the change is logged with who made it and every window is told. A presenter whose moved episodes would sit over another show's time is refused (409, `field` `rule`, `clashes`); others go on and the reply warns. The reply says what moved, what stayed and why.",
    params(("show" = String, Path, description = "The show's slug")),
    request_body = RuleInput,
    responses(
        (status = 200, description = "The rule is set; what moved and what stayed.", body = RuleChangedOut),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_show_rule(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
    Json(input): Json<RuleInput>,
) -> Answer {
    put_rule(&state, &headers, &slug, Scope::Show, input).await
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/series/{series}/rule",
    operation_id = "shows_put_series_rule",
    tag = "shows",
    summary = "Change when a series airs",
    description = "As the show's rule route, for one series: its own rule, which wins over the show's until `null` makes it follow the show again. Only this series' drafted episodes move.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show")),
    request_body = RuleInput,
    responses(
        (status = 200, description = "The rule is set; what moved and what stayed.", body = RuleChangedOut),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_series_rule(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n)): UrlPath<(String, u32)>,
    Json(input): Json<RuleInput>,
) -> Answer {
    put_rule(&state, &headers, &slug, Scope::Series(n), input).await
}

// The preview.

/// A rule to look at: nothing is written.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    /// The rule as it stands in the form.
    rule: Rule,
    /// The show it is for, when it exists: dates an episode fills are marked, a count counts
    /// the episodes already made, the show is left out of its own clashes, and `change` says what
    /// saving would do to the episodes.
    #[serde(default)]
    show: Option<String>,
    /// A series of that show: the rule is that series' own. Left out, it is the show's rule.
    #[serde(default)]
    series: Option<u32>,
    /// How many dates that air to return: 1 to 60, ten by default.
    #[serde(default)]
    limit: Option<usize>,
    /// For `change`: what to do with the drafted episodes (`move` by default).
    #[serde(default)]
    drafted: Option<Drafted>,
}

/// One date of a rule, as the preview shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct PreviewDate {
    /// The rule date: what an episode's `occurrence` holds.
    pub on: Date,
    /// The number the episode would take when drafting reaches this date.
    pub number: Option<u32>,
    /// `airs`, `skipped` (struck through, with Put back) or `filled` (an episode has it).
    pub state: PreviewState,
    /// The episode that fills it.
    pub episode: Option<u32>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub minutes: u16,
    /// The start on the station's clock, `20:00`.
    pub start: String,
    pub end: String,
    /// The end is on the day after.
    pub end_next_day: bool,
    /// The date's own nominal start, when it was moved.
    #[schema(value_type = Option<String>, example = "2027-04-06T20:00:00")]
    pub moved_from: Option<DateTime>,
    /// The clocks change over this start.
    pub clock: Option<ClockChange>,
    /// The date that drafting would take next: the first episode of a new show.
    pub first: bool,
    /// This date is under way now.
    pub under_way: bool,
    pub clashes: Vec<Clash>,
    /// This session may not go on over the clashes (a presenter, on `first` only).
    pub refused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewState {
    Airs,
    Skipped,
    Filled,
}

/// A start the studio offers in place of the rule's own, clear of the other shows.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ClearTime {
    /// Minutes after midnight.
    pub start: u16,
    /// The length in minutes, the rule's.
    pub minutes: u16,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct PreviewReply {
    /// No problem was found: the rule can be saved.
    pub ok: bool,
    pub zone: String,
    /// The sentence, the short form and the last date; `null` while the rule has a problem.
    pub words: Option<RuleWords>,
    /// What is wrong, by field: `start`, `minutes`, `repeat.every`, `repeat.weekdays`,
    /// `repeat.nth`, `repeat.day`, `repeat.dates`, `end`, `end.count`, `skip`, `moves`,
    /// `starts_on`.
    pub problems: Vec<RuleProblem>,
    /// Things to say that are not mistakes.
    pub notes: Vec<String>,
    pub dates: Vec<PreviewDate>,
    /// There are more dates than `limit`.
    pub more: bool,
    /// How many of `dates` clash.
    pub clash_count: usize,
    /// When every date shown that airs clashes, the nearest start within three hours, half an
    /// hour at a time, at which all of them are clear (*Try 21:00 to 22:00*); `null`
    /// otherwise, or when no such start exists. A moved date keeps its own time.
    pub clear: Option<ClearTime>,
    /// What saving would do to the episodes already made; with `show` only.
    pub change: Option<ChangePlan>,
}

pub(crate) fn stub_show(rule: &Rule) -> Show {
    Show {
        id: uuid::Uuid::nil(),
        slug: "preview".into(),
        name: "Preview".into(),
        description: String::new(),
        colour_slot: 1,
        layout: "poster".into(),
        slot: rule.as_slot(),
        rule: Some(rule.clone()),
        hosts: Vec::new(),
        tags: Vec::new(),
        explicit: false,
        feed_guid: None,
        cards: None,
        presenter: String::new(),
        contact: String::new(),
        links: Vec::new(),
        commercial_music: false,
        presenter_account: None,
        step: shows::Step::Solo,
    }
}

fn clock_of(zone: &TimeZone, t: Timestamp) -> (String, String, bool) {
    let z = t.to_zoned(zone.clone());
    (
        hhmm((z.hour() as u16) * 60 + z.minute() as u16),
        String::new(),
        false,
    )
}

#[utoipa::path(
    post,
    path = "/api/v1/schedule/rule/preview",
    operation_id = "schedule_rule_preview",
    tag = "schedule",
    summary = "Preview a rule",
    description = "A rule's sentence and short form (British English), its next dates with their state (airs, skipped, or filled by an episode), times on the station's clock, the clocks changing over them, and the other shows' times they overlap; the last date when it ends; any problem by field; and, with `show`, what saving it would do to the episodes already made (`change`). The studio asks about 250 ms after the last change and works out nothing itself. Writes nothing. A skipped date stays in the list, marked, so it can be put back; it does not count towards `limit` or towards a count of episodes. `refused` is true on the one date drafting would take next, for a presenter (or screener) session whose draft would sit over another show's time.",
    request_body = PreviewRequest,
    responses(
        (status = 200, description = "The preview.", body = PreviewReply),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn preview(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    Json(req): Json<PreviewRequest>,
) -> Answer {
    let actor = match crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        &headers,
        crate::staff::Need::Staff,
    ) {
        Ok(a) => a,
        Err(r) => return Ok(r),
    };
    let zone = state.zone();
    let now = Timestamp::now();
    let today = now.to_zoned(zone.clone()).date();
    let limit = req.limit.unwrap_or(10);
    if !(1..=60).contains(&limit) {
        return Err(Fail::Status(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ask for 1 to 60 dates".into(),
        ));
    }
    let reply_zone = state.zone.name();
    let mut problems = req.rule.problems();
    if !problems.is_empty() {
        return Ok(Json(PreviewReply {
            ok: false,
            zone: reply_zone,
            words: None,
            problems,
            notes: Vec::new(),
            dates: Vec::new(),
            more: false,
            clash_count: 0,
            clear: None,
            change: None,
        })
        .into_response());
    }

    // Where the rule would sit: on a show that exists (with its current series' episodes), or
    // on its own.
    let existing: Option<(Show, Option<Series>, Vec<Episode>)> = match &req.show {
        None => None,
        Some(slug) => {
            let pool = db(&state)?;
            let show = find_show(&state, slug).await?;
            let (series, episodes) = match req.series {
                Some(n) => {
                    let s = find_series(&state, &show, n).await?;
                    let eps = shows::list_episodes(pool, s.id).await?;
                    (Some(s), eps)
                }
                None => match planner::current_series(pool, show.id).await? {
                    // A series with a rule of its own is not governed by the show's.
                    Some(s) if s.rule.is_none() => {
                        let eps = shows::list_episodes(pool, s.id).await?;
                        (Some(s), eps)
                    }
                    _ => (None, Vec::new()),
                },
            };
            Some((show, series, episodes))
        }
    };
    let (show_new, series_new, episodes) = match existing.clone() {
        None => (stub_show(&req.rule), None, Vec::new()),
        Some((mut show, mut series, episodes)) => {
            match (&mut series, req.series) {
                (Some(s), Some(_)) => s.rule = Some(req.rule.clone()),
                _ => show.rule = Some(req.rule.clone()),
            }
            (show, series, episodes)
        }
    };
    let planned = shows::series_rule_planned(&show_new, series_new.as_ref(), &episodes)
        .unwrap_or_else(|| req.rule.clone());
    let effective = shows::series_rule(&show_new, series_new.as_ref(), &episodes);
    let mut words = planned.words(&zone, today);
    if let Some(e) = &effective {
        let adjusted = e.words(&zone, today);
        words.last = adjusted.last;
        words.last_on = adjusted.last_on;
    }
    let mut notes = words.notes.clone();
    let sched = Scheduled {
        rule: effective,
        show: show_new.clone(),
        series: series_new,
        episodes: episodes.clone(),
    };

    // The dates: from the day before now, those not yet over.
    let from = now
        .checked_sub(48.hours())
        .unwrap_or(now)
        .to_zoned(zone.clone())
        .date();
    let mut rows = Vec::new();
    let mut walk = sched
        .walk(&zone, from, now)
        .filter(|w| w.occ.end() > now)
        .peekable();
    let mut counted = 0;
    while counted < limit {
        let Some(w) = walk.next() else { break };
        if !w.skipped {
            counted += 1;
        }
        rows.push(w);
    }
    let more = walk.any(|w| !w.skipped);
    drop(walk);
    // A skipped date after the last that airs says nothing.
    while rows.last().is_some_and(|w| w.skipped) {
        rows.pop();
    }

    // The clashes: each date that airs against the other shows' times.
    let policy = clash_policy(&actor);
    let windows: Vec<(i64, i64)> = rows
        .iter()
        .filter(|w| !w.skipped && w.filled.is_none())
        .map(|w| (w.occ.start.as_second(), w.occ.end().as_second()))
        .collect();
    let own = existing.as_ref().map(|(s, _, _)| s.id);
    let found = if windows.is_empty() {
        Vec::new()
    } else {
        planner::clashes_for(db(&state)?, state.station.id, &zone, own, &windows, now).await?
    };
    let mut found = found.into_iter();
    let first_number = rows
        .iter()
        .find(|w| w.number.is_some())
        .and_then(|w| w.number);
    let mut dates = Vec::new();
    let mut clash_count = 0;
    for w in &rows {
        let clashes = if !w.skipped && w.filled.is_none() {
            found.next().unwrap_or_default()
        } else {
            Vec::new()
        };
        let first = w.number.is_some() && w.number == first_number;
        let refused = first && !clashes.is_empty() && policy == planner::Clashes::Refuse;
        if !clashes.is_empty() {
            clash_count += 1;
        }
        let mut clashes = clashes;
        if refused {
            for c in &mut clashes {
                c.refused = true;
            }
        }
        let (start, _, _) = clock_of(&zone, w.occ.start);
        let end_z = w.occ.end().to_zoned(zone.clone());
        let end = hhmm((end_z.hour() as u16) * 60 + end_z.minute() as u16);
        let start_date = w.occ.start.to_zoned(zone.clone()).date();
        dates.push(PreviewDate {
            on: w.occ.on,
            number: w.number,
            state: if w.skipped {
                PreviewState::Skipped
            } else if w.filled.is_some() {
                PreviewState::Filled
            } else {
                PreviewState::Airs
            },
            episode: w.filled,
            start_ms: w.occ.start.as_millisecond(),
            end_ms: w.occ.end().as_millisecond(),
            minutes: w.occ.minutes,
            start,
            end,
            end_next_day: end_z.date() > start_date,
            moved_from: w.occ.moved_from,
            clock: w.occ.clock,
            first,
            under_way: w.occ.start <= now && now < w.occ.end(),
            clashes,
            refused,
        });
    }

    // The offered clear time (new-show.md, the clash line): only when the whole show is in
    // another's way. Later wins a tie, as the design's example does.
    let clear = if !windows.is_empty() && clash_count == windows.len() {
        let airing: Vec<_> = rows
            .iter()
            .filter(|w| !w.skipped && w.filled.is_none())
            .collect();
        let base = i64::from(req.rule.start);
        let length = req.rule.minutes.unwrap_or(60);
        let mut found = None;
        'search: for step in 1..=6_i64 {
            for delta in [step * 30, -step * 30] {
                let start = base + delta;
                if !(0..1440).contains(&start) {
                    continue;
                }
                let shifted: Vec<(i64, i64)> = airing
                    .iter()
                    .map(|w| {
                        let by = if w.occ.moved_from.is_some() {
                            0
                        } else {
                            delta * 60
                        };
                        (w.occ.start.as_second() + by, w.occ.end().as_second() + by)
                    })
                    .collect();
                let hits =
                    planner::clashes_for(db(&state)?, state.station.id, &zone, own, &shifted, now)
                        .await?;
                if hits.iter().all(Vec::is_empty) {
                    found = Some(ClearTime {
                        start: start as u16,
                        minutes: length,
                    });
                    break 'search;
                }
            }
        }
        found
    } else {
        None
    };

    // Things to say that are not mistakes, and the mistakes only a look at the dates finds.
    let new_show = episodes.is_empty();
    if dates.is_empty() && new_show {
        let r = &req.rule;
        problems.push(match (&r.repeat, r.starts_on) {
            (weavesignal_model::rule::Repeat::Once, Some(d)) => RuleProblem {
                field: "starts_on".into(),
                message: format!(
                    "That time has passed. Pick a time still to come. ({})",
                    long_date(d, true, today)
                ),
            },
            _ => match (&r.end, r.starts_on) {
                (weavesignal_model::rule::End::Until { until }, Some(first)) if *until < first => {
                    RuleProblem {
                        field: "end".into(),
                        message: format!(
                            "This makes no dates. It ends on {}, before its first date, {}.",
                            long_date(*until, false, today),
                            long_date(first, false, today)
                        ),
                    }
                }
                _ => RuleProblem {
                    field: "repeat".into(),
                    message: "This makes no dates.".into(),
                },
            },
        });
    } else if new_show
        && req.rule.starts_on.is_some_and(|d| d < today)
        && !matches!(req.rule.repeat, weavesignal_model::rule::Repeat::Once)
        && let Some(d) = dates.iter().find(|d| d.state != PreviewState::Skipped)
    {
        notes.push(format!(
            "The first date has passed. The first episode will be {}.",
            long_date(d.on, true, today)
        ));
    }

    // What saving would do to the episodes already made.
    let change = match &req.show {
        Some(slug) if problems.is_empty() => {
            let scope = req.series.map_or(Scope::Show, Scope::Series);
            Some(
                ruleplan::preview_change(
                    db(&state)?,
                    state.station.id,
                    slug,
                    scope,
                    &Some(req.rule.clone()),
                    req.drafted.unwrap_or_default(),
                    &zone,
                    now,
                )
                .await
                .map_err(|e| match e {
                    ShowError::Missing(m) => Fail::Status(StatusCode::NOT_FOUND, m),
                    other => other.into(),
                })?,
            )
        }
        _ => None,
    };

    Ok(Json(PreviewReply {
        ok: problems.is_empty(),
        zone: reply_zone,
        words: problems.is_empty().then_some(words),
        problems,
        notes,
        dates,
        more,
        clash_count,
        clear,
        change,
    })
    .into_response())
}

// The dotted blocks.

/// A date to draft, for the week's dotted blocks.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct OpenDateOut {
    /// The show's path.
    pub show: String,
    pub name: String,
    pub colour_slot: u8,
    /// The show's current series (the next one to be made is 1).
    pub series: u32,
    pub on: Date,
    /// The number the episode would take when drafting reaches this date.
    pub number: Option<u32>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub minutes: u16,
    #[schema(value_type = Option<String>, example = "2027-04-06T20:00:00")]
    pub moved_from: Option<DateTime>,
    pub clock: Option<ClockChange>,
    /// The rule in short, for a tooltip.
    pub short: String,
}

/// Every unfilled, unskipped date of every show's current series that ends after `from_ms` and
/// starts before `to_ms`, and not already over.
pub(crate) async fn open_dates(
    state: &ShowsState,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<OpenDateOut>, Fail> {
    let zone = state.zone();
    let now = Timestamp::now();
    let today = now.to_zoned(zone.clone()).date();
    let from = Timestamp::from_millisecond(from_ms.max(now.as_millisecond()))
        .map_err(|_| Fail::Status(StatusCode::UNPROCESSABLE_ENTITY, "no such time".into()))?;
    let to = Timestamp::from_millisecond(to_ms)
        .map_err(|_| Fail::Status(StatusCode::UNPROCESSABLE_ENTITY, "no such time".into()))?;
    let mut out = Vec::new();
    for s in planner::scheduled(db(state)?, state.station.id).await? {
        if s.rule.is_none() {
            continue;
        }
        let short = s.short(&zone, today);
        for d in s.open_dates(&zone, from, to, now) {
            out.push(OpenDateOut {
                show: s.show.slug.clone(),
                name: s.show.name.clone(),
                colour_slot: s.show.colour_slot,
                series: s.series.as_ref().map_or(1, |x| x.number),
                on: d.occ.on,
                number: d.number,
                start_ms: d.occ.start.as_millisecond(),
                end_ms: d.occ.end().as_millisecond(),
                minutes: d.occ.minutes,
                moved_from: d.occ.moved_from,
                clock: d.occ.clock,
                short: short.clone(),
            });
        }
    }
    out.sort_by_key(|d| (d.start_ms, d.show.clone()));
    Ok(out)
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(put_show_rule, put_series_rule, preview))]
pub(crate) struct Api;

#[cfg(test)]
mod change_tests {
    use super::change_token;

    #[test]
    fn a_save_token_is_short_and_plain() {
        assert!(change_token("3f2a9c1e-7b4d-4e0a-9c2f-1a2b3c4d5e6f"));
        assert!(change_token("a_b-C9"));
        assert!(!change_token(""));
        assert!(!change_token(&"x".repeat(65)));
        assert!(!change_token("has space"));
        assert!(!change_token("<script>"));
    }
}
