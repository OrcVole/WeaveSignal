// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The bridge's uplink (Phase 17g; [`weavesignal_proto::bridge_wire`]): one outbound WebSocket
//! from the studio app to the station's public address on 443, carrying what the host's own
//! software sends to the app on loopback ([`crate::bridge_local`]) byte for byte.
//!
//! - **The key** is the show key's Icecast password (`<key id>-<secret>`), from the system
//!   keychain; it goes in the upgrade's `Authorization` and is never logged.
//! - **Bounded:** at most 2 s of the stream waits here (by the rate it arrives at, at least 64
//!   KiB); older bytes are dropped, because a burst after a stall would trip the station's 2
//!   Mbit/s limit and become standing delay. Over 1.6 Mbit/s the app says "send sound only"
//!   before the station refuses.
//! - **Reconnecting** at 1, 2, 4 … 30 s with jitter. MP3, AAC and MPEG-TS find their own way
//!   back into a stream; Ogg and FLAC cannot start mid-way, so the app keeps their header pages
//!   (Ogg's pages before the first sound, FLAC's `fLaC` and metadata) and sends them first on
//!   every new connection, then the stream from its next page or frame. Dropping Ogg or FLAC
//!   bytes on a connection that stays up would splice the stream, so for them an overflow ends
//!   the connection and the next one starts with the heads.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use weavesignal_proto::bridge_wire::{self as wire, Hello, State, Title};

use crate::bridge_local::Event;

/// The longest the uplink waits between tries.
const MAX_BACKOFF_S: u64 = 30;
/// What may wait here: this long at the stream's rate.
const HOLD: Duration = Duration::from_secs(2);
const MIN_HOLD_BYTES: usize = 64 * 1024;
/// Over this, the app asks for sound only (the station's limit is 2 Mbit/s).
const TOO_FAST_BPS: usize = 1_600_000 / 8;
/// The most header bytes kept.
const MAX_HEADS: usize = 256 * 1024;

/// What the screen shows.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Status {
    /// `off`, `waiting` (for the host's software), `connecting`, `up` or `retrying`.
    pub uplink: String,
    /// The host's software and what it sends, while connected locally.
    pub software: Option<String>,
    pub format: Option<String>,
    /// The station's last word: `standing_by`, `test` or `refused`, and its sentence.
    pub station_state: Option<String>,
    pub station_message: Option<String>,
    /// Why the last connection ended or failed.
    pub problem: Option<String>,
    /// Seconds until the next try.
    pub retry_in_s: Option<u64>,
    pub sent_bytes: u64,
    pub dropped_bytes: u64,
    pub kbps: u32,
    /// The host's software sends more than the station takes: ask for sound only.
    pub too_fast: bool,
}

/// Where to send, and with which key.
#[derive(Clone)]
pub struct Target {
    /// The station's public address (`https://radio.example.org`).
    pub url: String,
    /// `<key id>-<secret>`.
    pub password: String,
}

impl Target {
    /// The WebSocket's address.
    pub fn ws_url(&self) -> Result<String, String> {
        let base = self.url.trim().trim_end_matches('/');
        let rest = base
            .strip_prefix("https://")
            .map(|r| format!("wss://{r}"))
            .or_else(|| base.strip_prefix("http://").map(|r| format!("ws://{r}")))
            .ok_or("The station's address starts with https://")?;
        Ok(format!("{rest}{}", wire::PATH))
    }
}

/// The wait before try `n` (from 0): 1, 2, 4 … 30 s, give or take a quarter.
pub fn backoff(n: u32, jitter: f64) -> Duration {
    let base = 1u64 << n.min(5);
    let s = base.min(MAX_BACKOFF_S) as f64;
    Duration::from_secs_f64(s * (0.75 + 0.5 * jitter.clamp(0.0, 1.0)))
}

fn jitter() -> f64 {
    let mut b = [0u8; 2];
    let _ = getrandom::fill(&mut b);
    f64::from(u16::from_le_bytes(b)) / f64::from(u16::MAX)
}

// ---- the heads Ogg and FLAC need ----

