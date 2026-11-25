// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows' queries (migration 0071): show access rules, listener keys, families, devices,
//! use records and the leak report's figures. The rules themselves are in
//! [`crate::listener_keys`]; the control plane seals and hashes (`crates/control/src/access.rs`),
//! and this file stores and finds what it is given, so it never sees a key's text.
//!
//! **Limits are checked under the key's row lock.** A redeem selects the key `FOR UPDATE`, counts
//! its devices and writes the new one in one transaction, so two phones entering the last place
//! at once cannot both get it ([`begin_listen`]).

use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::StationId;
use crate::listener_keys::{self as rules, Key, Kind, LeakStats, Media, Refusal, State};

macro_rules! ms {
    ($col:literal) => {
        concat!("(extract(epoch FROM ", $col, ") ", "* 1000)::bigint")
    };
}

// ---- a show's access rule ----

/// A show's access rule, as kept. A show with no row is public.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ShowAccess)]
pub struct Access {
    pub show_id: Uuid,
    /// `public` or `key`.
    pub rule: String,
    /// From this moment the show is public whatever the rule says (Unix milliseconds).
    pub opens_at_ms: Option<i64>,
    /// What the public hears while the show is keyed: `announcement`, `fallback` or `teaser`.
    pub slate: String,
    /// A line the announcement slate shows (a sign-up address, when the public programme
    /// returns); empty: the station's default.
    pub slate_note: String,
    pub updated_ms: i64,
    pub updated_by: String,
}

const ACCESS_COLUMNS: &str = concat!(
    "show_id, rule, ",
    ms!("opens_at"),
    ", slate, slate_note, ",
    ms!("updated_at"),
    ", updated_by"
);

fn access_of(r: &sqlx::postgres::PgRow) -> Result<Access, sqlx::Error> {
    Ok(Access {
        show_id: r.try_get(0)?,
        rule: r.try_get(1)?,
        opens_at_ms: r.try_get(2)?,
        slate: r.try_get(3)?,
        slate_note: r.try_get(4)?,
        updated_ms: r.try_get(5)?,
        updated_by: r.try_get(6)?,
    })
}

pub async fn get_access(
    pool: &sqlx::PgPool,
    station: StationId,
    show: Uuid,
) -> Result<Option<Access>, sqlx::Error> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {ACCESS_COLUMNS} FROM show_access WHERE station_id = $1 AND show_id = $2"
    )))
    .bind(station.as_uuid())
    .bind(show)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(access_of).transpose()
}

/// Every show that has a rule row (a show without one is public).
pub async fn list_access(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Access>, sqlx::Error> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {ACCESS_COLUMNS} FROM show_access WHERE station_id = $1 ORDER BY show_id"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    rows.iter().map(access_of).collect()
}

/// Set a show's rule; one row per show.
pub async fn set_access(
    pool: &sqlx::PgPool,
    station: StationId,
    a: &Access,
    by: &str,
) -> Result<Access, sqlx::Error> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO show_access (station_id, show_id, rule, opens_at, slate, slate_note, updated_by)
         VALUES ($1, $2, $3, CASE WHEN $4::float8 IS NULL THEN NULL ELSE to_timestamp($4::float8 / 1000) END, $5, $6, $7)
         ON CONFLICT (station_id, show_id) DO UPDATE
           SET rule = EXCLUDED.rule, opens_at = EXCLUDED.opens_at, slate = EXCLUDED.slate,
               slate_note = EXCLUDED.slate_note, updated_at = now(), updated_by = EXCLUDED.updated_by
         RETURNING {ACCESS_COLUMNS}"
    )))
    .bind(station.as_uuid())
    .bind(a.show_id)
    .bind(&a.rule)
    .bind(a.opens_at_ms.map(|m| m as f64))
    .bind(&a.slate)
    .bind(&a.slate_note)
    .bind(by)
    .fetch_one(pool)
    .await?;
    access_of(&row)
}

/// A keyed show as the public page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedShow {
    pub access: Access,
    pub slug: String,
    pub name: String,
}

/// The shows with rule `key`, by name: what the player and the schedule mark with a lock.
pub async fn list_keyed(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<KeyedShow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT a.show_id, a.rule, (extract(epoch FROM a.opens_at) * 1000)::bigint, a.slate, a.slate_note,
                (extract(epoch FROM a.updated_at) * 1000)::bigint, a.updated_by, s.slug, s.name
         FROM show_access a JOIN show s ON s.station_id = a.station_id AND s.id = a.show_id
         WHERE a.station_id = $1 AND a.rule = 'key' ORDER BY s.name, s.id",
    )
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(KeyedShow {
                access: access_of(r)?,
                slug: r.try_get(7)?,
                name: r.try_get(8)?,
            })
        })
        .collect()
}

/// Id, slug and name of these shows.
pub async fn show_refs(
    pool: &sqlx::PgPool,
    station: StationId,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, slug, name FROM show WHERE station_id = $1 AND id = ANY($2) ORDER BY name, id",
    )
    .bind(station.as_uuid())
    .bind(ids)
    .fetch_all(pool)
    .await
}

// ---- families ----

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyFamily)]
pub struct Family {
    pub id: Uuid,
    pub label: String,
    pub note: String,
    pub created_ms: i64,
    pub created_by: String,
    /// How many keys it has issued (replaced ones included).
    pub keys: u32,
}

fn family_of(r: &sqlx::postgres::PgRow) -> Result<Family, sqlx::Error> {
    Ok(Family {
        id: r.try_get(0)?,
        label: r.try_get(1)?,
        note: r.try_get(2)?,
        created_ms: r.try_get(3)?,
        created_by: r.try_get(4)?,
        keys: r.try_get::<i64, _>(5)? as u32,
    })
}

