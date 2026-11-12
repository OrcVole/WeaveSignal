// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station the packaged app runs (ADR-0019 §3) and its key (ADR-0121), or a staff
//! member's sign-in (ADR-0127), kept the same way: the address in a
//! small file of the app's own, the key in the system's keychain (Secret Service on Linux,
//! Credential Manager on Windows, the Keychain on macOS). On a Linux desktop without a
//! keyring the key goes in a file only this user can read, and the setup says so.
//!
//! The page never holds the key: every request to the station goes out from here, which adds
//! it. The page names only a path under the saved station's `/api/`, so it cannot send the key
//! anywhere else. Bodies come back over a channel as they arrive, so the studio's event
//! stream works as before.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, JavaScriptChannelId, Request};
use tauri::{AppHandle, Manager, State, Webview};

/// The keychain entry's service; the account is the station's address.
const SERVICE: &str = "weavesignal-studio";

/// Where the key is kept, as the setup shows it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kept {
    Keychain,
    File,
}

/// The station as the page may know it: its address and where its key is kept.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct StationInfo {
    pub url: String,
    pub kept: Kept,
}

/// Requests in flight, so the page can stop one (an event stream it no longer follows).
pub struct Stations {
    client: reqwest::Client,
    next: AtomicU32,
    open: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
}

impl Stations {
    pub fn new() -> Self {
        Stations {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                // A login proxy in front of the station (Cloudron's, with bearer support) turns
                // the station's 401 into a redirect to its login page. Followed, it would come
                // back as that page with 200; kept, it is read as the 401 it was.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("an HTTP client with the platform's TLS"),
            next: AtomicU32::new(1),
            open: Mutex::new(HashMap::new()),
        }
    }
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("no folder for the app's settings: {e}"))
}

fn info_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(dir(app)?.join("station.json"))
}

fn key_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(dir(app)?.join("station.key"))
}

/// What the sign-in says when plain http is tried on an address that is not nearby (ADR-0139).
/// The page checks the same rule before it asks (`apps/studio/src/lib/address.ts`); keep the two
/// sentences alike.
const PLAIN_HTTP_REFUSED: &str = "Plain http:// is only for this computer and private networks, \
    like http://192.168.1.20:8080. For a station on the internet, use https://, like \
    https://radio.example.org.";

/// Whether plain http may be spoken to this host (ADR-0139): this computer, or a private
/// network. Decided from the address as written, with no DNS lookup, so a name other than
/// `localhost` is never nearby: a name can point anywhere.
///
/// Private is RFC 1918 (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10),
/// RFC 4193 unique local (fc00::/7), and carrier-grade NAT (100.64/10), which Tailscale gives
/// every machine of a tailnet and which is encrypted end to end between them.
fn is_nearby(host: url::Host<&str>) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr};
    fn v4(a: Ipv4Addr) -> bool {
        let [o0, o1, ..] = a.octets();
        a.is_loopback()
            || a.is_private()
            || a.is_link_local()
            || (o0 == 100 && (64..=127).contains(&o1))
    }
    fn v6(a: Ipv6Addr) -> bool {
        if let Some(m) = a.to_ipv4_mapped() {
            return v4(m);
        }
        let first = a.segments()[0];
        a.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
    }
    match host {
        url::Host::Domain(d) => d == "localhost",
        url::Host::Ipv4(a) => v4(a),
        url::Host::Ipv6(a) => v6(a),
    }
}

/// A station's address: https for any host and port, plain http only for this computer and
/// private networks (ADR-0139); a host, no path, query or credentials.
pub fn clean_url(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(url).map_err(|_| {
        "The address starts with http:// or https://, like https://radio.example.org".to_owned()
    })?;
    let ok_scheme = matches!(parsed.scheme(), "http" | "https");
    if !ok_scheme
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(
            "The address is only the station's, like https://radio.example.org, with no path."
                .to_owned(),
        );
    }
    if parsed.scheme() == "http" && !parsed.host().is_some_and(is_nearby) {
        return Err(PLAIN_HTTP_REFUSED.to_owned());
    }
    Ok(url.to_owned())
}

