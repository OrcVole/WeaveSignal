// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A page left open across a station update still names the old build's code files, and the
// update removed them: the first part loaded on demand (the call, the player's video) then fails
// with "Failed to fetch dynamically imported module" (the talk station, 2026-11-11). Vite says so
// with `vite:preloadError`; the page loads itself afresh, at most once a minute, so a file that
// is really missing cannot make it reload for ever.

const KEY = 'weavesignal.reloaded-for-stale-code';
const ONCE_MS = 60_000;

/** Whether to reload now, given when the page last reloaded for this (ms since the epoch, or
 * nothing stored). */
export function shouldReload(now: number, last: string | null): boolean {
  const t = last === null ? Number.NaN : Number(last);
  return !Number.isFinite(t) || now - t >= ONCE_MS || now < t;
}

export function reloadWhenStale(): void {
  globalThis.addEventListener?.('vite:preloadError', (event) => {
    let last: string | null = null;
    try {
      last = sessionStorage.getItem(KEY);
    } catch {
      // No storage (a private window): reload anyway, once per page.
    }
    if (!shouldReload(Date.now(), last)) return;
    try {
      sessionStorage.setItem(KEY, String(Date.now()));
    } catch {
      // As above.
    }
    event.preventDefault();
    location.reload();
  });
}
