// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal desk in this window (1.3.0, step R13): which strip each person has, the rehearsed
// show's carts, the presses the tiles and the keys make, the move, and what the window remembers
// of a move it pressed. The rules are `rehearsalDesk.ts`; the station's view is the store in
// `rehearsal.svelte.ts`.
//
// Everything here goes to `/api/v1/rehearsal/*`. Nothing in this file calls the live desk's
// commands, playout or the carts' live route: a key pressed at the rehearsal desk cannot reach
// the air (build plan section 1.7, "a stray key").

import type { Action } from '../../../../packages/ui/keymap.ts';
import { cartKey } from '../../../../packages/ui/keymap.ts';
import { cartAt, type Catalogue } from '../../../../packages/ui/carts.ts';
import { FieldError, catalogue, moveRehearsal, sandboxed, type RehearsalPerson } from './api.ts';
import { account } from './account.svelte.ts';
import { firstName } from './format.ts';
import { ownIdentity } from './invite.ts';
import { onStationChange } from './reset.ts';
import { mixCommand } from './rehearsal.ts';
import { chooseDeskStudio, rehearsal, rehearsalPhase, sendRehearsal, takeRehearsalView } from './rehearsal.svelte.ts';
import { LIVE_KEY_HERE, assignSlots, mayRun, movedWords, studioChoice, type Moved } from './rehearsalDesk.ts';
import { say, studio } from './studio.svelte.ts';
import { setup } from './wizard.svelte.ts';

const fresh = () => ({
  /** The session the strips below are for: a new session starts them afresh. */
  session: null as string | null,
  /** Each person's strip in this window, by identity (`assignSlots`). */
  slots: {} as Record<string, number>,
  /** The rehearsed show's cart banks beside the station's, and whose they are. */
  catalogue: null as Catalogue | null,
  catalogueShow: null as string | null,
  /** The station's carts version the catalogue was read at: a cart added or changed on the
   *  Carts tab during a rehearsal moves it, and the glance reads again. */
  catalogueVersion: null as number | null,
  /** Who this window is talking to at the rehearsal desk, by identity (a held key or button). */
  talking: [] as string[],
  /** The station refused a press with 403: its sentence, kept for this session. The screen is
   *  then read-only whatever the window worked out for itself. */
  refused: null as string | null,
  /** The move dialog is open. */
  moveAsk: false,
  /** The move this window pressed, until its standing-by time has passed. */
  moved: null as Moved | null,
  /** A live key pressed here has said so once. */
  told: false,
});

export const rdesk = $state(fresh());

/** Who each held talk opened, by key, so letting go closes that same talk. */
const heldTalk = new Map<string, { me: string; to: string }>();

onStationChange('rehearsal desk', () => {
  heldTalk.clear();
  Object.assign(rdesk, fresh());
});

/** Whether the Desk tab draws its choice of studio: where the station has a rehearsal studio to
 *  speak of (1.3.0 or later), and never in the sandbox, which has no station. */
export function choiceShown(): boolean {
  return !sandboxed() && studioChoice(rehearsalPhase(), rehearsal.view).shown;
}

/** Keep the strips in step with the station's list: called whenever the view changes. */
export function settleSlots(): void {
  const v = rehearsal.view;
  const id = v?.session?.id ?? null;
  if (id !== rdesk.session) {
    rdesk.session = id;
    rdesk.slots = {};
    rdesk.refused = null;
    rdesk.talking = [];
    heldTalk.clear();
  }
  const next = assignSlots(rdesk.slots, v?.people ?? []);
  const same = Object.keys(next).length === Object.keys(rdesk.slots).length && Object.entries(next).every(([k, s]) => rdesk.slots[k] === s);
  if (!same) rdesk.slots = next;
}

/** The person on a strip, or null. */
export function personAt(slot: number): RehearsalPerson | null {
  return rehearsal.view?.people.find((p) => rdesk.slots[p.identity] === slot) ?? null;
}

/** The show's presenter account, where this window has read the shows. */
function presenterOf(show: string | null | undefined): string | null {
  return setup.shows.find((s) => s.slug === show)?.presenter_account ?? null;
}