/// A path the page may ask for: under `/api/`, with no way out of it.
pub fn clean_path(path: &str) -> Result<&str, String> {
    let ok = path.starts_with("/api/")
        && !path.contains("..")
        && !path.contains('#')
        && !path.contains('\\')
        && !path.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        Ok(path)
    } else {
        Err(format!("not a station path: {path}"))
    }
}

/// The request body the page sent, as bytes. The page sends raw bytes; over Tauri's custom IPC
/// protocol they arrive as `Raw`, but when the webview falls back to its postMessage IPC (the
/// custom protocol blocked, as the CSP did until 1.0.1's test run) Tauri serialises them as a JSON
/// array of numbers. Sending that array on as JSON made every body a list of numbers, and the
/// station answered 400 to each: no invitation, no handover, nothing with a body.
pub fn request_bytes(body: &InvokeBody) -> Result<Vec<u8>, String> {
    match body {
        InvokeBody::Raw(b) => Ok(b.clone()),
        InvokeBody::Json(serde_json::Value::Null) => Ok(Vec::new()),
        InvokeBody::Json(serde_json::Value::Array(a)) => a
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| "a request body that is not bytes".to_owned())
            })
            .collect(),
        InvokeBody::Json(v) => serde_json::to_vec(v).map_err(|e| e.to_string()),
    }
}

/// Run keychain work on a thread of its own. The keychain crate's Secret Service client
/// (zbus) blocks on a runtime of its own, which panics inside an async command's Tokio worker
/// ("Cannot start a runtime from within a runtime"; found on a real desktop, 2026-11-01).
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().expect("the keychain thread")
}

fn read_info(app: &AppHandle) -> Option<StationInfo> {
    let text = std::fs::read_to_string(info_path(app).ok()?).ok()?;
    // A station saved by a build that accepted plain http anywhere is not used again unless the
    // address passes today's rule (ADR-0139): the key must not cross the internet in clear. The
    // setup opens and asks for the address again.
    serde_json::from_str::<StationInfo>(&text)
        .ok()
        .filter(|i| clean_url(&i.url).is_ok())
}

fn read_key(app: &AppHandle, info: &StationInfo) -> Result<String, String> {
    match info.kept {
        Kept::Keychain => keyring::Entry::new(SERVICE, &info.url)
            .and_then(|e| e.get_password())
            .map_err(|e| format!("the station's key is not in the keychain: {e}")),
        Kept::File => std::fs::read_to_string(key_path(app)?)
            .map(|k| k.trim().to_owned())
            .map_err(|e| format!("the station's key file cannot be read: {e}")),
    }
}

/// Write the key to a file only this user can read.
fn write_key_file(path: &std::path::Path, key: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(key.as_bytes())
}

fn forget_keys(app: &AppHandle) {
    if let Some(old) = read_info(app)
        && let Ok(e) = keyring::Entry::new(SERVICE, &old.url)
    {
        let _ = e.delete_credential();
    }
    if let Ok(p) = key_path(app) {
        let _ = std::fs::remove_file(p);
    }
}

/// The saved station, if any.
#[tauri::command]
pub fn station_info(app: AppHandle) -> Option<StationInfo> {
    read_info(&app)
}

/// Ask a station whether it takes this key, before saving it: the HTTP status it answers.
#[tauri::command]
pub async fn station_check(
    stations: State<'_, Stations>,
    url: String,
    key: String,
) -> Result<u16, String> {
    let url = clean_url(&url)?;
    let res = stations
        .client
        .get(format!("{url}/api/v1/carts"))
        .bearer_auth(key.trim())
        .send()
        .await
        .map_err(|_| "No station answered at that address.".to_owned())?;
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok());
    if login_redirect(res.status().as_u16(), location) {
        return Ok(401);
    }
    Ok(res.status().as_u16())
}

/// Save the station and its key: in the keychain, or where there is none, in a file only this
/// user can read. A key saved before for another station is forgotten.
#[tauri::command]
pub fn station_save(app: AppHandle, url: String, key: String) -> Result<StationInfo, String> {
    let url = clean_url(&url)?;
    save(&app, url, key.trim())
}