const FAMILY_SELECT: &str = concat!(
    "SELECT f.id, f.label, f.note, ",
    ms!("f.created_at"),
    ", f.created_by, (SELECT count(*) FROM listener_key k WHERE k.station_id = f.station_id AND k.family_id = f.id) FROM listener_key_family f"
);

/// Make a family. The label is the station's own: two families do not share one (the unique
/// index answers, and the caller says so).
pub async fn create_family(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
    label: &str,
    note: &str,
    by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO listener_key_family (id, station_id, label, note, created_by)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(station.as_uuid())
    .bind(label)
    .bind(note)
    .bind(by)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_families(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Family>, sqlx::Error> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{FAMILY_SELECT} WHERE f.station_id = $1 ORDER BY f.created_at DESC, f.id"
    )))
    .bind(station.as_uuid())
    .fetch_all(pool)
    .await?;
    rows.iter().map(family_of).collect()
}

pub async fn get_family(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Family>, sqlx::Error> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{FAMILY_SELECT} WHERE f.station_id = $1 AND f.id = $2"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(family_of).transpose()
}

// ---- keys ----

/// What a new key is made from. The sealed text and email, and their hashes, come from the
/// control plane.
pub struct NewKey<'a> {
    pub id: Uuid,
    pub family_id: Option<Uuid>,
    pub kind: Kind,
    pub label: &'a str,
    pub note: &'a str,
    /// The email's keyed hash, nonce and sealed form.
    pub email: Option<(&'a [u8; 32], &'a [u8], &'a [u8])>,
    pub media: Media,
    pub all_shows: bool,
    pub shows: &'a [Uuid],
    pub valid_from_ms: Option<i64>,
    pub valid_until_ms: Option<i64>,
    pub max_devices: Option<u32>,
    pub max_listens: Option<u32>,
    pub max_people: Option<u32>,
    pub text_hash: &'a [u8; 32],
    pub text_nonce: &'a [u8],
    pub text_sealed: &'a [u8],
    pub replaces_id: Option<Uuid>,
    pub created_by: &'a str,
}

/// The columns of a key, then `shows` as an aggregate: `key_of` reads them by position.
const KEY_SELECT: &str = concat!(
    "SELECT k.id, k.family_id, k.kind, k.label, k.note, (k.email_hash IS NOT NULL), k.media, k.all_shows, ",
    "COALESCE((SELECT array_agg(s.show_id ORDER BY s.show_id) FROM listener_key_show s WHERE s.key_id = k.id), '{}'), ",
    ms!("k.valid_from"),
    ", ",
    ms!("k.valid_until"),
    ", k.max_devices, k.max_listens, k.max_people, k.state, k.state_reason, k.listens_used, ",
    ms!("k.first_used_at"),
    ", ",
    ms!("k.last_used_at"),
    ", k.replaces_id, k.replaced_by_id, ",
    ms!("k.created_at"),
    ", k.created_by, ",
    ms!("k.updated_at"),
    " FROM listener_key k"
);

fn opt_u32(r: &sqlx::postgres::PgRow, i: usize) -> Result<Option<u32>, sqlx::Error> {
    Ok(r.try_get::<Option<i32>, _>(i)?.map(|v| v as u32))
}

fn key_of(r: &sqlx::postgres::PgRow) -> Result<Key, sqlx::Error> {
    let bad =
        |what: &str| sqlx::Error::Decode(format!("a listener key with an unknown {what}").into());
    Ok(Key {
        id: r.try_get(0)?,
        family_id: r.try_get(1)?,
        kind: Kind::parse(&r.try_get::<String, _>(2)?).ok_or_else(|| bad("kind"))?,
        label: r.try_get(3)?,
        note: r.try_get(4)?,
        has_email: r.try_get(5)?,
        media: Media::parse(&r.try_get::<String, _>(6)?).ok_or_else(|| bad("media"))?,
        all_shows: r.try_get(7)?,
        shows: r.try_get(8)?,
        valid_from_ms: r.try_get(9)?,
        valid_until_ms: r.try_get(10)?,
        max_devices: opt_u32(r, 11)?,
        max_listens: opt_u32(r, 12)?,
        max_people: opt_u32(r, 13)?,
        state: State::parse(&r.try_get::<String, _>(14)?).ok_or_else(|| bad("state"))?,
        state_reason: r.try_get(15)?,
        listens_used: r.try_get::<i32, _>(16)? as u32,
        first_used_ms: r.try_get(17)?,
        last_used_ms: r.try_get(18)?,
        replaces_id: r.try_get(19)?,
        replaced_by_id: r.try_get(20)?,
        created_ms: r.try_get(21)?,
        created_by: r.try_get(22)?,
        updated_ms: r.try_get(23)?,
    })
}

