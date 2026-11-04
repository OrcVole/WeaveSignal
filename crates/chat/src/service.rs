// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat service: the room, the database and the rules together. Posting, the hold and its
//! release, moderation with its log and statements of reasons, reports and notices, the
//! private lane to the studio, sign-in, export and erasure.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tokio::sync::watch;
use uuid::Uuid;
use weavesignal_model::Station;

use crate::config::{Config, StationSettings};
use crate::control::Studio;
use crate::key::EmailKey;
use crate::legal::{self, Outcome, Reason};
use crate::room::{Audience, Chatter, Room, reaction_list};
use crate::rules::{
    self, Draft, FilterHit, Hold, Kind, MessageKind, Modes, Poster, Restriction, Standing, Verdict,
};
use crate::store::{self, Account, Db, NewAction};
use crate::wire::{
    AccountRef, AckState, Author, Contact, Held, Item, ItemKind, Me, ModSnapshot, Msg, MsgState,
    ReplyRef, RoomRules, ServerFrame, Statement, StudioMsg,
};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A refusal the HTTP routes turn into a status and a sentence.
#[derive(Debug, thiserror::Error)]
pub enum Problem {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("the chat's database did not answer")]
    Db(#[from] sqlx::Error),
}

/// Who acts: the studio (the admin bearer or a staff session) or a moderator's account.
#[derive(Clone, Debug)]
pub struct Actor {
    pub name: String,
    pub account: Option<Uuid>,
    pub staff: bool,
}

/// Studio messages' fate, kept in memory (the lane is live, not a record of its own).
#[derive(Clone, Debug, Default)]
struct LaneNote {
    passed_ms: Option<i64>,
    reply: Option<String>,
    invited_ms: Option<i64>,
}

pub struct Chat {
    pub station: Station,
    pub db: Db,
    pub config: Config,
    /// The key that keeps email addresses and sign-in codes unreadable in the database
    /// (ADR-0125).
    pub email_key: EmailKey,
    pub room: Mutex<Room>,
    pub settings: RwLock<StationSettings>,
    /// The station's retention for the chat, from its Jurisdiction setting.
    retention: RwLock<crate::config::Retention>,
    /// The chat's minimum age under the station's jurisdiction (ADR-0148), read with the
    /// retention setting.
    jurisdiction_age: std::sync::atomic::AtomicU8,
    pub studio: watch::Receiver<Studio>,
    next_id: AtomicI64,
    lane: Mutex<std::collections::HashMap<i64, LaneNote>>,
    /// Reactions per chatter in the last ten seconds (ADR-0120), in memory only.
    reactions: crate::limits::Window<Uuid>,
}

impl Chat {
    /// Forget the reaction counts whose window has passed.
    pub fn forget_old_limits(&self, now_ms: i64) {
        self.reactions.forget_old(now_ms);
    }
}