/// Keeps the bytes a decoder needs before it can start mid-stream, and finds where a stream
/// can be joined.
pub struct Heads {
    format: &'static str,
    /// Bytes not yet scanned (a page split across chunks).
    carry: Vec<u8>,
    heads: Vec<u8>,
    /// Still collecting heads (Ogg: until the first page with sound; restarted by a new
    /// stream's first page, as a chained stream sends at each track).
    collecting: bool,
    /// FLAC: the metadata is complete.
    flac_done: bool,
}

impl Heads {
    pub fn new(format: &'static str) -> Self {
        Heads {
            format,
            carry: Vec::new(),
            heads: Vec::new(),
            collecting: true,
            flac_done: false,
        }
    }

    /// Whether this format needs heads at all.
    pub fn needed(&self) -> bool {
        matches!(self.format, "ogg" | "flac")
    }

    /// The heads, once known.
    pub fn heads(&self) -> Option<&[u8]> {
        match self.format {
            "ogg" if !self.collecting && !self.heads.is_empty() => Some(&self.heads),
            "flac" if self.flac_done => Some(&self.heads),
            _ => None,
        }
    }

    /// Look at the next bytes of the stream.
    pub fn feed(&mut self, chunk: &[u8]) {
        match self.format {
            "ogg" => self.feed_ogg(chunk),
            "flac" if !self.flac_done => self.feed_flac(chunk),
            _ => {}
        }
    }

    fn feed_ogg(&mut self, chunk: &[u8]) {
        self.carry.extend_from_slice(chunk);
        let mut at = 0;
        loop {
            let b = &self.carry[at..];
            let Some(start) = b.windows(4).position(|w| w == b"OggS") else {
                at = self.carry.len().saturating_sub(3);
                break;
            };
            let page = &b[start..];
            if page.len() < 27 {
                at += start;
                break;
            }
            // The page's first sound ends the heads as soon as its header is seen.
            let bos = page[5] & 0x02 != 0;
            let granule = u64::from_le_bytes(page[6..14].try_into().expect("8 bytes"));
            // A page seen again once whole changes nothing here: this is idempotent.
            if bos {
                self.heads.clear();
                self.collecting = true;
            }
            if self.collecting && granule != 0 {
                self.collecting = false;
            }
            let segs = usize::from(page[26]);
            if page.len() < 27 + segs {
                at += start;
                break;
            }
            let body: usize = page[27..27 + segs].iter().map(|&x| usize::from(x)).sum();
            let len = 27 + segs + body;
            if page.len() < len {
                at += start;
                break;
            }
            if self.collecting {
                if self.heads.len() + len <= MAX_HEADS {
                    self.heads.extend_from_slice(&page[..len]);
                } else {
                    self.collecting = false;
                }
            }
            at += start + len;
        }
        self.carry.drain(..at.min(self.carry.len()));
    }

    fn feed_flac(&mut self, chunk: &[u8]) {
        self.carry.extend_from_slice(chunk);
        let b = &self.carry;
        if b.len() < 4 {
            return;
        }
        if &b[..4] != b"fLaC" {
            // Not a stream from its start: nothing to keep.
            self.flac_done = true;
            self.heads.clear();
            self.carry.clear();
            return;
        }
        let mut at = 4;
        loop {
            if b.len() < at + 4 {
                return;
            }
            let last = b[at] & 0x80 != 0;
            let len = (usize::from(b[at + 1]) << 16)
                | (usize::from(b[at + 2]) << 8)
                | usize::from(b[at + 3]);
            if b.len() < at + 4 + len {
                if at + 4 + len > MAX_HEADS {
                    self.flac_done = true;
                    self.carry.clear();
                }
                return;
            }
            at += 4 + len;
            if last {
                self.heads = b[..at].to_vec();
                self.flac_done = true;
                self.carry.clear();
                return;
            }
        }
    }

    /// Where in `b` the stream may be joined: an Ogg page's start, a FLAC frame's sync code;
    /// `Some(0)` for formats that find their own way in.
    pub fn boundary(&self, b: &[u8]) -> Option<usize> {
        match self.format {
            "ogg" => b.windows(4).position(|w| w == b"OggS"),
            "flac" => b
                .windows(2)
                .position(|w| w[0] == 0xFF && (w[1] == 0xF8 || w[1] == 0xF9)),
            _ => Some(0),
        }
    }
}