/// Make a key, with the shows it opens, and log it. Pass a transaction to make several at once.
pub async fn create_key(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    k: &NewKey<'_>,
) -> Result<Key, sqlx::Error> {
    sqlx::query(
        "INSERT INTO listener_key
           (id, station_id, family_id, kind, label, note, email_hash, email_nonce, email_sealed,
            media, all_shows, valid_from, valid_until, max_devices, max_listens, max_people,
            text_hash, text_nonce, text_sealed, replaces_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            CASE WHEN $12::float8 IS NULL THEN NULL ELSE to_timestamp($12::float8 / 1000) END,
            CASE WHEN $13::float8 IS NULL THEN NULL ELSE to_timestamp($13::float8 / 1000) END,
            $14, $15, $16, $17, $18, $19, $20, $21)",
    )
    .bind(k.id)
    .bind(station.as_uuid())
    .bind(k.family_id)
    .bind(k.kind.as_str())
    .bind(k.label)
    .bind(k.note)
    .bind(k.email.map(|e| e.0.to_vec()))
    .bind(k.email.map(|e| e.1.to_vec()))
    .bind(k.email.map(|e| e.2.to_vec()))
    .bind(k.media.as_str())
    .bind(k.all_shows)
    .bind(k.valid_from_ms.map(|m| m as f64))
    .bind(k.valid_until_ms.map(|m| m as f64))
    .bind(k.max_devices.map(|v| v as i32))
    .bind(k.max_listens.map(|v| v as i32))
    .bind(k.max_people.map(|v| v as i32))
    .bind(k.text_hash.to_vec())
    .bind(k.text_nonce)
    .bind(k.text_sealed)
    .bind(k.replaces_id)
    .bind(k.created_by)
    .execute(&mut *tx)
    .await?;
    set_key_shows(tx, station, k.id, k.shows).await?;
    log(
        tx,
        station,
        k.id,
        k.created_by,
        "created",
        serde_json::json!({ "kind": k.kind.as_str(), "replaces": k.replaces_id }),
    )
    .await?;
    Ok(get_key_conn(tx, station, k.id)
        .await?
        .expect("the key just made"))
}

/// The shows a key opens, replacing the set.
pub async fn set_key_shows(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    key: Uuid,
    shows: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM listener_key_show WHERE station_id = $1 AND key_id = $2")
        .bind(station.as_uuid())
        .bind(key)
        .execute(&mut *tx)
        .await?;
    for s in shows {
        sqlx::query(
            "INSERT INTO listener_key_show (station_id, key_id, show_id) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(station.as_uuid())
        .bind(key)
        .bind(s)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

pub async fn get_key_conn(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
) -> Result<Option<Key>, sqlx::Error> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{KEY_SELECT} WHERE k.station_id = $1 AND k.id = $2"
    )))
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(key_of).transpose()
}

pub async fn get_key(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Key>, sqlx::Error> {
    let mut c = pool.acquire().await?;
    get_key_conn(&mut c, station, id).await
}

/// A key found by the keyed hash of what a person typed.
pub async fn find_by_text(
    pool: &sqlx::PgPool,
    station: StationId,
    text_hash: &[u8; 32],
) -> Result<Option<Key>, sqlx::Error> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{KEY_SELECT} WHERE k.station_id = $1 AND k.text_hash = $2"
    )))
    .bind(station.as_uuid())
    .bind(text_hash.to_vec())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(key_of).transpose()
}

/// What narrows the list.
#[derive(Default)]
pub struct Filter<'a> {
    pub family: Option<Uuid>,
    pub state: Option<State>,
    pub kind: Option<Kind>,
    /// A show it opens (or `all_shows`).
    pub show: Option<Uuid>,
    /// A part of the label, the note or the ticket number, case ignored.
    pub text: Option<&'a str>,
    pub limit: i64,
    pub offset: i64,
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Keys, newest first, with the total that matches.
pub async fn list_keys(
    pool: &sqlx::PgPool,
    station: StationId,
    f: &Filter<'_>,
) -> Result<(Vec<Key>, i64), sqlx::Error> {
    let wher = "WHERE k.station_id = $1
         AND ($2::uuid IS NULL OR k.family_id = $2)
         AND ($3::text IS NULL OR k.state = $3)
         AND ($4::text IS NULL OR k.kind = $4)
         AND ($5::uuid IS NULL OR k.all_shows OR EXISTS (SELECT 1 FROM listener_key_show s WHERE s.key_id = k.id AND s.show_id = $5))
         AND ($6::text IS NULL OR k.label ILIKE $6 OR k.note ILIKE $6)";
    let pat = f.text.map(|t| format!("%{}%", like_escape(t)));
    let total: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM listener_key k {wher}"
    )))
    .bind(station.as_uuid())
    .bind(f.family)
    .bind(f.state.map(State::as_str))
    .bind(f.kind.map(Kind::as_str))
    .bind(f.show)
    .bind(pat.as_deref())
    .fetch_one(pool)
    .await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{KEY_SELECT} {wher} ORDER BY k.created_at DESC, k.id LIMIT $7 OFFSET $8"
    )))
    .bind(station.as_uuid())
    .bind(f.family)
    .bind(f.state.map(State::as_str))
    .bind(f.kind.map(Kind::as_str))
    .bind(f.show)
    .bind(pat.as_deref())
    .bind(f.limit)
    .bind(f.offset)
    .fetch_all(pool)
    .await?;
    Ok((rows.iter().map(key_of).collect::<Result<_, _>>()?, total))
}

