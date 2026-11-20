// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! VBAN in (the presenter kit's server half, Phase 17g): a studio PC on the same network sends
//! its sound as VBAN, and the ingest takes it as a show key's source, standing by at the desk
//! until a person puts it on air, with the post, LINK LOST and the monitor link as for push.
//!
//! VBAN is unauthenticated UDP, so the only thing that lets a packet in is a **binding** in the
//! station's kit settings: a sender's address *and* a stream name, bound to a show key. Every
//! other packet is dropped and counted by its reason, in this order, cheapest first: the
//! sender's address (an IPv4 address inside IPv6 is read as IPv4), the packet itself
//! ([`weavesignal_proto::vban::parse`]), then the name. An address does not stop spoofing on a
//! LAN; that is why what comes in only stands by.
//!
//! Each bound stream's packets are checked in turn: its counter (a gap of a few packets is
//! filled with silence of the same length, so the follower at the desk has no slip to chase; a
//! packet from behind or twice is dropped), its format (one that changes mid-way closes the
//! stream and opens it again as new), and its rate (48,000 frames a second, with a quarter's
//! grace over ten seconds: VBAN carries no compressed media, so the ingest's byte window does
//! not fit it). 48 kHz only for now; up to eight channels, of which the binding's pair is
//! taken, mono heard in both ears.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use weavesignal_proto::vban::{self, DataType, Header, StreamName};

/// The most channels a stream may have.
pub const MAX_CHANNELS: u16 = 8;
/// A stream that sends nothing for this long is closed.
const SILENT_END: Duration = Duration::from_secs(2);
/// The longest run of lost packets filled with silence; more is a restart.
const MAX_GAP: u32 = 64;
/// Counters this far behind are late packets; further is a restart.
const MAX_BEHIND: u32 = 1_000;
/// Frames a stream may send over [`WINDOW`]: 48,000 a second with a quarter's grace.
const MAX_FRAMES_PER_WINDOW: u64 = 48_000 * 10 * 5 / 4;
const WINDOW: Duration = Duration::from_secs(10);
/// The most streams at once (the design's four inputs).
pub const MAX_STREAMS: usize = 4;

/// A binding: one sender's stream, as a show key's source.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Rule {
    pub address: IpAddr,
    pub name: String,
    pub key_id: String,
    /// The channels taken (1-based, as a mixer's labels say), left then right.
    #[serde(default = "first_pair")]
    pub channels: [u16; 2],
}

fn first_pair() -> [u16; 2] {
    [1, 2]
}

/// An IPv4 address inside IPv6 (`::ffff:192.168.1.20`) as IPv4.
pub fn canonical(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(addr, IpAddr::V4),
        v4 => v4,
    }
}

/// What was dropped and why, for the status.
#[derive(Debug, Default)]
pub struct Counts(Mutex<BTreeMap<&'static str, u64>>);

impl Counts {
    pub fn add(&self, reason: &'static str, n: u64) {
        *self.0.lock().expect("counts").entry(reason).or_default() += n;
    }

    pub fn snapshot(&self) -> BTreeMap<&'static str, u64> {
        self.0.lock().expect("counts").clone()
    }

    pub fn get(&self, reason: &str) -> u64 {
        self.snapshot().get(reason).copied().unwrap_or(0)
    }
}

/// Find a packet's binding: the sender's address, then the packet, then its name.
pub fn gate<'a, 'p>(
    rules: &'a [Rule],
    from: IpAddr,
    packet: &'p [u8],
) -> Result<(&'a Rule, Header, &'p [u8]), &'static str> {
    let from = canonical(from);
    if !rules.iter().any(|r| canonical(r.address) == from) {
        return Err("address");
    }
    let (header, payload) = vban::parse(packet).map_err(|r| r.word())?;
    rules
        .iter()
        .find(|r| canonical(r.address) == from && r.name == header.name.as_str())
        .map(|r| (r, header, payload))
        .ok_or("name")
}