/// Keep a station and its bearer (the station key, or a staff session's token).
fn save(app: &AppHandle, url: String, key: &str) -> Result<StationInfo, String> {
    let app = app.clone();
    if key.is_empty() {
        return Err("The key is empty.".to_owned());
    }
    forget_keys(&app);
    std::fs::create_dir_all(dir(&app)?).map_err(|e| e.to_string())?;
    let kept = match keyring::Entry::new(SERVICE, &url).and_then(|e| {
        e.set_password(key)?;
        // Read back: some keyrings accept a write they do not keep.
        e.get_password().map(|k| k == key)
    }) {
        Ok(true) => Kept::Keychain,
        _ => {
            write_key_file(&key_path(&app)?, key)
                .map_err(|e| format!("the key could not be kept: {e}"))?;
            Kept::File
        }
    };
    let info = StationInfo { url, kept };
    std::fs::write(
        info_path(&app)?,
        serde_json::to_string_pretty(&info).expect("station info serialises"),
    )
    .map_err(|e| e.to_string())?;
    let _ = std::fs::write(last_path(&app)?, &info.url);
    Ok(info)
}

fn last_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(dir(app)?.join("last-station.txt"))
}

/// The last station's address, for the sign-in screen after a sign-out.
#[tauri::command]
pub fn station_last(app: AppHandle) -> Option<String> {
    let url = std::fs::read_to_string(last_path(&app).ok()?).ok()?;
    clean_url(&url).ok()
}

/// What a sign-in answers the page: the station, and who signed in. The session's token goes
/// straight into the keychain, as a station key does (ADR-0121); the page never sees it.
#[derive(Serialize)]
pub struct SignedIn {
    station: StationInfo,
    account: serde_json::Value,
    expires_ms: u64,
}

/// Whether an answer is a login proxy's redirect to its login page: the station refused the
/// bearer (401), and the proxy sent the person to sign in there instead.
fn login_redirect(status: u16, location: Option<&str>) -> bool {
    (300..400).contains(&status) && location.is_some_and(|l| l.contains("/login"))
}

/// The station's own sentence from a refusal, or a plain one.
fn refusal_text(status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or_else(|| match status {
            404 | 405 => {
                "That station does not offer staff sign-in yet: use its station key.".to_owned()
            }
            _ => format!("The station answered {status}."),
        })
}

/// One of the station's sign-in routes, asked without a session: the status, the `Location` of
/// a redirect if there is one, and the body.
async fn post_sign_in(
    stations: &Stations,
    url: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<(u16, Option<String>, String), String> {
    let res = stations
        .client
        .post(format!("{url}{path}"))
        // Behind Cloudron's login (proxyAuth with `supportsBearerAuth`), only a request that
        // carries a bearer reaches the station; a sign-in has none yet, so it would be sent to
        // Cloudron's login page. This placeholder gets it through; the route ignores it.
        .bearer_auth("sign-in")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|_| "No station answered at that address.".to_owned())?;
    let status = res.status().as_u16();
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = res.text().await.unwrap_or_default();
    Ok((status, location, body))
}

/// A station's answer to a sign-in, kept: the session's token goes into the keychain where a
/// station key would, and the page gets the station and who signed in. `refused` is the words
/// for a login proxy's redirect, which is the station's refusal under another name.
async fn keep_sign_in(
    app: &AppHandle,
    url: String,
    (status, location, body): (u16, Option<String>, String),
    refused: &str,
) -> Result<SignedIn, String> {
    if login_redirect(status, location.as_deref()) {
        return Err(refused.to_owned());
    }
    if status != 201 {
        return Err(refusal_text(status, &body));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "The station's answer was not understood.")?;
    let token = v
        .get("token")
        .and_then(|t| t.as_str())
        .filter(|t| t.starts_with("rss_"))
        .ok_or("The station's answer had no sign-in in it.")?;
    let token = token.to_owned();
    let handle = app.clone();
    let station = off_runtime(move || save(&handle, url, &token))?;
    Ok(SignedIn {
        station,
        account: v.get("account").cloned().unwrap_or_default(),
        expires_ms: v.get("expires_ms").and_then(|e| e.as_u64()).unwrap_or(0),
    })
}

