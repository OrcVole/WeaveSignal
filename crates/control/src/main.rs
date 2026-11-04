// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use weavesignal_control::config::{self, Config};
use weavesignal_control::{ports, telemetry};
use weavesignal_model::StationId;

/// WeaveSignal's control plane.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The settings file.
    #[arg(
        long,
        short,
        env = "WEAVESIGNAL_CONFIG",
        default_value = "control.toml"
    )]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the control plane (the default).
    Serve,
    /// Check that every configured listening port is free, as setup does (ADR-0075).
    CheckPorts,
    /// Print sample settings with a fresh station identifier.
    SampleConfig,
    /// Print the API's OpenAPI description as JSON (ADR-0195), or with `--markdown` the
    /// documentation's reference page made from it. Reads no settings.
    Openapi {
        /// The reference page for the product documentation instead of the JSON.
        #[arg(long)]
        markdown: bool,
    },
    /// The media worker: an untrusted audio file on standard input, clean FLAC on standard
    /// output, its report on standard error. The control plane runs it for each upload.
    MediaWorker {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// The cart wall's offline jobs.
    Carts {
        #[command(subcommand)]
        command: CartsCommand,
    },
    /// Print a short-lived room token for one person (needs WEAVESIGNAL_LIVEKIT_API_SECRET).
    RoomToken {
        /// Their identity in the room.
        #[arg(long)]
        identity: String,
        /// presenter, dj, guest, caller, screener, producer or listener (a listener's token only
        /// listens). A caller's token signed here carries no consent to record: the desk takes
        /// them, but the control plane will not send them to the desk or put them on air.
        #[arg(long)]
        role: weavesignal_proto::Role,
        /// The name others see.
        #[arg(long, default_value = "")]
        name: String,
        /// Seconds the token can be used to join; at most the station's room.token_ttl_s.
        #[arg(long)]
        ttl_s: Option<u64>,
    },
}

#[derive(Subcommand)]
enum CartsCommand {
    /// Load a cart pack (a directory with carts.json, as assets/carts-demo) into the
    /// station's banks, each file through the media worker.
    LoadPack {
        /// The pack's directory.
        dir: PathBuf,
    },
}

fn main() -> ExitCode {
    // The media worker reads no settings and restricts itself before anything else runs.
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("media-worker") {
        return weavesignal_media::worker::run(&raw[2..]);
    }
    let cli = Cli::parse();
    if let Some(Command::SampleConfig) = cli.command {
        print!("{}", config::sample(StationId::new_random()));
        return ExitCode::SUCCESS;
    }
    if let Some(Command::Openapi { markdown }) = cli.command {
        let doc = weavesignal_control::openapi::document();
        if markdown {
            print!("{}", weavesignal_control::openapi::reference_page(&doc));
        } else {
            println!("{}", weavesignal_control::openapi::json(&doc));
        }
        return ExitCode::SUCCESS;
    }
    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("weavesignal-control: {e}");
            return ExitCode::from(2);
        }
    };
    match cli.command {
        Some(Command::CheckPorts) => check_ports(&config),
        Some(Command::Carts {
            command: CartsCommand::LoadPack { dir },
        }) => {
            telemetry::init(&config.log);
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("weavesignal-control: cannot start the runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(weavesignal_control::load_pack(config, &dir)) {
                Ok(n) => {
                    println!("{n} carts loaded from {}", dir.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("weavesignal-control: {e:#}");
                    ExitCode::from(2)
                }
            }
        }
        Some(Command::RoomToken {
            identity,
            role,
            name,
            ttl_s,
        }) => room_token(&config, &identity, role, &name, ttl_s),
        _ => {
            telemetry::init(&config.log);
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("weavesignal-control: cannot start the runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(weavesignal_control::serve(config)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e:#}");
                    eprintln!("weavesignal-control: {e:#}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn room_token(
    config: &Config,
    identity: &str,
    role: weavesignal_proto::Role,
    name: &str,
    ttl_s: Option<u64>,
) -> ExitCode {
    let Some(room) = weavesignal_control::room_state(config) else {
        eprintln!(
            "weavesignal-control: set {} to issue room tokens",
            config::LIVEKIT_SECRET_ENV
        );
        return ExitCode::from(2);
    };
    if let Err(e) = weavesignal_proto::check_identity(identity, &room.config.desk_identity) {
        eprintln!("weavesignal-control: {e}");
        return ExitCode::from(2);
    }
    let ttl = ttl_s
        .unwrap_or(room.config.token_ttl_s)
        .min(room.config.token_ttl_s);
    match room
        .keys
        .participant(&room.config.name, identity, name, role, ttl)
    {
        Ok(t) => {
            println!("{t}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("weavesignal-control: {e}");
            ExitCode::from(2)
        }
    }
}

fn check_ports(config: &Config) -> ExitCode {
    let clashes = ports::check_all(&config.ports, config.listen.internal);
    for (spec, port) in config.ports.iter() {
        let state = if clashes.iter().any(|c| c.key == spec.key) {
            "TAKEN"
        } else {
            "free"
        };
        println!(
            "{state:5}  {port:>5}/{}  {}  ({})",
            spec.proto, spec.key, spec.what
        );
    }
    if clashes.is_empty() {
        println!("every port is free");
        ExitCode::SUCCESS
    } else {
        for clash in &clashes {
            eprintln!("weavesignal-control: {clash}");
        }
        ExitCode::from(1)
    }
}
