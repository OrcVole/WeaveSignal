// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Pictures and documents posted in the chat (ADR-0258, ADR-0262).
//!
//! **Off by default.** A station setting (`attachments.uploads`, `CHAT_ATTACHMENTS`) is the
//! default until an admin uses the studio's switch (Settings > Chat pictures), which then decides
//! at once (the pattern of ADR-0259). When on, a **trusted** chatter (the standing that may post
//! links, ADR-0068) signed in by email may attach one file to a message.
//!
//! **What is taken** is decided from the bytes alone ([`weavesignal_media::attachment::sniff`]):
//! a picture (PNG, JPEG, WebP), which the media worker rewrites in its sandbox to a legible size
//! with a thumbnail and nothing but its pixels; or a document (PDF, OpenDocument text or
//! spreadsheet, Markdown or plain text), passed on unchanged and only ever offered as a download
//! under a name the station makes, with its own name shown as text.
//!
//! **Held, in the avatars' queue.** The file is a row in `chat_avatar` (`kind` `picture` or
//! `document`, migration 0031) and its message carries it (`chat_message.attachment`, 0032).
//! Both wait until staff or a moderator approve it on the Screener tab, where it is one more entry
//! in the same queue as avatars: the same approve and refuse, the 30-day wait (ADR-0254), the
//! suspected-CSAM reason with its year of evidence and export (ADR-0257), delete-now, and Report
//! this on a posted one (ADR-0260). Approving it shows the message to everyone; refusing it, or
//! removing it later, takes the message down too. Its files go when its row goes: when its
//! message goes under the station's retention, at an erasure, or at once on a refusal, unless an
//! evidence hold keeps it withheld for staff.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use weavesignal_media::attachment::{Refused, Sniffed, display_name, named_markdown, sniff};
use weavesignal_model::StationId;

use crate::avatar::{Row, Viewer, WorkerError};
use crate::legal::{Outcome, Reason};
use crate::rules::{Kind, MessageKind, Standing};
use crate::service::{Actor, Chat, PostRequest, Problem};
use crate::store::{self, Account, Db, NewAction};
use crate::wire::{AckState, Attachment, HeldMessage, ServerFrame};

/// `[attachments]` in `chat.toml`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct AttachmentConfig {
    /// Trusted chatters may attach pictures and documents. Off unless the station turns it on;
    /// the default until the studio's switch is used (`attachments` in the chat's settings).
    pub uploads: bool,
    /// Where they are kept: a directory this service can write that is part of the backups.
    pub dir: PathBuf,
    /// The largest picture taken, in KiB.
    pub max_picture_kb: u32,
    /// The largest document taken, in KiB.
    pub max_document_kb: u32,
    /// A picture's long side as shown, in pixels (a long strip keeps 800 across instead).
    pub long_side: u32,
    /// The thumbnail fits a square this size.
    pub thumb_side: u32,
    /// Attachments one chatter may post in ten minutes (0 is off).
    pub per_chatter: u32,
    /// Attachments that may wait for staff at once.
    pub max_waiting: u32,
    /// The media worker's program; this program itself when not set.
    pub worker: Option<PathBuf>,
    pub worker_timeout_s: u64,
}

impl Default for AttachmentConfig {
    fn default() -> Self {
        Self {
            uploads: false,
            dir: PathBuf::from("/var/lib/weavesignal/chat-attachments"),
            max_picture_kb: 10 * 1024,
            max_document_kb: 5 * 1024,
            long_side: 1600,
            thumb_side: 480,
            per_chatter: 10,
            max_waiting: 200,
            worker: None,
            worker_timeout_s: 30,
        }
    }
}

