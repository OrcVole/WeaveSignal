// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// `sqlx::migrate!` embeds the migrations at compile time, and cargo does not know a new file in
// `migrations/` should rebuild this crate: a merge that added 0026 left the control plane's
// tests on 0025 against a database already at 0026 (VersionMissing, 2026-11-10). sqlx's
// documented fix.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
