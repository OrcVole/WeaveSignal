// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The telephone line's rows (Phase 17f, migration 0046): a phone call's consent with its ban
//! key and last digits, number bans by ban key, and the station's own recording of the phone
//! words. No phone number is ever stored ([`crate::caller_id`]).

use crate::{ConsentRecord, StationId};

/// A phone call's line fields beside its consent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneLine {
    /// `None` for a withheld caller.
    pub ban_key: Option<String>,
    /// The last digits LiveKit left (at most four).
    pub number_tail: String,
}

/// Keep a phone caller's consent (or, with the words off, the record that none was asked:
/// `given_by` `not_asked`); returns its row's identifier.
pub async fn record_phone_consent(
    pool: &sqlx::PgPool,
    station: StationId,
    c: &ConsentRecord,
    line: &PhoneLine,
) -> Result<i64, sqlx::Error> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO caller_consent
           (station_id, identity, name, topic, given_by, words, words_version, accepted_at,
            show_id, presenter, contact, line, ban_key, number_tail)
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::double precision / 1000.0),
                 $9, $10, $11, 'phone', $12, $13)
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
    .bind(&line.ban_key)
    .bind(&line.number_tail)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// The consents of the calls a takedown's number may have come in as: their row identifiers,
/// newest first. The typed number is never passed here, only its ban keys.
pub async fn consents_by_ban_keys(
    pool: &sqlx::PgPool,
    station: StationId,
    ban_keys: &[String],
) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM caller_consent
         WHERE station_id = $1 AND line = 'phone' AND ban_key = ANY($2)
         ORDER BY accepted_at DESC, id DESC",
    )
    .bind(station.as_uuid())
    .bind(ban_keys)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// A number's ban as the database keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneBanRecord {
    pub ban_key: String,
    pub number_tail: String,
    pub reason: String,
    pub made_by: String,
    /// Milliseconds since the Unix epoch.
    pub made_at_ms: i64,
    pub expires_at_ms: Option<i64>,
}

/// Every ban of the station that has not lapsed at `now_ms`.
pub async fn bans(
    pool: &sqlx::PgPool,
    station: StationId,
    now_ms: i64,
) -> Result<Vec<PhoneBanRecord>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, f64, Option<f64>)> = sqlx::query_as(
        "SELECT ban_key, number_tail, reason, made_by,
                extract(epoch FROM made_at)::double precision * 1000,
                extract(epoch FROM expires_at)::double precision * 1000
         FROM phone_ban
         WHERE station_id = $1
           AND (expires_at IS NULL OR expires_at > to_timestamp($2::double precision / 1000.0))
         ORDER BY made_at, ban_key",
    )
    .bind(station.as_uuid())
    .bind(now_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(ban_key, number_tail, reason, made_by, at, exp)| PhoneBanRecord {
                ban_key,
                number_tail,
                reason,
                made_by,
                made_at_ms: at.round() as i64,
                expires_at_ms: exp.map(|e| e.round() as i64),
            },
        )
        .collect())
}

/// Ban a number by its ban key, or renew its ban (the reason, the maker and the end replaced).
pub async fn put_ban(
    pool: &sqlx::PgPool,
    station: StationId,
    b: &PhoneBanRecord,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO phone_ban (station_id, ban_key, number_tail, reason, made_by, made_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, to_timestamp($6::double precision / 1000.0),
                 to_timestamp($7::double precision / 1000.0))
         ON CONFLICT (station_id, ban_key) DO UPDATE
           SET reason = EXCLUDED.reason, made_by = EXCLUDED.made_by,
               made_at = EXCLUDED.made_at, expires_at = EXCLUDED.expires_at",
    )
    .bind(station.as_uuid())
    .bind(&b.ban_key)
    .bind(&b.number_tail)
    .bind(&b.reason)
    .bind(&b.made_by)
    .bind(b.made_at_ms as f64)
    .bind(b.expires_at_ms.map(|e| e as f64))
    .execute(pool)
    .await?;
    Ok(())
}

/// Lift a ban; whether there was one.
pub async fn lift_ban(
    pool: &sqlx::PgPool,
    station: StationId,
    ban_key: &str,
) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM phone_ban WHERE station_id = $1 AND ban_key = $2")
            .bind(station.as_uuid())
            .bind(ban_key)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

/// The station's own recording of the phone words.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneWordsRecord {
    pub words: String,
    pub words_version: String,
    /// `<uuid>.flac` in the phone-words directory.
    pub file: String,
    pub duration_ms: i32,
    /// The gain the desk plays it at (the media worker's play gain), in dB.
    pub gain_db: f32,
    pub recorded_by: String,
    pub recorded_at_ms: i64,
}

