// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Joining from a browser (M3): the consent words and their version, the call-in rate limit,
//! the identities the control plane mints, and the one state each person's own page shows.
//!
//! **What a person's page says.** The first live run failed because nothing on a guest's page
//! said when the presenter was live. So every join page follows its own [`MeState`]: one
//! *phase* (waiting, being screened, ready, in the presenter's ear, on air, off air, ended),
//! with what changes it (the screener talking to you, talkback, muted, OPEN while playout is
//! down), and whether the show itself is on air. It is computed here, from the desk's status and
//! the studio's queue, for one identity only: no page sees anyone else's name, topic or note.

use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;

use serde::Serialize;
use sha2::{Digest, Sha256};
use weavesignal_proto::Role;

use crate::desk::DeskStatus;
use crate::studio::{CallerState, PLACE_HELD_MS, View};

/// The consent words as the caller reads them: the station's name, how long a call is kept
/// (`kept`, a phrase from [`kept_phrase`]) and how to ask for it to be taken down.
pub fn consent_words(template: &str, station: &str, kept: &str, contact: &str) -> String {
    template
        .replace("{station}", station)
        .replace("{kept}", kept)
        .replace("{contact}", contact.trim())
        .trim()
        .to_owned()
}

/// The `{contact}` phrase (ADR-0215): the show on air's takedown contact first, then the
/// station's own as the host, "<show's>. This station is hosted by <station>: <station's>";
/// the station's alone when the show has none. A show's contact is optional, the station's is
/// not: call-in stays closed without it ([`crate::public`]).
pub fn contact_phrase(show_contact: &str, station: &str, station_contact: &str) -> String {
    let (show_contact, station_contact) = (show_contact.trim(), station_contact.trim());
    if show_contact.is_empty() {
        station_contact.to_owned()
    } else {
        format!("{show_contact}. This station is hosted by {station}: {station_contact}")
    }
}

/// How long programme recordings (the archive and its podcast) are kept, as the consent words
/// say it, exactly: "for two years", "for 30 days", or "only until the show ends" when nothing
/// is kept.
pub fn kept_phrase(period: weavesignal_model::retention::Period) -> String {
    use weavesignal_model::retention::Period;
    fn n(k: u32, one: &str, many: &str) -> String {
        const WORDS: [&str; 10] = [
            "", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        ];
        let num = WORDS
            .get(k as usize)
            .filter(|w| !w.is_empty())
            .map_or_else(|| k.to_string(), |w| (*w).to_owned());
        format!("for {num} {}", if k == 1 { one } else { many })
    }
    match period {
        Period::Days { days: 0 } => "only until the show ends".to_owned(),
        Period::Days { days } if days % 365 == 0 => n(days / 365, "year", "years"),
        Period::Days { days } => n(days, "day", "days"),
        _ => "for as long as the station's archive is kept".to_owned(),
    }
}

