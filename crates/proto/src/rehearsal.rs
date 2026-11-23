// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The rehearsal studio (ADR-0276 option B, ADR-0292; design `rehearsal-mode-build-plan.md`):
//! a second LiveKit room, `<room>-rehearsal`, mixed by a second desk that has no way to the air.
//! Its name sits outside every other prefix hung off the studio room (callers' booths, phone
//! booths, monitor rooms), both ways, so neither desk ever mistakes one for the other.
//!
//! People who rehearse and then move to the live studio arrive **standing by**: the control plane
//! puts [`STANDBY_ATTRIBUTE`] on their live token, and the live desk starts them off air while its
//! time is still ahead, instead of on air as their role would (`Role::on_air_at_join`).
//!
//! **Guests and invitations arrive standing by too** (the operator's answer to the 1.3 plan's
//! question 9, 2026-11-21): until 1.2 a guest went on air the moment their link joined, which
//! could put their "can you hear me?" into someone else's show. Every guest now stands by at the
//! desk until a press puts them on air ([`arrives_standing_by`]); any other invitation stands by
//! when its token carries the attribute ([`UNTIL_PUT_ON_AIR`] for "until a press").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Role;

/// What the rehearsal room's name adds to the studio room's.
pub const REHEARSAL_SUFFIX: &str = "-rehearsal";

/// The rehearsal desk's identity prefix by default (the live desk's is `desk`).
pub const REHEARSAL_DESK_IDENTITY: &str = "desk-rehearsal";

/// The LiveKit participant attribute that keeps a person standing by at the live desk until a
/// moment: milliseconds since the Unix epoch, as decimal digits. Set only by the control plane
/// (tokens grant no right to change one's own attributes).
pub const STANDBY_ATTRIBUTE: &str = "weavesignal.standby_until";

/// The attribute's value for "standing by until a press puts them on air", with no time when
/// it lapses by itself.
pub const UNTIL_PUT_ON_AIR: u64 = u64::MAX;

/// The rehearsal room of a studio room: `studio-rehearsal`.
pub fn rehearsal_room(studio_room: &str) -> String {
    format!("{studio_room}{REHEARSAL_SUFFIX}")
}

/// Whether `room` is this studio room's rehearsal room (and nothing else).
pub fn is_rehearsal_room(studio_room: &str, room: &str) -> bool {
    room.strip_prefix(studio_room) == Some(REHEARSAL_SUFFIX)
}

/// Whether a room's name has the rehearsal room's shape, whatever studio room it hangs off: a
/// live desk refuses to mix such a room, and a rehearsal desk refuses any other.
pub fn looks_like_rehearsal_room(room: &str) -> bool {
    room.len() > REHEARSAL_SUFFIX.len() && room.ends_with(REHEARSAL_SUFFIX)
}

/// Which studio a pass leads to.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Studio {
    /// The live studio: the room the station airs. Every pass made before 1.3 leads here.
    #[default]
    Live,
    /// The rehearsal studio: `<room>-rehearsal`, whose desk reaches no air.
    Rehearsal,
}

impl Studio {
    pub fn as_str(self) -> &'static str {
        match self {
            Studio::Live => "live",
            Studio::Rehearsal => "rehearsal",
        }
    }

    pub fn is_live(&self) -> bool {
        *self == Studio::Live
    }

    /// The LiveKit room this studio uses for a station whose studio room is `studio_room`.
    pub fn room(self, studio_room: &str) -> String {
        match self {
            Studio::Live => studio_room.to_owned(),
            Studio::Rehearsal => rehearsal_room(studio_room),
        }
    }
}

/// The standing-by time a participant's attributes carry, if any and readable.
pub fn standby_until_ms(attributes: &BTreeMap<String, String>) -> Option<u64> {
    attributes.get(STANDBY_ATTRIBUTE)?.trim().parse().ok()
}

/// Whether someone whose attributes carry `standby_until_ms` stands by at `now_ms`: only while
/// the time is still ahead. Absent, nobody stands by because of it.
pub fn stands_by(standby_until_ms: Option<u64>, now_ms: u64) -> bool {
    standby_until_ms.is_some_and(|t| t > now_ms)
}

