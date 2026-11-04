// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The chat service's settings: a TOML file (`chat.toml`), with secrets from the environment.
//! The station's own chat settings (terms, the accountable person, the minimum age, comments
//! only, guests, word lists) are kept in the database and edited from the studio; this file
//! holds what the server needs to start, and their first values.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use weavesignal_model::{Station, StationId};

use crate::key::EmailKey;

/// The database URL; it carries a password, so it is never read from the file.
pub const DATABASE_URL_ENV: &str = "WEAVESIGNAL_DATABASE_URL";
/// The station's admin bearer, the same one the control plane takes (until accounts arrive
/// for the studio's people): the studio app's chat routes accept it.
pub const ADMIN_TOKEN_ENV: &str = "WEAVESIGNAL_CONTROL_ADMIN_TOKEN";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub station: StationConfig,
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default)]
    pub control: ControlLink,
    #[serde(default)]
    pub mail: MailConfig,
    #[serde(default)]
    pub limits: Limits,
    /// The station's chat settings until someone edits them from the studio.
    #[serde(default)]
    pub defaults: StationSettings,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(skip)]
    pub database_url: Option<String>,
    #[serde(skip)]
    pub admin_token: Option<String>,
    /// The secret that keys the hash of chatters' email addresses (ADR-0125); like the database
    /// URL it is never read from the file. The chat will not start without it.
    #[serde(skip)]
    pub email_key: Option<EmailKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StationConfig {
    /// The same identifier as the control plane's (ADR-0047).
    pub id: StationId,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct ListenConfig {
    /// Loopback, behind the station's proxy (ADR-0048 rule 3).
    pub address: IpAddr,
    /// The `chat` port of the station's port registry (ADR-0075): 7311 by default.
    pub port: u16,
    /// Take the client's address from `X-Forwarded-For` (only behind the station's own proxy,
    /// which sets or appends it): its last entry, the one that proxy added.
    pub forwarded_for: bool,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 7311,
            forwarded_for: false,
        }
    }
}

/// The control plane, which keeps the room's modes as the screener switches them (S, K, L and
/// G G, ADR-0068), the show's title and the listener delay. The chat follows its studio view.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct ControlLink {
    /// The control plane on the server itself, e.g. `http://127.0.0.1:7310`; empty, the chat
    /// runs on its own with the default modes.
    pub url: String,
}

impl Default for ControlLink {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:7310".into(),
        }
    }
}

/// How sign-in codes reach a chatter's inbox.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct MailConfig {
    /// `command`: pipe each email to a sendmail-compatible program (msmtp, sendmail -t);
    /// `outbox`: write each to a file in `outbox_dir` (development and tests);
    /// `off`: email sign-in is off and the page says so.
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
            from: "chat@station.example".into(),
            command: vec!["/usr/sbin/sendmail".into(), "-t".into(), "-i".into()],
            outbox_dir: PathBuf::from("/var/lib/weavesignal/chat-outbox"),
        }
    }
}

/// How long each kind of chat record is kept: read from the station's Jurisdiction setting
/// (ADR-0113, ADR-0114; `weavesignal_model::retention`, migration 0005) at start and before each
/// sweep, never from this file. No retention is the default for a new station.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retention {
    /// The room's messages and the private lane to the studio (`chat_messages`).
    pub messages: Keep,
    /// Reports, notices, the moderation log and statements of reasons (`moderation_records`).
    pub moderation: Keep,
    /// Chatters' IP addresses, with the access logs (`access_logs`).
    pub addresses: Keep,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            messages: Keep::None,
            moderation: Keep::None,
            addresses: Keep::None,
        }
    }
}

impl Retention {
    /// The chat's periods from the station's setting. "With the archive" keeps messages as
    /// long as the programme recordings (the archive itself arrives in M8); 0 days is No
    /// retention.
    pub fn from_periods(p: &weavesignal_model::retention::Periods) -> Self {
        use weavesignal_model::retention::Period;
        let keep = |period: Period| match period {
            Period::Days { days: 0 } | Period::WithCall => Keep::None,
            Period::Days { days } => Keep::Days(days),
            Period::WithArchive => match p.programme_recordings {
                Period::Days { days: 0 } => Keep::None,
                Period::Days { days } => Keep::Days(days),
                _ => Keep::Forever,
            },
            Period::AfterRecordings { .. } | Period::NoLimit => Keep::Forever,
        };
        Retention {
            messages: keep(p.chat_messages),
            moderation: keep(p.moderation_records),
            addresses: keep(p.access_logs),
        }
    }
}

