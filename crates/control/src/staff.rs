// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Staff accounts and sign-in (ADR-0127).
//!
//! Each member of staff signs in as themselves with a password and gets a session token, which
//! the studio app keeps in the keychain (ADR-0121) and sends as its bearer. The control plane
//! holds the live sessions in memory, as it holds ended passes (ADR-0119), writes them to the
//! database and reads them back at start; so every check stays a lookup, and an account turned
//! off loses its powers on its next request.
//!
//! The station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`) stays the station's own credential: the
//! chat reads the studio's view with it, and it creates the first admin account, so a station
//! is never locked out of its own staff list.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `POST /api/v1/sessions` | anyone | `username` and `password` for a session token |
//! | `POST /api/v1/sessions/code` | anyone | `username` and `email`: a code is emailed if they name one account (always 202) |
//! | `POST /api/v1/sessions/code/finish` | anyone | `username`, `email` and `code` for a session token |
//! | `PUT /api/v1/sessions/me/email` | a session | set or clear your own sign-in address (`current` password, `email`) |
//! | `GET /api/v1/sessions/me` | a session or the key | who the bearer is, and their role |
//! | `DELETE /api/v1/sessions/me` | a session | sign out |
//! | `POST /api/v1/sessions/me/password` | a session | change your own password (`current`, `new`) |
//! | `GET /api/v1/staff` | admin | every account |
//! | `POST /api/v1/staff` | admin | a new account: `username`, `name`, `role`, `password` |
//! | `PUT /api/v1/staff/{id}` | admin | `name`, `role`, `disabled` (off ends its sessions) |
//! | `POST /api/v1/staff/{id}/password` | admin | set someone's password (ends their sessions) |
//! | `PUT /api/v1/staff/{id}/email` | admin | set or clear the address their codes go to |
//!
//! **The emailed code** (ADR-0127) is a second way in beside the password, on when the station
//! has mail (`[mail]` and `WEAVESIGNAL_CONTROL_EMAIL_KEY`). An address is kept only as a keyed
//! hash ([`crate::mailkey`]); the person types it at sign-in with their username, and a code
//! is sent only if both name one account that is on. Asking always answers the same, so it tells
//! nobody which usernames or addresses exist; the lookup and the sending happen after the answer.
//! A code is six digits, kept as a keyed hash, good for ten minutes, five tries and one use; a
//! new code spends the older ones. Asking is limited per account (3 in ten minutes) and per
//! address (10), guessing by the same lockout as passwords, counted apart from them so that
//! someone trying passwords at a username does not shut its owner out of the emailed code.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use weavesignal_model::StationId;
use weavesignal_model::staff::{self as model, Account, Role, Session};

use crate::app::{AppState, problem, same};
use crate::config::{MailConfig, MailMode};
use crate::hub::now_ms;
use crate::mail;
use crate::mailkey::MailKey;

/// A session ends this long after it was last used...
pub const SESSION_IDLE_MS: u64 = 12 * 3_600_000;
/// ...and never later than this after sign-in.
pub const SESSION_LONGEST_MS: u64 = 7 * 24 * 3_600_000;
/// A session's end is written forward at most this often while it is in use.
const EXTEND_EVERY_MS: u64 = 15 * 60_000;
/// Failed sign-ins are counted over this window...
const FAIL_WINDOW_MS: u64 = 15 * 60_000;
/// ...and refused past this many for one username, or for one address.
const FAILS_PER_USERNAME: usize = 5;
const FAILS_PER_ADDRESS: usize = 20;
/// Emailed codes asked for are counted over ten minutes: per account (kept in the database,
/// only for codes actually made) and per address (kept in memory, every ask).
const CODES_PER_ACCOUNT: i64 = 3;
const CODES_PER_ADDRESS: usize = 10;
const CODE_WINDOW_MS: u64 = 10 * 60_000;

/// Who a request comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Actor {
    /// The station key: the station itself, with an admin's powers.
    Station,
    /// A member of staff, signed in.
    Staff(Account),
    /// An API token (ADR-0195), granted this route by the gate. Never an admin.
    Token(crate::api::TokenActor),
}

impl Actor {
    pub fn is_admin(&self) -> bool {
        match self {
            Actor::Station => true,
            Actor::Staff(a) => a.role.is_admin(),
            Actor::Token(_) => false,
        }
    }

    /// How logs name them: a username, or "station key".
    pub fn label(&self) -> String {
        match self {
            Actor::Station => "station key".into(),
            Actor::Staff(a) => a.username.clone(),
            Actor::Token(t) => format!("API token {}", t.name),
        }
    }
}

/// What a route needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Any member of staff, or the station key: running the studio.
    Staff,
    /// An admin, or the station key: retention, erasure, personal data, the staff list.
    Admin,
}

/// What the emailed code needs: how mail goes out, the key that keeps addresses unreadable, and
/// the station's name for the email's words.
#[derive(Clone, Debug)]
pub struct Mailer {
    pub config: MailConfig,
    pub key: MailKey,
    pub station: String,
}

impl Mailer {
    /// The mailer the settings describe: `None` with mail off, an error with mail on and no key.
    pub fn from_config(
        config: &crate::config::Config,
        station: &str,
    ) -> Result<Option<Self>, String> {
        if config.mail.mode == MailMode::Off {
            return Ok(None);
        }
        let key = config.email_key.clone().ok_or_else(|| {
            format!(
                "`mail.mode` is on, so {} must be set: at least {} characters, for example `openssl rand -hex 32`",
                MailKey::ENV,
                MailKey::MIN_LEN
            )
        })?;
        Ok(Some(Self {
            config: config.mail.clone(),
            key,
            station: station.to_owned(),
        }))
    }
}

