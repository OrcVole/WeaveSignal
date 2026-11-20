// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! `[phone]`: the telephone line's settings (Phase 17f). **Everything is off by default**, and
//! the line cannot be turned on until a trunk is set: the station's number, the provider's
//! addresses, and the station's public address for the media (`nat_1_to_1_ip`; ADR-0290 forbids
//! asking an outside STUN server for it). Every port is a setting (ADR-0075). The trunk's
//! password is never in this file: it comes from [`TRUNK_PASSWORD_ENV`] (a secret file on Compose,
//! a Podman secret on Quadlets, a write-only admin setting on Cloudron).
//!
//! **RTP.** LiveKit's SIP service takes one even port per call from its range (10000 to 20000 by
//! default; `pkg/config/config.go`, read 2026-11-20). The station gives it a small range of its
//! own, two ports per line, starting on an even port, so 8 lines are 16 ports: clear of Cloudron's
//! own TURN (3478, 5349, 50000 to 51000) and of Linux's ephemeral range.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::ConfigError;

/// Where the trunk's digest password comes from: the environment, never the file.
pub const TRUNK_PASSWORD_ENV: &str = "WEAVESIGNAL_PHONE_TRUNK_PASSWORD";
/// The most lines at once a station may set.
pub const MAX_LINES: u16 = 16;
/// Ports Cloudron's own TURN reportedly holds (AGENTS.md's trap; not measured): a phone port may
/// not be among them.
const CLOUDRON_HELD: &[(u16, u16)] = &[(3478, 3478), (5349, 5349), (50000, 51000)];
/// Linux's ephemeral range: a long-lived listener there can clash with an outgoing connection.
const EPHEMERAL: (u16, u16) = (32768, 60999);

/// The help text for [`PhoneConfig::consent_words`]: the setting's description in Settings and
/// the docs (Fable turns it into an inline tooltip in 17j). Plain words; not legal advice.
pub const CONSENT_WORDS_HELP: &str = "Before a phone caller joins the queue, play them a few words: who they have reached, that the call may go on air and be recorded, and to press a key to carry on. Your own recording plays if you have made one, otherwise a short one that comes with WeaveSignal. Leave this on if you broadcast on FM, AM or DAB (a broadcaster's code usually asks you to tell callers the call is recorded for broadcast), if you record or keep calls (the archive, a podcast, clips), or if your callers may not know they can be heard on air. An internet-only station may turn it off if it never records or keeps calls and the number is only ever given out on air as a live phone-in, so that calling it is itself agreeing to be on air. With it off, callers go straight into the queue and the station's records say no words were played. This is not legal advice: the rules differ from country to country, so if you are unsure, leave it on.";

/// The telephone line.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct PhoneConfig {
    /// Take phone calls. Off by default; refused while no trunk is set ([`Self::trunk_set`]).
    pub enabled: bool,
    /// The station's number or numbers as the provider sends them in the INVITE's To user.
    pub numbers: Vec<String>,
    /// The provider's addresses (IPs or CIDRs) that may send calls; the trunk accepts no other.
    pub allowed_addresses: Vec<String>,
    /// The digest user the provider authenticates with, if it offers digest auth (the password
    /// from [`TRUNK_PASSWORD_ENV`]).
    pub auth_username: String,
    /// The station's public IP address, given to LiveKit's SIP service as `nat_1_to_1_ip`.
    pub public_ip: String,
    /// The station's calling code without `+` (`44`): how a typed number's forms are made for a
    /// ban or a takedown. Empty, only the form typed and its international forms.
    pub country_code: String,
    /// Calls at once (4, 8 or 16 offered; 1 to [`MAX_LINES`]). Sets the RTP range's size.
    pub lines: u16,
    /// SIP signalling, UDP and TCP.
    pub sip_port: u16,
    /// SIP over TLS; 0 is off.
    pub sip_tls_port: u16,
    /// The first port of the RTP range, even; the range is `2 × lines` ports.
    pub rtp_port: u16,
    /// Play the consent words before a caller joins the queue ([`CONSENT_WORDS_HELP`]). On by
    /// default when the line is on.
    pub consent_words: bool,
    /// Which shipped recording plays until the station records its own: `alba` (the default),
    /// `edinburgh`, or `own` (none; [`super::words::DEFAULT_VOICE_HELP`]).
    pub default_voice: super::words::PhoneVoice,
    /// Where the image keeps the shipped recordings; copied into `words_dir` at start.
    pub defaults_dir: PathBuf,
    /// How long a caller has to press a key, from the first word (seconds).
    pub words_timeout_s: u32,
    /// How many times the words play before the call ends without a key.
    pub words_plays: u8,
    /// Calls one number (one ban key) may place in an hour. Withheld callers share one count.
    pub calls_per_number_per_hour: u32,
    /// The longest a call may last (seconds); the trunk's `max_call_duration`.
    pub max_call_s: u32,
    /// How long an INVITE may ring before it is given up (seconds); the trunk's
    /// `ringing_timeout`.
    pub ringing_timeout_s: u32,
    /// Where the phone words' recordings are (the desk's own setting names the same folder):
    /// the shipped default and the station's own.
    pub words_dir: PathBuf,
}

