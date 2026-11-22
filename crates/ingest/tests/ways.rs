// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The supervisor with its two new ways in (Phase 17g), against a fake control plane and a fake
//! desk on loopback: no stack, no containers. The bridge carries an MP3 file through the real
//! decoding worker to the desk; a wrong key is told why; VBAN from the bound sender reaches the
//! desk as `vban`, and a stranger's never does.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use weavesignal_ingest::supervisor::{Settings, run};
use weavesignal_proto::bridge_wire::{self as wire, State};
use weavesignal_proto::vban::{DataType, Packetiser, StreamName};

/// A free port on loopback, for a listener the supervisor opens itself.
fn free_tcp() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// What the fake control plane saw.
#[derive(Default)]
struct Seen {
    admits: Vec<Value>,
    events: Vec<Value>,
    reports: Vec<Value>,
}

async fn read_request(s: &mut TcpStream) -> Option<(String, String, Value)> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1024];
    let end = loop {
        let n = s.read(&mut b).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&b[..n]);
        if let Some(e) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break e;
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let len: usize = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = buf[end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut b).await.ok()?;
        body.extend_from_slice(&b[..n]);
    }
    let first = head.lines().next().unwrap_or_default().to_owned();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Some((method, path, json))
}

/// A control plane that admits key `abcd2345` with secret `thesecret` over the bridge, and the
/// bound VBAN stream `Studio`.
async fn fake_control(config: Value, seen: Arc<Mutex<Seen>>) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let config = config.clone();
            let seen = seen.clone();
            tokio::spawn(async move {
                while let Some((_, path, body)) = read_request(&mut s).await {
                    let reply = match path.rsplit('/').next().unwrap_or_default() {
                        "config" => config.clone(),
                        "admit" => {
                            seen.lock().unwrap().admits.push(body.clone());
                            let standby = json!({"decision": "standby", "identity": "source-abcd2345",
                                "name": "Finlay Grant", "show": "steeped"});
                            match (
                                body["way"].as_str(),
                                body["secret"].as_str(),
                                body["stream"].as_str(),
                            ) {
                                (Some("bridge"), Some("thesecret"), _) => standby,
                                (Some("vban"), None, Some("Studio")) => standby,
                                _ => json!({"decision": "refused", "reason": "wrong_key",
                                    "message": "The key is wrong."}),
                            }
                        }
                        "events" => {
                            seen.lock().unwrap().events.push(body);
                            json!({})
                        }
                        "report" => {
                            seen.lock().unwrap().reports.push(body);
                            json!({"close": []})
                        }
                        _ => json!({}),
                    };
                    let text = reply.to_string();
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
                        text.len()
                    );
                    if s.write_all(answer.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

/// A desk that takes sources and says, for each, its head and how many bytes of sound came.
async fn fake_desk(tx: mpsc::UnboundedSender<(String, usize)>) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(b[0]);
                }
                s.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await.unwrap();
                let head = String::from_utf8_lossy(&head).to_string();
                let mut wav = [0u8; 44];
                if s.read_exact(&mut wav).await.is_err() {
                    return;
                }
                let mut total = 0;
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            total += n;
                            let _ = tx.send((head.clone(), total));
                        }
                    }
                }
            });
        }
    });
    addr
}

struct World {
    seen: Arc<Mutex<Seen>>,
    desk: mpsc::UnboundedReceiver<(String, usize)>,
    bridge: u16,
    vban: u16,
}

async fn world() -> World {
    let (bridge, vban) = (free_tcp(), free_udp());
    let config = json!({"listen": true, "port_icecast": 0, "port_srt": 0, "test_max_ms": 60_000,
        "port_bridge": bridge,
        "vban": {"bind": "127.0.0.1", "port": vban,
                 "rules": [{"address": "127.0.0.2", "name": "Studio", "key_id": "abcd2345"}]}});
    let seen = Arc::new(Mutex::new(Seen::default()));
    let control = fake_control(config, seen.clone()).await;
    let (tx, desk) = mpsc::unbounded_channel();
    let desk_addr = fake_desk(tx).await;
    let settings = Settings {
        control: format!("http://{control}"),
        token: "t".repeat(40),
        desk: desk_addr,
        desk_password: "desk-password".into(),
        bind: "127.0.0.1".parse().unwrap(),
        exe: env!("CARGO_BIN_EXE_weavesignal-ingest").into(),
        // WHIP stays off (no public address, and no WHIP port named), so nothing listens here.
        whip_http: "127.0.0.1:9".parse().unwrap(),
        public_ip: None,
    };
    tokio::spawn(run(settings));
    // The supervisor reads its settings at once, then listens.
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", bridge)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    World {
        seen,
        desk,
        bridge,
        vban,
    }
}

