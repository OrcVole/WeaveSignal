// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The gate a phone caller passes before the queue (Phase 17f, design section 2): pure state,
//! no I/O. The control plane feeds it what the desk reports (a call appeared, a key was pressed,
//! the words finished) and the clock, and carries out the [`Action`]s it returns.
//!
//! ```text
//! appeared ─┬─ call-in closed, banned, lines full, too many calls, words not ready ─▶ refused
//!           ├─ words off ─────────────────────────────────────────────────────────▶ admitted (not asked)
//!           └─ words ─┬─ any key, to today's words ───────────────────────────────▶ admitted (agreed)
//!                     ├─ any key, to older words (changed mid-call) ─▶ today's words again
//!                     ├─ the words end ─▶ again, until `plays` ─▶ no key: hung up
//!                     └─ `timeout` from the first word ──────────────────────────▶ timed out: hung up
//! ```
//!
//! **Keys are RFC 4733 events.** LiveKit's SIP service decodes the trunk's `telephone-event`
//! payloads (it has no in-band detector; `media-sdk/dtmf/dtmf.go`, read 2026-11-20) and sends
//! each key to the room as a `SipDTMF` data packet with its event `code` and `digit`. The desk
//! numbers each key it sees ([`PhoneKey::seq`]) and reports it in its status, which the control
//! plane polls, so the same key can arrive twice: the gate acts on a sequence number once.
//!
//! **Consent is to the words that were played.** A key carries the version of the words the desk
//! was playing; only today's version is recorded as agreed. Nothing is recorded for a call that
//! never got through: the rate counters are all that remains of it.

use super::words::PhoneWords;

/// One key press as the desk reports it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct PhoneKey {
    /// The desk's own count of keys, rising for the life of the desk.
    pub seq: u64,
    /// The phone caller's person key (`phone-<random>`).
    pub person: String,
    /// The RFC 4733 event code (0 to 15), as `SipDTMF.code`. Absent is 0: protobuf leaves a
    /// zero out, so the `0` key arrives with no code at all (the spike, 2026-11-20).
    #[serde(default)]
    pub code: u32,
    /// The version of the words that was playing when the key came, if any.
    #[serde(default)]
    pub words_version: Option<String>,
}

/// The key an RFC 4733 event code stands for (RFC 4733 section 3.2, table 3): `0` to `9`, `*`,
/// `#`, `A` to `D`. Other codes are tones, not keys.
pub fn rfc4733_key(code: u32) -> Option<char> {
    match code {
        0..=9 => char::from_digit(code, 10),
        10 => Some('*'),
        11 => Some('#'),
        12..=15 => Some((b'A' + (code - 12) as u8) as char),
        _ => None,
    }
}

/// Why a call is refused before anything is played. The desk never subscribes and the control
/// plane removes the caller: they hear ringing, then the end (question 9's default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhoneRefusal {
    CallInClosed,
    Banned,
    LinesFull,
    TooManyCalls,
    WordsNotReady,
}

/// How a call that never reached the queue ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallEnded {
    Refused(PhoneRefusal),
    /// The words played `plays` times without a key.
    NoKey,
    /// No key within the timeout.
    TimedOut,
    /// The caller hung up first.
    HungUp,
}

/// Who agreed: the caller with a key, or nobody, because the station turned the words off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhoneAgreed {
    Caller,
    NotAsked,
}

/// What the control plane does next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Remove the caller without answering.
    Refuse(PhoneRefusal),
    /// Tell the desk to play these words into the caller's return (`phone_words`), at this
    /// gain in millibels.
    PlayWords {
        file: String,
        version: String,
        gain_mb: i32,
    },
    /// Record the consent, let the caller's sound in (`phone_admit`) and put them in the queue.
    Admit {
        agreed: PhoneAgreed,
        version: String,
        text: String,
        at_ms: u64,
    },
    /// End the call (`phone_hang_up`).
    HangUp(CallEnded),
}

/// Where a call stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Words {
        words: PhoneWords,
        started_ms: u64,
        plays: u8,
    },
    Admitted,
    CallEnded(CallEnded),
}

/// What decides a call at its start.
#[derive(Clone, Debug)]
pub struct Facts {
    pub call_in_open: bool,
    pub banned: bool,
    pub lines_free: bool,
    pub within_limit: bool,
    /// The words that play today, and whether they are ready.
    pub words: PhoneWords,
    pub words_ready: bool,
    pub timeout_ms: u64,
    pub plays: u8,
}