/// The live sessions and the failed sign-ins, in memory.
#[derive(Default)]
pub struct Staff {
    db: Option<sqlx::PgPool>,
    mailer: Option<Mailer>,
    sessions: Mutex<HashMap<[u8; 32], Session>>,
    failures: Mutex<HashMap<String, Vec<u64>>>,
    /// When each address last asked for an emailed code.
    asks: Mutex<HashMap<IpAddr, Vec<u64>>>,
    /// The public API's state (ADR-0195): API tokens, rate limits, events out. It lives here
    /// because this is the one thing every route's state already shares.
    pub api: Arc<crate::api::Api>,
}

impl Staff {
    pub fn new(db: Option<sqlx::PgPool>) -> Self {
        Self {
            db,
            ..Default::default()
        }
    }

    /// The public API's state (ADR-0195).
    pub fn with_api(mut self, api: Arc<crate::api::Api>) -> Self {
        self.api = api;
        self
    }

    /// Turn the emailed code on (or leave it off, with `None`).
    pub fn with_mailer(mut self, mailer: Option<Mailer>) -> Self {
        self.mailer = mailer;
        self
    }

    /// Count one ask for a code from an address; false (and not counted) when it has asked
    /// too often lately. No address (a loopback peer) is not limited, as for failed sign-ins.
    fn may_ask(&self, addr: Option<IpAddr>, now: u64) -> bool {
        let Some(addr) = addr else {
            return true;
        };
        let mut asks = self.asks.lock().expect("code asks");
        asks.retain(|_, at| {
            at.retain(|t| now.saturating_sub(*t) < CODE_WINDOW_MS);
            !at.is_empty()
        });
        let at = asks.entry(addr).or_default();
        if at.len() >= CODES_PER_ADDRESS {
            return false;
        }
        at.push(now);
        true
    }

    /// The live session a token names, its end moved later as it is used.
    fn session(&self, token: &str, now: u64) -> Option<Account> {
        let hash = model::token_hash(token);
        let mut sessions = self.sessions.lock().expect("staff sessions");
        let s = sessions.get_mut(&hash)?;
        if s.expires_ms <= now {
            sessions.remove(&hash);
            return None;
        }
        let wanted = (now + SESSION_IDLE_MS).min(s.created_ms + SESSION_LONGEST_MS);
        if wanted >= s.expires_ms + EXTEND_EVERY_MS {
            s.expires_ms = wanted;
            if let Some(pool) = self.db.clone()
                && let Ok(rt) = tokio::runtime::Handle::try_current()
            {
                rt.spawn(async move {
                    if let Err(e) = model::extend_session(&pool, &hash, wanted).await {
                        tracing::warn!(error = %e, "cannot write a staff session's new end");
                    }
                });
            }
        }
        Some(s.account.clone())
    }

    fn insert(&self, s: Session) {
        let now = now_ms();
        let mut sessions = self.sessions.lock().expect("staff sessions");
        sessions.retain(|_, s| s.expires_ms > now);
        sessions.insert(s.token_hash, s);
    }

    fn remove(&self, hash: &[u8; 32]) {
        self.sessions.lock().expect("staff sessions").remove(hash);
    }

    /// End an account's sessions, all or all but one.
    fn end_account(&self, id: Uuid, keep: Option<&[u8; 32]>) {
        self.sessions
            .lock()
            .expect("staff sessions")
            .retain(|h, s| s.account.id != id || Some(h) == keep);
    }

    /// A changed account's name or role, in its live sessions at once.
    fn update_account(&self, a: &Account) {
        for s in self.sessions.lock().expect("staff sessions").values_mut() {
            if s.account.id == a.id {
                s.account = a.clone();
            }
        }
    }

    /// Whether this username or address has failed too often lately.
    fn locked(&self, username: &str, addr: Option<IpAddr>, now: u64) -> bool {
        let mut f = self.failures.lock().expect("sign-in failures");
        f.retain(|_, at| {
            at.retain(|t| now.saturating_sub(*t) < FAIL_WINDOW_MS);
            !at.is_empty()
        });
        let by_name = f.get(&format!("u:{username}")).map_or(0, Vec::len);
        let by_addr = addr
            .and_then(|a| f.get(&format!("a:{a}")))
            .map_or(0, Vec::len);
        by_name >= FAILS_PER_USERNAME || by_addr >= FAILS_PER_ADDRESS
    }

    fn failed(&self, username: &str, addr: Option<IpAddr>, now: u64) {
        let mut f = self.failures.lock().expect("sign-in failures");
        f.entry(format!("u:{username}")).or_default().push(now);
        if let Some(a) = addr {
            f.entry(format!("a:{a}")).or_default().push(now);
        }
    }

    fn succeeded(&self, username: &str) {
        self.failures
            .lock()
            .expect("sign-in failures")
            .remove(&format!("u:{username}"));
    }

    /// Read back the live sessions at start.
    pub async fn restore(&self, station: StationId) -> usize {
        let Some(pool) = &self.db else {
            return 0;
        };
        match model::live_sessions(pool, station).await {
            Ok(all) => {
                let n = all.len();
                for s in all {
                    self.insert(s);
                }
                n
            }
            Err(e) => {
                tracing::warn!(error = %e, "cannot read back staff sessions; staff sign in again");
                0
            }
        }
    }
}

fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}

