// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The DJ's show setup, as the window holds it (ADR-0218): whether the wizard is open, the
// show tied to this account, and the decision to open it by itself for a newly signed-in
// presenter. Not in the sandbox, which has no shows to tie.

import { sandboxed, type Me } from './api.ts';
import { account } from './account.svelte.ts';
import * as api from './schedule.ts';
import type { Show } from './schedule.ts';
import { myShow, needsSetup, skipKey } from './wizard.ts';

export const setup = $state({
  /** The wizard is open over the surface. */
  open: false,
  /** Every show, as last fetched (for the slug check and the colour). */
  shows: [] as Show[],
  /** The station has answered `/api/v1/shows` once for this window. */
  loaded: false,
  /** The last fetch's problem, in the station's words. */
  error: '',
});

/** The show tied to this window's account, from the last fetch. */
export function mine(): Show | null {
  return myShow(account.me, setup.shows);
}

function skipped(me: Me | null): boolean {
  const id = me?.account?.id;
  if (!id) return false;
  try {
    return globalThis.localStorage?.getItem(skipKey(id)) === '1';
  } catch {
    return false;
  }
}

/** Remember on this machine that this account skipped the wizard. */
export function rememberSkip(me: Me | null): void {
  const id = me?.account?.id;
  if (!id) return;
  try {
    globalThis.localStorage?.setItem(skipKey(id), '1');
  } catch {
    // A private window: the wizard asks again next time.
  }
}

/** Fetch the shows; a station that cannot answer leaves what was there. */
export async function loadShows(): Promise<void> {
  if (sandboxed()) return;
  try {
    setup.shows = await api.shows();
    setup.error = '';
  } catch (e) {
    setup.error = e instanceof Error ? e.message : String(e);
  } finally {
    setup.loaded = true;
  }
}

/** After the station has said who the window is: open the wizard for a presenter with no
 *  show of their own, once per machine unless they skip it (ADR-0218). */
export async function decideSetup(): Promise<void> {
  if (sandboxed() || !account.me || account.me.kind !== 'staff') return;
  await loadShows();
  if (setup.error) return;
  if (needsSetup(account.me, setup.shows, skipped(account.me))) setup.open = true;
}

export function openSetup(): void {
  setup.open = true;
}

export function closeSetup(): void {
  setup.open = false;
}
