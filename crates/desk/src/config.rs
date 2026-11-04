// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk's settings: a TOML file, with the LiveKit secret from the environment.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use weavesignal_mixer::{ChainSettings, DelaySettings, LimiterSettings};
use weavesignal_proto::{Keys, Role};

/// The environment variable that holds LiveKit's API secret.
pub const SECRET_ENV: &str = "WEAVESIGNAL_LIVEKIT_API_SECRET";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub livekit: LiveKit,
    #[serde(default)]
    pub desk: DeskSettings,
    #[serde(default)]
    pub programme: Programme,
    /// Each role's input chain. A role left out takes its default (presenters and DJs: the
    /// spike's compressor, make-up steered by the leveller, and limiter; everyone else:
    /// none); an empty table turns a role's processing off.
    #[serde(default)]
    pub inputs: BTreeMap<Role, ChainSettings>,
    #[serde(default)]
    pub room: RoomWatch,
    #[serde(default)]
    pub control: Control,
    #[serde(default)]
    pub carts: Carts,
    #[serde(default)]
    pub video: Video,
}

/// The video path (M5, ADR-0009): off by default per station. Cameras in the room, the vision
/// mixer following the audio, the slate, AV1 to CMAF HLS, and the programme picture back into
/// the room for the people on the show (ADR-0028).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Video {
    pub enabled: bool,
    /// `auto` (NVENC when this machine has it, else SVT-AV1), `nvenc` or `svt`.
    pub encoder: EncoderChoice,
    /// The renditions, best first, from [`Rung::PRESETS`]; ADR-0027's default is 480p20
    /// alone. Each is one AV1 encoder (one NVENC session).
    pub rungs: Vec<String>,
    /// NVENC sessions the desk may open; GeForce cards allow 12 per machine, shared with
    /// every other program. A rung past it is encoded by SVT-AV1 instead, and says so.
    pub nvenc_sessions: u32,
    /// The codecs the desk accepts from cameras; it decodes VP8, VP9 and AV1, never H.264
    /// or H.265 (no decoder in the desk's image).
    pub camera_codecs: Vec<String>,
    /// How far behind the clock a frame is composed, so every camera's picture for that
    /// moment has arrived (and the picture stays with the voices, which wait up to
    /// `desk.input_prebuffer_ms`).
    pub lag_ms: u32,
    /// A camera with no new picture for this long is treated as off.
    pub stale_ms: u32,
    /// The name shown under a person cut to, for this long; 0: no lower thirds.
    pub lower_third_ms: u32,
    /// The vision mixer's rule.
    pub switch: Switching,
    pub hls: Hls,
    pub room: RoomPicture,
    /// Playout's output back to the desk, for the video stream's sound while nobody is live
    /// (ADR-0172).
    pub playout_feed: PlayoutFeed,
    /// The station's name on the slate until the control plane sends now-playing.
    pub station: String,
    /// The slate's still: a PNG the control plane writes (its `video.still_file`, made by the
    /// media worker), shown on the slate while nobody is on camera (ADR-0010). Watched, so it
    /// can come, change and go while the desk runs; no file, a slate of text.
    pub still_file: PathBuf,
}

/// Playout's output back to the desk (ADR-0172): the video stream's sound (and the audio-only
/// toggle's) is the desk's delayed programme while someone is live, and what playout sends
/// listeners otherwise. Playout sends it as it sends the listener process its feed: PCM in
/// WAV over loopback, Icecast's source protocol, playout connecting and trying again while
/// the desk is away. The desk listens, so either side restarts without the other noticing.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct PlayoutFeed {
    /// With video on; off, the video stream's sound is the desk's programme at all times.
    pub enabled: bool,
    /// Loopback only, like every link between the station's own processes.
    pub bind: IpAddr,
    /// `ports.desk_feed` in the control plane's settings (ADR-0075).
    pub port: u16,
    /// How far ahead of the mixing pass the feed is placed as it arrives, to ride out how it
    /// is sent in bursts; the feed's own lateness against the desk's clock.
    pub cushion_ms: u32,
    /// The switch between the desk and the feed is an equal-power crossfade this long.
    pub crossfade_ms: u32,
    /// After the last person goes off air (as heard after the delay), how long the desk's
    /// programme stays before the feed: playout's copy of the desk arrives a little later
    /// than the desk's own, so switching at once would play the show's last words twice.
    pub off_air_hold_ms: u32,
    /// With no feed for this long, the feed is away and the desk's programme is used.
    pub absent_ms: u32,
}

