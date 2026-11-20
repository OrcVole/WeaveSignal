// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The bridge's local side (Phase 17g): the studio app stands in for the station on this
//! computer, so the host's own software (BUTT, Mixxx, OBS, Voicemeeter) sends to it as it would
//! to any Icecast server or SRT listener, and the app carries the bytes on ([`crate::bridge_up`]).
//!
//! - **Icecast** on `127.0.0.1` (8100 by default): a `PUT` or `SOURCE` with basic
//!   authentication, any user, the **local password** the app made; `Content-Type` names the
//!   format. Titles come as `GET /admin/metadata?mode=updinfo&song=…`, as Icecast takes them.
//! - **SRT** on `127.0.0.1` (9100 by default): a caller with the local password as its
//!   passphrase; MPEG-TS.
//!
//! Loopback only, and a password, so another program on the computer cannot speak through the
//! bridge, and a web page cannot reach it (`PUT` and `SOURCE` need a preflight it never
//! answers). One source at a time; nothing here decodes a byte.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use futures::StreamExt;
use srt_protocol::options::KeySize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

pub const ICECAST_PORT: u16 = 8100;
pub const SRT_PORT: u16 = 9100;
const MAX_HEAD: usize = 8 * 1024;
/// A local source that sends nothing for this long has gone.
const SILENT_END: Duration = Duration::from_secs(10);

/// What the local side hands the uplink.
pub enum Event {
    /// The host's software connected: what it sends, and its bytes until it closes.
    Source {
        format: &'static str,
        software: String,
        media: mpsc::Receiver<Bytes>,
    },
    /// A title from the host's software.
    Title(String),
}

/// A request's head, as the local side reads it.
#[derive(Debug, PartialEq)]
pub enum Head {
    Source {
        format: &'static str,
        software: String,
    },
    Metadata {
        title: String,
    },
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The worker's format for a `Content-Type`, as the station names them.
pub fn format_of(content_type: &str) -> Option<&'static str> {
    let t = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match t.as_str() {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" => Some("mp3"),
        "audio/aac" | "audio/aacp" | "audio/x-aac" => Some("aac"),
        "application/ogg" | "audio/ogg" | "audio/opus" | "audio/vorbis" | "audio/x-vorbis+ogg" => {
            Some("ogg")
        }
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "video/mp2t" | "video/mpegts" => Some("mpegts"),
        _ => None,
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read an Icecast request's head against the local password: `Err` with the status and a
/// sentence for the host's software.
pub fn parse(head: &str, password: &str) -> Result<Head, (u16, &'static str)> {
    let first = head.lines().next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or("/"),
    );
    let is_source = matches!(method, "PUT" | "SOURCE");
    let is_meta = method == "GET" && target.starts_with("/admin/metadata");
    if !is_source && !is_meta {
        return Err((405, "This takes an Icecast source only."));
    }
    let given = header(head, "authorization")
        .and_then(|v| {
            v.strip_prefix("Basic ")
                .or_else(|| v.strip_prefix("basic "))
        })
        .and_then(|b| {
            base64::engine::general_purpose::STANDARD
                .decode(b.trim())
                .ok()
        })
        .and_then(|raw| String::from_utf8(raw).ok());
    let ok = given
        .as_deref()
        .and_then(|t| t.split_once(':'))
        .is_some_and(|(_, p)| same(p.as_bytes(), password.as_bytes()));
    if !ok {
        return Err((
            401,
            "The password is wrong: use the local password the studio app shows.",
        ));
    }
    if is_meta {
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
        let song = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("song="))
            .map(unescape)
            .unwrap_or_default();
        return Ok(Head::Metadata {
            title: clean(&song),
        });
    }
    let format = header(head, "content-type")
        .and_then(format_of)
        .ok_or((415, "Set your software to MP3, AAC, Ogg, FLAC or MPEG-TS."))?;
    Ok(Head::Source {
        format,
        software: clean(header(head, "user-agent").unwrap_or_default()),
    })
}

fn answer(status: u16, why: &str) -> String {
    let text = match status {
        401 => "Unauthorized",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        415 => "Unsupported Media Type",
        _ => "Service Unavailable",
    };
    let auth = if status == 401 {
        "WWW-Authenticate: Basic realm=\"WeaveSignal bridge\"\r\n"
    } else {
        ""
    };
    format!(
        "HTTP/1.0 {status} {text}\r\nServer: weavesignal-studio\r\n{auth}Content-Length: {}\r\n\r\n{why}\n",
        why.len() + 1
    )
}

/// Only one source at a time; cleared when its connection ends.
struct Busy(Arc<AtomicBool>);

impl Busy {
    fn take(flag: &Arc<AtomicBool>) -> Option<Busy> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Busy(flag.clone()))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The local listeners, sharing one source at a time.
#[derive(Clone)]
pub struct Local {
    password: Arc<String>,
    busy: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
}

