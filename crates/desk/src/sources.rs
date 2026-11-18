// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Hosts in their own studios (ADR-0280): the ingest hands the desk each source that is standing
//! by, one loopback connection each ([`weavesignal_proto::source_wire`]), and the desk takes it
//! in as it takes a person: at the desk, off air, through its own leveller.
//!
//! The ingest has already admitted the source with the control plane, decoded it and made it
//! 48 kHz float; nothing here parses a host's media. The desk checks the password, places the
//! samples on its timeline a cushion ahead of the mixing pass (as playout's feed, [`crate::feed`]),
//! and takes the source off the desk when the connection closes. A newer connection for the same
//! source replaces an older one. One source's connection failing touches no other.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use weavesignal_proto::source_wire as wire;

use crate::config::SourcesIn;
use crate::feed::{Placer, check_request, read_head, running_frames};
use crate::pipeline::{Desk, RATE, SOURCE_PREFIX, SourceFacts};

const CHANNELS: usize = 2;
/// A source that sends nothing for this long is gone (the ingest closes it sooner itself).
const SILENT_DROP: Duration = Duration::from_secs(6);

/// The listener, while the desk takes sources.
pub struct Sources {
    shared: Arc<Shared>,
}

struct Shared {
    stop: AtomicBool,
    /// Each source's newest connection; an older one stops when its number is not this.
    current: Mutex<HashMap<String, u64>>,
    next: AtomicU64,
}

impl Sources {
    /// Listen on `settings.bind`:`settings.port` for the ingest; `None` when sources are off or
    /// the desk has no password for them (then nothing on the machine can hand it a source).
    pub fn start(desk: &Desk, settings: &SourcesIn, password: Option<String>) -> Option<Sources> {
        if !settings.enabled {
            return None;
        }
        let Some(password) = password.filter(|p| !p.is_empty()) else {
            tracing::info!(
                env = crate::config::SOURCES_PASSWORD_ENV,
                "no password for sources: the desk takes no host's own studio"
            );
            return None;
        };
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            current: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        });
        let addr = SocketAddr::new(settings.bind, settings.port);
        let ctx = Ctx {
            desk: desk.clone(),
            shared: shared.clone(),
            password,
            cushion: i64::from(settings.cushion_ms) * i64::from(RATE) / 1000,
        };
        let spawned = std::thread::Builder::new()
            .name("desk-sources".into())
            .spawn(move || ctx.listen(addr));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "cannot start the sources' listener");
            return None;
        }
        Some(Sources { shared })
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Ctx {
    desk: Desk,
    shared: Arc<Shared>,
    password: String,
    cushion: i64,
}

/// What the request says of the source.
#[derive(Debug, PartialEq)]
struct Request {
    identity: String,
    name: String,
    way: String,
    format: String,
    key: String,
    show: String,
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn parse_request(head: &str) -> Result<Request> {
    let first = head.lines().next().unwrap_or_default();
    let path = first.split_whitespace().nth(1).unwrap_or_default();
    let Some(identity) = path.strip_prefix(wire::PATH_PREFIX) else {
        bail!("not a source's path");
    };
    if !identity.starts_with(SOURCE_PREFIX)
        || identity.len() > 64
        || !identity
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("`{}` is not a source's identity", wire::clean(identity));
    }
    let value = |n: &str| wire::clean(header(head, n).unwrap_or_default());
    Ok(Request {
        identity: identity.to_owned(),
        name: value(wire::NAME),
        way: value(wire::WAY),
        format: value(wire::FORMAT),
        key: value(wire::KEY),
        show: value(wire::SHOW),
    })
}

