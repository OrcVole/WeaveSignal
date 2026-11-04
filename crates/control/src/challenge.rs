// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Call-in's challenge (ADR-0006: "challenge and rate limit"): a small proof of work the
//! caller's own browser does while they type, instead of a third-party CAPTCHA.
//!
//! **How it works.** `GET /api/v1/call-in/challenge` hands out a signed challenge: a random
//! salt, a difficulty in bits and an expiry, MACed with a key the control plane makes at
//! start. The page finds a `nonce` (decimal digits) such that `SHA-256(salt ":" nonce)` starts
//! with that many zero bits, in a Web Worker, while the caller fills in the lobby; the call-in
//! route checks the MAC, the expiry and the hash, and spends the salt so one answer places one
//! call. Nothing about the caller is sent anywhere else, and nothing is stored but the spent
//! salts until they expire.
//!
//! **Why this and not a puzzle.** It asks nothing of the person: no images, no audio, no
//! timing, nothing to perceive or solve (WCAG 2.2 SC 3.3.8, accessible authentication), so a
//! screen-reader user and a switch user call in exactly as anyone else does. It costs a flood
//! CPU time per call on top of the per-address and station-wide limits; it does not stop a
//! determined attacker with fast hardware, which the limits and the screener still handle.
//!
//! **Under pressure** (the station's minute already half used), new challenges ask for
//! `challenge_bits_busy` bits instead, so a flood gets slower while a single caller still gets
//! through in seconds.

use std::collections::BTreeMap;
use std::sync::Mutex;

use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = hmac::Hmac<Sha256>;

/// The token's version prefix.
const PREFIX: &str = "rsc1";
/// The most bits a station may ask for: about 17 million hashes, minutes on a slow phone.
pub const MAX_BITS: u8 = 24;
/// The nonce's length bound, in digits (u64's range).
const NONCE_MAX: usize = 20;

/// A challenge as the page receives it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Challenge {
    /// The signed challenge, sent back as it is.
    pub token: String,
    /// What the page hashes: `salt ":" nonce`.
    pub salt: String,
    pub bits: u8,
    pub expires_at_ms: u64,
}

/// The page's answer, in the call-in request.
#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub token: String,
    pub nonce: String,
}

/// Why an answer is refused; each says what the page should do (fetch a new challenge).
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the check is missing")]
    Missing,
    #[error("the check is not one this station issued")]
    Forged,
    #[error("the check expired")]
    Expired,
    #[error("the check was already used")]
    Spent,
    #[error("the check's answer is wrong")]
    Wrong,
}

/// The challenges' key and the salts already spent.
pub struct Challenges {
    key: [u8; 32],
    spent: Mutex<BTreeMap<String, u64>>,
}

impl std::fmt::Debug for Challenges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Challenges").finish_non_exhaustive()
    }
}

impl Default for Challenges {
    /// A fresh random key: challenges outstanding when the control plane restarts are refused,
    /// and the page fetches a new one.
    fn default() -> Self {
        let mut key = [0u8; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self::with_key(key)
    }
}

/// Whether `SHA-256(salt ":" nonce)` starts with `bits` zero bits.
pub fn meets(salt: &str, nonce: &str, bits: u8) -> bool {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(b":");
    h.update(nonce.as_bytes());
    leading_zero_bits(&h.finalize()) >= u32::from(bits)
}

fn leading_zero_bits(digest: &[u8]) -> u32 {
    let mut n = 0;
    for &b in digest {
        if b == 0 {
            n += 8;
        } else {
            return n + b.leading_zeros();
        }
    }
    n
}

/// The first nonce from 0 that meets `bits` (for tests and the cost measurement).
pub fn solve(salt: &str, bits: u8) -> u64 {
    (0u64..)
        .find(|n| meets(salt, &n.to_string(), bits))
        .expect("some nonce meets any bits up to 256")
}

impl Challenges {
    pub fn with_key(key: [u8; 32]) -> Self {
        Self {
            key,
            spent: Mutex::new(BTreeMap::new()),
        }
    }