/// Who the request comes from, if they may do what the route needs; otherwise the refusal.
#[allow(clippy::result_large_err)] // the refusal is the response itself
pub fn check(
    station_key: Option<&str>,
    staff: &Staff,
    headers: &HeaderMap,
    need: Need,
) -> Result<Actor, Response> {
    let given = bearer(headers);
    let actor = if given.starts_with(weavesignal_model::tokens::TOKEN_PREFIX) {
        // An API token passes only where the gate granted it this route (ADR-0195), and is
        // never an admin: the `Need::Admin` test below refuses it.
        Actor::Token(staff.api.authorise(given)?)
    } else if given.starts_with(model::TOKEN_PREFIX) {
        match staff.session(given, now_ms()) {
            Some(a) => Actor::Staff(a),
            None => {
                return Err(problem(
                    StatusCode::UNAUTHORIZED,
                    "Your sign-in has ended. Sign in again.",
                ));
            }
        }
    } else {
        let Some(expected) = station_key else {
            return Err(problem(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "the studio's routes need a staff sign-in, or the station key: set {}",
                    crate::config::ADMIN_TOKEN_ENV
                ),
            ));
        };
        if !same(given.as_bytes(), expected.as_bytes()) {
            return Err(problem(
                StatusCode::UNAUTHORIZED,
                "a valid bearer token is needed",
            ));
        }
        Actor::Station
    };
    if need == Need::Admin && !actor.is_admin() {
        return Err(problem(
            StatusCode::FORBIDDEN,
            if matches!(actor, Actor::Token(_)) {
                "An API token cannot do that: only an admin can."
            } else {
                "Only an admin can do that. Ask the station's admin."
            },
        ));
    }
    Ok(actor)
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/sessions", post(sign_in))
        .route("/api/v1/sessions/code", post(ask_code))
        .route("/api/v1/sessions/code/finish", post(finish_code))
        .route("/api/v1/sessions/me/email", put(own_email))
        .route("/api/v1/sessions/me", get(me).delete(sign_out))
        .route("/api/v1/sessions/me/password", post(own_password))
        .route("/api/v1/staff", get(list).post(create))
        .route("/api/v1/staff/{id}", put(change))
        .route("/api/v1/staff/{id}/password", post(set_password))
        .route("/api/v1/staff/{id}/email", put(set_email))
}

#[allow(clippy::result_large_err)]
fn db(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    state.db.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "staff accounts need the database",
        )
    })
}

fn db_problem(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "staff accounts: database error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "the database refused that",
    )
}

fn field(field: &str, detail: impl Into<String>) -> Response {
    #[derive(Serialize)]
    struct P {
        error: String,
        field: String,
    }
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(P {
            error: detail.into(),
            field: field.into(),
        }),
    )
        .into_response()
}

#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn hash(password: String) -> Result<String, Response> {
    tokio::task::spawn_blocking(move || model::hash_password(&password))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r)
        .map_err(|e| {
            tracing::error!(error = %e, "cannot hash a password");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot keep that password",
            )
        })
}

async fn verify(password: String, hash: Option<String>) -> bool {
    tokio::task::spawn_blocking(move || model::verify_password(&password, hash.as_deref()))
        .await
        .unwrap_or(false)
}

#[derive(Serialize, utoipa::ToSchema)]
struct Me {
    /// `staff`, `station` (the station key) or `token`.
    kind: &'static str,
    role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<Account>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_ms: Option<u64>,
}

