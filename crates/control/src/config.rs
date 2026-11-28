// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The control plane's settings: a TOML file, with secrets from the environment.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use weavesignal_model::{Station, StationId};

use crate::mailkey::MailKey;
use crate::ports::Ports;
use crate::studio::RundownItem;

/// The environment variable that holds the database URL; it carries a password, so it is not
/// read from the settings file.
pub const DATABASE_URL_ENV: &str = "WEAVESIGNAL_DATABASE_URL";
/// The environment variable that holds LiveKit's API secret.
pub const LIVEKIT_SECRET_ENV: &str = "WEAVESIGNAL_LIVEKIT_API_SECRET";
/// The environment variable that holds the bearer token for the control plane's admin routes,
/// until accounts arrive. Unset, those routes answer 503.
pub const ADMIN_TOKEN_ENV: &str = "WEAVESIGNAL_CONTROL_ADMIN_TOKEN";
/// The environment variable that holds the station's own consent words (`CONSENT_TEXT` on the
/// Compose bundle and the Quadlets): set and not blank, it takes the place of
/// `join.consent_text`. Compose cannot escape a quote into the settings file it writes, but an
/// environment variable takes any text whole. Unset, empty or blank: the settings file's words,
/// else the default.
pub const CONSENT_TEXT_ENV: &str = "WEAVESIGNAL_CONSENT_TEXT";
/// The environment variable that sets the cart wall's loudness target (`CARTS_LOUDNESS_LUFS` on
/// the Compose bundle, the Quadlets and the Cloudron package, whose settings files are written
/// for them): set, it takes the place of `carts.loudness_lufs`. Unset or empty: the settings
/// file's, else the station's default ([`CartsConfig::target`]).
pub const CARTS_LOUDNESS_ENV: &str = "WEAVESIGNAL_CARTS_LOUDNESS_LUFS";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub station: StationConfig,
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default)]
    pub ports: Ports,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub room: RoomConfig,
    #[serde(default)]
    pub desk: DeskConfig,
    #[serde(default)]
    pub studio: StudioConfig,
    #[serde(default)]
    pub playout: PlayoutConfig,
    #[serde(default)]
    pub carts: CartsConfig,
    #[serde(default)]
    pub join: JoinConfig,
    /// The video path's stills and thumbnails (M5, ADR-0010).
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub publishing: PublishingConfig,
    #[serde(default)]
    pub web: WebConfig,
    /// The station's setup page (ADR-0237).
    #[serde(default)]
    pub setup: SetupConfig,
    /// HLS content steering among the station's edges (M7, ADR-0051).
    #[serde(default)]
    pub steering: crate::steering::SteeringConfig,
    /// How staff sign-in codes reach staff by email (ADR-0127); off by default.
    #[serde(default)]
    pub mail: MailConfig,
    /// The public API: rate limits and the webhook guard (ADR-0195).
    #[serde(default)]
    pub api: crate::api::ApiConfig,
    /// The presenter kit's server half (Phase 17g).
    #[serde(default)]
    pub kit: KitConfig,
    /// The telephone line (Phase 17f); off by default, and off until a trunk is set.
    #[serde(default)]
    pub phone: crate::phone::config::PhoneConfig,
    /// The rehearsal studio (1.3.0): a second desk that never reaches the air. Off unless the
    /// installation turns it on (`REHEARSAL`).
    #[serde(default)]
    pub rehearsal: RehearsalConfig,
    /// Keyed shows (1.3.2): listener keys, their tokens and their limits.
    #[serde(default)]
    pub keys: KeysConfig,
    /// Set from [`DATABASE_URL_ENV`]; never from the file.
    #[serde(skip)]
    pub database_url: Option<String>,
    /// Set from [`LIVEKIT_SECRET_ENV`]; never from the file.
    #[serde(skip)]
    pub livekit_secret: Option<String>,
    /// Set from [`ADMIN_TOKEN_ENV`]; never from the file.
    #[serde(skip)]
    pub admin_token: Option<String>,
    /// Set from [`MailKey::ENV`]; never from the file. It keys the hash of staff email
    /// addresses and sign-in codes; with `mail.mode` on, the control plane will not start
    /// without it.
    #[serde(skip)]
    pub email_key: Option<MailKey>,
    /// From [`crate::apikey::ApiKey::ENV`], else derived from the station key; never from the
    /// file. It keys API tokens and webhook secrets (ADR-0195); with neither, they are off.
    #[serde(skip)]
    pub api_key: Option<crate::apikey::ApiKey>,
}

/// The `[kit]` settings: what the installation allows of the presenter kit.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct KitConfig {
    /// Whether VBAN (unauthenticated UDP for one local network) may be used at all: off by
    /// default; never on Cloudron, whatever this says.
    pub vban: bool,
}

/// How staff sign-in codes reach a staff member's inbox (ADR-0127). Off by default: a station
/// with no mail signs staff in with a password.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct MailConfig {
    /// `command`: pipe each email to a sendmail-compatible program (msmtp, sendmail -t);
    /// `outbox`: write each to a file in `outbox_dir` (development and tests);
    /// `off`: staff sign in with a password only.
    pub mode: MailMode,
    /// The sender address the station uses.
    pub from: String,
    pub command: Vec<String>,
    pub outbox_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MailMode {
    Command,
    Outbox,
    Off,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            mode: MailMode::Off,
            from: "weavesignal@station.example".into(),
            command: vec!["/usr/sbin/sendmail".into(), "-t".into(), "-i".into()],
            outbox_dir: PathBuf::from("/var/lib/weavesignal/staff-outbox"),
        }
    }
}

/// The public pages (`apps/web`'s build), served by the control plane ([`crate::web`]).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct WebConfig {
    /// The build's directory (`apps/web/dist`; `/opt/weavesignal/web` in the server image);
    /// unset, the control plane serves the API only.
    pub dir: Option<PathBuf>,
    /// Whether search engines may list the station's public pages, feeds and cards (ADR-0133).
    /// Off by default: every response says `noindex`, the pages carry a robots meta tag, and
    /// `/robots.txt` disallows everything. On, the public address may be listed; the API and
    /// the invite and echo-test pages never are.
    pub listed: bool,
}

/// The station's setup page (ADR-0237). What it stores is in the database; these two are for
/// the one answer that cannot be: the listener stream's password, which only a package whose
/// own proxy enforces it has (the Cloudron package sets both; the Compose bundle neither).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct SetupConfig {
    /// A file the package's start script reads at each start (`LISTENERS_AUTH` and
    /// `LISTENERS_PASSWORD`, each checked; it is never run). Set, the setup page offers the
    /// stream's password and writes its answer here, to apply at the next restart. An absolute
    /// path.
    pub overrides_file: Option<PathBuf>,
    /// Whether the stream asks for a password as this station was started, for the page to
    /// show; the start script writes it.
    pub stream_auth: Option<bool>,
    /// A graphics card for the desk's video, as the install script found it (1.2.1, ADR-0292):
    /// `nvidia` when `weavesignal.sh` saw an NVIDIA card's CDI spec on this server (`GPU_FOUND`),
    /// `none` when it looked and saw none. Left out where nothing looks (the Cloudron package,
    /// whose apps are given no card): the studio then offers none.
    pub gpu_found: Option<String>,
    /// Whether the desk has been handed the card: the settings' `GPU`, `nvidia` or `none`.
    pub gpu: Option<String>,
    /// Whether video is on (`VIDEO`): a card encodes nothing without it.
    pub video: Option<bool>,
}

/// Retention (ADRs 0113-0116): where recordings are kept, and how often the sweeper deletes
/// what has expired. The periods themselves are the station's setting, in the database.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RetentionConfig {
    /// The directory recordings' files are named relative to; the sweeper deletes nothing
    /// outside it.
    pub recordings_dir: PathBuf,
    /// How often the sweeper runs, in seconds (at least 60).
    pub sweep_every_s: u64,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            recordings_dir: PathBuf::from("/var/lib/weavesignal/recordings"),
            sweep_every_s: 3600,
        }
    }
}

