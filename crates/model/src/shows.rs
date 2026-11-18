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
use crate::planner::{Clash, Clashes};
use crate::rule::{End, Occurrence, Repeat, Rule};

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
    /// A refusal that already says why in a whole sentence: a show that has aired is not
    /// deleted (ADR-0279).
    #[error("{0}")]
    Kept(String),
    /// There is nothing to do: a series whose rule has no more dates to draft.
    #[error("{0}")]
    Nothing(String),
    /// The draft would sit over another show's time and the one drafting may not do that.
    #[error("{}", .0.message)]
    Clash(Box<crate::planner::ClashRefusal>),
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

/// How much of the studio a show uses (ADR-0271): each step adds one kind of person. Whatever a
/// step leaves out is switched off while the show is on air.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    /// One DJ.
    Solo,
    /// Adds guests, by invitation.
    Guests,
    /// Adds listeners who call in, and Dump.
    Callers,
    /// Adds the screener's seat, the picture, the next DJ and the handover.
    Full,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Solo, Step::Guests, Step::Callers, Step::Full];

    pub fn as_str(self) -> &'static str {
        match self {
            Step::Solo => "solo",
            Step::Guests => "guests",
            Step::Callers => "callers",
            Step::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Step::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// The word the interface uses for it: `Solo`, `Guests`, `Callers`, `Full`.
    pub fn word(self) -> &'static str {
        match self {
            Step::Solo => "Solo",
            Step::Guests => "Guests",
            Step::Callers => "Callers",
            Step::Full => "Full",
        }
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn step_of(row: &sqlx::postgres::PgRow, col: &str) -> Result<Step, sqlx::Error> {
    let raw: String = row.try_get(col)?;
    Step::parse(&raw).ok_or_else(|| sqlx::Error::Decode(format!("`{raw}` is not a step").into()))
}

/// A show made without a step starts at Solo.
fn solo() -> Step {
    Step::Solo
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
    /// The weekly slot, while the rule is exactly that (`rule.as_slot()`), else `null`: what an
    /// older studio reads. Read `rule`.
    pub slot: Option<Slot>,
    /// When the show airs (release 1.0.4, ADR-0278).
    #[serde(default)]
    pub rule: Option<Rule>,
    pub hosts: Vec<Uuid>,
    pub tags: Vec<String>,
    pub explicit: bool,
    pub feed_guid: Option<Uuid>,
    pub cards: Option<serde_json::Value>,
    /// The presenter's on-air name (ADR-0214); empty when the show has none of its own.
    #[serde(default)]
    pub presenter: String,
    /// The show's own takedown contact (ADR-0214): an email address or a link; empty when the
    /// station's is the only one.
    #[serde(default)]
    pub contact: String,
    /// Links to the presenter's channels (ADR-0219: Rumble, Odysee, AzuraCast…).
    #[serde(default)]
    pub links: Vec<String>,
    /// Whether the show plays commercial music (ADR-0219): the station's own reminder that a
    /// UK webcast of it needs PPL and PRS licences. It blocks nothing.
    #[serde(default)]
    pub commercial_music: bool,
    /// The staff account that presents the show (ADR-0218): the wizard's answers are its
    /// defaults. Cleared when the account goes.
    #[serde(default)]
    pub presenter_account: Option<Uuid>,
    /// How much of the studio the show uses (ADR-0271). Shows that existed before steps are
    /// `full`; a new show starts at `solo`.
    #[serde(default = "solo")]
    pub step: Step,
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
    /// The weekly slot an older studio sends with every save. With `rule` given it is ignored;
    /// without, see [`rule_write`].
    #[serde(default)]
    pub slot: Option<Slot>,
    /// When the show airs. Absent leaves it to `slot` (an older studio); `null` clears it.
    #[serde(default, deserialize_with = "double")]
    pub rule: Option<Option<Rule>>,
    /// Make series 1 if the show has none and draft its first episode, in the same transaction
    /// (a refusal, such as a presenter's clash, saves nothing).
    #[serde(default)]
    pub draft_first: bool,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub explicit: bool,
    /// The presenter's on-air name (ADR-0214). Absent keeps what is stored; empty clears it.
    #[serde(default)]
    pub presenter: Option<String>,
    /// The show's takedown contact (ADR-0214). Absent keeps what is stored; empty clears it.
    #[serde(default)]
    pub contact: Option<String>,
    /// Links to the presenter's channels (ADR-0219). Absent keeps what is stored.
    #[serde(default)]
    pub links: Option<Vec<String>>,
    /// Whether the show plays commercial music (ADR-0219). Absent keeps what is stored.
    #[serde(default)]
    pub commercial_music: Option<bool>,
    /// The staff account that presents the show (ADR-0218). Absent keeps what is stored;
    /// `null` clears it. A member of staff who is not an admin may name only themselves.
    #[serde(default, deserialize_with = "double")]
    pub presenter_account: Option<Option<Uuid>>,
    /// The show's step (ADR-0271). Absent keeps what is stored, and a new show starts at
    /// `solo`.
    #[serde(default)]
    pub step: Option<Step>,
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
        if let Some(Some(r)) = &self.rule {
            r.check()?;
        }
        check_people(&self.hosts)?;
        check_tags(&self.tags)?;
        if let Some(p) = &self.presenter {
            text("presenter's name", p.trim(), 0, 120)?;
        }
        if let Some(c) = &self.contact {
            text("takedown contact", c.trim(), 0, 200)?;
        }
        if let Some(links) = &self.links {
            check_links(links)?;
        }
        Ok(())
    }
}

