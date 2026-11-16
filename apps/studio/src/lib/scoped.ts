// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A preference this computer keeps for each station apart (ADR-0285): the cart bank the window
// last showed, the name typed into Settings. In the packaged app the key carries the station's
// id; in a browser the page's own address is the station, and the key is as it always was.
//
// The theme and the text size are the app's, not a station's, and do not come here.

import { savedStation } from './api.ts';

/** The id of the station an install from before the list carried over as (`stations.rs`). */
const FIRST = 's1';

/** The storage key for the station the window is on. */
export function stationKey(key: string): string {
  const id = savedStation()?.id;
  return id ? `${key}@${id}` : key;
}

/** Read a station's preference. The first station also finds what a build from before the
 *  list kept under the bare key, so nothing is lost on the update; no other station does. */
export function readScoped(key: string): string | null {
  try {
    const store = globalThis.localStorage;
    const own = store?.getItem(stationKey(key)) ?? null;
    if (own !== null) return own;
    return savedStation()?.id === FIRST ? (store?.getItem(key) ?? null) : null;
  } catch {
    return null;
  }
}

/** Keep a station's preference; false where there is no storage (a private window). */
export function writeScoped(key: string, value: string): boolean {
  try {
    globalThis.localStorage?.setItem(stationKey(key), value);
    return true;
  } catch {
    return false;
  }
}
