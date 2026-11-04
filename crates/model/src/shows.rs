// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Shows, series and episodes (A42, A43; ADR-0055; migration 0007).
//!
//! Three levels. A field on a lower level either inherits from the level above (it is `None`
//! here and NULL in the table) or is set on its own level; an inherited field follows its
//! parent by reference, so changing a series' hosts changes every episode that still
//! inherits them. Creating the next episode fills in its number (the last plus one) and its air
//! time (the next slot of the series' weekly rule); the person adds the title and the guests,
//! and a whole season can be drafted at once. When an episode goes on air its resolved values
//! are written into it and it is frozen: later changes above it do not rewrite what was
//! broadcast, which keeps the archive, the feeds and the music log true (D44).

use std::collections::HashMap;

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::StationId;

/// A weekly slot: a weekday (1 Monday to 7 Sunday), a start in minutes after midnight in the
/// station's zone, and a length in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub weekday: u8,
    pub start: u16,
    #[serde(default)]
    pub minutes: Option<u16>,
}

impl Slot {
    pub fn check(&self) -> Result<(), ShowError> {
        if !(1..=7).contains(&self.weekday) || self.start >= 1440 {
            return Err(ShowError::Invalid(
                "a slot is a weekday 1 to 7 and a start before 24:00".into(),
            ));
        }
        if self.minutes.is_some_and(|m| m == 0 || m > 1440) {
            return Err(ShowError::Invalid("a slot lasts 1 to 1,440 minutes".into()));
        }
        Ok(())
    }

    pub fn weekday(&self) -> Weekday {
        Weekday::from_monday_one_offset(self.weekday as i8).unwrap_or(Weekday::Monday)
    }

    /// The first time this slot starts strictly after `after`.
    pub fn next_after(&self, zone: &TimeZone, after: Timestamp) -> Option<Timestamp> {
        let local = after.to_zoned(zone.clone());
        let mut day = local.date();
        for _ in 0..15 {
            if day.weekday() == self.weekday() {
                let at = day
                    .at((self.start / 60) as i8, (self.start % 60) as i8, 0, 0)
                    .to_zoned(zone.clone())
                    .ok()?
                    .timestamp();
                if at > after {
                    return Some(at);
                }
            }
            day = day.tomorrow().ok()?;
        }
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShowError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0} has aired and is frozen; what was broadcast is not rewritten")]
    Frozen(String),
    #[error("{0} does not exist")]
    Missing(String),
    /// A correction is for an episode that has aired; before that, edit it (ADR-0177).
    #[error("{0} has not aired; edit it instead of correcting it")]
    NotAired(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn text(what: &str, s: &str, min: usize, max: usize) -> Result<(), ShowError> {
    let n = s.chars().count();
    if n < min || n > max || s.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ShowError::Invalid(format!(
            "the {what} is {min} to {max} characters, without control characters"
        )));
    }
    Ok(())
}

/// A show's path segment: lower-case letters and digits in hyphen-joined words.
pub fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 60
        && s.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Show {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub colour_slot: u8,
    /// `poster` or `slate`.
    pub layout: String,
    pub slot: Option<Slot>,
    pub hosts: Vec<Uuid>,
    pub tags: Vec<String>,
    pub explicit: bool,
    pub feed_guid: Option<Uuid>,
    pub cards: Option<serde_json::Value>,
}

/// A show as the studio app sends it; hosts by name.
#[derive(Clone, Debug, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub colour_slot: u8,
    #[serde(default = "poster")]
    pub layout: String,
    #[serde(default)]
    pub slot: Option<Slot>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub explicit: bool,
}

fn poster() -> String {
    "poster".into()
}

impl ShowInput {
    pub fn check(&self) -> Result<(), ShowError> {
        text("show name", self.name.trim(), 1, 160)?;
        text("description", &self.description, 0, 4000)?;
        if !(1..=12).contains(&self.colour_slot) {
            return Err(ShowError::Invalid(
                "the colour is a bank slot, 1 to 12 (ADR-0066)".into(),
            ));
        }
        if !matches!(self.layout.as_str(), "poster" | "slate") {
            return Err(ShowError::Invalid(
                "the layout is `poster` or `slate` (ADR-0067)".into(),
            ));
        }
        if let Some(s) = &self.slot {
            s.check()?;
        }
        check_people(&self.hosts)?;
        check_tags(&self.tags)
    }
}

fn check_people(names: &[String]) -> Result<(), ShowError> {
    if names.len() > 20 {
        return Err(ShowError::Invalid("at most 20 people".into()));
    }
    for n in names {
        text("person's name", n.trim(), 1, 128)?;
    }
    Ok(())
}

fn check_tags(tags: &[String]) -> Result<(), ShowError> {
    if tags.len() > 40 {
        return Err(ShowError::Invalid("at most 40 tags".into()));
    }
    for t in tags {
        text("tag", t.trim(), 1, 60)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Series {
    pub id: Uuid,
    pub show_id: Uuid,
    pub number: u32,
    pub title: Option<String>,
    pub description: Option<String>,
    /// `None` inherits the show's.
    pub hosts: Option<Vec<Uuid>>,
    pub slot: Option<Slot>,
    pub starts_on: Option<Date>,
    pub planned_episodes: Option<u32>,
    pub tags: Option<Vec<String>>,
    pub cards: Option<serde_json::Value>,
}

/// A series as the studio app sends it: a field left out or `null` inherits the show's.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SeriesInput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hosts: Option<Vec<String>>,
    #[serde(default)]
    pub slot: Option<Slot>,
    #[serde(default)]
    pub starts_on: Option<Date>,
    #[serde(default)]
    pub planned_episodes: Option<u32>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

impl SeriesInput {
    pub fn check(&self) -> Result<(), ShowError> {
        if let Some(t) = &self.title {
            text("series title", t.trim(), 1, 160)?;
        }
        if let Some(d) = &self.description {
            text("description", d, 0, 4000)?;
        }
        if let Some(h) = &self.hosts {
            check_people(h)?;
        }
        if let Some(s) = &self.slot {
            s.check()?;
        }
        if self.planned_episodes.is_some_and(|n| n == 0 || n > 520) {
            return Err(ShowError::Invalid(
                "a series plans 1 to 520 episodes".into(),
            ));
        }
        if let Some(t) = &self.tags {
            check_tags(t)?;
        }
        Ok(())
    }
}

/// Whether an episode is broadcast live or played from a recording (ADR-0188). Studios, the
/// other half of that decision, are not modelled yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeKind {
    #[default]
    Live,
    Prerecorded,
}

impl EpisodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EpisodeKind::Live => "live",
            EpisodeKind::Prerecorded => "prerecorded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "live" => Some(EpisodeKind::Live),
            "prerecorded" => Some(EpisodeKind::Prerecorded),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Episode {
    pub id: Uuid,
    pub series_id: Uuid,
    pub number: u32,
    pub title: String,
    pub description: String,
    /// Live unless marked pre-recorded (ADR-0188).
    pub kind: EpisodeKind,
    /// Seconds since the Unix epoch.
    pub air_at: Option<i64>,
    /// `None` inherits the slot's length.
    pub minutes: Option<u16>,
    pub hosts: Option<Vec<Uuid>>,
    pub guests: Vec<Uuid>,
    pub tags: Option<Vec<String>>,
    pub aired_at: Option<i64>,
    pub frozen: Option<Resolved>,
    pub files: Option<serde_json::Value>,
    pub cards: Option<serde_json::Value>,
}

/// `Some(None)` from JSON `null`: "reset to inherit"; a field left out is `None`.
fn double<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

/// A change to an episode. An inheriting field set to `null` goes back to inheriting.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EpisodePatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Live or pre-recorded (ADR-0188); it has no level above to inherit from.
    #[serde(default)]
    pub kind: Option<EpisodeKind>,
    #[serde(default, deserialize_with = "double")]
    pub air_at: Option<Option<Zoned>>,
    #[serde(default, deserialize_with = "double")]
    pub minutes: Option<Option<u16>>,
    #[serde(default, deserialize_with = "double")]
    pub hosts: Option<Option<Vec<String>>>,
    #[serde(default)]
    pub guests: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double")]
    pub tags: Option<Option<Vec<String>>>,
}

