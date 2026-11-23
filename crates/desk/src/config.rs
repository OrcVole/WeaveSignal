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
    /// The phone line (17f): where its words are.
    #[serde(default)]
    pub phone: PhoneIn,
    #[serde(default)]
    pub video: Video,
    /// Hosts in their own studios (ADR-0280): what the ingest hands the desk.
    #[serde(default)]
    pub sources: SourcesIn,
}

/// Where the ingest hands the desk the sound of a host's own studio (ADR-0280): PCM in WAV over
/// loopback, one connection for each source standing by, as playout's feed comes (ADR-0172).
/// The desk listens; the ingest connects with the password in [`SOURCES_PASSWORD_ENV`], and
/// without one the desk takes no sources at all.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct SourcesIn {
    pub enabled: bool,
    /// Loopback only, like every link between the station's own processes.
    pub bind: IpAddr,
    /// `ports.desk_sources` in the control plane's settings (ADR-0075).
    pub port: u16,
    /// How far ahead of the mixing pass a source's sound is placed as it arrives: it comes in
    /// codec frames, after a network, so a little more than playout's feed needs.
    pub cushion_ms: u32,
    /// Follow each source's clock (Phase 17g's one clock): resample it once, at a ratio steered
    /// to hold it the cushion ahead, instead of placing it afresh each time it has drifted by
    /// the cushion. On by default.
    pub follow: bool,
    /// The slow gain that brings a source to the desk's level (no gate, no phone band).
    pub leveller: weavesignal_mixer::SourceLevellerSettings,
}

/// The environment variable that holds the password the ingest sends with each source. Unset,
/// the desk does not listen for sources.
pub const SOURCES_PASSWORD_ENV: &str = "WEAVESIGNAL_DESK_SOURCES_PASSWORD";

impl Default for SourcesIn {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port: 7318,
            cushion_ms: 300,
            follow: true,
            leveller: weavesignal_mixer::SourceLevellerSettings::default(),
        }
    }
}

impl SourcesIn {
    fn validate(&self) -> Result<(), String> {
        if !self.bind.is_loopback() {
            return Err(format!(
                "sources.bind is {}; the ingest hands sources over on loopback only",
                self.bind
            ));
        }
        if self.enabled && self.port == 0 {
            return Err("sources.port is 0".into());
        }
        if !(50..=2_000).contains(&self.cushion_ms) {
            return Err("sources.cushion_ms must be 50 to 2000".into());
        }
        self.leveller
            .validate()
            .map_err(|e| format!("sources.leveller: {e}"))
    }
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
    // --- rtmp-out (ADR-0229, ADR-0230) -------------------------------------------------
    /// The picture the vision mixer composes, from [`Rung::PRESETS`] or [`Rung::CANVASES`];
    /// empty, the first HLS rung's (as before). A canvas larger than the first rung is
    /// scaled down for HLS, so the HLS rungs do not change; it is what RTMP out can send at
    /// most, and every camera, slate and frame is made at its size.
    pub canvas: String,
    /// RTMP out: the programme's picture and sound, behind the delay, to video platforms.
    pub rtmp: RtmpOut,
}

/// RTMP out (ADR-0229, ADR-0230; `rtmp-out`): the delayed programme picture and the video's
/// sound (ADR-0172) to each destination the control plane sends. The destinations are not
/// here: they and their stream keys live in the control plane's database and reach the desk
/// over its socket. Nothing is encoded while no destination is on.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RtmpOut {
    /// The picture sent, no larger and no faster than the canvas; empty, the canvas.
    pub rung: String,
    /// The `compatible` profile: H.264 and AAC, classic RTMP (Rumble, Odysee, PeerTube).
    pub compatible: RtmpCompatible,
    /// The `open` profile: AV1 and Opus over Enhanced RTMP.
    pub open: RtmpOpen,
    /// A picture keyframe this often (Rumble asks for 2 s).
    pub keyframe_s: u32,
    /// A destination that fails is tried again after this, doubling to `retry_max_s`.
    pub retry_min_s: u32,
    pub retry_max_s: u32,
    /// A destination that has not taken this much (milliseconds of the stream) is too slow:
    /// it is dropped and connects again.
    pub backlog_ms: u32,
}

