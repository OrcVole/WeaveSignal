// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The bridge's station end (Phase 17g; [`weavesignal_proto::bridge_wire`]): a WebSocket on
//! loopback (`ports.source_bridge`), behind the station's own proxy on its public address, that
//! takes a host's stream from the studio app as a third way in, `bridge`.
//!
//! Before the upgrade it reads only the key from `Authorization` (the Icecast push's password,
//! [`crate::icecast::credentials_value`]); a request without one is refused there, 401. The
//! client's address is the proxy's `X-Forwarded-For` (the last entry) when the peer is loopback,
//! so the wrong-key limit counts the host, not the proxy. The first message names the format,
//! within 10 s; messages are at most 256 KiB, frames 64 KiB. After that the supervisor treats
//! it as it treats an Icecast source: the control plane's admission, one connection per key,
//! the decoding worker, the desk.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use weavesignal_proto::bridge_wire::{self as wire, Hello, State, Title};

use crate::icecast::{self, Credentials};

/// How long the upgrade and the first message may take.
pub const OPENING: Duration = Duration::from_secs(10);

/// A bridge that has upgraded, named its key and said what it sends.
pub struct Accepted {
    pub ws: WebSocketStream<TcpStream>,
    pub credentials: Credentials,
    /// The host's address: the proxy's last `X-Forwarded-For` entry, or the peer.
    pub address: IpAddr,
    /// The worker's format.
    pub format: &'static str,
    /// The software the app named.
    pub software: String,
}

#[allow(clippy::result_large_err)]
fn refuse(status: StatusCode, why: &str) -> ErrorResponse {
    let mut r = ErrorResponse::new(Some(why.to_owned()));
    *r.status_mut() = status;
    r
}

/// The limits on what either side sends.
pub fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(wire::MAX_MESSAGE))
        .max_frame_size(Some(wire::MAX_FRAME))
}

/// Upgrade `stream` from `peer`, read the key and the first message.
// The callback's error type is tungstenite's own HTTP response.
#[allow(clippy::result_large_err)]
pub async fn accept(stream: TcpStream, peer: SocketAddr) -> Result<Accepted> {
    let mut credentials = None;
    let mut forwarded = None;
    let callback = |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
        if req.uri().path() != wire::PATH {
            return Err(refuse(StatusCode::NOT_FOUND, "not the bridge's path"));
        }
        let auth = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let Some(c) = auth.and_then(icecast::credentials_value) else {
            return Err(refuse(StatusCode::UNAUTHORIZED, "The key is wrong."));
        };
        credentials = Some(c);
        forwarded = weavesignal_proto::last_forwarded(
            req.headers()
                .get_all("x-forwarded-for")
                .iter()
                .map(|v| v.to_str().ok()),
        );
        Ok(res)
    };
    let mut ws = tokio::time::timeout(
        OPENING,
        tokio_tungstenite::accept_hdr_async_with_config(stream, callback, Some(config())),
    )
    .await
    .context("no upgrade within 10 s")?
    .context("the upgrade failed")?;
    let credentials = credentials.context("no key")?;
    // Only the station's own proxy, on this machine, may say where a connection came from.
    let address = match forwarded {
        Some(a) if peer.ip().is_loopback() => a,
        _ => peer.ip(),
    };
    let first = tokio::time::timeout(OPENING, ws.next())
        .await
        .context("no first message within 10 s")?;
    let hello = match first {
        Some(Ok(Message::Text(t))) => serde_json::from_str::<Hello>(t.as_str()).ok(),
        _ => None,
    };
    let Some(hello) = hello else {
        let _ = send_state(
            &mut ws,
            "refused",
            "The first message must name the format.",
        )
        .await;
        bail!("no format named");
    };
    let Some(format) = wire::format(&hello.format) else {
        let why = format!(
            "The station cannot read `{}`. Set your software to MP3, AAC, Ogg, FLAC or MPEG-TS.",
            icecast::clean_title(&hello.format)
        );
        let _ = send_state(&mut ws, "refused", &why).await;
        bail!("a format the bridge does not carry");
    };
    Ok(Accepted {
        ws,
        credentials,
        address,
        format,
        software: icecast::clean_title(&hello.name),
    })
}