    fn sign(&self, body: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.key).expect("HMAC takes a key of any length");
        mac.update(body.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A new challenge of `bits`, good until `now_ms + ttl_ms`.
    pub fn issue(&self, bits: u8, now_ms: u64, ttl_ms: u64) -> Challenge {
        let bits = bits.min(MAX_BITS);
        let salt = uuid::Uuid::new_v4().simple().to_string();
        let exp = now_ms + ttl_ms;
        let body = format!("{PREFIX}.{exp}.{bits}.{salt}");
        let sig = self.sign(&body);
        Challenge {
            token: format!("{body}.{sig}"),
            salt,
            bits,
            expires_at_ms: exp,
        }
    }

    /// Check an answer and spend its salt; `Ok` is the bits it was worth.
    pub fn redeem(&self, answer: Option<&Answer>, now_ms: u64) -> Result<u8, Refused> {
        let a = answer.ok_or(Refused::Missing)?;
        let mut parts = a.token.split('.');
        let (Some(PREFIX), Some(exp), Some(bits), Some(salt), Some(sig), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(Refused::Forged);
        };
        let body = format!("{PREFIX}.{exp}.{bits}.{salt}");
        if !crate::app::same(self.sign(&body).as_bytes(), sig.as_bytes()) {
            return Err(Refused::Forged);
        }
        let (Ok(exp), Ok(bits)) = (exp.parse::<u64>(), bits.parse::<u8>()) else {
            return Err(Refused::Forged);
        };
        if now_ms >= exp {
            return Err(Refused::Expired);
        }
        if a.nonce.is_empty()
            || a.nonce.len() > NONCE_MAX
            || !a.nonce.bytes().all(|b| b.is_ascii_digit())
            || !meets(salt, &a.nonce, bits)
        {
            return Err(Refused::Wrong);
        }
        let mut spent = self.spent.lock().expect("challenges lock");
        spent.retain(|_, &mut until| until > now_ms);
        if spent.insert(salt.to_owned(), exp).is_some() {
            return Err(Refused::Spent);
        }
        Ok(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(c: &Challenge) -> Answer {
        Answer {
            token: c.token.clone(),
            nonce: solve(&c.salt, c.bits).to_string(),
        }
    }

    #[test]
    fn an_answer_places_one_call_and_only_before_it_expires() {
        let ch = Challenges::default();
        let c = ch.issue(8, 1_000, 60_000);
        let a = answer(&c);
        assert_eq!(ch.redeem(Some(&a), 2_000), Ok(8));
        assert_eq!(ch.redeem(Some(&a), 2_001), Err(Refused::Spent));
        let late = ch.issue(8, 1_000, 60_000);
        assert_eq!(
            ch.redeem(Some(&answer(&late)), 61_000),
            Err(Refused::Expired)
        );
        assert_eq!(ch.redeem(None, 2_000), Err(Refused::Missing));
    }

    #[test]
    fn a_changed_token_or_a_wrong_nonce_is_refused() {
        let ch = Challenges::default();
        let c = ch.issue(12, 0, 60_000);
        let mut a = answer(&c);
        // Easier bits, same signature: refused.
        a.token = a.token.replacen(".12.", ".1.", 1);
        assert_eq!(ch.redeem(Some(&a), 1), Err(Refused::Forged));
        // Another station's key: refused.
        let other = Challenges::default().issue(12, 0, 60_000);
        assert_eq!(ch.redeem(Some(&answer(&other)), 1), Err(Refused::Forged));
        // A nonce that does not meet the bits, or is not digits.
        let mut a = answer(&c);
        let good: u64 = a.nonce.parse().unwrap();
        a.nonce = (0..good)
            .find(|n| !meets(&c.salt, &n.to_string(), 12))
            .unwrap()
            .to_string();
        assert_eq!(ch.redeem(Some(&a), 1), Err(Refused::Wrong));
        a.nonce = format!(" {good}");
        assert_eq!(ch.redeem(Some(&a), 1), Err(Refused::Wrong));
        a.nonce = good.to_string();
        assert_eq!(ch.redeem(Some(&a), 1), Ok(12));
    }

    #[test]
    fn bits_are_capped() {
        assert_eq!(Challenges::default().issue(40, 0, 1).bits, MAX_BITS);
    }

    /// The page's solver (apps/web/src/lib/pow.ts) finds the same first nonce: the two
    /// SHA-256s and the leading-bit count agree.
    #[test]
    fn the_shared_vector_matches_the_page() {
        assert_eq!(solve("0123456789abcdef0123456789abcdef", 12), 6092);
        assert_eq!(leading_zero_bits(&[0, 0, 0b0001_0000, 0xff]), 19);
    }
}