impl Default for RtmpOut {
    fn default() -> Self {
        Self {
            rung: String::new(),
            compatible: RtmpCompatible::default(),
            open: RtmpOpen::default(),
            keyframe_s: 2,
            retry_min_s: 2,
            retry_max_s: 60,
            backlog_ms: 4_000,
        }
    }
}

/// How the desk's WebRTC senders (each return, the programme picture) control congestion:
/// `webrtcsink`'s own `congestion-control` option. `Gcc` is GStreamer's `rtpgccbwe`, a free
/// implementation (MPL-2.0) of Google's published algorithm, and what the desk has used since
/// 1.0.1; `Homegrown` is webrtcsink's own simple sender-side heuristic; `Disabled` applies none.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CongestionControl {
    #[default]
    Gcc,
    Homegrown,
    Disabled,
}

impl CongestionControl {
    /// The nick `webrtcsink`'s `GstWebRTCSinkCongestionControl` enum takes.
    pub fn nick(self) -> &'static str {
        match self {
            CongestionControl::Gcc => "gcc",
            CongestionControl::Homegrown => "homegrown",
            CongestionControl::Disabled => "disabled",
        }
    }
}

/// What encodes H.264 for `compatible`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum H264Choice {
    /// NVENC when this machine has a session free, else OpenH264.
    Auto,
    Openh264,
    Nvenc,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RtmpCompatible {
    pub h264: H264Choice,
    /// `high`, `main` or `baseline` (OpenH264 makes `high` without B-frames).
    pub h264_profile: String,
    /// The picture's bitrate at constant rate; 0, the rung's default ([`Rung::h264_kbps`]).
    pub video_kbps: u32,
    /// AAC-LC, as Rumble asks: 128 kbit/s at 44.1 kHz, stereo.
    pub aac_kbps: u32,
    pub aac_rate: u32,
}