/// The environment variable that holds the source password playout sends with the feed (the
/// listener process's, `WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD`). Unset, the desk takes the
/// feed from any process on the machine, and says so when it starts.
pub const FEED_PASSWORD_ENV: &str = "WEAVESIGNAL_PLAYOUT_FEED_PASSWORD";

impl Default for PlayoutFeed {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port: 7317,
            cushion_ms: 200,
            crossfade_ms: 300,
            off_air_hold_ms: 3_000,
            absent_ms: 500,
        }
    }
}

impl PlayoutFeed {
    fn validate(&self) -> Result<(), String> {
        if !self.bind.is_loopback() {
            return Err(format!(
                "video.playout_feed.bind is {}; the feed from playout stays on loopback",
                self.bind
            ));
        }
        if self.enabled && self.port == 0 {
            return Err("video.playout_feed.port is 0".into());
        }
        if !(50..=1_000).contains(&self.cushion_ms) {
            return Err("video.playout_feed.cushion_ms must be 50 to 1000".into());
        }
        if !(20..=5_000).contains(&self.crossfade_ms) {
            return Err("video.playout_feed.crossfade_ms must be 20 to 5000".into());
        }
        if self.off_air_hold_ms > 30_000 {
            return Err("video.playout_feed.off_air_hold_ms must be at most 30000".into());
        }
        if !(100..=10_000).contains(&self.absent_ms) {
            return Err("video.playout_feed.absent_ms must be 100 to 10000".into());
        }
        Ok(())
    }
}

impl Default for Video {
    fn default() -> Self {
        Self {
            enabled: false,
            encoder: EncoderChoice::Auto,
            rungs: vec!["480p20".to_owned()],
            nvenc_sessions: 12,
            camera_codecs: vec!["VP8".to_owned(), "AV1".to_owned(), "VP9".to_owned()],
            lag_ms: 100,
            stale_ms: 1_000,
            lower_third_ms: 6_000,
            switch: Switching::default(),
            hls: Hls::default(),
            room: RoomPicture::default(),
            playout_feed: PlayoutFeed::default(),
            station: "WeaveSignal".to_owned(),
            still_file: PathBuf::from("/var/lib/weavesignal/carts/stills/slate.png"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EncoderChoice {
    Auto,
    Nvenc,
    Svt,
}

/// The vision mixer's rule ([`crate::vision::Rule`]).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Switching {
    pub speaking_lufs: f32,
    pub margin_db: f32,
    pub switch_after_ms: u32,
    pub min_shot_ms: u32,
}

impl Default for Switching {
    fn default() -> Self {
        Self {
            speaking_lufs: -45.0,
            margin_db: 3.0,
            switch_after_ms: 600,
            min_shot_ms: 2_000,
        }
    }
}

/// The CMAF HLS the desk writes: one AV1 rendition per rung, Opus audio, 2 s segments.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Hls {
    /// Where the playlists and segments go; the control plane serves them (`/hls/`).
    pub dir: PathBuf,
    /// The segment length, and the keyframe interval.
    pub segment_ms: u32,
    /// Segments listed in each playlist.
    pub playlist_segments: u32,
    /// Segment files kept on disk (more than listed, for players a little behind).
    pub keep_segments: u32,
    /// The audio rendition's Opus bitrate.
    pub opus_bitrate: u32,
}

impl Default for Hls {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("/var/lib/weavesignal/hls"),
            segment_ms: 2_000,
            playlist_segments: 6,
            keep_segments: 12,
            opus_bitrate: 96_000,
        }
    }
}

