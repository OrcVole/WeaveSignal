// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use weavesignal_chat::config::{self, Config};

/// WeaveSignal's live chat.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The settings file.
    #[arg(
        long,
        short,
        env = "WEAVESIGNAL_CHAT_CONFIG",
        default_value = "chat.toml"
    )]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the chat (the default).
    Serve,
    /// Print sample settings.
    SampleConfig,
}

fn main() -> ExitCode {
    // The media worker (ADR-0048 rule 2; a chatter's picture, ADR-0235) reads no settings and
    // restricts itself before anything else runs.
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("media-worker") {
        return weavesignal_media::worker::run(&raw[2..]);
    }
    let cli = Cli::parse();
    if let Some(Command::SampleConfig) = cli.command {
        print!("{}", config::SAMPLE);
        return ExitCode::SUCCESS;
    }
    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("weavesignal-chat: {e}");
            return ExitCode::from(2);
        }
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            config.log.filter.clone().unwrap_or_else(|| "info".into()),
        )
    });
    if config.log.format == "pretty" {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("weavesignal-chat: cannot start the runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(weavesignal_chat::serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            eprintln!("weavesignal-chat: {e:#}");
            ExitCode::from(2)
        }
    }
}
