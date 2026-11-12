// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The room in memory: who is connected, the recovery ring of public events, the latest
//! visible messages, held messages, reaction counts and each chatter's recent posts. One lock,
//! held only for in-memory work, never across an await.
//!
//! **Fan-out.** A frame is serialised once and handed to each matching connection's bounded
//! queue with `try_send`; a connection whose queue is full is dropped from the room and its
//! writer closes the socket with 4008, so one slow browser never holds the others up
//! (Centrifugo's rule, ADR-0031). The client reconnects and recovers what it missed.
//!
//! **Recovery.** Public events carry an offset within the epoch; the ring keeps the latest
//! ones. Erasure starts a new epoch, so nothing erased can come back through recovery.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::ws::Utf8Bytes;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::rules::{Poster, Restriction};
use crate::store::Account;
use crate::wire::{Held, Msg, Position, REACTIONS, ServerFrame};

/// What a connection's writer sends.
#[derive(Debug)]
pub enum Out {
    Text(Utf8Bytes),
}

/// Who a frame goes to.
#[derive(Clone, Copy, Debug)]
pub enum Audience {
    All,
    /// Moderators and staff.
    Mods,
    /// The studio's own people only (the private lane, ADR-0040).
    Staff,
    Account(Uuid),
    /// Moderators, and one chatter (their own held message, say).
    ModsAnd(Uuid),
    StaffAnd(Uuid),
}

pub struct Conn {
    tx: mpsc::Sender<Out>,
    account: Option<Uuid>,
    moderator: bool,
    staff: bool,
    /// Set when the room dropped this connection for falling behind.
    slow: Arc<AtomicBool>,
}

/// A chatter as the room knows them while they are around.
#[derive(Clone, Debug)]
pub struct Chatter {
    pub account: Account,
    pub restriction: Option<Restriction>,
}

pub struct Room {
    pub epoch: String,
    pub offset: u64,
    ring: VecDeque<(u64, Utf8Bytes)>,
    ring_cap: usize,
    /// The latest visible room messages, oldest first.
    pub recent: VecDeque<Msg>,
    recent_cap: usize,
    pub pinned: Option<Msg>,
    conns: HashMap<u64, Conn>,
    next_conn: u64,
    pub chatters: HashMap<Uuid, Chatter>,
    pub posters: HashMap<Uuid, Poster>,
    /// Held messages by id, with their author.
    pub held: BTreeMap<i64, (Held, Uuid)>,
    reactions: HashMap<i64, Reactions>,
}

/// One message's reaction counts, and who gave which (a second press takes it back).
type Reactions = (Vec<u32>, HashSet<(Uuid, usize)>);

pub fn new_epoch() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_owned()
}

impl Room {
    pub fn new(ring_cap: usize, recent_cap: usize) -> Self {
        Room {
            epoch: new_epoch(),
            offset: 0,
            ring: VecDeque::new(),
            ring_cap,
            recent: VecDeque::new(),
            recent_cap: recent_cap.max(200),
            pinned: None,
            conns: HashMap::new(),
            next_conn: 1,
            chatters: HashMap::new(),
            posters: HashMap::new(),
            held: BTreeMap::new(),
            reactions: HashMap::new(),
        }
    }

    pub fn count(&self) -> usize {
        self.conns.len()
    }