/// The words' version: the first 16 hex digits of their SHA-256, so the log can say exactly
/// which words a caller agreed to and a page that shows older words is asked to read again.
pub fn consent_version(words: &str) -> String {
    Sha256::digest(words.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A fresh room identity for a person who joins from a browser: `<role>-<12 hex>`, inside
/// [`weavesignal_proto::check_identity`]'s alphabet and clear of the desk's prefix.
pub fn mint_identity(role: Role) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("{}-{}", role.as_str(), &id[..12])
}

/// A caller's name and topic, checked: trimmed, no control characters, within bounds.
pub fn clean_line(text: &str, max: usize, what: &str) -> Result<String, String> {
    let t = text.trim();
    if t.chars().any(char::is_control) {
        return Err(format!("{what} must be one line"));
    }
    if t.chars().count() > max {
        return Err(format!("{what} is at most {max} characters"));
    }
    Ok(t.to_owned())
}

pub const NAME_MAX: usize = 60;
pub const TOPIC_MAX: usize = 140;

/// Call-in's rate limit: so many calls per address per hour, and so many for the station per
/// minute. Fixed windows of timestamps; memory is bounded by the limits themselves: every
/// address's hour is pruned on each call, so at most an hour of the station's calls is kept.
#[derive(Debug, Default)]
pub struct Limiter {
    by_address: BTreeMap<IpAddr, VecDeque<u64>>,
    station: VecDeque<u64>,
}

const HOUR_MS: u64 = 3_600_000;
const MINUTE_MS: u64 = 60_000;

impl Limiter {
    /// How many calls the station took in the minute before `now` (for the challenge's
    /// difficulty under pressure).
    pub fn station_load(&self, now: u64) -> usize {
        self.station
            .iter()
            .filter(|&&t| now.saturating_sub(t) < MINUTE_MS)
            .count()
    }

    /// Whether one more call from `addr` at `now` is allowed; if it is, it is counted.
    pub fn allow(
        &mut self,
        addr: Option<IpAddr>,
        now: u64,
        per_hour: u32,
        per_minute: u32,
    ) -> bool {
        self.station.retain(|&t| now.saturating_sub(t) < MINUTE_MS);
        if self.station.len() >= per_minute as usize {
            return false;
        }
        // Forget every address whose hour has passed, not only this one's.
        self.by_address.retain(|_, q| {
            q.retain(|&t| now.saturating_sub(t) < HOUR_MS);
            !q.is_empty()
        });
        if let Some(a) = addr {
            if self.by_address.get(&a).map_or(0, VecDeque::len) >= per_hour as usize {
                return false;
            }
            self.by_address.entry(a).or_default().push_back(now);
        }
        self.station.push_back(now);
        true
    }
}

/// Where a person stands, as their own page shows it.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The desk has not seen them in the room yet (or the desk is not answering).
    Joining,
    /// A caller in the queue, hearing the programme.
    Waiting,
    /// A caller the screener has picked up.
    Screening,
    /// A caller sent to the desk: next.
    Ready,
    /// A caller the presenter declined for now: still in the queue (DESK BUSY).
    DeskBusy,
    /// The presenter hears them in their headphones, off air: their turn is coming.
    Cue,
    /// The room hears them (and listeners, unless `open`).
    OnAir,
    /// Not on air: a guest or presenter taken off, a remote DJ before the handover.
    OffAir,
    /// The studio ended the call or the invite was withdrawn.
    Ended,
}

/// One person's state, for their own page.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct MeState {
    pub identity: String,
    pub role: Role,
    pub phase: Phase,
    /// On air while playout is down: the room hears them, listeners do not (ADR-0062).
    pub open: bool,
    /// The studio has muted them.
    pub muted: bool,
    /// A caller's place in the queue (1 is next) and the queue's length.
    pub place: Option<usize>,
    pub queue: usize,
    /// The screener is talking to them (held T, ADR-0095).
    pub screener_talking: bool,
    /// Someone in the studio is talking in their ear (talkback, for presenters and DJs).
    pub talkback: bool,
    /// Their link is down; the place is held until this time (ADR-0101).
    pub held_until_ms: Option<u64>,
    pub station: String,
    pub show: String,
    /// The presenter's name, and whether the room hears them now.
    pub presenter: String,
    pub presenter_live: bool,
    /// Listeners hear the show: playout is connected and someone is open.
    pub show_on_air: bool,
    pub desk_reachable: bool,
}

/// One person's state from the studio's view and the desk's people.
pub fn me_state(
    identity: &str,
    role: Role,
    view: &View,
    desk: Option<&DeskStatus>,
    ended: bool,
) -> MeState {
    let people = desk.map(|d| d.people.as_slice()).unwrap_or(&[]);
    let me = people.iter().find(|p| p.identity == identity);
    let playout = view.desk.playout.connected;
    let presenter = view
        .seats
        .presenter
        .as_deref()
        .and_then(|id| people.iter().find(|p| p.identity == id));
    let presenter_live = presenter.is_some_and(|p| p.on_air && !p.mute && p.present);
    let anyone_open = people
        .iter()
        .any(|p| p.on_air && !p.mute && p.present && p.role.can_air());
    let caller = view.callers.iter().find(|c| c.identity == identity);
    let queue = view
        .callers
        .iter()
        .filter(|c| !c.on_air && c.dropped_ms.is_none())
        .count();
    let place = caller.filter(|c| !c.on_air).map(|c| {
        view.callers
            .iter()
            .filter(|o| !o.on_air && o.dropped_ms.is_none())
            .position(|o| o.identity == identity)
            .map_or(c.ordinal, |i| i + 1)
    });
    let phase = if ended {
        Phase::Ended
    } else {
        match me {
            Some(p) if p.on_air => Phase::OnAir,
            Some(p) if p.cue => Phase::Cue,
            Some(_) if role == Role::Caller => match caller {
                None => Phase::Joining,
                Some(c) if c.declined.is_some() => Phase::DeskBusy,
                Some(c) => match c.state {
                    CallerState::Waiting => Phase::Waiting,
                    CallerState::Screening => Phase::Screening,
                    CallerState::Ready => Phase::Ready,
                },
            },
            Some(_) => Phase::OffAir,
            None => Phase::Joining,
        }
    };
    let held_until_ms = caller.and_then(|c| c.dropped_ms).map(|t| t + PLACE_HELD_MS);
    let screener_talking = people
        .iter()
        .any(|p| p.role == Role::Screener && p.talk_to.as_deref() == Some(identity));
    let talkback = role.hears_talkback()
        && people.iter().any(|p| {
            matches!(p.role, Role::Screener | Role::Producer)
                && (p.talk || p.talk_to.as_deref() == Some(identity))
        });
    MeState {
        identity: identity.to_owned(),
        role,
        phase,
        open: phase == Phase::OnAir && !playout,
        muted: me.is_some_and(|p| p.mute),
        place: if matches!(phase, Phase::Ended | Phase::OnAir) {
            None
        } else {
            place
        },
        queue,
        screener_talking: screener_talking && !ended,
        talkback: talkback && !ended,
        held_until_ms,
        station: view.station.clone(),
        show: view.show.title.clone(),
        presenter: if view.seats.presenter_name.is_empty() {
            view.seats.presenter.clone().unwrap_or_default()
        } else {
            view.seats.presenter_name.clone()
        },
        presenter_live,
        show_on_air: playout && anyone_open,
        desk_reachable: view.desk.reachable,
    }
}

