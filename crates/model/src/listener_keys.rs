// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Keyed shows (1.3.2, migration 0071; `docs/reference/design/keyed-shows.md`): what a listener
//! key is, what it may do and when it may not, and how the leak report ranks keys.
//!
//! This file is the part that needs no database, so that every rule has a unit test: the key
//! text and how it is typed, the verdict on a key at a moment ([`judge`]), a show's access rule
//! at a moment ([`effective`]), and the score that sorts the leak report ([`leak_score`]). The
//! queries are in [`crate::listener_keys_db`].
//!
//! A key is a *listener's* key. It is not a show key (ADR-0280: a host's way in), which has its
//! own types and routes; the words here are "listener key" wherever the two could be confused.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The letters a key's text is made of: A to Z without I, L and O, and the digits 2 to 9. No
/// character can be read as another, so a key read out on air or off a ticket is typed right.
pub const TEXT_ALPHABET: &[u8; 31] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// How many characters a key's text has (three groups of four).
pub const TEXT_LEN: usize = 12;

/// The text as it is compared and kept: capitals and digits only, so that case, spaces, dashes
/// and any other punctuation a person types are ignored.
pub fn normalise_text(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Whether a normalised text could be a key's: the right length, in the alphabet.
pub fn text_is_wellformed(norm: &str) -> bool {
    norm.len() == TEXT_LEN && norm.bytes().all(|b| TEXT_ALPHABET.contains(&b))
}

/// A normalised text as it is shown: `KETM-7QMP-2D4X`.
pub fn format_text(norm: &str) -> String {
    let mut out = String::with_capacity(norm.len() + 2);
    for (i, c) in norm.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// The kind of key (design section 3.1). Public-by-time is not a key but a show's `opens_at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = ListenerKeyKind)]
pub enum Kind {
    /// One named person.
    Personal,
    /// Many people who share it, up to `max_people`.
    Group,
    /// A relay or partner station: no devices, a stable stream address, rotated by staff.
    Machine,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Personal => "personal",
            Kind::Group => "group",
            Kind::Machine => "machine",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "personal" => Kind::Personal,
            "group" => Kind::Group,
            "machine" => Kind::Machine,
            _ => return None,
        })
    }
}

/// What a key opens: the sound alone, or the picture with the sound. A key for the picture
/// (`video`) carries the show's sound with it (the operator, 2026-11-25: a picture with no
/// voice is never what anyone means), so `video` and `both` open the same things; both names
/// are kept, and a key reads back as it was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = ListenerKeyMedia)]
pub enum Media {
    /// The sound alone: the keyed streams and the HLS audio rendition, never the picture.
    Sound,
    /// The picture, and the show's sound with it.
    Video,
    /// Sound and picture.
    Both,
}

impl Media {
    pub fn as_str(self) -> &'static str {
        match self {
            Media::Sound => "sound",
            Media::Video => "video",
            Media::Both => "both",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "sound" => Media::Sound,
            "video" => Media::Video,
            "both" => Media::Both,
            _ => return None,
        })
    }

    /// Every key opens the sound: a key for the picture carries it too.
    pub fn sound(self) -> bool {
        true
    }

    pub fn video(self) -> bool {
        self != Media::Sound
    }
}

/// Where a key is in its life. A key that has expired is still `active`: the window decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = ListenerKeyState)]
pub enum State {
    Active,
    /// Switched off for now and kept (a late payment); resumed with one call.
    Paused,
    /// Final. The holder is told it was ended by the station.
    Revoked,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Paused => "paused",
            State::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => State::Active,
            "paused" => State::Paused,
            "revoked" => State::Revoked,
            _ => return None,
        })
    }
}

/// A key as staff see it: everything but its text, its sealed email and their hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ListenerKey)]
pub struct Key {
    pub id: Uuid,
    pub family_id: Option<Uuid>,
    pub kind: Kind,
    /// Who it is for: a person's name, a group's, a partner's, or a ticket number.
    pub label: String,
    pub note: String,
    /// Whether an email address is kept (for the lost-key request and sending the key); the
    /// address itself is shown only by a deliberate call.
    pub has_email: bool,
    pub media: Media,
    /// Opens every keyed show.
    pub all_shows: bool,
    /// The shows it opens when it is not `all_shows` (show ids).
    pub shows: Vec<Uuid>,
    /// From when the key works (inclusive), as Unix milliseconds; null: already.
    pub valid_from_ms: Option<i64>,
    /// Until when (exclusive); null: no end.
    pub valid_until_ms: Option<i64>,
    /// Browsers or apps that may hold it at once; null: any number.
    pub max_devices: Option<u32>,
    /// How many times it may start a listen; null: any number.
    pub max_listens: Option<u32>,
    /// How many different devices may ever use it (a group key's people); null: any number.
    pub max_people: Option<u32>,
    pub state: State,
    pub state_reason: String,
    pub listens_used: u32,
    pub first_used_ms: Option<i64>,
    pub last_used_ms: Option<i64>,
    /// The key this one replaced, and the key that replaced it.
    pub replaces_id: Option<Uuid>,
    pub replaced_by_id: Option<Uuid>,
    pub created_ms: i64,
    pub created_by: String,
    pub updated_ms: i64,
}

