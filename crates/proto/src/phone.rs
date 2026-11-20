// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Phone booths (Phase 17f, design `2026-11-20-17f-phone-line-design.md`): a caller on the
//! telephone reaches the station through LiveKit's SIP service, whose dispatch rule makes a room
//! per call, as a browser caller has a booth (ADR-0193, [`crate::booth`]). Only the desk joins
//! beside them.
//!
//! **The room's name is LiveKit's, not ours.** An Individual dispatch rule with `room_prefix`
//! `<studio>-phone` and `hide_phone_number` names the room `<studio>-phone_<tail>_<random>`
//! (`EvaluateDispatchRule` in livekit/protocol's `sip/sip.go`, read from `main` on 2026-11-20;
//! the spike checks it at livekit-sip v1.25.0):
//!
//! - `<tail>` is the last four characters of the INVITE's From user when it has more than four,
//!   **one character when it has one to four, and nothing when it is empty**. A withheld call's
//!   From user is a word (`anonymous`), so its tail is letters (`mous`). The tail is for the
//!   screener's eyes only; who is withheld is read from the identity (the model's `caller_id`),
//!   never from the room.
//! - `<random>` is LiveKit's `guid.New("")`: 12 characters of shortuuid's alphabet.
//!
//! So the name is parsed **from the right**: the random part follows the last `_`, and the tail
//! is whatever lies between the prefix and it. The person's key at the desk and in the studio is
//! `phone-<random>`: unique per call and inside [`crate::check_identity`]'s alphabet. LiveKit's
//! own identity for the caller (`sip_` and 16 hex digits) is only ever the ban key's input.

/// What every phone booth's name starts with, after the studio room's name (the dispatch rule's
/// `room_prefix`; LiveKit adds the `_`).
const PHONE: &str = "-phone";
/// What a phone caller's person key starts with.
pub const PHONE_PERSON_PREFIX: &str = "phone-";
/// The participant attribute the dispatch rule sets on a phone caller: `phone`.
pub const LINE_ATTRIBUTE: &str = "weavesignal.line";
/// [`LINE_ATTRIBUTE`]'s value for a telephone caller.
pub const LINE_PHONE: &str = "phone";
/// What LiveKit's SIP identities start with.
pub const SIP_IDENTITY_PREFIX: &str = "sip_";

/// The dispatch rule's `room_prefix` for a studio room: `studio-phone`.
pub fn phone_room_prefix(studio_room: &str) -> String {
    format!("{studio_room}{PHONE}")
}

/// What every phone booth of a studio room starts with: `studio-phone_`.
pub fn phone_booth_prefix(studio_room: &str) -> String {
    format!("{studio_room}{PHONE}_")
}

/// One phone booth, read from its room's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhoneBooth<'a> {
    /// The From user's last characters as LiveKit kept them (at most four; maybe none).
    pub tail: &'a str,
    /// LiveKit's random part: what makes the call's person key.
    pub random: &'a str,
}

impl PhoneBooth<'_> {
    /// The caller's person key at the desk and in the studio: `phone-<random>`.
    pub fn person(&self) -> String {
        format!("{PHONE_PERSON_PREFIX}{}", self.random)
    }
}

/// The phone booth `room` is, if it is one of this studio's.
pub fn phone_booth<'a>(studio_room: &str, room: &'a str) -> Option<PhoneBooth<'a>> {
    let rest = room.strip_prefix(&phone_booth_prefix(studio_room))?;
    let (tail, random) = rest.rsplit_once('_')?;
    let random_ok = !random.is_empty()
        && random.len() <= 32
        && random.chars().all(|c| c.is_ascii_alphanumeric());
    (random_ok && tail.chars().count() <= 4).then_some(PhoneBooth { tail, random })
}

/// Whether a person key is a phone caller's.
pub fn is_phone_person(identity: &str) -> bool {
    identity
        .strip_prefix(PHONE_PERSON_PREFIX)
        .is_some_and(|r| !r.is_empty())
}

/// Whether a participant identity is one LiveKit's SIP service made.
pub fn is_sip_identity(identity: &str) -> bool {
    identity
        .strip_prefix(SIP_IDENTITY_PREFIX)
        .is_some_and(|r| !r.is_empty())
}

/// One key from a phone booth: LiveKit's `SipDTMF` as it arrives in a `DataPacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SipKey {
    /// The RFC 4733 event code; a packet without one is 0 (protobuf leaves a zero out, so the
    /// `0` key has no code at all: the spike, 2026-11-20).
    pub code: u32,
    /// The key as text (`"5"`, `"#"`), when the packet carries it.
    pub digit: Option<String>,
}

/// The key in a LiveKit `DataPacket`, if it is a `sip_dtmf` one (field 6; `SipDTMF` has `code` 3
/// and `digit` 4, `livekit_models.proto` at the pinned protocol commit). Anything else (user
/// data, transcriptions, a packet that is not protobuf at all) is `None`: the desk reads only
/// keys from a phone booth's data channel, and never fails on what it cannot read.
pub fn sip_key(packet: &[u8]) -> Option<SipKey> {
    let mut found = None;
    for (num, value) in fields(packet)? {
        if let (6, Field::Bytes(inner)) = (num, value) {
            let mut key = SipKey {
                code: 0,
                digit: None,
            };
            for (n, v) in fields(inner)? {
                match (n, v) {
                    (3, Field::Varint(c)) => key.code = u32::try_from(c).ok()?,
                    (4, Field::Bytes(d)) => {
                        key.digit = std::str::from_utf8(d).ok().map(str::to_owned);
                    }
                    _ => {}
                }
            }
            found = Some(key);
        }
    }
    found
}

