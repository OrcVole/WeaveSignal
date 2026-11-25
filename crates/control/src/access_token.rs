// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The signed token a keyed show's listener carries (1.3.2).
//!
//! One format, owned by the `weavesignal-keyed` crate, which the control plane makes and checks
//! and every edge checks in nginx's njs (`deploy/images/edge/keyed.js`): `ks1.<claims>.<sig>`,
//! the claims as compact JSON and an ECDSA P-256 signature (SHA-256, r then s), both base64url
//! without padding, about 200 characters. P-256 and not Ed25519 because the edge's njs cannot
//! check Ed25519 (the spike, 2026-11-23). An edge holds only the public key: it can check tokens
//! and cannot make one.
//!
//! | Claim | Meaning |
//! |---|---|
//! | `k` | the key's id (never its text) |
//! | `s` | the one show it opens (its id) |
//! | `m` | `a` sound, `v` video, `av` both |
//! | `nbf`, `exp` | from the show's window's lead to its tail and the HLS lag, in Unix seconds; never past the key's end |
//! | `d` | the device's id, for the renewal (edges ignore it) |
//!
//! The token goes in the path (`/hls/k/<show>/<token>/live.m3u8`, `/listen/k/<show>/<token>/
//! programme-keyed.opus`), never the query, so a playlist's relative addresses carry it to every
//! segment. It lasts the show's window (the operator's choice for players that cannot change
//! it mid-play): a key revoked or paused is refused by edges and the origin through the revoked
//! list, and at the holder's next renewal.
//!
//! Behind the signature sits [`TokenSigner`], so another scheme could be added without changing
//! keys, claims or routes; today it has one implementation, [`P256Signer`].

pub use weavesignal_keyed::{Claims, Media as TokenMedia, Refusal as TokenError};
use weavesignal_model::listener_keys::Media;

/// The most shows a key names; a key that opens more is made `all_shows` or refused.
pub const MAX_SHOWS: usize = 32;

/// What signs tokens and checks their signatures.
pub trait TokenSigner: Send + Sync {
    /// The algorithm's name, as `GET /api/v1/listener-keys/signing` says it.
    fn alg(&self) -> &'static str;
    /// The token text for these claims.
    fn mint(&self, claims: &Claims) -> String;
    /// The public key an edge checks tokens with (raw, uncompressed for P-256).
    fn public_key(&self) -> Vec<u8>;
}

/// ECDSA P-256 from a 32-byte secret derived from the station's token key: the same token key
/// makes the same signing key, so the public key edges hold lasts as long as it.
pub struct P256Signer {
    signer: weavesignal_keyed::Signer,
    public: Vec<u8>,
}

impl P256Signer {
    pub fn from_secret(secret: &[u8; 32]) -> Self {
        let signer = weavesignal_keyed::Signer::from_secret(secret);
        let public = signer.public_key();
        Self { signer, public }
    }
}

impl TokenSigner for P256Signer {
    fn alg(&self) -> &'static str {
        "p256"
    }

    fn mint(&self, claims: &Claims) -> String {
        self.signer.mint(claims)
    }

    fn public_key(&self) -> Vec<u8> {
        self.public.clone()
    }
}

/// The token's media for a key's. A key for the picture carries the show's sound with it (the
/// operator, 2026-11-25: a picture with no voice is never what anyone means), so its token
/// opens both: the keyed Opus and MP3 streams and the HLS audio rendition beside the picture.
/// It is said here, in what is minted, and not in the check: an edge on an older image reads
/// `av` as it always did, so a video key is heard through it without a new edge image. No key
/// mints a picture-only token (`v`); the checks still know one, for a token made before this.
pub fn media(m: Media) -> TokenMedia {
    match m {
        Media::Sound => TokenMedia::Sound,
        Media::Video | Media::Both => TokenMedia::Both,
    }
}

/// Check a token's form, signature and time (with `skew_s` of leeway) and read its claims.
pub fn decode(
    signer: &dyn TokenSigner,
    token: &str,
    now_s: i64,
    skew_s: i64,
) -> Result<Claims, TokenError> {
    weavesignal_keyed::read(&signer.public_key(), token, now_s, skew_s)
}

