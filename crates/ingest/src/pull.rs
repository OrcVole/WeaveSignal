// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Pulled sources (ADR-0280, PLAN 17b): the station opens a stream itself, from an address kept
//! with a show key, instead of waiting for a host's software to connect. What it opens comes back
//! as an [`Opened`]: the worker's format and the bytes on a channel, which the supervisor hands to
//! the same session, decoding worker and desk link a pushed source has.
//!
//! | Address | How | Format |
//! |---|---|---|
//! | `http://` or `https://` | `GET`, the body as it comes (Icecast, any HTTP audio) | from `Content-Type` ([`crate::icecast::format_of`]) |
//! | `….m3u8` | the playlist read again and again, each new segment fetched ([`crate::hls`]) | MPEG-TS, AAC or MP3 segments, by their first bytes |
//! | `srt://host:port` | the station calls the listener, with any `streamid=` | MPEG-TS |
//!
//! **The guard.** Every connection goes to an address that was checked after its name resolved
//! ([`weavesignal_proto::pull::check_resolved`]): for HTTP the agent's own resolver refuses a name
//! with any address inside the station's own network, so a redirect, a playlist's variant and
//! every segment pass the same check; for SRT the ingest resolves, checks, and calls only the
//! addresses it checked. The name itself is checked too (`localhost`, `.internal`). The control
//! plane says whether the installation allows its own network (`WEAVESIGNAL_PULL_ALLOW_PRIVATE`).
//!
//! HTTPS is rustls with ring and Mozilla's roots (webpki-roots), the operator's pick (2026-11-20).

use std::io::Read;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use srt_tokio::SrtSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use weavesignal_proto::pull::{
    PullKind, PullTarget, PullUrlError, check_resolved, is_private_host, parse_pull_url,
};

/// How long a name may take to resolve.
const RESOLVE: Duration = Duration::from_secs(5);
/// How long a connection, and the server's first answer, may take.
const CONNECT: Duration = Duration::from_secs(10);
/// A stream that sends nothing for this long has ended.
pub const SILENT_END: Duration = Duration::from_secs(10);
/// Redirects followed for one request, each through the guard.
const REDIRECTS: u32 = 5;

/// Why a pull could not be opened, or ended.
#[derive(Debug, thiserror::Error)]
pub enum PullError {
    #[error(transparent)]
    Address(#[from] PullUrlError),
    #[error("an srt:// address is not a web address")]
    NotHttp,
    #[error("the name {0} does not resolve")]
    NoName(String),
    #[error("could not connect to {0}")]
    Connect(String),
    #[error("the server answered {0}")]
    Status(u16),
    #[error("the station cannot read this stream's type ({0}); MP3, AAC, Ogg or FLAC can be read")]
    ContentType(String),
    #[error("the playlist cannot be read: {0}")]
    Playlist(String),
    #[error("{0}")]
    Segment(String),
    #[error("{0}")]
    Http(String),
}

impl PullError {
    fn from_ureq(e: ureq::Error) -> Self {
        match e {
            ureq::Error::Other(inner) => match inner.downcast::<PullUrlError>() {
                Ok(p) => PullError::Address(*p),
                Err(other) => PullError::Http(other.to_string()),
            },
            ureq::Error::HostNotFound => PullError::Http("the name does not resolve".into()),
            ureq::Error::StatusCode(c) => PullError::Status(c),
            other => PullError::Http(other.to_string()),
        }
    }
}

// ---- the guard, in the agent's resolver ----

/// The default resolver, then the guard on the name and on every address it gave.
#[derive(Debug)]
struct Guarded {
    allow_private: bool,
}

impl Resolver for Guarded {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if !self.allow_private && uri.host().is_some_and(is_private_host) {
            return Err(ureq::Error::Other(Box::new(PullUrlError::Private)));
        }
        let addrs = DefaultResolver::default().resolve(uri, config, timeout)?;
        let ips: Vec<IpAddr> = addrs.iter().map(|a| a.ip()).collect();
        check_resolved(&ips, self.allow_private).map_err(|e| ureq::Error::Other(Box::new(e)))?;
        Ok(addrs)
    }
}

/// An HTTP(S) client whose every connection has passed the guard. Its calls block: run them on
/// a blocking thread.
pub struct Fetcher {
    agent: ureq::Agent,
    allow_private: bool,
}

/// A response that is the stream: its address after any redirect, its type, and its body.
pub struct Response {
    pub url: String,
    pub content_type: Option<String>,
    pub body: Box<dyn Read + Send>,
}

impl Fetcher {
    /// With Mozilla's roots.
    pub fn new(allow_private: bool) -> Self {
        Self::with_roots(allow_private, RootCerts::WebPki)
    }

