// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The Podcast Taxonomy Project's roles and groups, for `podcast:person` (ADR-0014 names the
//! persons taxonomy among the ports). `data/taxonomy.json` is the namespace repository's
//! `taxonomy.json` (Podcastindex-org/podcast-namespace at commit d4036dc, 2026-10-10, CC0-1.0),
//! unmodified; recorded in NOTICES.md.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    group: String,
    role: String,
}

fn entries() -> &'static [(String, String)] {
    static ALL: OnceLock<Vec<(String, String)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let raw: Vec<Entry> = serde_json::from_str(include_str!("../data/taxonomy.json"))
            .expect("the bundled taxonomy is valid JSON");
        raw.into_iter()
            .map(|e| (e.group.to_lowercase(), e.role.to_lowercase()))
            .collect()
    })
}

/// Whether `role` is a role of `group` (both compared without case, as the namespace says).
pub fn is_role(group: &str, role: &str) -> bool {
    let (g, r) = (group.to_lowercase(), role.to_lowercase());
    entries().iter().any(|(eg, er)| *eg == g && *er == r)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hosts_and_guests_are_cast_roles() {
        assert!(super::is_role("cast", "host"));
        assert!(super::is_role("Cast", "Guest"));
        assert!(super::is_role("cast", "co-host"));
        assert!(!super::is_role("cast", "producer"));
        assert!(super::entries().len() > 60);
    }
}
