// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's ingest (ADR-0280, ADR-0284): a host's own software sends to the station over
//! Icecast or SRT with a show key, or the station pulls a stream itself (PLAN 17b); the ingest
//! asks the control plane before it reads any media, decodes in a sandboxed worker for each
//! connection, and hands what stands by to the desk.
//!
//! - [`supervisor`]: the listeners, the control plane's answers, the limits, the desk link.
//! - [`bridge`]: the studio app's WebSocket, a third way in (Phase 17g).
//! - [`decode`]: the worker, one per connection, and its readers by name.
//! - [`ts`]: the sound out of MPEG-TS.
//! - [`icecast`]: the source protocol's head.
//! - [`vban_in`]: VBAN from a studio PC on the network, bound to a show key (Phase 17g).
//! - [`frame`]: what a worker writes to the supervisor.
//! - [`pull`]: a stream the station opens itself (HTTP, HTTPS, SRT), behind the address guard.
//! - [`hls`]: an HLS playlist followed, its segments fetched and paced.
//! - [`whip_http`]: WHIP's HTTP side (17d).
//! - [`opus_frames`]: a WHIP source's Opus payloads on their way to its worker.
//! - [`whip_rtc`]: a WHIP offer answered, ICE-lite, Opus only.
//! - [`whip_mux`]: one UDP socket for every WHIP session.
//! - [`whip`]: WHIP glued to the supervisor, as Icecast and SRT are.

pub mod bridge;
pub mod decode;
pub mod frame;
pub mod hls;
pub mod icecast;
pub mod opus_frames;
pub mod pull;
pub mod supervisor;
pub mod ts;
pub mod vban_in;
pub mod whip;
pub mod whip_http;
pub mod whip_mux;
pub mod whip_rtc;