    /// With the roots given (a test's own certificate).
    pub fn with_roots(allow_private: bool, roots: RootCerts) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // A proxy from the environment would be resolved, and checked, in the stream's place.
            .proxy(None)
            .max_redirects(REDIRECTS)
            .user_agent(concat!("WeaveSignal/", env!("CARGO_PKG_VERSION")))
            .timeout_resolve(Some(RESOLVE))
            .timeout_connect(Some(CONNECT))
            .timeout_recv_response(Some(CONNECT))
            .timeout_per_read(Some(SILENT_END))
            .tls_config(TlsConfig::builder().root_certs(roots).build())
            .build();
        Fetcher {
            agent: ureq::Agent::with_parts(
                config,
                DefaultConnector::default(),
                Guarded { allow_private },
            ),
            allow_private,
        }
    }

    /// Check `url` as an address a pull may open over HTTP(S).
    pub fn target(&self, url: &str) -> Result<PullTarget, PullError> {
        let t = parse_pull_url(url, self.allow_private)?;
        if t.kind == PullKind::Srt {
            return Err(PullError::NotHttp);
        }
        Ok(t)
    }

    /// `GET` `url`: a 200 with its body, or why not.
    pub fn open(&self, url: &str) -> Result<Response, PullError> {
        use ureq::ResponseExt;
        let t = self.target(url)?;
        let wire = t.url.split('#').next().unwrap_or(&t.url).to_owned();
        let res = self.agent.get(&wire).call().map_err(PullError::from_ureq)?;
        let status = res.status().as_u16();
        if status != 200 {
            return Err(PullError::Status(status));
        }
        let url = res.get_uri().to_string();
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(Response {
            url,
            content_type,
            body: Box::new(res.into_body().into_reader()),
        })
    }

    /// `GET` `url` whole, up to `limit` bytes: its address after any redirect, and the bytes.
    pub fn fetch(&self, url: &str, limit: u64) -> Result<(String, Vec<u8>), PullError> {
        let r = self.open(url)?;
        let mut body = Vec::new();
        r.body
            .take(limit + 1)
            .read_to_end(&mut body)
            .map_err(|e| PullError::Http(e.to_string()))?;
        if body.len() as u64 > limit {
            return Err(PullError::Http(format!("more than {limit} bytes")));
        }
        Ok((r.url, body))
    }
}

// ---- what a pull hands the session ----

/// A pull that is open: what the worker reads, and the bytes as they come.
pub struct Opened {
    /// The worker's format: `mp3`, `aac`, `ogg`, `flac` or `mpegts`.
    pub format: &'static str,
    pub input: mpsc::Receiver<Bytes>,
    /// The address for the log, `host:port`.
    pub address: String,
    /// A task feeding `input`, ended with the session.
    pub task: Option<JoinHandle<()>>,
}