    /// Add a connection; its queue's first frame should be the welcome, sent by the caller
    /// with [`Room::send_to`] while the lock is still held.
    pub fn register(
        &mut self,
        tx: mpsc::Sender<Out>,
        account: Option<Uuid>,
        moderator: bool,
        staff: bool,
        slow: Arc<AtomicBool>,
    ) -> u64 {
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            id,
            Conn {
                tx,
                account,
                moderator,
                staff,
                slow,
            },
        );
        id
    }

    pub fn unregister(&mut self, id: u64) {
        self.conns.remove(&id);
    }

    /// Send one frame to one connection (its welcome).
    pub fn send_to(&mut self, id: u64, frame: &ServerFrame) {
        let text = encode(frame);
        if let Some(c) = self.conns.get(&id)
            && c.tx.try_send(Out::Text(text)).is_err()
        {
            self.drop_slow(id);
        }
    }

    fn drop_slow(&mut self, id: u64) {
        if let Some(c) = self.conns.remove(&id) {
            c.slow.store(true, Ordering::Relaxed);
        }
    }

    /// Send a frame to an audience; slow connections are dropped.
    pub fn send(&mut self, to: Audience, frame: &ServerFrame) {
        let text = encode(frame);
        self.send_text(to, &text);
    }

    fn send_text(&mut self, to: Audience, text: &Utf8Bytes) {
        let mut slow = Vec::new();
        for (id, c) in &self.conns {
            let yes = match to {
                Audience::All => true,
                Audience::Mods => c.moderator,
                Audience::Staff => c.staff,
                Audience::Account(a) => c.account == Some(a),
                Audience::ModsAnd(a) => c.moderator || c.account == Some(a),
                Audience::StaffAnd(a) => c.staff || c.account == Some(a),
            };
            if yes && c.tx.try_send(Out::Text(text.clone())).is_err() {
                slow.push(*id);
            }
        }
        for id in slow {
            self.drop_slow(id);
        }
    }

    /// Publish a public event: give it the next offset, keep it for recovery, send it to all.
    pub fn publish(&mut self, make: impl FnOnce(u64) -> ServerFrame) {
        self.offset += 1;
        let frame = make(self.offset);
        let text = encode(&frame);
        self.ring.push_back((self.offset, text.clone()));
        while self.ring.len() > self.ring_cap {
            self.ring.pop_front();
        }
        self.send_text(Audience::All, &text);
    }

    /// The events after a position, if the ring still holds all of them.
    pub fn since(&self, pos: &Position) -> Option<Vec<Utf8Bytes>> {
        if pos.epoch != self.epoch || pos.offset > self.offset {
            return None;
        }
        if pos.offset == self.offset {
            return Some(Vec::new());
        }
        let first = self.ring.front().map(|(o, _)| *o)?;
        if pos.offset + 1 < first {
            return None;
        }
        Some(
            self.ring
                .iter()
                .filter(|(o, _)| *o > pos.offset)
                .map(|(_, t)| t.clone())
                .collect(),
        )
    }

    /// A message became visible in the room.
    pub fn show(&mut self, m: Msg) {
        self.recent.push_back(m.clone());
        while self.recent.len() > self.recent_cap {
            self.recent.pop_front();
        }
        self.publish(|offset| ServerFrame::Msg { offset, m });
    }

    /// Messages left the room.
    pub fn hide(&mut self, ids: &[i64]) {
        if ids.is_empty() {
            return;
        }
        self.recent.retain(|m| !ids.contains(&m.id));
        if self.pinned.as_ref().is_some_and(|p| ids.contains(&p.id)) {
            self.pinned = None;
        }
        for id in ids {
            self.reactions.remove(id);
        }
        let ids = ids.to_vec();
        self.publish(|offset| ServerFrame::Hide { offset, ids });
    }

    /// A chatter's avatar changed (ADR-0235): the messages the room still holds take it, so a
    /// page that opens later sees it, and every open page is told.
    pub fn set_avatar(&mut self, account: Uuid, avatar: Option<crate::wire::Avatar>) {
        let id = account.to_string();
        for m in self.recent.iter_mut().chain(self.pinned.iter_mut()) {
            if m.author.id == id {
                m.author.avatar = avatar.clone();
            }
        }
        for (h, who) in self.held.values_mut() {
            if *who == account {
                h.m.author.avatar = avatar.clone();
            }
        }
        self.publish(|offset| ServerFrame::Avatar {
            offset,
            account: id,
            avatar,
        });
    }

    /// Erasure: the messages go, and a new epoch starts so recovery cannot replay them.
    pub fn forget(&mut self, ids: &[i64]) {
        self.hide(ids);
        self.epoch = new_epoch();
        self.offset = 0;
        self.ring.clear();
    }

    pub fn find(&self, id: i64) -> Option<&Msg> {
        self.recent.iter().rev().find(|m| m.id == id)
    }

    pub fn update_recent(&mut self, id: i64, f: impl FnOnce(&mut Msg)) -> Option<Msg> {
        let m = self.recent.iter_mut().rev().find(|m| m.id == id)?;
        f(m);
        Some(m.clone())
    }

    /// Toggle a chatter's reaction; returns the new counts, or `None` for an unknown message
    /// or emoji.
    pub fn react(&mut self, id: i64, who: Uuid, emoji: &str) -> Option<Vec<(String, u32)>> {
        let i = REACTIONS.iter().position(|e| *e == emoji)?;
        self.find(id)?;
        let (counts, who_set) = self
            .reactions
            .entry(id)
            .or_insert_with(|| (vec![0; REACTIONS.len()], HashSet::new()));
        if who_set.remove(&(who, i)) {
            counts[i] = counts[i].saturating_sub(1);
        } else {
            who_set.insert((who, i));
            counts[i] += 1;
        }
        let out = reaction_list(counts);
        self.update_recent(id, |m| m.reactions = out.clone());
        Some(out)
    }
}

pub fn reaction_list(counts: &[u32]) -> Vec<(String, u32)> {
    REACTIONS
        .iter()
        .zip(counts)
        .filter(|(_, n)| **n > 0)
        .map(|(e, n)| ((*e).to_owned(), *n))
        .collect()
}