/// The live room: which LiveKit room the station's people join, and their tokens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RoomConfig {
    /// LiveKit's URL as people's browsers and apps reach it, through the proxy: an origin
    /// (`wss://station.example`), to which LiveKit's client adds `/rtc` itself.
    pub public_url: String,
    pub api_key: String,
    /// LiveKit's server API on the server itself (not through the proxy): the control plane
    /// takes a dropped caller out of the room with it.
    pub api_url: String,
    /// The room the studio runs in.
    pub name: String,
    /// The desk's identity prefix; nobody else may join under it.
    pub desk_identity: String,
    /// How long a person's room token can be used to join, in seconds (short-lived by design;
    /// at most four hours).
    pub token_ttl_s: u64,
}

impl Default for RoomConfig {
    fn default() -> Self {
        Self {
            public_url: "wss://station.example".to_owned(),
            api_key: "weavesignal".to_owned(),
            api_url: "http://127.0.0.1:7880".to_owned(),
            name: "studio".to_owned(),
            desk_identity: "desk".to_owned(),
            token_ttl_s: 600,
        }
    }
}

/// The desk's control socket (ADR-0088), which the studio app's desk and screener drive.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct DeskConfig {
    /// The socket the desk listens on; left out, the studio app shows the desk as not set up.
    pub socket: Option<PathBuf>,
    /// How often the desk is asked for its status, in milliseconds.
    pub poll_ms: u64,
}

impl Default for DeskConfig {
    fn default() -> Self {
        Self {
            socket: None,
            poll_ms: 250,
        }
    }
}

/// The `[keys]` settings (1.3.2): keyed shows. A show is keyed in the studio (its access rule);
/// these are the installation's figures for how keys work.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct KeysConfig {
    /// The renewal's deadline, in seconds: a player renews within it (tokens are signed with
    /// ECDSA P-256 and last their show's window; a revoked or paused key is refused through the
    /// revoked list at once, and a signed-out device at its next renewal).
    pub token_ttl_s: u64,
    /// How often a player renews its token, in seconds (told to the player; less than the
    /// token's life).
    pub renew_every_s: u64,
    /// The leeway, in seconds, the edge gives a token's start and end for its own clock.
    pub clock_skew_s: u64,
    /// A device that has not renewed for this long (seconds) no longer counts as holding its
    /// key, so a closed browser frees its place.
    pub device_hold_s: u64,
    /// A device back after this long (seconds) starts a new listen; sooner, it carries on the
    /// same one (a page reload does not use a listen).
    pub listen_gap_s: u64,
    /// A renewal writes the device's last sign of life at most this often, in seconds.
    pub touch_every_s: u64,
    /// How long the use records behind the leak report are kept, in days (ADR-0122).
    pub use_records_days: u32,
    /// How many days before a key ends the player says so.
    pub ends_soon_days: u32,
    /// Redeems and renewals one address may make per minute (0: no limit).
    pub redeem_per_minute: u32,
    /// Keys one address may get wrong in ten minutes before it is refused for a while (0: no
    /// limit).
    pub failures_per_ten_minutes: u32,
    /// Lost-key requests one address may make per hour, and one email address (0: no limit).
    pub lost_per_hour: u32,
    /// Where a person gets a key, in the station's words: shown beside Enter a key.
    pub get_a_key_text: String,
    /// A link for it (a sign-up page, a shop); empty for none.
    pub get_a_key_url: String,
    /// The public gate shuts this long before a keyed show's window, in seconds.
    pub window_lead_s: u64,
    /// And stays shut this long after it, in seconds, on top of the desk's profanity delay
    /// (which the control plane adds as the desk reports it).
    pub window_tail_s: u64,
    /// How long after a moment the desk's HLS may still write it into a segment, in seconds:
    /// the public HLS stays held back this long after a keyed window, and a segment written
    /// within it is served only on its show's keyed path. Not measured on a real desk yet: a
    /// generous bound, since the error then goes one way only.
    pub hls_lag_s: u64,
    /// RTMP out stops this long before a keyed window's lead, in seconds (the control plane
    /// hands the desk its destinations every few seconds).
    pub rtmp_lead_s: u64,
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            token_ttl_s: 120,
            renew_every_s: 60,
            clock_skew_s: 5,
            device_hold_s: 360,
            listen_gap_s: 600,
            touch_every_s: 30,
            use_records_days: 90,
            ends_soon_days: 7,
            redeem_per_minute: 120,
            failures_per_ten_minutes: 20,
            lost_per_hour: 5,
            get_a_key_text: String::new(),
            get_a_key_url: String::new(),
            window_lead_s: 3,
            window_tail_s: 5,
            hls_lag_s: 30,
            rtmp_lead_s: 15,
        }
    }
}

impl KeysConfig {
    /// The margins around every keyed window, before the desk's delay is added to the tail.
    pub fn margins(&self) -> crate::keyed::Margins {
        let ms = |s: u64| i64::try_from(s.saturating_mul(1000)).unwrap_or(i64::MAX);
        crate::keyed::Margins {
            lead_ms: ms(self.window_lead_s),
            tail_ms: ms(self.window_tail_s),
            hls_lag_ms: ms(self.hls_lag_s),
            rtmp_lead_ms: ms(self.rtmp_lead_s),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |s: String| Err(ConfigError::Invalid(s));
        if !(30..=900).contains(&self.token_ttl_s) {
            return bad(format!(
                "`keys.token_ttl_s` is {}; a token lives 30 to 900 s",
                self.token_ttl_s
            ));
        }
        if self.renew_every_s < 10 || self.renew_every_s * 2 > self.token_ttl_s {
            return bad(format!(
                "`keys.renew_every_s` is {}; renew at least every {} s (half the token's life) and not faster than every 10 s",
                self.renew_every_s,
                self.token_ttl_s / 2
            ));
        }
        if self.clock_skew_s > 60 {
            return bad(format!(
                "`keys.clock_skew_s` is {}; allow 0 to 60 s",
                self.clock_skew_s
            ));
        }
        if self.device_hold_s < self.token_ttl_s || self.device_hold_s > 86_400 {
            return bad(format!(
                "`keys.device_hold_s` is {}; a device holds a key for at least a token's life ({} s) and at most a day",
                self.device_hold_s, self.token_ttl_s
            ));
        }
        if !(60..=86_400).contains(&self.listen_gap_s) {
            return bad(format!(
                "`keys.listen_gap_s` is {}; allow 60 to 86400 s",
                self.listen_gap_s
            ));
        }
        if !(1..=self.renew_every_s).contains(&self.touch_every_s) {
            return bad(format!(
                "`keys.touch_every_s` is {}; allow 1 to the renewal's {} s",
                self.touch_every_s, self.renew_every_s
            ));
        }
        if !(1..=3650).contains(&self.use_records_days) {
            return bad(format!(
                "`keys.use_records_days` is {}; keep use records 1 to 3650 days",
                self.use_records_days
            ));
        }
        if self.ends_soon_days > 365 {
            return bad(format!(
                "`keys.ends_soon_days` is {}; allow 0 to 365",
                self.ends_soon_days
            ));
        }
        if self.get_a_key_text.chars().count() > 500 || self.get_a_key_url.chars().count() > 300 {
            return bad(
                "`keys.get_a_key_text` is at most 500 characters and `keys.get_a_key_url` 300"
                    .into(),
            );
        }
        if !(1..=60).contains(&self.window_lead_s) || !(1..=300).contains(&self.window_tail_s) {
            return bad(format!(
                "`keys.window_lead_s` is {} and `keys.window_tail_s` {}; allow 1 to 60 s before and 1 to 300 s after",
                self.window_lead_s, self.window_tail_s
            ));
        }
        if !(5..=600).contains(&self.hls_lag_s) || !(5..=300).contains(&self.rtmp_lead_s) {
            return bad(format!(
                "`keys.hls_lag_s` is {} and `keys.rtmp_lead_s` {}; allow 5 to 600 s and 5 to 300 s",
                self.hls_lag_s, self.rtmp_lead_s
            ));
        }
        if !self.get_a_key_url.is_empty()
            && !(self.get_a_key_url.starts_with("https://")
                || self.get_a_key_url.starts_with("http://"))
        {
            return bad("`keys.get_a_key_url` must start with https:// (or http://)".into());
        }
        Ok(())
    }
}

/// The rehearsal studio (1.3.0, #23): the rehearsal desk's control socket and the studio's
/// limits. The rehearsal desk mixes `<room.name><room_suffix>` and has no way to the air.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RehearsalConfig {
    /// Whether this station has a rehearsal studio. The installation writes it from its
    /// `REHEARSAL` setting; off, the studio's rehearsal routes say it is off here.
    pub enabled: bool,
    /// The rehearsal desk's control socket (`/run/weavesignal/desk-rehearsal.sock`); needed
    /// when `enabled`.
    pub socket: Option<PathBuf>,
    /// The rehearsal desk's identity in its room (its returns are `<identity>-mm-<person>`).
    pub desk_identity: String,
    /// What follows the live room's name to make the rehearsal room's: `studio-rehearsal`.
    pub room_suffix: String,
    /// The most people in the rehearsal room at once.
    pub max_people: u16,
    /// An open session with nobody in it for this long ends by itself, in seconds.
    pub idle_end_s: u64,
}

