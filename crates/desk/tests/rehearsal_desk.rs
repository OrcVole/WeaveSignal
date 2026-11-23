// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A rehearsal desk built as the binary builds it (`weavesignal_desk::launch`), without a room:
//! it has no programme output, no video path, no hosts' sources listener, and its control socket
//! takes only the desk's keys (the rehearsal build plan's section 1.7, step R2). And the live
//! desk's arrivals (R3): a guest, and anyone whose token's standing-by time is ahead, start off
//! air; a rehearsal desk starts everyone by their role. `just desk-check` runs these; the stack's
//! canary (`just test-rehearsal`) proves the same against real LiveKit and Liquidsoap.

use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::config::{Config, DeskMode, SourcesIn};
use weavesignal_desk::control::{self, Reply};
use weavesignal_desk::pipeline::{Desk, Outputs, SourceFacts};
use weavesignal_desk::sources::Sources;
use weavesignal_proto::Role;

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn rehearsal_config() -> Config {
    let mut c = Config::from_toml(
        r#"
[livekit]
url = "ws://127.0.0.1:7"
api_key = "devkey"
room = "studio-rehearsal"

[desk]
mode = "rehearsal"
identity = "desk-rehearsal"
workers = 2
realtime = false
"#,
        "rehearsal test",
    )
    .unwrap();
    c.livekit.api_secret = "test-only-secret-test-only-secret-0".into();
    c
}

fn live_config(srt_port: u16) -> Config {
    let mut c = Config::from_toml(
        &format!(
            r#"
[livekit]
url = "ws://127.0.0.1:7"
api_key = "devkey"
room = "studio"

[programme]
srt_port = {srt_port}

[desk]
workers = 2
realtime = false
"#
        ),
        "live test",
    )
    .unwrap();
    c.livekit.api_secret = "test-only-secret-test-only-secret-0".into();
    c
}

/// Every element in the desk's pipeline, by factory.
fn factories(desk: &Desk) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = desk.pipeline().iterate_recurse();
    while let Ok(Some(e)) = it.next() {
        if let Some(f) = e.factory() {
            out.push(f.name().to_string());
        }
        out.push(format!("name:{}", e.name()));
    }
    out
}

fn reply(desk: &Desk, line: &str) -> serde_json::Value {
    serde_json::to_value(control::handle(desk, line)).unwrap()
}

#[test]
fn a_rehearsal_desk_is_built_with_no_way_to_the_air() {
    gst::init().unwrap();
    // The live desk, as a control: built the same way, it has its SRT listener.
    let live = weavesignal_desk::launch(live_config(free_udp_port())).unwrap();
    assert_eq!(live.desk.desk_mode(), DeskMode::Live);
    assert!(
        factories(&live.desk).iter().any(|f| f == "srtsink"),
        "the live desk has its programme output, so the check below can see one"
    );
    drop(live);

    let r = weavesignal_desk::launch(rehearsal_config()).unwrap();
    let desk = &r.desk;
    assert_eq!(desk.desk_mode(), DeskMode::Rehearsal);
    let made = factories(desk);
    for never in [
        "srtsink",
        "matroskamux",
        "cmafmux",
        "hlscmafsink",
        "flvmux",
        "rtmp2sink",
        "rtmpsink",
    ] {
        assert!(
            !made.iter().any(|f| f == never),
            "a rehearsal desk made `{never}`: {made:?}"
        );
    }
    assert!(
        !made
            .iter()
            .any(|n| n.starts_with("name:programme-") || n.contains("feed")),
        "no programme output and no playout feed: {made:?}"
    );
    assert!(desk.video().is_none(), "no video path");
    assert!(r.sources.is_none(), "no hosts' sources listener");
    assert!(!desk.config().sources.enabled);
}

