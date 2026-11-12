// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's data model.
//!
//! The station identifier is in the model from the first migration (ADR-0047): release one
//! shows one station, and every table that follows carries a `station_id`.

pub mod carts;
pub mod passes;
pub mod playout;
pub mod retention;
pub mod setup;
pub mod shows;
pub mod staff;
pub mod tokens;
pub mod webhooks;
// rtmp-out (ADR-0230)
pub mod rtmp;

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The migrations, embedded at build time from `migrations/`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// A station's identifier: a UUID, stable for the station's life.
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
#[serde(transparent)]
pub struct StationId(Uuid);

impl StationId {
    /// A fresh identifier, for a new station.
    pub fn new_random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for StationId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl fmt::Display for StationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for StationId {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| ModelError::BadStationId(s.to_owned()))
    }
}

/// The longest station name, in characters; the migration's CHECK holds the same limit.
pub const STATION_NAME_MAX: usize = 200;

/// A station: what release one's interface shows one of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Station {
    pub id: StationId,
    pub name: String,
}

impl Station {
    /// A station with a checked name: not blank, at most [`STATION_NAME_MAX`] characters.
    pub fn new(id: StationId, name: impl Into<String>) -> Result<Self, ModelError> {
        let name = name.into();
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ModelError::BlankStationName);
        }
        if trimmed.chars().count() > STATION_NAME_MAX {
            return Err(ModelError::StationNameTooLong(STATION_NAME_MAX));
        }
        Ok(Self {
            id,
            name: trimmed.to_owned(),
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("`{0}` is not a station identifier (expected a UUID)")]
    BadStationId(String),
    #[error("the station's name is blank")]
    BlankStationName,
    #[error("the station's name is longer than {0} characters")]
    StationNameTooLong(usize),
}

/// Apply every migration, then make sure `station` has a row for this station. The name stored
/// is the one in force: the one an admin set on the station's setup page (ADR-0237,
/// `station_setup.name`) when there is one, else the settings file's, updated if it changed.
/// Returns the station as stored, so the control plane and the chat start with the same name.
pub async fn ensure_station(
    pool: &sqlx::PgPool,
    station: &Station,
) -> Result<Station, sqlx::Error> {
    MIGRATOR.run(pool).await?;
    let (id, name): (Uuid, String) = sqlx::query_as(
        "INSERT INTO station (id, name) VALUES ($1, $2)
         ON CONFLICT (id) DO UPDATE SET name = COALESCE(
             (SELECT s.name FROM station_setup s WHERE s.station_id = EXCLUDED.id),
             EXCLUDED.name)
         RETURNING id, name",
    )
    .bind(station.id.as_uuid())
    .bind(&station.name)
    .fetch_one(pool)
    .await?;
    Ok(Station {
        id: id.into(),
        name,
    })
}

/// A caller's consent to record, as the log keeps it (migration 0002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentRecord {
    pub identity: String,
    pub name: String,
    pub topic: String,
    /// `caller` (the player page) or `station` (given on their behalf).
    pub given_by: &'static str,
    pub words: String,
    pub words_version: String,
    /// Milliseconds since the Unix epoch.
    pub accepted_at_ms: u64,
    /// The show on air when the consent was given, by the schedule (ADR-0214); `None` when no
    /// show was, or the show has since been deleted.
    pub show_id: Option<uuid::Uuid>,
    /// The presenter the words named (ADR-0214); empty when they named none.
    pub presenter: String,
    /// The takedown contact exactly as the words said it (ADR-0214, ADR-0215): the show's and
    /// then the station's as the host, or the station's alone. A later change to the show
    /// never rewrites it.
    pub contact: String,
}