/// The sealed text of a key: nonce and sealed form.
pub async fn sealed_text(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<(Vec<u8>, Vec<u8>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT text_nonce, text_sealed FROM listener_key WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The sealed email of a key: nonce and sealed form.
pub async fn sealed_email(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<(Vec<u8>, Vec<u8>)>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let row: Option<(Option<Vec<u8>>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT email_nonce, email_sealed FROM listener_key WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(n, s)| Some((n?, s?))))
}

/// What a change sets; `None` leaves the field.
#[derive(Default)]
pub struct Change<'a> {
    pub label: Option<&'a str>,
    pub note: Option<&'a str>,
    /// `Some(None)` removes the email; `Some(Some(..))` sets it.
    #[allow(clippy::type_complexity)]
    pub email: Option<Option<(&'a [u8; 32], &'a [u8], &'a [u8])>>,
    pub media: Option<Media>,
    pub all_shows: Option<bool>,
    pub shows: Option<&'a [Uuid]>,
    /// `Some(None)` removes the bound.
    pub valid_from_ms: Option<Option<i64>>,
    pub valid_until_ms: Option<Option<i64>>,
    pub max_devices: Option<Option<u32>>,
    pub max_listens: Option<Option<u32>>,
    pub max_people: Option<Option<u32>>,
}

/// Change a key in place: its text stays the same, so nothing needs sending again. A key that
/// is no longer over its end (`valid_until` moved on) may send `key.ended` again.
pub async fn change_key(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
    c: &Change<'_>,
    by: &str,
    detail: serde_json::Value,
    action: &str,
) -> Result<Option<Key>, sqlx::Error> {
    let found: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM listener_key WHERE station_id = $1 AND id = $2 FOR UPDATE")
            .bind(station.as_uuid())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if found.is_none() {
        return Ok(None);
    }
    // One statement with a CASE per field keeps the change atomic and the list of columns short.
    let (set_email, e_hash, e_nonce, e_sealed) = match c.email {
        None => (false, None, None, None),
        Some(None) => (true, None, None, None),
        Some(Some((h, n, s))) => (true, Some(h.to_vec()), Some(n.to_vec()), Some(s.to_vec())),
    };
    let times = |o: Option<Option<i64>>| match o {
        None => (false, None),
        Some(v) => (true, v.map(|m| m as f64)),
    };
    let (set_from, from) = times(c.valid_from_ms);
    let (set_until, until) = times(c.valid_until_ms);
    let lim = |o: Option<Option<u32>>| match o {
        None => (false, None),
        Some(v) => (true, v.map(|m| m as i32)),
    };
    let (set_dev, dev) = lim(c.max_devices);
    let (set_lis, lis) = lim(c.max_listens);
    let (set_peo, peo) = lim(c.max_people);
    sqlx::query(
        "UPDATE listener_key SET
           label = COALESCE($3, label),
           note = COALESCE($4, note),
           email_hash = CASE WHEN $5 THEN $6 ELSE email_hash END,
           email_nonce = CASE WHEN $5 THEN $7 ELSE email_nonce END,
           email_sealed = CASE WHEN $5 THEN $8 ELSE email_sealed END,
           media = COALESCE($9, media),
           all_shows = COALESCE($10, all_shows),
           valid_from = CASE WHEN $11 THEN (CASE WHEN $12::float8 IS NULL THEN NULL ELSE to_timestamp($12::float8 / 1000) END) ELSE valid_from END,
           valid_until = CASE WHEN $13 THEN (CASE WHEN $14::float8 IS NULL THEN NULL ELSE to_timestamp($14::float8 / 1000) END) ELSE valid_until END,
           max_devices = CASE WHEN $15 THEN $16 ELSE max_devices END,
           max_listens = CASE WHEN $17 THEN $18 ELSE max_listens END,
           max_people = CASE WHEN $19 THEN $20 ELSE max_people END,
           ended_notified_at = CASE WHEN $13 THEN NULL ELSE ended_notified_at END,
           updated_at = now()
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(c.label)
    .bind(c.note)
    .bind(set_email)
    .bind(e_hash)
    .bind(e_nonce)
    .bind(e_sealed)
    .bind(c.media.map(Media::as_str))
    .bind(c.all_shows)
    .bind(set_from)
    .bind(from)
    .bind(set_until)
    .bind(until)
    .bind(set_dev)
    .bind(dev)
    .bind(set_lis)
    .bind(lis)
    .bind(set_peo)
    .bind(peo)
    .execute(&mut *tx)
    .await?;
    if let Some(shows) = c.shows {
        set_key_shows(tx, station, id, shows).await?;
    }
    log(tx, station, id, by, action, detail).await?;
    get_key_conn(tx, station, id).await
}

/// Which way a state changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Pause,
    Resume,
    Revoke,
}

/// The result of a state change: the key, and whether this call ended it (so that the control
/// plane sends `key.ended` exactly once).
pub struct Moved {
    pub key: Key,
    pub ended_now: bool,
    /// The key was already in the state asked for.
    pub unchanged: bool,
}

/// Pause, resume or revoke. Revoked is final: a revoked key cannot be paused or resumed
/// (`Err(Refusal)` is not used here; the caller reads `key.state`).
pub async fn move_key(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
    mv: Move,
    reason: &str,
    by: &str,
) -> Result<Option<Moved>, sqlx::Error> {
    let cur: Option<(String, bool)> = sqlx::query_as(
        "SELECT state, ended_notified_at IS NOT NULL FROM listener_key
         WHERE station_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(station.as_uuid())
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((state, notified)) = cur else {
        return Ok(None);
    };
    let (to, action) = match mv {
        Move::Pause => (State::Paused, "paused"),
        Move::Resume => (State::Active, "resumed"),
        Move::Revoke => (State::Revoked, "revoked"),
    };
    let from = State::parse(&state).unwrap_or(State::Active);
    // Revoked is final; a key already in the state asked for is left as it is.
    if from == State::Revoked || from == to {
        let key = get_key_conn(tx, station, id).await?.expect("the key");
        return Ok(Some(Moved {
            key,
            ended_now: false,
            unchanged: true,
        }));
    }
    // Resuming a key clears the sent `key.ended` too: it may end again later.
    let ended_now = to == State::Revoked && !notified;
    sqlx::query(
        "UPDATE listener_key SET state = $3, state_reason = $4, state_at = now(), updated_at = now(),
           ended_notified_at = CASE WHEN $3 = 'revoked' THEN COALESCE(ended_notified_at, now())
                                    WHEN $3 = 'active' THEN NULL ELSE ended_notified_at END
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(to.as_str())
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    log(
        tx,
        station,
        id,
        by,
        action,
        serde_json::json!({ "reason": reason }),
    )
    .await?;
    let key = get_key_conn(tx, station, id).await?.expect("the key");
    Ok(Some(Moved {
        key,
        ended_now,
        unchanged: false,
    }))
}

/// Link a replaced key to the key that replaced it.
pub async fn link_replacement(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    old: Uuid,
    new: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE listener_key SET replaced_by_id = $3, updated_at = now()
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(old)
    .bind(new)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Carry the listens already used onto a replacement, so replacing a key does not give its
/// holder a fresh allowance.
pub async fn carry_listens(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    from: Uuid,
    to: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE listener_key n SET listens_used = o.listens_used
         FROM listener_key o
         WHERE n.station_id = $1 AND n.id = $3 AND o.station_id = $1 AND o.id = $2",
    )
    .bind(station.as_uuid())
    .bind(from)
    .bind(to)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Keys with this email (by its keyed hash) that are worth sending: not revoked, not ended.
/// Their ids; the control plane opens each key's sealed text.
pub async fn live_keys_for_email(
    pool: &sqlx::PgPool,
    station: StationId,
    email_hash: &[u8; 32],
    now_ms: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM listener_key
         WHERE station_id = $1 AND email_hash = $2 AND state <> 'revoked'
           AND (valid_until IS NULL OR valid_until > to_timestamp($3::float8 / 1000))
         ORDER BY created_at, id LIMIT 20",
    )
    .bind(station.as_uuid())
    .bind(email_hash.to_vec())
    .bind(now_ms as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ---- the change log ----

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyLogEntry)]
pub struct LogEntry {
    pub at_ms: i64,
    pub actor: String,
    /// `created`, `changed`, `extended`, `paused`, `resumed`, `revoked`, `replaced`,
    /// `text_shown`, `lost_key_asked`, `device_ended`.
    pub action: String,
    #[schema(value_type = Object)]
    pub detail: serde_json::Value,
}

pub async fn log(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    key: Uuid,
    actor: &str,
    action: &str,
    detail: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO listener_key_log (station_id, key_id, actor, action, detail)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(station.as_uuid())
    .bind(key)
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn read_log(
    pool: &sqlx::PgPool,
    station: StationId,
    key: Uuid,
    limit: i64,
) -> Result<Vec<LogEntry>, sqlx::Error> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {}, actor, action, detail FROM listener_key_log
         WHERE station_id = $1 AND key_id = $2 ORDER BY at DESC, id DESC LIMIT $3",
        ms!("at")
    )))
    .bind(station.as_uuid())
    .bind(key)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(LogEntry {
                at_ms: r.try_get(0)?,
                actor: r.try_get(1)?,
                action: r.try_get(2)?,
                detail: r.try_get(3)?,
            })
        })
        .collect()
}

