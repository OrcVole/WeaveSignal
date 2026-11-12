// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat's HTTP routes, all under `/api/v1/chat/` (the list, with shapes, is in
//! `packages/ui/chat.ts`). Public: the station's chat facts, sign-in, guests, notices, the
//! socket. A chatter's own (their session as a bearer): themselves, reports, reviews. The
//! studio's (the station's admin bearer, or a moderator's session): moderation, the log, a
//! chatter's history. The station admin's (the admin bearer): settings, export, erasure,
//! evidence holds, the risk-assessment template.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{MailMode, StationSettings};
use crate::legal::{self, Reason};
use crate::limits::AddressKey;
use crate::mail;
use crate::rules::{self, Kind, Standing};
use crate::service::{Actor, Chat, ModCommand, NoticeRequest, Problem, me_of};
use crate::store::{self, Account, CodeCheck, NewAccount};
use crate::wire::{Me, Statement};

pub struct AppState {
    pub chat: Arc<Chat>,
    pub admin_token: Option<String>,
    pub sockets: AtomicUsize,
    /// Sign-in codes asked for per address in the current ten minutes.
    pub codes: std::sync::Mutex<std::collections::HashMap<IpAddr, (i64, u32)>>,
    /// Per-address limits, in memory only (ADR-0120).
    pub limits: Limiters,
}

/// Ten minutes, the window of every per-address count.
const TEN_MINUTES_MS: i64 = 600_000;

/// The chat's per-address limits (ADR-0120; review 0025, C1): kept in memory for minutes and
/// never written, so no address outlives its window.
#[derive(Debug)]
pub struct Limiters {
    pub sockets: crate::limits::Open,
    pub notices: crate::limits::Window<AddressKey>,
    pub guests: crate::limits::Window<AddressKey>,
    pub signin_tries: crate::limits::Window<AddressKey>,
}

impl Limiters {
    pub fn new(l: &crate::config::Limits) -> Self {
        use crate::limits::{Open, Window};
        Limiters {
            sockets: Open::new(l.sockets_per_address),
            notices: Window::new(l.notices_per_address, TEN_MINUTES_MS),
            guests: Window::new(l.guests_per_address, TEN_MINUTES_MS),
            signin_tries: Window::new(l.signin_tries_per_address, TEN_MINUTES_MS),
        }
    }

    /// Forget every window that has passed.
    pub fn forget_old(&self, now_ms: i64) {
        self.notices.forget_old(now_ms);
        self.guests.forget_old(now_ms);
        self.signin_tries.forget_old(now_ms);
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let upload = state.chat.config.avatars.max_upload_bytes();
    let attachment = state.chat.config.attachments.max_upload_bytes();
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/chat/info", get(info))
        .route("/api/v1/chat/socket", get(socket))
        .route("/api/v1/chat/signin/start", post(signin_start))
        .route("/api/v1/chat/signin/finish", post(signin_finish))
        .route("/api/v1/chat/guest", post(guest))
        .route("/api/v1/chat/notices", post(notice))
        .route("/api/v1/chat/me", get(me))
        .route("/api/v1/chat/signout", post(signout))
        .route(
            "/api/v1/chat/me/avatar",
            get(my_avatar)
                .put(put_avatar)
                .delete(delete_avatar)
                .layer(axum::extract::DefaultBodyLimit::max(upload)),
        )
        .route(
            "/api/v1/chat/me/avatar/stock",
            axum::routing::put(put_stock),
        )
        .route("/api/v1/chat/avatars/{id}", get(avatar))
        // Pictures and documents posted in the chat (ADR-0262).
        .route(
            "/api/v1/chat/attachments",
            post(post_attachment).layer(axum::extract::DefaultBodyLimit::max(attachment)),
        )
        .route("/api/v1/chat/pictures/{id}", get(picture))
        .route("/api/v1/chat/pictures/{id}/thumb", get(picture_thumb))
        .route("/api/v1/chat/files/{id}", get(document))
        .route("/api/v1/chat/mod/avatars/{id}", get(mod_avatar))
        .route("/api/v1/chat/mod/kept-pictures", get(kept_pictures))
        .route("/api/v1/chat/evidence", get(evidence_list))
        .route("/api/v1/chat/evidence/{id}/picture", get(evidence_picture))
        .route("/api/v1/chat/evidence/{id}/export", get(evidence_export))
        .route("/api/v1/chat/reports", post(report))
        .route("/api/v1/chat/statements/{id}/review", post(review))
        .route("/api/v1/chat/staff/session", post(staff_session))
        .route("/api/v1/chat/modes", post(modes))
        .route("/api/v1/chat/mod", post(moderate))
        .route("/api/v1/chat/mod/log", get(mod_log))
        .route("/api/v1/chat/mod/accounts/{id}", get(account_detail))
        .route(
            "/api/v1/chat/admin/settings",
            get(get_settings).put(put_settings),
        )
        .route("/api/v1/chat/admin/accounts/{id}/export", get(export))
        .route("/api/v1/chat/admin/accounts/{id}/erase", post(erase))
        .route("/api/v1/chat/admin/holds", get(holds))
        .route("/api/v1/chat/admin/risk-assessment", get(risk_assessment))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct ProblemBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    needs: Option<Vec<&'static str>>,
}

fn problem(code: StatusCode, error: impl Into<String>) -> Response {
    (
        code,
        Json(ProblemBody {
            error: error.into(),
            field: None,
            needs: None,
        }),
    )
        .into_response()
}

fn field_problem(code: StatusCode, field: &'static str, error: impl Into<String>) -> Response {
    (
        code,
        Json(ProblemBody {
            error: error.into(),
            field: Some(field),
            needs: None,
        }),
    )
        .into_response()
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let code = match &self {
            Problem::NotFound(_) => StatusCode::NOT_FOUND,
            Problem::Conflict(_) => StatusCode::CONFLICT,
            Problem::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Problem::Forbidden(_) => StatusCode::FORBIDDEN,
            Problem::TooMany(_) => StatusCode::TOO_MANY_REQUESTS,
            Problem::Failed(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Problem::Db(e) => {
                tracing::error!(error = %e, "database");
                StatusCode::SERVICE_UNAVAILABLE
            }
        };
        problem(code, self.to_string())
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// The studio sends a name beyond Latin-1 percent-encoded (a header cannot carry it).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_admin(state: &AppState, headers: &HeaderMap) -> bool {
    match (&state.admin_token, bearer(headers)) {
        (Some(want), Some(given)) => same(want.as_bytes(), given.as_bytes()),
        _ => false,
    }
}

/// The signed-in chatter, from their session bearer.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn chatter(state: &AppState, headers: &HeaderMap) -> Result<Account, Response> {
    let Some(token) = bearer(headers) else {
        return Err(problem(StatusCode::UNAUTHORIZED, "Sign in first."));
    };
    match store::session_account(&state.chat.db, state.chat.station.id, token).await {
        Ok(Some(a)) => Ok(a),
        Ok(None) => Err(problem(
            StatusCode::UNAUTHORIZED,
            "Your sign-in has ended. Sign in again.",
        )),
        Err(e) => Err(Problem::Db(e).into_response()),
    }
}

/// The studio (the admin bearer) or a moderator (their session).
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn moderator(state: &AppState, headers: &HeaderMap) -> Result<Actor, Response> {
    if is_admin(state, headers) {
        let name = headers
            .get("x-weavesignal-actor")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                percent_decode(s.trim())
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "The studio".into());
        return Ok(Actor {
            name,
            account: None,
            staff: true,
            admin: true,
            staff_account: None,
        });
    }
    if let Some(member) = staff_member(state, headers).await? {
        // A member of staff moderates under their own account (`chat-staff-identity` b): the
        // log names who acted, and an account turned off is refused on its next action.
        let a = store::staff_account_for(
            &state.chat.db,
            state.chat.station.id,
            member.account.id,
            &member.account.name,
        )
        .await
        .map_err(|e| Problem::Db(e).into_response())?;
        return Ok(Actor {
            name: a.name.clone(),
            account: Some(a.id),
            staff: true,
            admin: member.account.role.is_admin(),
            staff_account: Some(member.account.id),
        });
    }
    let a = chatter(state, headers).await?;
    if !a.standing.moderates() {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Only moderators can do that.",
        ));
    }
    let restricted = store::restriction(&state.chat.db, state.chat.station.id, a.id)
        .await
        .map_err(|e| Problem::Db(e).into_response())?;
    if restricted.is_some() {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "A restricted account cannot moderate.",
        ));
    }
    Ok(Actor {
        name: a.name.clone(),
        account: Some(a.id),
        staff: a.standing == Standing::Staff,
        admin: false,
        staff_account: None,
    })
}

