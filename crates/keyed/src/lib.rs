// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//! Keyed shows' listening tokens (1.3.2 spike, PLAN 19a; design
//! `docs/reference/design/keyed-shows.md` in the project folder).
//!
//! A listener redeems a key at the control plane and gets a token: short-lived, naming the key,
//! one show, sound or video or both, and a time window. Whoever serves the show (the control
//! plane at the origin, an HLS edge, an audio relay's front) checks it with the station's
//! public key, a clock and nothing else: no database, no call home. Only the control plane
//! holds the private key, so an edge run by someone else can check tokens but never make one.
//!
//! The text form is `ks1.<claims>.<signature>`: the claims as compact JSON and the ECDSA P-256
//! signature (SHA-256, 64 bytes, r then s) of `ks1.<claims>`, both base64url without padding
//! (about 190 characters). P-256, not Ed25519: the edge checks it in nginx's njs, and Alpine
//! 3.24's nginx-mod-http-js (njs 0.9.9 without QuickJS) knows ECDSA but not Ed25519 (the spike,
//! 2026-11-23); both keep the private key at the control plane alone. It goes
//! in the path, never the query (`/hls/k/<show>/<token>/live.m3u8`, `/listen/k/<show>/<token>/
//! programme-keyed.opus`), so a playlist's relative addresses carry it to every segment, and
//! a radio app needs nothing but the address.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ring::rand::SystemRandom;
use ring::signature::{
    ECDSA_P256_SHA256_FIXED, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _,
    UnparsedPublicKey,
};
use serde::{Deserialize, Serialize};

/// The version prefix: a token signed under another scheme can never be read as this one.
pub const PREFIX: &str = "ks1";
/// The longest token text accepted, so a caller can refuse oversized input before decoding.
pub const MAX_LEN: usize = 512;

/// What a token opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Media {
    /// Sound: the keyed Opus and MP3 mounts and the HLS audio rendition.
    #[serde(rename = "a")]
    Sound,
    /// Video: the HLS video renditions.
    #[serde(rename = "v")]
    Video,
    /// Both.
    #[serde(rename = "av")]
    Both,
}

impl Media {
    /// Whether a token for `self` opens a request that needs `want`.
    pub fn opens(self, want: Media) -> bool {
        match (self, want) {
            (Media::Both, _) => true,
            (have, want) => have == want,
        }
    }
}

/// The signed claims.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// The key's identifier (not its text): revocation lists and use records name it.
    #[serde(rename = "k")]
    pub key: String,
    /// The one keyed show this token opens.
    #[serde(rename = "s")]
    pub show: String,
    /// Sound, video or both.
    #[serde(rename = "m")]
    pub media: Media,
    /// Valid from (Unix seconds).
    pub nbf: i64,
    /// Valid until (Unix seconds): the end of the show's window and its margins (1.3.2: a
    /// token lasts the window, and a revoke reaches edges through the revoked list), never past
    /// the key's own end.
    pub exp: i64,
    /// The device that holds it (the control plane's id for this browser or app), so a renewal
    /// finds its place on the key. Not checked by edges; left out of tokens that have none.
    #[serde(rename = "d", default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// Why a token was refused. The order of checks is the order here: a bad signature is never
/// reported as expired, so nothing about an unsigned text is revealed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Malformed,
    BadSignature,
    NotYet,
    Expired,
    WrongShow,
    WrongMedia,
    Revoked,
}

/// The control plane's signer.
pub struct Signer(EcdsaKeyPair, SystemRandom);

impl Signer {
    /// A new private key as PKCS#8, to keep sealed with the station's other secrets.
    pub fn generate() -> Vec<u8> {
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("the system's random numbers")
            .as_ref()
            .to_vec()
    }