impl Default for RtmpCompatible {
    fn default() -> Self {
        Self {
            h264: H264Choice::Auto,
            h264_profile: "high".into(),
            video_kbps: 0,
            aac_kbps: 128,
            aac_rate: 44_100,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RtmpOpen {
    /// 0, the rung's default ([`Rung::av1_kbps`]).
    pub video_kbps: u32,
    pub opus_kbps: u32,
}

impl Default for RtmpOpen {
    fn default() -> Self {
        Self {
            video_kbps: 0,
            opus_kbps: 128,
        }
    }
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
            canvas: String::new(),
            rtmp: RtmpOut::default(),
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

    /// Larger pictures, for the canvas and RTMP out only (not HLS rungs; `rtmp-out`). Their
    /// `kbps` is the AV1 figure [`Rung::av1_kbps`] uses for them.
    pub const CANVASES: [Rung; 3] = [
        Rung {
            name: "720p30",
            width: 1280,
            height: 720,
            fps: 30,
            kbps: 1_500,
        },
        Rung {
            name: "720p25",
            width: 1280,
            height: 720,
            fps: 25,
            kbps: 1_300,
        },
        Rung {
            name: "1080p30",
            width: 1920,
            height: 1080,
            fps: 30,
            kbps: 2_700,
        },
    ];

    /// A rung or a canvas by name.
    pub fn any(name: &str) -> Option<Rung> {
        Self::named(name).or_else(|| Self::CANVASES.iter().copied().find(|r| r.name == name))
    }

    /// H.264's bitrate for RTMP out at this size and rate, about 0.09 bits a pixel (720p30:
    /// 2,500 kbit/s, in Rumble's 2,000–3,000), in hundreds.
    pub fn h264_kbps(&self) -> u32 {
        let px = u64::from(self.width) * u64::from(self.height) * u64::from(self.fps);
        (((px * 9 / 100_000 + 50) / 100).max(3) * 100) as u32
    }

    /// AV1's bitrate for RTMP out: the HLS figure for a preset, the canvas's own otherwise.
    pub fn av1_kbps(&self) -> u32 {
        self.kbps
    }

    /// Fits inside `outer`: no larger, no faster.
    pub fn fits(&self, outer: &Rung) -> bool {
        self.width <= outer.width && self.height <= outer.height && self.fps <= outer.fps
    }
}

impl Video {
    /// The rungs, in the order given.
    pub fn rung_list(&self) -> Vec<Rung> {
        self.rungs.iter().filter_map(|n| Rung::named(n)).collect()
    }

    /// The picture the vision mixer composes: `canvas` if set, else the first rung's size
    /// and rate.
    pub fn canvas(&self) -> Rung {
        if let Some(c) = Rung::any(&self.canvas) {
            return c;
        }
        self.rung_list()
            .first()
            .copied()
            .unwrap_or(Rung::PRESETS[0])
    }

    /// RTMP out's picture: `rtmp.rung` if set, else the canvas.
    pub fn rtmp_rung(&self) -> Rung {
        Rung::any(&self.rtmp.rung).unwrap_or_else(|| self.canvas())
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
        self.validate_rtmp()?;
        self.playout_feed.validate()
    }

    /// The canvas and RTMP out (`rtmp-out`): nothing is ever scaled up.
    fn validate_rtmp(&self) -> Result<(), String> {
        let all: Vec<&str> = Rung::PRESETS
            .iter()
            .chain(&Rung::CANVASES)
            .map(|r| r.name)
            .collect();
        if !self.canvas.is_empty() && Rung::any(&self.canvas).is_none() {
            return Err(format!(
                "video.canvas: `{}` is not one of {all:?}",
                self.canvas
            ));
        }
        let canvas = self.canvas();
        if let Some(first) = self.rung_list().first()
            && !first.fits(&canvas)
        {
            return Err(format!(
                "video.canvas {} is smaller or slower than the first HLS rung {}: pictures are never scaled up",
                canvas.name, first.name
            ));
        }
        let r = &self.rtmp;
        if !r.rung.is_empty() {
            let Some(rung) = Rung::any(&r.rung) else {
                return Err(format!(
                    "video.rtmp.rung: `{}` is not one of {all:?}",
                    r.rung
                ));
            };
            if !rung.fits(&canvas) {
                return Err(format!(
                    "video.rtmp.rung {} is larger or faster than the canvas {}: set video.canvas to it (pictures are never scaled up)",
                    rung.name, canvas.name
                ));
            }
        }
        if !["high", "main", "baseline"].contains(&r.compatible.h264_profile.as_str()) {
            return Err("video.rtmp.compatible.h264_profile is high, main or baseline".into());
        }
        if r.compatible.video_kbps != 0 && !(300..=8_000).contains(&r.compatible.video_kbps) {
            return Err("video.rtmp.compatible.video_kbps must be 0 (the rung's) or 300 to 8000 (Rumble's ceiling)".into());
        }
        if r.open.video_kbps != 0 && !(200..=8_000).contains(&r.open.video_kbps) {
            return Err("video.rtmp.open.video_kbps must be 0 (the rung's) or 200 to 8000".into());
        }
        if !(64..=320).contains(&r.compatible.aac_kbps) {
            return Err("video.rtmp.compatible.aac_kbps must be 64 to 320".into());
        }
        if ![44_100, 48_000].contains(&r.compatible.aac_rate) {
            return Err("video.rtmp.compatible.aac_rate is 44100 or 48000".into());
        }
        if !(32..=510).contains(&r.open.opus_kbps) {
            return Err("video.rtmp.open.opus_kbps must be 32 to 510".into());
        }
        if !(1..=10).contains(&r.keyframe_s) {
            return Err("video.rtmp.keyframe_s must be 1 to 10".into());
        }
        if r.retry_min_s == 0 || r.retry_max_s < r.retry_min_s || r.retry_max_s > 3_600 {
            return Err("video.rtmp: retry_min_s at least 1, retry_max_s from it to 3600".into());
        }
        if !(1_000..=30_000).contains(&r.backlog_ms) {
            return Err("video.rtmp.backlog_ms must be 1000 to 30000".into());
        }
        Ok(())
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

/// The phone line's side at the desk (17f). The desk finds phone booths by their names
/// whatever this says: a station without the line has none. It plays the phone words from
/// `words_dir`, only by a name the control plane gives it (`<uuid>.flac`, or the shipped
/// `phone-words-default.flac`), never a path.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct PhoneIn {
    /// `phone.words_dir` in the control plane's settings; the desk only reads it.
    pub words_dir: PathBuf,
}

impl Default for PhoneIn {
    fn default() -> Self {
        Self {
            words_dir: PathBuf::from("/var/lib/weavesignal/phone-words"),
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
    /// STUN servers the desk's WebRTC connections may ask for their addresses (ADR-0290): a
    /// comma-separated list of `host:port`, the station's `STUN_SERVERS` setting. Empty, the
    /// default, the desk contacts none: GStreamer's `webrtcsink` and `webrtcsrc` would
    /// otherwise ask Google's. `webrtcbin` takes one server, so the first is used.
    #[serde(default)]
    pub stun_servers: String,
    /// No ICE over TCP for the desk's WebRTC connections: set for a rehearsal desk (1.3), never
    /// from the file. libnice otherwise listens on a passive TCP port for each connection, and a
    /// rehearsal desk opens no listener at all (section 1.9); it reaches LiveKit over UDP.
    #[serde(skip)]
    pub no_ice_tcp: bool,
}

impl LiveKit {
    /// The URL for the `stun-server` property of every WebRTC element the desk makes; `None`,
    /// the default, clears the element's own default. An unreadable list also gives `None`
    /// (`Config::validate` refuses it first).
    pub fn stun_server_url(&self) -> Option<String> {
        weavesignal_proto::parse_stun_servers(&self.stun_servers)
            .ok()
            .and_then(|s| weavesignal_proto::webrtcbin_url(&s))
    }
}

/// Which studio a desk mixes (1.3, the rehearsal studio; design `rehearsal-mode-build-plan.md`
/// section 1.7).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeskMode {
    /// The live studio: the programme to playout, and everything else a station sends.
    #[default]
    Live,
    /// The rehearsal studio: the returns (mix-minus) and carts only. Built with no way to the
    /// air: no programme output, no video, HLS or RTMP, no hosts' sources, no VBAN out, no phone,
    /// no monitor links, and its control socket refuses every request but the desk's keys.
    Rehearsal,
}

impl DeskMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DeskMode::Live => "live",
            DeskMode::Rehearsal => "rehearsal",
        }
    }

    /// What a desk in this mode sends out.
    pub fn outputs(self) -> crate::pipeline::Outputs {
        match self {
            DeskMode::Live => crate::pipeline::Outputs::ALL,
            DeskMode::Rehearsal => crate::pipeline::Outputs::REHEARSAL,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct DeskSettings {
    /// `live` (the default) or `rehearsal` (1.3): a rehearsal desk mixes `<room>-rehearsal` and
    /// has no route to the air (section 1.7 of the rehearsal build plan).
    pub mode: DeskMode,
    /// The prefix of the desk's own room identities: `<prefix>-<participant>` subscribes to a
    /// participant, `<prefix>-mm-<participant>` publishes their mix-minus.
    pub identity: String,
    /// The receive jitter buffer, set inside LiveKit's bins. LiveKit's own default of 200 ms
    /// doubled a guest's delay in spike 01 (503 ms against 165 ms at 50 ms).
    pub jitter_buffer_ms: u32,
    /// The receive jitter buffer for cameras, set on each video stream once its caps say
    /// video. A camera's frame spans many packets and one missing packet loses the frame (and,
    /// for AV1, every frame after it until the next keyframe): at the voices' 50 ms a
    /// browser's camera on the talk station lost 2.7 % of its packets as late and flickered to
    /// the slate every few seconds (2026-11-12), while its retransmissions all came too late.
    /// The picture is not conversational: it may wait for them.
    pub video_jitter_buffer_ms: u32,
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
    /// The senders' congestion control: `gcc` (the default, Google's published algorithm as
    /// GStreamer's `rtpgccbwe`), `homegrown` (webrtcsink's own heuristic) or `disabled`. It
    /// looks only at the desk-to-room hop; `docs/.../congestion-control` says what it can and
    /// cannot do.
    pub congestion_control: CongestionControl,
}

impl Default for DeskSettings {
    fn default() -> Self {
        Self {
            mode: DeskMode::Live,
            identity: "desk".to_owned(),
            jitter_buffer_ms: 50,
            video_jitter_buffer_ms: 300,
            mix_buffer_ms: 20,
            input_prebuffer_ms: 50,
            workers: 0,
            realtime: true,
            return_opus_bitrate: 128_000,
            congestion_control: CongestionControl::Gcc,
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
        if c.desk.mode == DeskMode::Rehearsal {
            // A rehearsal desk takes no host's own studio. `sources.enabled` defaults to true, so
            // only a file that turns it on by name is refused; left out, it is off here.
            let raw: toml::Table = toml::from_str(text)
                .map_err(|e| ConfigError::Parse(path.to_owned(), Box::new(e)))?;
            let asked = raw
                .get("sources")
                .and_then(|s| s.get("enabled"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            if asked {
                return Err(ConfigError::Invalid(
                    "sources.enabled is true on a rehearsal desk; a host's own studio stands by at the live desk only".into(),
                ));
            }
            c.sources.enabled = false;
            c.livekit.no_ice_tcp = true;
        }
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
        self.sources.validate().map_err(ConfigError::Invalid)?;
        if !(self.livekit.url.starts_with("ws://") || self.livekit.url.starts_with("wss://")) {
            return invalid(format!(
                "livekit.url `{}` is not a ws:// or wss:// URL",
                self.livekit.url
            ));
        }
        if let Err(e) = weavesignal_proto::parse_stun_servers(&self.livekit.stun_servers) {
            return invalid(format!("livekit.stun_servers: {e}"));
        }
        if self.desk.jitter_buffer_ms > 1000 {
            return invalid(format!(
                "desk.jitter_buffer_ms is {}; above 1000 ms the room is unusable",
                self.desk.jitter_buffer_ms
            ));
        }
        if self.desk.video_jitter_buffer_ms > 2000 {
            return invalid(format!(
                "desk.video_jitter_buffer_ms is {}; above 2000 ms the picture is too far behind",
                self.desk.video_jitter_buffer_ms
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
        self.validate_mode()?;
        if self.room.link_timeout_ms < 1_000 {
            return invalid("room.link_timeout_ms must be at least 1000".into());
        }
        Ok(())
    }
}

impl Config {
    /// The walls between a rehearsal desk and the air (section 1.7): what it may not be given.
    /// A live desk may not mix a room shaped like a rehearsal room, which would air it.
    fn validate_mode(&self) -> Result<(), ConfigError> {
        let invalid = |m: String| Err(ConfigError::Invalid(m));
        let rehearsal_room = weavesignal_proto::looks_like_rehearsal_room(&self.livekit.room);
        match self.desk.mode {
            DeskMode::Live if rehearsal_room => invalid(format!(
                "livekit.room `{}` is a rehearsal room; a live desk would put it on air. Set desk.mode = \"rehearsal\" or mix the studio room",
                self.livekit.room
            )),
            DeskMode::Live => Ok(()),
            DeskMode::Rehearsal => {
                if !rehearsal_room {
                    return invalid(format!(
                        "livekit.room `{}` is not a rehearsal room (`<room>{}`); a rehearsal desk mixes only that",
                        self.livekit.room,
                        weavesignal_proto::REHEARSAL_SUFFIX
                    ));
                }
                if self.video.enabled {
                    return invalid(
                        "video.enabled is true on a rehearsal desk; it sends no picture, HLS, RTMP or playout feed".into(),
                    );
                }
                if self.sources.enabled {
                    return invalid(
                        "sources.enabled is true on a rehearsal desk; a host's own studio stands by at the live desk only".into(),
                    );
                }
                if self.desk.identity == DeskSettings::default().identity {
                    return invalid(format!(
                        "desk.identity is `{}`, the live desk's; a rehearsal desk is `{}`",
                        self.desk.identity,
                        weavesignal_proto::REHEARSAL_DESK_IDENTITY
                    ));
                }
                Ok(())
            }
        }
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
    fn the_desk_contacts_no_stun_server_unless_one_is_named() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.livekit.stun_servers, "");
        assert_eq!(
            c.livekit.stun_server_url(),
            None,
            "no default: not Google's"
        );
        let named = MINIMAL.replace(
            "[livekit]",
            "[livekit]\nstun_servers = \"stun.example.org:3478, other.example.net:3478\"",
        );
        let c = Config::from_toml(&named, "t").unwrap();
        assert_eq!(
            c.livekit.stun_server_url().as_deref(),
            Some("stun://stun.example.org:3478"),
            "webrtcbin takes one server: the first"
        );
        let bad = MINIMAL.replace(
            "[livekit]",
            "[livekit]\nstun_servers = \"stun.example.org\"",
        );
        let err = Config::from_toml(&bad, "t").unwrap_err().to_string();
        assert!(err.contains("livekit.stun_servers"), "{err}");
    }

    #[test]
    fn the_senders_congestion_control_is_gcc_unless_chosen() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.desk.congestion_control, CongestionControl::Gcc);
        for (word, want) in [
            ("gcc", CongestionControl::Gcc),
            ("homegrown", CongestionControl::Homegrown),
            ("disabled", CongestionControl::Disabled),
        ] {
            let text = format!("{MINIMAL}\n[desk]\ncongestion_control = \"{word}\"\n");
            let c = Config::from_toml(&text, "t").unwrap();
            assert_eq!(c.desk.congestion_control, want);
            assert_eq!(c.desk.congestion_control.nick(), word);
        }
        let bad = format!("{MINIMAL}\n[desk]\ncongestion_control = \"google\"\n");
        let err = Config::from_toml(&bad, "t").unwrap_err().to_string();
        assert!(
            err.contains("congestion_control") || err.contains("google"),
            "{err}"
        );
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

    // rtmp-out
    #[test]
    fn rtmp_out_sends_the_canvas_by_default_and_never_scales_up() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.video.rtmp_rung().name, "480p20");
        assert_eq!(c.video.rtmp.keyframe_s, 2);
        assert_eq!(
            (
                c.video.rtmp.compatible.aac_kbps,
                c.video.rtmp.compatible.aac_rate
            ),
            (128, 44_100)
        );
        // 720p30 for RTMP out: a larger canvas; HLS keeps its 480p20.
        let text = format!(
            "{MINIMAL}\n[video]\nenabled = true\ncanvas = \"720p30\"\n[video.rtmp]\nrung = \"720p30\"\n"
        );
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.video.canvas().name, "720p30");
        assert_eq!(c.video.rung_list()[0].name, "480p20");
        assert_eq!(c.video.rtmp_rung().h264_kbps(), 2_500);
        assert_eq!(Rung::named("480p20").unwrap().h264_kbps(), 700);
        for bad in [
            "[video]\n[video.rtmp]\nrung = \"720p30\"",
            "[video]\ncanvas = \"240p15\"",
            "[video]\ncanvas = \"4k\"",
            "[video]\n[video.rtmp]\nkeyframe_s = 0",
            "[video]\n[video.rtmp.compatible]\nh264_profile = \"extended\"",
            "[video]\n[video.rtmp.compatible]\nvideo_kbps = 9000",
            "[video]\n[video.rtmp.compatible]\naac_rate = 32000",
            "[video]\n[video.rtmp]\nretry_min_s = 10\nretry_max_s = 5",
        ] {
            let text = format!("{MINIMAL}\n{bad}\n");
            assert!(Config::from_toml(&text, "t").is_err(), "{bad}");
        }
    }

    const REHEARSAL: &str = r#"
[livekit]
url = "ws://127.0.0.1:7880"
api_key = "devkey"
room = "studio-rehearsal"

[desk]
mode = "rehearsal"
identity = "desk-rehearsal"
"#;

    #[test]
    fn a_desk_is_live_unless_its_settings_say_rehearsal() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.desk.mode, DeskMode::Live);
        assert!(c.sources.enabled, "a live desk still takes hosts' sources");
        assert!(!c.livekit.no_ice_tcp);
        let c = Config::from_toml(REHEARSAL, "t").unwrap();
        assert_eq!(c.desk.mode, DeskMode::Rehearsal);
        assert!(!c.sources.enabled, "a rehearsal desk takes none, unasked");
        assert!(c.livekit.no_ice_tcp, "and listens on no ICE-TCP port");
        assert!(!c.video.enabled);
        assert!(!c.desk.mode.outputs().srt);
        assert!(c.desk.mode.outputs().mix_minus_to_room);
        assert!(DeskMode::Live.outputs().srt);
    }

    #[test]
    fn a_rehearsal_desk_refuses_video_sources_and_the_live_room() {
        for (extra, why) in [
            ("[video]\nenabled = true\n", "video"),
            ("[sources]\nenabled = true\n", "sources"),
            (
                "[sources]\nenabled = true\nport = 7318\n",
                "sources with a port",
            ),
        ] {
            let e = Config::from_toml(&format!("{REHEARSAL}\n{extra}"), "t").unwrap_err();
            assert!(e.to_string().contains("rehearsal desk"), "{why}: {e}");
        }
        // `sources.enabled = false` by name is fine, as is a sources table without it.
        assert!(
            Config::from_toml(&format!("{REHEARSAL}\n[sources]\nenabled = false\n"), "t").is_ok()
        );
        let c = Config::from_toml(&format!("{REHEARSAL}\n[sources]\nport = 7400\n"), "t").unwrap();
        assert!(!c.sources.enabled);
        // The studio room, or the live desk's identity: refused.
        let live_room = REHEARSAL.replace("studio-rehearsal", "studio");
        assert!(Config::from_toml(&live_room, "t").is_err());
        let live_identity = REHEARSAL.replace("identity = \"desk-rehearsal\"", "");
        assert!(Config::from_toml(&live_identity, "t").is_err());
        // A desk built by hand past `from_toml` is still checked.
        let mut c = Config::from_toml(REHEARSAL, "t").unwrap();
        c.sources.enabled = true;
        assert!(c.validate().is_err());
        let mut c = Config::from_toml(REHEARSAL, "t").unwrap();
        c.video.enabled = true;
        assert!(c.validate().is_err());
    }

    #[test]
    fn a_live_desk_refuses_a_rehearsal_room() {
        let text = MINIMAL.replace("room = \"studio\"", "room = \"studio-rehearsal\"");
        let e = Config::from_toml(&text, "t").unwrap_err();
        assert!(e.to_string().contains("rehearsal room"), "{e}");
    }

    /// The settings an install route writes for its rehearsal desk, with each `${NAME:-default}`
    /// given its default (and `${NAME}` a stand-in), as the route would with nothing set.
    fn rendered(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(at) = rest.find("${") {
            out.push_str(&rest[..at]);
            let end = rest[at..].find('}').expect("a closing brace") + at;
            let inner = &rest[at + 2..end];
            out.push_str(match inner.split_once(":-") {
                Some((_, default)) => default,
                None => "stand-in",
            });
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn the_compose_rehearsal_desks_settings_load_as_a_rehearsal_desk() {
        let yaml = include_str!("../../../deploy/compose/rehearsal.yaml");
        let block: String = yaml
            .split_once("    content: |\n")
            .expect("the desk-rehearsal settings")
            .1
            .lines()
            .take_while(|l| l.is_empty() || l.starts_with("      "))
            .map(|l| format!("{}\n", l.strip_prefix("      ").unwrap_or(l)))
            .collect();
        let c = Config::from_toml(&rendered(&block), "rehearsal.yaml").unwrap();
        assert_eq!(c.desk.mode, DeskMode::Rehearsal);
        assert_eq!(c.livekit.room, "studio-rehearsal");
        assert_eq!(c.desk.identity, "desk-rehearsal");
        assert!(!c.sources.enabled && !c.video.enabled && !c.desk.realtime);
        assert!(!c.desk.mode.outputs().srt);
    }

    #[test]
    fn the_cloudron_rehearsal_desks_settings_load_as_a_rehearsal_desk() {
        let start = include_str!("../../../deploy/cloudron/start.sh");
        let block = start
            .split_once("cat > \"$run/desk-rehearsal.toml\" <<TOML\n")
            .expect("start.sh writes the rehearsal desk's settings")
            .1
            .split_once("\nTOML\n")
            .expect("the settings end")
            .0
            .replace("$LIVEKIT_HTTP_PORT", "7880")
            .replace("$LIVEKIT_API_KEY", "weavesignal")
            .replace("$ROOM_NAME", "studio")
            .replace("$stun_servers", "")
            .replace("$congestion_control", "gcc")
            .replace("$run", "/run/weavesignal")
            .replace("$data", "/app/data");
        assert!(!block.contains('$'), "a variable left in: {block}");
        let c = Config::from_toml(&block, "start.sh").unwrap();
        assert_eq!(c.desk.mode, DeskMode::Rehearsal);
        assert_eq!(c.livekit.room, "studio-rehearsal");
        assert!(!c.sources.enabled && !c.video.enabled);
    }

    #[test]
    fn an_unknown_mode_is_refused() {
        let text = REHEARSAL.replace("mode = \"rehearsal\"", "mode = \"backstage\"");
        assert!(Config::from_toml(&text, "t").is_err());
    }
}