/// Check a token for one show and medium, refusing a key `revoked` names.
pub fn check(
    signer: &dyn TokenSigner,
    token: &str,
    show: &str,
    want: TokenMedia,
    now_s: i64,
    skew_s: i64,
    revoked: impl Fn(&str) -> bool,
) -> Result<Claims, TokenError> {
    weavesignal_keyed::verify(
        &signer.public_key(),
        token,
        &weavesignal_keyed::Want { show, media: want },
        now_s,
        skew_s,
        revoked,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(now: i64) -> Claims {
        Claims {
            key: "00000000-0000-0000-0000-000000000001".into(),
            show: "00000000-0000-0000-0000-00000000000a".into(),
            media: TokenMedia::Both,
            nbf: now,
            exp: now + 120,
            device: Some("00000000-0000-0000-0000-000000000002".into()),
        }
    }

    #[test]
    fn a_token_round_trips_and_is_the_edges_format() {
        let s = P256Signer::from_secret(&[7; 32]);
        let t = s.mint(&claims(1_000));
        assert!(t.starts_with("ks1."), "{t}");
        assert_eq!(decode(&s, &t, 1_050, 5), Ok(claims(1_000)));
        assert_eq!(s.public_key().len(), 65);
    }

    #[test]
    fn an_expired_token_is_refused_beyond_its_leeway() {
        let s = P256Signer::from_secret(&[7; 32]);
        let t = s.mint(&claims(1_000));
        assert!(decode(&s, &t, 1_119, 0).is_ok());
        assert_eq!(decode(&s, &t, 1_120, 0), Err(TokenError::Expired));
        assert!(decode(&s, &t, 1_124, 5).is_ok());
        assert_eq!(decode(&s, &t, 1_125, 5), Err(TokenError::Expired));
    }

    #[test]
    fn another_stations_token_is_refused() {
        let a = P256Signer::from_secret(&[7; 32]);
        let b = P256Signer::from_secret(&[8; 32]);
        let t = a.mint(&claims(1_000));
        assert_eq!(decode(&b, &t, 1_010, 0), Err(TokenError::BadSignature));
        for bad in ["", "wk1e.a.b", "ks1.a.b", "ks1"] {
            assert!(decode(&a, bad, 1_010, 0).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_token_opens_its_show_and_its_media_only() {
        let s = P256Signer::from_secret(&[7; 32]);
        let mut c = claims(1_000);
        c.media = TokenMedia::Sound;
        let t = s.mint(&c);
        let show = c.show.clone();
        assert!(check(&s, &t, &show, TokenMedia::Sound, 1_010, 0, |_| false).is_ok());
        assert_eq!(
            check(&s, &t, &show, TokenMedia::Video, 1_010, 0, |_| false),
            Err(TokenError::WrongMedia)
        );
        assert_eq!(
            check(&s, &t, "another", TokenMedia::Sound, 1_010, 0, |_| false),
            Err(TokenError::WrongShow)
        );
        assert_eq!(
            check(&s, &t, &show, TokenMedia::Sound, 1_010, 0, |k| k == c.key),
            Err(TokenError::Revoked)
        );
    }

    #[test]
    fn a_video_keys_token_opens_the_picture_and_the_shows_sound() {
        let s = P256Signer::from_secret(&[7; 32]);
        let show = claims(1_000).show;
        let token = |m| {
            s.mint(&Claims {
                media: media(m),
                ..claims(1_000)
            })
        };
        // What each request needs: the keyed Opus and MP3 streams are sound; under the keyed
        // HLS the audio rendition is sound and the rest (the picture) video.
        let stream = TokenMedia::Sound;
        let needs = weavesignal_keyed::hls_media;
        for m in [Media::Video, Media::Both] {
            let t = token(m);
            for want in [
                stream,
                needs("audio.m3u8"),
                needs("audio/seg00012.m4s"),
                needs("live.m3u8"),
                needs("v0/seg00012.m4s"),
            ] {
                assert!(
                    check(&s, &t, &show, want, 1_010, 0, |_| false).is_ok(),
                    "a {m:?} key, asked for {want:?}"
                );
            }
        }
        // A sound key is as it was: the sound, and never the picture.
        let t = token(Media::Sound);
        assert!(check(&s, &t, &show, stream, 1_010, 0, |_| false).is_ok());
        assert!(check(&s, &t, &show, needs("audio.m3u8"), 1_010, 0, |_| false).is_ok());
        assert_eq!(
            check(&s, &t, &show, needs("live.m3u8"), 1_010, 0, |_| false),
            Err(TokenError::WrongMedia)
        );
    }
}