/// Sign in to a station as a member of staff (ADR-0127): a username and password for a
/// session token, kept where a station key would be.
#[tauri::command]
pub async fn station_sign_in(
    app: AppHandle,
    stations: State<'_, Stations>,
    url: String,
    username: String,
    password: String,
) -> Result<SignedIn, String> {
    let url = clean_url(&url)?;
    let answer = post_sign_in(
        &stations,
        &url,
        "/api/v1/sessions",
        serde_json::json!({ "username": username.trim(), "password": password }),
    )
    .await?;
    keep_sign_in(
        &app,
        url,
        answer,
        "That username and password do not match an account here.",
    )
    .await
}

/// Ask a station to email a sign-in code (ADR-0127). The station answers the same whether or
/// not the username and address name an account, so this says only that it was asked.
#[tauri::command]
pub async fn station_code_ask(
    stations: State<'_, Stations>,
    url: String,
    username: String,
    email: String,
) -> Result<(), String> {
    let url = clean_url(&url)?;
    let (status, location, body) = post_sign_in(
        &stations,
        &url,
        "/api/v1/sessions/code",
        serde_json::json!({ "username": username.trim(), "email": email.trim() }),
    )
    .await?;
    if status == 202 {
        return Ok(());
    }
    if login_redirect(status, location.as_deref()) {
        return Err("The station refused that. Check the address and try again.".to_owned());
    }
    Err(refusal_text(status, &body))
}

/// Sign in with the code the station emailed: a session token, kept as a password's is.
#[tauri::command]
pub async fn station_code_finish(
    app: AppHandle,
    stations: State<'_, Stations>,
    url: String,
    username: String,
    email: String,
    code: String,
) -> Result<SignedIn, String> {
    let url = clean_url(&url)?;
    let answer = post_sign_in(
        &stations,
        &url,
        "/api/v1/sessions/code/finish",
        serde_json::json!({ "username": username.trim(), "email": email.trim(), "code": code.trim() }),
    )
    .await?;
    keep_sign_in(
        &app,
        url,
        answer,
        "That code does not match, or has run out. Ask for a new one.",
    )
    .await
}

/// Sign out: the station ends the session, and this computer forgets it. The address is kept
/// for the next sign-in. A station key is only forgotten here (it has no session to end).
#[tauri::command]
pub async fn station_sign_out(app: AppHandle, stations: State<'_, Stations>) -> Result<(), String> {
    let handle = app.clone();
    let key = off_runtime(move || read_info(&handle).map(|info| (read_key(&handle, &info), info)));
    if let Some((Ok(key), info)) = key
        && key.starts_with("rss_")
    {
        // Best effort: an unreachable station ends the session itself when it runs out.
        let _ = stations
            .client
            .delete(format!("{}/api/v1/sessions/me", info.url))
            .bearer_auth(key)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
    }
    let handle = app.clone();
    off_runtime(move || {
        forget_keys(&handle);
        if let Ok(p) = info_path(&handle) {
            let _ = std::fs::remove_file(p);
        }
    });
    Ok(())
}

