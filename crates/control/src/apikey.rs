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
// rtmp-out: the key RTMP stream keys are sealed under (ADR-0230).
const RTMP_LABEL: &[u8] = b"weavesignal-control/rtmp-stream-key/v1";
// Hosts in their own studios (ADR-0280): a show key's secret, derived; a monitor link's, hashed.
const SHOW_KEY_LABEL: &[u8] = b"weavesignal-control/show-key-secret/v1";
const MONITOR_LABEL: &[u8] = b"weavesignal-control/monitor-secret/v1";
// The phone line (17f): the key a caller's ban key is made under; never in the database.
const PHONE_BAN_LABEL: &[u8] = b"weavesignal-control/phone-ban-key/v1";
// Keyed shows (1.3.2): a listener key's text is found by a keyed hash and kept sealed; an email
// address and a listener's address are kept only as keyed hashes; tokens are signed under a key
// of their own. Each label makes a different key from the same secret.
const LISTENER_TEXT_LABEL: &[u8] = b"weavesignal-control/listener-key-text/v1";
const LISTENER_SEAL_LABEL: &[u8] = b"weavesignal-control/listener-key-seal/v1";
const LISTENER_EMAIL_LABEL: &[u8] = b"weavesignal-control/listener-key-email/v1";
const LISTENER_ADDR_LABEL: &[u8] = b"weavesignal-control/listener-key-address/v1";
const LISTENER_TOKEN_LABEL: &[u8] = b"weavesignal-control/listener-key-token/v1";