/// A member of staff's live session, when the bearer is one (ADR-0127): looked up in the
/// control plane's table on every call, so a sign-out, a new password or an account turned off
/// ends the chat's powers at once. `Ok(None)` when the bearer is not a staff session.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn staff_member(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<weavesignal_model::staff::Session>, Response> {
    let Some(token) =
        bearer(headers).filter(|t| t.starts_with(weavesignal_model::staff::TOKEN_PREFIX))
    else {
        return Ok(None);
    };
    match weavesignal_model::staff::session_for_token(&state.chat.db, state.chat.station.id, token)
        .await
    {
        Ok(Some(s)) => Ok(Some(s)),
        Ok(None) => Err(problem(
            StatusCode::UNAUTHORIZED,
            "Your sign-in has ended. Sign in again.",
        )),
        Err(e) => Err(Problem::Db(e).into_response()),
    }
}

/// `None` when the studio may go on: the station key, or a member of staff (an admin, where
/// `admin` asks for one: the chat's settings, exports, erasure, holds and risk assessment).
async fn studio_only(state: &AppState, headers: &HeaderMap, admin: bool) -> Option<Response> {
    match staff_member(state, headers).await {
        Ok(Some(s)) if admin && !s.account.role.is_admin() => {
            return Some(problem(
                StatusCode::FORBIDDEN,
                "Only an admin can do that. Ask the station's admin.",
            ));
        }
        Ok(Some(_)) => return None,
        Ok(None) => {}
        Err(r) => return Some(r),
    }
    if state.admin_token.is_none() {
        return Some(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "the studio's routes need a staff sign-in, or the station key: set {}",
                crate::config::ADMIN_TOKEN_ENV
            ),
        ));
    }
    (!is_admin(state, headers))
        .then(|| problem(StatusCode::UNAUTHORIZED, "a valid bearer token is needed"))
}

fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if state.chat.config.listen.forwarded_for
        && let Some(ip) = forwarded_address(headers)
    {
        return ip;
    }
    peer.ip()
}

/// The address the station's own proxy saw: the **last** `X-Forwarded-For` entry. A proxy that
/// appends (Cloudron's nginx) keeps whatever the client sent in front of it, so the first entry
/// is the client's to forge, which would escape every per-address limit and fill their maps;
/// one that replaces the header (the Compose bundle's Caddy) sends one entry, also the last.
fn forwarded_address(headers: &HeaderMap) -> Option<IpAddr> {
    weavesignal_proto::last_forwarded(
        headers
            .get_all("x-forwarded-for")
            .iter()
            .map(|v| v.to_str().ok()),
    )
}

/// The address to keep with a session: none under No retention (ADR-0115).
fn kept_address(state: &AppState, ip: IpAddr) -> Option<IpAddr> {
    state.chat.retention().addresses.addresses().map(|_| ip)
}