impl Default for RehearsalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            socket: None,
            desk_identity: "desk-rehearsal".to_owned(),
            room_suffix: "-rehearsal".to_owned(),
            max_people: 6,
            idle_end_s: 1_800,
        }
    }
}

impl RehearsalConfig {
    /// The rehearsal room's name beside the live room `room`.
    pub fn room(&self, room: &str) -> String {
        format!("{room}{}", self.room_suffix)
    }

    pub fn validate(&self, live_desk_identity: &str) -> Result<(), ConfigError> {
        if self.enabled && self.socket.is_none() {
            return Err(ConfigError::Invalid(
                "`rehearsal.enabled` is on but `rehearsal.socket` is not set: name the rehearsal desk's control socket".into(),
            ));
        }
        let suffix_ok = self.room_suffix.len() > 1
            && self.room_suffix.len() <= 40
            && self.room_suffix.starts_with('-')
            && self
                .room_suffix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        // The live desk finds booths, phone booths and monitor rooms by these words in a room's
        // name: the rehearsal room must never look like one of them.
        let clashes = ["-booth-", "-phone", "-monitor-"]
            .iter()
            .any(|w| self.room_suffix.contains(w));
        if !suffix_ok || clashes {
            return Err(ConfigError::Invalid(format!(
                "`rehearsal.room_suffix` is `{}`; write a dash and lower-case letters, digits or dashes (at most 40), without `-booth-`, `-phone` or `-monitor-`",
                self.room_suffix
            )));
        }
        if self.desk_identity.trim().is_empty() || self.desk_identity == live_desk_identity {
            return Err(ConfigError::Invalid(
                "`rehearsal.desk_identity` must be set, and not the live desk's `room.desk_identity`".into(),
            ));
        }
        if !(1..=20).contains(&self.max_people) {
            return Err(ConfigError::Invalid(format!(
                "`rehearsal.max_people` is {}; allow 1 to 20",
                self.max_people
            )));
        }
        if !(60..=86_400).contains(&self.idle_end_s) {
            return Err(ConfigError::Invalid(format!(
                "`rehearsal.idle_end_s` is {}; end an empty session after 60 to 86400 s",
                self.idle_end_s
            )));
        }
        Ok(())
    }
}

/// Playout's control socket (Liquidsoap's server socket). Carts no longer fire through it:
/// they sound in the desk (ADR-0117).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct PlayoutConfig {
    /// The socket playout listens on (`control_socket` in its data).
    pub socket: Option<PathBuf>,
    /// How often playout is asked for its state, in milliseconds.
    pub poll_ms: u64,
    /// The listener process's socket (`listeners.control_socket` in the data): the legal
    /// logger is turned on and off through it. Left out, the logger cannot record.
    pub listeners_socket: Option<PathBuf>,
    /// The music library's files (FLAC, named by UUID); playout reads the same directory (its
    /// `library.dir`).
    pub library_dir: PathBuf,
    /// The library's loudness target, in LUFS.
    pub library_lufs: f64,
    /// The longest track, in seconds.
    pub library_max_seconds: f64,
    /// The largest track upload, in megabytes.
    pub library_max_upload_mb: u64,
}

impl Default for PlayoutConfig {
    fn default() -> Self {
        Self {
            socket: None,
            poll_ms: 250,
            listeners_socket: None,
            library_dir: PathBuf::from("/var/lib/weavesignal/library"),
            library_lufs: -23.0,
            library_max_seconds: 1800.0,
            library_max_upload_mb: 200,
        }
    }
}

/// The cart wall: where the media worker's files live, the loudness they are brought to, and
/// the demo pack.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct CartsConfig {
    /// The cart files (FLAC, named by UUID); the desk reads the same directory (its
    /// `carts.dir`) and plays them.
    pub media_dir: PathBuf,
    /// The station's loudness target for carts, in LUFS (EBU R128, as the desk's meters), when
    /// the settings file (or [`CARTS_LOUDNESS_ENV`]) sets one. Left out, the station's default
    /// is in force: -23 for a station whose database is older than 1.3.3 (migration 0073 kept
    /// it), -16 for one installed since ([`CartsConfig::with_station_default`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f64>,
    /// The true peak a cart's gain may not push it above, in dBTP.
    pub ceiling_dbtp: f64,
    /// The largest upload, in megabytes.
    pub max_upload_mb: u64,
    /// The longest cart, in seconds.
    pub max_seconds: f64,
    /// How long the media worker may take over one file, in seconds.
    pub worker_timeout_s: u64,
    /// The media worker's program, run as `<worker> media-worker normalise`: a
    /// `weavesignal-control`; left out, this program.
    pub worker: Option<PathBuf>,
    /// A cart pack (a directory with `carts.json`), loaded as the banks when the station has
    /// none; `assets/carts-demo` is the placeholder pack.
    pub demo_pack: Option<PathBuf>,
}

impl Default for CartsConfig {
    fn default() -> Self {
        Self {
            media_dir: PathBuf::from("/var/lib/weavesignal/carts"),
            loudness_lufs: None,
            ceiling_dbtp: -1.0,
            max_upload_mb: 64,
            max_seconds: 600.0,
            worker_timeout_s: 120,
            worker: None,
            demo_pack: None,
        }
    }
}

impl CartsConfig {
    /// A new station's loudness target for carts, in LUFS (1.3.3): the internet-radio norm, as
    /// playout's own processing (ADR-0044).
    pub const NEW_STATION_LUFS: f64 = -16.0;

    /// The loudness the media worker brings each cart to: the one set, else
    /// [`Self::NEW_STATION_LUFS`]. A station from before 1.3.3 has its old default set by
    /// [`Self::with_station_default`] before this is read.
    pub fn target(&self) -> f64 {
        self.loudness_lufs.unwrap_or(Self::NEW_STATION_LUFS)
    }

    /// Fill a target the settings left out with the station's own default from its database
    /// (`station.carts_loudness_default`: -23 for every station migration 0073 found, none for
    /// a station made since). A target that is set wins.
    pub fn with_station_default(mut self, station_default: Option<f64>) -> Self {
        self.loudness_lufs = self.loudness_lufs.or(station_default);
        self
    }

    fn check(&self) -> Result<(), ConfigError> {
        if !(-40.0..=-5.0).contains(&self.target()) || !(-10.0..=0.0).contains(&self.ceiling_dbtp) {
            return Err(ConfigError::Invalid(
                "`carts.loudness_lufs` is -40 to -5 and `carts.ceiling_dbtp` -10 to 0".into(),
            ));
        }
        Ok(())
    }
}

