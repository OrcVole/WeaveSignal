// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The control plane's link to the desk: the desk's control socket, one JSON object per line
//! each way (ADR-0088). The link asks for the desk's status every `poll_ms` and passes on
//! commands (on air, mute, cue, talk); a desk that does not answer is reported, not hidden,
//! and the link keeps trying. The cart wall fires and stops its carts through it too, and
//! reads the play events the desk keeps for the per-play log (ADR-0117).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use uuid::Uuid;
use weavesignal_proto::Role;

use weavesignal_model::carts::CartMode;

/// How long one request may take before the desk counts as not answering.
const TIMEOUT: Duration = Duration::from_secs(2);

/// A person as the desk reports them.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskPerson {
    pub identity: String,
    #[serde(default)]
    pub name: String,
    pub role: Role,
    pub on_air: bool,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub cue: bool,
    pub talk: bool,
    #[serde(default)]
    pub talk_to: Option<String>,
    pub present: bool,
    #[serde(default)]
    pub away_ms: Option<u64>,
    pub linked: bool,
    #[serde(default)]
    pub lufs: Option<f32>,
    #[serde(default)]
    pub lufs_peak: Option<f32>,
}

/// Whether playout is connected to the desk's programme output.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct PlayoutLink {
    pub connected: bool,
    pub for_ms: Option<u64>,
}

/// The profanity delay between the desk and playout, as the desk reports it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct DeskDelay {
    /// How far behind the programme listeners are now.
    pub delay_ms: u32,
    /// What it builds back up to; 0 when the delay is off.
    pub target_ms: u32,
    /// Building back up after a dump or a start; the desk refuses a dump meanwhile.
    pub rebuilding: bool,
    /// Dumps since the desk started.
    #[serde(default)]
    pub dumps: u64,
    /// How long ago the last dump was.
    #[serde(default)]
    pub dumped_ms_ago: Option<u64>,
}

/// Which of the desk's cart players a cart sounds in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Pool {
    /// Eight players for one-shots and sweepers.
    Shot,
    /// Two for beds: they loop, and dip under an open microphone.
    Bed,
}

impl Pool {
    pub fn of(mode: CartMode) -> Pool {
        match mode {
            CartMode::Bed => Pool::Bed,
            CartMode::OneShot | CartMode::Sweeper => Pool::Shot,
        }
    }
}

/// One cart sounding at the desk.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskSounding {
    pub cart: String,
    pub pool: Pool,
    pub player: u8,
    pub mode: CartMode,
    /// In the presenters' ears only, not on air.
    pub cue: bool,
    /// The play's number, as its play events carry it.
    pub play: u64,
    pub elapsed_ms: u64,
    pub remaining_ms: Option<u64>,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub stopping: bool,
    #[serde(default)]
    pub ducked: bool,
}

/// The carts in the desk's status.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskCarts {
    /// Beds on air are dipped (a microphone open on air, or a sweeper sounding).
    pub ducked: bool,
    pub mic_open: bool,
    pub sounding: Vec<DeskSounding>,
    /// The number of the desk's last play event.
    pub play_seq: u64,
    /// When the desk started: a desk that started again numbers its events from 1 again.
    #[serde(default)]
    pub started_ms: u64,
}

/// A cart fired or ended at the desk: what the per-play log records.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct PlayEvent {
    /// The event's number at the desk (it starts again when the desk does).
    pub seq: u64,
    /// The play it belongs to: a fire and its end share it.
    pub play: u64,
    /// `fired` or `ended`.
    pub event: String,
    pub cart: String,
    pub mode: CartMode,
    pub pool: Pool,
    pub player: u8,
    /// Cued into the presenters' ears, not aired.
    pub cue: bool,
    /// When, in ms since the Unix epoch, at the desk; listeners hear it the delay later.
    pub at_ms: u64,
    /// How long it sounded (an end only).
    #[serde(default)]
    pub played_ms: Option<u64>,
    /// `ran_out`, `stopped` or `failed` (an end only).
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Where a fired cart sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskFired {
    pub pool: Pool,
    pub player: u8,
    pub play: u64,
}

