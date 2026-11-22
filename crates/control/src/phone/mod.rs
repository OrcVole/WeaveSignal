// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The telephone line (Phase 17f; design `research/2026-11-20-17f-phone-line-design.md` in the
//! project folder): LiveKit's SIP service against a trunk; a phone caller hears the consent
//! words, presses a key, and only then joins the queue as a browser caller does. Off by default,
//! and off until a trunk is set.
//!
//! - [`config`]: `[phone]`, the settings and their checks.
//! - [`words`]: what the caller hears first (the station's recording, the shipped default, or
//!   nothing) and its version.
//! - [`gate`]: the state machine from a call appearing to the queue, with keys as RFC 4733
//!   events.
//! - [`limits`]: bans by ban key, calls per number, lines at once.
//! - [`line`]: the gates at run time, one per phone booth, fed from each status poll.
//! - [`trunk`]: the inbound trunk and dispatch rule as LiveKit's SIP API takes them.
//! - [`sync`]: keeps both in LiveKit while the line is on, and deletes them when it is off.
//! - [`api`]: the line's routes.

pub mod api;
pub mod config;
pub mod gate;
pub mod limits;
pub mod line;
pub mod sync;
pub mod trunk;
pub mod words;
