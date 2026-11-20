// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line at run time (Phase 17f): one [`Gate`] per phone booth the desk reports, fed
//! from each status poll, and what the hub must do about it ([`Todo`]). Pure apart from its own
//! lock: the hub carries the todos out (the desk's requests, the studio, the consent record).
//!
//! Each poll: a booth seen for the first time makes its ban key (an HMAC of LiveKit's identity,
//! [`BanKeys`]) and opens a gate with the facts of the moment; keys numbered after the last one
//! acted on go to their gates; a booth whose words finished once more is told so; every gate
//! hears the clock; a booth gone from the desk before its caller got through is a hang-up.

use std::collections::BTreeMap;
use std::sync::Mutex;

use weavesignal_model::caller_id::{BanKeys, is_withheld_identity};

use super::config::PhoneConfig;
use super::gate::{Action, CallEnded, Facts, Gate, PhoneAgreed, PhoneRefusal, Step};
use super::limits::{Bans, NumberLimiter, lines_free};
use super::words::{PhoneVoice, PhoneWords, WordsProblem};
use crate::desk::DeskPhone;
use crate::studio::{CallIn, Consent, ConsentBy, PhoneCaller};

/// Why the line takes no calls now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
    /// `phone.enabled` is off, or no trunk is set.
    Off,
    /// No key to make ban keys with (neither the token key nor the station key is set).
    NoKey,
    /// The words cannot play (the station's recording is of older words, or the default
    /// recording is missing).
    Words(WordsProblem),
}

/// What the hub does next for one phone caller.
#[derive(Clone, Debug, PartialEq)]
pub enum Todo {
    /// `phone_words` at the desk.
    Words {
        person: String,
        file: String,
        version: String,
        gain_mb: i32,
    },
    /// Record the consent, put the caller in the studio's queue, `phone_admit` at the desk.
    Admit { call: CallIn, line: PhoneCaller },
    /// `phone_hang_up` at the desk: refused before an answer, or ended at the gate.
    HangUp { person: String, why: CallEnded },
}

/// What the station is like at this poll, beyond the desk's report.
pub struct Moment<'a> {
    /// Call-in is open (the same switch as the player page's).
    pub call_in_open: bool,
    /// The browser's consent words, filled, as today (for a station recording's version).
    pub consent_today: &'a str,
    /// The station's own recording: its words and file.
    pub station_recording: Option<super::words::StationRecording>,
    /// The shipped recordings are in the phone-words directory.
    pub default_present: bool,
    /// What an admin changed from the studio, over the installation's settings.
    pub settings: PhoneSettings,
    pub bans: &'a Bans,
    /// Phone callers already in the queue (they hold lines).
    pub in_queue: usize,
}

struct Call {
    gate: Gate,
    tail: String,
    withheld: bool,
    ban_key: Option<String>,
    words_done: u32,
}

#[derive(Default)]
struct State {
    calls: BTreeMap<String, Call>,
    last_seq: Option<u64>,
    limiter: NumberLimiter,
}

/// The phone line's settings an admin changed from the studio (`PUT /api/v1/phone/consent`,
/// kept in the database, migration 0047): each `None` until changed there, when the
/// installation's setting applies (`phone.consent_words`, `phone.default_voice`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhoneSettings {
    pub consent_words: Option<bool>,
    pub default_voice: Option<PhoneVoice>,
}

/// What the line knows of the station between polls, set at start and when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub call_in_open: bool,
    pub consent_today: String,
    pub station_recording: Option<super::words::StationRecording>,
    pub default_present: bool,
    pub settings: PhoneSettings,
}

/// The line.
pub struct Line {
    pub config: PhoneConfig,
    keys: Option<BanKeys>,
    state: Mutex<State>,
    context: Mutex<Context>,
}

impl Line {
    pub fn new(config: PhoneConfig, keys: Option<BanKeys>) -> Self {
        Self {
            config,
            keys,
            state: Mutex::new(State::default()),
            context: Mutex::new(Context::default()),
        }
    }

