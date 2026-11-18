// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The ingest's supervisor (`weavesignal-ingest run`): the two public listeners, Icecast (TCP)
//! and SRT (UDP), each on the port the station's rules give and only while a show key exists.
//!
//! For each connection it reads only what the protocol needs to name the key (the HTTP head, or
//! SRT's stream id), asks the control plane (`/api/v1/internal/sources/admit`) before any media
//! is read, and then:
//!
//! - holds **one connection per key**: an older one that is still sending wins; one that has
//!   sent nothing for 2 s gives way (design section 9);
//! - copies the media into a **decoding worker** of its own ([`crate::decode`]), counting it
//!   against **2 Mbit/s** (over ten seconds, with a quarter's grace for bursts);
//! - for a source **standing by**, hands the worker's 48 kHz float to the desk over loopback
//!   ([`weavesignal_proto::source_wire`]), connecting again if the desk restarts; a **test**
//!   never reaches the desk, ends after its time, and is asked about again every ten seconds so
//!   that one still open when its slot's door opens stands by without reconnecting, and a
//!   source standing by (not on air) when its slot's door closes becomes a test the same way,
//!   its time running from then, when the control plane's reply to a report says so;
//! - reports every connection to the control plane every second, and closes those the reply
//!   names (a key ended, a source disconnected at the desk).
//!
//! A worker that dies ends its own source only. The supervisor itself parses no media.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use srt_protocol::options::{KeySize, Passphrase};
use srt_protocol::packet::{RejectReason, ServerRejectReason};
use srt_protocol::settings::KeySettings;
use srt_tokio::SrtListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use weavesignal_proto::source_wire as wire;

use crate::{frame, icecast};

/// The most bytes a source may send over [`RATE_WINDOW`]: 2 Mbit/s, with a quarter's grace.
const MAX_BYTES_PER_WINDOW: usize = 2_000_000 / 8 * 10 * 5 / 4;
const RATE_WINDOW: Duration = Duration::from_secs(10);
/// An older connection that has sent nothing for this long gives way to a newer one.
const GIVES_WAY: Duration = Duration::from_secs(2);
/// A connection that sends nothing for this long is closed.
const SILENT_END: Duration = Duration::from_secs(10);
/// How many connections the ingest holds at once.
const MAX_CONNECTIONS: usize = 32;
/// What the log says when a standing-by source becomes a test as its slot's door closes (the
/// control plane's `DOOR_CLOSED_DETAIL`, which the ingest does not link).
const DOOR_CLOSED_DETAIL: &str = "the slot's door closed";
/// A test is asked about again this often.
const RECHECK: Duration = Duration::from_secs(10);

/// Where the supervisor finds the others.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The control plane, on loopback: `http://127.0.0.1:7310`.
    pub control: String,
    /// The bearer for the control plane's internal routes.
    pub token: String,
    /// The desk's sources listener.
    pub desk: SocketAddr,
    pub desk_password: String,
    /// Where the two public listeners bind.
    pub bind: IpAddr,
    /// This program, run again as the worker.
    pub exe: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// ---- the control plane ----

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Config {
    listen: bool,
    port_icecast: u16,
    port_srt: u16,
    test_max_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct Admit {
    decision: String,
    reason: Option<String>,
    message: Option<String>,
    identity: Option<String>,
    name: Option<String>,
    show: Option<String>,
    passphrase: Option<String>,
    max_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
struct Reported {
    key_id: String,
    mode: &'static str,
    way: &'static str,
    format: String,
    since_ms: u64,
    kbps: u32,
    level_db: Option<f32>,
    title: Option<String>,
}

#[derive(Clone)]
struct Control {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl Control {
    fn new(base: &str, token: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(4)))
            .build()
            .into();
        Control {
            base: base.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            agent,
        }
    }

    async fn call<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        path: &'static str,
        body: Option<serde_json::Value>,
    ) -> Result<Option<T>> {
        let c = self.clone();
        tokio::task::spawn_blocking(move || {
            let url = format!("{}/api/v1/internal/sources/{path}", c.base);
            let auth = format!("Bearer {}", c.token);
            let mut res = match body {
                Some(b) => c
                    .agent
                    .post(&url)
                    .header("Authorization", &auth)
                    .send_json(&b),
                None => c.agent.get(&url).header("Authorization", &auth).call(),
            }
            .with_context(|| format!("the control plane's {path}"))?;
            if res.status() == 204 {
                return Ok(None);
            }
            Ok(Some(res.body_mut().read_json::<T>()?))
        })
        .await?
    }

    async fn config(&self) -> Result<Config> {
        self.call("config", None)
            .await?
            .context("the control plane sent no settings")
    }

    async fn admit(
        &self,
        key_id: &str,
        secret: Option<&str>,
        way: &str,
        addr: &str,
        again: bool,
    ) -> Result<Admit> {
        let mut body = json!({"key_id": key_id, "way": way, "address": addr, "again": again});
        if let Some(s) = secret {
            body["secret"] = json!(s);
        }
        self.call("admit", Some(body))
            .await?
            .context("the control plane sent no decision")
    }

    /// Log what happened; failures are only logged here.
    async fn event(
        &self,
        key_id: &str,
        event: &str,
        reason: Option<&str>,
        detail: &str,
        way: &str,
        addr: Option<&str>,
    ) {
        let body = json!({"key_id": key_id, "event": event, "reason": reason, "detail": detail, "way": way, "address": addr});
        if let Err(e) = self.call::<serde_json::Value>("events", Some(body)).await {
            tracing::warn!(error = %format!("{e:#}"), "cannot tell the control plane what happened");
        }
    }
}