fn new_token() -> String {
    format!("rsc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn new_code() -> String {
    let b = Uuid::new_v4();
    let n = u32::from_le_bytes(b.as_bytes()[..4].try_into().expect("four bytes")) % 1_000_000;
    format!("{n:06}")
}

// ---- public ----

#[derive(Serialize)]
struct ReasonInfo {
    reason: Reason,
    words: &'static str,
    hint: &'static str,
}

#[derive(Serialize)]
struct Retention {
    messages: String,
    moderation: String,
    addresses: String,
}

#[derive(Serialize)]
struct Info {
    station: String,
    terms: String,
    terms_version: String,
    accountable_person: String,
    contact: String,
    min_age: u8,
    rules: crate::wire::RoomRules,
    reasons: Vec<ReasonInfo>,
    mode_words: &'static str,
    retention: Retention,
    email_signin: bool,
    avatars: crate::avatar::AvatarInfo,
    attachments: crate::attach::AttachmentInfo,
}

async fn info(State(state): State<Arc<AppState>>) -> Response {
    let c = &state.chat;
    let s = c.settings();
    let (messages, moderation, addresses) = legal::retention_words(&c.retention());
    Json(Info {
        station: c.station.name.clone(),
        terms_version: s.terms_version(),
        terms: s.terms.clone(),
        accountable_person: s.accountable_person.clone(),
        contact: s.contact.clone(),
        min_age: c.min_age(),
        rules: c.rules(),
        reasons: Reason::ALL
            .into_iter()
            .map(|r| ReasonInfo {
                reason: r,
                words: r.words(),
                hint: r.hint(),
            })
            .collect(),
        mode_words: legal::mode_words(s.comments_only),
        retention: Retention {
            messages,
            moderation,
            addresses,
        },
        email_signin: c.config.mail.mode != MailMode::Off,
        avatars: c.avatar_info(),
        attachments: c.attachment_info(),
    })
    .into_response()
}

struct SocketGuard(Arc<AppState>, AddressKey);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.0.sockets.fetch_sub(1, Ordering::Relaxed);
        self.0.limits.sockets.close(self.1);
    }
}

async fn socket(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let key = AddressKey::from(client_ip(&state, &headers, peer));
    if !state.limits.sockets.open(key) {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many chat windows are open from here. Close one and try again.",
        );
    }
    let n = state.sockets.fetch_add(1, Ordering::Relaxed);
    if n >= state.chat.config.limits.max_sockets {
        state.sockets.fetch_sub(1, Ordering::Relaxed);
        state.limits.sockets.close(key);
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The chat is full. Try again in a minute.",
        );
    }
    let guard = SocketGuard(state.clone(), key);
    // Browsers send small frames: an 8 KiB read buffer (tungstenite's default is 128 KiB, which
    // at 4,000 sockets is half a gigabyte for nothing) and frames written as they come.
    ws.max_message_size(crate::ws::MAX_FRAME)
        .read_buffer_size(8 * 1024)
        .write_buffer_size(0)
        .on_upgrade(move |socket| async move {
            crate::ws::serve(guard.0.chat.clone(), socket).await;
            drop(guard);
        })
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartReq {
    email: String,
}

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

async fn signin_start(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<StartReq>,
) -> Response {
    let c = &state.chat;
    if c.config.mail.mode == MailMode::Off {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Email sign-in is not set up at this station yet.",
        );
    }
    let email = req.email.trim().to_lowercase();
    if !valid_email(&email) {
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "email",
            "Enter an email address, like name@example.com.",
        );
    }
    let ip = client_ip(&state, &headers, peer);
    {
        let now = crate::service::now_ms();
        let mut codes = state.codes.lock().expect("codes");
        codes.retain(|_, (t, _)| now - *t < 600_000);
        let e = codes.entry(ip).or_insert((now, 0));
        if e.1 >= c.config.limits.codes_per_address {
            return problem(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many codes asked for from here. Try again in ten minutes.",
            );
        }
        e.1 += 1;
    }
    match store::recent_codes(&c.db, c.station.id, &c.email_key, &email).await {
        Ok(n) if n >= i64::from(c.config.limits.codes_per_email) => {
            return problem(
                StatusCode::TOO_MANY_REQUESTS,
                "We've sent several codes to that address. Use the newest, or try again in ten minutes.",
            );
        }
        Ok(_) => {}
        Err(e) => return Problem::Db(e).into_response(),
    }
    let code = new_code();
    if let Err(e) = store::create_code(&c.db, c.station.id, &c.email_key, &email, &code, 10).await {
        return Problem::Db(e).into_response();
    }
    let body = format!(
        "Your code to join {station}'s chat is {code}\n\nIt works for 10 minutes. If you didn't ask for it, ignore this email: nothing happens without the code.\n",
        station = c.station.name
    );
    if let Err(e) = mail::send(
        &c.config.mail,
        &email,
        &format!("{code} is your {} chat code", c.station.name),
        &body,
    )
    .await
    {
        tracing::warn!(error = %e, "a sign-in code could not be sent");
        return problem(
            StatusCode::BAD_GATEWAY,
            "The code could not be sent. Try again in a moment.",
        );
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"sent": true, "expires_s": 600})),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishReq {
    email: String,
    code: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    age_ok: bool,
    #[serde(default)]
    terms_version: Option<String>,
}

#[derive(Serialize)]
struct SignedIn {
    session: String,
    me: Me,
}

async fn signed_in(state: &AppState, a: &Account, ip: IpAddr, days: u32) -> Response {
    let token = new_token();
    if let Err(e) = store::create_session(
        &state.chat.db,
        state.chat.station.id,
        a.id,
        &token,
        days,
        kept_address(state, ip),
    )
    .await
    {
        return Problem::Db(e).into_response();
    }
    let restriction = store::restriction(&state.chat.db, state.chat.station.id, a.id)
        .await
        .ok()
        .flatten();
    Json(SignedIn {
        session: token,
        me: me_of(a, restriction),
    })
    .into_response()
}

