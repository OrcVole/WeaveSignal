// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A pulled source's address (ADR-0291, migration 0042): the show whose source a partner station
//! or a host opens by giving the station a URL to read. The parse only checks the text a person
//! typed and does no network I/O; whoever resolves the name checks every address it gave with
//! [`check_resolved`] before connecting to one of them: the control plane when the address is
//! saved (`crates/control/src/sources.rs`), and the ingest at every connection
//! (`crates/ingest/src/pull.rs`), which connects only to the addresses it checked.
//!
//! **Parse** (`parse_pull_url`): trim, split the scheme from the authority, and split the
//! authority from the path. The scheme is `http`, `https` or `srt`; the host is lower case and,
//! for an IPv6 literal, written without its brackets; the path and its query keep their case
//! (a mount, a token, an SRT stream id). An address that is private by its name or its literal
//! address is refused unless `allow_private` is set.
//!
//! **The guard** (`is_private_host`, `is_private_addr`, `check_resolved`): what a pull must not
//! reach. A name that is private whatever it resolves to is refused here; a name that has been
//! resolved is checked again against every address it gave, so a rebinding cannot slip one past
//! the name.

use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};
use utoipa::ToSchema;

/// How a pulled source is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PullKind {
    /// An Icecast address or any HTTP(S) audio stream.
    Http,
    /// An `.m3u8` playlist.
    Hls,
    /// An SRT caller.
    Srt,
}

/// A parsed and checked pull address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullTarget {
    /// `http`, `https` or `srt`, lower case.
    pub scheme: String,
    /// The host, lower case; no brackets for an IPv6 literal.
    pub host: String,
    /// The port, always present.
    pub port: u16,
    /// The path, with any query, or `/` when empty; its case is kept.
    pub path: String,
    /// The normalised `scheme://host:port/path`.
    pub url: String,
    /// The form the address takes.
    pub kind: PullKind,
}

/// Why a pull address was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PullUrlError {
    /// The address is empty.
    #[error("the address is empty")]
    Empty,
    /// The address is longer than 2048 characters.
    #[error("the address is longer than 2048 characters")]
    TooLong,
    /// The address must start with http://, https:// or srt://.
    #[error("the address must start with http://, https:// or srt://")]
    Scheme,
    /// The address has no host.
    #[error("the address has no host")]
    NoHost,
    /// The address must not hold a user name or password.
    #[error("the address must not hold a user name or password")]
    Credentials,
    /// The port is not a number from 1 to 65535.
    #[error("the port is not a number from 1 to 65535")]
    Port,
    /// An srt:// address needs a port.
    #[error("an srt:// address needs a port")]
    SrtNeedsPort,
    /// The address points inside the station's own network.
    #[error("the address points inside the station's own network")]
    Private,
}

/// A pull address, parsed and checked. `allow_private` lets a private name or literal through.
///
/// The scheme is lower case; the host is lower case and, for an IPv6 literal, has no brackets.
/// The path keeps its query, or is `/` when there is none. `url` is the normalised
/// `scheme://host:port path`. A private name or literal address is `Err(Private)` unless
/// `allow_private` is set.
pub fn parse_pull_url(input: &str, allow_private: bool) -> Result<PullTarget, PullUrlError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PullUrlError::Empty);
    }
    if trimmed.chars().count() > 2048 {
        return Err(PullUrlError::TooLong);
    }

    // Only the scheme is matched without regard to case; the rest keeps its own (the host is
    // lowered below, the path never).
    let lower_head = trimmed.get(..8).unwrap_or(trimmed).to_ascii_lowercase();
    let scheme = if lower_head.starts_with("https://") {
        "https"
    } else if lower_head.starts_with("http://") {
        "http"
    } else if lower_head.starts_with("srt://") {
        "srt"
    } else {
        return Err(PullUrlError::Scheme);
    };
    let rest = &trimmed[scheme.len() + 3..];

    // The authority runs to the first path, query or fragment; the path keeps its query.
    let authority_end = rest
        .find(|c| ['/', '?', '#'].contains(&c))
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let path = &rest[authority_end..];

    if authority.contains('@') {
        return Err(PullUrlError::Credentials);
    }

    let (host_raw, port_raw) = if let Some(h) = authority.strip_prefix('[') {
        let end = h.find(']').ok_or(PullUrlError::NoHost)?;
        let host = &h[..end];
        let rest = &h[end + 1..];
        let port = rest.strip_prefix(':').map(|s| s.to_owned());
        (host.to_owned(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p.to_owned())),
            None => (authority.to_owned(), None),
        }
    };

    if host_raw.is_empty() {
        return Err(PullUrlError::NoHost);
    }

    let port = match port_raw {
        None => match scheme {
            "http" => 80,
            "https" => 443,
            _ => return Err(PullUrlError::SrtNeedsPort),
        },
        Some(p) => {
            let port: u16 = p.parse().map_err(|_| PullUrlError::Port)?;
            if !(1..=65535).contains(&port) {
                return Err(PullUrlError::Port);
            }
            port
        }
    };

    if !allow_private && is_private_host(&host_raw) {
        return Err(PullUrlError::Private);
    }

    let host = host_raw.to_ascii_lowercase();
    let kind = if scheme == "srt" {
        PullKind::Srt
    } else if path
        .split('?')
        .next()
        .map(|p| p.to_ascii_lowercase().ends_with(".m3u8"))
        .unwrap_or(false)
    {
        PullKind::Hls
    } else {
        PullKind::Http
    };

    let path = if path.is_empty() { "/" } else { path };
    let display_host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    let url = format!("{scheme}://{display_host}:{port}{path}");

    Ok(PullTarget {
        scheme: scheme.to_owned(),
        host,
        port,
        path: path.to_owned(),
        url,
        kind,
    })
}