fn me_of(state: &AppState, actor: &Actor, headers: &HeaderMap) -> Me {
    match actor {
        Actor::Station => Me {
            kind: "station",
            role: Role::Admin,
            account: None,
            expires_ms: None,
        },
        // No scope opens this route, so the gate never grants a token here; if one ever did,
        // it would be shown as what it is, with the lowest role.
        Actor::Token(_) => Me {
            kind: "token",
            role: Role::Screener,
            account: None,
            expires_ms: None,
        },
        Actor::Staff(a) => Me {
            kind: "staff",
            role: a.role,
            account: Some(a.clone()),
            expires_ms: state
                .staff
                .sessions
                .lock()
                .expect("staff sessions")
                .get(&model::token_hash(bearer(headers)))
                .map(|s| s.expires_ms),
        },
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct SignIn {
    username: String,
    password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
struct SignedIn {
    token: String,
    expires_ms: u64,
    account: Account,
}

#[utoipa::path(
    post,
    path = "/api/v1/sessions",
    operation_id = "staff_sign_in",
    tag = "sessions",
    summary = "Sign in with a password",
    description = "`username` and `password` for a session token (starts `rss_`). A refused sign-in answers 401 with the same sentence whether the username exists or not, after the same work.",
    request_body = SignIn,
    responses(
        (status = 201, description = "The session.", body = SignedIn),
        (status = 401, description = "That username and password do not match an account here.", body = crate::openapi::Problem),
        (status = 429, description = "Too many tries: five failures for a username within 15 minutes, or twenty for an address. Wait for the window to pass.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn sign_in(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<SignIn>,
) -> Response {
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let refused = || {
        problem(
            StatusCode::UNAUTHORIZED,
            "That username and password do not match an account here.",
        )
    };
    let Some(username) = model::clean_username(&req.username) else {
        return refused();
    };
    if req.password.chars().count() > model::PASSWORD_MAX {
        return refused();
    }
    let addr = crate::public::peer(
        state.join.forwarded_for,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
    );
    let now = now_ms();
    if state.staff.locked(&username, addr, now) {
        tracing::warn!(%username, "staff sign-in: too many failures, refused for now");
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many tries. Wait fifteen minutes, or ask an admin to set a new password.",
        );
    }
    let found = match model::account_for_sign_in(pool, state.station.id, &username).await {
        Ok(f) => f,
        Err(e) => return db_problem(e),
    };
    let (account, stored) = match found {
        Some((a, h)) if a.disabled_ms.is_none() => (Some(a), h),
        _ => (None, None),
    };
    let ok = verify(req.password, stored).await && account.is_some();
    let Some(account) = account.filter(|_| ok) else {
        state.staff.failed(&username, addr, now);
        tracing::info!(%username, "staff sign-in refused");
        return refused();
    };
    state.staff.succeeded(&username);
    start_session(&state, pool, account, now, "password").await
}

/// A session for an account that has proved who it is, answered as a sign-in is.
async fn start_session(
    state: &AppState,
    pool: &sqlx::PgPool,
    account: Account,
    now: u64,
    by: &'static str,
) -> Response {
    let token = model::new_token();
    let s = Session {
        token_hash: model::token_hash(&token),
        account: account.clone(),
        created_ms: now,
        expires_ms: now + SESSION_IDLE_MS,
    };
    if let Err(e) = model::create_session(
        pool,
        state.station.id,
        account.id,
        &s.token_hash,
        s.expires_ms,
    )
    .await
    {
        return db_problem(e);
    }
    let expires_ms = s.expires_ms;
    state.staff.insert(s);
    tracing::info!(username = %account.username, role = %account.role, by, "staff signed in");
    (
        StatusCode::CREATED,
        Json(SignedIn {
            token,
            expires_ms,
            account,
        }),
    )
        .into_response()
}

// ---- the emailed code (ADR-0127) ----

/// A six-digit code from the operating system's randomness, every code equally likely.
fn new_code() -> String {
    loop {
        let b = Uuid::new_v4();
        let n = u32::from_le_bytes(b.as_bytes()[..4].try_into().expect("four bytes"));
        // 4,294,000,000 is the largest multiple of a million that fits in 32 bits.
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// An address that could be one: a local part, an `@` and a dotted domain, no spaces or
/// control characters, at most 254 characters. It is not checked further; the code that comes
/// back is the check.
fn valid_email(e: &str) -> bool {
    let e = e.trim();
    let Some((local, domain)) = e.rsplit_once('@') else {
        return false;
    };
    e.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !e
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == ',' || c == '<' || c == '>')
}

const CODE_OFF: &str = "This station does not email sign-in codes. Sign in with your password.";
const CODE_REFUSED: &str = "That code does not match, or has run out. Ask for a new one.";

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct AskCode {
    username: String,
    email: String,
}

/// Ask for a code. Whatever the username and address are, a station with mail answers the
/// same words in the same time, so the answer tells a stranger nothing about who has an
/// account or which address it holds. What depends on them waits until after the answer.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/code",
    operation_id = "staff_ask_code",
    tag = "sessions",
    summary = "Ask for an emailed sign-in code",
    description = "A station with mail answers 202 for every username and address alike, and sends a code only to an address that belongs to that username's account when it is on, so neither the words nor the time tell a stranger who has an account. 503 when the station has no mail; 422 (`field` `email`) for something that is not an address; 429 when one address has asked ten times in ten minutes.",
    request_body = AskCode,
    responses(
        (status = 202, description = "Asked.", body = CodeAsked),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 429, description = "Too many tries: five failures for a username within 15 minutes, or twenty for an address. Wait for the window to pass.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn ask_code(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<AskCode>,
) -> Response {
    if state.staff.mailer.is_none() {
        return problem(StatusCode::SERVICE_UNAVAILABLE, CODE_OFF);
    }
    if let Err(r) = db(&state) {
        return r;
    }
    let email = req.email.trim().to_lowercase();
    if !valid_email(&email) {
        return field("email", "Enter your email address, like name@example.com.");
    }
    let addr = crate::public::peer(
        state.join.forwarded_for,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
    );
    if !state.staff.may_ask(addr, now_ms()) {
        tracing::warn!("staff sign-in code: too many asked for from one address, refused for now");
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many codes asked for from here. Try again in ten minutes, or sign in with your password.",
        );
    }
    let username = model::clean_username(&req.username);
    tokio::spawn(async move {
        if let Some(username) = username {
            send_code(&state, &username, &email).await;
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "asked": true, "expires_s": model::CODE_MINUTES * 60 })),
    )
        .into_response()
}

/// The part of asking that depends on who is asking: find the account that holds this address,
/// and if it is this username's and is on, keep a code for it and mail it. Nothing here is
/// logged with an address or a code in it.
async fn send_code(state: &AppState, username: &str, email: &str) {
    let (Some(mailer), Some(pool)) = (&state.staff.mailer, &state.db) else {
        return;
    };
    let station = state.station.id;
    let account =
        match model::account_for_email_hash(pool, station, &mailer.key.email_hash(email)).await {
            Ok(Some(a)) if a.username == username => a,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!(error = %e, "staff sign-in code: cannot look an account up");
                return;
            }
        };
    let code = new_code();
    let hash = mailer.key.code_hash(account.id, email, &code);
    match model::issue_code(pool, station, account.id, &hash, CODES_PER_ACCOUNT).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!(username = %account.username, "staff sign-in code: asked for too often (or the account is off); none sent");
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, "staff sign-in code: cannot keep a code");
            return;
        }
    }
    let body = format!(
        "Your code to sign in to {station}'s studio is {code}\n\nIt works for {minutes} minutes, once. If you did not ask for it, ignore this email: nothing happens without the code.\n",
        station = mailer.station,
        minutes = model::CODE_MINUTES,
    );
    match mail::send(
        &mailer.config,
        email,
        &format!("{code} is your {} sign-in code", mailer.station),
        &body,
    )
    .await
    {
        Ok(()) => tracing::info!(username = %account.username, "staff sign-in code sent"),
        // The error holds no address (the mailer's own words are scrubbed) and no code.
        Err(e) => tracing::warn!(error = %e, "staff sign-in code could not be sent"),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct FinishCode {
    username: String,
    email: String,
    code: String,
}

/// Sign in with a code. One sentence for every way it can fail, so it says nothing of which
/// usernames or addresses exist; the failures count towards the same lockout as a password's,
/// kept apart from the password's own count.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/code/finish",
    operation_id = "staff_finish_code",
    tag = "sessions",
    summary = "Sign in with an emailed code",
    description = "`username`, `email` and the six-digit `code`. Any failure (unknown username, an account that is off, a wrong, used, expired or spent code) answers 401 with one sentence.",
    request_body = FinishCode,
    responses(
        (status = 201, description = "The session.", body = SignedIn),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 401, description = "That code does not match, or has run out. Ask for a new one.", body = crate::openapi::Problem),
        (status = 429, description = "Too many tries: five failures for a username within 15 minutes, or twenty for an address. Wait for the window to pass.", body = crate::openapi::Problem),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn finish_code(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<FinishCode>,
) -> Response {
    let Some(mailer) = &state.staff.mailer else {
        return problem(StatusCode::SERVICE_UNAVAILABLE, CODE_OFF);
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let code = req.code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return field("code", "The code is six digits.");
    }
    let email = req.email.trim().to_lowercase();
    if !valid_email(&email) {
        return field("email", "Enter your email address, like name@example.com.");
    }
    let refused = || problem(StatusCode::UNAUTHORIZED, CODE_REFUSED);
    let Some(username) = model::clean_username(&req.username) else {
        return refused();
    };
    // Usernames cannot hold a colon, so this count never meets a password's.
    let counted = format!("code:{username}");
    let addr = crate::public::peer(
        state.join.forwarded_for,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
    );
    let now = now_ms();
    if state.staff.locked(&counted, addr, now) {
        tracing::warn!(%username, "staff sign-in by code: too many failures, refused for now");
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many tries. Wait fifteen minutes, or sign in with your password.",
        );
    }
    let account = match model::account_for_sign_in(pool, state.station.id, &username).await {
        Ok(f) => f.map(|(a, _)| a).filter(|a| a.disabled_ms.is_none()),
        Err(e) => return db_problem(e),
    };
    let verdict = match &account {
        Some(a) => {
            model::check_code(pool, state.station.id, a.id, |want| {
                mailer.key.code_matches(a.id, &email, code, want)
            })
            .await
        }
        None => {
            // The same database round trip as for an account with no live code (no account has
            // the nil id, so nothing is found), so the time does not tell them apart.
            model::check_code(pool, state.station.id, Uuid::nil(), |_| false).await
        }
    };
    let verdict = match verdict {
        Ok(v) => v,
        Err(e) => return db_problem(e),
    };
    let Some(account) = account.filter(|_| verdict == model::CodeCheck::Good) else {
        state.staff.failed(&counted, addr, now);
        tracing::info!(%username, "staff sign-in by code refused");
        return refused();
    };
    state.staff.succeeded(&counted);
    start_session(&state, pool, account, now, "emailed code").await
}