/// A protobuf field's value, as far as [`sip_key`] needs it.
enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut n = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*i)?;
        *i += 1;
        n |= u64::from(c & 0x7f) << shift;
        if c < 0x80 {
            return Some(n);
        }
    }
    None
}

/// The top-level fields of a protobuf message; `None` if it is not one.
fn fields(b: &[u8]) -> Option<Vec<(u64, Field<'_>)>> {
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let key = varint(b, &mut i)?;
        let value = match key & 7 {
            0 => Field::Varint(varint(b, &mut i)?),
            2 => {
                let n = usize::try_from(varint(b, &mut i)?).ok()?;
                let end = i.checked_add(n)?;
                let v = b.get(i..end)?;
                i = end;
                Field::Bytes(v)
            }
            1 => {
                i = i.checked_add(8).filter(|e| *e <= b.len())?;
                Field::Fixed
            }
            5 => {
                i = i.checked_add(4).filter(|e| *e <= b.len())?;
                Field::Fixed
            }
            _ => return None,
        };
        out.push((key >> 3, value));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{booth, check_identity, monitor_room};

    /// A `DataPacket` with a `sip_dtmf` field, as protobuf encodes it.
    fn dtmf_packet(code: Option<u8>, digit: &str) -> Vec<u8> {
        let mut inner = Vec::new();
        if let Some(c) = code {
            inner.extend([3 << 3, c]);
        }
        inner.extend([(4 << 3) | 2, digit.len() as u8]);
        inner.extend(digit.as_bytes());
        // `kind` (field 1) first, as LiveKit sends it, then field 6.
        let mut p = vec![1 << 3, 0, (6 << 3) | 2, inner.len() as u8];
        p.extend(inner);
        p
    }

    #[test]
    fn a_key_is_read_from_a_data_packet() {
        assert_eq!(
            sip_key(&dtmf_packet(Some(5), "5")),
            Some(SipKey {
                code: 5,
                digit: Some("5".into())
            })
        );
        assert_eq!(sip_key(&dtmf_packet(Some(11), "#")).unwrap().code, 11);
        // The `0` key: protobuf leaves the zero code out.
        assert_eq!(
            sip_key(&dtmf_packet(None, "0")),
            Some(SipKey {
                code: 0,
                digit: Some("0".into())
            })
        );
    }

    #[test]
    fn other_packets_are_not_keys() {
        // User data (field 2) and an empty packet.
        assert_eq!(sip_key(&[(2 << 3) | 2, 1, 0x41]), None);
        assert_eq!(sip_key(&[]), None);
        // Not protobuf: a truncated length, a bad wire type, a varint that never ends.
        assert_eq!(sip_key(&[(6 << 3) | 2, 9, 1]), None);
        assert_eq!(sip_key(&[0x0f]), None);
        assert_eq!(sip_key(&[0x08, 0xff, 0xff, 0xff]), None);
        assert_eq!(sip_key(b"{\"key\": 5}"), None);
    }

    #[test]
    fn the_rule_prefix_and_the_booths() {
        assert_eq!(phone_room_prefix("studio"), "studio-phone");
        assert_eq!(phone_booth_prefix("studio"), "studio-phone_");
    }

    #[test]
    fn a_booth_is_read_from_the_right() {
        let b = phone_booth("studio", "studio-phone_4471_Xk3pQ9mZr2Ta").unwrap();
        assert_eq!(b.tail, "4471");
        assert_eq!(b.random, "Xk3pQ9mZr2Ta");
        assert_eq!(b.person(), "phone-Xk3pQ9mZr2Ta");
        assert_eq!(check_identity(&b.person(), "desk"), Ok(()));
    }

    #[test]
    fn short_empty_and_lettered_tails() {
        // A From user of one to four characters keeps one; an empty one keeps none.
        let b = phone_booth("studio", "studio-phone_7_Xk3pQ9mZr2Ta").unwrap();
        assert_eq!(b.tail, "7");
        let b = phone_booth("studio", "studio-phone__Xk3pQ9mZr2Ta").unwrap();
        assert_eq!(b.tail, "");
        // Withheld: `anonymous` keeps its last four letters.
        let b = phone_booth("studio", "studio-phone_mous_Xk3pQ9mZr2Ta").unwrap();
        assert_eq!(b.tail, "mous");
        // A tail with an underscore in it is still split at the last one.
        let b = phone_booth("studio", "studio-phone_a_b_Xk3pQ9mZr2Ta").unwrap();
        assert_eq!((b.tail, b.random), ("a_b", "Xk3pQ9mZr2Ta"));
    }

    #[test]
    fn other_rooms_are_not_phone_booths() {
        for room in [
            "studio",
            "studio-phone",
            "studio-phone_",
            "studio-phone_4471",
            "studio-phone_4471_",
            "studio-phone_4471_bad!",
            "studio-phone_12345_Xk3pQ9mZr2Ta",
            "other-phone_4471_Xk3pQ9mZr2Ta",
            &booth("studio", "caller-0123456789ab"),
            &booth("studio", "phone_4471_Xk3pQ9mZr2Ta"),
            &monitor_room("studio", "k1"),
            // Another studio called `studio-phone`: its booths and monitors are not ours.
            &booth("studio-phone", "caller-0123456789ab"),
            &monitor_room("studio-phone", "k1"),
        ] {
            assert_eq!(phone_booth("studio", room), None, "{room}");
        }
    }

    #[test]
    fn person_and_sip_identities() {
        assert!(is_phone_person("phone-Xk3pQ9mZr2Ta"));
        assert!(!is_phone_person("phone-"));
        assert!(!is_phone_person("caller-0123"));
        assert!(is_sip_identity("sip_0123456789abcdef"));
        assert!(!is_sip_identity("sip_"));
        assert!(!is_sip_identity("caller-sip_1"));
    }
}
