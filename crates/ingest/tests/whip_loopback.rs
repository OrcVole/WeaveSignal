// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WHIP (17d) through the whole ingest on loopback, with no container: the supervisor
//! (`run`) against a stand-in control plane and a stand-in desk, the real decoding worker
//! (this crate's binary), and a str0m client sending a tone's Opus as OBS would. It checks:
//!
//! 1. an offer with the key's secret is answered `201` with an SDP answer and a session's
//!    address; the source stands by, its sound reaches the desk with `X-Source-Way: whip`, and
//!    the report says `whip`, `standby` and the worker's `rtp/opus 48 kHz stereo`;
//! 2. a wrong bearer is refused (`401`), and so is a request without the ingest's own bearer;
//! 3. a second connection with the key is refused (`409`) while the first sends;
//! 4. a `DELETE` with the wrong secret is refused, and with the right one ends the source.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use str0m::change::SdpAnswer;
use str0m::format::Codec;
use str0m::media::{Direction, Frequency, MediaKind, MediaTime};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, RtcConfig};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use weavesignal_ingest::supervisor::{Settings, run};
use weavesignal_proto::source_wire as wire;

const TOKEN: &str = "whip-loopback-ingest-token-test-only-0";
const DESK_PASSWORD: &str = "whip-loopback-desk-test-only";
const KEY: &str = "abcd2345";
const SECRET: &str = "CJ95WZA67WVREB8C8KZ5NV2E";

/// What the stand-ins saw.
#[derive(Default)]
struct Seen {
    events: Vec<Value>,
    reports: Vec<Value>,
    desk_heads: Vec<String>,
    desk_bytes: usize,
}

type Shared = Arc<Mutex<Seen>>;

/// A free port on loopback for `proto` (taken and let go; the test's own ports are not shared).
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

/// Read one HTTP request: its head and body.
async fn read_request(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut piece = [0u8; 4096];
    let end = loop {
        let n = s.read(&mut piece).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&piece[..n]);
        if let Some(e) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break e;
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut body = buf[end + 4..].to_vec();
    let len = head
        .lines()
        .find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while body.len() < len {
        let n = s.read(&mut piece).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&piece[..n]);
    }
    Some((head, body))
}

/// The control plane's internal routes, as the ingest asks them.
async fn control(listener: TcpListener, seen: Shared, port_whip: u16) {
    loop {
        let Ok((mut s, _)) = listener.accept().await else {
            return;
        };
        let seen = seen.clone();
        tokio::spawn(async move {
            let Some((head, body)) = read_request(&mut s).await else {
                return;
            };
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            assert!(head.contains(&format!("Bearer {TOKEN}")), "{head}");
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let reply: Option<Value> = match path.as_str() {
                "/api/v1/internal/sources/config" => Some(json!({
                    "listen": true, "port_icecast": 0, "port_srt": 0,
                    "port_whip": port_whip, "test_max_ms": 60_000
                })),
                "/api/v1/internal/sources/admit" => {
                    assert_eq!(body["way"], "whip", "{body}");
                    Some(if body["secret"] == SECRET && body["key_id"] == KEY {
                        json!({"decision": "standby", "identity": wire::identity(KEY),
                               "name": "Morag Campbell", "show": "steeped", "slot": 5})
                    } else {
                        json!({"decision": "refused", "reason": "wrong_key",
                               "message": "The key is wrong."})
                    })
                }
                "/api/v1/internal/sources/events" => {
                    seen.lock().unwrap().events.push(body);
                    None
                }
                "/api/v1/internal/sources/report" => {
                    seen.lock().unwrap().reports.push(body);
                    Some(json!({"close": [], "to_test": []}))
                }
                other => panic!("the ingest asked for {other}"),
            };
            let answer = match reply {
                Some(v) => {
                    let b = v.to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                        b.len()
                    )
                }
                None => "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_owned(),
            };
            let _ = s.write_all(answer.as_bytes()).await;
        });
    }
}

/// The desk's sources listener: takes each source's head and counts its sound.
async fn desk(listener: TcpListener, seen: Shared) {
    loop {
        let Ok((mut s, _)) = listener.accept().await else {
            return;
        };
        let seen = seen.clone();
        tokio::spawn(async move {
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if s.read(&mut b).await.unwrap_or(0) == 0 {
                    return;
                }
                head.push(b[0]);
            }
            seen.lock()
                .unwrap()
                .desk_heads
                .push(String::from_utf8_lossy(&head).into_owned());
            let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\n").await;
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match s.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => seen.lock().unwrap().desk_bytes += n,
                }
            }
        });
    }
}

/// A WHIP request to the ingest's loopback listener, as the control plane passes it on.
async fn whip(
    port: u16,
    method: &str,
    path: &str,
    ingest_token: &str,
    bearer: &str,
    body: &str,
) -> (u16, Vec<(String, String)>, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: ingest\r\nx-weavesignal-ingest: {ingest_token}\r\nx-weavesignal-address: 198.51.100.7\r\nAuthorization: Bearer {bearer}\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect();
    (status, headers, body.to_owned())
}