/// The address a person's codes go to, set or cleared (`None`): kept as a keyed hash, and any
/// code asked for before is spent. The new account is returned, and its live sessions told.
async fn apply_email(
    state: &AppState,
    pool: &sqlx::PgPool,
    by: &str,
    account: &Account,
    email: Option<String>,
) -> Response {
    let hash = match &email {
        None => None,
        Some(e) => {
            let Some(mailer) = &state.staff.mailer else {
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "This station does not email sign-in codes yet: turn on mail in its settings first.",
                );
            };
            if !valid_email(e) {
                return field("email", "Enter an email address, like name@example.com.");
            }
            Some(mailer.key.email_hash(e))
        }
    };
    match model::set_email_hash(pool, state.station.id, account.id, hash.as_deref()).await {
        Ok(true) => {}
        Ok(false) => return problem(StatusCode::NOT_FOUND, "no such account"),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return field(
                "email",
                "That address already belongs to another account here.",
            );
        }
        Err(e) => return db_problem(e),
    }
    match model::get_account(pool, state.station.id, account.id).await {
        Ok(Some(a)) => {
            state.staff.update_account(&a);
            tracing::info!(by, username = %a.username, set = a.has_email, "staff sign-in address changed");
            Json(a).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => db_problem(e),
    }
}

/// An address from a request: a string sets it, `null` clears it, and it must be given.
#[allow(clippy::result_large_err)] // the refusal is the response itself
fn email_of(v: &serde_json::Value) -> Result<Option<String>, Response> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(s.trim().to_lowercase())),
        _ => Err(field(
            "email",
            "Give an email address, or null to take the address away.",
        )),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct SetEmail {
    email: serde_json::Value,
}