/// Whether someone the live desk sees for the first time arrives standing by (off air although
/// their role would be on air at joining): every guest (question 9), and anyone whose token's
/// standing-by time is still ahead. Callers, DJs, screeners and producers join off air anyway.
pub fn arrives_standing_by(role: Role, standby_until_ms: Option<u64>, now_ms: u64) -> bool {
    role == Role::Guest || stands_by(standby_until_ms, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::{SIP_IDENTITY_PREFIX, phone_booth, phone_booth_prefix, phone_room_prefix};
    use crate::{booth, booth_owner, monitor_key, monitor_room, room_for};

    #[test]
    fn the_rehearsal_room_is_named_after_the_studio_room() {
        assert_eq!(rehearsal_room("studio"), "studio-rehearsal");
        assert!(is_rehearsal_room("studio", "studio-rehearsal"));
        assert!(!is_rehearsal_room("studio", "studio"));
        assert!(!is_rehearsal_room("studio", "studio-rehearsal-booth-x"));
        assert!(!is_rehearsal_room("other", "studio-rehearsal"));
        assert_eq!(Studio::Live.room("studio"), "studio");
        assert_eq!(Studio::Rehearsal.room("studio"), "studio-rehearsal");
        assert!(looks_like_rehearsal_room("studio-rehearsal"));
        assert!(!looks_like_rehearsal_room("-rehearsal"));
        assert!(!looks_like_rehearsal_room("studio"));
    }

    #[test]
    fn the_rehearsal_room_is_no_booth_phone_booth_or_monitor_room() {
        for studio in ["studio", "s", "kettle-fm"] {
            let r = rehearsal_room(studio);
            assert_eq!(booth_owner(studio, &r), None, "{r} read as a booth");
            assert_eq!(monitor_key(studio, &r), None, "{r} read as a monitor room");
            assert!(
                phone_booth(studio, &r).is_none(),
                "{r} read as a phone booth"
            );
            // LiveKit's SIP dispatch rule makes rooms `<prefix>_<…>` from this prefix: it must
            // never name the rehearsal room, nor the rehearsal room start with it.
            assert!(!r.starts_with(&phone_room_prefix(studio)));
            assert!(!r.starts_with(&phone_booth_prefix(studio)));
            assert!(!phone_room_prefix(studio).starts_with(&r));
            // Nobody's room, worked out from their role, is ever the rehearsal room.
            for role in Role::ALL {
                assert_ne!(room_for(studio, role, "gregor"), r);
            }
        }
    }

    #[test]
    fn no_booth_phone_booth_or_monitor_room_is_the_rehearsal_room() {
        let studio = "studio";
        for id in ["rehearsal", "x", "rehearsal-booth-x", "c7f3"] {
            assert!(!is_rehearsal_room(studio, &booth(studio, id)));
            assert!(!is_rehearsal_room(studio, &monitor_room(studio, id)));
            assert!(!is_rehearsal_room(
                studio,
                &format!("{}{id}", phone_booth_prefix(studio))
            ));
        }
        // The rehearsal room's own would-be booths are not the live studio's booths either.
        let rb = booth(&rehearsal_room(studio), "x");
        assert_eq!(booth_owner(studio, &rb), None);
        assert_eq!(
            monitor_key(studio, &monitor_room(&rehearsal_room(studio), "k")),
            None
        );
        assert!(
            !format!("{}{}", rehearsal_room(studio), "-phone_1")
                .starts_with(&phone_booth_prefix(studio))
        );
        assert!(!REHEARSAL_DESK_IDENTITY.starts_with(SIP_IDENTITY_PREFIX));
    }

    #[test]
    fn studios_read_and_write_as_words_and_default_to_live() {
        assert_eq!(
            serde_json::to_string(&Studio::Rehearsal).unwrap(),
            "\"rehearsal\""
        );
        assert_eq!(
            serde_json::from_str::<Studio>("\"live\"").unwrap(),
            Studio::Live
        );
        assert_eq!(Studio::default(), Studio::Live);
        assert!(serde_json::from_str::<Studio>("\"backstage\"").is_err());
    }

    #[test]
    fn standing_by_lasts_only_while_its_time_is_ahead() {
        let mut a = BTreeMap::new();
        assert_eq!(standby_until_ms(&a), None);
        assert!(!stands_by(standby_until_ms(&a), 1_000));
        a.insert(STANDBY_ATTRIBUTE.to_owned(), "2000".to_owned());
        assert_eq!(standby_until_ms(&a), Some(2_000));
        assert!(stands_by(Some(2_000), 1_999));
        assert!(!stands_by(Some(2_000), 2_000));
        assert!(!stands_by(Some(2_000), 5_000));
        a.insert(STANDBY_ATTRIBUTE.to_owned(), "soon".to_owned());
        assert_eq!(standby_until_ms(&a), None, "an unreadable time is no time");
        assert!(stands_by(Some(UNTIL_PUT_ON_AIR), u64::MAX - 1));
    }

    #[test]
    fn guests_always_and_others_by_their_token_arrive_standing_by() {
        let now = 1_000;
        // Question 9: every guest, with or without the attribute.
        assert!(arrives_standing_by(Role::Guest, None, now));
        assert!(arrives_standing_by(Role::Guest, Some(10), now));
        // A presenter goes on at joining unless the token says otherwise.
        assert!(!arrives_standing_by(Role::Presenter, None, now));
        assert!(!arrives_standing_by(Role::Presenter, Some(now), now));
        assert!(arrives_standing_by(Role::Presenter, Some(now + 1), now));
        assert!(arrives_standing_by(
            Role::Presenter,
            Some(UNTIL_PUT_ON_AIR),
            now
        ));
    }
}
