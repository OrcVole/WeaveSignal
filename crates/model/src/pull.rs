// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A pulled source's address (ADR-0291, migration 0042): the parse and the guard live in
//! [`weavesignal_proto::pull`], because the ingest checks every address it connects to as well,
//! after it resolves the name, and the ingest does not link this crate.

pub use weavesignal_proto::pull::*;