    /// From the PKCS#8 that [`Signer::generate`] made.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, ring::error::KeyRejected> {
        let rng = SystemRandom::new();
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &rng)
            .map(|k| Self(k, rng))
    }

    /// From a 32-byte secret (the control plane's, derived from the station's token key under
    /// a label of its own): the same secret makes the same key, so the public key edges hold
    /// lasts as long as the token key and nothing more is stored. A secret that is not a P-256
    /// private key (about one in four billion) is hashed until one is.
    pub fn from_secret(secret: &[u8; 32]) -> Self {
        use p256::elliptic_curve::sec1::ToEncodedPoint as _;
        let mut s = *secret;
        loop {
            if let Ok(sk) = p256::SecretKey::from_slice(&s) {
                let public = sk.public_key().to_encoded_point(false);
                let rng = SystemRandom::new();
                let pair = EcdsaKeyPair::from_private_key_and_public_key(
                    &ECDSA_P256_SHA256_FIXED_SIGNING,
                    &s,
                    public.as_bytes(),
                    &rng,
                )
                .expect("a valid P-256 private key and its public key");
                return Self(pair, rng);
            }
            let d = ring::digest::digest(&ring::digest::SHA256, &s);
            s.copy_from_slice(d.as_ref());
        }
    }

    /// The public key edges are given (65 bytes, uncompressed; base64url in their settings).
    pub fn public_key(&self) -> Vec<u8> {
        self.0.public_key().as_ref().to_vec()
    }

    /// The token text for `claims`.
    pub fn mint(&self, claims: &Claims) -> String {
        let json = serde_json::to_vec(claims).expect("claims are plain data");
        let signed = format!("{PREFIX}.{}", B64.encode(json));
        let sig = self
            .0
            .sign(&self.1, signed.as_bytes())
            .expect("the system's random numbers");
        format!("{signed}.{}", B64.encode(sig.as_ref()))
    }
}

/// What a request needs.
pub struct Want<'a> {
    pub show: &'a str,
    pub media: Media,
}

/// Checks `token` for `want` at `now` (Unix seconds), allowing `skew` seconds of clock
/// difference either side, and refuses a key `revoked` names.
pub fn verify(
    public_key: &[u8],
    token: &str,
    want: &Want<'_>,
    now: i64,
    skew: i64,
    revoked: impl Fn(&str) -> bool,
) -> Result<Claims, Refusal> {
    let claims = read(public_key, token, now, skew)?;
    if claims.show != want.show {
        return Err(Refusal::WrongShow);
    }
    if !claims.media.opens(want.media) {
        return Err(Refusal::WrongMedia);
    }
    if revoked(&claims.key) {
        return Err(Refusal::Revoked);
    }
    Ok(claims)
}

/// Checks a token's form, signature and time only, and reads its claims: what a renewal needs
/// (the show and media are the control plane's to judge again).
pub fn read(public_key: &[u8], token: &str, now: i64, skew: i64) -> Result<Claims, Refusal> {
    if token.len() > MAX_LEN {
        return Err(Refusal::Malformed);
    }
    let (signed, sig) = token.rsplit_once('.').ok_or(Refusal::Malformed)?;
    let (prefix, payload) = signed.split_once('.').ok_or(Refusal::Malformed)?;
    if prefix != PREFIX {
        return Err(Refusal::Malformed);
    }
    let sig = B64.decode(sig).map_err(|_| Refusal::Malformed)?;
    if sig.len() != 64 {
        return Err(Refusal::Malformed);
    }
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key)
        .verify(signed.as_bytes(), &sig)
        .map_err(|_| Refusal::BadSignature)?;
    let json = B64.decode(payload).map_err(|_| Refusal::Malformed)?;
    let claims: Claims = serde_json::from_slice(&json).map_err(|_| Refusal::Malformed)?;
    if now + skew < claims.nbf {
        return Err(Refusal::NotYet);
    }
    if now - skew >= claims.exp {
        return Err(Refusal::Expired);
    }
    Ok(claims)
}

/// What an HLS path under `/hls/k/<show>/<token>/` needs: the audio rendition is sound, the
/// rest (the multivariant playlist and the video renditions) video. The same rule is in the
/// edge's `keyed.js`.
pub fn hls_media(rest: &str) -> Media {
    if rest == "audio.m3u8" || rest.starts_with("audio/") {
        Media::Sound
    } else {
        Media::Video
    }
}

/// A time the public gate was shut for a keyed show, in milliseconds since the Unix epoch:
/// from playout's `keyed` events (shut, open) and the show whose window the control plane had
/// open. `end` is `None` while the gate is still shut.
#[derive(Clone, Debug)]
pub struct Shut {
    pub show: String,
    pub start: i64,
    pub end: Option<i64>,
}

/// What an HLS segment may be served as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// No keyed moment in it: the public path and every keyed path may serve it.
    Public,
    /// Only this keyed show's moments (and perhaps public ones): its keyed path only.
    Keyed(String),
    /// More than one keyed show: no path serves it.
    Mixed,
}

