// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//! Makes keyed-show tokens for the spike's tests (tests/keyed/run-edge.sh); the control plane
//! does this at redeem in 19b.
//!   keyed-token new <key-file>                  a new private key (PKCS#8)
//!   keyed-token public <key-file>               its public key, base64url
//!   keyed-token mint <key-file> <key> <show> <a|v|av> <nbf> <exp>
use base64::Engine as _;
use weavesignal_keyed::{Claims, Media, Signer};

fn signer(path: &str) -> Signer {
    Signer::from_pkcs8(&std::fs::read(path).expect("the key file")).expect("a P-256 key")
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("new") if a.len() == 2 => std::fs::write(&a[1], Signer::generate()).expect("written"),
        Some("public") if a.len() == 2 => println!(
            "{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signer(&a[1]).public_key())
        ),
        Some("mint") if a.len() == 7 => {
            let media = match a[4].as_str() {
                "a" => Media::Sound,
                "v" => Media::Video,
                "av" => Media::Both,
                m => panic!("media is a, v or av, not {m}"),
            };
            let c = Claims {
                key: a[2].clone(),
                show: a[3].clone(),
                media,
                nbf: a[5].parse().expect("nbf"),
                exp: a[6].parse().expect("exp"),
                device: None,
            };
            println!("{}", signer(&a[1]).mint(&c));
        }
        _ => {
            eprintln!(
                "keyed-token new <key-file> | public <key-file> | mint <key-file> <key> <show> <a|v|av> <nbf> <exp>"
            );
            std::process::exit(2);
        }
    }
}
