// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The open read-only routes (ADR-0195): what a station's own website needs, with no bearer.
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/now-playing` | what listeners are told is playing (in [`crate::onair_api`]) |
//! | `GET /api/v1/now-playing/events` | the same as server-sent events, a `now_playing` event per change (in [`crate::onair_api`]) |
//! | `GET /api/v1/public/station` | the station's public facts: name, the show now, the stream, whether the show is live, whether call-in is open, the video, the zone |
//! | `GET /api/v1/public/schedule?days=7` | the episodes airing from now for `days` (1 to 14), in air order: show, series, number, times, kind and hosts; an episode's title, description, guests and tags once it has aired |
//! | `GET /api/v1/public/episodes?limit=20&before_ms=` | aired episodes with published files, newest first: where to hear them, their cards, their feed |
//!
//! These and nothing else in the API carry `Access-Control-Allow-Origin: *`, and are rate
//! limited per address ([`crate::api::gate`]); unlike the rest of the API they follow the
//! station's listing setting for search engines ([`crate::web::robots`]).
//!
//! **Only what the public already sees.** Nothing here names a caller, a chat account, a
//! presenter at the desk, or a staff member; there are no ids of people, no card or media paths
//! beyond the public URLs the feed already carries, and none of the studio's own fields (what a
//! field inherits, a frozen copy's flags). People come from the same lists the feeds and cards
//! are made from, so someone erased under ADR-0122 is gone from them: their record is deleted,
//! and the frozen copy of what aired has their name taken out. An episode's own title and
//! description are the station's text, as the feed shows them.
//!
//! **An episode that has not aired is shown as its slot**: show, series, number, time, length,
//! kind and hosts, but not its title, description, guests, tags or own card. The station has
//! no step that publishes an episode before it airs, so a season drafted from the rule (with
//! placeholder words) or a guest pencilled in is not something the station has announced yet.
//! What has aired is its frozen copy, as the feed shows it.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use weavesignal_model::shows::{self, Episode, Resolved, Series, Show};

use crate::app::{AppState, problem};
use crate::hub::now_ms;
use crate::shows::ShowsState;

/// The routes that need the control plane's state.
pub fn station_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/public/station", get(station))
}

/// The routes that need the shows.
pub fn show_routes() -> Router<Arc<ShowsState>> {
    Router::new()
        .route("/api/v1/public/schedule", get(schedule))
        .route("/api/v1/public/episodes", get(episodes))
}

fn cached(mut res: Response, seconds: u32) -> Response {
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(&format!("public, max-age={seconds}")).expect("digits"),
    );
    res
}

/// The station's public facts, as the player page shows them (without the presenter).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicStation {
    /// The station's name.
    pub station: String,
    /// The show on now.
    pub show: String,
    /// The listener stream; empty when not set up.
    pub stream_url: String,
    /// Listeners hear the live show now (the desk is on air).
    pub live: bool,
    /// The player page takes calls now.
    pub call_in_open: bool,
    /// The HLS addresses while the desk has video on; `null` otherwise.
    pub video: Option<crate::public::VideoInfo>,
    /// The station's time zone (IANA), the one every time here is shown in.
    pub zone: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/public/station",
    operation_id = "public_station",
    tag = "public",
    summary = "The station's public facts",
    description = "For a station's own website: the station, the show now, the stream, whether the show is live, whether call-in is open, the video, the zone. No bearer; CORS open to any origin; limited per address. Cached for 5 s.",
    responses(
        (status = 200, description = "The facts.", body = PublicStation),
        (status = 429, response = crate::openapi::TooManyRequests),
    ),
    security(()),
)]
async fn station(State(state): State<Arc<AppState>>) -> Response {
    let axum::Json(p) = crate::public::player(State(state.clone())).await;
    cached(
        Json(PublicStation {
            station: p.station,
            show: p.show,
            stream_url: p.stream_url,
            live: p.show_on_air,
            call_in_open: p.call_in.open,
            video: p.video,
            zone: state.hub.zone.name(),
        })
        .into_response(),
        5,
    )
}

// ---- the schedule ----

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct ScheduleQuery {
    /// How many days from now, 1 to 14.
    #[serde(default = "week")]
    #[param(default = 7, minimum = 1, maximum = 14)]
    days: u32,
}

fn week() -> u32 {
    7
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
fn db(state: &ShowsState) -> Result<&sqlx::PgPool, Response> {
    state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the schedule is kept in the database",
        )
    })
}

fn server_error(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "public routes: database");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

fn public_url(state: &ShowsState) -> Option<&str> {
    let p = state.publishing.public_url.trim_end_matches('/');
    (!p.is_empty()).then_some(p)
}

/// A card's public address, if the level has drawn one and the station has a public address.
fn card(public: Option<&str>, cards: Option<&Value>, which: &str) -> Option<String> {
    let file = cards?.get(which)?.as_str()?;
    Some(format!("{}/cards/{file}", public?))
}

fn air_time(zone: &jiff::tz::TimeZone, ms: u64) -> Option<String> {
    let ts = Timestamp::from_millisecond(i64::try_from(ms).ok()?).ok()?;
    Some(
        ts.to_zoned(zone.clone())
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicShow {
    /// The show's path segment, as in its feed's and page's addresses.
    pub slug: String,
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicSeries {
    pub number: u32,
    pub title: Option<String>,
}

/// One episode as the public schedule and the episode list show it.
///
/// An episode that has not aired is a slot in the schedule: the show, the series, its number,
/// when and how long, its kind and its hosts. Its own words (title, description), its guests,
/// its tags and its own card are left out (`null` and empty) until it airs, because the
/// station has no step that publishes a drafted episode before then: a season drafted from the
/// rule, or a guest pencilled in, is not yet something the station has announced.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicEpisode {
    pub id: Uuid,
    pub show: PublicShow,
    pub series: PublicSeries,
    pub number: u32,
    /// It has aired: its words are the frozen copy of what went out (corrected, if staff
    /// corrected it).
    pub aired: bool,
    /// `null` until it airs.
    pub title: Option<String>,
    /// `null` until it airs.
    pub description: Option<String>,
    pub kind: shows::EpisodeKind,
    /// Its air time in the station's zone (RFC 3339 with the offset).
    pub air_at: Option<String>,
    pub air_at_ms: Option<u64>,
    /// Its length: the episode's, else its slot's.
    pub minutes: Option<u16>,
    pub hosts: Vec<String>,
    /// Empty until it airs.
    pub guests: Vec<String>,
    /// Empty until it airs.
    pub tags: Vec<String>,
    pub explicit: bool,
    /// The square card: the episode's once it has aired, else the show's.
    pub image: Option<String>,
}

fn episode_out(
    state: &ShowsState,
    show: &Show,
    series: &Series,
    ep: &Episode,
    r: &Resolved,
) -> PublicEpisode {
    let public = public_url(state);
    let zone = state.zone.get();
    let aired = ep.frozen.is_some();
    let e = r.episode.as_ref();
    let air_ms = e
        .and_then(|e| e.air_at)
        .or(ep.air_at)
        .and_then(|s| u64::try_from(s).ok())
        .map(|s| s * 1000);
    let minutes = e
        .and_then(|e| e.minutes)
        .or(ep.minutes)
        .or(r.slot.and_then(|s| s.minutes));
    let show_card = card(public, show.cards.as_ref(), "square");
    PublicEpisode {
        id: ep.id,
        show: PublicShow {
            slug: show.slug.clone(),
            name: r.show.clone(),
        },
        series: PublicSeries {
            number: series.number,
            title: r.series.as_ref().and_then(|s| s.title.clone()),
        },
        number: ep.number,
        aired,
        title: aired.then(|| e.map_or_else(|| ep.title.clone(), |e| e.title.clone())),
        description: aired
            .then(|| e.map_or_else(|| ep.description.clone(), |e| e.description.clone())),
        kind: e.map_or(ep.kind, |e| e.kind),
        air_at: air_ms.and_then(|ms| air_time(&zone, ms)),
        air_at_ms: air_ms,
        minutes,
        hosts: r.hosts.clone(),
        guests: if aired { r.guests.clone() } else { Vec::new() },
        tags: if aired { r.tags.clone() } else { Vec::new() },
        explicit: r.explicit,
        image: if aired {
            card(public, ep.cards.as_ref(), "square").or(show_card)
        } else {
            show_card
        },
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicSchedule {
    /// The station's time zone (IANA).
    pub zone: String,
    pub from_ms: u64,
    pub to_ms: u64,
    /// In air order.
    pub episodes: Vec<PublicEpisode>,
}

#[utoipa::path(
    get,
    path = "/api/v1/public/schedule",
    operation_id = "public_schedule",
    tag = "public",
    summary = "The coming schedule",
    description = "For a station's own website: the episodes with an air time from now for `days` days, in air order. An episode that has not aired shows as its slot (show, series, number, time, length, kind, hosts); its title, description, guests and tags appear once it has aired. No person a retention rule erased appears. No bearer; CORS open to any origin; limited per address. Cached for 30 s.",
    params(ScheduleQuery),
    responses(
        (status = 200, description = "The schedule.", body = PublicSchedule),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn schedule(
    State(state): State<Arc<ShowsState>>,
    Query(q): Query<ScheduleQuery>,
) -> Response {
    if !(1..=14).contains(&q.days) {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "ask for 1 to 14 days");
    }
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let from_ms = now_ms();
    let to_ms = from_ms + u64::from(q.days) * 86_400_000;
    let from_s = (from_ms / 1000) as i64;
    let to_s = (to_ms / 1000) as i64;
    let (people, found) = match tokio::try_join!(
        shows::people(pool, state.station.id),
        shows::episodes_between(pool, state.station.id, from_s, to_s)
    ) {
        Ok(r) => r,
        Err(e) => return server_error(e),
    };
    let zone = state.zone.get();
    let mut siblings: HashMap<Uuid, Vec<Episode>> = HashMap::new();
    let mut out = Vec::with_capacity(found.len());
    for (show, series, ep) in found {
        if let std::collections::hash_map::Entry::Vacant(v) = siblings.entry(series.id) {
            match shows::list_episodes(pool, series.id).await {
                Ok(all) => v.insert(all),
                Err(e) => return server_error(e),
            };
        }
        // What aired is its frozen copy (erasure takes names out of it); the rest resolves.
        let r = ep.frozen.clone().unwrap_or_else(|| {
            shows::resolve_episode(&show, &series, &ep, &people, &zone, &siblings[&series.id])
        });
        out.push(episode_out(&state, &show, &series, &ep, &r));
    }
    cached(
        Json(PublicSchedule {
            zone: state.zone.name(),
            from_ms,
            to_ms,
            episodes: out,
        })
        .into_response(),
        30,
    )
}

// ---- published episodes ----

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct EpisodesQuery {
    /// How many, 1 to 50.
    #[serde(default = "twenty")]
    #[param(default = 20, minimum = 1, maximum = 50)]
    limit: usize,
    /// Only episodes that aired before this time (the last one's `air_at_ms`, to page back).
    #[serde(default)]
    before_ms: Option<u64>,
}

fn twenty() -> usize {
    20
}

/// A published audio file.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicFile {
    pub url: String,
    pub mime: String,
    /// In bytes.
    pub length: u64,
    pub duration_ms: Option<u64>,
}

/// An aired episode with its published files.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublishedEpisode {
    #[serde(flatten)]
    pub episode: PublicEpisode,
    /// The podcast file (MP3 or M4A).
    pub audio: Option<PublicFile>,
    /// The Opus file, where one is published.
    pub audio_opus: Option<PublicFile>,
    /// The episode's page.
    pub link: Option<String>,
    /// The show's podcast feed.
    pub feed: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PublishedEpisodes {
    /// Newest first.
    pub episodes: Vec<PublishedEpisode>,
}

fn file(public: Option<&str>, v: Option<&Value>) -> Option<PublicFile> {
    let (public, v) = (public?, v?);
    Some(PublicFile {
        url: format!("{public}/media/episodes/{}", v.get("file")?.as_str()?),
        mime: v.get("mime")?.as_str()?.to_owned(),
        length: v.get("length")?.as_u64()?,
        duration_ms: v.get("duration_ms").and_then(Value::as_u64),
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/public/episodes",
    operation_id = "public_episodes",
    tag = "public",
    summary = "Published episodes",
    description = "For a station's own website: aired episodes with published files, newest first, with where to hear them, their card, their page and their feed. Page back with `before_ms`. No bearer; CORS open to any origin; limited per address. Cached for 30 s.",
    params(EpisodesQuery),
    responses(
        (status = 200, description = "The episodes.", body = PublishedEpisodes),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn episodes(
    State(state): State<Arc<ShowsState>>,
    Query(q): Query<EpisodesQuery>,
) -> Response {
    if !(1..=50).contains(&q.limit) {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "ask for 1 to 50 episodes");
    }
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let all = match shows::list_shows(pool, state.station.id).await {
        Ok(s) => s,
        Err(e) => return server_error(e),
    };
    let public = public_url(&state);
    let mut found: Vec<(i64, Show, Series, Episode)> = Vec::new();
    for show in all {
        match shows::published_episodes(pool, show.id).await {
            Ok(list) => {
                for (series, ep) in list {
                    let when = ep.air_at.or(ep.aired_at).unwrap_or(0);
                    if q.before_ms
                        .is_none_or(|b| (when as u64).saturating_mul(1000) < b)
                    {
                        found.push((when, show.clone(), series, ep));
                    }
                }
            }
            Err(e) => return server_error(e),
        }
    }
    found.sort_by_key(|(when, _, _, ep)| std::cmp::Reverse((*when, ep.number)));
    found.truncate(q.limit);
    let mut out = Vec::with_capacity(found.len());
    for (_, show, series, ep) in found {
        // An aired episode is its frozen copy; one without (it cannot be) is skipped.
        let Some(r) = ep.frozen.clone() else { continue };
        let files = ep.files.as_ref();
        out.push(PublishedEpisode {
            episode: episode_out(&state, &show, &series, &ep, &r),
            audio: file(public, files.and_then(|f| f.get("enclosure"))),
            audio_opus: file(public, files.and_then(|f| f.get("opus"))),
            link: public.map(|p| format!("{p}/{}/s{}/e{}", show.slug, series.number, ep.number)),
            feed: public.map(|p| format!("{p}/feeds/{}.xml", show.slug)),
        });
    }
    cached(
        Json(PublishedEpisodes { episodes: out }).into_response(),
        30,
    )
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(station, schedule, episodes))]
pub(crate) struct Api;