impl PullTarget {
    /// The value of `name` in the address's query (`?streamid=…&latency=…`), percent-decoded;
    /// the first one when it is given twice. A literal `#` is kept as part of the value, as
    /// ffmpeg reads SRT addresses (`streamid=#!::r=…`).
    pub fn query_value(&self, name: &str) -> Option<String> {
        let query = self.path.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k == name).then(|| percent_decode(v))
        })
    }

    /// The path to ask an HTTP server for: the path and query, without a fragment.
    pub fn request_path(&self) -> &str {
        let p = self.path.split('#').next().unwrap_or("/");
        if p.is_empty() { "/" } else { p }
    }
}

/// `%XX` decoded (and `+` left alone); a malformed escape is kept as it is.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// An address a pull must not reach unless allowed: loopback, unspecified, private (10/8,
/// 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10), carrier-grade NAT (100.64/10),
/// broadcast, multicast, and IPv6 unique-local (fc00::/7). An IPv4-mapped IPv6 address is judged
/// as its IPv4 address.
pub fn is_private_addr(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(a) => is_private_v4(a),
        std::net::IpAddr::V6(a) => {
            if let Some(m) = a.to_ipv4_mapped() {
                return is_private_v4(m);
            }
            is_private_v6(a)
        }
    }
}

fn is_private_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    o[0] == 0 // unspecified, 0.0.0.0/8
        || o[0] == 127 // loopback, 127.0.0.0/8
        || o[0] == 10 // private, 10/8
        || o[0] == 172 && (16..=31).contains(&o[1]) // private, 172.16/12
        || o[0] == 192 && o[1] == 168 // private, 192.168/16
        || o[0] == 169 && o[1] == 254 // link-local, 169.254/16
        || o[0] == 100 && (64..=127).contains(&o[1]) // carrier-grade NAT, 100.64/10
        || o == [255, 255, 255, 255] // broadcast
        || (224..=239).contains(&o[0]) // multicast, 224.0.0.0/4
}

fn is_private_v6(a: Ipv6Addr) -> bool {
    let seg = a.segments();
    a.is_loopback()
        || a.is_unspecified()
        || seg[0] & 0xfe00 == 0xfc00 // unique-local, fc00::/7
        || seg[0] & 0xffc0 == 0xfe80 // link-local, fe80::/10
        || seg[0] & 0xff00 == 0xff00 // multicast, ff00::/8
}

/// A host name that is private whatever it resolves to: `localhost`, anything ending `.localhost`,
/// `.local`, `.internal`, `.lan` or `.home.arpa`, or a literal IP for which
/// [`is_private_addr`] is true. A trailing dot is ignored; the match is case-insensitive.
pub fn is_private_host(host: &str) -> bool {
    let mut h = host.trim().to_ascii_lowercase();
    if h.ends_with('.') {
        h.pop();
    }
    if h.is_empty() {
        return false;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_private_addr(ip);
    }
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".lan")
        || h.ends_with(".home.arpa")
}

