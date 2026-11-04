// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat's rows (migration 0006). Times cross the boundary as epoch milliseconds: sqlx is
//! built without a date crate, so every timestamp is read as `extract(epoch ...)` and written
//! with `to_timestamp`.

use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use weavesignal_model::StationId;

use crate::config::StationSettings;
use crate::key::{EmailKey, legacy_email_hash};
use crate::rules::{Kind, MessageKind, Restriction, Standing};

pub type Db = PgPool;

/// SHA-256, for session tokens (long and random, so no key is needed). Email addresses and
/// sign-in codes go through [`EmailKey`] instead (ADR-0125).
pub fn hash(text: &str) -> Vec<u8> {
    Sha256::digest(text.as_bytes()).to_vec()
}

/// What `chat_account.email_hash_version` holds for a hash made under the station's key.
const KEYED: i16 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub kind: Kind,
    pub name: String,
    pub standing: Standing,
    pub created_ms: i64,
    pub age_declared: bool,
    pub terms_version: Option<String>,
    pub erased: bool,
}

type AccountRow = (
    Uuid,
    String,
    String,
    String,
    i64,
    bool,
    Option<String>,
    bool,
);

const ACCOUNT_COLS: &str = "id, kind, name, standing,
    (extract(epoch FROM created_at) * 1000)::bigint,
    age_declared_at IS NOT NULL, terms_version, erased_at IS NOT NULL";

fn account(r: AccountRow) -> Account {
    Account {
        id: r.0,
        kind: match r.1.as_str() {
            "guest" => Kind::Guest,
            "staff" => Kind::Staff,
            _ => Kind::Email,
        },
        name: r.2,
        standing: Standing::parse(&r.3).unwrap_or(Standing::Member),
        created_ms: r.4,
        age_declared: r.5,
        terms_version: r.6,
        erased: r.7,
    }
}

pub async fn account_by_id(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<Account>> {
    let q = format!("SELECT {ACCOUNT_COLS} FROM chat_account WHERE station_id = $1 AND id = $2");
    Ok(sqlx::query_as::<_, AccountRow>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(id)
        .fetch_optional(db)
        .await?
        .map(account))
}

/// The account of an address whose owner has just proved it, by a sign-in code sent there.
///
/// Only call this after [`check_code`] said [`CodeCheck::Good`] for the same address: an account
/// made before ADR-0125 is found by its old plain hash and re-keyed here, which is safe only
/// because the owner has just shown they hold the address (the address itself is never stored).
pub async fn account_by_verified_email(
    db: &Db,
    st: StationId,
    key: &EmailKey,
    email: &str,
) -> sqlx::Result<Option<Account>> {
    let keyed = key.email_hash(email);
    if let Some(a) = account_by_keyed_hash(db, st, &keyed).await? {
        return Ok(Some(a));
    }
    // An account from before the keyed hash: one statement finds it by the plain hash and swaps
    // in the keyed one, so two sign-ins at once cannot both do it half-way.
    let q = format!(
        "UPDATE chat_account SET email_hash = $3, email_hash_version = {KEYED}
         WHERE station_id = $1 AND email_hash = $2 AND email_hash_version = 1 AND erased_at IS NULL
         RETURNING {ACCOUNT_COLS}"
    );
    if let Some(r) = sqlx::query_as::<_, AccountRow>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(legacy_email_hash(email))
        .bind(&keyed)
        .fetch_optional(db)
        .await?
    {
        return Ok(Some(account(r)));
    }
    // A sign-in at the same moment may have re-keyed it between the two statements above: its
    // UPDATE held the row, so ours matched nothing, and it has committed by now. Look again, or
    // that person would be offered a new account their own address already holds.
    account_by_keyed_hash(db, st, &keyed).await
}

async fn account_by_keyed_hash(
    db: &Db,
    st: StationId,
    keyed: &[u8],
) -> sqlx::Result<Option<Account>> {
    let q = format!(
        "SELECT {ACCOUNT_COLS} FROM chat_account
         WHERE station_id = $1 AND email_hash = $2 AND email_hash_version = {KEYED} AND erased_at IS NULL"
    );
    Ok(sqlx::query_as::<_, AccountRow>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(keyed)
        .fetch_optional(db)
        .await?
        .map(account))
}

/// How many accounts still hold the old plain hash (they are re-keyed when their owners next
/// sign in). Logged at start.
pub async fn legacy_email_hashes(db: &Db, st: StationId) -> sqlx::Result<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM chat_account
         WHERE station_id = $1 AND email_hash IS NOT NULL AND email_hash_version = 1",
    )
    .bind(st.as_uuid())
    .fetch_one(db)
    .await?;
    Ok(n)
}

pub async fn name_taken(db: &Db, st: StationId, name: &str) -> sqlx::Result<bool> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM chat_account
         WHERE station_id = $1 AND lower(name) = lower($2) AND erased_at IS NULL AND kind <> 'staff'",
    )
    .bind(st.as_uuid())
    .bind(name)
    .fetch_one(db)
    .await?;
    Ok(n > 0)
}

pub struct NewAccount<'a> {
    pub kind: Kind,
    /// The address as [`EmailKey::email_hash`] keeps it; the address itself is never passed.
    pub email_hash: Option<Vec<u8>>,
    pub name: &'a str,
    pub standing: Standing,
    pub terms_version: Option<&'a str>,
    pub age_declared: bool,
}

