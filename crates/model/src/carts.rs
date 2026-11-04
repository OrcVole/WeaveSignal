// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The cart wall's rows (ADR-0020): banks and carts, per station (migration 0003).

use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::StationId;

/// Banks per station: `⌃F1` to `⌃F5`.
pub const BANKS: u8 = 5;
/// Carts per bank: the four rows of six under the left hand (`1`–`6`, `Q`–`Y`, `A`–`H`, `Z`–`N`).
pub const SLOTS: u8 = 24;
/// Slots in the cart palette (design system; ADR-0066).
pub const COLOUR_SLOTS: u8 = 12;

/// How a cart plays (report 08's three modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CartMode {
    /// Plays once.
    OneShot,
    /// Loops until stopped, ducked under an open microphone.
    Bed,
    /// Plays once, and ducks any bed while it sounds.
    Sweeper,
}

impl CartMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CartMode::OneShot => "one_shot",
            CartMode::Bed => "bed",
            CartMode::Sweeper => "sweeper",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "one_shot" => Some(CartMode::OneShot),
            "bed" => Some(CartMode::Bed),
            "sweeper" => Some(CartMode::Sweeper),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Bank {
    /// 1 to [`BANKS`].
    pub number: u8,
    pub name: String,
    /// 1 to [`COLOUR_SLOTS`]: the bank's (and its show's) colour.
    pub colour_slot: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Cart {
    pub id: Uuid,
    pub bank: u8,
    /// 1 to [`SLOTS`].
    pub slot: u8,
    pub label: String,
    /// A second line: an intro's artist, say.
    pub detail: String,
    pub mode: CartMode,
    /// The media worker's FLAC, `<uuid>.flac`; `None` is FILE MISSING.
    pub file: Option<String>,
    pub duration_ms: u32,
    /// Applied at play time, to bring the cart to the station's loudness target.
    pub gain_db: f32,
    /// The file as it came: integrated loudness and true peak.
    pub loudness_lufs: Option<f32>,
    pub true_peak_dbtp: Option<f32>,
    /// Milliseconds to the first vocal, for the talk-up countdown.
    pub ramp_ms: Option<u32>,
    /// Where it came from: an uploaded file's name, or `demo:<id>`.
    pub origin: String,
    pub licence: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CartError {
    #[error("bank {0} does not exist; banks are 1 to {BANKS}")]
    Bank(u8),
    #[error("slot {0} does not exist; slots are 1 to {SLOTS}")]
    Slot(u8),
    #[error("colour slot {0} does not exist; the palette has {COLOUR_SLOTS}")]
    Colour(u8),
    #[error("a {0} is 1 to {1} characters, without control characters")]
    Text(&'static str, usize),
    #[error("`{0}` is not a cart file name")]
    File(String),
    #[error("the gain is -40 to 20 dB")]
    Gain,
}

fn text(what: &'static str, s: &str, min: usize, max: usize) -> Result<(), CartError> {
    let n = s.chars().count();
    if n < min || n > max || s.chars().any(char::is_control) {
        return Err(CartError::Text(what, max));
    }
    Ok(())
}

/// A cart file's name: a UUID and `.flac`, as the media worker's files are named (playout
/// refuses anything else).
pub fn is_cart_file(name: &str) -> bool {
    name.strip_suffix(".flac")
        .and_then(|stem| Uuid::parse_str(stem).ok())
        .is_some_and(|u| name == format!("{}.flac", u.hyphenated()))
}

impl Bank {
    pub fn check(&self) -> Result<(), CartError> {
        if !(1..=BANKS).contains(&self.number) {
            return Err(CartError::Bank(self.number));
        }
        if !(1..=COLOUR_SLOTS).contains(&self.colour_slot) {
            return Err(CartError::Colour(self.colour_slot));
        }
        text("bank name", &self.name, 1, 80)
    }
}

impl Cart {
    pub fn check(&self) -> Result<(), CartError> {
        if !(1..=BANKS).contains(&self.bank) {
            return Err(CartError::Bank(self.bank));
        }
        if !(1..=SLOTS).contains(&self.slot) {
            return Err(CartError::Slot(self.slot));
        }
        text("label", &self.label, 1, 120)?;
        text("detail", &self.detail, 0, 120)?;
        if let Some(f) = &self.file
            && !is_cart_file(f)
        {
            return Err(CartError::File(f.clone()));
        }
        if !(-40.0..=20.0).contains(&self.gain_db) {
            return Err(CartError::Gain);
        }
        Ok(())
    }
}

/// Every bank and cart of a station, banks by number and carts by bank and slot.
pub async fn load(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<(Vec<Bank>, Vec<Cart>), sqlx::Error> {
    let banks = sqlx::query(
        "SELECT number, name, colour_slot FROM cart_bank WHERE station_id = $1 ORDER BY number",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Bank {
        number: r.get::<i16, _>("number") as u8,
        name: r.get("name"),
        colour_slot: r.get::<i16, _>("colour_slot") as u8,
    })
    .collect();
    let carts = sqlx::query(
        "SELECT id, bank, slot, label, detail, mode, file, duration_ms, gain_db, loudness_lufs,
                true_peak_dbtp, ramp_ms, origin, licence
         FROM cart WHERE station_id = $1 ORDER BY bank, slot",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Cart {
        id: r.get("id"),
        bank: r.get::<i16, _>("bank") as u8,
        slot: r.get::<i16, _>("slot") as u8,
        label: r.get("label"),
        detail: r.get("detail"),
        mode: CartMode::parse(r.get("mode")).unwrap_or(CartMode::OneShot),
        file: r.get("file"),
        duration_ms: r.get::<i32, _>("duration_ms").max(0) as u32,
        gain_db: r.get("gain_db"),
        loudness_lufs: r.get("loudness_lufs"),
        true_peak_dbtp: r.get("true_peak_dbtp"),
        ramp_ms: r.get::<Option<i32>, _>("ramp_ms").map(|v| v.max(0) as u32),
        origin: r.get("origin"),
        licence: r.get("licence"),
    })
    .collect();
    Ok((banks, carts))
}

/// Make or rename a bank.
pub async fn put_bank(
    pool: &sqlx::PgPool,
    station: StationId,
    bank: &Bank,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO cart_bank (station_id, number, name, colour_slot) VALUES ($1, $2, $3, $4)
         ON CONFLICT (station_id, number) DO UPDATE
         SET name = EXCLUDED.name, colour_slot = EXCLUDED.colour_slot",
    )
    .bind(station.as_uuid())
    .bind(i16::from(bank.number))
    .bind(&bank.name)
    .bind(i16::from(bank.colour_slot))
    .execute(pool)
    .await?;
    Ok(())
}

/// Put a cart in its bank and slot, replacing what was there. Returns the file the replaced
/// cart used, if another, for the caller to delete once the new row is in.
pub async fn put_cart(
    pool: &sqlx::PgPool,
    station: StationId,
    cart: &Cart,
) -> Result<Option<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let old: Option<Option<String>> = sqlx::query_scalar(
        "DELETE FROM cart WHERE station_id = $1 AND bank = $2 AND slot = $3 RETURNING file",
    )
    .bind(station.as_uuid())
    .bind(i16::from(cart.bank))
    .bind(i16::from(cart.slot))
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO cart (id, station_id, bank, slot, label, detail, mode, file, duration_ms,
                           gain_db, loudness_lufs, true_peak_dbtp, ramp_ms, origin, licence)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(cart.id)
    .bind(station.as_uuid())
    .bind(i16::from(cart.bank))
    .bind(i16::from(cart.slot))
    .bind(&cart.label)
    .bind(&cart.detail)
    .bind(cart.mode.as_str())
    .bind(&cart.file)
    .bind(i32::try_from(cart.duration_ms).unwrap_or(i32::MAX))
    .bind(cart.gain_db)
    .bind(cart.loudness_lufs)
    .bind(cart.true_peak_dbtp)
    .bind(cart.ramp_ms.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
    .bind(&cart.origin)
    .bind(&cart.licence)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(old.flatten().filter(|f| Some(f) != cart.file.as_ref()))
}

/// Empty a slot; returns the cart that was there.
pub async fn delete_cart(
    pool: &sqlx::PgPool,
    station: StationId,
    bank: u8,
    slot: u8,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "DELETE FROM cart WHERE station_id = $1 AND bank = $2 AND slot = $3 RETURNING id",
    )
    .bind(station.as_uuid())
    .bind(i16::from(bank))
    .bind(i16::from(slot))
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart() -> Cart {
        Cart {
            id: Uuid::new_v4(),
            bank: 1,
            slot: 24,
            label: "Applause".into(),
            detail: String::new(),
            mode: CartMode::OneShot,
            file: Some(format!("{}.flac", Uuid::new_v4())),
            duration_ms: 12_010,
            gain_db: -7.0,
            loudness_lufs: Some(-16.0),
            true_peak_dbtp: Some(-1.2),
            ramp_ms: None,
            origin: "demo:applause-1".into(),
            licence: "CC0 1.0".into(),
        }
    }

    #[test]
    fn carts_and_banks_are_checked_before_they_are_kept() {
        assert_eq!(cart().check(), Ok(()));
        let mut c = cart();
        c.slot = 25;
        assert_eq!(c.check(), Err(CartError::Slot(25)));
        let mut c = cart();
        c.bank = 6;
        assert_eq!(c.check(), Err(CartError::Bank(6)));
        let mut c = cart();
        c.file = Some("../../etc/passwd".into());
        assert!(matches!(c.check(), Err(CartError::File(_))));
        let mut c = cart();
        c.label = "two\nlines".into();
        assert!(matches!(c.check(), Err(CartError::Text("label", 120))));
        let b = Bank {
            number: 1,
            name: "Reactions".into(),
            colour_slot: 13,
        };
        assert_eq!(b.check(), Err(CartError::Colour(13)));
    }

    #[test]
    fn only_uuid_flac_names_are_cart_files() {
        assert!(is_cart_file(&format!("{}.flac", Uuid::new_v4())));
        assert!(!is_cart_file("x.flac"));
        assert!(!is_cart_file(&format!("{}.opus", Uuid::new_v4())));
        let upper = format!("{}.flac", Uuid::new_v4())
            .to_uppercase()
            .replace(".FLAC", ".flac");
        assert!(!is_cart_file(&upper), "playout's pattern is lowercase");
    }

    #[test]
    fn modes_use_the_tables_words() {
        for m in [CartMode::OneShot, CartMode::Bed, CartMode::Sweeper] {
            assert_eq!(CartMode::parse(m.as_str()), Some(m));
        }
    }
}