/// The station's recording, if it has one.
pub async fn words(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Option<PhoneWordsRecord>, sqlx::Error> {
    let row: Option<(String, String, String, i32, f32, String, f64)> = sqlx::query_as(
        "SELECT words, words_version, file, duration_ms, gain_db, recorded_by,
                extract(epoch FROM recorded_at)::double precision * 1000
         FROM phone_words WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(words, words_version, file, duration_ms, gain_db, recorded_by, at)| PhoneWordsRecord {
            words,
            words_version,
            file,
            duration_ms,
            gain_db,
            recorded_by,
            recorded_at_ms: at.round() as i64,
        },
    ))
}

/// Keep the station's recording, replacing the one before; returns the old file, which the
/// caller deletes once nothing plays it.
pub async fn put_words(
    pool: &sqlx::PgPool,
    station: StationId,
    w: &PhoneWordsRecord,
) -> Result<Option<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let old: Option<(String,)> =
        sqlx::query_as("SELECT file FROM phone_words WHERE station_id = $1 FOR UPDATE")
            .bind(station.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
    sqlx::query(
        "INSERT INTO phone_words
           (station_id, words, words_version, file, duration_ms, gain_db, recorded_by, recorded_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::double precision / 1000.0))
         ON CONFLICT (station_id) DO UPDATE
           SET words = EXCLUDED.words, words_version = EXCLUDED.words_version,
               file = EXCLUDED.file, duration_ms = EXCLUDED.duration_ms, gain_db = EXCLUDED.gain_db,
               recorded_by = EXCLUDED.recorded_by, recorded_at = EXCLUDED.recorded_at",
    )
    .bind(station.as_uuid())
    .bind(&w.words)
    .bind(&w.words_version)
    .bind(&w.file)
    .bind(w.duration_ms)
    .bind(w.gain_db)
    .bind(&w.recorded_by)
    .bind(w.recorded_at_ms as f64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(old.map(|(f,)| f).filter(|f| f != &w.file))
}

/// Remove the station's recording (the shipped default plays again); returns its file.
pub async fn delete_words(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("DELETE FROM phone_words WHERE station_id = $1 RETURNING file")
            .bind(station.as_uuid())
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(f,)| f))
}

/// The phone line's settings an admin changed from the studio (migration 0047): each `None`
/// until set there, when the installation's setting applies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhoneSettingsRecord {
    pub consent_words: Option<bool>,
    /// `alba`, `edinburgh` or `own`.
    pub default_voice: Option<String>,
}

/// The settings an admin changed, if any.
pub async fn settings(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<PhoneSettingsRecord, sqlx::Error> {
    let row: Option<(Option<bool>, Option<String>)> = sqlx::query_as(
        "SELECT consent_words, default_voice FROM phone_settings WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|(consent_words, default_voice)| PhoneSettingsRecord {
            consent_words,
            default_voice,
        })
        .unwrap_or_default())
}

/// Change the settings: a field that is `None` is left as it was. Returns them as kept.
pub async fn put_settings(
    pool: &sqlx::PgPool,
    station: StationId,
    change: &PhoneSettingsRecord,
    by: &str,
) -> Result<PhoneSettingsRecord, sqlx::Error> {
    let (consent_words, default_voice): (Option<bool>, Option<String>) = sqlx::query_as(
        "INSERT INTO phone_settings (station_id, consent_words, default_voice, updated_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (station_id) DO UPDATE
           SET consent_words = COALESCE(EXCLUDED.consent_words, phone_settings.consent_words),
               default_voice = COALESCE(EXCLUDED.default_voice, phone_settings.default_voice),
               updated_by = EXCLUDED.updated_by, updated_at = now()
         RETURNING consent_words, default_voice",
    )
    .bind(station.as_uuid())
    .bind(change.consent_words)
    .bind(change.default_voice.as_deref())
    .bind(by)
    .fetch_one(pool)
    .await?;
    Ok(PhoneSettingsRecord {
        consent_words,
        default_voice,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn migration_0047_keeps_two_settings_and_no_number() {
        let m = crate::MIGRATOR
            .iter()
            .find(|m| m.version == 47)
            .expect("migration 0047");
        let sql = m.sql.as_str();
        assert!(sql.contains("CREATE TABLE phone_settings"));
        assert!(sql.contains("'alba', 'edinburgh', 'own'"));
        assert!(!sql.contains("number"));
    }

    #[test]
    fn migration_0046_keeps_no_number() {
        let m = crate::MIGRATOR
            .iter()
            .find(|m| m.version == 46)
            .expect("migration 0046");
        let sql = m.sql.as_str();
        for t in [
            "phone_ban",
            "phone_words",
            "ban_key",
            "number_tail",
            "not_asked",
        ] {
            assert!(sql.contains(t), "{t}");
        }
        // No column for a number, in any spelling.
        for bad in [
            " number text",
            "phone_number",
            "from_user",
            "caller_id text",
        ] {
            assert!(!sql.contains(bad), "{bad}");
        }
    }
}
