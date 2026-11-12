// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The desk's control socket: a Unix socket, readable and writable by the desk's user only,
//! that takes one JSON request per line and answers one JSON line. The control plane (and,
//! until the studio app does it, an operator with `socat`) puts people on air, holds
//! talkback, and reads the desk's status. A socket, not a port: nothing to expose.
//!
//! ```text
//! {"op": "status"}
//! {"op": "on_air", "identity": "callum", "on": true}
//! {"op": "mute", "identity": "gregor", "on": true}
//! {"op": "cue", "identity": "finlay", "on": true}
//! {"op": "talk", "identity": "moira", "on": true}
//! {"op": "talk_to", "identity": "moira", "to": "callum", "on": true}
//! {"op": "dump"}
//! {"op": "cart_fire", "cart": "<uuid>", "file": "<uuid>.flac", "gain_db": -7.0,
//!  "mode": "one_shot", "cue": false, "duration_ms": 3000}
//! {"op": "cart_stop", "cart": "<uuid>"}
//! {"op": "cart_stop_all"}                (or with "cue": true for the cued only, false for air)
//! {"op": "cart_plays", "after": 41}
//! {"op": "picture", "mode": "auto"}       (or "slate", or "person" with "identity")
//! {"op": "now_playing", "station": "…", "show": "…", "title": "…", "artist": "…"}
//! {"op": "caption", "text": "…", "ms": 8000}   (text null or "" takes it down)
//! {"op": "thumbs"}                       (a small JPEG of each camera, base64, by identity)
//! {"op": "mode", "mode": "auto"}           (or "live")
//! {"op": "rtmp", "set": "<id>", "destinations": [{"name": "rumble", "url": "rtmp://…/live",
//!  "key": "…", "profile": "compatible", "enabled": true}]}
//! ```
//!
//! `rtmp` (ADR-0230, `rtmp-out`) replaces RTMP out's destinations, with video on; the status's
//! `video.rtmp` reports each one (never its key) and the `set` it holds, so the control plane
//! sends the set again to a desk that restarted.
//!
//! `mode` is the station's mode as playout runs it (ADR-0045, ADR-0183): `live`, the desk first,
//! or `auto`, automation only. The desk starts in live mode; the control plane compares the
//! status's `mode` with the station's setting every few seconds and sends `mode` when they
//! differ, so a desk that restarted is told again. In auto mode the video's sound is playout's
//! feed whoever is live (ADR-0172). It is taken with video off too, and answers `invalid` for
//! any other word.
//!
//! With video on (ADR-0009), the status carries `video`: the picture's mode and shot, whose
//! cameras have a picture, each rung's encoder, whether HLS and the room picture run, and the
//! frame counts; each person carries `camera`. `picture` chooses who chooses the picture:
//! `auto` follows the audio, `slate` holds the slate (desk B's F7), `person` holds one
//! camera. `now_playing` sets the slate's text (ADR-0010). Both answer an error with video off.
//! With video on the status also carries `sound`, the video stream's sound (ADR-0172):
//! `{"source": "desk" | "playout", "live": …, "feed_connected": …, "feed_connections": …}`.
//!
//! `status` answers with everyone the desk knows, whether playout is connected, the station's
//! mode, the profanity delay and how the mixing pass keeps time:
//! `{"ok": true, "people": [...], "playout": {"connected": true, "for_ms": 5120}, "mode": "live",
//! "delay": {"delay_ms": 8000, "target_ms": 8000, "rebuilding": false, "dumps": 0,
//! "dumped_ms_ago": null}, "mix": {"blocks": 9000, "late": 0, "max_pass_us": 410,
//! "realtime": false}}`. `dump` empties the delay (listeners jump to the live programme) and
//! answers with the delay as it is then. It is allowed while the delay rebuilds, and refused
//! only when nothing is held (no delay, or right after a dump).
//! The status also carries `carts`: what is sounding, whether beds are dipped, and
//! `play_seq`, the number of the last play event (ADR-0117).
//!
//! `cart_fire` answers `{"ok": true, "pool": "shot", "player": 3, "play": 17}`, or an error
//! with a `code`: `invalid`, `missing` (no such file in the cart directory), `busy` (every
//! player of the pool is sounding) or `decode`. `cart_stop` and `cart_stop_all` fade carts
//! out over 300 ms and answer how many (`stopped`). `cart_plays` answers the play events after
//! a number, oldest first, for the per-play log.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::carts::{CartMode, CartsStatus, Fire, Pool};
use crate::hls_sound::StationMode;
use crate::pipeline::{DelayStatus, Desk, HlsSoundStatus, MixStats, PersonStatus, PlayoutLink};
use crate::video::{NowPlaying, VideoStatus};
use crate::vision::PictureMode;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status,
    OnAir {
        identity: String,
        on: bool,
    },
    Mute {
        identity: String,
        on: bool,
    },
    Cue {
        identity: String,
        on: bool,
    },
    Talk {
        identity: String,
        on: bool,
    },
    TalkTo {
        identity: String,
        to: String,
        on: bool,
    },
    Dump,
    CartFire {
        cart: String,
        file: String,
        gain_db: f32,
        mode: CartMode,
        #[serde(default)]
        cue: bool,
        #[serde(default)]
        duration_ms: Option<u64>,
    },
    CartStop {
        cart: String,
    },
    CartStopAll {
        #[serde(default)]
        cue: Option<bool>,
    },
    CartPlays {
        #[serde(default)]
        after: u64,
    },
    Picture {
        mode: String,
        #[serde(default)]
        identity: Option<String>,
    },
    Caption {
        #[serde(default)]
        text: Option<String>,
        #[serde(default = "caption_ms")]
        ms: u32,
    },
    /// A small JPEG of each camera with a picture, in base64 (video on).
    Thumbs,
    NowPlaying {
        #[serde(default)]
        station: Option<String>,
        #[serde(default)]
        show: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        artist: Option<String>,
    },
    Mode {
        mode: String,
    },
    /// RTMP out's destinations, with their keys (`rtmp-out`).
    Rtmp {
        set: String,
        destinations: Vec<crate::rtmp::Destination>,
    },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Reply {
    Status {
        ok: bool,
        people: Vec<PersonStatus>,
        playout: PlayoutLink,
        /// The station's mode, as the control plane last said (live until it does).
        mode: StationMode,
        delay: DelayStatus,
        mix: MixStats,
        carts: CartsStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        video: Option<Box<VideoStatus>>,
        /// The video stream's sound (video on): the desk's or playout's (ADR-0172).
        #[serde(skip_serializing_if = "Option::is_none")]
        sound: Option<HlsSoundStatus>,
    },
    Dumped {
        ok: bool,
        delay: DelayStatus,
    },
    Fired {
        ok: bool,
        pool: Pool,
        player: u8,
        play: u64,
    },
    Stopped {
        ok: bool,
        stopped: usize,
    },
    Plays {
        ok: bool,
        plays: Vec<crate::carts::PlayEvent>,
    },
    Thumbs {
        ok: bool,
        thumbs: std::collections::BTreeMap<String, String>,
    },
    Done {
        ok: bool,
    },
    Error {
        ok: bool,
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
    },
}