/// The programme picture published into the room, live (before the delay), for the people on
/// the show (ADR-0028): VP8, which every browser decodes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RoomPicture {
    pub publish: bool,
    /// Thumbnails of the other cameras along the bottom.
    pub thumbnails: bool,
}

impl Default for RoomPicture {
    fn default() -> Self {
        Self {
            publish: true,
            thumbnails: true,
        }
    }
}

/// One rendition's picture and bitrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rung {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub kbps: u32,
}

impl Rung {
    /// 480p20 at 600 kbit/s is ADR-0027's default (bench 01: VMAF 94 to 96 on NVENC). The
    /// lower rungs' bitrates come from the same measurement at their own sizes
    /// (`tests/video/vmaf-rungs.sh`, 2026-10-26).
    pub const PRESETS: [Rung; 3] = [
        Rung {
            name: "480p20",
            width: 854,
            height: 480,
            fps: 20,
            kbps: 600,
        },
        Rung {
            name: "360p20",
            width: 640,
            height: 360,
            fps: 20,
            kbps: 350,
        },
        Rung {
            name: "240p15",
            width: 426,
            height: 240,
            fps: 15,
            kbps: 160,
        },
    ];

    pub fn named(name: &str) -> Option<Rung> {
        Self::PRESETS.iter().copied().find(|r| r.name == name)
    }
}

impl Video {
    /// The rungs, in the order given.
    pub fn rung_list(&self) -> Vec<Rung> {
        self.rungs.iter().filter_map(|n| Rung::named(n)).collect()
    }

    /// The picture the vision mixer composes: the first rung's size and rate.
    pub fn canvas(&self) -> Rung {
        self.rung_list()
            .first()
            .copied()
            .unwrap_or(Rung::PRESETS[0])
    }

    fn validate(&self) -> Result<(), String> {
        if self.rungs.is_empty() {
            return Err("video.rungs is empty".into());
        }
        let names: Vec<&str> = Rung::PRESETS.iter().map(|r| r.name).collect();
        for r in &self.rungs {
            if Rung::named(r).is_none() {
                return Err(format!("video.rungs: `{r}` is not one of {names:?}"));
            }
        }
        let list = self.rung_list();
        if list
            .windows(2)
            .any(|w| w[1].height > w[0].height || w[1].fps > w[0].fps)
        {
            return Err("video.rungs must go from the best down".into());
        }
        for c in &self.camera_codecs {
            if !["VP8", "VP9", "AV1"].contains(&c.as_str()) {
                return Err(format!(
                    "video.camera_codecs: `{c}` cannot be decoded at the desk (VP8, VP9, AV1)"
                ));
            }
        }
        if self.camera_codecs.is_empty() {
            return Err("video.camera_codecs is empty".into());
        }
        if !(20..=1_000).contains(&self.lag_ms) {
            return Err("video.lag_ms must be 20 to 1000".into());
        }
        if !(200..=10_000).contains(&self.stale_ms) {
            return Err("video.stale_ms must be 200 to 10000".into());
        }
        if !(1_000..=10_000).contains(&self.hls.segment_ms)
            || !self.hls.segment_ms.is_multiple_of(1_000)
        {
            return Err("video.hls.segment_ms must be whole seconds, 1000 to 10000".into());
        }
        if self.hls.playlist_segments < 3 || self.hls.keep_segments < self.hls.playlist_segments {
            return Err("video.hls: at least 3 segments listed, and at least as many kept".into());
        }
        if !(6_000..=510_000).contains(&self.hls.opus_bitrate) {
            return Err("video.hls.opus_bitrate must be 6000 to 510000".into());
        }
        if self.nvenc_sessions > 64 {
            return Err("video.nvenc_sessions must be 0 to 64".into());
        }
        self.playout_feed.validate()
    }
}