/** Whether this window may drive the open session: the rule the station applies, unless the
 *  station has already said no. */
export function mayDrive(): boolean {
  const s = rehearsal.view?.session;
  return !!s && !rdesk.refused && mayRun(account.me, presenterOf(s.show));
}

/** Whether this window may open the rehearsal studio for a show. */
export function mayOpen(show: string): boolean {
  return mayRun(account.me, presenterOf(show));
}

/** This window's own person in the rehearsal room (its account's microphone link there), whose
 *  voice Talk carries; null when it has not joined. */
export function meHere(): RehearsalPerson | null {
  const id = account.me?.kind === 'staff' && account.me.account ? ownIdentity(account.me.account.id) : null;
  return (id && rehearsal.view?.people.find((p) => p.identity === id && p.present)) || null;
}

/** Fetch the rehearsed show's carts when the session's show changes, or the station's carts
 *  have changed since (`version`: the live view's `carts.version`, which every change moves). */
export async function settleCatalogue(version: number | null = null): Promise<void> {
  const show = rehearsal.view?.session?.show ?? null;
  const stale = version !== null && rdesk.catalogueVersion !== null && version !== rdesk.catalogueVersion;
  if (show === rdesk.catalogueShow && (rdesk.catalogue || !show) && !stale) return;
  if (show !== rdesk.catalogueShow) rdesk.catalogue = null;
  rdesk.catalogueShow = show;
  rdesk.catalogueVersion = version;
  if (!show) return;
  try {
    const cat = await catalogue(show);
    if (rdesk.catalogueShow === show) rdesk.catalogue = cat;
  } catch {
    // The glance says the carts could not be read; the next view tries again.
    if (rdesk.catalogueShow === show) rdesk.catalogueShow = null;
  }
}

/** A press the station answered: a 403 makes the screen read-only for this session. */
async function press(c: Parameters<typeof sendRehearsal>[0]): Promise<boolean> {
  const ok = await sendRehearsal(c);
  if (!ok && rehearsal.errorStatus === 403) rdesk.refused = rehearsal.error;
  return ok;
}

function talkingTo(identity: string, on: boolean): void {
  rdesk.talking = on ? [...new Set([...rdesk.talking, identity])] : rdesk.talking.filter((x) => x !== identity);
}

async function talkHeld(key: string, held: boolean, open: { me: string; to: string } | null): Promise<void> {
  if (!held) {
    const h = heldTalk.get(key);
    heldTalk.delete(key);
    if (!h) return;
    talkingTo(h.to, false);
    await press({ op: 'talk_to', identity: h.me, to: h.to, on: false });
    return;
  }
  if (!open || heldTalk.has(key)) return;
  heldTalk.set(key, open);
  talkingTo(open.to, true);
  await press({ op: 'talk_to', identity: open.me, to: open.to, on: true });
}

/** Let every held talk go (the window lost the keyboard, or the Desk tab is leaving the
 *  rehearsal desk): each closes the talk it opened, at the rehearsal desk. */
export function releaseRehearsalTalk(): Promise<void> {
  return Promise.all([...heldTalk.keys()].map((k) => talkHeld(k, false, null))).then(() => {});
}

/** Why Talk is off at the rehearsal desk for this window, or null when it works. */
export function talkWhyOff(to: { identity: string } | null): string | null {
  const me = meHere();
  if (!me) return 'Talk is your own voice to one person. Join the rehearsal with your own link first: Invite someone…, as a presenter, in your name.';
  if (to && to.identity === me.identity) return 'This is you: Talk needs someone else to talk to.';
  return null;
}

