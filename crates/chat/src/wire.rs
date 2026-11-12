// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat's wire protocol: JSON frames over one WebSocket, `GET /api/v1/chat/socket`, each
//! an object tagged by `t`. `packages/ui/chat.ts` holds the same types for the pages; change
//! both together.
//!
//! **Opening.** The client's first frame is `hello`, with its session token if signed in (never
//! in the URL, so it stays out of logs) and, on a reconnect, the position it last saw. The
//! server answers `welcome`.
//!
//! **Recovery** (Centrifugo's offset and epoch, ADR-0031). Every public event carries an
//! `offset`, counting up from 1 within an `epoch` (a fresh epoch per server start). A client
//! that reconnects sends `since: {epoch, offset}`; if the epoch matches and the server still
//! holds every event after that offset, `welcome.recovered` is true and `welcome.missed` holds
//! them in order. Otherwise `recovered` is false and `history` is a fresh snapshot.
//!
//! **Two timestamps** (ADR-0038). Each message carries `server_ms` (the server's clock when it
//! arrived) and `playhead_ms` (the programme time the sender was seeing, from their player;
//! null when the player had none). "Sync chat to my stream" shows a message once the viewer's
//! own playhead reaches its `server_ms` minus nothing: the listener delay is in `welcome`.
//!
//! **Slow clients.** Each connection has a bounded queue; a client that cannot keep up is
//! closed with code 4008 and recovers on reconnect, so one slow browser never stalls the room.

use serde::{Deserialize, Serialize};

use crate::rules::{MessageKind, Modes, Standing};

/// Close code for a client whose queue overflowed: reconnect and recover.
pub const CLOSE_SLOW: u16 = 4008;
/// Close code for a session that was signed out, erased or banned while connected.
pub const CLOSE_SIGNED_OUT: u16 = 4001;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Position {
    pub epoch: String,
    pub offset: u64,
}

/// A frame from the browser.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientFrame {
    Hello {
        #[serde(default)]
        session: Option<String>,
        #[serde(default)]
        since: Option<Position>,
    },
    Post {
        /// The client's own id for the post, echoed in its `ack`.
        nonce: String,
        kind: MessageKind,
        body: String,
        #[serde(default)]
        reply_to: Option<i64>,
        /// The programme time the sender was seeing, epoch ms (ADR-0038).
        #[serde(default)]
        playhead_ms: Option<i64>,
    },
    React {
        id: i64,
        emoji: String,
    },
    Ping,
}

/// A chatter's avatar as everyone sees it (ADR-0235): one of the shipped set, by its id in
/// `packages/ui/avatars/manifest.json`, or their own picture once staff approved it, by its id
/// (`GET /api/v1/chat/avatars/{id}`). None at all is the plain default, their initial.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Avatar {
    Stock { id: String },
    Upload { id: String },
}

/// Who wrote a message, as everyone sees it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub standing: Standing,
    pub guest: bool,
    pub avatar: Option<Avatar>,
}

/// The message a reply answers, named so the reply reads without scrolling.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReplyRef {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MsgState {
    Visible,
    Held,
    Hidden,
}

/// A message.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Msg {
    pub id: i64,
    pub kind: MessageKind,
    pub body: String,
    pub author: Author,
    pub reply_to: Option<ReplyRef>,
    pub server_ms: i64,
    pub playhead_ms: Option<i64>,
    pub state: MsgState,
    pub pinned: bool,
    pub featured: bool,
    /// Reaction counts by emoji, in the fixed order of [`REACTIONS`].
    pub reactions: Vec<(String, u32)>,
    /// A picture or document the message carries (ADR-0262).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Attachment>,
}

/// A picture or document posted in the chat, as a message carries it (ADR-0262). Its files are
/// at `/api/v1/chat/pictures/{id}` (and `/thumb`) or `/api/v1/chat/files/{id}`, served only
/// once staff or a moderator approved it, as long as its message is in view.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    /// `picture` or `document`.
    pub kind: String,
    /// The name the chatter's file had: shown as text, never used to name a file.
    pub name: String,
    /// The type the station found in its bytes, and the only one it sends.
    pub mime: String,
    /// The type in words ("PDF", "OpenDocument text", "JPEG").
    pub label: String,
    pub bytes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_height: Option<u32>,
}

/// The reactions a chatter can add (custom emoji come later).
pub const REACTIONS: &[&str] = &["👍", "❤️", "😂", "😮", "👏", "📻"];