#[cfg(test)]
mod contact_tests {
    use super::{consent_words, contact_phrase};

    const WORDS: &str = "You'll be live on {station}, and we may keep your call in our archive and podcast {kept}. To have it taken down, contact {contact}.";

    #[test]
    fn the_shows_contact_comes_first_and_the_station_is_named_as_the_host() {
        let c = contact_phrase("shona@example.org", "Kettle FM", "info@kettle.example");
        assert_eq!(
            c,
            "shona@example.org. This station is hosted by Kettle FM: info@kettle.example"
        );
        let w = consent_words(WORDS, "Kettle FM", "for two years", &c);
        assert_eq!(
            w,
            "You'll be live on Kettle FM, and we may keep your call in our archive and podcast for two years. To have it taken down, contact shona@example.org. This station is hosted by Kettle FM: info@kettle.example."
        );
    }

    #[test]
    fn without_a_show_contact_only_the_stations_is_said() {
        assert_eq!(
            contact_phrase("  ", "Kettle FM", " info@kettle.example "),
            "info@kettle.example"
        );
        let w = consent_words(
            WORDS,
            "Kettle FM",
            "for 30 days",
            &contact_phrase("", "Kettle FM", "info@kettle.example"),
        );
        assert!(w.ends_with("To have it taken down, contact info@kettle.example."));
        assert!(!w.contains("hosted by"));
    }