/// The first track's Opus packets from an Ogg fixture: a second of a 1 kHz tone, stereo.
fn tone_packets() -> Vec<Vec<u8>> {
    let path = format!(
        "{}/tests/fixtures/chained-opus.ogg",
        env!("CARGO_MANIFEST_DIR")
    );
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(std::fs::read(path).unwrap())),
        MediaSourceStreamOptions::default(),
    );
    let mut reader =
        symphonia::default::formats::OggReader::try_new(mss, FormatOptions::default()).unwrap();
    let track = reader.default_track(TrackType::Audio).unwrap().id;
    let mut out = Vec::new();
    loop {
        match reader.next_packet() {
            Ok(Some(p)) if p.track_id == track => out.push(p.data.to_vec()),
            Ok(Some(_)) => {}
            Err(SymErr::ResetRequired) | Ok(None) | Err(_) => return out,
        }
    }
}

/// A sending client: offers sound, takes the answer, and sends the tone (looped) every 20 ms
/// while `sending` is true, answering the station until `stop`.
struct Client {
    offer: String,
    task: tokio::task::JoinHandle<usize>,
    answer_tx: tokio::sync::oneshot::Sender<String>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

fn client() -> Client {
    let std_socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    std_socket.set_nonblocking(true).unwrap();
    let local = std_socket.local_addr().unwrap();
    let mut rtc = RtcConfig::new()
        .set_crypto_provider(Arc::new(str0m::crypto::from_feature_flags()))
        .build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = rtc.sdp_api();
    let mid = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
    // OBS sends its picture too; the station receives it and discards it (whip-video-discard a).
    let vmid = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
    let (offer, pending) = api.apply().unwrap();
    let (answer_tx, answer_rx) = tokio::sync::oneshot::channel::<String>();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopping = stop.clone();
    let task = tokio::spawn(async move {
        let socket = UdpSocket::from_std(std_socket).unwrap();
        let Ok(answer) = answer_rx.await else {
            return 0;
        };
        rtc.sdp_api()
            .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
            .unwrap();
        let packets = tone_packets();
        let mut buf = vec![0u8; 2048];
        let (mut connected, mut sent) = (false, 0usize);
        let mut next_send = Instant::now();
        let mut ts: u32 = 0x7fff_f000;
        while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
            let now = Instant::now();
            if rtc.handle_input(Input::Timeout(now)).is_err() {
                break;
            }
            if connected && now >= next_send {
                let w = rtc.writer(mid).unwrap();
                let pt = w
                    .payload_params()
                    .find(|p| p.spec().codec == Codec::Opus)
                    .unwrap()
                    .pt();
                let p = packets[sent % packets.len()].clone();
                w.write(
                    pt,
                    now,
                    MediaTime::new(u64::from(ts), Frequency::FORTY_EIGHT_KHZ),
                    p,
                )
                .unwrap();
                // A small H.264 picture (one IDR slice in Annex B) beside each sound packet: the
                // station takes it and passes none of it to the desk.
                let vpt = rtc
                    .writer(vmid)
                    .unwrap()
                    .payload_params()
                    .find(|p| p.spec().codec == Codec::H264)
                    .map(|p| p.pt());
                if let Some(vpt) = vpt {
                    let _ = rtc.writer(vmid).unwrap().write(
                        vpt,
                        now,
                        MediaTime::new(u64::from(ts) * 15 / 8, Frequency::NINETY_KHZ),
                        vec![0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00, 0x33, 0xff],
                    );
                }
                ts = ts.wrapping_add(960);
                sent += 1;
                next_send += Duration::from_millis(20);
            }
            let mut wake = now + Duration::from_millis(5);
            loop {
                match rtc.poll_output() {
                    Ok(Output::Timeout(t)) => {
                        wake = wake.min(t);
                        break;
                    }
                    Ok(Output::Transmit(t)) => {
                        let _ = socket.send_to(&t.contents, t.destination).await;
                    }
                    Ok(Output::Event(Event::Connected)) => {
                        connected = true;
                        next_send = Instant::now();
                    }
                    Ok(Output::Event(_)) => {}
                    Err(_) => return sent,
                }
            }
            let wait = wake.max(Instant::now()) - Instant::now();
            if let Ok(Ok((n, source))) =
                tokio::time::timeout(wait, socket.recv_from(&mut buf)).await
            {
                let Ok(contents) = buf[..n].try_into() else {
                    continue;
                };
                let input = Input::Receive(
                    Instant::now(),
                    Receive {
                        proto: Protocol::Udp,
                        source,
                        destination: local,
                        contents,
                    },
                );
                if rtc.handle_input(input).is_err() {
                    break;
                }
            }
        }
        sent
    });
    Client {
        offer: offer.to_sdp_string(),
        task,
        answer_tx,
        stop,
    }
}