// ---- devices ----

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyDevice)]
pub struct Device {
    pub id: Uuid,
    /// The browser or app, as it named itself.
    pub label: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    pub ended_ms: Option<i64>,
    /// The show it last listened to.
    pub last_show_id: Option<Uuid>,
}

const DEVICE_SELECT: &str = concat!(
    "SELECT id, label, ",
    ms!("first_seen_at"),
    ", ",
    ms!("last_seen_at"),
    ", ",
    ms!("ended_at"),
    ", last_show_id FROM listener_key_device"
);

fn device_of(r: &sqlx::postgres::PgRow) -> Result<Device, sqlx::Error> {
    Ok(Device {
        id: r.try_get(0)?,
        label: r.try_get(1)?,
        first_seen_ms: r.try_get(2)?,
        last_seen_ms: r.try_get(3)?,
        ended_ms: r.try_get(4)?,
        last_show_id: r.try_get(5)?,
    })
}

/// A key's devices, the live ones first.
pub async fn devices(
    pool: &sqlx::PgPool,
    station: StationId,
    key: Uuid,
) -> Result<Vec<Device>, sqlx::Error> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{DEVICE_SELECT} WHERE station_id = $1 AND key_id = $2
         ORDER BY (ended_at IS NULL) DESC, last_seen_at DESC LIMIT 200"
    )))
    .bind(station.as_uuid())
    .bind(key)
    .fetch_all(pool)
    .await?;
    rows.iter().map(device_of).collect()
}

/// Sign a device out: it is refused at its next renewal. `false` when the key has no such
/// device or it was out already.
pub async fn end_device(
    pool: &sqlx::PgPool,
    station: StationId,
    key: Uuid,
    device: Uuid,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE listener_key_device SET ended_at = now()
         WHERE station_id = $1 AND key_id = $2 AND id = $3 AND ended_at IS NULL",
    )
    .bind(station.as_uuid())
    .bind(key)
    .bind(device)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

// ---- redeem and renew ----

/// How long a device counts as holding a key after its last sign of life, how long a pause
/// continues a listen rather than starting one, and how seldom a renewal writes.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub hold_ms: i64,
    pub gap_ms: i64,
    pub touch_ms: i64,
}

/// One redeem (a key's text typed or sent) or renewal (a token's holder asking again).
pub struct Listen<'a> {
    pub station: StationId,
    /// Find the key by the hash of its text (redeem) or by id (renew).
    pub by: Lookup,
    /// The device's id as the browser kept it, if it has one.
    pub device: Option<Uuid>,
    /// What the browser or app calls itself; kept short.
    pub device_label: &'a str,
    /// The show it wants to hear, if it named one.
    pub show: Option<Uuid>,
    /// The listener's address as a keyed hash.
    pub addr_hash: [u8; 8],
    pub now_ms: i64,
    pub renew: bool,
    pub timing: Timing,
}

pub enum Lookup {
    Text([u8; 32]),
    Id(Uuid),
}

/// What a listen was granted.
pub struct Granted {
    pub key: Key,
    pub device: Uuid,
    /// This redeem began a listen and used one of the key's listens.
    pub new_listen: bool,
    /// This was the key's first use anywhere (send `key.first_used`).
    pub first_use: bool,
}