    #[test]
    fn words_without_a_contact_placeholder_have_nowhere_for_the_shows() {
        let w = consent_words(
            "Live on {station}.",
            "Kettle FM",
            "",
            &contact_phrase("x@y", "Kettle FM", "z@w"),
        );
        assert_eq!(w, "Live on Kettle FM.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::{DeskPerson, PlayoutLink};
    use crate::studio::{CallIn, Consent, ConsentBy, Studio};

    fn person(id: &str, name: &str, role: Role) -> DeskPerson {
        DeskPerson {
            identity: id.into(),
            name: name.into(),
            role,
            on_air: role.on_air_at_join(),
            mute: false,
            cue: false,
            talk: false,
            talk_to: None,
            present: true,
            away_ms: None,
            linked: true,
            lufs: None,
            lufs_peak: None,
        }
    }

    fn consented(s: &mut Studio, id: &str) {
        s.call_in(CallIn {
            identity: id.into(),
            name: "Jessie Brown".into(),
            topic: "No bus after 23:10".into(),
            consent: Consent {
                at_ms: 0,
                version: consent_version("words"),
                by: ConsentBy::Caller,
                text: "words".into(),
            },
        });
    }

    struct World {
        studio: Studio,
        desk: DeskStatus,
    }

    impl World {
        fn new(people: Vec<DeskPerson>, playout: bool) -> Self {
            let mut studio = Studio::new("The Morning Kettle", vec![]);
            consented(&mut studio, "caller-jessie");
            consented(&mut studio, "caller-iona");
            let desk = DeskStatus {
                people,
                playout: PlayoutLink {
                    connected: playout,
                    for_ms: Some(1),
                },
                mode: None,
                delay: None,
                carts: None,
                video: None,
            };
            studio.sync(&desk, 0);
            World { studio, desk }
        }
        fn sync(&mut self, now: u64) {
            self.studio.sync(&self.desk, now);
        }
        fn me(&self, id: &str, role: Role) -> MeState {
            let v = self.studio.view("Kettle FM", Some(&self.desk), None, 0);
            me_state(id, role, &v, Some(&self.desk), false)
        }
        fn p(&mut self, id: &str) -> &mut DeskPerson {
            self.desk
                .people
                .iter_mut()
                .find(|p| p.identity == id)
                .unwrap()
        }
    }

    fn room() -> Vec<DeskPerson> {
        vec![
            person("rory", "Rory Bruce", Role::Presenter),
            person("elspeth", "Elspeth Gordon", Role::Screener),
            person("caller-iona", "Iona MacIntosh", Role::Caller),
            person("caller-jessie", "Jessie Brown", Role::Caller),
        ]
    }

    #[test]
    fn the_consent_words_carry_the_station_and_a_stable_version() {
        let w = consent_words("{station} may record it. ", "Kettle FM", "", "");
        assert_eq!(w, "Kettle FM may record it.");
        assert_eq!(
            consent_version(&w),
            consent_version("Kettle FM may record it.")
        );
        assert_ne!(
            consent_version(&w),
            consent_version("Kettle FM may record it")
        );
        assert_eq!(consent_version(&w).len(), 16);
    }

    #[test]
    fn minted_identities_pass_the_identity_rule() {
        for role in [Role::Caller, Role::Guest, Role::Dj, Role::Presenter] {
            let id = mint_identity(role);
            assert!(
                weavesignal_proto::check_identity(&id, "desk").is_ok(),
                "{id}"
            );
            assert!(id.starts_with(role.as_str()));
        }
        assert_ne!(mint_identity(Role::Caller), mint_identity(Role::Caller));
    }

    #[test]
    fn names_and_topics_are_one_bounded_line() {
        assert_eq!(
            clean_line("  Jessie  ", NAME_MAX, "name"),
            Ok("Jessie".into())
        );
        assert!(clean_line("a\nb", NAME_MAX, "name").is_err());
        assert!(clean_line(&"x".repeat(NAME_MAX + 1), NAME_MAX, "name").is_err());
    }

    #[test]
    fn the_limiter_counts_per_address_and_for_the_station() {
        let mut l = Limiter::default();
        let a: IpAddr = "192.0.2.7".parse().unwrap();
        let b: IpAddr = "192.0.2.8".parse().unwrap();
        assert!(l.allow(Some(a), 0, 2, 100));
        assert!(l.allow(Some(a), 1, 2, 100));
        assert!(!l.allow(Some(a), 2, 2, 100), "a third call within the hour");
        assert!(
            l.allow(Some(b), 3, 2, 100),
            "another address is not held back"
        );
        assert!(l.allow(Some(a), HOUR_MS + 1, 2, 100), "a new hour");
        // Addresses that called once are forgotten after their hour, whoever calls next.
        let mut l = Limiter::default();
        for i in 0..50u8 {
            assert!(l.allow(Some(IpAddr::from([198, 51, 100, i])), u64::from(i), 6, 100));
        }
        assert_eq!(l.by_address.len(), 50);
        assert!(l.allow(Some(b), HOUR_MS + 100, 6, 100));
        assert_eq!(l.by_address.len(), 1, "only the latest caller is kept");
        // A refused address leaves nothing behind.
        let mut l = Limiter::default();
        assert!(!l.allow(Some(a), 0, 0, 100));
        assert!(l.by_address.is_empty());
        let mut l = Limiter::default();
        assert!(l.allow(None, 0, 9, 1));
        assert!(!l.allow(None, 10, 9, 1), "the station's minute is full");
        assert!(l.allow(None, MINUTE_MS + 1, 9, 1));
    }

    #[test]
    fn a_caller_waits_with_a_place_then_is_screened_ready_cued_and_on_air() {
        let mut w = World::new(room(), true);
        let me = w.me("caller-jessie", Role::Caller);
        assert_eq!(me.phase, Phase::Waiting);
        assert_eq!((me.place, me.queue), (Some(2), 2));
        assert!(me.presenter_live && me.show_on_air);
        assert_eq!(me.presenter, "Rory Bruce");
        assert!(!me.screener_talking);

        w.studio.screen("caller-jessie", 1).unwrap();
        w.p("elspeth").talk_to = Some("caller-jessie".into());
        let me = w.me("caller-jessie", Role::Caller);
        assert_eq!(me.phase, Phase::Screening);
        assert!(me.screener_talking, "the screener holds T");
        assert!(
            !w.me("caller-iona", Role::Caller).screener_talking,
            "only to her"
        );

        w.studio.send("caller-jessie", 2).unwrap();
        w.sync(3);
        assert_eq!(w.me("caller-jessie", Role::Caller).phase, Phase::Ready);
        w.studio.decline("caller-jessie", "two-way", 4).unwrap();
        assert_eq!(w.me("caller-jessie", Role::Caller).phase, Phase::DeskBusy);
        w.studio.send("caller-jessie", 5).unwrap();
        w.p("caller-jessie").cue = true;
        w.sync(6);
        assert_eq!(w.me("caller-jessie", Role::Caller).phase, Phase::Cue);
        w.p("caller-jessie").on_air = true;
        w.sync(7);
        let me = w.me("caller-jessie", Role::Caller);
        assert_eq!((me.phase, me.open, me.place), (Phase::OnAir, false, None));
        assert_eq!(
            w.me("caller-iona", Role::Caller).place,
            Some(1),
            "Iona is next now"
        );
    }

    #[test]
    fn on_air_while_playout_is_down_is_open() {
        let w = World::new(room(), false);
        let me = w.me("rory", Role::Presenter);
        assert_eq!((me.phase, me.open), (Phase::OnAir, true));
        assert!(!me.show_on_air, "listeners hear nothing of the room");
    }

    #[test]
    fn a_dropped_link_shows_the_held_place_and_an_ended_call_says_so() {
        let mut w = World::new(room(), true);
        w.p("caller-iona").present = false;
        w.sync(10_000);
        let me = w.me("caller-iona", Role::Caller);
        assert_eq!(me.held_until_ms, Some(10_000 + PLACE_HELD_MS));
        let v = w.studio.view("Kettle FM", Some(&w.desk), None, 0);
        let ended = me_state("caller-iona", Role::Caller, &v, Some(&w.desk), true);
        assert_eq!((ended.phase, ended.place), (Phase::Ended, None));
    }

    #[test]
    fn a_dj_is_off_air_until_the_handover_and_hears_talkback() {
        let mut people = room();
        people.push(person("finlay", "Finlay Grant", Role::Dj));
        let mut w = World::new(people, true);
        let me = w.me("finlay", Role::Dj);
        assert_eq!(me.phase, Phase::OffAir);
        w.p("elspeth").talk = true;
        assert!(w.me("finlay", Role::Dj).talkback);
        assert!(
            !w.me("caller-jessie", Role::Caller).talkback,
            "callers hear no talkback"
        );
        w.p("finlay").on_air = true;
        w.p("finlay").mute = true;
        let me = w.me("finlay", Role::Dj);
        assert_eq!((me.phase, me.muted), (Phase::OnAir, true));
    }

    #[test]
    fn someone_the_desk_has_not_seen_is_joining() {
        let w = World::new(room(), true);
        assert_eq!(w.me("guest-new", Role::Guest).phase, Phase::Joining);
        let v = w.studio.view("Kettle FM", None, Some("no desk".into()), 0);
        let me = me_state("rory", Role::Presenter, &v, None, false);
        assert_eq!(me.phase, Phase::Joining);
        assert!(!me.desk_reachable);
    }
}

#[cfg(test)]
mod consent_tests {
    use super::*;
    use weavesignal_model::retention::Period;

    #[test]
    fn how_long_a_call_is_kept_reads_plainly_and_exactly() {
        assert_eq!(kept_phrase(Period::days(0)), "only until the show ends");
        assert_eq!(kept_phrase(Period::days(365)), "for one year");
        assert_eq!(kept_phrase(Period::days(730)), "for two years");
        assert_eq!(
            kept_phrase(Period::days(30)),
            "for 30 days",
            "never rounded to a month"
        );
        assert_eq!(kept_phrase(Period::days(42)), "for 42 days");
        assert_eq!(
            kept_phrase(Period::WithArchive),
            "for as long as the station's archive is kept"
        );
    }

    #[test]
    fn the_default_words_are_two_sentences_with_everything_filled_in() {
        let w = consent_words(
            crate::config::CONSENT_TEXT,
            "Kettle FM",
            &kept_phrase(Period::days(730)),
            " calls@kettle.example ",
        );
        assert_eq!(
            w,
            "You'll be live on Kettle FM, and we may keep your call in our archive and podcast for two years. To have it taken down, contact calls@kettle.example."
        );
        assert!(!w.contains('{'));
    }
}