/// The video path as the desk reports it (M5, ADR-0009): absent with video off.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskVideo {
    /// Who chooses the picture: `{"mode": "auto"}`, `{"mode": "slate"}` (F7), or
    /// `{"mode": "person", "identity": …}`.
    #[serde(default)]
    pub mode: serde_json::Value,
    /// What is on air: `slate` or `person` (with `identity`).
    #[serde(default)]
    pub shot: String,
    #[serde(default)]
    pub identity: Option<String>,
    /// Whose camera has a picture at the desk.
    #[serde(default)]
    pub cameras: Vec<String>,
    /// The encoders and the HLS writer are running.
    #[serde(default)]
    pub hls: bool,
    /// The programme picture is published into the room.
    #[serde(default)]
    pub room: bool,
    /// The slate has a picture (ADR-0010).
    #[serde(default)]
    pub still: bool,
}

/// The station's mode as playout runs it (`programme.mode`, ADR-0045): `live`, the desk first
/// with automation on dead air, or `auto`, automation only (ADR-0183's AUTO lamp).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StationMode {
    #[default]
    Live,
    Auto,
}

impl StationMode {
    /// The settings' word (`live` or `auto`, as `put_settings` checks it).
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "live" => Some(StationMode::Live),
            "auto" => Some(StationMode::Auto),
            _ => None,
        }
    }
}

/// The desk's answer to `status`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DeskStatus {
    pub people: Vec<DeskPerson>,
    #[serde(default)]
    pub playout: PlayoutLink,
    /// The station's mode as the desk was last told it (it starts live); `None` from a desk
    /// too old to take it.
    #[serde(default)]
    pub mode: Option<StationMode>,
    /// The profanity delay; `None` from a desk too old to have one.
    #[serde(default)]
    pub delay: Option<DeskDelay>,
    /// The carts sounding; `None` from a desk too old to play them.
    #[serde(default)]
    pub carts: Option<DeskCarts>,
    /// The video path; `None` with video off (or from a desk without it).
    #[serde(default)]
    pub video: Option<DeskVideo>,
}

/// A command for the desk; the socket's own words.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeskCommand {
    OnAir {
        identity: String,
        on: bool,
    },
    Mute {
        identity: String,
        on: bool,
    },
    Cue {
        identity: String,
        on: bool,
    },
    Talk {
        identity: String,
        on: bool,
    },
    TalkTo {
        identity: String,
        to: String,
        on: bool,
    },
    /// Dump the profanity delay (`Del Del`): listeners jump to the live programme, and the
    /// delay builds back up. Allowed while it rebuilds; refused only when nothing is held.
    Dump,
    /// Who chooses the picture (video on): `auto` follows the audio, `slate` holds the slate
    /// (the desk's F7), `person` holds one camera.
    Picture {
        mode: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<String>,
    },
    /// A line over the picture for `ms` (a featured chat message); `text` empty takes it down.
    Caption {
        #[serde(default)]
        text: Option<String>,
        #[serde(default = "caption_ms")]
        ms: u32,
    },
    /// The slate's text (ADR-0010), from what playout says is playing.
    NowPlaying {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        station: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        show: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artist: Option<String>,
    },
    /// The station's mode (ADR-0045): in auto mode the video's sound is playout's feed whoever
    /// is live at the desk (ADR-0172). Sent when the desk's status says another.
    Mode {
        mode: StationMode,
    },
}

impl DeskCommand {
    /// The person the command acts on; none for a dump.
    pub fn identity(&self) -> Option<&str> {
        match self {
            DeskCommand::OnAir { identity, .. }
            | DeskCommand::Mute { identity, .. }
            | DeskCommand::Cue { identity, .. }
            | DeskCommand::Talk { identity, .. }
            | DeskCommand::TalkTo { identity, .. } => Some(identity),
            DeskCommand::Picture { identity, .. } => identity.as_deref(),
            DeskCommand::Dump
            | DeskCommand::NowPlaying { .. }
            | DeskCommand::Caption { .. }
            | DeskCommand::Mode { .. } => None,
        }
    }
}