impl EpisodePatch {
    pub fn check(&self) -> Result<(), ShowError> {
        if let Some(t) = &self.title {
            text("episode title", t.trim(), 0, 200)?;
        }
        if let Some(d) = &self.description {
            text("description", d, 0, 4000)?;
        }
        if let Some(Some(m)) = self.minutes
            && (m == 0 || m > 1440)
        {
            return Err(ShowError::Invalid(
                "an episode lasts 1 to 1,440 minutes".into(),
            ));
        }
        if let Some(Some(h)) = &self.hosts {
            check_people(h)?;
        }
        if let Some(g) = &self.guests {
            check_people(g)?;
        }
        if let Some(Some(t)) = &self.tags {
            check_tags(t)?;
        }
        Ok(())
    }
}

/// An episode, series or show with everything it inherits filled in: what its card, its feed
/// item and its archive file say. An aired episode's is the frozen copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Resolved {
    pub show: String,
    pub slug: String,
    pub description: String,
    pub colour_slot: u8,
    pub layout: String,
    pub slot: Option<Slot>,
    pub hosts: Vec<String>,
    pub guests: Vec<String>,
    pub tags: Vec<String>,
    pub explicit: bool,
    pub series: Option<ResolvedSeries>,
    pub episode: Option<ResolvedEpisode>,
    /// The fields that came from the level above, for the editor's "inherited" mark.
    pub inherited: Vec<String>,
    pub frozen: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ResolvedSeries {
    pub number: u32,
    pub title: Option<String>,
    pub first: Option<Date>,
    pub last: Option<Date>,
    pub episodes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ResolvedEpisode {
    pub id: Uuid,
    pub number: u32,
    pub title: String,
    pub description: String,
    /// Frozen copies from before ADR-0188 have none, and were all live.
    #[serde(default)]
    pub kind: EpisodeKind,
    pub air_at: Option<i64>,
    pub minutes: Option<u16>,
}

fn names(ids: &[Uuid], people: &HashMap<Uuid, String>) -> Vec<String> {
    ids.iter()
        .filter_map(|id| people.get(id).cloned())
        .collect()
}

/// The show on its own.
pub fn resolve_show(show: &Show, people: &HashMap<Uuid, String>) -> Resolved {
    Resolved {
        show: show.name.clone(),
        slug: show.slug.clone(),
        description: show.description.clone(),
        colour_slot: show.colour_slot,
        layout: show.layout.clone(),
        slot: show.slot,
        hosts: names(&show.hosts, people),
        guests: Vec::new(),
        tags: show.tags.clone(),
        explicit: show.explicit,
        series: None,
        episode: None,
        inherited: Vec::new(),
        frozen: false,
    }
}

/// The dates a series runs: from its first episode (or its start date) to its last planned
/// one, counted on the rule when the plan is known, or to its latest episode.
pub fn series_run(
    series: &Series,
    slot: Option<Slot>,
    zone: &TimeZone,
    episodes: &[Episode],
) -> (Option<Date>, Option<Date>) {
    let date = |t: i64| {
        Timestamp::from_second(t)
            .ok()
            .map(|ts| ts.to_zoned(zone.clone()).date())
    };
    let aired: Vec<i64> = episodes.iter().filter_map(|e| e.air_at).collect();
    let first = series
        .starts_on
        .or_else(|| aired.iter().min().and_then(|t| date(*t)));
    let planned_last = match (first, slot, series.planned_episodes) {
        (Some(first), Some(slot), Some(n)) if n > 0 => {
            let start = first
                .at(0, 0, 0, 0)
                .to_zoned(zone.clone())
                .ok()
                .map(|z| z.timestamp() - 1.second());
            let mut t = start;
            for _ in 0..n {
                t = t.and_then(|after| slot.next_after(zone, after));
            }
            t.map(|ts| ts.to_zoned(zone.clone()).date())
        }
        _ => None,
    };
    let latest = aired.iter().max().and_then(|t| date(*t));
    let last = match (planned_last, latest) {
        (Some(p), Some(l)) => Some(p.max(l)),
        (p, l) => p.or(l),
    };
    (first, last)
}

/// A series with what it inherits from its show.
pub fn resolve_series(
    show: &Show,
    series: &Series,
    people: &HashMap<Uuid, String>,
    zone: &TimeZone,
    episodes: &[Episode],
) -> Resolved {
    let mut r = resolve_show(show, people);
    let mut inherited = Vec::new();
    match &series.hosts {
        Some(h) => r.hosts = names(h, people),
        None => inherited.push("hosts".to_owned()),
    }
    match series.slot {
        Some(s) => r.slot = Some(s),
        None => inherited.push("slot".to_owned()),
    }
    match &series.tags {
        Some(t) => r.tags = t.clone(),
        None => inherited.push("tags".to_owned()),
    }
    match &series.description {
        Some(d) => r.description = d.clone(),
        None => inherited.push("description".to_owned()),
    }
    let (first, last) = series_run(series, r.slot, zone, episodes);
    r.series = Some(ResolvedSeries {
        number: series.number,
        title: series.title.clone(),
        first,
        last,
        episodes: series
            .planned_episodes
            .or_else(|| (!episodes.is_empty()).then_some(episodes.len() as u32)),
    });
    r.inherited = inherited;
    r
}

/// An episode with what it inherits; an aired episode is its frozen copy.
pub fn resolve_episode(
    show: &Show,
    series: &Series,
    episode: &Episode,
    people: &HashMap<Uuid, String>,
    zone: &TimeZone,
    siblings: &[Episode],
) -> Resolved {
    if let Some(frozen) = &episode.frozen {
        return frozen.clone();
    }
    let mut r = resolve_series(show, series, people, zone, siblings);
    let mut inherited = r.inherited.clone();
    match &episode.hosts {
        Some(h) => {
            r.hosts = names(h, people);
            inherited.retain(|f| f != "hosts");
        }
        None => {
            if !inherited.iter().any(|f| f == "hosts") {
                inherited.push("hosts".to_owned());
            }
        }
    }
    match &episode.tags {
        Some(t) => {
            r.tags = t.clone();
            inherited.retain(|f| f != "tags");
        }
        None => {
            if !inherited.iter().any(|f| f == "tags") {
                inherited.push("tags".to_owned());
            }
        }
    }
    if episode.minutes.is_none() {
        inherited.push("minutes".to_owned());
    }
    r.guests = names(&episode.guests, people);
    r.episode = Some(ResolvedEpisode {
        id: episode.id,
        number: episode.number,
        title: episode.title.clone(),
        description: episode.description.clone(),
        kind: episode.kind,
        air_at: episode.air_at,
        minutes: episode.minutes.or_else(|| r.slot.and_then(|s| s.minutes)),
    });
    r.inherited = inherited;
    r
}

/// The number and air time the next `n` episodes take: the last number plus one, and the next
/// slots of the rule after the latest episode (or from the series' start date, or now).
pub fn fill_next(
    slot: Option<Slot>,
    zone: &TimeZone,
    starts_on: Option<Date>,
    existing: &[Episode],
    n: usize,
    now: Timestamp,
) -> Vec<(u32, Option<i64>)> {
    let mut number = existing.iter().map(|e| e.number).max().unwrap_or(0);
    let mut after = existing
        .iter()
        .filter_map(|e| e.air_at)
        .max()
        .and_then(|t| Timestamp::from_second(t).ok());
    if after.is_none() {
        after = starts_on
            .and_then(|d| d.at(0, 0, 0, 0).to_zoned(zone.clone()).ok())
            .map(|z| z.timestamp() - 1.second())
            .or(Some(now));
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        number += 1;
        let at = match (slot, after) {
            (Some(s), Some(a)) => s.next_after(zone, a),
            _ => None,
        };
        if at.is_some() {
            after = at;
        }
        out.push((number, at.map(|t| t.as_second())));
    }
    out
}

// The database.

macro_rules! show_columns {
    () => {
        "id, slug, name, description, colour_slot, layout, slot_weekday, slot_start, slot_minutes, hosts, tags, explicit, feed_guid, cards::text AS cards"
    };
}

fn slot_of(row: &sqlx::postgres::PgRow) -> Result<Option<Slot>, sqlx::Error> {
    let weekday: Option<i16> = row.try_get("slot_weekday")?;
    let start: Option<i16> = row.try_get("slot_start")?;
    let minutes: Option<i16> = row.try_get("slot_minutes")?;
    Ok(match (weekday, start) {
        (Some(w), Some(s)) => Some(Slot {
            weekday: w as u8,
            start: s as u16,
            minutes: minutes.map(|m| m as u16),
        }),
        _ => None,
    })
}

fn json_of(
    row: &sqlx::postgres::PgRow,
    col: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let raw: Option<String> = row.try_get(col)?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

fn show_of(row: &sqlx::postgres::PgRow) -> Result<Show, sqlx::Error> {
    Ok(Show {
        id: row.try_get("id")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        colour_slot: row.try_get::<i16, _>("colour_slot")? as u8,
        layout: row.try_get("layout")?,
        slot: slot_of(row)?,
        hosts: row.try_get("hosts")?,
        tags: row.try_get("tags")?,
        explicit: row.try_get("explicit")?,
        feed_guid: row.try_get("feed_guid")?,
        cards: json_of(row, "cards")?,
    })
}

/// The people with these names, made if new: the remembered guest list.
pub async fn people_ids(
    pool: &sqlx::PgPool,
    station: StationId,
    names: &[String],
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO person (id, station_id, name) VALUES ($1, $2, $3)
             ON CONFLICT (station_id, name) DO UPDATE SET name = EXCLUDED.name
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(station.as_uuid())
        .bind(n.trim())
        .fetch_one(pool)
        .await?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Every person the station remembers, by identifier.
pub async fn people(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<HashMap<Uuid, String>, sqlx::Error> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM person WHERE station_id = $1")
            .bind(station.as_uuid())
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// Make or change a show by its path segment.
pub async fn put_show(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    input: &ShowInput,
) -> Result<Show, ShowError> {
    if !is_slug(slug) {
        return Err(ShowError::Invalid(format!(
            "`{slug}` is not a show path: lower-case letters and digits, joined by hyphens"
        )));
    }
    input.check()?;
    let hosts = people_ids(pool, station, &input.hosts).await?;
    let slot = input.slot;
    let row = sqlx::query(concat!(
        "INSERT INTO show (id, station_id, slug, name, description, colour_slot, layout,
                           slot_weekday, slot_start, slot_minutes, hosts, tags, explicit)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT (station_id, slug) DO UPDATE SET
           name = EXCLUDED.name, description = EXCLUDED.description,
           colour_slot = EXCLUDED.colour_slot, layout = EXCLUDED.layout,
           slot_weekday = EXCLUDED.slot_weekday, slot_start = EXCLUDED.slot_start,
           slot_minutes = EXCLUDED.slot_minutes, hosts = EXCLUDED.hosts, tags = EXCLUDED.tags,
           explicit = EXCLUDED.explicit, updated_at = now()
         RETURNING ",
        show_columns!()
    ))
    .bind(Uuid::new_v4())
    .bind(station.as_uuid())
    .bind(slug)
    .bind(input.name.trim())
    .bind(&input.description)
    .bind(i16::from(input.colour_slot))
    .bind(&input.layout)
    .bind(slot.map(|s| i16::from(s.weekday)))
    .bind(slot.map(|s| s.start as i16))
    .bind(slot.and_then(|s| s.minutes).map(|m| m as i16))
    .bind(&hosts)
    .bind(&input.tags)
    .bind(input.explicit)
    .fetch_one(pool)
    .await?;
    Ok(show_of(&row)?)
}

pub async fn get_show(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
) -> Result<Option<Show>, sqlx::Error> {
    let row = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE station_id = $1 AND slug = $2"
    ))
    .bind(station.as_uuid())
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(show_of).transpose()
}

pub async fn list_shows(pool: &sqlx::PgPool, station: StationId) -> Result<Vec<Show>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE station_id = $1 ORDER BY name"
    ))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    rows.iter().map(show_of).collect()
}

