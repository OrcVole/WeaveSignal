// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! One socket: the `hello`, the `welcome` (recovered or a fresh snapshot), then posts,
//! reactions and pings in, and the room's frames out through a bounded queue.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::value::RawValue;
use tokio::sync::mpsc;

use crate::room::{Out, encode};
use crate::service::{Chat, PostRequest, now_ms};
use crate::store;
use crate::wire::{CLOSE_SIGNED_OUT, CLOSE_SLOW, ClientFrame, ServerFrame};

/// How long a socket may take to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// A socket silent this long (no frame, no ping) is closed; pages ping every 25 s.
const IDLE: Duration = Duration::from_secs(75);
/// The largest frame a browser may send.
pub const MAX_FRAME: usize = 8 * 1024;

pub async fn serve(chat: Arc<Chat>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // ---- hello ----
    let first = tokio::time::timeout(HELLO_WITHIN, stream.next()).await;
    let Ok(Some(Ok(Message::Text(text)))) = first else {
        let _ = sink.close().await;
        return;
    };
    let Ok(ClientFrame::Hello { session, since }) = serde_json::from_str::<ClientFrame>(&text)
    else {
        let _ = sink
            .send(Message::Text(encode(&ServerFrame::Error {
                words: "The first frame must be hello.".into(),
            })))
            .await;
        let _ = sink.close().await;
        return;
    };
    let account = match &session {
        None => None,
        Some(token) => match store::session_account(&chat.db, chat.station.id, token).await {
            Ok(Some(a)) => Some(a),
            Ok(None) => {
                let _ = sink
                    .send(Message::Close(Some(CloseFrame {
                        code: CLOSE_SIGNED_OUT,
                        reason: "signed out".into(),
                    })))
                    .await;
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "a session could not be read");
                let _ = sink.close().await;
                return;
            }
        },
    };
    let me = match &account {
        Some(a) => match chat.load_chatter(a).await {
            Ok(me) => Some(me),
            Err(_) => {
                let _ = sink.close().await;
                return;
            }
        },
        None => None,
    };
    let moderator = account.as_ref().is_some_and(|a| a.standing.moderates());
    let staff = account
        .as_ref()
        .is_some_and(|a| a.standing == crate::rules::Standing::Staff);
    let statements = match &account {
        Some(a) => chat.statements_of(a.id).await.unwrap_or_default(),
        None => vec![],
    };
    let moderation = if moderator {
        chat.mod_snapshot(staff).await.ok()
    } else {
        None
    };

    // ---- welcome, then the room ----
    let (tx, mut rx) = mpsc::channel::<Out>(chat.config.limits.socket_queue);
    let slow = Arc::new(AtomicBool::new(false));
    let conn = {
        let mut room = chat.room.lock().expect("room");
        let recovered = since.as_ref().and_then(|p| room.since(p));
        let (missed, history) = match &recovered {
            Some(m) => (
                m.iter()
                    .filter_map(|t| RawValue::from_string(t.as_str().to_owned()).ok())
                    .collect(),
                vec![],
            ),
            None => {
                let n = chat.config.limits.history;
                let skip = room.recent.len().saturating_sub(n);
                (vec![], room.recent.iter().skip(skip).cloned().collect())
            }
        };
        let welcome = ServerFrame::Welcome {
            epoch: room.epoch.clone(),
            offset: room.offset,
            recovered: recovered.is_some(),
            missed,
            history,
            station: chat.station.name.clone(),
            show: chat.show(),
            me: me.clone(),
            rules: chat.rules(),
            listener_delay_ms: chat.listener_delay_ms(),
            server_now_ms: now_ms(),
            pinned: room.pinned.clone(),
            count: room.count() + 1,
            statements,
            moderation,
        };
        let id = room.register(
            tx,
            account.as_ref().map(|a| a.id),
            moderator,
            staff,
            slow.clone(),
        );
        room.send_to(id, &welcome);
        id
    };

    // ---- the writer: the queue to the socket; a dropped queue closes as too slow ----
    let writer_slow = slow.clone();
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let Out::Text(t) = out;
            if sink.send(Message::Text(t)).await.is_err() {
                return;
            }
        }
        if writer_slow.load(Ordering::Relaxed) {
            let _ = sink
                .send(Message::Close(Some(CloseFrame {
                    code: CLOSE_SLOW,
                    reason: "behind: reconnect to catch up".into(),
                })))
                .await;
        }
        let _ = sink.close().await;
    });

    // ---- the reader ----
    let who = account.as_ref().map(|a| a.id);
    loop {
        let next = tokio::time::timeout(IDLE, stream.next()).await;
        let text = match next {
            Ok(Some(Ok(Message::Text(t)))) => t,
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            _ => break,
        };
        if slow.load(Ordering::Relaxed) {
            break;
        }
        if text.len() > MAX_FRAME {
            reply(
                &chat,
                conn,
                ServerFrame::Error {
                    words: "That frame is too large.".into(),
                },
            );
            continue;
        }
        let frame = match serde_json::from_str::<ClientFrame>(&text) {
            Ok(f) => f,
            Err(_) => {
                reply(
                    &chat,
                    conn,
                    ServerFrame::Error {
                        words: "The chat did not understand that frame.".into(),
                    },
                );
                continue;
            }
        };
        match frame {
            ClientFrame::Ping => reply(&chat, conn, ServerFrame::Pong),
            ClientFrame::Hello { .. } => {}
            ClientFrame::Post {
                nonce,
                kind,
                body,
                reply_to,
                playhead_ms,
            } => {
                let nonce: String = nonce.chars().take(64).collect();
                let Some(who) = who else {
                    reply(
                        &chat,
                        conn,
                        ServerFrame::Ack {
                            nonce,
                            id: None,
                            state: crate::wire::AckState::Refused,
                            code: Some("signed_out".into()),
                            words: Some("Sign in to post. Reading needs no account.".into()),
                            retry_ms: None,
                        },
                    );
                    continue;
                };
                let a = chat
                    .post(
                        who,
                        PostRequest {
                            nonce,
                            kind,
                            body,
                            reply_to,
                            playhead_ms,
                        },
                    )
                    .await;
                reply(&chat, conn, a);
            }
            ClientFrame::React { id, emoji } => {
                let Some(who) = who else { continue };
                if let Some(problem) = chat.react(who, id, &emoji) {
                    reply(&chat, conn, problem);
                }
            }
        }
    }
    chat.room.lock().expect("room").unregister(conn);
    writer.abort();
}

fn reply(chat: &Chat, conn: u64, f: ServerFrame) {
    chat.room.lock().expect("room").send_to(conn, &f);
}