/// Base 32 in lower case (RFC 4648's alphabet, no padding): what show keys are written in, so a
/// key id and a secret survive any software's field and join unambiguously with a hyphen.
pub fn base32(bytes: &[u8]) -> String {
    const ABC: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ABC[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ABC[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

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

    /// The key RTMP out's stream keys are sealed under in the database (ADR-0230, `rtmp-out`),
    /// 32 bytes for ChaCha20-Poly1305; changing the token key leaves them unreadable (an admin
    /// enters them again).
    pub fn rtmp_key(&self) -> [u8; 32] {
        self.mac(RTMP_LABEL).finalize().into_bytes().into()
    }

    /// A show key's secret (ADR-0280): 24 base-32 characters (120 bits), derived from its key id
    /// and the salt the database keeps, so the database alone cannot give it. The SRT listener
    /// needs it in plain to answer the handshake, which is why it is derived, not only hashed.
    pub fn show_key_secret(&self, key_id: &str, salt: &[u8]) -> String {
        let mut m = self.mac(SHOW_KEY_LABEL);
        m.update(key_id.as_bytes());
        m.update(&[0]);
        m.update(salt);
        base32(&m.finalize().into_bytes()[..15])
    }

    /// The key phone callers' ban keys are made under (17f): a number's ban key is the same
    /// for as long as this key is, and a copy of the database alone cannot be turned back into
    /// numbers. Changing the token key lapses every number ban (they no longer match).
    pub fn phone_ban_keys(&self) -> weavesignal_model::caller_id::BanKeys {
        weavesignal_model::caller_id::BanKeys::new(
            self.mac(PHONE_BAN_LABEL).finalize().into_bytes().into(),
        )
    }

    /// The keyed hash a listener key's (normalised) text is found by.
    pub fn listener_text_hash(&self, norm_text: &str) -> [u8; 32] {
        let mut m = self.mac(LISTENER_TEXT_LABEL);
        m.update(norm_text.as_bytes());
        m.finalize().into_bytes().into()
    }

    /// The key listener keys' text and email addresses are sealed under (ChaCha20-Poly1305).
    /// Changing the token key leaves them unreadable: staff replace the keys (the hashes that
    /// find a key by its text change with it too, so no key text works any more).
    pub fn listener_seal_key(&self) -> [u8; 32] {
        self.mac(LISTENER_SEAL_LABEL).finalize().into_bytes().into()
    }

    /// The keyed hash of a listener key's email address (trimmed, lower case): it finds a
    /// holder's keys for the lost-key request and is not the address.
    pub fn listener_email_hash(&self, email: &str) -> [u8; 32] {
        let mut m = self.mac(LISTENER_EMAIL_LABEL);
        m.update(email.trim().to_lowercase().as_bytes());
        m.finalize().into_bytes().into()
    }

    /// A listener's address as a keyed hash, cut to 8 bytes (64 bits): enough to tell addresses
    /// apart in a leak report and not to turn back into one. Fold an IPv6 address to its /64
    /// first.
    pub fn listener_addr_hash(&self, addr: std::net::IpAddr) -> [u8; 8] {
        let mut m = self.mac(LISTENER_ADDR_LABEL);
        match addr {
            std::net::IpAddr::V4(a) => m.update(&a.octets()),
            std::net::IpAddr::V6(a) => m.update(&a.octets()),
        }
        let full = m.finalize().into_bytes();
        let mut out = [0u8; 8];
        out.copy_from_slice(&full[..8]);
        out
    }

    /// The secret the ECDSA P-256 key that signs keyed shows' tokens is made from (1.3.2): the
    /// same token key, the same signing key and the same public key at every edge.
    pub fn listener_token_key(&self) -> [u8; 32] {
        self.mac(LISTENER_TOKEN_LABEL)
            .finalize()
            .into_bytes()
            .into()
    }

    /// The stored form of a monitor link's secret.
    pub fn monitor_hash(&self, secret: &str) -> [u8; 32] {
        let mut m = self.mac(MONITOR_LABEL);
        m.update(secret.as_bytes());
        m.finalize().into_bytes().into()
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
    fn a_show_keys_secret_follows_its_id_salt_and_key() {
        let s = key(A).show_key_secret("abcd2345", &[1; 16]);
        assert_eq!(s.len(), 24);
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
        );
        assert_eq!(s, key(A).show_key_secret("abcd2345", &[1; 16]));
        assert_ne!(s, key(A).show_key_secret("abcd2346", &[1; 16]));
        assert_ne!(s, key(A).show_key_secret("abcd2345", &[2; 16]));
        assert_ne!(s, key(B).show_key_secret("abcd2345", &[1; 16]));
        assert_ne!(key(A).monitor_hash(&s), key(A).token_hash(&s));
    }

    #[test]
    fn base32_is_rfc_4648_in_lower_case() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "my");
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
        assert_eq!(base32(&[0xff; 5]), "77777777");
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
    fn listener_key_hashes_are_keyed_and_apart() {
        let (a, b) = (key(A), key(B));
        assert_eq!(
            a.listener_text_hash("KETM7QMP2D4X"),
            a.listener_text_hash("KETM7QMP2D4X")
        );
        assert_ne!(
            a.listener_text_hash("KETM7QMP2D4X"),
            b.listener_text_hash("KETM7QMP2D4X")
        );
        // The same text under another use's label is another value.
        assert_ne!(a.listener_text_hash("x"), a.listener_email_hash("x"));
        assert_ne!(a.listener_text_hash("x"), a.token_hash("x"));
        // An email is the same whatever its case and edges.
        assert_eq!(
            a.listener_email_hash(" Ishbel@Example.org "),
            a.listener_email_hash("ishbel@example.org")
        );
        let (ip1, ip2): (std::net::IpAddr, std::net::IpAddr) = (
            "203.0.113.7".parse().unwrap(),
            "203.0.113.8".parse().unwrap(),
        );
        assert_eq!(a.listener_addr_hash(ip1), a.listener_addr_hash(ip1));
        assert_ne!(a.listener_addr_hash(ip1), a.listener_addr_hash(ip2));
        assert_ne!(a.listener_addr_hash(ip1), b.listener_addr_hash(ip1));
        assert_ne!(a.listener_seal_key(), a.rtmp_key());
        assert_ne!(a.listener_seal_key(), a.listener_token_key());
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        assert!(!format!("{:?}", key(A)).contains("0123"));
    }
}
