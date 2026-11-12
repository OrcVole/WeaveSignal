// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Chatters' avatars (ADR-0235).
//!
//! **The shipped set.** Seventy-two avatars come with WeaveSignal (`packages/ui/avatars/`); a
//! chatter chooses one by its id, and the list the chat accepts is the same `manifest.json` the
//! pages draw from, built into this program.
//!
//! **A picture of their own.** A chatter uploads a PNG or JPEG (the page crops it to a square
//! first). The upload is never parsed here: the media worker, this program run again as a
//! restricted process (ADR-0048 rule 2), decodes it and writes a small PNG that holds nothing
//! but pixels. That PNG is kept on disk, `<avatars.dir>/<id>.png`, with a row in `chat_avatar`
//! (migration 0026) that says whose it is and where it stands:
//!
//! - **held**: nobody sees it but its owner and the station's staff and moderators, until one of
//!   them approves it on the Screener tab (ADR-0255). Until then everyone else sees the shipped
//!   avatar the chatter chose, or their initial. One nobody decides on in `avatars.wait_days`
//!   (30) is deleted by the retention task and its owner told (ADR-0254).
//! - **approved**: shown beside the chatter's messages; staff or a moderator can remove it
//!   later.
//! - refused or removed: deleted at once, row and file, and the chatter gets a statement of
//!   reasons like any other moderation decision. Only where an evidence hold names the account
//!   (or the reason is one that preserves evidence at once) is the file kept, **withheld** from
//!   everyone but staff, until the hold is released, and staff may delete it at once (ADR-0257).
//!
//! **Originals and evidence (ADR-0257).** The upload as it arrived is kept beside the clean PNG,
//! `<id>.orig`, while the picture waits, and never served; approving it deletes the original, so an approved picture keeps only its clean copy. A picture refused or removed as
//! *suspected child sexual abuse material* is withheld like any evidence, and also keeps that
//! original, a snapshot of the uploader and the decision (`chat_avatar.evidence`), and stays at
//! least `avatars.evidence_days` (a year) whatever happens to the hold; only the one person the
//! chat's settings name (`evidence_viewer`) can see or export it. The same `withheld` state and
//! evidence hold as for the other reasons, not a second mechanism.
//!
//! Whether a picture may be fetched is decided from its row on every request, never from its
//! address being hard to guess.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use weavesignal_model::StationId;

use crate::legal::{self, Outcome, Reason};
use crate::rules::{Kind, Restriction};
use crate::service::{Chat, Problem, me_of, now_ms};
use crate::store::{self, Account, Db, NewAction};
use crate::wire::{AccountRef, HeldAvatar, Me, ServerFrame};

/// Who the moderation record names for what the chat does by itself.
const EXPIRY_ACTOR: &str = "the chat (automatic)";

// ---- the shipped set ----

/// The shipped avatars' list, the file the pages read too.
const MANIFEST: &str = include_str!("../../../packages/ui/avatars/manifest.json");

#[derive(Deserialize)]
struct Manifest {
    avatars: Vec<Stock>,
}

#[derive(Deserialize)]
struct Stock {
    id: String,
}

/// The ids of the shipped avatars.
pub fn stock_ids() -> &'static [String] {
    static IDS: OnceLock<Vec<String>> = OnceLock::new();
    IDS.get_or_init(|| {
        serde_json::from_str::<Manifest>(MANIFEST)
            .expect("packages/ui/avatars/manifest.json parses")
            .avatars
            .into_iter()
            .map(|a| a.id)
            .collect()
    })
}

pub fn is_stock(id: &str) -> bool {
    stock_ids().iter().any(|s| s == id)
}

// ---- settings ----

/// `[avatars]` in `chat.toml`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct AvatarConfig {
    /// Chatters may upload a picture of their own; false leaves the shipped avatars only. This is
    /// the default until the studio's switch is used (ADR-0259, `avatar_uploads` in the chat's
    /// settings).
    pub uploads: bool,
    /// Where uploaded pictures are kept, one small PNG each: a directory this service can write
    /// that is part of the station's backups.
    pub dir: PathBuf,
    /// The largest upload taken, in KiB. The page sends a cropped square far smaller.
    pub max_upload_kb: u32,
    /// An avatar is rewritten as a PNG this many pixels square.
    pub side: u32,
    /// Uploads one chatter may make in ten minutes (0 is off).
    pub uploads_per_chatter: u32,
    /// Pictures that may wait for staff at once; more are refused until some are decided, so
    /// a flood cannot fill the disk or bury the queue.
    pub max_waiting: u32,
    /// A picture nobody has decided on after this many days is deleted and its owner told
    /// (ADR-0254).
    pub wait_days: u32,
    /// A picture refused as suspected child sexual abuse material is kept, with its original and
    /// the uploader's details, at least this many days (ADR-0257; the law asks for a year).
    pub evidence_days: u32,
    /// The media worker's program; this program itself when not set.
    pub worker: Option<PathBuf>,
    pub worker_timeout_s: u64,
}

impl Default for AvatarConfig {
    fn default() -> Self {
        Self {
            uploads: true,
            dir: PathBuf::from("/var/lib/weavesignal/chat-avatars"),
            max_upload_kb: 2048,
            side: 128,
            uploads_per_chatter: 5,
            max_waiting: 500,
            wait_days: 30,
            evidence_days: 366,
            worker: None,
            worker_timeout_s: 20,
        }
    }
}

impl AvatarConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(64..=8192).contains(&self.max_upload_kb)
            || !(64..=256).contains(&self.side)
            || !(1..=100_000).contains(&self.max_waiting)
            || !(1..=365).contains(&self.wait_days)
            || !(366..=3650).contains(&self.evidence_days)
            || !(1..=120).contains(&self.worker_timeout_s)
        {
            return Err(
                "`avatars`: max_upload_kb 64-8192, side 64-256, max_waiting 1-100000, wait_days 1-365, evidence_days 366-3650, worker_timeout_s 1-120"
                    .into(),
            );
        }
        if self.dir.as_os_str().is_empty() {
            return Err("`avatars.dir` is empty: name a directory the chat can write".into());
        }
        Ok(())
    }

    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_kb as usize * 1024
    }

    fn file(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.png", id.hyphenated()))
    }

    /// The upload as it arrived (ADR-0257): never served, only exported as evidence.
    fn original(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.orig", id.hyphenated()))
    }
}

/// What the pages need to know about avatars, in `GET /api/v1/chat/info`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AvatarInfo {
    /// Signed-in chatters (not guests) may upload a picture of their own.
    pub uploads: bool,
    pub max_upload_kb: u32,
    pub side: u32,
}

/// A picture the station is keeping out of view (ADR-0257), as the studio's people list it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct KeptPicture {
    pub id: String,
    pub account: KeptOwner,
    pub reason: Option<String>,
    pub reason_words: Option<&'static str>,
    pub uploaded_ms: i64,
    pub decided_ms: Option<i64>,
    pub decided_by: Option<String>,
    /// The earliest it may be deleted by the sweep (evidence only).
    pub keep_until_ms: Option<i64>,
    /// Kept as evidence of suspected child sexual abuse material: only the named person sees it.
    pub evidence: bool,
    pub has_original: bool,
    /// `avatar`, `picture` or `document` (ADR-0262).
    pub kind: String,
    /// A document's name as its chatter had it (shown as text).
    pub file_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct KeptOwner {
    pub id: String,
    pub name: String,
}

/// What deleting a picture kept as evidence says before it asks for the confirmation.
pub const EVIDENCE_DELETE_WORDS: &str = "This picture is kept as evidence of suspected child sexual abuse material. The law asks you to report it and then keep it, for at least a year. Delete it only if you have decided that is right for the station; the deletion is recorded. Confirm to go on.";

/// A reason as a moderator reads it, from what the table keeps.
fn reason_words(s: &str) -> Option<&'static str> {
    Reason::ALL
        .into_iter()
        .chain(Reason::STAFF)
        .find(|r| r.as_str() == s)
        .map(Reason::words)
}

/// A time as the README writes it: UTC, to the minute.
fn format_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}