impl AttachmentConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(256..=32 * 1024).contains(&self.max_picture_kb)
            || !(64..=20 * 1024).contains(&self.max_document_kb)
            || !(640..=4096).contains(&self.long_side)
            || !(64..=1024).contains(&self.thumb_side)
            || !(1..=100_000).contains(&self.max_waiting)
            || !(1..=120).contains(&self.worker_timeout_s)
        {
            return Err(
                "`attachments`: max_picture_kb 256-32768, max_document_kb 64-20480, long_side 640-4096, thumb_side 64-1024, max_waiting 1-100000, worker_timeout_s 1-120"
                    .into(),
            );
        }
        if self.dir.as_os_str().is_empty() {
            return Err("`attachments.dir` is empty: name a directory the chat can write".into());
        }
        Ok(())
    }

    /// The largest request body the upload route reads.
    pub fn max_upload_bytes(&self) -> usize {
        self.max_picture_kb.max(self.max_document_kb) as usize * 1024
    }

    /// A picture as shown (PNG or JPEG).
    pub fn full(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.full", id.hyphenated()))
    }

    pub fn thumb(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.thumb", id.hyphenated()))
    }

    /// The upload as it arrived: a picture's is never served; a document's is the document.
    pub fn original(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.orig", id.hyphenated()))
    }

    pub fn files(&self, id: Uuid) -> [PathBuf; 3] {
        [self.full(id), self.thumb(id), self.original(id)]
    }
}

/// What the pages need to know, in `GET /api/v1/chat/info`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AttachmentInfo {
    /// Trusted chatters may attach now.
    pub uploads: bool,
    pub max_picture_kb: u32,
    pub max_document_kb: u32,
    /// What the file chooser offers (the station decides by the bytes, not these).
    pub accept: &'static str,
}

/// The file chooser's list: a hint only.
pub const ACCEPT: &str = "image/png,image/jpeg,image/webp,application/pdf,application/vnd.oasis.opendocument.text,application/vnd.oasis.opendocument.spreadsheet,text/markdown,text/plain,.md,.markdown,.txt,.odt,.ods,.pdf,.png,.jpg,.jpeg,.webp";

/// A message with a file, as the route takes it.
#[derive(Clone, Debug, Default)]
pub struct Upload {
    pub nonce: String,
    /// The file's own name: shown as text.
    pub name: String,
    /// The message's words, which may be empty.
    pub body: String,
    pub reply_to: Option<i64>,
    pub playhead_ms: Option<i64>,
}

/// A served file.
pub struct Served {
    pub mime: String,
    pub bytes: Vec<u8>,
    /// Anyone may see it (approved, its message in view): a browser may keep it, asking again.
    pub public: bool,
    /// The name a download is saved under: the station's, never the chatter's.
    pub download_name: String,
    pub id: Uuid,
}

/// Which file of a posted attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Picture,
    Thumb,
    Document,
}

/// A type in words, from the type the station found.
pub fn label(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or("").trim() {
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/webp" => "WebP",
        "application/pdf" => "PDF",
        "application/vnd.oasis.opendocument.text" => "OpenDocument text",
        "application/vnd.oasis.opendocument.spreadsheet" => "OpenDocument spreadsheet",
        "text/markdown" => "Markdown",
        "text/plain" => "Plain text",
        _ => "File",
    }
}

fn extension(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or("").trim() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "application/vnd.oasis.opendocument.text" => "odt",
        "application/vnd.oasis.opendocument.spreadsheet" => "ods",
        "text/markdown" => "md",
        "text/plain" => "txt",
        _ => "bin",
    }
}

/// The columns that describe an attachment, as JSON: the message's own query reads them too.
pub(crate) const JSON: &str = "json_build_object('id', v.id, 'kind', v.kind,
    'name', COALESCE(v.file_name, ''), 'mime', v.mime, 'bytes', v.bytes, 'width', v.width,
    'height', v.height, 'thumb_width', v.thumb_width, 'thumb_height', v.thumb_height)";

/// The JSON of [`JSON`] as the wire's attachment.
pub fn from_json(v: &serde_json::Value) -> Option<Attachment> {
    let mime = v.get("mime")?.as_str()?.to_owned();
    let u = |k: &str| v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32);
    Some(Attachment {
        id: v.get("id")?.as_str()?.to_owned(),
        kind: v.get("kind")?.as_str()?.to_owned(),
        name: v.get("name")?.as_str().unwrap_or("").to_owned(),
        label: label(&mime).to_owned(),
        mime,
        bytes: u("bytes").unwrap_or(0),
        width: u("width"),
        height: u("height"),
        thumb_width: u("thumb_width"),
        thumb_height: u("thumb_height"),
    })
}

