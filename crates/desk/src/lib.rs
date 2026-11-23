// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's desk: a hidden participant in the LiveKit room that mixes the programme and
//! a mix-minus for every participant, and sends the programme to Liquidsoap over SRT on
//! loopback (ADR-0003, ADR-0049). The same binary with `[desk] mode = "rehearsal"` mixes the
//! rehearsal studio (1.3): the returns and carts only, with no way to the air.

pub mod analysis;
pub mod carts;
pub mod census;
pub mod config;
pub mod control;
pub mod feed;
pub mod flv;
pub mod hls_sound;
pub mod matrix;
pub mod phone;
pub mod picture;
pub mod pipeline;
pub mod realtime;
pub mod room;
pub mod rtmp;
pub mod sources;
pub mod vban_out;
pub mod video;
pub mod vision;

use anyhow::Result;

/// A desk as the binary starts it: the desk with the outputs its mode allows, its control
/// socket, and, for a live desk only, the listener for hosts' own studios.
pub struct Launched {
    pub desk: pipeline::Desk,
    /// `None` on a rehearsal desk, and on a live desk with sources off or no password.
    pub sources: Option<sources::Sources>,
}

/// Build the desk the settings describe, as the binary does: the outputs come from
/// `[desk] mode` (a rehearsal desk gets [`pipeline::Outputs::REHEARSAL`]), never from the caller.
pub fn launch(config: config::Config) -> Result<Launched> {
    let outputs = config.desk.mode.outputs();
    let desk = pipeline::Desk::new(config, outputs)?;
    if let Some(path) = &desk.config().control.socket {
        control::listen(desk.clone(), path)?;
    }
    // Hosts in their own studios (ADR-0280), from the ingest over loopback.
    let sources = sources::Sources::start(
        &desk,
        &desk.config().sources,
        std::env::var(config::SOURCES_PASSWORD_ENV).ok(),
    );
    tracing::info!(mode = desk.desk_mode().as_str(), room = %desk.config().livekit.room, "desk built");
    Ok(Launched { desk, sources })
}
