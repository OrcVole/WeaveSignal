// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Bookings of the rehearsal studio (1.3.0, #24, step R14): the routes that make, change and
//! delete them, the shape the schedule's week gives rehearsals in, and the check Open makes.
//!
//! The rehearsal studio is first come, with optional bookings (the operator's answer to the
//! build plan's question 6): a booking holds the studio for its show from its start to its end,
//! and Open for another show is refused while one does. Bookings never share time (409). Who
//! may book: whoever may change the show's step (its presenter, a producer, an admin, the
//! station key); reading them is staff's, and an API token's with `schedule:read`.
//!
//! The design is `docs/reference/design/rehearsal-mode-build-plan.md` in the project folder
//! (sections 1.12 and 2).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::rehearsal::Ended;
use weavesignal_model::rehearsal_bookings::{self as model, Booked, BookingRow, SessionRow};

use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::staff::{Actor, Need};

/// The bookings' routes. Kept first in the file: the tests that check every route is described
/// and placed for API tokens read the sources up to the first test-only item.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/rehearsal/bookings", get(list).post(create))
        .route(
            "/api/v1/rehearsal/bookings/{id}",
            axum::routing::patch(change).delete(delete),
        )
}

// --- the shape ------------------------------------------------------------------------------

/// What a rehearsal block is: a booking (the studio held ahead), or a session (the studio as it
/// was used, or is being).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RehearsalKind {
    Booking,
    Session,
}

/// A block in the schedule's Rehearsal column: a booking or a session, in one shape. The
/// bookings' routes answer with it too, so the studio app has one type for both.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RehearsalBlock {
    /// The booking's or the session's id.
    pub id: Uuid,
    pub kind: RehearsalKind,
    /// The show's slug.
    pub show: String,
    pub show_name: String,
    /// 1 to 12: the show's bank slot in the palette (ADR-0066).
    pub colour_slot: u8,
    /// From, in ms since the Unix epoch: a booking's start, a session's opening.
    pub start_ms: i64,
    /// To: a booking's end; a session's end, or now while it is open.
    pub end_ms: i64,
    /// Who booked it, or who opened the session, as the studio names them ("Finlay").
    pub by: String,
    /// A session still open (its `end_ms` is now). Always false for a booking.
    pub open: bool,
    /// How a session ended (`ended`, `moved` or `idle`); null while it is open, and for a
    /// booking.
    pub ended: Option<Ended>,
}

impl RehearsalBlock {
    pub fn of_booking(b: &BookingRow) -> Self {
        RehearsalBlock {
            id: b.booking.id,
            kind: RehearsalKind::Booking,
            show: b.show_slug.clone(),
            show_name: b.show_name.clone(),
            colour_slot: u8::try_from(b.colour_slot).unwrap_or(1),
            start_ms: b.booking.starts_ms,
            end_ms: b.booking.ends_ms,
            by: b.booking.booked_by.clone(),
            open: false,
            ended: None,
        }
    }

    pub fn of_session(r: &SessionRow, now_ms: i64) -> Self {
        let s = &r.session;
        RehearsalBlock {
            id: s.id,
            kind: RehearsalKind::Session,
            show: s.show_slug.clone(),
            show_name: s.show_name.clone(),
            colour_slot: u8::try_from(r.colour_slot).unwrap_or(1),
            start_ms: s.opened_at_ms,
            end_ms: s.ended_at_ms.unwrap_or(now_ms.max(s.opened_at_ms)),
            by: s.opened_by.clone(),
            open: s.ended.is_none(),
            ended: s.ended,
        }
    }
}

/// The rehearsal blocks that share any time with `[from_ms, to_ms)`: every booking, then every
/// session, each in start order. What the schedule's week gives as `rehearsals`.
pub async fn blocks_between(
    pool: &sqlx::PgPool,
    station: weavesignal_model::StationId,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<RehearsalBlock>, sqlx::Error> {
    let now = now_ms() as i64;
    let (bookings, sessions) = tokio::try_join!(
        model::bookings_between(pool, station, from_ms, to_ms),
        model::sessions_between(pool, station, from_ms, to_ms),
    )?;
    Ok(bookings
        .iter()
        .map(RehearsalBlock::of_booking)
        .chain(sessions.iter().map(|s| RehearsalBlock::of_session(s, now)))
        .collect())
}

// --- the check Open makes -------------------------------------------------------------------

/// `Err` with the 409 to send when a booking for another show holds the rehearsal studio now;
/// `Ok` when none does, when the booking is the show's own, or the station has no database (it
/// has no bookings then).
#[allow(clippy::result_large_err)] // the refusal is the response itself
pub(crate) async fn refuse_if_booked(state: &AppState, show_id: Uuid) -> Result<(), Response> {
    let Some(pool) = &state.db else {
        return Ok(());
    };
    match model::holding_now(pool, state.station.id, now_ms() as i64).await {
        Ok(Some(b)) if b.booking.show_id != show_id => Err(problem(
            StatusCode::CONFLICT,
            format!(
                "The rehearsal studio is booked for {} until {} ({}). It is free for another show after that.",
                b.show_name,
                clock(state, b.booking.ends_ms),
                b.booking.booked_by
            ),
        )),
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::error!(error = %e, "rehearsal bookings: database");
            Err(problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the database failed",
            ))
        }
    }
}