/// What became of a packet on its stream.
#[derive(Debug, PartialEq)]
pub enum Taken {
    /// Its samples (and any silence before them) were added.
    Samples,
    /// Dropped, for this reason.
    Dropped(&'static str),
    /// The format changed or the sender restarted: close this stream and open it again.
    Reopen,
}

/// One bound stream's state.
pub struct Stream {
    rate: u32,
    channels: u16,
    data: DataType,
    pair: (u16, u16),
    expected: u32,
    arrivals: VecDeque<(Instant, u64)>,
    /// Packets lost on the way (filled with silence) since it opened; a packet dropped late is
    /// counted in the ingest's `late`, not here.
    pub lost: u64,
}

impl Stream {
    /// A stream from its first packet; `Err` with the reason when the station does not take
    /// its format.
    pub fn open(h: &Header, rule: &Rule) -> Result<Stream, &'static str> {
        if h.rate != 48_000 {
            return Err("rate");
        }
        if h.channels > MAX_CHANNELS {
            return Err("channels");
        }
        let pick = |c: u16| c.saturating_sub(1).min(h.channels - 1);
        Ok(Stream {
            rate: h.rate,
            channels: h.channels,
            data: h.data,
            pair: (pick(rule.channels[0]), pick(rule.channels[1])),
            expected: h.frame,
            arrivals: VecDeque::new(),
            lost: 0,
        })
    }

    /// What the host's software sends, as the desk's tile shows it.
    pub fn info(&self) -> String {
        let kind = match self.data {
            DataType::I16 => "16-bit",
            DataType::I24 => "24-bit",
            DataType::I32 => "32-bit",
            DataType::F32 => "float",
        };
        let ch = match self.channels {
            1 => "mono".to_owned(),
            2 => "stereo".to_owned(),
            n => format!("{n} channels"),
        };
        format!("vban {kind} 48 kHz {ch}")
    }

    /// Take one packet received at `now`: its samples as 48 kHz stereo `f32` on `out`.
    pub fn take(&mut self, h: &Header, payload: &[u8], now: Instant, out: &mut Vec<f32>) -> Taken {
        if (h.rate, h.channels, h.data) != (self.rate, self.channels, self.data) {
            return Taken::Reopen;
        }
        let ahead = h.frame.wrapping_sub(self.expected);
        let behind = self.expected.wrapping_sub(h.frame);
        let gap = if ahead == 0 {
            0
        } else if ahead <= MAX_GAP {
            ahead
        } else if behind <= MAX_BEHIND {
            return Taken::Dropped("late");
        } else {
            return Taken::Reopen;
        };
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > WINDOW)
        {
            self.arrivals.pop_front();
        }
        let silence = u64::from(gap) * u64::from(h.samples);
        let frames = silence + u64::from(h.samples);
        let sent: u64 = self.arrivals.iter().map(|(_, n)| n).sum();
        if sent + frames > MAX_FRAMES_PER_WINDOW {
            // Counted as taken, so that what follows is not filled with silence for it.
            self.expected = h.frame.wrapping_add(1);
            return Taken::Dropped("too_fast");
        }
        self.arrivals.push_back((now, frames));
        self.expected = h.frame.wrapping_add(1);
        self.lost += u64::from(gap);
        out.resize(out.len() + 2 * silence as usize, 0.0);
        vban::to_stereo(h, payload, self.pair, out);
        Taken::Samples
    }
}

/// A stream opened: the supervisor admits it with the control plane and hands its sound on.
pub struct Opened {
    pub key_id: String,
    pub address: IpAddr,
    pub name: String,
    /// What it sends (`vban 24-bit 48 kHz stereo`).
    pub info: String,
    /// 48 kHz stereo `f32`, little-endian, as a decoding worker's frames carry it.
    pub pcm: mpsc::Receiver<Vec<u8>>,
    /// Packets lost on the way since it opened ([`Stream::lost`]), kept up to date by the
    /// listener: the ingest's report carries it to the clock readout.
    pub lost: Arc<AtomicU64>,
}

struct Live {
    stream: Stream,
    tx: mpsc::Sender<Vec<u8>>,
    last: Instant,
    lost: Arc<AtomicU64>,
}

