// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The key that keeps a chatter's email address unreadable in the database (ADR-0125).
//!
//! An address is stored as an HMAC-SHA-256 of its lower-case form under a secret the station
//! holds outside the database ([`EmailKey::ENV`]). A copy of the database alone then cannot be
//! tested against a list of addresses, which a plain SHA-256 allows. The sign-in code's hash is
//! keyed the same way, under its own label, so one never stands in for the other.
//!
//! The key is not rotated by this service: a different key finds none of the accounts made
//! under the old one (the docs say so, and say to keep the key with the database's backups).

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The label each hash is made under, so a hash of one kind is never valid as another.
const EMAIL_LABEL: &[u8] = b"weavesignal-chat/email/v2";
const CODE_LABEL: &[u8] = b"weavesignal-chat/signin-code/v2";

#[derive(Clone, PartialEq, Eq)]
pub struct EmailKey(Vec<u8>);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error(
        "{} is {} characters; it must be at least {} (for example `openssl rand -hex 32`)",
        EmailKey::ENV,
        .0,
        EmailKey::MIN_LEN
    )]
    TooShort(usize),
}

impl EmailKey {
    /// Where the key comes from: the environment, never the settings file.
    pub const ENV: &'static str = "WEAVESIGNAL_CHAT_EMAIL_KEY";
    /// The shortest key accepted, in characters (as for the LiveKit secret).
    pub const MIN_LEN: usize = 32;

    /// A key from its text. Surrounding whitespace (a trailing newline from a file) is not part
    /// of the key.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let text = text.trim();
        let n = text.chars().count();
        if n < Self::MIN_LEN {
            return Err(KeyError::TooShort(n));
        }
        Ok(Self(text.as_bytes().to_vec()))
    }

    fn mac(&self, label: &[u8]) -> HmacSha256 {
        let mut m = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        m.update(label);
        m.update(&[0]);
        m
    }

    fn email_mac(&self, email: &str) -> HmacSha256 {
        let mut m = self.mac(EMAIL_LABEL);
        m.update(normal(email).as_bytes());
        m
    }

    fn code_mac(&self, email: &str, code: &str) -> HmacSha256 {
        let mut m = self.mac(CODE_LABEL);
        m.update(normal(email).as_bytes());
        m.update(&[0]);
        m.update(code.trim().as_bytes());
        m
    }

    /// The stored form of an address: 32 bytes, the same for every spelling of its case.
    pub fn email_hash(&self, email: &str) -> Vec<u8> {
        self.email_mac(email).finalize().into_bytes().to_vec()
    }

    /// The stored form of a sign-in code sent to an address.
    pub fn code_hash(&self, email: &str, code: &str) -> Vec<u8> {
        self.code_mac(email, code).finalize().into_bytes().to_vec()
    }

    /// Whether `want` is the hash of this code for this address, compared in constant time.
    pub fn code_matches(&self, email: &str, code: &str, want: &[u8]) -> bool {
        self.code_mac(email, code).verify_slice(want).is_ok()
    }
}

/// An address as it is compared: trimmed and lower-case.
fn normal(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The unkeyed hash this service stored before ADR-0125. It exists only to find an account made
/// then, so that its owner's next sign-in can re-key it; nothing new is ever stored under it.
pub fn legacy_email_hash(email: &str) -> Vec<u8> {
    Sha256::digest(normal(email).as_bytes()).to_vec()
}

impl fmt::Debug for EmailKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EmailKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210";

    fn key(s: &str) -> EmailKey {
        EmailKey::parse(s).unwrap()
    }

    #[test]
    fn a_short_key_is_refused_and_a_newline_is_not_part_of_one() {
        assert_eq!(EmailKey::parse("short"), Err(KeyError::TooShort(5)));
        assert_eq!(EmailKey::parse(""), Err(KeyError::TooShort(0)));
        assert_eq!(EmailKey::parse(&" ".repeat(40)), Err(KeyError::TooShort(0)));
        assert_eq!(key(A), key(&format!("{A}\n")));
        assert!(
            EmailKey::parse("short")
                .unwrap_err()
                .to_string()
                .contains("WEAVESIGNAL_CHAT_EMAIL_KEY")
        );
    }

    #[test]
    fn the_hash_is_keyed_and_ignores_case_and_spaces() {
        let k = key(A);
        let h = k.email_hash("Elspeth.Gordon@mail.example");
        assert_eq!(h.len(), 32);
        assert_eq!(h, k.email_hash("  elspeth.gordon@MAIL.example "));
        assert_ne!(h, k.email_hash("someone.else@mail.example"));
        // Another key, another hash; and neither is the plain SHA-256 a database copy could test.
        assert_ne!(h, key(B).email_hash("elspeth.gordon@mail.example"));
        assert_ne!(h, legacy_email_hash("elspeth.gordon@mail.example"));
    }

    #[test]
    fn a_code_matches_only_its_own_address_key_and_digits() {
        let k = key(A);
        let want = k.code_hash("Rory@mail.example", "123456");
        assert!(k.code_matches("rory@mail.example", " 123456 ", &want));
        assert!(!k.code_matches("rory@mail.example", "123457", &want));
        assert!(!k.code_matches("morag@mail.example", "123456", &want));
        assert!(!key(B).code_matches("rory@mail.example", "123456", &want));
        // The wrong length is a mismatch, not a panic.
        assert!(!k.code_matches("rory@mail.example", "123456", &want[..31]));
        assert!(!k.code_matches("rory@mail.example", "123456", &[]));
    }

    #[test]
    fn the_labels_keep_an_email_hash_from_being_a_code_hash() {
        let k = key(A);
        // Even where the inputs line up byte for byte, the label differs.
        assert_ne!(k.email_hash("a@b.example"), k.code_hash("a@b.example", ""));
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let shown = format!("{:?}", key(A));
        assert!(!shown.contains("0123456789"), "{shown}");
    }

    #[test]
    fn the_legacy_hash_is_the_plain_sha256_of_the_lower_case_address() {
        let want: Vec<u8> = Sha256::digest(b"rory@mail.example").to_vec();
        assert_eq!(legacy_email_hash(" Rory@Mail.example "), want);
    }
}
