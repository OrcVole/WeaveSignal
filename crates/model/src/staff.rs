// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Staff accounts and their sessions (ADR-0127, migration 0012).
//!
//! Each member of staff signs in as themselves with a password, which is kept only as an
//! Argon2id hash. A session is a random token kept only as its SHA-256. The control plane holds
//! the live sessions in memory and writes them here; the chat reads them here.

use std::fmt;
use std::str::FromStr;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::StationId;

/// What a staff session's token starts with, so a station key is never mistaken for one.
pub const TOKEN_PREFIX: &str = "rss_";

/// A password's shortest and longest, in characters (no rules on what it contains, as NIST
/// SP 800-63B advises; a long phrase is welcome).
pub const PASSWORD_MIN: usize = 10;
pub const PASSWORD_MAX: usize = 256;

/// A staff member's role, station-wide (`top-roles-scoped` a). Release one enforces two
/// tiers: `Admin` alone may change retention, erase people, see personal data and manage
/// staff; every role may run the studio. The role is recorded either way, so a finer split
/// can follow without a migration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = StaffRole)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Producer,
    Presenter,
    Screener,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Producer => "producer",
            Role::Presenter => "presenter",
            Role::Screener => "screener",
        }
    }

    pub fn is_admin(self) -> bool {
        self == Role::Admin
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "producer" => Ok(Role::Producer),
            "presenter" => Ok(Role::Presenter),
            "screener" => Ok(Role::Screener),
            other => Err(format!("no such role: {other}")),
        }
    }
}

/// A staff account as the routes show it (never the hash).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Account {
    pub id: Uuid,
    pub username: String,
    pub name: String,
    pub role: Role,
    /// Unix milliseconds; `None` while the account is on.
    pub disabled_ms: Option<u64>,
    pub created_ms: u64,
    /// Whether an address is set for the emailed code (the address itself is never kept).
    pub has_email: bool,
}

/// A live session as the control plane holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub token_hash: [u8; 32],
    pub account: Account,
    pub created_ms: u64,
    pub expires_ms: u64,
}

