// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Playout's output back to the desk (ADR-0172), for the video stream's sound while nobody is
//! live.
//!
//! Playout already sends its programme to the listener process as PCM in WAV over loopback,
//! with Icecast's source protocol (`output.icecast`, M4). It sends the desk a second copy the
//! same way, and the desk is the server: it listens, and playout connects and tries again
//! every few seconds while the desk is away. Either can restart without the other noticing
//! more than a gap in this feed; the desk's own output never waits for it (a desk outputs from
//! its first moment, spike 01), and nothing in playout's path to listeners waits for the desk
//! (playout sends it from a buffer on a clock of its own).
//!
//! Each connection starts with a WAV header, as `output.icecast` sends one each time it
//! connects. The samples go into an [`InputRing`] for the mixing pass, placed on the desk's
//! timeline as they arrive, a cushion ahead of the pass, and one after another from there:
//! playout and the desk keep the same wall-clock time, so the feed is placed afresh only when
//! it has drifted or paused by more than the cushion. A newer connection replaces an older
//! one (playout restarted before the old one timed out).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use weavesignal_mixer::InputRing;

use crate::config::PlayoutFeed;

const RATE: u32 = crate::pipeline::RATE;
const CHANNELS: usize = 2;

/// How long a connection may send nothing before it is dropped (playout gone without closing
/// it); playout connects again by itself.
const SILENT_DROP: Duration = Duration::from_secs(5);
/// The most a request's head may be.
const MAX_HEAD: usize = 8 * 1024;

/// The feed, as the mixing pass and the status see it.
pub struct Feed {
    ring: Arc<InputRing>,
    shared: Arc<Shared>,
}

struct Shared {
    stop: AtomicBool,
    /// The newest connection's number; an older one stops reading when it changes.
    generation: AtomicU64,
    connected: AtomicBool,
    connections: AtomicU64,
}

impl Feed {
    /// Listen for playout's feed on `settings.bind`:`settings.port`, placing what arrives on
    /// `pipeline`'s running time. A port that cannot be bound is tried again every few
    /// seconds; the desk goes on meanwhile, its programme as the video's sound.
    pub fn start(
        settings: &PlayoutFeed,
        password: Option<String>,
        pipeline: &gst::Pipeline,
    ) -> Result<Feed> {
        let ring = Arc::new(InputRing::new(CHANNELS, RATE as usize));
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            connected: AtomicBool::new(false),
            connections: AtomicU64::new(0),
        });
        if password.is_none() {
            tracing::warn!(
                env = crate::config::FEED_PASSWORD_ENV,
                "the feed from playout has no password: any process on this machine may send the video's sound"
            );
        }
        let addr = SocketAddr::new(settings.bind, settings.port);
        let ctx = Conn {
            ring: ring.clone(),
            shared: shared.clone(),
            password,
            pipeline: pipeline.downgrade(),
            cushion: i64::from(settings.cushion_ms) * i64::from(RATE) / 1000,
        };
        std::thread::Builder::new()
            .name("desk-feed".into())
            .spawn(move || ctx.listen(addr))?;
        Ok(Feed { ring, shared })
    }

    /// Where the feed's samples wait for the mixing pass.
    pub fn ring(&self) -> &InputRing {
        &self.ring
    }

    /// Whether playout is connected now.
    pub fn connected(&self) -> bool {
        self.shared.connected.load(Ordering::Relaxed)
    }

    /// Connections taken since the desk started.
    pub fn connections(&self) -> u64 {
        self.shared.connections.load(Ordering::Relaxed)
    }

    /// Stop listening, and drop the connection.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Conn {
    ring: Arc<InputRing>,
    shared: Arc<Shared>,
    password: Option<String>,
    pipeline: glib::WeakRef<gst::Pipeline>,
    cushion: i64,
}