/// The time-out the screener's `O` gives (the design: "Time out 10 min").
pub const TIMEOUT_MINUTES: u32 = 10;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModCommand {
    Hide {
        id: i64,
        reason: Reason,
        #[serde(default)]
        note: String,
        #[serde(default)]
        then: Then,
        #[serde(default)]
        automated_disclosed: bool,
        #[serde(default)]
        report: Option<Uuid>,
    },
    Release {
        id: i64,
    },
    ReleaseAll,
    HideAllHeld {
        reason: Reason,
    },
    Timeout {
        account: Uuid,
        minutes: u32,
        reason: Reason,
        #[serde(default)]
        note: String,
        #[serde(default)]
        report: Option<Uuid>,
    },
    Ban {
        account: Uuid,
        reason: Reason,
        #[serde(default)]
        note: String,
        #[serde(default)]
        report: Option<Uuid>,
    },
    Lift {
        account: Uuid,
    },
    Dismiss {
        report: Uuid,
        #[serde(default)]
        note: String,
    },
    Acknowledge {
        report: Uuid,
    },
    Escalate {
        report: Uuid,
        #[serde(default)]
        note: String,
    },
    ReleaseHold {
        hold: Uuid,
    },
    Pin {
        id: i64,
    },
    Unpin,
    Feature {
        id: i64,
    },
    Post {
        body: String,
        #[serde(default)]
        kind: Option<MessageKind>,
        #[serde(default)]
        pin: bool,
    },
    Standing {
        account: Uuid,
        standing: Standing,
    },
    Reply {
        id: i64,
        body: String,
    },
    Pass {
        id: i64,
    },
    Invite {
        id: i64,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    #[default]
    None,
    Timeout,
    Ban,
}

/// What a `post` frame asked for.
pub struct PostRequest {
    pub nonce: String,
    pub kind: MessageKind,
    pub body: String,
    pub reply_to: Option<i64>,
    pub playhead_ms: Option<i64>,
}

fn ack(nonce: &str, id: Option<i64>, state: AckState) -> ServerFrame {
    ServerFrame::Ack {
        nonce: nonce.to_owned(),
        id,
        state,
        code: None,
        words: None,
        retry_ms: None,
    }
}

fn refused(nonce: &str, code: &str, words: String, retry_ms: Option<u64>) -> ServerFrame {
    ServerFrame::Ack {
        nonce: nonce.to_owned(),
        id: None,
        state: AckState::Refused,
        code: Some(code.to_owned()),
        words: Some(words),
        retry_ms,
    }
}

pub fn author(a: &Account) -> Author {
    Author {
        id: a.id.to_string(),
        name: a.name.clone(),
        standing: a.standing,
        guest: a.kind == Kind::Guest,
    }
}

impl Chat {
    pub async fn new(
        station: Station,
        db: Db,
        config: Config,
        email_key: EmailKey,
        studio: watch::Receiver<Studio>,
    ) -> Result<Arc<Self>, Problem> {
        let settings = store::load_settings(&db, station.id, &config.defaults).await?;
        let next = store::next_message_id(&db, station.id).await?;
        let mut room = Room::new(config.limits.recovery_events, config.limits.history);
        for r in store::history(&db, station.id, 200).await? {
            room.recent.push_back(msg_of(&r, None));
        }
        room.pinned = store::pinned(&db, station.id)
            .await?
            .map(|r| msg_of(&r, None));
        let reactions = crate::limits::Window::new(config.limits.reactions_per_chatter, 10_000);
        let chat = Arc::new(Chat {
            station,
            db,
            config,
            email_key,
            room: Mutex::new(room),
            settings: RwLock::new(settings),
            retention: RwLock::new(crate::config::Retention::default()),
            jurisdiction_age: std::sync::atomic::AtomicU8::new(13),
            studio,
            next_id: AtomicI64::new(next),
            lane: Mutex::new(Default::default()),
            reactions,
        });
        chat.reload_retention().await?;
        // Messages held when the service stopped wait for a moderator now: their countdown
        // died with the old process.
        for r in store::held_messages(&chat.db, chat.station.id).await? {
            if let Some(a) = &r.account {
                let h = Held {
                    m: msg_of(&r, None),
                    release_at_ms: None,
                    why: "Held before the chat restarted.".into(),
                    flagged: r.flagged.clone(),
                    account_since_ms: a.created_ms,
                };
                chat.room.lock().expect("room").held.insert(r.id, (h, a.id));
            }
        }
        Ok(chat)
    }

    pub fn retention(&self) -> crate::config::Retention {
        self.retention.read().expect("retention").clone()
    }

    /// Read the station's retention setting again (ADR-0113): No retention when it has none.
    pub async fn reload_retention(&self) -> Result<crate::config::Retention, Problem> {
        let setting = weavesignal_model::retention::load_setting(&self.db, self.station.id).await?;
        let r = crate::config::Retention::from_periods(&setting.periods());
        *self.retention.write().expect("retention") = r.clone();
        self.jurisdiction_age
            .store(setting.chat_min_age(), std::sync::atomic::Ordering::Relaxed);
        Ok(r)
    }

    /// The age a chatter must declare (ADR-0148): the station's own, else its jurisdiction's.
    pub fn min_age(&self) -> u8 {
        self.settings().min_age.unwrap_or_else(|| {
            self.jurisdiction_age
                .load(std::sync::atomic::Ordering::Relaxed)
        })
    }

    /// As [`Self::min_age`], with the jurisdiction read again first, for a sign-up: a station
    /// that has just changed its jurisdiction is not held to the old age for ten minutes. A
    /// database that does not answer leaves the last age read.
    pub async fn min_age_now(&self) -> u8 {
        if self.settings().min_age.is_none()
            && let Err(e) = self.reload_retention().await
        {
            tracing::warn!(error = %e, "cannot read the retention setting; the last minimum age read stands");
        }
        self.min_age()
    }

    fn room(&self) -> std::sync::MutexGuard<'_, Room> {
        self.room.lock().expect("the room's lock")
    }

    pub fn settings(&self) -> StationSettings {
        self.settings.read().expect("settings").clone()
    }

    /// The room's modes now: the control plane's (or the defaults when it is not linked) and
    /// the station's own two settings.
    pub fn modes(&self) -> Modes {
        let s = self.studio.borrow();
        let st = self.settings.read().expect("settings");
        let mut m = if s.linked {
            Modes {
                slow: s.slow,
                hold_new: s.hold_new,
                links_trusted: s.links_trusted,
                accounts_only: s.accounts_only,
                min_age_24h: s.min_age_24h,
                influx: s.influx,
                ..Modes::default()
            }
        } else {
            Modes::default()
        };
        m.comments_only = st.comments_only;
        m.guests = st.guests;
        m
    }

    pub fn rules(&self) -> RoomRules {
        let modes = self.modes();
        RoomRules {
            min_age_minutes: modes.min_age_minutes(),
            modes,
            slow_s: rules::SLOW_S,
            max_chars: rules::MAX_CHARS,
            hold_ms: rules::HOLD_MS,
        }
    }

    pub fn show(&self) -> String {
        self.studio.borrow().show.clone()
    }

    pub fn listener_delay_ms(&self) -> u64 {
        let s = self.studio.borrow();
        if s.linked {
            s.listener_delay_ms
        } else {
            15_000
        }
    }

    /// Tell every page the rules changed (a mode, the show, a setting).
    pub fn announce_rules(&self) {
        let f = ServerFrame::Rules {
            rules: self.rules(),
            show: self.show(),
            listener_delay_ms: self.listener_delay_ms(),
        };
        self.room().send(Audience::All, &f);
    }

    // ---- who ----

    /// Load a chatter into the room's memory (their restriction too); returns what the page
    /// shows them about themselves.
    pub async fn load_chatter(&self, a: &Account) -> Result<Me, Problem> {
        let restriction = store::restriction(&self.db, self.station.id, a.id).await?;
        let me = me_of(a, restriction);
        self.room().chatters.insert(
            a.id,
            Chatter {
                account: a.clone(),
                restriction,
            },
        );
        Ok(me)
    }

    pub async fn statements_of(&self, account: Uuid) -> Result<Vec<Statement>, Problem> {
        Ok(store::statements(&self.db, self.station.id, account)
            .await?
            .into_iter()
            .map(|s| Statement {
                id: s.0.to_string(),
                at_ms: s.1,
                text: s.2,
                automated: s.3,
                review_by_ms: s.4,
                review_requested: s.5,
            })
            .collect())
    }

    // ---- posting ----

    pub async fn post(self: &Arc<Self>, who: Uuid, p: PostRequest) -> ServerFrame {
        let now = now_ms();
        let settings = self.settings();
        let modes = self.modes();
        let body = p.body.trim().to_owned();
        // The word filter runs before the lock; the rules call it only for a message they
        // would otherwise accept.
        let hit = if rules::station_word(&body, &settings.blocked_words) {
            Some(FilterHit::StationWord)
        } else if rules::rustrict_flags(&body) {
            Some(FilterHit::Rustrict)
        } else {
            None
        };
        let (verdict, account, reply, id) = {
            let mut room = self.room();
            let Some(ch) = room.chatters.get(&who).cloned() else {
                return refused(
                    &p.nonce,
                    "signed_out",
                    "Sign in again to post.".into(),
                    None,
                );
            };
            let reply = match p.reply_to {
                None => None,
                Some(rid) => match room.find(rid) {
                    Some(m) => Some(ReplyRef {
                        id: m.id,
                        name: m.author.name.clone(),
                    }),
                    None => {
                        return refused(
                            &p.nonce,
                            "unknown_reply",
                            "That message is no longer in the room.".into(),
                            None,
                        );
                    }
                },
            };
            let poster = room.posters.entry(who).or_insert_with(|| Poster {
                kind: ch.account.kind,
                standing: ch.account.standing,
                created_ms: ch.account.created_ms.max(0) as u64,
                restriction: None,
                recent: Default::default(),
            });
            poster.standing = ch.account.standing;
            poster.restriction = ch.restriction;
            let draft = Draft {
                kind: p.kind,
                body: &body,
                reply: reply.is_some(),
            };
            let verdict = rules::judge(now as u64, &modes, poster, &draft, |_| hit.clone());
            if !matches!(verdict, Verdict::Refuse(_)) {
                poster.remember(now as u64, rules::words_hash(&body));
            }
            let id = match verdict {
                Verdict::Refuse(_) => 0,
                _ => self.next_id.fetch_add(1, Ordering::Relaxed),
            };
            (verdict, ch.account, reply, id)
        };
        let hold = match verdict {
            Verdict::Refuse(r) => return refused(&p.nonce, r.code(), r.words(), r.retry_ms()),
            Verdict::Hold(h) => Some(h),
            Verdict::Post => None,
        };
        // A sender's playhead from the future or more than an hour back is not a playhead.
        let playhead = p
            .playhead_ms
            .filter(|ph| (now - 3_600_000..=now + 5_000).contains(ph));
        let flagged = match &hold {
            Some(Hold::Flagged(hit)) => Some(hit.as_str()),
            _ => None,
        };
        if let Err(e) = store::insert_message(
            &self.db,
            self.station.id,
            &store::NewMessage {
                id,
                account: who,
                kind: p.kind,
                body: &body,
                reply_to: reply.as_ref().map(|r| r.id),
                server_ms: now,
                playhead_ms: playhead,
                held: hold.is_some(),
                flagged,
            },
        )
        .await
        {
            tracing::error!(error = %e, "a message could not be stored");
            return refused(
                &p.nonce,
                "unavailable",
                "The chat could not take that just now. Try again.".into(),
                Some(2_000),
            );
        }
        let m = Msg {
            id,
            kind: p.kind,
            body,
            author: author(&account),
            reply_to: reply,
            server_ms: now,
            playhead_ms: playhead,
            state: if hold.is_some() {
                MsgState::Held
            } else {
                MsgState::Visible
            },
            pinned: false,
            featured: false,
            reactions: vec![],
        };
        let mut room = self.room();
        match hold {
            Some(h) => {
                let release_at_ms = h.auto_release_ms().map(|d| now + d as i64);
                let held = Held {
                    why: h.why(now.saturating_sub(account.created_ms).max(0) as u64),
                    flagged: flagged.map(str::to_owned),
                    release_at_ms,
                    account_since_ms: account.created_ms,
                    m,
                };
                room.held.insert(id, (held.clone(), who));
                room.send(Audience::Mods, &ServerFrame::ModHeld { h: held });
                drop(room);
                if let Some(at) = release_at_ms {
                    let chat = self.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis((at - now).max(0) as u64)).await;
                        let _ = chat.release(&[id]).await;
                    });
                }
                ack(&p.nonce, Some(id), AckState::Held)
            }
            None if p.kind == MessageKind::Studio => {
                let s = StudioMsg {
                    m: m.clone(),
                    account_since_ms: account.created_ms,
                    passed_ms: None,
                    reply: None,
                    invited_ms: None,
                };
                room.send(Audience::Staff, &ServerFrame::ModStudio { s });
                room.send(Audience::Account(who), &ServerFrame::Mine { m });
                ack(&p.nonce, Some(id), AckState::Visible)
            }
            None => {
                room.show(m);
                ack(&p.nonce, Some(id), AckState::Visible)
            }
        }
    }

    /// Release held messages into the room (a moderator's ⏎, or the hold's own countdown).
    /// Returns the ids released.
    pub async fn release(&self, ids: &[i64]) -> Result<Vec<i64>, Problem> {
        let still: Vec<i64> = {
            let room = self.room();
            ids.iter()
                .copied()
                .filter(|i| room.held.contains_key(i))
                .collect()
        };
        if still.is_empty() {
            return Ok(vec![]);
        }
        let changed = store::set_state(&self.db, self.station.id, &still, "visible").await?;
        let mut room = self.room();
        let mut out = Vec::new();
        for id in changed {
            if let Some((h, who)) = room.held.remove(&id) {
                let mut m = h.m;
                m.state = MsgState::Visible;
                if m.kind == MessageKind::Studio {
                    room.send(
                        Audience::Staff,
                        &ServerFrame::ModStudio {
                            s: StudioMsg {
                                m: m.clone(),
                                account_since_ms: h.account_since_ms,
                                passed_ms: None,
                                reply: None,
                                invited_ms: None,
                            },
                        },
                    );
                    room.send(Audience::Account(who), &ServerFrame::Mine { m });
                } else {
                    room.show(m);
                }
                room.send(
                    Audience::Account(who),
                    &ServerFrame::Released { ids: vec![id] },
                );
                out.push(id);
            }
        }
        if !out.is_empty() {
            room.send(Audience::Mods, &ServerFrame::ModUnheld { ids: out.clone() });
        }
        Ok(out)
    }

    pub fn react(&self, who: Uuid, id: i64, emoji: &str) -> Option<ServerFrame> {
        if !self.reactions.take(who, now_ms()) {
            return Some(ServerFrame::Error {
                words: "Slow down: wait a few seconds before reacting again.".into(),
            });
        }
        let mut room = self.room();
        if room
            .chatters
            .get(&who)
            .is_some_and(|c| matches!(c.restriction, Some(Restriction::Banned)))
        {
            return Some(ServerFrame::Error {
                words: "You can't react: this account is banned.".into(),
            });
        }
        match room.react(id, who, emoji) {
            Some(reactions) => {
                room.publish(|offset| ServerFrame::React {
                    offset,
                    id,
                    reactions,
                });
                None
            }
            None => Some(ServerFrame::Error {
                words: "That message or reaction is not in the room.".into(),
            }),
        }
    }

    // ---- moderation ----

    /// The moderator's opening snapshot: open items, held messages, the studio lane (staff).
    pub async fn mod_snapshot(&self, staff: bool) -> Result<ModSnapshot, Problem> {
        let mut items = Vec::new();
        for r in store::open_items(&self.db, self.station.id).await? {
            items.push(self.item(&r).await?);
        }
        let studio = if staff {
            let lane = self.lane.lock().expect("lane").clone();
            store::studio_lane(&self.db, self.station.id)
                .await?
                .into_iter()
                .map(|r| {
                    let note = lane.get(&r.id).cloned().unwrap_or_default();
                    StudioMsg {
                        account_since_ms: r.account.as_ref().map(|a| a.created_ms).unwrap_or(0),
                        m: msg_of(&r, None),
                        passed_ms: note.passed_ms,
                        reply: note.reply,
                        invited_ms: note.invited_ms,
                    }
                })
                .collect()
        } else {
            vec![]
        };
        let held = self.room().held.values().map(|(h, _)| h.clone()).collect();
        Ok(ModSnapshot {
            items,
            held,
            studio,
        })
    }

    async fn item(&self, r: &store::ReportRow) -> Result<Item, Problem> {
        let account = match r.account_id {
            None => None,
            Some(id) => match store::account_by_id(&self.db, self.station.id, id).await? {
                None => None,
                Some(a) => {
                    let restricted = store::restriction(&self.db, self.station.id, id)
                        .await?
                        .map(|x| match x {
                            Restriction::Banned => "banned".to_owned(),
                            Restriction::TimedOut { .. } => "timed out".to_owned(),
                        });
                    Some(AccountRef {
                        id: a.id.to_string(),
                        name: a.name.clone(),
                        since_ms: a.created_ms,
                        standing: a.standing,
                        guest: a.kind == Kind::Guest,
                        restricted,
                    })
                }
            },
        };
        let message: Option<Msg> = r.evidence.get("message").and_then(|m| {
            let mut m: serde_json::Value = m.clone();
            if m.get("state").is_none() {
                m["state"] = "visible".into();
            }
            serde_json::from_value::<MsgEvidence>(m)
                .ok()
                .map(|e| e.into_msg())
        });
        let automated = r
            .evidence
            .get("flagged")
            .and_then(|f| f.as_str())
            .map(str::to_owned);
        Ok(Item {
            id: r.id.to_string(),
            kind: match r.kind.as_str() {
                "notice" => ItemKind::Notice,
                "review" => ItemKind::Review,
                _ => ItemKind::Report,
            },
            reason: r.reason.clone(),
            count: r.count.max(1) as u32,
            message,
            account,
            detail: r.detail.clone(),
            location: r.location.clone(),
            contact: (!r.contact_email.is_empty() || !r.contact_name.is_empty()).then(|| Contact {
                name: r.contact_name.clone(),
                email: r.contact_email.clone(),
            }),
            created_ms: r.created_ms,
            due_ms: r.due_ms,
            automated,
            status: r.status.clone(),
        })
    }

    fn done(&self, by: &str, what: String) -> String {
        self.room().send(
            Audience::Mods,
            &ServerFrame::ModDone {
                by: by.to_owned(),
                what: what.clone(),
                at_ms: now_ms(),
            },
        );
        what
    }

    async fn close_items(&self, ids: Vec<Uuid>, status: &str, by: &str) {
        let mut room = self.room();
        for id in ids {
            room.send(
                Audience::Mods,
                &ServerFrame::ModItemDone {
                    id: id.to_string(),
                    status: status.to_owned(),
                    by: by.to_owned(),
                },
            );
        }
    }

    /// Send a chatter their statement of reasons, store it, and log it.
    async fn tell(
        &self,
        account: Uuid,
        action_id: i64,
        text: &str,
        automated: bool,
    ) -> Result<(), Problem> {
        let (id, at_ms, review_by_ms) = store::insert_statement(
            &self.db,
            self.station.id,
            account,
            action_id,
            text,
            automated,
            crate::config::REVIEW_DAYS,
        )
        .await?;
        self.room().send(
            Audience::Account(account),
            &ServerFrame::Statement {
                s: Statement {
                    id: id.to_string(),
                    at_ms,
                    text: text.to_owned(),
                    automated,
                    review_by_ms,
                    review_requested: false,
                },
            },
        );
        Ok(())
    }

    async fn restrict(&self, account: Uuid, r: Restriction, reason: Reason) -> Result<(), Problem> {
        store::restrict(&self.db, self.station.id, account, r, reason.as_str()).await?;
        let mut room = self.room();
        if let Some(c) = room.chatters.get_mut(&account) {
            c.restriction = Some(r);
            let me = me_of(&c.account, Some(r));
            room.send(Audience::Account(account), &ServerFrame::You { me });
        }
        Ok(())
    }

    /// Take messages out of the room and out of the hold.
    async fn take_out(&self, ids: &[i64]) -> Result<Vec<i64>, Problem> {
        let changed = store::set_state(&self.db, self.station.id, ids, "hidden").await?;
        let mut room = self.room();
        let mut unheld = Vec::new();
        for id in &changed {
            if let Some((_, who)) = room.held.remove(id) {
                unheld.push(*id);
                room.send(
                    Audience::Account(who),
                    &ServerFrame::Rejected { ids: vec![*id] },
                );
            }
        }
        let shown: Vec<i64> = changed
            .iter()
            .copied()
            .filter(|i| !unheld.contains(i))
            .collect();
        if !unheld.is_empty() {
            room.send(Audience::Mods, &ServerFrame::ModUnheld { ids: unheld });
        }
        room.hide(&shown);
        Ok(changed)
    }

    pub async fn moderate(&self, actor: &Actor, cmd: ModCommand) -> Result<String, Problem> {
        let st = self.station.id;
        let by = actor.name.as_str();
        let staff_only = |what: &str| -> Result<(), Problem> {
            if actor.staff {
                Ok(())
            } else {
                Err(Problem::Forbidden(format!("Only the studio can {what}.")))
            }
        };
        match cmd {
            ModCommand::Hide {
                id,
                reason,
                note,
                then,
                automated_disclosed,
                report,
            } => {
                let row = store::message(&self.db, st, id)
                    .await?
                    .ok_or_else(|| Problem::NotFound("That message is gone.".into()))?;
                let automated = row.flagged.is_some() || automated_disclosed;
                let was_held = row.state == "held";
                self.take_out(&[id]).await?;
                let account = row.account.as_ref().map(|a| a.id);
                let action = store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: if was_held { "reject" } else { "hide" },
                        account_id: account,
                        message_id: Some(id),
                        report_id: report,
                        reason: reason.as_str(),
                        ground: if reason.unlawful() {
                            "unlawful"
                        } else {
                            "terms"
                        },
                        note: &note,
                        automated,
                        until_ms: None,
                    },
                )
                .await?;
                let mut outcome = if was_held {
                    Outcome::Rejected
                } else {
                    Outcome::Hidden
                };
                if let Some(a) = account {
                    match then {
                        Then::None => {}
                        Then::Timeout => {
                            let until = now_ms() + i64::from(TIMEOUT_MINUTES) * 60_000;
                            self.restrict(
                                a,
                                Restriction::TimedOut {
                                    until_ms: until as u64,
                                },
                                reason,
                            )
                            .await?;
                            store::log(
                                &self.db,
                                st,
                                &NewAction {
                                    actor: by,
                                    action: "timeout",
                                    account_id: Some(a),
                                    message_id: Some(id),
                                    report_id: report,
                                    reason: reason.as_str(),
                                    ground: "",
                                    note: "",
                                    automated: false,
                                    until_ms: Some(until),
                                },
                            )
                            .await?;
                            outcome = Outcome::HiddenAndTimedOut {
                                minutes: TIMEOUT_MINUTES,
                            };
                        }
                        Then::Ban => {
                            self.ban(a, reason, by, report).await?;
                            outcome = Outcome::HiddenAndBanned;
                        }
                    }
                    if let Some(acct) = &row.account
                        && acct.kind != Kind::Staff
                    {
                        let text = legal::statement(
                            &self.station.name,
                            &self.show(),
                            Some(&row.body),
                            outcome,
                            reason,
                            automated,
                            &self.retention(),
                        );
                        self.tell(a, action, &text, automated).await?;
                    }
                }
                let mut closed = store::decide_for_message(&self.db, st, id, by).await?;
                if let Some(r) = report {
                    closed.extend(store::decide(&self.db, st, r, "actioned", by).await?);
                }
                self.close_items(closed, "actioned", by).await;
                let who = row
                    .account
                    .map(|a| a.name)
                    .unwrap_or_else(|| "someone".into());
                Ok(self.done(
                    by,
                    format!("Hid {who}'s message · {}", reason.words().to_lowercase()),
                ))
            }
            ModCommand::Release { id } => {
                let done = self.release(&[id]).await?;
                if done.is_empty() {
                    return Err(Problem::Conflict("That message is no longer held.".into()));
                }
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "release",
                        account_id: None,
                        message_id: Some(id),
                        report_id: None,
                        reason: "",
                        ground: "",
                        note: "",
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                Ok(self.done(by, "Released a held message".into()))
            }
            ModCommand::ReleaseAll => {
                let ids: Vec<i64> = self.room().held.keys().copied().collect();
                let done = self.release(&ids).await?;
                Ok(self.done(by, format!("Released {} held messages", done.len())))
            }
            ModCommand::HideAllHeld { reason } => {
                let held: Vec<(i64, Uuid, String, Option<String>)> = self
                    .room()
                    .held
                    .iter()
                    .map(|(id, (h, who))| (*id, *who, h.m.body.clone(), h.flagged.clone()))
                    .collect();
                let ids: Vec<i64> = held.iter().map(|h| h.0).collect();
                self.take_out(&ids).await?;
                for (id, who, body, flagged) in &held {
                    let automated = flagged.is_some();
                    let action = store::log(
                        &self.db,
                        st,
                        &NewAction {
                            actor: by,
                            action: "reject",
                            account_id: Some(*who),
                            message_id: Some(*id),
                            report_id: None,
                            reason: reason.as_str(),
                            ground: "terms",
                            note: "bulk",
                            automated,
                            until_ms: None,
                        },
                    )
                    .await?;
                    let text = legal::statement(
                        &self.station.name,
                        &self.show(),
                        Some(body),
                        Outcome::Rejected,
                        reason,
                        automated,
                        &self.retention(),
                    );
                    self.tell(*who, action, &text, automated).await?;
                }
                Ok(self.done(by, format!("Hid {} held messages", held.len())))
            }
            ModCommand::Timeout {
                account,
                minutes,
                reason,
                note,
                report,
            } => {
                if !(1..=7 * 24 * 60).contains(&minutes) {
                    return Err(Problem::Invalid("A time-out is 1 minute to 7 days.".into()));
                }
                let a = self.account(account).await?;
                let until = now_ms() + i64::from(minutes) * 60_000;
                self.restrict(
                    account,
                    Restriction::TimedOut {
                        until_ms: until as u64,
                    },
                    reason,
                )
                .await?;
                let action = store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "timeout",
                        account_id: Some(account),
                        message_id: None,
                        report_id: report,
                        reason: reason.as_str(),
                        ground: if reason.unlawful() {
                            "unlawful"
                        } else {
                            "terms"
                        },
                        note: &note,
                        automated: false,
                        until_ms: Some(until),
                    },
                )
                .await?;
                let text = legal::statement(
                    &self.station.name,
                    &self.show(),
                    None,
                    Outcome::TimedOut { minutes },
                    reason,
                    false,
                    &self.retention(),
                );
                self.tell(account, action, &text, false).await?;
                if let Some(r) = report {
                    let closed = store::decide(&self.db, st, r, "actioned", by).await?;
                    self.close_items(closed, "actioned", by).await;
                }
                Ok(self.done(by, format!("Timed out {} for {minutes} min", a.name)))
            }
            ModCommand::Ban {
                account,
                reason,
                note,
                report,
            } => {
                let a = self.account(account).await?;
                if a.kind == Kind::Staff {
                    return Err(Problem::Conflict(
                        "The studio's own accounts cannot be banned.".into(),
                    ));
                }
                let action = self.ban(account, reason, by, report).await?;
                if !note.is_empty() {
                    store::log(
                        &self.db,
                        st,
                        &NewAction {
                            actor: by,
                            action: "note",
                            account_id: Some(account),
                            message_id: None,
                            report_id: report,
                            reason: reason.as_str(),
                            ground: "",
                            note: &note,
                            automated: false,
                            until_ms: None,
                        },
                    )
                    .await?;
                }
                let text = legal::statement(
                    &self.station.name,
                    &self.show(),
                    None,
                    Outcome::Banned,
                    reason,
                    false,
                    &self.retention(),
                );
                self.tell(account, action, &text, false).await?;
                if let Some(r) = report {
                    let closed = store::decide(&self.db, st, r, "actioned", by).await?;
                    self.close_items(closed, "actioned", by).await;
                }
                Ok(self.done(by, format!("Banned {}", a.name)))
            }
            ModCommand::Lift { account } => {
                let a = self.account(account).await?;
                if !store::lift(&self.db, st, account).await? {
                    return Err(Problem::Conflict(format!(
                        "{} is not timed out or banned.",
                        a.name
                    )));
                }
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "lift",
                        account_id: Some(account),
                        message_id: None,
                        report_id: None,
                        reason: "",
                        ground: "",
                        note: "",
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                let mut room = self.room();
                if let Some(c) = room.chatters.get_mut(&account) {
                    c.restriction = None;
                    let me = me_of(&c.account, None);
                    room.send(Audience::Account(account), &ServerFrame::You { me });
                }
                drop(room);
                Ok(self.done(by, format!("Lifted {}'s restriction", a.name)))
            }
            ModCommand::Dismiss { report, note } => {
                let r = self.report(report).await?;
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: if r.kind == "review" {
                            "review_upheld"
                        } else {
                            "dismiss"
                        },
                        account_id: r.account_id,
                        message_id: r.message_id,
                        report_id: Some(report),
                        reason: &r.reason,
                        ground: "",
                        note: &note,
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                let closed = store::decide(&self.db, st, report, "dismissed", by).await?;
                self.close_items(closed, "dismissed", by).await;
                Ok(self.done(
                    by,
                    if r.kind == "review" {
                        "Kept the decision after review".into()
                    } else {
                        "Left a reported message as it is".into()
                    },
                ))
            }
            ModCommand::Acknowledge { report } => {
                let r = self.report(report).await?;
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "acknowledge",
                        account_id: r.account_id,
                        message_id: r.message_id,
                        report_id: Some(report),
                        reason: &r.reason,
                        ground: "",
                        note: "",
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                let closed = store::decide(&self.db, st, report, "acknowledged", by).await?;
                self.close_items(closed, "acknowledged", by).await;
                Ok(self.done(by, "Acknowledged a notice".into()))
            }
            ModCommand::Escalate { report, note } => {
                let r = self.report(report).await?;
                let mut ids: Vec<i64> = r.message_id.into_iter().collect();
                if let Some(a) = r.account_id {
                    ids.extend(store::visible_ids_of(&self.db, st, a).await?);
                }
                ids.sort_unstable();
                ids.dedup();
                let why = if note.trim().is_empty() {
                    format!("escalated: {}", r.reason)
                } else {
                    note.clone()
                };
                let hold = store::hold_evidence(
                    &self.db,
                    st,
                    report,
                    r.account_id,
                    &ids,
                    &legal::excerpt(&why, 280),
                    by,
                )
                .await?;
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "escalate",
                        account_id: r.account_id,
                        message_id: r.message_id,
                        report_id: Some(report),
                        reason: &r.reason,
                        ground: "",
                        note: &note,
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                sqlx::query(
                    "UPDATE chat_report SET status = 'escalated' WHERE station_id = $1 AND id = $2",
                )
                .bind(st.as_uuid())
                .bind(report)
                .execute(&self.db)
                .await?;
                if let Some(row) = store::report_by_id(&self.db, st, report).await? {
                    let item = self.item(&row).await?;
                    self.room()
                        .send(Audience::Mods, &ServerFrame::ModItem { item });
                }
                Ok(self.done(
                    by,
                    format!(
                        "Preserved the evidence (hold {}) and escalated",
                        &hold.simple().to_string()[..8]
                    ),
                ))
            }
            ModCommand::ReleaseHold { hold } => {
                staff_only("release an evidence hold")?;
                if !store::release_hold(&self.db, st, hold, by).await? {
                    return Err(Problem::NotFound("No open hold with that id.".into()));
                }
                Ok(self.done(by, "Released an evidence hold".into()))
            }
            ModCommand::Pin { id } => {
                let m = self.visible(id).await?;
                store::set_pinned(&self.db, st, Some(id)).await?;
                let mut room = self.room();
                let mut pinned = m;
                pinned.pinned = true;
                room.update_recent(id, |x| x.pinned = true);
                room.pinned = Some(pinned.clone());
                room.publish(|offset| ServerFrame::Pin {
                    offset,
                    m: Some(pinned),
                });
                drop(room);
                Ok(self.done(by, "Pinned a message".into()))
            }
            ModCommand::Unpin => {
                store::set_pinned(&self.db, st, None).await?;
                let mut room = self.room();
                room.pinned = None;
                room.publish(|offset| ServerFrame::Pin { offset, m: None });
                drop(room);
                Ok(self.done(by, "Unpinned".into()))
            }
            ModCommand::Feature { id } => {
                staff_only("put a message on air")?;
                let mut m = self.visible(id).await?;
                store::set_featured(&self.db, st, id).await?;
                m.featured = true;
                let mut room = self.room();
                room.update_recent(id, |x| x.featured = true);
                room.publish(|offset| ServerFrame::Feature { offset, m });
                drop(room);
                Ok(self.done(by, "Featured a message on the player page".into()))
            }
            ModCommand::Post { body, kind, pin } => {
                staff_only("post as the station")?;
                let kind = kind.unwrap_or(MessageKind::Chat);
                if !matches!(kind, MessageKind::Chat | MessageKind::Announcement) {
                    return Err(Problem::Invalid(
                        "The studio posts chat or announcements.".into(),
                    ));
                }
                let body = body.trim().to_owned();
                if body.is_empty() || body.chars().count() > rules::MAX_CHARS {
                    return Err(Problem::Invalid(format!(
                        "Write 1 to {} characters.",
                        rules::MAX_CHARS
                    )));
                }
                let account = match actor.account {
                    Some(a) => self.account(a).await?,
                    None => store::staff_account(&self.db, st, by).await?,
                };
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let now = now_ms();
                store::insert_message(
                    &self.db,
                    st,
                    &store::NewMessage {
                        id,
                        account: account.id,
                        kind,
                        body: &body,
                        reply_to: None,
                        server_ms: now,
                        playhead_ms: None,
                        held: false,
                        flagged: None,
                    },
                )
                .await?;
                let m = Msg {
                    id,
                    kind,
                    body,
                    author: author(&account),
                    reply_to: None,
                    server_ms: now,
                    playhead_ms: None,
                    state: MsgState::Visible,
                    pinned: false,
                    featured: false,
                    reactions: vec![],
                };
                self.room().show(m);
                if pin {
                    Box::pin(self.moderate(actor, ModCommand::Pin { id })).await?;
                }
                Ok(self.done(
                    by,
                    if kind == MessageKind::Announcement {
                        "Posted an announcement".into()
                    } else {
                        "Posted to the room".into()
                    },
                ))
            }
            ModCommand::Standing { account, standing } => {
                staff_only("change a chatter's standing")?;
                if standing == Standing::Staff {
                    return Err(Problem::Invalid(
                        "Staff is not a standing a chatter can be given.".into(),
                    ));
                }
                if !store::set_standing(&self.db, st, account, standing).await? {
                    return Err(Problem::NotFound("No chatter with that id.".into()));
                }
                let a = self.account(account).await?;
                store::log(
                    &self.db,
                    st,
                    &NewAction {
                        actor: by,
                        action: "standing",
                        account_id: Some(account),
                        message_id: None,
                        report_id: None,
                        reason: standing.as_str(),
                        ground: "",
                        note: "",
                        automated: false,
                        until_ms: None,
                    },
                )
                .await?;
                let mut room = self.room();
                let restriction = room.chatters.get(&account).and_then(|c| c.restriction);
                room.chatters.insert(
                    account,
                    Chatter {
                        account: a.clone(),
                        restriction,
                    },
                );
                room.send(
                    Audience::Account(account),
                    &ServerFrame::You {
                        me: me_of(&a, restriction),
                    },
                );
                drop(room);
                Ok(self.done(by, format!("{} is now {}", a.name, standing.as_str())))
            }
            ModCommand::Reply { id, body } => {
                staff_only("reply in the studio lane")?;
                let row = self.studio_row(id).await?;
                let body = body.trim().to_owned();
                if body.is_empty() || body.chars().count() > rules::MAX_CHARS {
                    return Err(Problem::Invalid(format!(
                        "Write 1 to {} characters.",
                        rules::MAX_CHARS
                    )));
                }
                let to = row
                    .account
                    .as_ref()
                    .map(|a| a.id)
                    .ok_or_else(|| Problem::NotFound("The chatter is gone.".into()))?;
                self.room().send(
                    Audience::Account(to),
                    &ServerFrame::Private {
                        id: Uuid::new_v4().to_string(),
                        at_ms: now_ms(),
                        from: format!("{} studio", self.station.name),
                        body: body.clone(),
                        invite: false,
                    },
                );
                self.lane_note(id, |n| n.reply = Some(body.clone()));
                self.lane_update(&row);
                Ok(self.done(
                    by,
                    format!(
                        "Replied to {}",
                        row.account.map(|a| a.name).unwrap_or_default()
                    ),
                ))
            }
            ModCommand::Pass { id } => {
                staff_only("pass a message to the presenter")?;
                let row = self.studio_row(id).await?;
                self.lane_note(id, |n| n.passed_ms = Some(now_ms()));
                self.lane_update(&row);
                let presenter = self.studio.borrow().presenter.clone();
                let to = if presenter.is_empty() {
                    "the presenter".to_owned()
                } else {
                    presenter
                };
                Ok(self.done(
                    by,
                    format!(
                        "Passed {}'s message to {to}",
                        row.account.map(|a| a.name).unwrap_or_default()
                    ),
                ))
            }
            ModCommand::Invite { id } => {
                staff_only("invite a chatter to call in")?;
                let row = match self.studio_row(id).await {
                    Ok(r) => r,
                    Err(_) => store::message(&self.db, st, id)
                        .await?
                        .ok_or_else(|| Problem::NotFound("That message is gone.".into()))?,
                };
                let to = row
                    .account
                    .as_ref()
                    .map(|a| a.id)
                    .ok_or_else(|| Problem::NotFound("The chatter is gone.".into()))?;
                self.room().send(Audience::Account(to), &ServerFrame::Private {
                    id: Uuid::new_v4().to_string(), at_ms: now_ms(), from: format!("{} studio", self.station.name),
                    body: "The studio would like you to call in. Press Call the studio when you're ready.".into(),
                    invite: true,
                });
                if row.kind == MessageKind::Studio {
                    self.lane_note(id, |n| n.invited_ms = Some(now_ms()));
                    self.lane_update(&row);
                }
                Ok(self.done(
                    by,
                    format!(
                        "Invited {} to call in",
                        row.account.map(|a| a.name).unwrap_or_default()
                    ),
                ))
            }
        }
    }

    /// Ban: the account cannot post, and its past messages leave the room (Owncast's rule).
    async fn ban(
        &self,
        account: Uuid,
        reason: Reason,
        by: &str,
        report: Option<Uuid>,
    ) -> Result<i64, Problem> {
        self.restrict(account, Restriction::Banned, reason).await?;
        let ids = store::visible_ids_of(&self.db, self.station.id, account).await?;
        self.take_out(&ids).await?;
        Ok(store::log(
            &self.db,
            self.station.id,
            &NewAction {
                actor: by,
                action: "ban",
                account_id: Some(account),
                message_id: None,
                report_id: report,
                reason: reason.as_str(),
                ground: if reason.unlawful() {
                    "unlawful"
                } else {
                    "terms"
                },
                note: "",
                automated: false,
                until_ms: None,
            },
        )
        .await?)
    }

    async fn account(&self, id: Uuid) -> Result<Account, Problem> {
        store::account_by_id(&self.db, self.station.id, id)
            .await?
            .ok_or_else(|| Problem::NotFound("No chatter with that id.".into()))
    }

    async fn report(&self, id: Uuid) -> Result<store::ReportRow, Problem> {
        store::report_by_id(&self.db, self.station.id, id)
            .await?
            .ok_or_else(|| Problem::NotFound("No report with that id.".into()))
    }

    async fn visible(&self, id: i64) -> Result<Msg, Problem> {
        if let Some(m) = self.room().find(id) {
            return Ok(m.clone());
        }
        match store::message(&self.db, self.station.id, id).await? {
            Some(r) if r.state == "visible" && r.kind != MessageKind::Studio => {
                Ok(msg_of(&r, None))
            }
            _ => Err(Problem::NotFound("That message is not in the room.".into())),
        }
    }

    async fn studio_row(&self, id: i64) -> Result<store::Row, Problem> {
        match store::message(&self.db, self.station.id, id).await? {
            Some(r) if r.kind == MessageKind::Studio => Ok(r),
            _ => Err(Problem::NotFound(
                "That is not a message to the studio.".into(),
            )),
        }
    }

    fn lane_note(&self, id: i64, f: impl FnOnce(&mut LaneNote)) {
        f(self.lane.lock().expect("lane").entry(id).or_default());
    }

    fn lane_update(&self, row: &store::Row) {
        let note = self
            .lane
            .lock()
            .expect("lane")
            .get(&row.id)
            .cloned()
            .unwrap_or_default();
        let s = StudioMsg {
            m: msg_of(row, None),
            account_since_ms: row.account.as_ref().map(|a| a.created_ms).unwrap_or(0),
            passed_ms: note.passed_ms,
            reply: note.reply,
            invited_ms: note.invited_ms,
        };
        self.room()
            .send(Audience::Staff, &ServerFrame::ModStudio { s });
    }

    // ---- reports, notices, reviews ----

    /// A chatter reports a message or a user. Reports on the same message join one item.
    pub async fn report_by_chatter(
        &self,
        reporter: &Account,
        message_id: Option<i64>,
        account_id: Option<Uuid>,
        reason: Reason,
        detail: &str,
    ) -> Result<Uuid, Problem> {
        let st = self.station.id;
        if detail.chars().count() > 2000 {
            return Err(Problem::Invalid(
                "Keep the detail under 2,000 characters.".into(),
            ));
        }
        let (subject, evidence) = match (message_id, account_id) {
            (Some(mid), _) => {
                let row = store::message(&self.db, st, mid)
                    .await?
                    .filter(|r| {
                        r.kind != MessageKind::Studio
                            || r.account.as_ref().map(|a| a.id) == Some(reporter.id)
                    })
                    .ok_or_else(|| Problem::NotFound("That message is gone.".into()))?;
                let ev = serde_json::json!({
                    "message": msg_of(&row, None),
                    "flagged": row.flagged,
                    "state": row.state,
                });
                (row.account.map(|a| a.id), ev)
            }
            (None, Some(aid)) => {
                let a = self.account(aid).await?;
                (
                    Some(a.id),
                    serde_json::json!({"account": {"id": a.id, "name": a.name}}),
                )
            }
            (None, None) => {
                return Err(Problem::Invalid(
                    "Say which message or chatter you are reporting.".into(),
                ));
            }
        };
        if subject == Some(reporter.id) {
            return Err(Problem::Invalid("You can't report yourself.".into()));
        }
        let existing = store::open_item_for(&self.db, st, message_id, subject).await?;
        let id = store::insert_report(
            &self.db,
            st,
            &store::NewReport {
                kind: "report",
                message_id,
                account_id: subject,
                reporter_id: Some(reporter.id),
                reason: reason.as_str(),
                detail,
                location: "",
                contact_name: "",
                contact_email: "",
                evidence,
                statement_id: None,
                due_hours: reason.due_hours(),
            },
        )
        .await?;
        let item_id = existing.unwrap_or(id);
        if reason.preserve_at_once() {
            let ids: Vec<i64> = message_id.into_iter().collect();
            store::hold_evidence(
                &self.db,
                st,
                id,
                subject,
                &ids,
                reason.words(),
                "automatic: the report's reason",
            )
            .await?;
        }
        if let Some(row) = store::open_items(&self.db, st)
            .await?
            .into_iter()
            .find(|r| r.id == item_id)
        {
            let item = self.item(&row).await?;
            self.room()
                .send(Audience::Mods, &ServerFrame::ModItem { item });
        }
        Ok(id)
    }

    /// A notice from someone without an account (DSA Article 16).
    pub async fn notice(&self, n: &NoticeRequest) -> Result<Uuid, Problem> {
        let reason =
            Reason::parse(&n.reason).ok_or_else(|| Problem::Invalid("Choose a reason.".into()))?;
        let field = |name: &str, v: &str, max: usize| -> Result<(), Problem> {
            if v.trim().is_empty() || v.chars().count() > max {
                Err(Problem::Invalid(format!(
                    "{name} is needed, up to {max} characters."
                )))
            } else {
                Ok(())
            }
        };
        field("Where it is", &n.location, 500)?;
        field("Why", &n.detail, 2000)?;
        if n.name.chars().count() > 120 {
            return Err(Problem::Invalid(
                "Your name is up to 120 characters.".into(),
            ));
        }
        let email = n.email.trim();
        if email.len() > 254 || !email.contains('@') || email.contains(char::is_whitespace) {
            return Err(Problem::Invalid(
                "Give an email address the station can answer.".into(),
            ));
        }
        if !n.good_faith {
            return Err(Problem::Invalid(
                "Confirm the notice is accurate and complete.".into(),
            ));
        }
        let id = store::insert_report(
            &self.db,
            self.station.id,
            &store::NewReport {
                kind: "notice",
                message_id: None,
                account_id: None,
                reporter_id: None,
                reason: reason.as_str(),
                detail: n.detail.trim(),
                location: n.location.trim(),
                contact_name: n.name.trim(),
                contact_email: email,
                evidence: serde_json::json!({}),
                statement_id: None,
                due_hours: reason.due_hours(),
            },
        )
        .await?;
        if let Some(row) = store::report_by_id(&self.db, self.station.id, id).await? {
            let item = self.item(&row).await?;
            self.room()
                .send(Audience::Mods, &ServerFrame::ModItem { item });
        }
        Ok(id)
    }

    /// A chatter asks for a review of a statement of reasons.
    pub async fn review(
        &self,
        who: &Account,
        statement: Uuid,
        text: &str,
    ) -> Result<Uuid, Problem> {
        if text.trim().is_empty() || text.chars().count() > 2000 {
            return Err(Problem::Invalid(
                "Say why, in up to 2,000 characters.".into(),
            ));
        }
        let st = self.station.id;
        let said = store::request_review(&self.db, st, who.id, statement)
            .await?
            .ok_or_else(|| {
                Problem::Conflict(
                    "That decision can no longer be reviewed: asked already, or the window closed."
                        .into(),
                )
            })?;
        let id = store::insert_report(
            &self.db,
            st,
            &store::NewReport {
                kind: "review",
                message_id: None,
                account_id: Some(who.id),
                reporter_id: Some(who.id),
                reason: "other",
                detail: text.trim(),
                location: &said,
                contact_name: "",
                contact_email: "",
                evidence: serde_json::json!({"statement": statement}),
                statement_id: Some(statement),
                due_hours: None,
            },
        )
        .await?;
        if let Some(row) = store::report_by_id(&self.db, st, id).await? {
            let item = self.item(&row).await?;
            self.room()
                .send(Audience::Mods, &ServerFrame::ModItem { item });
        }
        Ok(id)
    }

    // ---- erasure and settings ----

    pub async fn erase(&self, id: Uuid, by: &str) -> Result<usize, Problem> {
        self.account(id).await?;
        let gone = store::erase(&self.db, self.station.id, id).await?;
        store::log(
            &self.db,
            self.station.id,
            &NewAction {
                actor: by,
                action: "erase",
                account_id: Some(id),
                message_id: None,
                report_id: None,
                reason: "",
                ground: "",
                note: &format!("{} messages", gone.len()),
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        let mut room = self.room();
        room.chatters.remove(&id);
        room.posters.remove(&id);
        let held: Vec<i64> = room
            .held
            .iter()
            .filter(|(_, (_, w))| *w == id)
            .map(|(i, _)| *i)
            .collect();
        for h in &held {
            room.held.remove(h);
        }
        if !held.is_empty() {
            room.send(Audience::Mods, &ServerFrame::ModUnheld { ids: held });
        }
        room.forget(&gone);
        Ok(gone.len())
    }

    pub async fn save_settings(&self, s: StationSettings) -> Result<(), Problem> {
        s.validate().map_err(Problem::Invalid)?;
        store::save_settings(&self.db, self.station.id, &s).await?;
        *self.settings.write().expect("settings") = s;
        self.announce_rules();
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct NoticeRequest {
    pub reason: String,
    pub location: String,
    pub detail: String,
    #[serde(default)]
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub good_faith: bool,
}

/// A message as kept in a report's evidence (the wire shape, read back).
#[derive(Deserialize)]
struct MsgEvidence {
    id: i64,
    kind: MessageKind,
    body: String,
    author: AuthorEvidence,
    #[serde(default)]
    server_ms: i64,
    #[serde(default)]
    playhead_ms: Option<i64>,
}
#[derive(Deserialize)]
struct AuthorEvidence {
    id: String,
    name: String,
    standing: Standing,
    #[serde(default)]
    guest: bool,
}
impl MsgEvidence {
    fn into_msg(self) -> Msg {
        Msg {
            id: self.id,
            kind: self.kind,
            body: self.body,
            author: Author {
                id: self.author.id,
                name: self.author.name,
                standing: self.author.standing,
                guest: self.author.guest,
            },
            reply_to: None,
            server_ms: self.server_ms,
            playhead_ms: self.playhead_ms,
            state: MsgState::Visible,
            pinned: false,
            featured: false,
            reactions: vec![],
        }
    }
}

pub fn me_of(a: &Account, r: Option<Restriction>) -> Me {
    Me {
        id: a.id.to_string(),
        name: a.name.clone(),
        kind: a.kind,
        standing: a.standing,
        created_ms: a.created_ms,
        timed_out_until_ms: match r {
            Some(Restriction::TimedOut { until_ms }) => Some(until_ms as i64),
            _ => None,
        },
        banned: matches!(r, Some(Restriction::Banned)),
        moderator: a.standing.moderates(),
    }
}

pub fn msg_of(r: &store::Row, reply_name: Option<String>) -> Msg {
    let author = r.account.as_ref().map(author).unwrap_or(Author {
        id: String::new(),
        name: "erased".into(),
        standing: Standing::Member,
        guest: false,
    });
    Msg {
        id: r.id,
        kind: r.kind,
        body: r.body.clone(),
        author,
        reply_to: r.reply_to.map(|id| ReplyRef {
            id,
            name: reply_name.clone().unwrap_or_default(),
        }),
        server_ms: r.server_ms,
        playhead_ms: r.playhead_ms,
        state: match r.state.as_str() {
            "held" => MsgState::Held,
            "hidden" => MsgState::Hidden,
            _ => MsgState::Visible,
        },
        pinned: r.pinned,
        featured: r.featured,
        reactions: reaction_list(&[]),
    }
}
