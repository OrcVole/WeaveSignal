// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A WHIP offer answered (17d): one [`str0m::Rtc`] per session, **ICE-lite** with the station's
//! one host candidate (its public IPv4 address and the WHIP port), so no STUN and no TURN are
//! asked of anyone (ADR-0290). Sound only reaches the station: Opus is taken, and a picture the
//! host's software sends is **accepted and discarded** (the operator's choice, 2026-11-20,
//! `whip-video-discard` a): OBS's WHIP output sends sound and picture together and may not take
//! an answer that refuses its picture, so the answer receives the picture in any codec OBS may
//! send (H.264, the default; AV1, H.265, VP8, VP9), and [`crate::whip_mux`] passes on Opus alone
//! and drops the rest. The cost is the picture's bandwidth into the station and its unpacking.
//!
//! The session's I/O is [`crate::whip_mux`]'s; nothing here touches a socket.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use str0m::change::SdpOffer;
use str0m::config::CryptoProvider;
use str0m::{Candidate, Rtc, RtcConfig};

/// Why an offer is not answered.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WhipError {
    /// Not an SDP offer str0m can read (400).
    #[error("the offer is not one the station can read: {0}")]
    Offer(String),
    /// No Opus sound in it (400): the station takes sound, and only Opus.
    #[error("the offer sends no Opus sound")]
    NoAudio,
    /// The station's side failed (500).
    #[error("the station could not answer: {0}")]
    Station(String),
}

/// The DTLS and SRTP backend, made once.
fn crypto() -> Arc<CryptoProvider> {
    static P: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(str0m::crypto::from_feature_flags()))
        .clone()
}

/// The configuration for one session: ICE-lite, Opus for the sound and the video codecs a
/// host's software may send (received, then discarded), the crypto provider set on it rather than
/// as the process's default.
pub fn config() -> RtcConfig {
    RtcConfig::new()
        .set_crypto_provider(crypto())
        .set_ice_lite(true)
        .clear_codecs()
        .enable_opus(true, false)
        .enable_h264(true)
        .enable_av1(true)
        .enable_h265(true)
        .enable_vp8(true)
        .enable_vp9(true)
}

/// Whether an SDP (an offer, or the answer to it) has an `m=audio` line that is not refused
/// (port 0) and names Opus.
fn takes_opus(sdp: &str) -> bool {
    let mut in_audio = false;
    let mut audio_open = false;
    for line in sdp.lines() {
        let line = line.trim();
        if let Some(m) = line.strip_prefix("m=") {
            in_audio = m.starts_with("audio ");
            audio_open = in_audio && m.split_whitespace().nth(1).is_some_and(|p| p != "0");
            continue;
        }
        if in_audio
            && audio_open
            && line.starts_with("a=rtpmap:")
            && line.to_ascii_lowercase().contains(" opus/48000")
        {
            return true;
        }
    }
    false
}

