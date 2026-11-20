// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! One UDP socket for every WHIP session (17d): each datagram goes to the session that
//! [`str0m::Rtc::accepts`] it (ICE's username for STUN, the nominated address for the rest), and
//! each session's Opus leaves on a channel of its own, with its RTP timestamp, for the source's
//! decoding worker. The station's port is the one in the answer's candidate, so a datagram's
//! destination is taken to be that candidate's address, whatever address the socket is bound to
//! (behind a NAT or a container's network, the two differ).
//!
//! A session ends, here and only here, when ICE says it disconnected, when the host's software
//! ends it (a WHIP `DELETE`), when its media has stopped for [`SILENT_END`] (one whose ICE never
//! connected included), when str0m fails it, or when the channel's receiver is dropped (the
//! source ended at the station). Its channel then closes, which ends the source. One session's
//! failure never ends the others.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use str0m::format::Codec;
use str0m::net::{Protocol, Receive};
use str0m::{Event, IceConnectionState, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// A session whose media has stopped this long, or that never started, is ended.
pub const SILENT_END: Duration = Duration::from_secs(10);
/// Payloads waiting for the source's worker: two seconds of 20 ms packets.
const QUEUE: usize = 100;
/// The longest time between looks at every session's timers.
const TICK: Duration = Duration::from_millis(100);

/// What a session's channel carries: an Opus payload with its RTP timestamp (48 kHz).
pub type Payload = (u32, Bytes);

enum Cmd {
    Add {
        session: String,
        rtc: Box<Rtc>,
        tx: mpsc::Sender<Payload>,
    },
    End(String),
}

/// A handle on the running mux: add a session, end one.
#[derive(Clone)]
pub struct MuxHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl MuxHandle {
    /// Hand `rtc` (answered by [`crate::whip_rtc::answer`]) to the mux under `session`; its Opus
    /// arrives on the receiver. `None` when the mux has stopped.
    pub fn add(&self, session: String, rtc: Rtc) -> Option<mpsc::Receiver<Payload>> {
        let (tx, rx) = mpsc::channel(QUEUE);
        self.tx
            .send(Cmd::Add {
                session,
                rtc: Box::new(rtc),
                tx,
            })
            .ok()?;
        Some(rx)
    }

    /// End `session` as its host's software asked (a WHIP `DELETE`).
    pub fn end(&self, session: &str) {
        let _ = self.tx.send(Cmd::End(session.to_owned()));
    }
}

/// The mux, until [`Mux::run`] is given its socket.
pub struct Mux {
    rx: mpsc::UnboundedReceiver<Cmd>,
    local: SocketAddr,
}

/// A mux whose sessions are reached at `local`, the address in their answers' candidate.
pub fn new(local: SocketAddr) -> (MuxHandle, Mux) {
    let (tx, rx) = mpsc::unbounded_channel();
    (MuxHandle { tx }, Mux { rx, local })
}

struct Session {
    id: String,
    rtc: Box<Rtc>,
    tx: mpsc::Sender<Payload>,
    last_media: Instant,
    dropped: u64,
    /// Why it ends, once it does.
    end: Option<&'static str>,
}

impl Session {
    fn input(&mut self, input: Input) {
        if let Err(e) = self.rtc.handle_input(input) {
            tracing::info!(session = %self.id, error = %e, "a WHIP session failed");
            self.end = Some("the WebRTC session failed");
        }
    }
}

impl Mux {
    /// Serve every session on `socket` until every handle is dropped.
    pub async fn run(mut self, socket: UdpSocket) {
        let mut sessions: Vec<Session> = Vec::new();
        let mut buf = vec![0u8; 2048];
        let mut open = true;
        loop {
            let now = Instant::now();
            let mut wake = now + TICK;
            for s in sessions.iter_mut() {
                s.input(Input::Timeout(now));
                while s.end.is_none() {
                    match s.rtc.poll_output() {
                        Ok(Output::Timeout(t)) => {
                            wake = wake.min(t);
                            break;
                        }
                        Ok(Output::Transmit(t)) => {
                            let _ = socket.send_to(&t.contents, t.destination).await;
                        }
                        Ok(Output::Event(e)) => s.event(e),
                        Err(e) => {
                            tracing::info!(session = %s.id, error = %e, "a WHIP session failed");
                            s.end = Some("the WebRTC session failed");
                        }
                    }
                }
                if s.end.is_none() {
                    if s.tx.is_closed() {
                        s.end = Some("ended at the station");
                    } else if now.duration_since(s.last_media) > SILENT_END {
                        s.end = Some("no sound for 10 s");
                    } else if !s.rtc.is_alive() {
                        s.end = Some("the WebRTC session closed");
                    }
                }
            }
            sessions.retain_mut(|s| match s.end {
                None => true,
                Some(why) => {
                    s.rtc.disconnect();
                    tracing::info!(session = %s.id, %why, dropped = s.dropped, "a WHIP session ended");
                    false
                }
            });
            if !open && sessions.is_empty() {
                return;
            }
            let wake = wake.max(Instant::now());
            tokio::select! {
                cmd = self.rx.recv(), if open => match cmd {
                    Some(Cmd::Add { session, rtc, tx }) => sessions.push(Session {
                        id: session,
                        rtc,
                        tx,
                        last_media: Instant::now(),
                        dropped: 0,
                        end: None,
                    }),
                    Some(Cmd::End(id)) => {
                        for s in sessions.iter_mut().filter(|s| s.id == id) {
                            s.end = Some("ended by the host's software");
                        }
                    }
                    None => open = false,
                },
                got = socket.recv_from(&mut buf) => {
                    let Ok((n, source)) = got else { continue };
                    let Ok(contents) = buf[..n].try_into() else { continue };
                    let input = Input::Receive(
                        Instant::now(),
                        Receive {
                            proto: Protocol::Udp,
                            source,
                            destination: self.local,
                            contents,
                        },
                    );
                    if let Some(s) = sessions.iter_mut().find(|s| s.end.is_none() && s.rtc.accepts(&input)) {
                        s.input(input);
                    }
                }
                _ = tokio::time::sleep_until(wake.into()) => {}
            }
        }
    }
}

impl Session {
    fn event(&mut self, e: Event) {
        match e {
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.end = Some("ICE disconnected");
            }
            Event::MediaData(d) if d.params.spec().codec == Codec::Opus => {
                self.last_media = Instant::now();
                // The RTP timestamp at the codec's 48 kHz clock; str0m extends it to 64 bits,
                // and its low 32 are RTP's own.
                let ts = d.time.numer() as u32;
                if self
                    .tx
                    .try_send((ts, Bytes::copy_from_slice(&d.data)))
                    .is_err()
                {
                    self.dropped += 1;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use str0m::change::SdpAnswer;
    use str0m::media::{Direction, Frequency, MediaKind, MediaTime};
    use str0m::{Candidate, RtcConfig};

    /// A sending client over loopback: offers sound, takes the answer, sends `packets` 20 ms
    /// apart with timestamps from `ts0`, then stops.
    async fn client(answerer: impl Fn(&str) -> String, ts0: u32, packets: Vec<Vec<u8>>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let mut rtc = RtcConfig::new()
            .set_crypto_provider(std::sync::Arc::new(str0m::crypto::from_feature_flags()))
            .build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        let mut api = rtc.sdp_api();
        let mid = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let (offer, pending) = api.apply().unwrap();
        let answer = answerer(&offer.to_sdp_string());
        rtc.sdp_api()
            .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
            .unwrap();
        let mut buf = vec![0u8; 2048];
        let mut connected = false;
        let mut sent = 0usize;
        let mut next_send = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(10);
        while sent < packets.len() && Instant::now() < deadline {
            let now = Instant::now();
            rtc.handle_input(Input::Timeout(now)).unwrap();
            if connected && now >= next_send {
                let w = rtc.writer(mid).unwrap();
                let pt = w
                    .payload_params()
                    .find(|p| p.spec().codec == Codec::Opus)
                    .unwrap()
                    .pt();
                w.write(
                    pt,
                    now,
                    MediaTime::new(
                        u64::from(ts0.wrapping_add(sent as u32 * 960)),
                        Frequency::FORTY_EIGHT_KHZ,
                    ),
                    packets[sent].clone(),
                )
                .unwrap();
                sent += 1;
                next_send = now + Duration::from_millis(20);
            }
            let mut wake = now + Duration::from_millis(5);
            loop {
                match rtc.poll_output().unwrap() {
                    Output::Timeout(t) => {
                        wake = wake.min(t);
                        break;
                    }
                    Output::Transmit(t) => {
                        socket.send_to(&t.contents, t.destination).await.unwrap();
                    }
                    Output::Event(Event::Connected) => connected = true,
                    Output::Event(_) => {}
                }
            }
            let wait = wake.max(Instant::now()) - Instant::now();
            if let Ok(Ok((n, source))) =
                tokio::time::timeout(wait, socket.recv_from(&mut buf)).await
            {
                let input = Input::Receive(
                    Instant::now(),
                    Receive {
                        proto: Protocol::Udp,
                        source,
                        destination: local,
                        contents: buf[..n].try_into().unwrap(),
                    },
                );
                rtc.handle_input(input).unwrap();
            }
        }
        assert_eq!(sent, packets.len(), "the client connected and sent");
        // Keep answering for a moment so the last packets are acknowledged.
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    /// A valid 20 ms Opus packet (CELT, silence) marked with `n` in its padding-free payload.
    fn packet(n: u8) -> Vec<u8> {
        // TOC 0xF8: CELT fullband 20 ms, mono, one frame; the rest is the frame's data.
        vec![0xF8, 0xFF, 0xFE, n]
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_clients_each_arrive_on_their_own_receiver() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let (handle, mux) = new(local);
        let running = tokio::spawn(mux.run(socket));

        let mut receivers = Vec::new();
        let mut clients = Vec::new();
        for (who, ts0) in [(1u8, 1_000u32), (2u8, u32::MAX - 10_000)] {
            let (tx, rx) = std::sync::mpsc::channel();
            let h = handle.clone();
            let answerer = move |offer: &str| {
                let (rtc, answer) = crate::whip_rtc::answer(offer, local, Instant::now()).unwrap();
                tx.send(h.add(format!("session{who}"), rtc).unwrap())
                    .unwrap();
                answer
            };
            let packets: Vec<Vec<u8>> = (0..25).map(|_| packet(who)).collect();
            clients.push(tokio::spawn(client(answerer, ts0, packets)));
            receivers.push((who, ts0, rx));
        }
        let mut got = Vec::new();
        for (who, ts0, rx) in receivers {
            let mut rx = tokio::task::spawn_blocking(move || rx.recv().unwrap())
                .await
                .unwrap();
            let mut payloads = Vec::new();
            while let Ok(Some((ts, p))) =
                tokio::time::timeout(Duration::from_secs(5), rx.recv()).await
            {
                payloads.push((ts, p));
                if payloads.len() >= 20 {
                    break;
                }
            }
            got.push((who, ts0, payloads));
        }
        for c in clients {
            c.await.unwrap();
        }
        for (who, ts0, payloads) in got {
            assert!(
                payloads.len() >= 20,
                "client {who}: {} payloads",
                payloads.len()
            );
            assert!(
                payloads.iter().all(|(_, p)| p.last() == Some(&who)),
                "client {who} heard only its own sound"
            );
            for (ts, _) in &payloads {
                let k = ts.wrapping_sub(ts0);
                assert!(k % 960 == 0 && k < 25 * 960, "client {who}: timestamp {ts}");
            }
        }
        drop(handle);
        let _ = tokio::time::timeout(Duration::from_secs(15), running).await;
    }

    /// A host behind NAT: its offer names only addresses the station cannot reach (here none
    /// at all, and a private one), and its packets come from an address the station never saw.
    /// ICE-lite learns it from the host's checks (a peer-reflexive candidate).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_host_whose_offer_names_no_reachable_address_is_heard() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let (handle, mux) = new(local);
        let running = tokio::spawn(mux.run(socket));
        for (n, rewrite) in [(3u8, None), (4u8, Some("10.0.0.9"))] {
            let (tx, rx) = std::sync::mpsc::channel();
            let h = handle.clone();
            let answerer = move |offer: &str| {
                let offer: String = offer
                    .lines()
                    .filter_map(|l| match (l.starts_with("a=candidate"), rewrite) {
                        (false, _) => Some(l.to_owned()),
                        (true, None) => None,
                        (true, Some(ip)) => Some(l.replace("127.0.0.1", ip)),
                    })
                    .map(|l| l + "\r\n")
                    .collect();
                assert!(!offer.contains("127.0.0.1 "), "{offer}");
                let (rtc, answer) = crate::whip_rtc::answer(&offer, local, Instant::now()).unwrap();
                tx.send(h.add(format!("nat{n}"), rtc).unwrap()).unwrap();
                answer
            };
            let packets: Vec<Vec<u8>> = (0..25).map(|_| packet(n)).collect();
            let c = tokio::spawn(client(answerer, 7, packets));
            let mut rx = tokio::task::spawn_blocking(move || rx.recv().unwrap())
                .await
                .unwrap();
            let mut got = 0;
            while let Ok(Some((_, p))) =
                tokio::time::timeout(Duration::from_secs(5), rx.recv()).await
            {
                assert_eq!(p.last(), Some(&n));
                got += 1;
                if got >= 20 {
                    break;
                }
            }
            c.await.unwrap();
            assert!(got >= 20, "client {n}: {got} payloads");
        }
        drop(handle);
        let _ = tokio::time::timeout(Duration::from_secs(15), running).await;
    }

    #[tokio::test]
    async fn a_session_that_never_connects_ends_after_ten_seconds_of_nothing() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let (handle, mux) = new(local);
        let (rtc, _answer) = crate::whip_rtc::answer(
            &{
                let mut c = RtcConfig::new()
                    .set_crypto_provider(std::sync::Arc::new(str0m::crypto::from_feature_flags()))
                    .build(Instant::now());
                let mut api = c.sdp_api();
                api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
                api.apply().unwrap().0.to_sdp_string()
            },
            local,
            Instant::now(),
        )
        .unwrap();
        let mut rx = handle.add("lonely".into(), rtc).unwrap();
        tokio::spawn(mux.run(socket));
        let ended = tokio::time::timeout(SILENT_END + Duration::from_secs(5), rx.recv()).await;
        assert!(matches!(ended, Ok(None)), "the channel closed: {ended:?}");
    }

    #[tokio::test]
    async fn delete_ends_a_session_and_closes_its_channel() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let (handle, mux) = new(local);
        let mut c = RtcConfig::new()
            .set_crypto_provider(std::sync::Arc::new(str0m::crypto::from_feature_flags()))
            .build(Instant::now());
        let mut api = c.sdp_api();
        api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let offer = api.apply().unwrap().0.to_sdp_string();
        let (rtc, _) = crate::whip_rtc::answer(&offer, local, Instant::now()).unwrap();
        let mut rx = handle.add("s1".into(), rtc).unwrap();
        tokio::spawn(mux.run(socket));
        handle.end("s1");
        let ended = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
        assert!(matches!(ended, Ok(None)), "{ended:?}");
    }
}
