// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! VBAN out (the presenter kit's server half, Phase 17g): a bus of the desk sent as VBAN to one
//! receiver on the station's own network: the programme after the profanity delay (what
//! listeners hear, for a transmitter or a recorder), the programme before it (a studio's
//! monitors), or a mix-minus or a host's monitor mix (a local guest's return).
//!
//! The mixing pass hands each sender a copy of the bus's block with the block's **place in the
//! mix**, without waiting ([`VbanOut::offer`]: a sender that falls behind loses blocks, counted,
//! and never holds the pass up). The sender's own thread cuts the blocks into packets
//! ([`weavesignal_proto::vban::Packetiser`]: the counter follows the mix, a restart of the mix
//! from the clock sends what was left as a short packet first) and sends each when its first
//! frame is due, by position: anchored at the first block, so blocks the pass mixed several at
//! once (catching up) go out spread over their own length, not in a burst. It is never stamped
//! or paced by when a block happened to arrive (the caller-return trap, AGENTS.md); a block
//! more than 200 ms from where the anchor says re-anchors.
//!
//! Unicast to one named address from the station's named address; never broadcast or
//! multicast (the control plane refuses those before they get here).

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use weavesignal_proto::vban::{DataType, Packetiser, StreamName};

use crate::pipeline::RATE;

/// Blocks waiting for a sender: half a second at 20 ms.
const QUEUE: usize = 25;
/// A block this far from where the anchor puts it starts a new anchor.
const REANCHOR: Duration = Duration::from_millis(200);

/// What a VBAN out sends.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OutSource {
    /// The programme after the profanity delay.
    Programme,
    /// The programme before it.
    Live,
    /// A person's mix-minus, or a host's own studio's monitor mix: the bus of that identity.
    Bus(String),
}

impl OutSource {
    /// From the control plane's word: `programme`, `programme_live`, `monitor:<key id>`,
    /// `mix_minus:<identity>`.
    pub fn parse(s: &str) -> Option<OutSource> {
        match s.split_once(':') {
            None if s == "programme" => Some(OutSource::Programme),
            None if s == "programme_live" => Some(OutSource::Live),
            Some(("monitor", k)) if !k.is_empty() => {
                Some(OutSource::Bus(weavesignal_proto::source_wire::identity(k)))
            }
            Some(("mix_minus", id)) if !id.is_empty() => Some(OutSource::Bus(id.to_owned())),
            _ => None,
        }
    }
}

/// One output as the control plane sends it (`{"op": "kit"}`).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutSpec {
    pub address: String,
    pub port: u16,
    pub name: String,
    pub source: String,
    /// `s16`, `s24` or `f32`.
    pub format: String,
}

/// One output as the status reports it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutStatus {
    pub name: String,
    pub address: String,
    pub source: String,
    pub packets: u64,
    pub dropped: u64,
    pub problem: Option<String>,
}

#[derive(Default)]
struct Counts {
    packets: AtomicU64,
    dropped: AtomicU64,
    problem: Mutex<Option<String>>,
}

/// One VBAN out and its thread.
pub struct VbanOut {
    pub source: OutSource,
    spec: OutSpec,
    tx: SyncSender<(i64, Vec<f32>)>,
    counts: Arc<Counts>,
    stop: Arc<AtomicBool>,
}

