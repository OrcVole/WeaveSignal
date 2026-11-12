// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's desk: a hidden participant in the LiveKit room that mixes the programme and
//! a mix-minus for every participant, and sends the programme to Liquidsoap over SRT on
//! loopback (ADR-0003, ADR-0049).

pub mod analysis;
pub mod carts;
pub mod census;
pub mod config;
pub mod control;
pub mod feed;
pub mod flv;
pub mod hls_sound;
pub mod matrix;
pub mod picture;
pub mod pipeline;
pub mod realtime;
pub mod room;
pub mod rtmp;
pub mod video;
pub mod vision;