    pub fn context(&self) -> Context {
        self.context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_context(&self, c: Context) {
        *self.context.lock().unwrap_or_else(|p| p.into_inner()) = c;
    }

    /// A moment from the context, the studio's bans and its phone callers in the queue.
    pub fn moment<'a>(c: &'a Context, bans: &'a Bans, in_queue: usize) -> Moment<'a> {
        Moment {
            call_in_open: c.call_in_open,
            consent_today: &c.consent_today,
            station_recording: c.station_recording.clone(),
            default_present: c.default_present,
            settings: c.settings,
            bans,
            in_queue,
        }
    }

    /// Whether the consent words play: the studio's setting, else the installation's.
    pub fn consent_words(&self, s: PhoneSettings) -> bool {
        s.consent_words.unwrap_or(self.config.consent_words)
    }

    /// Which shipped recording plays until the station records its own: the studio's setting,
    /// else the installation's.
    pub fn default_voice(&self, s: PhoneSettings) -> PhoneVoice {
        s.default_voice.unwrap_or(self.config.default_voice)
    }

    /// The words that play now.
    pub fn words(&self, m: &Moment<'_>) -> PhoneWords {
        PhoneWords::choose(
            self.consent_words(m.settings),
            self.default_voice(m.settings),
            m.station_recording.clone(),
        )
    }

    /// Why the line is closed, if it is.
    pub fn closed(&self, m: &Moment<'_>) -> Option<Closed> {
        if !self.config.on() {
            return Some(Closed::Off);
        }
        if self.keys.is_none() {
            return Some(Closed::NoKey);
        }
        self.words(m)
            .problem(m.consent_today, m.default_present)
            .map(Closed::Words)
    }

    /// The ban key of a caller's LiveKit identity; `None` withheld or without a key.
    pub fn ban_key_of(&self, sip_identity: &str) -> Option<String> {
        if sip_identity.is_empty() || is_withheld_identity(sip_identity) {
            return None;
        }
        self.keys.as_ref().map(|k| k.of_identity(sip_identity))
    }

    /// The ban keys a typed number may have come in as (a takedown, a ban by hand).
    pub fn ban_keys_of_typed(&self, typed: &str) -> Vec<String> {
        self.keys.as_ref().map_or_else(Vec::new, |k| {
            k.of_typed_number(typed, &self.config.country_code)
        })
    }

    /// Calls at the gate now (they hold lines too).
    pub fn at_gate(&self) -> usize {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.calls
            .values()
            .filter(|c| matches!(c.gate.step(), Step::Words { .. }))
            .count()
    }

    /// One poll: the desk's phone status in, todos out.
    pub fn step(&self, phone: &DeskPhone, m: &Moment<'_>, now: u64) -> Vec<Todo> {
        let today = self.words(m);
        let closed = self.closed(m);
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut todo = Vec::new();
        let present: Vec<&str> = phone.booths.iter().map(|b| b.person.as_str()).collect();

        // Gone from the desk before getting through: hung up.
        st.calls.retain(|person, c| {
            let here = present.contains(&person.as_str());
            if !here {
                c.gate.hung_up();
            }
            here
        });

        // New booths: a gate each, with the facts of the moment.
        for b in &phone.booths {
            if st.calls.contains_key(&b.person) || b.admitted {
                continue;
            }
            let ban_key = self.ban_key_of(&b.sip_identity);
            let withheld = is_withheld_identity(&b.sip_identity);
            let busy = m.in_queue
                + st.calls
                    .values()
                    .filter(|c| matches!(c.gate.step(), Step::Words { .. }))
                    .count();
            let banned = m.bans.of(ban_key.as_deref(), now).is_some();
            let within_limit = closed.is_some()
                || banned
                || st.limiter.allow(
                    ban_key.as_deref(),
                    now,
                    self.config.calls_per_number_per_hour,
                );
            let (gate, actions) = Gate::appear(
                Facts {
                    call_in_open: m.call_in_open && closed != Some(Closed::Off),
                    banned,
                    lines_free: lines_free(busy, self.config.lines),
                    within_limit,
                    words: today.clone(),
                    words_ready: closed.is_none(),
                    timeout_ms: u64::from(self.config.words_timeout_s) * 1000,
                    plays: self.config.words_plays,
                },
                now,
            );
            let call = Call {
                gate,
                tail: b.tail.clone(),
                withheld,
                ban_key,
                words_done: b.words_done,
            };
            Self::todos(&b.person, &call, actions, &mut todo);
            st.calls.insert(b.person.clone(), call);
        }

        // Keys after the last one acted on.
        let last = st.last_seq;
        for k in phone.keys.iter().filter(|k| last.is_none_or(|l| k.seq > l)) {
            st.last_seq = Some(st.last_seq.map_or(k.seq, |l| l.max(k.seq)));
            if let Some(c) = st.calls.get_mut(&k.person) {
                let a = c.gate.key(k, &today, now);
                Self::todos(&k.person, c, a, &mut todo);
            }
        }

        // Words that finished again, and the clock.
        for b in &phone.booths {
            if let Some(c) = st.calls.get_mut(&b.person) {
                while c.words_done < b.words_done {
                    c.words_done += 1;
                    let a = c.gate.words_ended(now);
                    Self::todos(&b.person, c, a, &mut todo);
                }
            }
        }
        for (person, c) in st.calls.iter_mut() {
            let a = c.gate.tick(now);
            Self::todos(person, c, a, &mut todo);
        }
        todo
    }

    fn todos(person: &str, c: &Call, actions: Vec<Action>, out: &mut Vec<Todo>) {
        for a in actions {
            out.push(match a {
                Action::Refuse(r) => Todo::HangUp {
                    person: person.to_owned(),
                    why: CallEnded::Refused(r),
                },
                Action::HangUp(why) => Todo::HangUp {
                    person: person.to_owned(),
                    why,
                },
                Action::PlayWords {
                    file,
                    version,
                    gain_mb,
                } => Todo::Words {
                    person: person.to_owned(),
                    file,
                    version,
                    gain_mb,
                },
                Action::Admit {
                    agreed,
                    version,
                    text,
                    at_ms,
                } => Todo::Admit {
                    call: CallIn {
                        identity: person.to_owned(),
                        name: String::new(),
                        topic: String::new(),
                        consent: Consent {
                            at_ms,
                            version,
                            by: match agreed {
                                PhoneAgreed::Caller => ConsentBy::Caller,
                                PhoneAgreed::NotAsked => ConsentBy::NotAsked,
                            },
                            text,
                        },
                    },
                    line: PhoneCaller {
                        tail: c.tail.clone(),
                        withheld: c.withheld,
                        ban_key: c.ban_key.clone(),
                    },
                },
            });
        }
    }
}

/// Words for a refusal in the log: never a number.
pub fn refusal_words(r: PhoneRefusal) -> &'static str {
    match r {
        PhoneRefusal::CallInClosed => "call-in is closed",
        PhoneRefusal::Banned => "the number is banned",
        PhoneRefusal::LinesFull => "every line is busy",
        PhoneRefusal::TooManyCalls => "too many calls from the number this hour",
        PhoneRefusal::WordsNotReady => "the phone words cannot play",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::DeskPhoneBooth;
    use crate::phone::gate::PhoneKey;
    use crate::phone::limits::PhoneBan;
    use crate::phone::words::ALBA_FILE;
    use weavesignal_model::caller_id::livekit_identity;

    fn line(words_on: bool) -> Line {
        Line::new(
            PhoneConfig {
                enabled: true,
                numbers: vec!["+441314960000".into()],
                allowed_addresses: vec!["127.0.0.1/32".into()],
                public_ip: "127.0.0.1".into(),
                lines: 2,
                consent_words: words_on,
                ..PhoneConfig::default()
            },
            Some(BanKeys::new([5; 32])),
        )
    }

    fn booth(person: &str, from: &str) -> DeskPhoneBooth {
        DeskPhoneBooth {
            person: person.into(),
            tail: weavesignal_model::caller_id::livekit_tail(from).into(),
            sip_identity: livekit_identity(from),
            answered: false,
            admitted: false,
            words_playing: None,
            words_done: 0,
        }
    }

    fn moment(bans: &Bans) -> Moment<'_> {
        Moment {
            call_in_open: true,
            consent_today: "Live on Kettle FM.",
            station_recording: None,
            default_present: true,
            settings: PhoneSettings::default(),
            bans,
            in_queue: 0,
        }
    }

