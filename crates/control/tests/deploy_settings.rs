// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Issue 5: `[studio]` and `[steering]` from the environment on the Compose bundle and the
//! Cloudron package. The two install routes write the control plane's settings themselves, so
//! these tests read what they write and give it to the control plane's own parser:
//!
//! - Compose: `deploy/compose/compose.yaml` holds every default again (a template cannot leave a
//!   line out), so with nothing set they must equal the control plane's own; and the examples in
//!   `.env.example` must parse and say what they claim.
//! - Cloudron: `deploy/cloudron/station-sections.sh` writes only what is set, so nothing set is
//!   nothing written, and the same examples must come out the same.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use weavesignal_control::config::Config;

const MINIMAL: &str =
    "[station]\nid = \"6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10\"\nname = \"Kettle FM\"\n";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The commented examples in `.env.example` (`# NAME=value`), as the settings they show:
/// single quotes around a value are taken off, as Compose and the shell take them.
fn examples() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in read("deploy/compose/.env.example").lines() {
        let Some(rest) = line.strip_prefix("# ") else {
            continue;
        };
        let Some((k, v)) = rest.split_once('=') else {
            continue;
        };
        if k.starts_with("STUDIO_") || k.starts_with("STEERING_") {
            let v = v
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
                .unwrap_or(v);
            out.insert(k.to_owned(), v.to_owned());
        }
    }
    out
}

/// The lines of Compose's control settings from `[studio]` to the end of `[steering]`, with each
/// `${NAME:-default}` replaced by `env`'s value for NAME when it is not empty, else its default
/// (Compose's own rule).
fn compose_fragment(env: &BTreeMap<String, String>) -> String {
    let yaml = read("deploy/compose/compose.yaml");
    let mut keep = false;
    let mut lines = Vec::new();
    for line in yaml.lines() {
        let t = line.trim();
        if t == "[studio]" {
            keep = true;
        }
        if keep && !t.starts_with('#') && !t.is_empty() {
            lines.push(t.to_owned());
        }
        if keep && t.starts_with("edges = [") {
            break;
        }
    }
    assert!(
        lines.last().is_some_and(|l| l.starts_with("edges = [")),
        "compose.yaml's control settings no longer end with the steering edges"
    );
    let mut text = lines.join("\n");
    while let Some(start) = text.find("${") {
        let end = start + text[start..].find('}').expect("an unclosed ${");
        let inner = text[start + 2..end].to_owned();
        let (name, default) = inner.split_once(":-").expect("a ${NAME:-default}");
        let value = env
            .get(name)
            .filter(|v| !v.is_empty())
            .map_or(default, |v| v);
        text.replace_range(start..=end, value);
    }
    text
}

fn parse(fragment: &str) -> Config {
    parse_whole(&format!("{MINIMAL}\n{fragment}\n"))
}

fn parse_whole(text: &str) -> Config {
    Config::from_toml(text, "fragment").unwrap_or_else(|e| panic!("{e}\n{text}"))
}

