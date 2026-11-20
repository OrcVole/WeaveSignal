// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! `weavesignal-ingest run`: the supervisor, with the public listeners for hosts' own studios. `weavesignal-ingest decode --format …` is its worker, started by it for each
//! connection; it is not run by hand.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use weavesignal_ingest::supervisor::{Settings, run};

/// The control plane's bearer for the ingest's routes.
const TOKEN_ENV: &str = "WEAVESIGNAL_INGEST_TOKEN";
/// The desk's password for sources.
const DESK_PASSWORD_ENV: &str = "WEAVESIGNAL_DESK_SOURCES_PASSWORD";

#[derive(Parser)]
#[command(
    name = "weavesignal-ingest",
    version,
    about = "Hosts' own studios into the desk (ADR-0280)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Listen for hosts' software (the ports come from the station's rules).
    Run {
        /// The control plane, on loopback.
        #[arg(
            long,
            env = "WEAVESIGNAL_INGEST_CONTROL",
            default_value = "http://127.0.0.1:7310"
        )]
        control: String,
        /// The desk's sources listener (`desk.sources`), on loopback.
        #[arg(
            long,
            env = "WEAVESIGNAL_INGEST_DESK",
            default_value = "127.0.0.1:7318"
        )]
        desk: SocketAddr,
        /// Where the public listeners bind.
        #[arg(long, env = "WEAVESIGNAL_INGEST_BIND", default_value = "0.0.0.0")]
        bind: IpAddr,
        /// Where WHIP's requests arrive from the control plane (`ingest_whip`), on loopback.
        #[arg(
            long,
            env = "WEAVESIGNAL_INGEST_WHIP",
            default_value = "127.0.0.1:7319"
        )]
        whip_http: SocketAddr,
        /// The station's public IPv4 address, WHIP's one ICE candidate. Unset or empty, WHIP
        /// stays off.
        #[arg(long, env = "WEAVESIGNAL_INGEST_PUBLIC_IP")]
        public_ip: Option<String>,
    },
    /// The decoding worker (started by `run`, one per connection).
    Decode {
        #[arg(long)]
        format: String,
    },
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // The worker before anything else: it restricts itself before reading a byte, and starts
    // no runtime or threads.
    if args.get(1).map(String::as_str) == Some("decode") {
        return weavesignal_ingest::decode::run(&args[2..]);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let Command::Run {
        control,
        desk,
        bind,
        whip_http,
        public_ip,
    } = cli.command
    else {
        unreachable!("decode is handled above");
    };
    let need = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let (Some(token), Some(desk_password)) = (need(TOKEN_ENV), need(DESK_PASSWORD_ENV)) else {
        tracing::error!("{TOKEN_ENV} and {DESK_PASSWORD_ENV} must both be set");
        return ExitCode::from(2);
    };
    let public_ip = match public_ip
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(s) => match s.parse::<Ipv4Addr>() {
            Ok(ip) => Some(ip),
            // Icecast and SRT do not need it: WHIP stays off, and the rest runs.
            Err(_) => {
                tracing::warn!(address = %s, "WEAVESIGNAL_INGEST_PUBLIC_IP is not an IPv4 address: WHIP stays off");
                None
            }
        },
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "cannot find this program to start its workers");
            return ExitCode::FAILURE;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "cannot start");
            return ExitCode::FAILURE;
        }
    };
    let settings = Settings {
        control,
        token,
        desk,
        desk_password,
        bind,
        exe,
        whip_http,
        public_ip,
    };
    match rt.block_on(run(settings)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}