async fn bridge_client(port: u16, password: &str) -> tokio_tungstenite::WebSocketStream<TcpStream> {
    let mut req = format!("ws://127.0.0.1:{port}{}", wire::PATH)
        .into_client_request()
        .unwrap();
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("source:{password}"));
    req.headers_mut()
        .insert("authorization", format!("Basic {auth}").parse().unwrap());
    // The station's proxy says who the host is.
    req.headers_mut()
        .insert("x-forwarded-for", "198.51.100.4".parse().unwrap());
    let s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio_tungstenite::client_async(req, s).await.unwrap().0
}

async fn next_state(ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>) -> State {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("an answer within 10 s")
            .expect("open")
            .expect("readable");
        if let Message::Text(t) = m {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// Wait for the desk to have had at least `bytes` from a source whose head contains `way`.
async fn desk_heard(w: &mut World, way: &str, bytes: usize) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let (head, total) = tokio::time::timeout_at(deadline, w.desk.recv())
            .await
            .expect("the desk heard it in time")
            .unwrap();
        if head.contains(&format!("X-Source-Way: {way}")) && total >= bytes {
            return head;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bridge_and_vban_reach_the_desk_and_strangers_do_not() {
    let mut w = world().await;

    // A wrong key over the bridge is told so, and nothing reaches the desk.
    let mut bad = bridge_client(w.bridge, "abcd2345-wrong").await;
    bad.send(Message::text(r#"{"format":"mp3","name":"BUTT"}"#))
        .await
        .unwrap();
    let s = next_state(&mut bad).await;
    assert_eq!(
        (s.state.as_str(), s.message.as_str()),
        ("refused", "The key is wrong.")
    );

    // The right key: standing by, an MP3 file's bytes through the real worker to the desk.
    let mut ws = bridge_client(w.bridge, "abcd2345-thesecret").await;
    ws.send(Message::text(r#"{"format":"mp3","name":"BUTT 1.45"}"#))
        .await
        .unwrap();
    let s = next_state(&mut ws).await;
    assert_eq!(s.state, "standing_by");
    let mp3 = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone-44k.mp3"
    ))
    .unwrap();
    for piece in mp3.chunks(4096) {
        ws.send(Message::binary(piece.to_vec())).await.unwrap();
    }
    ws.send(Message::text(r#"{"title":"Runrig - Loch Lomond"}"#))
        .await
        .unwrap();
    // A second of 48 kHz stereo float is 384,000 bytes; the fixture is longer than that.
    let head = desk_heard(&mut w, "bridge", 100_000).await;
    assert!(head.starts_with("PUT /sources/source-abcd2345 "), "{head}");
    assert!(head.contains("X-Source-Format: mp3"), "{head}");
    {
        let seen = w.seen.lock().unwrap();
        let bridge_admit = seen
            .admits
            .iter()
            .find(|a| a["secret"] == "thesecret")
            .unwrap();
        // The host's address, from the proxy's header: the wrong-key limit counts the host.
        assert_eq!(bridge_admit["address"], "198.51.100.4");
        assert_eq!(bridge_admit["way"], "bridge");
    }
    // The title reaches the report.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let titled = w.seen.lock().unwrap().reports.iter().any(|r| {
            r["connections"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["title"] == "Runrig - Loch Lomond"))
        });
        if titled {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "no title reported");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    ws.close(None).await.unwrap();
    drop(ws);

    // VBAN: a stranger on 127.0.0.3 sends the bound name; the bound sender on 127.0.0.2 too.
    let to: SocketAddr = ([127, 0, 0, 1], w.vban).into();
    let stranger = UdpSocket::bind("127.0.0.3:0").await.unwrap();
    let bound = UdpSocket::bind("127.0.0.2:0").await.unwrap();
    let mut pk = Packetiser::new(StreamName::new("Studio").unwrap(), DataType::I24);
    let mut pos = 0i64;
    // Two seconds of 20 ms blocks, paced roughly as a sender would. One of the bound sender's
    // packets, a second in, is lost on the way: the clock readout's count.
    for block in 0..100 {
        let mut packets = Vec::new();
        pk.push(pos, &vec![0.2f32; 2 * 960], &mut packets);
        pos += 960;
        for (i, p) in packets.iter().enumerate() {
            stranger.send_to(p, to).await.unwrap();
            if !(block == 50 && i == 1) {
                bound.send_to(p, to).await.unwrap();
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let head = desk_heard(&mut w, "vban", 50_000).await;
    assert!(
        head.contains("X-Source-Format: vban 24-bit 48 kHz stereo"),
        "{head}"
    );
    {
        let seen = w.seen.lock().unwrap();
        let vban_admits: Vec<&Value> = seen.admits.iter().filter(|a| a["way"] == "vban").collect();
        assert!(!vban_admits.is_empty());
        assert!(
            vban_admits
                .iter()
                .all(|a| a["address"] == "127.0.0.2" && a["stream"] == "Studio"),
            "{vban_admits:?}"
        );
        // The stranger's packets were counted, by their address, in a report.
        let counted: HashMap<String, u64> = seen
            .reports
            .iter()
            .rev()
            .find_map(|r| serde_json::from_value(r["vban_dropped"].clone()).ok())
            .unwrap_or_default();
        assert!(
            counted.get("address").copied().unwrap_or(0) > 0,
            "{counted:?}"
        );
    }
    // The lost packet reaches the report beside the VBAN connection (the clock readout's
    // `lost`); no other way reports one.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let lost = w.seen.lock().unwrap().reports.iter().rev().find_map(|r| {
            r["connections"].as_array().and_then(|c| {
                c.iter()
                    .find(|c| c["way"] == "vban")
                    .and_then(|c| c["lost"].as_u64())
            })
        });
        if lost == Some(1) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no lost packet reported: {lost:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(w.seen.lock().unwrap().reports.iter().all(|r| {
        r["connections"].as_array().is_none_or(|c| {
            c.iter()
                .all(|c| c["way"] == "vban" || c.get("lost").is_none())
        })
    }));
}

/// Opus inside MPEG-TS over the bridge (1.2.1): refused as soon as the stream's map is read, with
/// words that say what the station cannot read, sent to the app before the socket closes and
/// kept in the key's log. 1.2.0 stood it by in silence for over five minutes.
#[tokio::test(flavor = "multi_thread")]
async fn opus_inside_ts_is_refused_at_once_with_its_reason() {
    let w = world().await;
    let mut ws = bridge_client(w.bridge, "abcd2345-thesecret").await;
    ws.send(Message::text(r#"{"format":"mpegts","name":"OBS 33.0.2"}"#))
        .await
        .unwrap();
    assert_eq!(next_state(&mut ws).await.state, "standing_by");
    let ts = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone-48k-opus-video.ts"
    ))
    .unwrap();
    let started = std::time::Instant::now();
    for piece in ts.chunks(1316) {
        if ws.send(Message::binary(piece.to_vec())).await.is_err() {
            break;
        }
    }
    let s = next_state(&mut ws).await;
    assert_eq!(s.state, "refused", "{}", s.message);
    assert!(
        s.message
            .starts_with("The station cannot read what your software sends: Opus inside MPEG-TS"),
        "{}",
        s.message
    );
    assert!(s.message.contains("AAC"), "{}", s.message);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "refused at once, not after {:?}",
        started.elapsed()
    );
    // The key's log has the refusal with the same words.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let found = w.seen.lock().unwrap().events.iter().any(|e| {
            e["event"] == "refused"
                && e["reason"] == "format"
                && e["detail"]
                    .as_str()
                    .is_some_and(|d| d.starts_with("Opus inside MPEG-TS"))
        });
        if found {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no refusal logged: {:?}",
            w.seen.lock().unwrap().events
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
