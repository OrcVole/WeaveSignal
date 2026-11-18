// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Roles in the room and the identities people join it with.
//!
//! A participant's role travels in their access token as the LiveKit attribute
//! [`ROLE_ATTRIBUTE`]. Tokens are signed by the control plane and grant no right to change
//! one's own attributes, so the desk can trust the role it reads from the room.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The LiveKit participant attribute that carries a participant's [`Role`].
pub const ROLE_ATTRIBUTE: &str = "weavesignal.role";

/// What a participant does in the room; the desk routes their voice by it.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Presents the show from the studio; on air from joining.
    Presenter,
    /// A remote DJ: presents like a presenter, but joins off air and is put on air by the
    /// handover (ADR-0074).
    Dj,
    /// A guest in the room; on air from joining.
    Guest,
    /// A listener calling in: off air until sent to air; hears the screener (ADR-0006).
    Caller,
    /// Screens callers: talks to callers, never to listeners; talkback to presenters.
    Screener,
    /// Produces the show: never on air; talkback to presenters.
    Producer,
    /// Only listens: a monitor, a recorder, a test's ear. Never at the desk (no mix-minus, no
    /// voice), and its token cannot publish.
    Listener,
    /// A host's own studio (ADR-0280): what their software sends, taken by the ingest with a
    /// show key, never through the room. At the desk like a person, with a slot and its keys,
    /// but standing by until a press puts it on air; no mix-minus of its own yet (the monitor
    /// link comes later), and no room token is ever made for it.
    Source,
}

impl Role {
    pub const ALL: [Role; 8] = [
        Role::Presenter,
        Role::Dj,
        Role::Guest,
        Role::Caller,
        Role::Screener,
        Role::Producer,
        Role::Listener,
        Role::Source,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Presenter => "presenter",
            Role::Dj => "dj",
            Role::Guest => "guest",
            Role::Caller => "caller",
            Role::Screener => "screener",
            Role::Producer => "producer",
            Role::Listener => "listener",
            Role::Source => "source",
        }
    }

    /// Whether the role can ever reach the programme.
    pub fn can_air(self) -> bool {
        matches!(
            self,
            Role::Presenter | Role::Dj | Role::Guest | Role::Caller | Role::Source
        )
    }

    /// Whether the participant is on air the moment they join.
    pub fn on_air_at_join(self) -> bool {
        matches!(self, Role::Presenter | Role::Guest)
    }

    /// Whether the desk takes the participant in: everyone but a listener gets a mix-minus,
    /// and their voice is mixed by the matrix.
    pub fn at_desk(self) -> bool {
        !matches!(self, Role::Listener)
    }

    /// Whether the role may publish into the room: a listener's token may not.
    pub fn may_publish(self) -> bool {
        self.at_desk() && !self.from_ingest()
    }

    /// Whether the role reaches the desk from the ingest (a host's own software) and never
    /// through the room: no room token is made for it, and the room's list never yields it.
    pub fn from_ingest(self) -> bool {
        matches!(self, Role::Source)
    }

    /// Whether the role hears talkback (from producers and screeners).
    pub fn hears_talkback(self) -> bool {
        matches!(self, Role::Presenter | Role::Dj)
    }

    /// Whether the role's ear holds the cue bus: what is cued (pre-fade listen) and the cued
    /// carts. A presenter or DJ hears it in their mix-minus; a host's own studio hears it on
    /// its monitor link (design 4.5: the programme without them, the cue bus, and talkback
    /// addressed to them). Talkback in general (`hears_talkback`) is not theirs.
    pub fn hears_cue(self) -> bool {
        self.hears_talkback() || self == Role::Source
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Role::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .ok_or_else(|| format!("`{s}` is not a role"))
    }
}

/// Why an identity cannot be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("participant identity `{0}` must be 1 to 64 characters of a-z, A-Z, 0-9, `-` or `_`")]
    Malformed(String),
    #[error("participant identity `{0}` begins with the desk's own prefix `{1}-`")]
    Reserved(String, String),
}

/// An identity is safe in GStreamer element names and clear of the desk's own prefix.
pub fn check_identity(identity: &str, desk_prefix: &str) -> Result<(), IdentityError> {
    let ok = (1..=64).contains(&identity.len())
        && identity
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(IdentityError::Malformed(identity.to_owned()));
    }
    if identity == desk_prefix || identity.starts_with(&format!("{desk_prefix}-")) {
        return Err(IdentityError::Reserved(
            identity.to_owned(),
            desk_prefix.to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_as_lowercase_words() {
        for r in Role::ALL {
            assert_eq!(r.as_str().parse::<Role>(), Ok(r));
            assert_eq!(
                serde_json::to_string(&r).unwrap(),
                format!("\"{}\"", r.as_str())
            );
        }
        assert!("Presenter".parse::<Role>().is_err());
    }

    #[test]
    fn only_presenters_and_guests_are_on_air_at_join() {
        let on: Vec<Role> = Role::ALL
            .into_iter()
            .filter(|r| r.on_air_at_join())
            .collect();
        assert_eq!(on, vec![Role::Presenter, Role::Guest]);
        assert!(!Role::Screener.can_air() && !Role::Producer.can_air());
    }

    #[test]
    fn a_listener_is_never_at_the_desk_and_never_publishes() {
        let l = Role::Listener;
        assert!(!l.at_desk() && !l.may_publish() && !l.can_air() && !l.on_air_at_join());
        assert!(
            Role::ALL
                .iter()
                .filter(|r| !r.at_desk())
                .eq([&Role::Listener])
        );
        assert_eq!("listener".parse::<Role>(), Ok(Role::Listener));
    }

    #[test]
    fn a_source_airs_but_stands_by_and_never_comes_through_the_room() {
        let s = Role::Source;
        assert!(s.can_air() && s.at_desk() && !s.on_air_at_join());
        assert!(!s.may_publish() && s.from_ingest() && !s.hears_talkback());
        assert_eq!("source".parse::<Role>(), Ok(Role::Source));
        assert_eq!(
            Role::ALL.iter().filter(|r| r.from_ingest()).count(),
            1,
            "only a source comes from the ingest"
        );
    }

    #[test]
    fn identities_are_checked() {
        assert_eq!(check_identity("guest-2", "desk"), Ok(()));
        assert!(matches!(
            check_identity("desk-mm-guest", "desk"),
            Err(IdentityError::Reserved(..))
        ));
        assert!(matches!(
            check_identity("desk", "desk"),
            Err(IdentityError::Reserved(..))
        ));
        assert!(matches!(
            check_identity("has space", "desk"),
            Err(IdentityError::Malformed(_))
        ));
        assert!(matches!(
            check_identity("", "desk"),
            Err(IdentityError::Malformed(_))
        ));
        assert!(check_identity(&"a".repeat(65), "desk").is_err());
    }
}