    #[test]
    fn a_call_hears_the_words_and_a_key_lets_it_in_with_four_digits() {
        let l = line(true);
        let bans = Bans::default();
        let mut phone = DeskPhone {
            booths: vec![booth("phone-A", "+447700900123")],
            keys: vec![],
        };
        let t = l.step(&phone, &moment(&bans), 0);
        let version = PhoneWords::Default(crate::phone::words::PhoneVoice::Alba).version();
        assert_eq!(
            t,
            vec![Todo::Words {
                person: "phone-A".into(),
                file: ALBA_FILE.into(),
                version: version.clone(),
                gain_mb: 0
            }]
        );
        // The same poll again changes nothing.
        assert!(l.step(&phone, &moment(&bans), 100).is_empty());
        phone.keys.push(PhoneKey {
            seq: 1,
            person: "phone-A".into(),
            code: 5,
            words_version: Some(version.clone()),
        });
        let t = l.step(&phone, &moment(&bans), 4_000);
        match &t[..] {
            [Todo::Admit { call, line }] => {
                assert_eq!(call.identity, "phone-A");
                assert_eq!(call.consent.by, ConsentBy::Caller);
                assert_eq!(call.consent.version, version);
                assert_eq!(line.tail, "0123");
                assert!(!line.withheld);
                assert!(line.ban_key.as_deref().unwrap().starts_with("pb_"));
            }
            other => panic!("{other:?}"),
        }
        // The key reported again is not acted on twice.
        assert!(l.step(&phone, &moment(&bans), 4_100).is_empty());
    }