/// The cart wall's players (ADR-0117): where the cart files are, and how far ahead of the
/// play head each sounding cart is decoded.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Carts {
    /// The cart files the media worker wrote (`carts.media_dir` in the control plane's
    /// settings); the desk only reads them, and only by a name the control plane gives it
    /// (a UUID and `.flac`).
    pub dir: PathBuf,
    /// How much decoded audio waits ahead of each sounding cart; a bed loops inside it.
    pub ahead_ms: u32,
    /// How long a fire waits for a cart's first audio before answering.
    pub start_wait_ms: u32,
}

impl Default for Carts {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("/var/lib/weavesignal/carts"),
            ahead_ms: 1_000,
            start_wait_ms: 500,
        }
    }
}

/// How the desk follows who is in the room.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RoomWatch {
    /// LiveKit's server API, on the server: `http://127.0.0.1:<ports.livekit_http>`. Left
    /// empty, it is the signalling URL with `ws` turned into `http`.
    pub api_url: String,
    /// How often the desk lists the room's participants.
    pub poll_ms: u64,
    /// How long a person who left keeps their mix-minus, so a dropped link that rejoins
    /// finds it again.
    pub leave_grace_ms: u64,
    /// A subscription that has routed no audio after this long is made again (spike 01: one
    /// desk start in three linked nobody, cause unknown).
    pub link_timeout_ms: u64,
}

impl Default for RoomWatch {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            poll_ms: 500,
            leave_grace_ms: 30_000,
            link_timeout_ms: 8_000,
        }
    }
}

