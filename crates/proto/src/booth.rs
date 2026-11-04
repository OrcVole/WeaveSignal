// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Booths (ADR-0193): each caller waits and talks in a LiveKit room of their own, which only
//! the desk joins beside them; everyone else is in the studio room. The control plane names a
//! caller's booth in their token, and the desk finds booths by the same rule.

use crate::Role;

/// What every booth's name starts with, after the studio room's name.
const BOOTH: &str = "-booth-";

/// The prefix of every booth of a studio room: `studio-booth-`.
pub fn booth_prefix(studio_room: &str) -> String {
    format!("{studio_room}{BOOTH}")
}

/// A caller's booth: `studio-booth-<identity>`.
pub fn booth(studio_room: &str, identity: &str) -> String {
    format!("{}{identity}", booth_prefix(studio_room))
}

/// The room a person joins: a caller their booth, anyone else the studio room.
pub fn room_for(studio_room: &str, role: Role, identity: &str) -> String {
    if role == Role::Caller {
        booth(studio_room, identity)
    } else {
        studio_room.to_owned()
    }
}

/// The caller a booth belongs to, if `room` is one of this studio's booths.
pub fn booth_owner<'a>(studio_room: &str, room: &'a str) -> Option<&'a str> {
    room.strip_prefix(&booth_prefix(studio_room))
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callers_get_a_booth_and_everyone_else_the_studio() {
        assert_eq!(
            room_for("studio", Role::Caller, "c7f3"),
            "studio-booth-c7f3"
        );
        for r in [
            Role::Presenter,
            Role::Dj,
            Role::Guest,
            Role::Screener,
            Role::Producer,
            Role::Listener,
        ] {
            assert_eq!(room_for("studio", r, "morag"), "studio");
        }
    }

    #[test]
    fn a_booth_names_its_caller() {
        assert_eq!(booth_owner("studio", "studio-booth-c7f3"), Some("c7f3"));
        assert_eq!(booth_owner("studio", "studio-booth-"), None);
        assert_eq!(booth_owner("studio", "studio"), None);
        assert_eq!(booth_owner("studio", "other-booth-c7f3"), None);
    }
}