/// Keep the show's `podcast:guid` the first time its feed is made; afterwards it never
/// changes, even if the feed's address does.
pub async fn keep_feed_guid(
    pool: &sqlx::PgPool,
    show: Uuid,
    guid: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE show SET feed_guid = COALESCE(feed_guid, $2) WHERE id = $1 RETURNING feed_guid",
    )
    .bind(show)
    .bind(guid)
    .fetch_one(pool)
    .await
}

macro_rules! series_columns {
    () => {
        "id, show_id, number, title, description, hosts, slot_weekday, slot_start, slot_minutes, starts_on, planned_episodes, tags, cards::text AS cards"
    };
}

fn series_of(row: &sqlx::postgres::PgRow) -> Result<Series, sqlx::Error> {
    let starts: Option<String> = row.try_get("starts_on")?;
    Ok(Series {
        id: row.try_get("id")?,
        show_id: row.try_get("show_id")?,
        number: row.try_get::<i32, _>("number")? as u32,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        hosts: row.try_get("hosts")?,
        slot: slot_of(row)?,
        starts_on: starts.and_then(|s| s.parse().ok()),
        planned_episodes: row
            .try_get::<Option<i32>, _>("planned_episodes")?
            .map(|n| n as u32),
        tags: row.try_get("tags")?,
        cards: json_of(row, "cards")?,
    })
}

