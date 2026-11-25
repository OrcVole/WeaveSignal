// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows, the listener's side (1.3.2): entering a key, keeping its token fresh, seeing and
//! signing out devices, and asking for a lost key. None needs a bearer: the key's text is the
//! credential, and the token it earns is the next.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `POST /api/v1/keyed/redeem` | a listener | a key's text (and a show, a device) for a signed token for one show's window |
//! | `POST /api/v1/keyed/renew` | a listener | a token for a fresh one: revoke, pause, sign-out, expiry and the clock bite here |
//! | `POST /api/v1/keyed/devices` | a key's holder | the devices that hold the key |
//! | `POST /api/v1/keyed/devices/end` | a key's holder | sign one out |
//! | `POST /api/v1/keyed/lost` | a listener | ask for a lost key to be sent: the same answer matched or not |
//! | `GET /api/v1/public/keyed-shows` | anyone | which shows need a key, when they open to everyone, how to get a key |
//!
//! (Until 1.3.2's integration these were under `/api/v1/listen/`; the spike's names won.)
//!
//! **Why these are not under `/api/v1/public/`.** That prefix is for open `GET`s that carry
//! CORS to any website. Redeeming is a `POST` from the station's own player; it has limits of
//! its own (per address, and a wrong-key count) and is never open to other origins.
//!
//! **A token is for one show's window** (the operator's choice, so players that cannot change
//! the token mid-play keep playing): the show named, else the keyed show on now that the key
//! opens, else the next one; from the window's lead to its tail and the HLS lag, never past the
//! key's end. A key revoked or paused is refused at once by the origin and the edges (the
//! revoked list, [`crate::access_serve`]); a key signed out, shortened or narrowed is refused at
//! the holder's next renewal, which re-reads the key from the database each time (only a
//! renewal within `keys.touch_every_s` of the last writes nothing).
//!
//! **Counting.** A redeem begins a *listen* (and uses one of a key's listens) when the device is
//! new or has been away longer than `keys.listen_gap_s`; a reload within the gap carries on the
//! same listen. A renewal never counts, never makes a device, and so can never be refused for a
//! limit: a listen in progress is not cut at the limit, only the next one is.

// The `Err` of these helpers is the response to send, built once per refused request.
#![allow(clippy::result_large_err)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use weavesignal_model::listener_keys::{self as rules, Effective, Kind, Media, Refusal};
use weavesignal_model::listener_keys_db::{self as db, Device, Lookup, Timing, Verdict};

use crate::access::{self, clean_line};
use crate::access_token::{self, Claims, TokenError};
use crate::app::{AppState, problem};
use crate::events::KEY_FIRST_USED;
use crate::hub::now_ms;
use crate::keyed::AHEAD_MS;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/keyed/redeem", post(redeem))
        .route("/api/v1/keyed/renew", post(renew))
        .route("/api/v1/keyed/devices", post(devices))
        .route("/api/v1/keyed/devices/end", post(end_device))
        .route("/api/v1/keyed/lost", post(lost))
        .route("/api/v1/public/keyed-shows", get(keyed_shows))
}

const WINDOW_1M: u64 = 60_000;
const WINDOW_10M: u64 = 600_000;
const WINDOW_1H: u64 = 3_600_000;

/// What these routes need to answer: the database and the station's key, or the refusal.
fn ready(state: &AppState) -> Result<(&sqlx::PgPool, &crate::apikey::ApiKey), Response> {
    let (Some(pool), Some(key)) = (state.db.as_ref(), state.staff.api.key()) else {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Keys are not set up at this station.",
        ));
    };
    Ok((pool, key))
}

fn server_error(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "keyed shows: database error");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "the database failed")
}

fn limited(wait_s: u64, what: &str) -> Response {
    let mut res = problem(StatusCode::TOO_MANY_REQUESTS, what);
    res.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&wait_s.max(1).to_string()).expect("digits"),
    );
    res
}