/// A retention period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// No retention: messages go when the scheduled episode they were written in ends, or
    /// [`NO_RETENTION_MESSAGES`] after they were written where nothing is scheduled (ADR-0149);
    /// moderation records go when the review window a statement promises has closed; addresses
    /// are never stored.
    None,
    Days(u32),
    /// No end the chat applies.
    Forever,
}

/// Under No retention, how long a message lives when nothing is scheduled where it was
/// written: a long show's length. A message written during a scheduled episode goes when the
/// episode ends instead.
pub const NO_RETENTION_MESSAGES: Duration = Duration::from_secs(6 * 3600);
/// How long a chatter has to ask for a review of a decision (the statement says so).
pub const REVIEW_DAYS: u32 = 14;

impl Keep {
    pub fn messages(self) -> Duration {
        match self {
            Keep::None => NO_RETENTION_MESSAGES,
            Keep::Days(d) => days(d),
            Keep::Forever => Duration::MAX,
        }
    }
    pub fn moderation(self) -> Duration {
        match self {
            Keep::None => days(REVIEW_DAYS),
            Keep::Days(d) => days(d.max(REVIEW_DAYS)),
            Keep::Forever => Duration::MAX,
        }
    }
    /// `None`: do not store addresses at all.
    pub fn addresses(self) -> Option<Duration> {
        match self {
            Keep::None => None,
            Keep::Days(d) => Some(days(d)),
            Keep::Forever => Some(Duration::MAX),
        }
    }
    /// "12 months", "30 days".
    pub fn words(self) -> String {
        match self {
            Keep::None => "not kept".into(),
            Keep::Forever => "with no end set".into(),
            Keep::Days(d) if d % 365 == 0 => {
                let y = d / 365;
                if y == 1 {
                    "12 months".into()
                } else {
                    format!("{y} years")
                }
            }
            Keep::Days(d) if d % 30 == 0 && d >= 60 => format!("{} months", d / 30),
            Keep::Days(1) => "1 day".into(),
            Keep::Days(d) => format!("{d} days"),
        }
    }
}

fn days(d: u32) -> Duration {
    Duration::from_secs(u64::from(d) * 86_400)
}

/// The server's own limits.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// The most sockets at once; more are refused with 503 so a flood cannot exhaust the box.
    pub max_sockets: usize,
    /// Frames queued for one socket before it is closed as too slow (it recovers on reconnect).
    pub socket_queue: usize,
    /// Public events kept for recovery after a reconnect.
    pub recovery_events: usize,
    /// Messages a fresh page gets.
    pub history: usize,
    /// A session's life, in days.
    pub session_days: u32,
    /// Sign-in codes one address may ask for in ten minutes, and one email address.
    pub codes_per_address: u32,
    pub codes_per_email: u32,
    /// Sockets one address may hold open at once (ADR-0120). An IPv6 address counts by its
    /// /64. Generous, as a carrier's NAT puts many listeners behind one address; 0 is off.
    pub sockets_per_address: usize,
    /// Notices one address may send in ten minutes (0 is off).
    pub notices_per_address: u32,
    /// Guest accounts one address may make in ten minutes (0 is off).
    pub guests_per_address: u32,
    /// Sign-in codes one address may try in ten minutes, right or wrong (0 is off).
    pub signin_tries_per_address: u32,
    /// Reactions one chatter may send in ten seconds (0 is off).
    pub reactions_per_chatter: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sockets: 10_000,
            socket_queue: 512,
            recovery_events: 2_000,
            history: 50,
            session_days: 30,
            codes_per_address: 10,
            codes_per_email: 3,
            sockets_per_address: 50,
            notices_per_address: 10,
            guests_per_address: 10,
            signin_tries_per_address: 20,
            reactions_per_chatter: 10,
        }
    }
}

