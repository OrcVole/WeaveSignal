// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Calendar files (release 1.0.5, ADR-0281): the station's schedule as a calendar a person
//! subscribes to, and a calendar file read into new shows.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/public/schedule.ics` | anyone | every show with a rule, one VEVENT each, in the station's zone |
//! | `GET /api/v1/public/shows/{show}.ics` | anyone | one show's |
//! | `POST /api/v1/schedule/import/preview` | an admin or a producer | the shows a calendar file proposes, each with its rule, its sentence, its next dates and their clashes; nothing is written |
//! | `POST /api/v1/schedule/import` | an admin or a producer | make the chosen shows in one transaction, at Solo, with no episode drafted |
//!
//! The translation itself is [`weavesignal_model::ics`]. The export carries the public words
//! only (a show's name and description), as the public schedule does: nothing drafted is
//! published before it airs. The studio works out no dates: it draws what the preview answers
//! and sends back the rules it chose.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jiff::civil::Date;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use serde_json::json;
use weavesignal_model::ics::{self, ExportCalendar, ExportShow, Mapping, Proposal};
use weavesignal_model::planner::{self, Clash, Scheduled};
use weavesignal_model::rule::{Rule, RuleWords};
use weavesignal_model::shows::{self, ImportedShow, Level, Resolved, Show};

use crate::app::problem;
use crate::rules::{PreviewDate, PreviewState, stub_show};
use crate::shows::{Answer, Fail, ShowOut, ShowsState, db, show_out};

/// How long a calendar may be kept by whoever fetched it, as the public schedule.
const CACHE_SECONDS: u32 = 30;
/// The import routes take the file wrapped in JSON: a 1 MB file and its escaping.
const IMPORT_BODY_LIMIT: usize = 3 * 1024 * 1024;