/// Forget the station and its key.
#[tauri::command]
pub fn station_forget(app: AppHandle) -> Result<(), String> {
    forget_keys(&app);
    if let Ok(p) = info_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// The start of an answer; its body follows on the channel: raw bytes as they arrive, then
/// `{"end":true}` or `{"error":"..."}`.
#[derive(Serialize)]
pub struct Head {
    id: u32,
    status: u16,
    status_text: String,
    content_type: Option<String>,
}

fn header<'a>(req: &'a Request<'_>, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

/// A request to the saved station, with its key. Named in headers: `x-station-method`,
/// `x-station-path`, `x-station-channel`, and `x-station-content-type` when there is a body;
/// the body is the invoke's raw bytes.
#[tauri::command]
pub async fn station_fetch(
    app: AppHandle,
    webview: Webview,
    stations: State<'_, Stations>,
    request: Request<'_>,
) -> Result<Head, String> {
    let info = read_info(&app).ok_or("no station is set up")?;
    let key = {
        let (handle, info) = (app.clone(), info.clone());
        off_runtime(move || read_key(&handle, &info))?
    };
    let method = header(&request, "x-station-method").unwrap_or("GET");
    let method = reqwest::Method::from_str(&method.to_ascii_uppercase())
        .map_err(|_| format!("not a method: {method}"))?;
    let path = clean_path(header(&request, "x-station-path").unwrap_or(""))?;
    let channel: Channel<InvokeResponseBody> = JavaScriptChannelId::from_str(
        header(&request, "x-station-channel").ok_or("no channel for the answer")?,
    )
    .map_err(|e| e.to_owned())?
    .channel_on(webview);
    let body = request_bytes(request.body())?;
    let mut out = stations
        .client
        .request(method, format!("{}{path}", info.url))
        .bearer_auth(key);
    if let Some(ct) = header(&request, "x-station-content-type") {
        out = out.header(reqwest::header::CONTENT_TYPE, ct);
    }
    if !body.is_empty() {
        out = out.body(body);
    }
    let mut res = out.send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() {
            "The station did not answer.".to_owned()
        } else {
            e.to_string()
        }
    })?;
    let id = stations.next.fetch_add(1, Ordering::Relaxed);
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok());
    if login_redirect(res.status().as_u16(), location) {
        // The station refused the bearer and a login proxy redirected: answer the page the 401
        // the station gave, in the station's words, so it offers Sign in again.
        let body = br#"{"error":"Your sign-in has ended. Sign in again."}"#.to_vec();
        let _ = channel.send(InvokeResponseBody::Raw(body));
        let _ = channel.send(InvokeResponseBody::Json(r#"{"end":true}"#.to_owned()));
        return Ok(Head {
            id,
            status: 401,
            status_text: "Unauthorized".to_owned(),
            content_type: Some("application/json".to_owned()),
        });
    }
    let head = Head {
        id,
        status: res.status().as_u16(),
        status_text: res.status().canonical_reason().unwrap_or("").to_owned(),
        content_type: res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    };
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let end = loop {
            match res.chunk().await {
                Ok(Some(bytes)) if bytes.is_empty() => continue,
                Ok(Some(bytes)) => {
                    if channel
                        .send(InvokeResponseBody::Raw(bytes.to_vec()))
                        .is_err()
                    {
                        break None;
                    }
                }
                Ok(None) => break Some(r#"{"end":true}"#.to_owned()),
                Err(e) => {
                    break Some(serde_json::json!({ "error": e.to_string() }).to_string());
                }
            }
        };
        if let Some(end) = end {
            let _ = channel.send(InvokeResponseBody::Json(end));
        }
        if let Some(s) = handle.try_state::<Stations>() {
            s.open.lock().expect("open requests").remove(&id);
        }
    });
    stations
        .open
        .lock()
        .expect("open requests")
        .insert(id, task);
    Ok(head)
}

/// The facts of the studio's view that decide whether the app may restart (ADR-0140): the same
/// ones the page's `updateGate` reads (`apps/studio/src/lib/update.ts`). Anything else in the
/// view is ignored, and a field a station leaves out reads as nothing open.
#[derive(Deserialize, Default)]
struct DeskFacts {
    #[serde(default)]
    desk: DeskLink,
    #[serde(default)]
    sources: Vec<StripFacts>,
}

#[derive(Deserialize, Default)]
struct DeskLink {
    #[serde(default)]
    reachable: bool,
    #[serde(default)]
    playout: PlayoutLink,
}

#[derive(Deserialize, Default)]
struct PlayoutLink {
    #[serde(default)]
    connected: bool,
}

#[derive(Deserialize)]
struct StripFacts {
    #[serde(default)]
    on_air: bool,
    #[serde(default)]
    mute: bool,
}

/// Why a restart must wait, from the body of `GET /api/v1/studio`; None when nothing is open
/// or the body is not a view. The words are the page's (`updateGate`).
fn strips_open(body: &[u8]) -> Option<String> {
    let facts: DeskFacts = serde_json::from_slice(body).ok()?;
    let open = facts.sources.iter().filter(|s| s.on_air && !s.mute).count();
    if open == 0 {
        return None;
    }
    let live = facts.desk.reachable && facts.desk.playout.connected;
    let what = if open == 1 {
        "1 strip is open on the desk".to_owned()
    } else {
        format!("{open} strips are open on the desk")
    };
    Some(format!(
        "{}: {what}. Updating waits until nothing is open.",
        if live {
            "The station is on air"
        } else {
            "The room is open"
        }
    ))
}