/// A refusal as the player reads it: a status, a code, a sentence, and what the words need.
fn refusal_response(r: Refusal, extra: serde_json::Value) -> Response {
    let status = match r {
        Refusal::NotRecognised => StatusCode::NOT_FOUND,
        Refusal::TooManyDevices | Refusal::PeopleLimit | Refusal::ListensUsedUp => {
            StatusCode::CONFLICT
        }
        _ => StatusCode::FORBIDDEN,
    };
    let mut body = json!({ "error": r.sentence(), "code": r.code() });
    match r {
        Refusal::NotYetValid { from_ms } => body["from_ms"] = json!(from_ms),
        Refusal::Ended { until_ms } => body["until_ms"] = json!(until_ms),
        _ => {}
    }
    if let (Some(o), Some(e)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// The address the limits and the use records count: the proxy's header where
/// `join.forwarded_for` takes it, an IPv6 address as its /64, and nothing for a loopback peer.
/// With no address the per-address limits do not count at all (behind a proxy that is not
/// trusted every listener would share one count, and twenty wrong keys would shut the station);
/// only the use records fall back to a single placeholder.
fn address(state: &AppState, headers: &HeaderMap, conn: Option<SocketAddr>) -> Option<IpAddr> {
    state.staff.api.peer(headers, conn)
}

/// The use records' address when there is none: one value for all of them.
fn record_address(addr: Option<IpAddr>) -> IpAddr {
    addr.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// A per-address limit that does nothing without an address.
fn hit(
    c: &crate::access::Counter,
    addr: Option<IpAddr>,
    now: u64,
    window_ms: u64,
    limit: u32,
) -> Result<(), u64> {
    match addr {
        Some(a) => c.hit(&a.to_string(), now, window_ms, limit),
        None => Ok(()),
    }
}

fn peek(
    c: &crate::access::Counter,
    addr: Option<IpAddr>,
    now: u64,
    window_ms: u64,
    limit: u32,
) -> Result<(), u64> {
    match addr {
        Some(a) => c.peek(&a.to_string(), now, window_ms, limit),
        None => Ok(()),
    }
}

fn add(c: &crate::access::Counter, addr: Option<IpAddr>, now: u64, window_ms: u64) {
    if let Some(a) = addr {
        c.add(&a.to_string(), now, window_ms);
    }
}

/// A browser or app named briefly, from what it calls itself.
pub fn device_label(user_agent: &str) -> String {
    let ua = user_agent;
    let browser = [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("Firefox/", "Firefox"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("Brave", "Brave"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
        ("VLC", "VLC"),
        ("Lavf", "FFmpeg"),
        ("curl/", "curl"),
    ]
    .iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, n)| *n);
    let os = [
        ("Android", "Android"),
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Windows", "Windows"),
        ("Mac OS X", "macOS"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, n)| *n);
    match (browser, os) {
        (Some(b), Some(o)) => format!("{b} on {o}"),
        (Some(b), None) => b.to_owned(),
        (None, Some(o)) => format!("A browser on {o}"),
        (None, None) => "A browser or app".to_owned(),
    }
}

fn user_agent(headers: &HeaderMap) -> &str {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

// ---- redeem and renew ----

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenRedeem)]
#[serde(deny_unknown_fields)]
struct RedeemBody {
    /// The key as it was typed or sent: any case, with or without dashes.
    key: String,
    /// The show the listener wants (id or slug), if they named one.
    #[serde(default)]
    show: Option<String>,
    /// The device's id as this browser kept it from last time; left out on a first visit.
    #[serde(default)]
    device: Option<Uuid>,
    /// What to call this browser or app; left out, it is worked out from what it says it is.
    #[serde(default)]
    device_label: Option<String>,
}

/// A show a key opens.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenShow)]
struct ShowRef {
    id: Uuid,
    slug: String,
    name: String,
}

/// What the holder is told about the key: enough for "Welcome. This key opens the Friday
/// supporters' show and 3 more, until 30 Nov."
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenKey)]
struct HolderKey {
    id: Uuid,
    /// The name staff gave it, to find it again. It is theirs: the station's player does not
    /// show it to the key's holder, and another player should not either.
    label: String,
    kind: Kind,
    /// `video` opens what `both` does: a key for the picture carries the show's sound.
    media: Media,
    /// Opens every keyed show.
    all_shows: bool,
    /// The shows it opens when it is not `all_shows`.
    shows: Vec<ShowRef>,
    valid_from_ms: Option<i64>,
    valid_until_ms: Option<i64>,
    /// The key ends within `keys.ends_soon_days`: say so.
    ends_soon: bool,
}

/// Where the token opens its show: the addresses carry it in their path.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenUrls)]
struct Urls {
    /// The keyed HLS the key opens, on the station's HLS address (`join.hls_url`: the station's
    /// `/hls`, or an edge's): `audio.m3u8` (the sound alone) for a sound key, `live.m3u8`
    /// (picture and sound, with `audio.m3u8` beside it) for a key that has the picture (`video`
    /// or `both`: a key for the picture always carries the show's sound). A sound key is
    /// refused the picture (`wrong_media`).
    hls: String,
    /// The keyed Opus stream at the station: every key opens it, a `video` key too.
    opus: String,
    /// The keyed MP3 stream, when the station has its MP3 stream on.
    mp3: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenToken)]
struct Granted {
    /// The signed token (`ks1.…`, ECDSA P-256): it goes in the path of the keyed addresses
    /// (`urls`), never a query. Keep it in memory, not in storage.
    token: String,
    /// When it ends (Unix ms): the end of its show's window and margins, or the key's end.
    expires_ms: i64,
    /// When it starts (Unix ms): its window's lead. A token for a show later today waits.
    starts_ms: i64,
    /// The show it is for.
    show: ShowRef,
    /// Its keyed addresses.
    urls: Urls,
    /// Renew about this often (seconds).
    renew_in_s: u64,
    /// The device's id: keep it, and send it with the next redeem.
    device: Uuid,
    key: HolderKey,
}

