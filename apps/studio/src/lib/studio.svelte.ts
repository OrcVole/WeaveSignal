// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The studio app's state: the control plane's latest view, the link to it, the clock, and
// what is only this window's (the tab, the screener's selection, an armed key, a note).

import { catalogue, type LinkState, type View } from './api.ts';
import type { Catalogue } from '../../../../packages/ui/carts.ts';
import type { Tab, TwoPress } from '../../../../packages/ui/keymap.ts';
import type { AirAsk } from './air.ts';
import { lookOf, showTookAir, stationStep, tabShown } from './steps.ts';
import { onStationChange } from './reset.ts';
import { readScoped, writeScoped } from './scoped.ts';
import { BACK_NOTE_MS, backOnAir, lostId, withoutLetGo } from './post.ts';

export interface LocalNote {
  id: number;
  tone: 'good' | 'warning' | 'serious';
  title: string;
  message: string;
  until: number;
}

// Declared before the store, which reads the bank as it is made: where it sat after the store,
// the read threw inside a `try` and every window opened on bank 1 whatever was kept.
const BANK_KEY = 'weavesignal.cart-bank';

/** What `studio` is when the page first loads: a change of station puts it back (ADR-0285). */
const fresh = () => ({
  view: null as View | null,
  link: 'connecting' as LinkState,
  linkWhy: '' as string,
  /** The control plane's clock minus this machine's, from the last view. */
  skew: 0,
  /** This machine's clock, ticking every 100 ms for the studio clock's tenths. */
  now: Date.now(),
  tab: 'desk' as Tab,
  /** The Desk tab's studio (1.3.0): the live desk, or the rehearsal desk (`desk.studio`). This
   *  window's own choice, never remembered; set with `chooseDeskStudio` (rehearsal.svelte.ts). */
  deskStudio: 'live' as 'live' | 'rehearsal',
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
  /** The hosts' own studios whose arrival this window has been told of and dismissed (ADR-0280),
   *  by identity. */
  toldSources: [] as string[],
  /** The strip each host's own studio was last seen on, by its show key: a source whose link is
   *  lost has left the view, and its kept tile stays on its strip (ADR-0280). */
  sourceSlots: {} as Record<string, number>,
  /** The lost links whose toast this window has dismissed, each by its key and when it was lost. */
  lostDismissed: [] as string[],
  /** The kept places this window was told are let go (Off air), each by its key and when it was
   *  lost, until the station's view stops carrying them. */
  letGo: [] as string[],
  /** The graces whose offer this window has put away with Not now, each by its handover. */
  graceDismissed: [] as string[],
  /** The clock's Hold or Let it run was pressed and the view has not caught up: what `held_by`
   *  was at the press. Null when nothing is waited for. */
  holdAsked: null as { was: string | null; at: number } | null,
  /** The cart banks and carts, as last fetched: the station's, and `cartShow`'s own. */
  catalogue: null as Catalogue | null,
  /** The show whose own cart banks this window works with, by its slug (prepare.ts,
   *  `cartShowFor`); null: the station's banks alone. */
  cartShow: null as string | null,
  /** The cart bank this window shows (⌃F1 to ⌃F5); F1 to F6 fire its top row. */
  bank: readBank(),
  /** The cart the arrows have moved to on the wall (1 to 24), or null. */
  cartFocus: null as number | null,
  /** What the station's setup still lacks, for an admin, while its setup page is offered
   *  (ADR-0267); null when done, not known, or not an admin's to see. */
  setupPending: null as string[] | null,
  /** The station's time zone (IANA), for the rundown's times; empty until read. */
  zone: '',
  /** The station's version, from `/health`, for the frame (lib/version.ts); null until read. */
  stationVersion: null as string | null,
  /** The ask the desk's air dialog is open on (End show, Hand to automation, Take the air
   *  back; ADR-0272): set by ⌃⇧E and by the buttons, cleared when the dialog closes. */
  airAsk: null as AirAsk | null,
  /** The DJ this window's first ⇧⏎ put on air, until the second press hands over (ADR-0231). */
  handoverTo: null as string | null,
  /** Show everything (ADR-0274): this window draws the whole studio while the station stays at
   *  the show's step. A look, not a switch; never remembered, and this window's alone. */
  showAll: false,
  /** The step dialog (`studio.step.menu`) is open. */
  stepMenu: false,
  /** The note a hidden part's key shows, in the desk's toast slot, without a tone colour. */
  offNote: null as { title: string; message: string; until: number } | null,
  /** The hidden parts whose key has already said so in this session: each says it once. */
  offTold: [] as string[],
});

export const studio = $state(fresh());

