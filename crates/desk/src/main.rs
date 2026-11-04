// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use weavesignal_desk::config::Config;
use weavesignal_desk::pipeline::{Desk, ErrorScope, Outputs};
use weavesignal_desk::room::{self, RoomApi};

/// WeaveSignal's desk.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The settings file.
    #[arg(
        long,
        short,
        env = "WEAVESIGNAL_DESK_CONFIG",
        default_value = "desk.toml"
    )]
    config: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .flatten_event(true)
        .with_writer(std::io::stderr)
        .init();
    // A panic on any thread (a GStreamer streaming thread, the mixing thread) is the desk's
    // last word: say where, in the log and plainly on stderr, then let it end the process.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("?").to_owned();
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        tracing::error!(thread = %thread, at = %at, panic = %what, "the desk panicked");
        eprintln!("weavesignal-desk: panic on thread {thread} at {at}: {what}");
        default_hook(info);
        // A panic unwinds only its own thread: the mixing thread gone would leave every bus
        // silent with the process alive and the SRT link open, so nothing would restart it,
        // and a panic under the state lock poisons it for everyone (review 0025, D2). End
        // the process; its supervisor starts it again within seconds.
        std::process::abort();
    }));
    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("weavesignal-desk: {e}");
            return ExitCode::from(2);
        }
    };
    match run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(config: Config) -> anyhow::Result<()> {
    let desk = Desk::new(config, Outputs::ALL)?;
    if let Some(path) = &desk.config().control.socket {
        weavesignal_desk::control::listen(desk.clone(), path)?;
    }
    let main_loop = glib::MainLoop::new(None, false);
    // The fatal error, with the element it came from, for the last line the desk writes.
    let failed = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));

    let bus = desk.pipeline().bus().expect("a pipeline has a bus");
    let _watch = {
        let main_loop = main_loop.clone();
        let failed = failed.clone();
        let desk = desk.clone();
        bus.add_watch_local(move |_, msg| {
            use gst::MessageView;
            match msg.view() {
                MessageView::Error(e) => {
                    let src = msg.src().map(|s| s.path_string()).unwrap_or_default();
                    let (scope, repair) = desk.classify_error(msg);
                    match scope {
                        ErrorScope::Participant(id) => {
                            tracing::warn!(participant = %id, source = %src, error = %e.error(), debug = ?e.debug(), "one person's failure: taking down only what is theirs");
                        }
                        ErrorScope::Stale => {
                            tracing::debug!(source = %src, error = %e.error(), "an error from something already taken down");
                        }
                        ErrorScope::Fatal => {
                            tracing::error!(source = %src, error = %e.error(), debug = ?e.debug(), "pipeline error; stopping so the supervisor restarts the desk");
                            let line = format!(
                                "{src}: {} ({})",
                                e.error(),
                                e.debug().map(|d| d.to_string()).unwrap_or_default()
                            );
                            eprintln!("weavesignal-desk: fatal pipeline error from {line}");
                            failed.borrow_mut().get_or_insert(line);
                            main_loop.quit();
                        }
                    }
                    // Off the main loop: stopping a broken WebRTC element can take a while.
                    // Here instead if no thread can be had (a pids limit): never left broken.
                    if let Some(r) = repair {
                        let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(r)));
                        let (d, s) = (desk.clone(), shared.clone());
                        let spawned = std::thread::Builder::new()
                            .name("desk-repair".into())
                            .spawn(move || {
                                if let Some(r) = s.lock().ok().and_then(|mut g| g.take()) {
                                    d.repair(r);
                                }
                            });
                        if let Err(e) = spawned {
                            tracing::warn!(error = %e, "no thread for the repair; repairing on the main loop");
                            if let Some(r) = shared.lock().ok().and_then(|mut g| g.take()) {
                                desk.repair(r);
                            }
                        }
                    }
                }
                MessageView::Warning(w) => {
                    let src = msg.src().map(|s| s.path_string()).unwrap_or_default();
                    tracing::warn!(source = %src, warning = %w.error(), "pipeline warning");
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        })?
    };
    {
        // SIGINT and SIGTERM: quit the main loop from the handler's thread.
        let main_loop = main_loop.clone();
        ctrlc::set_handler(move || {
            tracing::info!("stopping");
            main_loop.quit();
        })?;
    }
    desk.pipeline().set_state(gst::State::Playing)?;
    tracing::info!("desk running");
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let c = desk.config();
        let api = RoomApi::new(&c.api_url(), c.keys(), &c.livekit.room, &c.desk.identity);
        let desk = desk.clone();
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("room-watch".into())
            .spawn(move || room::watch(desk, api, stop))?
    };
    main_loop.run();
    stop.store(true, Ordering::SeqCst);
    let _ = watcher.join();
    // Video first: its encoders (NVENC) must stop before the process ends.
    if let Some(v) = desk.video() {
        v.stop();
    }
    desk.pipeline().set_state(gst::State::Null)?;
    if let Some(line) = failed.borrow_mut().take() {
        anyhow::bail!("the desk stopped on a pipeline error from {line}");
    }
    Ok(())
}