/// Listen on `socket` until the rules' sender goes; open a stream for each bound sender's
/// first good packet and hand it to `opened`.
pub async fn listen(
    socket: UdpSocket,
    mut rules: watch::Receiver<Vec<Rule>>,
    opened: mpsc::Sender<Opened>,
    counts: Arc<Counts>,
) {
    let mut buf = vec![0u8; 2048];
    let mut live: HashMap<(IpAddr, StreamName), Live> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut samples = Vec::with_capacity(4096);
    loop {
        tokio::select! {
            changed = rules.changed() => {
                if changed.is_err() {
                    return;
                }
                // A binding removed closes its stream.
                let now = rules.borrow().clone();
                live.retain(|(addr, name), _| {
                    now.iter().any(|r| canonical(r.address) == *addr && r.name == name.as_str())
                });
            }
            _ = tick.tick() => {
                live.retain(|_, l| l.last.elapsed() < SILENT_END && !l.tx.is_closed());
            }
            got = socket.recv_from(&mut buf) => {
                let Ok((n, peer)) = got else {
                    continue;
                };
                let from = canonical(peer.ip());
                let now = Instant::now();
                let rules_now = rules.borrow().clone();
                let (rule, header, payload) = match gate(&rules_now, from, &buf[..n]) {
                    Ok(x) => x,
                    Err(why) => {
                        counts.add(why, 1);
                        continue;
                    }
                };
                let key = (from, header.name);
                samples.clear();
                let taken = match live.get_mut(&key) {
                    Some(l) => l.stream.take(&header, payload, now, &mut samples),
                    None => Taken::Reopen,
                };
                match taken {
                    Taken::Samples => {}
                    Taken::Dropped(why) => {
                        counts.add(why, 1);
                        continue;
                    }
                    Taken::Reopen => {
                        live.remove(&key);
                        if live.len() >= MAX_STREAMS {
                            counts.add("too_many", 1);
                            continue;
                        }
                        let mut stream = match Stream::open(&header, rule) {
                            Ok(s) => s,
                            Err(why) => {
                                counts.add(why, 1);
                                continue;
                            }
                        };
                        let (tx, rx) = mpsc::channel(256);
                        let info = stream.info();
                        let lost = Arc::new(AtomicU64::new(0));
                        if opened
                            .send(Opened {
                                key_id: rule.key_id.clone(),
                                address: from,
                                name: rule.name.clone(),
                                info,
                                pcm: rx,
                                lost: lost.clone(),
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        stream.take(&header, payload, now, &mut samples);
                        live.insert(key, Live { stream, tx, last: now, lost });
                    }
                }
                if let Some(l) = live.get_mut(&key) {
                    l.last = now;
                    l.lost.store(l.stream.lost, Ordering::Relaxed);
                    let mut bytes = Vec::with_capacity(samples.len() * 4);
                    for s in &samples {
                        bytes.extend_from_slice(&s.to_le_bytes());
                    }
                    if l.tx.try_send(bytes).is_err() {
                        counts.add("backlog", 1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weavesignal_proto::vban::{Packetiser, write};

    fn rule(addr: &str, name: &str) -> Rule {
        Rule {
            address: addr.parse().unwrap(),
            name: name.into(),
            key_id: "abcd2345".into(),
            channels: [1, 2],
        }
    }

    fn packet(name: &str, frame: u32, channels: u16, data: DataType, samples: u16) -> Vec<u8> {
        let h = Header {
            rate: 48_000,
            samples,
            channels,
            data,
            name: StreamName::new(name).unwrap(),
            frame,
        };
        let mut p = Vec::new();
        write(&h, &mut p).unwrap();
        for i in 0..usize::from(samples) * usize::from(channels) {
            let v = (i % 7) as i16 * 1000;
            match data {
                DataType::I16 => p.extend_from_slice(&v.to_le_bytes()),
                _ => p.extend(std::iter::repeat_n(0u8, data.bytes())),
            }
        }
        p
    }

    #[test]
    fn the_address_is_checked_before_the_packet_and_the_name_after() {
        let rules = vec![rule("192.168.1.20", "Studio")];
        let good = packet("Studio", 0, 2, DataType::I16, 240);
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(gate(&rules, ip("192.168.1.20"), &good).is_ok());
        // An IPv4 sender seen on an IPv6 socket.
        assert!(gate(&rules, ip("::ffff:192.168.1.20"), &good).is_ok());
        // A stranger's garbage is refused for its address, never parsed.
        assert_eq!(
            gate(&rules, ip("192.168.1.21"), b"junk").err(),
            Some("address")
        );
        assert_eq!(
            gate(&rules, ip("192.168.1.20"), b"junk").err(),
            Some("size")
        );
        let other = packet("Stream1", 0, 2, DataType::I16, 240);
        assert_eq!(gate(&rules, ip("192.168.1.20"), &other).err(), Some("name"));
        // Names are matched exactly.
        let lower = packet("studio", 0, 2, DataType::I16, 240);
        assert_eq!(gate(&rules, ip("192.168.1.20"), &lower).err(), Some("name"));
    }

    fn parsed(p: &[u8]) -> (Header, &[u8]) {
        vban::parse(p).unwrap()
    }

    #[test]
    fn a_lost_packet_is_silence_of_its_length_and_a_late_one_is_dropped() {
        let r = rule("10.0.0.2", "Studio");
        let t = Instant::now();
        let p0 = packet("Studio", 10, 2, DataType::I16, 240);
        let (h0, d0) = parsed(&p0);
        let mut s = Stream::open(&h0, &r).unwrap();
        let mut out = Vec::new();
        assert_eq!(s.take(&h0, d0, t, &mut out), Taken::Samples);
        assert_eq!(out.len(), 480);
        // Packets 11 and 12 lost: 480 frames of silence, then 13's 240.
        let p13 = packet("Studio", 13, 2, DataType::I16, 240);
        let (h13, d13) = parsed(&p13);
        out.clear();
        assert_eq!(s.take(&h13, d13, t, &mut out), Taken::Samples);
        assert_eq!(out.len(), 2 * (480 + 240));
        assert!(out[..960].iter().all(|&v| v == 0.0));
        assert_eq!(s.lost, 2);
        // 12 arriving now, and 13 again: late, dropped.
        let p12 = packet("Studio", 12, 2, DataType::I16, 240);
        let (h12, d12) = parsed(&p12);
        assert_eq!(s.take(&h12, d12, t, &mut out), Taken::Dropped("late"));
        assert_eq!(s.take(&h13, d13, t, &mut out), Taken::Dropped("late"));
        // A counter far away is a sender that restarted.
        let far = packet("Studio", 90_000, 2, DataType::I16, 240);
        let (hf, df) = parsed(&far);
        assert_eq!(s.take(&hf, df, t, &mut out), Taken::Reopen);
        // The counter wraps.
        let p_max = packet("Studio", u32::MAX, 2, DataType::I16, 240);
        let (hm, dm) = parsed(&p_max);
        let mut w = Stream::open(&hm, &r).unwrap();
        assert_eq!(w.take(&hm, dm, t, &mut out), Taken::Samples);
        let p_zero = packet("Studio", 0, 2, DataType::I16, 240);
        let (hz, dz) = parsed(&p_zero);
        assert_eq!(w.take(&hz, dz, t, &mut out), Taken::Samples);
        assert_eq!(w.lost, 0);
    }

    #[test]
    fn a_format_change_reopens_and_odd_formats_are_refused() {
        let r = rule("10.0.0.2", "Studio");
        let t = Instant::now();
        let a = packet("Studio", 0, 2, DataType::I16, 240);
        let (ha, da) = parsed(&a);
        let mut s = Stream::open(&ha, &r).unwrap();
        s.take(&ha, da, t, &mut Vec::new());
        let b = packet("Studio", 1, 2, DataType::I24, 160);
        let (hb, db) = parsed(&b);
        assert_eq!(s.take(&hb, db, t, &mut Vec::new()), Taken::Reopen);
        let mut h = hb;
        h.rate = 44_100;
        assert_eq!(Stream::open(&h, &r).err(), Some("rate"));
        let wide = packet("Studio", 0, 9, DataType::I16, 50);
        assert_eq!(Stream::open(&parsed(&wide).0, &r).err(), Some("channels"));
        assert_eq!(
            Stream::open(&hb, &r).unwrap().info(),
            "vban 24-bit 48 kHz stereo"
        );
    }

    #[test]
    fn the_bindings_pair_is_taken_from_many_channels() {
        let mut r = rule("10.0.0.2", "Desk");
        r.channels = [3, 4];
        let p = packet("Desk", 0, 4, DataType::I16, 1);
        let (h, d) = parsed(&p);
        let mut s = Stream::open(&h, &r).unwrap();
        let mut out = Vec::new();
        s.take(&h, d, Instant::now(), &mut out);
        assert_eq!(out, vec![2000.0 / 32_768.0, 3000.0 / 32_768.0]);
    }

    #[test]
    fn more_than_48000_frames_a_second_is_dropped() {
        let r = rule("10.0.0.2", "Studio");
        let t = Instant::now();
        let first = packet("Studio", 0, 2, DataType::I16, 240);
        let mut s = Stream::open(&parsed(&first).0, &r).unwrap();
        let mut dropped = 0;
        // Ten seconds' worth and a half again, all at once.
        for i in 0..3000u32 {
            let p = packet("Studio", i, 2, DataType::I16, 240);
            let (h, d) = parsed(&p);
            if s.take(&h, d, t, &mut Vec::new()) == Taken::Dropped("too_fast") {
                dropped += 1;
            }
        }
        assert_eq!(dropped, 3000 - 2500);
        // Later, it may send again.
        let p = packet("Studio", 3000, 2, DataType::I16, 240);
        let (h, d) = parsed(&p);
        let later = t + Duration::from_secs(11);
        assert_eq!(s.take(&h, d, later, &mut Vec::new()), Taken::Samples);
    }

    /// A UDP socket on a given loopback address (Linux routes all of 127/8 to loopback).
    async fn sender(ip: &str) -> UdpSocket {
        UdpSocket::bind(format!("{ip}:0")).await.unwrap()
    }

    #[tokio::test]
    async fn over_loopback_only_the_bound_sender_and_name_open_a_stream() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let to = socket.local_addr().unwrap();
        let (rules_tx, rules_rx) = watch::channel(vec![rule("127.0.0.2", "Studio")]);
        let (open_tx, mut open_rx) = mpsc::channel(4);
        let counts = Arc::new(Counts::default());
        let task = tokio::spawn(listen(socket, rules_rx, open_tx, counts.clone()));
        let bound = sender("127.0.0.2").await;
        let stranger = sender("127.0.0.3").await;
        let mut pk = Packetiser::new(StreamName::new("Studio").unwrap(), DataType::I16);
        let mut packets = Vec::new();
        pk.push(0, &vec![0.25f32; 2 * 960], &mut packets);
        // The stranger sends the very same packets (spoofing the name) and garbage.
        for p in &packets {
            stranger.send_to(p, to).await.unwrap();
        }
        stranger.send_to(b"VBAN nonsense", to).await.unwrap();
        // The bound sender under another name.
        let mut other = Packetiser::new(StreamName::new("Other").unwrap(), DataType::I16);
        let mut wrong = Vec::new();
        other.push(0, &vec![0.25f32; 480], &mut wrong);
        bound.send_to(&wrong[0], to).await.unwrap();
        for p in &packets {
            bound.send_to(p, to).await.unwrap();
        }
        let mut opened = tokio::time::timeout(Duration::from_secs(2), open_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(opened.key_id, "abcd2345");
        assert_eq!(opened.address, "127.0.0.2".parse::<IpAddr>().unwrap());
        assert_eq!(opened.info, "vban 16-bit 48 kHz stereo");
        let mut frames = 0;
        while frames < 960 {
            let pcm = tokio::time::timeout(Duration::from_secs(2), opened.pcm.recv())
                .await
                .unwrap()
                .unwrap();
            let first = f32::from_le_bytes([pcm[0], pcm[1], pcm[2], pcm[3]]);
            assert!((first - 0.25).abs() < 1e-3);
            frames += pcm.len() / 8;
        }
        assert_eq!(frames, 960);
        // Nothing else opened.
        assert!(open_rx.try_recv().is_err());
        assert_eq!(counts.get("address"), 5);
        assert_eq!(counts.get("name"), 1);
        // The binding removed closes the stream.
        rules_tx.send(Vec::new()).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(2), opened.pcm.recv()).await;
        assert!(matches!(closed, Ok(None)));
        drop(rules_tx);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}