/// The desk's control socket, for the control plane: on air, talkback, status.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Control {
    /// A Unix socket path; none turns the socket off.
    pub socket: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LiveKit {
    /// LiveKit's signalling URL, on the server: `ws://127.0.0.1:<ports.livekit_http>`.
    pub url: String,
    pub api_key: String,
    /// Set from [`SECRET_ENV`]; never from the file.
    #[serde(skip)]
    pub api_secret: String,
    pub room: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct DeskSettings {
    /// The prefix of the desk's own room identities: `<prefix>-<participant>` subscribes to a
    /// participant, `<prefix>-mm-<participant>` publishes their mix-minus.
    pub identity: String,
    /// The receive jitter buffer, set inside LiveKit's bins. LiveKit's own default of 200 ms
    /// doubled a guest's delay in spike 01 (503 ms against 165 ms at 50 ms).
    pub jitter_buffer_ms: u32,
    /// The desk's tick: one mixing pass computes every bus for a block this long, and each
    /// block is one Opus frame (10 or 20 ms).
    pub mix_buffer_ms: u32,
    /// How long the mixing pass waits for a voice that is late for its block (by its
    /// timestamps) before mixing without it; what comes later still is dropped, as a live
    /// mixer drops it. Voices that are on time are mixed as soon as the block's time has
    /// passed (the M1–M2 mixer waited at most the jitter buffer's 50 ms the same way).
    pub input_prebuffer_ms: u32,
    /// The threads that encode the returns; 0 picks one per core, at most four.
    pub workers: usize,
    /// Ask for real-time scheduling for the mixing thread and the encoders (granted where the
    /// host allows it: `CAP_SYS_NICE`, an `RLIMIT_RTPRIO`, or a container given the right).
    pub realtime: bool,
    /// Each return's Opus bitrate (opusenc's own default).
    pub return_opus_bitrate: u32,
}

impl Default for DeskSettings {
    fn default() -> Self {
        Self {
            identity: "desk".to_owned(),
            jitter_buffer_ms: 50,
            mix_buffer_ms: 20,
            input_prebuffer_ms: 50,
            workers: 0,
            realtime: true,
            return_opus_bitrate: 128_000,
        }
    }
}

impl DeskSettings {
    /// The encoding threads to start.
    pub fn worker_count(&self) -> usize {
        if self.workers > 0 {
            return self.workers;
        }
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(1, 4)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Programme {
    /// The address the desk's SRT listener binds; Liquidsoap connects to it as the caller.
    /// Loopback only: the desk runs beside Liquidsoap (ADR-0049). The desk listens rather
    /// than calls because `srtsink` sends Matroska's header to every caller that connects,
    /// while as a caller it reconnected without the header, and sometimes not at all
    /// (2026-10-25).
    pub srt_bind: IpAddr,
    /// `ports.srt_ingest` in the control plane's settings (ADR-0075).
    pub srt_port: u16,
    pub srt_latency_ms: u32,
    pub opus_bitrate: u32,
    /// The leaky queues before the encoder and before `srtsink`: with no playout connected,
    /// `srtsink` can block, and without them the whole desk stalls (spike 01).
    pub queue_ms: u32,
    /// The limiter on the programme and on every mix-minus. The desk mixes in 32-bit float,
    /// so a sum over full scale is not clipped before the limiter brings it under the ceiling.
    pub limiter: LimiterSettings,
    /// The profanity delay between the programme and playout, with its dump.
    pub delay: DelaySettings,
}

impl Default for Programme {
    fn default() -> Self {
        Self {
            srt_bind: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            srt_port: 7313,
            srt_latency_ms: 20,
            opus_bitrate: 96_000,
            queue_ms: 500,
            limiter: LimiterSettings::default(),
            delay: DelaySettings::default(),
        }
    }
}

impl Programme {
    pub fn srt_uri(&self) -> String {
        let host = match self.srt_bind {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        format!("srt://{host}:{}?mode=listener", self.srt_port)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read the settings file {0}: {1}")]
    Read(String, std::io::Error),
    #[error("the settings file {0} is not valid: {1}")]
    Parse(String, Box<toml::de::Error>),
    #[error("the settings are not valid: {0}")]
    Invalid(String),
}

impl Config {
    pub fn from_toml(text: &str, path: &str) -> Result<Self, ConfigError> {
        let mut c: Config =
            toml::from_str(text).map_err(|e| ConfigError::Parse(path.to_owned(), Box::new(e)))?;
        for role in [Role::Presenter, Role::Dj] {
            c.inputs
                .entry(role)
                .or_insert_with(ChainSettings::presenter);
        }
        // Callers sound like a phone line while we run diagnostics (operator, 2026-11-01); a
        // station turns it off with an `[inputs.caller]` of its own.
        c.inputs
            .entry(Role::Caller)
            .or_insert_with(ChainSettings::caller);
        c.validate()?;
        Ok(c)
    }

    /// A role's input chain (empty when it has none).
    pub fn chain(&self, role: Role) -> ChainSettings {
        self.inputs.get(&role).copied().unwrap_or_default()
    }

    /// LiveKit's keys.
    pub fn keys(&self) -> Keys {
        Keys {
            api_key: self.livekit.api_key.clone(),
            api_secret: self.livekit.api_secret.clone(),
        }
    }

    /// LiveKit's server API.
    pub fn api_url(&self) -> String {
        if !self.room.api_url.is_empty() {
            return self.room.api_url.trim_end_matches('/').to_owned();
        }
        let u = &self.livekit.url;
        let u = u
            .strip_prefix("wss://")
            .map(|r| format!("https://{r}"))
            .or_else(|| u.strip_prefix("ws://").map(|r| format!("http://{r}")))
            .unwrap_or_else(|| u.clone());
        u.trim_end_matches('/').to_owned()
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let shown = path.display().to_string();
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Read(shown.clone(), e))?;
        let mut c = Self::from_toml(&text, &shown)?;
        c.livekit.api_secret = std::env::var(SECRET_ENV).unwrap_or_default();
        if c.livekit.api_secret.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "LiveKit's API secret is not set; put it in {SECRET_ENV}"
            )));
        }
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |m: String| Err(ConfigError::Invalid(m));
        if !self.programme.srt_bind.is_loopback() {
            return invalid(format!(
                "programme.srt_bind is {}; the desk serves SRT on loopback only, and a remote desk connects through a tunnel (ADR-0049)",
                self.programme.srt_bind
            ));
        }
        if self.programme.srt_port == 0 {
            return invalid("programme.srt_port is 0".into());
        }
        if !(self.livekit.url.starts_with("ws://") || self.livekit.url.starts_with("wss://")) {
            return invalid(format!(
                "livekit.url `{}` is not a ws:// or wss:// URL",
                self.livekit.url
            ));
        }
        if self.desk.jitter_buffer_ms > 1000 {
            return invalid(format!(
                "desk.jitter_buffer_ms is {}; above 1000 ms the room is unusable",
                self.desk.jitter_buffer_ms
            ));
        }
        if ![10, 20].contains(&self.desk.mix_buffer_ms) {
            return invalid("desk.mix_buffer_ms must be 10 or 20 (an Opus frame)".into());
        }
        if !(20..=500).contains(&self.desk.input_prebuffer_ms) {
            return invalid("desk.input_prebuffer_ms must be 20 to 500".into());
        }
        if self.desk.workers > 64 {
            return invalid("desk.workers must be 0 (automatic) to 64".into());
        }
        if !(6_000..=510_000).contains(&self.desk.return_opus_bitrate) {
            return invalid("desk.return_opus_bitrate must be 6000 to 510000".into());
        }
        self.programme
            .delay
            .validate()
            .map_err(|e| ConfigError::Invalid(format!("programme.delay: {e}")))?;
        if !(6_000..=510_000).contains(&self.programme.opus_bitrate) {
            return invalid("programme.opus_bitrate must be 6000 to 510000".into());
        }
        let prefix_ok = !self.desk.identity.is_empty()
            && self
                .desk
                .identity
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !prefix_ok {
            return invalid(format!(
                "desk.identity `{}` is not a safe identity",
                self.desk.identity
            ));
        }
        for (role, chain) in &self.inputs {
            chain
                .validate()
                .map_err(|e| ConfigError::Invalid(format!("inputs.{role}: {e}")))?;
        }
        self.programme
            .limiter
            .validate()
            .map_err(|e| ConfigError::Invalid(format!("programme.limiter: {e}")))?;
        if !(100..=10_000).contains(&self.room.poll_ms) {
            return invalid("room.poll_ms must be 100 to 10000".into());
        }
        if !(200..=10_000).contains(&self.carts.ahead_ms) {
            return invalid("carts.ahead_ms must be 200 to 10000".into());
        }
        if !(50..=5_000).contains(&self.carts.start_wait_ms) {
            return invalid("carts.start_wait_ms must be 50 to 5000".into());
        }
        self.video.validate().map_err(ConfigError::Invalid)?;
        if self.room.link_timeout_ms < 1_000 {
            return invalid("room.link_timeout_ms must be at least 1000".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[livekit]
url = "ws://127.0.0.1:7880"
api_key = "devkey"
room = "studio"
"#;

    #[test]
    fn defaults_are_the_measured_ones() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.desk.jitter_buffer_ms, 50);
        assert_eq!(c.desk.mix_buffer_ms, 20);
        assert_eq!(c.programme.delay.target_ms, 8_000);
        assert!((1..=4).contains(&c.desk.worker_count()));
        assert_eq!(c.programme.srt_latency_ms, 20);
        assert_eq!(c.programme.queue_ms, 500);
        assert_eq!(c.programme.srt_uri(), "srt://127.0.0.1:7313?mode=listener");
        assert_eq!(c.programme.limiter.ceiling_db, -1.0);
        assert_eq!(c.room.poll_ms, 500);
        assert_eq!(c.api_url(), "http://127.0.0.1:7880");
        assert_eq!(c.chain(Role::Presenter), ChainSettings::presenter());
        assert_eq!(c.chain(Role::Dj), ChainSettings::presenter());
        assert!(c.chain(Role::Guest).is_empty());
        // Callers sound like a phone line while we run diagnostics (operator, 2026-11-01).
        assert_eq!(c.chain(Role::Caller), ChainSettings::caller());
        assert!(c.chain(Role::Caller).telephone.is_some());
    }

    #[test]
    fn a_role_left_out_keeps_its_default_and_an_empty_table_turns_it_off() {
        let text = format!(
            "{MINIMAL}\n[inputs.presenter]\n\n[inputs.caller]\n\n[inputs.guest]\nmakeup_db = 3.0\n"
        );
        let c = Config::from_toml(&text, "t").unwrap();
        assert!(c.chain(Role::Presenter).is_empty());
        assert!(
            c.chain(Role::Caller).is_empty(),
            "the phone sound turned off"
        );
        assert_eq!(c.chain(Role::Dj), ChainSettings::presenter());
        assert_eq!(c.chain(Role::Guest).makeup_db, 3.0);
    }

    #[test]
    fn a_bad_chain_is_refused_by_role() {
        let text = format!("{MINIMAL}\n[inputs.caller.limiter]\nceiling_db = 6.0\n");
        let err = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(err.contains("inputs.caller"), "{err}");
    }

    #[test]
    fn srt_off_loopback_is_refused() {
        let text = format!("{MINIMAL}\n[programme]\nsrt_bind = \"192.0.2.10\"\n");
        let err = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(err.contains("ADR-0049"), "{err}");
    }

    #[test]
    fn ipv6_loopback_is_bracketed() {
        let text = format!("{MINIMAL}\n[programme]\nsrt_bind = \"::1\"\nsrt_port = 19000\n");
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.programme.srt_uri(), "srt://[::1]:19000?mode=listener");
    }

    #[test]
    fn an_absurd_jitter_buffer_is_refused() {
        let text = format!("{MINIMAL}\n[desk]\njitter_buffer_ms = 5000\n");
        assert!(Config::from_toml(&text, "t").is_err());
    }

    #[test]
    fn the_example_parses_and_documents_the_defaults() {
        let text = include_str!("../desk.example.toml");
        let c = Config::from_toml(text, "desk.example.toml").unwrap();
        let d = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.chain(Role::Presenter), d.chain(Role::Presenter));
        assert_eq!(c.programme.limiter, d.programme.limiter);
        assert_eq!(c.programme.delay, d.programme.delay);
        assert_eq!(c.desk, d.desk);
        assert_eq!(c.room, d.room);
        assert_eq!(c.carts, d.carts);
        assert_eq!(c.video, d.video);
    }

    #[test]
    fn video_is_off_by_default_at_480p20() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert!(!c.video.enabled);
        assert_eq!(c.video.canvas().name, "480p20");
        assert_eq!((c.video.canvas().width, c.video.canvas().kbps), (854, 600));
        assert_eq!(c.video.hls.segment_ms, 2_000);
        // Playout's output comes back on loopback once video is on (ADR-0172).
        let f = &c.video.playout_feed;
        assert!(f.enabled && f.bind.is_loopback());
        assert_eq!(
            (f.port, f.crossfade_ms, f.off_air_hold_ms),
            (7317, 300, 3_000)
        );
    }

    #[test]
    fn the_feed_from_playout_stays_on_loopback() {
        let text = format!("{MINIMAL}\n[video.playout_feed]\nbind = \"0.0.0.0\"\n");
        let err = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(err.contains("loopback"), "{err}");
        let text = format!("{MINIMAL}\n[video.playout_feed]\nenabled = false\nport = 0\n");
        assert!(Config::from_toml(&text, "t").is_ok());
    }

    #[test]
    fn bad_video_settings_are_refused() {
        for bad in [
            "rungs = [\"1080p60\"]",
            "rungs = [\"240p15\", \"480p20\"]",
            "camera_codecs = [\"H264\"]",
            "rungs = []",
        ] {
            let text = format!("{MINIMAL}\n[video]\n{bad}\n");
            assert!(Config::from_toml(&text, "t").is_err(), "{bad}");
        }
        let text =
            format!("{MINIMAL}\n[video]\nenabled = true\nrungs = [\"480p20\", \"240p15\"]\n");
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.video.rung_list().len(), 2);
    }
}