fn caption_ms() -> u32 {
    8_000
}

#[derive(Debug, thiserror::Error)]
pub enum DeskError {
    #[error("the desk's control socket is not configured (`desk.socket`)")]
    NotConfigured,
    #[error("the desk is not answering on {path}: {reason}")]
    Unreachable { path: String, reason: String },
    #[error("the desk refused: {0}")]
    Refused(String),
    /// A refusal with the desk's word for it (`busy`, `missing`, `invalid`, `decode`).
    #[error("the desk refused: {message}")]
    Rejected { code: String, message: String },
}

#[derive(Deserialize, utoipa::ToSchema)]
struct Reply {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    people: Option<Vec<DeskPerson>>,
    #[serde(default)]
    playout: Option<PlayoutLink>,
    #[serde(default)]
    mode: Option<StationMode>,
    #[serde(default)]
    delay: Option<DeskDelay>,
    #[serde(default)]
    carts: Option<DeskCarts>,
    #[serde(default)]
    video: Option<DeskVideo>,
    #[serde(default)]
    pool: Option<Pool>,
    #[serde(default)]
    player: Option<u8>,
    #[serde(default)]
    play: Option<u64>,
    #[serde(default)]
    stopped: Option<usize>,
    #[serde(default)]
    plays: Option<Vec<PlayEvent>>,
    #[serde(default)]
    thumbs: Option<std::collections::BTreeMap<String, String>>,
}

impl Reply {
    fn refused(self) -> DeskError {
        let message = self.error.unwrap_or_default();
        match self.code {
            Some(code) => DeskError::Rejected { code, message },
            None => DeskError::Refused(message),
        }
    }
}

struct Conn {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
}

/// One connection to the desk's control socket, made again after any failure.
pub struct DeskClient {
    path: Option<PathBuf>,
    conn: tokio::sync::Mutex<Option<Conn>>,
}

impl DeskClient {
    pub fn new(path: Option<&Path>) -> Self {
        DeskClient {
            path: path.map(Path::to_path_buf),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    pub fn configured(&self) -> bool {
        self.path.is_some()
    }

    async fn ask(&self, line: &str) -> Result<Reply, DeskError> {
        let Some(path) = &self.path else {
            return Err(DeskError::NotConfigured);
        };
        let unreachable = |reason: String| DeskError::Unreachable {
            path: path.display().to_string(),
            reason,
        };
        let mut guard = self.conn.lock().await;
        let exchange = async {
            if guard.is_none() {
                let stream = UnixStream::connect(path).await.map_err(|e| e.to_string())?;
                let (r, w) = stream.into_split();
                *guard = Some(Conn {
                    read: BufReader::new(r),
                    write: w,
                });
            }
            let conn = guard.as_mut().expect("connected above");
            conn.write
                .write_all(format!("{line}\n").as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            let mut answer = String::new();
            let n = conn
                .read
                .read_line(&mut answer)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("the desk closed the connection".to_owned());
            }
            serde_json::from_str::<Reply>(&answer).map_err(|e| format!("not a reply: {e}"))
        };
        match tokio::time::timeout(TIMEOUT, exchange).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(reason)) => {
                *guard = None;
                Err(unreachable(reason))
            }
            Err(_) => {
                *guard = None;
                Err(unreachable(format!("no answer in {} s", TIMEOUT.as_secs())))
            }
        }
    }

