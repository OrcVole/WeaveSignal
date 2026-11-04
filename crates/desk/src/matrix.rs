// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The mix matrix: which buses exist, and through which gate each voice reaches each bus.
//!
//! Pure data, no GStreamer, so the rules are tested on their own. The rules are the ones the
//! spike's live run used (2026-10-25):
//!
//! | Voice of | Programme | Presenter, DJ | Guest, producer | Caller | Screener |
//! |---|---|---|---|---|---|
//! | presenter, DJ, guest | on air | on air or cued | on air | on air | on air |
//! | caller | on air | on air or cued | on air | on air | always |
//! | screener | never | talkback | never | talking to them | — |
//! | producer | never | talkback | never | never | never |
//!
//! Nobody hears their own voice. "On air" passes while the person is on air and not muted:
//! presenters and guests are on air from joining, callers and DJs only once sent to air.
//! "Cued" is pre-fade listen: a cued source reaches the presenters' and DJs' ears whatever its
//! fader, and never the programme. "Talkback" passes while the talker holds talk. A caller
//! waiting off air hears the programme, and the screener only while the screener talks to
//! them (the screener's `T`, held, to the selected caller; M2); only the screener hears them.
//!
//! Anyone can also talk to one person (`talk_to`, held): their voice then reaches that
//! person's ear only, and leaves the programme and every other ear while held (the desk's
//! Talk keys `A` to `H`, and `⌃T` to the other seat).

use weavesignal_proto::Role;

/// A mix the desk makes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bus {
    /// What listeners hear.
    Programme,
    /// What one participant hears: everyone they should hear, never themselves.
    MixMinus(String),
}

impl Bus {
    /// A stem for GStreamer element names; identities are checked to be safe in them.
    pub fn stem(&self) -> String {
        match self {
            Bus::Programme => "bus-programme".to_owned(),
            Bus::MixMinus(id) => format!("bus-mm-{id}"),
        }
    }
}

/// When a voice passes onto a bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Whenever the voice is there.
    Always,
    /// While the voice's owner is on air (and not muted).
    OnAir,
    /// While the voice's owner is on air, or cued (pre-fade listen, into a presenter's ear).
    OnAirOrCue,
    /// While the voice's owner holds talkback to presenters and DJs.
    Talk,
    /// Only while the voice's owner talks to this bus's owner (`talk_to`).
    TalkTo,
}

/// A voice's state at the desk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    pub on_air: bool,
    /// Muted: off every bus it airs on, without losing its on-air state.
    pub mute: bool,
    /// Cued: heard in the presenters' and DJs' ears, never by listeners.
    pub cue: bool,
    /// Holding talkback to presenters and DJs (screener, producer).
    pub talk: bool,
    /// Holding talk to one person: their ear only, off the programme while held.
    pub talk_to: Option<String>,
}

impl Switches {
    pub fn at_join(role: Role) -> Self {
        Switches {
            on_air: role.on_air_at_join(),
            ..Switches::default()
        }
    }

    /// Whether a voice passes `gate` onto the bus owned by `owner` (`None`: the programme).
    pub fn passes(&self, gate: Gate, owner: Option<&str>) -> bool {
        let private = self.talk_to.is_some();
        let aired = self.on_air && !self.mute && !private;
        let to_owner = owner.is_some() && self.talk_to.as_deref() == owner;
        to_owner
            || match gate {
                Gate::Always => true,
                Gate::OnAir => aired,
                Gate::OnAirOrCue => aired || (self.cue && !private),
                Gate::Talk => self.talk,
                Gate::TalkTo => false,
            }
    }
}

/// Where a voice can go: the programme, or one person's mix-minus.
#[derive(Clone, Copy, Debug)]
pub enum Dest<'a> {
    Programme,
    MixMinus { identity: &'a str, role: Role },
}

/// The gate between a voice and a destination, or `None` when the voice never goes there.
pub fn gate(source: &str, source_role: Role, dest: Dest<'_>) -> Option<Gate> {
    match dest {
        Dest::Programme => source_role.can_air().then_some(Gate::OnAir),
        Dest::MixMinus { identity, role } => {
            if identity == source {
                return None;
            }
            match source_role {
                Role::Presenter | Role::Dj | Role::Guest | Role::Caller
                    if role.hears_talkback() =>
                {
                    Some(Gate::OnAirOrCue)
                }
                Role::Presenter | Role::Dj | Role::Guest => Some(Gate::OnAir),
                Role::Caller if role == Role::Screener => Some(Gate::Always),
                Role::Caller => Some(Gate::OnAir),
                Role::Screener if role == Role::Caller => Some(Gate::TalkTo),
                Role::Screener | Role::Producer if role.hears_talkback() => Some(Gate::Talk),
                Role::Screener | Role::Producer => None,
                Role::Listener => None,
            }
        }
    }
}

/// The bus's owner, for [`Switches::passes`]: `None` for the programme.
pub fn owner(bus: &Bus) -> Option<&str> {
    match bus {
        Bus::Programme => None,
        Bus::MixMinus(id) => Some(id),
    }
}

