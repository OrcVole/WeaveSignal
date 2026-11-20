// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's studio app: the desk, the screener, the cart wall, the calendar and the
//! presenter kit in one window (ADR-0061). M2's first slice: the shell and desk B's layout.
//! The native publishing path (ADR-0013) and the presenter kit (ADR-0042) live here later; the
//! bridge (Phase 17g) carries a host's own software to the station from here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod bridge_local;
mod bridge_up;
mod calendar;
mod open;
mod station;
mod stations;
mod update;

use tauri::Manager;

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
        // The bridge (Phase 17g): the host's own software on loopback, carried to the station
        // over one WebSocket; the show key and the local password stay in the keychain.
        .manage(bridge::Bridge::new())
        // `calendar_save` writes a calendar file the page read from the station into the
        // Downloads folder (Subscribe…'s Download .ics, ADR-0281): the webview has no download
        // the page can count on.
        // `calendar_read` reads a calendar file at a path the person pasted or dropped (Import a
        // calendar…): an `.ics` of at most 1 MB that begins as a calendar, and no other file.
        // `open_link` hands the Solo DJ's own microphone link to the person's browser (ADR-0274):
        // a web address only, never through a shell.
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
            station::stations_list,
            station::stations_switch,
            station::stations_rename,
            station::stations_note,
            station::stations_sign_out,
            station::stations_remove,
            update::update_check,
            update::update_download,
            update::update_install,
            update::update_rollback,
            open::open_link,
            calendar::calendar_save,
            calendar::calendar_read,
            bridge::bridge_view,
            bridge::bridge_save,
            bridge::bridge_start,
            bridge::bridge_stop,
            bridge::bridge_new_password,
            bridge::bridge_forget,
            bridge::window_close_keep,
            bridge::window_close_stop,
        ])
        // Closing the window stops the bridge, and with it the host's software's way to the
        // station: while it runs the first close is a question the page puts (keyboard first:
        // Keep the window open has the focus, Esc keeps it); a second close while it is open
        // goes through, so a page that cannot answer never holds the window.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let bridge = window.state::<bridge::Bridge>();
                if bridge.on_close_requested() == bridge::OnClose::Ask {
                    api.prevent_close();
                    use tauri::Emitter;
                    // Should the page not hear it, the question stays open and the next press
                    // closes.
                    let _ = window.emit(bridge::CLOSE_ASKED, ());
                }
            }
        })
        // A window larger than the screen's free space (a 1280 by 800 screen less its task bar,
        // the 1.0.4 run on Windows) hid dialogs' buttons under the task bar: it opens maximised.
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                fit_to_screen(&window);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the studio app could not start");
}

/// Whether a window of this outer size is larger than the monitor's work area (the screen
/// less its task bar or panels) in either direction, all in physical pixels.
fn larger_than(window: (u32, u32), work_area: (u32, u32)) -> bool {
    window.0 > work_area.0 || window.1 > work_area.1
}

/// Maximise the window when it would not fit the screen's free space; a screen with room
/// keeps the window's own size. A monitor that cannot be read leaves it as it is.
fn fit_to_screen(window: &tauri::WebviewWindow) {
    let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) else {
        return;
    };
    let area = monitor.work_area().size;
    if larger_than((size.width, size.height), (area.width, area.height)) {
        let _ = window.maximize();
    }
}

#[cfg(test)]
mod tests {
    use super::larger_than;

    #[test]
    fn a_window_larger_than_the_free_space_is_maximised() {
        // 1440 by 900 asked, on a 1280 by 800 screen with a 48 px task bar.
        assert!(larger_than((1440, 900), (1280, 752)));
        // The minimum height on a 1366 by 768 laptop's free space.
        assert!(larger_than((1440, 900), (1366, 720)));
        assert!(!larger_than((1280, 600), (1366, 720)));
        // A large screen keeps the asked size.
        assert!(!larger_than((1440, 900), (2560, 1392)));
        assert!(!larger_than((1440, 900), (1440, 900)));
    }
}