/// The video slate's still (ADR-0010): a picture the studio uploads, which the media worker makes
/// into a clean PNG and the desk shows on the slate while nobody is on camera.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct VideoConfig {
    /// The slate's still, a PNG the control plane writes and the desk reads (the desk's
    /// `video.still_file`). It sits in the carts' directory, a volume the two already share.
    pub still_file: PathBuf,
    /// The largest upload, in megabytes.
    pub still_max_upload_mb: u64,
    /// The picture is made to fit this many pixels, never enlarged; the desk fits it again to
    /// its canvas.
    pub still_max_width: u32,
    pub still_max_height: u32,
    /// A PNG made the station's still when the station has none and has never had this one
    /// (ADR-0200): the demo station's kettle, `assets/stills-demo`. Left out or empty, none.
    pub demo_still: Option<PathBuf>,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            still_file: PathBuf::from("/var/lib/weavesignal/carts/stills/slate.png"),
            still_max_upload_mb: 24,
            still_max_width: 1280,
            still_max_height: 720,
            demo_still: None,
        }
    }
}

impl VideoConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(1..=24).contains(&self.still_max_upload_mb) {
            return Err(ConfigError::Invalid(format!(
                "`video.still_max_upload_mb` is {}; a still is 1 to 24 MB (the media worker reads no more)",
                self.still_max_upload_mb
            )));
        }
        for (name, v) in [
            ("still_max_width", self.still_max_width),
            ("still_max_height", self.still_max_height),
        ] {
            if !(16..=8192).contains(&v) {
                return Err(ConfigError::Invalid(format!(
                    "`video.{name}` is {v}; 16 to 8192 pixels"
                )));
            }
        }
        if self.still_file.file_name().is_none() {
            return Err(ConfigError::Invalid(
                "`video.still_file` names a file".into(),
            ));
        }
        Ok(())
    }
}

/// How people join from a browser (M3): the player page's call-in, the caller's consent to
/// record, and the station's invite links for guests, remote DJs and presenters.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct JoinConfig {
    /// The listener stream as the player page reaches it, through the proxy; empty, the
    /// player says the stream is not set up.
    pub stream_url: String,
    /// The Opus stream's constant bit rate in kbit/s (ADR-0233; the listener process's
    /// `listeners.bitrate`), so the player can name it beside the MP3 streams.
    pub stream_kbps: u32,
    /// The MP3 streams beside the Opus one (ADR-0002, ADR-0232): the player offers each one
    /// that is on, at [`Self::stream_url`]'s address with its own mount.
    pub mp3: Vec<Mp3Stream>,
    /// Whether the player page offers "call in" (ADR-0006).
    pub call_in: bool,
    /// What a caller agrees to before they can call. `{station}` becomes the station's name,
    /// `{kept}` how long programme recordings are kept (from the Jurisdiction setting), and
    /// `{contact}` [`Self::consent_contact`], and `{presenter}` the show on air's presenter, else
    /// the station's name (issue #26; the default words do not use it). Changing any of them
    /// changes the words' version; a caller who agreed to older words is asked again.
    pub consent_text: String,
    /// How a caller asks for their call to be taken down: an email address or a link. Call-in
    /// stays closed while the words name `{contact}` and this is empty (ADR-0124's wording).
    pub consent_contact: String,
    /// How long a caller's pass lasts: their wait, the call, and a rejoin (seconds).
    pub call_pass_ttl_s: u64,
    /// How long an invite link works, unless the invite asks for less (seconds, at most a day).
    pub invite_ttl_s: u64,
    /// Calls one address may place in an hour.
    pub calls_per_address_per_hour: u32,
    /// Calls the whole station takes in a minute.
    pub calls_per_minute: u32,
    /// Take the caller's address from `X-Forwarded-For` (set this only behind the station's
    /// own reverse proxy, which sets or appends the header): its last entry, the one that proxy
    /// added.
    pub forwarded_for: bool,
    /// Call-in's challenge (ADR-0006): the proof of work's difficulty in bits, which the
    /// caller's browser meets while they type; 0 turns it off. Each bit doubles the work.
    pub challenge_bits: u8,
    /// The difficulty while the station's minute is at least half used (a flood).
    pub challenge_bits_busy: u8,
    /// How long a challenge can be answered (seconds): a lobby left open longer fetches another.
    pub challenge_ttl_s: u64,
    /// Where the desk writes its CMAF HLS (the desk's `video.hls.dir`); the control plane
    /// serves it at `/hls/` while the desk has video on (ADR-0009).
    pub hls_dir: PathBuf,
    /// The HLS as the player reaches it: `/hls` through the proxy, or an edge or CDN's
    /// address for the same files (ADR-0017).
    pub hls_url: String,
    /// The station's public address (`publishing.public_url`), copied in at start and not a
    /// setting of its own: an invite's link is made absolute with it, because on Cloudron the
    /// join page is on the public domain while the studio app talks to the studio domain,
    /// behind Cloudron's login. Empty, the link stays relative to wherever the app reaches.
    #[serde(skip)]
    pub public_url: String,
    /// The STUN servers the station's operator has named (ADR-0290), `host:port` each; the
    /// pages' only licence to ask any. Written as one comma-separated text (the station's
    /// `STUN_SERVERS`, which LiveKit and the desk read too) or as a list. Empty, the default:
    /// the pages strip every STUN server LiveKit hands them.
    #[serde(deserialize_with = "stun_servers")]
    pub stun_servers: Vec<String>,
}

/// `join.stun_servers`: a comma-separated text or a list of `host:port`, each checked.
fn stun_servers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Text {
        One(String),
        Many(Vec<String>),
    }
    let text = match Text::deserialize(d)? {
        Text::One(t) => t,
        Text::Many(list) => list.join(","),
    };
    weavesignal_proto::parse_stun_servers(&text).map_err(serde::de::Error::custom)
}

/// One MP3 stream the listener process encodes (`listeners.compat`, `listeners.compat_high`).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Mp3Stream {
    /// Off, the player does not offer it (the bundle writes both, on or off). A mount that is
    /// not a mount name, or one already offered, is left out of the player, not refused: the
    /// listener process is what refuses a station's streams.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Its mount on the listener port, e.g. `programme.mp3`.
    pub mount: String,
    /// Its constant bit rate, in kbit/s.
    pub kbps: u32,
}

fn yes() -> bool {
    true
}

/// A mount's name as the listener process takes it: letters, digits, `.`, `_` and `-`.
pub fn is_mount_name(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The default consent wording (operator, 2026-11-01: two sentences, for internet radio); a
/// station changes it in `join.consent_text`.
pub const CONSENT_TEXT: &str = "You'll be live on {station}, and we may keep your call in our archive and podcast {kept}. To have it taken down, contact {contact}.";

impl Default for JoinConfig {
    fn default() -> Self {
        Self {
            stream_url: String::new(),
            stream_kbps: 96,
            mp3: Vec::new(),
            call_in: true,
            consent_text: CONSENT_TEXT.to_owned(),
            consent_contact: String::new(),
            call_pass_ttl_s: 4 * 3600,
            invite_ttl_s: 4 * 3600,
            calls_per_address_per_hour: 6,
            calls_per_minute: 30,
            forwarded_for: false,
            challenge_bits: 18,
            challenge_bits_busy: 21,
            challenge_ttl_s: 30 * 60,
            hls_dir: PathBuf::from("/var/lib/weavesignal/hls"),
            hls_url: "/hls".to_owned(),
            public_url: String::new(),
            stun_servers: Vec::new(),
        }
    }
}

/// The show on air and what is coming up, until the schedule arrives (M4); and the callers
/// banned before, until accounts and the moderation log arrive (M6).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct StudioConfig {
    pub show: String,
    pub rundown: Vec<RundownItem>,
    pub banned: Vec<BannedCaller>,
    /// How far behind the studio listeners hear, in milliseconds, not counting the profanity
    /// delay (the studio view adds that delay's current length, ADR-0165): a setting, shown
    /// labelled as one, until the station measures it (ADR-0103). Spike 01 measured about 15 s.
    ///
    /// Since 1.1.1 this is the old way to say it, kept so that a station set up before keeps
    /// the same figure: the mount's hold and a player allowance, with the buffer's nominal 0.5 s
    /// inside. It is used only while neither of the two settings below is set.
    pub listener_delay_ms: u64,
    /// The listener mount's own 12 s hold is a constant (`studio::DEFAULT_MOUNT_HOLD_MS`): it is
    /// not a setting, since playout has no way to change it (ADR-0289).
    /// What a listener's own player is taken to add, in milliseconds (ADR-0289): 500 unless set.
    #[serde(
        deserialize_with = "optional_ms",
        skip_serializing_if = "Option::is_none"
    )]
    pub player_allowance_ms: Option<u64>,
}

