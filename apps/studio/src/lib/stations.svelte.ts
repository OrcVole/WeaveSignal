// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station switcher as the window holds it (ADR-0285;
// docs/reference/design/station-switcher.md): the list of stations this computer keeps, the
// dialog's state, and the one path by which the window leaves a station.
//
// Leaving is a fixed order, and `stations.test.ts` holds it: let every held key go and wait
// for the releases to land; close the links (the event stream, the chat's socket and loop);
// seal the page so nothing more is asked; empty every store; tell the Rust side, which stops
// every request still open and moves the window; then load the page again, as sign-in always
// has. The page that comes up has never seen the station left.
//
// This store is the app's, not a station's: the list is what the switcher shows on every
// station, so a change of station does not empty it.

import * as api from './api.ts';
import { account } from './account.svelte.ts';
import { whoLine } from './account.ts';
import { preparing } from './prepare.svelte.ts';
import { closeLinks, resetStores } from './reset.ts';
import { say, studio } from './studio.svelte.ts';
import { arrivalNote, leaveAsk, stationName, type Arrival, type LeaveAsk } from './stations.ts';

/** Where the window is going when it leaves its station. */
export type Leave = { kind: 'switch'; id: string } | { kind: 'sign-out'; id: string } | { kind: 'remove'; id: string };

export const stations = $state({
  /** Every station kept on this computer; null until read (and in a browser, always). */
  list: null as api.StationList | null,
  /** The Stations dialog (`studio.stations`) is open. */
  open: false,
  /** The sign-in screen is open over the studio to add a station. */
  adding: false,
  /** The row opened out with Manage. */
  manage: null as string | null,
  /** The row whose Remove is being asked, in place. */
  removing: null as string | null,
  /** The leave question (`studio.stations.leave`), and where the window goes on a yes. */
  ask: null as { leave: Leave; words: LeaveAsk } | null,
  /** The window is leaving: nothing else may start. */
  busy: false,
  /** The last change's problem, in plain words. */
  problem: '',
});

/** Open the Stations dialog fresh: from the frame's station name, or `⌃⇧S` on any tab
 *  (ADR-0287). Only the packaged app keeps stations; a browser window has none to list. */
export function openStations(): void {
  if (!api.packaged()) return;
  stations.ask = null;
  stations.manage = null;
  stations.removing = null;
  stations.problem = '';
  stations.open = true;
  void loadStations();
}

/** The row of the station the window is on. */
export function hereRow(): api.StationRow | null {
  const l = stations.list;
  return l?.stations.find((s) => s.id === l.current) ?? null;
}

/** What the frame calls the station: the label given here, else the station's own name as it
 *  says it now, else what was last seen, else the app's name. */
export function hereName(): string {
  const row = hereRow();
  return row?.label || studio.view?.station || (row ? stationName(row) : '') || 'WeaveSignal';
}

/** Read the list (packaged); a failure leaves what was there. */
export async function loadStations(): Promise<void> {
  if (!api.packaged()) return;
  try {
    stations.list = await api.listStations();
  } catch {
    // The frame then shows the station's name without the list.
  }
}

// ---- the frame's wiring ----

/** What the frame does before the window leaves: let every held key go and wait for the
 *  releases to land (a talk left open at a station would have no way to close). */
let release: (() => Promise<void>) | null = null;
export function onBeforeLeave(fn: (() => Promise<void>) | null): void {
  release = fn;
}

/** Load the page again. A test replaces it. */
let reload = (): void => globalThis.location.reload();
export function setReload(fn: () => void): void {
  reload = fn;
}

const ARRIVAL_KEY = 'weavesignal.arrived';

// ---- leaving ----

/** Ask to leave the station the window is on, or to change another station in the list. A
 *  change to another station (sign out of it, remove it) is done at once: the window is not
 *  on it. Leaving the window's own station is asked first when something is open there. */
export async function requestLeave(leave: Leave): Promise<void> {
  if (stations.busy) return;
  stations.problem = '';
  const here = stations.list?.current ?? null;
  const leaving = leave.kind === 'switch' ? leave.id !== here : leave.id === here;
  if (leave.kind === 'switch' && leave.id === here) return;
  if (!leaving) return changeOther(leave);
  const row = hereRow();
  // Signed out of the station it is on, the window shows only the sign-in screen: nothing is
  // open from here, and there is nothing to ask.
  const words = row?.kept ? leaveAsk(studio.view, studio.link, preparing(), hereName()) : null;
  if (words) {
    stations.ask = { leave, words };
    stations.open = true;
    return;
  }
  await leaveNow(leave, false);
}