/// The check after a name has been resolved (DNS rebinding): every address must pass. An empty
/// list is `Err(Private)`.
pub fn check_resolved(addrs: &[std::net::IpAddr], allow_private: bool) -> Result<(), PullUrlError> {
    if addrs.is_empty() {
        return Err(PullUrlError::Private);
    }
    for a in addrs {
        if !allow_private && is_private_addr(*a) {
            return Err(PullUrlError::Private);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn an_http_address_uses_its_default_port() {
        let t = parse_pull_url("http://example.com", false).unwrap();
        assert_eq!(t.scheme, "http");
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 80);
        assert_eq!(t.path, "/");
        assert_eq!(t.kind, PullKind::Http);
        assert_eq!(t.url, "http://example.com:80/");
    }

    #[test]
    fn an_https_address_uses_its_default_port() {
        let t = parse_pull_url("https://example.com", false).unwrap();
        assert_eq!(t.scheme, "https");
        assert_eq!(t.port, 443);
        assert_eq!(t.kind, PullKind::Http);
        assert_eq!(t.url, "https://example.com:443/");
    }

    #[test]
    fn a_scheme_and_host_are_lower_case() {
        let t = parse_pull_url("HTTP://Example.COM/x", false).unwrap();
        assert_eq!(t.scheme, "http");
        assert_eq!(t.host, "example.com");
        assert_eq!(t.path, "/x");
    }

    #[test]
    fn the_path_and_query_keep_their_case() {
        let t = parse_pull_url("HTTP://Radio.Example.COM:8000/Live.MP3?Token=AbC", false).unwrap();
        assert_eq!(t.host, "radio.example.com");
        assert_eq!(t.path, "/Live.MP3?Token=AbC");
        assert_eq!(t.url, "http://radio.example.com:8000/Live.MP3?Token=AbC");
        let s = parse_pull_url("srt://Example.com:9000?streamid=Kettle-FM", false).unwrap();
        assert_eq!(s.path, "?streamid=Kettle-FM");
    }

    #[test]
    fn a_query_value_is_decoded() {
        let t = parse_pull_url(
            "srt://example.com:9000?streamid=%23!::r=live,m=request&latency=200",
            false,
        )
        .unwrap();
        assert_eq!(
            t.query_value("streamid").as_deref(),
            Some("#!::r=live,m=request")
        );
        assert_eq!(t.query_value("latency").as_deref(), Some("200"));
        assert_eq!(t.query_value("passphrase"), None);
        let h = parse_pull_url("http://example.com/live#here", false).unwrap();
        assert_eq!(h.request_path(), "/live");
        assert_eq!(h.query_value("x"), None);
    }

    #[test]
    fn the_path_keeps_its_query() {
        let t = parse_pull_url("http://example.com/a?b=c", false).unwrap();
        assert_eq!(t.path, "/a?b=c");
    }

    #[test]
    fn an_empty_path_becomes_a_slash() {
        let t = parse_pull_url("http://example.com/", false).unwrap();
        assert_eq!(t.path, "/");
    }

    #[test]
    fn an_m3u8_path_is_hls() {
        let t = parse_pull_url("http://example.com/live.m3u8", false).unwrap();
        assert_eq!(t.kind, PullKind::Hls);
    }

    #[test]
    fn an_m3u8_with_a_query_is_still_hls() {
        let t = parse_pull_url("http://example.com/playlist.m3u8?token=x", false).unwrap();
        assert_eq!(t.kind, PullKind::Hls);
        assert_eq!(t.path, "/playlist.m3u8?token=x");
    }

    #[test]
    fn a_non_m3u8_path_is_http() {
        let t = parse_pull_url("http://example.com/stream", false).unwrap();
        assert_eq!(t.kind, PullKind::Http);
    }

    #[test]
    fn an_srt_address_needs_a_port() {
        assert_eq!(
            parse_pull_url("srt://example.com", false),
            Err(PullUrlError::SrtNeedsPort)
        );
    }

    #[test]
    fn an_srt_address_keeps_its_port() {
        let t = parse_pull_url("srt://example.com:8889", false).unwrap();
        assert_eq!(t.scheme, "srt");
        assert_eq!(t.port, 8889);
        assert_eq!(t.kind, PullKind::Srt);
    }

    #[test]
    fn an_ipv6_host_is_stored_without_brackets() {
        let t = parse_pull_url("http://[2001:db8::1]:8080/", false).unwrap();
        assert_eq!(t.host, "2001:db8::1");
        assert_eq!(t.port, 8080);
        assert_eq!(t.url, "http://[2001:db8::1]:8080/");
    }

    #[test]
    fn credentials_are_refused() {
        assert_eq!(
            parse_pull_url("http://user@example.com/", false),
            Err(PullUrlError::Credentials)
        );
    }

    #[test]
    fn a_bad_port_is_refused() {
        assert_eq!(
            parse_pull_url("http://example.com:99999/", false),
            Err(PullUrlError::Port)
        );
    }

    #[test]
    fn a_port_below_one_is_refused() {
        assert_eq!(
            parse_pull_url("http://example.com:0/", false),
            Err(PullUrlError::Port)
        );
    }

    #[test]
    fn an_empty_address_is_empty() {
        assert_eq!(parse_pull_url("", false), Err(PullUrlError::Empty));
        assert_eq!(parse_pull_url("   ", false), Err(PullUrlError::Empty));
    }

    #[test]
    fn a_too_long_address_is_too_long() {
        let long = format!("http://host.{}", "a".repeat(2040));
        assert_eq!(parse_pull_url(&long, false), Err(PullUrlError::TooLong));
    }

    #[test]
    fn a_bad_scheme_is_scheme() {
        assert_eq!(
            parse_pull_url("ftp://example.com/", false),
            Err(PullUrlError::Scheme)
        );
    }

    #[test]
    fn an_address_with_no_host_is_no_host() {
        assert_eq!(
            parse_pull_url("http:///path", false),
            Err(PullUrlError::NoHost)
        );
    }

    #[test]
    fn a_private_name_is_refused_by_default() {
        assert_eq!(
            parse_pull_url("http://localhost/", false),
            Err(PullUrlError::Private)
        );
    }

    #[test]
    fn a_private_name_is_allowed_when_asked() {
        let t = parse_pull_url("http://localhost/", true).unwrap();
        assert_eq!(t.host, "localhost");
        assert_eq!(t.port, 80);
    }

    #[test]
    fn a_private_literal_is_refused_by_default() {
        assert_eq!(
            parse_pull_url("http://127.0.0.1/", false),
            Err(PullUrlError::Private)
        );
    }

    #[test]
    fn a_public_address_is_ok() {
        let t = parse_pull_url("http://example.com/", false).unwrap();
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 80);
    }

    #[test]
    fn is_private_addr_marks_the_private_ranges() {
        let v4 =
            |a: u8, b: u8, c: u8, d: u8| is_private_addr(IpAddr::V4(Ipv4Addr::new(a, b, c, d)));
        assert!(v4(10, 0, 0, 1));
        assert!(v4(192, 168, 1, 1));
        assert!(v4(172, 16, 0, 1));
        assert!(v4(172, 31, 255, 255));
        assert!(!v4(172, 15, 0, 1));
        assert!(v4(169, 254, 0, 1));
        assert!(v4(100, 64, 0, 1));
        assert!(v4(0, 0, 0, 0));
        assert!(v4(255, 255, 255, 255));
        assert!(v4(224, 0, 0, 1));
        assert!(v4(239, 255, 255, 255));
        assert!(!v4(172, 32, 0, 1));
        assert!(!v4(8, 8, 8, 8));
        assert!(!v4(1, 2, 3, 4));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0xff00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(!is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_private_addr(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0xffff, 10, 1
        ))));
    }

    #[test]
    fn is_private_host_marks_the_private_names() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("LOCALhost"));
        assert!(is_private_host("example.localhost"));
        assert!(is_private_host("example.local"));
        assert!(is_private_host("example.internal"));
        assert!(is_private_host("example.lan"));
        assert!(is_private_host("example.home.arpa"));
        assert!(is_private_host("localhost."));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("10.0.0.1"));
        assert!(is_private_host("::1"));
        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("example.com."));
        assert!(!is_private_host("8.8.8.8"));
        assert!(!is_private_host("2001:db8::1"));
    }

    #[test]
    fn check_resolved_requires_every_address_to_pass() {
        assert_eq!(check_resolved(&[], false), Err(PullUrlError::Private));
        assert_eq!(
            check_resolved(&[IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))], false),
            Ok(())
        );
    }

    #[test]
    fn check_resolved_refuses_a_private_address() {
        let addrs = [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        ];
        assert_eq!(check_resolved(&addrs, false), Err(PullUrlError::Private));
    }

    #[test]
    fn check_resolved_allows_private_when_asked() {
        let addrs = [IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        assert_eq!(check_resolved(&addrs, true), Ok(()));
    }
}