/// Keep one consent; returns its row's identifier.
pub async fn record_consent(
    pool: &sqlx::PgPool,
    station: StationId,
    c: &ConsentRecord,
) -> Result<i64, sqlx::Error> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO caller_consent
           (station_id, identity, name, topic, given_by, words, words_version, accepted_at,
            show_id, presenter, contact)
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::double precision / 1000.0),
                 $9, $10, $11)
         RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(&c.identity)
    .bind(&c.name)
    .bind(&c.topic)
    .bind(c.given_by)
    .bind(&c.words)
    .bind(&c.words_version)
    .bind(c.accepted_at_ms as f64)
    .bind(c.show_id)
    .bind(&c.presenter)
    .bind(&c.contact)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_id_round_trips_through_text_and_json() {
        let id = StationId::new_random();
        let text = id.to_string();
        assert_eq!(text.parse::<StationId>().unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<StationId>(&json).unwrap(), id);
    }

    #[test]
    fn a_bad_station_id_is_named() {
        let err = "not-a-uuid".parse::<StationId>().unwrap_err();
        assert_eq!(err, ModelError::BadStationId("not-a-uuid".into()));
    }

    #[test]
    fn station_names_are_trimmed_and_bounded() {
        let id = StationId::new_random();
        assert_eq!(Station::new(id, "  Kettle FM  ").unwrap().name, "Kettle FM");
        assert_eq!(Station::new(id, "   "), Err(ModelError::BlankStationName));
        let long = "a".repeat(STATION_NAME_MAX + 1);
        assert_eq!(
            Station::new(id, long),
            Err(ModelError::StationNameTooLong(STATION_NAME_MAX))
        );
    }

    #[test]
    fn the_third_migration_creates_the_cart_wall() {
        // 0003: 0002 is the caller-consent migration on the join-pages branch.
        let carts = MIGRATOR
            .iter()
            .find(|m| m.version == 3)
            .expect("migration 0003");
        assert!(carts.sql.as_str().contains("CREATE TABLE cart_bank"));
        assert!(carts.sql.as_str().contains("station_id"));
    }

    #[test]
    fn the_sixth_migration_creates_the_chat() {
        // 0006: 0004 and 0005 belong to other branches (M6's chat starts at 0006).
        let chat = MIGRATOR
            .iter()
            .find(|m| m.version == 6)
            .expect("migration 0006");
        for table in [
            "chat_account",
            "chat_session",
            "chat_message",
            "chat_report",
            "chat_action",
            "chat_statement",
            "chat_restriction",
            "chat_hold",
            "chat_settings",
        ] {
            assert!(
                chat.sql
                    .as_str()
                    .contains(&format!("CREATE TABLE {table} ")),
                "{table}"
            );
        }
        // Both timestamps from the first schema (ADR-0038).
        assert!(
            chat.sql
                .as_str()
                .contains("server_at   timestamptz NOT NULL")
        );
        assert!(chat.sql.as_str().contains("playhead_at timestamptz"));
    }

    #[test]
    fn the_fourth_migration_holds_playout() {
        let m = MIGRATOR
            .iter()
            .find(|m| m.version == 4)
            .expect("migration 0004");
        for table in [
            "playout_settings",
            "track",
            "clock",
            "schedule_item",
            "play_log",
            "listener_hour",
        ] {
            assert!(
                m.sql.as_str().contains(&format!("CREATE TABLE {table} ")),
                "{table}"
            );
        }
    }

    #[test]
    fn the_first_migration_creates_the_station_table() {
        let first = MIGRATOR.iter().next().expect("at least one migration");
        assert_eq!(first.version, 1);
        assert!(first.sql.as_str().contains("CREATE TABLE station"));
    }

    #[test]
    fn the_fifth_migration_adds_retention() {
        // 0005: 0004 is left to another branch's migration.
        let m = MIGRATOR
            .iter()
            .find(|m| m.version == 5)
            .expect("migration 0005");
        for table in [
            "retention_setting",
            "recording",
            "consent_recording",
            "retention_hold",
        ] {
            assert!(
                m.sql.as_str().contains(&format!("CREATE TABLE {table} ")),
                "{table}"
            );
        }
    }

    #[test]
    fn the_second_migration_keeps_callers_consent() {
        let second = MIGRATOR.iter().nth(1).expect("a second migration");
        assert_eq!(second.version, 2);
        assert!(second.sql.as_str().contains("CREATE TABLE caller_consent"));
    }
}