/// Ask the saved station, now, whether a strip is open on the desk (ADR-0140). The update
/// commands ask this before an install or a rollback restarts the app, because the page's view
/// can be stale or wrong. When the desk's state cannot be had (no station set up, no answer
/// within five seconds, a refusal) the page's rule applies: it cannot tell, it has said so, and
/// the person may go ahead. A held talk key is the page's to know; it is not checked here.
pub(crate) async fn desk_busy(app: &AppHandle, stations: &Stations) -> Option<String> {
    let info = read_info(app)?;
    let key = {
        let (handle, info) = (app.clone(), info.clone());
        off_runtime(move || read_key(&handle, &info)).ok()?
    };
    let res = stations
        .client
        .get(format!("{}/api/v1/studio", info.url))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    strips_open(&res.bytes().await.ok()?)
}

/// Stop reading an answer (the page aborted it).
#[tauri::command]
pub fn station_stop(stations: State<'_, Stations>, id: u32) {
    if let Some(t) = stations.open.lock().expect("open requests").remove(&id) {
        t.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_that_came_by_post_message_is_sent_as_the_bytes_it_was() {
        let json = br#"{"role":"guest","name":"Nessie"}"#;
        let numbers = serde_json::Value::Array(json.iter().map(|b| (*b).into()).collect());
        assert_eq!(
            request_bytes(&InvokeBody::Json(numbers)).unwrap(),
            json.to_vec()
        );
        assert_eq!(
            request_bytes(&InvokeBody::Raw(json.to_vec())).unwrap(),
            json.to_vec()
        );
        assert!(
            request_bytes(&InvokeBody::Json(serde_json::Value::Null))
                .unwrap()
                .is_empty()
        );
        assert!(request_bytes(&InvokeBody::Json(serde_json::json!([1, 256]))).is_err());
        assert!(request_bytes(&InvokeBody::Json(serde_json::json!([1, "a"]))).is_err());
    }

    #[test]
    fn an_open_strip_stops_a_restart_and_a_muted_or_closed_one_does_not() {
        let view = |sources: &str, playout: bool| {
            format!(
                r#"{{"station":"x","desk":{{"reachable":true,"playout":{{"connected":{playout}}}}},"sources":{sources},"callers":[]}}"#
            )
        };
        let one = view(r#"[{"slot":1,"on_air":true,"mute":false}]"#, true);
        assert_eq!(
            strips_open(one.as_bytes()).as_deref(),
            Some(
                "The station is on air: 1 strip is open on the desk. Updating waits until nothing is open."
            )
        );
        let two = view(
            r#"[{"on_air":true,"mute":false},{"on_air":true,"mute":false},{"on_air":true,"mute":true}]"#,
            false,
        );
        assert_eq!(
            strips_open(two.as_bytes()).as_deref(),
            Some(
                "The room is open: 2 strips are open on the desk. Updating waits until nothing is open."
            )
        );
        let quiet = view(
            r#"[{"on_air":true,"mute":true},{"on_air":false,"mute":false}]"#,
            true,
        );
        assert_eq!(strips_open(quiet.as_bytes()), None);
        assert_eq!(strips_open(view("[]", true).as_bytes()), None);
        // Not a view (a login page, an error): the desk cannot be seen, as the page's rule says.
        assert_eq!(strips_open(b"<html>sign in</html>"), None);
    }

    #[test]
    fn a_station_address_is_only_an_origin() {
        assert_eq!(
            clean_url(" https://radio.example.org/ ").unwrap(),
            "https://radio.example.org"
        );
        assert_eq!(
            clean_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
        // https: any host, any port.
        for ok in [
            "https://radio.example.org:8443",
            "https://203.0.113.9",
            "https://[2001:db8::1]:9443",
            "https://100.64.0.1",
        ] {
            assert_eq!(clean_url(ok).unwrap(), ok);
        }
        for bad in [
            "radio.example.org",
            "ftp://radio.example.org",
            "https://radio.example.org/studio",
            "https://user:pw@radio.example.org",
            "https://radio.example.org?x=1",
            "https://",
        ] {
            assert!(clean_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_page_names_only_a_path_under_api() {
        assert!(clean_path("/api/v1/studio/events").is_ok());
        assert!(clean_path("/api/v1/cards/preview?output=link").is_ok());
        for bad in [
            "",
            "/health",
            "https://elsewhere.example/api/v1/x",
            "//elsewhere.example/api/v1",
            "/api/../admin",
            "/api/v1/x y",
            "/api/v1/\\x",
        ] {
            assert!(clean_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_login_proxys_redirect_is_read_as_the_refusal_it_was() {
        assert!(login_redirect(
            302,
            Some("https://radio.example.org/login?redirect=/api/v1/studio")
        ));
        assert!(login_redirect(307, Some("/login")));
        assert!(!login_redirect(
            302,
            Some("https://radio.example.org/elsewhere")
        ));
        assert!(!login_redirect(302, None));
        assert!(!login_redirect(401, Some("/login")));
    }

    #[test]
    fn a_refusal_reads_as_the_station_wrote_it() {
        assert_eq!(
            refusal_text(
                401,
                r#"{"error":"That username and password do not match an account here."}"#
            ),
            "That username and password do not match an account here."
        );
        assert!(refusal_text(404, "").contains("station key"));
        assert_eq!(refusal_text(502, "<html>"), "The station answered 502.");
    }

    #[test]
    fn a_key_file_is_readable_by_its_owner_only() {
        let dir = std::env::temp_dir().join(format!("weavesignal-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("station.key");
        write_key_file(&p, "secret-key").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "secret-key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn plain_http_is_for_this_computer_and_private_networks() {
        for ok in [
            // This computer.
            "http://localhost:8080",
            "http://LOCALHOST",
            "http://127.0.0.1:8080",
            "http://127.5.5.5",
            "http://[::1]:8080",
            // RFC 1918, both ends of each block.
            "http://10.0.0.1",
            "http://10.255.255.255",
            "http://172.16.0.1",
            "http://172.31.255.254",
            "http://192.168.1.20:8080",
            // Link-local.
            "http://169.254.10.10",
            "http://[fe80::1]",
            "http://[febf::1]",
            // RFC 4193 unique local.
            "http://[fc00::1]",
            "http://[fd12:3456:789a::1]:8080",
            // Carrier-grade NAT, which Tailscale uses (both ends).
            "http://100.64.0.1",
            "http://100.101.102.103:8080",
            "http://100.127.255.254",
            // An IPv4 address inside IPv6.
            "http://[::ffff:192.168.1.5]",
        ] {
            assert_eq!(clean_url(ok).unwrap(), ok, "{ok}");
        }
    }

    #[test]
    fn plain_http_is_refused_beyond_the_neighbourhood() {
        for bad in [
            // Public addresses, and the edges of the private blocks.
            "http://203.0.113.9",
            "http://8.8.8.8:8080",
            "http://11.0.0.1",
            "http://172.15.255.255",
            "http://172.32.0.1",
            "http://192.167.1.1",
            "http://169.253.1.1",
            "http://100.63.255.255",
            "http://100.128.0.1",
            "http://[2001:db8::1]",
            "http://[fe00::1]",
            "http://[fec0::1]",
            "http://[::ffff:8.8.8.8]",
            // Names are never nearby, whatever they resolve to (no lookups).
            "http://radio.example.org",
            "http://radio.local",
            "http://localhost.example.org",
            "http://localhost.",
            "http://printer",
            "http://127.0.0.1.example.org",
        ] {
            let e = clean_url(bad).unwrap_err();
            assert!(e.contains("Plain http://"), "{bad}: {e}");
            // The same host over https is fine.
            let secure = bad.replacen("http://", "https://", 1);
            assert_eq!(clean_url(&secure).unwrap(), secure, "{secure}");
        }
    }
}