/// What the station sets for its chat, from the studio (kept in `chat_settings`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct StationSettings {
    /// The station's chat terms, as it writes them.
    pub terms: String,
    /// Who answers for the chat (the UK Act's named person), as the station names them.
    pub accountable_person: String,
    /// Where people reach the station about the chat (DSA Article 11/12 contact point).
    pub contact: String,
    /// The age a chatter declares they are at least (ADR-0032 item 6), when the station sets
    /// its own; `None` follows the station's jurisdiction (ADR-0148: the retention preset's
    /// age). A stored 13, the only default before ADR-0148, reads as following it: every
    /// preset's age is 13 or more, so a station that never chose gets its jurisdiction's.
    #[serde(deserialize_with = "jurisdiction_or_own")]
    pub min_age: Option<u8>,
    /// Comments and reactions only (ADR-0035); false is full chat, the default.
    pub comments_only: bool,
    /// Guests may post, every message held (ADR-0034).
    pub guests: bool,
    pub blocked_words: Vec<String>,
    pub forbidden_names: Vec<String>,
}

/// `min_age` as stored: absent, null or 13 follow the jurisdiction; any other number is the
/// station's own.
fn jurisdiction_or_own<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
    Ok(Option::<u8>::deserialize(d)?.filter(|a| *a != 13))
}

impl StationSettings {
    /// The first 16 hex digits of the terms' SHA-256: a chatter who agreed to older terms is
    /// asked again.
    pub fn terms_version(&self) -> String {
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(self.terms.trim().as_bytes());
        d.iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.min_age.is_some_and(|a| !(13..=21).contains(&a)) {
            return Err("the minimum age is 13 to 21".into());
        }
        if self.terms.len() > 20_000 {
            return Err("the terms are longer than 20,000 characters".into());
        }
        if self.accountable_person.chars().count() > 200 || self.contact.chars().count() > 300 {
            return Err(
                "the accountable person is at most 200 characters and the contact 300".into(),
            );
        }
        if self.blocked_words.len() > 2_000 || self.forbidden_names.len() > 2_000 {
            return Err("at most 2,000 blocked words and 2,000 forbidden names".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    /// `json` (the default) or `pretty`.
    pub format: String,
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

impl Config {
    pub fn from_toml(text: &str, path: &str) -> Result<Self, ConfigError> {
        let c: Config = toml::from_str(text).map_err(|e| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(e),
        })?;
        c.validate()?;
        Ok(c)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: shown.clone(),
            source,
        })?;
        let mut c = Self::from_toml(&text, &shown)?;
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        c.database_url = env(DATABASE_URL_ENV);
        c.admin_token = env(ADMIN_TOKEN_ENV);
        // A key that is set but too short is refused here; one that is missing is refused when
        // the service starts, with the same words, so a test can build a config without one.
        c.email_key = env(EmailKey::ENV)
            .map(|k| EmailKey::parse(&k))
            .transpose()
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.station()?;
        self.defaults.validate().map_err(ConfigError::Invalid)?;
        let l = &self.limits;
        if !(1..=100_000).contains(&l.max_sockets)
            || !(16..=65_536).contains(&l.socket_queue)
            || !(100..=100_000).contains(&l.recovery_events)
            || !(0..=500).contains(&l.history)
            || !(1..=365).contains(&l.session_days)
        {
            return Err(ConfigError::Invalid(
                "`limits`: max_sockets 1-100000, socket_queue 16-65536, recovery_events 100-100000, history 0-500, session_days 1-365".into(),
            ));
        }
        if self.mail.mode == MailMode::Command && self.mail.command.is_empty() {
            return Err(ConfigError::Invalid(
                "`mail.command` is empty: name a sendmail-compatible program".into(),
            ));
        }
        Ok(())
    }

    pub fn station(&self) -> Result<Station, ConfigError> {
        Station::new(self.station.id, &self.station.name)
            .map_err(|e| ConfigError::Invalid(e.to_string()))
    }
}

/// The documented sample settings.
pub const SAMPLE: &str = r#"# WeaveSignal's chat service (M6). Secrets come from the environment:
# WEAVESIGNAL_DATABASE_URL (the same database as the control plane) and
# WEAVESIGNAL_CONTROL_ADMIN_TOKEN (the studio app's bearer, the same as the control plane's) and
# WEAVESIGNAL_CHAT_EMAIL_KEY (at least 32 characters, e.g. `openssl rand -hex 32`: it keys the hash
# of chatters' email addresses, ADR-0125, and the chat will not start without it. Keep it with the
# database's backups: a new key finds none of the existing accounts).

[station]
# The same identifier and name as the control plane's [station].
id = "00000000-0000-0000-0000-000000000000"
name = "Kettle FM"

[listen]
address = "127.0.0.1"
port = 7311            # ports.chat; the proxy sends /api/v1/chat/ here, websockets included
forwarded_for = false  # true only behind the station's own proxy

[control]
url = "http://127.0.0.1:7310"   # the room's modes, the show and the listener delay come from here

[mail]
mode = "off"           # "command" (a sendmail-compatible program), "outbox" (files), or "off"
from = "chat@station.example"
command = ["/usr/sbin/sendmail", "-t", "-i"]
outbox_dir = "/var/lib/weavesignal/chat-outbox"

[defaults]
# The station's chat settings until they are edited from the studio.
# min_age = 16          # leave it out to follow the station's jurisdiction (ADR-0148)
comments_only = false   # true: comments and reactions only (ADR-0035)
guests = false          # true: guests may post, every message held (ADR-0034)
"#;

#[cfg(test)]
mod tests {