pub fn encode(frame: &ServerFrame) -> Utf8Bytes {
    Utf8Bytes::from(serde_json::to_string(frame).expect("frames serialise"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{MessageKind, Standing};
    use crate::wire::{Author, MsgState};

    fn msg(id: i64) -> Msg {
        Msg {
            id,
            kind: MessageKind::Chat,
            body: format!("message {id}"),
            author: Author {
                id: "a".into(),
                name: "Lesley Kerr".into(),
                standing: Standing::Member,
                guest: false,
                avatar: None,
            },
            reply_to: None,
            server_ms: 0,
            playhead_ms: None,
            state: MsgState::Visible,
            pinned: false,
            featured: false,
            reactions: vec![],
            attachment: None,
        }
    }

    fn conn(
        room: &mut Room,
        cap: usize,
        moderator: bool,
    ) -> (u64, mpsc::Receiver<Out>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel(cap);
        let slow = Arc::new(AtomicBool::new(false));
        let id = room.register(tx, None, moderator, false, slow.clone());
        (id, rx, slow)
    }

    #[test]
    fn a_reconnect_within_the_ring_recovers_what_it_missed() {
        let mut room = Room::new(10, 50);
        for i in 1..=5 {
            room.show(msg(i));
        }
        let pos = Position {
            epoch: room.epoch.clone(),
            offset: 3,
        };
        let missed = room.since(&pos).unwrap();
        assert_eq!(missed.len(), 2);
        assert!(missed[0].as_str().contains("\"offset\":4"));
        // Up to date: nothing missed.
        let now = Position {
            epoch: room.epoch.clone(),
            offset: 5,
        };
        assert_eq!(room.since(&now).unwrap().len(), 0);
    }

    #[test]
    fn a_gap_older_than_the_ring_or_another_epoch_is_not_recovered() {
        let mut room = Room::new(3, 50);
        for i in 1..=10 {
            room.show(msg(i));
        }
        let old = Position {
            epoch: room.epoch.clone(),
            offset: 2,
        };
        assert!(room.since(&old).is_none());
        let other = Position {
            epoch: "elsewhere".into(),
            offset: 9,
        };
        assert!(room.since(&other).is_none());
        let ahead = Position {
            epoch: room.epoch.clone(),
            offset: 99,
        };
        assert!(room.since(&ahead).is_none());
    }

    #[test]
    fn erasure_starts_a_new_epoch() {
        let mut room = Room::new(10, 50);
        room.show(msg(1));
        room.show(msg(2));
        let before = Position {
            epoch: room.epoch.clone(),
            offset: 1,
        };
        room.forget(&[1]);
        assert!(room.since(&before).is_none());
        assert!(room.find(1).is_none());
        assert!(room.find(2).is_some());
    }

    #[test]
    fn a_full_queue_drops_only_that_connection() {
        let mut room = Room::new(100, 50);
        let (_a, mut rx_a, slow_a) = conn(&mut room, 2, false);
        let (_b, _rx_b, slow_b) = conn(&mut room, 2, false);
        for i in 1..=3 {
            room.show(msg(i));
            // a keeps up, b never reads
            while rx_a.try_recv().is_ok() {}
        }
        assert!(!slow_a.load(Ordering::Relaxed));
        assert!(slow_b.load(Ordering::Relaxed));
        assert_eq!(room.count(), 1);
    }

    #[test]
    fn held_frames_reach_moderators_only() {
        let mut room = Room::new(100, 50);
        let (_m, mut rx_m, _) = conn(&mut room, 8, true);
        let (_v, mut rx_v, _) = conn(&mut room, 8, false);
        room.send(Audience::Mods, &ServerFrame::ModUnheld { ids: vec![1] });
        assert!(rx_m.try_recv().is_ok());
        assert!(rx_v.try_recv().is_err());
    }

    #[test]
    fn hiding_removes_from_the_snapshot_and_the_pin() {
        let mut room = Room::new(100, 50);
        room.show(msg(1));
        room.pinned = Some(msg(1));
        room.hide(&[1]);
        assert!(room.recent.is_empty());
        assert!(room.pinned.is_none());
    }

    #[test]
    fn a_reaction_toggles_per_chatter() {
        let mut room = Room::new(100, 50);
        room.show(msg(1));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(room.react(1, a, "👍").unwrap(), vec![("👍".to_owned(), 1)]);
        assert_eq!(room.react(1, b, "👍").unwrap(), vec![("👍".to_owned(), 2)]);
        assert_eq!(room.react(1, a, "👍").unwrap(), vec![("👍".to_owned(), 1)]);
        assert!(room.react(1, a, "🦀").is_none());
        assert!(room.react(9, a, "👍").is_none());
    }
}