/// Make the token for a granted listen: for `asked` if the listener named a show, else for the
/// keyed show on now that the key opens, else the next; for that show's window. A refusal when
/// the key does not open the show asked for, or no window is on the schedule.
async fn grant(
    state: &AppState,
    pool: &sqlx::PgPool,
    g: db::Granted,
    now: i64,
    asked: Option<Uuid>,
) -> Result<Granted, Response> {
    let rt = &state.staff.api.access;
    let api_key = state.staff.api.key().expect("checked by ready");
    rt.live.refresh(pool, state.station.id).await;
    let opens = |s: Uuid| g.key.all_shows || g.key.shows.contains(&s);
    let show = match asked {
        Some(s) if !opens(s) => return Err(refusal_response(Refusal::DoesNotOpenShow, json!({}))),
        Some(s) => Some(s),
        None => rt.live.first_for(opens, now),
    };
    let window = show.and_then(|s| rt.live.token_window(s, now).map(|w| (s, w)));
    let Some((show, (nbf_ms, exp_ms))) = window else {
        return Err(refusal_response(Refusal::NotScheduled, json!({})));
    };
    // Never past the key's end, nor before its start: a key that ends at 22:00 stops at 22:00.
    let exp_ms = g.key.valid_until_ms.map_or(exp_ms, |u| exp_ms.min(u));
    let nbf_ms = g.key.valid_from_ms.map_or(nbf_ms, |f| nbf_ms.max(f));
    if exp_ms <= nbf_ms.max(now) {
        return Err(refusal_response(Refusal::NotScheduled, json!({})));
    }
    let (nbf, exp) = (nbf_ms.div_euclid(1000), exp_ms.div_euclid(1000));
    let claims = Claims {
        key: g.key.id.to_string(),
        show: show.to_string(),
        media: access_token::media(g.key.media),
        nbf,
        exp,
        device: Some(g.device.to_string()),
    };
    let token = rt.signer(api_key).mint(&claims);
    let mut ids = g.key.shows.clone();
    if !ids.contains(&show) {
        ids.push(show);
    }
    let refs = db::show_refs(pool, state.station.id, &ids)
        .await
        .map_err(server_error)?;
    let this = refs
        .iter()
        .find(|(id, ..)| *id == show)
        .map(|(id, slug, name)| ShowRef {
            id: *id,
            slug: slug.clone(),
            name: name.clone(),
        })
        .unwrap_or(ShowRef {
            id: show,
            slug: String::new(),
            name: String::new(),
        });
    let soon = i64::from(rt.config.ends_soon_days) * 86_400_000;
    let ends_soon = g
        .key
        .valid_until_ms
        .is_some_and(|u| rt.config.ends_soon_days > 0 && u - now <= soon);
    let hls_base = state.join.hls_url.trim_end_matches('/');
    let urls = Urls {
        hls: keyed_hls_url(hls_base, show, &token, g.key.media),
        opus: format!(
            "/listen/k/{show}/{token}/{}",
            crate::access_serve::KEYED_OPUS
        ),
        mp3: (!state.join.mp3.is_empty()).then(|| {
            format!(
                "/listen/k/{show}/{token}/{}",
                crate::access_serve::KEYED_MP3
            )
        }),
    };
    let _ = AHEAD_MS;
    Ok(Granted {
        token,
        expires_ms: exp * 1000,
        starts_ms: nbf * 1000,
        show: this,
        urls,
        renew_in_s: rt.config.renew_every_s,
        device: g.device,
        key: HolderKey {
            id: g.key.id,
            label: g.key.label,
            kind: g.key.kind,
            media: g.key.media,
            all_shows: g.key.all_shows,
            shows: refs
                .into_iter()
                .filter(|(id, ..)| g.key.shows.contains(id))
                .map(|(id, slug, name)| ShowRef { id, slug, name })
                .collect(),
            valid_from_ms: g.key.valid_from_ms,
            valid_until_ms: g.key.valid_until_ms,
            ends_soon,
        },
    })
}

/// The keyed HLS playlist a key's token opens: the sound alone (`audio.m3u8`) for a sound key,
/// the picture with its sound (`live.m3u8`) for a key that has the picture (a `video` key's
/// token opens the sound beside it, as a `both` key's does: `access_token::media`). Never an
/// address the edge and the origin refuse the same token on.
fn keyed_hls_url(hls_base: &str, show: Uuid, token: &str, media: Media) -> String {
    let playlist = match media {
        Media::Sound => "audio.m3u8",
        Media::Video | Media::Both => "live.m3u8",
    };
    format!("{hls_base}/k/{show}/{token}/{playlist}")
}

fn timing(state: &AppState) -> Timing {
    let c = &state.staff.api.access.config;
    Timing {
        hold_ms: c.device_hold_s as i64 * 1000,
        gap_ms: c.listen_gap_s as i64 * 1000,
        touch_ms: c.touch_every_s as i64 * 1000,
    }
}

fn devices_json(ds: &[Device]) -> serde_json::Value {
    json!(
        ds.iter()
            .map(|d| json!({ "id": d.id, "label": d.label, "last_seen_ms": d.last_seen_ms }))
            .collect::<Vec<_>>()
    )
}