#[utoipa::path(
    put,
    path = "/api/v1/staff/{id}/email",
    operation_id = "staff_set_email",
    tag = "staff",
    summary = "Set someone's sign-in address",
    description = "Admin only. `email` (an address, or `null` to take it away). Setting needs the station's mail to be on (503 otherwise); one address belongs to one account (422, `field` `email`).",
    params(("id" = Uuid, Path, description = "The account")),
    request_body = SetEmail,
    responses(
        (status = 200, description = "The account.", body = weavesignal_model::staff::Account),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn set_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<SetEmail>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let email = match email_of(&req.email) {
        Ok(e) => e,
        Err(r) => return r,
    };
    let account = match model::get_account(pool, state.station.id, id).await {
        Ok(Some(a)) => a,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => return db_problem(e),
    };
    apply_email(&state, pool, &actor.label(), &account, email).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct OwnEmail {
    current: String,
    email: serde_json::Value,
}

/// A person's own address needs their password: a stolen session must not be able to make
/// itself a way back in that outlives the session.
#[utoipa::path(
    put,
    path = "/api/v1/sessions/me/email",
    operation_id = "staff_own_email",
    tag = "sessions",
    summary = "Set your own sign-in address",
    description = "`current` (your password) and `email` (an address, or `null` to take it away): the address your codes go to. The station key has no account: 422.",
    request_body = OwnEmail,
    responses(
        (status = 200, description = "The account.", body = weavesignal_model::staff::Account),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn own_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<OwnEmail>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Actor::Staff(a) = actor else {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the station key has no sign-in address: it is a key, not a person",
        );
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let email = match email_of(&req.email) {
        Ok(e) => e,
        Err(r) => return r,
    };
    let stored = match model::password_hash_of(pool, state.station.id, a.id).await {
        Ok(h) => h,
        Err(e) => return db_problem(e),
    };
    if !verify(req.current, stored).await {
        return field("current", "That is not your current password.");
    }
    apply_email(&state, pool, &a.username, &a, email).await
}

#[utoipa::path(
    get,
    path = "/api/v1/sessions/me",
    operation_id = "staff_me",
    tag = "sessions",
    summary = "Who the bearer is",
    description = "`kind` (`staff` or `station`), `role`, and for staff the `account` and when the session ends.",
    responses(
        (status = 200, description = "The bearer.", body = Me),
        (status = 401, response = crate::openapi::Unauthorized),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    match state.check(&headers, Need::Staff) {
        Ok(actor) => Json(me_of(&state, &actor, &headers)).into_response(),
        Err(r) => r,
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/sessions/me",
    operation_id = "staff_sign_out",
    tag = "sessions",
    summary = "Sign out",
    description = "Ends this session. The station key has no session: 422.",
    responses(
        (status = 204, description = "Signed out."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 422, response = crate::openapi::Unprocessable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn sign_out(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Actor::Staff(a) = actor else {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the station key is not a sign-in: there is nothing to sign out of",
        );
    };
    let hash = model::token_hash(bearer(&headers));
    state.staff.remove(&hash);
    if let Some(pool) = &state.db
        && let Err(e) = model::end_session(pool, &hash).await
    {
        tracing::warn!(error = %e, "cannot delete a signed-out staff session; it is ended until the control plane restarts");
    }
    tracing::info!(username = %a.username, "staff signed out");
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct OwnPassword {
    current: String,
    new: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/sessions/me/password",
    operation_id = "staff_own_password",
    tag = "sessions",
    summary = "Change your own password",
    description = "`current` and `new`; ends the account's other sessions.",
    request_body = OwnPassword,
    responses(
        (status = 204, description = "Changed."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
)]
async fn own_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<OwnPassword>,
) -> Response {
    let actor = match state.check(&headers, Need::Staff) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Actor::Staff(a) = actor else {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the station key has no password to change: it is set where the station is deployed",
        );
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let stored = match model::password_hash_of(pool, state.station.id, a.id).await {
        Ok(h) => h,
        Err(e) => return db_problem(e),
    };
    if !verify(req.current, stored).await {
        return field("current", "That is not your current password.");
    }
    if let Some(why) = model::password_problem(&req.new, &a.username) {
        return field("new", why);
    }
    let h = match hash(req.new).await {
        Ok(h) => h,
        Err(r) => return r,
    };
    let keep = model::token_hash(bearer(&headers));
    match model::set_password(pool, state.station.id, a.id, &h, Some(&keep)).await {
        Ok(true) => {}
        Ok(false) => return problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => return db_problem(e),
    }
    state.staff.end_account(a.id, Some(&keep));
    tracing::info!(username = %a.username, "staff changed their own password; their other sessions ended");
    StatusCode::NO_CONTENT.into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/staff",
    operation_id = "staff_list",
    tag = "staff",
    summary = "Staff accounts",
    description = "Admin only. Every account, on and off, by username.",
    responses(
        (status = 200, description = "The accounts.", body = AccountsBody),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = state.check(&headers, Need::Admin) {
        return r;
    }
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match model::list_accounts(pool, state.station.id).await {
        Ok(all) => Json(serde_json::json!({ "accounts": all })).into_response(),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct NewAccount {
    username: String,
    name: String,
    role: Role,
    password: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/staff",
    operation_id = "staff_create",
    tag = "staff",
    summary = "Make an account",
    description = "Admin only. `username`, `name`, `role` and `password`. The password is kept only as an Argon2id hash.",
    request_body = NewAccount,
    responses(
        (status = 201, description = "The account.", body = weavesignal_model::staff::Account),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewAccount>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(username) = model::clean_username(&req.username) else {
        return field(
            "username",
            "A username is up to 40 letters, digits, dots, dashes or underscores, starting with a letter or digit.",
        );
    };
    let Some(name) = model::clean_name(&req.name) else {
        return field("name", "A name is 1 to 80 characters.");
    };
    if let Some(why) = model::password_problem(&req.password, &username) {
        return field("password", why);
    }
    let h = match hash(req.password).await {
        Ok(h) => h,
        Err(r) => return r,
    };
    match model::create_account(pool, state.station.id, &username, &name, req.role, &h).await {
        Ok(a) => {
            tracing::info!(by = %actor.label(), username = %a.username, role = %a.role, "staff account made");
            (StatusCode::CREATED, Json(a)).into_response()
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            field("username", "That username is taken.")
        }
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct ChangeReq {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    disabled: Option<bool>,
}

#[utoipa::path(
    put,
    path = "/api/v1/staff/{id}",
    operation_id = "staff_change",
    tag = "staff",
    summary = "Change an account",
    description = "Admin only. Any of `name`, `role`, `disabled`; turning one off ends its sessions at once.",
    params(("id" = Uuid, Path, description = "The account")),
    request_body = ChangeReq,
    responses(
        (status = 200, description = "The account.", body = weavesignal_model::staff::Account),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn change(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ChangeReq>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    // An admin cannot lock themselves out by a slip: someone else turns them off or down.
    if let Actor::Staff(me) = &actor
        && me.id == id
        && (req.disabled == Some(true) || req.role.is_some_and(|r| !r.is_admin()))
    {
        return problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "You cannot turn off your own account or take away your own admin role. Another admin can.",
        );
    }
    let name = match req.name.as_deref().map(model::clean_name) {
        None => None,
        Some(Some(n)) => Some(n),
        Some(None) => return field("name", "A name is 1 to 80 characters."),
    };
    let c = model::Change {
        name,
        role: req.role,
        disabled: req.disabled,
    };
    match model::change_account(pool, state.station.id, id, &c).await {
        Ok(Some(a)) => {
            if a.disabled_ms.is_some() {
                state.staff.end_account(a.id, None);
            } else {
                state.staff.update_account(&a);
            }
            tracing::info!(by = %actor.label(), username = %a.username, role = %a.role, off = a.disabled_ms.is_some(), "staff account changed");
            Json(a).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => db_problem(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
struct SetPassword {
    password: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/staff/{id}/password",
    operation_id = "staff_set_password",
    tag = "staff",
    summary = "Set someone's password",
    description = "Admin only. Ends every session the account has.",
    params(("id" = Uuid, Path, description = "The account")),
    request_body = SetPassword,
    responses(
        (status = 204, description = "Set."),
        (status = 401, response = crate::openapi::Unauthorized),
        (status = 403, response = crate::openapi::Forbidden),
        (status = 404, response = crate::openapi::NotFound),
        (status = 422, response = crate::openapi::Unprocessable),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(("staff_session" = []), ("station_key" = [])),
    extensions(("x-admin" = json!(true))),
)]
async fn set_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<SetPassword>,
) -> Response {
    let actor = match state.check(&headers, Need::Admin) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pool = match db(&state) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let account = match model::get_account(pool, state.station.id, id).await {
        Ok(Some(a)) => a,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => return db_problem(e),
    };
    if let Some(why) = model::password_problem(&req.password, &account.username) {
        return field("password", why);
    }
    let h = match hash(req.password).await {
        Ok(h) => h,
        Err(r) => return r,
    };
    match model::set_password(pool, state.station.id, id, &h, None).await {
        Ok(true) => {}
        Ok(false) => return problem(StatusCode::NOT_FOUND, "no such account"),
        Err(e) => return db_problem(e),
    }
    state.staff.end_account(id, None);
    state.staff.succeeded(&account.username);
    tracing::info!(by = %actor.label(), username = %account.username, "staff password set by an admin; their sessions ended");
    StatusCode::NO_CONTENT.into_response()
}

// ---- the description (ADR-0195) ----

#[derive(Serialize, utoipa::ToSchema)]
struct CodeAsked {
    asked: bool,
    /// Seconds the code is good for.
    expires_s: u64,
}

#[derive(Serialize, utoipa::ToSchema)]
struct AccountsBody {
    accounts: Vec<Account>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    sign_in,
    ask_code,
    finish_code,
    own_email,
    me,
    sign_out,
    own_password,
    list,
    create,
    change,
    set_password,
    set_email
))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    fn account(role: Role) -> Account {
        Account {
            id: Uuid::new_v4(),
            username: "morag".into(),
            name: "Morag".into(),
            role,
            disabled_ms: None,
            created_ms: 0,
            has_email: false,
        }
    }

    fn headers(bearer: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            format!("Bearer {bearer}").parse().unwrap(),
        );
        h
    }

    fn signed_in(staff: &Staff, role: Role, expires_ms: u64) -> (String, Account) {
        let token = model::new_token();
        let a = account(role);
        staff.insert(Session {
            token_hash: model::token_hash(&token),
            account: a.clone(),
            created_ms: now_ms(),
            expires_ms,
        });
        (token, a)
    }

    #[test]
    fn the_station_key_is_an_admin() {
        let staff = Staff::default();
        let a = check(Some("key"), &staff, &headers("key"), Need::Admin).unwrap();
        assert_eq!(a, Actor::Station);
        let r = check(Some("key"), &staff, &headers("nope"), Need::Staff).unwrap_err();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = check(None, &staff, &headers("key"), Need::Staff).unwrap_err();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn a_session_runs_the_studio_and_only_an_admin_does_admin_things() {
        let staff = Staff::default();
        let (t, a) = signed_in(&staff, Role::Presenter, now_ms() + 60_000);
        assert_eq!(
            check(Some("key"), &staff, &headers(&t), Need::Staff).unwrap(),
            Actor::Staff(a.clone())
        );
        let r = check(Some("key"), &staff, &headers(&t), Need::Admin).unwrap_err();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        // Without a station key, sessions still work.
        assert!(check(None, &staff, &headers(&t), Need::Staff).is_ok());
        let (admin, _) = signed_in(&staff, Role::Admin, now_ms() + 60_000);
        assert!(check(None, &staff, &headers(&admin), Need::Admin).is_ok());
    }

    #[test]
    fn an_ended_or_unknown_session_is_refused() {
        let staff = Staff::default();
        let (t, _) = signed_in(&staff, Role::Admin, now_ms().saturating_sub(1));
        let r = check(Some("key"), &staff, &headers(&t), Need::Staff).unwrap_err();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = check(
            Some("key"),
            &staff,
            &headers(&model::new_token()),
            Need::Staff,
        )
        .unwrap_err();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn turning_an_account_off_ends_its_sessions_and_a_role_change_lands_at_once() {
        let staff = Staff::default();
        let (t1, a) = signed_in(&staff, Role::Admin, now_ms() + 60_000);
        let t2 = model::new_token();
        staff.insert(Session {
            token_hash: model::token_hash(&t2),
            account: a.clone(),
            created_ms: now_ms(),
            expires_ms: now_ms() + 60_000,
        });
        let demoted = Account {
            role: Role::Screener,
            ..a.clone()
        };
        staff.update_account(&demoted);
        assert_eq!(
            check(None, &staff, &headers(&t1), Need::Admin)
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );
        // Changing one's own password keeps the session that did it.
        staff.end_account(a.id, Some(&model::token_hash(&t2)));
        assert!(check(None, &staff, &headers(&t1), Need::Staff).is_err());
        assert!(check(None, &staff, &headers(&t2), Need::Staff).is_ok());
        staff.end_account(a.id, None);
        assert!(check(None, &staff, &headers(&t2), Need::Staff).is_err());
    }

    #[test]
    fn a_session_in_use_lives_on_but_never_past_its_longest() {
        let staff = Staff::default();
        let now = now_ms();
        let token = model::new_token();
        let hash = model::token_hash(&token);
        staff.insert(Session {
            token_hash: hash,
            account: account(Role::Producer),
            created_ms: now - SESSION_LONGEST_MS + 3_600_000,
            expires_ms: now + 60_000,
        });
        assert!(staff.session(&token, now).is_some());
        let end = staff.sessions.lock().unwrap()[&hash].expires_ms;
        // Moved later, but only to the longest a session lives from its sign-in.
        assert_eq!(end, now + 3_600_000);
    }

    #[test]
    fn sign_in_failures_lock_a_username_then_an_address() {
        let staff = Staff::default();
        let addr: IpAddr = "192.0.2.7".parse().unwrap();
        let now = now_ms();
        for _ in 0..FAILS_PER_USERNAME {
            assert!(!staff.locked("fraser", Some(addr), now));
            staff.failed("fraser", Some(addr), now);
        }
        assert!(staff.locked("fraser", Some(addr), now));
        assert!(!staff.locked("fraser", Some(addr), now + FAIL_WINDOW_MS));
        // The address, across usernames.
        for i in 0..FAILS_PER_ADDRESS {
            staff.failed(&format!("u{i}"), Some(addr), now);
        }
        assert!(staff.locked("ailsa", Some(addr), now));
        assert!(!staff.locked("ailsa", None, now));
    }

    #[test]
    fn codes_are_six_digits_and_differ() {
        let codes: Vec<String> = (0..200).map(|_| new_code()).collect();
        assert!(
            codes
                .iter()
                .all(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_digit()))
        );
        assert!(codes.iter().any(|c| c != &codes[0]));
    }

    #[test]
    fn an_address_is_checked_for_its_shape_only() {
        assert!(valid_email("morag.campbell@mail.example"));
        assert!(valid_email("  Fraser@Mail.Example "));
        for bad in [
            "",
            "morag",
            "morag@",
            "@mail.example",
            "morag@mail",
            "morag@.example",
            "morag@mail.",
            "mo rag@mail.example",
            "morag@mail.example\r\nBcc: x@y.example",
            "a@b.example, c@d.example",
            "<a@b.example>",
        ] {
            assert!(!valid_email(bad), "{bad:?}");
        }
        assert!(!valid_email(&format!("{}@mail.example", "a".repeat(250))));
    }

    #[test]
    fn an_address_may_ask_for_a_code_ten_times_in_ten_minutes() {
        let staff = Staff::default();
        let a: IpAddr = "192.0.2.9".parse().unwrap();
        let b: IpAddr = "192.0.2.10".parse().unwrap();
        let now = now_ms();
        for _ in 0..CODES_PER_ADDRESS {
            assert!(staff.may_ask(Some(a), now));
        }
        assert!(!staff.may_ask(Some(a), now));
        assert!(staff.may_ask(Some(b), now), "another address is not held");
        assert!(
            staff.may_ask(Some(a), now + CODE_WINDOW_MS),
            "the window passes"
        );
        // A loopback peer is no address, and is not counted.
        for _ in 0..50 {
            assert!(staff.may_ask(None, now));
        }
    }

    #[test]
    fn code_failures_are_counted_apart_from_a_passwords() {
        let staff = Staff::default();
        let now = now_ms();
        for _ in 0..FAILS_PER_USERNAME {
            staff.failed("fraser", None, now);
        }
        assert!(staff.locked("fraser", None, now));
        assert!(!staff.locked("code:fraser", None, now));
        assert!(
            model::clean_username("code:fraser").is_none(),
            "no username holds a colon"
        );
    }

    #[test]
    fn an_address_is_given_or_cleared_never_left_out() {
        use serde_json::json;
        assert_eq!(email_of(&json!(null)).unwrap(), None);
        assert_eq!(
            email_of(&json!(" Morag@Mail.Example ")).unwrap(),
            Some("morag@mail.example".into())
        );
        assert!(email_of(&json!(7)).is_err());
        assert!(email_of(&json!(["a@b.example"])).is_err());
    }

    #[test]
    fn mail_on_needs_the_key() {
        let mut c = crate::config::Config::from_toml(
            "[station]\nid = \"6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10\"\nname = \"Kettle FM\"\n",
            "t",
        )
        .unwrap();
        assert!(Mailer::from_config(&c, "Kettle FM").unwrap().is_none());
        c.mail.mode = MailMode::Outbox;
        let why = Mailer::from_config(&c, "Kettle FM").unwrap_err();
        assert!(why.contains("WEAVESIGNAL_CONTROL_EMAIL_KEY"), "{why}");
        c.email_key = Some(MailKey::parse(&"k".repeat(32)).unwrap());
        assert!(Mailer::from_config(&c, "Kettle FM").unwrap().is_some());
    }
}