// ---- the media worker ----

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub(crate) enum WorkerError {
    /// What the worker said is wrong with the picture: the chatter's to fix.
    Refused(String),
    /// The worker could not run, or broke: the station's to fix.
    Failed(String),
}

/// Run the media worker on one upload: the clean PNG it wrote.
async fn clean(cfg: &AvatarConfig, bytes: Vec<u8>) -> Result<Vec<u8>, WorkerError> {
    let side = cfg.side.to_string();
    let (out, _) = run_worker(
        cfg.worker.as_deref(),
        cfg.worker_timeout_s,
        "avatars.worker_timeout_s",
        &["avatar", "--side", &side],
        bytes,
    )
    .await?;
    if !out.starts_with(PNG_SIGNATURE) {
        return Err(WorkerError::Failed("its output is not a PNG".into()));
    }
    Ok(out)
}

/// Run the media worker (`media-worker <args>`, this program again unless `worker` names
/// another) on one upload, within `timeout_s`: what it wrote, and its report.
pub(crate) async fn run_worker(
    worker: Option<&Path>,
    timeout_s: u64,
    timeout_setting: &str,
    args: &[&str],
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, serde_json::Value), WorkerError> {
    let exe = match worker {
        Some(p) => p.to_owned(),
        None => std::env::current_exe()
            .map_err(|e| WorkerError::Failed(format!("cannot find this program: {e}")))?,
    };
    let mut child = tokio::process::Command::new(&exe)
        .arg("media-worker")
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| WorkerError::Failed(format!("cannot start {}: {e}", exe.display())))?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let feed = async move {
        // The worker may refuse early and close its input; that is its answer, not ours.
        let _ = stdin.write_all(&bytes).await;
        drop(stdin);
    };
    let read_out = async move {
        let mut v = Vec::new();
        stdout.read_to_end(&mut v).await.map(|_| v)
    };
    let read_err = async move {
        let mut v = Vec::new();
        stderr.read_to_end(&mut v).await.map(|_| v)
    };
    let work = async {
        let ((), out, err, status) = tokio::join!(feed, read_out, read_err, child.wait());
        (out, err, status)
    };
    let (out, err, status) = tokio::time::timeout(Duration::from_secs(timeout_s), work)
        .await
        .map_err(|_| {
            WorkerError::Failed(format!("no result in {timeout_s} s (`{timeout_setting}`)"))
        })?;
    let io = |e: std::io::Error| WorkerError::Failed(e.to_string());
    let (out, err, status) = (out.map_err(io)?, err.map_err(io)?, status.map_err(io)?);
    let text = String::from_utf8_lossy(&err);
    let last = text
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(last).unwrap_or_default();
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(if status.code() == Some(3) {
            WorkerError::Refused(e.to_owned())
        } else {
            WorkerError::Failed(e.to_owned())
        });
    }
    if !status.success() {
        return Err(WorkerError::Failed(format!(
            "it stopped ({status}): {}",
            text.trim().chars().take(300).collect::<String>()
        )));
    }
    let sandbox = json["sandbox"].as_str().unwrap_or("unknown");
    if sandbox != "landlock" {
        tracing::warn!(%sandbox, "the media worker ran without a full Landlock sandbox");
    }
    Ok((out, json))
}

/// The worker's refusal, in words for the person who chose the file.
fn refusal_words(e: &str) -> String {
    if e.starts_with("not a picture") {
        "That file isn't a picture this chat takes. Choose a PNG or JPEG photo or drawing.".into()
    } else if e.contains("the least is") {
        "That picture is too small to see. Choose one at least 32 pixels on each side.".into()
    } else if e.contains("the most is") || e.contains("larger than") {
        "That picture is too large. Choose a smaller one, or crop it first.".into()
    } else {
        "That picture couldn't be read; the file may be damaged. Try another.".into()
    }
}

// ---- the rows ----

/// An uploaded picture's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: Uuid,
    pub account: Uuid,
    pub state: String,
    pub bytes: i32,
    pub uploaded_ms: i64,
    /// Kept as evidence of suspected child sexual abuse material (ADR-0257).
    pub evidence: bool,
    /// The upload as it arrived is on disk (`<id>.orig`).
    pub has_original: bool,
    /// `avatar`, `picture` or `document` (ADR-0262).
    pub kind: String,
}

impl Row {
    /// A picture or document posted in the chat, not an avatar.
    pub fn is_attachment(&self) -> bool {
        self.kind != "avatar"
    }
}

type RowTuple = (Uuid, Uuid, String, i32, i64, bool, bool, String);

const COLS: &str = "id, account_id, state, bytes, (extract(epoch FROM uploaded_at) * 1000)::bigint,
    (evidence IS NOT NULL), has_original, kind";

fn row(r: RowTuple) -> Row {
    Row {
        id: r.0,
        account: r.1,
        state: r.2,
        bytes: r.3,
        uploaded_ms: r.4,
        evidence: r.5,
        has_original: r.6,
        kind: r.7,
    }
}

pub async fn by_id(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<Option<Row>> {
    let q = format!("SELECT {COLS} FROM chat_avatar WHERE station_id = $1 AND id = $2");
    Ok(sqlx::query_as::<_, RowTuple>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(id)
        .fetch_optional(db)
        .await?
        .map(row))
}

/// One account's picture in a state, if it has one.
async fn of_account(
    db: &Db,
    st: StationId,
    account: Uuid,
    state: &str,
) -> sqlx::Result<Option<Row>> {
    let q = format!(
        "SELECT {COLS} FROM chat_avatar WHERE station_id = $1 AND account_id = $2 AND state = $3
           AND kind = 'avatar'
         ORDER BY uploaded_at DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, RowTuple>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .bind(account)
        .bind(state)
        .fetch_optional(db)
        .await?
        .map(row))
}

/// Every picture kept for an account, whatever its state.
pub async fn ids_of(db: &Db, st: StationId, account: Uuid) -> sqlx::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM chat_avatar WHERE station_id = $1 AND account_id = $2")
            .bind(st.as_uuid())
            .bind(account)
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn waiting(db: &Db, st: StationId) -> sqlx::Result<Vec<Row>> {
    let q = format!(
        "SELECT {COLS} FROM chat_avatar WHERE station_id = $1 AND state = 'held'
         ORDER BY uploaded_at LIMIT 200"
    );
    Ok(sqlx::query_as::<_, RowTuple>(sqlx::AssertSqlSafe(q))
        .bind(st.as_uuid())
        .fetch_all(db)
        .await?
        .into_iter()
        .map(row)
        .collect())
}

pub(crate) async fn delete(db: &Db, st: StationId, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM chat_avatar WHERE station_id = $1 AND id = $2")
        .bind(st.as_uuid())
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Whether an open evidence hold names the account.
pub(crate) async fn under_hold(db: &Db, st: StationId, account: Uuid) -> sqlx::Result<bool> {
    let r: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM chat_hold WHERE station_id = $1 AND account_id = $2
                        AND released_at IS NULL)",
    )
    .bind(st.as_uuid())
    .bind(account)
    .fetch_one(db)
    .await?;
    Ok(r.0)
}

/// Who asks for a picture's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// Anyone: approved pictures only.
    Public,
    /// The signed-in chatter: their own, held or approved.
    Owner(Uuid),
    /// The studio's people: any, a withheld one included, but not one kept as evidence of
    /// suspected child sexual abuse material, which only the named person sees (ADR-0257).
    Staff,
    /// A chatter the station made a moderator (ADR-0255): the ones waiting and the ones showing,
    /// never one withheld.
    Moderator,
}

pub(crate) fn account_ref(a: &Account, restriction: Option<Restriction>) -> AccountRef {
    AccountRef {
        id: a.id.to_string(),
        name: a.name.clone(),
        since_ms: a.created_ms,
        standing: a.standing,
        guest: a.kind == Kind::Guest,
        restricted: restriction.map(|x| match x {
            Restriction::Banned => "banned".to_owned(),
            Restriction::TimedOut { .. } => "timed out".to_owned(),
        }),
        avatar: a.avatar(),
    }
}