impl Drop for Opened {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

/// The log's address for a target: `host:port`, cut to 64 characters.
pub fn address_of(t: &PullTarget) -> String {
    let host = if t.host.contains(':') {
        format!("[{}]", t.host)
    } else {
        t.host.clone()
    };
    format!("{host}:{}", t.port).chars().take(64).collect()
}

/// Open the pull at `url`, as its kind says.
pub async fn open(url: &str, allow_private: bool) -> Result<Opened, PullError> {
    let t = parse_pull_url(url, allow_private)?;
    match t.kind {
        PullKind::Srt => open_srt(&t, allow_private).await,
        PullKind::Http => open_http(Arc::new(Fetcher::new(allow_private)), t).await,
        PullKind::Hls => crate::hls::open(Arc::new(Fetcher::new(allow_private)), t).await,
    }
}

/// An HTTP(S) stream: the response's head on this side, its body read on a blocking thread
/// that ends when the stream does, when nothing comes for [`SILENT_END`], or when the session
/// drops the channel.
pub async fn open_http(fetcher: Arc<Fetcher>, t: PullTarget) -> Result<Opened, PullError> {
    let (tx, rx) = mpsc::channel::<Bytes>(64);
    let (head_tx, head_rx) = oneshot::channel::<Result<&'static str, PullError>>();
    let url = t.url.clone();
    tokio::task::spawn_blocking(move || {
        let r = match fetcher.open(&url) {
            Ok(r) => r,
            Err(e) => {
                let _ = head_tx.send(Err(e));
                return;
            }
        };
        let format = crate::icecast::format_of(r.content_type.as_deref().unwrap_or_default())
            .map_err(PullError::ContentType);
        let ok = format.is_ok();
        let _ = head_tx.send(format);
        if !ok {
            return;
        }
        let mut body = r.body;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match body.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if tx.blocking_send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let format = head_rx
        .await
        .map_err(|_| PullError::Http("the reader stopped".into()))??;
    Ok(Opened {
        format,
        input: rx,
        address: address_of(&t),
        task: None,
    })
}

/// An SRT listener, called: its name resolved and every address checked, then each called in
/// turn with the address's `streamid` (the access-control form `#!::r=…,m=request` is passed as
/// written). The listener sends MPEG-TS. An address with a passphrase never gets here: the control
/// plane does not keep one.
pub async fn open_srt(t: &PullTarget, allow_private: bool) -> Result<Opened, PullError> {
    let found = tokio::time::timeout(RESOLVE, tokio::net::lookup_host((t.host.as_str(), t.port)))
        .await
        .map_err(|_| PullError::NoName(t.host.clone()))?
        .map_err(|_| PullError::NoName(t.host.clone()))?;
    let addrs: Vec<std::net::SocketAddr> = found.collect();
    check_resolved(
        &addrs.iter().map(|a| a.ip()).collect::<Vec<_>>(),
        allow_private,
    )?;
    let stream_id = t.query_value("streamid");
    let mut socket = None;
    for a in &addrs {
        let call = SrtSocket::builder().call(*a, stream_id.as_deref());
        if let Ok(Ok(s)) = tokio::time::timeout(CONNECT, call).await {
            socket = Some(s);
            break;
        }
    }
    let Some(mut socket) = socket else {
        return Err(PullError::Connect(address_of(t)));
    };
    let (tx, rx) = mpsc::channel::<Bytes>(256);
    let task = tokio::spawn(async move {
        while let Ok(Some(Ok((_, data)))) = tokio::time::timeout(SILENT_END, socket.next()).await {
            if tx.send(data).await.is_err() {
                break;
            }
        }
        let _ = socket.close().await;
    });
    Ok(Opened {
        format: "mpegts",
        input: rx,
        address: address_of(t),
        task: Some(task),
    })
}

// ---- trying again ----

/// The wait before the next try: 1 s, doubling to 30 s, and back to 1 s after a pull that held
/// for [`HELD`].
#[derive(Debug)]
pub struct Backoff {
    next: Duration,
}

/// A pull that held this long counts as good: the next failure waits 1 s again.
pub const HELD: Duration = Duration::from_secs(30);
const FIRST: Duration = Duration::from_secs(1);
const MOST: Duration = Duration::from_secs(30);

impl Default for Backoff {
    fn default() -> Self {
        Backoff { next: FIRST }
    }
}

impl Backoff {
    /// The wait after a try that lasted `held`.
    pub fn after(&mut self, held: Duration) -> Duration {
        if held >= HELD {
            self.next = FIRST;
        }
        let wait = self.next;
        self.next = (self.next * 2).min(MOST);
        wait
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    /// A server on loopback that answers `answers.len()` connections, one answer each, then ends.
    /// Each answer is written whole and the connection closed. It returns each request's line.
    pub(crate) fn serve(answers: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in answers {
                let (mut s, _) = listener.accept().unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap_or(0) == 1 {
                    head.push(b[0]);
                }
                asked.push(
                    String::from_utf8_lossy(&head)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                );
                let _ = s.write_all(&answer);
            }
            asked
        });
        (port, server)
    }

    pub(crate) fn ok(content_type: &str, body: &[u8]) -> Vec<u8> {
        let mut a = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        a.extend_from_slice(body);
        a
    }

    #[tokio::test]
    async fn an_http_stream_comes_with_its_format_and_its_bytes() {
        let (port, server) = serve(vec![ok("audio/mpeg", b"\xff\xfbabc")]);
        let mut o = open(&format!("http://127.0.0.1:{port}/Live.MP3"), true)
            .await
            .unwrap();
        assert_eq!(o.format, "mp3");
        assert_eq!(o.address, format!("127.0.0.1:{port}"));
        let mut got = Vec::new();
        while let Some(b) = o.input.recv().await {
            got.extend_from_slice(&b);
        }
        assert_eq!(got, b"\xff\xfbabc");
        // The path keeps its case on the wire.
        assert_eq!(server.join().unwrap(), vec!["GET /Live.MP3 HTTP/1.1"]);
    }

    #[tokio::test]
    async fn an_icecast_mount_s_answer_has_no_length_and_ends_when_it_closes() {
        let (port, server) = serve(vec![
            b"HTTP/1.0 200 OK\r\nServer: Icecast 2.5.0\r\nContent-Type: audio/mpeg\r\nCache-Control: no-cache\r\nicy-name: Kettle FM\r\nicy-br: 128\r\n\r\n\xff\xfb\x90\x00stream"
                .to_vec(),
        ]);
        let mut o = open(&format!("http://127.0.0.1:{port}/live"), true)
            .await
            .unwrap();
        assert_eq!(o.format, "mp3");
        let mut got = Vec::new();
        while let Some(b) = o.input.recv().await {
            got.extend_from_slice(&b);
        }
        assert_eq!(got, b"\xff\xfb\x90\x00stream");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_redirect_is_followed_and_a_refusal_is_said() {
        let (port, server) = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: /real\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
            ok("application/ogg", b"OggS"),
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            ok("audio/x-ms-wma", b"x"),
        ]);
        let o = open(&format!("http://127.0.0.1:{port}/old"), true)
            .await
            .unwrap();
        assert_eq!(o.format, "ogg");
        drop(o);
        let e = open(&format!("http://127.0.0.1:{port}/gone"), true).await;
        assert!(matches!(e, Err(PullError::Status(404))), "{:?}", e.err());
        let e = open(&format!("http://127.0.0.1:{port}/wma"), true).await;
        assert!(matches!(e, Err(PullError::ContentType(_))), "{:?}", e.err());
        let asked = server.join().unwrap();
        assert_eq!(asked[0], "GET /old HTTP/1.1");
        assert_eq!(asked[1], "GET /real HTTP/1.1");
    }

    #[tokio::test]
    async fn the_station_s_own_network_is_refused_before_any_connection() {
        let e = open("http://127.0.0.1:9/live", false).await;
        assert!(
            matches!(e, Err(PullError::Address(PullUrlError::Private))),
            "{:?}",
            e.err()
        );
        let e = open("srt://10.1.2.3:9000", false).await;
        assert!(matches!(e, Err(PullError::Address(PullUrlError::Private))));
        // A name the parse does not know as private, resolving to loopback, is refused by the
        // resolver's guard (where the system's hosts file has one: Fedora's and Debian's do).
        if std::net::ToSocketAddrs::to_socket_addrs(&("localhost4", 80u16)).is_ok() {
            let f = Fetcher::new(false);
            let e = tokio::task::spawn_blocking(move || f.open("http://localhost4:9/live").err())
                .await
                .unwrap();
            assert!(
                matches!(e, Some(PullError::Address(PullUrlError::Private))),
                "{e:?}"
            );
        }
    }

    #[test]
    fn https_reaches_a_server_whose_certificate_it_trusts() {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let cert_der = made.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server_config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert_der.clone()], key)
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            // The first connection does not trust the certificate and gives up in its handshake;
            // the second does.
            for trusted in [false, true] {
                let (tcp, _) = listener.accept().unwrap();
                tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let conn = rustls::ServerConnection::new(server_config.clone()).unwrap();
                let mut tls = rustls::StreamOwned::new(conn, tcp);
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && tls.read(&mut b).unwrap_or(0) == 1 {
                    head.push(b[0]);
                }
                if trusted {
                    tls.write_all(&ok("audio/aac", b"\xff\xf1secure")).unwrap();
                    tls.conn.send_close_notify();
                    let _ = tls.flush();
                }
            }
        });
        let url = format!("https://localhost:{port}/live.aac");
        assert!(
            Fetcher::new(true).fetch(&url, 1024).is_err(),
            "Mozilla's roots do not know the test's certificate"
        );
        let roots = RootCerts::Specific(Arc::new(vec![
            ureq::tls::Certificate::from_der(cert_der.as_ref()).to_owned(),
        ]));
        let (got, body) = Fetcher::with_roots(true, roots).fetch(&url, 1024).unwrap();
        assert_eq!(got, url);
        assert_eq!(body, b"\xff\xf1secure");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn an_srt_listener_is_called_and_its_stream_comes_through() {
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let listener = tokio::spawn(async move {
            let mut s = SrtSocket::builder().listen_on(addr).await.unwrap();
            for i in 0..5u8 {
                let mut packet = vec![0x47u8; 188];
                packet[1] = i;
                s.send((std::time::Instant::now(), Bytes::from(packet)))
                    .await
                    .unwrap();
            }
            s.close().await.unwrap();
        });
        let mut o = open(&format!("srt://127.0.0.1:{port}?streamid=kettle"), true)
            .await
            .unwrap();
        assert_eq!(o.format, "mpegts");
        let mut got = Vec::new();
        while let Some(b) = o.input.recv().await {
            got.push(b[1]);
        }
        assert_eq!(got, vec![0, 1, 2, 3, 4]);
        listener.await.unwrap();
    }

    #[test]
    fn the_wait_doubles_to_half_a_minute_and_starts_again_after_a_good_pull() {
        let mut b = Backoff::default();
        let s = Duration::from_secs;
        let waits: Vec<_> = (0..7).map(|_| b.after(s(0))).collect();
        assert_eq!(waits, vec![s(1), s(2), s(4), s(8), s(16), s(30), s(30)]);
        assert_eq!(b.after(s(45)), s(1));
        assert_eq!(b.after(s(0)), s(2));
    }
}
