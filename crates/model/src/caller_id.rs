// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A phone caller's number, kept as little as it can be (Phase 17f, design section 3).
//!
//! **What the station keeps:** a **ban key** and **the last four digits**, never the number.
//! LiveKit's SIP service, with `hide_phone_number` on, already leaves the number out of the room:
//! the caller's identity is `sip_` and the first 16 hex digits of the SHA-256 of the INVITE's From
//! user, and the room and name carry its last four characters (`EvaluateDispatchRule` in
//! livekit/protocol's `sip/sip.go`, read from `main` on 2026-11-20; the spike checks it at
//! livekit-sip v1.25.0). That hash is unsalted and 64 bits over about 10^11 numbers, so it can be
//! enumerated in minutes (*inference*): it never reaches the database, a log or a consent record.
//! What the database holds is its HMAC under a station key ([`BanKeys`]), which is kept with the
//! station key and never in the database.
//!
//! **A number the station types** (a takedown, a ban made by hand) is turned into every form the
//! provider might have sent as the From user ([`number_forms`]: `+44…`, `44…`, `0044…`, `0…`),
//! each hashed as LiveKit does and then keyed, so the call is found whichever form came in. The
//! typed number lives only in that request.
//!
//! **Withheld numbers** (From `anonymous`, `restricted`, `unknown`, `unavailable`, `private`, or
//! empty) each give one identity for every such caller, so a ban cannot single one out
//! ([`is_withheld_identity`]). Caller ID can also be spoofed: a ban by number is advisory.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// What a ban key starts with, so one is never mistaken for an identity.
pub const BAN_KEY_PREFIX: &str = "pb_";
const BAN_LABEL: &[u8] = b"weavesignal/phone-ban-key/v1";

/// The From users a withheld call arrives with, as providers send them (lower case; each is also
/// tried capitalised and in capitals).
const WITHHELD: &[&str] = &[
    "",
    "anonymous",
    "restricted",
    "unknown",
    "unavailable",
    "private",
    "withheld",
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// LiveKit's identity for a caller whose From user is `from_user`, with `hide_phone_number` on:
/// `sip_` and the first 8 bytes of its SHA-256 in hex.
pub fn livekit_identity(from_user: &str) -> String {
    let h = Sha256::digest(from_user.as_bytes());
    format!("sip_{}", hex(&h[..8]))
}

/// The From user's tail as LiveKit keeps it in the room's name: the last four characters when
/// there are more than four, one when there are one to four, none when it is empty.
pub fn livekit_tail(from_user: &str) -> &str {
    let n = if from_user.len() <= 4 {
        from_user.len().min(1)
    } else {
        4
    };
    &from_user[from_user.len() - n..]
}

/// Whether LiveKit's identity is a withheld caller's.
pub fn is_withheld_identity(identity: &str) -> bool {
    WITHHELD.iter().any(|w| {
        let mut cap = w.to_string();
        if let Some(f) = cap.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        [w.to_string(), cap, w.to_ascii_uppercase()]
            .iter()
            .any(|form| livekit_identity(form) == identity)
    })
}

/// How the screener sees a caller's number: `···4471`, or `Withheld`. Only digits are shown; a
/// tail of letters from a From user we do not know as withheld is shown as `···` alone.
pub fn shown_number(tail: &str, withheld: bool) -> String {
    if withheld {
        return "Withheld".to_owned();
    }
    let digits: String = tail.chars().filter(char::is_ascii_digit).collect();
    format!("···{digits}")
}

/// Every form a provider might send a number in as the From user, from what the station typed.
/// `country` is the station's calling code without `+` (`44`), used to go between the national
/// form (`07700 900123`) and the international (`+447700900123`); empty, only the forms of what
/// was typed are made. Spaces, dashes, dots and brackets are dropped. Nothing else is kept.
pub fn number_forms(typed: &str, country: &str) -> Vec<String> {
    let t: String = typed
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')' | '\u{a0}'))
        .collect();
    let country: String = country.chars().filter(char::is_ascii_digit).collect();
    // The number with its country code and no prefix (`447700900123`), if it can be told.
    let international = if let Some(d) = t.strip_prefix('+') {
        Some(d.to_owned())
    } else if let Some(d) = t.strip_prefix("00") {
        Some(d.to_owned())
    } else if let Some(n) = t.strip_prefix('0').filter(|_| !country.is_empty()) {
        Some(format!("{country}{n}"))
    } else if !country.is_empty() && t.starts_with(&country) {
        Some(t.clone())
    } else {
        None
    };
    let mut forms = vec![t.clone()];
    if let Some(d) =
        international.filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    {
        forms.extend([format!("+{d}"), d.clone(), format!("00{d}")]);
        if let Some(n) = d
            .strip_prefix(country.as_str())
            .filter(|_| !country.is_empty())
        {
            forms.extend([format!("0{n}"), n.to_owned()]);
        }
    }
    forms.retain(|f| !f.is_empty());
    forms.sort();
    forms.dedup();
    forms
}