impl Conn {
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
                        tracing::error!(%addr, error = %e, "cannot listen for the feed from playout; trying again every 5 s (the video's sound is the desk's own meanwhile)");
                        warned = true;
                    }
                    for _ in 0..50 {
                        if self.stopped() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        };
        tracing::info!(%addr, "listening for the feed from playout (ADR-0172)");
        while !self.stopped() {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let c = self.clone();
                    let spawned = std::thread::Builder::new()
                        .name("desk-feed-in".into())
                        .spawn(move || c.serve(stream, peer));
                    if let Err(e) = spawned {
                        tracing::warn!(error = %e, "cannot start a thread for the feed from playout");
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the feed's listener");
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }

    fn serve(self, stream: TcpStream, peer: SocketAddr) {
        if let Err(e) = self.receive(stream) {
            tracing::info!(%peer, error = %format!("{e:#}"), "the feed from playout ended");
        }
    }

    fn receive(&self, mut stream: TcpStream) -> Result<()> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let _ = stream.set_nodelay(true);
        let mut reader = BufReader::new(stream.try_clone()?);
        let head = read_head(&mut reader)?;
        match check_request(&head, self.password.as_deref()) {
            Ok(()) => stream.write_all(b"HTTP/1.0 200 OK\r\nServer: weavesignal-desk\r\n\r\n")?,
            Err(refusal) => {
                let _ = stream.write_all(refusal.response().as_bytes());
                bail!("refused: {}", refusal.why());
            }
        }
        let wav = read_wav_header(&mut reader)?;
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.connections.fetch_add(1, Ordering::Relaxed);
        self.shared.connected.store(true, Ordering::Relaxed);
        tracing::info!(channels = wav.channels, "the feed from playout connected");
        let result = self.read_pcm(&mut reader, &wav, generation);
        if self.shared.generation.load(Ordering::Acquire) == generation {
            self.shared.connected.store(false, Ordering::Relaxed);
        }
        result
    }

    fn read_pcm(&self, reader: &mut impl Read, wav: &Wav, generation: u64) -> Result<()> {
        let frame_bytes = 2 * usize::from(wav.channels);
        let mut buf = vec![0u8; 4096];
        let mut have = 0;
        let mut placer = Placer::new(self.cushion);
        let mut quiet = Duration::ZERO;
        let mut samples = Vec::with_capacity(4096);
        loop {
            if self.shared.generation.load(Ordering::Acquire) != generation {
                bail!("a newer connection took over");
            }
            match reader.read(&mut buf[have..]) {
                Ok(0) => bail!("playout closed the connection"),
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
            to_stereo_f32(&buf[..whole], wav.channels, &mut samples);
            buf.copy_within(whole..have, 0);
            have -= whole;
            let Some(pipeline) = self.pipeline.upgrade() else {
                bail!("the desk is gone");
            };
            let Some(now) = running_frames(&pipeline) else {
                continue;
            };
            let frames = (samples.len() / CHANNELS) as i64;
            let pos = placer.place(now, frames);
            self.ring.push(pos, &samples, now);
        }
    }
}

/// The pipeline's running time now, in frames; `None` before it has a clock.
pub(crate) fn running_frames(p: &gst::Pipeline) -> Option<i64> {
    let (clock, base) = (p.clock()?, p.base_time()?);
    let t = clock.time().checked_sub(base)?;
    Some((t.nseconds() as i128 * i128::from(RATE) / 1_000_000_000) as i64)
}

/// Where each piece of the feed goes on the timeline: a cushion ahead of now when it first
/// arrives, then straight after the piece before, until that has drifted from arrival by more
/// than the cushion (a pause, a burst after a stall, or the clocks parting).
struct Placer {
    next: Option<i64>,
    cushion: i64,
}

impl Placer {
    fn new(cushion: i64) -> Self {
        Placer {
            next: None,
            cushion,
        }
    }

    fn place(&mut self, now: i64, frames: i64) -> i64 {
        let fresh = now + self.cushion - frames;
        let pos = match self.next {
            Some(p) if (p - fresh).abs() <= self.cushion => p,
            _ => fresh,
        };
        self.next = Some(pos + frames);
        pos
    }
}

fn to_stereo_f32(bytes: &[u8], channels: u16, out: &mut Vec<f32>) {
    let scale = 1.0 / 32768.0;
    for frame in bytes.chunks_exact(2 * usize::from(channels)) {
        let s = |i: usize| f32::from(i16::from_le_bytes([frame[2 * i], frame[2 * i + 1]])) * scale;
        if channels == 1 {
            out.extend([s(0), s(0)]);
        } else {
            out.extend([s(0), s(1)]);
        }
    }
}