impl Chat {
    pub fn avatar_info(&self) -> AvatarInfo {
        let c = &self.config.avatars;
        AvatarInfo {
            uploads: self.avatar_uploads_on(),
            max_upload_kb: c.max_upload_kb,
            side: c.side,
        }
    }

    /// Read the account again, bring the room's memory of it up to date, tell its owner, and,
    /// when what everyone sees changed, tell every page.
    async fn avatar_changed(&self, account: Uuid, public: bool) -> Result<Me, Problem> {
        let a = store::account_by_id(&self.db, self.station.id, account)
            .await?
            .ok_or_else(|| Problem::NotFound("No chatter with that id.".into()))?;
        let restriction = store::restriction(&self.db, self.station.id, account).await?;
        let me = me_of(&a, restriction);
        let mut room = self.room();
        if let Some(c) = room.chatters.get_mut(&account) {
            c.account = a.clone();
        }
        if public {
            room.set_avatar(account, a.avatar());
        }
        room.send(
            crate::room::Audience::Account(account),
            &ServerFrame::You { me: me.clone() },
        );
        Ok(me)
    }

    /// A chatter chooses one of the shipped avatars, or none (their initial).
    pub async fn choose_stock(&self, a: &Account, id: Option<&str>) -> Result<Me, Problem> {
        if let Some(id) = id
            && !is_stock(id)
        {
            return Err(Problem::Invalid(
                "That avatar isn't one of this chat's. Choose another.".into(),
            ));
        }
        self.may_change(a).await?;
        sqlx::query("UPDATE chat_account SET avatar_stock = $3 WHERE station_id = $1 AND id = $2")
            .bind(self.station.id.as_uuid())
            .bind(a.id)
            .bind(id)
            .execute(&self.db)
            .await?;
        // With a picture of their own approved, the shipped choice shows nowhere yet.
        self.avatar_changed(a.id, a.avatar_upload.is_none()).await
    }

    async fn may_change(&self, a: &Account) -> Result<(), Problem> {
        if store::restriction(&self.db, self.station.id, a.id)
            .await?
            .is_some()
        {
            return Err(Problem::Forbidden(
                "You can't change your avatar while you're timed out or banned.".into(),
            ));
        }
        Ok(())
    }