/// One attachment, as a message carries it.
pub(crate) async fn describe(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<Attachment>> {
    let q = format!(
        "SELECT {JSON} FROM chat_avatar v WHERE v.station_id = $1 AND v.id = $2 AND v.kind <> 'avatar'"
    );
    let r: Option<(serde_json::Value,)> = sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(r.and_then(|r| from_json(&r.0)))
}

/// The message that carries an attachment, if one does.
pub(crate) async fn message_of(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<i64>> {
    let r: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM chat_message WHERE station_id = $1 AND attachment = $2")
            .bind(st.as_uuid())
            .bind(id)
            .fetch_optional(db)
            .await?;
    Ok(r.map(|r| r.0))
}

/// A waiting attachment as the queue shows it, with its message; `None` when no message carries
/// it (nothing for anyone to decide).
pub(crate) async fn held_parts(
    db: &Db,
    st: StationId,
    id: Uuid,
) -> sqlx::Result<Option<(Attachment, Option<HeldMessage>)>> {
    let Some(at) = describe(db, st, id).await? else {
        return Ok(None);
    };
    let m: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, body FROM chat_message WHERE station_id = $1 AND attachment = $2",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(m.map(|(id, body)| (at, Some(HeldMessage { id, body }))))
}

/// What a snapshot of evidence keeps of a posted attachment: the file as described, and the
/// message that carried it (which goes with the chat's retention; the snapshot does not).
pub(crate) async fn evidence_of(
    db: &Db,
    st: StationId,
    id: Uuid,
) -> sqlx::Result<serde_json::Value> {
    let at = describe(db, st, id).await?;
    let m: Option<(i64, String, i64)> = sqlx::query_as(
        "SELECT id, body, (extract(epoch FROM server_at) * 1000)::bigint FROM chat_message
         WHERE station_id = $1 AND attachment = $2",
    )
    .bind(st.as_uuid())
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(serde_json::json!({
        "file": at,
        "message": m.map(|(id, body, at_ms)| serde_json::json!({"id": id, "body": body, "server_ms": at_ms})),
    }))
}

/// What a posted attachment showed, and where it is: a picture as rewritten, a document as it
/// arrived.
pub(crate) async fn shown_file(
    db: &Db,
    st: StationId,
    cfg: &AttachmentConfig,
    row: &Row,
) -> Result<(String, PathBuf), Problem> {
    let (mime,): (Option<String>,) =
        sqlx::query_as("SELECT mime FROM chat_avatar WHERE station_id = $1 AND id = $2")
            .bind(st.as_uuid())
            .bind(row.id)
            .fetch_one(db)
            .await?;
    let mime = mime.unwrap_or_else(|| "application/octet-stream".into());
    Ok(if row.kind == "picture" {
        (mime, cfg.full(row.id))
    } else {
        (mime, cfg.original(row.id))
    })
}

/// The worker's refusal of a picture, in the chatter's words.
fn picture_refusal(e: &str) -> String {
    if e.starts_with("not a picture") {
        "That picture couldn't be read as a PNG, JPEG or WebP. Try another.".into()
    } else if e.contains("animated") {
        "That picture is animated. Send a still picture.".into()
    } else if e.contains("the least is") {
        "That picture is too small to see.".into()
    } else if e.contains("the most is") || e.contains("larger than") {
        "That picture is too large. Send a smaller one.".into()
    } else {
        "That picture couldn't be read; the file may be damaged. Try another.".into()
    }
}

async fn write_file(
    dir: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{}.part", Uuid::new_v4().hyphenated()));
    tokio::fs::write(&tmp, bytes).await?;
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(())
}

impl Chat {
    /// Whether trusted chatters may attach now (ADR-0262): the studio's switch once used, else
    /// the install setting.
    pub fn attachments_on(&self) -> bool {
        self.settings
            .read()
            .expect("settings")
            .attachments
            .unwrap_or(self.config.attachments.uploads)
    }

    pub fn attachment_info(&self) -> AttachmentInfo {
        let c = &self.config.attachments;
        AttachmentInfo {
            uploads: self.attachments_on(),
            max_picture_kb: c.max_picture_kb,
            max_document_kb: c.max_document_kb,
            accept: ACCEPT,
        }
    }

    /// A trusted chatter posts a message with a picture or document. The file is checked by its
    /// bytes, a picture rewritten by the media worker, and the message stored held with it, for
    /// staff or a moderator to approve. Returns the message's id and the attachment.
    pub async fn post_attachment(
        self: &Arc<Self>,
        a: &Account,
        bytes: Vec<u8>,
        up: Upload,
        address: Option<std::net::IpAddr>,
    ) -> Result<(i64, Attachment), Problem> {
        let cfg = &self.config.attachments;
        let st = self.station.id;
        if !self.attachments_on() {
            return Err(Problem::Forbidden(
                "This station's chat doesn't take pictures or documents.".into(),
            ));
        }
        if a.kind != Kind::Email || a.standing < Standing::Trusted {
            return Err(Problem::Forbidden(
                "Pictures and documents are for trusted chatters. Keep chatting: the station's moderators can make you one."
                    .into(),
            ));
        }
        if store::restriction(&self.db, st, a.id).await?.is_some() {
            return Err(Problem::Forbidden(
                "You can't post while you're timed out or banned.".into(),
            ));
        }
        let sniffed = sniff(&bytes).map_err(|e| match e {
            Refused::Empty => Problem::Invalid("The file is empty.".into()),
            e => Problem::Invalid(e.to_string()),
        })?;
        let (cap_kb, what) = if sniffed.is_picture() {
            (cfg.max_picture_kb, "picture")
        } else {
            (cfg.max_document_kb, "document")
        };
        if bytes.len() > cap_kb as usize * 1024 {
            return Err(Problem::Invalid(format!(
                "That {what} is larger than this chat takes ({} MB at most).",
                (f64::from(cap_kb) / 1024.0 * 10.0).round() / 10.0
            )));
        }
        // The message's own rules first (closed hours, slow mode, too fast, too long), so a file
        // that could not be posted is never worked on.
        if !self.room().chatters.contains_key(&a.id) {
            self.load_chatter(a).await?;
        }
        if let Some((code, words, retry)) = self.precheck(a.id, &up.body, up.reply_to.is_some()) {
            return Err(match code {
                _ if retry.is_some() => Problem::TooMany(words),
                "banned" | "timed_out" | "closed" => Problem::Forbidden(words),
                _ => Problem::Invalid(words),
            });
        }
        if !self.attachment_uploads.take(a.id, crate::service::now_ms()) {
            return Err(Problem::TooMany(
                "You've posted several files in a few minutes. Try again in ten minutes.".into(),
            ));
        }
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM chat_avatar WHERE station_id = $1 AND state = 'held' AND kind <> 'avatar'",
        )
        .bind(st.as_uuid())
        .fetch_one(&self.db)
        .await?;
        if n >= i64::from(cfg.max_waiting) {
            return Err(Problem::TooMany(
                "The station has a lot of files to look at just now. Try again later.".into(),
            ));
        }
        let name = display_name(&up.name);
        let markdown = named_markdown(&name);
        let id = Uuid::new_v4();
        let failed = |e: String| {
            tracing::error!(error = %e, "the media worker failed on a chat picture");
            Problem::Failed(
                "The picture couldn't be made ready at the station. Try again in a moment.".into(),
            )
        };
        // A picture through the worker's sandbox: the picture and its thumbnail, nothing else.
        let picture = if sniffed.is_picture() {
            let long = cfg.long_side.to_string();
            let thumb = cfg.thumb_side.to_string();
            let (out, report) = crate::avatar::run_worker(
                cfg.worker.as_deref(),
                cfg.worker_timeout_s,
                "attachments.worker_timeout_s",
                &["picture", "--long-side", &long, "--thumb-side", &thumb],
                bytes.clone(),
            )
            .await
            .map_err(|e| match e {
                WorkerError::Refused(e) => Problem::Invalid(picture_refusal(&e)),
                WorkerError::Failed(e) => failed(e),
            })?;
            let report: weavesignal_media::picture::PictureReport =
                serde_json::from_value(report).map_err(|e| failed(format!("its report: {e}")))?;
            if report.full.bytes + report.thumb.bytes != out.len() || report.full.bytes == 0 {
                return Err(failed("its output's length is not what it reported".into()));
            }
            let (full, thumb) = out.split_at(report.full.bytes);
            let mime = |f: &str| {
                if f == "jpeg" {
                    "image/jpeg"
                } else {
                    "image/png"
                }
            };
            for (b, f) in [(full, &report.full.format), (thumb, &report.thumb.format)] {
                let ok = match f.as_str() {
                    "png" => b.starts_with(b"\x89PNG\r\n\x1a\n"),
                    "jpeg" => b.starts_with(&[0xff, 0xd8, 0xff]),
                    _ => false,
                };
                if !ok {
                    return Err(failed("its output is not the picture it reported".into()));
                }
            }
            Some((
                full.to_vec(),
                thumb.to_vec(),
                mime(&report.full.format),
                mime(&report.thumb.format),
                report,
            ))
        } else {
            None
        };
        let io = |e: std::io::Error| {
            tracing::error!(error = %e, dir = %cfg.dir.display(), "cannot keep a chat attachment (`attachments.dir`)");
            Problem::Failed(
                "The file couldn't be kept at the station. Try again in a moment.".into(),
            )
        };
        tokio::fs::create_dir_all(&cfg.dir).await.map_err(io)?;
        let forget = |id: Uuid| {
            let paths = cfg.files(id);
            async move {
                for p in paths {
                    let _ = tokio::fs::remove_file(p).await;
                }
            }
        };
        let written = async {
            write_file(&cfg.dir, &cfg.original(id), &bytes).await?;
            if let Some((full, thumb, ..)) = &picture {
                write_file(&cfg.dir, &cfg.full(id), full).await?;
                write_file(&cfg.dir, &cfg.thumb(id), thumb).await?;
            }
            Ok::<(), std::io::Error>(())
        }
        .await;
        if let Err(e) = written {
            forget(id).await;
            return Err(io(e));
        }
        let inserted = match &picture {
            Some((full, thumb, mime, thumb_mime, r)) => {
                sqlx::query(
                    "INSERT INTO chat_avatar (id, station_id, account_id, state, kind, bytes, side, mime,
                        file_name, width, height, thumb_mime, thumb_width, thumb_height, thumb_bytes,
                        has_original, upload_address)
                     VALUES ($1, $2, $3, 'held', 'picture', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, true, $14)",
                )
                .bind(id)
                .bind(st.as_uuid())
                .bind(a.id)
                .bind(full.len() as i32)
                .bind(i16::try_from(r.full.width.max(r.full.height)).unwrap_or(i16::MAX))
                .bind(*mime)
                .bind(&name)
                .bind(r.full.width as i32)
                .bind(r.full.height as i32)
                .bind(*thumb_mime)
                .bind(r.thumb.width as i32)
                .bind(r.thumb.height as i32)
                .bind(thumb.len() as i32)
                .bind(address.map(sqlx::types::ipnetwork::IpNetwork::from))
                .execute(&self.db)
                .await
            }
            None => {
                sqlx::query(
                    "INSERT INTO chat_avatar (id, station_id, account_id, state, kind, bytes, mime,
                        file_name, has_original, upload_address)
                     VALUES ($1, $2, $3, 'held', 'document', $4, $5, $6, true, $7)",
                )
                .bind(id)
                .bind(st.as_uuid())
                .bind(a.id)
                .bind(bytes.len() as i32)
                .bind(sniffed.document_mime(markdown))
                .bind(&name)
                .bind(address.map(sqlx::types::ipnetwork::IpNetwork::from))
                .execute(&self.db)
                .await
            }
        };
        if let Err(e) = inserted {
            forget(id).await;
            return Err(e.into());
        }
        let attachment = describe(&self.db, st, id)
            .await?
            .ok_or_else(|| Problem::Failed("The file couldn't be kept at the station.".into()))?;
        let frame = self
            .post(
                a.id,
                PostRequest {
                    nonce: up.nonce,
                    kind: MessageKind::Chat,
                    body: up.body,
                    reply_to: up.reply_to,
                    playhead_ms: up.playhead_ms,
                    attachment: Some(attachment.clone()),
                },
            )
            .await;
        match frame {
            ServerFrame::Ack {
                state: AckState::Held,
                id: Some(mid),
                ..
            } => Ok((mid, attachment)),
            ServerFrame::Ack {
                code,
                words,
                retry_ms,
                ..
            } => {
                let _ = crate::avatar::delete(&self.db, st, id).await;
                forget(id).await;
                let words =
                    words.unwrap_or_else(|| "The chat could not take that just now.".into());
                Err(match code.as_deref() {
                    _ if retry_ms.is_some() => Problem::TooMany(words),
                    Some("signed_out" | "banned" | "timed_out" | "closed") => {
                        Problem::Forbidden(words)
                    }
                    _ => Problem::Invalid(words),
                })
            }
            _ => {
                let _ = crate::avatar::delete(&self.db, st, id).await;
                forget(id).await;
                Err(Problem::Failed(
                    "The chat could not take that just now.".into(),
                ))
            }
        }
    }

    /// A chatter's own messages whose picture or document still waits for the station, oldest
    /// first: what their page shows as waiting.
    pub async fn own_waiting(&self, account: Uuid) -> Result<Vec<crate::wire::Msg>, Problem> {
        let ids: Vec<(i64,)> = sqlx::query_as(
            "SELECT m.id FROM chat_message m JOIN chat_avatar v ON v.id = m.attachment
             WHERE m.station_id = $1 AND m.account_id = $2 AND m.state = 'held' AND v.state = 'held'
             ORDER BY m.id LIMIT 50",
        )
        .bind(self.station.id.as_uuid())
        .bind(account)
        .fetch_all(&self.db)
        .await?;
        let mut out = Vec::new();
        for (id,) in ids {
            if let Some(r) = store::message(&self.db, self.station.id, id).await? {
                out.push(crate::service::msg_of(&r, None));
            }
        }
        Ok(out)
    }

    /// A posted picture's file (or thumbnail) or a document, for someone allowed to see it;
    /// `NotFound` for anyone else, so an address says nothing about whether it exists.
    pub async fn attachment_file(
        &self,
        id: Uuid,
        who: Viewer,
        which: Which,
    ) -> Result<Served, Problem> {
        let st = self.station.id;
        let gone = || Problem::NotFound("Nothing there.".into());
        let row = crate::avatar::by_id(&self.db, st, id)
            .await?
            .filter(|r| match which {
                Which::Picture | Which::Thumb => r.kind == "picture",
                Which::Document => r.kind == "document",
            })
            .ok_or_else(gone)?;
        let visible = || async {
            let r: Option<(String,)> = sqlx::query_as(
                "SELECT state FROM chat_message WHERE station_id = $1 AND attachment = $2",
            )
            .bind(st.as_uuid())
            .bind(id)
            .fetch_optional(&self.db)
            .await?;
            Ok::<bool, sqlx::Error>(r.is_some_and(|r| r.0 == "visible"))
        };
        let public = row.state == "approved" && visible().await?;
        let allowed = match who {
            Viewer::Public => public,
            Viewer::Owner(a) => public || (row.account == a && row.state != "withheld"),
            Viewer::Staff => !row.evidence,
            Viewer::Moderator => row.state != "withheld",
        };
        if !allowed {
            return Err(gone());
        }
        let (mime, thumb_mime): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT mime, thumb_mime FROM chat_avatar WHERE station_id = $1 AND id = $2",
        )
        .bind(st.as_uuid())
        .bind(id)
        .fetch_one(&self.db)
        .await?;
        let cfg = &self.config.attachments;
        let (path, mime) = match which {
            Which::Picture => (cfg.full(id), mime),
            Which::Thumb => (cfg.thumb(id), thumb_mime),
            Which::Document => (cfg.original(id), mime),
        };
        let mime = mime.unwrap_or_else(|| "application/octet-stream".into());
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            tracing::warn!(error = %e, %id, "a chat attachment's file cannot be read (`attachments.dir`)");
            gone()
        })?;
        let short = &id.simple().to_string()[..8];
        Ok(Served {
            download_name: format!("chat-{}-{short}.{}", row.kind, extension(&mime)),
            mime,
            bytes,
            public: public && who == Viewer::Public,
            id,
        })
    }

    /// Take the message that carried an attachment out of view: everyone's (if it was shown) and
    /// its author's own copy (it was waiting).
    pub(crate) async fn hide_attachment_message(
        &self,
        message: i64,
        owner: Uuid,
    ) -> Result<(), Problem> {
        let changed = store::set_state(&self.db, self.station.id, &[message], "hidden").await?;
        if changed.is_empty() {
            return Ok(());
        }
        let mut room = self.room();
        room.hide(&changed);
        room.send(
            crate::room::Audience::Account(owner),
            &ServerFrame::Rejected { ids: changed },
        );
        Ok(())
    }

    /// Staff or a moderator approve a waiting picture or document: its message is shown to
    /// everyone. Nobody approves their own.
    pub(crate) async fn attachment_approve(
        &self,
        actor: &Actor,
        row: &Row,
        a: &Account,
    ) -> Result<String, Problem> {
        let st = self.station.id;
        let by = actor.name.as_str();
        if actor.account == Some(row.account) {
            return Err(Problem::Forbidden(
                "Someone else must approve a picture or document of your own.".into(),
            ));
        }
        let done = sqlx::query(
            "UPDATE chat_avatar SET state = 'approved', decided_at = now(), decided_by = $3
             WHERE station_id = $1 AND id = $2 AND state = 'held'",
        )
        .bind(st.as_uuid())
        .bind(row.id)
        .bind(by)
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Problem::Conflict(
                "That is no longer waiting: it was decided or withdrawn.".into(),
            ));
        }
        // Approved, only the clean copy is kept: a picture's original goes. A document is passed
        // on as it arrived, so the file it has is its clean copy.
        if row.kind == "picture" {
            self.drop_original(row.id, &self.config.attachments.original(row.id))
                .await?;
        }
        let message = message_of(&self.db, st, row.id).await?;
        if let Some(mid) = message {
            let shown = sqlx::query(
                // Shown from now, so it sits in the room at the time everyone sees it, and the
                // station's retention runs from then.
                "UPDATE chat_message SET state = 'visible', server_at = now()
                 WHERE station_id = $1 AND id = $2 AND state = 'held'",
            )
            .bind(st.as_uuid())
            .bind(mid)
            .execute(&self.db)
            .await?;
            if shown.rows_affected() > 0
                && let Some(r) = store::message(&self.db, st, mid).await?
            {
                let reply_name = match r.reply_to {
                    Some(rid) => self.room().find(rid).map(|m| m.author.name.clone()),
                    None => None,
                };
                let m = crate::service::msg_of(&r, reply_name);
                let mut room = self.room();
                room.show(m);
                room.send(
                    crate::room::Audience::Account(row.account),
                    &ServerFrame::Released { ids: vec![mid] },
                );
            }
        }
        store::log(
            &self.db,
            st,
            &NewAction {
                actor: by,
                action: "attachment_approve",
                account_id: Some(row.account),
                message_id: message,
                report_id: None,
                reason: "",
                ground: "",
                note: &row.kind,
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        self.avatar_queue_done(row.id);
        Ok(self.mod_done(by, format!("Approved {}'s {}", a.name, row.kind)))
    }

    /// Staff or a moderator refuse a waiting picture or document: nobody else saw it, its message
    /// is not shown, and its author is told why.
    pub(crate) async fn attachment_refuse(
        &self,
        by: &str,
        row: &Row,
        a: &Account,
        reason: Reason,
        note: &str,
    ) -> Result<String, Problem> {
        let message = message_of(&self.db, self.station.id, row.id).await?;
        self.take_down(row, a, by, reason, None, note).await?;
        let document = row.kind == "document";
        self.avatar_statement(
            by,
            Outcome::AttachmentRefused { document },
            a,
            row.id,
            reason,
            note,
            None,
        )
        .await?;
        if let Some(m) = message {
            self.hide_attachment_message(m, row.account).await?;
        }
        self.avatar_queue_done(row.id);
        Ok(self.mod_done(
            by,
            format!(
                "Refused {}'s {} · {}",
                a.name,
                row.kind,
                reason.words().to_lowercase()
            ),
        ))
    }

    /// Staff or a moderator take down a picture or document that was approved and shown, and its
    /// message with it; its author is told why. Under a report, the report is closed.
    pub(crate) async fn attachment_remove(
        &self,
        by: &str,
        id: Uuid,
        reason: Reason,
        note: &str,
        report: Option<Uuid>,
    ) -> Result<String, Problem> {
        let st = self.station.id;
        let row = crate::avatar::by_id(&self.db, st, id)
            .await?
            .filter(|r| r.is_attachment() && r.state == "approved")
            .ok_or_else(|| {
                Problem::Conflict(
                    "That picture or document isn't showing: nothing to remove. A waiting one is refused."
                        .into(),
                )
            })?;
        let a = store::account_by_id(&self.db, st, row.account)
            .await?
            .ok_or_else(|| Problem::NotFound("No chatter with that id.".into()))?;
        let message = message_of(&self.db, st, row.id).await?;
        self.take_down(&row, &a, by, reason, report, note).await?;
        let document = row.kind == "document";
        self.avatar_statement(
            by,
            Outcome::AttachmentRemoved { document },
            &a,
            row.id,
            reason,
            note,
            report,
        )
        .await?;
        let mut closed = Vec::new();
        if let Some(m) = message {
            self.hide_attachment_message(m, row.account).await?;
            closed.extend(store::decide_for_message(&self.db, st, m, by).await?);
        }
        if let Some(r) = report {
            closed.extend(store::decide(&self.db, st, r, "actioned", by).await?);
        }
        {
            let mut room = self.room();
            for c in closed {
                room.send(
                    crate::room::Audience::Mods,
                    &ServerFrame::ModItemDone {
                        id: c.to_string(),
                        status: "actioned".into(),
                        by: by.to_owned(),
                    },
                );
            }
        }
        Ok(self.mod_done(
            by,
            format!(
                "Removed {}'s {} · {}",
                a.name,
                row.kind,
                reason.words().to_lowercase()
            ),
        ))
    }

    /// A message taken down by a moderator's Hide takes its attachment with it: deleted, or
    /// withheld for staff where an evidence hold names the account.
    pub(crate) async fn attachment_goes_with(
        &self,
        message: i64,
        by: &str,
        reason: Reason,
    ) -> Result<(), Problem> {
        let st = self.station.id;
        let r: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT attachment FROM chat_message WHERE station_id = $1 AND id = $2")
                .bind(st.as_uuid())
                .bind(message)
                .fetch_optional(&self.db)
                .await?;
        let Some((Some(id),)) = r else {
            return Ok(());
        };
        let Some(row) = crate::avatar::by_id(&self.db, st, id)
            .await?
            .filter(|r| r.state != "withheld")
        else {
            return Ok(());
        };
        let was_waiting = row.state == "held";
        self.delete_or_withhold(&row, by, Some(reason.as_str()))
            .await?;
        if was_waiting {
            self.avatar_queue_done(id);
        }
        Ok(())
    }

    /// A report about a posted picture or document (ADR-0260): its id, while it is still showing.
    pub(crate) async fn reported_attachment(
        &self,
        evidence: &serde_json::Value,
    ) -> Result<Option<String>, Problem> {
        let Some(id) = evidence
            .pointer("/attachment/id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            return Ok(None);
        };
        Ok(crate::avatar::by_id(&self.db, self.station.id, id)
            .await?
            .filter(|r| r.is_attachment() && r.state == "approved")
            .map(|r| r.id.to_string()))
    }
}