pub enum Verdict {
    Granted(Box<Granted>),
    Refused {
        refusal: Refusal,
        /// The key, when it was found (never for `NotRecognised`).
        key: Option<Box<Key>>,
        /// For `TooManyDevices`: who holds it, so the listener can sign one out.
        devices: Vec<Device>,
    },
}

fn refused(refusal: Refusal, key: Option<Key>, devices: Vec<Device>) -> Verdict {
    Verdict::Refused {
        refusal,
        key: key.map(Box::new),
        devices,
    }
}

/// The device id the use records give a *refused* try: it holds no place on the key, so it is
/// not counted as a device, but its address is counted, which is how a key posted in public
/// shows up in the leak report even when its limits turn most people away.
pub const REFUSED_DEVICE: Uuid = Uuid::nil();

/// Begin or continue a listen under the key's row lock (a renewal takes no lock: it changes no
/// count). See the module's words; the rules are [`rules::judge`] and the limits below. A redeem
/// the key refuses is recorded (its hashed address, against [`REFUSED_DEVICE`]) after the row's
/// lock is let go, so that the leak report sees every address that tried the key.
pub async fn begin_listen(pool: &sqlx::PgPool, l: &Listen<'_>) -> Result<Verdict, sqlx::Error> {
    let verdict = begin_listen_locked(pool, l).await?;
    if !l.renew
        && let Verdict::Refused { key: Some(key), .. } = &verdict
    {
        record_use(pool, l, key.id, REFUSED_DEVICE).await?;
    }
    Ok(verdict)
}

async fn record_use(
    pool: &sqlx::PgPool,
    l: &Listen<'_>,
    key: Uuid,
    device: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO listener_key_use (station_id, key_id, device_id, addr_hash, hour, first_at, last_at)
         VALUES ($1, $2, $3, $4,
                 date_trunc('hour', to_timestamp($5::float8 / 1000)),
                 to_timestamp($5::float8 / 1000), to_timestamp($5::float8 / 1000))
         ON CONFLICT (key_id, device_id, addr_hash, hour)
         DO UPDATE SET last_at = EXCLUDED.last_at, uses = listener_key_use.uses + 1",
    )
    .bind(l.station.as_uuid())
    .bind(key)
    .bind(device)
    .bind(l.addr_hash.to_vec())
    .bind(l.now_ms as f64)
    .execute(pool)
    .await?;
    Ok(())
}

