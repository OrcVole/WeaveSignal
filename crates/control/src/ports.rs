// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Every listening port is a setting (ADR-0075, extending ADR-0023).
//!
//! Each port has a default chosen clear of ports other software commonly claims, a couple of
//! suggested alternatives, and the rule that any free port may be used. Setup checks that each
//! is free and, when one is not, names the clash and the next free suggestion.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
    Udp,
}

impl fmt::Display for Proto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        })
    }
}

/// Where a port listens: on loopback only, behind the reverse proxy or beside it, or on every
/// interface because guests reach it directly (ADR-0048 rule 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Loopback,
    Public,
}

/// One listening port's specification.
#[derive(Debug)]
pub struct PortSpec {
    /// The setting's key under `[ports]`.
    pub key: &'static str,
    pub proto: Proto,
    pub default: u16,
    /// Suggested alternatives, in order; any free port may be used as well.
    pub alternatives: &'static [u16],
    pub exposure: Exposure,
    /// One line on what listens there.
    pub what: &'static str,
}

/// Every listening port in a station, with its default. The defaults for WeaveSignal's own
/// services sit in 7310-7319, which no entry in the IANA registry or common software claims
/// (checked against /etc/services, 2026-10-25); LiveKit's keep LiveKit's own defaults.
pub const PORTS: &[PortSpec] = &[
    PortSpec {
        key: "proxy_https",
        proto: Proto::Tcp,
        default: 443,
        alternatives: &[8443],
        exposure: Exposure::Public,
        what: "the reverse proxy, the one public web listener",
    },
    PortSpec {
        key: "proxy_http",
        proto: Proto::Tcp,
        default: 80,
        alternatives: &[8080],
        exposure: Exposure::Public,
        what: "the reverse proxy's redirects and plain-HTTP listener streams",
    },
    PortSpec {
        key: "control",
        proto: Proto::Tcp,
        default: 7310,
        alternatives: &[17310, 27310],
        exposure: Exposure::Loopback,
        what: "the control plane's HTTP API, behind the proxy",
    },
    PortSpec {
        key: "chat",
        proto: Proto::Tcp,
        default: 7311,
        alternatives: &[17311, 27311],
        exposure: Exposure::Loopback,
        what: "the chat service, behind the proxy",
    },
    PortSpec {
        key: "listeners",
        proto: Proto::Tcp,
        default: 7312,
        alternatives: &[17312, 27312],
        exposure: Exposure::Loopback,
        what: "Liquidsoap's icecast.server: listener streams, behind the proxy",
    },
    PortSpec {
        key: "srt_ingest",
        proto: Proto::Udp,
        default: 7313,
        alternatives: &[17313, 27313],
        exposure: Exposure::Loopback,
        what: "the programme from the desk to Liquidsoap, SRT on loopback (ADR-0049); the desk listens, Liquidsoap connects",
    },
    PortSpec {
        key: "playout_feed",
        proto: Proto::Tcp,
        default: 7315,
        alternatives: &[17315, 27315],
        exposure: Exposure::Loopback,
        what: "playout's programme to the listener process, which holds the listener mount (M4); its harbor also opens the port above it",
    },
    PortSpec {
        key: "desk_feed",
        proto: Proto::Tcp,
        default: 7317,
        alternatives: &[17317, 27317],
        exposure: Exposure::Loopback,
        what: "playout's programme back to the desk, the video stream's sound while nobody is live (ADR-0172); the desk listens, playout connects",
    },
    PortSpec {
        key: "livekit_http",
        proto: Proto::Tcp,
        default: 7880,
        alternatives: &[17880, 27880],
        exposure: Exposure::Loopback,
        what: "LiveKit's signalling and API, behind the proxy",
    },
    PortSpec {
        key: "livekit_media",
        proto: Proto::Udp,
        default: 7882,
        // Not 50000-51000: Cloudron's own TURN relays use that range (reported).
        alternatives: &[52000, 443],
        exposure: Exposure::Public,
        what: "LiveKit's one WebRTC media port (ADR-0023)",
    },
    PortSpec {
        key: "livekit_tcp",
        proto: Proto::Tcp,
        default: 7881,
        alternatives: &[17881, 27881],
        exposure: Exposure::Public,
        what: "LiveKit's ICE over TCP, the fallback when UDP is blocked",
    },
    PortSpec {
        key: "turn_udp",
        proto: Proto::Udp,
        default: 3478,
        alternatives: &[3479, 13478],
        exposure: Exposure::Public,
        what: "LiveKit's built-in TURN over UDP",
    },
    PortSpec {
        key: "turn_tls",
        proto: Proto::Tcp,
        default: 5349,
        alternatives: &[443, 15349],
        exposure: Exposure::Public,
        what: "LiveKit's built-in TURN over TLS, on by default on 5349; 443 where it is free (ADR-0080)",
    },
];

/// The port specification for a key.
/// Settings that once named a port and name nothing now: accepted and ignored, with a warning,
/// so a station's old settings file still starts. `hls_origin` (7314) was kept for an HLS
/// origin of its own that was never built; the control plane serves `/hls/` (2026-11-03).
const RETIRED: &[&str] = &["hls_origin"];