/// Wait until `f` holds, or fail after `secs`.
async fn until(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn whip_from_offer_to_the_desk_and_its_refusals() {
    let seen: Shared = Arc::default();
    let control_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let desk_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (control_at, desk_at) = (
        control_l.local_addr().unwrap(),
        desk_l.local_addr().unwrap(),
    );
    let port_whip = free_udp();
    let whip_http = free_tcp();
    tokio::spawn(control(control_l, seen.clone(), port_whip));
    tokio::spawn(desk(desk_l, seen.clone()));
    let ingest = tokio::spawn(run(Settings {
        control: format!("http://{control_at}"),
        token: TOKEN.into(),
        desk: desk_at,
        desk_password: DESK_PASSWORD.into(),
        bind: Ipv4Addr::LOCALHOST.into(),
        exe: env!("CARGO_BIN_EXE_weavesignal-ingest").into(),
        whip_http: SocketAddr::from((Ipv4Addr::LOCALHOST, whip_http)),
        public_ip: Some(Ipv4Addr::LOCALHOST),
    }));
    // The ingest reads its settings and opens WHIP's listener.
    let end = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect(("127.0.0.1", whip_http)).await.is_err() {
        assert!(Instant::now() < end, "WHIP's listener never opened");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let path = format!("/whip/{KEY}");

    // 2. Not the control plane: refused before anything else.
    let c = client();
    let (status, _, _) = whip(whip_http, "POST", &path, "not-the-token", SECRET, &c.offer).await;
    assert_eq!(status, 401);
    // A wrong key: refused with the control plane's sentence.
    let (status, _, body) = whip(whip_http, "POST", &path, TOKEN, "WRONG", &c.offer).await;
    assert_eq!((status, body.trim()), (401, "The key is wrong."));

    // 1. The right key: answered, standing by, heard at the desk.
    let (status, headers, answer) = whip(whip_http, "POST", &path, TOKEN, SECRET, &c.offer).await;
    assert_eq!(status, 201, "{answer}");
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert_eq!(get("content-type"), "application/sdp");
    let location = get("location");
    assert!(location.starts_with(&format!("/whip/{KEY}/")), "{location}");
    assert!(
        answer.contains("a=ice-lite")
            && answer.contains(&format!("127.0.0.1 {port_whip} typ host")),
        "{answer}"
    );
    assert!(
        answer
            .lines()
            .any(|l| l.starts_with("m=video ") && !l.starts_with("m=video 0 ")),
        "the picture is received (and discarded), not refused: {answer}"
    );
    c.answer_tx.send(answer).unwrap();
    // Half a second of 48 kHz stereo float at the desk.
    until(20, "the tone at the desk", || {
        seen.lock().unwrap().desk_bytes > 192_000
    })
    .await;
    {
        let s = seen.lock().unwrap();
        let head = s.desk_heads.first().unwrap();
        assert!(
            head.starts_with(&format!("PUT {}{}", wire::PATH_PREFIX, wire::identity(KEY))),
            "{head}"
        );
        assert!(head.contains(&format!("{}: whip", wire::WAY)), "{head}");
        assert!(
            head.contains(&format!("{}: rtp/opus 48 kHz stereo", wire::FORMAT)),
            "{head}"
        );
        assert!(
            s.events
                .iter()
                .any(|e| e["event"] == "admitted" && e["way"] == "whip"),
            "{:?}",
            s.events
        );
    }
    until(5, "a report of the WHIP source", || {
        seen.lock().unwrap().reports.iter().any(|r| {
            r["connections"].as_array().is_some_and(|c| {
                c.iter().any(|c| {
                    c["key_id"] == KEY
                        && c["way"] == "whip"
                        && c["mode"] == "standby"
                        && c["format"] == "rtp/opus 48 kHz stereo"
                        && c["kbps"].as_u64().is_some_and(|k| k > 0)
                })
            })
        })
    })
    .await;

    // 3. A second connection with the key while the first sends: refused.
    let second = client();
    let (status, _, body) = whip(whip_http, "POST", &path, TOKEN, SECRET, &second.offer).await;
    assert_eq!(status, 409, "{body}");
    drop(second.answer_tx);
    assert!(
        body.contains("Another connection is using this key"),
        "{body}"
    );

    // 4. DELETE: the wrong secret is refused; the right one ends the source.
    let (status, _, _) = whip(whip_http, "DELETE", &location, TOKEN, "WRONG", "").await;
    assert_eq!(status, 401);
    let (status, _, _) = whip(
        whip_http,
        "DELETE",
        "/whip/abcd2345/nosuchsession",
        TOKEN,
        SECRET,
        "",
    )
    .await;
    assert_eq!(status, 404);
    let (status, _, _) = whip(whip_http, "DELETE", &location, TOKEN, SECRET, "").await;
    assert_eq!(status, 200);
    until(10, "the source's end", || {
        seen.lock()
            .unwrap()
            .events
            .iter()
            .any(|e| e["event"] == "ended" && e["way"] == "whip")
    })
    .await;
    let ended: VecDeque<Value> = seen
        .lock()
        .unwrap()
        .events
        .iter()
        .filter(|e| e["event"] == "ended")
        .cloned()
        .collect();
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(
        ended[0]["detail"], "the host's software disconnected",
        "{ended:?}"
    );

    c.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let sent = c.task.await.unwrap();
    assert!(sent > 25, "the client sent {sent} packets");
    ingest.abort();
}