impl Local {
    pub fn new(password: String, events: mpsc::Sender<Event>) -> Self {
        Local {
            password: Arc::new(password),
            busy: Arc::new(AtomicBool::new(false)),
            events,
        }
    }

    /// Bind the Icecast side on loopback (port 0: any free one); the address it took.
    pub async fn icecast(
        &self,
        port: u16,
    ) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let l = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
        let at = l.local_addr()?;
        let me = self.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((s, peer)) = l.accept().await else {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let m = me.clone();
                tokio::spawn(async move { m.icecast_conn(s).await });
            }
        });
        Ok((at, task))
    }

    async fn icecast_conn(&self, mut s: TcpStream) {
        let mut buf = Vec::with_capacity(1024);
        let mut piece = [0u8; 1024];
        let rest = loop {
            let n = match tokio::time::timeout(Duration::from_secs(10), s.read(&mut piece)).await {
                Ok(Ok(n)) if n > 0 => n,
                _ => return,
            };
            buf.extend_from_slice(&piece[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break buf.split_off(end + 4);
            }
            if buf.len() > MAX_HEAD {
                return;
            }
        };
        let head = String::from_utf8_lossy(&buf).into_owned();
        match parse(&head, &self.password) {
            Err((status, why)) => {
                let _ = s.write_all(answer(status, why).as_bytes()).await;
            }
            Ok(Head::Metadata { title }) => {
                let _ = self.events.send(Event::Title(title)).await;
                let _ = s
                    .write_all(b"HTTP/1.0 200 OK\r\nContent-Type: text/xml\r\n\r\n<?xml version=\"1.0\"?>\n<iceresponse><message>Metadata update successful</message><return>1</return></iceresponse>\n")
                    .await;
            }
            Ok(Head::Source { format, software }) => {
                let Some(busy) = Busy::take(&self.busy) else {
                    let _ = s
                        .write_all(
                            answer(
                                403,
                                "Another program is already sending through the bridge.",
                            )
                            .as_bytes(),
                        )
                        .await;
                    return;
                };
                if s.write_all(b"HTTP/1.0 200 OK\r\nServer: weavesignal-studio\r\n\r\n")
                    .await
                    .is_err()
                {
                    return;
                }
                let (tx, media) = mpsc::channel(64);
                if self
                    .events
                    .send(Event::Source {
                        format,
                        software,
                        media,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                let _busy = busy;
                if !rest.is_empty() && tx.send(Bytes::from(rest)).await.is_err() {
                    return;
                }
                let mut b = vec![0u8; 16 * 1024];
                loop {
                    match tokio::time::timeout(SILENT_END, s.read(&mut b)).await {
                        Ok(Ok(n)) if n > 0 => {
                            if tx.send(Bytes::copy_from_slice(&b[..n])).await.is_err() {
                                return;
                            }
                        }
                        _ => return,
                    }
                }
            }
        }
    }

    /// Bind the SRT side on loopback, the local password as its passphrase.
    pub async fn srt(
        &self,
        port: u16,
    ) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let (listener, mut incoming) = srt_tokio::SrtListener::builder().bind(at).await?;
        let me = self.clone();
        let task = tokio::spawn(async move {
            let _keep = listener;
            while let Some(request) = incoming.incoming().next().await {
                if !request.remote().ip().is_loopback() {
                    continue;
                }
                let Some(busy) = Busy::take(&me.busy) else {
                    let _ = request
                        .reject(srt_protocol::packet::RejectReason::Server(
                            srt_protocol::packet::ServerRejectReason::Conflict,
                        ))
                        .await;
                    continue;
                };
                // A caller without the passphrase is refused; with it, the handshake proves it.
                let reject = |r| srt_protocol::packet::RejectReason::Server(r);
                let pass = srt_protocol::options::Passphrase::try_from(me.password.to_string());
                let (false, Ok(passphrase)) = (request.key_size() == KeySize::Unspecified, pass)
                else {
                    let _ = request
                        .reject(reject(
                            srt_protocol::packet::ServerRejectReason::Unauthorized,
                        ))
                        .await;
                    continue;
                };
                let keys = srt_protocol::settings::KeySettings {
                    key_size: request.key_size(),
                    passphrase,
                };
                let Ok(mut socket) = request.accept(Some(keys)).await else {
                    continue;
                };
                let (tx, media) = mpsc::channel(256);
                if me
                    .events
                    .send(Event::Source {
                        format: "mpegts",
                        software: "SRT".into(),
                        media,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::spawn(async move {
                    let _busy = busy;
                    while let Ok(Some(Ok((_, data)))) =
                        tokio::time::timeout(SILENT_END, socket.next()).await
                    {
                        if tx.send(data).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        Ok((at, task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(password: &str) -> String {
        format!(
            "Authorization: Basic {}\r\n",
            base64::engine::general_purpose::STANDARD.encode(format!("source:{password}"))
        )
    }

    #[test]
    fn a_butt_head_with_the_local_password_is_a_source() {
        let head = format!(
            "SOURCE /live ICE/1.0\r\nContent-Type: audio/ogg\r\nUser-Agent: butt 1.45.0\r\n{}\r\n",
            auth("localpass12345")
        );
        assert_eq!(
            parse(&head, "localpass12345"),
            Ok(Head::Source {
                format: "ogg",
                software: "butt 1.45.0".into()
            })
        );
        assert_eq!(parse(&head, "another-pass").unwrap_err().0, 401);
        let no_auth = "PUT /live HTTP/1.1\r\nContent-Type: audio/mpeg\r\n\r\n";
        assert_eq!(parse(no_auth, "localpass12345").unwrap_err().0, 401);
        let wma = format!(
            "PUT /live HTTP/1.1\r\nContent-Type: audio/x-ms-wma\r\n{}\r\n",
            auth("localpass12345")
        );
        assert_eq!(parse(&wma, "localpass12345").unwrap_err().0, 415);
        assert_eq!(parse("GET / HTTP/1.1\r\n\r\n", "x").unwrap_err().0, 405);
        let meta = format!(
            "GET /admin/metadata?mode=updinfo&song=Runrig+-+Loch%20Lomond HTTP/1.0\r\n{}\r\n",
            auth("localpass12345")
        );
        assert_eq!(
            parse(&meta, "localpass12345"),
            Ok(Head::Metadata {
                title: "Runrig - Loch Lomond".into()
            })
        );
    }

    #[tokio::test]
    async fn over_loopback_a_source_is_taken_once_and_a_wrong_password_refused() {
        let (tx, mut events) = mpsc::channel(8);
        let local = Local::new("localpass12345".into(), tx);
        let (at, _task) = local.icecast(0).await.unwrap();
        assert!(at.ip().is_loopback(), "bound beyond loopback: {at}");
        // A wrong password.
        let mut bad = TcpStream::connect(at).await.unwrap();
        bad.write_all(
            format!(
                "PUT /live HTTP/1.1\r\nContent-Type: audio/mpeg\r\n{}\r\n",
                auth("guess")
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut got = String::new();
        bad.read_to_string(&mut got).await.unwrap();
        assert!(got.starts_with("HTTP/1.0 401"), "{got}");
        // The right one: its bytes, the head's own included.
        let mut good = TcpStream::connect(at).await.unwrap();
        good.write_all(
            format!(
                "PUT /live HTTP/1.1\r\nContent-Type: audio/mpeg\r\nUser-Agent: Mixxx\r\n{}\r\nID3",
                auth("localpass12345")
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let Some(Event::Source {
            format,
            software,
            mut media,
        }) = events.recv().await
        else {
            panic!("no source")
        };
        assert_eq!((format, software.as_str()), ("mp3", "Mixxx"));
        assert_eq!(&media.recv().await.unwrap()[..], b"ID3");
        // A second program is refused while the first sends.
        let mut second = TcpStream::connect(at).await.unwrap();
        second
            .write_all(
                format!(
                    "PUT /live HTTP/1.1\r\nContent-Type: audio/mpeg\r\n{}\r\n",
                    auth("localpass12345")
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut got = String::new();
        second.read_to_string(&mut got).await.unwrap();
        assert!(got.starts_with("HTTP/1.0 403"), "{got}");
        // The first leaves; another may come.
        drop(good);
        assert!(media.recv().await.is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut third = TcpStream::connect(at).await.unwrap();
        third
            .write_all(
                format!(
                    "PUT /live HTTP/1.1\r\nContent-Type: audio/aac\r\n{}\r\n",
                    auth("localpass12345")
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        assert!(matches!(
            events.recv().await,
            Some(Event::Source { format: "aac", .. })
        ));
    }

    #[tokio::test]
    async fn the_srt_side_is_on_loopback_with_the_passphrase() {
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (tx, mut events) = mpsc::channel(8);
        let local = Local::new("localpass12345".into(), tx);
        let (at, _task) = local.srt(port).await.unwrap();
        assert!(at.ip().is_loopback(), "bound beyond loopback: {at}");
        // The wrong passphrase never connects.
        let wrong = tokio::time::timeout(
            Duration::from_secs(3),
            srt_tokio::SrtSocket::builder()
                .encryption(16, "not-the-password")
                .call(at, None),
        )
        .await;
        assert!(!matches!(wrong, Ok(Ok(_))));
        // The local password does, and its bytes arrive as MPEG-TS.
        let mut caller = srt_tokio::SrtSocket::builder()
            .encryption(16, "localpass12345")
            .call(at, None)
            .await
            .unwrap();
        use futures::SinkExt;
        caller
            .send((std::time::Instant::now(), Bytes::from_static(&[0x47; 188])))
            .await
            .unwrap();
        let Some(Event::Source {
            format, mut media, ..
        }) = events.recv().await
        else {
            panic!("no source")
        };
        assert_eq!(format, "mpegts");
        assert_eq!(media.recv().await.unwrap().len(), 188);
    }
}