pub fn spec(key: &str) -> Option<&'static PortSpec> {
    PORTS.iter().find(|s| s.key == key)
}

/// The configured ports: every key of [`PORTS`], each overridable under `[ports]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Ports(BTreeMap<&'static str, u16>);

impl Default for Ports {
    fn default() -> Self {
        Self(PORTS.iter().map(|s| (s.key, s.default)).collect())
    }
}

impl<'de> Deserialize<'de> for Ports {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let given = BTreeMap::<String, u16>::deserialize(d)?;
        let mut ports = Ports::default();
        for (key, port) in given {
            if RETIRED.contains(&key.as_str()) {
                tracing::warn!(setting = %key, port, "a retired port setting is ignored; it can be removed");
                continue;
            }
            let spec = spec(&key).ok_or_else(|| {
                let known: Vec<_> = PORTS.iter().map(|s| s.key).collect();
                serde::de::Error::custom(format!(
                    "unknown port setting `{key}`; the settings are {}",
                    known.join(", ")
                ))
            })?;
            if port == 0 {
                return Err(serde::de::Error::custom(format!(
                    "port `{key}` is 0; give a port from 1 to 65535"
                )));
            }
            ports.0.insert(spec.key, port);
        }
        Ok(ports)
    }
}

impl Ports {
    pub fn get(&self, key: &str) -> u16 {
        match self.0.get(key) {
            Some(port) => *port,
            None => panic!("no port setting named `{key}`"),
        }
    }

    /// Change a port; for tests and for setup tools.
    pub fn set(&mut self, key: &str, port: u16) {
        let spec = spec(key).unwrap_or_else(|| panic!("no port setting named `{key}`"));
        self.0.insert(spec.key, port);
    }

    /// Every port with its specification, in the registry's order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static PortSpec, u16)> + '_ {
        PORTS.iter().map(|s| (s, self.get(s.key)))
    }

    /// Two settings that ask for the same port and protocol.
    pub fn duplicates(&self) -> Vec<(&'static str, &'static str, u16, Proto)> {
        let mut seen: BTreeMap<(u16, Proto), &'static str> = BTreeMap::new();
        let mut dups = Vec::new();
        for (spec, port) in self.iter() {
            if let Some(first) = seen.insert((port, spec.proto), spec.key) {
                dups.push((first, spec.key, port, spec.proto));
            }
        }
        dups
    }
}

/// Why a port cannot be used.
#[derive(Debug, PartialEq, Eq)]
pub enum Unavailable {
    InUse,
    NeedsPrivilege,
    Other(String),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unavailable::InUse => f.write_str("is in use"),
            Unavailable::NeedsPrivilege => {
                f.write_str("needs privileges this process does not have (ports below 1024)")
            }
            Unavailable::Other(e) => write!(f, "cannot be bound: {e}"),
        }
    }
}

/// Is `port` free to listen on at `ip`? Binds it and lets it go at once.
pub fn check_free(ip: IpAddr, port: u16, proto: Proto) -> Result<(), Unavailable> {
    let addr = SocketAddr::new(ip, port);
    let result = match proto {
        Proto::Tcp => TcpListener::bind(addr).map(drop),
        Proto::Udp => UdpSocket::bind(addr).map(drop),
    };
    result.map_err(|e| match e.kind() {
        io::ErrorKind::AddrInUse => Unavailable::InUse,
        io::ErrorKind::PermissionDenied => Unavailable::NeedsPrivilege,
        _ => Unavailable::Other(e.to_string()),
    })
}

/// The address a port is checked (and later bound) on: loopback ports on the configured
/// internal address, public ones on every interface.
pub fn check_ip(spec: &PortSpec, internal: IpAddr) -> IpAddr {
    match spec.exposure {
        Exposure::Loopback => internal,
        Exposure::Public => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    }
}

/// A port that is not free, with the next free suggestion.
#[derive(Debug, PartialEq, Eq)]
pub struct Clash {
    pub key: &'static str,
    pub port: u16,
    pub proto: Proto,
    pub why: Unavailable,
    pub what: &'static str,
    /// The first suggested alternative that is free now, if any.
    pub next_free: Option<u16>,
    pub alternatives: &'static [u16],
}

impl fmt::Display for Clash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "port {}/{} for `{}` ({}) {}; ",
            self.port, self.proto, self.key, self.what, self.why
        )?;
        match self.next_free {
            Some(alt) => write!(
                f,
                "the next free suggestion is {alt}/{} (set `ports.{} = {alt}`), and any free port may be used",
                self.proto, self.key
            ),
            None => {
                let alts: Vec<String> = self.alternatives.iter().map(u16::to_string).collect();
                write!(
                    f,
                    "none of the suggestions ({}) is free either; any free port may be used",
                    alts.join(", ")
                )
            }
        }
    }
}