/// The presenter's channel links: at most ten, each one line of up to 400 characters.
fn check_links(links: &[String]) -> Result<(), ShowError> {
    if links.len() > 10 {
        return Err(ShowError::Invalid("at most 10 links".into()));
    }
    for l in links {
        text("link", l.trim(), 1, 400)?;
        if l.trim().contains(char::is_whitespace) {
            return Err(ShowError::Invalid("a link has no spaces".into()));
        }
    }
    Ok(())
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
    /// The series' own weekly slot while its own rule is exactly that, else `null`.
    pub slot: Option<Slot>,
    /// The series' own rule; `None` follows the show's.
    #[serde(default)]
    pub rule: Option<Rule>,
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
    /// The series' own rule; `null` follows the show's again. Absent leaves it to `slot`.
    #[serde(default, deserialize_with = "double")]
    pub rule: Option<Option<Rule>>,
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
        if let Some(Some(r)) = &self.rule {
            r.check()?;
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
    /// The rule date this episode fills, as the station's local date of the nominal start.
    /// Drafting sets it and dragging the episode keeps it (that makes the block MOVED); an
    /// episode made or left by hand has none.
    #[serde(default)]
    pub occurrence: Option<Date>,
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
    /// The weekly slot while the rule is exactly that (frozen copies from before 1.0.4 have only
    /// this).
    pub slot: Option<Slot>,
    /// When it airs, the series' own rule or else the show's.
    #[serde(default)]
    pub rule: Option<Rule>,
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
        rule: show.rule.clone(),
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

/// The rule that governs a series, ready to expand: the series' own, else its show's; the
/// series' start date and planned episodes win over the rule's own start and count (a start
/// date only moves a rule that repeats, and a plan only gives one an end); and a count is what
/// is left of it once the episodes the rule's dates do not account for are taken off (an
/// episode made by hand, or aired before the rule's start, still counts towards the series'
/// total). `None` when the series has no rule at all.
pub fn series_rule(show: &Show, series: Option<&Series>, episodes: &[Episode]) -> Option<Rule> {
    let mut rule = series_rule_planned(show, series, episodes)?;
    if let End::Count { count } = rule.end {
        let extra = episodes
            .iter()
            .filter(|e| e.occurrence.is_none_or(|o| !rule.gives(o)))
            .count();
        rule.end = End::Count {
            count: count.saturating_sub(extra as u32),
        };
    }
    Some(rule)
}

/// [`series_rule`] before the count is reduced by the episodes already made: the series' rule
/// as it is planned, whose sentence says *for 10 episodes*.
pub fn series_rule_planned(
    show: &Show,
    series: Option<&Series>,
    episodes: &[Episode],
) -> Option<Rule> {
    let mut rule = series
        .and_then(|s| s.rule.clone())
        .or_else(|| show.rule.clone())?;
    let repeats = !matches!(
        rule.repeat,
        Repeat::Once | Repeat::Dates { .. } | Repeat::Yearly { .. }
    );
    if let Some(series) = series {
        if repeats && let Some(d) = series.starts_on {
            rule.starts_on = Some(d);
        }
        if let Some(n) = series.planned_episodes
            && !matches!(rule.repeat, Repeat::Once | Repeat::Dates { .. })
            && !matches!(rule.end, End::Until { .. })
        {
            rule.end = End::Count { count: n };
        }
    }
    if matches!(rule.end, End::Count { .. }) && rule.starts_on.is_none() {
        // Counted from the first date it filled, else from wherever the caller starts.
        rule.starts_on = episodes.iter().filter_map(|e| e.occurrence).min();
    }
    Some(rule)
}

/// The dates a series runs: from its first episode (or its start date) to its last planned
/// one, counted on the rule when the plan is known, or to its latest episode.
pub fn series_run(
    show: &Show,
    series: &Series,
    zone: &TimeZone,
    episodes: &[Episode],
) -> (Option<Date>, Option<Date>) {
    let date = |t: i64| {
        Timestamp::from_second(t)
            .ok()
            .map(|ts| ts.to_zoned(zone.clone()).date())
    };
    let aired: Vec<i64> = episodes.iter().filter_map(|e| e.air_at).collect();
    let rule = series_rule(show, Some(series), episodes);
    let first = series
        .starts_on
        .or_else(|| aired.iter().min().and_then(|t| date(*t)))
        .or_else(|| rule.as_ref().and_then(|r| r.starts_on));
    let planned_last = match (first, &rule) {
        (Some(first), Some(rule)) if rule.end != End::Never => rule
            .occurrences(zone, first)
            .take(20_000)
            .last()
            .map(|o| o.on),
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
    match &series.rule {
        Some(rule) => {
            r.slot = rule.as_slot();
            r.rule = Some(rule.clone());
        }
        None => {
            inherited.push("slot".to_owned());
            inherited.push("rule".to_owned());
        }
    }
    match &series.tags {
        Some(t) => r.tags = t.clone(),
        None => inherited.push("tags".to_owned()),
    }
    match &series.description {
        Some(d) => r.description = d.clone(),
        None => inherited.push("description".to_owned()),
    }
    let (first, last) = series_run(show, series, zone, episodes);
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
        minutes: episode
            .minutes
            .or_else(|| r.rule.as_ref().and_then(|rule| rule.minutes)),
    });
    r.inherited = inherited;
    r
}

/// One episode `fill_next` drafts: its number, when it airs, the rule date it fills and the
/// length of its own when the date was moved with one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub number: u32,
    pub air_at: Option<i64>,
    pub occurrence: Option<Date>,
    pub minutes: Option<u16>,
}

/// The local date of an instant in epoch seconds.
fn local_date(zone: &TimeZone, t: i64) -> Option<Date> {
    Timestamp::from_second(t)
        .ok()
        .map(|ts| ts.to_zoned(zone.clone()).date())
}

/// The date drafting goes on from: the day after the latest date any episode fills (or, for an
/// episode made by hand, the date it airs). `None` with nothing drafted.
pub fn drafted_to(zone: &TimeZone, existing: &[Episode]) -> Option<Date> {
    existing
        .iter()
        .filter_map(|e| {
            e.occurrence
                .or_else(|| e.air_at.and_then(|t| local_date(zone, t)))
        })
        .max()
}

/// The number and air time the next `n` episodes take: the last number plus one, and the next
/// dates of the rule that no episode fills and the rule does not skip, after the latest date
/// drafted. With nothing drafted the first is the date under way (its length, or the studio's
/// 60 minutes), else the next on or after the rule's start; and with episodes drafted already,
/// the next after the latest, or the date under way if that is later: never a date in the past. A rule
/// that has no more dates gives fewer than `n`; no rule at all gives numbered episodes with no
/// time. `rule` is [`series_rule`]'s.
pub fn fill_next(
    rule: Option<&Rule>,
    zone: &TimeZone,
    existing: &[Episode],
    n: usize,
    now: Timestamp,
) -> Vec<Draft> {
    let mut number = existing.iter().map(|e| e.number).max().unwrap_or(0);
    let Some(rule) = rule else {
        return (0..n)
            .map(|_| {
                number += 1;
                Draft {
                    number,
                    air_at: None,
                    occurrence: None,
                    minutes: None,
                }
            })
            .collect();
    };
    let filled: std::collections::HashSet<Date> =
        existing.iter().filter_map(|e| e.occurrence).collect();
    // Never into the past: the first date is the one under way (its length, or the studio's 60
    // minutes), else the next. Drafting goes on after the latest date drafted, if that is later.
    let under_way = now
        .checked_sub(i64::from(rule.length()).minutes())
        .unwrap_or(now);
    let today = local_date(zone, under_way.as_second()).unwrap_or_default();
    let from = match drafted_to(zone, existing) {
        Some(d) => d.tomorrow().unwrap_or(d).max(today),
        None => today,
    };
    rule.occurrences(zone, from)
        .filter(|o| !filled.contains(&o.on))
        .filter(|o| o.start > under_way)
        .take(n)
        .map(|o| {
            number += 1;
            Draft {
                number,
                air_at: Some(o.start.as_second()),
                occurrence: Some(o.on),
                minutes: (o.minutes != rule.length()).then_some(o.minutes),
            }
        })
        .collect()
}

/// An occurrence's window as epoch seconds `[start, end)`.
pub fn window_of(o: &Occurrence) -> (i64, i64) {
    (o.start.as_second(), o.end().as_second())
}

// The database.

macro_rules! show_columns {
    () => {
        "id, slug, name, description, colour_slot, layout, rule::text AS rule, hosts, tags, explicit, feed_guid, cards::text AS cards, presenter, contact, links, commercial_music, presenter_account, step"
    };
}
pub(crate) use show_columns;

/// The rule a row holds; a stored rule that no longer parses is a decode error, not an absence.
fn rule_of(row: &sqlx::postgres::PgRow) -> Result<Option<Rule>, sqlx::Error> {
    let raw: Option<String> = row.try_get("rule")?;
    raw.map(|r| {
        serde_json::from_str::<Rule>(&r)
            .map_err(|e| sqlx::Error::Decode(format!("a stored rule does not parse: {e}").into()))
    })
    .transpose()
}

