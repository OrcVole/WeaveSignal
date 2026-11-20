// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The bridge (Phase 17g): the studio app carries a host's own software (BUTT, Mixxx, OBS) to
//! the station over one outbound WebSocket on the station's public address, port 443, so the
//! station needs no source port and the host's network needs nothing but HTTPS.
//!
//! ```text
//! GET /api/v1/sources/bridge HTTP/1.1
//! Upgrade: websocket
//! Authorization: Basic <any user>:<key id>-<secret>      (the Icecast push's password)
//!
//! app → station   text    {"format":"mp3","name":"BUTT 1.45"}      first, within 10 s
//! app → station   binary  the host's stream, byte for byte          (never decoded in the app)
//! app → station   text    {"title":"Blue Peter — Theme"}           whenever the software says
//! station → app   text    {"state":"standing_by","message":"…"}    on every change
//! ```
//!
//! The station decodes as it does for push; the app is a byte relay. A reconnect sends the
//! stream's header pages again first (Ogg, FLAC), so the decoder can start mid-way.

use serde::{Deserialize, Serialize};

/// The route, on the station's public address.
pub const PATH: &str = "/api/v1/sources/bridge";
/// The formats the bridge carries, by the decoding worker's names.
pub const FORMATS: [&str; 5] = ["mp3", "aac", "ogg", "flac", "mpegts"];
/// The largest message either side sends.
pub const MAX_MESSAGE: usize = 256 * 1024;
/// The largest WebSocket frame.
pub const MAX_FRAME: usize = 64 * 1024;

/// The app's first message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// One of [`FORMATS`].
    pub format: String,
    /// The software, for the station's log (`BUTT 1.45`).
    #[serde(default)]
    pub name: String,
}

/// A title from the host's software.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Title {
    pub title: String,
}

/// Where the source stands, as the desk sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// `standing_by`, `test` or `refused`.
    pub state: String,
    #[serde(default)]
    pub message: String,
}

/// The worker's format for a name the app sent; `None` for anything else.
pub fn format(name: &str) -> Option<&'static str> {
    FORMATS.iter().copied().find(|f| *f == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_the_documented_json() {
        let h: Hello = serde_json::from_str(r#"{"format":"mp3","name":"BUTT"}"#).unwrap();
        assert_eq!(format(&h.format), Some("mp3"));
        assert!(serde_json::from_str::<Hello>(r#"{"format":"mp3","x":1}"#).is_err());
        assert_eq!(format("wma"), None);
        let s = State {
            state: "test".into(),
            message: "Outside your slot.".into(),
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"state":"test","message":"Outside your slot."}"#
        );
    }
}
