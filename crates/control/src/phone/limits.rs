// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Bans and limits for phone callers (Phase 17f, design sections 3 and 4).
//!
//! **A ban is by ban key**, not by identity: a phone caller's person key is new on every call,
//! while the ban key (an HMAC of LiveKit's `sip_` identity, `weavesignal_model::caller_id`) is
//! the same each time the same number rings. A ban keeps the ban key, the last four digits, the
//! date and the reason, and expires with the call-in retention preset unless renewed. **A
//! withheld caller cannot be banned by number**: every withheld caller shares one identity, so
//! such a ban would refuse them all. Caller ID can be spoofed: a ban is advisory.
//!
//! **Limits:** so many calls per ban key an hour (withheld callers share one count), the
//! station's per-minute limit shared with browser call-in (`join::Limiter`, counted with no
//! address), and so many lines at once.

use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;

const HOUR_MS: u64 = 3_600_000;
/// The count withheld callers share.
const WITHHELD_COUNT: &str = "withheld";

/// What the screener's ban button says for a withheld caller.
pub const WITHHELD_UNBANNABLE: &str = "Cannot ban a withheld number";

/// A number's ban.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct PhoneBan {
    /// `pb_` and 32 hex digits; never the number.
    pub ban_key: String,
    /// The last digits as the screener saw them (at most four).
    pub tail: String,
    /// The day it was made (`2026-11-20`).
    pub date: String,
    pub reason: String,
    /// When it lapses (milliseconds since the epoch); none, never.
    pub expires_ms: Option<u64>,
}

/// Why a ban cannot be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BanError {
    #[error("{WITHHELD_UNBANNABLE}")]
    Withheld,
    #[error("not a ban key")]
    NotABanKey,
}

impl PhoneBan {
    /// A ban as the database keeps it, for the screener (its day in UTC).
    pub fn from_record(r: weavesignal_model::phone::PhoneBanRecord) -> Self {
        let date = jiff::Timestamp::from_millisecond(r.made_at_ms)
            .map(|t| t.strftime("%Y-%m-%d").to_string())
            .unwrap_or_default();
        PhoneBan {
            ban_key: r.ban_key,
            tail: r.number_tail,
            date,
            reason: r.reason,
            expires_ms: r.expires_at_ms.and_then(|e| u64::try_from(e).ok()),
        }
    }
}

/// The station's number bans, in memory (the database keeps them; migration 0046).
#[derive(Clone, Debug, Default)]
pub struct Bans(BTreeMap<String, PhoneBan>);

impl Bans {
    pub fn from_rows(rows: impl IntoIterator<Item = PhoneBan>) -> Self {
        Self(rows.into_iter().map(|b| (b.ban_key.clone(), b)).collect())
    }

    /// The ban on a caller, if any; a withheld caller (no ban key) never has one.
    pub fn of(&self, ban_key: Option<&str>, now: u64) -> Option<&PhoneBan> {
        self.0
            .get(ban_key?)
            .filter(|b| b.expires_ms.is_none_or(|e| now < e))
    }

    /// Ban a caller by their ban key; refused for a withheld caller.
    pub fn ban(&mut self, ban: PhoneBan, withheld: bool) -> Result<(), BanError> {
        if withheld {
            return Err(BanError::Withheld);
        }
        if !weavesignal_model::caller_id::is_ban_key(&ban.ban_key) {
            return Err(BanError::NotABanKey);
        }
        self.0.insert(ban.ban_key.clone(), ban);
        Ok(())
    }

    /// Lift a ban; whether there was one.
    pub fn lift(&mut self, ban_key: &str) -> bool {
        self.0.remove(ban_key).is_some()
    }

    /// Forget lapsed bans.
    pub fn prune(&mut self, now: u64) {
        self.0.retain(|_, b| b.expires_ms.is_none_or(|e| now < e));
    }

    pub fn list(&self) -> Vec<PhoneBan> {
        self.0.values().cloned().collect()
    }
}

