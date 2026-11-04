// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Killing and restarting playout, then the desk, brings the programme back to listeners
//! without a hand on it (M1's done-when). Ignored unless the stack is up; `just test-playout`
//! starts Liquidsoap (playout and icecast.server) and runs this in the desk image, and
//! restarts the playout container when this test asks for it with a marker file.
//!
//! A presenter's tone is mixed by the desk and sent to playout; a listener follows
//! icecast.server, reconnecting whenever its stream ends, as a player should. Each recovery
//! is timed from the fault to the first second the listener decodes again.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use weavesignal_desk::analysis::{Tap, ratio_db, tone_power};
use weavesignal_desk::config::Config;
use weavesignal_desk::pipeline::{Desk, Outputs, RAW_CAPS};
use weavesignal_proto::Role;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run `just test-playout`"))
}

/// A desk with a presenter's tone at `freq` in its programme, sending to playout.
fn desk(srt_port: u16, freq: f64) -> Desk {
    let config = Config::from_toml(
        &format!(
            "[livekit]\nurl = \"ws://127.0.0.1:1\"\napi_key = \"unused\"\nroom = \"unused\"\n\n[programme]\nsrt_port = {srt_port}\n\n[programme.delay]\ntarget_ms = 0\n\n[inputs.presenter]\n"
        ),
        "playout test",
    )
    .unwrap();
    let desk = Desk::new(
        config,
        Outputs {
            srt: true,
            mix_minus_to_room: false,
        },
    )
    .unwrap();
    let p = desk.pipeline();
    let src = gst::ElementFactory::make("audiotestsrc")
        .property("is-live", true)
        .property("freq", freq)
        .property("volume", 0.3f64)
        .property("samplesperbuffer", 480i32)
        .build()
        .unwrap();
    let caps = gst::ElementFactory::make("capsfilter")
        .property("caps", RAW_CAPS.parse::<gst::Caps>().unwrap())
        .build()
        .unwrap();
    p.add_many([&src, &caps]).unwrap();
    src.link(&caps).unwrap();
    desk.add_person("presenter", Role::Presenter).unwrap();
    desk.route_pad("presenter", &caps.static_pad("src").unwrap())
        .unwrap();
    desk
}

/// A listener: a new connection each time, with no ICY metadata asked for (bench 02), and
/// no do-timestamp (a TIME segment leaves oggdemux without a seqnum; see tests/room.rs).
fn listener(url: &str, n: u32) -> (gst::Pipeline, Tap) {
    let p = gst::Pipeline::with_name(&format!("listener-{n}"));
    let src = gst::ElementFactory::make("souphttpsrc")
        .property("location", url)
        .property("is-live", true)
        .property("iradio-mode", false)
        .build()
        .unwrap();
    let demux = gst::ElementFactory::make("oggdemux").build().unwrap();
    let dec = gst::ElementFactory::make("opusdec").build().unwrap();
    p.add_many([&src, &demux, &dec]).unwrap();
    src.link(&demux).unwrap();
    let dec_weak = dec.downgrade();
    demux.connect_pad_added(move |_, pad| {
        if let Some(dec) = dec_weak.upgrade() {
            let _ = pad.link(&dec.static_pad("sink").unwrap());
        }
    });
    let tap = Tap::attach(&p, &dec, &format!("heard-{n}")).unwrap();
    (p, tap)
}

/// A listener that reconnects one second after its stream ends or fails.
struct Follower {
    url: String,
    n: u32,
    current: Option<(gst::Pipeline, Tap)>,
    retry_at: Instant,
}

impl Follower {
    fn new(url: String) -> Self {
        Follower {
            url,
            n: 0,
            current: None,
            retry_at: Instant::now(),
        }
    }

    /// Keep the listener connected; the seconds decoded on the current connection.
    fn tick(&mut self) -> f64 {
        if let Some((p, tap)) = &self.current {
            let bus = p.bus().unwrap();
            let mut ended = false;
            while let Some(msg) = bus.pop() {
                if matches!(
                    msg.view(),
                    gst::MessageView::Error(_) | gst::MessageView::Eos(_)
                ) {
                    ended = true;
                }
            }
            if ended {
                p.set_state(gst::State::Null).unwrap();
                self.current = None;
                self.retry_at = Instant::now() + Duration::from_secs(1);
                return 0.0;
            }
            return tap.seconds();
        }
        if Instant::now() >= self.retry_at {
            self.n += 1;
            let (p, tap) = listener(&self.url, self.n);
            p.set_state(gst::State::Playing).unwrap();
            self.current = Some((p, tap));
        }
        0.0
    }

    /// The current connection's recording, if it has one.
    fn tap(&self) -> Option<Tap> {
        self.current.as_ref().map(|(_, t)| t.clone())
    }

    fn stop(&mut self) {
        if let Some((p, _)) = self.current.take() {
            p.set_state(gst::State::Null).unwrap();
        }
    }
}

fn errors(p: &gst::Pipeline) {
    let bus = p.bus().unwrap();
    while let Some(msg) = bus.pop() {
        if let gst::MessageView::Error(e) = msg.view() {
            panic!(
                "desk error from {:?}: {}",
                msg.src().map(|s| s.path_string()),
                e.error()
            );
        }
    }
}

