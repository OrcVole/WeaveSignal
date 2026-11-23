// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Who was on air, kept across a restart of the live desk (1.3).
//!
//! Since 1.3 guests, and everyone who joins by an invitation, arrive at the live desk standing
//! by until a press (`weavesignal_proto::rehearsal`). A desk that starts again sees everyone for
//! the first time, so without this a restart would leave the guest and the presenter who were on
//! air standing by, and the programme with nobody in it. The control plane keeps the set of
//! people on air from each status, notices a new desk by its `started_ms` (or, from a desk
//! before 1.3 that does not say, by its link coming back after it stopped answering), and puts
//! back on air those who were on air before, once each, as the new desk finds them present.
//!
//! Only guests, presenters and DJs are put back: callers go on air only by a press with their
//! consent on file, and hosts in their own studios come back through the ingest and the post's
//! rule. Someone the new desk already has on air, or who is not back within
//! [`PUT_BACK_WITHIN_MS`], is left as they are.

use std::collections::{BTreeMap, BTreeSet};

use weavesignal_proto::Role;

use crate::desk::DeskStatus;

/// How long after a restart the people it took off air are looked for.
pub const PUT_BACK_WITHIN_MS: u64 = 60_000;

/// Whether someone of this role is put back on air after a restart.
fn kept(role: Role) -> bool {
    matches!(role, Role::Guest | Role::Presenter | Role::Dj)
}

/// The record, held by the hub.
#[derive(Debug, Default)]
pub struct AirKept {
    /// The desk's `started_ms` at its last answer.
    started_ms: Option<u64>,
    /// Whether the desk has answered at all since this process started.
    seen: bool,
    /// The desk stopped answering after its last answer.
    lost: bool,
    /// Who was on air at the desk's last answer (guests, presenters, DJs).
    on_air: BTreeSet<String>,
    /// Who a restart took off air, and until when they are looked for.
    put_back: BTreeMap<String, u64>,
}

impl AirKept {
    /// The desk did not answer.
    pub fn lost(&mut self) {
        self.lost = true;
    }

