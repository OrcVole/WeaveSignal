// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Structured logging: JSON lines by default, filtered by the settings or `RUST_LOG`.

use tracing_subscriber::EnvFilter;

use crate::config::{LogConfig, LogFormat};

pub fn init(log: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(log.filter.as_deref().unwrap_or("info")))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    let result = match log.format {
        LogFormat::Json => builder.json().flatten_event(true).try_init(),
        LogFormat::Pretty => builder.try_init(),
    };
    // A second init (in tests) keeps the first subscriber.
    let _ = result;
}