/// What the listener decoded after `since`: seconds, and the presenter's tone in it (dB over
/// silence).
fn heard_since(lis: &Follower, since: Instant, freq: f64) -> (f64, f64) {
    let Some(tap) = lis.tap() else {
        return (0.0, f64::MIN);
    };
    let s: Vec<i16> = tap
        .chunks()
        .into_iter()
        .filter(|(t, _)| *t >= since)
        .flat_map(|(_, s)| s)
        .collect();
    (
        s.len() as f64 / 48_000.0,
        ratio_db(tone_power(&s, freq), 1.0),
    )
}

/// Run until the listener has decoded two seconds carrying the tone at `freq` after `since`,
/// or `limit` passes; how long after `since` that was.
fn until_heard(
    desk: &gst::Pipeline,
    lis: &mut Follower,
    (since, freq): (Instant, f64),
    limit: Duration,
) -> Option<Duration> {
    while since.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(100));
        errors(desk);
        lis.tick();
        let (secs, level) = heard_since(lis, since, freq);
        if secs >= 2.0 && level > 60.0 {
            return Some(since.elapsed());
        }
    }
    None
}

/// Ask the stack script to stop playout, wait, and start it again; return when it has.
fn restart_playout(dir: &Path, desk: &gst::Pipeline, lis: &mut Follower) -> Instant {
    let done = dir.join("playout-restarted");
    let _ = std::fs::remove_file(&done);
    std::fs::write(dir.join("restart-playout"), b"").unwrap();
    let asked = Instant::now();
    while !done.exists() {
        assert!(
            asked.elapsed() < Duration::from_secs(60),
            "the stack script never restarted playout"
        );
        std::thread::sleep(Duration::from_millis(100));
        errors(desk);
        lis.tick();
    }
    asked
}

#[test]
#[ignore = "needs the loopback stack: just test-playout"]
fn the_programme_comes_back_after_playout_then_the_desk_restart() {
    gst::init().unwrap();
    let srt_port: u16 = env("WEAVESIGNAL_ROOM_SRT_PORT").parse().unwrap();
    let dir = PathBuf::from(env("WEAVESIGNAL_STACK_DIR"));
    let mut lis = Follower::new(env("WEAVESIGNAL_ROOM_LISTENER_URL"));
    let d = desk(srt_port, 1000.0);
    d.pipeline().set_state(gst::State::Playing).unwrap();
    let first = until_heard(
        d.pipeline(),
        &mut lis,
        (Instant::now(), 1000.0),
        Duration::from_secs(90),
    );
    let conns_before = lis.n;
    println!("listener first heard the programme after {first:?} (connection {conns_before})");
    assert!(first.is_some(), "the listener never heard the programme");

    // Playout dies and comes back.
    let fault = restart_playout(&dir, d.pipeline(), &mut lis);
    let after_playout = until_heard(
        d.pipeline(),
        &mut lis,
        (fault, 1000.0),
        Duration::from_secs(90),
    );
    let conns_playout = lis.n;
    println!(
        "after playout was stopped and restarted, the listener heard the programme again {after_playout:?} after the stop (connection {conns_playout})"
    );

    // The desk dies, and a new one starts two seconds later.
    let fault = Instant::now();
    d.pipeline().set_state(gst::State::Null).unwrap();
    drop(d);
    let gap = Instant::now() + Duration::from_secs(2);
    while Instant::now() < gap {
        std::thread::sleep(Duration::from_millis(100));
        lis.tick();
    }
    // The new desk's presenter is at another pitch, so audio the listener still had
    // buffered from the old desk is not taken for the new one.
    let d = desk(srt_port, 1500.0);
    d.pipeline().set_state(gst::State::Playing).unwrap();
    let after_desk = until_heard(
        d.pipeline(),
        &mut lis,
        (Instant::now(), 1500.0),
        Duration::from_secs(90),
    )
    .map(|_| fault.elapsed());
    let conns_desk = lis.n;
    println!(
        "after the desk was stopped and a new one started, the listener heard the programme {after_desk:?} after the stop (connection {conns_desk})"
    );
    lis.stop();
    d.pipeline().set_state(gst::State::Null).unwrap();

    // JSON with `null` for a figure never reached (it printed Rust's `Some(..)` before).
    let report = serde_json::to_string_pretty(&serde_json::json!({
        "first_heard_s": first.map(|d| d.as_secs_f64()),
        "back_after_playout_restart_s": after_playout.map(|d| d.as_secs_f64()),
        "back_after_desk_restart_s": after_desk.map(|d| d.as_secs_f64()),
        "listener_connections": [conns_before, conns_playout, conns_desk],
    }))
    .expect("the report serialises")
        + "\n";
    println!("{report}");
    if let Ok(path) = std::env::var("WEAVESIGNAL_ROOM_REPORT") {
        let _ = std::fs::write(path, &report);
    }
    assert!(
        after_playout.is_some(),
        "the programme did not come back after playout restarted"
    );
    assert!(
        after_desk.is_some(),
        "the programme did not come back after the desk restarted"
    );
}