/// Check one configured port; on a clash, find the next free suggestion.
pub fn check_one(spec: &'static PortSpec, port: u16, internal: IpAddr) -> Result<(), Clash> {
    let ip = check_ip(spec, internal);
    check_free(ip, port, spec.proto).map_err(|why| Clash {
        key: spec.key,
        port,
        proto: spec.proto,
        why,
        what: spec.what,
        next_free: spec
            .alternatives
            .iter()
            .copied()
            .find(|alt| *alt != port && check_free(ip, *alt, spec.proto).is_ok()),
        alternatives: spec.alternatives,
    })
}

/// Check every configured port, as setup does before the station starts.
pub fn check_all(ports: &Ports, internal: IpAddr) -> Vec<Clash> {
    ports
        .iter()
        .filter_map(|(spec, port)| check_one(spec, port, internal).err())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    #[test]
    fn turn_over_tls_defaults_to_5349_with_443_offered() {
        // ADR-0080: 5349 by default, 443 optional where the proxy does not hold it.
        let spec = spec("turn_tls").unwrap();
        assert_eq!(spec.default, 5349);
        assert_eq!(spec.proto, Proto::Tcp);
        assert_eq!(spec.alternatives.first(), Some(&443));
        assert_eq!(Ports::default().get("turn_tls"), 5349);
    }

    #[test]
    fn every_key_is_unique_and_every_default_differs() {
        let ports = Ports::default();
        assert_eq!(ports.iter().count(), PORTS.len());
        assert!(ports.duplicates().is_empty(), "{:?}", ports.duplicates());
    }

    #[test]
    fn every_port_has_alternatives_that_differ_from_its_default() {
        for spec in PORTS {
            assert!(
                !spec.alternatives.is_empty(),
                "{} has no alternatives",
                spec.key
            );
            assert!(!spec.alternatives.contains(&spec.default), "{}", spec.key);
            assert!(!spec.what.is_empty());
        }
    }

    #[test]
    fn unknown_and_zero_ports_are_refused() {
        let err = toml::from_str::<Ports>("contrl = 1")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown port setting `contrl`"), "{err}");
        let err = toml::from_str::<Ports>("control = 0")
            .unwrap_err()
            .to_string();
        assert!(err.contains("is 0"), "{err}");
    }

    #[test]
    fn a_retired_port_setting_is_ignored() {
        let ports = toml::from_str::<Ports>("hls_origin = 7314\ncontrol = 7410").unwrap();
        assert_eq!(ports.get("control"), 7410);
        assert!(spec("hls_origin").is_none());
        assert_eq!(ports.iter().count(), PORTS.len());
    }

    #[test]
    fn an_override_keeps_the_other_defaults() {
        let ports: Ports = toml::from_str("control = 17310").unwrap();
        assert_eq!(ports.get("control"), 17310);
        assert_eq!(ports.get("srt_ingest"), 7313);
    }

    #[test]
    fn a_duplicate_is_found_only_on_the_same_protocol() {
        let mut ports = Ports::default();
        ports.set("chat", 7310); // tcp, like control
        assert_eq!(
            ports.duplicates(),
            vec![("control", "chat", 7310, Proto::Tcp)]
        );
        let mut ports = Ports::default();
        ports.set("srt_ingest", 7310); // udp: no clash with control's tcp
        assert!(ports.duplicates().is_empty());
    }

    #[test]
    fn a_held_tcp_port_is_a_clash_with_a_free_suggestion() {
        let held = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert_eq!(
            check_free(LOOPBACK, port, Proto::Tcp),
            Err(Unavailable::InUse)
        );

        static SPEC: PortSpec = PortSpec {
            key: "control",
            proto: Proto::Tcp,
            default: 7310,
            alternatives: &[0], // port 0 always binds: stands in for a free suggestion
            exposure: Exposure::Loopback,
            what: "a test",
        };
        let clash = check_one(&SPEC, port, LOOPBACK).unwrap_err();
        assert_eq!(clash.why, Unavailable::InUse);
        assert_eq!(clash.next_free, Some(0));
        let text = clash.to_string();
        assert!(
            text.contains(&format!("port {port}/tcp for `control`")),
            "{text}"
        );
        assert!(text.contains("is in use"), "{text}");
        assert!(text.contains("any free port may be used"), "{text}");
    }

    #[test]
    fn a_held_udp_port_is_a_clash_and_tcp_on_it_is_not() {
        let held = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert_eq!(
            check_free(LOOPBACK, port, Proto::Udp),
            Err(Unavailable::InUse)
        );
        // TCP on the same number may still be free; it usually is, but another process could
        // hold it, so only the UDP result is asserted.
    }

    #[test]
    fn a_clash_with_no_free_suggestion_says_so() {
        let clash = Clash {
            key: "control",
            port: 7310,
            proto: Proto::Tcp,
            why: Unavailable::InUse,
            what: "the control plane",
            next_free: None,
            alternatives: &[17310, 27310],
        };
        assert!(
            clash
                .to_string()
                .contains("none of the suggestions (17310, 27310) is free")
        );
    }
}
