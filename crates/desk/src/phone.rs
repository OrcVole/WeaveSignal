// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line at the desk (Phase 17f, module 9 of `2026-11-20-17f-phone-line-design.md`).
//!
//! LiveKit's SIP service makes a room per call (a **phone booth**, `<studio>-phone_<tail>_<random>`)
//! and puts the caller in it as a `sip_…` participant. The room watch finds those booths beside
//! the browser callers' (ADR-0193) and the desk mixes the caller as a caller under the person key
//! `phone-<random>`, with three differences, each the gate's (`crates/control/src/phone/gate.rs`):
//!
//! - **Nothing is answered on sight.** The SIP service answers a call only once its caller has
//!   subscribed to a track someone publishes in the booth (the spike, 2026-11-20), and the desk's
//!   publishing of a return is that track. So a phone caller's return is not published when they
//!   join: the first `phone_words` publishes it (the words answer the call), or `phone_admit`
//!   when the station's words are off. A call the control plane refuses is never answered; it
//!   rings until `phone_hang_up` ends it.
//! - **Held out until admitted.** The caller's voice reaches no bus (not even the screener's,
//!   whom a caller's voice reaches always) until `phone_admit`: nothing of a caller who never
//!   agreed is mixed, recorded or heard.
//! - **The words, then hold audio.** While the words play, they are the caller's whole return;
//!   when they end the return is a waiting caller's as ever (the programme), and the desk counts
//!   the play (`words_done`).
//!
//! **Keys** arrive as LiveKit `DataPacket`s (`sip_dtmf`) on the `_reliable` data channel the
//! server opens toward the desk's subscriber in the booth; the desk numbers each one for the life
//! of the desk and reports the last [`KEYS_KEPT`] in its status with the version of the words that
//! was playing (or last played) when it came. The control plane acts on each number once.
//!
//! **Two identities.** The person key is the desk's and the studio's name for the caller; the
//! `sip_…` identity is LiveKit's, used only to subscribe, to let the caller subscribe to their
//! return (ADR-0118's limit), to hang up, and reported to the control plane for the ban key. It
//! is never logged here, and neither is the booth's name (it carries the number's last digits).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, mpsc};

use serde::Serialize;
use weavesignal_mixer::CartFeed;

use crate::carts::Decoder;

/// Keys kept for the control plane to read.
pub const KEYS_KEPT: usize = 256;

/// A phone booth as the room watch found it, with its caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneSeen {
    /// `phone-<random>`.
    pub person: String,
    /// The booth's room.
    pub room: String,
    /// The From user's last characters as LiveKit kept them.
    pub tail: String,
    /// LiveKit's identity for the caller (`sip_…`).
    pub sip_identity: String,
}

/// One key as the desk reports it (the control plane's `PhoneKey`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhoneKeyStatus {
    pub seq: u64,
    pub person: String,
    pub code: u32,
    pub words_version: Option<String>,
}

/// One phone booth as the desk reports it (the control plane's `DeskPhoneBooth`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhoneBoothStatus {
    pub person: String,
    pub tail: String,
    /// For the ban key only: the status goes over the desk's own socket to the control plane,
    /// which never sends it on or logs it.
    pub sip_identity: String,
    pub answered: bool,
    pub admitted: bool,
    pub words_playing: Option<String>,
    pub words_done: u32,
}

/// The phone line in the desk's status.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PhoneStatus {
    pub booths: Vec<PhoneBoothStatus>,
    pub keys: Vec<PhoneKeyStatus>,
}

/// The words playing into one caller's return.
struct Words {
    version: String,
    gain: f32,
    feed: Arc<CartFeed>,
    /// `None` in tests that fill the feed themselves.
    decoder: Option<Decoder>,
}

struct Booth {
    room: String,
    tail: String,
    sip_identity: String,
    answered: bool,
    admitted: bool,
    words: Option<Words>,
    /// The version of the words last started, for a key that comes just after they end.
    last_version: Option<String>,
    words_done: u32,
}

/// The desk's phone booths and keys; kept in the desk's state, under its lock.
#[derive(Default)]
pub struct Phone {
    booths: BTreeMap<String, Booth>,
    keys: VecDeque<PhoneKeyStatus>,
    next_seq: u64,
    /// Calls to end in the room: (booth, `sip_…` identity), taken by the room watch.
    hang_ups: Vec<(String, String)>,
    /// Booths hung up: left out of the room watch's look until LiveKit has closed them.
    ended: BTreeSet<String>,
    /// Where decoders go to be stopped, off the mixing pass (made at the first).
    reaper: Option<mpsc::Sender<Decoder>>,
    buf: Vec<f32>,
}

