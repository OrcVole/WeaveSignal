// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rota (Phase 17c, design note `handover-window.md`): every airing of the coming twelve
//! hours, the one on air included, with who is down for it, where from, and how far they have
//! got. It reads the same facts as the post's rule (the schedule, the show keys, the desk and the
//! ingest's report) and decides nothing ([`weavesignal_model::handover_window::rota`]).
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /api/v1/rota` | staff; `sources:read` | the coming twelve hours, one row an airing |
//!
//! It answers while the desk is down (everyone reads `not_connected`, or `connecting` while the
//! ingest holds their connection): the rota is for planning as much as for the desk.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use weavesignal_model::handover_window::{self as hw, RotaRow};
use weavesignal_model::post::ShowFact;
use weavesignal_model::shows;
use weavesignal_model::sources::{self as model, Airing};

use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::staff::Need;

/// How far ahead the rota reads.
pub const HORIZON_S: i64 = 12 * 3600;
/// How far back the schedule is read for the airing on air now.
const LOOK_BACK_S: i64 = 30 * 3600;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/rota", get(get_rota))
}

/// The coming hours.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RotaView {
    /// The station's clock when it was read.
    pub now_ms: i64,
    /// The end of the stretch the rota covers (twelve hours on).
    pub until_ms: i64,
    /// One row an airing, in start order: the one on air now (`now`), then the rest.
    pub airings: Vec<RotaRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/rota",
    operation_id = "rota_get",
    tag = "sources",
    summary = "The rota of the coming hours",
    description = "Every airing from now to twelve hours on, the one on air included, in start order: the show, who is down for it (the labels of its show keys that have not ended, else its presenter; empty when nobody is), where they come from (`room` or `push`, their own studio), and how far they have got (`not_connected`, `connecting`, `ready`, `on_air`). A source is ready once steady for the rules' `steady_s`; a presenter in the room once at the desk. Read from the same facts as the handover window; it changes nothing. It answers while the desk is down. For staff; an API token with `sources:read`.",
    responses(
        (status = 200, description = "The rota.", body = RotaView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_rota(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Staff) {
        return r;
    }
    let Some(pool) = &state.db else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "the schedule is kept in the database: set {}",
                crate::config::DATABASE_URL_ENV
            ),
        );
    };
    match rota_now(&state, pool).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "rota: database");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
        }
    }
}

/// The rota as it stands now.
pub async fn rota_now(state: &AppState, pool: &sqlx::PgPool) -> Result<RotaView, sqlx::Error> {
    let now = now_ms() as i64;
    let now_s = now.div_euclid(1000);
    let station = state.station.id;
    let (settings, keys, all_shows, eps) = tokio::try_join!(
        model::get_settings(pool, station),
        model::live_keys(pool, station, now),
        shows::list_shows(pool, station),
        shows::episodes_between(pool, station, now_s - LOOK_BACK_S, now_s + HORIZON_S),
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
    let people = state
        .hub
        .desk_status()
        .map(|d| d.people)
        .unwrap_or_default();
    let conns = state.hub.sources.connections();
    let mut subjects: Vec<_> = keys
        .iter()
        .filter(|k| all_shows.iter().any(|s| s.id == k.show_id))
        .map(|k| crate::post::key_subject(k, &people, &conns, Vec::new()))
        .collect();
    subjects.extend(
        all_shows
            .iter()
            .filter(|s| airings.iter().any(|a| a.show == s.id))
            .filter_map(|s| crate::post::room_subject(s, &people, |_| Vec::new())),
    );
    let facts: Vec<ShowFact> = all_shows
        .iter()
        .map(|s| ShowFact {
            id: s.id,
            slug: s.slug.clone(),
            name: s.name.clone(),
        })
        .collect();
    let down = crate::post::down_of(&all_shows, &keys);
    Ok(RotaView {
        now_ms: now,
        until_ms: (now_s + HORIZON_S) * 1000,
        airings: hw::rota(
            now_s,
            HORIZON_S,
            &airings,
            &facts,
            &down,
            &subjects,
            u64::try_from(settings.steady_s).unwrap_or(0) * 1000,
        ),
    })
}

/// The route's OpenAPI path.
#[derive(utoipa::OpenApi)]
#[openapi(paths(get_rota), components(schemas(RotaView, RotaRow)))]
pub struct Api;