pub async fn create_account(db: &Db, st: StationId, a: &NewAccount<'_>) -> sqlx::Result<Account> {
    let id = Uuid::new_v4();
    let kind = match a.kind {
        Kind::Email => "email",
        Kind::Guest => "guest",
        Kind::Staff => "staff",
    };
    let q = format!(
        "INSERT INTO chat_account (id, station_id, kind, email_hash, email_hash_version, name, standing, terms_version, age_declared_at)
         VALUES ($1, $2, $3, $4, {KEYED}, $5, $6, $7, CASE WHEN $8 THEN now() END)
         RETURNING {ACCOUNT_COLS}"
    );
    let row: AccountRow = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(id)
        .bind(st.as_uuid())
        .bind(kind)
        .bind(a.email_hash.as_deref())
        .bind(a.name)
        .bind(a.standing.as_str())
        .bind(a.terms_version)
        .bind(a.age_declared)
        .fetch_one(db)
        .await?;
    Ok(account(row))
}

/// A member of staff's chat account, found by their staff account (ADR-0127,
/// `chat-staff-identity` b), made on first use and renamed when their name changes. Chat names
/// are at most 40 characters, so a longer staff name is cut there.
pub async fn staff_account_for(
    db: &Db,
    st: StationId,
    staff_id: Uuid,
    name: &str,
) -> sqlx::Result<Account> {
    let name: String = name.chars().take(40).collect::<String>().trim().to_owned();
    let q = format!(
        "INSERT INTO chat_account (id, station_id, kind, email_hash_version, name, standing, age_declared_at, staff_account_id)
         VALUES ($1, $2, 'staff', {KEYED}, $3, 'staff', now(), $4)
         ON CONFLICT (station_id, staff_account_id) WHERE staff_account_id IS NOT NULL
         DO UPDATE SET name = excluded.name
         RETURNING {ACCOUNT_COLS}"
    );
    let row: AccountRow = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(Uuid::new_v4())
        .bind(st.as_uuid())
        .bind(&name)
        .bind(staff_id)
        .fetch_one(db)
        .await?;
    Ok(account(row))
}

/// The studio's own account for a name ("Elspeth Gordon"), made on first use.
pub async fn staff_account(db: &Db, st: StationId, name: &str) -> sqlx::Result<Account> {
    let q = format!(
        "SELECT {ACCOUNT_COLS} FROM chat_account
         WHERE station_id = $1 AND kind = 'staff' AND name = $2 AND staff_account_id IS NULL
           AND erased_at IS NULL LIMIT 1"
    );
    if let Some(r) = sqlx::query_as::<_, AccountRow>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(name)
        .fetch_optional(db)
        .await?
    {
        return Ok(account(r));
    }
    create_account(
        db,
        st,
        &NewAccount {
            kind: Kind::Staff,
            email_hash: None,
            name,
            standing: Standing::Staff,
            terms_version: None,
            age_declared: true,
        },
    )
    .await
}

pub async fn accept_terms(db: &Db, id: Uuid, version: &str, age: bool) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE chat_account SET terms_version = $2,
           age_declared_at = CASE WHEN $3 THEN coalesce(age_declared_at, now()) ELSE age_declared_at END
         WHERE id = $1",
    )
    .bind(id)
    .bind(version)
    .bind(age)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_standing(db: &Db, st: StationId, id: Uuid, s: Standing) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE chat_account SET standing = $3 WHERE station_id = $1 AND id = $2 AND kind <> 'staff'",
    )
    .bind(st.as_uuid())
    .bind(id)
    .bind(s.as_str())
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

// ---- sessions and sign-in codes ----

pub async fn create_session(
    db: &Db,
    st: StationId,
    account: Uuid,
    token: &str,
    days: u32,
    address: Option<std::net::IpAddr>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO chat_session (token_hash, station_id, account_id, expires_at, address)
         VALUES ($1, $2, $3, now() + make_interval(days => $4), $5)",
    )
    .bind(hash(token))
    .bind(st.as_uuid())
    .bind(account)
    .bind(days as i32)
    .bind(address.map(sqlx::types::ipnetwork::IpNetwork::from))
    .execute(db)
    .await?;
    Ok(())
}

pub async fn session_account(db: &Db, st: StationId, token: &str) -> sqlx::Result<Option<Account>> {
    let q = format!(
        "SELECT {} FROM chat_session s JOIN chat_account a ON a.id = s.account_id
         WHERE s.token_hash = $1 AND s.station_id = $2 AND s.expires_at > now() AND a.erased_at IS NULL",
        ACCOUNT_COLS
            .replace("id, kind", "a.id, a.kind")
            .replace("created_at", "a.created_at")
    );
    Ok(sqlx::query_as::<_, AccountRow>(sqlx::AssertSqlSafe(q))
        .bind(hash(token))
        .bind(st.as_uuid())
        .fetch_optional(db)
        .await?
        .map(account))
}

pub async fn end_session(db: &Db, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM chat_session WHERE token_hash = $1")
        .bind(hash(token))
        .execute(db)
        .await?;
    Ok(())
}

/// Codes asked for in the last ten minutes for an email address.
pub async fn recent_codes(
    db: &Db,
    st: StationId,
    key: &EmailKey,
    email: &str,
) -> sqlx::Result<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM chat_signin
         WHERE station_id = $1 AND email_hash = $2 AND created_at > now() - interval '10 minutes'",
    )
    .bind(st.as_uuid())
    .bind(key.email_hash(email))
    .fetch_one(db)
    .await?;
    Ok(n)
}

pub async fn create_code(
    db: &Db,
    st: StationId,
    key: &EmailKey,
    email: &str,
    code: &str,
    minutes: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO chat_signin (id, station_id, email_hash, code_hash, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(mins => $5))",
    )
    .bind(Uuid::new_v4())
    .bind(st.as_uuid())
    .bind(key.email_hash(email))
    .bind(key.code_hash(email, code))
    .bind(minutes)
    .execute(db)
    .await?;
    Ok(())
}

