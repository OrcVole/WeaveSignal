// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Listener keys' state in the studio app (keyed shows, 1.3.2; lib/keys.ts has the rules): the
// station's keyed shows, which every tab reads for the key beside a show, and the Keys tab's
// own list, families, leak report and selection. Nothing of a key's text is ever kept here: a
// text is shown in the dialog that asked for it and is gone when that closes.

import { sandboxed } from './api.ts';
import { accessProblem, keepBody } from './keyed.ts';
import { SOON_DAYS, unreadyOf, type UnreadyWords } from './keys.ts';
import * as api from './keysApi.ts';
import type { KeyDevice, KeyFamily, KeyFilter, KeyLogEntry, KeyState, KeyKind, KeyedShow, KeyedShows, LeakReport, ListenerKey } from './keysApi.ts';
import { onStationChange } from './reset.ts';

// ---- the keyed shows, for the key beside a show on every tab ----

const freshKeyed = () => ({
  /** The station's keyed shows; null until read, and from a station that has none of this. */
  list: null as KeyedShows | null,
  /** `none`: the station is older than 1.3.2 (or the sandbox): nothing about keys is drawn. */
  state: 'unread' as 'unread' | 'ok' | 'none',
  at: 0,
});

export const keyed = $state(freshKeyed());
let keyedAsking: Promise<void> | null = null;
const KEYED_FRESH_MS = 30_000;

/** Read which shows need a key. Asked for by whichever tab draws a show first, and again after
 *  half a minute or when told (`force`): a rule changes rarely, and opens to everyone by itself. */
export function loadKeyed(force = false): Promise<void> {
  if (sandboxed()) {
    keyed.state = 'none';
    return Promise.resolve();
  }
  if (keyedAsking) return keyedAsking;
  if (!force && keyed.state !== 'unread' && Date.now() - keyed.at < KEYED_FRESH_MS) return Promise.resolve();
  keyedAsking = api
    .keyedShows()
    .then((list) => {
      keyed.list = list;
      keyed.state = 'ok';
    })
    .catch(() => {
      // A station older than 1.3.2 has no such route; one that did not answer keeps what was read.
      if (keyed.state === 'unread') keyed.state = 'none';
    })
    .finally(() => {
      keyed.at = Date.now();
      keyedAsking = null;
    });
  return keyedAsking;
}

/** Read the keyed shows again after a rule was saved: a reading already on its way may have
 *  left before the save, so it is waited out and the list is asked for afresh. */
export async function reloadKeyed(): Promise<void> {
  if (keyedAsking) await keyedAsking;
  await loadKeyed(true);
}

/** The keyed show with this path or id, while only a key opens it; null for a public show. */
export function keyedShow(slugOrId: string | null | undefined): KeyedShow | null {
  if (!slugOrId || !keyed.list) return null;
  return keyed.list.shows.find((s) => (s.slug === slugOrId || s.id === slugOrId) && s.now === 'key') ?? null;
}

/** The same, whether or not it has opened to everyone by now: the show has a rule. */
export function ruledShow(slugOrId: string | null | undefined): KeyedShow | null {
  if (!slugOrId || !keyed.list) return null;
  return keyed.list.shows.find((s) => s.slug === slugOrId || s.id === slugOrId) ?? null;
}

// ---- the way to a keyed show (the operator's click-through, 2026-11-25) ----

const freshAccessUi = () => ({
  /** A show whose Who can listen is to be brought into view when its editor is drawn; `pick`:
   *  with People with a key already chosen, not yet saved. */
  jump: null as { slug: string; pick: boolean } | null,
  /** New show is asked for from another tab, with Keyed show ticked (the Keys tab's list). */
  newKeyed: false,
});
/** What the show editor's Who can listen is asked to do next. */
export const accessUi = $state(freshAccessUi());

/** Keep a show for people with a key, in one press (New show's tick, the Keys tab's list). The
 *  rule it had is read first, so what the public hears is kept as it was set. It answers with
 *  the rule as saved, or throws a sentence that says what to do (`accessProblem`). */
