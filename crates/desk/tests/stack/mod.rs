// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What the stack tests share: the loopback room from `tests/room/run-stack.sh`, people who
//! publish a tone from processes of their own (so their threads and CPU are not the desk's),
//! the desk's error handling as the binary runs it, and LiveKit's own participant list.

#![allow(dead_code)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::config::Config;
use weavesignal_desk::pipeline::{Desk, ErrorScope};
use weavesignal_proto::{Keys, Role};

/// Whether the stack tests run with video on (`WEAVESIGNAL_ROOM_VIDEO=1`): the desk's video
/// path, and a camera beside each person's voice. The room test's figures, with and without.
pub fn video_on() -> bool {
    std::env::var("WEAVESIGNAL_ROOM_VIDEO").is_ok_and(|v| v == "1")
}

/// The desk's `[video]` settings for a stack test, its HLS in the stack's run directory.
pub fn video_toml(room: &str) -> String {
    let base = std::env::var("WEAVESIGNAL_STACK_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("\n[video]\nenabled = true\n\n[video.hls]\ndir = \"{base}/hls-{room}\"\n")
}

/// A person's camera colour, from their tone.
pub fn colour_of(freq: f64) -> u32 {
    let h = (freq as u32).wrapping_mul(2_654_435_761);
    0xff00_0000 | (h & 0x00ff_ffff)
}

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run the stack test"))
}

pub struct Room {
    pub url: String,
    pub keys: Keys,
    pub name: String,
}

impl Room {
    /// A fresh room per test: LiveKit keeps departed identities briefly (spike 01).
    pub fn fresh(stem: &str) -> Room {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Room {
            url: env("WEAVESIGNAL_ROOM_LIVEKIT_URL"),
            keys: Keys {
                api_key: env("WEAVESIGNAL_ROOM_API_KEY"),
                api_secret: env("WEAVESIGNAL_ROOM_API_SECRET"),
            },
            name: format!("{stem}-{stamp}"),
        }
    }

    /// The desk's settings for this room; `extra` is appended TOML.
    pub fn config(&self, extra: &str) -> Config {
        let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
        let mut c = Config::from_toml(
            &format!(
                r#"
[livekit]
url = "{}"
api_key = "{}"
room = "{}"

[programme]
srt_port = {srt_port}

# Tones are measured by their pitch, and the delay's rebuild plays them a little slow.
[programme.delay]
target_ms = 0

# Tones are not speech: no input processing, so levels stay as sent.
[inputs.presenter]
[inputs.dj]
{extra}{video}
"#,
                self.url,
                self.keys.api_key,
                self.name,
                video = if video_on() && !extra.contains("[video]") {
                    video_toml(&self.name)
                } else {
                    String::new()
                }
            ),
            "stack test",
        )
        .unwrap();
        c.livekit.api_secret = self.keys.api_secret.clone();
        c
    }

    /// Every identity LiveKit lists in the studio room and in every booth (ADR-0193), the
    /// desk's own included.
    pub fn identities(&self) -> Vec<String> {
        let mut all = self.identities_in(&self.name);
        for booth in self.booths() {
            all.extend(self.identities_in(&booth));
        }
        all
    }