// ---- connections ----

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Standby,
    Test { until: Instant },
}

struct Conn {
    id: u64,
    way: &'static str,
    mode: Mode,
    format: String,
    since_ms: u64,
    title: Option<String>,
    /// The Icecast password's secret, to check a metadata update against.
    secret: Option<String>,
    last_bytes: Instant,
    /// (when, how many) over the rate window.
    arrivals: VecDeque<(Instant, usize)>,
    level_db: Option<f32>,
    stop: Arc<Notify>,
    /// Why it was stopped, if from outside: logged with the end.
    stopped_for: Option<&'static str>,
    /// Asked by the control plane to become a private test (its slot's door closed): for how
    /// long, in ms. `demote` wakes the connection's loop to do it.
    demote_ms: Option<u64>,
    demote: Arc<Notify>,
}

impl Conn {
    fn kbps(&self) -> u32 {
        let since = Instant::now() - Duration::from_secs(5);
        let bytes: usize = self
            .arrivals
            .iter()
            .filter(|(t, _)| *t >= since)
            .map(|(_, n)| n)
            .sum();
        (bytes * 8 / 5 / 1000) as u32
    }
}

struct Shared {
    settings: Settings,
    control: Control,
    conns: Mutex<HashMap<String, Conn>>,
    next: AtomicU64,
    test_max: Mutex<Duration>,
}

type Ingest = Arc<Shared>;