fn json_of(
    row: &sqlx::postgres::PgRow,
    col: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let raw: Option<String> = row.try_get(col)?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub(crate) fn show_of(row: &sqlx::postgres::PgRow) -> Result<Show, sqlx::Error> {
    let rule = rule_of(row)?;
    Ok(Show {
        id: row.try_get("id")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        colour_slot: row.try_get::<i16, _>("colour_slot")? as u8,
        layout: row.try_get("layout")?,
        slot: rule.as_ref().and_then(Rule::as_slot),
        rule,
        hosts: row.try_get("hosts")?,
        tags: row.try_get("tags")?,
        explicit: row.try_get("explicit")?,
        feed_guid: row.try_get("feed_guid")?,
        cards: json_of(row, "cards")?,
        presenter: row.try_get("presenter")?,
        contact: row.try_get("contact")?,
        links: row.try_get("links")?,
        commercial_music: row.try_get("commercial_music")?,
        presenter_account: row.try_get("presenter_account")?,
        step: step_of(row, "step")?,
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

/// What a save does to a stored rule, from what the body carried (design section 9, *Old and
/// new studio builds*): a `rule` (a value, or `null`) sets or clears it and `slot` is ignored;
/// with no `rule` but a `slot`, the stored rule is kept if its slot form equals the slot, else the
/// weekly rule the slot means is set; with neither (or a `null` slot), a slot-shaped stored rule
/// is cleared (as **Clear** does in an older studio) and any other is kept, because the sender
/// could not see it.
pub fn rule_write(
    rule: &Option<Option<Rule>>,
    slot: Option<Slot>,
    stored: Option<&Rule>,
) -> Option<Rule> {
    match (rule, slot) {
        (Some(given), _) => given.clone(),
        (None, Some(slot)) => match stored {
            Some(r) if r.as_slot() == Some(slot) => Some(r.clone()),
            _ => Some(Rule::from(slot)),
        },
        (None, None) => stored.filter(|r| r.as_slot().is_none()).cloned(),
    }
}

/// The three columns an older build still reads, kept while the rule is slot-shaped.
fn slot_columns(rule: Option<&Rule>) -> (Option<i16>, Option<i16>, Option<i16>) {
    let slot = rule.and_then(Rule::as_slot);
    (
        slot.map(|s| i16::from(s.weekday)),
        slot.map(|s| s.start as i16),
        slot.and_then(|s| s.minutes).map(|m| m as i16),
    )
}

fn rule_json(rule: Option<&Rule>) -> Result<Option<String>, ShowError> {
    rule.map(serde_json::to_string)
        .transpose()
        .map_err(|e| ShowError::Invalid(format!("the rule cannot be stored: {e}")))
}

/// One station's schedule is changed by one request at a time (the advisory lock lasts to the
/// end of the transaction), so two drafts cannot both find a time free.
pub async fn lock_schedule(
    conn: &mut sqlx::PgConnection,
    station: StationId,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('schedule:' || $1::text, 0))")
        .bind(station.as_uuid())
        .execute(conn)
        .await?;
    Ok(())
}

/// Make or change a show by its path segment. The presenter's fields (ADR-0214, ADR-0219)
/// are kept when the input leaves them out, so an older studio build or an integration that
/// does not know them never wipes a DJ's details on save.
pub async fn put_show(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    input: &ShowInput,
) -> Result<Show, ShowError> {
    check_slug(slug)?;
    input.check()?;
    let hosts = people_ids(pool, station, &input.hosts).await?;
    let mut tx = pool.begin().await?;
    let show = put_show_in(&mut tx, station, slug, input, &hosts).await?;
    tx.commit().await?;
    Ok(show)
}

fn check_slug(slug: &str) -> Result<(), ShowError> {
    if !is_slug(slug) {
        return Err(ShowError::Invalid(format!(
            "`{slug}` is not a show path: lower-case letters and digits, joined by hyphens"
        )));
    }
    Ok(())
}

/// A show's path from its name, as the studio's `slugOf` makes it: lower case, accents taken
/// off, every run of other characters one hyphen, at most 60 characters (with no hyphen left
/// at either end).
pub fn slug_of(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let lower = name.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut gap = false;
    for c in lower
        .nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
    {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out.truncate(60);
    out.trim_end_matches('-').to_owned()
}

/// A path for a new show that no show in `taken` holds, as the studio's `freeSlug` makes it:
/// the name's, else `-2`, `-3`… (`show` for a name with no letter or digit).
pub fn free_slug(name: &str, taken: &std::collections::HashSet<String>) -> String {
    let base = match slug_of(name) {
        b if b.is_empty() => "show".to_owned(),
        b => b,
    };
    if !taken.contains(&base) {
        return base;
    }
    let stem = |n: usize| base[..base.len().min(n)].trim_end_matches('-').to_owned();
    for n in 2..100 {
        let s = format!("{}-{n}", stem(57));
        if !taken.contains(&s) {
            return s;
        }
    }
    loop {
        let tail = Uuid::new_v4().simple().to_string();
        let s = format!("{}-{}", stem(40), &tail[..8]);
        if !taken.contains(&s) {
            return s;
        }
    }
}

/// A show a calendar file proposed (ADR-0281), as the person chose to make it.
#[derive(Clone, Debug)]
pub struct ImportedShow {
    /// The path wanted (its cards were drawn with it); another is made free if it has been
    /// taken meanwhile.
    pub slug: String,
    pub name: String,
    pub description: String,
    pub rule: Rule,
    pub colour_slot: u8,
}

/// Make the shows of a calendar import in one transaction: each new, at step Solo, with no
/// episode drafted and no host; each at its wanted path, or, if that has been taken meanwhile,
/// at one made free against the station's shows and those made before it in the same import. The show table is locked against other writers meanwhile, so a
/// path chosen here cannot be taken (and an existing show overwritten) before the insert.
pub async fn import_shows(
    pool: &sqlx::PgPool,
    station: StationId,
    shows: &[ImportedShow],
) -> Result<Vec<Show>, ShowError> {
    let inputs: Vec<ShowInput> = shows
        .iter()
        .map(|s| ShowInput {
            name: s.name.trim().to_owned(),
            description: s.description.clone(),
            colour_slot: s.colour_slot,
            layout: poster(),
            slot: None,
            rule: Some(Some(s.rule.clone())),
            draft_first: false,
            hosts: Vec::new(),
            tags: Vec::new(),
            explicit: false,
            presenter: None,
            contact: None,
            links: None,
            commercial_music: None,
            presenter_account: None,
            step: Some(Step::Solo),
        })
        .collect();
    for i in &inputs {
        i.check()?;
    }
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE show IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let mut taken: std::collections::HashSet<String> =
        sqlx::query_scalar("SELECT slug FROM show WHERE station_id = $1")
            .bind(station.as_uuid())
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let mut made = Vec::with_capacity(inputs.len());
    for (s, input) in shows.iter().zip(&inputs) {
        let slug = if is_slug(&s.slug) && !taken.contains(&s.slug) {
            s.slug.clone()
        } else {
            free_slug(&input.name, &taken)
        };
        check_slug(&slug)?;
        taken.insert(slug.clone());
        made.push(put_show_in(&mut tx, station, &slug, input, &[]).await?);
    }
    tx.commit().await?;
    Ok(made)
}

/// [`put_show`] inside a transaction the caller holds; the input is checked and the hosts made.
pub async fn put_show_in(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    slug: &str,
    input: &ShowInput,
    hosts: &[Uuid],
) -> Result<Show, ShowError> {
    let stored: Option<Option<String>> = sqlx::query_scalar(
        "SELECT rule::text FROM show WHERE station_id = $1 AND slug = $2 FOR UPDATE",
    )
    .bind(station.as_uuid())
    .bind(slug)
    .fetch_optional(&mut *conn)
    .await?;
    let stored: Option<Rule> = stored
        .flatten()
        .map(|r| serde_json::from_str(&r))
        .transpose()
        .map_err(|e| ShowError::Invalid(format!("the stored rule does not parse: {e}")))?;
    let rule = rule_write(&input.rule, input.slot, stored.as_ref());
    let (slot_weekday, slot_start, slot_minutes) = slot_columns(rule.as_ref());
    let links: Option<Vec<String>> = input.links.as_ref().map(|l| {
        l.iter()
            .map(|x| x.trim().to_owned())
            .filter(|x| !x.is_empty())
            .collect()
    });
    // `presenter_account`: absent keeps, `null` clears, a value sets.
    let (account_given, account): (bool, Option<Uuid>) = match input.presenter_account {
        None => (false, None),
        Some(a) => (true, a),
    };
    let row = sqlx::query(concat!(
        "INSERT INTO show (id, station_id, slug, name, description, colour_slot, layout,
                           slot_weekday, slot_start, slot_minutes, hosts, tags, explicit,
                           presenter, contact, links, commercial_music, presenter_account, step,
                           rule)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 COALESCE($14, ''), COALESCE($15, ''), COALESCE($16, '{}'::text[]),
                 COALESCE($17, false), $19, COALESCE($20, 'solo'), $21::text::jsonb)
         ON CONFLICT (station_id, slug) DO UPDATE SET
           name = EXCLUDED.name, description = EXCLUDED.description,
           colour_slot = EXCLUDED.colour_slot, layout = EXCLUDED.layout,
           slot_weekday = EXCLUDED.slot_weekday, slot_start = EXCLUDED.slot_start,
           slot_minutes = EXCLUDED.slot_minutes, rule = EXCLUDED.rule,
           hosts = EXCLUDED.hosts, tags = EXCLUDED.tags,
           explicit = EXCLUDED.explicit,
           presenter = COALESCE($14, show.presenter),
           contact = COALESCE($15, show.contact),
           links = COALESCE($16, show.links),
           commercial_music = COALESCE($17, show.commercial_music),
           presenter_account = CASE WHEN $18 THEN $19 ELSE show.presenter_account END,
           step = COALESCE($20, show.step),
           updated_at = now()
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
    .bind(slot_weekday)
    .bind(slot_start)
    .bind(slot_minutes)
    .bind(hosts)
    .bind(&input.tags)
    .bind(input.explicit)
    .bind(input.presenter.as_ref().map(|p| p.trim().to_owned()))
    .bind(input.contact.as_ref().map(|c| c.trim().to_owned()))
    .bind(links)
    .bind(input.commercial_music)
    .bind(account_given)
    .bind(account)
    .bind(input.step.map(Step::as_str))
    .bind(rule_json(rule.as_ref())?)
    .fetch_one(&mut *conn)
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

/// A show by its id.
pub async fn get_show_by_id(pool: &sqlx::PgPool, id: Uuid) -> Result<Option<Show>, sqlx::Error> {
    let row = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(show_of).transpose()
}

/// Who changed a show's step: how the logs name them, and their account while there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepBy {
    pub label: String,
    pub account: Option<Uuid>,
}

/// One change of a show's step, as logged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct StepChange {
    pub from: Step,
    pub to: Step,
    pub by: String,
    pub at_ms: u64,
}

/// Change only a show's step, and log the change with who made it (ADR-0271). Returns the show
/// as it is now and the step it had; nothing is logged when the step is already `step`.
/// `ShowError::Missing` for a show that does not exist.
pub async fn set_step(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    step: Step,
    by: &StepBy,
) -> Result<(Show, Step), ShowError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE station_id = $1 AND slug = $2 FOR UPDATE"
    ))
    .bind(station.as_uuid())
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ShowError::Missing(format!("show `{slug}`")));
    };
    let mut show = show_of(&row)?;
    let from = show.step;
    if from != step {
        sqlx::query("UPDATE show SET step = $2, updated_at = now() WHERE id = $1")
            .bind(show.id)
            .bind(step.as_str())
            .execute(&mut *tx)
            .await?;
        log_step_change_in(&mut tx, station, show.id, from, step, by).await?;
        show.step = step;
    }
    tx.commit().await?;
    Ok((show, from))
}