impl Phone {
    pub fn new() -> Self {
        Self::default()
    }

    fn reap(&mut self, words: Option<Words>) {
        let Some(Words {
            decoder: Some(d),
            feed,
            ..
        }) = words
        else {
            return;
        };
        feed.close();
        let reaper = self.reaper.get_or_insert_with(crate::carts::reaper);
        // A reaper that has gone (the desk is stopping) drops it here instead.
        let _ = reaper.send(d);
    }

    /// The room watch's look: the booths with a caller now. Booths hung up are left out while
    /// LiveKit still lists them; booths gone are forgotten, their words stopped. What the desk
    /// should mix comes back.
    pub fn note(&mut self, seen: Vec<PhoneSeen>) -> Vec<PhoneSeen> {
        let rooms: BTreeSet<&str> = seen.iter().map(|s| s.room.as_str()).collect();
        self.ended.retain(|r| rooms.contains(r.as_str()));
        let live: Vec<PhoneSeen> = seen
            .into_iter()
            .filter(|s| !self.ended.contains(&s.room))
            .collect();
        let here: BTreeSet<&str> = live.iter().map(|s| s.person.as_str()).collect();
        let gone: Vec<String> = self
            .booths
            .keys()
            .filter(|p| !here.contains(p.as_str()))
            .cloned()
            .collect();
        for p in gone {
            if let Some(b) = self.booths.remove(&p) {
                self.reap(b.words);
            }
        }
        for s in &live {
            self.booths
                .entry(s.person.clone())
                .or_insert_with(|| Booth {
                    room: s.room.clone(),
                    tail: s.tail.clone(),
                    sip_identity: s.sip_identity.clone(),
                    answered: false,
                    admitted: false,
                    words: None,
                    last_version: None,
                    words_done: 0,
                });
        }
        live
    }

    /// LiveKit's identity for a phone caller.
    pub fn sip_identity(&self, person: &str) -> Option<&str> {
        self.booths.get(person).map(|b| b.sip_identity.as_str())
    }

    /// Whether a person's voice is held out of every bus: a phone caller not yet admitted (one
    /// the desk no longer knows as a booth stays held).
    pub fn held(&self, person: &str) -> bool {
        weavesignal_proto::phone::is_phone_person(person)
            && !self.booths.get(person).is_some_and(|b| b.admitted)
    }

    /// Whether a phone caller's return may be published: only once answered.
    pub fn may_publish(&self, person: &str) -> bool {
        !weavesignal_proto::phone::is_phone_person(person)
            || self.booths.get(person).is_some_and(|b| b.answered)
    }

    /// Mark a call answered; whether it was not before (its return is to be published now).
    /// `None`: no such booth.
    pub fn answer(&mut self, person: &str) -> Option<bool> {
        let b = self.booths.get_mut(person)?;
        let first = !b.answered;
        b.answered = true;
        Some(first)
    }

    /// Start the words for a caller (any playing are replaced); `None`: no such booth.
    pub fn start_words(
        &mut self,
        person: &str,
        version: &str,
        gain: f32,
        feed: Arc<CartFeed>,
        decoder: Option<Decoder>,
    ) -> Option<()> {
        let b = self.booths.get_mut(person)?;
        b.last_version = Some(version.to_owned());
        let old = b.words.replace(Words {
            version: version.to_owned(),
            gain,
            feed,
            decoder,
        });
        self.reap(old);
        Some(())
    }

    /// Let a caller's voice in; `false`: no such booth.
    pub fn admit(&mut self, person: &str) -> bool {
        self.booths
            .get_mut(person)
            .map(|b| b.admitted = true)
            .is_some()
    }

    /// End a call: the booth is forgotten, its words stopped, and the room watch asked to take
    /// the caller out of the room. `false`: no such booth.
    pub fn hang_up(&mut self, person: &str) -> bool {
        let Some(b) = self.booths.remove(person) else {
            return false;
        };
        self.ended.insert(b.room.clone());
        self.hang_ups.push((b.room, b.sip_identity));
        self.reap(b.words);
        true
    }