// ---- the queue ----

/// What waits to go, bounded at about 2 s of the stream.
struct Queue {
    q: VecDeque<Bytes>,
    bytes: usize,
    arrivals: VecDeque<(Instant, usize)>,
    dropped: u64,
}

impl Queue {
    fn new() -> Self {
        Queue {
            q: VecDeque::new(),
            bytes: 0,
            arrivals: VecDeque::new(),
            dropped: 0,
        }
    }

    /// Bytes a second, over the last five seconds.
    fn rate(&self) -> usize {
        self.arrivals.iter().map(|(_, n)| n).sum::<usize>() / 5
    }

    fn limit(&self) -> usize {
        (self.rate() * HOLD.as_secs() as usize).max(MIN_HOLD_BYTES)
    }

    fn arrived(&mut self, n: usize, now: Instant) {
        self.arrivals.push_back((now, n));
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(5))
        {
            self.arrivals.pop_front();
        }
    }

    /// Add a chunk; whether older bytes had to go.
    fn push(&mut self, b: Bytes) -> bool {
        self.bytes += b.len();
        self.q.push_back(b);
        let mut over = false;
        let limit = self.limit();
        while self.bytes > limit && self.q.len() > 1 {
            let old = self.q.pop_front().expect("more than one");
            self.bytes -= old.len();
            self.dropped += old.len() as u64;
            over = true;
        }
        over
    }

    fn pop(&mut self) -> Option<Bytes> {
        let b = self.q.pop_front()?;
        self.bytes -= b.len();
        Some(b)
    }

    fn clear(&mut self) {
        self.dropped += self.bytes as u64;
        self.q.clear();
        self.bytes = 0;
    }
}

// ---- one source's uplink ----

/// Why a connection ended.
enum End {
    /// The host's software stopped: the bridge waits for the next.
    LocalGone,
    /// The uplink failed; try again.
    Lost(String),
    /// The station refused: try again later (a key that is wrong stays wrong; the slot's
    /// door may open).
    Refused(String),
    /// Stopped from the screen.
    Stopped,
}