    /// Fold in the desk's answer; returns who to put back on air now.
    pub fn take(&mut self, st: &DeskStatus, now: u64) -> Vec<String> {
        let restarted = self.seen
            && match (self.started_ms, st.started_ms) {
                (Some(was), Some(is)) => was != is,
                // A desk before 1.3 does not say: its link lost and back is read as a restart,
                // which at worst puts back someone a restart took off.
                (None, None) => self.lost,
                _ => false,
            };
        if restarted {
            tracing::warn!(
                on_air = self.on_air.len(),
                "the live desk started again: the people it had on air are put back as it finds them"
            );
            for id in std::mem::take(&mut self.on_air) {
                self.put_back.insert(id, now + PUT_BACK_WITHIN_MS);
            }
        }
        self.seen = true;
        self.lost = false;
        self.started_ms = st.started_ms;
        self.put_back.retain(|_, until| now <= *until);
        let mut now_put = Vec::new();
        self.put_back.retain(|id, _| {
            match st.people.iter().find(|p| &p.identity == id) {
                // On air already (a press, or a desk from before 1.3 by their role): done.
                Some(p) if p.on_air => false,
                Some(p) if p.present && kept(p.role) => {
                    now_put.push(id.clone());
                    false
                }
                // Not back yet: looked for again at the next answer.
                _ => true,
            }
        });
        self.on_air = st
            .people
            .iter()
            .filter(|p| p.on_air && kept(p.role))
            .map(|p| p.identity.clone())
            .collect();
        now_put
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::DeskPerson;

    fn person(id: &str, role: Role, on_air: bool, present: bool) -> DeskPerson {
        DeskPerson {
            identity: id.into(),
            name: String::new(),
            role,
            on_air,
            mute: false,
            cue: false,
            talk: false,
            talk_to: None,
            present,
            away_ms: None,
            linked: present,
            lufs: None,
            lufs_peak: None,
            source: None,
        }
    }

    fn status(started: Option<u64>, people: Vec<DeskPerson>) -> DeskStatus {
        DeskStatus {
            people,
            started_ms: started,
            ..DeskStatus::default()
        }
    }

    #[test]
    fn a_restart_puts_back_the_guest_and_the_presenter_who_were_on_air() {
        let mut k = AirKept::default();
        let before = vec![
            person("eilidh", Role::Presenter, true, true),
            person("gregor", Role::Guest, true, true),
            person("kirsty", Role::Guest, false, true),
            person("c7f3", Role::Caller, true, true),
        ];
        assert!(k.take(&status(Some(1), before.clone()), 1_000).is_empty());
        assert!(k.take(&status(Some(1), before), 2_000).is_empty());
        // The desk starts again: nobody back yet.
        assert!(k.take(&status(Some(9), vec![]), 3_000).is_empty());
        // Eilidh is back standing by; Gregor not yet; Kirsty was off air; the caller is never
        // put back by this.
        let back = vec![
            person("eilidh", Role::Presenter, false, true),
            person("kirsty", Role::Guest, false, true),
            person("c7f3", Role::Caller, false, true),
        ];
        assert_eq!(k.take(&status(Some(9), back), 4_000), ["eilidh"]);
        let more = vec![
            person("eilidh", Role::Presenter, true, true),
            person("gregor", Role::Guest, false, true),
        ];
        assert_eq!(k.take(&status(Some(9), more.clone()), 5_000), ["gregor"]);
        // Once each.
        assert!(k.take(&status(Some(9), more), 6_000).is_empty());
    }

    #[test]
    fn nobody_is_put_back_without_a_restart_or_after_the_wait() {
        let mut k = AirKept::default();
        let on = vec![person("gregor", Role::Guest, true, true)];
        assert!(k.take(&status(Some(1), on), 0).is_empty());
        // Taken off by a press: the same desk, so nothing.
        let off = vec![person("gregor", Role::Guest, false, true)];
        assert!(k.take(&status(Some(1), off), 1_000).is_empty());

        let mut k = AirKept::default();
        let on = vec![person("gregor", Role::Guest, true, true)];
        k.take(&status(Some(1), on), 0);
        k.take(&status(Some(2), vec![]), 1_000);
        // Back after the wait: left standing by.
        let late = vec![person("gregor", Role::Guest, false, true)];
        assert!(
            k.take(&status(Some(2), late), 1_000 + PUT_BACK_WITHIN_MS + 1)
                .is_empty()
        );

        // A control plane that starts beside a running desk puts nobody on.
        let mut k = AirKept::default();
        let off = vec![person("gregor", Role::Guest, false, true)];
        assert!(k.take(&status(Some(5), off), 0).is_empty());
    }

    #[test]
    fn present_only_and_a_second_restart_keeps_who_is_still_owed() {
        let mut k = AirKept::default();
        let on = vec![person("gregor", Role::Guest, true, true)];
        k.take(&status(Some(1), on), 0);
        k.take(&status(Some(2), vec![]), 1_000);
        // Listed but their link not up: waited for.
        let away = vec![person("gregor", Role::Guest, false, false)];
        assert!(k.take(&status(Some(2), away), 2_000).is_empty());
        // The desk starts again before they are back: still owed.
        k.lost();
        assert!(k.take(&status(Some(3), vec![]), 3_000).is_empty());
        let here = vec![person("gregor", Role::Guest, false, true)];
        assert_eq!(k.take(&status(Some(3), here), 4_000), ["gregor"]);
    }

    #[test]
    fn a_desk_that_does_not_say_when_it_started_is_read_by_its_link() {
        let mut k = AirKept::default();
        let on = vec![person("gregor", Role::Guest, true, true)];
        k.take(&status(None, on.clone()), 0);
        // Answering throughout: no restart.
        let off = vec![person("gregor", Role::Guest, false, true)];
        assert!(k.take(&status(None, on), 500).is_empty());
        k.lost();
        assert_eq!(k.take(&status(None, off), 2_000), ["gregor"]);
    }
}