async fn begin_listen_locked(pool: &sqlx::PgPool, l: &Listen<'_>) -> Result<Verdict, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let lock = if l.renew { "" } else { " FOR UPDATE OF k" };
    let q = match &l.by {
        Lookup::Text(_) => {
            format!("{KEY_SELECT} WHERE k.station_id = $1 AND k.text_hash = $2{lock}")
        }
        Lookup::Id(_) => format!("{KEY_SELECT} WHERE k.station_id = $1 AND k.id = $2{lock}"),
    };
    let query = sqlx::query(sqlx::AssertSqlSafe(q)).bind(l.station.as_uuid());
    let query = match &l.by {
        Lookup::Text(h) => query.bind(h.to_vec()),
        Lookup::Id(id) => query.bind(*id),
    };
    let Some(row) = query.fetch_optional(&mut *tx).await? else {
        return Ok(refused(Refusal::NotRecognised, None, vec![]));
    };
    let key = key_of(&row)?;
    let view = rules::View {
        state: key.state,
        valid_from_ms: key.valid_from_ms,
        valid_until_ms: key.valid_until_ms,
        all_shows: key.all_shows,
        shows: &key.shows,
    };
    if let Err(r) = rules::judge(&view, l.now_ms, l.show) {
        return Ok(refused(r, Some(key), vec![]));
    }
    let station = l.station.as_uuid();
    let t = l.timing;
    let now = l.now_ms as f64;

    // The device this call is for.
    let existing: Option<(Uuid, i64, bool)> =
        match l.device {
            Some(d) => sqlx::query_as(
                "SELECT id, (extract(epoch FROM last_seen_at) * 1000)::bigint, ended_at IS NOT NULL
                 FROM listener_key_device WHERE station_id = $1 AND key_id = $2 AND id = $3",
            )
            .bind(station)
            .bind(key.id)
            .bind(d)
            .fetch_optional(&mut *tx)
            .await?,
            None => None,
        };
    let machine = key.kind == Kind::Machine;
    let (device, last_seen_ms, is_new_device) = match existing {
        Some((_, _, true)) if l.renew => {
            return Ok(refused(Refusal::DeviceSignedOut, Some(key), vec![]));
        }
        Some((id, seen, false)) => (id, seen, false),
        // A renewal never makes a device: no device, or a signed-out one, is refused.
        _ if l.renew => return Ok(refused(Refusal::DeviceSignedOut, Some(key), vec![])),
        // A redeem from a device that was signed out, or has none, is a new device.
        _ => {
            if !machine {
                if let Some(max) = key.max_devices {
                    let held: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM listener_key_device
                         WHERE station_id = $1 AND key_id = $2 AND ended_at IS NULL
                           AND last_seen_at > to_timestamp(($3::float8 - $4::float8) / 1000)",
                    )
                    .bind(station)
                    .bind(key.id)
                    .bind(now)
                    .bind(t.hold_ms as f64)
                    .fetch_one(&mut *tx)
                    .await?;
                    if held >= i64::from(max) {
                        let mut held_devices =
                            live_devices(&mut tx, l.station, key.id, l.now_ms, t.hold_ms).await?;
                        held_devices.truncate(50);
                        return Ok(refused(Refusal::TooManyDevices, Some(key), held_devices));
                    }
                }
                if let Some(max) = key.max_people {
                    let ever: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM listener_key_device WHERE station_id = $1 AND key_id = $2",
                    )
                    .bind(station)
                    .bind(key.id)
                    .fetch_one(&mut *tx)
                    .await?;
                    if ever >= i64::from(max) {
                        return Ok(refused(Refusal::PeopleLimit, Some(key), vec![]));
                    }
                }
            }
            (Uuid::new_v4(), 0, true)
        }
    };

    // Whether this begins a listen: a new device, or one that has been away longer than the gap.
    let new_listen = !l.renew && (is_new_device || l.now_ms - last_seen_ms >= t.gap_ms);
    if new_listen
        && let Some(max) = key.max_listens
        && key.listens_used >= max
    {
        return Ok(refused(Refusal::ListensUsedUp, Some(key), vec![]));
    }

    // Writes. A renewal writes only when the device has not been touched lately.
    let stale = is_new_device || l.now_ms - last_seen_ms >= t.touch_ms;
    let mut first_use = false;
    if is_new_device {
        sqlx::query(
            "INSERT INTO listener_key_device (id, station_id, key_id, label, first_seen_at, last_seen_at, last_show_id)
             VALUES ($1, $2, $3, $4, to_timestamp($5::float8 / 1000), to_timestamp($5::float8 / 1000), $6)",
        )
        .bind(device)
        .bind(station)
        .bind(key.id)
        .bind(l.device_label)
        .bind(now)
        .bind(l.show)
        .execute(&mut *tx)
        .await?;
    } else if stale {
        sqlx::query(
            "UPDATE listener_key_device SET last_seen_at = to_timestamp($3::float8 / 1000),
               label = CASE WHEN $4 = '' THEN label ELSE $4 END,
               last_show_id = COALESCE($5, last_show_id)
             WHERE id = $1 AND key_id = $2",
        )
        .bind(device)
        .bind(key.id)
        .bind(now)
        .bind(l.device_label)
        .bind(l.show)
        .execute(&mut *tx)
        .await?;
    }
    if stale || new_listen {
        let n = sqlx::query(
            "UPDATE listener_key SET last_used_at = to_timestamp($3::float8 / 1000),
               listens_used = listens_used + $4,
               first_used_at = COALESCE(first_used_at, to_timestamp($3::float8 / 1000))
             WHERE station_id = $1 AND id = $2 AND first_used_at IS NULL",
        )
        .bind(station)
        .bind(key.id)
        .bind(now)
        .bind(i32::from(new_listen))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        first_use = n == 1;
        if !first_use {
            sqlx::query(
                "UPDATE listener_key SET last_used_at = to_timestamp($3::float8 / 1000),
                   listens_used = listens_used + $4
                 WHERE station_id = $1 AND id = $2",
            )
            .bind(station)
            .bind(key.id)
            .bind(now)
            .bind(i32::from(new_listen))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "INSERT INTO listener_key_use (station_id, key_id, device_id, addr_hash, hour, first_at, last_at)
             VALUES ($1, $2, $3, $4,
                     date_trunc('hour', to_timestamp($5::float8 / 1000)),
                     to_timestamp($5::float8 / 1000), to_timestamp($5::float8 / 1000))
             ON CONFLICT (key_id, device_id, addr_hash, hour)
             DO UPDATE SET last_at = EXCLUDED.last_at, uses = listener_key_use.uses + 1",
        )
        .bind(station)
        .bind(key.id)
        .bind(device)
        .bind(l.addr_hash.to_vec())
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    let key = get_key_conn(&mut tx, l.station, key.id)
        .await?
        .expect("the key");
    tx.commit().await?;
    Ok(Verdict::Granted(Box::new(Granted {
        key,
        device,
        new_listen,
        first_use,
    })))
}

async fn live_devices(
    conn: &mut sqlx::PgConnection,
    station: StationId,
    key: Uuid,
    now_ms: i64,
    hold_ms: i64,
) -> Result<Vec<Device>, sqlx::Error> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{DEVICE_SELECT} WHERE station_id = $1 AND key_id = $2 AND ended_at IS NULL
           AND last_seen_at > to_timestamp(($3::float8 - $4::float8) / 1000)
         ORDER BY last_seen_at DESC"
    )))
    .bind(station.as_uuid())
    .bind(key)
    .bind(now_ms as f64)
    .bind(hold_ms as f64)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(device_of).collect()
}

// ---- the leak report ----

/// One key's row in the leak report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKeyLeak)]
pub struct Leak {
    pub key: Key,
    /// The family's label, when it has one.
    pub family: Option<String>,
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
    pub listens: u32,
    pub devices: u32,
    pub addresses: u32,
    pub addresses_24h: u32,
    pub addresses_7d: u32,
    pub peak_new_addresses_in_an_hour: u32,
    /// The "most likely passed on" score (see `listener_keys::leak_score`) and its reasons.
    pub score: u32,
    pub reasons: Vec<String>,
    /// A burst of new addresses: what a key posted in public looks like.
    pub spike: bool,
}