/// Calls per ban key in the last hour.
#[derive(Clone, Debug, Default)]
pub struct NumberLimiter(BTreeMap<String, VecDeque<u64>>);

impl NumberLimiter {
    /// Whether one more call from this number at `now` is allowed; if it is, it is counted.
    /// `ban_key` is `None` for a withheld caller, who shares one count with every other.
    pub fn allow(&mut self, ban_key: Option<&str>, now: u64, per_hour: u32) -> bool {
        self.0.retain(|_, q| {
            q.retain(|&t| now.saturating_sub(t) < HOUR_MS);
            !q.is_empty()
        });
        let key = ban_key.unwrap_or(WITHHELD_COUNT);
        let q = self.0.entry(key.to_owned()).or_default();
        if q.len() >= per_hour as usize {
            return false;
        }
        q.push_back(now);
        true
    }

    /// How many numbers are counted (memory is bounded by an hour of calls).
    pub fn numbers(&self) -> usize {
        self.0.len()
    }
}

/// Whether another line is free.
pub fn lines_free(in_use: usize, lines: u16) -> bool {
    in_use < usize::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "pb_0123456789abcdef0123456789abcdef";
    const K2: &str = "pb_fedcba9876543210fedcba9876543210";

    fn ban(key: &str, expires_ms: Option<u64>) -> PhoneBan {
        PhoneBan {
            ban_key: key.into(),
            tail: "4471".into(),
            date: "2026-11-20".into(),
            reason: "abusive".into(),
            expires_ms,
        }
    }

    #[test]
    fn a_ban_is_by_ban_key_and_lapses() {
        let mut b = Bans::default();
        b.ban(ban(K, Some(1_000)), false).unwrap();
        assert_eq!(b.of(Some(K), 999).map(|b| b.tail.as_str()), Some("4471"));
        assert!(b.of(Some(K), 1_000).is_none());
        assert!(b.of(Some(K2), 0).is_none());
        assert!(b.of(None, 0).is_none());
        b.prune(1_000);
        assert!(b.list().is_empty());
        b.ban(ban(K2, None), false).unwrap();
        assert!(b.of(Some(K2), u64::MAX).is_some());
        assert!(b.lift(K2));
        assert!(!b.lift(K2));
    }

    #[test]
    fn a_withheld_caller_cannot_be_banned() {
        let mut b = Bans::default();
        assert_eq!(b.ban(ban(K, None), true), Err(BanError::Withheld));
        assert_eq!(
            BanError::Withheld.to_string(),
            "Cannot ban a withheld number"
        );
        assert_eq!(
            b.ban(ban("sip_0123456789abcdef", None), false),
            Err(BanError::NotABanKey)
        );
        assert!(b.list().is_empty());
    }

    #[test]
    fn a_stored_ban_reads_back_with_its_day() {
        let b = PhoneBan::from_record(weavesignal_model::phone::PhoneBanRecord {
            ban_key: K.into(),
            number_tail: "4471".into(),
            reason: "abusive".into(),
            made_by: "Morag".into(),
            made_at_ms: 1_763_600_000_000,
            expires_at_ms: Some(1_763_700_000_000),
        });
        assert_eq!(b.date, "2025-11-20");
        assert_eq!(b.expires_ms, Some(1_763_700_000_000));
    }

    #[test]
    fn calls_per_number_per_hour() {
        let mut l = NumberLimiter::default();
        for i in 0..6 {
            assert!(l.allow(Some(K), i, 6));
        }
        assert!(!l.allow(Some(K), 10, 6));
        assert!(l.allow(Some(K2), 10, 6));
        // Withheld callers share one count.
        assert!(l.allow(None, 10, 1));
        assert!(!l.allow(None, 11, 1));
        // An hour on, the window is clear and old numbers are forgotten.
        assert!(l.allow(Some(K), HOUR_MS + 20, 6));
        assert_eq!(l.numbers(), 1);
    }

    #[test]
    fn lines_at_once() {
        assert!(lines_free(7, 8));
        assert!(!lines_free(8, 8));
    }
}