impl Ctx {
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }

    fn listen(self, addr: SocketAddr) {
        let mut warned = false;
        let listener = loop {
            if self.stopped() {
                return;
            }
            match TcpListener::bind(addr).and_then(|l| l.set_nonblocking(true).map(|()| l)) {
                Ok(l) => break l,
                Err(e) => {
                    if !warned {
                        tracing::error!(%addr, error = %e, "cannot listen for sources; trying again every 5 s");
                        warned = true;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        };
        tracing::info!(%addr, "listening for hosts' own studios from the ingest (ADR-0280)");
        while !self.stopped() {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let c = self.clone();
                    let spawned = std::thread::Builder::new()
                        .name("desk-source-in".into())
                        .spawn(move || c.serve(stream, peer));
                    if let Err(e) = spawned {
                        tracing::warn!(error = %e, "cannot start a thread for a source");
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the sources' listener");
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }

    fn serve(self, stream: TcpStream, peer: SocketAddr) {
        if !peer.ip().is_loopback() {
            return;
        }
        if let Err(e) = self.receive(stream) {
            tracing::info!(%peer, error = %format!("{e:#}"), "a source's connection ended");
        }
    }

    fn receive(&self, mut stream: TcpStream) -> Result<()> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let _ = stream.set_nodelay(true);
        let mut reader = BufReader::new(stream.try_clone()?);
        let head = read_head(&mut reader)?;
        if let Err(refusal) = check_request(&head, Some(&self.password)) {
            let _ = stream.write_all(refusal.response().as_bytes());
            bail!("refused: {}", refusal.why());
        }
        let req = match parse_request(&head) {
            Ok(r) => r,
            Err(e) => {
                let _ = stream.write_all(b"HTTP/1.0 400 Bad Request\r\n\r\n");
                return Err(e);
            }
        };
        stream.write_all(b"HTTP/1.0 200 OK\r\nServer: weavesignal-desk\r\n\r\n")?;
        let channels = wire::read_wav_f32_header(&mut reader)?;
        let generation = self.shared.next.fetch_add(1, Ordering::Relaxed);
        self.shared
            .current
            .lock()
            .expect("sources lock")
            .insert(req.identity.clone(), generation);
        let facts = SourceFacts {
            way: req.way.clone(),
            format: req.format.clone(),
            key: req.key.clone(),
            show: req.show.clone(),
            since: Instant::now(),
        };
        let name = if req.name.is_empty() {
            req.key.clone()
        } else {
            req.name.clone()
        };
        let ring = self.desk.add_source(&req.identity, &name, facts)?;
        tracing::info!(source = %req.identity, way = %req.way, format = %req.format, show = %req.show, "a source is standing by");
        let result = self.read_pcm(&mut reader, channels, &req.identity, generation, &ring);
        let mut current = self.shared.current.lock().expect("sources lock");
        if current.get(&req.identity) == Some(&generation) {
            current.remove(&req.identity);
            drop(current);
            self.desk.remove_person(&req.identity);
        }
        result
    }

    fn read_pcm(
        &self,
        reader: &mut impl Read,
        channels: u16,
        identity: &str,
        generation: u64,
        ring: &weavesignal_mixer::InputRing,
    ) -> Result<()> {
        let frame_bytes = 4 * usize::from(channels);
        let mut buf = vec![0u8; 16 * 1024];
        let mut have = 0;
        let mut placer = Placer::new(self.cushion);
        let mut quiet = Duration::ZERO;
        let mut samples = Vec::with_capacity(8 * 1024);
        loop {
            if self.stopped() {
                bail!("the desk is stopping");
            }
            if self
                .shared
                .current
                .lock()
                .expect("sources lock")
                .get(identity)
                != Some(&generation)
            {
                bail!("a newer connection took over");
            }
            match reader.read(&mut buf[have..]) {
                Ok(0) => bail!("the ingest closed the connection"),
                Ok(n) => {
                    have += n;
                    quiet = Duration::ZERO;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    quiet += Duration::from_secs(2);
                    if quiet >= SILENT_DROP {
                        bail!("nothing for {} s", quiet.as_secs());
                    }
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
            let whole = have / frame_bytes * frame_bytes;
            if whole == 0 {
                continue;
            }
            samples.clear();
            to_stereo(&buf[..whole], channels, &mut samples);
            buf.copy_within(whole..have, 0);
            have -= whole;
            let Some(now) = running_frames(self.desk.pipeline()) else {
                continue;
            };
            let frames = (samples.len() / CHANNELS) as i64;
            let (pos, _) = placer.place(now, frames);
            ring.push(pos, &samples, now);
        }
    }
}

/// 32-bit float frames to stereo, mono heard in both ears; anything not finite is silence.
fn to_stereo(bytes: &[u8], channels: u16, out: &mut Vec<f32>) {
    for frame in bytes.chunks_exact(4 * usize::from(channels)) {
        let s = |i: usize| {
            let v = f32::from_le_bytes([
                frame[4 * i],
                frame[4 * i + 1],
                frame[4 * i + 2],
                frame[4 * i + 3],
            ]);
            if v.is_finite() {
                v.clamp(-4.0, 4.0)
            } else {
                0.0
            }
        };
        if channels == 1 {
            out.extend([s(0), s(0)]);
        } else {
            out.extend([s(0), s(1)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_request_names_its_identity_and_facts() {
        let head = "PUT /sources/source-abcd2345 HTTP/1.0\r\nX-Source-Name: Finlay\u{1b} Grant\r\nX-Source-Way: srt\r\nX-Source-Format: aac 48 kHz stereo\r\nX-Source-Key: abcd2345\r\nX-Source-Show: steeped\r\n\r\n";
        let r = parse_request(head).unwrap();
        assert_eq!(r.identity, "source-abcd2345");
        assert_eq!(r.name, "Finlay Grant");
        assert_eq!((r.way.as_str(), r.show.as_str()), ("srt", "steeped"));
    }

    #[test]
    fn only_a_source_identity_is_taken() {
        for path in [
            "/sources/hamish",
            "/sources/source-a%20b",
            "/feed",
            "/sources/",
        ] {
            let head = format!("PUT {path} HTTP/1.0\r\n\r\n");
            assert!(parse_request(&head).is_err(), "{path}");
        }
    }

    #[test]
    fn nan_and_runaway_samples_are_tamed() {
        let mut bytes = Vec::new();
        for v in [f32::NAN, 100.0, 0.5, f32::NEG_INFINITY] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = Vec::new();
        to_stereo(&bytes, 2, &mut out);
        assert_eq!(out, vec![0.0, 4.0, 0.5, 0.0]);
        out.clear();
        to_stereo(&0.25f32.to_le_bytes(), 1, &mut out);
        assert_eq!(out, vec![0.25, 0.25]);
    }
}
