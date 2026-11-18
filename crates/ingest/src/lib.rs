// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's ingest (ADR-0280, ADR-0284): a host's own software sends to the station over
//! Icecast or SRT with a show key; the ingest asks the control plane before it reads any media,
//! decodes in a sandboxed worker for each connection, and hands what stands by to the desk.
//!
//! - [`supervisor`]: the listeners, the control plane's answers, the limits, the desk link.
//! - [`decode`]: the worker, one per connection, and its readers by name.
//! - [`ts`]: the sound out of MPEG-TS.
//! - [`resample`]: any rate to 48 kHz.
//! - [`icecast`]: the source protocol's head.
//! - [`frame`]: what a worker writes to the supervisor.

pub mod decode;
pub mod frame;
pub mod icecast;
pub mod resample;
pub mod supervisor;
pub mod ts;
