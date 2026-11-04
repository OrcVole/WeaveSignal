// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Passes: what a person who joins from a browser holds between room tokens.
//!
//! A room token lives ten minutes (ADR-0085), but a caller may wait longer than that, lose
//! their link and come back within five minutes to the same place (ADR-0101), and a guest's
//! invite link is opened some time after it was made. So the control plane gives each of
//! them a *pass*: a signed, short-lived statement of who they are in the room (identity, role,
//! name) that buys a fresh room token for that identity, and reads that person's own state.
//!
//! - A **call pass** is issued by the call-in route after the caller's consent to record is
//!   logged; it never names another identity.
//! - An **invite** is a pass the station creates for one guest, remote DJ or presenter; its
//!   link carries the pass in the URL's fragment, so it never reaches a server log.
//!
//! A pass is `rsp1.<claims>.<signature>`, base64url without padding, HMAC-SHA256 under a key
//! derived from the station's LiveKit secret with its own label, so a pass is never a valid
//! LiveKit token and a LiveKit token is never a valid pass.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::role::Role;
use crate::token::{Keys, TokenError};

type HmacSha256 = hmac::Hmac<Sha256>;

/// The prefix every pass starts with.
pub const PASS_PREFIX: &str = "rsp1";
/// The longest a pass may live: a day. Invites default shorter (the control plane's settings).
pub const MAX_PASS_TTL_S: u64 = 24 * 3600;
/// The label the pass key is derived under.
const PASS_LABEL: &[u8] = b"weavesignal pass v1";

/// How the pass was issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PassKind {
    /// From the call-in route, after consent.
    Call,
    /// From an invite the station made.
    Invite,
}

/// What a pass says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PassClaims {
    pub kind: PassKind,
    pub role: Role,
    /// The room identity it buys tokens for.
    pub id: String,
    /// The name the room shows.
    pub name: String,
    pub iat: u64,
    pub exp: u64,
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Keys {
    fn pass_mac(&self) -> HmacSha256 {
        let mut derive = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC takes a key of any length");
        derive.update(PASS_LABEL);
        let key = derive.finalize().into_bytes();
        HmacSha256::new_from_slice(&key).expect("HMAC takes a key of any length")
    }

    /// Sign a pass for `ttl_s` seconds from now.
    pub fn pass(
        &self,
        kind: PassKind,
        role: Role,
        identity: &str,
        name: &str,
        ttl_s: u64,
    ) -> Result<(String, PassClaims), TokenError> {
        if !(1..=MAX_PASS_TTL_S).contains(&ttl_s) {
            return Err(TokenError::Lifetime {
                asked: ttl_s,
                max: MAX_PASS_TTL_S,
            });
        }
        let iat = now_s();
        let claims = PassClaims {
            kind,
            role,
            id: identity.to_owned(),
            name: name.to_owned(),
            iat,
            exp: iat + ttl_s,
        };
        let body = B64.encode(serde_json::to_vec(&claims).expect("claims serialise"));
        let input = format!("{PASS_PREFIX}.{body}");
        let mut mac = self.pass_mac();
        mac.update(input.as_bytes());
        let sig = B64.encode(mac.finalize().into_bytes());
        Ok((format!("{input}.{sig}"), claims))
    }

    /// Check a pass's signature and lifetime, and return what it says.
    pub fn verify_pass(&self, pass: &str) -> Result<PassClaims, TokenError> {
        let mut parts = pass.split('.');
        let (Some(PASS_PREFIX), Some(b), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };
        let sig = B64.decode(s).map_err(|_| TokenError::Malformed)?;
        let mut mac = self.pass_mac();
        mac.update(format!("{PASS_PREFIX}.{b}").as_bytes());
        mac.verify_slice(&sig)
            .map_err(|_| TokenError::BadSignature)?;
        let body = B64.decode(b).map_err(|_| TokenError::Malformed)?;
        let claims: PassClaims =
            serde_json::from_slice(&body).map_err(|e| TokenError::Claims(e.to_string()))?;
        if now_s() >= claims.exp {
            return Err(TokenError::Expired);
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Keys {
        Keys {
            api_key: "m3test".into(),
            api_secret: "test-only-secret-test-only-secret-0".into(),
        }
    }

    #[test]
    fn a_pass_round_trips() {
        let k = keys();
        let (p, c) = k
            .pass(
                PassKind::Call,
                Role::Caller,
                "caller-1a2b",
                "Jessie Brown",
                3600,
            )
            .unwrap();
        assert!(p.starts_with("rsp1."));
        let back = k.verify_pass(&p).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.role, Role::Caller);
        assert_eq!(back.exp - back.iat, 3600);
    }

    #[test]
    fn a_pass_is_not_a_room_token_and_a_room_token_is_not_a_pass() {
        let k = keys();
        let (p, _) = k
            .pass(PassKind::Invite, Role::Guest, "guest-x", "", 60)
            .unwrap();
        assert!(k.verify(&p).is_err());
        let t = k
            .participant("studio", "guest-x", "", Role::Guest, 60)
            .unwrap();
        assert_eq!(k.verify_pass(&t), Err(TokenError::Malformed));
    }

    #[test]
    fn a_changed_or_foreign_pass_is_refused() {
        let k = keys();
        let (p, _) = k
            .pass(PassKind::Invite, Role::Guest, "guest-x", "", 60)
            .unwrap();
        let other = Keys {
            api_key: "m3test".into(),
            api_secret: "another-secret-another-secret-000".into(),
        };
        assert_eq!(other.verify_pass(&p), Err(TokenError::BadSignature));
        // Swap the claims for a presenter's, keep the signature.
        let forged = k
            .pass(PassKind::Invite, Role::Presenter, "guest-x", "", 60)
            .unwrap()
            .0;
        let mut a: Vec<&str> = p.split('.').collect();
        let b: Vec<&str> = forged.split('.').collect();
        a[1] = b[1];
        assert_eq!(k.verify_pass(&a.join(".")), Err(TokenError::BadSignature));
    }

    #[test]
    fn a_pass_lives_at_most_a_day() {
        let k = keys();
        assert!(
            k.pass(PassKind::Call, Role::Caller, "c", "", MAX_PASS_TTL_S + 1)
                .is_err()
        );
        assert!(k.pass(PassKind::Call, Role::Caller, "c", "", 0).is_err());
    }
}
