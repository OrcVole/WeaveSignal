// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What the station's setup page stores (ADR-0237): the name and takedown contact an admin
//! gave on the running station (migration 0028), and the station's look on its public pages
//! (migration 0029).
//!
//! The settings file gives the name and the contact their first values; a value stored here
//! wins, and `None` means the file's is in force, as the station's time zone already works
//! (`playout::seed_time_zone`). The name in force is also kept in `station.name`, where the
//! chat reads it when it starts: [`crate::ensure_station`] writes the stored name there, not
//! the file's, once there is one.

use sqlx::PgPool;

use crate::StationId;

/// The longest takedown contact, in characters (the settings file's rule, `join.consent_contact`).
pub const CONTACT_MAX: usize = 200;

/// The setup page's answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    /// The name set on the page; `None` leaves the settings file's in force.
    pub name: Option<String>,
    /// The takedown contact set on the page; `None` leaves the settings file's in force.
    pub consent_contact: Option<String>,
    /// When an admin finished setup, in milliseconds since the Unix epoch.
    pub completed_at_ms: Option<u64>,
    pub updated_by: Option<String>,
    pub updated_at_ms: Option<u64>,
}

type SetupRow = (Option<String>, Option<String>, Option<f64>, String, f64);

/// The setup page's answers; the default when nobody has saved any.
pub async fn load_setup(pool: &PgPool, station: StationId) -> Result<Setup, sqlx::Error> {
    let row: Option<SetupRow> = sqlx::query_as(
        "SELECT name, consent_contact, extract(epoch FROM completed_at)::float8 * 1000,
                updated_by, extract(epoch FROM updated_at)::float8 * 1000
           FROM station_setup WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        None => Setup::default(),
        Some((name, consent_contact, completed, by, at)) => Setup {
            name,
            consent_contact,
            completed_at_ms: completed.map(|ms| ms as u64),
            updated_by: Some(by),
            updated_at_ms: Some(at as u64),
        },
    })
}

/// Store the page's name and contact (`None` hands either back to the settings file), and
/// write the name now in force (`name`, else `file_name`) into `station.name`. All or nothing.
pub async fn save_setup(
    pool: &PgPool,
    station: StationId,
    name: Option<&str>,
    consent_contact: Option<&str>,
    file_name: &str,
    by: &str,
) -> Result<Setup, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO station_setup (station_id, name, consent_contact, updated_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (station_id) DO UPDATE
           SET name = EXCLUDED.name, consent_contact = EXCLUDED.consent_contact,
               updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(station.as_uuid())
    .bind(name)
    .bind(consent_contact)
    .bind(by)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE station SET name = $2 WHERE id = $1")
        .bind(station.as_uuid())
        .bind(name.unwrap_or(file_name))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    load_setup(pool, station).await
}

/// Mark setup finished (once: a later call keeps the first time).
pub async fn complete_setup(
    pool: &PgPool,
    station: StationId,
    by: &str,
) -> Result<Setup, sqlx::Error> {
    sqlx::query(
        "INSERT INTO station_setup (station_id, completed_at, updated_by)
         VALUES ($1, now(), $2)
         ON CONFLICT (station_id) DO UPDATE
           SET completed_at = COALESCE(station_setup.completed_at, now()),
               updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(station.as_uuid())
    .bind(by)
    .execute(pool)
    .await?;
    load_setup(pool, station).await
}

/// One brand picture as stored, without its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The first 16 hex digits of the PNG's SHA-256: its address carries it.
    pub version: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u32,
}

/// The station's look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branding {
    /// `#rrggbb`, lower case.
    pub accent: Option<String>,
    pub icon: Option<Picture>,
    pub banner: Option<Picture>,
    pub updated_by: Option<String>,
    pub updated_at_ms: Option<u64>,
}

/// Which brand picture: the column names are fixed here, never taken from a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureKind {
    Icon,
    Banner,
}

impl PictureKind {
    fn column(self) -> &'static str {
        match self {
            PictureKind::Icon => "icon",
            PictureKind::Banner => "banner",
        }
    }
}

type BrandingRow = (
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    String,
    f64,
);

fn picture(
    version: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    bytes: Option<i32>,
) -> Option<Picture> {
    Some(Picture {
        version: version?,
        width: u32::try_from(width?).ok()?,
        height: u32::try_from(height?).ok()?,
        bytes: u32::try_from(bytes?).ok()?,
    })
}