pub fn routes() -> Router<Arc<ShowsState>> {
    Router::new()
        .route("/api/v1/public/schedule.ics", get(schedule_ics))
        .route("/api/v1/public/shows/{file}", get(show_ics))
        .route(
            "/api/v1/schedule/import/preview",
            post(import_preview).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
        .route(
            "/api/v1/schedule/import",
            post(import).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
}

// ---- export ----

fn calendar(body: String, file: &str) -> Response {
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/calendar; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(&format!("public, max-age={CACHE_SECONDS}")).expect("digits"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{file}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    res
}

/// A show as the export writes it: its current series' rule as planned (the series' own start
/// and plan winning over the show's), and its public words.
fn export_show(state: &ShowsState, s: &Scheduled) -> Option<ExportShow> {
    let rule = shows::series_rule_planned(&s.show, s.series.as_ref(), &s.episodes)?;
    // A weekly slot with no start date begins at its first episode, so the calendar's first
    // date does not move from day to day.
    let anchor = rule
        .starts_on
        .is_none()
        .then(|| first_episode(state, s))
        .flatten();
    Some(ExportShow {
        id: s.show.id,
        name: s.show.name.clone(),
        description: s.show.description.clone(),
        // The show's page: ADR-0056's cards and the feed name `{public}/{slug}`, but no page
        // answers there yet (the pages are the player, join and echo test), so a calendar is
        // not given a link that would answer 404. Set it here when show pages exist.
        url: None,
        rule,
        anchor,
    })
}

fn first_episode(state: &ShowsState, s: &Scheduled) -> Option<Date> {
    let zone = state.zone();
    s.episodes
        .iter()
        .filter_map(|e| {
            e.occurrence.or_else(|| {
                e.air_at
                    .and_then(|t| Timestamp::from_second(t).ok())
                    .map(|t| t.to_zoned(zone.clone()).date())
            })
        })
        .min()
}

fn write(state: &ShowsState, name: String, shows: Vec<ExportShow>) -> String {
    ics::write(&ExportCalendar {
        name,
        zone_name: state.zone.name(),
        zone: state.zone(),
        shows,
        now: Timestamp::now(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/public/schedule.ics",
    operation_id = "public_schedule_ics",
    tag = "public",
    summary = "The schedule as a calendar file",
    description = "For a person's own calendar (subscribe to it, or download it): every show with a rule, as one VEVENT each (its RRULE or RDATE, EXDATE for skipped dates, an override with RECURRENCE-ID for each moved date), in the station's zone with its VTIMEZONE, so the calendar works out the dates and the clocks changing itself. A show's rule is its current series' (that series' own start date and planned episodes win). Only the public words: the show's name and its description (its page, as `URL`, once the station serves show pages); a drafted episode's title and guests are never in it. Each show's UID is `{show id}@weavesignal` and stays the same for the show's life. `text/calendar; charset=utf-8`. No bearer; CORS open to any origin; limited per address. Cached for 30 s. Release 1.0.5 (ADR-0281).",
    responses(
        (status = 200, description = "The calendar (iCalendar, RFC 5545).", content_type = "text/calendar", body = String),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn schedule_ics(State(state): State<Arc<ShowsState>>) -> Answer {
    let pool = db(&state)?;
    let all = planner::scheduled(pool, state.station.id).await?;
    let mut shows: Vec<ExportShow> = all.iter().filter_map(|s| export_show(&state, s)).collect();
    shows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    let body = write(&state, state.station_name(), shows);
    Ok(calendar(body, "schedule.ics"))
}

#[utoipa::path(
    get,
    path = "/api/v1/public/shows/{file}",
    operation_id = "public_show_ics",
    tag = "public",
    summary = "One show as a calendar file",
    description = "As `GET /api/v1/public/schedule.ics`, for one show: `{file}` is the show's path with `.ics` (`/api/v1/public/shows/the-late-kettle.ics`). A show without a rule gives a calendar with no events, so a subscription does not fail while a rule is cleared. No bearer; CORS open to any origin; limited per address. Cached for 30 s. Release 1.0.5 (ADR-0281).",
    params(("file" = String, Path, description = "The show's path and `.ics`: `the-late-kettle.ics`")),
    responses(
        (status = 200, description = "The calendar (iCalendar, RFC 5545).", content_type = "text/calendar", body = String),
        (status = 404, response = crate::openapi::NotFound),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn show_ics(State(state): State<Arc<ShowsState>>, UrlPath(file): UrlPath<String>) -> Answer {
    let Some(slug) = file.strip_suffix(".ics").filter(|s| shows::is_slug(s)) else {
        return Ok(problem(
            StatusCode::NOT_FOUND,
            format!("`{file}` is not a show's calendar: ask for its path with .ics"),
        ));
    };
    let pool = db(&state)?;
    let Some(show) = shows::get_show(pool, state.station.id, slug).await? else {
        return Ok(problem(
            StatusCode::NOT_FOUND,
            format!("show `{slug}` does not exist"),
        ));
    };
    let s = planner::scheduled_show(pool, &show).await?;
    let name = format!("{} on {}", show.name, state.station_name());
    let body = write(&state, name, export_show(&state, &s).into_iter().collect());
    Ok(calendar(body, &format!("{slug}.ics")))
}

// ---- import ----

/// Who may import a calendar (ADR-0281): the station key, an admin or a producer, and an API
/// token the gate let through (`shows:read` for the preview, `shows:write` to make shows, as for
/// one show). Not a presenter or a screener.
fn may_import(actor: &crate::staff::Actor) -> bool {
    use weavesignal_model::staff::Role;
    match actor {
        crate::staff::Actor::Station | crate::staff::Actor::Token(_) => true,
        crate::staff::Actor::Staff(a) => a.role.is_admin() || a.role == Role::Producer,
    }
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn importer(state: &ShowsState, headers: &HeaderMap) -> Result<crate::staff::Actor, Response> {
    let actor = crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        crate::staff::Need::Staff,
    )?;
    if !may_import(&actor) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Only an admin or a producer imports a calendar.",
        ));
    }
    Ok(actor)
}

/// A refusal about one field, for the studio to put beside it.
fn field(code: StatusCode, field: &str, error: impl Into<String>) -> Response {
    (code, Json(json!({ "error": error.into(), "field": field }))).into_response()
}

/// A calendar file to read, as JSON. The route also takes the file itself, sent as
/// `text/calendar`.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportPreviewRequest {
    /// The file's text (iCalendar, UTF-8): at most 1 MB and 200 events.
    ics: String,
    /// How many dates that air to show for each proposed show: 1 to 20, five by default.
    #[serde(default)]
    limit: Option<usize>,
}

/// One show the file proposes, with what the New show preview says of a rule.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportItem {
    #[serde(flatten)]
    pub proposal: Proposal,
    /// The path the show would take if made now with this name (`null` when refused). The
    /// import makes the path free again when it makes the show, so a renamed show takes its
    /// new name's.
    pub slug: Option<String>,
    /// The rule's sentence, short form and last date (`null` when refused).
    pub words: Option<RuleWords>,
    /// Its next dates that air (and the skipped ones among them, marked), with the other shows'
    /// times each overlaps, as the rule preview gives them.
    pub dates: Vec<PreviewDate>,
    /// There are more dates than those shown.
    pub more: bool,
    /// How many of `dates` clash with a show the station has. A show that is `already` on the
    /// station does not clash with itself: its own times are in neither `clashes` nor this.
    pub clash_count: usize,
    /// The show the station already has that this is: the same name (case and spaces folded)
    /// and the same dates and times still to come, as the station's own calendar file gives
    /// them. `null` for a show the station does not have. The studio leaves such a show
    /// unticked: made again, it would be a second show at the same times.
    pub already: Option<ImportAlready>,
    /// The other items of this file (by their place in `items`, from 0) whose dates overlap
    /// this one's in the twelve months from its first date to come.
    pub overlaps: Vec<usize>,
}

/// A show the station already has.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportAlready {
    /// Its path.
    pub slug: String,
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportPreviewReply {
    /// The station's zone: every time is on its clock.
    pub zone: String,
    /// The VEVENTs read.
    pub events: usize,
    /// About the file as a whole: cancelled events left out, changed dates that match nothing.
    pub notes: Vec<String>,
    /// In the file's order.
    pub items: Vec<ImportItem>,
    /// How many items can be made (those not refused).
    pub ready: usize,
}

/// The file from the body: the file itself (`text/calendar`), or JSON with `ics`.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn file_of(headers: &HeaderMap, body: &Bytes) -> Result<(Vec<u8>, Option<usize>), Response> {
    let is_calendar = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("text/calendar")
        });
    if is_calendar {
        return Ok((body.to_vec(), None));
    }
    match serde_json::from_slice::<ImportPreviewRequest>(body) {
        Ok(r) => Ok((r.ics.into_bytes(), r.limit)),
        Err(e) => Err(field(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ics",
            format!("Send the calendar file as text/calendar, or as JSON with `ics`: {e}."),
        )),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/schedule/import/preview",
    operation_id = "schedule_import_preview",
    tag = "schedule",
    summary = "Read a calendar file into proposed shows",
    description = "Reads an iCalendar file (at most 1 MB and 200 events; the body is the file itself as `text/calendar`, or JSON with `ics`) and proposes one show per recurring event, and one per run of single events with the same name (on chosen dates, a date at another time moved). Each item says how it was read (`mapping`): `exact`, a rule that gives exactly the calendar's dates and times; `dates`, an RRULE no show's rule can say (every other day, the fifth Friday, another zone whose clocks change on other dates, flagged `clock_differs`), offered as its dates for the next twelve months, at most 120, with `reason`; or `refused`, with `reason` (an all-day event, a zone we do not know, more than once a day, longer than a day, every date passed). Times in another zone are converted to the station's clock. Each item carries the rule in the New show's shape, its sentence (`words`), its next dates with the other shows' times they overlap (as `POST /api/v1/schedule/rule/preview` gives them), the path it would take, and the other items it overlaps. An item that is a show the station already has (the same name with case and spaces folded, and the same dates and times still to come, as when the station's own calendar file is read back) says so in `already`, and is not counted as a clash with itself. Writes nothing. A file that cannot be read is refused (422, `field` `ics`, a sentence); a file over the limits too (413). An admin or a producer (and the station key, and a token with `shows:read`); a presenter or a screener is refused (403). Release 1.0.5 (ADR-0281).",
    request_body(content = ImportPreviewRequest, description = "The file, as JSON; or the file itself with `Content-Type: text/calendar`."),
    responses(
        (status = 200, description = "The proposed shows.", body = ImportPreviewReply),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 413, description = "Larger than 1 MB, or more than 200 events.", body = crate::openapi::Problem),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn import_preview(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Answer {
    if let Err(r) = importer(&state, &headers) {
        return Ok(r);
    }
    let (bytes, limit) = match file_of(&headers, &body) {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let limit = limit.unwrap_or(5);
    if !(1..=20).contains(&limit) {
        return Ok(field(
            StatusCode::UNPROCESSABLE_ENTITY,
            "limit",
            "Ask for 1 to 20 dates.",
        ));
    }
    let zone = state.zone();
    let zone_name = state.zone.name();
    let now = Timestamp::now();
    let proposed = match ics::propose(&bytes, &zone, &zone_name, now) {
        Ok(p) => p,
        Err(e @ (ics::IcsError::TooLarge | ics::IcsError::TooManyEvents(_))) => {
            return Ok(field(StatusCode::PAYLOAD_TOO_LARGE, "ics", e.to_string()));
        }
        Err(e) => {
            return Ok(field(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ics",
                e.to_string(),
            ));
        }
    };
    let pool = db(&state)?;
    let today = now.to_zoned(zone.clone()).date();
    let mut taken: HashSet<String> = shows::list_shows(pool, state.station.id)
        .await?
        .into_iter()
        .map(|s| s.slug)
        .collect();
    // The shows the station has, each with its rule as its own calendar file writes it: a
    // proposal with one's name and one's dates is that show, not a new one that clashes with it
    // on every date (the station's own schedule.ics read back, the 1.0.5 test run).
    let standing: Vec<(ImportAlready, Rule)> = planner::scheduled(pool, state.station.id)
        .await?
        .iter()
        .filter_map(|s| {
            let rule = shows::series_rule_planned(&s.show, s.series.as_ref(), &s.episodes)?;
            Some((
                ImportAlready {
                    slug: s.show.slug.clone(),
                    name: s.show.name.clone(),
                },
                rule,
            ))
        })
        .collect();

    // Each item's dates to show, walked as a new show's would be.
    struct Rows {
        rows: Vec<planner::Walked>,
        more: bool,
    }
    let from = (now - 48.hours()).to_zoned(zone.clone()).date();
    let mut walked: Vec<Option<Rows>> = Vec::with_capacity(proposed.proposals.len());
    for p in &proposed.proposals {
        let Some(rule) = &p.rule else {
            walked.push(None);
            continue;
        };
        let sched = Scheduled {
            rule: Some(rule.clone()),
            show: stub_show(rule),
            series: None,
            episodes: Vec::new(),
        };
        let mut rows = Vec::new();
        let mut walk = sched
            .walk(&zone, from, now)
            .filter(|w| w.occ.end() > now)
            .take(2000)
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
        while rows.last().is_some_and(|w| w.skipped) {
            rows.pop();
        }
        walked.push(Some(Rows { rows, more }));
    }
    // The clashes with the station's shows: one look for every date of every item.
    let mut windows = Vec::new();
    for r in walked.iter().flatten() {
        for w in r.rows.iter().filter(|w| !w.skipped) {
            windows.push((w.occ.start.as_second(), w.occ.end().as_second()));
        }
    }
    let mut found = if windows.is_empty() {
        Vec::new()
    } else {
        planner::clashes_for(pool, state.station.id, &zone, None, &windows, now).await?
    }
    .into_iter();
    let overlaps = overlaps(&proposed.proposals, &zone, today, now);
    let mut items = Vec::with_capacity(proposed.proposals.len());
    let mut ready = 0;
    for ((p, rows), overlaps) in proposed.proposals.into_iter().zip(walked).zip(overlaps) {
        // The shows this one already is (two, where a station has twins): the first is named,
        // and none of them is a clash.
        let mut same: Vec<&ImportAlready> = standing
            .iter()
            .filter(|(s, rule)| p.is_show(&s.name, rule, &zone, now))
            .map(|(s, _)| s)
            .collect();
        same.sort_by(|a, b| a.slug.cmp(&b.slug));
        let mut dates = Vec::new();
        let mut clash_count = 0;
        let mut more = false;
        if let Some(r) = rows {
            more = r.more;
            for w in r.rows {
                let mut clashes = if w.skipped {
                    Vec::new()
                } else {
                    found.next().unwrap_or_default()
                };
                clashes.retain(|c| !same.iter().any(|s| s.slug == c.slug));
                if !clashes.is_empty() {
                    clash_count += 1;
                }
                dates.push(preview_date(&zone, w, clashes, now));
            }
        }
        let slug = p.rule.as_ref().map(|_| {
            let s = shows::free_slug(&p.name, &taken);
            taken.insert(s.clone());
            s
        });
        if p.mapping != Mapping::Refused {
            ready += 1;
        }
        items.push(ImportItem {
            words: p.rule.as_ref().map(|r| r.words(&zone, today)),
            proposal: p,
            slug,
            dates,
            more,
            clash_count,
            already: same.first().map(|s| (*s).clone()),
            overlaps,
        });
    }
    Ok(Json(ImportPreviewReply {
        zone: zone_name,
        events: proposed.events,
        notes: proposed.notes,
        items,
        ready,
    })
    .into_response())
}

/// One walked date as the rule preview shows it, for a show not yet made.
fn preview_date(
    zone: &jiff::tz::TimeZone,
    w: planner::Walked,
    clashes: Vec<Clash>,
    now: Timestamp,
) -> PreviewDate {
    let hm = |t: Timestamp| {
        let z = t.to_zoned(zone.clone());
        weavesignal_model::rule::hhmm((z.hour() as u16) * 60 + z.minute() as u16)
    };
    let end = w.occ.end();
    PreviewDate {
        on: w.occ.on,
        number: w.number,
        state: if w.skipped {
            PreviewState::Skipped
        } else {
            PreviewState::Airs
        },
        episode: None,
        start_ms: w.occ.start.as_millisecond(),
        end_ms: end.as_millisecond(),
        minutes: w.occ.minutes,
        start: hm(w.occ.start),
        end: hm(end),
        end_next_day: end.to_zoned(zone.clone()).date() > w.occ.start.to_zoned(zone.clone()).date(),
        moved_from: w.occ.moved_from,
        clock: w.occ.clock,
        first: false,
        under_way: w.occ.start <= now && now < end,
        clashes,
        refused: false,
    }
}

/// A rule's dates still to end, for twelve months from the first of them.
fn year_of_dates<'a>(
    rule: &'a Rule,
    zone: &'a jiff::tz::TimeZone,
    from: Date,
    now: Timestamp,
) -> impl Iterator<Item = weavesignal_model::rule::Occurrence> + 'a {
    let mut until: Option<Timestamp> = None;
    rule.occurrences(zone, from)
        .filter(move |o| o.end() > now)
        .take_while(move |o| {
            let end = *until.get_or_insert(o.start + jiff::SignedDuration::from_hours(24 * 366));
            o.start < end
        })
}

/// For each proposal, the others whose dates in the twelve months from its first overlap its
/// own by a minute or more.
fn overlaps(
    proposals: &[Proposal],
    zone: &jiff::tz::TimeZone,
    today: Date,
    now: Timestamp,
) -> Vec<Vec<usize>> {
    let mut spans: Vec<(i64, i64, usize)> = Vec::new();
    for (i, p) in proposals.iter().enumerate() {
        let Some(rule) = &p.rule else { continue };
        let from = rule.starts_on.unwrap_or(today).max(today);
        for o in year_of_dates(rule, zone, from, now).take(400) {
            spans.push((o.start.as_second(), o.end().as_second(), i));
        }
    }
    spans.sort();
    let mut out: Vec<HashSet<usize>> = vec![HashSet::new(); proposals.len()];
    let mut open: Vec<(i64, usize)> = Vec::new();
    for (start, end, i) in spans {
        open.retain(|(e, _)| *e - start >= 60);
        for (e, j) in &open {
            if *j != i && end.min(*e) - start >= 60 {
                out[i].insert(*j);
                out[*j].insert(i);
            }
        }
        open.push((end, i));
    }
    out.into_iter()
        .map(|s| {
            let mut v: Vec<usize> = s.into_iter().collect();
            v.sort_unstable();
            v
        })
        .collect()
}

/// A show to make, as the person chose it from the preview.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportChoice {
    /// The show's name: the preview's, or as the person renamed it (1 to 160 characters).
    name: String,
    /// The preview's `description`, or another (at most 4,000 characters).
    #[serde(default)]
    description: String,
    /// The preview's `rule`, as it came (or changed: it is checked as any rule is).
    rule: Rule,
}

/// The shows to make.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportRequest {
    /// 1 to 200.
    items: Vec<ImportChoice>,
}

/// A made show that overlaps another show's time.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportWarning {
    /// The made show's path and name.
    pub show: String,
    pub name: String,
    /// Its dates in the twelve months from its first (the first 60) that overlap another
    /// show's time, one entry per overlap.
    pub clashes: Vec<Clash>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportReply {
    /// The shows made, in the order sent, each with its path, rule, sentence and next date.
    pub made: Vec<ShowOut>,
    /// The made shows whose dates overlap another show's time, the others made in the same
    /// import included.
    pub warnings: Vec<ImportWarning>,
}

#[utoipa::path(
    post,
    path = "/api/v1/schedule/import",
    operation_id = "schedule_import",
    tag = "schedule",
    summary = "Make the shows chosen from a calendar file",
    description = "Makes the chosen shows (1 to 200: each a name, a description and a rule, as `POST /api/v1/schedule/import/preview` proposed them or as the person renamed them) in one transaction: each a new show at step Solo, with no episode drafted and no host, in the next colours of the palette; each path made from its name and made free against the station's shows and the others in the same import, as the studio makes a new show's. Each show's cards are drawn first, and a name that cannot fit refuses the whole import (422, `field` `items[n].name`); a rule that cannot stand refuses it too (422, `field` `items[n].rule`, the item named). Nothing is made unless everything is. The reply lists the shows made and, as warnings, the dates (in the twelve months from each show's first, the first 60) where each overlaps another show's time (the others made in the same import included); an overlap never refuses the import. An admin or a producer (and the station key, and a token with `shows:write`); a presenter or a screener is refused (403). Release 1.0.5 (ADR-0281).",
    request_body = ImportRequest,
    responses(
        (status = 201, description = "The shows are made.", body = ImportReply),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn import(State(state): State<Arc<ShowsState>>, headers: HeaderMap, body: Bytes) -> Answer {
    let actor = match importer(&state, &headers) {
        Ok(a) => a,
        Err(r) => return Ok(r),
    };
    let req: ImportRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return Ok(field(
                StatusCode::UNPROCESSABLE_ENTITY,
                "items",
                format!("Send the shows to make as JSON with `items`: {e}."),
            ));
        }
    };
    if req.items.is_empty() || req.items.len() > ics::MAX_EVENTS {
        return Ok(field(
            StatusCode::UNPROCESSABLE_ENTITY,
            "items",
            "Choose 1 to 200 shows to make.",
        ));
    }
    let pool = db(&state)?;
    for (i, c) in req.items.iter().enumerate() {
        let name = c.name.trim();
        if name.is_empty() || name.chars().count() > 160 {
            return Ok(field(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("items[{i}].name"),
                format!("Give show {} a name of 1 to 160 characters.", i + 1),
            ));
        }
        if c.description.chars().count() > 4000 {
            return Ok(field(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("items[{i}].description"),
                format!("“{name}”: a description has at most 4,000 characters."),
            ));
        }
        if let Some(p) = c.rule.problems().into_iter().next() {
            return Ok(field(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("items[{i}].rule"),
                format!("“{name}”: {}.", p.message),
            ));
        }
    }
    // Each show's path, as the studio would make it, before its cards are drawn: a card prints
    // the show's address.
    let existing = shows::list_shows(pool, state.station.id).await?;
    let mut taken: HashSet<String> = existing.iter().map(|s| s.slug.clone()).collect();
    let existing = existing.len();
    let chosen: Vec<ImportedShow> = req
        .items
        .iter()
        .enumerate()
        .map(|(i, c)| ImportedShow {
            slug: {
                let s = shows::free_slug(c.name.trim(), &taken);
                taken.insert(s.clone());
                s
            },
            name: c.name.trim().to_owned(),
            description: c.description.clone(),
            rule: c.rule.clone(),
            colour_slot: ((existing + i) % 12 + 1) as u8,
        })
        .collect();
    // The cards are drawn before anything is made, so a name that cannot fit refuses the
    // import; they are drawn side by side.
    let mut drawing = tokio::task::JoinSet::new();
    for (i, c) in chosen.iter().enumerate() {
        let provisional = provisional(c, &c.slug);
        // Without a public address (cards off) the names are checked all the same, against a
        // stand-in address, and nothing drawn is kept (1.2.1).
        let data = crate::shows::card_data_or_stand_in(&state, &provisional);
        drawing.spawn(async move { (i, crate::shows::draw(data).await) });
    }
    let mut renders: HashMap<usize, Vec<weavesignal_cards::Rendered>> = HashMap::new();
    while let Some(done) = drawing.join_next().await {
        let (i, drawn) =
            done.map_err(|e| Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        match drawn {
            Ok(r) => {
                renders.insert(i, r);
            }
            Err(Fail::Status(code, m)) if code == StatusCode::UNPROCESSABLE_ENTITY => {
                return Ok(field(
                    code,
                    &format!("items[{i}].name"),
                    format!("“{}”: {m}", chosen[i].name),
                ));
            }
            Err(other) => return Err(other),
        }
    }
    let made = shows::import_shows(pool, state.station.id, &chosen).await?;
    tracing::info!(count = made.len(), by = %actor.label(), "shows were made from a calendar file");
    // The cards are kept once the shows exist; a card that cannot be kept is drawn again on the
    // show's next save, and does not undo the import.
    let cards_on = state.printed_address().is_some();
    for (i, show) in made.iter().enumerate() {
        if !cards_on {
            break;
        }
        // A path taken meanwhile was made free again: its cards are drawn again for it.
        if show.slug != chosen[i].slug {
            let again = crate::shows::card_data(&state, &provisional(&chosen[i], &show.slug));
            match again {
                Ok(data) => match crate::shows::draw(data).await {
                    Ok(r) => {
                        renders.insert(i, r);
                    }
                    Err(_) => {
                        renders.remove(&i);
                    }
                },
                Err(_) => {
                    renders.remove(&i);
                }
            }
        }
        let Some(r) = renders.get(&i) else { continue };
        match crate::shows::keep(&state, r).await {
            Ok(cards) => {
                if let Err(e) = shows::set_cards(pool, Level::Show, show.id, &cards).await {
                    tracing::warn!(show = %show.slug, error = %e, "an imported show's cards were not kept");
                }
            }
            Err(Fail::Status(_, m)) => {
                tracing::warn!(show = %show.slug, problem = %m, "an imported show's cards were not kept")
            }
            Err(Fail::Clash(_)) => {}
        }
    }
    if let Some(live) = &state.live {
        live.wake_steps();
    }
    let warnings = warnings(&state, pool, &made).await?;
    let mut out = Vec::with_capacity(made.len());
    for show in made {
        let show = shows::get_show(pool, state.station.id, &show.slug)
            .await?
            .unwrap_or(show);
        out.push(show_out(&state, show, None));
    }
    Ok((
        StatusCode::CREATED,
        Json(ImportReply {
            made: out,
            warnings,
        }),
    )
        .into_response())
}

/// A show to be made, as its cards print it.
fn provisional(c: &ImportedShow, slug: &str) -> Resolved {
    Resolved {
        show: c.name.clone(),
        slug: slug.to_owned(),
        description: c.description.clone(),
        colour_slot: c.colour_slot,
        layout: "poster".into(),
        slot: c.rule.as_slot(),
        rule: Some(c.rule.clone()),
        hosts: Vec::new(),
        guests: Vec::new(),
        tags: Vec::new(),
        explicit: false,
        series: None,
        episode: None,
        inherited: Vec::new(),
        frozen: false,
    }
}

/// The made shows' dates in the twelve months from the first of them (the first 60 each) that
/// overlap another show's time: one look for them all.
async fn warnings(
    state: &ShowsState,
    pool: &sqlx::PgPool,
    made: &[Show],
) -> Result<Vec<ImportWarning>, Fail> {
    let zone = state.zone();
    let now = Timestamp::now();
    let today = now.to_zoned(zone.clone()).date();
    let mut windows = Vec::new();
    let mut owner = Vec::new();
    for (i, show) in made.iter().enumerate() {
        let Some(rule) = &show.rule else { continue };
        let from = rule.starts_on.unwrap_or(today).max(today);
        for o in year_of_dates(rule, &zone, from, now).take(60) {
            windows.push((o.start.as_second(), o.end().as_second()));
            owner.push(i);
        }
    }
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let found = planner::clashes_for(pool, state.station.id, &zone, None, &windows, now).await?;
    let mut by_show: Vec<Vec<Clash>> = vec![Vec::new(); made.len()];
    for (i, clashes) in owner.into_iter().zip(found) {
        let own = &made[i].slug;
        by_show[i].extend(clashes.into_iter().filter(|c| &c.slug != own));
    }
    Ok(made
        .iter()
        .zip(by_show)
        .filter(|(_, c)| !c.is_empty())
        .map(|(s, clashes)| ImportWarning {
            show: s.slug.clone(),
            name: s.name.clone(),
            clashes,
        })
        .collect())
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(schedule_ics, show_ics, import_preview, import))]
pub(crate) struct Api;