pub enum CodeCheck {
    Good,
    Wrong,
    /// Expired, used, or too many tries: ask for a new one.
    Spent,
}

/// Check a code against the newest live one for the address. A good code stays usable until
/// the account is made (a new chatter may need a second request to add a name), then is spent
/// by [`spend_codes`].
pub async fn check_code(
    db: &Db,
    st: StationId,
    key: &EmailKey,
    email: &str,
    code: &str,
) -> sqlx::Result<CodeCheck> {
    let row: Option<(Uuid, Vec<u8>, i16)> = sqlx::query_as(
        "SELECT id, code_hash, attempts FROM chat_signin
         WHERE station_id = $1 AND email_hash = $2 AND NOT used AND expires_at > now()
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(st.as_uuid())
    .bind(key.email_hash(email))
    .fetch_optional(db)
    .await?;
    let Some((id, want, attempts)) = row else {
        return Ok(CodeCheck::Spent);
    };
    if attempts >= 5 {
        return Ok(CodeCheck::Spent);
    }
    // Constant time (`Mac::verify_slice`): the comparison does not tell how much matched.
    if key.code_matches(email, code, &want) {
        Ok(CodeCheck::Good)
    } else {
        sqlx::query("UPDATE chat_signin SET attempts = attempts + 1 WHERE id = $1")
            .bind(id)
            .execute(db)
            .await?;
        Ok(CodeCheck::Wrong)
    }
}

pub async fn spend_codes(db: &Db, st: StationId, key: &EmailKey, email: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE chat_signin SET used = true WHERE station_id = $1 AND email_hash = $2")
        .bind(st.as_uuid())
        .bind(key.email_hash(email))
        .execute(db)
        .await?;
    Ok(())
}

// ---- restrictions ----

pub async fn restriction(
    db: &Db,
    st: StationId,
    account: Uuid,
) -> sqlx::Result<Option<Restriction>> {
    let row: Option<(String, Option<i64>)> = sqlx::query_as(
        "SELECT kind, (extract(epoch FROM until) * 1000)::bigint FROM chat_restriction
         WHERE station_id = $1 AND account_id = $2 AND (until IS NULL OR until > now())",
    )
    .bind(st.as_uuid())
    .bind(account)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(k, until)| match (k.as_str(), until) {
        ("timeout", Some(u)) => Restriction::TimedOut { until_ms: u as u64 },
        _ => Restriction::Banned,
    }))
}

pub async fn restrict(
    db: &Db,
    st: StationId,
    account: Uuid,
    r: Restriction,
    reason: &str,
) -> sqlx::Result<()> {
    let (kind, until) = match r {
        Restriction::Banned => ("ban", None),
        Restriction::TimedOut { until_ms } => ("timeout", Some(until_ms as f64)),
    };
    sqlx::query(
        "INSERT INTO chat_restriction (station_id, account_id, kind, until, reason)
         VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000.0), $5)
         ON CONFLICT (station_id, account_id) DO UPDATE
           SET kind = EXCLUDED.kind, until = EXCLUDED.until, reason = EXCLUDED.reason, at = now()",
    )
    .bind(st.as_uuid())
    .bind(account)
    .bind(kind)
    .bind(until)
    .bind(reason)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn lift(db: &Db, st: StationId, account: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM chat_restriction WHERE station_id = $1 AND account_id = $2")
        .bind(st.as_uuid())
        .bind(account)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ---- messages ----

/// The next message id: one more than any the station has used, counting messages already
/// swept or erased (`chat_message_floor`), so an id is never handed out twice.
pub async fn next_message_id(db: &Db, st: StationId) -> sqlx::Result<i64> {
    let (n,): (Option<i64>,) = sqlx::query_as(
        "SELECT GREATEST((SELECT max(id) FROM chat_message WHERE station_id = $1),
                         (SELECT floor FROM chat_message_floor WHERE station_id = $1))",
    )
    .bind(st.as_uuid())
    .fetch_one(db)
    .await?;
    Ok(n.unwrap_or(0) + 1)
}

/// Remember the highest message id the station has stored, before any of its messages are
/// deleted: the floor `next_message_id` starts above after a restart.
async fn raise_message_floor<'e, E>(db: E, st: StationId) -> sqlx::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query(
        "INSERT INTO chat_message_floor (station_id, floor)
           SELECT $1, max(id) FROM chat_message WHERE station_id = $1 HAVING max(id) IS NOT NULL
         ON CONFLICT (station_id)
           DO UPDATE SET floor = GREATEST(chat_message_floor.floor, EXCLUDED.floor)",
    )
    .bind(st.as_uuid())
    .execute(db)
    .await?;
    Ok(())
}

/// A stored message, with what the room needs to show it.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: i64,
    pub account: Option<Account>,
    pub kind: MessageKind,
    pub body: String,
    pub reply_to: Option<i64>,
    pub server_ms: i64,
    pub playhead_ms: Option<i64>,
    pub state: String,
    pub flagged: Option<String>,
    pub pinned: bool,
    pub featured: bool,
}

pub struct NewMessage<'a> {
    pub id: i64,
    pub account: Uuid,
    pub kind: MessageKind,
    pub body: &'a str,
    pub reply_to: Option<i64>,
    pub server_ms: i64,
    pub playhead_ms: Option<i64>,
    pub held: bool,
    pub flagged: Option<&'a str>,
}

pub async fn insert_message(db: &Db, st: StationId, m: &NewMessage<'_>) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO chat_message
           (station_id, id, account_id, kind, body, reply_to, server_at, playhead_at, state, flagged)
         VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision / 1000.0),
                 to_timestamp($8::double precision / 1000.0), $9, $10)",
    )
    .bind(st.as_uuid())
    .bind(m.id)
    .bind(m.account)
    .bind(m.kind.as_str())
    .bind(m.body)
    .bind(m.reply_to)
    .bind(m.server_ms as f64)
    .bind(m.playhead_ms.map(|p| p as f64))
    .bind(if m.held { "held" } else { "visible" })
    .bind(m.flagged)
    .execute(db)
    .await?;
    Ok(())
}