impl Default for PhoneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            numbers: Vec::new(),
            allowed_addresses: Vec::new(),
            auth_username: String::new(),
            public_ip: String::new(),
            country_code: String::new(),
            lines: 8,
            sip_port: 5060,
            sip_tls_port: 0,
            rtp_port: 7340,
            consent_words: true,
            default_voice: super::words::PhoneVoice::Alba,
            defaults_dir: PathBuf::from("/opt/weavesignal/phone-words"),
            words_timeout_s: 90,
            words_plays: 2,
            calls_per_number_per_hour: 6,
            max_call_s: 2 * 3600,
            ringing_timeout_s: 30,
            words_dir: PathBuf::from("/var/lib/weavesignal/phone-words"),
        }
    }
}

fn invalid(msg: String) -> ConfigError {
    ConfigError::Invalid(msg)
}

impl PhoneConfig {
    /// Whether a trunk is set: a number, and a way to tell the provider (its addresses or a
    /// digest user), and the station's public address.
    pub fn trunk_set(&self) -> bool {
        !self.numbers.iter().all(|n| n.trim().is_empty())
            && (!self.allowed_addresses.is_empty() || !self.auth_username.trim().is_empty())
            && !self.public_ip.trim().is_empty()
    }

    /// Whether the line takes calls: on, and a trunk set.
    pub fn on(&self) -> bool {
        self.enabled && self.trunk_set()
    }

    /// The RTP range, first and last port.
    pub fn rtp_range(&self) -> (u16, u16) {
        let count = 2 * self.lines;
        (
            self.rtp_port,
            self.rtp_port.saturating_add(count.saturating_sub(1)),
        )
    }

