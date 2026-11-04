// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! LiveKit access tokens: HS256 JSON Web Tokens signed with the station's LiveKit API secret.
//!
//! The control plane issues each person a short-lived token for one room, with their role as
//! an attribute; the desk signs itself a room-admin token to list the room's participants.
//! Neither grants the right to change one's own name, metadata or attributes, so a role read
//! from the room is the role the control plane gave.
//!
//! Who may hear what (ADR-0118): every page allows no one to subscribe to its tracks, and each
//! of the desk's mix-minus publishers allows only the person it is for. LiveKit lets a
//! participant with the recorder grant through whatever a publisher allows, so that grant goes
//! only to the desk's own listening connections and to monitors the station's admin asks for,
//! never to a token a pass gives out.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::role::{ROLE_ATTRIBUTE, Role};

type HmacSha256 = hmac::Hmac<Sha256>;

/// LiveKit's API key and secret.
#[derive(Clone)]
pub struct Keys {
    pub api_key: String,
    pub api_secret: String,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys")
            .field("api_key", &self.api_key)
            .field("api_secret", &"<hidden>")
            .finish()
    }
}

/// LiveKit's video grant (the fields WeaveSignal uses; LiveKit names them in camelCase).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Grants {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default)]
    pub room_join: bool,
    #[serde(default)]
    pub room_admin: bool,
    #[serde(default)]
    pub room_list: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_publish: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_subscribe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_publish_data: Option<bool>,
    #[serde(default)]
    pub can_update_own_metadata: bool,
    #[serde(default)]
    pub hidden: bool,
    /// Hears every track, whatever its publisher allows (LiveKit's exemption for recorders).
    #[serde(default)]
    pub recorder: bool,
}

/// The claims of a LiveKit token.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TokenClaims {
    /// The API key.
    pub iss: String,
    /// The participant's identity.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sub: String,
    pub nbf: u64,
    pub exp: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub video: Grants,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl TokenClaims {
    /// The role the token carries, if any.
    pub fn role(&self) -> Option<Role> {
        self.attributes.get(ROLE_ATTRIBUTE)?.parse().ok()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("the token is not three base64url parts")]
    Malformed,
    #[error("the token's signature does not match")]
    BadSignature,
    #[error("the token's claims are not valid JSON: {0}")]
    Claims(String),
    #[error("the token has expired or is not valid yet")]
    Expired,
    #[error("a token's lifetime must be 1 s to {max} s; {asked} s was asked")]
    Lifetime { asked: u64, max: u64 },
}

/// How long the desk's own tokens let it join: as long as the LiveKit signaller's own tokens
/// did when it signed them itself (6 h). Each connection is made with a fresh one.
pub const DESK_TTL_S: u64 = 6 * 3600;

