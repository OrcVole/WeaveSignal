// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's studio app: the desk, the screener, the cart wall, the calendar and the
//! presenter kit in one window (ADR-0061). M2's first slice: the shell and desk B's layout.
//! The native publishing path (ADR-0013) and the presenter kit (ADR-0042) live here later.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod station;
mod update;

fn main() {
    // `--version` answers before any window or toolkit starts, so an installed package can be
    // checked on a machine without a display (`just package-desktop-test`). A Windows release
    // build has no console, so there it prints nothing.
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("weavesignal-studio {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    tauri::Builder::default()
        // Updates are signed (ADR-0015): the public key and the endpoint are given when the
        // packages are built (`just package-desktop`); a build without them never updates.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The window's page is not the station's origin, and a station behind a login proxy
        // answers a browser's CORS preflight with a redirect: requests go out from Rust, which
        // also keeps the station's key out of the page (ADR-0121).
        .manage(station::Stations::new())
        // The update screen's commands (ADR-0140); the page is not granted the plugin's own.
        .manage(update::Updates::default())
        .invoke_handler(tauri::generate_handler![
            station::station_info,
            station::station_check,
            station::station_save,
            station::station_forget,
            station::station_sign_in,
            station::station_code_ask,
            station::station_code_finish,
            station::station_sign_out,
            station::station_last,
            station::station_fetch,
            station::station_stop,
            update::update_check,
            update::update_download,
            update::update_install,
            update::update_rollback,
        ])
        .run(tauri::generate_context!())
        .expect("the studio app could not start");
}