export async function keepShow(slug: string): Promise<api.ShowAccess> {
  let stored: api.ShowAccess;
  try {
    stored = await api.showAccess(slug);
  } catch (e) {
    throw new Error(accessProblem(e, 'read'));
  }
  try {
    const saved = await api.setShowAccess(slug, keepBody(stored, Date.now()));
    await reloadKeyed();
    return saved;
  } catch (e) {
    throw new Error(accessProblem(e, 'save', 'key'));
  }
}

/** The way back: the show is open to everyone again, and its keys are kept. */
export async function openToEveryone(slug: string): Promise<api.ShowAccess> {
  try {
    const saved = await api.setShowAccess(slug, { rule: 'public' });
    await reloadKeyed();
    return saved;
  } catch (e) {
    throw new Error(accessProblem(e, 'save', 'public'));
  }
}

// ---- the Keys tab ----

export type KeysView = 'keys' | 'families' | 'leaks';
export const PAGE = 60;

const freshUi = () => ({
  /** What the keymap asked for (`N`, `/`): counted, so the tab acts on each press once. */
  make: 0,
  search: 0,
  /** A dialog another tab asked the Keys tab to open once it is ready: Make a key (New show's
   *  Done, a show's Who can listen), or the list of shows to keep for people with a key. */
  ask: null as 'make' | 'keep' | null,
});
/** The keymap's presses on the Keys tab. */
export const keysUi = $state(freshUi());

const fresh = () => ({
  /** `loading`, `ok`, or why the whole screen is refused. */
  ready: 'loading' as 'loading' | 'ok' | UnreadyWords,
  view: 'keys' as KeysView,
  keys: [] as ListenerKey[],
  /** How many keys match the filter, before the page. */
  total: 0,
  /** How many keys the station has in all, unfiltered: for "no keys yet". */
  all: null as number | null,
  loadingMore: false,
  filter: { q: '', state: '' as KeyState | '', kind: '' as KeyKind | '', family: '', show: '' },
  families: [] as KeyFamily[],
  /** The key whose page is open. */
  selected: null as string | null,
  /** The selected key itself, when the page of the list does not hold it (opened from the leak
   *  report, from Find a key, or just made). */
  one: null as ListenerKey | null,
  devices: null as KeyDevice[] | null,
  log: null as KeyLogEntry[] | null,
  pageProblem: '',
  leaks: null as LeakReport | null,
  leaksFamily: '',
  leaksProblem: '',
  leaksLoading: false,
  /** One line under the toolbar, after something was done. */
  note: '',
  /** The station's pace and holds (`/listener-keys/signing`): how soon a change reaches a
   *  listener (its renewal), when a key ends soon, how long a device holds its place. */
  signing: null as { renewS: number; holdS: number | null; soonDays: number } | null,
});

export const ks = $state(fresh());

onStationChange('keys', () => {
  Object.assign(keyed, freshKeyed());
  Object.assign(accessUi, freshAccessUi());
  Object.assign(keysUi, freshUi());
  Object.assign(ks, fresh());
});

const filterOf = (): KeyFilter => ({
  ...(ks.filter.q.trim() ? { q: ks.filter.q.trim() } : {}),
  ...(ks.filter.state ? { state: ks.filter.state } : {}),
  ...(ks.filter.kind ? { kind: ks.filter.kind } : {}),
  ...(ks.filter.family ? { family: ks.filter.family } : {}),
  ...(ks.filter.show ? { show: ks.filter.show } : {}),
});
export const filtered = (): boolean => Object.keys(filterOf()).length > 0;

let asked = 0;

/** The list from its top, with the families and the keyed shows. A refusal of the list is a
 *  refusal of the screen, said in words (lib/keys.ts `unreadyOf`). */
export async function refreshKeys(): Promise<void> {
  const mine = ++asked;
  try {
    const [list, families] = await Promise.all([api.listKeys({ ...filterOf(), limit: PAGE }), api.listFamilies(), loadKeyed(true)]);
    if (mine !== asked) return;
    ks.keys = list.keys;
    ks.total = list.total;
    ks.families = families;
    if (!filtered()) ks.all = list.total;
    else if (ks.all === null) ks.all = (await api.listKeys({ limit: 1 })).total;
    ks.ready = 'ok';
    if (ks.signing === null)
      void api
        .keySigning()
        .then((s) => {
          ks.signing = { renewS: s.renew_every_s, holdS: s.device_hold_s ?? null, soonDays: s.ends_soon_days ?? SOON_DAYS };
        })
        .catch(() => {});
    if (ks.selected) void openKey(ks.selected, false);
  } catch (e) {
    if (mine !== asked) return;
    ks.ready = unreadyOf(e);
  }
}