    /// Everyone at the desk, and the playout link.
    pub async fn status(&self) -> Result<DeskStatus, DeskError> {
        let r = self.ask(r#"{"op":"status"}"#).await?;
        if !r.ok {
            return Err(DeskError::Refused(r.error.unwrap_or_default()));
        }
        Ok(DeskStatus {
            people: r.people.unwrap_or_default(),
            playout: r.playout.unwrap_or_default(),
            mode: r.mode,
            delay: r.delay,
            carts: r.carts,
            video: r.video,
        })
    }

    pub async fn send(&self, cmd: &DeskCommand) -> Result<(), DeskError> {
        let line = serde_json::to_string(cmd).expect("commands serialise");
        let r = self.ask(&line).await?;
        if r.ok { Ok(()) } else { Err(r.refused()) }
    }

    async fn ask_ok(&self, req: serde_json::Value) -> Result<Reply, DeskError> {
        let r = self.ask(&req.to_string()).await?;
        if r.ok { Ok(r) } else { Err(r.refused()) }
    }

    /// Fire a cart at the desk: its file (a name the media worker gave it) at its stored gain,
    /// on air or cued into the presenters' ears.
    pub async fn cart_fire(
        &self,
        cart: Uuid,
        file: &str,
        gain_db: f32,
        mode: CartMode,
        cue: bool,
        duration_ms: u32,
    ) -> Result<DeskFired, DeskError> {
        let r = self
            .ask_ok(serde_json::json!({
                "op": "cart_fire", "cart": cart.hyphenated().to_string(), "file": file,
                "gain_db": gain_db, "mode": mode, "cue": cue,
                "duration_ms": (mode != CartMode::Bed).then_some(duration_ms),
            }))
            .await?;
        match (r.pool, r.player, r.play) {
            (Some(pool), Some(player), Some(play)) => Ok(DeskFired { pool, player, play }),
            _ => Err(DeskError::Refused(
                "a fire answer without its player".into(),
            )),
        }
    }

    /// Fade out every player sounding a cart; how many.
    pub async fn cart_stop(&self, cart: Uuid) -> Result<usize, DeskError> {
        let r = self
            .ask_ok(serde_json::json!({"op": "cart_stop", "cart": cart.hyphenated().to_string()}))
            .await?;
        Ok(r.stopped.unwrap_or(0))
    }

    /// Fade out every cart (`cue`: only the cued, or only those on air); how many.
    pub async fn cart_stop_all(&self, cue: Option<bool>) -> Result<usize, DeskError> {
        let r = self
            .ask_ok(serde_json::json!({"op": "cart_stop_all", "cue": cue}))
            .await?;
        Ok(r.stopped.unwrap_or(0))
    }

    /// A small JPEG of each camera the desk sees, by identity, in base64 (video on).
    pub async fn thumbs(&self) -> Result<std::collections::BTreeMap<String, String>, DeskError> {
        let r = self.ask_ok(serde_json::json!({"op": "thumbs"})).await?;
        Ok(r.thumbs.unwrap_or_default())
    }

    /// The desk's play events after `after`, oldest first.
    pub async fn cart_plays(&self, after: u64) -> Result<Vec<PlayEvent>, DeskError> {
        let r = self
            .ask_ok(serde_json::json!({"op": "cart_plays", "after": after}))
            .await?;
        Ok(r.plays.unwrap_or_default())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A desk that speaks the socket's cart words, for tests: it keeps what is sounding and the
    //! play events, and never plays a sound.

    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    #[derive(Default)]
    pub struct FakeDesk {
        /// Every request, as sent.
        pub lines: Vec<Value>,
        /// (cart, pool, player, mode, cue, play, stopping)
        pub sounding: Vec<(String, String, u8, String, bool, u64, bool)>,
        pub mic_open: bool,
        /// File names it says are not in its cart directory.
        pub missing: Vec<String>,
        pub plays: Vec<Value>,
        play: u64,
        /// The station's mode as it was last told (`None`: a desk too old to report one).
        pub mode: Option<String>,
    }

    impl FakeDesk {
        /// Ops sent, in order.
        pub fn ops(&self) -> Vec<String> {
            self.lines
                .iter()
                .filter(|l| l["op"] != "status" && l["op"] != "cart_plays")
                .map(|l| l["op"].as_str().unwrap_or_default().to_owned())
                .collect()
        }

        /// The fading carts end, as the desk's pass would end them.
        pub fn finish_fades(&mut self) {
            let gone: Vec<_> = self.sounding.iter().filter(|s| s.6).cloned().collect();
            self.sounding.retain(|s| !s.6);
            for s in gone {
                self.event("ended", &s.0, &s.3, s.4, s.5);
            }
        }

        fn event(&mut self, kind: &str, cart: &str, mode: &str, cue: bool, play: u64) {
            let seq = self.plays.len() as u64 + 1;
            let pool = if mode == "bed" { "bed" } else { "shot" };
            self.plays.push(
                json!({"seq": seq, "play": play, "event": kind, "cart": cart,
                "mode": mode, "pool": pool, "player": 1, "cue": cue, "at_ms": 1_000 + seq,
                "played_ms": if kind == "ended" { Some(500) } else { None },
                "end": if kind == "ended" { Some("stopped") } else { None }}),
            );
        }

        fn answer(&mut self, v: &Value) -> Value {
            self.lines.push(v.clone());
            match v["op"].as_str().unwrap_or_default() {
                "status" => {
                    let sweeping = self
                        .sounding
                        .iter()
                        .any(|s| s.3 == "sweeper" && !s.4 && !s.6);
                    let sounding: Vec<Value> = self
                        .sounding
                        .iter()
                        .map(|s| {
                            json!({"cart": s.0, "pool": s.1, "player": s.2, "mode": s.3,
                                "cue": s.4, "play": s.5, "elapsed_ms": 500,
                                "remaining_ms": if s.3 == "bed" { None } else { Some(2_500) },
                                "loop": s.3 == "bed", "stopping": s.6})
                        })
                        .collect();
                    json!({"ok": true, "people": [], "playout": {"connected": true, "for_ms": 10},
                        "mode": self.mode,
                        "carts": {"ducked": self.mic_open || sweeping, "mic_open": self.mic_open,
                            "sounding": sounding, "play_seq": self.plays.len()}})
                }
                "cart_fire" => {
                    let file = v["file"].as_str().unwrap_or_default();
                    if self.missing.iter().any(|m| m == file) {
                        return json!({"ok": false, "code": "missing", "error": "no file"});
                    }
                    let mode = v["mode"].as_str().unwrap_or_default().to_owned();
                    let (pool, max) = if mode == "bed" {
                        ("bed", 2)
                    } else {
                        ("shot", 8)
                    };
                    let used: Vec<u8> = self
                        .sounding
                        .iter()
                        .filter(|s| s.1 == pool)
                        .map(|s| s.2)
                        .collect();
                    let Some(n) = (1..=max).find(|n| !used.contains(n)) else {
                        return json!({"ok": false, "code": "busy", "error": "every player"});
                    };
                    self.play += 1;
                    let cart = v["cart"].as_str().unwrap_or_default().to_owned();
                    let cue = v["cue"].as_bool().unwrap_or(false);
                    self.sounding.push((
                        cart.clone(),
                        pool.into(),
                        n,
                        mode.clone(),
                        cue,
                        self.play,
                        false,
                    ));
                    self.event("fired", &cart, &mode, cue, self.play);
                    json!({"ok": true, "pool": pool, "player": n, "play": self.play})
                }
                "cart_stop" => {
                    let mut n = 0;
                    for s in self.sounding.iter_mut().filter(|s| s.0 == v["cart"]) {
                        s.6 = true;
                        n += 1;
                    }
                    json!({"ok": true, "stopped": n})
                }
                "cart_stop_all" => {
                    let only = v["cue"].as_bool();
                    let mut n = 0;
                    for s in self
                        .sounding
                        .iter_mut()
                        .filter(|s| only.is_none_or(|c| s.4 == c) && !s.6)
                    {
                        s.6 = true;
                        n += 1;
                    }
                    json!({"ok": true, "stopped": n})
                }
                "mode" if self.mode.is_some() => {
                    self.mode = v["mode"].as_str().map(str::to_owned);
                    json!({"ok": true})
                }
                "mode" => json!({"ok": false, "error": "not a request: unknown variant `mode`"}),
                "cart_plays" => {
                    let after = v["after"].as_u64().unwrap_or(0);
                    let plays: Vec<&Value> = self
                        .plays
                        .iter()
                        .filter(|p| p["seq"].as_u64().unwrap_or(0) > after)
                        .collect();
                    json!({"ok": true, "plays": plays})
                }
                _ => json!({"ok": true}),
            }
        }
    }

    pub fn spawn(path: &std::path::Path) -> Arc<Mutex<FakeDesk>> {
        let state = Arc::new(Mutex::new(FakeDesk::default()));
        let listener = UnixListener::bind(path).unwrap();
        let st = state.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let st = st.clone();
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let v: Value = serde_json::from_str(&line).unwrap();
                        let answer = st.lock().unwrap().answer(&v);
                        w.write_all(format!("{answer}\n").as_bytes()).await.unwrap();
                    }
                });
            }
        });
        state
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::net::UnixListener;

    /// A desk that answers `status` with one person and takes any command for a known one.
    async fn fake_desk(path: &Path) {
        let listener = UnixListener::bind(path).unwrap();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                        let answer = match v["op"].as_str().unwrap() {
                            "status" => serde_json::json!({"ok": true, "people": [{
                                "identity": "eilidh", "name": "Eilidh Fraser-MacLeod",
                                "role": "presenter", "on_air": true, "mute": false, "cue": false,
                                "talk": false, "talk_to": null, "present": true, "away_ms": null,
                                "linked": true, "lufs": -22.6, "lufs_peak": -19.8}],
                                "playout": {"connected": true, "for_ms": 1200}}),
                            _ if v["identity"] == "eilidh" => serde_json::json!({"ok": true}),
                            _ => {
                                serde_json::json!({"ok": false, "error": "`x` is not at the desk"})
                            }
                        };
                        w.write_all(format!("{answer}\n").as_bytes()).await.unwrap();
                    }
                });
            }
        });
    }

    #[tokio::test]
    async fn status_and_commands_go_over_json_lines() {
        let dir = tempdir();
        let path = dir.join("desk.sock");
        fake_desk(&path).await;
        let desk = DeskClient::new(Some(&path));
        let st = desk.status().await.unwrap();
        assert_eq!(st.people[0].name, "Eilidh Fraser-MacLeod");
        assert_eq!(st.people[0].lufs, Some(-22.6));
        assert!(st.playout.connected);
        desk.send(&DeskCommand::Mute {
            identity: "eilidh".into(),
            on: true,
        })
        .await
        .unwrap();
        let refused = desk
            .send(&DeskCommand::OnAir {
                identity: "x".into(),
                on: true,
            })
            .await;
        assert!(matches!(refused, Err(DeskError::Refused(e)) if e.contains("not at the desk")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_missing_desk_is_unreachable_and_unconfigured_is_said() {
        let dir = tempdir();
        let desk = DeskClient::new(Some(&dir.join("none.sock")));
        assert!(matches!(
            desk.status().await,
            Err(DeskError::Unreachable { .. })
        ));
        assert!(matches!(
            DeskClient::new(None).status().await,
            Err(DeskError::NotConfigured)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commands_use_the_sockets_words() {
        let c = DeskCommand::TalkTo {
            identity: "elspeth".into(),
            to: "jessie".into(),
            on: true,
        };
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"op":"talk_to","identity":"elspeth","to":"jessie","on":true}"#
        );
        assert_eq!(c.identity(), Some("elspeth"));
        assert_eq!(
            serde_json::to_string(&DeskCommand::Dump).unwrap(),
            r#"{"op":"dump"}"#
        );
        assert_eq!(DeskCommand::Dump.identity(), None);
        let m = DeskCommand::Mode {
            mode: StationMode::Auto,
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"op":"mode","mode":"auto"}"#
        );
        assert_eq!(m.identity(), None);
        assert_eq!(StationMode::parse("auto"), Some(StationMode::Auto));
        assert_eq!(StationMode::parse("automation"), None);
    }

    pub(crate) fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("weavesignal-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