/// The gate for a voice onto a bus, given the bus owner's role.
pub fn gate_to_bus(
    source: &str,
    source_role: Role,
    bus: &Bus,
    owner_role: Option<Role>,
) -> Option<Gate> {
    match (bus, owner_role) {
        (Bus::Programme, _) => gate(source, source_role, Dest::Programme),
        (Bus::MixMinus(id), Some(role)) => {
            gate(source, source_role, Dest::MixMinus { identity: id, role })
        }
        (Bus::MixMinus(_), None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(identity: &str, role: Role) -> Dest<'_> {
        Dest::MixMinus { identity, role }
    }

    const ROOM: [(&str, Role); 6] = [
        ("hamish", Role::Presenter),
        ("gregor", Role::Guest),
        ("callum", Role::Caller),
        ("moira", Role::Screener),
        ("ailsa", Role::Producer),
        ("eilidh", Role::Dj),
    ];

    #[test]
    fn nobody_hears_themselves() {
        for (id, role) in ROOM {
            assert_eq!(gate(id, role, mm(id, role)), None, "{id}");
        }
    }

    #[test]
    fn presenters_and_guests_are_heard_by_everyone_while_on_air() {
        for (src, src_role) in [ROOM[0], ROOM[1], ROOM[5]] {
            assert_eq!(gate(src, src_role, Dest::Programme), Some(Gate::OnAir));
            for (id, role) in ROOM.iter().filter(|(i, _)| *i != src) {
                let want = if role.hears_talkback() {
                    Gate::OnAirOrCue
                } else {
                    Gate::OnAir
                };
                assert_eq!(
                    gate(src, src_role, mm(id, *role)),
                    Some(want),
                    "{src} to {id}"
                );
            }
        }
    }

    #[test]
    fn a_caller_reaches_the_screener_always_and_everyone_else_on_air() {
        let (c, r) = ROOM[2];
        assert_eq!(gate(c, r, Dest::Programme), Some(Gate::OnAir));
        assert_eq!(gate(c, r, mm("moira", Role::Screener)), Some(Gate::Always));
        assert_eq!(
            gate(c, r, mm("hamish", Role::Presenter)),
            Some(Gate::OnAirOrCue)
        );
        assert_eq!(gate(c, r, mm("other", Role::Caller)), Some(Gate::OnAir));
    }

    #[test]
    fn the_screener_talks_to_callers_and_by_talkback_to_presenters_never_to_listeners() {
        let (s, r) = ROOM[3];
        assert_eq!(gate(s, r, Dest::Programme), None);
        assert_eq!(gate(s, r, mm("callum", Role::Caller)), Some(Gate::TalkTo));
        assert_eq!(gate(s, r, mm("hamish", Role::Presenter)), Some(Gate::Talk));
        assert_eq!(gate(s, r, mm("eilidh", Role::Dj)), Some(Gate::Talk));
        assert_eq!(gate(s, r, mm("gregor", Role::Guest)), None);
        assert_eq!(gate(s, r, mm("ailsa", Role::Producer)), None);
    }

    #[test]
    fn the_producer_is_heard_only_by_presenters_on_talkback() {
        let (p, r) = ROOM[4];
        assert_eq!(gate(p, r, Dest::Programme), None);
        for (id, role) in ROOM.iter().filter(|(i, _)| *i != p) {
            let want = role.hears_talkback().then_some(Gate::Talk);
            assert_eq!(gate(p, r, mm(id, *role)), want, "{id}");
        }
    }

    #[test]
    fn switches_start_from_the_role() {
        assert!(Switches::at_join(Role::Presenter).on_air);
        assert!(Switches::at_join(Role::Guest).on_air);
        assert!(!Switches::at_join(Role::Caller).on_air);
        assert!(!Switches::at_join(Role::Dj).on_air);
        let s = Switches {
            talk: true,
            ..Switches::default()
        };
        assert!(
            s.passes(Gate::Always, None)
                && s.passes(Gate::Talk, Some("h"))
                && !s.passes(Gate::OnAir, None)
        );
    }

    #[test]
    fn mute_closes_the_air_cue_opens_the_presenters_ear_only() {
        let on = Switches::at_join(Role::Guest);
        let muted = Switches {
            mute: true,
            ..on.clone()
        };
        assert!(on.passes(Gate::OnAir, None) && on.passes(Gate::OnAirOrCue, Some("h")));
        assert!(!muted.passes(Gate::OnAir, None) && !muted.passes(Gate::OnAirOrCue, Some("h")));
        assert!(
            muted.passes(Gate::Always, Some("moira")),
            "mute is not a drop"
        );
        let cued = Switches {
            cue: true,
            ..Switches::default()
        };
        assert!(cued.passes(Gate::OnAirOrCue, Some("h")));
        assert!(!cued.passes(Gate::OnAir, None) && !cued.passes(Gate::OnAir, Some("g")));
    }

    #[test]
    fn talking_to_one_person_reaches_their_ear_only() {
        let talking = Switches {
            talk_to: Some("callum".into()),
            ..Switches::at_join(Role::Presenter)
        };
        assert!(talking.passes(Gate::OnAir, Some("callum")));
        assert!(talking.passes(Gate::TalkTo, Some("callum")));
        assert!(
            !talking.passes(Gate::OnAir, None),
            "private talk left the programme open"
        );
        assert!(!talking.passes(Gate::OnAir, Some("gregor")));
        assert!(!talking.passes(Gate::TalkTo, Some("iona")));
        assert!(!Switches::default().passes(Gate::TalkTo, Some("callum")));
    }

    #[test]
    fn bus_stems_are_distinct() {
        assert_eq!(Bus::Programme.stem(), "bus-programme");
        assert_eq!(Bus::MixMinus("guest".into()).stem(), "bus-mm-guest");
        assert_eq!(
            gate_to_bus("x", Role::Guest, &Bus::MixMinus("y".into()), None),
            None
        );
    }
}