/// A request's head: everything before the blank line.
fn read_head(reader: &mut impl BufRead) -> Result<String> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = (&mut *reader)
            .take(MAX_HEAD as u64)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            bail!("the connection closed before its request");
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            bail!("a request's head over {MAX_HEAD} bytes");
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Method(String),
    Password,
}

impl Refusal {
    fn response(&self) -> &'static str {
        match self {
            Refusal::Method(_) => "HTTP/1.0 405 Method Not Allowed\r\n\r\n",
            Refusal::Password => {
                "HTTP/1.0 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"weavesignal-desk\"\r\n\r\n"
            }
        }
    }

    fn why(&self) -> String {
        match self {
            Refusal::Method(m) => format!("`{m}` is not a source request"),
            Refusal::Password => "wrong or missing source password".into(),
        }
    }
}

/// A source request (`SOURCE`, or `PUT` as newer Icecast clients send), with the password
/// when the desk has one: HTTP basic authentication, any user name.
fn check_request(head: &str, password: Option<&str>) -> Result<(), Refusal> {
    let first = head.lines().next().unwrap_or_default();
    let method = first.split_whitespace().next().unwrap_or_default();
    if !matches!(method, "SOURCE" | "PUT") {
        return Err(Refusal::Method(method.chars().take(16).collect()));
    }
    let Some(want) = password else {
        return Ok(());
    };
    let given = head.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("authorization")
            .then(|| value.trim().to_owned())
    });
    let ok = given
        .as_deref()
        .and_then(|v| {
            v.strip_prefix("Basic ")
                .or_else(|| v.strip_prefix("basic "))
        })
        .and_then(|b64| base64_decode(b64.trim()))
        .and_then(|raw| String::from_utf8(raw).ok())
        .and_then(|cred| cred.split_once(':').map(|(_, p)| p.to_owned()))
        .is_some_and(|p| same(p.as_bytes(), want.as_bytes()));
    if ok { Ok(()) } else { Err(Refusal::Password) }
}

/// Equal, in a time that depends only on the lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        let len = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        out.extend_from_slice(&n.to_be_bytes()[1..1 + len]);
    }
    Some(out)
}

/// The feed's format, from its WAV header.
#[derive(Debug, PartialEq, Eq)]
struct Wav {
    channels: u16,
}

/// Read a WAV header up to the start of its samples: 16-bit PCM at 48 kHz, mono or stereo,
/// as playout sends (`%wav(stereo = true, samplerate = 48000)`). A streamed header gives no
/// real length, so the length is not read.
fn read_wav_header(reader: &mut impl Read) -> Result<Wav> {
    let mut riff = [0u8; 12];
    reader.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        bail!("the feed is not WAV");
    }
    let mut channels = None;
    loop {
        let mut chunk = [0u8; 8];
        reader.read_exact(&mut chunk)?;
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        match &chunk[0..4] {
            b"fmt " => {
                if !(16..=64).contains(&size) {
                    bail!("a WAV fmt chunk of {size} bytes");
                }
                let mut fmt = vec![0u8; size as usize + (size as usize & 1)];
                reader.read_exact(&mut fmt)?;
                let word = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
                let (format, ch, bits) = (word(0), word(2), word(14));
                let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                // 1: PCM; 0xFFFE: the extensible form, which carries PCM here.
                if !matches!(format, 1 | 0xFFFE) || bits != 16 || rate != RATE || ch == 0 || ch > 2
                {
                    bail!(
                        "the feed is {ch} channels of {bits}-bit format {format} at {rate} Hz; the desk takes 16-bit PCM at 48 kHz, mono or stereo"
                    );
                }
                channels = Some(ch);
            }
            b"data" => {
                return match channels {
                    Some(channels) => Ok(Wav { channels }),
                    None => bail!("a WAV data chunk before its fmt chunk"),
                };
            }
            _ => {
                if size > 1 << 20 {
                    bail!("a WAV chunk of {size} bytes before the samples");
                }
                let skip = u64::from(size) + u64::from(size & 1);
                io::copy(&mut (&mut *reader).take(skip), &mut io::sink())?;
            }
        }
    }
}