/// Run until the process is stopped.
pub async fn run(settings: Settings) -> Result<()> {
    let control = Control::new(&settings.control, &settings.token);
    let shared: Ingest = Arc::new(Shared {
        settings,
        control,
        conns: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        test_max: Mutex::new(Duration::from_secs(15 * 60)),
    });
    tokio::spawn(report_loop(shared.clone()));
    let mut listening: Option<(Config, Vec<JoinHandle<()>>)> = None;
    let mut warned = false;
    loop {
        match shared.control.config().await {
            Ok(cfg) => {
                warned = false;
                *shared.test_max.lock().expect("ingest lock") =
                    Duration::from_millis(cfg.test_max_ms);
                let same = listening.as_ref().is_some_and(|(c, _)| {
                    c.listen == cfg.listen
                        && c.port_icecast == cfg.port_icecast
                        && c.port_srt == cfg.port_srt
                });
                if !same {
                    if let Some((_, tasks)) = listening.take() {
                        for t in tasks {
                            t.abort();
                        }
                        tracing::info!("stopped listening for hosts' own studios");
                    }
                    let tasks = if cfg.listen {
                        // A port of 0 is a way the installation turned off (a port Cloudron's
                        // dashboard left disabled): nothing listens for it.
                        let mut tasks = Vec::new();
                        if cfg.port_icecast != 0 {
                            let ice = SocketAddr::new(shared.settings.bind, cfg.port_icecast);
                            tracing::info!(%ice, "listening for Icecast sources from hosts' own studios (ADR-0280)");
                            tasks.push(tokio::spawn(icecast_listener(shared.clone(), ice)));
                        }
                        if cfg.port_srt != 0 {
                            let srt = SocketAddr::new(shared.settings.bind, cfg.port_srt);
                            tracing::info!(%srt, "listening for SRT sources from hosts' own studios (ADR-0280)");
                            tasks.push(tokio::spawn(srt_listener(shared.clone(), srt)));
                        }
                        if tasks.is_empty() {
                            tracing::warn!(
                                "a show key exists, but both ways in are off (their ports are 0)"
                            );
                        }
                        tasks
                    } else {
                        tracing::info!("no show key yet: not listening");
                        Vec::new()
                    };
                    listening = Some((cfg, tasks));
                }
            }
            Err(e) => {
                if !warned {
                    tracing::warn!(error = %format!("{e:#}"), "cannot read the ingest's settings; trying again every 5 s");
                    warned = true;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn report_loop(shared: Ingest) {
    let mut warned = false;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let report: Vec<Reported> = {
            let conns = shared.conns.lock().expect("ingest lock");
            conns
                .iter()
                .map(|(k, c)| Reported {
                    key_id: k.clone(),
                    mode: if c.mode == Mode::Standby {
                        "standby"
                    } else {
                        "test"
                    },
                    way: c.way,
                    format: c.format.clone(),
                    since_ms: c.since_ms,
                    kbps: c.kbps(),
                    level_db: c.level_db,
                    title: c.title.clone(),
                })
                .collect()
        };
        #[derive(Deserialize)]
        struct Reply {
            close: Vec<String>,
            /// Keys standing by whose slot's door has closed and that are not on air.
            #[serde(default)]
            to_test: Vec<ToTest>,
        }
        #[derive(Deserialize)]
        struct ToTest {
            key_id: String,
            max_ms: u64,
        }
        match shared
            .control
            .call::<Reply>("report", Some(json!({ "connections": report })))
            .await
        {
            Ok(Some(r)) => {
                warned = false;
                let conns = shared.conns.lock().expect("ingest lock");
                for k in r.close {
                    if let Some(c) = conns.get(&k) {
                        c.stop.notify_one();
                    }
                }
                drop(conns);
                for t in r.to_test {
                    mark(&shared, &t.key_id, |c| {
                        if c.mode == Mode::Standby && c.demote_ms.is_none() {
                            c.demote_ms = Some(t.max_ms);
                            c.demote.notify_one();
                        }
                    });
                }
            }
            Ok(None) => {}
            Err(e) => {
                if !warned {
                    tracing::warn!(error = %format!("{e:#}"), "cannot report to the control plane");
                    warned = true;
                }
            }
        }
    }
}

/// Whether a newer connection for `key_id` may take over; asks an older silent one to go.
fn claim(shared: &Ingest, key_id: &str) -> bool {
    let conns = shared.conns.lock().expect("ingest lock");
    match conns.get(key_id) {
        Some(c) if c.last_bytes.elapsed() < GIVES_WAY => false,
        Some(c) => {
            c.stop.notify_one();
            true
        }
        None => true,
    }
}

fn full(shared: &Ingest) -> bool {
    shared.conns.lock().expect("ingest lock").len() >= MAX_CONNECTIONS
}

// ---- Icecast ----

async fn icecast_listener(shared: Ingest, addr: SocketAddr) {
    let listener = loop {
        match TcpListener::bind(addr).await {
            Ok(l) => break l,
            Err(e) => {
                tracing::error!(%addr, error = %e, "cannot listen for Icecast sources; trying again in 10 s");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    };
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let s = shared.clone();
                tokio::spawn(async move {
                    if let Err(e) = icecast_conn(s, stream, peer).await {
                        tracing::info!(%peer, error = %format!("{e:#}"), "an Icecast connection ended");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "Icecast listener");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// Read a head: everything up to the blank line, and what came after it.
async fn read_head(stream: &mut TcpStream) -> Result<(String, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut piece = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut piece))
            .await
            .context("no request within 10 s")??;
        if n == 0 {
            bail!("closed before its request");
        }
        buf.extend_from_slice(&piece[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((String::from_utf8_lossy(&buf).into_owned(), rest));
        }
        if buf.len() > icecast::MAX_HEAD {
            bail!("a head over {} bytes", icecast::MAX_HEAD);
        }
    }
}

async fn icecast_conn(shared: Ingest, mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let addr = peer.ip().to_string();
    let (head, rest) = read_head(&mut stream).await?;
    let request = match icecast::parse(&head) {
        Ok(r) => r,
        Err(icecast::Bad::Method) => {
            stream
                .write_all(
                    icecast::refusal(405, "This port takes Icecast sources only.").as_bytes(),
                )
                .await?;
            return Ok(());
        }
        Err(icecast::Bad::Credentials) => {
            shared
                .control
                .admit("", None, "icecast", &addr, false)
                .await
                .ok();
            stream
                .write_all(icecast::refusal(401, "The key is wrong.").as_bytes())
                .await?;
            return Ok(());
        }
    };
    let (credentials, format) = match request {
        icecast::Request::Metadata { credentials, title } => {
            let ok = {
                let mut conns = shared.conns.lock().expect("ingest lock");
                match conns.get_mut(&credentials.key_id) {
                    Some(c) if c.secret.as_deref() == Some(credentials.secret.as_str()) => {
                        c.title = (!title.is_empty()).then_some(title);
                        true
                    }
                    _ => false,
                }
            };
            let answer = if ok {
                "HTTP/1.0 200 OK\r\nContent-Type: text/xml\r\n\r\n<?xml version=\"1.0\"?>\n<iceresponse><message>Metadata update successful</message><return>1</return></iceresponse>\n".to_owned()
            } else {
                icecast::refusal(401, "No source with that key is connected.")
            };
            stream.write_all(answer.as_bytes()).await?;
            return Ok(());
        }
        icecast::Request::Source {
            credentials,
            format,
            ..
        } => (credentials, format),
    };
    if full(&shared) {
        stream
            .write_all(
                icecast::refusal(503, "The station is busy. Try again in a minute.").as_bytes(),
            )
            .await?;
        return Ok(());
    }
    let key_id = credentials.key_id.clone();
    let admitted = shared
        .control
        .admit(&key_id, Some(&credentials.secret), "icecast", &addr, false)
        .await?;
    if admitted.decision == "refused" {
        let status = match admitted.reason.as_deref() {
            Some("wrong_key" | "too_many") => 401,
            _ => 403,
        };
        let why = admitted.message.unwrap_or_default();
        stream
            .write_all(icecast::refusal(status, &why).as_bytes())
            .await?;
        return Ok(());
    }
    let format = match format {
        Ok(f) => f,
        Err(what) => {
            let why = format!(
                "The station cannot read what your software sends ({what}). Set it to MP3, AAC or Opus."
            );
            shared
                .control
                .event(
                    &key_id,
                    "refused",
                    Some("format"),
                    &what,
                    "icecast",
                    Some(&addr),
                )
                .await;
            stream
                .write_all(icecast::refusal(415, &why).as_bytes())
                .await?;
            return Ok(());
        }
    };
    if !claim(&shared, &key_id) {
        shared
            .control
            .event(
                &key_id,
                "refused",
                Some("in_use"),
                "",
                "icecast",
                Some(&addr),
            )
            .await;
        stream
            .write_all(
                icecast::refusal(
                    403,
                    "Another connection is using this key. Close your other software first.",
                )
                .as_bytes(),
            )
            .await?;
        return Ok(());
    }
    stream
        .write_all(b"HTTP/1.0 200 OK\r\nServer: weavesignal-ingest\r\n\r\n")
        .await?;
    let (tx, rx) = mpsc::channel::<Bytes>(64);
    let reader = tokio::spawn(async move {
        if !rest.is_empty() && tx.send(Bytes::from(rest)).await.is_err() {
            return;
        }
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match tokio::time::timeout(SILENT_END, stream.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
                Ok(Ok(n)) => {
                    if tx.send(Bytes::copy_from_slice(&buf[..n])).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    session(
        shared,
        key_id,
        "icecast",
        addr,
        admitted,
        format,
        rx,
        Some(credentials.secret),
    )
    .await;
    reader.abort();
    Ok(())
}

// ---- SRT ----

/// The key id in an SRT stream id: as it is, or `r=` of the access-control form (`#!::r=…`).
pub fn key_from_stream_id(sid: &str) -> String {
    let sid = sid.trim();
    if let Some(rest) = sid.strip_prefix("#!::") {
        return rest
            .split(',')
            .find_map(|kv| kv.strip_prefix("r="))
            .unwrap_or_default()
            .to_owned();
    }
    sid.to_owned()
}

async fn srt_listener(shared: Ingest, addr: SocketAddr) {
    let (_listener, mut incoming) = loop {
        match SrtListener::builder().bind(addr).await {
            Ok(l) => break l,
            Err(e) => {
                tracing::error!(%addr, error = %e, "cannot listen for SRT sources; trying again in 10 s");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    };
    while let Some(request) = incoming.incoming().next().await {
        let s = shared.clone();
        tokio::spawn(async move {
            if let Err(e) = srt_conn(s, request).await {
                tracing::info!(error = %format!("{e:#}"), "an SRT connection ended");
            }
        });
    }
}

async fn srt_conn(shared: Ingest, request: srt_tokio::ConnectionRequest) -> Result<()> {
    let addr = request.remote().ip().to_string();
    let key_id = key_from_stream_id(
        &request
            .stream_id()
            .map(|s| s.to_string())
            .unwrap_or_default(),
    );
    let reject = |r: ServerRejectReason| RejectReason::Server(r);
    if full(&shared) {
        request.reject(reject(ServerRejectReason::Overload)).await?;
        return Ok(());
    }
    // Two phases (the build's recce): the key id is public, so nothing is claimed until the
    // handshake has proved the passphrase.
    let admitted = shared
        .control
        .admit(&key_id, None, "srt", &addr, false)
        .await?;
    if admitted.decision == "refused" {
        let r = match admitted.reason.as_deref() {
            Some("wrong_key" | "too_many") => ServerRejectReason::Unauthorized,
            Some("in_use") => ServerRejectReason::Conflict,
            _ => ServerRejectReason::Forbidden,
        };
        request.reject(reject(r)).await?;
        return Ok(());
    }
    if request.key_size() == KeySize::Unspecified {
        shared
            .control
            .event(
                &key_id,
                "refused",
                Some("wrong_key"),
                "no passphrase",
                "srt",
                Some(&addr),
            )
            .await;
        request
            .reject(reject(ServerRejectReason::Unauthorized))
            .await?;
        return Ok(());
    }
    let Some(pass) = admitted
        .passphrase
        .clone()
        .and_then(|p| Passphrase::try_from(p).ok())
    else {
        request
            .reject(reject(ServerRejectReason::InternalServerError))
            .await?;
        bail!("the control plane gave no usable passphrase");
    };
    let keys = KeySettings {
        key_size: request.key_size(),
        passphrase: pass,
    };
    let mut socket = match request.accept(Some(keys)).await {
        Ok(s) => s,
        Err(e) => {
            // The handshake failed: almost always the wrong passphrase (libsrt's BADSECRET).
            shared
                .control
                .event(
                    &key_id,
                    "refused",
                    Some("wrong_key"),
                    "the SRT handshake failed",
                    "srt",
                    Some(&addr),
                )
                .await;
            bail!("the handshake failed: {e}");
        }
    };
    if !claim(&shared, &key_id) {
        shared
            .control
            .event(&key_id, "refused", Some("in_use"), "", "srt", Some(&addr))
            .await;
        let _ = socket.close().await;
        return Ok(());
    }
    let (tx, rx) = mpsc::channel::<Bytes>(256);
    let reader = tokio::spawn(async move {
        while let Ok(Some(Ok((_, data)))) = tokio::time::timeout(SILENT_END, socket.next()).await {
            if tx.send(data).await.is_err() {
                break;
            }
        }
        let _ = socket.close().await;
    });
    session(shared, key_id, "srt", addr, admitted, "mpegts", rx, None).await;
    reader.abort();
    Ok(())
}

// ---- one source ----

#[allow(clippy::too_many_arguments)]
async fn session(
    shared: Ingest,
    key_id: String,
    way: &'static str,
    addr: String,
    admitted: Admit,
    format: &'static str,
    mut input: mpsc::Receiver<Bytes>,
    secret: Option<String>,
) {
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(Notify::new());
    let demote = Arc::new(Notify::new());
    let test_until = |a: &Admit| {
        let max = a
            .max_ms
            .map(|m| Duration::from_millis(m.max(0) as u64))
            .unwrap_or_else(|| *shared.test_max.lock().expect("ingest lock"));
        Instant::now() + max
    };
    let mut mode = if admitted.decision == "standby" {
        Mode::Standby
    } else {
        Mode::Test {
            until: test_until(&admitted),
        }
    };
    {
        let mut conns = shared.conns.lock().expect("ingest lock");
        // Two connections with one key can both pass `claim` while each waits on the control
        // plane: the later one stands, and stops what it replaced.
        let replaced = conns.insert(
            key_id.clone(),
            Conn {
                id,
                way,
                mode: mode.clone(),
                format: String::new(),
                since_ms: now_ms(),
                title: None,
                secret: secret.clone(),
                last_bytes: Instant::now(),
                arrivals: VecDeque::new(),
                level_db: None,
                stop: stop.clone(),
                stopped_for: None,
                demote_ms: None,
                demote: demote.clone(),
            },
        );
        if let Some(old) = replaced {
            old.stop.notify_one();
        }
    }
    let event = if mode == Mode::Standby {
        "admitted"
    } else {
        "test"
    };
    shared
        .control
        .event(&key_id, event, None, "", way, Some(&addr))
        .await;
    tracing::info!(key = %key_id, %way, mode = %event, "a host's own studio connected");

    let mut identity = admitted
        .identity
        .clone()
        .unwrap_or_else(|| wire::identity(&key_id));
    let mut name = admitted.name.clone().unwrap_or_default();
    let mut show = admitted.show.clone().unwrap_or_default();

    let ended: &str = match run_worker(
        &shared,
        &key_id,
        format,
        &mut input,
        &stop,
        &demote,
        way,
        &addr,
        &mut mode,
        &mut identity,
        &mut name,
        &mut show,
        secret.as_deref(),
    )
    .await
    {
        Ok(why) => why,
        Err(e) => {
            tracing::warn!(key = %key_id, error = %format!("{e:#}"), "a source's worker failed");
            "the decoder failed"
        }
    };
    let stopped_for = {
        let mut conns = shared.conns.lock().expect("ingest lock");
        match conns.get(&key_id) {
            Some(c) if c.id == id => conns.remove(&key_id).and_then(|c| c.stopped_for),
            _ => None,
        }
    };
    if let Some(r) = stopped_for {
        shared
            .control
            .event(&key_id, "refused", Some(r), ended, way, Some(&addr))
            .await;
    }
    shared
        .control
        .event(&key_id, "ended", None, ended, way, Some(&addr))
        .await;
    tracing::info!(key = %key_id, why = %ended, "a host's own studio ended");
}

fn mark(shared: &Ingest, key_id: &str, f: impl FnOnce(&mut Conn)) {
    if let Some(c) = shared.conns.lock().expect("ingest lock").get_mut(key_id) {
        f(c);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_worker(
    shared: &Ingest,
    key_id: &str,
    format: &'static str,
    input: &mut mpsc::Receiver<Bytes>,
    stop: &Notify,
    demote: &Notify,
    way: &'static str,
    addr: &str,
    mode: &mut Mode,
    identity: &mut String,
    name: &mut String,
    show: &mut String,
    secret: Option<&str>,
) -> Result<&'static str> {
    let mut child = tokio::process::Command::new(&shared.settings.exe)
        .args(["decode", "--format", format])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("cannot start the decoding worker")?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let stderr_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = (&mut stderr).take(4096).read_to_string(&mut s).await;
        s
    });
    // The worker's output, as frames, on a channel.
    let (ftx, mut frames) = mpsc::channel::<(u8, Vec<u8>)>(64);
    let out_task = tokio::spawn(async move {
        loop {
            let mut head = [0u8; 5];
            if stdout.read_exact(&mut head).await.is_err() {
                return;
            }
            let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
            if len > frame::MAX {
                return;
            }
            let mut payload = vec![0u8; len];
            if stdout.read_exact(&mut payload).await.is_err()
                || ftx.send((head[0], payload)).await.is_err()
            {
                return;
            }
        }
    });
    let mut desk: Option<DeskLink> = None;
    let mut format_seen = String::new();
    let (mut sq, mut sn) = (0.0f64, 0usize);
    let mut recheck = tokio::time::interval(RECHECK);
    recheck.tick().await;
    let why = loop {
        let until = match mode {
            Mode::Test { until } => *until,
            Mode::Standby => Instant::now() + Duration::from_secs(86_400),
        };
        tokio::select! {
            _ = stop.notified() => break "closed by the station",
            _ = tokio::time::sleep_until(until.into()) => break "the test's time ran out",
            chunk = input.recv() => {
                let Some(chunk) = chunk else { break "the host's software disconnected" };
                let over = {
                    let mut conns = shared.conns.lock().expect("ingest lock");
                    let Some(c) = conns.get_mut(key_id) else { break "closed by the station" };
                    let now = Instant::now();
                    c.last_bytes = now;
                    c.arrivals.push_back((now, chunk.len()));
                    while c.arrivals.front().is_some_and(|(t, _)| now.duration_since(*t) > RATE_WINDOW) {
                        c.arrivals.pop_front();
                    }
                    let total: usize = c.arrivals.iter().map(|(_, n)| n).sum();
                    if total > MAX_BYTES_PER_WINDOW {
                        c.stopped_for = Some("too_fast");
                        true
                    } else {
                        false
                    }
                };
                if over {
                    break "more than 2 Mbit/s";
                }
                if stdin.write_all(&chunk).await.is_err() {
                    break "the decoder stopped";
                }
            }
            f = frames.recv() => {
                let Some((kind, payload)) = f else { break "the decoder stopped" };
                match kind {
                    frame::INFO => {
                        format_seen = wire::clean(&String::from_utf8_lossy(&payload));
                        let shown = format_seen.clone();
                        mark(shared, key_id, |c| c.format = shown);
                        if *mode == Mode::Standby && desk.is_none() {
                            desk = Some(DeskLink::open(shared, identity, name, way, &format_seen, key_id, show));
                        }
                    }
                    frame::PCM => {
                        for b in payload.as_chunks::<4>().0 {
                            let v = f64::from(f32::from_le_bytes(*b));
                            sq += v * v;
                        }
                        sn += payload.len() / 4;
                        if sn >= 96_000 {
                            let ms = sq / sn as f64;
                            let level = (ms > 1e-10).then(|| (10.0 * ms.log10() * 10.0).round() as f32 / 10.0);
                            mark(shared, key_id, |c| c.level_db = level);
                            (sq, sn) = (0.0, 0);
                        }
                        if let Some(d) = &desk {
                            d.send(payload);
                        }
                    }
                    _ => {}
                }
            }
            _ = demote.notified() => {
                // The slot's door closed on a source standing by that is not on air: off the
                // desk, the connection kept, a private test from now (its time starts here).
                let max_ms = {
                    let mut conns = shared.conns.lock().expect("ingest lock");
                    conns.get_mut(key_id).and_then(|c| c.demote_ms.take())
                };
                if let Some(max_ms) = max_ms.filter(|_| *mode == Mode::Standby) {
                    *mode = Mode::Test { until: Instant::now() + Duration::from_millis(max_ms) };
                    desk = None;
                    let now_mode = mode.clone();
                    mark(shared, key_id, |c| c.mode = now_mode);
                    shared.control.event(key_id, "test", None, DOOR_CLOSED_DETAIL, way, Some(addr)).await;
                    tracing::info!(key = %key_id, "the slot's door closed: now a private test");
                }
            }
            _ = recheck.tick(), if matches!(mode, Mode::Test { .. }) => {
                // A test still open when its slot's door opens stands by without reconnecting.
                if let Ok(a) = shared.control.admit(key_id, secret, way, addr, true).await {
                    match a.decision.as_str() {
                        "standby" => {
                            *mode = Mode::Standby;
                            if let Some(i) = a.identity { *identity = i; }
                            if let Some(n) = a.name { *name = n; }
                            if let Some(s) = a.show { *show = s; }
                            mark(shared, key_id, |c| c.mode = Mode::Standby);
                            shared.control.event(key_id, "admitted", None, "the slot's door opened", way, Some(addr)).await;
                            if !format_seen.is_empty() {
                                desk = Some(DeskLink::open(shared, identity, name, way, &format_seen, key_id, show));
                            }
                        }
                        "refused" => break "refused on a second look",
                        _ => {}
                    }
                }
            }
        }
    };
    drop(stdin);
    drop(desk);
    let _ = child.start_kill();
    let _ = child.wait().await;
    out_task.abort();
    let complaint = stderr_task.await.unwrap_or_default();
    if let Some(line) = complaint.lines().find(|l| l.contains("\"error\"")) {
        tracing::info!(key = %key_id, worker = %line, "the decoding worker's last word");
        if line.contains("\"unsupported\":true") {
            mark(shared, key_id, |c| c.stopped_for = Some("format"));
            return Ok("the station cannot read what the software sends");
        }
    }
    Ok(why)
}

// ---- the desk ----

/// A source's sound on its way to the desk: a task that holds the loopback connection, and
/// connects again every 2 s while the desk is away. Sound offered while it is away is dropped.
struct DeskLink {
    tx: mpsc::Sender<Vec<u8>>,
    task: JoinHandle<()>,
}

impl DeskLink {
    fn open(
        shared: &Ingest,
        identity: &str,
        name: &str,
        way: &str,
        format: &str,
        key: &str,
        show: &str,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(100);
        let s = shared.settings.clone();
        let head = {
            use base64::Engine;
            let auth = base64::engine::general_purpose::STANDARD
                .encode(format!("ingest:{}", s.desk_password));
            format!(
                "PUT {}{} HTTP/1.0\r\nAuthorization: Basic {auth}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n\r\n",
                wire::PATH_PREFIX,
                identity,
                wire::NAME,
                wire::clean(name),
                wire::WAY,
                way,
                wire::FORMAT,
                wire::clean(format),
                wire::KEY,
                wire::clean(key),
                wire::SHOW,
                wire::clean(show),
            )
        };
        let who = identity.to_owned();
        let task = tokio::spawn(async move {
            loop {
                match desk_connect(s.desk, &head).await {
                    Ok(mut stream) => {
                        tracing::info!(source = %who, "handed to the desk");
                        loop {
                            let Some(pcm) = rx.recv().await else { return };
                            if stream.write_all(&pcm).await.is_err() {
                                tracing::warn!(source = %who, "the desk went away; connecting again");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(source = %who, error = %format!("{e:#}"), "cannot hand the source to the desk; trying again in 2 s");
                    }
                }
                let wait = tokio::time::sleep(Duration::from_secs(2));
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        _ = &mut wait => break,
                        got = rx.recv() => if got.is_none() { return },
                    }
                }
            }
        });
        DeskLink { tx, task }
    }

    fn send(&self, pcm: Vec<u8>) {
        let _ = self.tx.try_send(pcm);
    }
}

impl Drop for DeskLink {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn desk_connect(addr: SocketAddr, head: &str) -> Result<TcpStream> {
    let mut stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr))
        .await
        .context("the desk did not answer")??;
    stream.set_nodelay(true)?;
    stream.write_all(head.as_bytes()).await?;
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    while !line.ends_with(b"\r\n\r\n") && line.len() < 1024 {
        if tokio::time::timeout(Duration::from_secs(3), stream.read(&mut b)).await?? == 0 {
            bail!("the desk closed the connection");
        }
        line.push(b[0]);
    }
    let text = String::from_utf8_lossy(&line);
    if !text.starts_with("HTTP/1.0 200") && !text.starts_with("HTTP/1.1 200") {
        bail!(
            "the desk refused: {}",
            text.lines().next().unwrap_or_default()
        );
    }
    stream.write_all(&wire::wav_f32_header()).await?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_id_is_the_stream_id_or_its_resource() {
        assert_eq!(key_from_stream_id("abcd2345"), "abcd2345");
        assert_eq!(key_from_stream_id(" abcd2345 "), "abcd2345");
        assert_eq!(key_from_stream_id("#!::r=abcd2345,m=publish"), "abcd2345");
        assert_eq!(key_from_stream_id("#!::u=finlay,r=abcd2345"), "abcd2345");
        assert_eq!(key_from_stream_id("#!::m=publish"), "");
    }

    #[test]
    fn two_megabits_a_second_with_a_quarters_grace() {
        assert_eq!(MAX_BYTES_PER_WINDOW, 3_125_000);
    }
}