    #[test]
    fn a_banned_number_is_refused_and_a_withheld_one_cannot_be_matched() {
        let l = line(true);
        let key = l.ban_key_of(&livekit_identity("+447700900123")).unwrap();
        let mut bans = Bans::default();
        bans.ban(
            PhoneBan {
                ban_key: key,
                tail: "0123".into(),
                date: "2026-11-20".into(),
                reason: String::new(),
                expires_ms: None,
            },
            false,
        )
        .unwrap();
        let phone = DeskPhone {
            booths: vec![
                booth("phone-A", "+447700900123"),
                booth("phone-W", "anonymous"),
            ],
            keys: vec![],
        };
        let t = l.step(&phone, &moment(&bans), 0);
        assert_eq!(
            t[0],
            Todo::HangUp {
                person: "phone-A".into(),
                why: CallEnded::Refused(PhoneRefusal::Banned)
            }
        );
        assert!(matches!(&t[1], Todo::Words { person, .. } if person == "phone-W"));
        assert_eq!(l.ban_key_of(&livekit_identity("anonymous")), None);
    }

    #[test]
    fn lines_full_and_the_line_closed() {
        let l = line(true);
        let bans = Bans::default();
        let phone = DeskPhone {
            booths: vec![
                booth("phone-A", "+447700900001"),
                booth("phone-B", "+447700900002"),
                booth("phone-C", "+447700900003"),
            ],
            keys: vec![],
        };
        let t = l.step(&phone, &moment(&bans), 0);
        assert_eq!(
            t[2],
            Todo::HangUp {
                person: "phone-C".into(),
                why: CallEnded::Refused(PhoneRefusal::LinesFull)
            }
        );
        assert_eq!(l.at_gate(), 2);
        // The station's recording of older words closes the line.
        let mut m = moment(&bans);
        m.station_recording = Some(crate::phone::words::StationRecording {
            text: "Live on Old FM. Press any key to carry on.".into(),
            file: "x.flac".into(),
            gain_mb: 0,
        });
        assert_eq!(
            l.closed(&m),
            Some(Closed::Words(WordsProblem::RecordingOutOfDate))
        );
        let off = Line::new(PhoneConfig::default(), None);
        assert_eq!(off.closed(&moment(&bans)), Some(Closed::Off));
        let nokey = Line::new(line(true).config, None);
        assert_eq!(nokey.closed(&moment(&bans)), Some(Closed::NoKey));
    }

    #[test]
    fn words_off_let_the_caller_straight_in_as_not_asked() {
        let l = line(false);
        let bans = Bans::default();
        let phone = DeskPhone {
            booths: vec![booth("phone-A", "+447700900123")],
            keys: vec![],
        };
        match &l.step(&phone, &moment(&bans), 0)[..] {
            [Todo::Admit { call, .. }] => assert_eq!(call.consent.by, ConsentBy::NotAsked),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn words_that_end_twice_hang_up_and_a_booth_that_goes_is_forgotten() {
        let l = line(true);
        let bans = Bans::default();
        let mut phone = DeskPhone {
            booths: vec![booth("phone-A", "+447700900123")],
            keys: vec![],
        };
        l.step(&phone, &moment(&bans), 0);
        phone.booths[0].words_done = 1;
        assert!(matches!(
            &l.step(&phone, &moment(&bans), 10_000)[..],
            [Todo::Words { .. }]
        ));
        phone.booths[0].words_done = 2;
        assert_eq!(
            l.step(&phone, &moment(&bans), 20_000),
            vec![Todo::HangUp {
                person: "phone-A".into(),
                why: CallEnded::NoKey
            }]
        );
        phone.booths.clear();
        assert!(l.step(&phone, &moment(&bans), 21_000).is_empty());
        assert_eq!(l.at_gate(), 0);
    }

    #[test]
    fn a_number_that_rings_too_often_is_refused() {
        let mut c = line(true).config;
        c.calls_per_number_per_hour = 1;
        c.lines = 4;
        let l = Line::new(c, Some(BanKeys::new([5; 32])));
        let bans = Bans::default();
        let one = DeskPhone {
            booths: vec![booth("phone-A", "+447700900123")],
            keys: vec![],
        };
        l.step(&one, &moment(&bans), 0);
        let two = DeskPhone {
            booths: vec![booth("phone-B", "+447700900123")],
            keys: vec![],
        };
        assert_eq!(
            l.step(&two, &moment(&bans), 1_000)[0],
            Todo::HangUp {
                person: "phone-B".into(),
                why: CallEnded::Refused(PhoneRefusal::TooManyCalls)
            }
        );
    }
}