impl StudioConfig {
    /// The delay's setting part as the settings say it: the mount's fixed 12 s plus
    /// `player_allowance_ms` when that is set (it wins over a changed `listener_delay_ms`);
    /// otherwise `listener_delay_ms` stands, as before.
    pub fn listener_path(&self) -> crate::studio::ListenerPath {
        use crate::studio::ListenerPath;
        let Some(player_allowance_ms) = self.player_allowance_ms else {
            // 13000 is the old default, so a station that never set it is shown in parts (and
            // takes the parts' defaults, 1.3.1's 3 s player allowance among them); any other old
            // figure cannot be split and stays whole.
            return if self.listener_delay_ms == StudioConfig::default().listener_delay_ms {
                ListenerPath::default_parts()
            } else {
                ListenerPath::Whole(self.listener_delay_ms)
            };
        };
        ListenerPath::Parts {
            player_allowance_ms,
        }
    }

    /// True when `listener_delay_ms` was changed from its default and is being ignored because
    /// `player_allowance_ms` is set: the two ways were both asked for.
    pub fn listener_delay_ms_is_overridden(&self) -> bool {
        self.player_allowance_ms.is_some()
            && self.listener_delay_ms != StudioConfig::default().listener_delay_ms
    }
}

/// A number of milliseconds that may also be written as a string, where an empty string is
/// "not set": the Compose bundle's settings file cannot leave a line out, so it writes
/// `player_allowance_ms = "${STUDIO_PLAYER_ALLOWANCE_MS:-}"`.
fn optional_ms<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Ms {
        Number(u64),
        Text(String),
    }
    match Option::<Ms>::deserialize(d)? {
        None => Ok(None),
        Some(Ms::Number(n)) => Ok(Some(n)),
        Some(Ms::Text(t)) if t.trim().is_empty() => Ok(None),
        Some(Ms::Text(t)) => t.trim().parse().map(Some).map_err(|_| {
            serde::de::Error::custom(format!("`{t}` is not a whole number of milliseconds"))
        }),
    }
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self {
            show: String::new(),
            rundown: Vec::new(),
            banned: Vec::new(),
            listener_delay_ms: 13_000,
            player_allowance_ms: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BannedCaller {
    pub identity: String,
    /// The day of the ban, `YYYY-MM-DD`.
    pub date: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StationConfig {
    /// The station's identifier (ADR-0047); `weavesignal-control sample-config` makes one.
    pub id: StationId,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ListenConfig {
    /// The address internal services bind; loopback, behind the reverse proxy (ADR-0048 rule 3).
    #[serde(default = "loopback")]
    pub internal: IpAddr,
}

fn loopback() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            internal: loopback(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// One JSON object per line, for the journal and log collectors.
    #[default]
    Json,
    /// Human-readable, for a terminal.
    Pretty,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    #[serde(default)]
    pub format: LogFormat,
    /// A tracing filter such as `info` or `weavesignal_control=debug`; `RUST_LOG` overrides it.
    #[serde(default)]
    pub filter: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read the settings file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("the settings file {path} is not valid: {source}")]
    Parse {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("the settings are not valid: {0}")]
    Invalid(String),
}

/// The station's own additions to a generated settings file (issue 5): a second file that may
/// hold only `[studio]` (the show, the rundown, the banned list) and `[steering]`, which the
/// Compose bundle and the Cloudron package do not write. Its sections replace the file's.
pub const EXTRA_ENV: &str = "WEAVESIGNAL_CONTROL_EXTRA";

/// The sections the additions file may hold.
const EXTRA_SECTIONS: [&str; 2] = ["studio", "steering"];

/// `base` with the sections of `extra` put in place of its own.
pub fn with_extra(
    base: &str,
    base_path: &str,
    extra: &str,
    extra_path: &str,
) -> Result<String, ConfigError> {
    let parse = |text: &str, path: &str| {
        text.parse::<toml::Table>().map_err(|e| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })
    };
    let mut table = parse(base, base_path)?;
    for (k, v) in parse(extra, extra_path)? {
        if !EXTRA_SECTIONS.contains(&k.as_str()) {
            return Err(ConfigError::Invalid(format!(
                "{extra_path} may hold only [studio] and [steering]; it has `{k}`"
            )));
        }
        table.insert(k, v);
    }
    toml::to_string(&table).map_err(|e| ConfigError::Invalid(e.to_string()))
}

impl Config {
    pub fn from_toml(text: &str, path: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text).map_err(|e| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Read the settings file, then the station's own additions (`WEAVESIGNAL_CONTROL_EXTRA`,
    /// when set and the file exists), then take secrets from the environment.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let shown = path.display().to_string();
        let mut text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: shown.clone(),
            source,
        })?;
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(extra) = env(EXTRA_ENV).filter(|p| Path::new(p).is_file()) {
            let more = std::fs::read_to_string(&extra).map_err(|source| ConfigError::Read {
                path: extra.clone(),
                source,
            })?;
            text = with_extra(&text, &shown, &more, &extra)?;
        }
        let mut config = Self::from_toml(&text, &shown)?;
        config.with_consent_text(env(CONSENT_TEXT_ENV).as_deref());
        config.with_carts_loudness(env(CARTS_LOUDNESS_ENV).as_deref())?;
        config.database_url = env(DATABASE_URL_ENV);
        config.livekit_secret = env(LIVEKIT_SECRET_ENV);
        config.admin_token = env(ADMIN_TOKEN_ENV);
        // A key that is set but too short is refused here; one that is missing is refused when
        // the control plane starts with mail on, with the same words.
        config.email_key = env(MailKey::ENV)
            .map(|k| MailKey::parse(&k))
            .transpose()
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        config.api_key = crate::apikey::ApiKey::resolve(
            env(crate::apikey::ApiKey::ENV).as_deref(),
            config.admin_token.as_deref(),
        )
        .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        Ok(config)
    }

    /// The consent words from [`CONSENT_TEXT_ENV`] in place of the settings file's, when given
    /// and not blank; otherwise the file's (or the default) stay.
    pub fn with_consent_text(&mut self, text: Option<&str>) {
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            self.join.consent_text = text.to_owned();
        }
    }

    /// The cart target from [`CARTS_LOUDNESS_ENV`] in place of the settings file's, when given
    /// and not blank; otherwise the file's (or none) stays.
    pub fn with_carts_loudness(&mut self, value: Option<&str>) -> Result<(), ConfigError> {
        let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(());
        };
        let lufs = v.parse::<f64>().map_err(|_| {
            ConfigError::Invalid(format!(
                "{CARTS_LOUDNESS_ENV} is `{v}`: a loudness in LUFS, such as -16"
            ))
        })?;
        self.carts.loudness_lufs = Some(lufs);
        self.carts.check()
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.station()?;
        self.publishing.validate()?;
        self.steering.validate()?;
        if self.mail.mode == MailMode::Command && self.mail.command.is_empty() {
            return Err(ConfigError::Invalid(
                "`mail.command` is empty: name a sendmail-compatible program".into(),
            ));
        }
        if self.mail.mode != MailMode::Off && self.mail.from.trim().is_empty() {
            return Err(ConfigError::Invalid("`mail.from` must not be empty".into()));
        }
        self.video.validate()?;
        self.phone.validate()?;
        self.rehearsal.validate(&self.room.desk_identity)?;
        self.keys.validate()?;
        let max = weavesignal_proto::token::MAX_PARTICIPANT_TTL_S;
        if !(1..=max).contains(&self.room.token_ttl_s) {
            return Err(ConfigError::Invalid(format!(
                "`room.token_ttl_s` is {}; a room token lives 1 to {max} s",
                self.room.token_ttl_s
            )));
        }
        if self.room.name.is_empty() || self.room.api_key.is_empty() {
            return Err(ConfigError::Invalid(
                "`room.name` and `room.api_key` must not be empty".into(),
            ));
        }
        if !(50..=5000).contains(&self.playout.poll_ms) {
            return Err(ConfigError::Invalid(format!(
                "`playout.poll_ms` is {}; ask playout every 50 to 5000 ms",
                self.playout.poll_ms
            )));
        }
        let c = &self.carts;
        c.check()?;
        if !(1..=1024).contains(&c.max_upload_mb)
            || !(1.0..=3600.0).contains(&c.max_seconds)
            || !(5..=3600).contains(&c.worker_timeout_s)
        {
            return Err(ConfigError::Invalid(
                "`carts.max_upload_mb` is 1 to 1024, `carts.max_seconds` 1 to 3600 and `carts.worker_timeout_s` 5 to 3600".into(),
            ));
        }
        if !(50..=5000).contains(&self.desk.poll_ms) {
            return Err(ConfigError::Invalid(format!(
                "`desk.poll_ms` is {}; ask the desk every 50 to 5000 ms",
                self.desk.poll_ms
            )));
        }
        for item in &self.studio.rundown {
            let ok = matches!(item.at.len(), 5 | 8)
                && item
                    .at
                    .split(':')
                    .all(|p| p.len() == 2 && p.parse::<u8>().is_ok());
            if !ok {
                return Err(ConfigError::Invalid(format!(
                    "`studio.rundown` has `at = \"{}\"`; write HH:MM or HH:MM:SS",
                    item.at
                )));
            }
        }
        let pass_max = weavesignal_proto::MAX_PASS_TTL_S;
        for (key, v) in [
            ("join.call_pass_ttl_s", self.join.call_pass_ttl_s),
            ("join.invite_ttl_s", self.join.invite_ttl_s),
        ] {
            if !(60..=pass_max).contains(&v) {
                return Err(ConfigError::Invalid(format!(
                    "`{key}` is {v}; a pass lives 60 to {pass_max} s"
                )));
            }
        }
        let max_bits = crate::challenge::MAX_BITS;
        if self.join.challenge_bits > max_bits
            || self.join.challenge_bits_busy > max_bits
            || self.join.challenge_bits_busy < self.join.challenge_bits
        {
            return Err(ConfigError::Invalid(format!(
                "`join.challenge_bits` and `join.challenge_bits_busy` are 0 to {max_bits}, the busy one no lower: each bit doubles a caller's wait"
            )));
        }
        if !(60..=24 * 3600).contains(&self.join.challenge_ttl_s) {
            return Err(ConfigError::Invalid(format!(
                "`join.challenge_ttl_s` is {}; a challenge lives 60 s to a day",
                self.join.challenge_ttl_s
            )));
        }
        if self.join.consent_text.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "`join.consent_text` must not be empty: a caller agrees to it before calling"
                    .into(),
            ));
        }
        if !(64..=256).contains(&self.join.stream_kbps) {
            return Err(ConfigError::Invalid(format!(
                "`join.stream_kbps` is {}; the Opus stream takes 64 to 256 kbit/s (OPUS_BITRATE)",
                self.join.stream_kbps
            )));
        }
        let contact = &self.join.consent_contact;
        if contact.chars().count() > 200 || contact.chars().any(char::is_control) {
            return Err(ConfigError::Invalid(
                "`join.consent_contact` is an email address or a link: at most 200 characters, on one line"
                    .into(),
            ));
        }
        if self
            .setup
            .overrides_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        {
            return Err(ConfigError::Invalid(
                "`setup.overrides_file` must be an absolute path".into(),
            ));
        }
        for (key, value) in [
            ("gpu_found", &self.setup.gpu_found),
            ("gpu", &self.setup.gpu),
        ] {
            if let Some(v) = value
                && !matches!(v.as_str(), "nvidia" | "none" | "auto")
            {
                return Err(ConfigError::Invalid(format!(
                    "`setup.{key}` is `{v}`; it is `nvidia` or `none`"
                )));
            }
        }
        if self.retention.sweep_every_s < 60 {
            return Err(ConfigError::Invalid(format!(
                "`retention.sweep_every_s` is {}; the sweeper runs at most once a minute",
                self.retention.sweep_every_s
            )));
        }
        if !self.retention.recordings_dir.is_absolute() {
            return Err(ConfigError::Invalid(
                "`retention.recordings_dir` must be an absolute path".into(),
            ));
        }
        if let Some((first, second, port, proto)) = self.ports.duplicates().into_iter().next() {
            return Err(ConfigError::Invalid(format!(
                "`ports.{first}` and `ports.{second}` both ask for {port}/{proto}"
            )));
        }
        Ok(())
    }

    pub fn station(&self) -> Result<Station, ConfigError> {
        Station::new(self.station.id, &self.station.name)
            .map_err(|e| ConfigError::Invalid(e.to_string()))
    }
}