/// Classifies a segment the desk finished writing at `written` (ms), `duration` long, whose
/// picture and sound were taken between `written - duration - latest` and `written - earliest`:
/// `latest` and `earliest` bound the desk's delay before HLS (the profanity delay, the encoder,
/// the to-desk feed's buffer) and must be measured, not assumed. A segment that may hold a
/// moment of a shut interval is keyed to that interval's show; the bounds make the error go one
/// way only (a public moment served as keyed, never the other way).
pub fn classify(written: i64, duration: i64, earliest: i64, latest: i64, shut: &[Shut]) -> Segment {
    let from = written - duration - latest;
    let to = written - earliest;
    let mut show: Option<&str> = None;
    for s in shut {
        let overlaps = s.start <= to && s.end.is_none_or(|e| e >= from);
        if !overlaps {
            continue;
        }
        match show {
            None => show = Some(&s.show),
            Some(x) if x == s.show => {}
            Some(_) => return Segment::Mixed,
        }
    }
    show.map_or(Segment::Public, |s| Segment::Keyed(s.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_keyed_by_what_they_may_hold() {
        let shut = [
            Shut {
                show: "a".into(),
                start: 100_000,
                end: Some(200_000),
            },
            Shut {
                show: "b".into(),
                start: 200_000,
                end: None,
            },
        ];
        // 2 s segments; the desk's delay before HLS is 7 to 9 s (a 7 s profanity delay).
        let c = |written| classify(written, 2_000, 7_000, 9_000, &shut);
        assert_eq!(
            c(100_000),
            Segment::Public,
            "taken 89-93 s, before the gate shut"
        );
        assert_eq!(
            c(106_999),
            Segment::Public,
            "taken to 99.999 s, before the gate shut"
        );
        assert_eq!(
            c(107_000),
            Segment::Keyed("a".into()),
            "may hold 100.000 s: the edges count"
        );
        assert_eq!(c(150_000), Segment::Keyed("a".into()));
        assert_eq!(
            c(207_000),
            Segment::Mixed,
            "may hold the end of a and the start of b"
        );
        assert_eq!(
            c(212_000),
            Segment::Keyed("b".into()),
            "taken 201-205 s, b still shut"
        );
        let open = [Shut {
            show: "a".into(),
            start: 100_000,
            end: Some(200_000),
        }];
        assert_eq!(
            classify(211_001, 2_000, 7_000, 9_000, &open),
            Segment::Public,
            "200.001-204 s"
        );
        assert_eq!(
            classify(211_000, 2_000, 7_000, 9_000, &open),
            Segment::Keyed("a".into())
        );
    }

    fn station() -> &'static [u8] {
        static KEY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        KEY.get_or_init(Signer::generate)
    }
    const NOW: i64 = 1_795_000_000;

    fn claims() -> Claims {
        Claims {
            key: "k-0001".into(),
            show: "show-a".into(),
            media: Media::Sound,
            nbf: NOW - 10,
            exp: NOW + 120,
            device: None,
        }
    }
    fn signer() -> Signer {
        Signer::from_pkcs8(station()).unwrap()
    }
    fn check(token: &str, show: &str, media: Media, now: i64) -> Result<Claims, Refusal> {
        verify(
            &signer().public_key(),
            token,
            &Want { show, media },
            now,
            5,
            |k| k == "k-revoked",
        )
    }

    #[test]
    fn a_minted_token_opens_its_show() {
        let t = signer().mint(&claims());
        assert!(
            t.starts_with("ks1.") && t.len() < 220,
            "{} chars: {t}",
            t.len()
        );
        assert_eq!(check(&t, "show-a", Media::Sound, NOW), Ok(claims()));
    }

    #[test]
    fn every_refusal() {
        let s = signer();
        let t = s.mint(&claims());
        assert_eq!(
            check(&t, "show-b", Media::Sound, NOW),
            Err(Refusal::WrongShow)
        );
        assert_eq!(
            check(&t, "show-a", Media::Video, NOW),
            Err(Refusal::WrongMedia)
        );
        assert_eq!(
            check(&t, "show-a", Media::Sound, NOW + 125),
            Err(Refusal::Expired)
        );
        assert_eq!(
            check(&t, "show-a", Media::Sound, NOW + 124),
            Ok(claims()),
            "inside the skew"
        );
        assert_eq!(
            check(&t, "show-a", Media::Sound, NOW - 16),
            Err(Refusal::NotYet)
        );
        let r = s.mint(&Claims {
            key: "k-revoked".into(),
            ..claims()
        });
        assert_eq!(
            check(&r, "show-a", Media::Sound, NOW),
            Err(Refusal::Revoked)
        );
        // Another station's key signs nothing here.
        let other = Signer::from_pkcs8(&Signer::generate())
            .unwrap()
            .mint(&claims());
        assert_eq!(
            check(&other, "show-a", Media::Sound, NOW),
            Err(Refusal::BadSignature)
        );
        for bad in [
            "",
            "ks1",
            "ks1.x",
            "ks2.e30.AAAA",
            "ks1..",
            &"a".repeat(600),
        ] {
            assert_eq!(
                check(bad, "show-a", Media::Sound, NOW),
                Err(Refusal::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_changed_claim_breaks_the_signature() {
        let t = signer().mint(&claims());
        let (signed, sig) = t.rsplit_once('.').unwrap();
        let (_, payload) = signed.split_once('.').unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(&B64.decode(payload).unwrap()).unwrap();
        json["exp"] = (NOW + 999_999).into();
        json["m"] = "av".into();
        let forged = format!(
            "ks1.{}.{sig}",
            B64.encode(serde_json::to_vec(&json).unwrap())
        );
        assert_eq!(
            check(&forged, "show-a", Media::Video, NOW),
            Err(Refusal::BadSignature)
        );
    }

    #[test]
    fn both_opens_sound_and_video() {
        let t = signer().mint(&Claims {
            media: Media::Both,
            ..claims()
        });
        assert!(check(&t, "show-a", Media::Sound, NOW).is_ok());
        assert!(check(&t, "show-a", Media::Video, NOW).is_ok());
        let v = signer().mint(&Claims {
            media: Media::Video,
            ..claims()
        });
        assert_eq!(
            check(&v, "show-a", Media::Sound, NOW),
            Err(Refusal::WrongMedia)
        );
    }

    #[test]
    fn a_secret_makes_the_same_key_every_time_and_another_secret_another() {
        let a = Signer::from_secret(&[7; 32]);
        let again = Signer::from_secret(&[7; 32]);
        assert_eq!(a.public_key(), again.public_key());
        assert_eq!(a.public_key().len(), 65);
        assert_ne!(a.public_key(), Signer::from_secret(&[8; 32]).public_key());
        // Not a private key (zero, and the curve's order and above): hashed until one is.
        assert_eq!(Signer::from_secret(&[0; 32]).public_key().len(), 65);
        assert_eq!(Signer::from_secret(&[0xff; 32]).public_key().len(), 65);
        let t = a.mint(&claims());
        assert_eq!(
            verify(
                &again.public_key(),
                &t,
                &Want {
                    show: "show-a",
                    media: Media::Sound
                },
                NOW,
                5,
                |_| false
            ),
            Ok(claims())
        );
    }

    #[test]
    fn the_device_rides_along_and_a_renewal_reads_without_a_show() {
        let c = Claims {
            device: Some("d-1".into()),
            ..claims()
        };
        let t = signer().mint(&c);
        assert_eq!(read(&signer().public_key(), &t, NOW, 5), Ok(c.clone()));
        let json: serde_json::Value =
            serde_json::from_slice(&B64.decode(t.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(json["d"], "d-1");
        // Without one, no `d` at all.
        let t = signer().mint(&claims());
        assert!(
            !t.split('.')
                .nth(1)
                .map(|p| String::from_utf8(B64.decode(p).unwrap()).unwrap())
                .unwrap()
                .contains("\"d\"")
        );
        assert_eq!(
            read(&signer().public_key(), &t, NOW + 200, 5),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn hls_paths_need_the_right_media() {
        assert_eq!(hls_media("audio.m3u8"), Media::Sound);
        assert_eq!(hls_media("audio/seg00012.m4s"), Media::Sound);
        assert_eq!(hls_media("live.m3u8"), Media::Video);
        assert_eq!(hls_media("v0/index.m3u8"), Media::Video);
        assert_eq!(hls_media("v1/seg00012.m4s"), Media::Video);
    }
}