/// Make or change a series; its fields replace the stored ones (`None` inherits).
pub async fn put_series(
    pool: &sqlx::PgPool,
    station: StationId,
    show: &Show,
    number: u32,
    input: &SeriesInput,
) -> Result<Series, ShowError> {
    if !(1..=9999).contains(&number) {
        return Err(ShowError::Invalid("a series is numbered 1 to 9999".into()));
    }
    input.check()?;
    let hosts = match &input.hosts {
        Some(h) => Some(people_ids(pool, station, h).await?),
        None => None,
    };
    let slot = input.slot;
    let row = sqlx::query(concat!(
        "INSERT INTO series (id, station_id, show_id, number, title, description, hosts,
                             slot_weekday, slot_start, slot_minutes, starts_on,
                             planned_episodes, tags)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT (show_id, number) DO UPDATE SET
           title = EXCLUDED.title, description = EXCLUDED.description, hosts = EXCLUDED.hosts,
           slot_weekday = EXCLUDED.slot_weekday, slot_start = EXCLUDED.slot_start,
           slot_minutes = EXCLUDED.slot_minutes, starts_on = EXCLUDED.starts_on,
           planned_episodes = EXCLUDED.planned_episodes, tags = EXCLUDED.tags,
           updated_at = now()
         RETURNING ",
        series_columns!()
    ))
    .bind(Uuid::new_v4())
    .bind(station.as_uuid())
    .bind(show.id)
    .bind(number as i32)
    .bind(input.title.as_deref().map(str::trim))
    .bind(&input.description)
    .bind(&hosts)
    .bind(slot.map(|s| i16::from(s.weekday)))
    .bind(slot.map(|s| s.start as i16))
    .bind(slot.and_then(|s| s.minutes).map(|m| m as i16))
    .bind(input.starts_on.map(|d| d.to_string()))
    .bind(input.planned_episodes.map(|n| n as i32))
    .bind(&input.tags)
    .fetch_one(pool)
    .await?;
    Ok(series_of(&row)?)
}

pub async fn get_series(
    pool: &sqlx::PgPool,
    show: Uuid,
    number: u32,
) -> Result<Option<Series>, sqlx::Error> {
    let row = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE show_id = $1 AND number = $2"
    ))
    .bind(show)
    .bind(number as i32)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(series_of).transpose()
}

pub async fn list_series(pool: &sqlx::PgPool, show: Uuid) -> Result<Vec<Series>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE show_id = $1 ORDER BY number"
    ))
    .bind(show)
    .fetch_all(pool)
    .await?;
    rows.iter().map(series_of).collect()
}

macro_rules! episode_columns {
    () => {
        "id, series_id, number, title, description, kind, extract(epoch FROM air_at)::bigint AS air_at, minutes, hosts, guests, tags, extract(epoch FROM aired_at)::bigint AS aired_at, frozen::text AS frozen, files::text AS files, cards::text AS cards"
    };
}

fn episode_of(row: &sqlx::postgres::PgRow) -> Result<Episode, sqlx::Error> {
    let frozen: Option<String> = row.try_get("frozen")?;
    Ok(Episode {
        id: row.try_get("id")?,
        series_id: row.try_get("series_id")?,
        number: row.try_get::<i32, _>("number")? as u32,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        kind: EpisodeKind::parse(&row.try_get::<String, _>("kind")?).unwrap_or_default(),
        air_at: row.try_get("air_at")?,
        minutes: row.try_get::<Option<i16>, _>("minutes")?.map(|m| m as u16),
        hosts: row.try_get("hosts")?,
        guests: row.try_get("guests")?,
        tags: row.try_get("tags")?,
        aired_at: row.try_get("aired_at")?,
        frozen: frozen.and_then(|f| serde_json::from_str(&f).ok()),
        files: json_of(row, "files")?,
        cards: json_of(row, "cards")?,
    })
}

pub async fn list_episodes(pool: &sqlx::PgPool, series: Uuid) -> Result<Vec<Episode>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE series_id = $1 ORDER BY number"
    ))
    .bind(series)
    .fetch_all(pool)
    .await?;
    rows.iter().map(episode_of).collect()
}

pub async fn get_episode(
    pool: &sqlx::PgPool,
    series: Uuid,
    number: u32,
) -> Result<Option<Episode>, sqlx::Error> {
    let row = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE series_id = $1 AND number = $2"
    ))
    .bind(series)
    .bind(number as i32)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(episode_of).transpose()
}

/// Every episode of the station airing in `[from_s, to_s)` (epoch seconds), with its series
/// and show, in air order: what the schedule calendar draws for a week.
pub async fn episodes_between(
    pool: &sqlx::PgPool,
    station: StationId,
    from_s: i64,
    to_s: i64,
) -> Result<Vec<(Show, Series, Episode)>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE station_id = $1 AND air_at >= to_timestamp($2) AND air_at < to_timestamp($3) ORDER BY air_at, number"
    ))
    .bind(station.as_uuid())
    .bind(from_s as f64)
    .bind(to_s as f64)
    .fetch_all(pool)
    .await?;
    let episodes: Vec<Episode> = rows.iter().map(episode_of).collect::<Result<_, _>>()?;
    let series_ids: Vec<Uuid> = episodes.iter().map(|e| e.series_id).collect();
    let series_rows = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE id = ANY($1)"
    ))
    .bind(&series_ids)
    .fetch_all(pool)
    .await?;
    let series: HashMap<Uuid, Series> = series_rows
        .iter()
        .map(|r| series_of(r).map(|s| (s.id, s)))
        .collect::<Result<_, _>>()?;
    let shows: HashMap<Uuid, Show> = list_shows(pool, station)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    Ok(episodes
        .into_iter()
        .filter_map(|e| {
            let s = series.get(&e.series_id)?.clone();
            let sh = shows.get(&s.show_id)?.clone();
            Some((sh, s, e))
        })
        .collect())
}

/// The new episodes' own fields; the rest is filled in or inherited.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewEpisodes {
    /// How many to draft from the rule: one, or a whole season ("create 10 episodes").
    #[serde(default = "one")]
    pub count: usize,
    /// The first one's title and guests; the rest start blank.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub guests: Vec<String>,
}

fn one() -> usize {
    1
}

/// Who made an episode (ADR-0198): how the logs name them, and their staff account if they
/// have one (the station key and API tokens have none).
#[derive(Clone, Copy, Debug)]
pub struct Maker<'a> {
    pub label: &'a str,
    pub account: Option<Uuid>,
}

/// The staff account that made an episode, if one did and still exists (ADR-0198). `None`
/// for an episode made by the station key or a token, or before makers were recorded.
pub async fn episode_maker(pool: &sqlx::PgPool, episode: Uuid) -> Result<Option<Uuid>, ShowError> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT made_by_account FROM episode WHERE id = $1")
            .bind(episode)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(a,)| a))
}