async fn log_step_change_in(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    show: Uuid,
    from: Step,
    to: Step,
    by: &StepBy,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO show_step_change (station_id, show_id, from_step, to_step, by_label, by_account)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(station.as_uuid())
    .bind(show)
    .bind(from.as_str())
    .bind(to.as_str())
    .bind(by.label.chars().take(160).collect::<String>())
    .bind(by.account)
    .execute(tx)
    .await?;
    Ok(())
}

/// Log a change of step made some other way (the show editor's save).
pub async fn log_step_change(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
    from: Step,
    to: Step,
    by: &StepBy,
) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    log_step_change_in(&mut conn, station, show, from, to, by).await
}

/// The changes made to a show's step, oldest first.
pub async fn step_changes(pool: &sqlx::PgPool, show: Uuid) -> Result<Vec<StepChange>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT from_step, to_step, by_label, (extract(epoch FROM at) * 1000)::bigint AS at_ms
         FROM show_step_change WHERE show_id = $1 ORDER BY id",
    )
    .bind(show)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(StepChange {
                from: step_of(r, "from_step")?,
                to: step_of(r, "to_step")?,
                by: r.try_get("by_label")?,
                at_ms: r.try_get::<i64, _>("at_ms")?.max(0) as u64,
            })
        })
        .collect()
}

/// The show whose episode started last at or before `now_s`, and when it started (epoch
/// seconds): the show last on air, or the one on air. `None` when no episode has started, which
/// is "no show has been on air yet". Read from the schedule, so it is the same after a restart.
pub async fn last_started(
    pool: &sqlx::PgPool,
    station: StationId,
    now_s: i64,
) -> Result<Option<(Show, i64)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT s.show_id, (extract(epoch FROM e.air_at))::bigint AS at_s
         FROM episode e JOIN series s ON s.id = e.series_id
         WHERE e.station_id = $1 AND e.air_at IS NOT NULL AND e.air_at <= to_timestamp($2)
         ORDER BY e.air_at DESC, e.number DESC LIMIT 1",
    )
    .bind(station.as_uuid())
    .bind(now_s as f64)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let id: Uuid = row.try_get("show_id")?;
    let at: i64 = row.try_get("at_s")?;
    Ok(get_show_by_id(pool, id).await?.map(|show| (show, at)))
}