/// Answer `offer` for a session reached at `host` (the station's public address and the WHIP
/// port, its one candidate): the session and the answer's SDP.
pub fn answer(offer: &str, host: SocketAddr, now: Instant) -> Result<(Rtc, String), WhipError> {
    let parsed = SdpOffer::from_sdp_string(offer).map_err(|e| WhipError::Offer(e.to_string()))?;
    if !takes_opus(offer) {
        return Err(WhipError::NoAudio);
    }
    let mut rtc = config().build(now);
    let candidate = Candidate::host(host, "udp").map_err(|e| WhipError::Station(e.to_string()))?;
    if rtc.add_local_candidate(candidate).is_none() {
        return Err(WhipError::Station(format!(
            "{host} is not an address a candidate can have"
        )));
    }
    let answer = rtc
        .sdp_api()
        .accept_offer(parsed)
        .map_err(|e| WhipError::Offer(e.to_string()))?
        .to_sdp_string();
    if !takes_opus(&answer) {
        return Err(WhipError::NoAudio);
    }
    Ok((rtc, answer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use str0m::media::{Direction, MediaKind};

    /// An offer as a sending client makes it, with sound and, if asked, a picture.
    fn client_offer(video: bool, audio: bool) -> String {
        let mut client = RtcConfig::new()
            .set_crypto_provider(crypto())
            .build(Instant::now());
        let mut api = client.sdp_api();
        if audio {
            api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        }
        if video {
            api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        }
        let (offer, _pending) = api.apply().expect("an offer");
        offer.to_sdp_string()
    }

    fn host() -> SocketAddr {
        "203.0.113.7:9712".parse().unwrap()
    }

    /// An offer shaped as OBS's WHIP output makes one: Opus and H.264, sending both.
    fn obs_offer() -> String {
        let mut client = RtcConfig::new()
            .set_crypto_provider(crypto())
            .clear_codecs()
            .enable_opus(true, false)
            .enable_h264(true)
            .build(Instant::now());
        let mut api = client.sdp_api();
        api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        let (offer, _pending) = api.apply().expect("an offer");
        offer.to_sdp_string()
    }

    /// The answer's m-line for `kind`, and whether it takes some codec (port not 0).
    fn m_line<'a>(answer: &'a str, kind: &str) -> &'a str {
        answer
            .lines()
            .find(|l| l.starts_with(&format!("m={kind}")))
            .unwrap_or_else(|| panic!("no m={kind} in {answer}"))
    }

    #[test]
    fn an_obs_offer_gets_its_sound_taken_and_its_picture_received_to_be_discarded() {
        let offer = obs_offer();
        assert!(offer.to_ascii_lowercase().contains("h264/90000"), "{offer}");
        let (_rtc, answer) = answer(&offer, host(), Instant::now()).unwrap();
        assert!(takes_opus(&answer), "{answer}");
        let video = m_line(&answer, "video");
        assert!(
            !video.starts_with("m=video 0 "),
            "the picture was refused: {video}"
        );
        assert!(
            answer.to_ascii_lowercase().contains("h264/90000"),
            "the picture's codec is not in the answer: {answer}"
        );
        // Both are received; the station sends nothing back.
        assert_eq!(answer.matches("a=recvonly").count(), 2, "{answer}");
        assert!(!answer.contains("a=sendrecv"), "{answer}");
    }

    #[test]
    fn sound_and_a_picture_get_opus_and_the_picture_received() {
        let offer = client_offer(true, true);
        assert!(offer.contains("m=video "), "{offer}");
        let (_rtc, answer) = answer(&offer, host(), Instant::now()).unwrap();
        assert!(takes_opus(&answer), "{answer}");
        assert!(answer.contains("a=ice-lite"), "{answer}");
        assert!(
            answer.contains("candidate:") && answer.contains("203.0.113.7 9712 typ host"),
            "one host candidate: {answer}"
        );
        assert_eq!(answer.matches("a=candidate:").count(), 1, "{answer}");
        let video = m_line(&answer, "video");
        assert!(!video.starts_with("m=video 0 "), "refused: {video}");
        assert!(answer.contains("a=recvonly"), "{answer}");
        assert!(!answer.to_ascii_lowercase().contains("stun:"), "{answer}");
    }

    #[test]
    fn an_offer_without_sound_is_refused() {
        let offer = client_offer(true, false);
        let e = answer(&offer, host(), Instant::now()).unwrap_err();
        assert_eq!(e, WhipError::NoAudio);
    }

    #[test]
    fn what_is_not_sdp_is_refused() {
        let e = answer("hello", host(), Instant::now()).unwrap_err();
        assert!(matches!(e, WhipError::Offer(_)), "{e:?}");
    }

    #[test]
    fn takes_opus_reads_the_audio_lines_only() {
        let sdp = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\nm=audio 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 PCMU/8000\r\n";
        assert!(!takes_opus(sdp));
        let sdp = "v=0\r\nm=audio 0 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n";
        assert!(!takes_opus(sdp), "a refused m-line");
        let sdp = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 OPUS/48000/2\r\n";
        assert!(takes_opus(sdp));
    }
}
