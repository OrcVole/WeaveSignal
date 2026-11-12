// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Prepare mode as the window holds it (ADR-0276): whether this window is Live or Preparing (the
// station's word, on `account.me`), the switch between them, the default when the window
// starts, and the frame's dialog. The rules and the words are in prepare.ts.

import * as api from './api.ts';
import { account } from './account.svelte.ts';
import { say, studio } from './studio.svelte.ts';
import { setup } from './wizard.svelte.ts';
import { canPrepare, cartShowFor, isPreparing, liveAsks, onAirFor, startsPreparing, type OnAir, type WindowIs } from './prepare.ts';

export const prepare = $state({
  /** The dialog (`studio.prepare.menu`) is open. */
  menu: false,
  /** What the dialog opens on: the band's Switch to Live… opens it with Live picked. */
  pick: null as WindowIs | null,
  /** A switch is on its way to the station. */
  busy: false,
  /** The station's own sentence when it refuses a switch. */
  problem: '',
  /** The start's default has been settled for this window (applied, or found not to apply). */
  settled: false,
  /** When a key or a button last met a control this window has switched off: the band answers. */
  nudgedAt: 0,
});

/** Whether this window is preparing: the station's word. */
export function preparing(): boolean {
  return isPreparing(account.me);
}

/** The show whose own cart banks this window works with (a cart wall per show, 1.0.2). */
export function cartShow(): string | null {
  return cartShowFor(preparing(), account.me, setup.shows, studio.view?.step);
}

/** What is on air by the schedule, as this account sees it. */
export function onAir(): OnAir {
  return onAirFor(account.me, setup.shows, studio.view?.step);
}

/** What the frame calls from before a switch to Preparing: let every held key go (a talk left
 *  open would have no way to close) and wait for the releases to land. */
let letGo: (() => Promise<void>) | null = null;
export function onBeforePreparing(fn: (() => Promise<void>) | null): void {
  letGo = fn;
}

/** Take the station's answer as who this window is now. */
function take(me: api.Me): void {
  if (me && typeof me.role === 'string' && typeof me.kind === 'string') account.me = me;
}

/** Switch this window to Preparing or back to Live. The station keeps the flag on the session
 *  and answers with it; a refusal leaves the window as it was, with the station's sentence. */
export async function setPreparing(on: boolean): Promise<boolean> {
  if (prepare.busy || !canPrepare(account.me)) return false;
  prepare.busy = true;
  prepare.problem = '';
  try {
    if (on) {
      await letGo?.();
      studio.armed = null;
      studio.handoverTo = null;
      studio.airAsk = null;
    }
    take(await api.setPreparing(on));
    // A deliberate switch stands: a reload of this window does not apply the default again.
    prepare.settled = true;
    api.settlePrepare(account.me?.account?.id ?? null);
    return true;
  } catch (e) {
    prepare.problem = e instanceof Error ? e.message : String(e);
    return false;
  } finally {
    prepare.busy = false;
  }
}

/** The band's button: straight to Live when there is nobody to take the desk from; while someone
 *  else's show is on air, the dialog asks once first (⏎ confirms, Esc keeps preparing). */
export function switchToLive(): void {
  if (liveAsks(onAir())) {
    prepare.pick = 'live';
    prepare.menu = true;
    return;
  }
  void setPreparing(false);
}

/** The default when the window starts (ADR-0276): the presenter of a show that is not on air now
 *  starts Preparing; everyone else stays as the station says, which is Live for a new sign-in.
 *  Settled once, when the station has said who this is, which shows there are and what is on
 *  air; never after a switch made by hand, and never towards Live. Called as each of the three
 *  arrives. If the shows cannot be read the window stays as it is. */
export function settleStart(): void {
  if (prepare.settled || !account.loaded) return;
  const me = account.me;
  if (!canPrepare(me)) {
    prepare.settled = true;
    return;
  }
  const id = me!.account?.id ?? null;
  // A reload of a window that has already settled it: the station still holds what it chose.
  if (id && api.prepareSettledFor() === id) {
    prepare.settled = true;
    return;
  }
  if (!setup.loaded || !studio.view) return;
  prepare.settled = true;
  if (setup.error) return;
  if (me!.preparing || !startsPreparing(me, setup.shows, studio.view.step)) {
    api.settlePrepare(id);
    return;
  }
  // The switch remembers itself when the station has taken it. If the station refuses, the
  // window stays live and says so in the station's words; the next start tries again.
  void setPreparing(true).then((ok) => {
    if (!ok) say('warning', 'This window could not start preparing', `${prepare.problem} It is live: its keys and buttons change what is on air.`, 15000);
  });
}

/** A command came back 409: the station may hold this window as preparing when the window
 *  thought it was live (the same sign-in switched in another window). Ask who this is again, so
 *  the frame and the band follow the station. */
export async function followStation(): Promise<void> {
  try {
    take(await api.whoAmI());
  } catch {
    // The 401 watch has its own way back; anything else leaves the window as it was.
  }
}

/** A key or a button met a control this window has switched off: nothing happens on air, and the
 *  band under the frame answers for a moment. */
export function nudge(): void {
  prepare.nudgedAt = Date.now();
}