/// The show whose episode starts next after `now_s`, and when (epoch seconds).
pub async fn next_start(
    pool: &sqlx::PgPool,
    station: StationId,
    now_s: i64,
) -> Result<Option<(Show, i64)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT s.show_id, (extract(epoch FROM e.air_at))::bigint AS at_s
         FROM episode e JOIN series s ON s.id = e.series_id
         WHERE e.station_id = $1 AND e.air_at IS NOT NULL AND e.air_at > to_timestamp($2)
         ORDER BY e.air_at ASC, e.number ASC LIMIT 1",
    )
    .bind(station.as_uuid())
    .bind(now_s as f64)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let id: Uuid = row.try_get("show_id")?;
    let at: i64 = row.try_get("at_s")?;
    Ok(get_show_by_id(pool, id).await?.map(|show| (show, at)))
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
        "id, show_id, number, title, description, hosts, rule::text AS rule, starts_on, planned_episodes, tags, cards::text AS cards"
    };
}
pub(crate) use series_columns;

pub(crate) fn series_of(row: &sqlx::postgres::PgRow) -> Result<Series, sqlx::Error> {
    let starts: Option<String> = row.try_get("starts_on")?;
    let rule = rule_of(row)?;
    Ok(Series {
        id: row.try_get("id")?,
        show_id: row.try_get("show_id")?,
        number: row.try_get::<i32, _>("number")? as u32,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        hosts: row.try_get("hosts")?,
        slot: rule.as_ref().and_then(Rule::as_slot),
        rule,
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
    let mut tx = pool.begin().await?;
    let series = put_series_in(&mut tx, station, show, number, input, hosts).await?;
    tx.commit().await?;
    Ok(series)
}

/// [`put_series`] inside a transaction the caller holds; the input is checked and the hosts made.
pub async fn put_series_in(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    show: &Show,
    number: u32,
    input: &SeriesInput,
    hosts: Option<Vec<Uuid>>,
) -> Result<Series, ShowError> {
    let stored: Option<Option<String>> = sqlx::query_scalar(
        "SELECT rule::text FROM series WHERE show_id = $1 AND number = $2 FOR UPDATE",
    )
    .bind(show.id)
    .bind(number as i32)
    .fetch_optional(&mut *conn)
    .await?;
    let stored: Option<Rule> = stored
        .flatten()
        .map(|r| serde_json::from_str(&r))
        .transpose()
        .map_err(|e| ShowError::Invalid(format!("the stored rule does not parse: {e}")))?;
    let rule = rule_write(&input.rule, input.slot, stored.as_ref());
    let (slot_weekday, slot_start, slot_minutes) = slot_columns(rule.as_ref());
    let row = sqlx::query(concat!(
        "INSERT INTO series (id, station_id, show_id, number, title, description, hosts,
                             slot_weekday, slot_start, slot_minutes, starts_on,
                             planned_episodes, tags, rule)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14::text::jsonb)
         ON CONFLICT (show_id, number) DO UPDATE SET
           title = EXCLUDED.title, description = EXCLUDED.description, hosts = EXCLUDED.hosts,
           slot_weekday = EXCLUDED.slot_weekday, slot_start = EXCLUDED.slot_start,
           slot_minutes = EXCLUDED.slot_minutes, rule = EXCLUDED.rule,
           starts_on = EXCLUDED.starts_on,
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
    .bind(slot_weekday)
    .bind(slot_start)
    .bind(slot_minutes)
    .bind(input.starts_on.map(|d| d.to_string()))
    .bind(input.planned_episodes.map(|n| n as i32))
    .bind(&input.tags)
    .bind(rule_json(rule.as_ref())?)
    .fetch_one(&mut *conn)
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
        "id, series_id, number, title, description, kind, extract(epoch FROM air_at)::bigint AS air_at, minutes, hosts, guests, tags, extract(epoch FROM aired_at)::bigint AS aired_at, frozen::text AS frozen, files::text AS files, cards::text AS cards, occurrence::text AS occurrence"
    };
}
pub(crate) use episode_columns;

pub(crate) fn episode_of(row: &sqlx::postgres::PgRow) -> Result<Episode, sqlx::Error> {
    let frozen: Option<String> = row.try_get("frozen")?;
    let occurrence: Option<String> = row.try_get("occurrence")?;
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
        occurrence: occurrence.and_then(|d| d.parse().ok()),
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

/// Every series of the station, in show and series order.
pub async fn all_series(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Series>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE station_id = $1 ORDER BY show_id, number"
    ))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    rows.iter().map(series_of).collect()
}

/// The episodes of these series, by series and number.
pub async fn episodes_of(
    pool: &sqlx::PgPool,
    series: &[Uuid],
) -> Result<Vec<Episode>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE series_id = ANY($1) ORDER BY series_id, number"
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

/// An episode with no length of its own or from its slot is taken to last this long.
pub const DEFAULT_EPISODE_MINUTES: i64 = 60;

/// When an episode is on air by the schedule, in epoch seconds `[start, end)`: from its air time
/// for its length (the episode's, else its series' or show's slot, else an hour). `None` for an
/// episode with no air time.
pub fn air_window(show: &Show, series: &Series, ep: &Episode) -> Option<(i64, i64)> {
    let start = ep.air_at?;
    let minutes = ep
        .minutes
        .or(series.rule.as_ref().and_then(|r| r.minutes))
        .or(show.rule.as_ref().and_then(|r| r.minutes))
        .map_or(DEFAULT_EPISODE_MINUTES, i64::from);
    Some((start, start + minutes * 60))
}

/// The show on air at `now_s` by the schedule: the entry whose episode's [`air_window`] holds
/// it, the latest-starting when two overlap (`episodes` in air order, as [`episodes_between`]
/// gives them). The one rule for the slate's picture (ADR-0200), the consent words (ADR-0214)
/// and the chat's hours (ADR-0234).
pub fn on_air_at(
    episodes: &[(Show, Series, Episode)],
    now_s: i64,
) -> Option<&(Show, Series, Episode)> {
    episodes.iter().rev().find(|(show, series, ep)| {
        air_window(show, series, ep).is_some_and(|(start, end)| start <= now_s && now_s < end)
    })
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

/// What drafting made, and the clashes it was allowed to go on with.
#[derive(Clone, Debug)]
pub struct Created {
    pub episodes: Vec<Episode>,
    pub warnings: Vec<Clash>,
}

/// Draft the next episodes of a series, each filled in from the rule and inheriting the rest.
/// `maker` is kept on each (ADR-0198). A series whose rule has no more dates answers
/// [`ShowError::Nothing`]. `clashes` says what to do with another show's time: refuse
/// ([`Clashes::Refuse`], a presenter's), return it as a warning, or not look.
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
    clashes: Clashes,
) -> Result<Created, ShowError> {
    if new.count == 0 || new.count > 52 {
        return Err(ShowError::Invalid(
            "draft 1 to 52 episodes at a time".into(),
        ));
    }
    text("episode title", new.title.trim(), 0, 200)?;
    check_people(&new.guests)?;
    let guests = people_ids(pool, station, &new.guests).await?;
    let mut tx = pool.begin().await?;
    lock_schedule(&mut tx, station).await?;
    let made = create_episodes_in(
        &mut tx, pool, station, show, series, zone, new, &guests, now, maker, clashes,
    )
    .await?;
    tx.commit().await?;
    let mut episodes = Vec::new();
    for n in made.0 {
        if let Some(e) = get_episode(pool, series.id, n).await? {
            episodes.push(e);
        }
    }
    Ok(Created {
        episodes,
        warnings: made.1,
    })
}