/// Why a key does not open a show now. Each has a code, for the player's words, and a sentence
/// in plain words (the design's section 3.3 and 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No such key (or one that was never made here).
    NotRecognised,
    /// The window has not started; `from_ms` says when it does.
    NotYetValid {
        from_ms: i64,
    },
    /// The window has ended.
    Ended {
        until_ms: i64,
    },
    Paused,
    Revoked,
    /// The key does not open the show asked for.
    DoesNotOpenShow,
    TooManyDevices,
    /// A group key's people are used up.
    PeopleLimit,
    ListensUsedUp,
    /// This device was signed out.
    DeviceSignedOut,
    /// The key works, but no show it opens (or the one asked for) has a keyed window on the
    /// schedule now or in the next fortnight, so there is nothing a token could be for (1.3.2).
    NotScheduled,
}

impl Refusal {
    /// A short stable word for software (`not_yet_valid`).
    pub fn code(self) -> &'static str {
        match self {
            Refusal::NotRecognised => "not_recognised",
            Refusal::NotYetValid { .. } => "not_yet_valid",
            Refusal::Ended { .. } => "ended",
            Refusal::Paused => "paused",
            Refusal::Revoked => "revoked",
            Refusal::DoesNotOpenShow => "does_not_open_show",
            Refusal::TooManyDevices => "too_many_devices",
            Refusal::PeopleLimit => "people_limit",
            Refusal::ListensUsedUp => "listens_used_up",
            Refusal::DeviceSignedOut => "device_signed_out",
            Refusal::NotScheduled => "not_scheduled",
        }
    }

    /// What the listener is told, in a sentence. The player puts the station's name and contact
    /// beside it.
    pub fn sentence(self) -> &'static str {
        match self {
            Refusal::NotRecognised => {
                "That key is not recognised. Check it: letters and dashes are ignored, and capitals do not matter."
            }
            Refusal::NotYetValid { .. } => "This key has not started yet.",
            Refusal::Ended { .. } => "This key has ended. Ask the station for a new one.",
            Refusal::Paused => "This key is switched off for now. Ask the station.",
            Refusal::Revoked => "This key was ended by the station. Ask the station for a new one.",
            Refusal::DoesNotOpenShow => "This key does not open this show.",
            Refusal::TooManyDevices => {
                "This key is already in use on its most devices. Sign out of one."
            }
            Refusal::PeopleLimit => "This key has reached the number of people it is for.",
            Refusal::ListensUsedUp => "This key's listens are used up.",
            Refusal::DeviceSignedOut => "This device was signed out. Enter the key again.",
            Refusal::NotScheduled => {
                "This key works, but none of its shows is on the schedule yet. Come back when one is."
            }
        }
    }
}

/// What the rules need of a key at one moment: the window, the state and the shows.
pub struct View<'a> {
    pub state: State,
    pub valid_from_ms: Option<i64>,
    pub valid_until_ms: Option<i64>,
    pub all_shows: bool,
    pub shows: &'a [Uuid],
}

/// The verdict on a key at `now_ms`: whether it works at all (its state and window), and, with a
/// `show`, whether it opens that show. The limits (devices, people, listens) need the database
/// and are checked by the query that holds the key's row.
pub fn judge(v: &View<'_>, now_ms: i64, show: Option<Uuid>) -> Result<(), Refusal> {
    match v.state {
        State::Revoked => return Err(Refusal::Revoked),
        State::Paused => return Err(Refusal::Paused),
        State::Active => {}
    }
    if let Some(from_ms) = v.valid_from_ms
        && now_ms < from_ms
    {
        return Err(Refusal::NotYetValid { from_ms });
    }
    if let Some(until_ms) = v.valid_until_ms
        && now_ms >= until_ms
    {
        return Err(Refusal::Ended { until_ms });
    }
    if let Some(show) = show
        && !v.all_shows
        && !v.shows.contains(&show)
    {
        return Err(Refusal::DoesNotOpenShow);
    }
    Ok(())
}

