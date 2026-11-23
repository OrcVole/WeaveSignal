// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The "new sounds are available" notice's state (lib/packNotice.ts has the rule): asked once
// after sign-in, for an admin or a producer, shown on the Desk and Carts tabs until dismissed.

import { account } from './account.svelte.ts';
import { catalogue, sandboxed } from './api.ts';
import { packView } from './cartEdit.ts';
import { DISMISSED_KEY, mayLoadPack, packFingerprint, packNewSounds } from './packNotice.ts';
import { onStationChange } from './reset.ts';
import { readScoped, writeScoped } from './scoped.ts';
import { studio } from './studio.svelte.ts';

/** When a pack was last seen loading, and how long to leave it before asking again. */
let lastBusy = 0;
const RETRY_MS = 5000;

const fresh = () => ({
  /** The notice is drawn: sounds are available and this pack has not been dismissed. */
  shown: false,
  /** How many sounds a load would add. */
  count: 0,
  /** The fingerprint of the pack the count is for. */
  print: '',
  /** The station has been asked for this sign-in. */
  asked: false,
});

export const packNotice = $state(fresh());

/** Ask the station, once per sign-in, whether the default pack has sounds the station lacks.
 *  Silent when it cannot say: a station older than 1.2.1 has no pack route. */
export async function checkPackNotice(): Promise<void> {
  if (packNotice.asked || sandboxed() || !mayLoadPack(account.me)) return;
  // A pack that is loading (a first start, or someone's Default pack…) has put only some of its
  // sounds on the carts so far: the rest would be counted as new. Ask again a few seconds on;
  // the window's view changes about every second, and asks no oftener than that here.
  if (Date.now() - lastBusy < RETRY_MS) return;
  if (studio.view?.carts.importing) return void (lastBusy = Date.now());
  try {
    const [view, cat] = await Promise.all([packView(), catalogue()]);
    if (view.loading) return void (lastBusy = Date.now());
    packNotice.asked = true;
    const count = packNewSounds(view, cat);
    const print = packFingerprint(view);
    packNotice.count = count;
    packNotice.print = print;
    packNotice.shown = count > 0 && readScoped(DISMISSED_KEY) !== print;
  } catch {
    // Not known (a station older than 1.2.1 has no pack route): nothing is said, for this sign-in.
    packNotice.asked = true;
  }
}

/** Hide the notice for this pack. */
export function dismissPackNotice(): void {
  packNotice.shown = false;
  writeScoped(DISMISSED_KEY, packNotice.print);
}

onStationChange('packNotice', () => {
  Object.assign(packNotice, fresh());
  lastBusy = 0;
});