/// `14:30`, in the station's zone.
fn clock(state: &AppState, ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| {
            t.to_zoned(state.hub.zone.get())
                .strftime("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

// --- the routes -----------------------------------------------------------------------------

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, create, change, delete),
    components(schemas(
        RehearsalBlock,
        RehearsalKind,
        BookingList,
        BookingRequest,
        BookingPatch,
        BookingClash
    ))
)]
pub struct Api;

/// The stretch of time the bookings are listed for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct Range {
    /// From, in milliseconds since the Unix epoch.
    pub from_ms: i64,
    /// To (exclusive); at most 62 days after `from_ms`.
    pub to_ms: i64,
}

/// The bookings in a stretch of time.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BookingList {
    /// Whether this station has a rehearsal studio. Off, bookings made while it was on are still
    /// listed, and none can be made.
    pub enabled: bool,
    /// Every booking that shares any time with the stretch, in start order.
    pub bookings: Vec<RehearsalBlock>,
}

/// A new booking.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BookingRequest {
    /// The show's slug.
    pub show: String,
    /// From, in ms since the Unix epoch: from now on (a minute's slack).
    pub start_ms: i64,
    /// To: at least five minutes and at most a day after `start_ms`.
    pub end_ms: i64,
}

/// A change to a booking: what is left out stays.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BookingPatch {
    /// Another show's slug.
    pub show: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

/// A booking refused because it would share time with another: the sentence, and the booking in
/// the way.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BookingClash {
    pub error: String,
    /// The booking in the way. Absent from a change refused because the booking is over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash: Option<RehearsalBlock>,
}

/// What a station without a rehearsal studio says.
const OFF: &str = "The rehearsal studio is off at this station, so it cannot be booked. An admin turns it on with the installation's REHEARSAL setting.";

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn pool(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the rehearsal studio keeps its bookings in the database, and none is configured",
        )
    })
}