    /// A chatter uploads a picture of their own. It replaces any picture of theirs still
    /// waiting; an approved one stays in view until staff approve the new one.
    pub async fn upload_avatar(
        &self,
        a: &Account,
        bytes: Vec<u8>,
        address: Option<std::net::IpAddr>,
    ) -> Result<Me, Problem> {
        let cfg = &self.config.avatars;
        let st = self.station.id;
        if !self.avatar_uploads_on() {
            return Err(Problem::Forbidden(
                "This station's chat uses its own set of avatars. Choose one of those.".into(),
            ));
        }
        if a.kind != Kind::Email {
            return Err(Problem::Forbidden(
                "Sign in with an email address to use a picture of your own. Guests can choose from the set.".into(),
            ));
        }
        self.may_change(a).await?;
        if !self.avatar_uploads.take(a.id, now_ms()) {
            return Err(Problem::TooMany(
                "You've uploaded several pictures in a few minutes. Try again in ten minutes."
                    .into(),
            ));
        }
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM chat_avatar WHERE station_id = $1 AND state = 'held' AND kind = 'avatar'
               AND account_id <> $2",
        )
        .bind(st.as_uuid())
        .bind(a.id)
        .fetch_one(&self.db)
        .await?;
        if n >= i64::from(cfg.max_waiting) {
            return Err(Problem::TooMany(
                "The station has a lot of pictures to look at just now. Try again later.".into(),
            ));
        }
        let png = match clean(cfg, bytes.clone()).await {
            Ok(p) => p,
            Err(WorkerError::Refused(e)) => return Err(Problem::Invalid(refusal_words(&e))),
            Err(WorkerError::Failed(e)) => {
                tracing::error!(error = %e, "the media worker failed on an avatar");
                return Err(Problem::Failed(
                    "The picture couldn't be made ready at the station. Try again in a moment."
                        .into(),
                ));
            }
        };
        let id = Uuid::new_v4();
        let io = |e: std::io::Error| {
            tracing::error!(error = %e, dir = %cfg.dir.display(), "cannot keep an avatar (`avatars.dir`)");
            Problem::Failed(
                "The picture couldn't be kept at the station. Try again in a moment.".into(),
            )
        };
        tokio::fs::create_dir_all(&cfg.dir).await.map_err(io)?;
        let path = cfg.file(id);
        let original = cfg.original(id);
        let tmp = cfg.dir.join(format!(".{}.part", id.hyphenated()));
        // The upload as it arrived stays beside the clean PNG (ADR-0257); only the PNG is served.
        tokio::fs::write(&tmp, &bytes).await.map_err(io)?;
        tokio::fs::rename(&tmp, &original).await.map_err(io)?;
        tokio::fs::write(&tmp, &png).await.map_err(|e| {
            let original = original.clone();
            tokio::spawn(async move { remove(&original).await });
            io(e)
        })?;
        tokio::fs::rename(&tmp, &path).await.map_err(io)?;
        // The side is in the PNG's header (bytes 16 to 20, big-endian).
        let side = i16::try_from(u32::from_be_bytes([png[16], png[17], png[18], png[19]]))
            .unwrap_or(i16::MAX);
        let mut tx = self.db.begin().await?;
        let replaced: Vec<(Uuid,)> = sqlx::query_as(
            "DELETE FROM chat_avatar WHERE station_id = $1 AND account_id = $2 AND state = 'held'
               AND kind = 'avatar' RETURNING id",
        )
        .bind(st.as_uuid())
        .bind(a.id)
        .fetch_all(&mut *tx)
        .await?;
        let inserted = sqlx::query(
            "INSERT INTO chat_avatar (id, station_id, account_id, state, bytes, side, has_original, upload_address)
             VALUES ($1, $2, $3, 'held', $4, $5, true, $6)",
        )
        .bind(id)
        .bind(st.as_uuid())
        .bind(a.id)
        .bind(png.len() as i32)
        .bind(side)
        .bind(address.map(sqlx::types::ipnetwork::IpNetwork::from))
        .execute(&mut *tx)
        .await;
        let committed = match inserted {
            Ok(_) => tx.commit().await,
            Err(e) => Err(e),
        };
        if let Err(e) = committed {
            let _ = tokio::fs::remove_file(&path).await;
            let _ = tokio::fs::remove_file(&original).await;
            return Err(e.into());
        }
        let replaced: Vec<Uuid> = replaced.into_iter().map(|r| r.0).collect();
        self.forget_avatar_files(&replaced).await;
        let restriction = store::restriction(&self.db, st, a.id).await?;
        let held = HeldAvatar {
            id: id.to_string(),
            account: account_ref(a, restriction),
            uploaded_ms: now_ms(),
            bytes: png.len() as u32,
            kind: "avatar".into(),
            attachment: None,
            message: None,
        };
        {
            let mut room = self.room();
            for old in &replaced {
                room.send(
                    crate::room::Audience::Mods,
                    &ServerFrame::ModAvatarDone {
                        id: old.to_string(),
                    },
                );
            }
            room.send(
                crate::room::Audience::Mods,
                &ServerFrame::ModAvatar { a: held },
            );
        }
        self.avatar_changed(a.id, false).await
    }

    /// A chatter takes their own picture away, waiting or approved.
    pub async fn withdraw_avatar(&self, a: &Account) -> Result<Me, Problem> {
        let st = self.station.id;
        // Deleted, under an evidence hold too (the operator, 2026-11-10: as before chat-2).
        let gone: Vec<(Uuid, String)> = sqlx::query_as(
            "DELETE FROM chat_avatar WHERE station_id = $1 AND account_id = $2
               AND state IN ('held', 'approved') AND kind = 'avatar' RETURNING id, state",
        )
        .bind(st.as_uuid())
        .bind(a.id)
        .fetch_all(&self.db)
        .await?;
        let ids: Vec<Uuid> = gone.iter().map(|g| g.0).collect();
        self.forget_avatar_files(&ids).await;
        {
            let mut room = self.room();
            for (id, state) in &gone {
                if state == "held" {
                    room.send(
                        crate::room::Audience::Mods,
                        &ServerFrame::ModAvatarDone { id: id.to_string() },
                    );
                }
            }
        }
        let public = gone.iter().any(|g| g.1 == "approved");
        self.avatar_changed(a.id, public).await
    }

    /// A picture's bytes, for someone allowed to see it; `NotFound` for anyone else, so a held
    /// picture's address says nothing about whether it exists.
    pub async fn avatar_file(&self, id: Uuid, who: Viewer) -> Result<Vec<u8>, Problem> {
        let gone = || Problem::NotFound("No picture there.".into());
        let row = by_id(&self.db, self.station.id, id)
            .await?
            .filter(|r| !r.is_attachment())
            .ok_or_else(gone)?;
        let allowed = match who {
            Viewer::Staff => !row.evidence,
            Viewer::Moderator => row.state != "withheld",
            Viewer::Owner(a) => row.account == a && row.state != "withheld",
            Viewer::Public => row.state == "approved",
        };
        if !allowed {
            return Err(gone());
        }
        tokio::fs::read(self.config.avatars.file(id))
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, %id, "an avatar's file cannot be read (`avatars.dir`)");
                gone()
            })
    }

    /// The signed-in chatter's own picture: the one waiting if there is one, else the approved.
    pub async fn own_avatar(&self, a: &Account) -> Result<(Uuid, Vec<u8>), Problem> {
        let id = a
            .avatar_held
            .or(a.avatar_upload)
            .ok_or_else(|| Problem::NotFound("You have no picture of your own.".into()))?;
        Ok((id, self.avatar_file(id, Viewer::Owner(a.id)).await?))
    }

    /// Pictures waiting for staff, oldest first.
    pub(crate) async fn held_avatars(&self) -> Result<Vec<HeldAvatar>, Problem> {
        let st = self.station.id;
        let mut out = Vec::new();
        for r in waiting(&self.db, st).await? {
            let Some(a) = store::account_by_id(&self.db, st, r.account).await? else {
                continue;
            };
            let restriction = store::restriction(&self.db, st, a.id).await?;
            let (attachment, message) = if r.is_attachment() {
                match crate::attach::held_parts(&self.db, st, r.id).await? {
                    Some((at, m)) => (Some(at), m),
                    // Not sent with a message (yet, or ever): not for anyone to decide.
                    None => continue,
                }
            } else {
                (None, None)
            };
            out.push(HeldAvatar {
                id: r.id.to_string(),
                account: account_ref(&a, restriction),
                uploaded_ms: r.uploaded_ms,
                bytes: r.bytes.max(0) as u32,
                kind: r.kind.clone(),
                attachment,
                message,
            });
        }
        Ok(out)
    }

    pub(crate) fn mod_done(&self, by: &str, what: String) -> String {
        self.room().send(
            crate::room::Audience::Mods,
            &ServerFrame::ModDone {
                by: by.to_owned(),
                what: what.clone(),
                at_ms: now_ms(),
            },
        );
        what
    }

    pub(crate) fn avatar_queue_done(&self, id: Uuid) {
        self.room().send(
            crate::room::Audience::Mods,
            &ServerFrame::ModAvatarDone { id: id.to_string() },
        );
    }

    async fn held_row(&self, id: Uuid) -> Result<(Row, Account), Problem> {
        let st = self.station.id;
        let row = by_id(&self.db, st, id)
            .await?
            .filter(|r| r.state == "held")
            .ok_or_else(|| {
                Problem::Conflict(
                    "That picture is no longer waiting: it was decided, replaced or withdrawn."
                        .into(),
                )
            })?;
        let a = store::account_by_id(&self.db, st, row.account)
            .await?
            .ok_or_else(|| Problem::NotFound("No chatter with that id.".into()))?;
        Ok((row, a))
    }

    /// Staff or a moderator approve a waiting picture: it takes the place of any approved before it.
    pub(crate) async fn avatar_approve(
        &self,
        actor: &crate::service::Actor,
        id: Uuid,
    ) -> Result<String, Problem> {
        let st = self.station.id;
        let by = actor.name.as_str();
        let (row, a) = self.held_row(id).await?;
        if row.is_attachment() {
            return self.attachment_approve(actor, &row, &a).await;
        }
        let mut tx = self.db.begin().await?;
        let old: Vec<(Uuid,)> = sqlx::query_as(
            "DELETE FROM chat_avatar WHERE station_id = $1 AND account_id = $2 AND state = 'approved'
               AND kind = 'avatar' RETURNING id",
        )
        .bind(st.as_uuid())
        .bind(row.account)
        .fetch_all(&mut *tx)
        .await?;
        let done = sqlx::query(
            "UPDATE chat_avatar SET state = 'approved', decided_at = now(), decided_by = $3
             WHERE station_id = $1 AND id = $2 AND state = 'held'",
        )
        .bind(st.as_uuid())
        .bind(id)
        .bind(by)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Problem::Conflict(
                "That picture is no longer waiting: it was decided, replaced or withdrawn.".into(),
            ));
        }
        tx.commit().await?;
        let old: Vec<Uuid> = old.into_iter().map(|r| r.0).collect();
        self.forget_avatar_files(&old).await;
        // Approved, only the clean copy is kept: the upload as it arrived goes (the operator,
        // 2026-11-10).
        self.drop_original(id, &self.config.avatars.original(id))
            .await?;
        store::log(
            &self.db,
            st,
            &NewAction {
                actor: by,
                action: "avatar_approve",
                account_id: Some(row.account),
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
        self.avatar_queue_done(id);
        self.avatar_changed(row.account, true).await?;
        Ok(self.mod_done(by, format!("Approved {}'s picture", a.name)))
    }

    /// Take a picture out of everyone's sight for good: deleted, unless an evidence hold names
    /// the account or the reason is one that preserves evidence at once; then it is withheld,
    /// for staff only, until the hold is released. A picture refused as suspected child sexual
    /// abuse material is withheld in every case, with its original and a snapshot of the uploader
    /// and the decision, and kept at least `avatars.evidence_days` (ADR-0257).
    pub(crate) async fn take_down(
        &self,
        row: &Row,
        owner: &Account,
        by: &str,
        reason: Reason,
        report: Option<Uuid>,
        note: &str,
    ) -> Result<(), Problem> {
        let st = self.station.id;
        if reason.preserve_at_once() {
            sqlx::query(
                "INSERT INTO chat_hold (id, station_id, report_id, account_id, reason, held_by)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(Uuid::new_v4())
            .bind(st.as_uuid())
            .bind(report)
            .bind(row.account)
            .bind(format!("picture: {}", reason.words()))
            .bind(format!("automatic: {by}'s reason"))
            .execute(&self.db)
            .await?;
        }
        if reason.keeps_evidence() {
            let snapshot = self.evidence_snapshot(row, owner, by, reason, note).await?;
            sqlx::query(
                "UPDATE chat_avatar SET state = 'withheld', decided_at = now(), decided_by = $3,
                        reason = $4, keep_until = now() + make_interval(days => $5::int),
                        evidence = $6
                 WHERE station_id = $1 AND id = $2",
            )
            .bind(st.as_uuid())
            .bind(row.id)
            .bind(by)
            .bind(reason.as_str())
            .bind(self.config.avatars.evidence_days as i32)
            .bind(snapshot)
            .execute(&self.db)
            .await?;
            return Ok(());
        }
        self.delete_or_withhold(row, by, Some(reason.as_str()))
            .await
    }

    /// A picture leaves view: deleted, row and file, unless an open evidence hold names the
    /// account, when it is withheld for staff until the hold is released.
    pub(crate) async fn delete_or_withhold(
        &self,
        row: &Row,
        by: &str,
        reason: Option<&str>,
    ) -> Result<(), Problem> {
        let st = self.station.id;
        if under_hold(&self.db, st, row.account).await? {
            sqlx::query(
                "UPDATE chat_avatar SET state = 'withheld', decided_at = now(), decided_by = $3,
                        reason = $4
                 WHERE station_id = $1 AND id = $2",
            )
            .bind(st.as_uuid())
            .bind(row.id)
            .bind(by)
            .bind(reason)
            .execute(&self.db)
            .await?;
        } else {
            delete(&self.db, st, row.id).await?;
            self.forget_avatar_files(&[row.id]).await;
        }
        Ok(())
    }

    /// What a report to the authorities starts from, made when the picture is refused so that an
    /// erasure, the address sweep or the moderation record's own retention cannot take it
    /// (ADR-0257): the uploader as the chat knows them, where they signed in from if the station
    /// keeps addresses, and the decision. The email address is not in it: the chat keeps only a
    /// keyed hash of it (ADR-0125), and says so in the export.
    async fn evidence_snapshot(
        &self,
        row: &Row,
        owner: &Account,
        by: &str,
        reason: Reason,
        note: &str,
    ) -> Result<serde_json::Value, Problem> {
        let st = self.station.id;
        let (upload_address, side): (Option<String>, Option<i16>) = sqlx::query_as(
            "SELECT host(upload_address), side FROM chat_avatar WHERE station_id = $1 AND id = $2",
        )
        .bind(st.as_uuid())
        .bind(row.id)
        .fetch_one(&self.db)
        .await?;
        let sessions: Vec<(String, i64)> = sqlx::query_as(
            "SELECT host(address), (extract(epoch FROM created_at) * 1000)::bigint FROM chat_session
             WHERE station_id = $1 AND account_id = $2 AND address IS NOT NULL ORDER BY created_at",
        )
        .bind(st.as_uuid())
        .bind(owner.id)
        .fetch_all(&self.db)
        .await?;
        Ok(serde_json::json!({
            "case": row.id,
            "reason": reason.as_str(),
            "reason_words": reason.words(),
            "station": self.station.name,
            "state_before": row.state,
            "account": {
                "id": owner.id,
                "name": owner.name,
                "kind": owner.kind,
                "standing": owner.standing,
                "created_ms": owner.created_ms,
                "email": "not kept: the chat keeps only a keyed hash of the address",
            },
            "picture": {
                "id": row.id,
                "uploaded_ms": row.uploaded_ms,
                "side_px": side,
                "clean_png_bytes": row.bytes,
                "has_original": row.has_original,
                "upload_address": upload_address,
            },
            "session_addresses": sessions
                .into_iter()
                .map(|(address, at_ms)| serde_json::json!({"address": address, "signed_in_ms": at_ms}))
                .collect::<Vec<_>>(),
            "kind": row.kind,
            // A picture or document posted in the chat (ADR-0262): what it was and the message
            // that carried it, kept here because the message goes with the chat's retention.
            "posted": if row.is_attachment() {
                crate::attach::evidence_of(&self.db, st, row.id).await?
            } else {
                serde_json::Value::Null
            },
            "decided_by": by,
            "decided_ms": now_ms(),
            "note": note,
            "moderation": [],
        }))
    }

    /// Put a decision's record into the snapshot of a picture kept as evidence, so the export
    /// has it after the moderation record's own retention has run.
    pub(crate) async fn add_to_evidence(
        &self,
        picture: Uuid,
        entry: serde_json::Value,
    ) -> Result<(), Problem> {
        sqlx::query(
            "UPDATE chat_avatar SET evidence = jsonb_set(evidence, '{moderation}',
                    COALESCE(evidence->'moderation', '[]'::jsonb) || $3::jsonb)
             WHERE station_id = $1 AND id = $2 AND evidence IS NOT NULL",
        )
        .bind(self.station.id.as_uuid())
        .bind(picture)
        .bind(serde_json::Value::Array(vec![entry]))
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Delete the pictures nobody decided on within `avatars.wait_days` (ADR-0254), and tell
    /// their owners: a notice in the same place as any decision about their picture (it is kept
    /// like a statement of reasons, so a chatter who is away reads it when they return), a line
    /// in the moderation record, and every open screener's queue is told. An account under an
    /// evidence hold has its picture withheld, as a refusal would. Returns how many.
    pub async fn expire_waiting_avatars(&self) -> Result<usize, Problem> {
        let st = self.station.id;
        let days = self.config.avatars.wait_days;
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM chat_avatar WHERE station_id = $1 AND state = 'held'
               AND uploaded_at < now() - make_interval(days => $2::int) ORDER BY uploaded_at LIMIT 200",
        )
        .bind(st.as_uuid())
        .bind(days as i32)
        .fetch_all(&self.db)
        .await?;
        let mut done = 0;
        for (id,) in rows {
            let Some(row) = by_id(&self.db, st, id).await?.filter(|r| r.state == "held") else {
                continue;
            };
            let Some(a) = store::account_by_id(&self.db, st, row.account).await? else {
                continue;
            };
            // A picture or document posted in the chat goes with its message (ADR-0262).
            let message_id = if row.is_attachment() {
                crate::attach::message_of(&self.db, st, row.id).await?
            } else {
                None
            };
            self.delete_or_withhold(&row, EXPIRY_ACTOR, None).await?;
            if let Some(m) = message_id {
                self.hide_attachment_message(m, a.id).await?;
            }
            let action = store::log(
                &self.db,
                st,
                &NewAction {
                    actor: EXPIRY_ACTOR,
                    action: if row.is_attachment() {
                        "attachment_expire"
                    } else {
                        "avatar_expire"
                    },
                    account_id: Some(a.id),
                    message_id,
                    report_id: None,
                    reason: "",
                    ground: "",
                    note: &format!("nobody decided on it in {days} days"),
                    automated: true,
                    until_ms: None,
                },
            )
            .await?;
            let text = if row.is_attachment() {
                legal::attachment_expired(&self.station.name, days, row.kind == "document")
            } else {
                legal::avatar_expired(&self.station.name, days)
            };
            let (sid, at_ms, review_by_ms) = store::insert_statement(
                &self.db,
                st,
                a.id,
                action,
                &text,
                true,
                crate::config::REVIEW_DAYS,
            )
            .await?;
            self.room().send(
                crate::room::Audience::Account(a.id),
                &ServerFrame::Statement {
                    s: crate::wire::Statement {
                        id: sid.to_string(),
                        at_ms,
                        text,
                        automated: true,
                        review_by_ms,
                        review_requested: false,
                    },
                },
            );
            self.avatar_queue_done(id);
            if !row.is_attachment() {
                self.avatar_changed(a.id, false).await?;
            }
            done += 1;
        }
        Ok(done)
    }

    /// Log a decision about a picture and send its owner the statement of reasons.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn avatar_statement(
        &self,
        by: &str,
        outcome: Outcome,
        a: &Account,
        picture: Uuid,
        reason: Reason,
        note: &str,
        report: Option<Uuid>,
    ) -> Result<(), Problem> {
        let st = self.station.id;
        let action_name = match outcome {
            Outcome::AvatarRefused => "avatar_refuse",
            Outcome::AttachmentRefused { .. } => "attachment_refuse",
            Outcome::AttachmentRemoved { .. } => "attachment_remove",
            _ => "avatar_remove",
        };
        let message_id = crate::attach::message_of(&self.db, st, picture).await?;
        let action = store::log(
            &self.db,
            st,
            &NewAction {
                actor: by,
                action: action_name,
                account_id: Some(a.id),
                message_id,
                report_id: report,
                reason: reason.as_str(),
                ground: if reason.unlawful() {
                    "unlawful"
                } else {
                    "terms"
                },
                note,
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        let text = (a.kind != Kind::Staff).then(|| {
            legal::statement(
                &self.station.name,
                "",
                None,
                outcome,
                reason,
                false,
                &self.retention(),
            )
        });
        if reason.keeps_evidence() {
            // The decision and what its owner was told go into the snapshot, for the export.
            self.add_to_evidence(
                picture,
                serde_json::json!({
                    "action_id": action,
                    "at_ms": now_ms(),
                    "actor": by,
                    "action": action_name,
                    "reason": reason.as_str(),
                    "ground": "unlawful",
                    "note": note,
                    "statement_to_the_uploader": text,
                }),
            )
            .await?;
        }
        let Some(text) = text else {
            return Ok(());
        };
        let (id, at_ms, review_by_ms) = store::insert_statement(
            &self.db,
            st,
            a.id,
            action,
            &text,
            false,
            crate::config::REVIEW_DAYS,
        )
        .await?;
        self.room().send(
            crate::room::Audience::Account(a.id),
            &ServerFrame::Statement {
                s: crate::wire::Statement {
                    id: id.to_string(),
                    at_ms,
                    text,
                    automated: false,
                    review_by_ms,
                    review_requested: false,
                },
            },
        );
        Ok(())
    }

    /// Staff or a moderator refuse a waiting picture: nobody else ever saw it, and its owner is told why.
    pub(crate) async fn avatar_refuse(
        &self,
        by: &str,
        id: Uuid,
        reason: Reason,
        note: &str,
    ) -> Result<String, Problem> {
        let (row, a) = self.held_row(id).await?;
        if row.is_attachment() {
            return self.attachment_refuse(by, &row, &a, reason, note).await;
        }
        self.take_down(&row, &a, by, reason, None, note).await?;
        self.avatar_statement(by, Outcome::AvatarRefused, &a, id, reason, note, None)
            .await?;
        self.avatar_queue_done(id);
        self.avatar_changed(row.account, false).await?;
        Ok(self.mod_done(
            by,
            format!(
                "Refused {}'s picture · {}",
                a.name,
                reason.words().to_lowercase()
            ),
        ))
    }

    /// A moderator removes a chatter's approved picture: their messages show the shipped
    /// avatar they chose again, and they are told why.
    pub(crate) async fn avatar_remove(
        &self,
        by: &str,
        account: Uuid,
        reason: Reason,
        note: &str,
        report: Option<Uuid>,
    ) -> Result<String, Problem> {
        let st = self.station.id;
        let a = store::account_by_id(&self.db, st, account)
            .await?
            .ok_or_else(|| Problem::NotFound("No chatter with that id.".into()))?;
        let row = of_account(&self.db, st, account, "approved")
            .await?
            .ok_or_else(|| {
                Problem::Conflict(format!(
                    "{} has no picture of their own showing: nothing to remove.",
                    a.name
                ))
            })?;
        // A report about a picture removes that picture, not another that came after it.
        if let Some(r) = report
            && let Some(rr) = store::report_by_id(&self.db, st, r).await?
            && let Some(named) = rr.evidence.pointer("/picture/id").and_then(|v| v.as_str())
            && named != row.id.to_string()
        {
            return Err(Problem::Conflict(format!(
                "{} has uploaded a different picture since the one reported. Look at the new one, and report it if it needs to go.",
                a.name
            )));
        }
        self.take_down(&row, &a, by, reason, report, note).await?;
        self.avatar_statement(by, Outcome::AvatarRemoved, &a, row.id, reason, note, report)
            .await?;
        self.avatar_changed(account, true).await?;
        if let Some(r) = report {
            let closed = store::decide(&self.db, st, r, "actioned", by).await?;
            let mut room = self.room();
            for id in closed {
                room.send(
                    crate::room::Audience::Mods,
                    &ServerFrame::ModItemDone {
                        id: id.to_string(),
                        status: "actioned".into(),
                        by: by.to_owned(),
                    },
                );
            }
        }
        Ok(self.mod_done(
            by,
            format!(
                "Removed {}'s picture · {}",
                a.name,
                reason.words().to_lowercase()
            ),
        ))
    }

    /// The picture of an account that an open report names, while it is still the one showing
    /// (ADR-0260): `None` when nobody reported a picture, or they have uploaded another since.
    pub(crate) async fn reported_picture(&self, account: Uuid) -> Result<Option<String>, Problem> {
        let st = self.station.id;
        let named: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT evidence #>> '{picture,id}' FROM chat_report
             WHERE station_id = $1 AND account_id = $2 AND kind = 'report' AND message_id IS NULL
               AND status IN ('open', 'escalated') AND evidence #>> '{picture,id}' IS NOT NULL
             ORDER BY created_at LIMIT 1",
        )
        .bind(st.as_uuid())
        .bind(account)
        .fetch_optional(&self.db)
        .await?;
        let Some((Some(named),)) = named else {
            return Ok(None);
        };
        let showing = of_account(&self.db, st, account, "approved").await?;
        Ok(showing.map(|r| r.id.to_string()).filter(|id| *id == named))
    }

    // ---- kept pictures, deleting them, and the evidence (ADR-0257) ----

    /// The person who may see and export pictures kept as evidence: the staff account the chat's
    /// settings name, else the station's first admin. An account turned off or gone does not
    /// count; the first admin answers then. `None` only when the station has no admin at all.
    pub async fn evidence_viewer(&self) -> Result<Option<(Uuid, String)>, Problem> {
        let st = self.station.id;
        let named = self.settings.read().expect("settings").evidence_viewer;
        if let Some(id) = named {
            let hit: Option<(Uuid, String)> = sqlx::query_as(
                "SELECT id, name FROM staff_account WHERE station_id = $1 AND id = $2 AND disabled_at IS NULL",
            )
            .bind(st.as_uuid())
            .bind(id)
            .fetch_optional(&self.db)
            .await?;
            if hit.is_some() {
                return Ok(hit);
            }
        }
        Ok(sqlx::query_as(
            "SELECT id, name FROM staff_account
             WHERE station_id = $1 AND role = 'admin' AND disabled_at IS NULL
             ORDER BY created_at, id LIMIT 1",
        )
        .bind(st.as_uuid())
        .fetch_optional(&self.db)
        .await?)
    }

    /// The pictures the station is keeping that are not in view: refused or removed, and withheld
    /// under an evidence hold or as evidence. Newest decision first.
    pub async fn kept_pictures(&self) -> Result<Vec<KeptPicture>, Problem> {
        type R = (
            Uuid,
            Uuid,
            String,
            Option<String>,
            i64,
            Option<i64>,
            Option<String>,
            Option<i64>,
            bool,
            bool,
            String,
            Option<String>,
        );
        let rows: Vec<R> = sqlx::query_as(
            "SELECT v.id, v.account_id, a.name, v.reason,
                    (extract(epoch FROM v.uploaded_at) * 1000)::bigint,
                    (extract(epoch FROM v.decided_at) * 1000)::bigint, v.decided_by,
                    (extract(epoch FROM v.keep_until) * 1000)::bigint,
                    (v.evidence IS NOT NULL), v.has_original, v.kind, v.file_name
             FROM chat_avatar v JOIN chat_account a ON a.id = v.account_id
             WHERE v.station_id = $1 AND v.state = 'withheld'
             ORDER BY v.decided_at DESC NULLS LAST LIMIT 500",
        )
        .bind(self.station.id.as_uuid())
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| KeptPicture {
                id: r.0.to_string(),
                account: KeptOwner {
                    id: r.1.to_string(),
                    name: r.2,
                },
                reason_words: r.3.as_deref().and_then(reason_words),
                reason: r.3,
                uploaded_ms: r.4,
                decided_ms: r.5,
                decided_by: r.6,
                keep_until_ms: r.7,
                evidence: r.8,
                has_original: r.9,
                kind: r.10,
                file_name: r.11,
            })
            .collect())
    }

    /// Delete a kept picture from the server at once (ADR-0257): the row and both files, with a
    /// line in the moderation record that says it was deleted, by whom and why. A picture kept as
    /// evidence of suspected child sexual abuse material is deleted only with the confirmation
    /// that the law asks for a report and then keeping it; the station decides.
    pub(crate) async fn avatar_delete(
        &self,
        by: &str,
        id: Uuid,
        why: &str,
        confirm_evidence: bool,
        may_delete_evidence: bool,
    ) -> Result<String, Problem> {
        let st = self.station.id;
        let why = why.trim();
        if why.is_empty() || why.chars().count() > 500 {
            return Err(Problem::Invalid(
                "Say why you are deleting it, up to 500 characters: it goes in the moderation record."
                    .into(),
            ));
        }
        let row = by_id(&self.db, st, id)
            .await?
            .filter(|r| r.state == "withheld")
            .ok_or_else(|| {
                Problem::Conflict(
                    "Only a picture the station is keeping can be deleted here. A waiting one is refused, a showing one removed."
                        .into(),
                )
            })?;
        if row.evidence && !may_delete_evidence {
            return Err(Problem::Forbidden(
                "Only the person named to see the evidence, or an admin, can delete this.".into(),
            ));
        }
        if row.evidence && !confirm_evidence {
            return Err(Problem::Conflict(EVIDENCE_DELETE_WORDS.into()));
        }
        let a = store::account_by_id(&self.db, st, row.account).await?;
        let (reason,): (Option<String>,) =
            sqlx::query_as("SELECT reason FROM chat_avatar WHERE station_id = $1 AND id = $2")
                .bind(st.as_uuid())
                .bind(id)
                .fetch_one(&self.db)
                .await?;
        delete(&self.db, st, id).await?;
        self.forget_avatar_files(&[id]).await;
        let note = if row.evidence {
            format!("kept as evidence, deleted on confirmation: {why}")
        } else {
            why.to_owned()
        };
        store::log(
            &self.db,
            st,
            &NewAction {
                actor: by,
                action: "avatar_delete",
                account_id: Some(row.account),
                message_id: None,
                report_id: None,
                reason: reason.as_deref().unwrap_or(""),
                ground: "",
                note: &note,
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        Ok(self.mod_done(
            by,
            format!(
                "Deleted {}'s kept picture · {why}",
                a.map_or_else(|| "a chatter".to_owned(), |a| a.name)
            ),
        ))
    }

    /// A picture kept as evidence, for the named person only: the cases, newest first.
    pub async fn evidence_cases(&self) -> Result<Vec<serde_json::Value>, Problem> {
        type Case = (
            Uuid,
            serde_json::Value,
            Option<i64>,
            i64,
            String,
            Option<String>,
        );
        let rows: Vec<Case> = sqlx::query_as(
            "SELECT id, evidence, (extract(epoch FROM keep_until) * 1000)::bigint,
                    (extract(epoch FROM uploaded_at) * 1000)::bigint, kind, file_name
             FROM chat_avatar WHERE station_id = $1 AND evidence IS NOT NULL
             ORDER BY decided_at DESC NULLS LAST",
        )
        .bind(self.station.id.as_uuid())
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, ev, keep, up, kind, file_name)| {
                serde_json::json!({
                    "id": id,
                    "kind": kind,
                    "file_name": file_name,
                    "account": ev["account"]["name"],
                    "account_id": ev["account"]["id"],
                    "uploaded_ms": up,
                    "decided_ms": ev["decided_ms"],
                    "decided_by": ev["decided_by"],
                    "keep_until_ms": keep,
                    "has_original": ev["picture"]["has_original"],
                    "upload_address": ev["picture"]["upload_address"],
                })
            })
            .collect())
    }

    /// What was shown of a picture kept as evidence (its clean PNG, a posted picture as it was
    /// rewritten, or a document as it arrived), with its type, for the named person (the caller has
    /// checked who they are).
    pub async fn evidence_picture(&self, id: Uuid) -> Result<(String, Vec<u8>), Problem> {
        let row = by_id(&self.db, self.station.id, id)
            .await?
            .filter(|r| r.evidence)
            .ok_or_else(|| Problem::NotFound("No picture kept as evidence there.".into()))?;
        let (mime, path) = if row.is_attachment() {
            let (mime, path) = crate::attach::shown_file(
                &self.db,
                self.station.id,
                &self.config.attachments,
                &row,
            )
            .await?;
            (mime, path)
        } else {
            ("image/png".to_owned(), self.config.avatars.file(row.id))
        };
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            tracing::warn!(error = %e, %id, "an evidence file cannot be read (`avatars.dir`, `attachments.dir`)");
            Problem::NotFound("No picture kept as evidence there.".into())
        })?;
        Ok((mime, bytes))
    }

    /// The evidence for a report to the authorities: one ZIP with the original upload, the picture
    /// as shown, the uploader's details and the moderation record, with a README that says what
    /// is and is not in it. Returns the file's name and bytes; the export is logged.
    pub async fn evidence_export(&self, id: Uuid, by: &str) -> Result<(String, Vec<u8>), Problem> {
        use sha2::{Digest, Sha256};
        let st = self.station.id;
        let row = by_id(&self.db, st, id)
            .await?
            .filter(|r| r.evidence)
            .ok_or_else(|| Problem::NotFound("No picture kept as evidence there.".into()))?;
        let (snapshot, keep_until): (serde_json::Value, Option<i64>) = sqlx::query_as(
            "SELECT evidence, (extract(epoch FROM keep_until) * 1000)::bigint
             FROM chat_avatar WHERE station_id = $1 AND id = $2",
        )
        .bind(st.as_uuid())
        .bind(id)
        .fetch_one(&self.db)
        .await?;
        let gone = |what: &str, e: std::io::Error| {
            tracing::error!(error = %e, %id, what, "an evidence file cannot be read (`avatars.dir`)");
            Problem::Failed("The evidence could not be read from the station's storage.".into())
        };
        // What was shown: an avatar's PNG, a posted picture as it was rewritten; a document was
        // shown as it arrived, so it is only the original (ADR-0262).
        let (shown_path, original_path) = if row.is_attachment() {
            let cfg = &self.config.attachments;
            let shown = (row.kind == "picture").then(|| cfg.full(id));
            (shown, cfg.original(id))
        } else {
            (
                Some(self.config.avatars.file(id)),
                self.config.avatars.original(id),
            )
        };
        let png = match &shown_path {
            Some(p) => Some(tokio::fs::read(p).await.map_err(|e| gone("picture", e))?),
            None => None,
        };
        let original = if row.has_original {
            Some(
                tokio::fs::read(original_path)
                    .await
                    .map_err(|e| gone("original", e))?,
            )
        } else {
            None
        };
        // The live moderation record for the account, beside the snapshot made at the decision.
        let record = store::account_record(&self.db, st, row.account).await?;
        let sha = |b: &[u8]| {
            Sha256::digest(b)
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<String>()
        };
        let file_ext = |b: &[u8]| {
            weavesignal_media::attachment::sniff(b)
                .map(|k| k.extension(false))
                .unwrap_or("bin")
        };
        let ext = original.as_deref().map_or("bin", file_ext);
        let shown_ext = png.as_deref().map_or("png", file_ext);
        let shown_name = format!("picture-as-shown.{shown_ext}");
        let what = match row.kind.as_str() {
            "picture" => "picture posted in the chat",
            "document" => "document posted in the chat",
            _ => "picture (a chatter's avatar)",
        };
        let metadata = serde_json::json!({
            "case": id,
            "exported_ms": now_ms(),
            "exported_by": by,
            "keep_until_ms": keep_until,
            "files": {
                "original": original.as_ref().map(|o| serde_json::json!({
                    "name": format!("original-upload.{ext}"), "bytes": o.len(), "sha256": sha(o),
                })),
                "as_shown": png.as_ref().map(|p| serde_json::json!({
                    "name": shown_name, "bytes": p.len(), "sha256": sha(p),
                })),
            },
            "snapshot_at_the_decision": snapshot,
        });
        let readme = format!(
            "Evidence of suspected child sexual abuse material\n\
             ================================================\n\
             Case: {id}\n\
             Station: {station}\n\
             Exported: {when} by {by}\n\
             \n\
             This archive holds what {station} kept about one uploaded {what} that a moderator\n\
             refused or removed as suspected child sexual abuse material (the chat's ADR-0257).\n\
             \n\
             {orig_line}\n\
             {shown_line}\n\
             metadata.json             The uploader as the chat knew them, the picture, the decision, file checksums.\n\
             moderation-record.json    The chat's moderation record for the account, as it stands now.\n\
             \n\
             What is NOT here, and why\n\
             -------------------------\n\
             * The uploader's email address. The chat keeps only a keyed hash of it (so no address can be\n\
               read from the database); the station's mail provider may hold the address.\n\
             * An IP address, unless the station's retention keeps addresses (it keeps none by default);\n\
               metadata.json lists any that it did keep.\n\
             \n\
             Handling\n\
             --------\n\
             * Do not forward, copy, print or screenshot the picture, and do not open it more than you must.\n\
             * Give this archive only to the reporting channel the station's runbook names (the National\n\
               Crime Agency in the UK, NCMEC in the US, the police of the Member State in the EU).\n\
             * The station keeps the originals until {keep}, at least; deleting them earlier is the\n\
               station's decision and is recorded.\n\
             \n\
             This is not legal advice.\n",
            station = self.station.name,
            when = format_ms(now_ms()),
            shown_line = match (&png, row.kind.as_str()) {
                (Some(_), "picture") => format!(
                    "{shown_name}  The picture as the chat re-encoded it and showed it: nothing but pixels."
                ),
                (Some(_), _) => format!(
                    "{shown_name}  The picture as the chat re-encoded it: a small PNG with nothing but pixels."
                ),
                (None, _) =>
                    "(A document is offered as it arrived: the original above is what was shown.)"
                        .to_owned(),
            },
            orig_line = if original.is_some() {
                if row.kind == "document" {
                    format!(
                        "original-upload.{ext}  The document exactly as it arrived (the chat passes documents on unchanged)."
                    )
                } else {
                    format!(
                        "original-upload.{ext}  The upload exactly as it arrived (never shown to anyone)."
                    )
                }
            } else {
                "(No original: an upload's original is deleted when it is approved, so only the re-encoded picture exists.)".to_owned()
            },
            keep = keep_until.map_or_else(|| "a year after the decision".to_owned(), format_ms),
        );
        let original_name = format!("original-upload.{ext}");
        let record_json = serde_json::to_vec_pretty(&record).unwrap_or_default();
        let metadata_json = serde_json::to_vec_pretty(&metadata).unwrap_or_default();
        let mut files: Vec<(&str, &[u8])> = vec![("README.txt", readme.as_bytes())];
        if let Some(o) = &original {
            files.push((original_name.as_str(), o));
        }
        if let Some(p) = &png {
            files.push((shown_name.as_str(), p));
        }
        files.push(("metadata.json", &metadata_json));
        files.push(("moderation-record.json", &record_json));
        let zip = crate::zip::stored(&files);
        store::log(
            &self.db,
            st,
            &NewAction {
                actor: by,
                action: "evidence_export",
                account_id: Some(row.account),
                message_id: None,
                report_id: None,
                reason: Reason::SuspectedCsam.as_str(),
                ground: "",
                note: &format!("{} {id}", row.kind),
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        Ok((format!("evidence-{id}.zip"), zip))
    }

    /// Note that the named person looked at a kept picture (the record is the only audit there
    /// is of who saw it).
    pub async fn evidence_viewed(&self, id: Uuid, by: &str) -> Result<(), Problem> {
        let row = by_id(&self.db, self.station.id, id).await?;
        store::log(
            &self.db,
            self.station.id,
            &NewAction {
                actor: by,
                action: "evidence_view",
                account_id: row.map(|r| r.account),
                message_id: None,
                report_id: None,
                reason: Reason::SuspectedCsam.as_str(),
                ground: "",
                note: &format!("picture {id}"),
                automated: false,
                until_ms: None,
            },
        )
        .await?;
        Ok(())
    }

    // ---- the files ----

    /// Delete an upload's original once it is approved: only the clean copy stays.
    pub(crate) async fn drop_original(&self, id: Uuid, path: &Path) -> Result<(), Problem> {
        sqlx::query(
            "UPDATE chat_avatar SET has_original = false WHERE station_id = $1 AND id = $2",
        )
        .bind(self.station.id.as_uuid())
        .bind(id)
        .execute(&self.db)
        .await?;
        remove(path).await;
        Ok(())
    }

    /// Remove the files of pictures whose rows are gone. A row that is still there (a picture
    /// withheld under an evidence hold) keeps its file.
    pub(crate) async fn forget_avatar_files(&self, ids: &[Uuid]) {
        for id in ids {
            match by_id(&self.db, self.station.id, *id).await {
                Ok(None) => {}
                Ok(Some(_)) => continue,
                Err(e) => {
                    tracing::warn!(error = %e, %id, "cannot check an avatar's row; its file stays for the sweep");
                    continue;
                }
            }
            remove(&self.config.avatars.file(*id)).await;
            remove(&self.config.avatars.original(*id)).await;
            for path in self.config.attachments.files(*id) {
                remove(&path).await;
            }
        }
    }

    /// The directory against the rows (after each retention sweep): a file with no row goes,
    /// so nothing outlives an erasure, a released hold or a crash between the two writes. A
    /// file younger than ten minutes is left alone: its row may not be written yet.
    pub async fn sweep_avatar_files(&self) {
        let avatars = self.config.avatars.dir.clone();
        self.sweep_files_in(&avatars).await;
        // Pictures and documents posted in the chat (ADR-0262), kept by the same rows.
        let attachments = self.config.attachments.dir.clone();
        if attachments != avatars {
            self.sweep_files_in(&attachments).await;
        }
    }

    async fn sweep_files_in(&self, dir: &Path) {
        let mut list = match tokio::fs::read_dir(dir).await {
            Ok(l) => l,
            // No upload yet: the directory is made with the first.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                tracing::warn!(error = %e, dir = %dir.display(), "cannot read the avatar directory (`avatars.dir`)");
                return;
            }
        };
        let mut removed = 0u32;
        while let Ok(Some(entry)) = list.next_entry().await {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let young = entry
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_none_or(|age| age < Duration::from_secs(600));
            if young {
                continue;
            }
            // `<id>.png` and `<id>.orig` (avatars), `<id>.full`, `<id>.thumb` and `<id>.orig`
            // (pictures and documents).
            let id = name
                .split_once('.')
                .filter(|(_, ext)| matches!(*ext, "png" | "orig" | "full" | "thumb"))
                .and_then(|(s, _)| Uuid::parse_str(s).ok());
            let orphan = match id {
                Some(id) => matches!(by_id(&self.db, self.station.id, id).await, Ok(None)),
                // A half-written upload left by a crash.
                None => name.starts_with('.') && name.ends_with(".part"),
            };
            if orphan {
                remove(&path).await;
                removed += 1;
            }
        }
        if removed > 0 {
            tracing::info!(removed, "avatar files with no record removed");
        }
    }
}

