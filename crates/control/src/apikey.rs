// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The key that keeps API tokens and webhook secrets out of the database (ADR-0195).
//!
//! A token's secret is kept as an HMAC-SHA-256 of the secret under a key the station holds
//! outside the database, so a copy of the database alone is not a set of working tokens. A
//! webhook's signing secret is not kept at all: it is derived under the same key from the
//! webhook's id and a salt the database holds, so the same copy cannot sign a delivery either.
//! Each use has its own label, so a hash made for one is never valid as another.
//!
//! **Where the key comes from** ([`ApiKey::resolve`]): `WEAVESIGNAL_CONTROL_TOKEN_KEY` if it is
//! set (at least 32 characters); otherwise one derived, under a label of its own, from the
//! station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`), which every bundle already sets; with
//! neither, there is no key and the feature **fails closed**: tokens cannot be made and no
//! `rsa_` bearer is accepted. Changing the key revokes every token and ends every webhook's
//! signatures (an admin makes them again), so a station that rotates its station key and wants
//! its tokens to survive sets `WEAVESIGNAL_CONTROL_TOKEN_KEY` first.

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const TOKEN_LABEL: &[u8] = b"weavesignal-control/api-token/v1";
const WEBHOOK_LABEL: &[u8] = b"weavesignal-control/webhook-secret/v1";
const DERIVE_LABEL: &[u8] = b"weavesignal-control/api-key-from-station-key/v1";

#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(Vec<u8>);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error(
        "{} is {} characters; it must be at least {} (for example `openssl rand -hex 32`)",
        ApiKey::ENV,
        .0,
        ApiKey::MIN_LEN
    )]
    TooShort(usize),
}

impl ApiKey {
    /// Where the key comes from: the environment, never the settings file.
    pub const ENV: &'static str = "WEAVESIGNAL_CONTROL_TOKEN_KEY";
    /// The shortest key accepted, in characters (as for the mail key and the chat's).
    pub const MIN_LEN: usize = 32;
    /// What a webhook secret starts with.
    pub const WEBHOOK_PREFIX: &'static str = "whsec_";

    /// A key from its text; surrounding whitespace is not part of it.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let text = text.trim();
        let n = text.chars().count();
        if n < Self::MIN_LEN {
            return Err(KeyError::TooShort(n));
        }
        Ok(Self(text.as_bytes().to_vec()))
    }

    /// The key a station has: its own, else one derived from the station key; `None` with
    /// neither (the feature is then off). A key of its own that is too short is an error:
    /// the station meant to set one.
    pub fn resolve(own: Option<&str>, station_key: Option<&str>) -> Result<Option<Self>, KeyError> {
        if let Some(own) = own {
            return Self::parse(own).map(Some);
        }
        Ok(station_key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(|k| {
                let mut m = HmacSha256::new_from_slice(k.as_bytes())
                    .expect("HMAC takes a key of any length");
                m.update(DERIVE_LABEL);
                Self(m.finalize().into_bytes().to_vec())
            }))
    }

    fn mac(&self, label: &[u8]) -> HmacSha256 {
        let mut m = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        m.update(label);
        m.update(&[0]);
        m
    }

    /// The stored form of a token's secret.
    pub fn token_hash(&self, secret: &str) -> [u8; 32] {
        let mut m = self.mac(TOKEN_LABEL);
        m.update(secret.as_bytes());
        m.finalize().into_bytes().into()
    }

    /// A webhook's signing secret: `whsec_` and 64 hex digits, the same for the same id and salt.
    pub fn webhook_secret(&self, id: Uuid, salt: &[u8; 16]) -> String {
        let mut m = self.mac(WEBHOOK_LABEL);
        m.update(id.as_bytes());
        m.update(salt);
        let bytes = m.finalize().into_bytes();
        format!("{}{}", Self::WEBHOOK_PREFIX, hex(&bytes))
    }
}

