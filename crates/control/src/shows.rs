// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Shows, their cards and their podcast feeds (M8), as routes of their own, merged into the
//! control plane's router in [`crate::serve`].
//!
//! The studio app's routes (admin bearer, until accounts arrive):
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/v1/shows` | every show, with its current cards |
//! | `PUT /api/v1/shows/{show}` | make or change a show; its cards are drawn first, and a name that cannot fit refuses the save (422) |
//! | `GET /api/v1/shows/{show}` | the show, resolved, and its series |
//! | `DELETE /api/v1/shows/{show}` | delete a show none of whose episodes has aired (ADR-0279), with its series, episodes, cart banks, cards and logs; an admin or a producer |
//! | `PUT /api/v1/shows/{show}/series/{n}` | make or change a series; a field left out inherits the show's |
//! | `GET /api/v1/shows/{show}/series/{n}` | the series and its episodes, resolved, with what each inherits |
//! | `POST /api/v1/shows/{show}/series/{n}/episodes` | draft the next episode, or `count` of them, filled in from the rule |
//! | `PATCH /api/v1/shows/{show}/series/{n}/episodes/{e}` | change an episode; `null` resets a field to inheriting |
//! | `POST /api/v1/shows/{show}/series/{n}/episodes/{e}/air` | on air: frozen, its cards drawn once more |
//! | `POST /api/v1/shows/{show}/series/{n}/episodes/{e}/corrections` | correct the public text of an aired episode (ADR-0177): its cards redrawn, its feed item changed under the same id, logged with who and why; `GET` lists the log |
//! | `PUT /api/v1/shows/{show}/series/{n}/episodes/{e}/files` | the published files (in `publishing.episodes_dir`): their tags and the card written into them, last |
//! | `POST /api/v1/shows/{show}/cards`, `.../series/{n}/cards`, `.../episodes/{e}/cards` | draw the cards again (ADR-0054: four renders per save) |
//! | `POST /api/v1/cards/preview?output=link` | a card from the fields in the body (`station` filled in when left out), as a PNG, nothing kept |
//! | `POST /api/v1/cards/words` | the same fields, checked: the alt text, the page the card links to, the level and the station fields |
//! | `GET /api/v1/people` | the remembered guests and hosts, by name |
//! | `POST /api/v1/people/{id}/erase` | erase a person at their request (ADR-0122): their record, their name in every list and in what aired, cards drawn again |
//! | `GET /api/v1/schedule/episodes?from_ms=&to_ms=` | every episode airing in the range with its show, series and resolved fields: the calendar's week |
//!
//! Public, no bearer: `GET /feeds/{show}.xml` (the podcast feed), `GET /cards/{file}` and
//! `GET /media/episodes/{file}` (files, with ranges). While a show is kept for people with a key
//! (1.3.3) its feed lists no episode and its files are not served; the file route serves only
//! files an episode names.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use weavesignal_cards::{
    CardData, CardError, EpisodeCard, Layout, Output, SeriesCard, ShowCard, StationCard,
};
use weavesignal_feeds::tags::{self, EpisodeTags};
use weavesignal_model::Station;
use weavesignal_model::listener_keys;
use weavesignal_model::planner::{self, Clash, Clashes, NextOn};
use weavesignal_model::rule::{Rule, RuleWords};
use weavesignal_model::ruleplan;
use weavesignal_model::shows::{
    self, CorrectionInput, Episode, EpisodePatch, Level, NewEpisodes, Resolved, Series,
    SeriesInput, Show, ShowError, ShowInput,
};

use crate::app::problem;
use crate::config::PublishingConfig;

pub struct ShowsState {
    pub station: Station,
    pub db: Option<sqlx::PgPool>,
    /// The station key, as the rest of the control plane's routes take it.
    pub admin_token: Option<String>,
    /// Staff sessions, shared with the rest of the control plane (ADR-0127).
    pub staff: Arc<crate::staff::Staff>,
    pub publishing: PublishingConfig,
    /// The station's one time zone, shared with playout's settings (review 0025, S1).
    pub zone: Arc<crate::zone::StationZone>,
    /// The station's name in force (ADR-0237), shared with the rest of the control plane;
    /// `None` (tests) leaves `station.name`.
    pub live: Option<Arc<crate::setup::Live>>,
    /// The studio's hub, for what deleting a show must tell the cart wall (ADR-0279); `None`
    /// (tests) leaves the wall alone.
    pub hub: Option<Arc<crate::hub::Hub>>,
    /// Where the slate's stills are kept, to take a deleted show's away (ADR-0200); `None`
    /// (tests) removes none.
    pub video: Option<crate::config::VideoConfig>,
}

impl ShowsState {
    /// The station's name as cards and feeds print it: the setup page's, else the settings
    /// file's.
    pub(crate) fn station_name(&self) -> String {
        self.live
            .as_ref()
            .map_or_else(|| self.station.name.clone(), |l| l.name())
    }

    pub(crate) fn zone(&self) -> TimeZone {
        self.zone.get()
    }

    /// `https://kettle.example`, without a trailing slash; `None` when cards are off.
    pub(crate) fn public(&self) -> Option<&str> {
        let p = self.publishing.public_url.trim_end_matches('/');
        (!p.is_empty()).then_some(p)
    }

    /// The address a card prints: the public address without its scheme (ADR-0056).
    pub(crate) fn printed_address(&self) -> Option<String> {
        self.public()
            .map(|p| p.split_once("://").map_or(p, |(_, rest)| rest).to_owned())
    }
}

pub fn router(state: Arc<ShowsState>) -> Router {
    let cards = ServeDir::new(&state.publishing.cards_dir);
    Router::new()
        .route("/api/v1/shows", get(list))
        .route(
            "/api/v1/shows/{show}",
            put(put_show).get(get_show).delete(delete_show),
        )
        .route("/api/v1/shows/{show}/cards", post(show_cards))
        .route(
            "/api/v1/shows/{show}/series/{series}",
            put(put_series).get(get_series),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/cards",
            post(series_cards),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes",
            post(create_episodes),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes/{episode}",
            axum::routing::patch(patch_episode),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes/{episode}/cards",
            post(episode_cards),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes/{episode}/air",
            post(air),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes/{episode}/files",
            put(put_files),
        )
        .route(
            "/api/v1/shows/{show}/series/{series}/episodes/{episode}/corrections",
            post(correct).get(list_corrections),
        )
        .route("/api/v1/cards/preview", post(preview))
        .route("/api/v1/cards/words", post(card_words))
        .route("/api/v1/schedule/episodes", get(episodes_between))
        .route("/api/v1/people", get(list_people))
        .route("/api/v1/people/{id}/erase", post(erase_person))
        .route("/feeds/{file}", get(feed))
        .route("/media/episodes/{file}", get(episode_file))
        // The open read-only routes for station websites (ADR-0195).
        .merge(crate::open::show_routes())
        // A show's rule: changing it, previewing it (release 1.0.4).
        .merge(crate::rules::routes())
        // Calendar files: the schedule as .ics, and a file read into new shows (release 1.0.5).
        .merge(crate::calendar::routes())
        .nest_service("/cards", cards)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// `None` when the request comes from a member of staff or the station key (as
/// `app::refusal`, for this state).
pub(crate) fn refusal(state: &ShowsState, headers: &HeaderMap) -> Option<Response> {
    crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        crate::staff::Need::Staff,
    )
    .err()
}

/// As [`refusal`], for personal data and erasure: an admin only.
pub(crate) fn admin_refusal(state: &ShowsState, headers: &HeaderMap) -> Option<Response> {
    crate::staff::check(
        state.admin_token.as_deref(),
        &state.staff,
        headers,
        crate::staff::Need::Admin,
    )
    .err()
}

/// A failure as a response: the field named where a person can fix it.
pub(crate) enum Fail {
    Status(StatusCode, String),
    /// A draft over another show's time, refused: 409 with the clashes (design section 9).
    Clash(Box<planner::ClashRefusal>),
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        match self {
            Fail::Status(code, msg) => problem(code, msg),
            Fail::Clash(c) => (
                StatusCode::CONFLICT,
                Json(json!({ "error": c.message, "field": "rule", "clashes": c.clashes })),
            )
                .into_response(),
        }
    }
}

impl From<ShowError> for Fail {
    fn from(e: ShowError) -> Self {
        let code = match &e {
            ShowError::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ShowError::Frozen(_)
            | ShowError::NotAired(_)
            | ShowError::Nothing(_)
            | ShowError::Kept(_) => StatusCode::CONFLICT,
            ShowError::Clash(c) => return Fail::Clash(c.clone()),
            ShowError::Missing(_) => StatusCode::NOT_FOUND,
            ShowError::Db(err) => {
                tracing::error!(error = %err, "shows: database");
                return Fail::Status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the database failed".into(),
                );
            }
        };
        Fail::Status(code, e.to_string())
    }
}

impl From<sqlx::Error> for Fail {
    fn from(e: sqlx::Error) -> Self {
        ShowError::Db(e).into()
    }
}