/// SHA-256 of a token: what the database keeps.
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// A fresh session token: the prefix and 244 random bits from the operating system.
pub fn new_token() -> String {
    format!(
        "{TOKEN_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

/// A username as kept: lower case, trimmed; `None` when it is not one the table accepts
/// (a letter or digit first, then letters, digits, `.`, `_` or `-`, at most 40).
pub fn clean_username(s: &str) -> Option<String> {
    let u = s.trim().to_lowercase();
    let mut chars = u.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    (first_ok && rest_ok && u.len() <= 40).then_some(u)
}

/// A display name as kept: spaces collapsed, no control characters, 1 to 80 characters.
pub fn clean_name(s: &str) -> Option<String> {
    let n = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = n.chars().count();
    ((1..=80).contains(&len) && !n.chars().any(char::is_control)).then_some(n)
}

/// Why a password is refused, in a sentence for the person choosing it; `None` when it is fine.
pub fn password_problem(password: &str, username: &str) -> Option<String> {
    let len = password.chars().count();
    if len < PASSWORD_MIN {
        return Some(format!(
            "A password needs at least {PASSWORD_MIN} characters."
        ));
    }
    if len > PASSWORD_MAX {
        return Some(format!("A password is at most {PASSWORD_MAX} characters."));
    }
    if password.trim().to_lowercase() == username {
        return Some("A password cannot be the username.".into());
    }
    None
}

/// Hash a password as Argon2id with a fresh salt (PHC string). Slow on purpose: call it off
/// the async threads.
pub fn hash_password(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Whether a password matches a stored hash. With no hash (an unknown username, or an account
/// with no password) it still spends the same work on a fixed hash, so the answer's timing
/// does not tell which usernames exist.
pub fn verify_password(password: &str, hash: Option<&str>) -> bool {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let dummy = DUMMY.get_or_init(|| hash_password("not anyone's password").unwrap_or_default());
    match hash {
        Some(h) => Argon2::default()
            .verify_password(password.as_bytes(), h)
            .is_ok(),
        None => {
            let _ = Argon2::default().verify_password(password.as_bytes(), dummy.as_str());
            false
        }
    }
}

const ACCOUNT_COLS: &str = "id, username, name, role,
    (extract(epoch FROM disabled_at) * 1000)::float8,
    (extract(epoch FROM created_at) * 1000)::float8,
    (email_hash IS NOT NULL)";

type AccountRow = (Uuid, String, String, String, Option<f64>, f64, bool);
type SignInRow = (
    Uuid,
    String,
    String,
    String,
    Option<f64>,
    f64,
    bool,
    Option<String>,
);

fn account(r: AccountRow) -> Account {
    Account {
        id: r.0,
        username: r.1,
        name: r.2,
        role: r.3.parse().unwrap_or(Role::Screener),
        disabled_ms: r.4.map(|ms| ms.max(0.0) as u64),
        created_ms: r.5.max(0.0) as u64,
        has_email: r.6,
    }
}

/// A new account. Fails on a taken username (a unique violation).
pub async fn create_account(
    pool: &sqlx::PgPool,
    station: StationId,
    username: &str,
    name: &str,
    role: Role,
    password_hash: &str,
) -> Result<Account, sqlx::Error> {
    let q = format!(
        "INSERT INTO staff_account (id, station_id, username, name, role, password_hash)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {ACCOUNT_COLS}"
    );
    let r: AccountRow = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(Uuid::new_v4())
        .bind(station.as_uuid())
        .bind(username)
        .bind(name)
        .bind(role.as_str())
        .bind(password_hash)
        .fetch_one(pool)
        .await?;
    Ok(account(r))
}

/// Every account, on or off, by username.
pub async fn list_accounts(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Account>, sqlx::Error> {
    let q =
        format!("SELECT {ACCOUNT_COLS} FROM staff_account WHERE station_id = $1 ORDER BY username");
    let rows: Vec<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(account).collect())
}

pub async fn get_account(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<Account>, sqlx::Error> {
    let q = format!("SELECT {ACCOUNT_COLS} FROM staff_account WHERE station_id = $1 AND id = $2");
    let r: Option<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(account))
}

/// The account a username signs in to, with its password hash; `None` when there is none.
pub async fn account_for_sign_in(
    pool: &sqlx::PgPool,
    station: StationId,
    username: &str,
) -> Result<Option<(Account, Option<String>)>, sqlx::Error> {
    let q = format!(
        "SELECT {ACCOUNT_COLS}, password_hash FROM staff_account
         WHERE station_id = $1 AND username = $2"
    );
    let r: Option<SignInRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .bind(username)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(|r| (account((r.0, r.1, r.2, r.3, r.4, r.5, r.6)), r.7)))
}

/// The stored hash of one account's password.
pub async fn password_hash_of(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let r: Option<(Option<String>,)> =
        sqlx::query_as("SELECT password_hash FROM staff_account WHERE station_id = $1 AND id = $2")
            .bind(station.as_uuid())
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(r.and_then(|r| r.0))
}

/// What an admin may change on an account; `None` leaves a field as it is.
#[derive(Clone, Debug, Default)]
pub struct Change {
    pub name: Option<String>,
    pub role: Option<Role>,
    pub disabled: Option<bool>,
}

/// Change an account. Turning one off ends its sessions here and its chat sessions, in the
/// same transaction; the control plane drops them from memory as well.
pub async fn change_account(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    c: &Change,
) -> Result<Option<Account>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let q = format!(
        "UPDATE staff_account SET
            name = coalesce($3, name),
            role = coalesce($4, role),
            disabled_at = CASE WHEN $5::boolean IS NULL THEN disabled_at
                               WHEN $5 THEN coalesce(disabled_at, now())
                               ELSE NULL END,
            updated_at = now()
         WHERE station_id = $1 AND id = $2 RETURNING {ACCOUNT_COLS}"
    );
    let r: Option<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .bind(id)
        .bind(c.name.as_deref())
        .bind(c.role.map(Role::as_str))
        .bind(c.disabled)
        .fetch_optional(&mut *tx)
        .await?;
    if c.disabled == Some(true) && r.is_some() {
        end_all_sessions(&mut tx, station, id).await?;
    }
    tx.commit().await?;
    Ok(r.map(account))
}

/// Set an account's password, ending every session it has (here and in the chat) except
/// `keep`, the session that changed it, when it is the person's own.
pub async fn set_password(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    password_hash: &str,
    keep: Option<&[u8; 32]>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let done = sqlx::query(
        "UPDATE staff_account SET password_hash = $3, updated_at = now()
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(password_hash)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if done {
        sqlx::query(
            "DELETE FROM staff_session WHERE station_id = $1 AND account_id = $2
               AND ($3::bytea IS NULL OR token_hash <> $3)",
        )
        .bind(station.as_uuid())
        .bind(id)
        .bind(keep.map(|k| k.to_vec()))
        .execute(&mut *tx)
        .await?;
        end_chat_sessions(&mut tx, station, id).await?;
        // A new password ends a code asked for before it, as it ends the sessions.
        spend_codes(&mut tx, id).await?;
    }
    tx.commit().await?;
    Ok(done)
}

/// How long an emailed sign-in code lives, in minutes.
pub const CODE_MINUTES: i32 = 10;
/// How many guesses one code takes before it is spent, right or wrong.
pub const CODE_TRIES: i16 = 5;

/// Set the keyed hash of the address an account's codes go to, or take it away with `None`.
/// Any code asked for before is spent: it was made for the old address. `Ok(false)` when there
/// is no such account; a unique violation when another account holds the address.
pub async fn set_email_hash(
    pool: &sqlx::PgPool,
    station: StationId,
    id: Uuid,
    hash: Option<&[u8]>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let done = sqlx::query(
        "UPDATE staff_account SET email_hash = $3, updated_at = now()
         WHERE station_id = $1 AND id = $2",
    )
    .bind(station.as_uuid())
    .bind(id)
    .bind(hash)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if done {
        spend_codes(&mut tx, id).await?;
    }
    tx.commit().await?;
    Ok(done)
}

/// The account whose address has this keyed hash, if it is on: whom a typed address names.
pub async fn account_for_email_hash(
    pool: &sqlx::PgPool,
    station: StationId,
    hash: &[u8],
) -> Result<Option<Account>, sqlx::Error> {
    let q = format!(
        "SELECT {ACCOUNT_COLS} FROM staff_account
         WHERE station_id = $1 AND email_hash = $2 AND disabled_at IS NULL"
    );
    let r: Option<AccountRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .bind(hash)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(account))
}

/// Keep a code asked for by an account, unless it has already asked for `max_recent` in the last
/// ten minutes (`Ok(false)`, nothing kept). The older codes are spent, so at most one is live:
/// the guesses an account can take are the tries of its newest code alone.
pub async fn issue_code(
    pool: &sqlx::PgPool,
    station: StationId,
    account: Uuid,
    code_hash: &[u8],
    max_recent: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The account's row is the lock: two requests at once count one after the other.
    let on: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM staff_account
         WHERE station_id = $1 AND id = $2 AND disabled_at IS NULL FOR UPDATE",
    )
    .bind(station.as_uuid())
    .bind(account)
    .fetch_optional(&mut *tx)
    .await?;
    if on.is_none() {
        return Ok(false);
    }
    sqlx::query(
        "DELETE FROM staff_signin WHERE account_id = $1 AND expires_at < now() - interval '1 day'",
    )
    .bind(account)
    .execute(&mut *tx)
    .await?;
    let (recent,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM staff_signin
         WHERE account_id = $1 AND created_at > now() - interval '10 minutes'",
    )
    .bind(account)
    .fetch_one(&mut *tx)
    .await?;
    if recent >= max_recent {
        return Ok(false);
    }
    spend_codes(&mut tx, account).await?;
    sqlx::query(
        "INSERT INTO staff_signin (id, station_id, account_id, code_hash, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(mins => $5))",
    )
    .bind(Uuid::new_v4())
    .bind(station.as_uuid())
    .bind(account)
    .bind(code_hash)
    .bind(CODE_MINUTES)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// What a guess at a code came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCheck {
    /// The code was right, and is now spent: it cannot be used again.
    Good,
    /// There was a live code and this was not it.
    Wrong,
    /// There was no live code: none asked for, expired, used, or out of tries.
    Spent,
}

/// Try a guess at the account's newest live code. `matches` compares the guess with the stored
/// hash (in constant time). Every guess counts as a try before it is compared, so concurrent
/// guesses cannot beat the limit, and a right code is spent by a single update, so two requests
/// with it cannot both sign in.
pub async fn check_code(
    pool: &sqlx::PgPool,
    station: StationId,
    account: Uuid,
    matches: impl FnOnce(&[u8]) -> bool,
) -> Result<CodeCheck, sqlx::Error> {
    let tried: Option<(Uuid, Vec<u8>)> = sqlx::query_as(
        "UPDATE staff_signin SET attempts = attempts + 1
         WHERE id = (SELECT id FROM staff_signin
                     WHERE station_id = $1 AND account_id = $2 AND NOT used
                       AND expires_at > now() AND attempts < $3
                     ORDER BY created_at DESC LIMIT 1 FOR UPDATE)
         RETURNING id, code_hash",
    )
    .bind(station.as_uuid())
    .bind(account)
    .bind(CODE_TRIES)
    .fetch_optional(pool)
    .await?;
    let Some((id, want)) = tried else {
        return Ok(CodeCheck::Spent);
    };
    if !matches(&want) {
        return Ok(CodeCheck::Wrong);
    }
    let won = sqlx::query("UPDATE staff_signin SET used = true WHERE id = $1 AND NOT used")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected()
        == 1;
    Ok(if won {
        CodeCheck::Good
    } else {
        CodeCheck::Spent
    })
}

/// Spend every code an account has asked for.
async fn spend_codes(tx: &mut sqlx::PgConnection, account: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE staff_signin SET used = true WHERE account_id = $1 AND NOT used")
        .bind(account)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

async fn end_all_sessions(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM staff_session WHERE station_id = $1 AND account_id = $2")
        .bind(station.as_uuid())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    spend_codes(tx, id).await?;
    end_chat_sessions(tx, station, id).await
}

/// A staff member's chat sessions end with their staff powers (review 0025, C2).
async fn end_chat_sessions(
    tx: &mut sqlx::PgConnection,
    station: StationId,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM chat_session WHERE station_id = $1 AND account_id IN
           (SELECT id FROM chat_account WHERE station_id = $1 AND staff_account_id = $2)",
    )
    .bind(station.as_uuid())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Keep a new session.
pub async fn create_session(
    pool: &sqlx::PgPool,
    station: StationId,
    account: Uuid,
    token_hash: &[u8; 32],
    expires_ms: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO staff_session (token_hash, station_id, account_id, expires_at)
         VALUES ($1, $2, $3, to_timestamp($4::float8 / 1000))",
    )
    .bind(token_hash.to_vec())
    .bind(station.as_uuid())
    .bind(account)
    .bind(expires_ms as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Move a session's end later (it is in use).
pub async fn extend_session(
    pool: &sqlx::PgPool,
    token_hash: &[u8; 32],
    expires_ms: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE staff_session SET expires_at = to_timestamp($2::float8 / 1000) WHERE token_hash = $1",
    )
    .bind(token_hash.to_vec())
    .bind(expires_ms as f64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn end_session(pool: &sqlx::PgPool, token_hash: &[u8; 32]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM staff_session WHERE token_hash = $1")
        .bind(token_hash.to_vec())
        .execute(pool)
        .await?;
    Ok(())
}

const SESSION_SELECT: &str = "SELECT s.token_hash,
    a.id, a.username, a.name, a.role,
    (extract(epoch FROM a.disabled_at) * 1000)::float8,
    (extract(epoch FROM a.created_at) * 1000)::float8,
    (a.email_hash IS NOT NULL),
    (extract(epoch FROM s.created_at) * 1000)::float8,
    (extract(epoch FROM s.expires_at) * 1000)::float8
  FROM staff_session s JOIN staff_account a ON a.id = s.account_id
  WHERE s.station_id = $1 AND s.expires_at > now() AND a.disabled_at IS NULL";

type SessionRow = (
    Vec<u8>,
    Uuid,
    String,
    String,
    String,
    Option<f64>,
    f64,
    bool,
    f64,
    f64,
);

fn session(r: SessionRow) -> Option<Session> {
    let token_hash: [u8; 32] = r.0.try_into().ok()?;
    Some(Session {
        token_hash,
        account: account((r.1, r.2, r.3, r.4, r.5, r.6, r.7)),
        created_ms: r.8.max(0.0) as u64,
        expires_ms: r.9.max(0.0) as u64,
    })
}

/// Every live session of an account that is on: what the control plane reads back at start.
/// Ended ones are deleted first.
pub async fn live_sessions(
    pool: &sqlx::PgPool,
    station: StationId,
) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query("DELETE FROM staff_session WHERE station_id = $1 AND expires_at <= now()")
        .bind(station.as_uuid())
        .execute(pool)
        .await?;
    let rows: Vec<SessionRow> = sqlx::query_as(SESSION_SELECT)
        .bind(station.as_uuid())
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().filter_map(session).collect())
}

/// The live session a token names, if any: how the chat checks a staff member on each action,
/// so one turned off or signed out loses the chat's powers at once.
pub async fn session_for_token(
    pool: &sqlx::PgPool,
    station: StationId,
    token: &str,
) -> Result<Option<Session>, sqlx::Error> {
    let q = format!("{SESSION_SELECT} AND s.token_hash = $2");
    let r: Option<SessionRow> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(station.as_uuid())
        .bind(token_hash(token).to_vec())
        .fetch_optional(pool)
        .await?;
    Ok(r.and_then(session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_kept_lower_case_and_plain() {
        assert_eq!(clean_username("  Morag.M "), Some("morag.m".into()));
        assert_eq!(clean_username("ailsa_b-2"), Some("ailsa_b-2".into()));
        assert_eq!(clean_username(".hidden"), None);
        assert_eq!(clean_username(""), None);
        assert_eq!(clean_username("two words"), None);
        assert_eq!(clean_username("eòghann"), None);
        assert_eq!(clean_username(&"a".repeat(41)), None);
    }

    #[test]
    fn password_rules_are_length_only() {
        assert!(password_problem("short", "fraser").is_some());
        assert!(password_problem("Fraser    ", "fraser").is_some());
        assert!(password_problem("correct horse battery", "fraser").is_none());
        assert!(password_problem("aaaaaaaaaa", "fraser").is_none());
        assert!(password_problem(&"x".repeat(257), "fraser").is_some());
    }

    #[test]
    fn a_password_hash_verifies_and_nothing_else_does() {
        let h = hash_password("a long enough phrase").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("a long enough phrase", Some(&h)));
        assert!(!verify_password("a long enough phrasE", Some(&h)));
        assert!(!verify_password("a long enough phrase", None));
        // Each hash has its own salt.
        assert_ne!(h, hash_password("a long enough phrase").unwrap());
    }

    #[test]
    fn tokens_carry_the_prefix_and_differ() {
        let a = new_token();
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_eq!(a.len(), TOKEN_PREFIX.len() + 64);
        assert_ne!(a, new_token());
        assert_ne!(token_hash(&a), token_hash(&new_token()));
    }

    #[test]
    fn roles_round_trip() {
        for r in [Role::Admin, Role::Producer, Role::Presenter, Role::Screener] {
            assert_eq!(r.as_str().parse::<Role>(), Ok(r));
        }
        assert!("owner".parse::<Role>().is_err());
    }
}