pub fn handle(desk: &Desk, line: &str) -> Reply {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Reply::Error {
                ok: false,
                code: None,
                error: format!("not a request: {e}"),
            };
        }
    };
    let done = |known: bool, id: &str| {
        if known {
            Reply::Done { ok: true }
        } else {
            Reply::Error {
                ok: false,
                code: None,
                error: format!("`{id}` is not at the desk"),
            }
        }
    };
    match req {
        Request::Status => Reply::Status {
            ok: true,
            people: desk.status(),
            playout: desk.playout(),
            mode: desk.mode(),
            delay: desk.delay(),
            mix: desk.mix_stats(),
            carts: desk.carts(),
            video: desk.video().map(|v| Box::new(v.status())),
            sound: desk.video().map(|_| desk.hls_sound()),
        },
        Request::Picture { mode, identity } => {
            let Some(v) = desk.video() else {
                return video_off();
            };
            let mode = match (mode.as_str(), identity) {
                ("auto", _) => PictureMode::Auto,
                ("slate", _) => PictureMode::Slate,
                ("person", Some(identity)) if desk.person(&identity).is_some() => {
                    PictureMode::Person { identity }
                }
                ("person", Some(identity)) => {
                    return Reply::Error {
                        ok: false,
                        code: None,
                        error: format!("`{identity}` is not at the desk"),
                    };
                }
                _ => {
                    return Reply::Error {
                        ok: false,
                        code: Some("invalid"),
                        error: "mode is auto, slate, or person with an identity".into(),
                    };
                }
            };
            v.set_mode(mode);
            Reply::Done { ok: true }
        }
        Request::Thumbs => {
            let Some(v) = desk.video() else {
                return video_off();
            };
            match v.thumbs() {
                Ok(thumbs) => Reply::Thumbs { ok: true, thumbs },
                Err(e) => Reply::Error {
                    ok: false,
                    code: Some("thumbs"),
                    error: format!("cannot make the thumbnails: {e:#}"),
                },
            }
        }
        Request::Caption { text, ms } => {
            let Some(v) = desk.video() else {
                return video_off();
            };
            v.set_caption(text.as_deref(), ms.min(60_000));
            Reply::Done { ok: true }
        }
        Request::NowPlaying {
            station,
            show,
            title,
            artist,
        } => {
            let Some(v) = desk.video() else {
                return video_off();
            };
            v.set_now_playing(NowPlaying {
                station,
                show,
                title,
                artist,
            });
            Reply::Done { ok: true }
        }
        Request::Rtmp { set, destinations } => {
            let Some(v) = desk.video() else {
                return video_off();
            };
            match v.rtmp().set_destinations(&set, destinations) {
                Ok(()) => Reply::Done { ok: true },
                Err(e) => Reply::Error {
                    ok: false,
                    code: Some("invalid"),
                    error: e,
                },
            }
        }
        Request::Mode { mode } => match StationMode::parse(&mode) {
            Some(m) => {
                desk.set_mode(m);
                Reply::Done { ok: true }
            }
            None => Reply::Error {
                ok: false,
                code: Some("invalid"),
                error: format!("`{mode}` is not a mode: live or auto"),
            },
        },
        Request::Dump => {
            if desk.dump() {
                Reply::Dumped {
                    ok: true,
                    delay: desk.delay(),
                }
            } else {
                let d = desk.delay();
                Reply::Error {
                    ok: false,
                    code: None,
                    error: if d.state.target_ms == 0 {
                        "there is no delay to dump".into()
                    } else {
                        "the delay holds nothing to dump".into()
                    },
                }
            }
        }
        Request::CartFire {
            cart,
            file,
            gain_db,
            mode,
            cue,
            duration_ms,
        } => {
            let fire = Fire {
                cart,
                file,
                gain_db,
                mode,
                cue,
                duration_ms,
            };
            match desk.fire_cart(&fire) {
                Ok(f) => Reply::Fired {
                    ok: true,
                    pool: f.pool,
                    player: f.player,
                    play: f.play,
                },
                Err(e) => Reply::Error {
                    ok: false,
                    code: Some(e.code()),
                    error: e.to_string(),
                },
            }
        }
        Request::CartStop { cart } => Reply::Stopped {
            ok: true,
            stopped: desk.stop_cart(&cart),
        },
        Request::CartStopAll { cue } => Reply::Stopped {
            ok: true,
            stopped: desk.stop_carts(cue),
        },
        Request::CartPlays { after } => Reply::Plays {
            ok: true,
            plays: desk.cart_plays(after),
        },
        Request::OnAir { identity, on } => done(desk.set_on_air(&identity, on), &identity),
        Request::Mute { identity, on } => done(desk.set_mute(&identity, on), &identity),
        Request::Cue { identity, on } => done(desk.set_cue(&identity, on), &identity),
        Request::Talk { identity, on } => done(desk.set_talk(&identity, on), &identity),
        Request::TalkTo { identity, to, on } => {
            if desk.set_talk_to(&identity, &to, on) {
                Reply::Done { ok: true }
            } else {
                Reply::Error {
                    ok: false,
                    code: None,
                    error: format!("`{identity}` cannot talk to `{to}`: not both at the desk"),
                }
            }
        }
    }
}