impl From<CardError> for Fail {
    fn from(e: CardError) -> Self {
        match e {
            CardError::DoesNotFit(_) | CardError::MissingGlyphs(_) | CardError::Invalid(_) => {
                Fail::Status(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
            }
            CardError::Render(_) => {
                tracing::error!(error = %e, "cards: render");
                Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        }
    }
}

pub(crate) fn missing(what: impl Into<String>) -> Fail {
    Fail::Status(
        StatusCode::NOT_FOUND,
        format!("{} does not exist", what.into()),
    )
}

pub(crate) fn db(state: &ShowsState) -> Result<&sqlx::PgPool, Fail> {
    state.db.as_ref().ok_or_else(|| {
        Fail::Status(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "shows are kept in the database: set {}",
                crate::config::DATABASE_URL_ENV
            ),
        )
    })
}

/// Why cards are off, in words an admin can act on: a station with no public address (a
/// loopback install) prints none. Shows are still made and saved without them (1.2.1).
pub(crate) const CARDS_OFF: &str = "cards are off: this station has no public address to print on them. Set `publishing.public_url` (on Compose and the Quadlets, `weavesignal.sh --public`, or PUBLIC_ORIGIN in the settings file for a station on a private network) and restart; shows and episodes are saved meanwhile, and their cards are drawn at their next save";

/// The address a save's cards are drawn with to check that the names fit, when the station has
/// none to print: nothing drawn with it is kept.
const FIT_CHECK_ADDRESS: &str = "station.example";

fn public(state: &ShowsState) -> Result<&str, Fail> {
    state.public().ok_or_else(|| {
        Fail::Status(
            StatusCode::SERVICE_UNAVAILABLE,
            "cards and feeds are off: this station has no public address. Set `publishing.public_url` (on Compose and the Quadlets, `weavesignal.sh --public`, or PUBLIC_ORIGIN in the settings file) and restart".into(),
        )
    })
}

pub(crate) type Answer = Result<Response, Fail>;

// Cards.

/// The card's fields from a resolved show, series or episode; 503 when cards are off.
pub(crate) fn card_data(state: &ShowsState, r: &Resolved) -> Result<CardData, Fail> {
    let address = state
        .printed_address()
        .ok_or_else(|| Fail::Status(StatusCode::SERVICE_UNAVAILABLE, CARDS_OFF.into()))?;
    Ok(card_data_at(state, r, address))
}

/// The card's fields for checking that the names fit: the station's address, or a stand-in when
/// it has none (cards off). What is drawn from it is kept only when cards are on.
pub(crate) fn card_data_or_stand_in(state: &ShowsState, r: &Resolved) -> CardData {
    let address = state
        .printed_address()
        .unwrap_or_else(|| FIT_CHECK_ADDRESS.into());
    card_data_at(state, r, address)
}

/// The card's fields, printing `address`.
fn card_data_at(state: &ShowsState, r: &Resolved, address: String) -> CardData {
    let zone = state.zone();
    let slot = r.slot.map(|s| {
        let start = jiff::civil::Time::new((s.start / 60) as i8, (s.start % 60) as i8, 0, 0)
            .unwrap_or(jiff::civil::Time::midnight());
        weavesignal_cards::Slot {
            weekday: weavesignal_cards::Weekday::from_jiff(s.weekday()),
            start,
            end: s
                .minutes
                .map(|m| start.wrapping_add(jiff::SignedDuration::from_mins(i64::from(m)))),
        }
    });
    CardData {
        layout: Layout::parse(&r.layout).unwrap_or(Layout::Poster),
        colour_slot: r.colour_slot,
        station: StationCard {
            name: state.station_name(),
            place: state.publishing.place.clone(),
            address,
        },
        show: ShowCard {
            name: r.show.clone(),
            slug: r.slug.clone(),
            description: r.description.clone(),
            slot,
        },
        series: r.series.as_ref().map(|s| SeriesCard {
            number: s.number,
            title: s.title.clone(),
            first: s.first,
            last: s.last,
            episodes: s.episodes,
        }),
        episode: r.episode.as_ref().map(|e| EpisodeCard {
            number: e.number,
            title: e.title.clone(),
            air: e
                .air_at
                .and_then(|t| Timestamp::from_second(t).ok())
                .map(|t| t.to_zoned(zone.clone())),
        }),
        hosts: r.hosts.clone(),
        guests: r.guests.clone(),
    }
}

/// The renders of one save, drawn off the async threads (the square is 3,000 pixels).
pub(crate) async fn draw(d: CardData) -> Result<Vec<weavesignal_cards::Rendered>, Fail> {
    tokio::task::spawn_blocking(move || weavesignal_cards::render_all(&d))
        .await
        .map_err(|e| Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(Fail::from)
}

/// Write the renders to the cards directory (each name is its content's hash, so a file is
/// never replaced; X keeps a fetched card for days) and say which are current.
pub(crate) async fn keep(
    state: &ShowsState,
    renders: &[weavesignal_cards::Rendered],
) -> Result<Value, Fail> {
    let dir = &state.publishing.cards_dir;
    tokio::fs::create_dir_all(dir).await.map_err(|e| {
        Fail::Status(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot make {}: {e}", dir.display()),
        )
    })?;
    let mut out = serde_json::Map::new();
    for r in renders {
        let path = dir.join(&r.file_name);
        if tokio::fs::metadata(&path).await.is_err() {
            let tmp = dir.join(format!(".{}.part", r.file_name));
            let written = async {
                tokio::fs::write(&tmp, &r.bytes).await?;
                tokio::fs::rename(&tmp, &path).await
            };
            written.await.map_err(|e| {
                Fail::Status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("cannot write {}: {e}", path.display()),
                )
            })?;
        }
        out.insert(
            r.output.as_str().to_owned(),
            Value::String(r.file_name.clone()),
        );
        out.insert("alt".into(), Value::String(r.alt.clone()));
    }
    Ok(Value::Object(out))
}

pub(crate) async fn draw_and_keep(
    state: &ShowsState,
    level: Level,
    id: uuid::Uuid,
    r: &Resolved,
) -> Result<Value, Fail> {
    let renders = draw(card_data(state, r)?).await?;
    let cards = keep(state, &renders).await?;
    shows::set_cards(db(state)?, level, id, &cards).await?;
    Ok(cards)
}

/// A save's cards: drawn and kept when the station prints an address; otherwise (a loopback
/// install, 1.2.1) drawn with a stand-in address only to check that the names fit, and nothing
/// kept, so the save goes through and its cards are drawn at a later save.
pub(crate) async fn save_cards(
    state: &ShowsState,
    level: Level,
    id: uuid::Uuid,
    r: &Resolved,
) -> Result<(), Fail> {
    if state.printed_address().is_some() {
        draw_and_keep(state, level, id, r).await?;
    } else {
        draw(card_data_or_stand_in(state, r)).await?;
    }
    Ok(())
}

#[derive(Deserialize, utoipa::ToSchema)]
struct PreviewQuery {
    #[serde(default)]
    output: Option<String>,
}

/// The card's fields from the editor: `station` may be left out, and the station's own name,
/// place and address (ADR-0056) are filled in, so the app never has to know them.
fn card_from_editor(state: &ShowsState, mut v: Value) -> Result<CardData, Fail> {
    if let Some(o) = v.as_object_mut()
        && !o.contains_key("station")
    {
        let address = state
            .printed_address()
            .ok_or_else(|| Fail::Status(StatusCode::SERVICE_UNAVAILABLE, CARDS_OFF.into()))?;
        o.insert(
            "station".into(),
            json!({ "name": state.station_name(), "place": state.publishing.place, "address": address }),
        );
    }
    serde_json::from_value(v)
        .map_err(|e| Fail::Status(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))
}

/// A card from the fields in the body, drawn and returned, nothing kept: for the editor's
/// preview and for checking a layout.
#[utoipa::path(
    post,
    path = "/api/v1/cards/preview",
    operation_id = "shows_preview",
    tag = "cards",
    summary = "Preview a card",
    description = "A card from the fields in the body (`station` filled in when left out), as a PNG; nothing is kept.",
    params(("output" = Option<String>, Query, description = "`link` (the default), `post` or `square`")),
    request_body = CardFields,
    responses(
        (status = 200, description = "The card.", body = crate::openapi::File, content_type = "image/png"),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn preview(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    Query(q): Query<PreviewQuery>,
    Json(v): Json<Value>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let d = card_from_editor(&state, v)?;
    let output = match q.output.as_deref() {
        None => Output::Link,
        Some(o) => Output::parse(o).ok_or_else(|| {
            Fail::Status(
                StatusCode::UNPROCESSABLE_ENTITY,
                "`output` is link, post or square".into(),
            )
        })?,
    };
    let r = tokio::task::spawn_blocking(move || weavesignal_cards::render(&d, output))
        .await
        .map_err(|e| Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok((
        [
            (header::CONTENT_TYPE, r.media_type.to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        r.bytes,
    )
        .into_response())
}

/// The words of a card from the fields in the body, nothing drawn: its alternative text, the
/// page it links to (ADR-0056), its level and the station fields the preview will print. The
/// same checks as the preview, so a refused name is refused here too, with the field named.
#[utoipa::path(
    post,
    path = "/api/v1/cards/words",
    operation_id = "shows_card_words",
    tag = "cards",
    summary = "The words of a card",
    description = "The same fields, checked as the preview checks them (a refused name is refused here too): the alt text, the page the card links to, the level and the station fields. Nothing is drawn.",
    request_body = CardFields,
    responses(
        (status = 200, description = "The card's words.", body = CardWords),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn card_words(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let d = card_from_editor(&state, v)?;
    d.check()?;
    // The link card is laid out (and drawn) so a name that does not fit is refused here as the
    // save would refuse it, with the field named.
    let fitted = d.clone();
    tokio::task::spawn_blocking(move || weavesignal_cards::render(&fitted, Output::Link))
        .await
        .map_err(|e| Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    let level = match d.level() {
        weavesignal_cards::Level::Show => "show",
        weavesignal_cards::Level::Series => "series",
        weavesignal_cards::Level::Episode => "episode",
    };
    Ok(Json(json!({
        "alt": weavesignal_cards::alt_text(&d),
        "path": d.path(),
        "level": level,
        "station": d.station,
    }))
    .into_response())
}

// The schedule calendar's week.

#[derive(Deserialize, utoipa::ToSchema)]
struct RangeQuery {
    from_ms: i64,
    to_ms: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct WeekEpisode {
    show: Show,
    series: Series,
    episode: Episode,
    resolved: Resolved,
    /// The studio column the episode is in (1.3.0): `studio`, for every show on air, wherever
    /// its host is. Shows on air share one programme, so they are never in two columns.
    #[schema(example = "studio")]
    studio: &'static str,
    /// Where its host comes from (1.3.0): `room`, at the desk, or `own`, their own studio with a
    /// show key that has not ended (ADR-0280). Worked out as the rota works it out, from the
    /// show keys standing now.
    from: HostFrom,
}

/// Where a show's host comes from, for the Studios view's *Own studio* chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HostFrom {
    /// The room: a presenter or DJ at the desk.
    Room,
    /// Their own studio, sending with a show key.
    Own,
}

impl From<weavesignal_model::handover_window::Origin> for HostFrom {
    fn from(o: weavesignal_model::handover_window::Origin) -> Self {
        match o {
            weavesignal_model::handover_window::Origin::Room => HostFrom::Room,
            weavesignal_model::handover_window::Origin::Push => HostFrom::Own,
        }
    }
}

/// A column of the Studios view (1.3.0): the live studio, and the rehearsal studio when the
/// station has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub(crate) struct StudioColumn {
    /// `studio` or `rehearsal`: what an episode's `studio` names.
    pub id: &'static str,
    /// The column's heading.
    pub name: &'static str,
}

/// The Studios view's columns: Studio, and Rehearsal when the station has a rehearsal studio.
pub(crate) fn studio_columns(rehearsal: bool) -> Vec<StudioColumn> {
    let mut out = vec![StudioColumn {
        id: "studio",
        name: "Studio",
    }];
    if rehearsal {
        out.push(StudioColumn {
            id: "rehearsal",
            name: "Rehearsal",
        });
    }
    out
}

/// Every episode airing in the range, with its show, series and resolved fields, and the zone
/// the station's air times are read in: one call fills the calendar's week.
#[utoipa::path(
    get,
    path = "/api/v1/schedule/episodes",
    operation_id = "shows_episodes_between",
    tag = "schedule",
    summary = "Episodes in a range",
    description = "Every episode airing in the range with its show, series and resolved fields, and `dates`: every date of every show's rule in the range that no episode fills, that the rule does not skip and that is not over (a show's dates are its newest series'), so the calendar's week draws its dotted blocks without expanding a rule. From release 1.0.5 also `public_url`, the station's public address (`null` when none is set), from which the studio makes the calendar links it gives out (ADR-0281). From release 1.3.0 also the Studios view's columns (`studios`: Studio, and Rehearsal when the station has a rehearsal studio), each episode's `studio` and `from` (`room` or `own`, as the rota says), and `rehearsals`: the rehearsal studio's bookings and its sessions in the range (empty without a rehearsal studio). The public calendar file never lists rehearsals. From release 1.3.1 each of `dates` also has `from` (`room` or `own`), as an episode's does.",
    params(("from_ms" = i64, Query, description = "From, in milliseconds since the Unix epoch"), ("to_ms" = i64, Query, description = "To (exclusive)")),
    responses(
        (status = 200, description = "The episodes.", body = WeekBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn episodes_between(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    Query(q): Query<RangeQuery>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    if q.to_ms <= q.from_ms || q.to_ms - q.from_ms > 62 * 24 * 3_600_000 {
        return Err(Fail::Status(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ask for at most 62 days, `to_ms` after `from_ms`".into(),
        ));
    }
    let pool = db(&state)?;
    let people = shows::people(pool, state.station.id).await?;
    let zone = state.zone();
    let found = shows::episodes_between(
        pool,
        state.station.id,
        q.from_ms.div_euclid(1000),
        q.to_ms.div_euclid(1000) + 1,
    )
    .await?;
    // Who is down for each show, as the rota works it out (`post::down_of` over the show keys
    // standing now), so the Studios view's chip and the rota never disagree.
    let keys = weavesignal_model::sources::live_keys(
        pool,
        state.station.id,
        Timestamp::now().as_millisecond(),
    )
    .await?;
    let mut week_shows: Vec<Show> = Vec::new();
    for (show, _, _) in &found {
        if !week_shows.iter().any(|s| s.id == show.id) {
            week_shows.push(show.clone());
        }
    }
    let down = crate::post::down_of(&week_shows, &keys);
    let from_of = |show: uuid::Uuid| {
        down.iter()
            .find(|d| d.show == show)
            .map_or(HostFrom::Room, |d| d.from.into())
    };
    let mut out = Vec::with_capacity(found.len());
    let mut siblings: std::collections::HashMap<uuid::Uuid, Vec<Episode>> = Default::default();
    for (show, series, episode) in found {
        if let std::collections::hash_map::Entry::Vacant(e) = siblings.entry(series.id) {
            e.insert(shows::list_episodes(pool, series.id).await?);
        }
        let resolved = shows::resolve_episode(
            &show,
            &series,
            &episode,
            &people,
            &zone,
            &siblings[&series.id],
        );
        out.push(WeekEpisode {
            from: from_of(show.id),
            studio: "studio",
            show,
            series,
            episode,
            resolved,
        });
    }
    let dates = crate::rules::open_dates(&state, q.from_ms, q.to_ms).await?;
    let rehearsal = state
        .hub
        .as_ref()
        .is_some_and(|h| h.rehearsal.get().is_some());
    let rehearsals = if rehearsal {
        crate::rehearsal_bookings::blocks_between(pool, state.station.id, q.from_ms, q.to_ms)
            .await?
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "zone": state.zone.name(),
        "episodes": out,
        "dates": dates,
        "public_url": state.public(),
        "studios": studio_columns(rehearsal),
        "rehearsals": rehearsals,
    }))
    .into_response())
}

// Shows.

/// A show, with its rule said in words and its next date (release 1.0.4). The rule itself is
/// `rule`; `slot` is the weekly slot while the rule is exactly that, else `null`.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ShowOut {
    #[serde(flatten)]
    pub show: Show,
    /// The show's own rule in words; `null` without one.
    pub rule_words: Option<RuleWords>,
    /// The next date of the show's current series.
    pub next_on: NextOn,
}

/// A series, with the words and next date of the rule that governs it (its own, else its
/// show's).
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SeriesOut {
    #[serde(flatten)]
    pub series: Series,
    pub rule_words: Option<RuleWords>,
    pub next_on: NextOn,
}

pub(crate) fn show_out(
    state: &ShowsState,
    show: Show,
    scheduled: Option<&planner::Scheduled>,
) -> ShowOut {
    let zone = state.zone();
    let now = Timestamp::now();
    let today = now.to_zoned(zone.clone()).date();
    let effective = match scheduled {
        Some(s) => s.rule.clone(),
        None => show.rule.clone(),
    };
    ShowOut {
        rule_words: show.rule.as_ref().map(|r| r.words(&zone, today)),
        next_on: planner::next_on(effective.as_ref(), &zone, now),
        show,
    }
}

pub(crate) fn series_out(
    state: &ShowsState,
    show: &Show,
    series: Series,
    episodes: &[Episode],
) -> SeriesOut {
    let zone = state.zone();
    let now = Timestamp::now();
    let today = now.to_zoned(zone.clone()).date();
    let effective = shows::series_rule(show, Some(&series), episodes);
    let words = shows::series_rule_planned(show, Some(&series), episodes).map(|planned| {
        let mut w = planned.words(&zone, today);
        if let Some(e) = &effective {
            let adjusted = e.words(&zone, today);
            w.last = adjusted.last;
            w.last_on = adjusted.last_on;
        }
        w
    });
    SeriesOut {
        rule_words: words,
        next_on: planner::next_on(effective.as_ref(), &zone, now),
        series,
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ShowView {
    pub show: ShowOut,
    pub resolved: Resolved,
    pub series: Vec<SeriesOut>,
    /// The episode `draft_first` drafted, if the save asked for one and a date was there.
    pub drafted: Option<EpisodeView>,
    /// Another show's time the save went on over (an admin's, a producer's or the key's).
    pub warnings: Vec<Clash>,
}

/// A show with its series, each with the episodes its rule's words and next date need.
pub(crate) async fn show_view(
    state: &ShowsState,
    show: Show,
    drafted: Option<EpisodeView>,
    warnings: Vec<Clash>,
) -> Result<ShowView, Fail> {
    let pool = db(state)?;
    let people = shows::people(pool, state.station.id).await?;
    let series = shows::list_series(pool, show.id).await?;
    let ids: Vec<uuid::Uuid> = series.iter().map(|s| s.id).collect();
    let episodes = shows::episodes_of(pool, &ids).await?;
    let current = series.last().map(|s| s.id);
    let mut out = Vec::new();
    let mut current_episodes = Vec::new();
    for s in series {
        let eps: Vec<Episode> = episodes
            .iter()
            .filter(|e| e.series_id == s.id)
            .cloned()
            .collect();
        if Some(s.id) == current {
            current_episodes = eps.clone();
        }
        out.push(series_out(state, &show, s, &eps));
    }
    let scheduled = planner::Scheduled {
        rule: shows::series_rule(&show, out.last().map(|s| &s.series), &current_episodes),
        series: out.last().map(|s| s.series.clone()),
        episodes: current_episodes,
        show: show.clone(),
    };
    Ok(ShowView {
        resolved: shows::resolve_show(&show, &people),
        show: show_out(state, show, Some(&scheduled)),
        series: out,
        drafted,
        warnings,
    })
}

pub(crate) async fn find_show(state: &ShowsState, slug: &str) -> Result<Show, Fail> {
    shows::get_show(db(state)?, state.station.id, slug)
        .await?
        .ok_or_else(|| missing(format!("show `{slug}`")))
}

#[utoipa::path(
    get,
    path = "/api/v1/shows",
    operation_id = "shows_list",
    tag = "shows",
    summary = "Every show",
    responses(
        (status = 200, description = "The shows.", body = Vec<ShowOut>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list(State(state): State<Arc<ShowsState>>, headers: HeaderMap) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let pool = db(&state)?;
    let all = planner::scheduled(pool, state.station.id).await?;
    let out: Vec<ShowOut> = all
        .iter()
        .map(|s| show_out(&state, s.show.clone(), Some(s)))
        .collect();
    Ok(Json(out).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}",
    operation_id = "shows_get_show",
    tag = "shows",
    summary = "A show",
    description = "The show, resolved, and its series.",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "The show.", body = ShowView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_show(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    Ok(Json(show_view(&state, show, None, Vec::new()).await?).into_response())
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}",
    operation_id = "shows_put_show",
    tag = "shows",
    summary = "Make or change a show",
    description = "Its cards are drawn first, and a name that cannot fit, or holds characters the card fonts cannot draw, refuses the save (422, with the field named). A station with no public address (`publishing.public_url` empty) still makes and saves the show, its names checked the same way, and keeps no cards until it has one (1.2.1). The presenter's fields (`presenter`, `contact`, `links`, `commercial_music`, `presenter_account`; ADR-0214, ADR-0219) are kept as stored when the body leaves them out, so a client that does not know them never wipes a DJ's details; send an empty value to clear one. A member of staff who is not an admin may name only their own account as `presenter_account` (403 otherwise); an API token may not set it at all. Changing `presenter` or `contact` takes an admin or the show's own presenter (ADR-0241; 403 otherwise); sending them unchanged is fine. `step` (`solo`, `guests`, `callers` or `full`, ADR-0271) is kept as stored when the body leaves it out, and a new show without it starts at `solo`; changing a stored show's step takes an admin, a producer or the show's presenter (403 otherwise; an API token may not), is logged, and takes effect at once if the show is on air. `rule` (release 1.0.4, ADR-0278) is when the show airs: a value sets it, `null` clears it, and left out leaves it to `slot` (an older studio's weekly slot) as the *When it airs: the rule* reference page tabulates; the drafted episodes are not moved by this (use `PUT …/rule` for that). `draft_first: true` makes series 1 if the show has none and drafts its first episode in the same transaction (the date under way, else the next); the reply adds `drafted`, and a refusal saves nothing. A presenter (or a screener) is refused a first episode over another show's time (409, `field` `rule`, the other show named, `clashes`); an admin, a producer, the station key and a token go on and the reply carries `warnings`. Every show carries `rule`, `rule_words` and `next_on`, and `slot` while the rule is exactly a weekly slot.",
    params(("show" = String, Path, description = "The show's slug")),
    request_body = weavesignal_model::shows::ShowInput,
    responses(
        (status = 200, description = "The show.", body = ShowView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_show(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
    Json(input): Json<ShowInput>,
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
    let pool = db(&state)?;
    input.check()?;
    // Who may tie a staff account to the show (ADR-0218): an admin anyone, a member of staff
    // only themselves, a token nobody.
    if let Some(Some(account)) = input.presenter_account {
        let allowed = match &actor {
            crate::staff::Actor::Station => true,
            crate::staff::Actor::Staff(a) => a.role.is_admin() || a.id == account,
            crate::staff::Actor::Token(_) => false,
        };
        if !allowed {
            return Ok(problem(
                StatusCode::FORBIDDEN,
                "only an admin names another account as a show's presenter",
            ));
        }
    }
    // Who may change the presenter's name and the takedown contact the consent words name
    // (ADR-0241): an admin (or the station key), or the show's own presenter. A save that sends
    // them unchanged passes, as the show editor sends every field.
    let stored = shows::get_show(pool, state.station.id, &slug).await?;
    let changes = |new: &Option<String>, old: Option<&str>| {
        new.as_deref()
            .is_some_and(|n| n.trim() != old.unwrap_or("").trim())
    };
    let touches = changes(
        &input.presenter,
        stored.as_ref().map(|s| s.presenter.as_str()),
    ) || changes(&input.contact, stored.as_ref().map(|s| s.contact.as_str()));
    if touches {
        let own = |id: uuid::Uuid| {
            stored.as_ref().and_then(|s| s.presenter_account) == Some(id)
                || input.presenter_account == Some(Some(id))
        };
        let allowed = match &actor {
            crate::staff::Actor::Station => true,
            crate::staff::Actor::Staff(a) => a.role.is_admin() || own(a.id),
            crate::staff::Actor::Token(_) => false,
        };
        if !allowed {
            return Ok(problem(
                StatusCode::FORBIDDEN,
                "only an admin or the show's own presenter changes its presenter and takedown contact",
            ));
        }
    }
    // Who may change a show's step (ADR-0271): an admin, a producer or the show's presenter, as
    // for the step route. A new show may start at any step; a save that sends the step it
    // already has passes, as the show editor sends every field.
    let step_change = match (&stored, input.step) {
        (Some(old), Some(new)) if old.step != new => {
            let presenter = old.presenter_account;
            let own =
                |id: uuid::Uuid| presenter == Some(id) || input.presenter_account == Some(Some(id));
            let allowed = match &actor {
                crate::staff::Actor::Staff(a) if own(a.id) => true,
                other => crate::steps::may_change(other, presenter),
            };
            if !allowed {
                return Ok(problem(
                    StatusCode::FORBIDDEN,
                    "Only an admin, a producer or the show's presenter changes its step.",
                ));
            }
            Some((old.step, new))
        }
        _ => None,
    };
    // The cards are drawn before the save, so a name that cannot fit refuses it (0008 §3).
    let provisional = Resolved {
        show: input.name.trim().to_owned(),
        slug: slug.clone(),
        description: input.description.clone(),
        colour_slot: input.colour_slot,
        layout: input.layout.clone(),
        // The card prints a weekly slot: the rule's when it is exactly one, else an older
        // studio's.
        slot: match &input.rule {
            Some(rule) => rule.as_ref().and_then(Rule::as_slot),
            None => input.slot,
        },
        rule: input.rule.clone().flatten(),
        hosts: input.hosts.iter().map(|h| h.trim().to_owned()).collect(),
        guests: Vec::new(),
        tags: input.tags.clone(),
        explicit: input.explicit,
        series: None,
        episode: None,
        inherited: Vec::new(),
        frozen: false,
    };
    if !shows::is_slug(&slug) {
        return Err(ShowError::Invalid(format!(
            "`{slug}` is not a show path: lower-case letters and digits, joined by hyphens"
        ))
        .into());
    }
    // Without a public address (a loopback install) the names are still checked against a
    // stand-in address, and the show is saved with no cards (1.2.1; 1.2.0 refused it, 503).
    let printed = state.printed_address();
    let renders = draw(card_data_or_stand_in(&state, &provisional)).await?;
    // The show, and with `draft_first` series 1 and its first episode, in one transaction: a
    // refusal (a presenter's clash among them) saves nothing.
    let made = ruleplan::put_show_drafting(
        pool,
        state.station.id,
        &slug,
        &input,
        &state.zone(),
        Timestamp::now(),
        shows::Maker {
            label: &actor.label(),
            account: match &actor {
                crate::staff::Actor::Staff(a) => Some(a.id),
                _ => None,
            },
        },
        clash_policy(&actor),
    )
    .await?;
    let show = made.show;
    if let Some((from, to)) = step_change {
        let by = shows::StepBy {
            label: actor.label(),
            account: match &actor {
                crate::staff::Actor::Staff(a) => Some(a.id),
                _ => None,
            },
        };
        shows::log_step_change(pool, state.station.id, show.id, from, to, &by).await?;
        tracing::info!(show = %show.slug, %from, %to, by = %by.label, "a show's step changed");
        // Told to every window at once, if this is the show the station's step comes from.
        if let Some(live) = &state.live {
            live.step_moved(&crate::steps::mover_name(&actor));
        }
    } else if stored.is_none() {
        // A new show: the studio may be waiting for the schedule to show it.
        if let Some(live) = &state.live {
            live.wake_steps();
        }
    }
    if let Some(old) = &stored
        && old.rule != show.rule
    {
        announce_rule_change(
            &state,
            &actor,
            &show,
            None,
            old.rule.as_ref(),
            show.rule.as_ref(),
        )
        .await;
    }
    if printed.is_some() {
        let cards = keep(&state, &renders).await?;
        shows::set_cards(pool, Level::Show, show.id, &cards).await?;
    }
    let show = find_show(&state, &slug).await?;
    let drafted = match made.drafted {
        Some((series, episode)) => {
            let view = series_view(&state, &show, series.clone()).await?;
            save_cards(&state, Level::Series, series.id, &view.resolved).await?;
            let r = resolve(&state, &show, &series, &episode).await?;
            save_cards(&state, Level::Episode, episode.id, &r).await?;
            let episode = find_episode(&state, &series, episode.number).await?;
            Some(EpisodeView {
                episode,
                resolved: r,
            })
        }
        None => None,
    };
    Ok(Json(show_view(&state, show, drafted, made.warnings).await?).into_response())
}

/// Who may delete a show (ADR-0279): the station key, an admin or a producer. Not a presenter,
/// the show's own included, a screener or an API token.
fn may_delete(actor: &crate::staff::Actor) -> bool {
    use weavesignal_model::staff::Role;
    match actor {
        crate::staff::Actor::Station => true,
        crate::staff::Actor::Staff(a) => a.role.is_admin() || a.role == Role::Producer,
        crate::staff::Actor::Token(_) => false,
    }
}

/// What a delete answers: what went with the show, and how many files.
#[derive(Serialize, utoipa::ToSchema)]
struct ShowDeleted {
    #[serde(flatten)]
    gone: shows::DeletedShow,
    /// Card pictures, the slate's still, published audio files and carts' files deleted from
    /// the disk with it.
    files_deleted: usize,
}

/// Delete a show that has never aired (ADR-0279).
#[utoipa::path(
    delete,
    path = "/api/v1/shows/{show}",
    operation_id = "shows_delete_show",
    tag = "shows",
    summary = "Delete a show that has never aired",
    description = "An admin or a producer (and the station key) may; a presenter, even of their own show, a screener and an API token may not (403). Refused (409, with one sentence naming how many episodes aired) when any of its episodes has aired or is on air now: an aired show stays as the station's record. Otherwise, in one transaction, the show goes with its series, drafted episodes, own cart banks and carts, and the logs of its step and rule changes; then its card pictures, its slate still, its carts' files and any published audio files only it named are deleted from the disk, and the open cart wall is read again. A call's consent keeps the show's name in words and loses only the link to it. It cannot be undone. Past plays, the generated log and the chat name no show by its id, so they are left as they are (a clock or a log item that named one of the show's own carts is left naming a cart that is gone, as when a show's bank is removed). Release 1.0.4 (ADR-0279).",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "What was deleted.", body = ShowDeleted),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn delete_show(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
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
    if !may_delete(&actor) {
        return Ok(problem(
            StatusCode::FORBIDDEN,
            "Only an admin or a producer deletes a show.",
        ));
    }
    let pool = db(&state)?;
    let gone =
        shows::delete_show(pool, state.station.id, &slug, Timestamp::now().as_second()).await?;
    tracing::info!(show = %gone.slug, by = %actor.label(), "a show was deleted");
    // The show is gone and cannot come back, so what follows never fails the request: the open
    // cart wall is read again and the step reconciler woken first (a studio may still list the
    // banks, or wait on the schedule), then the files go as far as they can, and a file that
    // cannot be deleted is logged.
    let mut files_deleted = 0;
    if let Some(hub) = &state.hub {
        files_deleted += gone
            .cart_files
            .iter()
            .filter(|(_, f)| {
                f.as_deref()
                    .is_some_and(weavesignal_model::carts::is_cart_file)
            })
            .count();
        hub.forget_show_carts(&gone.cart_files).await;
        // The show's host keys went with it (ADR-0280): their monitor seats leave the room
        // whether or not a page has an event stream open, and a connection with one is closed.
        for key_id in &gone.key_ids {
            hub.sources.close_later(key_id);
            crate::monitor::remove_seats(hub, key_id);
        }
    }
    if let Some(live) = &state.live {
        live.wake_steps();
    }
    match remove_unused_cards(&state, pool, gone.card_files.clone()).await {
        Ok(n) => files_deleted += n,
        Err(_) => {
            tracing::warn!(show = %gone.slug, "cannot tell which card files a deleted show left")
        }
    }
    match shows::episode_files_in_use(pool).await {
        Ok(in_use) => {
            for f in gone
                .media_files
                .iter()
                .filter(|f| file_name(f) && !in_use.contains(*f))
            {
                match tokio::fs::remove_file(state.publishing.episodes_dir.join(f)).await {
                    Ok(()) => files_deleted += 1,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        tracing::warn!(file = %f, error = %e, "cannot delete a deleted show's audio file")
                    }
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "cannot tell which audio files a deleted show left"),
    }
    if let Some(cfg) = &state.video {
        match tokio::fs::remove_file(crate::video::show_still(cfg, gone.id)).await {
            Ok(()) => files_deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(error = %e, "cannot delete a deleted show's slate still"),
        }
    }
    Ok(Json(ShowDeleted {
        gone,
        files_deleted,
    })
    .into_response())
}

/// How a draft over another show's time is treated for this actor (design section 9): an admin,
/// a producer, the station key and an API token are warned and may go on; a presenter, or a
/// screener (who runs no schedule), is refused.
pub(crate) fn clash_policy(actor: &crate::staff::Actor) -> Clashes {
    use weavesignal_model::staff::Role;
    match actor {
        crate::staff::Actor::Staff(a) if matches!(a.role, Role::Presenter | Role::Screener) => {
            Clashes::Refuse
        }
        _ => Clashes::Warn,
    }
}

/// Log a rule changed by a save, and tell every window.
pub(crate) async fn announce_rule_change(
    state: &ShowsState,
    actor: &crate::staff::Actor,
    show: &Show,
    series: Option<&Series>,
    from: Option<&Rule>,
    to: Option<&Rule>,
) {
    let Some(pool) = &state.db else { return };
    let by = shows::StepBy {
        label: actor.label(),
        account: match actor {
            crate::staff::Actor::Staff(a) => Some(a.id),
            _ => None,
        },
    };
    if let Err(e) = ruleplan::log_rule_change(
        pool,
        state.station.id,
        show.id,
        series.map(|s| s.id),
        from,
        to,
        &by,
    )
    .await
    {
        tracing::warn!(error = %e, "cannot log a changed rule");
    }
    tell_windows(state, actor, show, None);
}

/// *Moira changed when The Folk Club airs.* `change` is the token the saving window sent, if
/// any, given back on the notice so that window knows it for its own (1.0.5).
pub(crate) fn tell_windows(
    state: &ShowsState,
    actor: &crate::staff::Actor,
    show: &Show,
    change: Option<&str>,
) {
    tracing::info!(show = %show.slug, by = %actor.label(), "a show's rule changed");
    if let Some(live) = &state.live {
        live.rule_changed(
            &crate::steps::mover_name(actor),
            &show.name,
            &show.slug,
            change,
        );
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/cards",
    operation_id = "shows_show_cards",
    tag = "cards",
    summary = "Draw the show's cards again",
    description = "Four renders per save (ADR-0054).",
    params(("show" = String, Path, description = "The show's slug")),
    responses(
        (status = 200, description = "The cards now current.", body = CardsKept),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn show_cards(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(slug): UrlPath<String>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let people = shows::people(db(&state)?, state.station.id).await?;
    let r = shows::resolve_show(&show, &people);
    Ok(Json(draw_and_keep(&state, Level::Show, show.id, &r).await?).into_response())
}

// People.

#[derive(Serialize, utoipa::ToSchema)]
struct PersonView {
    id: uuid::Uuid,
    name: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/people",
    operation_id = "shows_list_people",
    tag = "people",
    summary = "Remembered people",
    description = "Admin only. The remembered guests and hosts, by name.",
    responses(
        (status = 200, description = "The people.", body = Vec<PersonView>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn list_people(State(state): State<Arc<ShowsState>>, headers: HeaderMap) -> Answer {
    if let Some(r) = admin_refusal(&state, &headers) {
        return Ok(r);
    }
    let mut people: Vec<PersonView> = shows::people(db(&state)?, state.station.id)
        .await?
        .into_iter()
        .map(|(id, name)| PersonView { id, name })
        .collect();
    people.sort_by_key(|p| p.name.to_lowercase());
    Ok(Json(people).into_response())
}

/// An episode named in an erasure's answer.
#[derive(Serialize, utoipa::ToSchema)]
struct EpisodeRef {
    show: String,
    series: u32,
    episode: u32,
    title: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct Erased {
    name: String,
    shows: usize,
    series: usize,
    episodes: usize,
    /// Cards drawn again without the name.
    cards_redrawn: usize,
    /// Card files that named them, deleted.
    card_files_deleted: usize,
    /// Cards that could not be drawn again (cards are off: no public address): still old.
    cards_not_redrawn: usize,
    /// Lines of the corrections log that held the name, redacted (ADR-0177).
    corrections_redacted: usize,
    /// Episodes whose title or description still names them: for a person to edit.
    mentions: Vec<EpisodeRef>,
    /// Published audio files whose tags may still name them: to be published again.
    files: Vec<EpisodeRef>,
}

/// Delete the card files in `files` that no show, series or episode names now. Card files are
/// named by their content and shared across stations, so a file another row names stays.
/// Returns how many went. Erasing a person (ADR-0122) and deleting a show (ADR-0279) both end
/// here.
async fn remove_unused_cards(
    state: &ShowsState,
    pool: &sqlx::PgPool,
    mut files: Vec<String>,
) -> Result<usize, Fail> {
    let in_use = shows::card_files_in_use(pool).await?;
    let mut deleted = 0;
    files.sort();
    files.dedup();
    for f in files.iter().filter(|f| !in_use.contains(*f)) {
        if f.contains('/') || f.starts_with('.') {
            continue;
        }
        match tokio::fs::remove_file(state.publishing.cards_dir.join(f)).await {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(file = %f, error = %e, "cannot delete a card file nothing names")
            }
        }
    }
    Ok(deleted)
}

/// Erase a person at their request (ADR-0122): the record, the name in every list and in what
/// aired, and the cards that showed it, drawn again; old card files no one names are deleted.
/// What only a person can fix (free text, published files' tags) is listed in the answer.
#[utoipa::path(
    post,
    path = "/api/v1/people/{id}/erase",
    operation_id = "shows_erase_person",
    tag = "people",
    summary = "Erase a person",
    description = "Admin only. At their request (ADR-0122): their record, their name in every list and in what aired, the cards drawn again, the corrections log redacted.",
    params(("id" = Uuid, Path, description = "The person")),
    responses(
        (status = 200, description = "What was erased.", body = Erased),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn erase_person(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<uuid::Uuid>,
) -> Answer {
    if let Some(r) = admin_refusal(&state, &headers) {
        return Ok(r);
    }
    let pool = db(&state)?;
    let Some(gone) = shows::erase_person(pool, state.station.id, id).await? else {
        return Err(missing("that person"));
    };
    tracing::info!(person = %id, shows = gone.shows.len(), series = gone.series.len(), episodes = gone.episodes.len(), "person erased");
    let people = shows::people(pool, state.station.id).await?;
    let mut old_files = Vec::new();
    let (mut redrawn, mut not_redrawn) = (0, 0);
    let mut mentions = Vec::new();
    let mut files = Vec::new();
    let cards_on = state.printed_address().is_some();
    for show in shows::list_shows(pool, state.station.id).await? {
        if gone.shows.contains(&show.id)
            && let Some(cards) = &show.cards
        {
            if cards_on {
                old_files.extend(shows::card_files(cards));
                let r = shows::resolve_show(&show, &people);
                draw_and_keep(&state, Level::Show, show.id, &r).await?;
                redrawn += 1;
            } else {
                not_redrawn += 1;
            }
        }
        for series in shows::list_series(pool, show.id).await? {
            let episodes = shows::list_episodes(pool, series.id).await?;
            if gone.series.contains(&series.id)
                && let Some(cards) = &series.cards
            {
                if cards_on {
                    old_files.extend(shows::card_files(cards));
                    let view = series_view(&state, &show, series.clone()).await?;
                    draw_and_keep(&state, Level::Series, series.id, &view.resolved).await?;
                    redrawn += 1;
                } else {
                    not_redrawn += 1;
                }
            }
            for ep in &episodes {
                let named = || EpisodeRef {
                    show: show.slug.clone(),
                    series: series.number,
                    episode: ep.number,
                    title: ep
                        .frozen
                        .as_ref()
                        .and_then(|f| f.episode.as_ref().map(|e| e.title.clone()))
                        .unwrap_or_else(|| ep.title.clone()),
                };
                if gone.mentions.contains(&ep.id) {
                    mentions.push(named());
                }
                if !gone.episodes.contains(&ep.id) {
                    continue;
                }
                if ep.files.is_some() {
                    files.push(named());
                }
                if let Some(cards) = &ep.cards {
                    if cards_on {
                        old_files.extend(shows::card_files(cards));
                        let r = resolve(&state, &show, &series, ep).await?;
                        draw_and_keep(&state, Level::Episode, ep.id, &r).await?;
                        redrawn += 1;
                    } else {
                        not_redrawn += 1;
                    }
                }
            }
        }
    }
    // Card files are named by their content and never replaced: the old ones that named the
    // person go once nothing names them.
    let deleted = remove_unused_cards(&state, pool, old_files).await?;
    Ok(Json(Erased {
        name: gone.name,
        shows: gone.shows.len(),
        series: gone.series.len(),
        episodes: gone.episodes.len(),
        cards_redrawn: redrawn,
        card_files_deleted: deleted,
        cards_not_redrawn: not_redrawn,
        corrections_redacted: gone.corrections_redacted,
        mentions,
        files,
    })
    .into_response())
}

// Series.

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SeriesView {
    series: SeriesOut,
    resolved: Resolved,
    episodes: Vec<EpisodeView>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct EpisodeView {
    episode: Episode,
    resolved: Resolved,
}

pub(crate) async fn find_series(state: &ShowsState, show: &Show, n: u32) -> Result<Series, Fail> {
    shows::get_series(db(state)?, show.id, n)
        .await?
        .ok_or_else(|| missing(format!("series {n} of `{}`", show.slug)))
}

pub(crate) async fn series_view(
    state: &ShowsState,
    show: &Show,
    series: Series,
) -> Result<SeriesView, Fail> {
    let pool = db(state)?;
    let people = shows::people(pool, state.station.id).await?;
    let zone = state.zone();
    let episodes = shows::list_episodes(pool, series.id).await?;
    let resolved = shows::resolve_series(show, &series, &people, &zone, &episodes);
    let views = episodes
        .iter()
        .map(|e| EpisodeView {
            resolved: shows::resolve_episode(show, &series, e, &people, &zone, &episodes),
            episode: e.clone(),
        })
        .collect();
    Ok(SeriesView {
        series: series_out(state, show, series, &episodes),
        resolved,
        episodes: views,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/series/{series}",
    operation_id = "shows_get_series",
    tag = "shows",
    summary = "A series",
    description = "The series and its episodes, resolved, with what each inherits.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show")),
    responses(
        (status = 200, description = "The series.", body = SeriesView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn get_series(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n)): UrlPath<(String, u32)>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    Ok(Json(series_view(&state, &show, series).await?).into_response())
}

#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/series/{series}",
    operation_id = "shows_put_series",
    tag = "shows",
    summary = "Make or change a series",
    description = "A field left out inherits the show's. `rule` is the series' own rule (release 1.0.4): a value sets it, `null` makes the series follow its show again, and left out leaves it to `slot` as a show's `PUT` does. `starts_on` and `planned_episodes` win over the rule's own start and count for this series.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show")),
    request_body = weavesignal_model::shows::SeriesInput,
    responses(
        (status = 200, description = "The series and its episodes.", body = SeriesView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_series(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n)): UrlPath<(String, u32)>,
    Json(input): Json<SeriesInput>,
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
    let show = find_show(&state, &slug).await?;
    // Checked against the card fonts before anything is kept (ADR-0176).
    let mut fields: Vec<(&'static str, &str)> = Vec::new();
    if let Some(t) = &input.title {
        fields.push(("series title", t));
    }
    if let Some(h) = &input.hosts {
        fields.extend(h.iter().map(|n| ("host name", n.as_str())));
    }
    weavesignal_cards::check_text(&fields)?;
    let before = shows::get_series(db(&state)?, show.id, n).await?;
    let series = shows::put_series(db(&state)?, state.station.id, &show, n, &input).await?;
    if before.as_ref().map(|b| &b.rule) != Some(&series.rule)
        && (before.is_some() || series.rule.is_some())
    {
        announce_rule_change(
            &state,
            &actor,
            &show,
            Some(&series),
            before.as_ref().and_then(|b| b.rule.as_ref()),
            series.rule.as_ref(),
        )
        .await;
    }
    let view = series_view(&state, &show, series).await?;
    save_cards(&state, Level::Series, view.series.series.id, &view.resolved).await?;
    let series = find_series(&state, &show, n).await?;
    Ok(Json(series_view(&state, &show, series).await?).into_response())
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/series/{series}/cards",
    operation_id = "shows_series_cards",
    tag = "cards",
    summary = "Draw the series' cards again",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show")),
    responses(
        (status = 200, description = "The cards now current.", body = CardsKept),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn series_cards(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n)): UrlPath<(String, u32)>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let view = series_view(&state, &show, series).await?;
    Ok(
        Json(draw_and_keep(&state, Level::Series, view.series.series.id, &view.resolved).await?)
            .into_response(),
    )
}

// Episodes.

pub(crate) async fn find_episode(
    state: &ShowsState,
    series: &Series,
    n: u32,
) -> Result<Episode, Fail> {
    shows::get_episode(db(state)?, series.id, n)
        .await?
        .ok_or_else(|| missing(format!("episode {n}")))
}

pub(crate) async fn resolve(
    state: &ShowsState,
    show: &Show,
    series: &Series,
    e: &Episode,
) -> Result<Resolved, Fail> {
    let pool = db(state)?;
    let people = shows::people(pool, state.station.id).await?;
    let siblings = shows::list_episodes(pool, series.id).await?;
    Ok(shows::resolve_episode(
        show,
        series,
        e,
        &people,
        &state.zone(),
        &siblings,
    ))
}

pub(crate) async fn episode_answer(
    state: &ShowsState,
    show: &Show,
    series: &Series,
    n: u32,
) -> Answer {
    let e = find_episode(state, series, n).await?;
    let resolved = resolve(state, show, series, &e).await?;
    Ok(Json(EpisodeView {
        episode: e,
        resolved,
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/series/{series}/episodes",
    operation_id = "shows_create_episodes",
    tag = "shows",
    summary = "Draft episodes",
    description = "The next episode, or `count` of them, filled in from the series' rule: the next dates it gives that no episode fills and it does not skip, each recorded as the date it fills (`occurrence`). With nothing drafted the first is the date under way now (its length, or an hour), else the next on or after the rule's start: never a date in the past. A rule with no more dates answers 409, and fewer than `count` are drafted when it has fewer left. A series with no rule drafts numbered episodes with no time. An episode that would overlap another show's by a minute or more is refused for a presenter (409, with `field` `rule`, the other show named and `clashes`); an admin, a producer, the station key and a token go on.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show")),
    request_body = weavesignal_model::shows::NewEpisodes,
    responses(
        (status = 201, description = "The new episodes.", body = Vec<EpisodeView>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn create_episodes(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n)): UrlPath<(String, u32)>,
    Json(new): Json<NewEpisodes>,
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
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    // Checked against the card fonts before anything is kept (ADR-0176), so a refused draft
    // adds no episode and no person.
    let mut fields: Vec<(&'static str, &str)> = vec![("episode title", new.title.as_str())];
    fields.extend(new.guests.iter().map(|g| ("guest name", g.as_str())));
    weavesignal_cards::check_text(&fields)?;
    let made = shows::create_episodes(
        db(&state)?,
        state.station.id,
        &show,
        &series,
        &state.zone(),
        &new,
        Timestamp::now(),
        shows::Maker {
            label: &actor.label(),
            account: match &actor {
                crate::staff::Actor::Staff(a) => Some(a.id),
                crate::staff::Actor::Station | crate::staff::Actor::Token(_) => None,
            },
        },
        clash_policy(&actor),
    )
    .await?;
    let mut out = Vec::new();
    for e in made.episodes {
        let r = resolve(&state, &show, &series, &e).await?;
        save_cards(&state, Level::Episode, e.id, &r).await?;
        let e = find_episode(&state, &series, e.number).await?;
        out.push(EpisodeView {
            episode: e,
            resolved: r,
        });
    }
    Ok((StatusCode::CREATED, Json(out)).into_response())
}

#[utoipa::path(
    patch,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}",
    operation_id = "shows_patch_episode",
    tag = "shows",
    summary = "Change an episode",
    description = "`null` resets a field to inheriting; `kind` is `live` or `prerecorded` (ADR-0188). An aired episode is frozen (409): correct it instead.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    request_body = weavesignal_model::shows::EpisodePatch,
    responses(
        (status = 200, description = "The episode.", body = EpisodeView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn patch_episode(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
    Json(patch): Json<EpisodePatch>,
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
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    // A presenter may not move an episode onto another show's time (design section 9).
    if clash_policy(&actor) == Clashes::Refuse
        && let Some(Some(at)) = &patch.air_at
    {
        let start = at.timestamp().as_second();
        let minutes = match patch.minutes {
            Some(Some(m)) => m,
            Some(None) => 60,
            None => ep.minutes.unwrap_or(60),
        };
        let minutes = if patch.minutes.is_none() && ep.minutes.is_none() {
            shows::series_rule(&show, Some(&series), &[]).map_or(60, |r| r.length())
        } else {
            minutes
        };
        let found = planner::clashes_for(
            db(&state)?,
            state.station.id,
            &state.zone(),
            Some(show.id),
            &[(start, start + i64::from(minutes) * 60)],
            Timestamp::now(),
        )
        .await?;
        let mut clashes: Vec<Clash> = found.into_iter().flatten().collect();
        if !clashes.is_empty() {
            for c in &mut clashes {
                c.refused = true;
            }
            return Err(Fail::Clash(Box::new(planner::ClashRefusal {
                message: clashes[0].words(),
                clashes,
            })));
        }
    }
    // The words are checked against the card fonts before anything is kept (ADR-0176), so a
    // refused save leaves the episode, and the people it would have added, as they were.
    check_card_words(&patch)?;
    let ep = shows::patch_episode(db(&state)?, state.station.id, &ep, &patch).await?;
    let r = resolve(&state, &show, &series, &ep).await?;
    save_cards(&state, Level::Episode, ep.id, &r).await?;
    episode_answer(&state, &show, &series, e).await
}

/// The words of an episode change that a card prints, against the card fonts.
fn check_card_words(patch: &EpisodePatch) -> Result<(), Fail> {
    let mut fields: Vec<(&'static str, &str)> = Vec::new();
    if let Some(t) = &patch.title {
        fields.push(("episode title", t));
    }
    if let Some(Some(h)) = &patch.hosts {
        fields.extend(h.iter().map(|n| ("host name", n.as_str())));
    }
    if let Some(g) = &patch.guests {
        fields.extend(g.iter().map(|n| ("guest name", n.as_str())));
    }
    Ok(weavesignal_cards::check_text(&fields)?)
}

#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}/cards",
    operation_id = "shows_episode_cards",
    tag = "cards",
    summary = "Draw the episode's cards again",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    responses(
        (status = 200, description = "The cards now current.", body = CardsKept),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn episode_cards(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    let r = resolve(&state, &show, &series, &ep).await?;
    Ok(Json(draw_and_keep(&state, Level::Episode, ep.id, &r).await?).into_response())
}

/// On air: the values are written into the episode, and its cards drawn once more from them.
#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}/air",
    operation_id = "shows_air",
    tag = "shows",
    summary = "Put an episode on air",
    description = "Frozen, its cards drawn once more.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    responses(
        (status = 200, description = "The episode.", body = EpisodeView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn air(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    let r = resolve(&state, &show, &series, &ep).await?;
    if shows::freeze_episode(db(&state)?, ep.id, &r).await? {
        let ep = find_episode(&state, &series, e).await?;
        let frozen = resolve(&state, &show, &series, &ep).await?;
        save_cards(&state, Level::Episode, ep.id, &frozen).await?;
    }
    episode_answer(&state, &show, &series, e).await
}

/// What a correction answers (ADR-0177).
#[derive(Serialize, utoipa::ToSchema)]
struct Corrected {
    #[serde(flatten)]
    episode: EpisodeView,
    correction: shows::Correction,
    /// The published files were tagged again with the corrected text and the new square.
    files_retagged: bool,
    /// Why they were not, when they should have been: the correction itself stands.
    files_problem: Option<String>,
}

/// Correct the public text of an aired episode (ADR-0177): the frozen record under it stays in
/// the log, the cards are drawn again from the corrected text, and the feed item changes under
/// the same episode id (its GUID). An admin or the member of staff who made the episode may
/// (ADR-0198), and the log names them. The cards
/// are drawn before anything is kept, so words that do not fit or cannot be drawn refuse the
/// correction with the field named.
#[utoipa::path(
    post,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}/corrections",
    operation_id = "shows_correct",
    tag = "shows",
    summary = "Correct an aired episode",
    description = "Corrects the public text of an aired episode (ADR-0177) with a `reason` and any of `title`, `description`, `hosts`, `guests` and `tags`. Its cards are drawn again, its feed item changes under the same id, and the log names who made the correction (a username, or `station key`). An admin (or the station key) may, and so may the member of staff who made the episode (ADR-0198); an episode made before makers were recorded, or by the station key or a token, is an admin's to correct.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    request_body = weavesignal_model::shows::CorrectionInput,
    responses(
        (status = 200, description = "The episode and the correction.", body = Corrected),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn correct(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
    Json(input): Json<CorrectionInput>,
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
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    if ep.aired_at.is_none() {
        return Err(ShowError::NotAired(format!("episode {e}")).into());
    }
    // ADR-0198: an admin, or the member of staff who made the episode.
    if !actor.is_admin() {
        let maker = shows::episode_maker(db(&state)?, ep.id).await?;
        let mine = matches!(&actor, crate::staff::Actor::Staff(a) if Some(a.id) == maker);
        if !mine {
            return Ok(crate::app::problem(
                StatusCode::FORBIDDEN,
                "Only an admin or the person who made this episode can correct it.",
            ));
        }
    }
    input.check()?;
    // Drawn and written to the cards directory before anything is kept: the files are named
    // by their content, so a refused or failed correction leaves only an unused file, and the
    // correction points at them in its own transaction.
    let cards = if state.printed_address().is_some() {
        let current = resolve(&state, &show, &series, &ep).await?;
        let renders = draw(card_data(&state, &input.applied(&current))?).await?;
        Some(keep(&state, &renders).await?)
    } else {
        None
    };
    let account = match &actor {
        crate::staff::Actor::Staff(a) => Some(a.id),
        crate::staff::Actor::Station | crate::staff::Actor::Token(_) => None,
    };
    let pool = db(&state)?;
    let correction = shows::correct_episode(
        pool,
        state.station.id,
        ep.id,
        &input,
        &actor.label(),
        account,
        cards.as_ref(),
    )
    .await?;
    tracing::info!(episode = %ep.id, by = %correction.made_by, "episode corrected");
    // Files already published carry the old title and card in their tags: write them again.
    let ep = find_episode(&state, &series, e).await?;
    let (mut retagged, mut problem) = (false, None);
    if let Some(files) = &ep.files {
        let name = |k: &str| {
            files
                .get(k)
                .and_then(|v| v.get("file"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        if let Some(enclosure) = name("enclosure") {
            let again = FilesInput {
                enclosure,
                opus: name("opus"),
            };
            // The correction is kept by now: a failure here is reported, not answered as one.
            match write_files(&state, &slug, n, e, &ep, &again).await {
                Ok(files) => match shows::set_files(pool, ep.id, &files).await {
                    Ok(()) => retagged = true,
                    Err(err) => problem = Some(format!("the files record: {err}")),
                },
                Err(Fail::Status(_, m)) => problem = Some(m),
                Err(Fail::Clash(c)) => problem = Some(c.message),
            }
            if let Some(m) = &problem {
                tracing::warn!(episode = %ep.id, problem = %m, "corrected episode's files not tagged again");
            }
        }
    }
    let ep = find_episode(&state, &series, e).await?;
    let resolved = resolve(&state, &show, &series, &ep).await?;
    Ok(Json(Corrected {
        episode: EpisodeView {
            episode: ep,
            resolved,
        },
        correction,
        files_retagged: retagged,
        files_problem: problem,
    })
    .into_response())
}

/// An episode's corrections log, oldest first.
#[utoipa::path(
    get,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}/corrections",
    operation_id = "shows_list_corrections",
    tag = "shows",
    summary = "An episode's corrections",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    responses(
        (status = 200, description = "The log, newest last.", body = Vec<weavesignal_model::shows::Correction>),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn list_corrections(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    Ok(Json(shows::list_corrections(db(&state)?, ep.id).await?).into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct FilesInput {
    /// The MP3 or AAC file (ADR-0018), as named in `publishing.episodes_dir`.
    enclosure: String,
    /// The Ogg Opus copy.
    #[serde(default)]
    opus: Option<String>,
}

/// A published file's name: a plain name in the episodes directory, nothing else.
fn file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp3" => Some("audio/mpeg"),
        "m4a" | "mp4" => Some("audio/mp4"),
        "opus" | "ogg" => Some("audio/ogg"),
        _ => None,
    }
}

/// The episode's tags and its card, written into each published file as the last step
/// (ADR-0019), then the files recorded for the feed. The files come from the station's own
/// recorder, not from an upload.
#[utoipa::path(
    put,
    path = "/api/v1/shows/{show}/series/{series}/episodes/{episode}/files",
    operation_id = "shows_put_files",
    tag = "shows",
    summary = "Name the episode's published files",
    description = "The files are in `publishing.episodes_dir`: their tags and the card are written into them, last.",
    params(("show" = String, Path, description = "The show's slug"), ("series" = u32, Path, description = "The series' number in the show"), ("episode" = u32, Path, description = "The episode's number in the series")),
    request_body = FilesInput,
    responses(
        (status = 200, description = "The episode.", body = EpisodeView),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 409, response = crate::openapi::Conflict),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn put_files(
    State(state): State<Arc<ShowsState>>,
    headers: HeaderMap,
    UrlPath((slug, n, e)): UrlPath<(String, u32, u32)>,
    Json(input): Json<FilesInput>,
) -> Answer {
    if let Some(r) = refusal(&state, &headers) {
        return Ok(r);
    }
    let show = find_show(&state, &slug).await?;
    let series = find_series(&state, &show, n).await?;
    let ep = find_episode(&state, &series, e).await?;
    let files = write_files(&state, &slug, n, e, &ep, &input).await?;
    shows::set_files(db(&state)?, ep.id, &files).await?;
    episode_answer(&state, &show, &series, e).await
}

/// Write the episode's tags and its current square into the named files; the files record
/// for the feed (`put_files` and a correction both keep it).
async fn write_files(
    state: &ShowsState,
    slug: &str,
    n: u32,
    e: u32,
    ep: &Episode,
    input: &FilesInput,
) -> Result<Value, Fail> {
    let public = public(state)?.to_owned();
    let show = find_show(state, slug).await?;
    let series = find_series(state, &show, n).await?;
    let r = resolve(state, &show, &series, ep).await?;
    let square = ep
        .cards
        .as_ref()
        .and_then(|c| c.get("square"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Fail::Status(
                StatusCode::CONFLICT,
                "the episode's cards have not been drawn yet".into(),
            )
        })?;
    let art = tokio::fs::read(state.publishing.cards_dir.join(square))
        .await
        .map_err(|e| {
            Fail::Status(
                StatusCode::CONFLICT,
                format!("the card square is missing: {e}"),
            )
        })?;
    let unprocessable = |m: String| Fail::Status(StatusCode::UNPROCESSABLE_ENTITY, m);
    let mut wanted: Vec<(&str, String)> = vec![("enclosure", input.enclosure.clone())];
    if let Some(o) = &input.opus {
        wanted.push(("opus", o.clone()));
    }
    let air = r
        .episode
        .as_ref()
        .and_then(|e| e.air_at)
        .and_then(|t| Timestamp::from_second(t).ok())
        .map(|t| t.to_zoned(state.zone()));
    let tags = EpisodeTags {
        title: r
            .episode
            .as_ref()
            .map(|e| e.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("{}, series {n}, episode {e}", r.show)),
        show: r.show.clone(),
        hosts: r.hosts.clone(),
        station: state.station_name(),
        series: Some(n),
        episode: Some(e),
        date: air.as_ref().map(Zoned::date),
        description: r
            .episode
            .as_ref()
            .map(|e| e.description.clone())
            .unwrap_or_default(),
        copyright: Some(state.publishing.copyright.clone()).filter(|c| !c.is_empty()),
        url: Some(format!("{public}/{slug}/s{n}/e{e}")),
    };
    let mut files = serde_json::Map::new();
    for (role, name) in wanted {
        if !file_name(&name) {
            return Err(unprocessable(format!("`{name}` is not a file name")));
        }
        let path: PathBuf = state.publishing.episodes_dir.join(&name);
        let Some(mime) = mime(&path) else {
            return Err(unprocessable(format!(
                "`{name}` is not MP3, AAC or Ogg Opus"
            )));
        };
        match (role, mime) {
            ("enclosure", "audio/mpeg" | "audio/mp4") | ("opus", "audio/ogg") => {}
            ("enclosure", _) => {
                return Err(unprocessable(
                    "the enclosure is MP3 or AAC, for Apple and Spotify (ADR-0018)".into(),
                ));
            }
            _ => return Err(unprocessable("the Opus copy is an .opus file".into())),
        }
        // Tagged in a copy beside it, then renamed over it, so a listener fetching the file
        // meanwhile, or a failure part-way, never meets a half-written one. The copy's name
        // starts with a dot, which no published name may (`file_name`).
        let (tags, art, p) = (tags.clone(), art.clone(), path.clone());
        let tmp = state.publishing.episodes_dir.join(format!(".retag-{name}"));
        let read = tokio::task::spawn_blocking(move || {
            let done = (|| -> Result<tags::ReadBack, tags::TagError> {
                std::fs::copy(&p, &tmp).map_err(|e| tags::TagError::Write(e.to_string()))?;
                tags::write(&tmp, &tags, &art)?;
                let read = tags::read(&tmp)?;
                std::fs::rename(&tmp, &p).map_err(|e| tags::TagError::Write(e.to_string()))?;
                Ok(read)
            })();
            if done.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            done
        })
        .await
        .map_err(|e| Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| unprocessable(format!("`{name}`: {e}")))?;
        let length = tokio::fs::metadata(&path)
            .await
            .map_err(|e| unprocessable(format!("`{name}`: {e}")))?
            .len();
        files.insert(
            role.to_owned(),
            json!({ "file": name, "mime": mime, "length": length, "duration_ms": read.duration_ms }),
        );
    }
    Ok(Value::Object(files))
}

// The feed.

fn image(
    public: &str,
    cards: Option<&Value>,
    which: &str,
    alt: &str,
) -> Option<weavesignal_feeds::Image> {
    let file = cards?.get(which)?.as_str()?;
    let (w, h) = Output::parse(which)?.size();
    Some(weavesignal_feeds::Image {
        href: format!("{public}/cards/{file}"),
        width: w,
        height: h,
        alt: alt.to_owned(),
        purpose: if which == "square" {
            "artwork"
        } else {
            "social"
        },
    })
}

fn media(public: &str, v: &Value, title: &str) -> Option<weavesignal_feeds::Media> {
    let file = v.get("file")?.as_str()?;
    let mime = v.get("mime")?.as_str()?.to_owned();
    let length = v.get("length")?.as_u64()?;
    let ms = v.get("duration_ms").and_then(Value::as_u64).unwrap_or(0);
    Some(weavesignal_feeds::Media {
        url: format!("{public}/media/episodes/{file}"),
        length,
        codecs: (mime == "audio/ogg").then(|| "opus".to_owned()),
        bitrate: (ms > 0).then(|| length * 8 * 1000 / ms),
        mime,
        title: Some(title.to_owned()),
    })
}

/// `GET /feeds/{show}.xml`: the show's podcast feed, its aired and published episodes newest
/// first, every one with its card and both enclosures.
#[utoipa::path(
    get,
    path = "/feeds/{file}",
    operation_id = "shows_feed",
    tag = "public",
    summary = "A show's podcast feed",
    description = "`{slug}.xml`, the show's RSS feed with its episodes. While the show is kept for people with a key the feed lists none (1.3.3). 503 while the show's card has not been drawn or `publishing.public_url` is not set.",
    params(("file" = String, Path, description = "The show's slug and `.xml`")),
    responses(
        (status = 200, description = "The feed.", body = String, content_type = "application/rss+xml"),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database or the card renderer failed.", body = crate::openapi::Problem),
    ),
    security(()),
)]
async fn feed(State(state): State<Arc<ShowsState>>, UrlPath(file): UrlPath<String>) -> Answer {
    let Some(slug) = file.strip_suffix(".xml") else {
        return Err(missing(format!("feed `{file}`")));
    };
    if !shows::is_slug(slug) {
        return Err(missing(format!("feed `{file}`")));
    }
    let public = public(&state)?.to_owned();
    let pool = db(&state)?;
    let show = find_show(&state, slug).await?;
    let people = shows::people(pool, state.station.id).await?;
    let feed_url = format!("{public}/feeds/{slug}.xml");
    // Assigned once, on the first fetch; later fetches only read it (a public route).
    let guid = match show.feed_guid {
        Some(g) => g,
        None => {
            shows::keep_feed_guid(pool, show.id, weavesignal_feeds::podcast_guid(&feed_url)).await?
        }
    };
    let resolved = shows::resolve_show(&show, &people);
    let alt = show
        .cards
        .as_ref()
        .and_then(|c| c.get("alt"))
        .and_then(Value::as_str)
        .unwrap_or(&show.name)
        .to_owned();
    let square = image(&public, show.cards.as_ref(), "square", &alt).ok_or_else(|| {
        Fail::Status(
            StatusCode::SERVICE_UNAVAILABLE,
            "the show's card has not been drawn yet".into(),
        )
    })?;
    let p = &state.publishing;
    let channel = weavesignal_feeds::Channel {
        feed_url,
        guid: Some(guid),
        title: show.name.clone(),
        description: show.description.clone(),
        link: format!("{public}/{slug}"),
        language: p.language.clone(),
        square,
        social: image(&public, show.cards.as_ref(), "link", &alt),
        author: state.station_name(),
        owner_name: Some(p.owner_name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| state.station_name()),
        owner_email: Some(p.owner_email.clone()).filter(|e| !e.is_empty()),
        copyright: Some(p.copyright.clone()).filter(|c| !c.is_empty()),
        category: Some(p.category.clone())
            .filter(|c| !c.is_empty())
            .map(|c| (c, Some(p.subcategory.clone()).filter(|s| !s.is_empty()))),
        explicit: show.explicit,
        locked: p.locked,
        location: Some(p.place.clone()).filter(|l| !l.is_empty()),
        persons: resolved
            .hosts
            .iter()
            .map(weavesignal_feeds::Person::host)
            .collect(),
        serial: false,
        built: Zoned::now().with_time_zone(state.zone()),
        generator: "WeaveSignal".into(),
    };
    let mut items = Vec::new();
    // A keyed show's feed stays, empty, until the show is open to everyone.
    let published = if kept_for_keys(&state, show.id).await? {
        Vec::new()
    } else {
        shows::published_episodes(pool, show.id).await?
    };
    for (series, ep) in published {
        let Some(r) = ep.frozen.clone() else { continue };
        let Some(files) = &ep.files else { continue };
        let kind = |v: &Value| match v.get("mime").and_then(Value::as_str) {
            Some("audio/mp4") => "AAC",
            _ => "MP3",
        };
        let Some(enclosure) = files
            .get("enclosure")
            .and_then(|v| media(&public, v, kind(v)))
        else {
            continue;
        };
        let alternates: Vec<_> = files
            .get("opus")
            .and_then(|v| media(&public, v, "Opus"))
            .into_iter()
            .collect();
        let alt = ep
            .cards
            .as_ref()
            .and_then(|c| c.get("alt"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let e = r.episode.clone();
        let when = e
            .as_ref()
            .and_then(|e| e.air_at)
            .or(ep.aired_at)
            .and_then(|t| Timestamp::from_second(t).ok())
            .unwrap_or_else(Timestamp::now)
            .to_zoned(state.zone());
        let mut persons: Vec<_> = r
            .hosts
            .iter()
            .map(weavesignal_feeds::Person::host)
            .collect();
        persons.extend(r.guests.iter().map(weavesignal_feeds::Person::guest));
        let title = e
            .as_ref()
            .map(|e| e.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("Series {}, episode {}", series.number, ep.number));
        items.push(weavesignal_feeds::Item {
            guid: ep.id.hyphenated().to_string(),
            title,
            description: e
                .as_ref()
                .map(|e| e.description.clone())
                .unwrap_or_default(),
            link: format!("{public}/{slug}/s{}/e{}", series.number, ep.number),
            published: when,
            duration_s: files
                .get("enclosure")
                .and_then(|v| v.get("duration_ms"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
                / 1000,
            enclosure,
            alternates,
            square: image(&public, ep.cards.as_ref(), "square", &alt),
            social: image(&public, ep.cards.as_ref(), "link", &alt),
            season: Some((
                series.number,
                series
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Series {}", series.number)),
            )),
            episode: Some(ep.number),
            episode_type: weavesignal_feeds::EpisodeType::Full,
            explicit: r.explicit,
            persons,
            transcript: None,
            chapters: None,
        });
    }
    let xml = weavesignal_feeds::rss::rss(&channel, &items).map_err(|e| {
        tracing::error!(error = %e, "feeds: write");
        Fail::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/rss+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        Body::from(xml),
    )
        .into_response())
}

// The files.

/// Whether a show is kept for people with a key now: its episodes' files are then in no feed,
/// public list or file route (1.3.3).
pub(crate) async fn kept_for_keys(state: &ShowsState, show: uuid::Uuid) -> Result<bool, Fail> {
    let access =
        weavesignal_model::listener_keys_db::get_access(db(state)?, state.station.id, show).await?;
    Ok(crate::public::keyed_by(
        access.as_ref(),
        crate::hub::now_ms() as i64,
    ))
}

/// `GET /media/episodes/{file}`: a published episode file, with ranges. Only a file an episode
/// names, and none whose show is kept for people with a key; the rules unread, nothing.
#[utoipa::path(
    get,
    path = "/media/episodes/{file}",
    operation_id = "shows_episode_file",
    tag = "public",
    summary = "A published episode file",
    description = "A file an episode names, with ranges, as the feed gives its address. 404 for a file no episode names, and while a show naming it is kept for people with a key (1.3.3).",
    params(("file" = String, Path, description = "The file's name")),
    responses(
        (status = 200, description = "The file.", content_type = "application/octet-stream"),
        (status = 206, description = "The range asked for."),
        (status = 404, response = crate::openapi::NotFound),
        (status = 503, response = crate::openapi::Unavailable),
        (status = 500, description = "The database failed.", body = crate::openapi::Problem),
    ),
    security(()),
)]
async fn episode_file(
    State(state): State<Arc<ShowsState>>,
    UrlPath(file): UrlPath<String>,
    request: Request<Body>,
) -> Answer {
    // The name checked is the one served: decoded once, here, and never from the address again.
    if !file_name(&file) {
        return Err(missing(format!("file `{file}`")));
    }
    let rules = shows::file_rules(db(&state)?, &file).await?;
    let now = crate::hub::now_ms() as i64;
    let keyed = rules.iter().any(|(rule, opens_at)| {
        rule.as_deref().is_some_and(|rule| {
            listener_keys::effective(rule, *opens_at, now) == listener_keys::Effective::Key
        })
    });
    if rules.is_empty() || keyed {
        return Err(missing(format!("file `{file}`")));
    }
    let served = ServeFile::new(state.publishing.episodes_dir.join(&file))
        .oneshot(request)
        .await;
    Ok(match served {
        Ok(response) => response.map(Body::new).into_response(),
        Err(never) => match never {},
    })
}

// ---- the description (ADR-0195) ----

/// Documentation of the answers and bodies the handlers build from `serde_json::Value`.
#[derive(Serialize, utoipa::ToSchema)]
struct CardsKept {
    /// The file names of the current cards, in `/cards/`.
    link: String,
    post: String,
    square: String,
    /// The cards' alternative text.
    alt: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct CardWords {
    alt: String,
    /// The page the card links to, on the station's public address.
    path: String,
    /// `show`, `series` or `episode`.
    level: String,
    /// The station fields the preview will print.
    station: serde_json::Value,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = WeekEpisodes)]
struct WeekBody {
    /// The zone the station's air times are read in.
    zone: String,
    episodes: Vec<WeekEpisode>,
    /// Every date of every show's rule in the range that no episode fills, is not skipped and is
    /// not over: what the week draws as dotted blocks, so the studio expands no rule itself.
    dates: Vec<crate::rules::OpenDateOut>,
    /// The station's public address (`publishing.public_url`, without a trailing slash), where
    /// listeners reach the calendar files, the cards and the feeds; `null` when none is set. The
    /// studio builds the links it gives out from this, never from the address it talks to,
    /// which on Cloudron is behind the login (release 1.0.5, ADR-0281).
    #[schema(example = "https://station.example")]
    public_url: Option<String>,
    /// The Studios view's columns, in order (1.3.0): `studio`, and `rehearsal` only when the
    /// station has a rehearsal studio. A later studio is one more entry. Absent from a station
    /// older than 1.3.0.
    studios: Vec<StudioColumn>,
    /// The rehearsal studio in the range (1.3.0): every booking, then every session (past or
    /// open; an open one ends now), each in start order. Empty when the station has no rehearsal
    /// studio. Never in the public calendar file.
    rehearsals: Vec<crate::rehearsal_bookings::RehearsalBlock>,
}

/// A card's fields, as `weavesignal-cards`' `CardData` has them (inheritance already resolved).
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct CardFields {
    /// The layout: `poster` or `slate`.
    layout: String,
    /// 1 to 12: the show's bank slot in the palette (ADR-0066).
    colour_slot: u8,
    /// `name`, `place` and `address`; left out, the station's own are filled in.
    station: Option<serde_json::Value>,
    /// `name`, `slug`, and optionally `description` and `slot` (`weekday`, `start`, `end`).
    show: serde_json::Value,
    /// `number`, and optionally `title`, `first`, `last` and `episodes`.
    series: Option<serde_json::Value>,
    /// `number`, `title`, and optionally `air` (a zoned time). Needs `series`.
    episode: Option<serde_json::Value>,
    hosts: Vec<String>,
    guests: Option<Vec<String>>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    list,
    get_show,
    put_show,
    delete_show,
    show_cards,
    put_series,
    get_series,
    series_cards,
    create_episodes,
    patch_episode,
    episode_cards,
    air,
    put_files,
    correct,
    list_corrections,
    preview,
    card_words,
    episodes_between,
    list_people,
    erase_person,
    feed,
    episode_file
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state(public: &str) -> Arc<ShowsState> {
        Arc::new(ShowsState {
            station: Station::new(weavesignal_model::StationId::new_random(), "Kettle FM").unwrap(),
            db: None,
            admin_token: Some("t0ken".into()),
            staff: Default::default(),
            publishing: PublishingConfig {
                public_url: public.into(),
                place: "Dunfermline".into(),
                ..PublishingConfig::default()
            },
            zone: Arc::new(crate::zone::StationZone::new("Europe/London")),
            live: None,
            hub: None,
            video: None,
        })
    }

    const CARD: &str = r#"{
        "layout": "slate", "colour_slot": 3,
        "station": {"name": "Kettle FM", "place": "Dunfermline", "address": "kettle.example"},
        "show": {"name": "Saturday Night Kettle", "slug": "saturday-night",
                 "slot": {"weekday": "saturday", "start": "20:00", "end": "23:00"}},
        "series": {"number": 3},
        "episode": {"number": 7, "title": "Ghost Train night", "air": "2026-10-24T20:00:00+01:00[Europe/London]"},
        "hosts": ["Shona Campbell", "Catriona MacKenzie-Buchanan"],
        "guests": ["Firth Lights", "Dr Una Robertson"]
    }"#;

    fn post(uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
        let mut b = Request::post(uri).header(header::CONTENT_TYPE, "application/json");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::from(body.to_owned())).unwrap()
    }

    #[tokio::test]
    async fn the_preview_route_draws_a_card_from_its_fields() {
        let app = router(state("https://kettle.example"));
        let res = app
            .clone()
            .oneshot(post(
                "/api/v1/cards/preview?output=link",
                CARD,
                Some("t0ken"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png");
        let png = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // 1,200 by 630, from the PNG header.
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1200);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 630);

        let res = app
            .clone()
            .oneshot(post("/api/v1/cards/preview", CARD, None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let long = CARD.replace(
            "Saturday Night Kettle",
            &"The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society "
                .repeat(3),
        );
        let res = app
            .oneshot(post("/api/v1/cards/preview", &long, Some("t0ken")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("show name"));
    }

    #[tokio::test]
    async fn the_words_route_fills_the_station_in_and_names_a_refused_field() {
        let app = router(state("https://kettle.example"));
        // The editor sends no station: the control plane's own is filled in.
        let without: serde_json::Value = serde_json::from_str(CARD).unwrap();
        let mut without = without;
        without.as_object_mut().unwrap().remove("station");
        let res = app
            .clone()
            .oneshot(post(
                "/api/v1/cards/words",
                &without.to_string(),
                Some("t0ken"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["level"], "episode");
        assert_eq!(body["path"], "/saturday-night/s3/e7");
        assert_eq!(body["station"]["address"], "kettle.example");
        assert!(
            body["alt"]
                .as_str()
                .unwrap()
                .starts_with("Saturday Night Kettle, Series 3 Episode 7, Ghost Train night")
        );
        // The preview takes the same shape without a station.
        let res = app
            .clone()
            .oneshot(post(
                "/api/v1/cards/preview",
                &without.to_string(),
                Some("t0ken"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        // A name that does not fit even at its floor is refused with the field named.
        let long = CARD.replace(
            "Saturday Night Kettle",
            &"The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society "
                .repeat(3),
        );
        let res = app
            .clone()
            .oneshot(post("/api/v1/cards/words", &long, Some("t0ken")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("show name"));
        // A colour outside the palette is refused with the field named.
        let bad = CARD.replace("\"colour_slot\": 3", "\"colour_slot\": 13");
        let res = app
            .oneshot(post("/api/v1/cards/words", &bad, Some("t0ken")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("colour slot 13"));
    }

    #[tokio::test]
    async fn without_a_database_or_an_address_the_routes_say_so() {
        let app = router(state(""));
        let res = app
            .clone()
            .oneshot(
                Request::get("/feeds/saturday-night.xml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let res = app
            .clone()
            .oneshot(
                Request::get("/api/v1/shows")
                    .header(header::AUTHORIZATION, "Bearer t0ken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let res = app
            .oneshot(
                Request::get("/feeds/..%2Fetc.xml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn published_file_names_are_plain() {
        assert!(file_name("saturday-night-s3e7.mp3"));
        assert!(!file_name("../etc/passwd"));
        assert!(!file_name(".hidden.mp3"));
        assert!(!file_name("a b.mp3"));
    }
}