/// The settings every page needs: what the room allows now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RoomRules {
    #[serde(flatten)]
    pub modes: Modes,
    pub slow_s: u64,
    pub min_age_minutes: u64,
    pub max_chars: usize,
    pub hold_ms: u64,
    /// When chatters may post (ADR-0234).
    pub hours: crate::hours::Hours,
    /// Chatters may upload a picture of their own now (ADR-0259): the studio's switch, so an open
    /// page learns of it at once.
    pub avatar_uploads: bool,
    /// Trusted chatters may attach a picture or document (ADR-0262), by the same kind of switch.
    pub attachments: bool,
}

/// The signed-in chatter's own account, as they see it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub kind: crate::rules::Kind,
    pub standing: Standing,
    pub created_ms: i64,
    /// A time-out's end, epoch ms.
    pub timed_out_until_ms: Option<i64>,
    pub banned: bool,
    /// The sentence a moderator or the station's view of "can I post" needs.
    pub moderator: bool,
    /// What everyone else sees beside their messages (ADR-0235).
    pub avatar: Option<Avatar>,
    /// The shipped avatar they chose, shown when no picture of their own is approved.
    pub avatar_stock: Option<String>,
    /// Their own picture waiting for staff: only they see it (`GET /api/v1/chat/me/avatar`).
    pub avatar_held: Option<String>,
}

/// A statement of reasons (DSA Article 17), as the chatter receives it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Statement {
    pub id: String,
    pub at_ms: i64,
    pub text: String,
    pub automated: bool,
    pub review_by_ms: i64,
    pub review_requested: bool,
}

/// A held message as a moderator sees it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Held {
    pub m: Msg,
    /// When it releases by itself, epoch ms; null when it waits for a moderator.
    pub release_at_ms: Option<i64>,
    pub why: String,
    pub flagged: Option<String>,
    pub account_since_ms: i64,
}

/// A report, a notice or a review request as a moderator sees it ("Needs you").
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub reason: String,
    /// How many reports name the same message (one item per message).
    pub count: u32,
    /// The message as it was when first reported.
    pub message: Option<Msg>,
    pub account: Option<AccountRef>,
    pub detail: String,
    pub location: String,
    pub contact: Option<Contact>,
    pub created_ms: i64,
    /// A fixed clock: an intimate image is taken down within 48 hours.
    pub due_ms: Option<i64>,
    /// The automated step involved, if any ("rustrict").
    pub automated: Option<String>,
    pub status: String,
    /// A report about the account's picture (ADR-0260): the picture's id, while it is still the
    /// one showing; it is fetched like any picture (`GET /api/v1/chat/mod/avatars/{id}`).
    pub picture: Option<String>,
    /// A report about a posted picture or document (ADR-0262): its id, while it is still showing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Report,
    Notice,
    Review,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountRef {
    pub id: String,
    pub name: String,
    pub since_ms: i64,
    pub standing: Standing,
    pub guest: bool,
    pub restricted: Option<String>,
    pub avatar: Option<Avatar>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub name: String,
    pub email: String,
}

/// A message to the studio (ADR-0040), with what the studio did about it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StudioMsg {
    pub m: Msg,
    pub account_since_ms: i64,
    pub passed_ms: Option<i64>,
    pub reply: Option<String>,
    pub invited_ms: Option<i64>,
}

/// An uploaded picture waiting for staff (ADR-0235). Staff fetch it at
/// `GET /api/v1/chat/mod/avatars/{id}`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HeldAvatar {
    pub id: String,
    pub account: AccountRef,
    pub uploaded_ms: i64,
    pub bytes: u32,
    /// `avatar`, or a `picture` or `document` posted in the chat (ADR-0262), in the same queue.
    pub kind: String,
    /// What a posted picture or document is, and the message that carries it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Attachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<HeldMessage>,
}

/// The message a waiting picture or document came with.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HeldMessage {
    pub id: i64,
    pub body: String,
}

/// What a moderator's socket gets on opening, beside `welcome`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModSnapshot {
    pub items: Vec<Item>,
    pub held: Vec<Held>,
    pub studio: Vec<StudioMsg>,
    /// Pictures waiting for approval, oldest first (staff and moderators both, ADR-0255).
    pub avatars: Vec<HeldAvatar>,
}