async fn remove(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "cannot delete an avatar's file; the sweep tries again")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_set_is_forty_to_eighty_with_ids_fit_for_a_file_name() {
        let ids = stock_ids();
        assert!((40..=80).contains(&ids.len()), "{} avatars", ids.len());
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(seen.insert(id), "{id} is listed twice");
            assert!(
                !id.is_empty()
                    && id.len() <= 40
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{id}"
            );
            let file = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../packages/ui/avatars")
                .join(format!("{id}.webp"));
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert!(
                bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
                "{id} is a WebP"
            );
            assert!(bytes.len() < 8 * 1024, "{id} is {} bytes", bytes.len());
        }
        assert!(is_stock("fox") && !is_stock("../fox") && !is_stock(""));
    }

    #[test]
    fn a_time_is_written_in_utc_to_the_minute() {
        assert_eq!(format_ms(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_ms(1_794_347_354_237), "2026-11-10 21:49 UTC");
        assert_eq!(
            format_ms(951_782_400_000 + 60_000 * 59),
            "2000-02-29 00:59 UTC"
        );
    }

    #[test]
    fn the_settings_have_bounds() {
        assert!(AvatarConfig::default().validate().is_ok());
        for bad in [
            AvatarConfig {
                side: 2048,
                ..Default::default()
            },
            AvatarConfig {
                max_upload_kb: 1,
                ..Default::default()
            },
            AvatarConfig {
                dir: PathBuf::new(),
                ..Default::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_refusal_is_said_in_the_chatters_words() {
        assert!(refusal_words("not a picture this worker reads (PNG or JPEG)").contains("PNG"));
        assert!(
            refusal_words("the picture is 8 by 8 pixels; the least is 32 on a side")
                .contains("too small")
        );
        assert!(
            refusal_words(
                "the picture is 9000 by 9000 pixels; the most is 4096 on a side and 16000000 in all"
            )
            .contains("too large")
        );
        assert!(refusal_words("the file is damaged: the PNG is damaged: x").contains("damaged"));
    }
}