/** Sign out of, or remove, a station the window is not on. */
async function changeOther(leave: Leave): Promise<void> {
  try {
    stations.list = leave.kind === 'remove' ? await api.removeStation(leave.id) : await api.signOutOfStation(leave.id);
    stations.removing = null;
    if (leave.kind === 'remove') stations.manage = null;
  } catch (e) {
    stations.problem = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
  }
}

/** Give a station a name on this computer; empty takes it away. */
export async function rename(id: string, label: string): Promise<boolean> {
  stations.problem = '';
  try {
    stations.list = await api.renameStation(id, label);
    return true;
  } catch (e) {
    stations.problem = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
    return false;
  }
}

/** Leave the station the window is on, in the fixed order, then load the page again. */
export async function leaveNow(leave: Leave, leftLive: boolean): Promise<void> {
  if (stations.busy) return;
  stations.busy = true;
  const from = hereRow()?.kept ? hereName() : null;
  // 1. Let go: the releases are sent to the station being left, while its link still stands.
  try {
    await release?.();
  } catch {
    // A release the station did not take: its own talk rule ends a talk nobody holds.
  }
  // 2. Close the links, then seal the page: nothing more is asked of the station.
  closeLinks();
  api.sealStation();
  // 3. Empty every store.
  resetStores();
  api.settlePrepare(null);
  try {
    const arrival: Arrival = { from, leftLive };
    if (leave.kind === 'switch') globalThis.sessionStorage?.setItem(ARRIVAL_KEY, JSON.stringify(arrival));
    else globalThis.sessionStorage?.removeItem(ARRIVAL_KEY);
  } catch {
    // No storage: the window arrives without its note.
  }
  // 4. Tell the Rust side: it stops every request still open, then moves the window.
  try {
    if (leave.kind === 'switch') await api.switchToStation(leave.id);
    else if (leave.kind === 'remove') await api.removeStation(leave.id);
    else await api.signOutOfStation(leave.id);
  } catch {
    // The page loads again either way, and comes up wherever the Rust side says it is.
  }
  // 5. Load the page again.
  reload();
}

/** A sign-in on the Add a station screen succeeded. A station that took the window (there was
 *  none, or the window was signed out of its own) needs only the reload sign-in always did; one
 *  that joined the list while the window is on another is switched to by the path above. */
export async function afterSignIn(saved: api.SavedStation): Promise<void> {
  if (saved.current !== false) {
    reload();
    return;
  }
  stations.adding = false;
  await loadStations();
  await requestLeave({ kind: 'switch', id: saved.id });
}

// ---- arriving ----

/** What the window was told as it left the last station; read once. */
function takeArrival(): Arrival | null {
  try {
    const raw = globalThis.sessionStorage?.getItem(ARRIVAL_KEY);
    globalThis.sessionStorage?.removeItem(ARRIVAL_KEY);
    const a = raw ? (JSON.parse(raw) as Partial<Arrival>) : null;
    return a ? { from: typeof a.from === 'string' ? a.from : null, leftLive: a.leftLive === true } : null;
  } catch {
    return null;
  }
}

/** Who the list says is signed in here: the frame's own words. */
function whoHere(): string | null {
  // Not known: the station has not said (it is away, or too old to say).
  if (!account.loaded || !account.me) return null;
  if (account.me.kind === 'station') return 'the station key';
  const w = whoLine(account.me);
  return `${w.name}, ${w.role}`;
}

/** Once the station has said who the window is: tell the list what the window sees here (its
 *  own name, who is signed in, its version), and, after a switch, say where the window is. */
export async function settleArrival(version: string | null): Promise<void> {
  const id = api.savedStation()?.id;
  if (!api.packaged() || !id) return;
  const who = whoHere();
  try {
    stations.list = await api.noteStation(id, { name: studio.view?.station ?? null, who, version });
  } catch {
    await loadStations();
  }
  const arrival = takeArrival();
  if (!arrival) return;
  const note = arrivalNote(hereName(), who, arrival, stations.list?.app_version ?? null, version);
  say('good', note.title, note.message, 8000);
}