/// A show's access as it stands at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effective {
    /// Anyone may listen.
    Public,
    /// Only a key opens it.
    Key,
}

/// A show's rule (`public` or `key`) at `now_ms`, given when it opens to everyone: from that
/// moment the show is public whatever the rule says (early access, a public teaser).
pub fn effective(rule: &str, opens_at_ms: Option<i64>, now_ms: i64) -> Effective {
    if rule != "key" {
        return Effective::Public;
    }
    match opens_at_ms {
        Some(at) if now_ms >= at => Effective::Public,
        _ => Effective::Key,
    }
}

/// How a key's use looks, for the leak report. Built from the use records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeakStats {
    pub kind: Option<Kind>,
    pub max_devices: Option<u32>,
    pub max_people: Option<u32>,
    pub listens: u32,
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
    /// Devices ever seen, and addresses ever seen (the addresses of redeems the key turned away
    /// too: a key posted in public is seen even when its limits hold).
    pub devices: u32,
    pub addresses: u32,
    /// Addresses seen in the last 24 hours and the last 7 days.
    pub addresses_24h: u32,
    pub addresses_7d: u32,
    /// The most addresses first seen for this key within one hour: a key put on social media
    /// gains many in one go.
    pub peak_new_addresses_in_an_hour: u32,
}

/// How many addresses and devices a key is expected to show without being passed on: a
/// personal key is one person on their phone, their laptop and their home; a group key is its
/// people each with the same.
pub fn allowance(s: &LeakStats) -> (u32, u32) {
    let devices_per_person = s.max_devices.unwrap_or(3).max(1);
    let people = match s.kind {
        Some(Kind::Group) => s.max_people.unwrap_or(10).max(1),
        _ => 1,
    };
    // A person has about as many addresses as devices (home, work, mobile data).
    (devices_per_person * people, devices_per_person * people)
}