/// Draft the next episodes of a series, each filled in from the rule and inheriting the rest.
/// `maker` is kept on each (ADR-0198).
#[allow(clippy::too_many_arguments)] // the station, the series' place and time, and who drafts
pub async fn create_episodes(
    pool: &sqlx::PgPool,
    station: StationId,
    show: &Show,
    series: &Series,
    zone: &TimeZone,
    new: &NewEpisodes,
    now: Timestamp,
    maker: Maker<'_>,
) -> Result<Vec<Episode>, ShowError> {
    if new.count == 0 || new.count > 52 {
        return Err(ShowError::Invalid(
            "draft 1 to 52 episodes at a time".into(),
        ));
    }
    text("episode title", new.title.trim(), 0, 200)?;
    check_people(&new.guests)?;
    let guests = people_ids(pool, station, &new.guests).await?;
    let mut tx = pool.begin().await?;
    // One drafter at a time per series: the numbers and slots follow the last one.
    sqlx::query("SELECT id FROM series WHERE id = $1 FOR UPDATE")
        .bind(series.id)
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE series_id = $1"
    ))
    .bind(series.id)
    .fetch_all(&mut *tx)
    .await?;
    let existing: Vec<Episode> = rows.iter().map(episode_of).collect::<Result<_, _>>()?;
    let slot = series.slot.or(show.slot);
    let mut ids = Vec::new();
    for (i, (number, air)) in fill_next(slot, zone, series.starts_on, &existing, new.count, now)
        .into_iter()
        .enumerate()
    {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO episode (id, station_id, series_id, number, title, guests, air_at,
                                  made_by, made_by_account)
             VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision), $8, $9)",
        )
        .bind(id)
        .bind(station.as_uuid())
        .bind(series.id)
        .bind(number as i32)
        .bind(if i == 0 { new.title.trim() } else { "" })
        .bind(if i == 0 { guests.clone() } else { Vec::new() })
        .bind(air.map(|t| t as f64))
        .bind(maker.label)
        .bind(maker.account)
        .execute(&mut *tx)
        .await?;
        ids.push(number);
    }
    tx.commit().await?;
    let mut out = Vec::new();
    for n in ids {
        if let Some(e) = get_episode(pool, series.id, n).await? {
            out.push(e);
        }
    }
    Ok(out)
}

/// Change an episode that has not aired.
pub async fn patch_episode(
    pool: &sqlx::PgPool,
    station: StationId,
    episode: &Episode,
    patch: &EpisodePatch,
) -> Result<Episode, ShowError> {
    if episode.aired_at.is_some() {
        return Err(ShowError::Frozen(format!("episode {}", episode.number)));
    }
    patch.check()?;
    let hosts = match &patch.hosts {
        Some(Some(h)) => Some(Some(people_ids(pool, station, h).await?)),
        Some(None) => Some(None),
        None => None,
    };
    let guests = match &patch.guests {
        Some(g) => Some(people_ids(pool, station, g).await?),
        None => None,
    };
    let row = sqlx::query(concat!(
        "UPDATE episode SET
           title = COALESCE($2, title),
           description = COALESCE($3, description),
           air_at = CASE WHEN $4 THEN to_timestamp($5::double precision) ELSE air_at END,
           minutes = CASE WHEN $6 THEN $7 ELSE minutes END,
           hosts = CASE WHEN $8 THEN $9 ELSE hosts END,
           guests = COALESCE($10, guests),
           tags = CASE WHEN $11 THEN $12 ELSE tags END,
           kind = COALESCE($13, kind),
           updated_at = now()
         WHERE id = $1 AND aired_at IS NULL
         RETURNING ",
        episode_columns!()
    ))
    .bind(episode.id)
    .bind(patch.title.as_deref().map(str::trim))
    .bind(&patch.description)
    .bind(patch.air_at.is_some())
    .bind(
        patch
            .air_at
            .as_ref()
            .and_then(|a| a.as_ref())
            .map(|z| z.timestamp().as_second() as f64),
    )
    .bind(patch.minutes.is_some())
    .bind(patch.minutes.flatten().map(|m| m as i16))
    .bind(hosts.is_some())
    .bind(hosts.flatten())
    .bind(guests)
    .bind(patch.tags.is_some())
    .bind(patch.tags.clone().flatten())
    .bind(patch.kind.map(EpisodeKind::as_str))
    .fetch_optional(pool)
    .await?;
    match row {
        Some(r) => Ok(episode_of(&r)?),
        None => Err(ShowError::Frozen(format!("episode {}", episode.number))),
    }
}

/// Put an episode on air: its resolved values are written into it and it is frozen.
/// Returns `false` if it had already aired (the first freeze stands).
pub async fn freeze_episode(
    pool: &sqlx::PgPool,
    episode: Uuid,
    resolved: &Resolved,
) -> Result<bool, ShowError> {
    let mut frozen = resolved.clone();
    frozen.frozen = true;
    frozen.inherited.clear();
    let json = serde_json::to_string(&frozen).map_err(|e| ShowError::Invalid(e.to_string()))?;
    let n = sqlx::query(
        "UPDATE episode SET aired_at = now(), frozen = $2::jsonb, updated_at = now()
         WHERE id = $1 AND aired_at IS NULL",
    )
    .bind(episode)
    .bind(json)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// A correction to the public text of an aired episode (ADR-0177). Fields left out are not
/// changed. What was broadcast (the air time, the length, the kind, the colour and layout) is
/// not text and is not corrected; the frozen record under it stays.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrectionInput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hosts: Option<Vec<String>>,
    #[serde(default)]
    pub guests: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Why, in a sentence: the log keeps it beside who made the correction.
    pub reason: String,
}

impl CorrectionInput {
    pub fn check(&self) -> Result<(), ShowError> {
        text("reason", self.reason.trim(), 3, 500)?;
        if let Some(t) = &self.title {
            text("episode title", t.trim(), 0, 200)?;
        }
        if let Some(d) = &self.description {
            text("description", d, 0, 4000)?;
        }
        if let Some(h) = &self.hosts {
            check_people(h)?;
        }
        if let Some(g) = &self.guests {
            check_people(g)?;
        }
        if let Some(t) = &self.tags {
            check_tags(t)?;
        }
        Ok(())
    }

    /// What the aired episode would say once corrected: `frozen` with the changed fields put
    /// in. The control plane draws the cards from this before it keeps anything.
    pub fn applied(&self, frozen: &Resolved) -> Resolved {
        let mut r = frozen.clone();
        if let Some(t) = &self.title
            && let Some(e) = r.episode.as_mut()
        {
            e.title = t.trim().to_owned();
        }
        if let Some(d) = &self.description
            && let Some(e) = r.episode.as_mut()
        {
            e.description = d.clone();
        }
        if let Some(h) = &self.hosts {
            r.hosts = clean_names(h);
        }
        if let Some(g) = &self.guests {
            r.guests = clean_names(g);
        }
        if let Some(t) = &self.tags {
            r.tags = t.iter().map(|t| t.trim().to_owned()).collect();
        }
        r
    }
}

/// Names trimmed and each once, in order.
fn clean_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(names.len());
    for n in names {
        let n = n.trim().to_owned();
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// One line of the corrections log. `old_values` and `new_values` hold the fields that
/// changed, as they were and as they became.
#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Correction {
    pub id: Uuid,
    pub episode_id: Uuid,
    /// Seconds since the Unix epoch.
    pub made_at: i64,
    /// How the log names who made it: a username, or "station key".
    pub made_by: String,
    pub reason: String,
    pub old_values: serde_json::Value,
    pub new_values: serde_json::Value,
    /// An erasure (ADR-0122) took a person's name out of this line.
    pub redacted: bool,
}

macro_rules! correction_columns {
    () => {
        "id, episode_id, extract(epoch FROM made_at)::bigint AS made_at, made_by, reason, old_values::text AS old_values, new_values::text AS new_values, redacted_at IS NOT NULL AS redacted"
    };
}

fn correction_of(row: &sqlx::postgres::PgRow) -> Result<Correction, sqlx::Error> {
    let parse = |col: &str| -> Result<serde_json::Value, sqlx::Error> {
        let t: String = row.try_get(col)?;
        serde_json::from_str(&t).map_err(|e| sqlx::Error::Decode(Box::new(e)))
    };
    Ok(Correction {
        id: row.try_get("id")?,
        episode_id: row.try_get("episode_id")?,
        made_at: row.try_get("made_at")?,
        made_by: row.try_get("made_by")?,
        reason: row.try_get("reason")?,
        old_values: parse("old_values")?,
        new_values: parse("new_values")?,
        redacted: row.try_get("redacted")?,
    })
}