type MsgRow = (
    i64,
    Option<Uuid>,
    String,
    String,
    Option<i64>,
    i64,
    Option<i64>,
    String,
    Option<String>,
    bool,
    bool,
);

const MSG_COLS: &str = "id, account_id, kind, body, reply_to,
    (extract(epoch FROM server_at) * 1000)::bigint,
    (extract(epoch FROM playhead_at) * 1000)::bigint,
    state, flagged, pinned, featured_at IS NOT NULL";

async fn rows(db: &Db, st: StationId, list: Vec<MsgRow>) -> sqlx::Result<Vec<Row>> {
    let mut out = Vec::with_capacity(list.len());
    let mut cache: std::collections::HashMap<Uuid, Option<Account>> = Default::default();
    for r in list {
        let account = match r.1 {
            None => None,
            Some(id) => {
                if let Some(a) = cache.get(&id) {
                    a.clone()
                } else {
                    let a = account_by_id(db, st, id).await?;
                    cache.insert(id, a.clone());
                    a
                }
            }
        };
        out.push(Row {
            id: r.0,
            account,
            kind: MessageKind::parse(&r.2).unwrap_or(MessageKind::Chat),
            body: r.3,
            reply_to: r.4,
            server_ms: r.5,
            playhead_ms: r.6,
            state: r.7,
            flagged: r.8,
            pinned: r.9,
            featured: r.10,
        });
    }
    Ok(out)
}

pub async fn message(db: &Db, st: StationId, id: i64) -> sqlx::Result<Option<Row>> {
    let q = format!("SELECT {MSG_COLS} FROM chat_message WHERE station_id = $1 AND id = $2");
    let list: Vec<MsgRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(id)
        .fetch_all(db)
        .await?;
    Ok(rows(db, st, list).await?.pop())
}

/// The latest visible room messages, oldest first.
pub async fn history(db: &Db, st: StationId, n: i64) -> sqlx::Result<Vec<Row>> {
    let q = format!(
        "SELECT * FROM (SELECT {MSG_COLS} FROM chat_message
           WHERE station_id = $1 AND state = 'visible' AND kind <> 'studio'
           ORDER BY id DESC LIMIT $2) t ORDER BY id"
    );
    let list: Vec<MsgRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(n)
        .fetch_all(db)
        .await?;
    rows(db, st, list).await
}

pub async fn pinned(db: &Db, st: StationId) -> sqlx::Result<Option<Row>> {
    let q = format!(
        "SELECT {MSG_COLS} FROM chat_message WHERE station_id = $1 AND pinned AND state = 'visible'
         ORDER BY id DESC LIMIT 1"
    );
    let list: Vec<MsgRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .fetch_all(db)
        .await?;
    Ok(rows(db, st, list).await?.pop())
}

/// The messages to the studio in the last six hours, oldest first.
pub async fn studio_lane(db: &Db, st: StationId) -> sqlx::Result<Vec<Row>> {
    let q = format!(
        "SELECT {MSG_COLS} FROM chat_message
         WHERE station_id = $1 AND kind = 'studio' AND state <> 'hidden'
           AND server_at > now() - interval '6 hours'
         ORDER BY id LIMIT 200"
    );
    let list: Vec<MsgRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .fetch_all(db)
        .await?;
    rows(db, st, list).await
}

pub async fn held_messages(db: &Db, st: StationId) -> sqlx::Result<Vec<Row>> {
    let q = format!(
        "SELECT {MSG_COLS} FROM chat_message WHERE station_id = $1 AND state = 'held' ORDER BY id"
    );
    let list: Vec<MsgRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .fetch_all(db)
        .await?;
    rows(db, st, list).await
}

pub async fn set_state(db: &Db, st: StationId, ids: &[i64], state: &str) -> sqlx::Result<Vec<i64>> {
    let changed: Vec<(i64,)> = sqlx::query_as(
        "UPDATE chat_message SET state = $3, pinned = CASE WHEN $3 = 'hidden' THEN false ELSE pinned END
         WHERE station_id = $1 AND id = ANY($2) AND state <> $3 RETURNING id",
    )
    .bind(st.as_uuid())
    .bind(ids)
    .bind(state)
    .fetch_all(db)
    .await?;
    Ok(changed.into_iter().map(|r| r.0).collect())
}

/// A chatter's visible room messages (a ban takes them out of the room).
pub async fn visible_ids_of(db: &Db, st: StationId, account: Uuid) -> sqlx::Result<Vec<i64>> {
    let r: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM chat_message WHERE station_id = $1 AND account_id = $2 AND state <> 'hidden'",
    )
    .bind(st.as_uuid())
    .bind(account)
    .fetch_all(db)
    .await?;
    Ok(r.into_iter().map(|r| r.0).collect())
}

pub async fn set_pinned(db: &Db, st: StationId, id: Option<i64>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE chat_message SET pinned = (id = $2) WHERE station_id = $1 AND (pinned OR id = $2)",
    )
    .bind(st.as_uuid())
    .bind(id.unwrap_or(-1))
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_featured(db: &Db, st: StationId, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE chat_message SET featured_at = now() WHERE station_id = $1 AND id = $2")
        .bind(st.as_uuid())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---- reports, notices, reviews ----

pub struct NewReport<'a> {
    pub kind: &'a str,
    pub message_id: Option<i64>,
    pub account_id: Option<Uuid>,
    pub reporter_id: Option<Uuid>,
    pub reason: &'a str,
    pub detail: &'a str,
    pub location: &'a str,
    pub contact_name: &'a str,
    pub contact_email: &'a str,
    pub evidence: Value,
    pub statement_id: Option<Uuid>,
    pub due_hours: Option<i32>,
}

