// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Whether this computer has opened or copied the station's player once (1.3.1, the design
// review's F1). The Desk's Listeners panel teaches "sound plays in your browser, not in this
// app" until then, and gives the room back afterwards. Kept for each station apart, in the same
// storage as the new-sounds notice's dismissal (lib/scoped.ts); where there is no storage the
// answer holds until the station changes or the window reloads.

import { onStationChange } from './reset.ts';
import { readScoped, writeScoped } from './scoped.ts';

/** The key it is kept under (`lib/scoped.ts` adds the station). */
export const PLAYER_USED_KEY = 'weavesignal.playerUsed';

/** Changes whenever the answer may have: what a reader depends on. */
const s = $state({ n: 0, mem: false });

/** The player has been opened or copied from this computer, for this station. */
export function playerUsed(): boolean {
  void s.n;
  return s.mem || readScoped(PLAYER_USED_KEY) === '1';
}

/** Note that the player was opened or copied. */
export function markPlayerUsed(): void {
  s.mem = true;
  writeScoped(PLAYER_USED_KEY, '1');
  s.n++;
}

onStationChange('playerUsed', () => {
  s.mem = false;
  s.n++;
});