    /// The booths LiveKit has open for this studio room.
    fn booths(&self) -> Vec<String> {
        let api = format!(
            "{}/twirp/livekit.RoomService/ListRooms",
            self.url.replacen("ws://", "http://", 1)
        );
        let token = self.keys.room_admin(&self.name, 60);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .into();
        let Ok(mut res) = agent
            .post(&api)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(serde_json::json!({}))
        else {
            return Vec::new();
        };
        let body = res.body_mut().read_to_string().unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        v["rooms"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| r["name"].as_str())
                    .filter(|n| weavesignal_proto::booth_owner(&self.name, n).is_some())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One call to LiveKit's room service as an admin of `room`: the reply as JSON, and its
    /// status.
    pub fn twirp(
        &self,
        method: &str,
        room: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let api = format!(
            "{}/twirp/livekit.RoomService/{method}",
            self.url.replacen("ws://", "http://", 1)
        );
        let token = self.keys.room_admin(room, 60);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut res = agent
            .post(&api)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .expect("LiveKit's server API answers");
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        (status, serde_json::from_str(&text).unwrap_or_default())
    }

    /// Mute or unmute a person's audio in the studio room, as LiveKit's server API does it
    /// (a page's own mute does the same through its client).
    pub fn mute_audio(&self, identity: &str, muted: bool) {
        let (_, list) = self.twirp(
            "ListParticipants",
            &self.name,
            serde_json::json!({ "room": self.name }),
        );
        let track = list["participants"]
            .as_array()
            .and_then(|a| a.iter().find(|p| p["identity"] == identity))
            .and_then(|p| p["tracks"].as_array())
            .and_then(|t| t.iter().find(|t| t["type"] == "AUDIO"))
            .and_then(|t| t["sid"].as_str())
            .unwrap_or_else(|| panic!("{identity} publishes no audio track: {list}"))
            .to_owned();
        let (status, reply) = self.twirp(
            "MutePublishedTrack",
            &self.name,
            serde_json::json!({ "room": self.name, "identity": identity, "track_sid": track, "muted": muted }),
        );
        assert_eq!(status, 200, "MutePublishedTrack answered {status}: {reply}");
    }

    /// Every identity LiveKit lists in one room.
    pub fn identities_in(&self, room: &str) -> Vec<String> {
        let api = format!(
            "{}/twirp/livekit.RoomService/ListParticipants",
            self.url.replacen("ws://", "http://", 1)
        );
        let token = self.keys.room_admin(room, 60);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .into();
        let Ok(mut res) = agent
            .post(&api)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(serde_json::json!({ "room": room }))
        else {
            return Vec::new();
        };
        let body = res.body_mut().read_to_string().unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        v["participants"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["identity"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A person publishing a steady tone into the room from a `gst-launch-1.0` of their own,
/// with a publish-only token carrying their role, as the control plane issues it to a native
/// publisher (ADR-0090). Killed when dropped, as a link that drops.
pub struct Person {
    pub identity: String,
    pub freq: f64,
    child: Child,
}

impl Person {
    pub fn join(room: &Room, identity: &str, role: Role, freq: f64) -> Person {
        // A caller joins their own booth (ADR-0193), anyone else the studio room.
        let in_room = weavesignal_proto::room_for(&room.name, role, identity);
        let token = room
            .keys
            .participant_with(&in_room, identity, identity, role, 600, false)
            .unwrap();
        Self::launch(
            room,
            &in_room,
            identity,
            freq,
            &token,
            video_on().then(|| colour_of(freq)),
        )
    }

    /// A person with a camera as well: a solid colour, 640x360 at 20 fps, VP8 from the
    /// sink's own encoder, as a browser would send it.
    pub fn join_with_camera(
        room: &Room,
        identity: &str,
        role: Role,
        freq: f64,
        argb: u32,
    ) -> Person {
        let in_room = weavesignal_proto::room_for(&room.name, role, identity);
        let token = room
            .keys
            .participant_with(&in_room, identity, identity, role, 600, false)
            .unwrap();
        Self::launch(room, &in_room, identity, freq, &token, Some(argb))
    }

    /// A person whose microphone carries digital silence (every sample zero) the whole time:
    /// a muted or quiet page, whose packets still arrive (issue 13).
    pub fn join_silent(room: &Room, identity: &str, role: Role) -> Person {
        let in_room = weavesignal_proto::room_for(&room.name, role, identity);
        let token = room
            .keys
            .participant_with(&in_room, identity, identity, role, 600, false)
            .unwrap();
        Self::launch(room, &in_room, identity, 0.0, &token, None)
    }

    /// `freq` 0: digital silence rather than a tone.
    fn launch(
        room: &Room,
        in_room: &str,
        identity: &str,
        freq: f64,
        token: &str,
        camera: Option<u32>,
    ) -> Person {
        let mut args: Vec<String> = [
            "-q",
            "-e",
            "livekitwebrtcsink",
            "name=s",
            "video-caps=video/x-vp8",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.extend([
            format!("signaller::ws-url={}", room.url),
            format!("signaller::auth-token={token}"),
            format!("signaller::room-name={in_room}"),
        ]);
        let sound = if freq > 0.0 {
            format!("freq={freq}")
        } else {
            "wave=silence".to_owned()
        };
        args.extend(
            [
                "audiotestsrc",
                "is-live=true",
                &sound,
                "volume=0.1",
                "samplesperbuffer=480",
                "!",
                "audio/x-raw,format=S16LE,rate=48000,channels=2",
                "!",
                "queue",
                "!",
                "s.",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        if let Some(argb) = camera {
            args.extend(
                [
                    "videotestsrc",
                    "is-live=true",
                    "pattern=solid-color",
                    &format!("foreground-color={argb}"),
                    "!",
                    "video/x-raw,width=640,height=360,framerate=20/1",
                    "!",
                    "queue",
                    "!",
                    "s.",
                ]
                .iter()
                .map(|s| s.to_string()),
            );
        }
        let child = Command::new("gst-launch-1.0")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("gst-launch-1.0 runs (the desk image has it)");
        Person {
            identity: identity.to_owned(),
            freq,
            child,
        }
    }
}

impl Person {
    /// Someone who only listens, to one producer's track (a mix-minus, say), with a role in
    /// their token: what the staging driver's ears were, and the desk took them for people.
    /// `in_room`: where the producer is (a caller's booth for their mix-minus, ADR-0193).
    pub fn ear(room: &Room, in_room: &str, identity: &str, role: Role, producer: &str) -> Person {
        let token = room
            .keys
            .participant_with(in_room, identity, identity, role, 600, true)
            .unwrap();
        let child = Command::new("gst-launch-1.0")
            .args([
                "-q",
                "-e",
                "livekitwebrtcsrc",
                "name=s",
                &format!("signaller::ws-url={}", room.url),
                &format!("signaller::auth-token={token}"),
                &format!("signaller::room-name={in_room}"),
                &format!("signaller::producer-peer-id={producer}"),
                "s.",
                "!",
                "queue",
                "!",
                "fakesink",
                "sync=false",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("gst-launch-1.0 runs (the desk image has it)");
        Person {
            identity: identity.to_owned(),
            freq: 0.0,
            child,
        }
    }

    /// Leave the room properly: an interrupt, so `gst-launch-1.0` stops the sink, which says
    /// goodbye to LiveKit. A killed process is a dropped link instead, which LiveKit notices
    /// only after its own timeout (about 20 s).
    pub fn leave(mut self) {
        let _ = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Person {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Handle every error waiting on the desk's bus as the binary does; panic on a fatal one.
/// Returns the people whose errors were contained.
pub fn pump_errors(desk: &Desk) -> Vec<String> {
    let bus = desk.pipeline().bus().unwrap();
    let mut contained = Vec::new();
    while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
        let gst::MessageView::Error(e) = msg.view() else {
            continue;
        };
        let src = msg
            .src()
            .map(|s| s.path_string().to_string())
            .unwrap_or_default();
        match desk.handle_error(&msg) {
            ErrorScope::Participant(id) => {
                println!("contained ({id}): {src}: {}", e.error());
                contained.push(id);
            }
            ErrorScope::Stale => println!("stale: {src}: {}", e.error()),
            ErrorScope::Fatal => panic!("desk error: {src}: {} ({:?})", e.error(), e.debug()),
        }
    }
    contained
}

/// Wait up to `limit` for `f`, handling the desk's errors meanwhile; how long it took.
pub fn wait_for(desk: &Desk, limit: Duration, mut f: impl FnMut() -> bool) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        pump_errors(desk);
        if f() {
            return Some(t0.elapsed());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Run for `d`, handling the desk's errors.
pub fn run_for(desk: &Desk, d: Duration) -> Vec<String> {
    let t0 = Instant::now();
    let mut contained = Vec::new();
    while t0.elapsed() < d {
        contained.extend(pump_errors(desk));
        std::thread::sleep(Duration::from_millis(50));
    }
    contained
}

/// How many of `people` the desk has linked (their voice reaches it).
pub fn linked(desk: &Desk, people: &[&str]) -> usize {
    desk.status()
        .iter()
        .filter(|s| s.linked && people.contains(&s.identity.as_str()))
        .count()
}

/// Write a FLAC tone into `dir` as the media worker would leave it (a UUID name): `secs` of
/// `freq`. Returns the file's name.
pub fn flac(dir: &std::path::Path, freq: f64, secs: f64) -> String {
    gst::init().unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!(
        "{:08x}-0000-4000-8000-{:012x}.flac",
        std::process::id(),
        stamp & 0xffff_ffff_ffff
    );
    let line = format!(
        "audiotestsrc num-buffers={} samplesperbuffer=480 freq={freq} volume=0.3 ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! flacenc ! filesink location={}",
        (secs * 100.0).round() as u32,
        dir.join(&name).display()
    );
    let p = gst::parse::launch(&line).unwrap();
    p.set_state(gst::State::Playing).unwrap();
    let msg = p
        .bus()
        .unwrap()
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(60),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the FLAC was written");
    assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
    p.set_state(gst::State::Null).unwrap();
    name
}