#[test]
fn a_rehearsal_desk_cannot_be_built_or_driven_toward_the_air() {
    gst::init().unwrap();
    // Asked for the programme output by its caller: refused where the desk is built.
    assert!(Desk::new(rehearsal_config(), Outputs::ALL).is_err());
    let mut c = rehearsal_config();
    c.video.enabled = true;
    assert!(Desk::new(c, Outputs::REHEARSAL).is_err());

    let desk = Desk::new(rehearsal_config(), Outputs::NONE).unwrap();
    // Hosts' sources: refused at the desk, and the listener never opens even when given a
    // password and a port turned on by hand.
    assert!(
        desk.add_source(
            "source-k3yid0ab",
            "Studio B",
            SourceFacts {
                way: "icecast".into(),
                format: "mp3".into(),
                key: "k3yid0ab".into(),
                show: "steeped".into(),
                since: std::time::Instant::now(),
                clock: Default::default(),
            }
        )
        .is_err()
    );
    let port = free_tcp_port();
    let forced = SourcesIn {
        enabled: true,
        port,
        ..SourcesIn::default()
    };
    assert!(Sources::start(&desk, &forced, Some("a-password".into())).is_none());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))).is_err(),
        "nothing listens on the sources port"
    );
    // A live desk given the same does listen: the refusal above is the mode's.
    let live = Desk::new(live_config(free_udp_port()), Outputs::NONE).unwrap();
    let port = free_tcp_port();
    let s = Sources::start(
        &live,
        &SourcesIn {
            enabled: true,
            port,
            ..SourcesIn::default()
        },
        Some("a-password".into()),
    );
    assert!(s.is_some());
    let t0 = std::time::Instant::now();
    while TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))).is_err() {
        assert!(
            t0.elapsed().as_secs() < 5,
            "the live desk's sources listener"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    drop(s);
    // Monitor links and VBAN out: refused.
    assert!(desk.open_monitor("k3yid0ab").is_err());
    assert!(desk.set_kit("s", None, Vec::new()).is_err());

    // The control socket: the status says which desk it is, the keys and carts are taken, and
    // everything that reaches beyond the room is refused with the code `rehearsal`.
    let st = reply(&desk, r#"{"op":"status"}"#);
    assert_eq!(st["ok"], true);
    assert_eq!(st["desk_mode"], "rehearsal");
    assert_eq!(
        reply(&live, r#"{"op":"status"}"#)["desk_mode"],
        "live",
        "the live desk says so too"
    );
    for line in [
        r#"{"op":"dump"}"#,
        r#"{"op":"cart_plays","after":0}"#,
        r#"{"op":"mode","mode":"auto"}"#,
        r#"{"op":"picture","mode":"slate"}"#,
        r#"{"op":"now_playing","title":"T"}"#,
        r#"{"op":"caption","text":"x"}"#,
        r#"{"op":"thumbs"}"#,
        r#"{"op":"rtmp","set":"s","destinations":[]}"#,
        r#"{"op":"kit","set":"s","bind":"127.0.0.1","outs":[]}"#,
        r#"{"op":"phone_words","person":"phone-x","file":"f.flac","version":"1"}"#,
        r#"{"op":"phone_admit","person":"phone-x"}"#,
        r#"{"op":"phone_hang_up","person":"phone-x"}"#,
    ] {
        let r = reply(&desk, line);
        assert_eq!(r["ok"], false, "{line}");
        assert_eq!(r["code"], "rehearsal", "{line}: {r}");
        // The same request on the live desk is not refused for being live.
        assert_ne!(reply(&live, line)["code"], "rehearsal", "{line}");
    }
    // The keys reach the desk (nobody is there, so it says so, but it is not refused).
    let r = reply(&desk, r#"{"op":"on_air","identity":"gregor","on":true}"#);
    assert_ne!(r["code"], "rehearsal");
    let r = reply(&desk, r#"{"op":"cart_stop_all"}"#);
    assert_eq!(r["ok"], true);
    assert!(matches!(
        control::handle(&desk, r#"{"op":"cue","identity":"x","on":true}"#),
        Reply::Error { code: None, .. }
    ));
}

#[test]
fn guests_and_moved_people_arrive_standing_by_at_the_live_desk_only() {
    gst::init().unwrap();
    let now = 1_700_000_000_000;
    let on = |desk: &Desk, id: &str| {
        desk.status()
            .into_iter()
            .find(|p| p.identity == id)
            .map(|p| p.on_air)
            .unwrap()
    };
    let live = Desk::new(live_config(free_udp_port()), Outputs::NONE).unwrap();
    // A presenter moved from rehearsal, the time ahead: standing by.
    live.add_person_arriving("eilidh", Role::Presenter, None, Some(now + 60_000), now)
        .unwrap();
    assert!(!on(&live, "eilidh"));
    // The time past: as the role says.
    live.add_person_arriving("finlay", Role::Presenter, None, Some(now - 1), now)
        .unwrap();
    assert!(on(&live, "finlay"));
    // No attribute: as 1.2 for a presenter.
    live.add_person_arriving("hamish", Role::Presenter, None, None, now)
        .unwrap();
    assert!(on(&live, "hamish"));
    // A guest, attribute or not (question 9): standing by. In 1.2 a guest went on air here.
    live.add_person_arriving("gregor", Role::Guest, None, None, now)
        .unwrap();
    assert!(!on(&live, "gregor"));
    // Back within the leave grace: kept as they were, not decided afresh.
    assert!(live.set_on_air("eilidh", true));
    live.mark_away("eilidh");
    live.add_person_arriving("eilidh", Role::Presenter, None, Some(now + 60_000), now)
        .unwrap();
    assert!(on(&live, "eilidh"), "a press made before stays");
    // A live desk restarted before the time: the same first sight, standing by again.
    drop(live);
    let restarted = Desk::new(live_config(free_udp_port()), Outputs::NONE).unwrap();
    restarted
        .add_person_arriving("eilidh", Role::Presenter, None, Some(now + 60_000), now)
        .unwrap();
    assert!(!on(&restarted, "eilidh"));
    // The press puts them on.
    assert!(restarted.set_on_air("eilidh", true));
    assert!(on(&restarted, "eilidh"));

    // The rehearsal desk starts everyone by their role: rehearsing guests are in its mix.
    let rehearsal = Desk::new(rehearsal_config(), Outputs::NONE).unwrap();
    rehearsal
        .add_person_arriving("kirsty", Role::Guest, None, None, now)
        .unwrap();
    assert!(on(&rehearsal, "kirsty"));
}