pub async fn insert_report(db: &Db, st: StationId, r: &NewReport<'_>) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chat_report (id, station_id, kind, message_id, account_id, reporter_id, reason,
           detail, location, contact_name, contact_email, evidence, statement_id, due_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
           CASE WHEN $14::int IS NULL THEN NULL ELSE now() + make_interval(hours => $14::int) END)",
    )
    .bind(id)
    .bind(st.as_uuid())
    .bind(r.kind)
    .bind(r.message_id)
    .bind(r.account_id)
    .bind(r.reporter_id)
    .bind(r.reason)
    .bind(r.detail)
    .bind(r.location)
    .bind(r.contact_name)
    .bind(r.contact_email)
    .bind(&r.evidence)
    .bind(r.statement_id)
    .bind(r.due_hours)
    .execute(db)
    .await?;
    Ok(id)
}

#[derive(Clone, Debug)]
pub struct ReportRow {
    pub id: Uuid,
    pub kind: String,
    pub message_id: Option<i64>,
    pub account_id: Option<Uuid>,
    pub reason: String,
    pub detail: String,
    pub location: String,
    pub contact_name: String,
    pub contact_email: String,
    pub evidence: Value,
    pub statement_id: Option<Uuid>,
    pub created_ms: i64,
    pub due_ms: Option<i64>,
    pub status: String,
    /// Open reports on the same message, this one included.
    pub count: i64,
}

type ReportTuple = (
    Uuid,
    String,
    Option<i64>,
    Option<Uuid>,
    String,
    String,
    String,
    String,
    String,
    Value,
    Option<Uuid>,
    i64,
    Option<i64>,
    String,
    i64,
);

fn report(r: ReportTuple) -> ReportRow {
    ReportRow {
        id: r.0,
        kind: r.1,
        message_id: r.2,
        account_id: r.3,
        reason: r.4,
        detail: r.5,
        location: r.6,
        contact_name: r.7,
        contact_email: r.8,
        evidence: r.9,
        statement_id: r.10,
        created_ms: r.11,
        due_ms: r.12,
        status: r.13,
        count: r.14,
    }
}

const REPORT_COLS: &str =
    "r.id, r.kind, r.message_id, r.account_id, r.reason, r.detail, r.location,
    r.contact_name, r.contact_email, r.evidence, r.statement_id,
    (extract(epoch FROM r.created_at) * 1000)::bigint,
    (extract(epoch FROM r.due_at) * 1000)::bigint, r.status";

/// Open items: one per reported message (the first report, with the count), every notice and
/// review on its own.
pub async fn open_items(db: &Db, st: StationId) -> sqlx::Result<Vec<ReportRow>> {
    let q = format!(
        "SELECT {REPORT_COLS}, (SELECT count(*) FROM chat_report o
             WHERE o.station_id = r.station_id AND o.status = 'open' AND o.kind = 'report'
               AND r.kind = 'report' AND o.message_id IS NOT DISTINCT FROM r.message_id
               AND o.account_id IS NOT DISTINCT FROM r.account_id)
         FROM chat_report r
         WHERE r.station_id = $1 AND r.status IN ('open', 'escalated')
           AND (r.kind <> 'report' OR r.id = (SELECT o.id FROM chat_report o
             WHERE o.station_id = r.station_id AND o.kind = 'report' AND o.status IN ('open', 'escalated')
               AND o.message_id IS NOT DISTINCT FROM r.message_id
               AND o.account_id IS NOT DISTINCT FROM r.account_id
             ORDER BY o.created_at LIMIT 1))
         ORDER BY r.created_at LIMIT 500"
    );
    let list: Vec<ReportTuple> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .fetch_all(db)
        .await?;
    Ok(list.into_iter().map(report).collect())
}

pub async fn report_by_id(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<ReportRow>> {
    let q = format!(
        "SELECT {REPORT_COLS}, 1::bigint FROM chat_report r WHERE r.station_id = $1 AND r.id = $2"
    );
    let r: Option<ReportTuple> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(r.map(report))
}

/// The open item for a message, if one exists (a new report joins it).
pub async fn open_item_for(
    db: &Db,
    st: StationId,
    message: Option<i64>,
    account: Option<Uuid>,
) -> sqlx::Result<Option<Uuid>> {
    let r: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM chat_report WHERE station_id = $1 AND kind = 'report' AND status IN ('open', 'escalated')
           AND message_id IS NOT DISTINCT FROM $2 AND account_id IS NOT DISTINCT FROM $3
         ORDER BY created_at LIMIT 1",
    )
    .bind(st.as_uuid())
    .bind(message)
    .bind(account)
    .fetch_optional(db)
    .await?;
    Ok(r.map(|r| r.0))
}

/// Close an item and every open report on the same message; returns the ids closed.
pub async fn decide(
    db: &Db,
    st: StationId,
    id: Uuid,
    status: &str,
    by: &str,
) -> sqlx::Result<Vec<Uuid>> {
    let r: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE chat_report r SET status = $3, decided_at = now(), decided_by = $4
         FROM chat_report t
         WHERE t.station_id = $1 AND t.id = $2 AND r.station_id = t.station_id
           AND r.status IN ('open', 'escalated')
           AND (r.id = t.id OR (t.kind = 'report' AND r.kind = 'report'
                AND r.message_id IS NOT DISTINCT FROM t.message_id
                AND r.account_id IS NOT DISTINCT FROM t.account_id))
         RETURNING r.id",
    )
    .bind(st.as_uuid())
    .bind(id)
    .bind(status)
    .bind(by)
    .fetch_all(db)
    .await?;
    Ok(r.into_iter().map(|r| r.0).collect())
}