/// A WAV header for a streamed feed, as playout's `%wav` writes one: for tests and fixtures.
pub fn wav_header(channels: u16) -> Vec<u8> {
    let mut h = Vec::with_capacity(44);
    let block = 2 * channels;
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&u32::MAX.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * u32::from(block)).to_le_bytes());
    h.extend_from_slice(&block.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&u32::MAX.to_le_bytes());
    h
}

/// `Basic …` for a user and password, as a source client sends it: for tests and fixtures.
pub fn basic_auth(user: &str, password: &str) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("{user}:{password}");
    let mut out = String::from("Basic ");
    for chunk in raw.as_bytes().chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_request_is_taken_with_the_password_and_refused_without() {
        let head = |auth: &str| {
            format!(
                "SOURCE /feed HTTP/1.0\r\nAuthorization: {auth}\r\nContent-Type: audio/wav\r\n\r\n"
            )
        };
        let good = head(&basic_auth("source", "hackme too"));
        assert_eq!(check_request(&good, Some("hackme too")), Ok(()));
        assert_eq!(check_request(&good, None), Ok(()));
        assert_eq!(
            check_request(&head(&basic_auth("source", "wrong")), Some("hackme too")),
            Err(Refusal::Password)
        );
        assert_eq!(
            check_request("SOURCE /feed HTTP/1.0\r\n\r\n", Some("x")),
            Err(Refusal::Password)
        );
        assert!(matches!(
            check_request("GET /admin/metadata HTTP/1.0\r\n\r\n", None),
            Err(Refusal::Method(_))
        ));
        let put = format!(
            "PUT /feed HTTP/1.1\r\nauthorization: {}\r\n\r\n",
            basic_auth("u", "p")
        );
        assert_eq!(check_request(&put, Some("p")), Ok(()));
    }

    #[test]
    fn base64_round_trips() {
        for s in ["", "a", "ab", "abc", "source:hackme", "x:ünï"] {
            let enc = basic_auth("", s);
            let dec = base64_decode(enc.strip_prefix("Basic ").unwrap()).unwrap();
            assert_eq!(String::from_utf8(dec).unwrap(), format!(":{s}"));
        }
        assert_eq!(base64_decode("!!!!"), None);
    }

    #[test]
    fn a_wav_header_is_read_to_its_samples() {
        let mut bytes = wav_header(2);
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        let mut r = &bytes[..];
        assert_eq!(read_wav_header(&mut r).unwrap(), Wav { channels: 2 });
        assert_eq!(r, &[1, 2, 3, 4]);
        // A chunk between fmt and data (LIST) is skipped.
        let mut with_list = wav_header(1);
        let data_at = with_list.len() - 8;
        let tail = with_list.split_off(data_at);
        with_list.extend_from_slice(b"LIST\x03\x00\x00\x00abc\x00");
        with_list.extend_from_slice(&tail);
        assert_eq!(
            read_wav_header(&mut &with_list[..]).unwrap(),
            Wav { channels: 1 }
        );
        // 44.1 kHz is refused.
        let mut other = wav_header(2);
        other[24..28].copy_from_slice(&44_100u32.to_le_bytes());
        assert!(read_wav_header(&mut &other[..]).is_err());
        assert!(read_wav_header(&mut &b"OggS0000000000000000"[..]).is_err());
    }

    #[test]
    fn mono_is_heard_in_both_ears() {
        let mut out = Vec::new();
        to_stereo_f32(&[0x00, 0x40, 0x00, 0xc0], 1, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        out.clear();
        to_stereo_f32(&[0x00, 0x40, 0x00, 0xc0], 2, &mut out);
        assert_eq!(out, [0.5, -0.5]);
    }

    #[test]
    fn the_feed_is_placed_a_cushion_ahead_and_then_back_to_back() {
        let mut p = Placer::new(9_600);
        // The first piece ends a cushion after it arrived.
        assert_eq!(p.place(100_000, 1_920), 100_000 + 9_600 - 1_920);
        // Jitter of a few ms in arrival: still back to back.
        assert_eq!(p.place(101_800, 1_920), 100_000 + 9_600);
        assert_eq!(p.place(104_200, 1_920), 100_000 + 9_600 + 1_920);
        // A pause of a second: placed afresh.
        assert_eq!(p.place(200_000, 1_920), 200_000 + 9_600 - 1_920);
    }
}