#[utoipa::path(
    post,
    path = "/api/v1/keyed/redeem",
    operation_id = "listen_redeem",
    tag = "listen",
    summary = "Enter a key",
    description = "No bearer: the key is the credential. `key` is the text as typed or sent (case, dashes and spaces are ignored); `show` the show wanted (id or slug), if any; `device` this browser's id from last time, if it has one. The answer is a signed token for one show's window (`show`: the one named, else the keyed show on now that the key opens, else the next one; from `starts_ms` to `expires_ms`), the keyed addresses with the token in their path (`urls`), the device's id to keep, and what the key is, for the welcome. Renew about every `renew_in_s` seconds: the renewal is the device's place on the key. A refusal carries a `code` and a plain sentence: `not_recognised` (404), `not_yet_valid` (403, with `from_ms`), `ended` (403, `until_ms`), `paused`, `revoked`, `does_not_open_show`, `not_scheduled` (403: none of its shows has a keyed window on the schedule in the next fortnight), `too_many_devices` (409, with the `devices` that hold it), `people_limit`, `listens_used_up` (409). Redeems are limited per address per minute, and wrong keys per address per ten minutes (429).",
    request_body = RedeemBody,
    responses(
        (status = 200, description = "The token and the key.", body = Granted),
        (status = 403, description = "The key is known but does not open now: `code` says why."),
        (status = 404, description = "No such key (`not_recognised`) or no such show."),
        (status = 409, description = "A limit of the key is reached (`code` says which)."),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn redeem(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<RedeemBody>,
) -> Response {
    let (pool, api_key) = match ready(&state) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let rt = &state.staff.api.access;
    let now = now_ms();
    let addr = address(&state, &headers, conn.map(|Extension(ConnectInfo(a))| a));
    if let Err(w) = hit(
        &rt.redeems,
        addr,
        now,
        WINDOW_1M,
        rt.config.redeem_per_minute,
    ) {
        return limited(w, "Too many tries just now. Wait a little and try again.");
    }
    if let Err(w) = peek(
        &rt.failures,
        addr,
        now,
        WINDOW_10M,
        rt.config.failures_per_ten_minutes,
    ) {
        return limited(
            w,
            "Too many keys tried from here. Wait a few minutes and try again.",
        );
    }
    let norm = rules::normalise_text(&body.key);
    if !rules::text_is_wellformed(&norm) {
        add(&rt.failures, addr, now, WINDOW_10M);
        return refusal_response(Refusal::NotRecognised, json!({}));
    }
    let station = state.station.id;
    let show = match body
        .show
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(s) => match Uuid::parse_str(s) {
            Ok(id) => Some(id),
            Err(_) => match weavesignal_model::shows::get_show(pool, station, s).await {
                Ok(Some(sh)) => Some(sh.id),
                Ok(None) => return problem(StatusCode::NOT_FOUND, "There is no such show."),
                Err(e) => return server_error(e),
            },
        },
    };
    // Which show the token will be for, decided before anything is counted: a key with nothing
    // on the schedule is told so without using a device or a listen. A key that is paused,
    // revoked or outside its window is left to the judgement below, whose words say more.
    rt.live.refresh(pool, station).await;
    let show = match db::find_by_text(pool, station, &api_key.listener_text_hash(&norm)).await {
        Err(e) => return server_error(e),
        Ok(Some(k))
            if k.state == weavesignal_model::listener_keys::State::Active
                && k.valid_from_ms.is_none_or(|f| f <= now as i64)
                && k.valid_until_ms.is_none_or(|u| u > now as i64) =>
        {
            let opens = |s: Uuid| k.all_shows || k.shows.contains(&s);
            match show {
                Some(s) if !opens(s) => {
                    return refusal_response(Refusal::DoesNotOpenShow, json!({ "label": k.label }));
                }
                Some(s) => {
                    if rt.live.token_window(s, now as i64).is_none() {
                        return refusal_response(
                            Refusal::NotScheduled,
                            json!({ "label": k.label }),
                        );
                    }
                    Some(s)
                }
                None => match rt.live.first_for(opens, now as i64) {
                    Some(s) => Some(s),
                    None => {
                        return refusal_response(
                            Refusal::NotScheduled,
                            json!({ "label": k.label }),
                        );
                    }
                },
            }
        }
        Ok(_) => show,
    };
    let label = body
        .device_label
        .as_deref()
        .and_then(|l| clean_line(l, 60))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| device_label(user_agent(&headers)));
    let verdict = db::begin_listen(
        pool,
        &db::Listen {
            station,
            by: Lookup::Text(api_key.listener_text_hash(&norm)),
            device: body.device,
            device_label: &label,
            show,
            addr_hash: api_key.listener_addr_hash(record_address(addr)),
            now_ms: now as i64,
            renew: false,
            timing: timing(&state),
        },
    )
    .await;
    match verdict {
        Err(e) => server_error(e),
        Ok(Verdict::Refused {
            refusal,
            key,
            devices,
        }) => {
            if refusal == Refusal::NotRecognised {
                add(&rt.failures, addr, now, WINDOW_10M);
            }
            let mut extra = json!({});
            if refusal == Refusal::TooManyDevices {
                extra["devices"] = devices_json(&devices);
                extra["max_devices"] = json!(key.as_ref().and_then(|k| k.max_devices));
                extra["device_hold_s"] = json!(rt.config.device_hold_s);
            }
            if let Some(k) = key.as_ref() {
                extra["label"] = json!(k.label);
            }
            refusal_response(refusal, extra)
        }
        Ok(Verdict::Granted(g)) => {
            if g.first_use {
                state
                    .staff
                    .api
                    .events
                    .publish(KEY_FIRST_USED, crate::access_keys::first_used_data(&g.key));
            }
            match grant(&state, pool, *g, now as i64, show).await {
                Ok(r) => (
                    StatusCode::OK,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(r),
                )
                    .into_response(),
                Err(r) => r,
            }
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenRenew)]
#[serde(deny_unknown_fields)]
struct RenewBody {
    /// The token the player holds.
    token: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/keyed/renew",
    operation_id = "listen_renew",
    tag = "listen",
    summary = "Renew a key's token",
    description = "No bearer: the token is the credential. Trades a token that has not ended for a fresh one for the same show (its window now, or its next), after re-reading the key: a key revoked, paused, ended, signed out or no longer opening the show is refused here (the same `code`s as redeeming, and `device_signed_out`; 403 or 409). A revoked or paused key is also refused at once by the station and its edges, whatever its token's life. A renewal does not count as a listen and never makes a device. A token that has ended is `401` (`token_ended`): redeem the key again. Limited per device.",
    request_body = RenewBody,
    responses(
        (status = 200, description = "The fresh token.", body = Granted),
        (status = 401, description = "The token is not valid or has ended (`token_ended`, `token_invalid`)."),
        (status = 403, description = "The key no longer opens: `code` says why."),
        (status = 409, description = "A limit of the key is reached."),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn renew(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<RenewBody>,
) -> Response {
    let (pool, api_key) = match ready(&state) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let rt = &state.staff.api.access;
    let now = now_ms();
    let signer = rt.signer(api_key);
    let read = match access_token::decode(signer, body.token.trim(), (now / 1000) as i64, 0) {
        // A token for a show later on (redeemed before its window) renews before it starts: its
        // signature is good and it has not ended, since it ends after it starts.
        Err(TokenError::NotYet) => {
            access_token::decode(signer, body.token.trim(), (now / 1000) as i64, i64::MAX / 4)
        }
        r => r,
    }
    .and_then(|c| {
        let ids = (
            Uuid::parse_str(&c.key).ok(),
            c.device.as_deref().and_then(|d| Uuid::parse_str(d).ok()),
            Uuid::parse_str(&c.show).ok(),
        );
        match ids {
            (Some(k), Some(d), Some(s)) => Ok((k, d, s)),
            _ => Err(TokenError::Malformed),
        }
    });
    let (key_id, device_id, show_id) = match read {
        Ok(c) => c,
        Err(TokenError::Expired) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "That token has ended. Enter the key again.", "code": "token_ended" })),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "That token is not valid.", "code": "token_invalid" })),
            )
                .into_response();
        }
    };
    // A device renews about once a minute; ten a minute is a player that is stuck in a loop.
    if let Err(w) = rt
        .redeems
        .hit(&format!("d:{device_id}"), now, WINDOW_1M, 10)
    {
        return limited(w, "This device is renewing too often.");
    }
    let addr = address(&state, &headers, conn.map(|Extension(ConnectInfo(a))| a));
    let verdict = db::begin_listen(
        pool,
        &db::Listen {
            station: state.station.id,
            by: Lookup::Id(key_id),
            device: Some(device_id),
            device_label: "",
            show: Some(show_id),
            addr_hash: api_key.listener_addr_hash(record_address(addr)),
            now_ms: now as i64,
            renew: true,
            timing: timing(&state),
        },
    )
    .await;
    match verdict {
        Err(e) => server_error(e),
        Ok(Verdict::Refused { refusal, key, .. }) => {
            let extra = key.map_or(json!({}), |k| json!({ "label": k.label }));
            refusal_response(refusal, extra)
        }
        Ok(Verdict::Granted(g)) => match grant(&state, pool, *g, now as i64, Some(show_id)).await {
            Ok(r) => (
                StatusCode::OK,
                [(header::CACHE_CONTROL, "no-store")],
                Json(r),
            )
                .into_response(),
            Err(r) => r,
        },
    }
}

