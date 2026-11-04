// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The studio app's state: the control plane's latest view, the link to it, the clock, and
// what is only this window's (the tab, the screener's selection, an armed key, a note).

import { catalogue, type LinkState, type View } from './api.ts';
import type { Catalogue } from '../../../../packages/ui/carts.ts';
import type { Tab, TwoPress } from '../../../../packages/ui/keymap.ts';

export interface LocalNote {
  id: number;
  tone: 'good' | 'warning' | 'serious';
  title: string;
  message: string;
  until: number;
}

export const studio = $state({
  view: null as View | null,
  link: 'connecting' as LinkState,
  linkWhy: '' as string,
  /** The control plane's clock minus this machine's, from the last view. */
  skew: 0,
  /** This machine's clock, ticking every 100 ms for the studio clock's tenths. */
  now: Date.now(),
  tab: 'desk' as Tab,
  /** Settings is open over the tab (station admin; reached from the frame, not a numbered tab). */
  settings: false,
  /** The screener's selected caller. */
  selected: null as string | null,
  /** The armed two-press key and when it was armed; `identity` is whose tile armed it, for an
   *  action aimed at one person (End pass), so no other tile shows it armed. */
  armed: null as { what: TwoPress; at: number; identity?: string } | null,
  /** Who this window's held talk keys are open to, for the lit buttons. */
  talking: [] as string[],
  /** A one-off note in the toast slot (a refused command, a key not built yet). */
  note: null as LocalNote | null,
  /** Toasts dismissed with Esc, by notice id. */
  dismissed: [] as number[],
  /** The cart banks and carts, as last fetched. */
  catalogue: null as Catalogue | null,
  /** The cart bank this window shows (⌃F1 to ⌃F5); F1 to F6 fire its top row. */
  bank: readBank(),
  /** The cart the arrows have moved to on the wall (1 to 24), or null. */
  cartFocus: null as number | null,
});

const BANK_KEY = 'weavesignal.cart-bank';

function readBank(): number {
  try {
    const n = Number(globalThis.localStorage?.getItem(BANK_KEY));
    return n >= 1 && n <= 5 ? n : 1;
  } catch {
    return 1;
  }
}

export function setBank(n: number): void {
  studio.bank = n;
  try {
    globalThis.localStorage?.setItem(BANK_KEY, String(n));
  } catch {
    // A private window: the bank is this session's only.
  }
}

let fetching = -1;

/** Fetch the catalogue when the view says it changed. */
async function refreshCatalogue(version: number): Promise<void> {
  if (fetching === version) return;
  fetching = version;
  try {
    studio.catalogue = await catalogue();
  } catch {
    fetching = -1; // try again with the next view
  }
}

let noteId = 0;

export function say(tone: LocalNote['tone'], title: string, message: string, ms = 6000): void {
  studio.note = { id: ++noteId, tone, title, message, until: Date.now() + ms };
}

/** The station's time now, in epoch ms. */
export function stationNow(): number {
  return studio.now + studio.skew;
}

export function takeView(v: View): void {
  studio.view = v;
  studio.skew = v.now_ms - Date.now();
  if (v.carts && v.carts.version !== studio.catalogue?.version) void refreshCatalogue(v.carts.version);
}
