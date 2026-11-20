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
//! Talk keys `A` to `H`, and `⌃T` to the other seat). While held, the talker also hears that
//! person back, whatever their gate, muted, standing by or waiting ([`reaches`]), on every Talk
//! key, not only the Screener tab's: a presenter talking to a caller waiting off
//! air, from the Screener tab with no screener in the room, hears the answer in their own ear
//! only, and the caller stays off the programme and out of the cue (issue 25).

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
                // A host's own studio is heard as a DJ is: by everyone while on air, and in the
                // presenters' and DJs' ears on cue (ADR-0280).
                Role::Presenter | Role::Dj | Role::Guest | Role::Caller | Role::Source
                    if role.hears_cue() =>
                {
                    Some(Gate::OnAirOrCue)
                }
                Role::Presenter | Role::Dj | Role::Guest | Role::Source => Some(Gate::OnAir),
                Role::Caller if role == Role::Screener => Some(Gate::Always),
                Role::Caller => Some(Gate::OnAir),
                Role::Screener if role == Role::Caller => Some(Gate::TalkTo),
                // A host's monitor link hears talk addressed to it, and nothing else of the
                // staff's talkback (design 4.5).
                Role::Screener | Role::Producer if role == Role::Source => Some(Gate::TalkTo),
                Role::Screener | Role::Producer if role.hears_talkback() => Some(Gate::Talk),
                Role::Screener | Role::Producer => None,
                Role::Listener => None,
            }
        }
    }
}

/// Whether a voice reaches a bus through `gate` (from [`gate_to_bus`]): its own switches pass,
/// or the bus's owner is talking to it and it is not talking privately to someone else, so a
/// talker hears the person they talk to. Only a voice with some way to that bus is heard back:
/// a voice the matrix never sends there (`None`) stays out.
pub fn reaches(
    voice: &str,
    switches: &Switches,
    gate: Option<Gate>,
    bus: &Bus,
    owner_talks_to: Option<&str>,
) -> bool {
    let owner = owner(bus);
    gate.is_some_and(|g| {
        switches.passes(g, owner)
            || (owner_talks_to == Some(voice)
                && switches.talk_to.as_deref().is_none_or(|t| Some(t) == owner))
    })
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
    fn a_source_is_heard_as_a_dj_is_and_stands_by_at_joining() {
        let src = ("source-abcd2345", Role::Source);
        assert_eq!(gate(src.0, src.1, Dest::Programme), Some(Gate::OnAir));
        for (id, role) in ROOM {
            let want = if role.hears_talkback() {
                Gate::OnAirOrCue
            } else {
                Gate::OnAir
            };
            assert_eq!(gate(src.0, src.1, mm(id, role)), Some(want), "{id}");
        }
        assert!(!Switches::at_join(Role::Source).on_air);
    }

    #[test]
    fn a_hosts_monitor_hears_the_programme_without_them_the_cue_bus_and_talk_to_them() {
        // Design 4.5: the mix-minus of a host's own studio, on its monitor link.
        let me = ("source-abcd2345", Role::Source);
        let mine = |src: &str, src_role: Role| gate(src, src_role, mm(me.0, me.1));
        // Everyone who airs is heard while on air, and cued voices on the cue bus.
        for (id, role) in [ROOM[0], ROOM[1], ROOM[2], ROOM[5]] {
            assert_eq!(mine(id, role), Some(Gate::OnAirOrCue), "{id}");
        }
        // Another host's own studio is heard as a DJ is.
        assert_eq!(
            mine("source-other000", Role::Source),
            Some(Gate::OnAirOrCue)
        );
        // Never themselves.
        assert_eq!(mine(me.0, me.1), None);
        // The staff's talkback is for presenters; a host hears it only addressed to them.
        assert_eq!(mine("moira", Role::Screener), Some(Gate::TalkTo));
        assert_eq!(mine("ailsa", Role::Producer), Some(Gate::TalkTo));
        let held = Switches {
            talk: true,
            ..Switches::default()
        };
        assert!(!held.passes(Gate::TalkTo, Some(me.0)), "general talkback");
        let to_me = Switches {
            talk_to: Some(me.0.to_owned()),
            ..Switches::default()
        };
        assert!(to_me.passes(Gate::TalkTo, Some(me.0)), "addressed to them");
        // Cued, off air: in their ear, never in the programme.
        let cued = Switches {
            cue: true,
            ..Switches::default()
        };
        assert!(cued.passes(Gate::OnAirOrCue, Some(me.0)));
        assert!(!cued.passes(Gate::OnAir, None));
        // The cue bus is theirs, as it is a presenter's; talkback in general is not.
        assert!(me.1.hears_cue() && !me.1.hears_talkback());
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
    fn a_presenter_talking_to_a_waiting_caller_hears_them_back_alone() {
        let presenter = Switches {
            talk_to: Some("callum".into()),
            ..Switches::at_join(Role::Presenter)
        };
        let waiting = Switches::at_join(Role::Caller);
        let to = |id: &str, role| {
            gate_to_bus(
                "callum",
                Role::Caller,
                &Bus::MixMinus(id.into()),
                Some(role),
            )
        };
        let hamish = Bus::MixMinus("hamish".into());
        // The presenter's voice reaches the waiting caller already (talk_to's own rule)...
        assert!(reaches(
            "hamish",
            &presenter,
            gate_to_bus(
                "hamish",
                Role::Presenter,
                &Bus::MixMinus("callum".into()),
                Some(Role::Caller)
            ),
            &Bus::MixMinus("callum".into()),
            None,
        ));
        // ...and while it is held the caller's answer reaches the presenter's ear,
        assert!(!waiting.passes(to("hamish", Role::Presenter).unwrap(), Some("hamish")));
        assert!(reaches(
            "callum",
            &waiting,
            to("hamish", Role::Presenter),
            &hamish,
            Some("callum")
        ));
        // but nobody else's, nor the programme, and not once the key is let go.
        assert!(!reaches(
            "callum",
            &waiting,
            to("eilidh", Role::Dj),
            &Bus::MixMinus("eilidh".into()),
            None
        ));
        assert!(!reaches(
            "callum",
            &waiting,
            to("gregor", Role::Guest),
            &Bus::MixMinus("gregor".into()),
            None
        ));
        assert!(!reaches(
            "callum",
            &waiting,
            gate_to_bus("callum", Role::Caller, &Bus::Programme, None),
            &Bus::Programme,
            None
        ));
        assert!(!reaches(
            "callum",
            &waiting,
            to("hamish", Role::Presenter),
            &hamish,
            None
        ));
        // A voice talking privately to someone else is not overheard by whoever talks to it.
        let screener = Switches {
            talk_to: Some("callum".into()),
            ..Switches::at_join(Role::Screener)
        };
        let s_to_h = gate_to_bus("moira", Role::Screener, &hamish, Some(Role::Presenter));
        assert!(!reaches("moira", &screener, s_to_h, &hamish, Some("moira")));
        // A voice the matrix never sends to that bus stays out, talked to or not.
        assert!(!reaches(
            "ailsa",
            &Switches::at_join(Role::Producer),
            gate_to_bus(
                "ailsa",
                Role::Producer,
                &Bus::MixMinus("gregor".into()),
                Some(Role::Guest)
            ),
            &Bus::MixMinus("gregor".into()),
            Some("ailsa")
        ));
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
