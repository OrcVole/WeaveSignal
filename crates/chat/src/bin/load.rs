// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat's load test (ADR-0031: Centrifugo becomes the transport only if this fails): many
//! readers on one room, a few posters, a steady rate then bursts, a cohort of slow clients that
//! never read, and a cohort that drops and reconnects with its position. It reports the
//! message latency each reader saw (receive time minus the message's server time, one clock on
//! one machine), what the slow and reconnecting cohorts got, and the server's CPU and memory
//! read from `/proc/<pid>` of the process it is given.
//!
//!   weavesignal-chat-load --base http://127.0.0.1:37311 --admin <bearer> --pid <server pid> \
//!     --sockets 4000 --steady-rate 7 --steady-s 30 --burst-rate 20 --burst-s 20

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser, Clone)]
struct Args {
    /// The chat's HTTP base.
    #[arg(long, default_value = "http://127.0.0.1:37311")]
    base: String,
    /// The station's admin bearer (posters are staff sessions, so the rules do not pace them).
    #[arg(long, env = "WEAVESIGNAL_CONTROL_ADMIN_TOKEN")]
    admin: String,
    /// The chat server's process id, for its CPU and memory.
    #[arg(long)]
    pid: Option<u32>,
    /// Readers in the room (anonymous, as most listeners are).
    #[arg(long, default_value_t = 4000)]
    sockets: usize,
    /// Of those, readers that never read their socket (the server must close them, not stall).
    #[arg(long, default_value_t = 40)]
    slow: usize,
    /// Of those, readers that drop and reconnect with their position mid-run.
    #[arg(long, default_value_t = 200)]
    reconnect: usize,
    #[arg(long, default_value_t = 8)]
    posters: usize,
    /// New sockets a second while the room fills.
    #[arg(long, default_value_t = 800)]
    connect_rate: u64,
    #[arg(long, default_value_t = 7.0)]
    steady_rate: f64,
    #[arg(long, default_value_t = 30)]
    steady_s: u64,
    #[arg(long, default_value_t = 20.0)]
    burst_rate: f64,
    #[arg(long, default_value_t = 20)]
    burst_s: u64,
    /// A short spike well past the expected peak.
    #[arg(long, default_value_t = 60.0)]
    spike_rate: f64,
    #[arg(long, default_value_t = 10)]
    spike_s: u64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn ws_url(base: &str) -> String {
    format!(
        "{}/api/v1/chat/socket",
        base.replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1)
    )
}

/// Latencies seen per phase (by the message's server time), in ms.
#[derive(Default)]
struct Seen {
    lat: HashMap<&'static str, Vec<u32>>,
    msgs: u64,
}

struct Shared {
    phase: std::sync::RwLock<Vec<(i64, &'static str)>>,
    seen: Mutex<Seen>,
    closed_slow: AtomicU64,
    closed_other: AtomicU64,
    recovered: AtomicU64,
    not_recovered: AtomicU64,
    missed_frames: AtomicU64,
    stop: AtomicBool,
}

impl Shared {
    fn phase_at(&self, t: i64) -> &'static str {
        let p = self.phase.read().expect("phase");
        p.iter()
            .rev()
            .find(|(s, _)| *s <= t)
            .map(|(_, n)| *n)
            .unwrap_or("fill")
    }
}

/// Open a socket with Nagle off (as browsers do for WebSockets); a slow reader also gets a
/// 4 KiB receive buffer, so the server's queue for it fills instead of the kernel's.
async fn connect(url: &str, small: bool) -> Option<Ws> {
    let addr = url
        .trim_start_matches("ws://")
        .split('/')
        .next()?
        .parse::<std::net::SocketAddr>()
        .ok()?;
    let sock = tokio::net::TcpSocket::new_v4().ok()?;
    if small {
        sock.set_recv_buffer_size(4096).ok()?;
    }
    let tcp = sock.connect(addr).await.ok()?;
    tcp.set_nodelay(true).ok()?;
    tokio_tungstenite::client_async(url, tcp)
        .await
        .ok()
        .map(|(ws, _)| ws)
}

type Ws = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// A reader's position in the room: the epoch and the last offset seen.
type Pos = Option<(String, u64)>;