    /// The calls to end in the room, for the room watch.
    pub fn take_hang_ups(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.hang_ups)
    }

    /// A call to end that the room watch could not: tried again on its next look.
    pub fn hang_up_again(&mut self, room: String, sip_identity: String) {
        self.hang_ups.push((room, sip_identity));
    }

    /// A key from a caller's booth: numbered and kept. `None`: not a booth the desk knows.
    pub fn key(&mut self, person: &str, code: u32) -> Option<u64> {
        let b = self.booths.get(person)?;
        let words_version = b
            .words
            .as_ref()
            .map(|w| w.version.clone())
            .or_else(|| b.last_version.clone());
        self.next_seq += 1;
        self.keys.push_back(PhoneKeyStatus {
            seq: self.next_seq,
            person: person.to_owned(),
            code,
            words_version,
        });
        while self.keys.len() > KEYS_KEPT {
            self.keys.pop_front();
        }
        Some(self.next_seq)
    }

    /// For the mixing pass: the words in a caller's return for this block of `samples`, if they
    /// are playing; the words' end is counted as a play done.
    pub fn render(&mut self, person: &str, samples: usize) -> Option<&[f32]> {
        let b = self.booths.get_mut(person)?;
        let w = b.words.as_ref()?;
        self.buf.clear();
        self.buf.resize(samples, 0.0);
        let pulled = w.feed.pull(&mut self.buf);
        let gain = w.gain;
        if pulled.drained {
            b.words_done += 1;
            let done = b.words.take();
            self.reap(done);
        }
        if gain != 1.0 {
            self.buf.iter_mut().for_each(|s| *s *= gain);
        }
        Some(&self.buf)
    }

    /// The booths of the people at the desk (`at_desk`), and the keys kept.
    pub fn status(&self, at_desk: impl Fn(&str) -> bool) -> PhoneStatus {
        PhoneStatus {
            booths: self
                .booths
                .iter()
                .filter(|(p, _)| at_desk(p))
                .map(|(p, b)| PhoneBoothStatus {
                    person: p.clone(),
                    tail: b.tail.clone(),
                    sip_identity: b.sip_identity.clone(),
                    answered: b.answered,
                    admitted: b.admitted,
                    words_playing: b.words.as_ref().map(|w| w.version.clone()),
                    words_done: b.words_done,
                })
                .collect(),
            keys: self.keys.iter().cloned().collect(),
        }
    }
}