export async function moreKeys(): Promise<void> {
  if (ks.loadingMore || ks.keys.length >= ks.total) return;
  ks.loadingMore = true;
  try {
    const list = await api.listKeys({ ...filterOf(), limit: PAGE, offset: ks.keys.length });
    const have = new Set(ks.keys.map((k) => k.id));
    ks.keys = [...ks.keys, ...list.keys.filter((k) => !have.has(k.id))];
    ks.total = list.total;
  } catch {
    // The list as it is stays; Show more can be pressed again.
  } finally {
    ks.loadingMore = false;
  }
}

/** The key whose page is open, from the list or by itself. */
export function selectedKey(): ListenerKey | null {
  if (!ks.selected) return null;
  return ks.keys.find((k) => k.id === ks.selected) ?? (ks.one?.id === ks.selected ? ks.one : null) ?? ks.leaks?.entries.find((e) => e.key.id === ks.selected)?.key ?? null;
}

/** Open a key's page: the key, the devices that hold it and its change log. */
export async function openKey(id: string, clear = true): Promise<void> {
  ks.selected = id;
  ks.pageProblem = '';
  if (clear) {
    ks.devices = null;
    ks.log = null;
  }
  try {
    const [key, devices, log] = await Promise.all([api.getKey(id), api.keyDevices(id), api.keyLog(id)]);
    if (ks.selected !== id) return;
    took(key);
    ks.devices = devices;
    ks.log = log;
  } catch (e) {
    if (ks.selected !== id) return;
    ks.pageProblem = e instanceof Error ? e.message : 'The key could not be read.';
  }
}

export function closeKey(): void {
  ks.selected = null;
  ks.one = null;
  ks.devices = null;
  ks.log = null;
  ks.pageProblem = '';
}

/** A key as the station just answered it: put in the list's place, the leak report's, and the page's. */
export function took(key: ListenerKey): void {
  ks.keys = ks.keys.map((k) => (k.id === key.id ? key : k));
  if (ks.leaks) ks.leaks = { ...ks.leaks, entries: ks.leaks.entries.map((e) => (e.key.id === key.id ? { ...e, key } : e)) };
  if (ks.selected === key.id) ks.one = key;
}

/** A key was made: it goes to the top of the list (when the filter is clear) and its page opens. */
export function added(key: ListenerKey): void {
  if (ks.all !== null) ks.all += 1;
  if (!filtered()) {
    ks.keys = [key, ...ks.keys.filter((k) => k.id !== key.id)];
    ks.total += 1;
  }
  ks.selected = key.id;
  ks.one = key;
  ks.devices = [];
  ks.log = null;
  void openKey(key.id, false);
}

/** A key is gone (erased): out of the list, the report and the page. */
export function removed(id: string): void {
  if (ks.keys.some((k) => k.id === id)) ks.total = Math.max(0, ks.total - 1);
  ks.keys = ks.keys.filter((k) => k.id !== id);
  if (ks.all !== null) ks.all = Math.max(0, ks.all - 1);
  if (ks.leaks) ks.leaks = { ...ks.leaks, entries: ks.leaks.entries.filter((e) => e.key.id !== id) };
  if (ks.selected === id) closeKey();
}

/** The leak report, for every key or one family's. */
export async function loadLeaks(family = ks.leaksFamily): Promise<void> {
  ks.leaksFamily = family;
  ks.leaksLoading = true;
  ks.leaksProblem = '';
  try {
    ks.leaks = await api.leakReport(family || undefined);
  } catch (e) {
    ks.leaksProblem = e instanceof Error ? e.message : 'The report could not be read.';
  } finally {
    ks.leaksLoading = false;
  }
}

export function setView(v: KeysView): void {
  ks.view = v;
  ks.note = '';
  if (v === 'leaks' && !ks.leaks && !ks.leaksLoading) void loadLeaks();
}

/** A family's label, by its id. */
export function familyLabel(id: string | null | undefined): string | null {
  return id ? (ks.families.find((f) => f.id === id)?.label ?? null) : null;
}