function readBank(): number {
  const n = Number(readScoped(BANK_KEY));
  return n >= 1 && n <= 5 ? n : 1;
}

export function setBank(n: number): void {
  studio.bank = n;
  // In a private window the bank is this session's only.
  writeScoped(BANK_KEY, String(n));
}

/** What this window draws: the station's step, or everything while Show everything is on. */
export function look() {
  return lookOf(stationStep(studio.view), studio.showAll);
}

/** When the look no longer has the tab this window is on (the step moved down, Back was
 *  pressed, or the next show took the air while the Screener was open), go to the Desk. */
function settleTab(): void {
  if (tabShown(studio.tab, look())) return;
  studio.tab = 'desk';
  if (typeof document !== 'undefined') document.documentElement.dataset.surface = 'desk';
}

/** Show everything, or back to the step. */
export function setShowAll(on: boolean): void {
  studio.showAll = on && stationStep(studio.view) !== 'full';
  settleTab();
}

let fetching = '';

/** Fetch the catalogue when the view says it changed, or this window's show did. */
async function refreshCatalogue(version: number): Promise<void> {
  const show = studio.cartShow;
  const key = `${version}:${show ?? ''}`;
  if (fetching === key) return;
  fetching = key;
  try {
    const cat = await catalogue(show);
    // The window's show moved while this was on its way: the next fetch is the one to keep.
    if (studio.cartShow === show) studio.catalogue = cat;
  } catch {
    fetching = ''; // try again with the next view
  }
}

/** Fetch the catalogue now: this window has just changed a bank or a cart (1.2.1) and shows
 *  the change without waiting for the next view. A failed fetch leaves what was there; the
 *  view's next version asks again. */
export async function reloadCatalogue(): Promise<void> {
  const show = studio.cartShow;
  try {
    const cat = await catalogue(show);
    if (studio.cartShow === show) studio.catalogue = cat;
  } catch {
    fetching = '';
  }
}

/** The show whose own cart banks this window works with changed (a live window's show took the
 *  air, or the window switched between Live and Preparing): fetch its catalogue. The tab this
 *  window shows stays, and nothing sounding is stopped. */
export function setCartShow(show: string | null): void {
  if (studio.cartShow === show) return;
  studio.cartShow = show;
  if (studio.view?.carts) void refreshCatalogue(studio.view.carts.version);
}

let noteId = 0;

export function say(tone: LocalNote['tone'], title: string, message: string, ms = 6000): void {
  studio.note = { id: ++noteId, tone, title, message, until: Date.now() + ms };
}

/** The station's time now, in epoch ms. */
export function stationNow(): number {
  return studio.now + studio.skew;
}

export function takeView(seen: View): void {
  // After Off air the station's view may carry the place for a moment more: it is read without it.
  const v = withoutLetGo(seen, studio.letGo);
  const still = seen.post?.link_lost ? lostId(seen.post.link_lost) : null;
  if (studio.letGo.some((id) => id !== still)) studio.letGo = studio.letGo.filter((id) => id === still);
  // Show everything lasts until the next show takes the air, and means nothing at Full.
  if (studio.showAll && (showTookAir(studio.view?.step, v.step) || stationStep(v) === 'full')) studio.showAll = false;
  // A host's own studio (ADR-0280): where each one sits, for the tile kept while its link is
  // lost; and a source that is back on air after a lost link, which the station posts no notice for.
  for (const s of v.sources ?? []) if (s.role === 'source' && s.source && studio.sourceSlots[s.source.key] !== s.slot) studio.sourceSlots = { ...studio.sourceSlots, [s.source.key]: s.slot };
  const back = backOnAir(studio.view, v);
  if (back) say('good', back.title, back.message, BACK_NOTE_MS);
  // The view has caught up with Hold or Let it run, or it never will: the button is free again.
  if (studio.holdAsked && ((v.post?.held_by ?? null) !== studio.holdAsked.was || Date.now() - studio.holdAsked.at > 5000)) studio.holdAsked = null;
  studio.view = v;
  settleTab();
  studio.skew = v.now_ms - Date.now();
  if (v.carts && (v.carts.version !== studio.catalogue?.version || (studio.catalogue.show ?? null) !== studio.cartShow)) void refreshCatalogue(v.carts.version);
}

/** The cart bank is kept for each station apart; read it again once the station is known
 *  (the store is made before the packaged app has read which station it is on). */
export function settleBank(): void {
  studio.bank = readBank();
}

// A change of station (ADR-0285): nothing of the last one stays. The tab goes back to the Desk.
onStationChange('studio', () => {
  Object.assign(studio, fresh(), { bank: 1 });
  fetching = '';
  noteId = 0;
});