async fn signin_finish(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<FinishReq>,
) -> Response {
    let c = &state.chat;
    let email = req.email.trim().to_lowercase();
    let tried_from = AddressKey::from(client_ip(&state, &headers, peer));
    if !state
        .limits
        .signin_tries
        .take(tried_from, crate::service::now_ms())
    {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many codes tried from here. Try again in ten minutes.",
        );
    }
    if !req.code.trim().chars().all(|ch| ch.is_ascii_digit()) || req.code.trim().len() != 6 {
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "code",
            "The code is six digits.",
        );
    }
    match store::check_code(&c.db, c.station.id, &c.email_key, &email, &req.code).await {
        Ok(CodeCheck::Good) => {}
        Ok(CodeCheck::Wrong) => {
            return field_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "code",
                "That code doesn't match. Check the newest email.",
            );
        }
        Ok(CodeCheck::Spent) => {
            return field_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "code",
                "That code has expired. Ask for a new one.",
            );
        }
        Err(e) => return Problem::Db(e).into_response(),
    }
    let settings = c.settings();
    let version = settings.terms_version();
    let ip = client_ip(&state, &headers, peer);
    let days = c.config.limits.session_days;
    match store::account_by_verified_email(&c.db, c.station.id, &c.email_key, &email).await {
        Err(e) => Problem::Db(e).into_response(),
        Ok(Some(a)) => {
            // Terms changed since they agreed: ask again.
            if a.terms_version.as_deref() != Some(version.as_str()) {
                if req.terms_version.as_deref() != Some(version.as_str()) {
                    return needs(
                        vec!["terms"],
                        "The chat's rules have changed. Read and accept them to carry on.",
                    );
                }
                if let Err(e) = store::accept_terms(&c.db, a.id, &version, req.age_ok).await {
                    return Problem::Db(e).into_response();
                }
            }
            if !a.age_declared && !req.age_ok {
                return needs(
                    vec!["age_ok"],
                    format!("Confirm you are {} or older.", c.min_age_now().await),
                );
            }
            if !a.age_declared
                && let Err(e) = store::accept_terms(&c.db, a.id, &version, true).await
            {
                return Problem::Db(e).into_response();
            }
            let _ = store::spend_codes(&c.db, c.station.id, &c.email_key, &email).await;
            signed_in(&state, &a, ip, days).await
        }
        Ok(None) => {
            let mut missing = Vec::new();
            let name = match req
                .name
                .as_deref()
                .map(|n| rules::check_name(n, &settings.forbidden_names))
            {
                None => {
                    missing.push("name");
                    None
                }
                Some(Err(why)) => {
                    return field_problem(StatusCode::UNPROCESSABLE_ENTITY, "name", why);
                }
                Some(Ok(n)) => Some(n),
            };
            if !req.age_ok {
                missing.push("age_ok");
            }
            if req.terms_version.as_deref() != Some(version.as_str()) {
                missing.push("terms");
            }
            if !missing.is_empty() {
                return needs(
                    missing,
                    "New here: choose a name, confirm your age and accept the chat's rules.",
                );
            }
            let name = name.expect("checked above");
            match store::name_taken(&c.db, c.station.id, &name).await {
                Ok(true) => {
                    return field_problem(
                        StatusCode::CONFLICT,
                        "name",
                        "That name is taken. Choose another.",
                    );
                }
                Ok(false) => {}
                Err(e) => return Problem::Db(e).into_response(),
            }
            let a = match store::create_account(
                &c.db,
                c.station.id,
                &NewAccount {
                    kind: Kind::Email,
                    email_hash: Some(c.email_key.email_hash(&email)),
                    name: &name,
                    standing: Standing::Member,
                    terms_version: Some(&version),
                    age_declared: true,
                },
            )
            .await
            {
                Ok(a) => a,
                Err(e) => {
                    if e.as_database_error()
                        .is_some_and(|d| d.is_unique_violation())
                    {
                        return field_problem(
                            StatusCode::CONFLICT,
                            "name",
                            "That name is taken. Choose another.",
                        );
                    }
                    return Problem::Db(e).into_response();
                }
            };
            let _ = store::spend_codes(&c.db, c.station.id, &c.email_key, &email).await;
            signed_in(&state, &a, ip, days).await
        }
    }
}

fn needs(list: Vec<&'static str>, error: impl Into<String>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ProblemBody {
            error: error.into(),
            field: list.first().copied(),
            needs: Some(list),
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestReq {
    name: String,
    #[serde(default)]
    age_ok: bool,
    #[serde(default)]
    terms_version: Option<String>,
}

async fn guest(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<GuestReq>,
) -> Response {
    let c = &state.chat;
    let settings = c.settings();
    if !settings.guests {
        return problem(
            StatusCode::FORBIDDEN,
            "This station asks chatters to sign in with an email address.",
        );
    }
    let from = AddressKey::from(client_ip(&state, &headers, peer));
    if !state.limits.guests.take(from, crate::service::now_ms()) {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many guest names made from here. Try again in ten minutes.",
        );
    }
    let name = match rules::check_name(&req.name, &settings.forbidden_names) {
        Ok(n) => n,
        Err(why) => return field_problem(StatusCode::UNPROCESSABLE_ENTITY, "name", why),
    };
    if !req.age_ok {
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "age_ok",
            format!("Confirm you are {} or older.", c.min_age_now().await),
        );
    }
    let version = settings.terms_version();
    if req.terms_version.as_deref() != Some(version.as_str()) {
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "terms",
            "Read and accept the chat's rules.",
        );
    }
    match store::name_taken(&c.db, c.station.id, &name).await {
        Ok(true) => {
            return field_problem(
                StatusCode::CONFLICT,
                "name",
                "That name is taken. Choose another.",
            );
        }
        Ok(false) => {}
        Err(e) => return Problem::Db(e).into_response(),
    }
    let a = match store::create_account(
        &c.db,
        c.station.id,
        &NewAccount {
            kind: Kind::Guest,
            email_hash: None,
            name: &name,
            standing: Standing::Member,
            terms_version: Some(&version),
            age_declared: true,
        },
    )
    .await
    {
        Ok(a) => a,
        Err(e) => return Problem::Db(e).into_response(),
    };
    let ip = client_ip(&state, &headers, peer);
    signed_in(&state, &a, ip, 1).await
}