/// One call's gate.
#[derive(Clone, Debug)]
pub struct Gate {
    step: Step,
    last_seq: Option<u64>,
    timeout_ms: u64,
    max_plays: u8,
}

impl Gate {
    /// A call appeared at the desk.
    pub fn appear(f: Facts, now: u64) -> (Self, Vec<Action>) {
        let mut g = Gate {
            step: Step::Admitted,
            last_seq: None,
            timeout_ms: f.timeout_ms,
            max_plays: f.plays.max(1),
        };
        let refusal = if !f.call_in_open {
            Some(PhoneRefusal::CallInClosed)
        } else if f.banned {
            Some(PhoneRefusal::Banned)
        } else if !f.lines_free {
            Some(PhoneRefusal::LinesFull)
        } else if !f.within_limit {
            Some(PhoneRefusal::TooManyCalls)
        } else if !f.words_ready {
            Some(PhoneRefusal::WordsNotReady)
        } else {
            None
        };
        if let Some(r) = refusal {
            g.step = Step::CallEnded(CallEnded::Refused(r));
            return (g, vec![Action::Refuse(r)]);
        }
        let actions = match f.words.file() {
            // Words that should play but have no file (`own` with nothing recorded) never let a
            // caller in unasked; the line is closed for them first, and this is the backstop.
            None if f.words != PhoneWords::Off => {
                let r = PhoneRefusal::WordsNotReady;
                g.step = Step::CallEnded(CallEnded::Refused(r));
                vec![Action::Refuse(r)]
            }
            None => vec![Action::Admit {
                agreed: PhoneAgreed::NotAsked,
                version: f.words.version(),
                text: f.words.text().to_owned(),
                at_ms: now,
            }],
            Some(file) => {
                let a = vec![Action::PlayWords {
                    file: file.to_owned(),
                    version: f.words.version(),
                    gain_mb: f.words.gain_mb(),
                }];
                g.step = Step::Words {
                    words: f.words,
                    started_ms: now,
                    plays: 1,
                };
                a
            }
        };
        (g, actions)
    }

    pub fn step(&self) -> &Step {
        &self.step
    }

    /// A key, with today's words (which may have changed since the call began).
    pub fn key(&mut self, press: &PhoneKey, today: &PhoneWords, now: u64) -> Vec<Action> {
        if self.last_seq.is_some_and(|s| press.seq <= s) {
            return Vec::new();
        }
        self.last_seq = Some(press.seq);
        if rfc4733_key(press.code).is_none() {
            return Vec::new();
        }
        let Step::Words { plays, .. } = &self.step else {
            return Vec::new();
        };
        let today_version = today.version();
        if press.words_version.as_deref() == Some(today_version.as_str()) {
            self.step = Step::Admitted;
            return vec![Action::Admit {
                agreed: PhoneAgreed::Caller,
                version: today_version,
                text: today.text().to_owned(),
                at_ms: now,
            }];
        }
        // Older words (or none heard yet): play today's, and count it.
        let Some(file) = today.file() else {
            if *today != PhoneWords::Off {
                return self.end(CallEnded::Refused(PhoneRefusal::WordsNotReady));
            }
            // The words were turned off mid-call: let them in as not asked.
            self.step = Step::Admitted;
            return vec![Action::Admit {
                agreed: PhoneAgreed::NotAsked,
                version: today_version,
                text: today.text().to_owned(),
                at_ms: now,
            }];
        };
        let plays = *plays;
        if plays >= self.max_plays {
            return self.end(CallEnded::NoKey);
        }
        let a = vec![Action::PlayWords {
            file: file.to_owned(),
            version: today_version,
            gain_mb: today.gain_mb(),
        }];
        if let Step::Words {
            words, plays: p, ..
        } = &mut self.step
        {
            *words = today.clone();
            *p += 1;
        }
        a
    }

    /// The words finished playing without a key: again, or the end.
    pub fn words_ended(&mut self, now: u64) -> Vec<Action> {
        if let Some(a) = self.timed_out(now) {
            return a;
        }
        let Step::Words { words, plays, .. } = &mut self.step else {
            return Vec::new();
        };
        if *plays >= self.max_plays {
            return self.end(CallEnded::NoKey);
        }
        *plays += 1;
        match words.file() {
            Some(file) => vec![Action::PlayWords {
                file: file.to_owned(),
                version: words.version(),
                gain_mb: words.gain_mb(),
            }],
            None => Vec::new(),
        }
    }