fn caption_ms() -> u32 {
    8_000
}

fn video_off() -> Reply {
    Reply::Error {
        ok: false,
        code: Some("video_off"),
        error: "video is off at this station".into(),
    }
}

fn serve_one(desk: &Desk, stream: UnixStream) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = handle(desk, &line);
        let mut text = serde_json::to_string(&reply).expect("replies serialise");
        text.push('\n');
        out.write_all(text.as_bytes())?;
    }
    Ok(())
}

/// Listen on `path` (replacing a stale socket) and serve each connection on its own thread.
pub fn listen(desk: Desk, path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("cannot remove the stale socket {}", path.display()))?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("cannot listen on {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    tracing::info!(socket = %path.display(), "control socket listening");
    std::thread::Builder::new()
        .name("desk-control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let desk = desk.clone();
                let _ = std::thread::Builder::new()
                    .name("desk-control-conn".into())
                    .spawn(move || {
                        if let Err(e) = serve_one(&desk, stream) {
                            tracing::debug!(error = %e, "control connection ended");
                        }
                    });
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_parse() {
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"status"}"#).unwrap(),
            Request::Status
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"on_air","identity":"callum","on":true}"#)
                .unwrap(),
            Request::OnAir {
                identity: "callum".into(),
                on: true
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(
                r#"{"op":"talk_to","identity":"moira","to":"callum","on":true}"#
            )
            .unwrap(),
            Request::TalkTo {
                identity: "moira".into(),
                to: "callum".into(),
                on: true
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"cue","identity":"f","on":false}"#).unwrap(),
            Request::Cue {
                identity: "f".into(),
                on: false
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"dump"}"#).unwrap(),
            Request::Dump
        );
        assert!(
            serde_json::from_str::<Request>(r#"{"op":"talk","identity":"m","on":true,"x":1}"#)
                .is_err()
        );
        assert_eq!(
            serde_json::from_str::<Request>(
                r#"{"op":"cart_fire","cart":"c","file":"f.flac","gain_db":-7.0,"mode":"bed"}"#
            )
            .unwrap(),
            Request::CartFire {
                cart: "c".into(),
                file: "f.flac".into(),
                gain_db: -7.0,
                mode: CartMode::Bed,
                cue: false,
                duration_ms: None
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"cart_stop_all","cue":true}"#).unwrap(),
            Request::CartStopAll { cue: Some(true) }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"cart_plays"}"#).unwrap(),
            Request::CartPlays { after: 0 }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"picture","mode":"person","identity":"a"}"#)
                .unwrap(),
            Request::Picture {
                mode: "person".into(),
                identity: Some("a".into())
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"thumbs"}"#).unwrap(),
            Request::Thumbs
        );
        assert!(
            serde_json::from_str::<Request>(r#"{"op":"now_playing","title":"T","x":1}"#).is_err()
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"op":"mode","mode":"auto"}"#).unwrap(),
            Request::Mode {
                mode: "auto".into()
            }
        );
    }
}