/// Show cards and podcast feeds (M8): the station's public address, which every card prints
/// (ADR-0056) and every feed, card file and episode file is reached under, and what the feeds
/// say about their owner.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct PublishingConfig {
    /// `https://` and the station's public host; empty, cards and feeds are off.
    pub public_url: String,
    /// Where the station is, for the cards and `podcast:location`; may be empty.
    pub place: String,
    /// The station's time zone (IANA), for air times on cards and the schedule rule.
    pub zone: String,
    /// The feeds' language (BCP 47).
    pub language: String,
    /// The feeds' owner (`itunes:owner`); empty, the station's name.
    pub owner_name: String,
    /// The owner's address for `itunes:owner` and `podcast:locked`; empty, neither is written.
    pub owner_email: String,
    pub copyright: String,
    /// Apple's podcast category and subcategory.
    pub category: String,
    pub subcategory: String,
    /// `podcast:locked`: other platforms may not import the feeds.
    pub locked: bool,
    /// The rendered cards, served at `/cards/` (content-hashed names, never overwritten).
    pub cards_dir: PathBuf,
    /// The published episode files, served at `/media/episodes/`.
    pub episodes_dir: PathBuf,
}

impl Default for PublishingConfig {
    fn default() -> Self {
        Self {
            public_url: String::new(),
            place: String::new(),
            zone: "Europe/London".into(),
            language: "en-gb".into(),
            owner_name: String::new(),
            owner_email: String::new(),
            copyright: String::new(),
            category: "Music".into(),
            subcategory: String::new(),
            locked: true,
            cards_dir: PathBuf::from("/var/lib/weavesignal/show-cards"),
            episodes_dir: PathBuf::from("/var/lib/weavesignal/episodes"),
        }
    }
}

impl PublishingConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let url = self.public_url.trim_end_matches('/');
        if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(ConfigError::Invalid(
                "`publishing.public_url` starts with https:// (or http:// on a test bench)".into(),
            ));
        }
        if url
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '<' || c == '>')
        {
            return Err(ConfigError::Invalid(
                "`publishing.public_url` is an address without spaces or quotes".into(),
            ));
        }
        if jiff::tz::TimeZone::get(&self.zone).is_err() {
            return Err(ConfigError::Invalid(format!(
                "`publishing.zone` is `{}`, not a time zone such as Europe/London",
                self.zone
            )));
        }
        Ok(())
    }
}

/// The documented sample settings, with a fresh station identifier.
pub fn sample(id: StationId) -> String {
    SAMPLE.replace("00000000-0000-0000-0000-000000000000", &id.to_string())
}