async fn reader(args: Arc<Args>, sh: Arc<Shared>, slow: bool, reconnect_at: Option<Instant>) {
    let url = ws_url(&args.base);
    let Some(mut ws) = connect(&url, slow).await else {
        sh.closed_other.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if ws
        .send(Message::Text(json!({"t": "hello"}).to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    if slow {
        // Never read: the server's queue for us fills and it must close us. Afterwards, read
        // what the kernel kept, to see whether the server's last word was 4008.
        while !sh.stop.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let drained = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(m) = ws.next().await {
                match m {
                    Ok(Message::Close(f)) => return f.is_some_and(|f| u16::from(f.code) == 4008),
                    Ok(_) => {}
                    Err(_) => return false,
                }
            }
            false
        })
        .await;
        if drained == Ok(true) {
            sh.closed_slow.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    let mut lat: HashMap<&'static str, Vec<u32>> = HashMap::new();
    let mut n = 0u64;
    let mut pos: Pos = None;
    let mut reconnect_at = reconnect_at;
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        if sh.stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(at) = reconnect_at
            && Instant::now() >= at
        {
            reconnect_at = None;
            let _ = ws.close(None).await;
            // Stay away a moment, so there is something to recover.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let Some(w) = connect(&url, false).await else {
                sh.closed_other.fetch_add(1, Ordering::Relaxed);
                break;
            };
            ws = w;
            let hello = match &pos {
                Some((e, o)) => json!({"t": "hello", "since": {"epoch": e, "offset": o}}),
                None => json!({"t": "hello"}),
            };
            if ws
                .send(Message::Text(hello.to_string().into()))
                .await
                .is_err()
            {
                break;
            }
            continue;
        }
        let next = tokio::select! {
            m = ws.next() => m,
            _ = ping.tick() => {
                let _ = ws.send(Message::Text(r#"{"t":"ping"}"#.into())).await;
                continue;
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => continue,
        };
        match next {
            Some(Ok(Message::Text(t))) => {
                let t = t.as_str();
                if t.starts_with(r#"{"t":"msg""#) {
                    let recv = now_ms();
                    let v: Value = serde_json::from_str(t).unwrap_or_default();
                    let sent = v["m"]["server_ms"].as_i64().unwrap_or(recv);
                    if let Some(o) = v["offset"].as_u64()
                        && let Some(p) = pos.as_mut()
                    {
                        p.1 = o;
                    }
                    lat.entry(sh.phase_at(sent))
                        .or_default()
                        .push((recv - sent).max(0) as u32);
                    n += 1;
                } else if t.starts_with(r#"{"t":"welcome""#) {
                    welcome(&sh, &mut pos, t);
                } else if let Some(p) = pos.as_mut()
                    && t.contains(r#""offset":"#)
                {
                    let v: Value = serde_json::from_str(t).unwrap_or_default();
                    if let Some(o) = v["offset"].as_u64() {
                        p.1 = o;
                    }
                }
            }
            Some(Ok(Message::Close(f))) => {
                if f.as_ref().is_some_and(|f| u16::from(f.code) == 4008) {
                    sh.closed_slow.fetch_add(1, Ordering::Relaxed);
                } else {
                    sh.closed_other.fetch_add(1, Ordering::Relaxed);
                }
                break;
            }
            Some(Ok(_)) => {}
            _ => {
                if !sh.stop.load(Ordering::Relaxed) {
                    sh.closed_other.fetch_add(1, Ordering::Relaxed);
                }
                break;
            }
        }
    }
    let mut s = sh.seen.lock().await;
    s.msgs += n;
    for (k, v) in lat {
        s.lat.entry(k).or_default().extend(v);
    }
}

fn welcome(sh: &Shared, pos: &mut Pos, t: &str) {
    let v: Value = serde_json::from_str(t).unwrap_or_default();
    let epoch = v["epoch"].as_str().unwrap_or_default().to_owned();
    if pos.is_some() {
        if v["recovered"] == true {
            sh.recovered.fetch_add(1, Ordering::Relaxed);
            let missed = v["missed"].as_array().map(|a| a.len()).unwrap_or(0);
            sh.missed_frames.fetch_add(missed as u64, Ordering::Relaxed);
            if let Some(o) = v["missed"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|l| l["offset"].as_u64())
                && let Some(p) = pos.as_mut()
            {
                p.1 = o;
            }
            if let Some(p) = pos.as_mut() {
                p.0 = epoch;
            }
            return;
        }
        sh.not_recovered.fetch_add(1, Ordering::Relaxed);
    }
    *pos = Some((epoch, v["offset"].as_u64().unwrap_or(0)));
}

async fn staff_session(base: &str, admin: &str, name: &str) -> anyhow::Result<String> {
    let url = format!("{base}/api/v1/chat/staff/session");
    let admin = admin.to_owned();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let mut r = ureq::post(&url)
            .header("Authorization", &format!("Bearer {admin}"))
            .send_json(json!({"name": name}))?;
        let v: Value = r.body_mut().read_json()?;
        Ok(v["session"].as_str().unwrap_or_default().to_owned())
    })
    .await?
}

/// One sample of the server: when, CPU (per cent of one core), resident KiB, open files.
type Sample = (i64, f64, u64, usize);

/// The server's CPU and resident memory, sampled each second.
async fn sample(pid: u32, sh: Arc<Shared>) -> Vec<Sample> {
    let hz = 100.0; // USER_HZ on Linux
    let read = || -> Option<(u64, u64, usize)> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = stat.rsplit_once(')')?.1;
        let f: Vec<&str> = after.split_whitespace().collect();
        let ticks = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let rss = status
            .lines()
            .find(|l| l.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()?;
        let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?.count();
        Some((ticks, rss, fds))
    };
    let mut out = Vec::new();
    let mut last = read();
    let mut t0 = Instant::now();
    while !sh.stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = read();
        let dt = t0.elapsed().as_secs_f64();
        t0 = Instant::now();
        if let (Some((a, _, _)), Some((b, rss, fds))) = (last, now) {
            out.push((now_ms(), (b - a) as f64 / hz / dt * 100.0, rss, fds));
        }
        last = now;
    }
    out
}

fn pct(v: &mut [u32], p: f64) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i]
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Arc::new(Args::parse());
    let sh = Arc::new(Shared {
        phase: std::sync::RwLock::new(vec![(0, "fill")]),
        seen: Mutex::new(Seen::default()),
        closed_slow: AtomicU64::new(0),
        closed_other: AtomicU64::new(0),
        recovered: AtomicU64::new(0),
        not_recovered: AtomicU64::new(0),
        missed_frames: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    });
    let sampler = args.pid.map(|pid| tokio::spawn(sample(pid, sh.clone())));

    // ---- posters ----
    let mut posters = Vec::new();
    for i in 0..args.posters {
        let s = staff_session(&args.base, &args.admin, &format!("Load poster {i}")).await?;
        let mut ws = connect(&ws_url(&args.base), false)
            .await
            .ok_or_else(|| anyhow::anyhow!("cannot reach the chat"))?;
        ws.send(Message::Text(
            json!({"t": "hello", "session": s}).to_string().into(),
        ))
        .await?;
        posters.push(ws);
    }

    // ---- fill the room ----
    let t_fill = Instant::now();
    let total = args.sockets;
    let mut tasks = Vec::with_capacity(total);
    let gap = Duration::from_micros(1_000_000 / args.connect_rate.max(1));
    let slow_every = (total / args.slow.max(1)).max(1);
    let reconnect_every = (total / args.reconnect.max(1)).max(1);
    let fill_s_est = total as u64 / args.connect_rate.max(1);
    for i in 0..total {
        let slow = args.slow > 0 && i % slow_every == 0 && i / slow_every < args.slow;
        let reconnect = !slow
            && args.reconnect > 0
            && i % reconnect_every == 1
            && i / reconnect_every < args.reconnect;
        // Reconnectors drop somewhere in the steady phase or the burst.
        let at = reconnect.then(|| {
            Instant::now()
                + Duration::from_secs(
                    fill_s_est + 5 + (i as u64 % (args.steady_s + args.burst_s).max(1)),
                )
        });
        tasks.push(tokio::spawn(reader(args.clone(), sh.clone(), slow, at)));
        tokio::time::sleep(gap).await;
    }
    let fill_s = t_fill.elapsed().as_secs_f64();
    eprintln!("filled {total} sockets in {fill_s:.1} s");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // ---- the phases ----
    let mut sent = 0u64;
    let mut ack_lat: Vec<u32> = Vec::new();
    let phases = [
        ("steady", args.steady_rate, args.steady_s),
        ("burst", args.burst_rate, args.burst_s),
        ("spike", args.spike_rate, args.spike_s),
    ];
    let (tx_ack, mut rx_ack) = tokio::sync::mpsc::unbounded_channel::<(String, i64)>();
    let mut sinks = Vec::new();
    for ws in posters {
        let (sink, mut stream) = ws.split();
        sinks.push(sink);
        let tx = tx_ack.clone();
        tokio::spawn(async move {
            while let Some(Ok(m)) = stream.next().await {
                if let Message::Text(t) = m
                    && t.as_str().starts_with(r#"{"t":"ack""#)
                {
                    let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                    let _ = tx.send((v["nonce"].as_str().unwrap_or_default().to_owned(), now_ms()));
                }
            }
        });
    }
    let mut pending: HashMap<String, i64> = HashMap::new();
    let mut acked = 0u64;
    for (name, rate, secs) in phases {
        if secs == 0 || rate <= 0.0 {
            continue;
        }
        sh.phase.write().expect("phase").push((now_ms(), name));
        let every = Duration::from_secs_f64(1.0 / rate);
        let end = Instant::now() + Duration::from_secs(secs);
        let mut next = Instant::now();
        let mut k = 0usize;
        while Instant::now() < end {
            let nonce = format!("{name}-{sent}");
            let body =
                format!("{name} message {sent}: the N22 ran two an hour till three in the morning");
            let n_sinks = sinks.len();
            let p = &mut sinks[k % n_sinks];
            k += 1;
            pending.insert(nonce.clone(), now_ms());
            p.send(Message::Text(
                json!({"t": "post", "nonce": nonce, "kind": "chat", "body": body})
                    .to_string()
                    .into(),
            ))
            .await?;
            sent += 1;
            while let Ok((n, at)) = rx_ack.try_recv() {
                if let Some(t0) = pending.remove(&n) {
                    ack_lat.push((at - t0).max(0) as u32);
                    acked += 1;
                }
            }
            next += every;
            tokio::time::sleep_until(next.into()).await;
        }
        eprintln!("{name}: {rate}/s for {secs} s");
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    while let Ok((n, at)) = rx_ack.try_recv() {
        if let Some(t0) = pending.remove(&n) {
            ack_lat.push((at - t0).max(0) as u32);
            acked += 1;
        }
    }
    let unanswered = pending.len();
    sh.stop.store(true, Ordering::Relaxed);
    for t in tasks {
        let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
    }
    let samples = match sampler {
        Some(s) => s.await.unwrap_or_default(),
        None => vec![],
    };

    // ---- the report ----
    let seen = sh.seen.lock().await;
    let readers = total - args.slow;
    println!("# weavesignal-chat load test");
    println!(
        "sockets {total} (readers {readers}, slow {}, reconnecting {}), posters {}, filled in {fill_s:.1} s",
        args.slow, args.reconnect, args.posters
    );
    println!(
        "posted {sent}, acked {acked}, unanswered {unanswered}; ack latency p50 {} ms p99 {} ms",
        pct(&mut ack_lat.clone(), 0.5),
        pct(&mut ack_lat, 0.99)
    );
    println!(
        "frames delivered to readers {} (posted x readers = {})",
        seen.msgs,
        sent * readers as u64
    );
    let mut lat = seen.lat.clone();
    for name in ["steady", "burst", "spike"] {
        if let Some(v) = lat.get_mut(name) {
            let n = v.len();
            let max = *v.iter().max().unwrap_or(&0);
            let (p50, p95, p99) = (pct(v, 0.5), pct(v, 0.95), pct(v, 0.99));
            println!(
                "latency {name:6} n={n:>9}  p50 {p50:>4} ms  p95 {p95:>4} ms  p99 {p99:>4} ms  max {max:>5} ms"
            );
        }
    }
    println!(
        "slow clients closed with 4008: {} of {}; other closes {}",
        sh.closed_slow.load(Ordering::Relaxed),
        args.slow,
        sh.closed_other.load(Ordering::Relaxed)
    );
    println!(
        "reconnects recovered {} (frames replayed {}), not recovered {}",
        sh.recovered.load(Ordering::Relaxed),
        sh.missed_frames.load(Ordering::Relaxed),
        sh.not_recovered.load(Ordering::Relaxed)
    );
    let phases = sh.phase.read().expect("phase").clone();
    for (i, (start, name)) in phases.iter().enumerate() {
        let end = phases.get(i + 1).map(|p| p.0).unwrap_or(i64::MAX);
        let s: Vec<&Sample> = samples
            .iter()
            .filter(|x| x.0 > *start && x.0 <= end)
            .collect();
        if s.is_empty() {
            continue;
        }
        let avg = s.iter().map(|x| x.1).sum::<f64>() / s.len() as f64;
        let max = s.iter().map(|x| x.1).fold(0.0, f64::max);
        let rss = s.iter().map(|x| x.2).max().unwrap_or(0);
        let fds = s.iter().map(|x| x.3).max().unwrap_or(0);
        println!(
            "server {name:6} cpu avg {avg:>5.1} % max {max:>5.1} % of a core; rss max {:.1} MiB; fds max {fds}",
            rss as f64 / 1024.0
        );
    }
    Ok(())
}