/// The station's look, without the pictures' bytes; the default when none is set.
pub async fn load_branding(pool: &PgPool, station: StationId) -> Result<Branding, sqlx::Error> {
    let row: Option<BrandingRow> = sqlx::query_as(
        "SELECT accent,
                icon_version, icon_width, icon_height, octet_length(icon),
                banner_version, banner_width, banner_height, octet_length(banner),
                updated_by, extract(epoch FROM updated_at)::float8 * 1000
           FROM station_branding WHERE station_id = $1",
    )
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        None => Branding::default(),
        Some((accent, iv, iw, ih, ib, bv, bw, bh, bb, by, at)) => Branding {
            accent,
            icon: picture(iv, iw, ih, ib),
            banner: picture(bv, bw, bh, bb),
            updated_by: Some(by),
            updated_at_ms: Some(at as u64),
        },
    })
}

/// Set the accent (`#rrggbb`, lower case; the caller checks it) or clear it.
pub async fn set_accent(
    pool: &PgPool,
    station: StationId,
    accent: Option<&str>,
    by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO station_branding (station_id, accent, updated_by) VALUES ($1, $2, $3)
         ON CONFLICT (station_id) DO UPDATE
           SET accent = EXCLUDED.accent, updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(station.as_uuid())
    .bind(accent)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

/// A brand picture to store: the PNG the media worker wrote, its version and its size.
pub struct NewPicture<'a> {
    pub png: &'a [u8],
    pub version: &'a str,
    pub width: u32,
    pub height: u32,
}

/// Store a brand picture.
pub async fn set_picture(
    pool: &PgPool,
    station: StationId,
    kind: PictureKind,
    picture: NewPicture<'_>,
    by: &str,
) -> Result<(), sqlx::Error> {
    let c = kind.column();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO station_branding (station_id, {c}, {c}_version, {c}_width, {c}_height, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (station_id) DO UPDATE
           SET {c} = EXCLUDED.{c}, {c}_version = EXCLUDED.{c}_version,
               {c}_width = EXCLUDED.{c}_width, {c}_height = EXCLUDED.{c}_height,
               updated_by = EXCLUDED.updated_by, updated_at = now()"
    )))
    .bind(station.as_uuid())
    .bind(picture.png)
    .bind(picture.version)
    .bind(i32::try_from(picture.width).unwrap_or(i32::MAX))
    .bind(i32::try_from(picture.height).unwrap_or(i32::MAX))
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a brand picture. `false` when there was none.
pub async fn clear_picture(
    pool: &PgPool,
    station: StationId,
    kind: PictureKind,
    by: &str,
) -> Result<bool, sqlx::Error> {
    let c = kind.column();
    let done = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE station_branding
            SET {c} = NULL, {c}_version = NULL, {c}_width = NULL, {c}_height = NULL,
                updated_by = $2, updated_at = now()
          WHERE station_id = $1 AND {c} IS NOT NULL"
    )))
    .bind(station.as_uuid())
    .bind(by)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// A brand picture's PNG and its version, for serving.
pub async fn picture_bytes(
    pool: &PgPool,
    station: StationId,
    kind: PictureKind,
) -> Result<Option<(Vec<u8>, String)>, sqlx::Error> {
    let c = kind.column();
    let row: Option<(Option<Vec<u8>>, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(
        format!("SELECT {c}, {c}_version FROM station_branding WHERE station_id = $1"),
    ))
    .bind(station.as_uuid())
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((Some(png), Some(version))) => Some((png, version)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    /// The website's wizard offers the Jurisdiction presets by key and name
    /// (`packages/wizard/presets.ts`); this holds its list against the presets in the code, so
    /// a preset added, renamed or re-licensed fails here until the wizard follows. Custom is
    /// left out there on purpose.
    #[test]
    fn the_wizards_preset_list_is_the_codes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/wizard/presets.ts"
        );
        let text = std::fs::read_to_string(path).expect("packages/wizard/presets.ts");
        let listed: Vec<String> = text
            .lines()
            .filter(|l| l.trim_start().starts_with("{ key: '"))
            .map(|l| l.trim().trim_end_matches(',').to_owned())
            .collect();
        let expected: Vec<String> = crate::retention::PRESETS
            .iter()
            .filter(|p| p.key != crate::retention::CUSTOM)
            .map(|p| {
                format!(
                    "{{ key: '{}', name: '{}', licensed: {} }}",
                    p.key, p.name, p.licensed
                )
            })
            .collect();
        assert_eq!(listed, expected);
    }
}