const SAMPLE: &str = include_str!("../control.example.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stations_additions_bring_the_show_and_steering_and_nothing_else() {
        let base = "[station]\nname = \"Kettle FM\"\n[studio]\nshow = \"Old\"\n";
        let extra = "[studio]\nshow = \"The Morning Kettle\"\n[[studio.rundown]]\nat = \"10:07\"\ntitle = \"Handover\"\nkey = \"⇧⏎\"\n";
        let merged = with_extra(base, "control.toml", extra, "station.toml").unwrap();
        let t: toml::Table = merged.parse().unwrap();
        assert_eq!(t["studio"]["show"].as_str(), Some("The Morning Kettle"));
        assert_eq!(t["studio"]["rundown"][0]["key"].as_str(), Some("⇧⏎"));
        assert_eq!(t["station"]["name"].as_str(), Some("Kettle FM"));
        // Anything else is refused, naming the file and the section.
        let e = with_extra(
            base,
            "control.toml",
            "[room]\nname = \"x\"\n",
            "station.toml",
        )
        .unwrap_err();
        assert!(
            e.to_string()
                .contains("station.toml may hold only [studio] and [steering]; it has `room`"),
            "{e}"
        );
        // A file that is not TOML names itself.
        let e = with_extra(base, "control.toml", "[studio", "station.toml").unwrap_err();
        assert!(e.to_string().contains("station.toml"), "{e}");
    }

    const MINIMAL: &str = r#"
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
"#;

    #[test]
    fn a_minimal_file_takes_every_default() {
        let c = Config::from_toml(MINIMAL, "test").unwrap();
        assert_eq!(c.listen.internal, loopback());
        assert_eq!(c.ports, Ports::default());
        assert_eq!(c.log.format, LogFormat::Json);
        assert_eq!(c.station().unwrap().name, "Kettle FM");
    }

    #[test]
    fn the_sample_parses_and_matches_the_defaults() {
        let c = Config::from_toml(&sample(StationId::new_random()), "sample").unwrap();
        assert_eq!(
            c.ports,
            Ports::default(),
            "the sample documents the defaults"
        );
        assert_eq!(c.room, RoomConfig::default());
        assert_eq!(
            c.join,
            JoinConfig::default(),
            "the sample documents the join defaults"
        );
        assert_eq!(c.publishing, PublishingConfig::default());
        assert_eq!(
            c.mail,
            MailConfig::default(),
            "the sample documents the mail defaults, with mail off"
        );
        assert_eq!(
            c.video,
            VideoConfig::default(),
            "the sample documents the video defaults"
        );
        assert_eq!(
            c.steering,
            crate::steering::SteeringConfig::default(),
            "the sample documents the steering defaults, with steering off"
        );
    }

    fn studio_from(extra: &str) -> StudioConfig {
        Config::from_toml(&format!("{MINIMAL}\n[studio]\n{extra}"), "t")
            .unwrap()
            .studio
    }

    #[test]
    fn the_listener_delay_is_the_old_figure_the_parts_or_both() {
        use crate::studio::ListenerPath::{Parts, Whole};
        // Nothing set: the parts' defaults, 12 s of mount, 3 s of player and the buffer's 0.5 s
        // (the old 13 s default had 0.5 s for the player, until 1.3.1).
        let none = studio_from("");
        assert_eq!(
            none.listener_path(),
            crate::studio::ListenerPath::default_parts()
        );
        assert_eq!(none.listener_path().rest_ms(false), 15_500);
        assert_eq!(none.listener_path().rest_ms(true), 15_000);
        // The old default written out is the same station.
        assert_eq!(
            studio_from("listener_delay_ms = 13000\n").listener_path(),
            none.listener_path()
        );
        assert_eq!(
            none.listener_path().rest_ms(false),
            crate::studio::ListenerPath::default_parts().rest_ms(false)
        );
        assert!(!none.listener_delay_ms_is_overridden());
        // An old value alone keeps its meaning (an upgrade changes nothing).
        let old = studio_from("listener_delay_ms = 9000\n");
        assert_eq!(old.listener_path(), Whole(9_000));
        assert_eq!(
            old.listener_path().mount_hold_ms(),
            None,
            "it cannot be split"
        );
        assert_eq!(old.listener_path().total_ms(), 9_000);
        assert!(!old.listener_delay_ms_is_overridden());
        // The allowance alone: the mount's 12 s plus it.
        assert_eq!(
            studio_from("player_allowance_ms = 3000\n").listener_path(),
            Parts {
                player_allowance_ms: 3_000
            }
        );
        assert_eq!(
            studio_from("player_allowance_ms = 3000\n")
                .listener_path()
                .rest_ms(true),
            12_000 + 3_000
        );
        // Both: the allowance wins, and the config says the old figure is being ignored.
        let both = studio_from("listener_delay_ms = 9000\nplayer_allowance_ms = 700\n");
        assert_eq!(
            both.listener_path(),
            Parts {
                player_allowance_ms: 700
            }
        );
        assert!(both.listener_delay_ms_is_overridden());
        // An old figure left at its default beside the allowance is no conflict.
        assert!(
            !studio_from("listener_delay_ms = 13000\nplayer_allowance_ms = 700\n")
                .listener_delay_ms_is_overridden()
        );
        // The Compose bundle writes the allowance as a string, empty when unset.
        let empty = studio_from("player_allowance_ms = \"\"\n");
        assert_eq!(empty.player_allowance_ms, None);
        assert_eq!(empty, StudioConfig::default());
        let text = studio_from("player_allowance_ms = \" 700 \"\n");
        assert_eq!(text.player_allowance_ms, Some(700));
        // The install script's finding of a graphics card (1.2.1): as the Compose bundle writes
        // it, left out (the Cloudron package), and a word that is neither.
        let gpu =
            format!("{MINIMAL}\n[setup]\ngpu_found = \"nvidia\"\ngpu = \"none\"\nvideo = true\n");
        let c = Config::from_toml(&gpu, "t").expect("the bundle's [setup]");
        assert_eq!(
            (
                c.setup.gpu_found.as_deref(),
                c.setup.gpu.as_deref(),
                c.setup.video
            ),
            (Some("nvidia"), Some("none"), Some(true))
        );
        let c = Config::from_toml(MINIMAL, "t").expect("no [setup]");
        assert_eq!(
            (c.setup.gpu_found, c.setup.gpu, c.setup.video),
            (None, None, None)
        );
        let bad = format!("{MINIMAL}\n[setup]\ngpu_found = \"amd\"\n");
        assert!(
            Config::from_toml(&bad, "t")
                .unwrap_err()
                .to_string()
                .contains("setup.gpu_found")
        );
        // The mount's hold is not a setting: a station that writes it is refused, loudly.
        let hold = format!("{MINIMAL}\n[studio]\nmount_hold_ms = 10000\n");
        assert!(Config::from_toml(&hold, "t").is_err());
        let bad = format!("{MINIMAL}\n[studio]\nplayer_allowance_ms = \"a lot\"\n");
        assert!(
            Config::from_toml(&bad, "t")
                .unwrap_err()
                .to_string()
                .contains("whole number")
        );
    }

    #[test]
    fn the_sample_lists_every_port() {
        for spec in crate::ports::PORTS {
            assert!(
                SAMPLE.contains(&format!("{} = {}", spec.key, spec.default)),
                "control.example.toml does not document `{}`",
                spec.key
            );
        }
    }

    #[test]
    fn a_station_identifier_is_required() {
        let err = Config::from_toml("[station]\nname = \"Kettle FM\"\n", "t").unwrap_err();
        assert!(err.to_string().contains("missing field `id`"), "{err}");
    }

    #[test]
    fn unknown_keys_are_refused() {
        let text = format!("{MINIMAL}\n[listen]\nintenral = \"127.0.0.1\"\n");
        assert!(Config::from_toml(&text, "t").is_err());
    }

    #[test]
    fn two_settings_on_one_port_are_refused() {
        let text = format!("{MINIMAL}\n[ports]\nchat = 7310\n");
        let err = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(
            err.contains("`ports.control` and `ports.chat` both ask for 7310/tcp"),
            "{err}"
        );
    }

    #[test]
    fn room_tokens_are_short_lived() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.room.token_ttl_s, 600);
        let text = format!("{MINIMAL}\n[room]\ntoken_ttl_s = 86400\n");
        let err = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(err.contains("token_ttl_s"), "{err}");
    }

    #[test]
    fn the_desk_and_the_rundown_are_settings() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.desk.socket, None);
        assert_eq!(c.desk.poll_ms, 250);
        let text = format!(
            "{MINIMAL}\n[desk]\nsocket = \"/run/weavesignal/desk.sock\"\n\n[studio]\nshow = \"The Morning Kettle\"\n\n[[studio.rundown]]\nat = \"14:00\"\ntitle = \"News\"\nhard_post = true\n"
        );
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.studio.rundown[0].title, "News");
        assert!(c.studio.rundown[0].hard_post);
        let bad = text.replace("14:00", "2pm");
        assert!(
            Config::from_toml(&bad, "t")
                .unwrap_err()
                .to_string()
                .contains("HH:MM")
        );
    }

    #[test]
    fn playout_and_the_carts_are_settings() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.playout.socket, None);
        // Left out: the station's default, which the database decides (migration 0073).
        assert_eq!(c.carts.loudness_lufs, None);
        assert_eq!(c.carts.target(), -16.0);
        assert_eq!(
            c.carts.clone().with_station_default(Some(-23.0)).target(),
            -23.0
        );
        assert_eq!(c.carts.clone().with_station_default(None).target(), -16.0);
        let text = format!(
            "{MINIMAL}\n[playout]\nsocket = \"/run/weavesignal-playout/playout.sock\"\n\n[carts]\nmedia_dir = \"/srv/carts\"\nloudness_lufs = -16.0\ndemo_pack = \"/opt/weavesignal/carts-demo\"\n"
        );
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.carts.media_dir, PathBuf::from("/srv/carts"));
        assert_eq!(c.carts.loudness_lufs, Some(-16.0));
        // Set in the file, it wins over a station's old default.
        assert_eq!(
            c.carts.clone().with_station_default(Some(-23.0)).target(),
            -16.0
        );
        let bad = text.replace("-16.0", "0.0");
        assert!(Config::from_toml(&bad, "t").is_err());
    }

    #[test]
    fn the_cart_target_from_the_environment() {
        let mut c = Config::from_toml(MINIMAL, "t").unwrap();
        // Unset or empty, as a package passes `CARTS_LOUDNESS_LUFS` nobody set: nothing set.
        c.with_carts_loudness(None).unwrap();
        c.with_carts_loudness(Some("")).unwrap();
        c.with_carts_loudness(Some("  ")).unwrap();
        assert_eq!(c.carts.loudness_lufs, None);
        assert_eq!(
            c.carts.clone().with_station_default(Some(-23.0)).target(),
            -23.0
        );
        c.with_carts_loudness(Some("-16")).unwrap();
        assert_eq!(c.carts.loudness_lufs, Some(-16.0));
        assert_eq!(
            c.carts.clone().with_station_default(Some(-23.0)).target(),
            -16.0
        );
        assert!(c.with_carts_loudness(Some("loud")).is_err());
        assert!(c.with_carts_loudness(Some("-3")).is_err());
    }

    #[test]
    fn joining_has_defaults_and_bounds() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert!(c.join.call_in);
        assert!(c.join.consent_text.contains("{station}"));
        assert_eq!(c.join.invite_ttl_s, 14_400);
        let text = format!("{MINIMAL}\n[join]\ninvite_ttl_s = 172800\n");
        assert!(Config::from_toml(&text, "t").is_err());
        let text = format!("{MINIMAL}\n[join]\nconsent_text = \" \"\n");
        assert!(Config::from_toml(&text, "t").is_err());
    }

    #[test]
    fn consent_words_from_the_environment_replace_the_files_unless_blank() {
        let mut c = Config::from_toml(MINIMAL, "t").unwrap();
        // Unset, empty or blank: the default words, exactly as shipped.
        for unset in [None, Some(""), Some("  \t")] {
            c.with_consent_text(unset);
            assert_eq!(c.join.consent_text, CONSENT_TEXT);
        }
        // A station's own words, quotes, commas, braces and all, taken whole.
        let own = r#"You'll be on {presenter}'s show on {station}, "live"; kept {kept}, {contact} \ 100% $5."#;
        c.with_consent_text(Some(own));
        assert_eq!(c.join.consent_text, own);
        // The settings file's own words stay when the environment gives none.
        let text = format!("{MINIMAL}\n[join]\nconsent_text = \"Mine, on {{station}}.\"\n");
        let mut c = Config::from_toml(&text, "t").unwrap();
        c.with_consent_text(Some(""));
        assert_eq!(c.join.consent_text, "Mine, on {station}.");
    }

    #[test]
    fn the_pages_ask_no_stun_server_unless_the_station_names_one() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert!(c.join.stun_servers.is_empty());
        // As the Compose bundle and the Cloudron package write it: one text, possibly empty.
        let text = format!("{MINIMAL}\n[join]\nstun_servers = \"\"\n");
        assert!(
            Config::from_toml(&text, "t")
                .unwrap()
                .join
                .stun_servers
                .is_empty()
        );
        let text =
            format!("{MINIMAL}\n[join]\nstun_servers = \"A.example:3478, b.example:3478\"\n");
        let c = Config::from_toml(&text, "t").unwrap();
        assert_eq!(c.join.stun_servers, ["a.example:3478", "b.example:3478"]);
        // A list is read the same way.
        let text = format!("{MINIMAL}\n[join]\nstun_servers = [\"a.example:3478\"]\n");
        assert_eq!(
            Config::from_toml(&text, "t").unwrap().join.stun_servers,
            ["a.example:3478"]
        );
        // A server that is not host:port stops the start, naming itself.
        let text = format!("{MINIMAL}\n[join]\nstun_servers = \"stun.example.org\"\n");
        let e = Config::from_toml(&text, "t").unwrap_err().to_string();
        assert!(e.contains("stun.example.org"), "{e}");
    }

    #[test]
    fn the_streams_rates_and_mounts_are_checked() {
        let c = Config::from_toml(MINIMAL, "t").unwrap();
        assert_eq!(c.join.stream_kbps, 96);
        assert!(c.join.mp3.is_empty());
        // As the Compose bundle writes them: both MP3 streams listed, on or off.
        let both = |a: &str, b: &str, kbps: u32| {
            format!(
                "{MINIMAL}\n[join]\nstream_kbps = {kbps}\nmp3 = [\n  {{ enabled = true, mount = \"{a}\", kbps = 128 }},\n  {{ enabled = false, mount = \"{b}\", kbps = 320 }},\n]\n"
            )
        };
        let c = Config::from_toml(&both("programme.mp3", "programme-high.mp3", 128), "t").unwrap();
        assert_eq!(c.join.stream_kbps, 128);
        assert_eq!(c.join.mp3.len(), 2);
        assert!(c.join.mp3[0].enabled && !c.join.mp3[1].enabled);
        assert!(Config::from_toml(&both("programme.mp3", "x.mp3", 300), "t").is_err());
        assert!(Config::from_toml(&both("programme.mp3", "x.mp3", 48), "t").is_err());
        // An MP3 stream's mount and rate are the listener process's to refuse: a station whose
        // 1.0.0 settings started keeps starting, and the player leaves out what it cannot reach.
        assert!(Config::from_toml(&both("../etc", "x.mp3", 96), "t").is_ok());
    }

    #[test]
    fn a_blank_station_name_is_refused() {
        let text = MINIMAL.replace("Kettle FM", " ");
        assert!(Config::from_toml(&text, "t").is_err());
    }
}
