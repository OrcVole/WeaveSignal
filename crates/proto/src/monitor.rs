// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Monitor rooms (ADR-0280, design 4.5): a host whose own studio is at the desk may open a
//! monitor link, and hear the programme without themselves through a LiveKit room of their own,
//! `<room>-monitor-<key id>`, as a caller has a booth (ADR-0193, [`crate::booth`]). Only the desk
//! publishes there, and only the link's two seats may subscribe (ADR-0118).
//!
//! The monitor link's token is a plain `listener` token for that room (it cannot publish) with no
//! recorder grant, so what the desk publishes there reaches only the seats it names. A seat is a
//! fixed identity, `monitor-<key id>-1` or `-2`: two browsers may listen at once, a third is
//! refused by the control plane, and the desk's publisher can name both seats before it publishes.

/// What every monitor room's name starts with, after the studio room's name.
const MONITOR: &str = "-monitor-";
/// The seats of a monitor link: two browsers may listen.
pub const SEATS: usize = 2;

/// The prefix of every monitor room of a studio room: `studio-monitor-`.
pub fn monitor_prefix(studio_room: &str) -> String {
    format!("{studio_room}{MONITOR}")
}

/// The monitor room of a show key: `studio-monitor-<key id>`.
pub fn monitor_room(studio_room: &str, key_id: &str) -> String {
    format!("{}{key_id}", monitor_prefix(studio_room))
}

/// The key a monitor room belongs to, if `room` is one of this studio's monitor rooms.
pub fn monitor_key<'a>(studio_room: &str, room: &'a str) -> Option<&'a str> {
    room.strip_prefix(&monitor_prefix(studio_room))
        .filter(|id| !id.is_empty())
}

/// A seat's identity in the monitor room: `monitor-<key id>-<n>` (`n` from 1).
pub fn monitor_seat(key_id: &str, n: usize) -> String {
    format!("monitor-{key_id}-{n}")
}

/// Both seats of a key's monitor room, in order.
pub fn monitor_seats(key_id: &str) -> Vec<String> {
    (1..=SEATS).map(|n| monitor_seat(key_id, n)).collect()
}

/// Whether `identity` is one of this key's seats.
pub fn is_monitor_seat(key_id: &str, identity: &str) -> bool {
    (1..=SEATS).any(|n| identity == monitor_seat(key_id, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_has_a_room_and_two_seats() {
        assert_eq!(
            monitor_room("studio", "k3yid0ab"),
            "studio-monitor-k3yid0ab"
        );
        assert_eq!(
            monitor_key("studio", "studio-monitor-k3yid0ab"),
            Some("k3yid0ab")
        );
        assert_eq!(monitor_key("studio", "studio-monitor-"), None);
        assert_eq!(monitor_key("studio", "studio-booth-k3yid0ab"), None);
        assert_eq!(monitor_key("other", "studio-monitor-k3yid0ab"), None);
        assert_eq!(
            monitor_seats("k3yid0ab"),
            vec!["monitor-k3yid0ab-1", "monitor-k3yid0ab-2"]
        );
        assert!(is_monitor_seat("k3yid0ab", "monitor-k3yid0ab-2"));
        assert!(!is_monitor_seat("k3yid0ab", "monitor-k3yid0ab-3"));
        assert!(!is_monitor_seat("k3yid0ab", "monitor-other000-1"));
    }

    #[test]
    fn a_monitor_room_is_never_a_booth_and_a_booth_never_a_monitor_room() {
        let studio = "studio";
        assert_eq!(
            crate::booth_owner(studio, &monitor_room(studio, "abc")),
            None
        );
        assert_eq!(monitor_key(studio, &crate::booth(studio, "abc")), None);
    }
}