/// Close the open reports that name a message (it was hidden from elsewhere).
pub async fn decide_for_message(
    db: &Db,
    st: StationId,
    message: i64,
    by: &str,
) -> sqlx::Result<Vec<Uuid>> {
    let r: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE chat_report SET status = 'actioned', decided_at = now(), decided_by = $3
         WHERE station_id = $1 AND message_id = $2 AND status = 'open' RETURNING id",
    )
    .bind(st.as_uuid())
    .bind(message)
    .bind(by)
    .fetch_all(db)
    .await?;
    Ok(r.into_iter().map(|r| r.0).collect())
}

// ---- the moderation log and statements ----

pub struct NewAction<'a> {
    pub actor: &'a str,
    pub action: &'a str,
    pub account_id: Option<Uuid>,
    pub message_id: Option<i64>,
    pub report_id: Option<Uuid>,
    pub reason: &'a str,
    pub ground: &'a str,
    pub note: &'a str,
    pub automated: bool,
    pub until_ms: Option<i64>,
}

pub async fn log(db: &Db, st: StationId, a: &NewAction<'_>) -> sqlx::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO chat_action (station_id, actor, action, account_id, message_id, report_id,
           reason, ground, note, automated, until)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, to_timestamp($11::double precision / 1000.0))
         RETURNING id",
    )
    .bind(st.as_uuid())
    .bind(a.actor)
    .bind(a.action)
    .bind(a.account_id)
    .bind(a.message_id)
    .bind(a.report_id)
    .bind(a.reason)
    .bind(a.ground)
    .bind(a.note)
    .bind(a.automated)
    .bind(a.until_ms.map(|u| u as f64))
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub type LogRow = (
    i64,
    i64,
    String,
    String,
    Option<Uuid>,
    Option<String>,
    Option<i64>,
    String,
    String,
    bool,
    Option<i64>,
);

pub async fn read_log(db: &Db, st: StationId, limit: i64) -> sqlx::Result<Vec<LogRow>> {
    sqlx::query_as(
        "SELECT l.id, (extract(epoch FROM l.at) * 1000)::bigint, l.actor, l.action, l.account_id, a.name,
                l.message_id, l.reason, l.note, l.automated, (extract(epoch FROM l.until) * 1000)::bigint
         FROM chat_action l LEFT JOIN chat_account a ON a.id = l.account_id
         WHERE l.station_id = $1 ORDER BY l.id DESC LIMIT $2",
    )
    .bind(st.as_uuid())
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn insert_statement(
    db: &Db,
    st: StationId,
    account: Uuid,
    action: i64,
    text: &str,
    automated: bool,
    review_days: u32,
) -> sqlx::Result<(Uuid, i64, i64)> {
    let id = Uuid::new_v4();
    let (at, by): (i64, i64) = sqlx::query_as(
        "INSERT INTO chat_statement (id, station_id, account_id, action_id, text, automated, review_by)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(days => $7))
         RETURNING (extract(epoch FROM at) * 1000)::bigint, (extract(epoch FROM review_by) * 1000)::bigint",
    )
    .bind(id)
    .bind(st.as_uuid())
    .bind(account)
    .bind(action)
    .bind(text)
    .bind(automated)
    .bind(review_days as i32)
    .fetch_one(db)
    .await?;
    Ok((id, at, by))
}

pub type StatementRow = (Uuid, i64, String, bool, i64, bool);

pub async fn statements(db: &Db, st: StationId, account: Uuid) -> sqlx::Result<Vec<StatementRow>> {
    sqlx::query_as(
        "SELECT id, (extract(epoch FROM at) * 1000)::bigint, text, automated,
                (extract(epoch FROM review_by) * 1000)::bigint, review_requested_at IS NOT NULL
         FROM chat_statement WHERE station_id = $1 AND account_id = $2 ORDER BY at DESC LIMIT 50",
    )
    .bind(st.as_uuid())
    .bind(account)
    .fetch_all(db)
    .await
}

/// Mark a review asked for; `None` when the statement is not theirs, already reviewed, or the
/// window has closed.
pub async fn request_review(
    db: &Db,
    st: StationId,
    account: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<String>> {
    let r: Option<(String,)> = sqlx::query_as(
        "UPDATE chat_statement SET review_requested_at = now()
         WHERE station_id = $1 AND account_id = $2 AND id = $3
           AND review_requested_at IS NULL AND review_by > now()
         RETURNING text",
    )
    .bind(st.as_uuid())
    .bind(account)
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(r.map(|r| r.0))
}

// ---- evidence holds ----

pub async fn hold_evidence(
    db: &Db,
    st: StationId,
    report: Uuid,
    account: Option<Uuid>,
    messages: &[i64],
    reason: &str,
    by: &str,
) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chat_hold (id, station_id, report_id, account_id, message_ids, reason, held_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(st.as_uuid())
    .bind(report)
    .bind(account)
    .bind(messages)
    .bind(reason)
    .bind(by)
    .execute(db)
    .await?;
    Ok(id)
}

pub async fn release_hold(db: &Db, st: StationId, id: Uuid, by: &str) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE chat_hold SET released_at = now(), released_by = $3
         WHERE station_id = $1 AND id = $2 AND released_at IS NULL",
    )
    .bind(st.as_uuid())
    .bind(id)
    .bind(by)
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub type HoldRow = (
    Uuid,
    Option<Uuid>,
    Option<Uuid>,
    Vec<i64>,
    String,
    String,
    i64,
    Option<i64>,
    Option<String>,
);

pub async fn holds(db: &Db, st: StationId) -> sqlx::Result<Vec<HoldRow>> {
    sqlx::query_as(
        "SELECT id, report_id, account_id, message_ids, reason, held_by,
                (extract(epoch FROM held_at) * 1000)::bigint,
                (extract(epoch FROM released_at) * 1000)::bigint, released_by
         FROM chat_hold WHERE station_id = $1 ORDER BY held_at DESC LIMIT 200",
    )
    .bind(st.as_uuid())
    .fetch_all(db)
    .await
}

// ---- settings ----

pub async fn load_settings(
    db: &Db,
    st: StationId,
    first: &StationSettings,
) -> sqlx::Result<StationSettings> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT settings FROM chat_settings WHERE station_id = $1")
            .bind(st.as_uuid())
            .fetch_optional(db)
            .await?;
    Ok(row
        .and_then(|(v,)| serde_json::from_value(v).ok())
        .unwrap_or_else(|| first.clone()))
}

