// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone line's inbound trunk and dispatch rule, as LiveKit's SIP API takes them (17f,
//! module 7): Twirp JSON bodies for `livekit.SIP/CreateSIPInboundTrunk` and
//! `CreateSIPDispatchRule` (field names from `livekit_sip.proto` at the protocol commit
//! livekit-sip v1.25.0 pins, `e6dd7c1a6bf6`; read 2026-11-20).
//!
//! The station makes both again at every start, from the settings, so nothing in Redis needs to
//! survive (design section 5): the trunk and rule this station made carry its name
//! ([`OWNER_NAME`]) and are deleted first; ones another tool made are left alone. Only the
//! comparison and the bodies are here; the calls are the hub's.

use serde_json::{Value, json};
use weavesignal_proto::phone::{LINE_ATTRIBUTE, LINE_PHONE, phone_room_prefix};

use super::config::PhoneConfig;

/// The name the station gives its trunk and its rule, to find its own again.
pub const OWNER_NAME: &str = "weavesignal-phone-line";

/// `CreateSIPInboundTrunkRequest`: the station's numbers, the provider's addresses, digest
/// auth when a user is set (the password from the environment, never the settings file), and
/// the time limits. The body holds the password: it is never logged.
pub fn inbound_trunk(c: &PhoneConfig, password: Option<&str>) -> Value {
    let mut trunk = json!({
        "name": OWNER_NAME,
        "numbers": c.numbers.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect::<Vec<_>>(),
        "allowed_addresses": c.allowed_addresses,
        "ringing_timeout": format!("{}s", c.ringing_timeout_s),
        "max_call_duration": format!("{}s", c.max_call_s),
        // A booth nobody answers (refused at the gate) ends busy, not as an error.
        "ringing_timeout_status": "SIP_STATUS_BUSY_HERE",
    });
    let user = c.auth_username.trim();
    if !user.is_empty() {
        trunk["auth_username"] = json!(user);
        trunk["auth_password"] = json!(password.unwrap_or_default());
    }
    json!({ "trunk": trunk })
}

/// `CreateSIPDispatchRuleRequest`: a phone booth per call (an Individual rule with the studio's
/// prefix), the number hidden (`hide_phone_number`: LiveKit keeps a hash and the last four
/// characters), and the attributes that make the caller a caller on the phone line.
pub fn dispatch_rule(studio_room: &str, trunk_id: &str) -> Value {
    json!({
        "dispatch_rule": {
            "name": OWNER_NAME,
            "trunk_ids": [trunk_id],
            "hide_phone_number": true,
            "rule": { "dispatch_rule_individual": { "room_prefix": phone_room_prefix(studio_room) } },
            "attributes": {
                (weavesignal_proto::ROLE_ATTRIBUTE): "caller",
                (LINE_ATTRIBUTE): LINE_PHONE,
            },
        }
    })
}

/// The ids of this station's own trunks or rules in a `ListSIPInboundTrunk` or
/// `ListSIPDispatchRule` answer (`items`, each with its id field and `name`), to delete them
/// before making them again.
pub fn own_ids(list: &Value, id_field: &str) -> Vec<String> {
    list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|i| i["name"] == OWNER_NAME)
                .filter_map(|i| i[id_field].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PhoneConfig {
        PhoneConfig {
            enabled: true,
            numbers: vec![" +441314960000 ".into(), String::new()],
            allowed_addresses: vec!["203.0.113.0/24".into()],
            public_ip: "198.51.100.7".into(),
            ..PhoneConfig::default()
        }
    }

    #[test]
    fn the_trunk_takes_the_stations_number_from_the_providers_addresses() {
        let t = inbound_trunk(&cfg(), None);
        assert_eq!(t["trunk"]["name"], OWNER_NAME);
        assert_eq!(t["trunk"]["numbers"], json!(["+441314960000"]));
        assert_eq!(t["trunk"]["allowed_addresses"], json!(["203.0.113.0/24"]));
        assert_eq!(t["trunk"]["ringing_timeout"], "30s");
        assert_eq!(t["trunk"]["max_call_duration"], "7200s");
        assert_eq!(t["trunk"]["ringing_timeout_status"], "SIP_STATUS_BUSY_HERE");
        // No digest user: no auth fields at all.
        assert!(t["trunk"].get("auth_username").is_none());
        assert!(t["trunk"].get("auth_password").is_none());
        let c = PhoneConfig {
            auth_username: "kettle".into(),
            ..cfg()
        };
        let t = inbound_trunk(&c, Some("from-the-environment"));
        assert_eq!(t["trunk"]["auth_username"], "kettle");
        assert_eq!(t["trunk"]["auth_password"], "from-the-environment");
    }

    #[test]
    fn the_rule_makes_a_hidden_number_phone_booth_per_call() {
        let r = dispatch_rule("studio", "ST_abc");
        let d = &r["dispatch_rule"];
        assert_eq!(d["hide_phone_number"], true);
        assert_eq!(d["trunk_ids"], json!(["ST_abc"]));
        assert_eq!(
            d["rule"]["dispatch_rule_individual"]["room_prefix"],
            "studio-phone"
        );
        assert_eq!(d["attributes"]["weavesignal.role"], "caller");
        assert_eq!(d["attributes"]["weavesignal.line"], "phone");
    }

    #[test]
    fn only_the_stations_own_are_found_to_replace() {
        let list = json!({"items": [
            {"sip_trunk_id": "ST_1", "name": OWNER_NAME},
            {"sip_trunk_id": "ST_2", "name": "someone else's"},
            {"sip_trunk_id": "ST_3", "name": OWNER_NAME},
        ]});
        assert_eq!(own_ids(&list, "sip_trunk_id"), ["ST_1", "ST_3"]);
        assert!(own_ids(&json!({}), "sip_trunk_id").is_empty());
    }
}
