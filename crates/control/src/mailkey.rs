// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The key that keeps a staff member's email address unreadable in the database (ADR-0127), as
//! the chat's key does for a chatter's (ADR-0125, `crates/chat/src/key.rs`).
//!
//! An address is stored as an HMAC-SHA-256 of its lower-case form under a secret the station
//! holds outside the database ([`MailKey::ENV`]). A copy of the database alone cannot be tested
//! against a list of addresses. A sign-in code's hash is keyed the same way and names the
//! account and the address it was made for. The labels below differ from the chat's, so a hash
//! made by one service is never valid in the other even under one shared secret.
//!
//! The key is not rotated by the control plane: a different key finds none of the addresses
//! set under the old one (an admin sets them again). Keep it with the database's backups.

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// The label each hash is made under, so a hash of one kind is never valid as another.
const EMAIL_LABEL: &[u8] = b"weavesignal-control/staff-email/v1";
const CODE_LABEL: &[u8] = b"weavesignal-control/staff-code/v1";

#[derive(Clone, PartialEq, Eq)]
pub struct MailKey(Vec<u8>);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error(
        "{} is {} characters; it must be at least {} (for example `openssl rand -hex 32`)",
        MailKey::ENV,
        .0,
        MailKey::MIN_LEN
    )]
    TooShort(usize),
}

impl MailKey {
    /// Where the key comes from: the environment, never the settings file.
    pub const ENV: &'static str = "WEAVESIGNAL_CONTROL_EMAIL_KEY";
    /// The shortest key accepted, in characters (as for the LiveKit secret and the chat's key).
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

    fn code_mac(&self, account: Uuid, email: &str, code: &str) -> HmacSha256 {
        let mut m = self.mac(CODE_LABEL);
        m.update(account.as_bytes());
        m.update(&[0]);
        m.update(normal(email).as_bytes());
        m.update(&[0]);
        m.update(code.trim().as_bytes());
        m
    }

    /// The stored form of an address: 32 bytes, the same for every spelling of its case.
    pub fn email_hash(&self, email: &str) -> Vec<u8> {
        let mut m = self.mac(EMAIL_LABEL);
        m.update(normal(email).as_bytes());
        m.finalize().into_bytes().to_vec()
    }

    /// The stored form of a sign-in code sent to an address for one account.
    pub fn code_hash(&self, account: Uuid, email: &str, code: &str) -> Vec<u8> {
        self.code_mac(account, email, code)
            .finalize()
            .into_bytes()
            .to_vec()
    }

    /// Whether `want` is the hash of this code for this account and address, compared in
    /// constant time.
    pub fn code_matches(&self, account: Uuid, email: &str, code: &str, want: &[u8]) -> bool {
        self.code_mac(account, email, code)
            .verify_slice(want)
            .is_ok()
    }
}

/// An address as it is compared: trimmed and lower-case.
fn normal(email: &str) -> String {
    email.trim().to_lowercase()
}

impl fmt::Debug for MailKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210";

    fn key(s: &str) -> MailKey {
        MailKey::parse(s).unwrap()
    }

    #[test]
    fn a_short_key_is_refused_and_a_newline_is_not_part_of_one() {
        assert_eq!(MailKey::parse("short"), Err(KeyError::TooShort(5)));
        assert_eq!(MailKey::parse(&" ".repeat(40)), Err(KeyError::TooShort(0)));
        assert_eq!(key(A), key(&format!("{A}\n")));
        assert!(
            MailKey::parse("short")
                .unwrap_err()
                .to_string()
                .contains("WEAVESIGNAL_CONTROL_EMAIL_KEY")
        );
    }

    #[test]
    fn the_hash_is_keyed_and_ignores_case_and_spaces() {
        let k = key(A);
        let h = k.email_hash("Elspeth.Gordon@mail.example");
        assert_eq!(h.len(), 32);
        assert_eq!(h, k.email_hash("  elspeth.gordon@MAIL.example "));
        assert_ne!(h, k.email_hash("someone.else@mail.example"));
        assert_ne!(h, key(B).email_hash("elspeth.gordon@mail.example"));
    }

    #[test]
    fn a_code_matches_only_its_own_account_address_key_and_digits() {
        let k = key(A);
        let (rory, morag) = (Uuid::new_v4(), Uuid::new_v4());
        let want = k.code_hash(rory, "Rory@mail.example", "123456");
        assert_eq!(want.len(), 32);
        assert!(k.code_matches(rory, "rory@mail.example", " 123456 ", &want));
        assert!(!k.code_matches(rory, "rory@mail.example", "123457", &want));
        assert!(!k.code_matches(rory, "morag@mail.example", "123456", &want));
        assert!(!k.code_matches(morag, "rory@mail.example", "123456", &want));
        assert!(!key(B).code_matches(rory, "rory@mail.example", "123456", &want));
        // The wrong length is a mismatch, not a panic.
        assert!(!k.code_matches(rory, "rory@mail.example", "123456", &want[..31]));
        assert!(!k.code_matches(rory, "rory@mail.example", "123456", &[]));
    }

    #[test]
    fn the_labels_keep_an_email_hash_from_being_a_code_hash() {
        let k = key(A);
        assert_ne!(
            k.email_hash("a@b.example"),
            k.code_hash(Uuid::nil(), "a@b.example", "")
        );
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let shown = format!("{:?}", key(A));
        assert!(!shown.contains("0123456789"), "{shown}");
    }
}