/// The station's key for ban keys; from the control plane's key (`apikey`), never the database.
#[derive(Clone)]
pub struct BanKeys([u8; 32]);

impl std::fmt::Debug for BanKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BanKeys(..)")
    }
}

impl BanKeys {
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    /// The ban key of LiveKit's identity for a caller: `pb_` and 32 hex digits (128 bits).
    pub fn of_identity(&self, identity: &str) -> String {
        let mut m = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        m.update(BAN_LABEL);
        m.update(&[0]);
        m.update(identity.as_bytes());
        format!("{BAN_KEY_PREFIX}{}", hex(&m.finalize().into_bytes()[..16]))
    }

    /// Every ban key a typed number may have come in as ([`number_forms`]).
    pub fn of_typed_number(&self, typed: &str, country: &str) -> Vec<String> {
        let mut keys: Vec<String> = number_forms(typed, country)
            .iter()
            .map(|f| self.of_identity(&livekit_identity(f)))
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

/// Whether text is a ban key's form.
pub fn is_ban_key(text: &str) -> bool {
    text.strip_prefix(BAN_KEY_PREFIX)
        .is_some_and(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn livekits_identity_is_sha256_of_the_from_user() {
        // `printf +447700900123 | sha256sum` = a8acc3a90a7b4e4dc65e93db9240ed26….
        let id = livekit_identity("+447700900123");
        assert_eq!(id, "sip_a8acc3a90a7b4e4d");
        assert!(id.starts_with("sip_"));
        assert_eq!(id.len(), 4 + 16);
        let full = hex(&Sha256::digest(b"+447700900123"));
        assert_eq!(&id[4..], &full[..16]);
        // A known vector: `printf anonymous | sha256sum` = 2f183a4e64493af3f377f745eda502363cd3e7ef6e4d266d444758de0a85fcc8.
        assert_eq!(livekit_identity("anonymous"), "sip_2f183a4e64493af3");
    }

    #[test]
    fn tails_as_livekit_cuts_them() {
        assert_eq!(livekit_tail("+447700900123"), "0123");
        assert_eq!(livekit_tail("12345"), "2345");
        assert_eq!(livekit_tail("1234"), "4");
        assert_eq!(livekit_tail("7"), "7");
        assert_eq!(livekit_tail(""), "");
        assert_eq!(livekit_tail("anonymous"), "mous");
    }

    #[test]
    fn withheld_callers_are_told_by_their_identity() {
        for w in [
            "anonymous",
            "Anonymous",
            "ANONYMOUS",
            "restricted",
            "unknown",
            "",
            "Private",
        ] {
            assert!(is_withheld_identity(&livekit_identity(w)), "{w}");
        }
        assert!(!is_withheld_identity(&livekit_identity("+447700900123")));
        assert!(!is_withheld_identity("phone-abc"));
    }

    #[test]
    fn the_screener_sees_four_digits_or_withheld() {
        assert_eq!(shown_number("0123", false), "···0123");
        assert_eq!(shown_number("mous", true), "Withheld");
        assert_eq!(shown_number("7", false), "···7");
        assert_eq!(shown_number("ab", false), "···");
    }

    #[test]
    fn a_typed_number_gives_every_form() {
        let f = number_forms("07700 900123", "44");
        for want in [
            "07700900123",
            "+447700900123",
            "447700900123",
            "00447700900123",
            "7700900123",
        ] {
            assert!(f.contains(&want.to_owned()), "{want} in {f:?}");
        }
        // The forms agree whichever form was typed.
        assert_eq!(number_forms("+447700900123", "44"), f);
        assert_eq!(number_forms("00447700900123", "44"), f);
        assert_eq!(number_forms("447700900123", "44"), f);
        // Without a country code, only what was typed and its international forms.
        assert_eq!(
            number_forms("+15550100", ""),
            vec!["+15550100", "0015550100", "15550100"]
        );
        assert_eq!(number_forms("5550100", ""), vec!["5550100"]);
    }

    #[test]
    fn ban_keys_are_keyed_and_found_from_a_typed_number() {
        let k = BanKeys::new([7; 32]);
        let other = BanKeys::new([8; 32]);
        let id = livekit_identity("+447700900123");
        let key = k.of_identity(&id);
        assert!(is_ban_key(&key));
        assert_ne!(key, other.of_identity(&id));
        assert_ne!(key, k.of_identity(&livekit_identity("+447700900124")));
        assert!(k.of_typed_number("07700 900123", "44").contains(&key));
        assert!(!k.of_typed_number("07700 900124", "44").contains(&key));
        // The number is in neither the key nor its debug form.
        assert!(!key.contains("7700900123"));
        assert_eq!(format!("{k:?}"), "BanKeys(..)");
    }
}