/// Whether a phone words file name is one the control plane gives: `<name>.flac` (a station's
/// own recording, a UUID) or `<name>.opus` (a recording that ships with WeaveSignal, such as
/// `phone-words-alba`), with only letters, digits and `-` before it, never a path.
pub fn words_file_ok(file: &str) -> bool {
    let stem = file
        .strip_suffix(".flac")
        .or_else(|| file.strip_suffix(".opus"));
    stem.is_some_and(|stem| {
        !stem.is_empty()
            && stem.len() <= 64
            && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn seen(person: &str, room: &str) -> PhoneSeen {
        PhoneSeen {
            person: person.into(),
            room: room.into(),
            tail: "0123".into(),
            sip_identity: "sip_a8acc3a90a7b4e4d".into(),
        }
    }

    const P: &str = "phone-Xk3pQ9mZr2Ta";
    const ROOM: &str = "studio-phone_0123_Xk3pQ9mZr2Ta";

    #[test]
    fn a_phone_caller_is_held_and_unanswered_until_the_gate_says() {
        let mut ph = Phone::new();
        // Before the room watch has seen the booth: held, and no return.
        assert!(ph.held(P));
        assert!(!ph.may_publish(P));
        // Anyone else is neither.
        assert!(!ph.held("callum"));
        assert!(ph.may_publish("callum"));
        ph.note(vec![seen(P, ROOM)]);
        assert!(ph.held(P));
        assert!(!ph.may_publish(P));
        // The words (or, with the words off, the admission) answer the call, once.
        assert_eq!(ph.answer(P), Some(true));
        assert_eq!(ph.answer(P), Some(false));
        assert!(ph.may_publish(P));
        assert!(ph.held(P), "answered is not admitted");
        assert!(ph.admit(P));
        assert!(!ph.held(P));
        assert_eq!(ph.answer("phone-other"), None);
        assert!(!ph.admit("phone-other"));
    }

    #[test]
    fn keys_are_numbered_for_the_life_of_the_desk_with_the_words_version() {
        let mut ph = Phone::new();
        ph.note(vec![seen(P, ROOM)]);
        assert_eq!(ph.key(P, 5), Some(1));
        let feed = Arc::new(CartFeed::new(2, Duration::from_millis(100)));
        ph.start_words(P, "v1", 1.0, feed, None).unwrap();
        assert_eq!(ph.key(P, 0), Some(2));
        assert_eq!(ph.key("phone-unknown", 1), None);
        let st = ph.status(|_| true);
        assert_eq!(st.keys.len(), 2);
        assert_eq!(st.keys[0].words_version, None);
        assert_eq!(st.keys[1].words_version.as_deref(), Some("v1"));
        assert_eq!(st.keys[1].code, 0);
        for _ in 0..(KEYS_KEPT + 10) {
            ph.key(P, 1);
        }
        let st = ph.status(|_| true);
        assert_eq!(st.keys.len(), KEYS_KEPT);
        assert_eq!(st.keys.last().unwrap().seq, (KEYS_KEPT + 12) as u64);
    }

    #[test]
    fn the_words_play_then_count_as_done_and_a_late_key_keeps_their_version() {
        let mut ph = Phone::new();
        ph.note(vec![seen(P, ROOM)]);
        let feed = Arc::new(CartFeed::new(2, Duration::from_secs(1)));
        assert!(feed.push(&[0.5; 8]));
        feed.finish();
        ph.start_words(P, "v2", 0.5, feed, None).unwrap();
        assert_eq!(
            ph.status(|_| true).booths[0].words_playing.as_deref(),
            Some("v2")
        );
        let block = ph.render(P, 6).unwrap().to_vec();
        assert_eq!(block, vec![0.25; 6]);
        // The last of it, then silence, and the play is done.
        let block = ph.render(P, 6).unwrap().to_vec();
        assert_eq!(block, vec![0.25, 0.25, 0.0, 0.0, 0.0, 0.0]);
        let b = &ph.status(|_| true).booths[0];
        assert_eq!((b.words_playing.as_deref(), b.words_done), (None, 1));
        assert!(
            ph.render(P, 6).is_none(),
            "the return is a waiting caller's again"
        );
        ph.key(P, 1);
        assert_eq!(
            ph.status(|_| true).keys[0].words_version.as_deref(),
            Some("v2")
        );
    }

    #[test]
    fn a_hang_up_is_handed_to_the_room_watch_and_the_booth_is_left_out_until_closed() {
        let mut ph = Phone::new();
        ph.note(vec![seen(P, ROOM)]);
        assert!(ph.hang_up(P));
        assert!(!ph.hang_up(P));
        assert_eq!(
            ph.take_hang_ups(),
            vec![(ROOM.to_owned(), "sip_a8acc3a90a7b4e4d".to_owned())]
        );
        assert!(ph.take_hang_ups().is_empty());
        // LiveKit still lists it a moment: not mixed again.
        assert!(ph.note(vec![seen(P, ROOM)]).is_empty());
        assert!(ph.status(|_| true).booths.is_empty());
        // Once LiveKit has closed it, the name is forgotten too.
        assert!(ph.note(vec![]).is_empty());
        assert!(ph.ended.is_empty());
    }

    #[test]
    fn booths_gone_from_the_room_are_forgotten() {
        let mut ph = Phone::new();
        let other = "phone-Ab12Cd34Ef56";
        let live = ph.note(vec![
            seen(P, ROOM),
            seen(other, "studio-phone_9876_Ab12Cd34Ef56"),
        ]);
        assert_eq!(live.len(), 2);
        ph.answer(P);
        ph.note(vec![seen(P, ROOM)]);
        let st = ph.status(|_| true);
        assert_eq!(st.booths.len(), 1);
        assert!(st.booths[0].answered, "a booth still there keeps its state");
        // Only the people at the desk are reported.
        assert!(ph.status(|p| p != P).booths.is_empty());
    }

    #[test]
    fn words_files_are_names_never_paths() {
        assert!(words_file_ok("phone-words-default.flac"));
        assert!(words_file_ok("phone-words-alba.opus"));
        assert!(words_file_ok("0b6f3c1e-8a3e-4c8b-9a51-2f1d0c9e7b11.flac"));
        for bad in [
            "",
            ".flac",
            "../carts/x.flac",
            "/etc/passwd",
            "words.wav",
            "x.ogg",
            "a b.flac",
            "x.flac.flac",
        ] {
            assert!(!words_file_ok(bad), "{bad}");
        }
    }
}