// ---- devices ----

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenDevices)]
#[serde(deny_unknown_fields)]
struct DevicesBody {
    key: String,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenDeviceList)]
struct DeviceList {
    devices: Vec<Device>,
    /// A device holds its place on the key while it has renewed within this many seconds: one
    /// seen within it is listening now.
    device_hold_s: u64,
}

/// The key a holder named, or the refusal: counted as a wrong key when it is not one.
async fn holder_key(
    state: &AppState,
    headers: &HeaderMap,
    conn: Option<SocketAddr>,
    typed: &str,
) -> Result<weavesignal_model::listener_keys::Key, Response> {
    let (pool, api_key) = ready(state)?;
    let rt = &state.staff.api.access;
    let now = now_ms();
    let addr = address(state, headers, conn);
    hit(
        &rt.redeems,
        addr,
        now,
        WINDOW_1M,
        rt.config.redeem_per_minute,
    )
    .map_err(|w| limited(w, "Too many tries just now. Wait a little and try again."))?;
    peek(
        &rt.failures,
        addr,
        now,
        WINDOW_10M,
        rt.config.failures_per_ten_minutes,
    )
    .map_err(|w| {
        limited(
            w,
            "Too many keys tried from here. Wait a few minutes and try again.",
        )
    })?;
    let norm = rules::normalise_text(typed);
    let found = if rules::text_is_wellformed(&norm) {
        db::find_by_text(pool, state.station.id, &api_key.listener_text_hash(&norm))
            .await
            .map_err(server_error)?
    } else {
        None
    };
    found.ok_or_else(|| {
        add(&rt.failures, addr, now, WINDOW_10M);
        refusal_response(Refusal::NotRecognised, json!({}))
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/keyed/devices",
    operation_id = "listen_devices",
    tag = "listen",
    summary = "The devices that hold a key",
    description = "No bearer: the key is the credential. The browsers and apps that have used it, the live ones first, so a holder who is told the key is in use on its most devices can sign one out. Wrong keys are counted like a redeem's.",
    request_body = DevicesBody,
    responses(
        (status = 200, description = "The devices.", body = DeviceList),
        (status = 404, description = "No such key (`not_recognised`)."),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn devices(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<DevicesBody>,
) -> Response {
    let key = match holder_key(
        &state,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
        &body.key,
    )
    .await
    {
        Ok(k) => k,
        Err(r) => return r,
    };
    match db::devices(
        state.db.as_ref().expect("checked"),
        state.station.id,
        key.id,
    )
    .await
    {
        Ok(devices) => Json(DeviceList {
            devices,
            device_hold_s: state.staff.api.access.config.device_hold_s,
        })
        .into_response(),
        Err(e) => server_error(e),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenDeviceEnd)]
#[serde(deny_unknown_fields)]
struct EndBody {
    key: String,
    device: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/v1/keyed/devices/end",
    operation_id = "listen_devices_end",
    tag = "listen",
    summary = "Sign a device out of a key",
    description = "No bearer: the key is the credential. The device is refused at its next renewal and its place is free at once. 404 for a key not recognised or a device the key does not have.",
    request_body = EndBody,
    responses(
        (status = 204, description = "Signed out."),
        (status = 404, description = "No such key or device."),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn end_device(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<EndBody>,
) -> Response {
    let key = match holder_key(
        &state,
        &headers,
        conn.map(|Extension(ConnectInfo(a))| a),
        &body.key,
    )
    .await
    {
        Ok(k) => k,
        Err(r) => return r,
    };
    let pool = state.db.as_ref().expect("checked");
    match db::end_device(pool, state.station.id, key.id, body.device).await {
        Ok(true) => {
            if let Ok(mut c) = pool.acquire().await {
                let _ = db::log(
                    &mut c,
                    state.station.id,
                    key.id,
                    "the holder",
                    "device_ended",
                    json!({ "device": body.device }),
                )
                .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => problem(StatusCode::NOT_FOUND, "That key has no such device."),
        Err(e) => server_error(e),
    }
}

// ---- a lost key ----

/// What a lost-key request is answered with (`202`), always the same shape.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = ListenLostAnswer)]
struct LostAnswer {
    /// What to tell the person, in the station's words for whether it sends mail.
    message: String,
    /// The station sends email: "we will email it". False: nothing is sent; the request is
    /// logged for staff, who send the key themselves.
    mail: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = ListenLost)]
#[serde(deny_unknown_fields)]
struct LostBody {
    /// The email address the key was made with.
    email: String,
}

/// The answer to every lost-key request that was within the limits, when the station sends mail.
const LOST_ANSWER: &str = "If a key was made for that address, the station is sending it now. Check your inbox and your spam folder.";

/// The same, when it does not (no outgoing mail is set up): the request is logged on the keys
/// made with that address, which staff can see and send themselves. Never says whether one matched.
const LOST_ANSWER_NO_MAIL: &str = "This station does not send email itself. If a key was made for that address, the station's staff can see your request and will send it to you. If you do not hear from them, ask them.";

/// The body of every lost-key answer within the limits. `mail` is a fact about the station
/// (whether its outgoing mail is set up), never about the address asked for, so it cannot be
/// used to find out who has a key.
fn lost_answer(mail: bool) -> serde_json::Value {
    json!({
        "message": if mail { LOST_ANSWER } else { LOST_ANSWER_NO_MAIL },
        "mail": mail,
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/keyed/lost",
    operation_id = "listen_lost",
    tag = "listen",
    summary = "Ask for a lost key",
    description = "No bearer. The station emails the key's link to the address if a key was made with it, and says the same thing whether one matched or not (`202`), so the route cannot be used to find out who has a key. The work is done after the answer, so it takes the same time either way. Limited per address (`keys.lost_per_hour`, 429) and per email address (further requests are answered the same and send nothing). With mail off, nothing is sent and the request is logged on the matching keys (`lost_key_asked`) for staff to send the key themselves.",
    request_body = LostBody,
    responses(
        (status = 202, description = "Always, within the limits: `{ message, mail }`. `mail` says whether the station sends email (the message follows it: with mail off the request is for staff to answer, and the screen should say so). It is about the station, never about the address, so it is the same whether a key matched or not.", body = LostAnswer),
        (status = 429, response = crate::openapi::TooManyRequests),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn lost(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<LostBody>,
) -> Response {
    let (_, api_key) = match ready(&state) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let rt = &state.staff.api.access;
    let now = now_ms();
    let addr = address(&state, &headers, conn.map(|Extension(ConnectInfo(a))| a));
    if let Err(w) = hit(
        &rt.lost_by_address,
        addr,
        now,
        WINDOW_1H,
        rt.config.lost_per_hour,
    ) {
        return limited(w, "Too many requests from here. Try again later.");
    }
    let answer = (
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(lost_answer(state.staff.mailer().is_some())),
    );
    // Not an address at all: the same answer, and nothing to do.
    let Some(email) = access::clean_email(&body.email) else {
        return answer.into_response();
    };
    let hash = api_key.listener_email_hash(&email);
    if rt
        .lost_by_email
        .hit(&crate::apikey::hex(&hash), now, WINDOW_1H, 3)
        .is_err()
    {
        return answer.into_response();
    }
    let state2 = state.clone();
    tokio::spawn(async move {
        if let Err(e) = send_lost(&state2, &email, hash).await {
            tracing::warn!(error = %e, "keyed shows: a lost-key request could not be answered");
        }
    });
    answer.into_response()
}

/// Find the keys made with this email and send them (or, with mail off, log the request on
/// them for staff). Nothing here is seen by the one who asked.
async fn send_lost(state: &AppState, email: &str, hash: [u8; 32]) -> Result<(), String> {
    let (pool, api_key) = ready(state).map_err(|_| "no database or key".to_owned())?;
    let station = state.station.id;
    let ids = db::live_keys_for_email(pool, station, &hash, now_ms() as i64)
        .await
        .map_err(|e| e.to_string())?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut lines = Vec::new();
    for id in &ids {
        if let Ok(Some((n, s))) = db::sealed_text(pool, station, *id).await
            && let Some(text) = access::open_text(api_key, *id, &n, &s)
        {
            let label = db::get_key(pool, station, *id)
                .await
                .ok()
                .flatten()
                .map(|k| k.label)
                .unwrap_or_default();
            lines.push((
                label,
                rules::format_text(&text),
                access::link(&state.join.public_url, &text),
            ));
        }
    }
    let mailer = state.staff.mailer();
    let sent = match (mailer, lines.is_empty()) {
        (Some(m), false) => {
            let station_name = state.station_name();
            let mut text = format!("Someone asked for your key to {station_name}. Here it is.\n\n");
            for (label, key, link) in &lines {
                text.push_str(&format!("{label}\n  Key: {key}\n  Open: {link}\n\n"));
            }
            text.push_str(
                "If you did not ask, you can ignore this message: nobody else was sent your key.",
            );
            crate::mail::send(
                &m.config,
                email,
                &format!("Your key to {station_name}"),
                &text,
            )
            .await
            .map_err(|e| e.to_string())?;
            true
        }
        _ => false,
    };
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    for id in ids {
        let _ = db::log(
            &mut conn,
            station,
            id,
            "the holder",
            "lost_key_asked",
            json!({ "sent": sent }),
        )
        .await;
    }
    Ok(())
}

// ---- which shows are keyed ----

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = KeyedShowRow)]
struct KeyedRow {
    id: Uuid,
    slug: String,
    name: String,
    /// What the show is now: `key` (only a key opens it) or `public` (it has opened to everyone).
    now: &'static str,
    /// When it opens to everyone, if it does (Unix milliseconds).
    opens_at_ms: Option<i64>,
    /// What the public hears while it is keyed: `announcement`, `fallback` or `teaser`.
    slate: String,
    slate_note: String,
}

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = KeyedShowList)]
struct KeyedList {
    /// Where to get a key, in the station's words (`keys.get_a_key_text`); may be empty.
    get_a_key_text: String,
    get_a_key_url: String,
    /// The station sends email (1.3.2), so "Lost your key?" can say it will email the key; with
    /// false the player says staff will answer the request.
    mail: bool,
    /// The keyed show whose window is open now (with its margins), if one is: while it is, the
    /// public stream plays the slate and the public picture is held back, so the player shows
    /// the lock and offers to enter a key.
    on_now: Option<Uuid>,
    shows: Vec<KeyedRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/public/keyed-shows",
    operation_id = "public_keyed_shows",
    tag = "public",
    summary = "The keyed shows",
    description = "No bearer; open to any website like the other public routes, and limited per address. The shows that need a key (so the schedule and the player can show a lock), when each opens to everyone, what the public hears meanwhile, and where to get a key. It says nothing about who holds a key.",
    responses(
        (status = 200, description = "The keyed shows.", body = KeyedList),
        (status = 503, response = crate::openapi::Unavailable),
    ),
    security(()),
)]
async fn keyed_shows(State(state): State<Arc<AppState>>) -> Response {
    let Some(pool) = state.db.as_ref() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Keys are kept in the database.",
        );
    };
    let now = now_ms() as i64;
    let rt = &state.staff.api.access;
    let cfg = &rt.config;
    rt.live.refresh(pool, state.station.id).await;
    let on_now = rt.live.active(now).first().copied();
    match db::list_keyed(pool, state.station.id).await {
        Ok(rows) => Json(KeyedList {
            get_a_key_text: cfg.get_a_key_text.clone(),
            get_a_key_url: cfg.get_a_key_url.clone(),
            mail: state.staff.mailer().is_some(),
            on_now,
            shows: rows
                .into_iter()
                .map(|r| KeyedRow {
                    id: r.access.show_id,
                    slug: r.slug,
                    name: r.name,
                    now: match rules::effective(&r.access.rule, r.access.opens_at_ms, now) {
                        Effective::Key => "key",
                        Effective::Public => "public",
                    },
                    opens_at_ms: r.access.opens_at_ms,
                    slate: r.access.slate,
                    slate_note: r.access.slate_note,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => server_error(e),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(redeem, renew, devices, end_device, lost, keyed_shows))]
pub(crate) struct Api;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_given_the_hls_address_its_token_opens() {
        let show = Uuid::nil();
        let base = "https://station.example/hls";
        let at = |m| keyed_hls_url(base, show, "ks1.a.b", m);
        assert_eq!(
            at(Media::Sound),
            format!("{base}/k/{show}/ks1.a.b/audio.m3u8")
        );
        assert_eq!(
            at(Media::Both),
            format!("{base}/k/{show}/ks1.a.b/live.m3u8")
        );
        assert_eq!(
            at(Media::Video),
            format!("{base}/k/{show}/ks1.a.b/live.m3u8")
        );
        // Each is what the token check asks for of that medium: a sound key's address is a
        // sound file, a picture key's a picture one.
        for (m, want) in [
            (Media::Sound, weavesignal_keyed::Media::Sound),
            (Media::Both, weavesignal_keyed::Media::Video),
            (Media::Video, weavesignal_keyed::Media::Video),
        ] {
            let url = at(m);
            let rest = url.rsplit('/').next().unwrap();
            assert_eq!(weavesignal_keyed::hls_media(rest), want, "{url}");
            // And the token minted for that key opens it, with the sound beside it: a key for
            // the picture is never a picture with no voice.
            let minted = access_token::media(m);
            assert!(minted.opens(want), "{m:?}: {url}");
            assert!(
                minted.opens(weavesignal_keyed::hls_media("audio.m3u8")),
                "{m:?}: the sound beside {url}"
            );
        }
    }

    #[test]
    fn a_device_is_named_from_what_it_says_it_is() {
        assert_eq!(
            device_label("Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"),
            "Firefox on Linux"
        );
        assert_eq!(
            device_label(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
            ),
            "Safari on iPhone"
        );
        assert_eq!(
            device_label(
                "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/126.0.0.0 Mobile Safari/537.36"
            ),
            "Chrome on Android"
        );
        assert_eq!(device_label("VLC/3.0.20 LibVLC/3.0.20"), "VLC");
        assert_eq!(device_label(""), "A browser or app");
    }

    #[test]
    fn a_refusal_answers_with_a_code_a_sentence_and_what_the_words_need() {
        let r = refusal_response(
            Refusal::NotYetValid { from_ms: 1234 },
            json!({ "label": "Ishbel" }),
        );
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = refusal_response(Refusal::NotRecognised, json!({}));
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = refusal_response(Refusal::TooManyDevices, json!({}));
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert_eq!(
            r.headers().get(header::CACHE_CONTROL).map(|v| v.as_bytes()),
            Some(&b"no-store"[..])
        );
    }

    #[test]
    fn the_lost_key_answer_follows_whether_the_station_sends_mail_and_nothing_else() {
        let on = lost_answer(true);
        let off = lost_answer(false);
        assert_eq!(on["mail"], true);
        assert_eq!(off["mail"], false);
        assert!(
            on["message"]
                .as_str()
                .unwrap()
                .contains("is sending it now")
        );
        let words = off["message"].as_str().unwrap();
        assert!(words.contains("does not send email"), "{words}");
        assert!(!words.contains("is sending it now"), "{words}");
        // The same keys in both: the answer has a message and `mail`, never who has a key.
        assert_eq!(on.as_object().unwrap().len(), 2);
        assert_eq!(off.as_object().unwrap().len(), 2);
    }
}
