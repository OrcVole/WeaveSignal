// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The update feed, checked the way the updater checks it (ADR-0015, ADR-0140): each entry's
//! signature verifies, with the public key, over the file its URL names, and the version the
//! signature was made for is the version the manifest announces (`requireSignedVersion`).
//!
//! `just feed-test` makes a throwaway key under `target/`, signs stand-in packages, builds the
//! feed with `deploy/desktop/latest-json.sh`, serves it on a loopback port, downloads what the
//! manifest names into a folder, and runs this test over that folder. On its own the test is
//! ignored: it needs that folder (`WEAVESIGNAL_FEED_DIR`).
//!
//! The folder holds `latest.json`, `pubkey` (the `.pub` file's contents, as the app is built
//! with it) and the downloaded files under the names the URLs end in.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};

fn decoded(b64: &str) -> String {
    String::from_utf8(STANDARD.decode(b64.trim()).expect("base64")).expect("utf-8")
}

/// What `verify_signature` and `verify_signed_version` in tauri-plugin-updater 2.14.0 do.
fn verify(data: &[u8], signature: &str, pubkey: &str, announced: &str) -> Result<(), String> {
    let key = PublicKey::decode(&decoded(pubkey)).map_err(|e| format!("public key: {e}"))?;
    let sig = Signature::decode(&decoded(signature)).map_err(|e| format!("signature: {e}"))?;
    key.verify(data, &sig, true)
        .map_err(|e| format!("verify: {e}"))?;
    let signed = sig
        .trusted_comment()
        .split('\t')
        .find_map(|f| f.strip_prefix("version:"))
        .ok_or("the signature carries no version")?;
    if signed.trim_start_matches('v') == announced.trim_start_matches('v') {
        Ok(())
    } else {
        Err(format!("signed for {signed}, announced {announced}"))
    }
}

fn folder() -> PathBuf {
    PathBuf::from(std::env::var("WEAVESIGNAL_FEED_DIR").expect("WEAVESIGNAL_FEED_DIR"))
}

fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("latest.json")).expect("latest.json"))
        .expect("latest.json is JSON")
}

#[test]
#[ignore = "needs the folder `just feed-test` makes"]
fn every_entry_verifies_over_the_file_it_names() {
    let dir = folder();
    let pubkey = std::fs::read_to_string(dir.join("pubkey")).expect("pubkey");
    let m = manifest(&dir);
    let version = m["version"].as_str().expect("version");
    let platforms = m["platforms"].as_object().expect("platforms");
    assert!(!platforms.is_empty());
    for (target, entry) in platforms {
        let url = entry["url"].as_str().expect("url");
        let name = url.rsplit('/').next().unwrap();
        let data =
            std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{target}: {name}: {e}"));
        let signature = entry["signature"].as_str().expect("signature");
        verify(&data, signature, &pubkey, version).unwrap_or_else(|e| panic!("{target}: {e}"));
    }
}

#[test]
#[ignore = "needs the folder `just feed-test` makes"]
fn a_changed_file_a_changed_version_or_another_key_is_refused() {
    let dir = folder();
    let pubkey = std::fs::read_to_string(dir.join("pubkey")).expect("pubkey");
    let m = manifest(&dir);
    let version = m["version"].as_str().expect("version");
    let (target, entry) = m["platforms"].as_object().unwrap().iter().next().unwrap();
    let name = entry["url"].as_str().unwrap().rsplit('/').next().unwrap();
    let mut data = std::fs::read(dir.join(name)).unwrap();
    let signature = entry["signature"].as_str().unwrap();

    // The announced version is not the signed one: an old package passed off as a new one.
    assert!(
        verify(&data, signature, &pubkey, "9.9.9").is_err(),
        "{target}: a version the signature was not made for was accepted"
    );
    // One byte changed.
    data[0] ^= 1;
    assert!(
        verify(&data, signature, &pubkey, version).is_err(),
        "{target}: a changed file was accepted"
    );
    // A different key (the signature's own key id no longer matches).
    let other = std::fs::read_to_string(dir.join("pubkey-other")).expect("pubkey-other");
    data[0] ^= 1;
    assert!(
        verify(&data, signature, &other, version).is_err(),
        "{target}: another key's signature was accepted"
    );
}