/** One action at the rehearsal desk: a strip's key or button, or F1 to F6. */
export async function runRehearsal(a: Action): Promise<void> {
  // Letting a talk go always goes through, whatever has changed since it was opened.
  if (a.type === 'talk' && !a.held) return talkHeld(`talk:${a.slot}`, false, null);
  const v = rehearsal.view;
  if (!v?.session) return void say('good', 'No rehearsal is open', 'Open the rehearsal studio for a show first; its people and carts then answer these keys.');
  if (!mayDrive()) {
    return void say('good', `This is ${v.session.opened_by || 'someone else'}’s rehearsal`, rdesk.refused ?? `${v.session.show_name} is not your show, so you can watch its rehearsal and not drive it.`);
  }
  switch (a.type) {
    case 'on':
    case 'cue':
    case 'mute': {
      const p = personAt(a.slot);
      if (!p) return void say('good', `Nobody on strip ${a.slot}`, 'People appear on a strip when they join the rehearsal with their link.');
      if (a.type === 'on') await press(mixCommand(p.identity, !p.on));
      else if (a.type === 'cue') await press({ op: 'cue', identity: p.identity, on: !p.cue });
      else await press({ op: 'mute', identity: p.identity, on: !p.mute });
      return;
    }
    case 'talk': {
      const p = personAt(a.slot);
      if (!p) return;
      const why = talkWhyOff(p);
      if (why) return void say('good', `${firstName(p.name, 'They')} would not hear you`, why);
      const me = meHere();
      await talkHeld(`talk:${a.slot}`, true, me ? { me: me.identity, to: p.identity } : null);
      return;
    }
    case 'cart': {
      const cart = cartAt(rdesk.catalogue, studio.bank, a.n);
      if (!cart) return void say('good', 'No cart here yet', `Slot ${cartKey(a.n)} of this bank is empty. Add carts on the Carts tab: a show’s carts are the same in rehearsal and on air.`);
      if (cart.missing || !cart.file) return void say('serious', 'File missing', `“${cart.label}” has no sound: its file is gone from the station. The key does nothing.`);
      const sounding = v.carts_sounding.some((s) => s.cart === cart.id && !s.stopping);
      await press(sounding ? { op: 'cart_stop', cart: cart.id } : { op: 'cart_fire', cart: cart.id });
      return;
    }
    default:
      return;
  }
}

/** A key that drives the live studio was pressed at the rehearsal desk: said once a session. */
export function tellLiveKey(): void {
  if (rdesk.told) return;
  rdesk.told = true;
  say('good', LIVE_KEY_HERE.title, LIVE_KEY_HERE.message, 8000);
}

/** The frame's own way of letting every held key go, set by App.svelte while it is mounted. */
let frameLetGo: (() => Promise<void>) | null = null;
export function onBeforeStudioSwitch(letGo: (() => Promise<void>) | null): void {
  frameLetGo = letGo;
}

/** The Desk tab's studio, changed from the choice in its head: every held key is let go at the
 *  desk it was opened on before the tab shows the other. */
export async function switchDeskStudio(which: 'live' | 'rehearsal'): Promise<void> {
  if (studio.deskStudio === which) return;
  await frameLetGo?.();
  await releaseRehearsalTalk();
  studio.armed = null;
  chooseDeskStudio(which);
}

/** Move everyone to the live studio (the dialog's Move). True when the station did it: the Desk
 *  tab then shows the live desk, where they stand by. A refusal is returned in the station's
 *  own words, for the dialog. */
export async function moveNow(): Promise<{ ok: true } | { ok: false; why: string }> {
  const s = rehearsal.view?.session;
  if (!s) return { ok: false, why: 'No rehearsal is open.' };
  await releaseRehearsalTalk();
  try {
    const r = await moveRehearsal();
    rdesk.moved = { ...r, show: s.show, show_name: s.show_name };
    rdesk.moveAsk = false;
    // The session is closed; the stream says so within a moment, and the screen need not wait.
    if (rehearsal.view) takeRehearsalView({ ...rehearsal.view, session: null, people: [], carts_sounding: [], window: null });
    chooseDeskStudio('live');
    const w = movedWords(rdesk.moved);
    say('good', w.title, w.message, 12000);
    return { ok: true };
  } catch (e) {
    if (e instanceof FieldError && e.status === 403) rdesk.refused = e.message;
    return { ok: false, why: e instanceof Error && e.message ? e.message : 'The station did not answer. Check the link to the station, then try again.' };
  }
}

/** Forget a move whose standing-by time has passed, or whose people are all on air or gone. */
export function settleMoved(now: number): void {
  const m = rdesk.moved;
  if (!m) return;
  if (now >= m.opening.standby_until_ms) rdesk.moved = null;
}