fn db_problem(e: &sqlx::Error) -> Response {
    tracing::error!(error = %e, "rehearsal bookings: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

/// A show by its slug: its id, name and presenter's account.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn show_by_slug(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
) -> Result<weavesignal_model::shows::Show, Response> {
    match weavesignal_model::shows::get_show(pool, state.station.id, slug.trim()).await {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(problem(
            StatusCode::NOT_FOUND,
            format!("show `{}` does not exist", slug.trim()),
        )),
        Err(e) => Err(db_problem(&e)),
    }
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn may_book(actor: &Actor, name: &str, presenter: Option<Uuid>) -> Result<(), Response> {
    if crate::steps::may_change(actor, presenter) {
        Ok(())
    } else {
        Err(problem(
            StatusCode::FORBIDDEN,
            format!("Only an admin, a producer or the presenter of {name} books its rehearsal."),
        ))
    }
}

fn times_problem(why: &str, field: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": why, "field": field })),
    )
        .into_response()
}

fn clash_response(state: &AppState, hit: &BookingRow) -> Response {
    let clash = RehearsalBlock::of_booking(hit);
    (
        StatusCode::CONFLICT,
        Json(BookingClash {
            error: format!(
                "That time is booked for {} from {} until {} ({}): bookings never share time.",
                hit.show_name,
                clock(state, hit.booking.starts_ms),
                clock(state, hit.booking.ends_ms),
                hit.booking.booked_by
            ),
            clash: Some(clash),
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/rehearsal/bookings",
    operation_id = "rehearsal_bookings",
    tag = "rehearsal",
    summary = "The rehearsal studio's bookings",
    description = "Every booking of the rehearsal studio that shares any time with the stretch asked for (at most 62 days), in start order. A booking holds the studio for its show: Open for another show is refused while one does. The rehearsal studio is first come; bookings are optional. The schedule's week also gives them, with the sessions, as `rehearsals`. For staff; an API token with `schedule:read`.",
    params(Range),
    responses(
        (status = 200, description = "The bookings.", body = BookingList),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<Range>,
) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    if q.to_ms <= q.from_ms || q.to_ms - q.from_ms > 62 * 24 * 3_600_000 {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ask for at most 62 days, `to_ms` after `from_ms`",
        );
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match model::bookings_between(pool, state.station.id, q.from_ms, q.to_ms).await {
        Ok(found) => Json(BookingList {
            enabled: state.hub.rehearsal.get().is_some(),
            bookings: found.iter().map(RehearsalBlock::of_booking).collect(),
        })
        .into_response(),
        Err(e) => db_problem(&e),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/rehearsal/bookings",
    operation_id = "rehearsal_book",
    tag = "rehearsal",
    summary = "Book the rehearsal studio",
    description = "Books the rehearsal studio for one show from `start_ms` until `end_ms`: 201 and the booking. From now on (a minute's slack), at least five minutes and at most a day (422 naming the field). Bookings never share time: one that would is 409, with the sentence and the booking in the way (`clash`); one that starts as another ends does not share time. Whoever may change the show's step may (its presenter, a producer, an admin, the station key); 403 otherwise. 503 when the station has no rehearsal studio or no database.",
    request_body = BookingRequest,
    responses(
        (status = 201, description = "The booking.", body = RehearsalBlock),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, description = "It would share time with another booking.", body = BookingClash),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BookingRequest>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if state.hub.rehearsal.get().is_none() {
        return problem(StatusCode::SERVICE_UNAVAILABLE, OFF);
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let show = match show_by_slug(&state, pool, &req.show).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = may_book(&actor, &show.name, show.presenter_account) {
        return r;
    }
    let now = now_ms() as i64;
    if let Some(why) = model::check_times(req.start_ms, req.end_ms, now) {
        let field = if req.start_ms < now - 60_000 && req.end_ms > req.start_ms {
            "start_ms"
        } else {
            "end_ms"
        };
        return times_problem(why, field);
    }
    let by = crate::steps::mover_name(&actor);
    match model::create(
        pool,
        state.station.id,
        show.id,
        req.start_ms,
        req.end_ms,
        &by,
    )
    .await
    {
        Ok(Booked::Done(b)) => {
            tracing::info!(show = %b.show_slug, by = %by, "rehearsal studio booked");
            (StatusCode::CREATED, Json(RehearsalBlock::of_booking(&b))).into_response()
        }
        Ok(Booked::Clash(hit)) => clash_response(&state, &hit),
        Ok(Booked::Missing) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the booking was not kept",
        ),
        Err(e) => db_problem(&e),
    }
}

/// The booking named in the path, and that `actor` may change it: it has not ended, and they may
/// change its show's step.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn changeable(
    state: &AppState,
    pool: &sqlx::PgPool,
    actor: &Actor,
    id: Uuid,
) -> Result<BookingRow, Response> {
    let b = match model::booking(pool, state.station.id, id).await {
        Ok(Some(b)) => b,
        Ok(None) => return Err(problem(StatusCode::NOT_FOUND, "no such booking")),
        Err(e) => return Err(db_problem(&e)),
    };
    may_book(actor, &b.show_name, b.presenter_account)?;
    if b.booking.ends_ms <= now_ms() as i64 {
        return Err(problem(
            StatusCode::CONFLICT,
            format!(
                "That booking for {} is over: it stays in the schedule as it was.",
                b.show_name
            ),
        ));
    }
    Ok(b)
}

#[utoipa::path(
    patch,
    path = "/api/v1/rehearsal/bookings/{id}",
    operation_id = "rehearsal_booking_change",
    tag = "rehearsal",
    summary = "Change a booking",
    description = "Moves or resizes a booking, or gives it to another show; what is left out stays. The same checks as a new booking, except that a booking under way keeps its start. One that is over is 409, and stays as it was. Sharing time with another booking is 409 with `clash`. Whoever may change the step of the booking's show, and of the show it goes to; 403 otherwise.",
    params(("id" = Uuid, Path, description = "The booking's id")),
    request_body = BookingPatch,
    responses(
        (status = 200, description = "The booking, changed.", body = RehearsalBlock),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, description = "It would share time with another booking (with `clash`), or it is over.", body = BookingClash),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn change(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<BookingPatch>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if state.hub.rehearsal.get().is_none() {
        return problem(StatusCode::SERVICE_UNAVAILABLE, OFF);
    }
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let old = match changeable(&state, pool, &actor, id).await {
        Ok(b) => b,
        Err(r) => return r,
    };
    let show_id = match req.show.as_deref() {
        Some(slug) if slug.trim() != old.show_slug => {
            let show = match show_by_slug(&state, pool, slug).await {
                Ok(s) => s,
                Err(r) => return r,
            };
            if let Err(r) = may_book(&actor, &show.name, show.presenter_account) {
                return r;
            }
            show.id
        }
        _ => old.booking.show_id,
    };
    let start = req.start_ms.unwrap_or(old.booking.starts_ms);
    let end = req.end_ms.unwrap_or(old.booking.ends_ms);
    let now = now_ms() as i64;
    // A booking under way keeps its start, though that is in the past.
    let floor = if start == old.booking.starts_ms {
        now.min(start)
    } else {
        now
    };
    if let Some(why) = model::check_times(start, end, floor) {
        let field = if start < floor - 60_000 && end > start {
            "start_ms"
        } else {
            "end_ms"
        };
        return times_problem(why, field);
    }
    if end <= now {
        return times_problem("A booking ends from now on.", "end_ms");
    }
    match model::change(pool, state.station.id, id, show_id, start, end).await {
        Ok(Booked::Done(b)) => Json(RehearsalBlock::of_booking(&b)).into_response(),
        Ok(Booked::Clash(hit)) => clash_response(&state, &hit),
        Ok(Booked::Missing) => problem(StatusCode::NOT_FOUND, "no such booking"),
        Err(e) => db_problem(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/rehearsal/bookings/{id}",
    operation_id = "rehearsal_booking_delete",
    tag = "rehearsal",
    summary = "Delete a booking",
    description = "Frees the rehearsal studio from a booking that is not over: 204. One that is over is 409, and stays in the schedule. Whoever may change the step of the booking's show (a producer or an admin included); 403 otherwise. An open session is not ended by this.",
    params(("id" = Uuid, Path, description = "The booking's id")),
    responses(
        (status = 204, description = "Deleted."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match pool(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let b = match changeable(&state, pool, &actor, id).await {
        Ok(b) => b,
        Err(r) => return r,
    };
    match model::delete(pool, state.station.id, id).await {
        Ok(true) => {
            tracing::info!(show = %b.show_slug, by = %actor.label(), "rehearsal booking deleted");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "no such booking"),
        Err(e) => db_problem(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weavesignal_model::rehearsal::{Booking, Session};

    #[test]
    fn a_booking_and_a_session_are_one_shape() {
        let b = BookingRow {
            booking: Booking {
                id: Uuid::from_u128(1),
                show_id: Uuid::from_u128(2),
                starts_ms: 1_000,
                ends_ms: 2_000,
                booked_by: "Finlay".into(),
            },
            show_slug: "steeped".into(),
            show_name: "Steeped".into(),
            colour_slot: 4,
            presenter_account: None,
        };
        let v = serde_json::to_value(RehearsalBlock::of_booking(&b)).unwrap();
        assert_eq!(
            v,
            json!({"id": Uuid::from_u128(1), "kind": "booking", "show": "steeped",
                "show_name": "Steeped", "colour_slot": 4, "start_ms": 1_000, "end_ms": 2_000,
                "by": "Finlay", "open": false, "ended": null})
        );
        let session = |ended_at_ms, ended| SessionRow {
            session: Session {
                id: Uuid::from_u128(3),
                show_id: Uuid::from_u128(2),
                show_slug: "steeped".into(),
                show_name: "Steeped".into(),
                presenter_account: None,
                opened_by: "Kirsty".into(),
                opened_at_ms: 5_000,
                ended_at_ms,
                ended,
                moved_at_ms: None,
                moved: Vec::new(),
                opening: None,
            },
            colour_slot: 4,
        };
        // An open session runs until now.
        let open = RehearsalBlock::of_session(&session(None, None), 9_000);
        assert_eq!(
            (open.kind, open.end_ms, open.open),
            (RehearsalKind::Session, 9_000, true)
        );
        assert_eq!(open.by, "Kirsty");
        // An ended one until its end, with how it ended.
        let gone = RehearsalBlock::of_session(&session(Some(7_000), Some(Ended::Moved)), 9_000);
        assert_eq!(
            (gone.end_ms, gone.open, gone.ended),
            (7_000, false, Some(Ended::Moved))
        );
        assert_eq!(
            serde_json::to_value(&gone).unwrap()["ended"],
            json!("moved")
        );
    }
}