/// The station's name for a document's extension, from what its bytes showed (for tests and the
/// docs: the route uses the stored type).
pub fn station_extension(s: Sniffed, name: &str) -> &'static str {
    s.extension(named_markdown(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_are_off_by_default_and_have_bounds() {
        let c = AttachmentConfig::default();
        assert!(!c.uploads, "off unless the station turns it on");
        assert!(c.validate().is_ok());
        assert!(
            AttachmentConfig {
                max_document_kb: 1,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            AttachmentConfig {
                dir: PathBuf::new(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn a_type_has_words_and_the_stations_own_extension() {
        assert_eq!(label("application/pdf"), "PDF");
        assert_eq!(label("text/markdown; charset=utf-8"), "Markdown");
        assert_eq!(extension("text/plain; charset=utf-8"), "txt");
        assert_eq!(
            extension("application/vnd.oasis.opendocument.spreadsheet"),
            "ods"
        );
        assert_eq!(station_extension(Sniffed::Text, "notes.md"), "md");
        assert_eq!(station_extension(Sniffed::Pdf, "evil.png"), "pdf");
    }

    #[test]
    fn json_from_the_database_reads_as_the_wire_attachment() {
        let v = serde_json::json!({"id": "a1", "kind": "picture", "name": "shot.png",
            "mime": "image/png", "bytes": 1234, "width": 1600, "height": 900,
            "thumb_width": 480, "thumb_height": 270});
        let a = from_json(&v).unwrap();
        assert_eq!(
            (a.label.as_str(), a.width, a.thumb_height),
            ("PNG", Some(1600), Some(270))
        );
        let d = serde_json::json!({"id": "d1", "kind": "document", "name": "order.pdf",
            "mime": "application/pdf", "bytes": 9, "width": null, "height": null,
            "thumb_width": null, "thumb_height": null});
        let d = from_json(&d).unwrap();
        assert_eq!((d.kind.as_str(), d.width), ("document", None));
    }
}
