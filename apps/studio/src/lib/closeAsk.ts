// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Closing the window while the bridge runs (Phase 17g): closing stops the bridge, and with it the
// host's software's way to the station, so the app asks first. The question comes from Rust (the
// window's close request, `close-asked`); the answer goes back as one of two commands, and Rust
// closes the window itself (src-tauri/src/bridge.rs). A second close while the question is open
// goes through, so a page that cannot answer never holds the window.

/** The event Rust sends when the window would close with the bridge running. */
export const CLOSE_ASKED = 'close-asked';

export const CLOSE_WORDS = {
  title: 'The bridge is running',
  body: 'Closing the studio app stops the bridge: your software loses its server on this computer, and your source leaves the desk at the station.',
  keep: 'Keep the window open',
  stop: 'Stop the bridge and close',
};

async function call(cmd: string): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke(cmd);
}

/** Keep the window open: a later close asks again. */
export const keepOpen = () => call('window_close_keep');
/** Stop the bridge, then close the window. */
export const stopAndClose = () => call('window_close_stop');

/** Hear Rust's question; the returned function stops listening. */
export async function onCloseAsked(ask: () => void): Promise<() => void> {
  const { listen } = await import('@tauri-apps/api/event');
  return listen(CLOSE_ASKED, () => ask());
}