/// Tell the app where its source stands.
pub async fn send_state<S>(ws: &mut S, state: &str, message: &str) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let text = serde_json::to_string(&State {
        state: state.to_owned(),
        message: message.to_owned(),
    })?;
    ws.send(Message::Text(Utf8Bytes::from(text))).await?;
    Ok(())
}

/// What the bridge's connection carries once admitted: the media for the worker, titles, and
/// the states to send back.
pub struct Pump {
    pub media: mpsc::Receiver<Bytes>,
    pub titles: mpsc::Receiver<String>,
    pub states: mpsc::UnboundedSender<State>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Pump {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Pump {
    /// End the connection after the states already queued are sent (at most a second): a
    /// refusal's words reach the app before the socket closes.
    pub async fn finish(mut self) {
        // The session's own senders are gone by now; this one is the last.
        let (closed, _) = mpsc::unbounded_channel();
        drop(std::mem::replace(&mut self.states, closed));
        let task = std::mem::replace(&mut self.task, tokio::spawn(async {}));
        let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
    }
}

/// Split an admitted bridge into its parts. The media ends when the app closes, sends nothing
/// for `silent`, or the states' sender is dropped (the station ended the source).
pub fn pump(ws: WebSocketStream<TcpStream>, silent: Duration) -> Pump {
    let (media_tx, media) = mpsc::channel::<Bytes>(64);
    let (title_tx, titles) = mpsc::channel::<String>(8);
    let (states, mut state_rx) = mpsc::unbounded_channel::<State>();
    let task = tokio::spawn(async move {
        let (mut sink, mut stream) = ws.split();
        loop {
            tokio::select! {
                s = state_rx.recv() => {
                    let Some(s) = s else { break };
                    let text = serde_json::to_string(&s).unwrap_or_default();
                    if sink.send(Message::Text(Utf8Bytes::from(text))).await.is_err() {
                        break;
                    }
                }
                m = tokio::time::timeout(silent, stream.next()) => {
                    match m {
                        Ok(Some(Ok(Message::Binary(b)))) => {
                            if media_tx.send(b).await.is_err() {
                                break;
                            }
                        }
                        Ok(Some(Ok(Message::Text(t)))) => {
                            if let Ok(t) = serde_json::from_str::<Title>(t.as_str()) {
                                let _ = title_tx.try_send(icecast::clean_title(&t.title));
                            }
                        }
                        Ok(Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)))) => {}
                        _ => break,
                    }
                }
            }
        }
        let _ = sink.close().await;
    });
    Pump {
        media,
        titles,
        states,
        task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    fn auth(password: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("source:{password}"))
        )
    }

    async fn server() -> (TcpListener, SocketAddr) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap();
        (l, a)
    }

    async fn client(
        addr: SocketAddr,
        password: Option<&str>,
        forwarded: Option<&str>,
    ) -> Result<WebSocketStream<TcpStream>> {
        let mut req = format!("ws://{addr}{}", wire::PATH).into_client_request()?;
        if let Some(p) = password {
            req.headers_mut()
                .insert("authorization", auth(p).parse().unwrap());
        }
        if let Some(f) = forwarded {
            req.headers_mut()
                .insert("x-forwarded-for", f.parse().unwrap());
        }
        let stream = TcpStream::connect(addr).await?;
        Ok(tokio_tungstenite::client_async(req, stream).await?.0)
    }

    #[tokio::test]
    async fn a_bridge_names_its_key_its_host_and_its_format() {
        let (l, addr) = server().await;
        let srv = tokio::spawn(async move {
            let (s, peer) = l.accept().await.unwrap();
            accept(s, peer).await
        });
        let mut c = client(
            addr,
            Some("abcd2345-thesecret"),
            Some("6.6.6.6, 198.51.100.4"),
        )
        .await
        .unwrap();
        c.send(Message::text(r#"{"format":"ogg","name":"BUTT 1.45"}"#))
            .await
            .unwrap();
        let a = srv.await.unwrap().unwrap();
        assert_eq!(a.credentials.key_id, "abcd2345");
        assert_eq!(a.credentials.secret, "thesecret");
        assert_eq!(a.format, "ogg");
        assert_eq!(a.software, "BUTT 1.45");
        // From loopback (the proxy), the last forwarded entry is the host.
        assert_eq!(a.address, "198.51.100.4".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn no_key_is_refused_before_the_upgrade() {
        let (l, addr) = server().await;
        let srv = tokio::spawn(async move {
            let (s, peer) = l.accept().await.unwrap();
            accept(s, peer).await
        });
        let mut raw = TcpStream::connect(addr).await.unwrap();
        raw.write_all(
            format!(
                "GET {} HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nAuthorization: {}\r\n\r\n",
                wire::PATH,
                auth("hackme")
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut got = String::new();
        raw.read_to_string(&mut got).await.unwrap();
        assert!(got.starts_with("HTTP/1.1 401"), "{got}");
        assert!(srv.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_wrong_first_message_or_format_is_answered_and_closed() {
        for (first, why) in [
            (Message::binary(vec![0u8; 10]), "must name the format"),
            (Message::text(r#"{"format":"wma"}"#), "cannot read `wma`"),
        ] {
            let (l, addr) = server().await;
            let srv = tokio::spawn(async move {
                let (s, peer) = l.accept().await.unwrap();
                accept(s, peer).await
            });
            let mut c = client(addr, Some("abcd2345-x"), None).await.unwrap();
            c.send(first).await.unwrap();
            let Some(Ok(Message::Text(t))) = c.next().await else {
                panic!("no answer")
            };
            let s: State = serde_json::from_str(t.as_str()).unwrap();
            assert_eq!(s.state, "refused");
            assert!(s.message.contains(why), "{}", s.message);
            assert!(srv.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn media_titles_and_states_flow_and_a_huge_message_ends_it() {
        let (l, addr) = server().await;
        let srv = tokio::spawn(async move {
            let (s, peer) = l.accept().await.unwrap();
            accept(s, peer).await.unwrap()
        });
        let mut c = client(addr, Some("abcd2345-x"), None).await.unwrap();
        c.send(Message::text(r#"{"format":"mp3"}"#)).await.unwrap();
        let a = srv.await.unwrap();
        // Not from the proxy: a forwarded header would be ignored, and the peer is the host.
        assert!(a.address.is_loopback());
        let mut p = pump(a.ws, Duration::from_secs(5));
        c.send(Message::binary(vec![1u8, 2, 3])).await.unwrap();
        c.send(Message::text(r#"{"title":"Blue Peter\u0007"}"#))
            .await
            .unwrap();
        assert_eq!(&p.media.recv().await.unwrap()[..], &[1, 2, 3]);
        assert_eq!(p.titles.recv().await.unwrap(), "Blue Peter");
        p.states
            .send(State {
                state: "standing_by".into(),
                message: String::new(),
            })
            .unwrap();
        let Some(Ok(Message::Text(t))) = c.next().await else {
            panic!()
        };
        assert!(t.as_str().contains("standing_by"));
        // Over 256 KiB in one message: the station's reader stops, the media ends.
        let _ = c
            .send(Message::binary(vec![0u8; wire::MAX_MESSAGE + 1]))
            .await;
        let end = tokio::time::timeout(Duration::from_secs(5), p.media.recv()).await;
        assert!(matches!(end, Ok(None)));
    }
}