/// A frame to the browser.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerFrame {
    Welcome {
        epoch: String,
        offset: u64,
        recovered: bool,
        /// Events after `since`, when recovered: the frames as they were sent.
        missed: Vec<Box<serde_json::value::RawValue>>,
        /// The latest visible room messages, oldest first, when not recovered.
        history: Vec<Msg>,
        station: String,
        show: String,
        me: Option<Me>,
        rules: RoomRules,
        listener_delay_ms: u64,
        server_now_ms: i64,
        pinned: Option<Msg>,
        count: usize,
        statements: Vec<Statement>,
        /// Only on a moderator's socket.
        moderation: Option<ModSnapshot>,
    },
    /// A message in the room (public; carries an offset).
    Msg {
        offset: u64,
        m: Msg,
    },
    /// Messages taken out of the room (hidden, a banned chatter's past messages, erased).
    Hide {
        offset: u64,
        ids: Vec<i64>,
    },
    /// The pinned message changed (null: unpinned).
    Pin {
        offset: u64,
        m: Option<Msg>,
    },
    /// A message put on air (its text is shown in the audio player too).
    Feature {
        offset: u64,
        m: Msg,
    },
    /// Reaction counts on one message.
    React {
        offset: u64,
        id: i64,
        reactions: Vec<(String, u32)>,
    },
    /// A chatter's avatar changed (a shipped one chosen, a picture approved or removed): every
    /// message of theirs on the page takes it (public; carries an offset).
    Avatar {
        offset: u64,
        account: String,
        avatar: Option<Avatar>,
    },
    /// People reading the room.
    Count {
        n: usize,
    },
    /// The room's rules changed (a mode switched).
    Rules {
        rules: RoomRules,
        show: String,
        listener_delay_ms: u64,
    },
    /// The answer to a `post`: posted, held (with why), or refused (with why and when to retry).
    Ack {
        nonce: String,
        id: Option<i64>,
        state: AckState,
        code: Option<String>,
        words: Option<String>,
        retry_ms: Option<u64>,
    },
    /// The chatter's own standing changed (a time-out, a ban, trusted).
    You {
        me: Me,
    },
    /// A statement of reasons for this chatter.
    Statement {
        s: Statement,
    },
    /// A private message from the studio to this chatter: a reply, or an invitation to call.
    Private {
        id: String,
        at_ms: i64,
        from: String,
        body: String,
        invite: bool,
    },
    /// The chatter's own message to the studio, as kept (they see their side of the lane).
    Mine {
        m: Msg,
    },
    /// A held message released into the room: the sender learns it is visible.
    Released {
        ids: Vec<i64>,
    },
    /// Held messages a moderator turned down, to their sender (the statement follows).
    Rejected {
        ids: Vec<i64>,
    },
    // ---- moderators only ----
    ModHeld {
        h: Held,
    },
    ModUnheld {
        ids: Vec<i64>,
    },
    /// A picture arrived for approval (staff only).
    ModAvatar {
        a: HeldAvatar,
    },
    /// A waiting picture was approved, refused, replaced or withdrawn (staff only).
    ModAvatarDone {
        id: String,
    },
    ModItem {
        item: Item,
    },
    ModItemDone {
        id: String,
        status: String,
        by: String,
    },
    ModStudio {
        s: StudioMsg,
    },
    /// A line for the moderation toast: who did what (so two moderators see each other's acts).
    ModDone {
        by: String,
        what: String,
        at_ms: i64,
    },
    Pong,
    /// A problem with the frame just sent.
    Error {
        words: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AckState {
    Visible,
    Held,
    Refused,
}

impl ServerFrame {
    /// The public offset this event carries, if it is part of the room's recoverable stream.
    pub fn offset(&self) -> Option<u64> {
        match self {
            ServerFrame::Msg { offset, .. }
            | ServerFrame::Hide { offset, .. }
            | ServerFrame::Pin { offset, .. }
            | ServerFrame::Feature { offset, .. }
            | ServerFrame::React { offset, .. }
            | ServerFrame::Avatar { offset, .. } => Some(*offset),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_tagged_by_t() {
        let f: ClientFrame = serde_json::from_str(
            r#"{"t":"post","nonce":"n1","kind":"request","body":"Tide's Out","playhead_ms":5}"#,
        )
        .unwrap();
        assert!(matches!(
            f,
            ClientFrame::Post {
                kind: MessageKind::Request,
                playhead_ms: Some(5),
                reply_to: None,
                ..
            }
        ));
        let h: ClientFrame = serde_json::from_str(r#"{"t":"hello"}"#).unwrap();
        assert!(matches!(
            h,
            ClientFrame::Hello {
                session: None,
                since: None
            }
        ));
        let s = serde_json::to_value(ServerFrame::Hide {
            offset: 7,
            ids: vec![1, 2],
        })
        .unwrap();
        assert_eq!(
            s,
            serde_json::json!({"t": "hide", "offset": 7, "ids": [1, 2]})
        );
        let c = serde_json::to_value(ServerFrame::Count { n: 3 }).unwrap();
        assert_eq!(c["t"], "count");
    }
}