/// Carry one local source to the station until it ends or `stop` is dropped.
pub async fn carry(
    target: Target,
    format: &'static str,
    software: String,
    mut media: mpsc::Receiver<Bytes>,
    mut titles: mpsc::Receiver<String>,
    status: Arc<Mutex<Status>>,
    mut stop: mpsc::Receiver<()>,
) {
    let set = |f: &dyn Fn(&mut Status)| f(&mut status.lock().expect("bridge status"));
    set(&|s| {
        s.software = Some(software.clone());
        s.format = Some(format.into());
        s.problem = None;
    });
    let mut heads = Heads::new(format);
    let mut queue = Queue::new();
    let mut tries = 0u32;
    // Refusals in a row: a station that admits the source and then refuses what it sends (Opus
    // inside MPEG-TS, 1.2.1) says standing by first, which resets `tries`; without this the
    // bridge would try again every second. Reset once a connection lasts a minute.
    let mut refused_in_a_row = 0u32;
    // The first connection carries the stream from its start; every later one must join it.
    let mut join = false;
    let mut last_title: Option<String> = None;
    loop {
        set(&|s| {
            s.uplink = "connecting".into();
            s.retry_in_s = None;
        });
        let began = Instant::now();
        let end = connect_and_send(
            &target,
            format,
            &software,
            &mut media,
            &mut titles,
            &mut heads,
            &mut queue,
            join,
            &mut last_title,
            &status,
            &mut stop,
            &mut tries,
        )
        .await;
        join = true;
        let why = match end {
            End::LocalGone | End::Stopped => {
                set(&|s| {
                    s.uplink = "waiting".into();
                    s.software = None;
                    s.format = None;
                    s.station_state = None;
                    s.station_message = None;
                    s.too_fast = false;
                });
                return;
            }
            End::Lost(why) => {
                if began.elapsed() >= Duration::from_secs(60) {
                    refused_in_a_row = 0;
                }
                why
            }
            End::Refused(why) => {
                refused_in_a_row = refused_in_a_row.saturating_add(1);
                why
            }
        };
        let wait = backoff(tries.max(refused_in_a_row), jitter());
        tries = tries.saturating_add(1);
        set(&|s| {
            s.uplink = "retrying".into();
            s.problem = Some(why.clone());
            s.retry_in_s = Some(wait.as_secs());
        });
        // Keep reading the host's software while waiting, within the bound.
        let until = tokio::time::Instant::now() + wait;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(until) => break,
                _ = stop.recv() => return,
                t = titles.recv() => if let Some(t) = t { last_title = Some(t) },
                m = media.recv() => {
                    let Some(m) = m else {
                        set(&|s| { s.uplink = "waiting".into(); s.software = None; s.format = None; });
                        return;
                    };
                    heads.feed(&m);
                    queue.arrived(m.len(), Instant::now());
                    queue.push(m);
                    set(&|s| s.dropped_bytes = queue.dropped);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn connect_and_send(
    target: &Target,
    format: &'static str,
    software: &str,
    media: &mut mpsc::Receiver<Bytes>,
    titles: &mut mpsc::Receiver<String>,
    heads: &mut Heads,
    queue: &mut Queue,
    join: bool,
    last_title: &mut Option<String>,
    status: &Arc<Mutex<Status>>,
    stop: &mut mpsc::Receiver<()>,
    tries: &mut u32,
) -> End {
    let set = |f: &dyn Fn(&mut Status)| f(&mut status.lock().expect("bridge status"));
    let url = match target.ws_url() {
        Ok(u) => u,
        Err(e) => return End::Refused(e),
    };
    let mut req = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => return End::Refused(format!("not an address: {e}")),
    };
    let auth =
        base64::engine::general_purpose::STANDARD.encode(format!("source:{}", target.password));
    match format!("Basic {auth}").parse() {
        Ok(v) => {
            req.headers_mut().insert("authorization", v);
        }
        Err(_) => return End::Refused("the key cannot be sent".into()),
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(wire::MAX_MESSAGE))
        .max_frame_size(Some(wire::MAX_FRAME));
    let connected = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async_with_config(req, Some(config), true),
    )
    .await;
    let ws = match connected {
        Ok(Ok((ws, _))) => ws,
        Ok(Err(e)) => return End::Lost(format!("cannot reach the station: {e}")),
        Err(_) => return End::Lost("the station did not answer within 10 s".into()),
    };
    let (mut sink, mut stream) = ws.split();
    let hello = serde_json::to_string(&Hello {
        format: format.into(),
        name: software.into(),
    })
    .unwrap_or_default();
    if sink
        .send(Message::Text(Utf8Bytes::from(hello)))
        .await
        .is_err()
    {
        return End::Lost("the station closed at once".into());
    }
    // A writer of its own, so that a slow network never stops the reading of the host's
    // software: what it has not taken waits in the bounded queue.
    let (wtx, mut wrx) = mpsc::channel::<Message>(16);
    let writer = tokio::spawn(async move {
        while let Some(m) = wrx.recv().await {
            if sink.send(m).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });
    let abort = scopeguard(writer);
    set(&|s| {
        s.uplink = "up".into();
        s.problem = None;
    });
    // A new connection joins a stream already running: the heads first, then the stream from
    // where it may be joined.
    let mut resync = join && heads.needed();
    if join && let Some(h) = heads.heads() {
        let _ = wtx.send(Message::binary(h.to_vec())).await;
    }
    if let Some(t) = last_title.clone() {
        let text = serde_json::to_string(&Title { title: t }).unwrap_or_default();
        let _ = wtx.send(Message::Text(Utf8Bytes::from(text))).await;
    }
    let mut rate_tick = tokio::time::interval(Duration::from_secs(1));
    let end = loop {
        // Hand the writer what it can take.
        while let Ok(permit) = wtx.try_reserve() {
            let Some(mut b) = queue.pop() else {
                drop(permit);
                break;
            };
            if resync {
                match heads.boundary(&b) {
                    Some(i) => {
                        b = b.slice(i..);
                        resync = false;
                    }
                    None => continue,
                }
            }
            let n = b.len() as u64;
            permit.send(Message::binary(b));
            set(&|s| s.sent_bytes += n);
        }
        if wtx.is_closed() {
            break End::Lost("the connection to the station dropped".into());
        }
        tokio::select! {
            _ = stop.recv() => break End::Stopped,
            _ = rate_tick.tick() => {
                let rate = queue.rate();
                set(&|s| {
                    s.kbps = (rate * 8 / 1000) as u32;
                    s.too_fast = rate > TOO_FAST_BPS;
                });
            }
            t = titles.recv() => {
                if let Some(t) = t {
                    *last_title = Some(t.clone());
                    let text = serde_json::to_string(&Title { title: t }).unwrap_or_default();
                    let _ = wtx.send(Message::Text(Utf8Bytes::from(text))).await;
                }
            }
            m = media.recv() => {
                let Some(m) = m else { break End::LocalGone };
                heads.feed(&m);
                queue.arrived(m.len(), Instant::now());
                let over = queue.push(m);
                set(&|s| s.dropped_bytes = queue.dropped);
                if over && heads.needed() {
                    // Ogg and FLAC cannot lose bytes mid-connection: start again with the heads.
                    queue.clear();
                    break End::Lost("the network fell behind the stream; joining again".into());
                }
            }
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(st) = serde_json::from_str::<State>(t.as_str()) {
                            let refused = st.state == "refused";
                            if !refused {
                                *tries = 0;
                            }
                            set(&|s| {
                                s.station_state = Some(st.state.clone());
                                s.station_message = Some(st.message.clone());
                            });
                            if refused {
                                break End::Refused(st.message);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        break End::Lost("the station closed the connection".into());
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    };
    if matches!(end, End::LocalGone) {
        // The host's software stopped: what it sent last still goes, then the socket closes.
        while let Some(mut b) = queue.pop() {
            if resync {
                match heads.boundary(&b) {
                    Some(i) => {
                        b = b.slice(i..);
                        resync = false;
                    }
                    None => continue,
                }
            }
            if wtx.send(Message::binary(b)).await.is_err() {
                break;
            }
        }
        drop(wtx);
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            let mut a = abort;
            (&mut a.0).await
        })
        .await;
        return end;
    }
    drop(wtx);
    drop(abort);
    end
}

/// Abort a task when dropped.
struct Guard(tokio::task::JoinHandle<()>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn scopeguard(t: tokio::task::JoinHandle<()>) -> Guard {
    Guard(t)
}

/// Run the bridge: each local source in turn, carried until it ends, until `events` closes.
pub async fn run(target: Target, mut events: mpsc::Receiver<Event>, status: Arc<Mutex<Status>>) {
    status.lock().expect("bridge status").uplink = "waiting".into();
    let mut title_tx: Option<mpsc::Sender<String>> = None;
    let mut current: Option<(tokio::task::JoinHandle<()>, mpsc::Sender<()>)> = None;
    while let Some(e) = events.recv().await {
        match e {
            Event::Title(t) => {
                if let Some(tx) = &title_tx {
                    let _ = tx.try_send(t);
                }
            }
            Event::Source {
                format,
                software,
                media,
            } => {
                if let Some((task, _)) = current.take() {
                    task.abort();
                }
                let (ttx, trx) = mpsc::channel(8);
                title_tx = Some(ttx);
                let (stop_tx, stop_rx) = mpsc::channel(1);
                let task = tokio::spawn(carry(
                    target.clone(),
                    format,
                    software,
                    media,
                    trx,
                    status.clone(),
                    stop_rx,
                ));
                current = Some((task, stop_tx));
            }
        }
    }
    if let Some((task, _)) = current {
        task.abort();
    }
    status.lock().expect("bridge status").uplink = "off".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../../crates/ingest/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// Decode what one connection carried with the station's own decoder: the frames of sound
    /// it made, or its error.
    fn decodes(format: &str, bytes: Vec<u8>) -> Result<usize, String> {
        let mut out = Vec::new();
        weavesignal_ingest::decode::decode(format, std::io::Cursor::new(bytes), &mut out)
            .map_err(|e| e.to_string())?;
        let mut r = &out[..];
        let mut frames = 0;
        while let Some((kind, p)) = weavesignal_ingest::frame::read(&mut r).unwrap() {
            if kind == weavesignal_ingest::frame::PCM {
                frames += p.len() / 8;
            }
        }
        Ok(frames)
    }

    #[test]
    fn the_wait_doubles_to_thirty_seconds() {
        let s: Vec<u64> = (0..8).map(|n| backoff(n, 0.5).as_secs()).collect();
        assert_eq!(s, vec![1, 2, 4, 8, 16, 30, 30, 30]);
        assert!(backoff(0, 0.0) < backoff(0, 1.0));
        let t = Target {
            url: "https://radio.example.org/".into(),
            password: "x".into(),
        };
        assert_eq!(
            t.ws_url().unwrap(),
            "wss://radio.example.org/api/v1/sources/bridge"
        );
    }

    #[test]
    fn ogg_and_flac_heads_are_kept_and_a_stream_joined_mid_way_decodes() {
        // tone-48k.oga is one page of sound after its heads: only its heads are checked.
        let mut h = Heads::new("ogg");
        h.feed(&fixture("tone-48k.oga"));
        assert_eq!(h.heads().map(<[u8]>::len), Some(160));
        for (name, format) in [("chained-opus.ogg", "ogg"), ("tone-44k.flac", "flac")] {
            let bytes = fixture(name);
            let mut h = Heads::new(format);
            for c in bytes.chunks(777) {
                h.feed(c);
            }
            let heads = h
                .heads()
                .unwrap_or_else(|| panic!("{name}: no heads"))
                .to_vec();
            assert!(!heads.is_empty() && heads.len() < bytes.len() / 2, "{name}");
            // Joined at an arbitrary byte inside the sound, without heads: the station cannot
            // read it.
            let cut = heads.len() + 13;
            let tail = &bytes[cut..];
            let joined_bare = decodes(format, tail.to_vec());
            // With the heads and from the next boundary: it decodes, sound comes out.
            let at = h
                .boundary(tail)
                .unwrap_or_else(|| panic!("{name}: no boundary"));
            let mut rejoined = heads.clone();
            rejoined.extend_from_slice(&tail[at..]);
            let frames = decodes(format, rejoined).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(frames > 4_800, "{name}: {frames} frames");
            assert!(
                joined_bare.is_err() || joined_bare.unwrap() < frames,
                "{name}: the bare tail did as well"
            );
        }
        // MP3 needs none, and joins anywhere.
        let h = Heads::new("mp3");
        assert!(!h.needed());
        assert_eq!(h.boundary(b"xx"), Some(0));
    }

    #[test]
    fn the_queue_holds_about_two_seconds_and_drops_the_oldest() {
        let mut q = Queue::new();
        let t = Instant::now();
        // 16 KB a second for five seconds: the bound is 64 KiB (the minimum).
        for _ in 0..5 {
            q.arrived(16_000, t);
        }
        for _ in 0..10 {
            q.push(Bytes::from(vec![0u8; 16_000]));
        }
        assert!(q.bytes <= MIN_HOLD_BYTES);
        assert!(q.dropped >= 16_000 * 5);
        // 200 KB a second: two seconds, 400 KB.
        let mut q = Queue::new();
        for _ in 0..50 {
            q.arrived(20_000, t);
        }
        assert_eq!(q.limit(), 400_000);
    }

    /// The station's end, as the ingest runs it: each connection's binary bytes, in order.
    async fn station(drop_first_after: usize) -> (SocketAddr, mpsc::UnboundedReceiver<Vec<u8>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut n = 0;
            loop {
                let (s, peer) = l.accept().await.unwrap();
                n += 1;
                let a = weavesignal_ingest::bridge::accept(s, peer).await.unwrap();
                assert_eq!(a.credentials.key_id, "abcd2345");
                let mut p = weavesignal_ingest::bridge::pump(a.ws, Duration::from_secs(5));
                p.states
                    .send(State {
                        state: "standing_by".into(),
                        message: String::new(),
                    })
                    .unwrap();
                let mut got = Vec::new();
                while let Some(b) = p.media.recv().await {
                    got.extend_from_slice(&b);
                    if n == 1 && got.len() >= drop_first_after {
                        break;
                    }
                }
                tx.send(got).unwrap();
                drop(p);
            }
        });
        (addr, rx)
    }

    async fn until(status: &Arc<Mutex<Status>>, uplink: &str) {
        for _ in 0..200 {
            if status.lock().unwrap().uplink == uplink {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the uplink never was {uplink}");
    }

    /// A station that stands each connection by and then refuses what it sends (Opus inside
    /// MPEG-TS, 1.2.1): the bridge waits longer after each refusal, not a second each time, and
    /// shows the station's words.
    #[tokio::test]
    async fn a_station_that_refuses_what_is_sent_is_tried_again_ever_more_slowly() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (s, peer) = l.accept().await.unwrap();
                let a = weavesignal_ingest::bridge::accept(s, peer).await.unwrap();
                let p = weavesignal_ingest::bridge::pump(a.ws, Duration::from_secs(5));
                for (state, message) in [
                    ("standing_by", ""),
                    (
                        "refused",
                        "The station cannot read what your software sends: Opus inside MPEG-TS.",
                    ),
                ] {
                    p.states
                        .send(State {
                            state: state.into(),
                            message: message.into(),
                        })
                        .unwrap();
                }
                p.finish().await;
            }
        });
        let status = Arc::new(Mutex::new(Status::default()));
        let (media_tx, media) = mpsc::channel(64);
        let (_titles_tx, titles) = mpsc::channel(8);
        let (stop_tx, stop) = mpsc::channel(1);
        let target = Target {
            url: format!("http://{addr}"),
            password: "abcd2345-thesecret".into(),
        };
        let task = tokio::spawn(carry(
            target,
            "mpegts",
            "OBS".into(),
            media,
            titles,
            status.clone(),
            stop,
        ));
        media_tx
            .send(Bytes::from_static(&[0x47; 188]))
            .await
            .unwrap();
        let mut waits = Vec::new();
        for _ in 0..2 {
            until(&status, "retrying").await;
            let s = status.lock().unwrap().clone();
            assert!(
                s.problem
                    .as_deref()
                    .unwrap_or_default()
                    .contains("Opus inside MPEG-TS"),
                "{:?}",
                s.problem
            );
            waits.push(s.retry_in_s.unwrap());
            // The wait over, it connects again (and is refused again).
            for _ in 0..200 {
                if status.lock().unwrap().uplink != "retrying" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        // backoff(1) and backoff(2): about 2 s, then about 4 s (each give or take a quarter).
        assert!(waits[0] >= 1 && waits[1] >= 3, "{waits:?}");
        drop(stop_tx);
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
    }

    #[tokio::test]
    async fn a_dropped_uplink_joins_again_with_the_heads_first() {
        let (addr, mut got) = station(4_000).await;
        let status = Arc::new(Mutex::new(Status::default()));
        let (media_tx, media) = mpsc::channel(64);
        let (_titles_tx, titles) = mpsc::channel(8);
        let (_stop_tx, stop) = mpsc::channel(1);
        let target = Target {
            url: format!("http://{addr}"),
            password: "abcd2345-thesecret".into(),
        };
        let task = tokio::spawn(carry(
            target,
            "ogg",
            "BUTT".into(),
            media,
            titles,
            status.clone(),
            stop,
        ));
        let bytes = fixture("chained-opus.ogg");
        let (start, rest) = bytes.split_at(5_000);
        for c in start.chunks(1000) {
            media_tx.send(Bytes::copy_from_slice(c)).await.unwrap();
        }
        let first = got.recv().await.unwrap();
        assert!(first.starts_with(b"OggS"));
        // The station dropped the first connection after 4 KB; the app tries again.
        until(&status, "retrying").await;
        until(&status, "up").await;
        for c in rest.chunks(1000) {
            media_tx.send(Bytes::copy_from_slice(c)).await.unwrap();
        }
        drop(media_tx);
        let second = tokio::time::timeout(Duration::from_secs(10), got.recv())
            .await
            .unwrap()
            .unwrap();
        // The second starts with the heads, joins at a page, and the station's decoder reads it.
        let mut h = Heads::new("ogg");
        h.feed(start);
        assert!(second.starts_with(h.heads().unwrap()));
        assert!(decodes("ogg", second).unwrap() > 4_800);
        task.await.unwrap();
        let s = status.lock().unwrap().clone();
        assert_eq!(s.uplink, "waiting");
    }
}