/// The leak report: every key of a family (or of the station), each with its use figures and
/// score, the most likely passed on first. Keys with no use are listed last.
pub async fn leak_report(
    pool: &sqlx::PgPool,
    station: StationId,
    family: Option<Uuid>,
    now_ms: i64,
) -> Result<Vec<Leak>, sqlx::Error> {
    let (keys, _) = list_keys(
        pool,
        station,
        &Filter {
            family,
            limit: 5000,
            ..Default::default()
        },
    )
    .await?;
    if keys.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = keys.iter().map(|k| k.id).collect();
    let now = now_ms as f64;
    // Per key: when seen, how many devices and addresses (ever, 24 h, 7 d).
    let rows = sqlx::query(
        "SELECT key_id,
                (extract(epoch FROM min(first_at)) * 1000)::bigint,
                (extract(epoch FROM max(last_at)) * 1000)::bigint,
                count(DISTINCT device_id) FILTER (WHERE device_id <> '00000000-0000-0000-0000-000000000000'),
                count(DISTINCT addr_hash),
                count(DISTINCT addr_hash) FILTER (WHERE last_at > to_timestamp(($3::float8 - 86400000) / 1000)),
                count(DISTINCT addr_hash) FILTER (WHERE last_at > to_timestamp(($3::float8 - 604800000) / 1000))
         FROM listener_key_use WHERE station_id = $1 AND key_id = ANY($2) GROUP BY key_id",
    )
    .bind(station.as_uuid())
    .bind(&ids)
    .bind(now)
    .fetch_all(pool)
    .await?;
    let mut stats: std::collections::HashMap<Uuid, LeakStats> = std::collections::HashMap::new();
    for r in &rows {
        let id: Uuid = r.try_get(0)?;
        stats.insert(
            id,
            LeakStats {
                first_seen_ms: r.try_get(1)?,
                last_seen_ms: r.try_get(2)?,
                devices: r.try_get::<i64, _>(3)? as u32,
                addresses: r.try_get::<i64, _>(4)? as u32,
                addresses_24h: r.try_get::<i64, _>(5)? as u32,
                addresses_7d: r.try_get::<i64, _>(6)? as u32,
                ..Default::default()
            },
        );
    }
    // The most addresses first seen by a key within one hour: an address's first hour for the
    // key, counted per hour.
    let bursts: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT key_id, max(n) FROM (
            SELECT key_id, date_trunc('hour', first_seen) AS h, count(*) AS n FROM (
              SELECT key_id, addr_hash, min(first_at) AS first_seen FROM listener_key_use
              WHERE station_id = $1 AND key_id = ANY($2) GROUP BY key_id, addr_hash
            ) a GROUP BY key_id, date_trunc('hour', first_seen)
         ) b GROUP BY key_id",
    )
    .bind(station.as_uuid())
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    for (id, n) in bursts {
        if let Some(s) = stats.get_mut(&id) {
            s.peak_new_addresses_in_an_hour = n as u32;
        }
    }
    let families: std::collections::HashMap<Uuid, String> = list_families(pool, station)
        .await?
        .into_iter()
        .map(|f| (f.id, f.label))
        .collect();
    let mut out: Vec<Leak> = keys
        .into_iter()
        .map(|key| {
            let mut s = stats.remove(&key.id).unwrap_or_default();
            s.kind = Some(key.kind);
            s.max_devices = key.max_devices;
            s.max_people = key.max_people;
            s.listens = key.listens_used;
            let (score, reasons) = rules::leak_score(&s);
            Leak {
                family: key.family_id.and_then(|f| families.get(&f).cloned()),
                first_seen_ms: s.first_seen_ms,
                last_seen_ms: s.last_seen_ms,
                listens: s.listens,
                devices: s.devices,
                addresses: s.addresses,
                addresses_24h: s.addresses_24h,
                addresses_7d: s.addresses_7d,
                peak_new_addresses_in_an_hour: s.peak_new_addresses_in_an_hour,
                spike: s.peak_new_addresses_in_an_hour >= 3,
                score,
                reasons,
                key,
            }
        })
        .collect();
    // Most likely passed on first; then by addresses, then the most recently seen; unused last.
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.addresses_7d.cmp(&a.addresses_7d))
            .then(b.last_seen_ms.cmp(&a.last_seen_ms))
            .then(a.key.id.cmp(&b.key.id))
    });
    Ok(out)
}

// ---- the sweep ----

/// A key that has just ended by its window: what `key.ended` carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    pub id: Uuid,
    pub label: String,
    pub family_id: Option<Uuid>,
    pub kind: Kind,
}

/// Mark the keys whose window has passed as announced and return them, once each across
/// restarts (a key extended or resumed may end again).
pub async fn take_expired(
    pool: &sqlx::PgPool,
    station: StationId,
    now_ms: i64,
) -> Result<Vec<Ended>, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE listener_key SET ended_notified_at = now()
         WHERE station_id = $1 AND ended_notified_at IS NULL AND state <> 'revoked'
           AND valid_until IS NOT NULL AND valid_until <= to_timestamp($2::float8 / 1000)
         RETURNING id, label, family_id, kind",
    )
    .bind(station.as_uuid())
    .bind(now_ms as f64)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(Ended {
                id: r.try_get(0)?,
                label: r.try_get(1)?,
                family_id: r.try_get(2)?,
                kind: Kind::parse(&r.try_get::<String, _>(3)?)
                    .ok_or_else(|| sqlx::Error::Decode("an unknown key kind".into()))?,
            })
        })
        .collect()
}

/// Delete use records older than `days`, and devices that have been out of sight that long.
/// A device a key's people limit counts is kept for as long as its key exists unless it was
/// signed out; the limit is "devices ever", so only signed-out ones go.
pub async fn prune(pool: &sqlx::PgPool, station: StationId, days: u32) -> Result<u64, sqlx::Error> {
    let a = sqlx::query(
        "DELETE FROM listener_key_use WHERE station_id = $1 AND hour < now() - make_interval(days => $2)",
    )
    .bind(station.as_uuid())
    .bind(days as i32)
    .execute(pool)
    .await?
    .rows_affected();
    let b = sqlx::query(
        "DELETE FROM listener_key_device WHERE station_id = $1 AND ended_at IS NOT NULL
           AND last_seen_at < now() - make_interval(days => $2)",
    )
    .bind(station.as_uuid())
    .bind(days as i32)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(a + b)
}

/// Erase a key and everything kept about its use (ADR-0122: erasure follows the person).
pub async fn erase_key(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    // Devices have no foreign key of their own to the use records; the cascade covers both.
    let n = sqlx::query("DELETE FROM listener_key WHERE station_id = $1 AND id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n == 1)
}