/// An episode's corrections, oldest first.
pub async fn list_corrections(
    pool: &sqlx::PgPool,
    episode: Uuid,
) -> Result<Vec<Correction>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        correction_columns!(),
        " FROM episode_correction c WHERE c.episode_id = $1 ORDER BY c.made_at, c.id"
    ))
    .bind(episode)
    .fetch_all(pool)
    .await?;
    rows.iter().map(correction_of).collect()
}

/// Correct the public text of an episode that has aired (ADR-0177). The frozen record is
/// updated in place, which is what the cards and the feed read, under the same episode id; the
/// fields as they were go in the log with who made the correction and why. A correction that
/// changes nothing is refused. `by` names the actor in the log, and `account` is their staff
/// account when they have one. `cards`, the cards already drawn and kept from the corrected
/// text, are pointed at in the same transaction, so the record and its cards never disagree.
pub async fn correct_episode(
    pool: &sqlx::PgPool,
    station: StationId,
    episode: Uuid,
    input: &CorrectionInput,
    by: &str,
    account: Option<Uuid>,
    cards: Option<&serde_json::Value>,
) -> Result<Correction, ShowError> {
    input.check()?;
    // People are remembered by name, outside the transaction as everywhere else.
    let hosts = match &input.hosts {
        Some(h) => Some(people_ids(pool, station, &clean_names(h)).await?),
        None => None,
    };
    let guests = match &input.guests {
        Some(g) => Some(people_ids(pool, station, &clean_names(g)).await?),
        None => None,
    };
    let mut tx = pool.begin().await?;
    let row: Option<(Option<String>, bool, i32)> = sqlx::query_as(
        "SELECT frozen::text, aired_at IS NOT NULL, number FROM episode
         WHERE id = $1 AND station_id = $2 FOR UPDATE",
    )
    .bind(episode)
    .bind(station.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;
    let Some((frozen, aired, number)) = row else {
        return Err(ShowError::Missing("that episode".into()));
    };
    if !aired {
        return Err(ShowError::NotAired(format!("episode {number}")));
    }
    let before: Resolved = frozen
        .and_then(|f| serde_json::from_str(&f).ok())
        .ok_or_else(|| ShowError::Invalid(format!("episode {number} has no frozen record")))?;
    let after = input.applied(&before);
    let (mut old, mut new) = (serde_json::Map::new(), serde_json::Map::new());
    let mut note = |field: &str, was: serde_json::Value, now: serde_json::Value| {
        if was != now {
            old.insert(field.to_owned(), was);
            new.insert(field.to_owned(), now);
        }
    };
    let (bt, at) = (before.episode.as_ref(), after.episode.as_ref());
    if input.title.is_some() {
        note(
            "title",
            serde_json::json!(bt.map(|e| e.title.as_str())),
            serde_json::json!(at.map(|e| e.title.as_str())),
        );
    }
    if input.description.is_some() {
        note(
            "description",
            serde_json::json!(bt.map(|e| e.description.as_str())),
            serde_json::json!(at.map(|e| e.description.as_str())),
        );
    }
    if input.hosts.is_some() {
        note(
            "hosts",
            serde_json::json!(before.hosts),
            serde_json::json!(after.hosts),
        );
    }
    if input.guests.is_some() {
        note(
            "guests",
            serde_json::json!(before.guests),
            serde_json::json!(after.guests),
        );
    }
    if input.tags.is_some() {
        note(
            "tags",
            serde_json::json!(before.tags),
            serde_json::json!(after.tags),
        );
    }
    if old.is_empty() {
        return Err(ShowError::Invalid(
            "the correction changes nothing: change a field, or leave the episode as aired".into(),
        ));
    }
    let json = serde_json::to_string(&after).map_err(|e| ShowError::Invalid(e.to_string()))?;
    let title = after.episode.as_ref().map(|e| e.title.clone());
    let description = after.episode.as_ref().map(|e| e.description.clone());
    // The columns follow the frozen copy, so the schedule and the editor read what the feed
    // says. A host or tag list left out stays as it is.
    sqlx::query(
        "UPDATE episode SET frozen = $2::jsonb,
                title = COALESCE($3, title),
                description = COALESCE($4, description),
                hosts = CASE WHEN $5 THEN $6 ELSE hosts END,
                guests = COALESCE($7, guests),
                tags = CASE WHEN $8 THEN $9 ELSE tags END,
                cards = COALESCE($10::jsonb, cards),
                updated_at = now()
         WHERE id = $1",
    )
    .bind(episode)
    .bind(json)
    .bind(input.title.is_some().then_some(title).flatten())
    .bind(input.description.is_some().then_some(description).flatten())
    .bind(hosts.is_some())
    .bind(hosts)
    .bind(guests)
    .bind(input.tags.is_some())
    .bind(input.tags.as_ref().map(|_| after.tags.clone()))
    .bind(cards.map(serde_json::Value::to_string))
    .execute(&mut *tx)
    .await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO episode_correction
             (id, station_id, episode_id, made_by, made_by_account, reason, old_values, new_values)
         VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb)",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(episode)
    .bind(by)
    .bind(account)
    .bind(input.reason.trim())
    .bind(serde_json::Value::Object(old).to_string())
    .bind(serde_json::Value::Object(new).to_string())
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query(concat!(
        "SELECT ",
        correction_columns!(),
        " FROM episode_correction WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(correction_of(&row)?)
}

/// `value` with a person's name taken out: a list drops the entry, a string loses the name
/// wherever it stands (any case) and keeps the rest. Reports whether anything changed.
fn redact(value: &mut serde_json::Value, name: &str) -> bool {
    match value {
        serde_json::Value::Array(items) => {
            let before = items.len();
            let lower = name.to_lowercase();
            items.retain(|v| v.as_str().is_none_or(|s| s.to_lowercase() != lower));
            let mut changed = items.len() != before;
            for v in items {
                changed |= redact(v, name);
            }
            changed
        }
        serde_json::Value::Object(map) => map.values_mut().fold(false, |c, v| redact(v, name) | c),
        serde_json::Value::String(s) => {
            let out = replace_name(s, name);
            let changed = out != *s;
            *s = out;
            changed
        }
        _ => false,
    }
}

/// `text` with every occurrence of `name`, in any case, replaced by "[erased]".
fn replace_name(text: &str, name: &str) -> String {
    let wanted: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    if wanted.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        // The longest run of characters from `i` whose lower-case forms spell the name.
        let mut matched = 0;
        let mut end = i;
        for (off, c) in text[i..].char_indices() {
            let mut low = c.to_lowercase();
            let all = low.all(|l| {
                let ok = wanted.get(matched) == Some(&l);
                matched += 1;
                ok
            });
            if !all {
                matched = 0;
                break;
            }
            end = i + off + c.len_utf8();
            if matched == wanted.len() {
                break;
            }
        }
        if matched == wanted.len() {
            out.push_str("[erased]");
            i = end;
        } else {
            let c = text[i..].chars().next().expect("a character at i");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Which level a set of cards belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Show,
    Series,
    Episode,
}

/// Point a show, series or episode at its current cards.
pub async fn set_cards(
    pool: &sqlx::PgPool,
    level: Level,
    id: Uuid,
    cards: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let sql = match level {
        Level::Show => "UPDATE show SET cards = $2::jsonb WHERE id = $1",
        Level::Series => "UPDATE series SET cards = $2::jsonb WHERE id = $1",
        Level::Episode => "UPDATE episode SET cards = $2::jsonb WHERE id = $1",
    };
    sqlx::query(sql)
        .bind(id)
        .bind(cards.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Record an episode's published files (ADR-0018).
pub async fn set_files(
    pool: &sqlx::PgPool,
    episode: Uuid,
    files: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE episode SET files = $2::jsonb, updated_at = now() WHERE id = $1")
        .bind(episode)
        .bind(files.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// The episodes of a show that have aired and have files: its feed's items, newest first.
pub async fn published_episodes(
    pool: &sqlx::PgPool,
    show: Uuid,
) -> Result<Vec<(Series, Episode)>, sqlx::Error> {
    let series = list_series(pool, show).await?;
    let mut out = Vec::new();
    for s in series {
        for e in list_episodes(pool, s.id).await? {
            if e.aired_at.is_some() && e.files.is_some() {
                out.push((s.clone(), e));
            }
        }
    }
    out.sort_by_key(|(_, e)| std::cmp::Reverse(e.air_at.or(e.aired_at)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn london() -> TimeZone {
        TimeZone::get("Europe/London").unwrap()
    }

    fn saturday_eight() -> Slot {
        Slot {
            weekday: 6,
            start: 20 * 60,
            minutes: Some(180),
        }
    }

    fn ts(s: &str) -> Timestamp {
        s.parse::<Zoned>().unwrap().timestamp()
    }

    fn episode(number: u32, air: Option<&str>) -> Episode {
        Episode {
            id: Uuid::new_v4(),
            series_id: Uuid::nil(),
            number,
            title: String::new(),
            description: String::new(),
            kind: EpisodeKind::Live,
            air_at: air.map(|a| ts(a).as_second()),
            minutes: None,
            hosts: None,
            guests: vec![],
            tags: None,
            aired_at: None,
            frozen: None,
            files: None,
            cards: None,
        }
    }

    #[test]
    fn a_season_fills_in_from_its_start_date_across_the_clock_change() {
        let starts = Some("2026-09-05".parse().unwrap());
        let got = fill_next(
            Some(saturday_eight()),
            &london(),
            starts,
            &[],
            10,
            ts("2026-08-01T12:00[Europe/London]"),
        );
        assert_eq!(got.len(), 10);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[9].0, 10);
        let local = |t: i64| {
            Timestamp::from_second(t)
                .unwrap()
                .to_zoned(london())
                .strftime("%Y-%m-%d %H:%M %Z")
                .to_string()
        };
        assert_eq!(local(got[0].1.unwrap()), "2026-09-05 20:00 BST");
        assert_eq!(local(got[6].1.unwrap()), "2026-10-17 20:00 BST");
        assert_eq!(local(got[7].1.unwrap()), "2026-10-24 20:00 BST");
        // The clocks go back on 25 October: still eight o'clock, now GMT.
        assert_eq!(local(got[8].1.unwrap()), "2026-10-31 20:00 GMT");
        assert_eq!(local(got[9].1.unwrap()), "2026-11-07 20:00 GMT");
    }

    #[test]
    fn the_next_episode_follows_the_last() {
        let existing = vec![
            episode(6, Some("2026-10-17T20:00[Europe/London]")),
            episode(5, Some("2026-10-10T20:00[Europe/London]")),
        ];
        let got = fill_next(
            Some(saturday_eight()),
            &london(),
            None,
            &existing,
            1,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(got[0].0, 7);
        assert_eq!(
            got[0].1,
            Some(ts("2026-10-24T20:00[Europe/London]").as_second())
        );
        // No rule: numbered, not scheduled.
        assert_eq!(
            fill_next(None, &london(), None, &existing, 1, Timestamp::now())[0],
            (7, None)
        );
    }

    fn show() -> Show {
        Show {
            id: Uuid::new_v4(),
            slug: "saturday-night".into(),
            name: "Saturday Night Kettle".into(),
            description: "Live music.".into(),
            colour_slot: 3,
            layout: "poster".into(),
            slot: Some(saturday_eight()),
            hosts: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
            tags: vec!["music".into()],
            explicit: false,
            feed_guid: None,
            cards: None,
        }
    }

    fn series(show: &Show) -> Series {
        Series {
            id: Uuid::new_v4(),
            show_id: show.id,
            number: 3,
            title: None,
            description: None,
            hosts: None,
            slot: None,
            starts_on: Some("2026-09-05".parse().unwrap()),
            planned_episodes: Some(10),
            tags: None,
            cards: None,
        }
    }

    fn people() -> HashMap<Uuid, String> {
        [
            (Uuid::from_u128(1), "Shona Campbell".to_owned()),
            (Uuid::from_u128(2), "Catriona MacKenzie-Buchanan".to_owned()),
            (Uuid::from_u128(3), "Hamish Wallace".to_owned()),
            (Uuid::from_u128(4), "Firth Lights".to_owned()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn an_episode_inherits_until_set_and_follows_its_parent_by_reference() {
        let show = show();
        let mut series = series(&show);
        let mut e = episode(7, Some("2026-10-24T20:00[Europe/London]"));
        e.guests = vec![Uuid::from_u128(4)];
        let r = resolve_episode(&show, &series, &e, &people(), &london(), &[]);
        assert_eq!(r.hosts, ["Shona Campbell", "Catriona MacKenzie-Buchanan"]);
        assert!(r.inherited.contains(&"hosts".to_owned()));
        assert_eq!(r.guests, ["Firth Lights"]);
        assert_eq!(
            r.episode.as_ref().unwrap().minutes,
            Some(180),
            "the slot's length"
        );
        let s = r.series.unwrap();
        assert_eq!(
            (s.first, s.last),
            (
                Some("2026-09-05".parse().unwrap()),
                Some("2026-11-07".parse().unwrap())
            )
        );

        // The series changes its hosts: the inheriting episode follows.
        series.hosts = Some(vec![Uuid::from_u128(3)]);
        let r = resolve_episode(&show, &series, &e, &people(), &london(), &[]);
        assert_eq!(r.hosts, ["Hamish Wallace"]);

        // Set on the episode: it stops inheriting.
        e.hosts = Some(vec![Uuid::from_u128(1)]);
        let r = resolve_episode(&show, &series, &e, &people(), &london(), &[]);
        assert_eq!(r.hosts, ["Shona Campbell"]);
        assert!(!r.inherited.contains(&"hosts".to_owned()));
    }

    #[test]
    fn what_has_aired_is_frozen() {
        let show = show();
        let mut series = series(&show);
        let mut e = episode(7, Some("2026-10-24T20:00[Europe/London]"));
        let mut frozen = resolve_episode(&show, &series, &e, &people(), &london(), &[]);
        frozen.frozen = true;
        e.frozen = Some(frozen.clone());
        e.aired_at = Some(0);
        series.hosts = Some(vec![Uuid::from_u128(3)]);
        let r = resolve_episode(&show, &series, &e, &people(), &london(), &[]);
        assert_eq!(r, frozen, "later changes do not rewrite what was broadcast");
        assert_eq!(r.hosts, ["Shona Campbell", "Catriona MacKenzie-Buchanan"]);
    }

    #[test]
    fn a_patch_tells_reset_from_left_out() {
        let p: EpisodePatch =
            serde_json::from_str(r#"{"hosts": null, "title": "Ghost Train night"}"#).unwrap();
        assert_eq!(p.hosts, Some(None), "null: back to inheriting");
        assert_eq!(p.tags, None, "left out: unchanged");
        assert_eq!(p.title.as_deref(), Some("Ghost Train night"));
        assert_eq!(p.kind, None);
        let p: EpisodePatch = serde_json::from_str(r#"{"kind": "prerecorded"}"#).unwrap();
        assert_eq!(p.kind, Some(EpisodeKind::Prerecorded));
    }

    #[test]
    fn an_erased_name_leaves_lists_and_text_in_any_case() {
        let mut v = serde_json::json!({
            "guests": ["Morag Ross", "Gregor Stewart"],
            "description": "With MORAG ross and, later, morag Ross again.",
            "title": "Ferries"
        });
        assert!(redact(&mut v, "Morag Ross"));
        assert_eq!(v["guests"], serde_json::json!(["Gregor Stewart"]));
        assert_eq!(
            v["description"],
            "With [erased] and, later, [erased] again."
        );
        assert_eq!(v["title"], "Ferries");
        assert!(!redact(&mut v, "Morag Ross"), "nothing left to take out");
        assert_eq!(
            replace_name("Rùnach and Rùnach", "rùnach"),
            "[erased] and [erased]"
        );
        assert_eq!(
            replace_name("Mor", "Morag Ross"),
            "Mor",
            "a part is not the name"
        );
    }

    #[test]
    fn slugs_are_path_segments() {
        assert!(is_slug("saturday-night"));
        assert!(is_slug("s3"));
        assert!(!is_slug("../etc"));
        assert!(!is_slug("Saturday"));
        assert!(!is_slug("a--b"));
        assert!(!is_slug("-a"));
    }
}

/// What erasing a person changed (ADR-0122).
#[derive(Clone, Debug, Default, PartialEq, Serialize, utoipa::ToSchema)]
pub struct ErasedPerson {
    /// The name that was erased.
    pub name: String,
    /// Shows, series and episodes whose people lists or frozen copies lost them: their cards
    /// are to be drawn again.
    pub shows: Vec<Uuid>,
    pub series: Vec<Uuid>,
    pub episodes: Vec<Uuid>,
    /// Episodes whose title or description still names them: free text, for a person to edit.
    pub mentions: Vec<Uuid>,
    /// Lines of the corrections log (ADR-0177) that held the name and were redacted.
    pub corrections_redacted: usize,
}

/// Erase a person (a guest or a host) at a listener's or guest's request (ADR-0122): their
/// record goes, they leave every show's, series' and episode's list, and their name leaves
/// what aired, frozen copies included (an erasure is the one change a frozen episode takes).
/// `None` when the station has no such person.
pub async fn erase_person(
    pool: &sqlx::PgPool,
    station: StationId,
    person: Uuid,
) -> Result<Option<ErasedPerson>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let Some((name,)): Option<(String,)> =
        sqlx::query_as("SELECT name FROM person WHERE station_id = $1 AND id = $2 FOR UPDATE")
            .bind(station.as_uuid())
            .bind(person)
            .fetch_optional(&mut *tx)
            .await?
    else {
        return Ok(None);
    };
    let shows: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE show SET hosts = array_remove(hosts, $2), updated_at = now()
         WHERE station_id = $1 AND $2 = ANY(hosts) RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(person)
    .fetch_all(&mut *tx)
    .await?;
    let series: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE series SET hosts = array_remove(hosts, $2), updated_at = now()
         WHERE station_id = $1 AND $2 = ANY(hosts) RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(person)
    .fetch_all(&mut *tx)
    .await?;
    let mut episodes: Vec<Uuid> = sqlx::query_as::<_, (Uuid,)>(
        "UPDATE episode SET guests = array_remove(guests, $2),
                hosts = CASE WHEN hosts IS NULL THEN NULL ELSE array_remove(hosts, $2) END,
                updated_at = now()
         WHERE station_id = $1 AND ($2 = ANY(guests) OR $2 = ANY(hosts)) RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(person)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect();
    // What aired: the frozen copies carry names, not ids.
    let frozen: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, frozen::text FROM episode
         WHERE station_id = $1 AND frozen IS NOT NULL
           AND (frozen->'guests' ? $2 OR frozen->'hosts' ? $2)",
    )
    .bind(station.as_uuid())
    .bind(&name)
    .fetch_all(&mut *tx)
    .await?;
    for (id, text) in frozen {
        let Ok(mut r) = serde_json::from_str::<Resolved>(&text) else {
            continue;
        };
        r.guests.retain(|n| n != &name);
        r.hosts.retain(|n| n != &name);
        sqlx::query("UPDATE episode SET frozen = $2::jsonb, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(serde_json::to_string(&r).expect("a resolved episode serialises"))
            .execute(&mut *tx)
            .await?;
        if !episodes.contains(&id) {
            episodes.push(id);
        }
    }
    // The corrections log keeps fields as they were: the name leaves it too (ADR-0177).
    let lines: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, old_values::text, new_values::text, reason FROM episode_correction
         WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_all(&mut *tx)
    .await?;
    let mut corrections_redacted = 0;
    for (id, old, new, reason) in lines {
        let (Ok(mut old), Ok(mut new)) = (
            serde_json::from_str::<serde_json::Value>(&old),
            serde_json::from_str::<serde_json::Value>(&new),
        ) else {
            continue;
        };
        let mut reason = serde_json::Value::String(reason);
        let changed =
            redact(&mut old, &name) | redact(&mut new, &name) | redact(&mut reason, &name);
        if !changed {
            continue;
        }
        sqlx::query(
            "UPDATE episode_correction
             SET old_values = $2::jsonb, new_values = $3::jsonb, reason = $4, redacted_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(old.to_string())
        .bind(new.to_string())
        .bind(reason.as_str().unwrap_or_default())
        .execute(&mut *tx)
        .await?;
        corrections_redacted += 1;
    }
    let mentions: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM episode
         WHERE station_id = $1
           AND (strpos(lower(title || ' ' || description), lower($2)) > 0
                OR strpos(lower(coalesce(frozen->'episode'->>'title', '') || ' '
                                || coalesce(frozen->'episode'->>'description', '')), lower($2)) > 0)",
    )
    .bind(station.as_uuid())
    .bind(&name)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM person WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(person)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    episodes.sort();
    Ok(Some(ErasedPerson {
        name,
        shows: shows.into_iter().map(|(id,)| id).collect(),
        series: series.into_iter().map(|(id,)| id).collect(),
        episodes,
        mentions: mentions.into_iter().map(|(id,)| id).collect(),
        corrections_redacted,
    }))
}

/// Every card file any show, series or episode names now (all stations: they share a
/// directory), so files no longer named can be deleted.
pub async fn card_files_in_use(
    pool: &sqlx::PgPool,
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT v FROM (
            SELECT cards FROM show WHERE cards IS NOT NULL
            UNION ALL SELECT cards FROM series WHERE cards IS NOT NULL
            UNION ALL SELECT cards FROM episode WHERE cards IS NOT NULL
         ) c, jsonb_each_text(c.cards) AS e(k, v) WHERE k <> 'alt'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

/// The card files one show, series or episode names now.
pub fn card_files(cards: &serde_json::Value) -> Vec<String> {
    cards
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| k.as_str() != "alt")
                .filter_map(|(_, v)| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