impl Drop for VbanOut {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// When each block's packets are due, by its place in the mix.
#[derive(Debug, Default)]
pub struct Pacer {
    anchor: Option<(i64, Instant)>,
}

impl Pacer {
    /// When the frame at `pos` is due, `now` being when its block arrived.
    pub fn due(&mut self, pos: i64, now: Instant) -> Instant {
        let at = |(p0, t0): (i64, Instant)| {
            let d = pos - p0;
            if d >= 0 {
                t0 + Duration::from_nanos((d as u64) * 1_000_000_000 / u64::from(RATE))
            } else {
                t0.checked_sub(Duration::from_nanos(
                    (d.unsigned_abs()) * 1_000_000_000 / u64::from(RATE),
                ))
                .unwrap_or(t0)
            }
        };
        let fresh = match self.anchor {
            None => true,
            Some(a) => {
                let due = at(a);
                due > now + REANCHOR || now > due + REANCHOR
            }
        };
        if fresh {
            self.anchor = Some((pos, now));
        }
        at(self.anchor.expect("set above"))
    }
}

impl VbanOut {
    /// Start sending `spec` from `bind` (port chosen by the system).
    pub fn start(spec: OutSpec, bind: IpAddr) -> Result<VbanOut> {
        let source = OutSource::parse(&spec.source).context("not a source the desk can send")?;
        let name = StreamName::new(&spec.name).context("a stream name of 1 to 16 characters")?;
        let data = DataType::from_name(&spec.format).context("`s16`, `s24` or `f32`")?;
        let to: IpAddr = spec.address.trim().parse().context("an IP address")?;
        if to.is_multicast() || to.is_unspecified() {
            bail!("one receiver's address, never multicast");
        }
        if spec.port == 0 {
            bail!("a port from 1 to 65535");
        }
        let socket = UdpSocket::bind(SocketAddr::new(bind, 0))
            .with_context(|| format!("cannot send from {bind}"))?;
        socket
            .connect(SocketAddr::new(to, spec.port))
            .context("cannot reach that address")?;
        let (tx, rx) = sync_channel(QUEUE);
        let counts = Arc::new(Counts::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (c, s) = (counts.clone(), stop.clone());
        let label = spec.name.clone();
        std::thread::Builder::new()
            .name("desk-vban-out".into())
            .spawn(move || send_loop(socket, Packetiser::new(name, data), rx, c, s))
            .context("cannot start a VBAN out thread")?;
        tracing::info!(name = %label, to = %spec.address, port = spec.port, source = %spec.source, "VBAN out (Phase 17g)");
        Ok(VbanOut {
            source,
            spec,
            tx,
            counts,
            stop,
        })
    }

    /// Hand the block at `pos` to the sender without waiting; a full queue drops it, counted.
    pub fn offer(&self, pos: i64, block: &[f32]) {
        match self.tx.try_send((pos, block.to_vec())) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.counts.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Say why it sends nothing now (its source is not at the desk), or that it does.
    pub fn set_problem(&self, problem: Option<String>) {
        *self.counts.problem.lock().expect("vban out") = problem;
    }

    pub fn status(&self) -> OutStatus {
        OutStatus {
            name: self.spec.name.clone(),
            address: self.spec.address.clone(),
            source: self.spec.source.clone(),
            packets: self.counts.packets.load(Ordering::Relaxed),
            dropped: self.counts.dropped.load(Ordering::Relaxed),
            problem: self.counts.problem.lock().expect("vban out").clone(),
        }
    }
}

fn send_loop(
    socket: UdpSocket,
    mut pk: Packetiser,
    rx: Receiver<(i64, Vec<f32>)>,
    counts: Arc<Counts>,
    stop: Arc<AtomicBool>,
) {
    let mut pacer = Pacer::default();
    let mut packets = Vec::new();
    let mut warned = false;
    while !stop.load(Ordering::Relaxed) {
        let (pos, block) = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(b) => b,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        packets.clear();
        let waiting = pk.pending_frames();
        pk.push(pos, &block, &mut packets);
        // The first packet carries the frames left from before, then this block's own.
        let mut first = pos - waiting as i64;
        for p in &packets {
            let frames = weavesignal_proto::vban::parse(p).map_or(0, |(h, _)| i64::from(h.samples));
            let due = pacer.due(first, Instant::now());
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            match socket.send(p) {
                Ok(_) => {
                    counts.packets.fetch_add(1, Ordering::Relaxed);
                    warned = false;
                }
                Err(e) => {
                    if !warned {
                        tracing::warn!(error = %e, "VBAN out cannot send");
                        warned = true;
                    }
                    *counts.problem.lock().expect("vban out") = Some(e.to_string());
                }
            }
            first += frames;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(port: u16, source: &str) -> OutSpec {
        OutSpec {
            address: "127.0.0.1".into(),
            port,
            name: "Programme".into(),
            source: source.into(),
            format: "s16".into(),
        }
    }

    #[test]
    fn sources_are_read_from_the_control_planes_words() {
        assert_eq!(OutSource::parse("programme"), Some(OutSource::Programme));
        assert_eq!(OutSource::parse("programme_live"), Some(OutSource::Live));
        assert_eq!(
            OutSource::parse("monitor:abcd2345"),
            Some(OutSource::Bus("source-abcd2345".into()))
        );
        assert_eq!(
            OutSource::parse("mix_minus:hamish"),
            Some(OutSource::Bus("hamish".into()))
        );
        assert_eq!(OutSource::parse("everything"), None);
        assert!(VbanOut::start(spec(0, "programme"), "127.0.0.1".parse().unwrap()).is_err());
        let mut m = spec(6980, "programme");
        m.address = "239.0.0.1".into();
        assert!(VbanOut::start(m, "127.0.0.1".parse().unwrap()).is_err());
    }

    #[test]
    fn the_pacer_spreads_a_catch_up_by_position_and_reanchors_after_a_jump() {
        let mut p = Pacer::default();
        let t0 = Instant::now();
        // Eight 20 ms blocks arrive at once: each is due 20 ms after the one before.
        let dues: Vec<Duration> = (0..8).map(|i| p.due(i * 960, t0) - t0).collect();
        assert_eq!(dues[0], Duration::ZERO);
        assert_eq!(dues[7], Duration::from_millis(140));
        // The mix restarted from the clock far ahead: a new anchor, not a long sleep.
        let later = t0 + Duration::from_millis(500);
        assert_eq!(p.due(10 * 48_000, later), later);
    }

    #[test]
    fn packets_follow_the_mix_and_a_catch_up_is_not_a_burst() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let port = rx.local_addr().unwrap().port();
        let out = VbanOut::start(spec(port, "programme"), "127.0.0.1".parse().unwrap()).unwrap();
        let block = vec![0.25f32; 2 * 960];
        // Eight blocks offered at once (the pass catching up), then a jump in position.
        let t0 = Instant::now();
        for i in 0..8 {
            out.offer(i * 960, &block);
        }
        out.offer(1_000_000, &vec![0.25f32; 2 * 1000]);
        let mut got = Vec::new();
        let mut buf = [0u8; 2048];
        while got.len() < 36 {
            let n = rx.recv(&mut buf).unwrap();
            let (h, _) = weavesignal_proto::vban::parse(&buf[..n]).unwrap();
            got.push((t0.elapsed(), h.frame, h.samples));
        }
        // 32 packets of 240 for the eight blocks, then four for the 1,000 frames (no remainder
        // was waiting), in order, the counter rising by one.
        assert!(got.windows(2).all(|w| w[1].1 == w[0].1 + 1));
        assert!(got[..32].iter().all(|g| g.2 == 240));
        // Spread over the blocks' own 160 ms, not sent at once.
        assert!(got[31].0 >= Duration::from_millis(130), "{:?}", got[31].0);
        assert_eq!(out.status().packets, 36);
    }

    #[test]
    fn a_sender_that_falls_behind_drops_blocks_and_never_blocks_the_pass() {
        // Nothing listens; the sender sleeps by position, so a burst of 200 blocks overflows.
        let out = VbanOut::start(spec(9, "programme"), "127.0.0.1".parse().unwrap()).unwrap();
        let block = vec![0.0f32; 2 * 960];
        let t = Instant::now();
        for i in 0..200 {
            out.offer(i * 960, &block);
        }
        assert!(t.elapsed() < Duration::from_millis(100), "the pass waited");
        assert!(out.status().dropped > 100);
    }
}