/// "Most likely passed on": a score for sorting, and the reasons in words for a person.
///
/// - Each address beyond the allowance seen in the last 7 days counts 2.
/// - Each device beyond the allowance ever seen counts 5 (the key was entered on more
///   devices than it is for, even if the device limit turned some away).
/// - A burst (3 or more new addresses within one hour) counts 10 and 2 for each one beyond
///   the third: that is what a key posted in public looks like.
///
/// A key with no excess scores 0 and has no reasons. The score is a hint for staff, not a
/// verdict: a mobile network changes a person's address, which is why it is a sum and not a
/// single count, and why the report shows the reasons.
pub fn leak_score(s: &LeakStats) -> (u32, Vec<String>) {
    let (addr_allow, dev_allow) = allowance(s);
    let mut score = 0;
    let mut reasons = Vec::new();
    let over_addr = s.addresses_7d.saturating_sub(addr_allow);
    if over_addr > 0 {
        score += over_addr * 2;
        reasons.push(format!(
            "{} addresses in the last 7 days, where about {addr_allow} is usual",
            s.addresses_7d
        ));
    }
    let over_dev = s.devices.saturating_sub(dev_allow);
    if over_dev > 0 {
        score += over_dev * 5;
        reasons.push(format!(
            "{} devices have used it, where about {dev_allow} is usual",
            s.devices
        ));
    }
    if s.peak_new_addresses_in_an_hour >= 3 {
        score += 10 + 2 * (s.peak_new_addresses_in_an_hour - 3);
        reasons.push(format!(
            "{} new addresses within one hour",
            s.peak_new_addresses_in_an_hour
        ));
    }
    (score, reasons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_is_compared_without_case_or_punctuation() {
        assert_eq!(normalise_text("ketm-7qmp 2d4x"), "KETM7QMP2D4X");
        assert_eq!(normalise_text("  KETM.7QMP/2D4X\n"), "KETM7QMP2D4X");
        assert_eq!(format_text("KETM7QMP2D4X"), "KETM-7QMP-2D4X");
    }

    #[test]
    fn the_alphabet_has_no_lookalikes_and_a_text_must_be_in_it() {
        for c in b"ILO01" {
            assert!(!TEXT_ALPHABET.contains(c), "{}", *c as char);
        }
        assert_eq!(TEXT_ALPHABET.len(), 31);
        assert!(text_is_wellformed("KETM7QMP2D4X"));
        assert!(!text_is_wellformed("KETM7QMP2D4"));
        assert!(!text_is_wellformed("KETM7QMP2D4O"));
        assert!(!text_is_wellformed("KETM7QMP2D4XX"));
    }

    fn view(state: State, from: Option<i64>, until: Option<i64>) -> View<'static> {
        View {
            state,
            valid_from_ms: from,
            valid_until_ms: until,
            all_shows: true,
            shows: &[],
        }
    }

    #[test]
    fn a_key_is_judged_by_its_state_then_its_window() {
        let ok = view(State::Active, Some(100), Some(200));
        assert_eq!(judge(&ok, 100, None), Ok(()));
        assert_eq!(judge(&ok, 199, None), Ok(()));
        assert_eq!(
            judge(&ok, 99, None),
            Err(Refusal::NotYetValid { from_ms: 100 })
        );
        assert_eq!(judge(&ok, 200, None), Err(Refusal::Ended { until_ms: 200 }));
        assert_eq!(
            judge(&view(State::Paused, None, None), 5, None),
            Err(Refusal::Paused)
        );
        // Revoked wins over a window that has not started or has ended.
        assert_eq!(
            judge(&view(State::Revoked, Some(1000), None), 5, None),
            Err(Refusal::Revoked)
        );
        assert_eq!(judge(&view(State::Active, None, None), 5, None), Ok(()));
    }

    #[test]
    fn a_key_opens_the_shows_it_names_or_all_of_them() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let shows = [a];
        let v = View {
            state: State::Active,
            valid_from_ms: None,
            valid_until_ms: None,
            all_shows: false,
            shows: &shows,
        };
        assert_eq!(judge(&v, 1, Some(a)), Ok(()));
        assert_eq!(judge(&v, 1, Some(b)), Err(Refusal::DoesNotOpenShow));
        assert_eq!(judge(&v, 1, None), Ok(()));
        assert_eq!(judge(&view(State::Active, None, None), 1, Some(b)), Ok(()));
    }

    #[test]
    fn a_keyed_show_opens_to_everyone_at_its_time() {
        assert_eq!(effective("public", None, 5), Effective::Public);
        assert_eq!(effective("key", None, 5), Effective::Key);
        assert_eq!(effective("key", Some(10), 9), Effective::Key);
        assert_eq!(effective("key", Some(10), 10), Effective::Public);
        // A public rule stays public with a time set.
        assert_eq!(effective("public", Some(10), 1), Effective::Public);
    }

    #[test]
    fn every_refusal_has_a_code_and_a_sentence() {
        let all = [
            Refusal::NotRecognised,
            Refusal::NotYetValid { from_ms: 1 },
            Refusal::Ended { until_ms: 1 },
            Refusal::Paused,
            Refusal::Revoked,
            Refusal::DoesNotOpenShow,
            Refusal::TooManyDevices,
            Refusal::PeopleLimit,
            Refusal::ListensUsedUp,
            Refusal::DeviceSignedOut,
        ];
        let mut codes = std::collections::BTreeSet::new();
        for r in all {
            assert!(codes.insert(r.code()), "{}", r.code());
            assert!(r.sentence().ends_with('.'));
            // The word the listener sees is "key", never "token".
            assert!(!r.sentence().to_lowercase().contains("token"));
        }
    }

    #[test]
    fn a_quiet_key_scores_nothing() {
        let s = LeakStats {
            kind: Some(Kind::Personal),
            max_devices: Some(2),
            devices: 2,
            addresses: 3,
            addresses_7d: 2,
            addresses_24h: 1,
            ..Default::default()
        };
        assert_eq!(leak_score(&s), (0, vec![]));
    }

    #[test]
    fn a_key_on_many_addresses_in_an_hour_scores_high_with_its_reasons() {
        let s = LeakStats {
            kind: Some(Kind::Personal),
            max_devices: Some(2),
            devices: 6,
            addresses: 9,
            addresses_7d: 9,
            peak_new_addresses_in_an_hour: 5,
            ..Default::default()
        };
        let (score, reasons) = leak_score(&s);
        // 7 addresses over 2 (14) + 4 devices over 2 (20) + a burst of 5 (10 + 4).
        assert_eq!(score, 14 + 20 + 14);
        assert_eq!(reasons.len(), 3);
        assert!(reasons[2].contains("5 new addresses"));
    }

    #[test]
    fn a_group_key_is_expected_to_have_more_addresses() {
        let group = LeakStats {
            kind: Some(Kind::Group),
            max_devices: Some(2),
            max_people: Some(20),
            devices: 30,
            addresses_7d: 30,
            ..Default::default()
        };
        let personal = LeakStats {
            kind: Some(Kind::Personal),
            ..group.clone()
        };
        assert_eq!(leak_score(&group).0, 0);
        assert!(leak_score(&personal).0 > 0);
    }

    #[test]
    fn a_burst_below_three_is_not_one() {
        let s = LeakStats {
            peak_new_addresses_in_an_hour: 2,
            ..Default::default()
        };
        assert_eq!(leak_score(&s).0, 0);
    }
}