    #[test]
    fn a_stored_13_follows_the_jurisdiction_and_another_age_is_the_stations_own() {
        let read = |j: &str| serde_json::from_str::<StationSettings>(j).unwrap().min_age;
        assert_eq!(read("{}"), None);
        assert_eq!(read(r#"{"min_age": null}"#), None);
        assert_eq!(read(r#"{"min_age": 13}"#), None);
        assert_eq!(read(r#"{"min_age": 16}"#), Some(16));
        let mut s = StationSettings::default();
        assert_eq!(s.min_age, None);
        s.min_age = Some(25);
        assert!(s.validate().is_err());
        s.min_age = Some(21);
        assert!(s.validate().is_ok());
    }

    use super::*;

    #[test]
    fn the_sample_parses_with_no_retention_by_default() {
        let c = Config::from_toml(SAMPLE, "sample").unwrap();
        assert_eq!(c.listen.port, 7311);
        assert_eq!(c.mail.mode, MailMode::Off);
        assert!(!c.defaults.comments_only && !c.defaults.guests);
    }

    #[test]
    fn retention_follows_the_jurisdiction_preset() {
        use weavesignal_model::retention::{Period, preset};
        let none = Retention::from_periods(&preset("none").unwrap().periods());
        assert_eq!(none, Retention::default());
        assert_eq!(Keep::None.addresses(), None);
        assert_eq!(Keep::None.moderation(), Duration::from_secs(14 * 86_400));
        // A period shorter than the review window still keeps a decision until it closes.
        assert_eq!(Keep::Days(7).moderation(), Duration::from_secs(14 * 86_400));
        assert_eq!(Keep::Days(365).words(), "12 months");
        let mut p = preset("none").unwrap().periods();
        p.chat_messages = Period::WithArchive;
        p.programme_recordings = Period::days(42);
        p.moderation_records = Period::days(365);
        p.access_logs = Period::days(30);
        let r = Retention::from_periods(&p);
        assert_eq!(r.messages, Keep::Days(42));
        assert_eq!(r.moderation, Keep::Days(365));
        assert_eq!(
            r.addresses.addresses(),
            Some(Duration::from_secs(30 * 86_400))
        );
    }

    #[test]
    fn the_terms_version_follows_the_words() {
        let mut s = StationSettings::default();
        let empty = s.terms_version();
        s.terms = "Be kind to the callers.".into();
        assert_ne!(s.terms_version(), empty);
        assert_eq!(s.terms_version().len(), 16);
    }
}