    /// Every port the line listens on, with its setting's name, as `(name, first, last)`.
    pub fn ports(&self) -> Vec<(&'static str, u16, u16)> {
        let (a, b) = self.rtp_range();
        let mut p = vec![
            ("phone.sip_port", self.sip_port, self.sip_port),
            ("phone.rtp_port", a, b),
        ];
        if self.sip_tls_port != 0 {
            p.push(("phone.sip_tls_port", self.sip_tls_port, self.sip_tls_port));
        }
        p
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(1..=MAX_LINES).contains(&self.lines) {
            return Err(invalid(format!(
                "`phone.lines` is {}; 1 to {MAX_LINES} calls at once",
                self.lines
            )));
        }
        if !self.rtp_port.is_multiple_of(2) {
            return Err(invalid(format!(
                "`phone.rtp_port` is {}; LiveKit's SIP service uses even ports, so the range starts on one",
                self.rtp_port
            )));
        }
        if u32::from(self.rtp_port) + 2 * u32::from(self.lines) - 1 > u32::from(u16::MAX) {
            return Err(invalid(
                "`phone.rtp_port` leaves no room for the range".into(),
            ));
        }
        let ports = self.ports();
        for &(name, a, b) in &ports {
            if a < 1024 {
                return Err(invalid(format!("`{name}` is {a}; use a port from 1024 up")));
            }
            for &(ha, hb) in CLOUDRON_HELD {
                if a <= hb && ha <= b {
                    return Err(invalid(format!(
                        "`{name}` ({a}-{b}) overlaps {ha}-{hb}, which Cloudron's own TURN holds"
                    )));
                }
            }
            if a <= EPHEMERAL.1 && EPHEMERAL.0 <= b {
                return Err(invalid(format!(
                    "`{name}` ({a}-{b}) is inside Linux's ephemeral range {}-{}; choose ports below {}",
                    EPHEMERAL.0, EPHEMERAL.1, EPHEMERAL.0
                )));
            }
        }
        for (i, &(n1, a1, b1)) in ports.iter().enumerate() {
            for &(n2, a2, b2) in &ports[i + 1..] {
                if a1 <= b2 && a2 <= b1 {
                    return Err(invalid(format!("`{n1}` and `{n2}` overlap")));
                }
            }
        }
        if self.words_plays == 0 || self.words_plays > 5 {
            return Err(invalid(format!(
                "`phone.words_plays` is {}; 1 to 5",
                self.words_plays
            )));
        }
        if !(10..=600).contains(&self.words_timeout_s) {
            return Err(invalid(format!(
                "`phone.words_timeout_s` is {}; 10 to 600 seconds",
                self.words_timeout_s
            )));
        }
        if !(60..=12 * 3600).contains(&self.max_call_s) {
            return Err(invalid(format!(
                "`phone.max_call_s` is {}; 60 seconds to 12 hours",
                self.max_call_s
            )));
        }
        if !(5..=120).contains(&self.ringing_timeout_s) {
            return Err(invalid(format!(
                "`phone.ringing_timeout_s` is {}; 5 to 120 seconds",
                self.ringing_timeout_s
            )));
        }
        if self.calls_per_number_per_hour == 0 {
            return Err(invalid(
                "`phone.calls_per_number_per_hour` is 0; at least 1".into(),
            ));
        }
        if !self.country_code.chars().all(|c| c.is_ascii_digit()) || self.country_code.len() > 3 {
            return Err(invalid(format!(
                "`phone.country_code` is {:?}; one to three digits without `+` (44), or empty",
                self.country_code
            )));
        }
        let ip = self.public_ip.trim();
        if !ip.is_empty() {
            match ip.parse::<IpAddr>() {
                Ok(a) if a.is_loopback() || a.is_unspecified() => {
                    // Loopback is for a local test only; the line still starts.
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(invalid(format!(
                        "`phone.public_ip` is {ip:?}; the station's public IP address (not a name: no STUN, ADR-0290)"
                    )));
                }
            }
        }
        for a in &self.allowed_addresses {
            let (addr, bits) = a.split_once('/').unwrap_or((a.as_str(), ""));
            let ok = addr.trim().parse::<IpAddr>().is_ok_and(|ip| {
                bits.is_empty()
                    || bits
                        .parse::<u8>()
                        .is_ok_and(|b| b <= if ip.is_ipv4() { 32 } else { 128 })
            });
            if !ok {
                return Err(invalid(format!(
                    "`phone.allowed_addresses` has {a:?}; an IP address or a CIDR (203.0.113.0/24)"
                )));
            }
        }
        if self.enabled && !self.trunk_set() {
            return Err(invalid(
                "`phone.enabled` is on but no trunk is set: `phone.numbers`, `phone.allowed_addresses` or `phone.auth_username`, and `phone.public_ip`"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trunk() -> PhoneConfig {
        PhoneConfig {
            enabled: true,
            numbers: vec!["+441314960000".into()],
            allowed_addresses: vec!["203.0.113.0/24".into()],
            public_ip: "198.51.100.7".into(),
            ..PhoneConfig::default()
        }
    }

    #[test]
    fn everything_is_off_by_default_and_valid() {
        let c = PhoneConfig::default();
        assert!(!c.enabled);
        assert!(!c.on());
        assert!(c.consent_words);
        assert_eq!(c.lines, 8);
        assert_eq!(c.rtp_range(), (7340, 7355));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn the_line_cannot_be_turned_on_without_a_trunk() {
        let c = PhoneConfig {
            enabled: true,
            ..PhoneConfig::default()
        };
        assert!(!c.on());
        assert!(c.validate().is_err());
        let c = trunk();
        assert!(c.on());
        assert!(c.validate().is_ok());
        // Digest auth instead of addresses is a trunk too; a public address is still needed.
        let c = PhoneConfig {
            allowed_addresses: vec![],
            auth_username: "kettle".into(),
            ..trunk()
        };
        assert!(c.validate().is_ok());
        let c = PhoneConfig {
            public_ip: String::new(),
            ..trunk()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn the_rtp_range_follows_the_lines() {
        for (lines, last) in [(4, 7347), (8, 7355), (16, 7371)] {
            let c = PhoneConfig { lines, ..trunk() };
            assert_eq!(c.rtp_range(), (7340, last));
            assert!(c.validate().is_ok());
        }
        for lines in [0, 17] {
            assert!(PhoneConfig { lines, ..trunk() }.validate().is_err());
        }
    }

    #[test]
    fn bad_ports_are_refused() {
        for c in [
            PhoneConfig {
                rtp_port: 7341,
                ..trunk()
            },
            PhoneConfig {
                rtp_port: 3470,
                ..trunk()
            },
            PhoneConfig {
                sip_port: 5349,
                ..trunk()
            },
            PhoneConfig {
                rtp_port: 50000,
                ..trunk()
            },
            PhoneConfig {
                rtp_port: 40000,
                ..trunk()
            },
            PhoneConfig {
                sip_port: 7344,
                ..trunk()
            },
            PhoneConfig {
                sip_tls_port: 5060,
                ..trunk()
            },
            PhoneConfig {
                sip_port: 80,
                ..trunk()
            },
        ] {
            assert!(c.validate().is_err(), "{:?}", c.ports());
        }
        let c = PhoneConfig {
            sip_tls_port: 5061,
            ..trunk()
        };
        assert!(c.validate().is_ok());
        assert_eq!(c.ports().len(), 3);
    }

    #[test]
    fn addresses_and_codes_are_checked() {
        for c in [
            PhoneConfig {
                public_ip: "sip.example.org".into(),
                ..trunk()
            },
            PhoneConfig {
                allowed_addresses: vec!["203.0.113.0/33".into()],
                ..trunk()
            },
            PhoneConfig {
                allowed_addresses: vec!["provider".into()],
                ..trunk()
            },
            PhoneConfig {
                country_code: "+44".into(),
                ..trunk()
            },
        ] {
            assert!(c.validate().is_err());
        }
        let c = PhoneConfig {
            allowed_addresses: vec!["2001:db8::/32".into(), "198.51.100.9".into()],
            country_code: "44".into(),
            ..trunk()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn a_settings_file_reads_the_section() {
        let c: PhoneConfig = toml::from_str(
            r#"
            enabled = false
            lines = 4
            consent_words = false
            default_voice = "edinburgh"
            "#,
        )
        .unwrap();
        assert_eq!(c.default_voice, super::super::words::PhoneVoice::Edinburgh);
        assert_eq!(
            PhoneConfig::default().default_voice,
            super::super::words::PhoneVoice::Alba
        );
        assert_eq!(c.lines, 4);
        assert!(!c.consent_words);
        assert!(toml::from_str::<PhoneConfig>("password = \"x\"").is_err());
    }

    #[test]
    fn the_help_says_when_it_is_and_is_not_needed() {
        assert!(CONSENT_WORDS_HELP.contains("Leave this on if you broadcast"));
        assert!(CONSENT_WORDS_HELP.contains("An internet-only station may turn it off if"));
        assert!(CONSENT_WORDS_HELP.contains("not legal advice"));
    }
}