async fn notice(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<NoticeRequest>,
) -> Response {
    let from = AddressKey::from(client_ip(&state, &headers, peer));
    if !state.limits.notices.take(from, crate::service::now_ms()) {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many notices sent from here. Try again in ten minutes, or write to the station.",
        );
    }
    match state.chat.notice(&req).await {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---- a chatter's own ----

#[derive(Serialize)]
struct MeBody {
    me: Me,
    statements: Vec<Statement>,
}

async fn me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let restriction = store::restriction(&state.chat.db, state.chat.station.id, a.id)
        .await
        .ok()
        .flatten();
    match state.chat.statements_of(a.id).await {
        Ok(statements) => Json(MeBody {
            me: me_of(&a, restriction),
            statements,
        })
        .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn signout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(t) = bearer(&headers)
        && let Err(e) = store::end_session(&state.chat.db, t).await
    {
        return Problem::Db(e).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---- pictures and documents in the chat (ADR-0262) ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentQuery {
    /// The file's own name, shown as text.
    #[serde(default)]
    name: String,
    /// The message's words, which may be empty.
    #[serde(default)]
    body: String,
    #[serde(default)]
    nonce: String,
    #[serde(default)]
    reply_to: Option<i64>,
    #[serde(default)]
    playhead_ms: Option<i64>,
}

/// A trusted chatter posts a message with a picture or document: the file is the request's body,
/// the message's words and the file's name are in the query. It is held until staff or a
/// moderator approve it; the answer is the message's id and the attachment as the room will show
/// it.
async fn post_attachment(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<AttachmentQuery>,
    body: axum::body::Bytes,
) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let address = kept_address(&state, client_ip(&state, &headers, peer));
    let up = crate::attach::Upload {
        nonce: q.nonce.chars().take(64).collect(),
        name: q.name,
        body: q.body,
        reply_to: q.reply_to,
        playhead_ms: q.playhead_ms,
    };
    match state
        .chat
        .post_attachment(&a, body.to_vec(), up, address)
        .await
    {
        Ok((id, attachment)) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"id": id, "state": "held", "attachment": attachment})),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

/// Who asks for a posted file: anyone with no bearer; with one, its owner, the station's people
/// or a moderator.
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn file_viewer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::avatar::Viewer, Response> {
    use crate::avatar::Viewer;
    if bearer(headers).is_none() {
        return Ok(Viewer::Public);
    }
    if is_admin(state, headers) || staff_member(state, headers).await?.is_some() {
        return Ok(Viewer::Staff);
    }
    let a = chatter(state, headers).await?;
    Ok(if a.standing.moderates() {
        Viewer::Moderator
    } else {
        Viewer::Owner(a.id)
    })
}

/// A served file: a picture inline, a document only as a download under the station's own name.
/// Either way the type is the one the station found in its bytes, a browser may not guess
/// another (`nosniff`), and nothing in it may run (`sandbox`).
fn served(f: crate::attach::Served, download: bool, headers: &HeaderMap) -> Response {
    let cache = if f.public {
        "public, no-cache"
    } else {
        "private, no-store"
    };
    let tag = format!("\"{}\"", f.id.simple());
    if f.public
        && headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|t| t.trim() == tag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, tag),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
        )
            .into_response();
    }
    let disposition = if download {
        format!("attachment; filename=\"{}\"", f.download_name)
    } else {
        "inline".to_owned()
    };
    let mut res = (
        [
            (header::CONTENT_TYPE, f.mime),
            (header::CACHE_CONTROL, cache.to_owned()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; sandbox".to_owned(),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::HeaderName::from_static("cross-origin-resource-policy"),
                "same-origin".to_owned(),
            ),
        ],
        f.bytes,
    )
        .into_response();
    if f.public
        && let Ok(v) = tag.parse()
    {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

async fn attachment_response(
    state: &AppState,
    headers: &HeaderMap,
    id: Uuid,
    which: crate::attach::Which,
) -> Response {
    let who = match file_viewer(state, headers).await {
        Ok(w) => w,
        Err(r) => return r,
    };
    match state.chat.attachment_file(id, who, which).await {
        Ok(f) => served(f, which == crate::attach::Which::Document, headers),
        Err(e) => e.into_response(),
    }
}

/// A picture posted in the chat, as the station rewrote it.
async fn picture(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    attachment_response(&state, &headers, id, crate::attach::Which::Picture).await
}

/// Its thumbnail.
async fn picture_thumb(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    attachment_response(&state, &headers, id, crate::attach::Which::Thumb).await
}

/// A document posted in the chat, as it arrived, only ever as a download.
async fn document(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    attachment_response(&state, &headers, id, crate::attach::Which::Document).await
}

// ---- avatars (ADR-0235) ----

/// A picture's bytes as a response: a PNG and nothing a browser could take for anything else.
fn png(bytes: Vec<u8>, etag: Option<Uuid>, headers: &HeaderMap) -> Response {
    let cache = if etag.is_some() {
        // Shown beside every message, so a browser keeps it; but it asks again each time, and
        // a picture staff removed stops showing at once.
        "public, no-cache"
    } else {
        "private, no-store"
    };
    let tag = etag.map(|id| format!("\"{}\"", id.simple()));
    if let Some(tag) = &tag
        && headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|t| t.trim() == tag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, tag.clone()),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
        )
            .into_response();
    }
    let mut res = (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; sandbox",
            ),
            (header::CONTENT_DISPOSITION, "inline"),
        ],
        bytes,
    )
        .into_response();
    if let Some(tag) = tag
        && let Ok(v) = tag.parse()
    {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

/// An approved picture, for anyone. A held, refused or removed one answers 404, as an id that
/// never existed does.
async fn avatar(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    match state
        .chat
        .avatar_file(id, crate::avatar::Viewer::Public)
        .await
    {
        Ok(bytes) => png(bytes, Some(id), &headers),
        Err(e) => e.into_response(),
    }
}

/// A picture for the studio's people and the chatters the station made moderators (ADR-0255):
/// the ones waiting on the Screener tab and the ones showing. The studio's people also get one
/// withheld under an evidence hold.
async fn mod_avatar(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = match moderator(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let who = if actor.staff {
        crate::avatar::Viewer::Staff
    } else {
        crate::avatar::Viewer::Moderator
    };
    match state.chat.avatar_file(id, who).await {
        Ok(bytes) => png(bytes, None, &headers),
        Err(e) => e.into_response(),
    }
}

/// The pictures the station is keeping out of view, for the studio's people, and who may see the
/// ones kept as evidence (ADR-0257).
async fn kept_pictures(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = studio_only(&state, &headers, false).await {
        return r;
    }
    let viewer = match state.chat.evidence_viewer().await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let you = match staff_member(&state, &headers).await {
        Ok(Some(m)) => viewer.as_ref().is_some_and(|v| v.0 == m.account.id),
        _ => false,
    };
    match state.chat.kept_pictures().await {
        Ok(pictures) => Json(serde_json::json!({
            "pictures": pictures,
            "evidence_viewer": viewer.map(|v| serde_json::json!({"id": v.0, "name": v.1})),
            "you_are_the_evidence_viewer": you,
        }))
        .into_response(),
        Err(e) => e.into_response(),
    }
}

/// The one person the settings name (else the first admin), signed in as themselves: not the
/// station key, not another admin (ADR-0257).
#[allow(clippy::result_large_err)] // the refusal is the response itself
async fn evidence_person(state: &AppState, headers: &HeaderMap) -> Result<String, Response> {
    let Some(member) = staff_member(state, headers).await? else {
        return Err(problem(
            StatusCode::UNAUTHORIZED,
            "Sign in to the studio as yourself. The station key cannot open the evidence.",
        ));
    };
    match state.chat.evidence_viewer().await {
        Ok(Some((id, _))) if id == member.account.id => Ok(member.account.name),
        Ok(Some((_, name))) => Err(problem(
            StatusCode::FORBIDDEN,
            format!(
                "Only {name}, named in the chat's settings, can see a picture kept as evidence."
            ),
        )),
        Ok(None) => Err(problem(
            StatusCode::FORBIDDEN,
            "The station has no admin to see a picture kept as evidence: add one.",
        )),
        Err(e) => Err(e.into_response()),
    }
}

async fn evidence_list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(r) = evidence_person(&state, &headers).await {
        return r;
    }
    match state.chat.evidence_cases().await {
        Ok(cases) => Json(serde_json::json!({"cases": cases})).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn evidence_picture(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let by = match evidence_person(&state, &headers).await {
        Ok(b) => b,
        Err(r) => return r,
    };
    match state.chat.evidence_picture(id).await {
        Ok((mime, bytes)) => {
            let _ = state.chat.evidence_viewed(id, &by).await;
            if mime == "image/png" {
                png(bytes, None, &headers)
            } else {
                // A posted picture shown as a JPEG, or a document: never opened in the page.
                served(
                    crate::attach::Served {
                        download_name: format!("evidence-{}", id.simple()),
                        mime,
                        bytes,
                        public: false,
                        id,
                    },
                    true,
                    &headers,
                )
            }
        }
        Err(e) => e.into_response(),
    }
}

async fn evidence_export(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let by = match evidence_person(&state, &headers).await {
        Ok(b) => b,
        Err(r) => return r,
    };
    match state.chat.evidence_export(id, &by).await {
        Ok((name, zip)) => (
            [
                (header::CONTENT_TYPE, "application/zip".to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{name}\""),
                ),
                (header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            zip,
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

/// The chatter's own picture: the one waiting for staff if there is one, else the approved one.
async fn my_avatar(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.chat.own_avatar(&a).await {
        Ok((_, bytes)) => png(bytes, None, &headers),
        Err(e) => e.into_response(),
    }
}

#[derive(Serialize)]
struct MeOnly {
    me: Me,
}

/// Upload a picture: the body is the PNG or JPEG itself. It waits for staff.
async fn put_avatar(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    // The uploader's address is kept with the picture only where the station keeps addresses
    // (ADR-0115), for a report to the authorities if the picture proves to be evidence.
    let address = kept_address(&state, client_ip(&state, &headers, peer));
    match state.chat.upload_avatar(&a, body.to_vec(), address).await {
        Ok(me) => (StatusCode::ACCEPTED, Json(MeOnly { me })).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn delete_avatar(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.chat.withdraw_avatar(&a).await {
        Ok(me) => Json(MeOnly { me }).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StockReq {
    /// One of the shipped avatars, or null for the plain default.
    id: Option<String>,
}

async fn put_stock(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<StockReq>,
) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.chat.choose_stock(&a, req.id.as_deref()).await {
        Ok(me) => Json(MeOnly { me }).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportReq {
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(default)]
    account_id: Option<Uuid>,
    /// With `account_id`: report their picture, which must be the one showing (ADR-0260).
    #[serde(default)]
    avatar_id: Option<Uuid>,
    /// With `message_id`: report the picture or document the message carries (ADR-0262).
    #[serde(default)]
    attachment_id: Option<Uuid>,
    reason: Reason,
    #[serde(default)]
    detail: String,
}

async fn report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ReportReq>,
) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state
        .chat
        .report_by_chatter(
            &a,
            req.message_id,
            req.account_id,
            req.avatar_id,
            req.attachment_id,
            req.reason,
            req.detail.trim(),
        )
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewReq {
    text: String,
}

async fn review(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ReviewReq>,
) -> Response {
    let a = match chatter(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.chat.review(&a, id, &req.text).await {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---- the studio ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaffReq {
    name: String,
    #[serde(default)]
    role: Option<String>,
}

async fn staff_session(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<StaffReq>,
) -> Response {
    if let Some(r) = studio_only(&state, &headers, false).await {
        return r;
    }
    let ip = client_ip(&state, &headers, peer);
    // Signed in as themselves: their account's name, whatever was typed.
    if let Ok(Some(member)) = staff_member(&state, &headers).await {
        let a = match store::staff_account_for(
            &state.chat.db,
            state.chat.station.id,
            member.account.id,
            &member.account.name,
        )
        .await
        {
            Ok(a) => a,
            Err(e) => return Problem::Db(e).into_response(),
        };
        return signed_in(&state, &a, ip, 1).await;
    }
    // The station key: the typed name, as before staff accounts (the log says so).
    let name: String = req.name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(40).collect();
    if name.is_empty() {
        return field_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "name",
            "Give the name the studio posts and acts as.",
        );
    }
    let _ = req.role;
    let a = match store::staff_account(&state.chat.db, state.chat.station.id, &name).await {
        Ok(a) => a,
        Err(e) => return Problem::Db(e).into_response(),
    };
    signed_in(&state, &a, ip, 1).await
}

/// Whether the bearer is an API token (ADR-0195), by its prefix alone: whether it is a live
/// one is the control plane's to say.
fn api_token(headers: &HeaderMap) -> bool {
    bearer(headers).is_some_and(|t| t.starts_with(weavesignal_model::tokens::TOKEN_PREFIX))
}

/// The room's modes live in the control plane (the screener's S, K, L and influx's G G, with
/// its undo, ADR-0068); its route shares this service's prefix, so the station's proxy sends
/// it here and the chat passes it on unchanged, the caller's bearer with it.
///
/// An API token with `chat:moderate` may switch them (issue 2). The chat does not check the
/// token itself: the control plane holds the live tokens and the key their hashes are made
/// under, and the request passed on meets its gate like any other, which checks the token is
/// known, unexpired and unrevoked, holds the scope this route needs, and is within its
/// per-minute limit, and records its use. Checking it here as well would mean giving the chat
/// that key (`WEAVESIGNAL_CONTROL_TOKEN_KEY` where a station sets one) and keeping two copies
/// of the rules in step, for a check the control plane makes on this same call anyway. So a
/// token passes this door unchecked, and only this one: every other studio route of the chat
/// still refuses it.
async fn modes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !api_token(&headers)
        && let Some(r) = studio_only(&state, &headers, false).await
    {
        return r;
    }
    let base = state
        .chat
        .config
        .control
        .url
        .trim_end_matches('/')
        .to_owned();
    if base.is_empty() {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "The chat's modes are kept by the control plane, and none is set up.",
        );
    }
    // The caller's own bearer, so the control plane knows who switched the mode.
    let token = bearer(&headers).unwrap_or_default().to_owned();
    type Answer = (u16, Option<String>, String);
    let out = tokio::task::spawn_blocking(move || -> Result<Answer, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(5)))
            .build()
            .into();
        let mut r = agent
            .post(&format!("{base}/api/v1/chat/modes"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(&body[..])
            .map_err(|e| e.to_string())?;
        let code = r.status().as_u16();
        // A token over its per-minute limit is told when to try again (429).
        let retry = r
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok((
            code,
            retry,
            r.body_mut().read_to_string().unwrap_or_default(),
        ))
    })
    .await;
    match out {
        Ok(Ok((code, retry, text))) => {
            let mut res = (
                StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY),
                [(header::CONTENT_TYPE, "application/json")],
                text,
            )
                .into_response();
            if let Some(v) = retry.and_then(|v| header::HeaderValue::from_str(&v).ok()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            res
        }
        _ => problem(
            StatusCode::BAD_GATEWAY,
            "The control plane did not answer; the modes are unchanged.",
        ),
    }
}

async fn moderate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(cmd): Json<ModCommand>,
) -> Response {
    let actor = match moderator(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    match state.chat.moderate(&actor, cmd).await {
        Ok(done) => Json(serde_json::json!({"ok": true, "done": done})).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default)]
    limit: Option<i64>,
}

async fn mod_log(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<LogQuery>,
) -> Response {
    if let Err(r) = moderator(&state, &headers).await {
        return r;
    }
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    match store::read_log(&state.chat.db, state.chat.station.id, limit).await {
        Ok(rows) => Json(serde_json::json!({"log": rows.into_iter().map(|r| serde_json::json!({
            "id": r.0, "at_ms": r.1, "actor": r.2, "action": r.3, "account": r.4, "account_name": r.5,
            "message_id": r.6, "reason": r.7, "note": r.8, "automated": r.9, "until_ms": r.10,
        })).collect::<Vec<_>>()}))
        .into_response(),
        Err(e) => Problem::Db(e).into_response(),
    }
}

async fn account_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(r) = moderator(&state, &headers).await {
        return r;
    }
    match store::export(&state.chat.db, state.chat.station.id, id).await {
        Ok(Some(mut v)) => {
            // The moderator's view: the last 50 messages, no session addresses.
            if let Some(m) = v.get_mut("messages").and_then(|m| m.as_array_mut()) {
                let skip = m.len().saturating_sub(50);
                m.drain(..skip);
            }
            if let Some(o) = v.as_object_mut() {
                o.remove("sessions");
            }
            Json(v).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "No chatter with that id."),
        Err(e) => Problem::Db(e).into_response(),
    }
}

// ---- station admin ----

async fn get_settings(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    Json(state.chat.settings()).into_response()
}

async fn put_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut doc): Json<serde_json::Value>,
) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    // A document that leaves out one of the studio's switches or the evidence viewer keeps what
    // is stored (the operator, 2026-11-10): a script that writes the terms does not turn them off.
    if let (Some(given), Ok(serde_json::Value::Object(stored))) = (
        doc.as_object_mut(),
        serde_json::to_value(state.chat.settings()),
    ) {
        for key in ["avatar_uploads", "evidence_viewer", "attachments"] {
            if !given.contains_key(key)
                && let Some(v) = stored.get(key)
            {
                given.insert(key.to_owned(), v.clone());
            }
        }
    }
    let s: StationSettings = match serde_json::from_value(doc) {
        Ok(s) => s,
        Err(e) => {
            return Problem::Invalid(format!("The settings are not valid: {e}")).into_response();
        }
    };
    // The record names who: the staff member, else the station key's own name.
    let by = match staff_member(&state, &headers).await {
        Ok(Some(m)) => m.account.name,
        _ => "station admin".to_owned(),
    };
    match state.chat.save_settings(&by, s).await {
        Ok(()) => Json(state.chat.settings()).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn export(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    match store::export(&state.chat.db, state.chat.station.id, id).await {
        Ok(Some(v)) => {
            let _ = store::log(
                &state.chat.db,
                state.chat.station.id,
                &store::NewAction {
                    actor: "station admin",
                    action: "export",
                    account_id: Some(id),
                    message_id: None,
                    report_id: None,
                    reason: "",
                    ground: "",
                    note: "",
                    automated: false,
                    until_ms: None,
                },
            )
            .await;
            (
                [(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"chat-{id}.json\""),
                )],
                Json(v),
            )
                .into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "No chatter with that id."),
        Err(e) => Problem::Db(e).into_response(),
    }
}

async fn erase(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    match state.chat.erase(id, "station admin").await {
        Ok(n) => Json(serde_json::json!({"erased": true, "messages": n})).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn holds(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    match store::holds(&state.chat.db, state.chat.station.id).await {
        Ok(rows) => Json(serde_json::json!({"holds": rows.into_iter().map(|h| serde_json::json!({
            "id": h.0, "report": h.1, "account": h.2, "message_ids": h.3, "reason": h.4, "held_by": h.5,
            "held_ms": h.6, "released_ms": h.7, "released_by": h.8,
        })).collect::<Vec<_>>()}))
        .into_response(),
        Err(e) => Problem::Db(e).into_response(),
    }
}

async fn risk_assessment(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(r) = studio_only(&state, &headers, true).await {
        return r;
    }
    let c = &state.chat;
    let s = c.settings();
    let (m, mo, a) = legal::retention_words(&c.retention());
    let date = {
        let days = crate::service::now_ms() / 86_400_000;
        civil(days)
    };
    let text = legal::RISK_ASSESSMENT_TEMPLATE
        .replace("{station}", &c.station.name)
        .replace("{date}", &date)
        .replace("{mode}", legal::mode_words(s.comments_only))
        .replace(
            "{guests}",
            if s.guests {
                "; guests where the station allows them, every message held"
            } else {
                ""
            },
        )
        .replace(
            "{accountable}",
            if s.accountable_person.is_empty() {
                "(not named yet)"
            } else {
                &s.accountable_person
            },
        )
        .replace(
            "{contact}",
            if s.contact.is_empty() {
                "(not set yet)"
            } else {
                &s.contact
            },
        )
        .replace("{min_age}", &c.min_age().to_string())
        .replace("{review}", &crate::config::REVIEW_DAYS.to_string())
        .replace("{retention}", &format!("{m} {mo} {a}"));
    (
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        text,
    )
        .into_response()
}

/// Days since 1970-01-01 as `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_addresses_are_checked_loosely_but_safely() {
        assert!(valid_email("elspeth.gordon@mail.example"));
        assert!(!valid_email("elspeth"));
        assert!(!valid_email("a@b"));
        assert!(!valid_email("a b@mail.example"));
        assert!(!valid_email("a@mail.example\r\nBcc: x@y.example"));
    }

    #[test]
    fn actor_names_are_percent_decoded() {
        assert_eq!(percent_decode("Elspeth%20Gordon"), "Elspeth Gordon");
        assert_eq!(percent_decode("Eils%C3%ADdh"), "Eilsídh");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..50 {
            let c = new_code();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|ch| ch.is_ascii_digit()));
        }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(20_752), "2026-10-26");
    }

    #[test]
    fn the_forwarded_address_is_the_last_entry_not_a_forged_first() {
        let ip = |v: &[&str]| {
            let mut h = HeaderMap::new();
            for line in v {
                h.append("x-forwarded-for", line.parse().unwrap());
            }
            forwarded_address(&h)
        };
        let real: IpAddr = "198.51.100.4".parse().unwrap();
        assert_eq!(ip(&["198.51.100.4"]), Some(real));
        // What the client sent, then what an appending proxy (Cloudron's nginx) added.
        assert_eq!(ip(&["6.6.6.6, 7.7.7.7, 198.51.100.4"]), Some(real));
        assert_eq!(ip(&["6.6.6.6", "203.0.113.9,198.51.100.4"]), Some(real));
        // A forged first entry varied per request is still one address.
        assert_eq!(
            ip(&["10.0.0.1, 198.51.100.4"]),
            ip(&["10.0.0.2, 198.51.100.4"])
        );
        assert_eq!(ip(&["6.6.6.6, unknown"]), None);
        assert_eq!(ip(&[]), None);
    }
}