pub async fn save_settings(db: &Db, st: StationId, s: &StationSettings) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO chat_settings (station_id, settings) VALUES ($1, $2)
         ON CONFLICT (station_id) DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
    )
    .bind(st.as_uuid())
    .bind(serde_json::to_value(s).unwrap_or_default())
    .execute(db)
    .await?;
    Ok(())
}

// ---- export, erasure and retention ----

/// Everything kept about one chatter, as JSON (GDPR Article 15 and 20).
pub async fn export(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<Value>> {
    let Some(a) = account_by_id(db, st, id).await? else {
        return Ok(None);
    };
    let msgs: Vec<(i64, String, String, i64, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, kind, body, (extract(epoch FROM server_at) * 1000)::bigint,
                (extract(epoch FROM playhead_at) * 1000)::bigint, state
         FROM chat_message WHERE station_id = $1 AND account_id = $2 ORDER BY id",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_all(db)
    .await?;
    let reports: Vec<(Uuid, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT id, kind, reason, detail, (extract(epoch FROM created_at) * 1000)::bigint, status
         FROM chat_report WHERE station_id = $1 AND (reporter_id = $2 OR account_id = $2)",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_all(db)
    .await?;
    let actions = sqlx::query_as::<_, (i64, String, String, String, bool)>(
        "SELECT (extract(epoch FROM at) * 1000)::bigint, action, reason, actor, automated
         FROM chat_action WHERE station_id = $1 AND account_id = $2 ORDER BY id",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_all(db)
    .await?;
    let sts = statements(db, st, id).await?;
    let sessions: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT (extract(epoch FROM created_at) * 1000)::bigint, host(address)
         FROM chat_session WHERE account_id = $1",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let restriction = restriction(db, st, id).await?;
    Ok(Some(serde_json::json!({
        "account": {
            "id": a.id, "kind": a.kind, "name": a.name, "standing": a.standing,
            "created_ms": a.created_ms, "age_declared": a.age_declared,
            "terms_version": a.terms_version,
            "email": "kept only as a SHA-256 hash, never as written",
        },
        "restriction": restriction.map(|r| match r {
            Restriction::Banned => serde_json::json!({"kind": "ban"}),
            Restriction::TimedOut { until_ms } => serde_json::json!({"kind": "timeout", "until_ms": until_ms}),
        }),
        "messages": msgs.iter().map(|m| serde_json::json!({
            "id": m.0, "kind": m.1, "body": m.2, "server_ms": m.3, "playhead_ms": m.4, "state": m.5,
        })).collect::<Vec<_>>(),
        "reports": reports.iter().map(|r| serde_json::json!({
            "id": r.0, "kind": r.1, "reason": r.2, "detail": r.3, "created_ms": r.4, "status": r.5,
        })).collect::<Vec<_>>(),
        "moderation": actions.iter().map(|x| serde_json::json!({
            "at_ms": x.0, "action": x.1, "reason": x.2, "by": x.3, "automated": x.4,
        })).collect::<Vec<_>>(),
        "statements": sts.iter().map(|s| serde_json::json!({
            "id": s.0, "at_ms": s.1, "text": s.2, "automated": s.3, "review_by_ms": s.4, "review_requested": s.5,
        })).collect::<Vec<_>>(),
        "sessions": sessions.iter().map(|s| serde_json::json!({"created_ms": s.0, "address": s.1})).collect::<Vec<_>>(),
    })))
}

/// Erase a chatter: their messages (except any under an evidence hold), sessions, sign-in
/// codes and statements go; the account row stays, anonymised, so the moderation log still
/// reads. Returns the ids of the messages removed.
pub async fn erase(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Vec<i64>> {
    let mut tx = db.begin().await?;
    raise_message_floor(&mut *tx, st).await?;
    let gone: Vec<(i64,)> = sqlx::query_as(
        "DELETE FROM chat_message m WHERE m.station_id = $1 AND m.account_id = $2
           AND NOT EXISTS (SELECT 1 FROM chat_hold h WHERE h.station_id = m.station_id
                           AND h.released_at IS NULL AND m.id = ANY(h.message_ids))
         RETURNING m.id",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM chat_session WHERE account_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM chat_statement WHERE account_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // Reports they made keep their reason, not their text; reports about them lose the words
    // unless a hold keeps them.
    sqlx::query(
        "UPDATE chat_report r SET detail = '', contact_name = '', contact_email = '',
           evidence = CASE WHEN r.account_id = $2 AND NOT EXISTS (SELECT 1 FROM chat_hold h
             WHERE h.report_id = r.id AND h.released_at IS NULL) THEN '{}'::jsonb ELSE r.evidence END
         WHERE r.station_id = $1 AND (r.reporter_id = $2 OR r.account_id = $2)",
    )
    .bind(st.as_uuid())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE chat_account SET name = 'erased ' || left(id::text, 8), email_hash = NULL,
           erased_at = now() WHERE station_id = $1 AND id = $2",
    )
    .bind(st.as_uuid())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(gone.into_iter().map(|r| r.0).collect())
}

/// What one sweep removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub messages: u64,
    pub moderation: u64,
    pub addresses: u64,
    pub sessions: u64,
}

/// Apply the retention periods (seconds; `None`: no end): messages older than `messages_s` go
/// (with `until_show_ends`, a message written during a scheduled episode goes when that
/// episode ends instead, and `messages_s` applies only to messages written outside any
/// episode: ADR-0149), unless a hold names them, by the chat's escalation (`chat_hold`) or the station's
/// retention holds (ADR-0116: `retention_hold`, by record or by a window of time); decided
/// reports, log entries and statements older than `moderation_s` go unless a hold names them
/// (an open report always stays until decided); addresses older than `addresses_s` are cleared
/// (`None`: none are stored, so clear all).
pub async fn sweep(
    db: &Db,
    st: StationId,
    messages_s: Option<i64>,
    until_show_ends: bool,
    moderation_s: Option<i64>,
    addresses_s: Option<i64>,
) -> sqlx::Result<Swept> {
    let s = st.as_uuid();
    let mut messages = 0;
    if let Some(messages_s) = messages_s {
        raise_message_floor(db, st).await?;
        messages = sqlx::query(
            "DELETE FROM chat_message m WHERE m.station_id = $1
               AND COALESCE(
                     CASE WHEN $3 THEN
                       (SELECT max(x.ends_at) FROM (
                          SELECT e.air_at + make_interval(mins => COALESCE(e.minutes,
                                   CASE WHEN se.slot_weekday IS NOT NULL THEN se.slot_minutes
                                        ELSE sh.slot_minutes END)::int) AS ends_at
                            FROM episode e
                            JOIN series se ON se.id = e.series_id
                            JOIN show sh ON sh.id = se.show_id
                           WHERE e.station_id = m.station_id AND e.air_at <= m.server_at) x
                         WHERE x.ends_at > m.server_at)
                     END,
                     m.server_at + make_interval(secs => $2)) < now()
               AND NOT EXISTS (SELECT 1 FROM chat_hold h WHERE h.station_id = m.station_id
                               AND h.released_at IS NULL AND m.id = ANY(h.message_ids))
               AND NOT retention_held(m.station_id, 'chat_message', m.id, m.server_at, m.server_at)",
        )
        .bind(s)
        .bind(messages_s as f64)
        .bind(until_show_ends)
        .execute(db)
        .await?
        .rows_affected();
    }
    let mut moderation = 0;
    if let Some(moderation_s) = moderation_s {
        moderation += sqlx::query(
            "DELETE FROM chat_report r WHERE r.station_id = $1 AND r.status NOT IN ('open', 'escalated')
               AND r.decided_at < now() - make_interval(secs => $2)
               AND NOT EXISTS (SELECT 1 FROM chat_hold h WHERE h.report_id = r.id AND h.released_at IS NULL)
               AND NOT retention_held(r.station_id, 'moderation_record', -1, r.created_at, r.decided_at)",
        )
        .bind(s)
        .bind(moderation_s as f64)
        .execute(db)
        .await?
        .rows_affected();
        moderation += sqlx::query(
            "DELETE FROM chat_statement t WHERE t.station_id = $1
               AND t.at < now() - make_interval(secs => $2) AND t.review_by < now()
               AND NOT retention_held(t.station_id, 'moderation_record', -1, t.at, t.at)",
        )
        .bind(s)
        .bind(moderation_s as f64)
        .execute(db)
        .await?
        .rows_affected();
        moderation += sqlx::query(
            "DELETE FROM chat_action l WHERE l.station_id = $1 AND l.at < now() - make_interval(secs => $2)
               AND NOT EXISTS (SELECT 1 FROM chat_hold h WHERE h.released_at IS NULL
                               AND (h.report_id = l.report_id OR h.account_id = l.account_id))
               AND NOT retention_held(l.station_id, 'moderation_record', l.id, l.at, l.at)",
        )
        .bind(s)
        .bind(moderation_s as f64)
        .execute(db)
        .await?
        .rows_affected();
        moderation += sqlx::query(
            "DELETE FROM chat_hold WHERE station_id = $1 AND released_at < now() - make_interval(secs => $2)",
        )
        .bind(s)
        .bind(moderation_s as f64)
        .execute(db)
        .await?
        .rows_affected();
    }
    let addresses = sqlx::query(
        "UPDATE chat_session SET address = NULL WHERE station_id = $1 AND address IS NOT NULL
           AND ($2::double precision IS NULL OR created_at < now() - make_interval(secs => $2))",
    )
    .bind(s)
    .bind(addresses_s.map(|a| a as f64))
    .execute(db)
    .await?
    .rows_affected();
    let mut sessions =
        sqlx::query("DELETE FROM chat_session WHERE station_id = $1 AND expires_at < now()")
            .bind(s)
            .execute(db)
            .await?
            .rows_affected();
    sessions += sqlx::query(
        "DELETE FROM chat_signin WHERE station_id = $1 AND created_at < now() - interval '1 day'",
    )
    .bind(s)
    .execute(db)
    .await?
    .rows_affected();
    sqlx::query(
        "DELETE FROM chat_restriction WHERE station_id = $1 AND until IS NOT NULL AND until < now()",
    )
    .bind(s)
    .execute(db)
    .await?;
    Ok(Swept {
        messages,
        moderation,
        addresses,
        sessions,
    })
}