/// [`create_episodes`] inside a transaction the caller holds (with the schedule locked): the
/// numbers made and the clashes allowed. `pool` reads the other shows.
#[allow(clippy::too_many_arguments)] // the transaction, the pool, and what create_episodes takes
pub async fn create_episodes_in(
    tx: &mut sqlx::PgConnection,
    pool: &sqlx::PgPool,
    station: StationId,
    show: &Show,
    series: &Series,
    zone: &TimeZone,
    new: &NewEpisodes,
    guests: &[Uuid],
    now: Timestamp,
    maker: Maker<'_>,
    clashes: Clashes,
) -> Result<(Vec<u32>, Vec<Clash>), ShowError> {
    // One drafter at a time per series: the numbers and dates follow the last one.
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
    let rule = series_rule(show, Some(series), &existing);
    let drafts = fill_next(rule.as_ref(), zone, &existing, new.count, now);
    if rule.is_some() && drafts.is_empty() {
        return Err(ShowError::Nothing(match rule.as_ref().map(|r| &r.end) {
            Some(End::Count { .. }) => "all the series' episodes are made".to_owned(),
            _ => "the rule has no more dates to draft".to_owned(),
        }));
    }
    let length = rule.as_ref().map_or(60, Rule::length);
    let windows: Vec<(i64, i64)> = drafts
        .iter()
        .filter_map(|d| {
            let start = d.air_at?;
            Some((start, start + i64::from(d.minutes.unwrap_or(length)) * 60))
        })
        .collect();
    let mut warnings = Vec::new();
    if clashes != Clashes::Ignore {
        let found =
            crate::planner::clashes_for(pool, station, zone, Some(show.id), &windows, now).await?;
        let mut all: Vec<Clash> = found.into_iter().flatten().collect();
        if clashes == Clashes::Refuse && !all.is_empty() {
            for c in &mut all {
                c.refused = true;
            }
            return Err(ShowError::Clash(Box::new(crate::planner::ClashRefusal {
                message: all[0].words(),
                clashes: all,
            })));
        }
        warnings.append(&mut all);
    }
    let mut ids = Vec::new();
    for (i, d) in drafts.iter().enumerate() {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO episode (id, station_id, series_id, number, title, guests, air_at,
                                  made_by, made_by_account, occurrence, minutes)
             VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision), $8, $9,
                     $10::text::date, $11)",
        )
        .bind(id)
        .bind(station.as_uuid())
        .bind(series.id)
        .bind(d.number as i32)
        .bind(if i == 0 { new.title.trim() } else { "" })
        .bind(if i == 0 { guests.to_vec() } else { Vec::new() })
        .bind(d.air_at.map(|t| t as f64))
        .bind(maker.label)
        .bind(maker.account)
        .bind(d.occurrence.map(|o| o.to_string()))
        .bind(d.minutes.map(|m| m as i16))
        .execute(&mut *tx)
        .await?;
        ids.push(d.number);
    }
    Ok((ids, warnings))
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

    /// The studio's own cases (`schedule.test.ts`, `wizard.test.ts`): an imported show takes the
    /// path the studio would have given it.
    #[test]
    fn a_new_show_takes_the_path_the_studio_would_give_it() {
        assert_eq!(slug_of("Saturday Night Kettle"), "saturday-night-kettle");
        assert_eq!(slug_of("Rùnach & co."), "runach-co");
        assert_eq!(slug_of("  Ceòl -- agus  Còmhradh!  "), "ceol-agus-comhradh");
        let taken = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        assert_eq!(free_slug("The Night Show", &taken(&[])), "the-night-show");
        assert_eq!(
            free_slug("The Night Show", &taken(&["the-night-show"])),
            "the-night-show-2"
        );
        assert_eq!(
            free_slug(
                "The Night Show",
                &taken(&["the-night-show", "the-night-show-2"])
            ),
            "the-night-show-3"
        );
        assert_eq!(free_slug("!!!", &taken(&[])), "show");
        // Long names stay paths: 60 characters at most, never ending in a hyphen.
        let long = "A Very Long Name For A Show That Goes On And On Past Sixty-One Characters";
        let s = free_slug(long, &taken(&[]));
        assert!(is_slug(&s) && s.len() <= 60, "{s}");
        let again = free_slug(long, &taken(&[s.as_str()]));
        assert!(is_slug(&again) && again.ends_with("-2"), "{again}");
    }

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
            occurrence: None,
        }
    }

    fn weekly(starts_on: Option<&str>) -> Option<Rule> {
        Some(Rule {
            starts_on: starts_on.map(|d| d.parse().unwrap()),
            ..Rule::from(saturday_eight())
        })
    }

    fn local(t: i64) -> String {
        Timestamp::from_second(t)
            .unwrap()
            .to_zoned(london())
            .strftime("%Y-%m-%d %H:%M %Z")
            .to_string()
    }

    #[test]
    fn a_season_fills_in_from_its_start_date_across_the_clock_change() {
        let got = fill_next(
            weekly(Some("2026-09-05")).as_ref(),
            &london(),
            &[],
            10,
            ts("2026-08-01T12:00[Europe/London]"),
        );
        assert_eq!(got.len(), 10);
        assert_eq!(got[0].number, 1);
        assert_eq!(got[9].number, 10);
        assert_eq!(local(got[0].air_at.unwrap()), "2026-09-05 20:00 BST");
        assert_eq!(got[0].occurrence, Some("2026-09-05".parse().unwrap()));
        assert_eq!(local(got[6].air_at.unwrap()), "2026-10-17 20:00 BST");
        assert_eq!(local(got[7].air_at.unwrap()), "2026-10-24 20:00 BST");
        // The clocks go back on 25 October: still eight o'clock, now GMT.
        assert_eq!(local(got[8].air_at.unwrap()), "2026-10-31 20:00 GMT");
        assert_eq!(local(got[9].air_at.unwrap()), "2026-11-07 20:00 GMT");
    }

    #[test]
    fn a_season_is_never_drafted_into_the_past() {
        // The rule began in September; it is now the end of October and nothing is drafted.
        let got = fill_next(
            weekly(Some("2026-09-05")).as_ref(),
            &london(),
            &[],
            2,
            ts("2026-10-27T12:00[Europe/London]"),
        );
        assert_eq!(local(got[0].air_at.unwrap()), "2026-10-31 20:00 GMT");
        assert_eq!(got[0].number, 1);
        // A start date in the future wins over now.
        let got = fill_next(
            weekly(Some("2026-12-05")).as_ref(),
            &london(),
            &[],
            1,
            ts("2026-10-27T12:00[Europe/London]"),
        );
        assert_eq!(local(got[0].air_at.unwrap()), "2026-12-05 20:00 GMT");
    }

    #[test]
    fn the_next_episode_follows_the_last() {
        let mut sixth = episode(6, Some("2026-10-17T20:00[Europe/London]"));
        sixth.occurrence = Some("2026-10-17".parse().unwrap());
        let existing = vec![sixth, episode(5, Some("2026-10-10T20:00[Europe/London]"))];
        let got = fill_next(
            weekly(None).as_ref(),
            &london(),
            &existing,
            1,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(got[0].number, 7);
        assert_eq!(
            got[0].air_at,
            Some(ts("2026-10-24T20:00[Europe/London]").as_second())
        );
        // No rule: numbered, not scheduled.
        assert_eq!(
            fill_next(None, &london(), &existing, 1, Timestamp::now())[0],
            Draft {
                number: 7,
                air_at: None,
                occurrence: None,
                minutes: None
            }
        );
    }

    #[test]
    fn dates_that_are_filled_or_skipped_are_passed_over() {
        let mut rule = weekly(Some("2026-10-03")).unwrap();
        rule.skip = vec!["2026-10-10".parse().unwrap()];
        let mut first = episode(1, Some("2026-10-03T20:00[Europe/London]"));
        first.occurrence = Some("2026-10-03".parse().unwrap());
        let got = fill_next(
            Some(&rule),
            &london(),
            &[first],
            2,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(local(got[0].air_at.unwrap()), "2026-10-17 20:00 BST");
        assert_eq!(local(got[1].air_at.unwrap()), "2026-10-24 20:00 BST");
        assert_eq!((got[0].number, got[1].number), (2, 3));
        // A rule that has no more dates gives fewer than asked.
        let mut once = weekly(Some("2026-10-03")).unwrap();
        once.repeat = Repeat::Once;
        let got = fill_next(
            Some(&once),
            &london(),
            &[],
            3,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(got.len(), 1);
        let got = fill_next(
            Some(&once),
            &london(),
            &[],
            1,
            ts("2026-10-04T12:00[Europe/London]"),
        );
        assert!(got.is_empty());
    }

    #[test]
    fn a_moved_date_is_drafted_at_its_moved_time_and_fills_the_date_it_was_moved_from() {
        use crate::rule::Move;
        let mut rule = weekly(Some("2026-10-03")).unwrap();
        rule.moves = vec![Move {
            from: "2026-10-10".parse().unwrap(),
            to: "2026-10-11".parse::<Date>().unwrap().at(10, 0, 0, 0),
            minutes: Some(30),
        }];
        let got = fill_next(
            Some(&rule),
            &london(),
            &[],
            2,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(local(got[0].air_at.unwrap()), "2026-10-03 20:00 BST");
        assert_eq!(got[0].minutes, None, "the rule's own length");
        assert_eq!(got[1].occurrence, Some("2026-10-10".parse().unwrap()));
        assert_eq!(local(got[1].air_at.unwrap()), "2026-10-11 10:00 BST");
        assert_eq!(
            got[1].minutes,
            Some(30),
            "a length of its own for this date"
        );
    }

    #[test]
    fn an_episode_frozen_before_rules_still_reads() {
        // A frozen copy as 1.0.3 stored it: no `rule`, only `slot`.
        let old = serde_json::json!({
            "show": "Saturday Night Kettle", "slug": "saturday-night", "description": "",
            "colour_slot": 3, "layout": "poster",
            "slot": {"weekday": 6, "start": 1200, "minutes": 180},
            "hosts": [], "guests": [], "tags": [], "explicit": false,
            "series": null, "episode": null, "inherited": [], "frozen": true
        });
        let r: Resolved = serde_json::from_value(old).expect("an old frozen copy reads");
        assert_eq!(r.rule, None);
        assert_eq!(r.slot.unwrap().weekday, 6);
        // And an episode row's JSON from before `occurrence`.
        let e: Episode = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(), "series_id": Uuid::nil(), "number": 1, "title": "", "description": "",
            "kind": "live", "air_at": null, "minutes": null, "hosts": null, "guests": [],
            "tags": null, "aired_at": null, "frozen": null, "files": null, "cards": null
        }))
        .unwrap();
        assert_eq!(e.occurrence, None);
    }

    #[test]
    fn a_series_left_alone_is_not_drafted_into_the_past_either() {
        // The latest episode was five weeks ago; it is now Wednesday 21 October.
        let mut old = episode(1, Some("2026-09-12T20:00[Europe/London]"));
        old.occurrence = Some("2026-09-12".parse().unwrap());
        let got = fill_next(
            weekly(None).as_ref(),
            &london(),
            &[old],
            2,
            ts("2026-10-21T12:00[Europe/London]"),
        );
        assert_eq!(local(got[0].air_at.unwrap()), "2026-10-24 20:00 BST");
        assert_eq!(got[0].number, 2);
        assert_eq!(local(got[1].air_at.unwrap()), "2026-10-31 20:00 GMT");
    }

    #[test]
    fn the_first_episode_is_the_slot_under_way() {
        // Saturday 20:00 for three hours; it is 21:30 that Saturday and nothing is drafted.
        let rule = weekly(None).unwrap();
        let got = fill_next(
            Some(&rule),
            &london(),
            &[],
            2,
            ts("2026-10-17T21:30[Europe/London]"),
        );
        assert_eq!(
            got[0].air_at,
            Some(ts("2026-10-17T20:00[Europe/London]").as_second())
        );
        assert_eq!(
            got[1].air_at,
            Some(ts("2026-10-24T20:00[Europe/London]").as_second())
        );
        // Once it has ended (23:00), the next one.
        let got = fill_next(
            Some(&rule),
            &london(),
            &[],
            1,
            ts("2026-10-17T23:00[Europe/London]"),
        );
        assert_eq!(
            got[0].air_at,
            Some(ts("2026-10-24T20:00[Europe/London]").as_second())
        );
        // A slot without a length counts as an hour, as the studio app draws it.
        let short = Rule {
            minutes: None,
            ..rule
        };
        let at = |now| fill_next(Some(&short), &london(), &[], 1, ts(now))[0].air_at;
        assert_eq!(
            at("2026-10-17T20:59[Europe/London]"),
            Some(ts("2026-10-17T20:00[Europe/London]").as_second())
        );
        assert_eq!(
            at("2026-10-17T21:00[Europe/London]"),
            Some(ts("2026-10-24T20:00[Europe/London]").as_second())
        );
    }

    #[test]
    fn a_count_is_what_is_left_after_the_episodes_the_dates_do_not_account_for() {
        let show = show();
        let mut series = series(&show);
        series.starts_on = Some("2026-10-03".parse().unwrap());
        series.planned_episodes = Some(5);
        // One episode was made by hand (no date) and one aired before the series' start.
        let hand = episode(1, Some("2026-09-30T20:00[Europe/London]"));
        let mut on_rule = episode(2, Some("2026-10-03T20:00[Europe/London]"));
        on_rule.occurrence = Some("2026-10-03".parse().unwrap());
        let eps = vec![hand, on_rule];
        let rule = series_rule(&show, Some(&series), &eps).unwrap();
        assert_eq!(rule.end, End::Count { count: 4 });
        let got = fill_next(
            Some(&rule),
            &london(),
            &eps,
            10,
            ts("2026-10-01T12:00[Europe/London]"),
        );
        assert_eq!(got.len(), 3, "five in all: two made, three to come");
        assert_eq!(got[2].occurrence, Some("2026-10-24".parse().unwrap()));
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
            rule: Some(Rule::from(saturday_eight())),
            hosts: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
            tags: vec!["music".into()],
            explicit: false,
            feed_guid: None,
            cards: None,
            presenter: String::new(),
            contact: String::new(),
            links: Vec::new(),
            commercial_music: false,
            presenter_account: None,
            step: Step::Full,
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
            rule: None,
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

/// What deleting a show removed (ADR-0279), and the files that went with its rows: the caller
/// deletes them once the transaction has committed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct DeletedShow {
    pub slug: String,
    pub name: String,
    /// Its series, and the drafted episodes in them.
    pub series: usize,
    pub episodes: usize,
    /// Its own cart banks and the carts in them.
    pub cart_banks: usize,
    pub carts: usize,
    /// The show's own id, which names its slate still.
    #[serde(skip)]
    pub id: Uuid,
    /// The card files the show, its series and its episodes named: some may still be named
    /// by another row (card files are named by their content), so see [`card_files_in_use`].
    #[serde(skip)]
    pub card_files: Vec<String>,
    /// The audio files its episodes named, as published in `episodes_dir`.
    #[serde(skip)]
    pub media_files: Vec<String>,
    /// Its carts' files in the carts' directory, with the cart ids.
    #[serde(skip)]
    pub cart_files: Vec<(Uuid, Option<String>)>,
    /// The ids of the show's host keys (ADR-0280), which go with it by their foreign key: the
    /// caller takes each one's monitor seats out of the room and closes its connection.
    #[serde(skip)]
    pub key_ids: Vec<String>,
}

/// The sentence for a show that cannot be deleted because it has aired or is on air. `aired`
/// is how many of its episodes have aired.
pub fn kept_because_aired(name: &str, aired: usize, on_air: bool) -> String {
    if aired > 0 {
        let n = if aired == 1 {
            "1 episode".to_owned()
        } else {
            format!("{aired} episodes")
        };
        format!(
            "{name} has aired {n}, so it stays as the station's record: only a show that has never aired can be deleted."
        )
    } else if on_air {
        format!("{name} is on air now, so it cannot be deleted.")
    } else {
        format!("{name} cannot be deleted.")
    }
}

/// Delete a show that has never aired (ADR-0279), with its series, episodes, cart banks, and
/// its step and rule logs, all in one transaction. Refused (`ShowError::Kept`) when any of its
/// episodes has aired (`aired_at`, which is also `frozen`, or its time has ended by `now_s`) or
/// is on air at `now_s` by the schedule; `Missing` for a show that does not exist. Past plays, the generated log and chat
/// keep nothing of a show by its id (a clock or log item naming one of its own carts is left naming
/// a cart that is gone, as when a show's bank is removed), and a call's consent keeps the show as words
/// (`caller_consent.show_id` is `ON DELETE SET NULL`), so nothing else changes. The episodes
/// are locked first, so a concurrent *Put on air* either finishes before the check or finds the
/// episode gone.
pub async fn delete_show(
    pool: &sqlx::PgPool,
    station: StationId,
    slug: &str,
    now_s: i64,
) -> Result<DeletedShow, ShowError> {
    let mut tx = pool.begin().await?;
    lock_schedule(&mut tx, station).await?;
    let Some(row) = sqlx::query(concat!(
        "SELECT ",
        show_columns!(),
        " FROM show WHERE station_id = $1 AND slug = $2 FOR UPDATE"
    ))
    .bind(station.as_uuid())
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(ShowError::Missing(format!("show `{slug}`")));
    };
    let show = show_of(&row)?;
    let series: Vec<Series> = sqlx::query(concat!(
        "SELECT ",
        series_columns!(),
        " FROM series WHERE show_id = $1 ORDER BY number"
    ))
    .bind(show.id)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(series_of)
    .collect::<Result<_, _>>()?;
    let ids: Vec<Uuid> = series.iter().map(|s| s.id).collect();
    let episodes: Vec<Episode> = sqlx::query(concat!(
        "SELECT ",
        episode_columns!(),
        " FROM episode WHERE series_id = ANY($1) ORDER BY series_id, number FOR UPDATE"
    ))
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(episode_of)
    .collect::<Result<_, _>>()?;
    // An episode has aired once *On air now* froze it, or once its time has come: nothing
    // freezes an episode by itself, and a show that went out for weeks without the button
    // still has a record to keep (ADR-0279).
    let window = |e: &Episode| {
        series
            .iter()
            .find(|s| s.id == e.series_id)
            .and_then(|s| air_window(&show, s, e))
    };
    let on_air = episodes
        .iter()
        .any(|e| window(e).is_some_and(|(start, end)| start <= now_s && now_s < end));
    let aired = episodes
        .iter()
        .filter(|e| e.aired_at.is_some() || window(e).is_some_and(|(_, end)| end <= now_s))
        .count();
    if aired > 0 || on_air {
        return Err(ShowError::Kept(kept_because_aired(
            &show.name, aired, on_air,
        )));
    }
    let mut cards: Vec<String> = show.cards.iter().flat_map(card_files).collect();
    cards.extend(
        series
            .iter()
            .filter_map(|s| s.cards.as_ref())
            .flat_map(card_files),
    );
    cards.extend(
        episodes
            .iter()
            .filter_map(|e| e.cards.as_ref())
            .flat_map(card_files),
    );
    cards.sort();
    cards.dedup();
    let mut media_files: Vec<String> = episodes
        .iter()
        .filter_map(|e| e.files.as_ref()?.as_object())
        .flat_map(|o| o.values())
        .filter_map(|f| f.get("file")?.as_str().map(str::to_owned))
        .collect();
    media_files.sort();
    media_files.dedup();
    let cart_files: Vec<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT c.id, c.file FROM cart c JOIN cart_bank b ON b.id = c.bank_id WHERE b.show_id = $1",
    )
    .bind(show.id)
    .fetch_all(&mut *tx)
    .await?;
    let (cart_banks,): (i64,) = sqlx::query_as("SELECT count(*) FROM cart_bank WHERE show_id = $1")
        .bind(show.id)
        .fetch_one(&mut *tx)
        .await?;
    // The host keys go with the show too, so their ids are read before they do.
    let key_ids: Vec<(String,)> = sqlx::query_as("SELECT key_id FROM show_key WHERE show_id = $1")
        .bind(show.id)
        .fetch_all(&mut *tx)
        .await?;
    // The series, episodes, cart banks and carts, and the two change logs, go with the show by
    // their foreign keys.
    sqlx::query("DELETE FROM show WHERE id = $1")
        .bind(show.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(DeletedShow {
        slug: show.slug,
        name: show.name,
        series: series.len(),
        episodes: episodes.len(),
        cart_banks: cart_banks as usize,
        carts: cart_files.len(),
        id: show.id,
        card_files: cards,
        media_files,
        cart_files,
        key_ids: key_ids.into_iter().map(|(k,)| k).collect(),
    })
}

/// Every published audio file any episode names now (all stations share the directory).
pub async fn episode_files_in_use(
    pool: &sqlx::PgPool,
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT f.value ->> 'file' FROM episode e, jsonb_each(e.files) f
         WHERE e.files IS NOT NULL AND f.value ->> 'file' IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
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

#[cfg(test)]
mod delete_tests {
    use super::kept_because_aired;

    #[test]
    fn the_refusal_names_how_many_aired_in_one_plain_sentence() {
        assert_eq!(
            kept_because_aired("The Folk Club", 1, false),
            "The Folk Club has aired 1 episode, so it stays as the station's record: only a show that has never aired can be deleted."
        );
        assert_eq!(
            kept_because_aired("The Folk Club", 12, true),
            "The Folk Club has aired 12 episodes, so it stays as the station's record: only a show that has never aired can be deleted."
        );
        assert_eq!(
            kept_because_aired("The Folk Club", 0, true),
            "The Folk Club is on air now, so it cannot be deleted."
        );
    }
}
