// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station's optional STUN servers (ADR-0290). A station contacts no STUN server unless its
//! operator names one: the setting is one text, `STUN_SERVERS`, a comma-separated list of
//! `host:port` that LiveKit, the desk and the join pages all read the same way. The shell
//! scripts that write the services' settings check it with the same rule (`deploy/`).

/// The most servers a station may name: `webrtcbin` takes one, LiveKit and the pages take a
/// short list, and a longer one is a typing mistake.
pub const MAX_STUN_SERVERS: usize = 4;

/// Parse the setting: entries split by commas or white space, each `host:port` (a DNS name or an
/// IPv4 address, and a port from 1 to 65535). Empty text is no servers. Repeats are dropped,
/// the order is kept.
pub fn parse_stun_servers(text: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for entry in text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|e| !e.is_empty())
    {
        check_entry(entry)?;
        let entry = entry.to_ascii_lowercase();
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    if out.len() > MAX_STUN_SERVERS {
        return Err(format!(
            "{} STUN servers are named; at most {MAX_STUN_SERVERS} are used",
            out.len()
        ));
    }
    Ok(out)
}

fn check_entry(entry: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("STUN server `{entry}` is not host:port ({why})"));
    if entry.contains("://") || entry.starts_with("stun:") || entry.starts_with("stuns:") {
        return bad(
            "leave out `stun:`; the address is a name or an IPv4 address, a colon and a port",
        );
    }
    let Some((host, port)) = entry.rsplit_once(':') else {
        return bad("it has no port");
    };
    if !matches!(port.parse::<u16>(), Ok(p) if p >= 1) || !port.bytes().all(|b| b.is_ascii_digit())
    {
        return bad("the port is a number from 1 to 65535");
    }
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if !host_ok {
        return bad("the host is a DNS name or an IPv4 address");
    }
    Ok(())
}

/// The URL `webrtcbin`'s `stun-server` property takes for the first server, if any.
pub fn webrtcbin_url(servers: &[String]) -> Option<String> {
    servers.first().map(|s| format!("stun://{s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_text_is_no_servers() {
        assert_eq!(parse_stun_servers(""), Ok(vec![]));
        assert_eq!(parse_stun_servers("  , ,\t"), Ok(vec![]));
    }

    #[test]
    fn servers_split_on_commas_and_spaces_in_order_without_repeats() {
        assert_eq!(
            parse_stun_servers(
                "stun.example.org:3478, Other.example.net:19302 stun.example.org:3478"
            ),
            Ok(vec![
                "stun.example.org:3478".to_owned(),
                "other.example.net:19302".to_owned()
            ])
        );
        assert_eq!(
            parse_stun_servers("192.0.2.7:3478"),
            Ok(vec!["192.0.2.7:3478".to_owned()])
        );
    }

    #[test]
    fn a_mistyped_entry_names_itself() {
        for bad in [
            "stun.example.org",
            "stun.example.org:",
            ":3478",
            "stun.example.org:0",
            "stun.example.org:65536",
            "stun.example.org:34x8",
            "stun:stun.example.org:3478",
            "stun://stun.example.org:3478",
            "[2001:db8::1]:3478",
            "-bad.example.org:3478",
            "a..b:3478",
            "under_score.example.org:3478",
        ] {
            let e = parse_stun_servers(bad).unwrap_err();
            assert!(e.contains(&format!("`{bad}`")), "{bad}: {e}");
        }
    }

    #[test]
    fn more_than_four_is_refused() {
        let five = "a.example:1,b.example:1,c.example:1,d.example:1,e.example:1";
        assert!(parse_stun_servers(five).unwrap_err().contains("at most 4"));
    }

    #[test]
    fn the_desk_takes_the_first_as_a_stun_url() {
        let s = parse_stun_servers("a.example:3478,b.example:3478").unwrap();
        assert_eq!(webrtcbin_url(&s).as_deref(), Some("stun://a.example:3478"));
        assert_eq!(webrtcbin_url(&[]), None);
    }
}