    /// The clock: the timeout from the first word.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        self.timed_out(now).unwrap_or_default()
    }

    /// The caller hung up.
    pub fn hung_up(&mut self) {
        if matches!(self.step, Step::Words { .. }) {
            self.step = Step::CallEnded(CallEnded::HungUp);
        }
    }

    fn timed_out(&mut self, now: u64) -> Option<Vec<Action>> {
        match self.step {
            Step::Words { started_ms, .. } if now.saturating_sub(started_ms) >= self.timeout_ms => {
                Some(self.end(CallEnded::TimedOut))
            }
            _ => None,
        }
    }

    fn end(&mut self, e: CallEnded) -> Vec<Action> {
        self.step = Step::CallEnded(e);
        vec![Action::HangUp(e)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::words::{ALBA_FILE, StationRecording, station_words_today};

    fn facts(words: PhoneWords) -> Facts {
        Facts {
            call_in_open: true,
            banned: false,
            lines_free: true,
            within_limit: true,
            words,
            words_ready: true,
            timeout_ms: 90_000,
            plays: 2,
        }
    }

    fn press(seq: u64, code: u32, w: &PhoneWords) -> PhoneKey {
        PhoneKey {
            seq,
            person: "phone-Xk3pQ9mZr2Ta".into(),
            code,
            words_version: Some(w.version()),
        }
    }

    #[test]
    fn the_zero_key_arrives_without_a_code() {
        // Protobuf leaves a zero out: the desk may pass the packet's fields on as they came.
        let k: PhoneKey =
            serde_json::from_str(r#"{"seq": 3, "person": "phone-A", "words_version": null}"#)
                .unwrap();
        assert_eq!(rfc4733_key(k.code), Some('0'));
    }

    #[test]
    fn rfc4733_codes_are_keys() {
        assert_eq!(rfc4733_key(0), Some('0'));
        assert_eq!(rfc4733_key(1), Some('1'));
        assert_eq!(rfc4733_key(9), Some('9'));
        assert_eq!(rfc4733_key(10), Some('*'));
        assert_eq!(rfc4733_key(11), Some('#'));
        assert_eq!(rfc4733_key(12), Some('A'));
        assert_eq!(rfc4733_key(15), Some('D'));
        assert_eq!(rfc4733_key(16), None);
        assert_eq!(rfc4733_key(32), None);
    }

    #[test]
    fn refusals_come_before_anything_is_played_in_order() {
        type Set = fn(&mut Facts);
        let cases: [(Set, PhoneRefusal); 5] = [
            (|f| f.call_in_open = false, PhoneRefusal::CallInClosed),
            (|f| f.banned = true, PhoneRefusal::Banned),
            (|f| f.lines_free = false, PhoneRefusal::LinesFull),
            (|f| f.within_limit = false, PhoneRefusal::TooManyCalls),
            (|f| f.words_ready = false, PhoneRefusal::WordsNotReady),
        ];
        for (set, want) in cases {
            let mut f = facts(PhoneWords::Default(crate::phone::words::PhoneVoice::Alba));
            set(&mut f);
            let (g, a) = Gate::appear(f, 0);
            assert_eq!(a, vec![Action::Refuse(want)]);
            assert_eq!(g.step(), &Step::CallEnded(CallEnded::Refused(want)));
        }
        // Closed and banned: closed is said, the ban is not looked at.
        let mut f = facts(PhoneWords::Default(crate::phone::words::PhoneVoice::Alba));
        f.call_in_open = false;
        f.banned = true;
        assert_eq!(
            Gate::appear(f, 0).1,
            vec![Action::Refuse(PhoneRefusal::CallInClosed)]
        );
    }

    #[test]
    fn any_key_to_todays_words_admits_once() {
        let w = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba);
        let (mut g, a) = Gate::appear(facts(w.clone()), 1_000);
        assert_eq!(
            a,
            vec![Action::PlayWords {
                file: ALBA_FILE.into(),
                version: w.version(),
                gain_mb: 0
            }]
        );
        // `#` is a key like any other.
        let a = g.key(&press(1, 11, &w), &w, 5_000);
        assert_eq!(
            a,
            vec![Action::Admit {
                agreed: PhoneAgreed::Caller,
                version: w.version(),
                text: w.text().into(),
                at_ms: 5_000
            }]
        );
        assert_eq!(g.step(), &Step::Admitted);
        // The same key reported again, and later keys, do nothing.
        assert!(g.key(&press(1, 11, &w), &w, 5_100).is_empty());
        assert!(g.key(&press(2, 1, &w), &w, 5_200).is_empty());
    }

    #[test]
    fn a_repeated_report_is_acted_on_once_and_tones_are_not_keys() {
        let w = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba);
        let (mut g, _) = Gate::appear(facts(w.clone()), 0);
        assert!(g.key(&press(4, 20, &w), &w, 10).is_empty());
        // Seq 4 was seen (a tone): the same report again is ignored, and an older one too.
        assert!(g.key(&press(4, 1, &w), &w, 20).is_empty());
        assert!(g.key(&press(3, 1, &w), &w, 30).is_empty());
        assert_eq!(g.key(&press(5, 1, &w), &w, 40).len(), 1);
    }

    #[test]
    fn no_consent_without_todays_version() {
        let today = "Live on Kettle FM.";
        let old = PhoneWords::Station(StationRecording {
            text: station_words_today("Live on Old FM."),
            file: "old.flac".into(),
            gain_mb: 0,
        });
        let new = PhoneWords::Station(StationRecording {
            text: station_words_today(today),
            file: "new.flac".into(),
            gain_mb: 0,
        });
        let (mut g, _) = Gate::appear(facts(old.clone()), 0);
        // A key to the old words (changed mid-call) plays today's instead.
        let a = g.key(&press(1, 1, &old), &new, 2_000);
        assert_eq!(
            a,
            vec![Action::PlayWords {
                file: "new.flac".into(),
                version: new.version(),
                gain_mb: 0
            }]
        );
        // A key with no version heard is not consent either; it counts as the last play.
        let mut none = press(2, 1, &new);
        none.words_version = None;
        assert_eq!(
            g.key(&none, &new, 3_000),
            vec![Action::HangUp(CallEnded::NoKey)]
        );
        // Fresh call: a key to today's words is consent.
        let (mut g, _) = Gate::appear(facts(new.clone()), 0);
        assert!(matches!(
            g.key(&press(1, 1, &new), &new, 1_000)[..],
            [Action::Admit {
                agreed: PhoneAgreed::Caller,
                ..
            }]
        ));
    }

    #[test]
    fn two_plays_without_a_key_hang_up() {
        let w = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba);
        let (mut g, _) = Gate::appear(facts(w.clone()), 0);
        assert_eq!(
            g.words_ended(10_000),
            vec![Action::PlayWords {
                file: ALBA_FILE.into(),
                version: w.version(),
                gain_mb: 0
            }]
        );
        assert_eq!(
            g.words_ended(20_000),
            vec![Action::HangUp(CallEnded::NoKey)]
        );
        assert_eq!(g.step(), &Step::CallEnded(CallEnded::NoKey));
        assert!(g.key(&press(1, 1, &w), &w, 21_000).is_empty());
    }

    #[test]
    fn the_timeout_runs_from_the_first_word() {
        let w = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba);
        let mut f = facts(w.clone());
        f.plays = 5;
        let (mut g, _) = Gate::appear(f, 1_000);
        assert!(g.tick(90_999).is_empty());
        assert_eq!(g.tick(91_000), vec![Action::HangUp(CallEnded::TimedOut)]);
        assert!(g.tick(200_000).is_empty());
        // A play ending after the timeout ends the call too.
        let (mut g, _) = Gate::appear(facts(w), 0);
        assert_eq!(
            g.words_ended(95_000),
            vec![Action::HangUp(CallEnded::TimedOut)]
        );
    }

    #[test]
    fn words_off_admit_at_once_as_not_asked() {
        let (g, a) = Gate::appear(facts(PhoneWords::Off), 7);
        assert_eq!(g.step(), &Step::Admitted);
        match &a[..] {
            [
                Action::Admit {
                    agreed: PhoneAgreed::NotAsked,
                    at_ms: 7,
                    text,
                    ..
                },
            ] => assert!(text.contains("No words were played")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_hang_up_during_the_words_ends_the_gate() {
        let w = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba);
        let (mut g, _) = Gate::appear(facts(w.clone()), 0);
        g.hung_up();
        assert_eq!(g.step(), &Step::CallEnded(CallEnded::HungUp));
        assert!(g.key(&press(1, 1, &w), &w, 10).is_empty());
        assert!(g.tick(1_000_000).is_empty());
    }
}
