// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The address the station's own proxy saw, from `X-Forwarded-For`; the control plane and the
//! chat read it the same way (review 0028, C2: each had its own copy).

use std::net::IpAddr;

/// The **last** `X-Forwarded-For` entry, from the header's lines in order (`None` for a line
/// that is not text). A proxy that appends (Cloudron's nginx) keeps whatever the client sent in
/// front of what it saw, so the first entry is the client's to forge; one that replaces the
/// header (the Compose bundle's Caddy) sends one entry, which is also the last.
pub fn last_forwarded<'a>(
    mut lines: impl DoubleEndedIterator<Item = Option<&'a str>>,
) -> Option<IpAddr> {
    lines.next_back()??.rsplit(',').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_entry_counts_and_a_forged_first_does_not() {
        let ip = |lines: &[Option<&str>]| last_forwarded(lines.iter().copied());
        let real: IpAddr = "198.51.100.4".parse().unwrap();
        assert_eq!(ip(&[Some("198.51.100.4")]), Some(real));
        assert_eq!(ip(&[Some("6.6.6.6, 7.7.7.7, 198.51.100.4")]), Some(real));
        assert_eq!(
            ip(&[Some("6.6.6.6"), Some("203.0.113.9,198.51.100.4")]),
            Some(real)
        );
        assert_eq!(ip(&[Some("6.6.6.6, unknown")]), None);
        assert_eq!(
            ip(&[Some("198.51.100.4"), None]),
            None,
            "an unreadable last line is no address"
        );
        assert_eq!(ip(&[]), None);
    }
}