/// The longest a participant's token may live: short-lived by design. It bounds only when the
/// token can be used to join; LiveKit keeps a joined participant's session alive itself.
pub const MAX_PARTICIPANT_TTL_S: u64 = 4 * 3600;

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Keys {
    /// Sign any claims.
    pub fn sign(&self, claims: &TokenClaims) -> String {
        let header = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let body = B64.encode(serde_json::to_vec(claims).expect("claims serialise"));
        let signing_input = format!("{header}.{body}");
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(signing_input.as_bytes());
        let sig = B64.encode(mac.finalize().into_bytes());
        format!("{signing_input}.{sig}")
    }

    /// A participant's token for one room: join, publish and subscribe; their role as an
    /// attribute; no data channel, no changing their own metadata or attributes.
    pub fn participant(
        &self,
        room: &str,
        identity: &str,
        name: &str,
        role: Role,
        ttl_s: u64,
    ) -> Result<String, TokenError> {
        self.participant_with(room, identity, name, role, ttl_s, true)
    }

    /// A participant's token, choosing whether it may subscribe. A GStreamer
    /// `livekitwebrtcsink` that joins with the subscribe grant publishes a track that carries
    /// no media (LiveKit 1.13.10 with gst-plugins-rs 0.15.4, 2026-10-25: the server makes the
    /// subscriber connection primary, and the sink does not follow), so a native publisher
    /// joins without it and hears its mix-minus through a second, listening connection.
    pub fn participant_with(
        &self,
        room: &str,
        identity: &str,
        name: &str,
        role: Role,
        ttl_s: u64,
        can_subscribe: bool,
    ) -> Result<String, TokenError> {
        if !(1..=MAX_PARTICIPANT_TTL_S).contains(&ttl_s) {
            return Err(TokenError::Lifetime {
                asked: ttl_s,
                max: MAX_PARTICIPANT_TTL_S,
            });
        }
        let now = now_s();
        let claims = TokenClaims {
            iss: self.api_key.clone(),
            sub: identity.to_owned(),
            // A little slack for clocks that differ between the control plane and LiveKit.
            nbf: now.saturating_sub(10),
            exp: now + ttl_s,
            name: name.to_owned(),
            video: Grants {
                room: Some(room.to_owned()),
                room_join: true,
                can_publish: Some(role.may_publish()),
                can_subscribe: Some(can_subscribe),
                can_publish_data: Some(false),
                ..Grants::default()
            },
            attributes: BTreeMap::from([(ROLE_ATTRIBUTE.to_owned(), role.as_str().to_owned())]),
        };
        Ok(self.sign(&claims))
    }

    /// A monitor's token (a `listener` asked for by the station's admin): it hears every
    /// track, whatever its publisher allows, and cannot publish. Never for a pass.
    pub fn monitor(
        &self,
        room: &str,
        identity: &str,
        name: &str,
        ttl_s: u64,
    ) -> Result<String, TokenError> {
        let token = self.participant_with(room, identity, name, Role::Listener, ttl_s, true)?;
        let mut claims = self.verify(&token).expect("a token just signed verifies");
        claims.video.recorder = true;
        Ok(self.sign(&claims))
    }

    /// The desk's token for a connection that only listens (to one person's voice and camera):
    /// hidden from the room, and let through whatever the person's page allows, which is no
    /// one but such a connection.
    pub fn desk_input(&self, room: &str, identity: &str) -> String {
        self.desk_token(
            room,
            identity,
            Grants {
                can_publish: Some(false),
                can_subscribe: Some(true),
                hidden: true,
                recorder: true,
                ..Grants::default()
            },
        )
    }

    /// The desk's token for a connection that only publishes (a mix-minus, the room picture).
    /// A `livekitwebrtcsink` that may subscribe sends no media (see [`Self::participant_with`]).
    pub fn desk_output(&self, room: &str, identity: &str) -> String {
        self.desk_token(
            room,
            identity,
            Grants {
                can_publish: Some(true),
                can_subscribe: Some(false),
                ..Grants::default()
            },
        )
    }

    fn desk_token(&self, room: &str, identity: &str, grants: Grants) -> String {
        let now = now_s();
        self.sign(&TokenClaims {
            iss: self.api_key.clone(),
            sub: identity.to_owned(),
            nbf: now.saturating_sub(10),
            exp: now + DESK_TTL_S,
            name: identity.to_owned(),
            video: Grants {
                room: Some(room.to_owned()),
                room_join: true,
                can_publish_data: Some(false),
                ..grants
            },
            attributes: BTreeMap::new(),
        })
    }

    /// A room-admin token for the server API (listing participants), valid for `ttl_s`.
    pub fn room_admin(&self, room: &str, ttl_s: u64) -> String {
        let now = now_s();
        self.sign(&TokenClaims {
            iss: self.api_key.clone(),
            nbf: now.saturating_sub(10),
            exp: now + ttl_s,
            video: Grants {
                room: Some(room.to_owned()),
                room_admin: true,
                room_list: true,
                ..Grants::default()
            },
            ..TokenClaims::default()
        })
    }

    /// Check a token's signature and lifetime, and return its claims.
    pub fn verify(&self, token: &str) -> Result<TokenClaims, TokenError> {
        let mut parts = token.split('.');
        let (Some(h), Some(b), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };
        let sig = B64.decode(s).map_err(|_| TokenError::Malformed)?;
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(format!("{h}.{b}").as_bytes());
        mac.verify_slice(&sig)
            .map_err(|_| TokenError::BadSignature)?;
        let body = B64.decode(b).map_err(|_| TokenError::Malformed)?;
        let claims: TokenClaims =
            serde_json::from_slice(&body).map_err(|e| TokenError::Claims(e.to_string()))?;
        let now = now_s();
        if now < claims.nbf || now >= claims.exp {
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
            api_key: "m1test".into(),
            api_secret: "test-only-secret-test-only-secret-0".into(),
        }
    }

    #[test]
    fn a_participant_token_carries_identity_room_and_role() {
        let k = keys();
        let t = k
            .participant("studio", "gregor", "Gregor Stewart", Role::Guest, 600)
            .unwrap();
        let c = k.verify(&t).unwrap();
        assert_eq!(c.iss, "m1test");
        assert_eq!(c.sub, "gregor");
        assert_eq!(c.name, "Gregor Stewart");
        assert_eq!(c.role(), Some(Role::Guest));
        assert_eq!(c.video.room.as_deref(), Some("studio"));
        assert!(c.video.room_join && !c.video.room_admin && !c.video.can_update_own_metadata);
        assert_eq!(c.video.can_publish_data, Some(false));
        assert!(c.exp - c.nbf <= 600 + 10);
    }

    #[test]
    fn the_wire_form_uses_livekits_names() {
        let k = keys();
        let t = k
            .participant("studio", "moira", "", Role::Screener, 60)
            .unwrap();
        let body = t.split('.').nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&B64.decode(body).unwrap()).unwrap();
        assert_eq!(json["video"]["roomJoin"], true);
        assert_eq!(json["video"]["canUpdateOwnMetadata"], false);
        assert_eq!(json["attributes"]["weavesignal.role"], "screener");
        assert!(json.get("name").is_none(), "an empty name is left out");
    }

    #[test]
    fn a_token_signed_with_another_secret_is_refused() {
        let t = keys()
            .participant("studio", "x", "", Role::Guest, 60)
            .unwrap();
        let other = Keys {
            api_secret: "another".into(),
            ..keys()
        };
        assert_eq!(other.verify(&t), Err(TokenError::BadSignature));
        let mut tampered = t.clone();
        tampered.insert(t.find('.').unwrap() + 3, 'A');
        assert!(keys().verify(&tampered).is_err());
    }

    #[test]
    fn a_native_publisher_token_does_not_subscribe() {
        let k = keys();
        let t = k
            .participant_with("studio", "hamish", "", Role::Presenter, 60, false)
            .unwrap();
        let c = k.verify(&t).unwrap();
        assert_eq!(c.video.can_subscribe, Some(false));
        assert_eq!(c.role(), Some(Role::Presenter));
    }

    #[test]
    fn lifetimes_are_short() {
        let k = keys();
        assert!(matches!(
            k.participant("r", "x", "", Role::Dj, 0),
            Err(TokenError::Lifetime { .. })
        ));
        assert!(
            k.participant("r", "x", "", Role::Dj, MAX_PARTICIPANT_TTL_S + 1)
                .is_err()
        );
        assert!(
            k.participant("r", "x", "", Role::Dj, MAX_PARTICIPANT_TTL_S)
                .is_ok()
        );
    }

    #[test]
    fn an_expired_token_is_refused() {
        let k = keys();
        let t = k.sign(&TokenClaims {
            iss: "m1test".into(),
            nbf: 1,
            exp: 2,
            ..TokenClaims::default()
        });
        assert_eq!(k.verify(&t), Err(TokenError::Expired));
    }

    #[test]
    fn a_room_admin_token_lists_and_administers_one_room() {
        let k = keys();
        let c = k.verify(&k.room_admin("studio", 60)).unwrap();
        assert!(c.video.room_admin && c.video.room_list && !c.video.room_join);
        assert_eq!(c.video.room.as_deref(), Some("studio"));
        assert_eq!(c.role(), None);
    }

    #[test]
    fn only_the_desks_inputs_and_monitors_hear_past_a_publishers_limits() {
        let k = keys();
        let guest = k
            .verify(
                &k.participant("studio", "gregor", "", Role::Guest, 60)
                    .unwrap(),
            )
            .unwrap();
        assert!(!guest.video.recorder && !guest.video.hidden);

        let input = k
            .verify(&k.desk_input("studio", "desk-in-gregor-3"))
            .unwrap();
        assert!(input.video.recorder && input.video.hidden && input.video.room_join);
        assert_eq!(input.video.can_publish, Some(false));
        assert_eq!(input.video.can_subscribe, Some(true));
        assert_eq!(input.role(), None, "the desk is nobody at its own desk");

        let output = k
            .verify(&k.desk_output("studio", "desk-mm-gregor"))
            .unwrap();
        assert!(!output.video.recorder && !output.video.hidden);
        assert_eq!(output.video.can_publish, Some(true));
        assert_eq!(output.video.can_subscribe, Some(false));
        assert_eq!(output.sub, "desk-mm-gregor");

        let monitor = k
            .verify(&k.monitor("studio", "ear-gregor", "", 60).unwrap())
            .unwrap();
        assert!(monitor.video.recorder && !monitor.video.hidden);
        assert_eq!(monitor.role(), Some(Role::Listener));
        assert_eq!(monitor.video.can_publish, Some(false));
    }

    #[test]
    fn keys_hide_the_secret_when_printed() {
        assert!(!format!("{:?}", keys()).contains("test-only-secret"));
    }
}