/// `sha256=<hex>`: what `X-WeaveSignal-Signature` carries, the HMAC-SHA-256 of the body under
/// the webhook's secret (the whole `whsec_…` text).
pub fn signature(secret: &str, body: &[u8]) -> String {
    let mut m =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    m.update(body);
    format!("sha256={}", hex(&m.finalize().into_bytes()))
}

/// What `X-WeaveSignal-Signature` carries on a delivery: [`signature`] over the timestamp
/// (`X-WeaveSignal-Timestamp`, Unix seconds), a full stop, and the exact body. A receiver that
/// checks it and refuses a timestamp more than a few minutes old cannot be replayed an old
/// delivery.
pub fn delivery_signature(secret: &str, timestamp: u64, body: &[u8]) -> String {
    let mut signed = format!("{timestamp}.").into_bytes();
    signed.extend_from_slice(body);
    signature(secret, &signed)
}

pub fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210";

    fn key(s: &str) -> ApiKey {
        ApiKey::parse(s).unwrap()
    }

    #[test]
    fn a_short_key_is_refused_and_names_its_variable() {
        let e = ApiKey::parse("short").unwrap_err();
        assert_eq!(e, KeyError::TooShort(5));
        assert!(e.to_string().contains("WEAVESIGNAL_CONTROL_TOKEN_KEY"));
        assert_eq!(key(A), key(&format!("{A}\n")));
    }

    #[test]
    fn the_key_is_its_own_else_the_station_keys_else_none() {
        assert_eq!(
            ApiKey::resolve(Some(A), Some("station")).unwrap(),
            Some(key(A))
        );
        assert_eq!(
            ApiKey::resolve(Some("short"), Some("station")),
            Err(KeyError::TooShort(5))
        );
        let derived = ApiKey::resolve(None, Some("station-key")).unwrap().unwrap();
        assert_ne!(derived, key("station-key-station-key-station-key"));
        assert_eq!(
            Some(&derived),
            ApiKey::resolve(None, Some(" station-key\n"))
                .unwrap()
                .as_ref()
        );
        assert_eq!(ApiKey::resolve(None, None).unwrap(), None);
        assert_eq!(ApiKey::resolve(None, Some("  ")).unwrap(), None);
    }

    #[test]
    fn a_token_hash_is_keyed() {
        let h = key(A).token_hash("rsa_secret");
        assert_eq!(h, key(A).token_hash("rsa_secret"));
        assert_ne!(h, key(A).token_hash("rsa_secreT"));
        assert_ne!(h, key(B).token_hash("rsa_secret"));
    }

    #[test]
    fn a_webhook_secret_follows_its_id_salt_and_key() {
        let (id, salt) = (Uuid::new_v4(), [7u8; 16]);
        let s = key(A).webhook_secret(id, &salt);
        assert!(s.starts_with("whsec_") && s.len() == 6 + 64, "{s}");
        assert_eq!(s, key(A).webhook_secret(id, &salt));
        assert_ne!(s, key(A).webhook_secret(id, &[8u8; 16]));
        assert_ne!(s, key(A).webhook_secret(Uuid::new_v4(), &salt));
        assert_ne!(s, key(B).webhook_secret(id, &salt));
    }

    #[test]
    fn the_signature_is_the_hmac_of_the_body() {
        // RFC 4231 test case 2.
        assert_eq!(
            signature("Jefe", b"what do ya want for nothing?"),
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn a_delivery_signs_its_timestamp_with_its_body() {
        let s = delivery_signature("whsec_x", 1_700_000_000, b"{}");
        assert_eq!(s, signature("whsec_x", b"1700000000.{}"));
        assert_ne!(s, delivery_signature("whsec_x", 1_700_000_001, b"{}"));
        assert_ne!(s, signature("whsec_x", b"{}"));
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        assert!(!format!("{:?}", key(A)).contains("0123"));
    }
}