/// What `station_sections` writes for these settings.
fn cloudron_fragment(env: &BTreeMap<String, String>) -> String {
    let script = root().join("deploy/cloudron/station-sections.sh");
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg("set -euo pipefail; source \"$1\"; station_sections")
        .arg("bash")
        .arg(script)
        .env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("bash");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn compose_with_nothing_set_is_the_control_planes_defaults() {
    let c = parse(&compose_fragment(&BTreeMap::new()));
    let d = parse("");
    assert_eq!(c.studio, d.studio);
    assert_eq!(c.steering, d.steering);
    assert!(!c.steering.enabled());
}

#[test]
fn the_examples_in_env_example_set_what_they_say_on_both_routes() {
    let env = examples();
    assert!(env.len() >= 4, "the examples were not found: {env:?}");
    for (route, fragment) in [
        ("compose", compose_fragment(&env)),
        ("cloudron", cloudron_fragment(&env)),
    ] {
        let c = parse(&fragment);
        assert_eq!(c.studio.show, "The Morning Kettle", "{route}");
        assert_eq!(c.studio.listener_delay_ms, 13_000, "{route}");
        assert_eq!(c.studio.player_allowance_ms, Some(500), "{route}");
        assert_eq!(c.studio.rundown.len(), 2, "{route}");
        assert!(c.studio.rundown[0].hard_post, "{route}");
        assert_eq!(c.studio.rundown[0].key.as_deref(), Some("F2"), "{route}");
        assert_eq!(c.studio.banned.len(), 1, "{route}");
        assert_eq!(c.studio.banned[0].identity, "tavish", "{route}");
        assert!(c.steering.enabled(), "{route}");
        assert_eq!(c.steering.edges.len(), 2, "{route}");
        assert_eq!(c.steering.edges[0].pathway, "edge-1", "{route}");
        assert_eq!(c.steering.edges[1].weight, 0, "{route}");
    }
}

#[test]
fn every_setting_reaches_its_key_on_both_routes() {
    let env: BTreeMap<String, String> = [
        ("STUDIO_SHOW", "Kettle \"Late\" \\ Show"),
        ("STUDIO_LISTENER_DELAY_MS", "9000"),
        ("STUDIO_PLAYER_ALLOWANCE_MS", "700"),
        ("STEERING_TTL_S", "120"),
        ("STEERING_CHECK_EVERY_S", "10"),
        ("STEERING_CHECK_TIMEOUT_MS", "1500"),
        ("STEERING_DOWN_AFTER", "3"),
        ("STEERING_CLONE_FROM", "origin"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    // Compose cannot escape a quote or a backslash in the show (weavesignal.sh refuses them).
    let mut compose_env = env.clone();
    compose_env.insert("STUDIO_SHOW".into(), "Kettle Late Show".into());
    let c = parse(&compose_fragment(&compose_env));
    assert_eq!(c.studio.show, "Kettle Late Show");
    let k = parse(&cloudron_fragment(&env));
    assert_eq!(k.studio.show, "Kettle \"Late\" \\ Show");
    for c in [c, k] {
        assert_eq!(c.studio.listener_delay_ms, 9000);
        assert_eq!(c.studio.player_allowance_ms, Some(700));
        assert_eq!(c.steering.ttl_s, 120);
        assert_eq!(c.steering.check_every_s, 10);
        assert_eq!(c.steering.check_timeout_ms, 1500);
        assert_eq!(c.steering.down_after, 3);
        assert_eq!(c.steering.clone_from, "origin");
    }
}

/// An installed station keeps what it set before the split (ADR-0289), on both routes: only
/// the old figure set means the old figure; the parts set mean the parts; both, the parts.
#[test]
fn the_old_listener_delay_setting_still_means_what_it_did_on_both_routes() {
    use weavesignal_control::studio::ListenerPath::{self, Parts, Whole};
    let set = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    for (what, env, want) in [
        ("nothing set", set(&[]), ListenerPath::default_parts()),
        (
            "the old line as an install has had it",
            set(&[("STUDIO_LISTENER_DELAY_MS", "13000")]),
            ListenerPath::default_parts(),
        ),
        (
            "the old line changed",
            set(&[("STUDIO_LISTENER_DELAY_MS", "9000")]),
            Whole(9_000),
        ),
        (
            "the old line, the allowance's line empty (a package update's line)",
            set(&[
                ("STUDIO_LISTENER_DELAY_MS", "9000"),
                ("STUDIO_PLAYER_ALLOWANCE_MS", ""),
            ]),
            Whole(9_000),
        ),
        (
            "the allowance",
            set(&[("STUDIO_PLAYER_ALLOWANCE_MS", "700")]),
            Parts {
                player_allowance_ms: 700,
            },
        ),
        (
            "both: the allowance wins",
            set(&[
                ("STUDIO_LISTENER_DELAY_MS", "9000"),
                ("STUDIO_PLAYER_ALLOWANCE_MS", "700"),
            ]),
            Parts {
                player_allowance_ms: 700,
            },
        ),
    ] {
        for (route, fragment) in [
            ("compose", compose_fragment(&env)),
            ("cloudron", cloudron_fragment(&env)),
        ] {
            assert_eq!(
                parse(&fragment).studio.listener_path(),
                want,
                "{what}, {route}"
            );
        }
    }
}

#[test]
fn cloudron_with_nothing_set_writes_nothing() {
    assert_eq!(cloudron_fragment(&BTreeMap::new()), "");
    // Empty is unset: the lines `weavesignal.env` gains with a package update.
    let empties: BTreeMap<String, String> = [
        "STUDIO_SHOW",
        "STUDIO_LISTENER_DELAY_MS",
        "STUDIO_PLAYER_ALLOWANCE_MS",
        "STUDIO_RUNDOWN",
        "STUDIO_BANNED",
        "STEERING_EDGES",
        "STEERING_TTL_S",
        "STEERING_CHECK_EVERY_S",
        "STEERING_CHECK_TIMEOUT_MS",
        "STEERING_DOWN_AFTER",
        "STEERING_CLONE_FROM",
    ]
    .into_iter()
    .map(|k| (k.to_owned(), String::new()))
    .collect();
    assert_eq!(cloudron_fragment(&empties), "");
}

#[test]
fn cloudron_refuses_a_setting_that_is_not_a_whole_number() {
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg("set -euo pipefail; source \"$1\"; station_sections")
        .arg("bash")
        .arg(root().join("deploy/cloudron/station-sections.sh"))
        .env_clear()
        .env("STEERING_TTL_S", "5 minutes");
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("STEERING_TTL_S=5 minutes"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn the_stations_own_file_still_replaces_a_section() {
    // The Cloudron package writes [studio] and [steering] from weavesignal.env and the control
    // plane then adds /app/data/station.toml, whose sections replace those whole.
    let base = format!(
        "{MINIMAL}{}",
        cloudron_fragment(&[("STUDIO_SHOW".to_owned(), "From the env".to_owned())].into())
    );
    let merged = weavesignal_control::config::with_extra(
        &base,
        "control.toml",
        "[studio]\nshow = \"From the file\"\n",
        "station.toml",
    )
    .unwrap();
    assert_eq!(parse_whole(&merged).studio.show, "From the file");
}
