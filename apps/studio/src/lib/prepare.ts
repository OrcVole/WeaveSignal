// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Prepare mode (ADR-0276), the rules without a page: a staff window is Live or Preparing. A
// preparing window shows the live desk but cannot drive it, so the next DJ can set up their own
// show (the Schedule tab, Settings, the show's step) while someone else's is on air. Here: who
// starts preparing, which actions change what is on air, when switching to Live asks first, and
// every word the frame, its band and its dialog say. Pure, so the rules are tested on their own.
//
// The station is the authority: it keeps the flag on the session and refuses a preparing window's
// commands with 409 and its own sentence. These rules keep the window in line with it, so a key
// does nothing rather than earning a refusal.
//
// "Mode" already means the station's (live or auto, ADR-0183), so this is always "prepare" in the
// code, and "this window" on screen.

import type { Me, StepView } from './api.ts';
import type { Action, TwoPress } from '../../../../packages/ui/keymap.ts';

/** A window's two states, as the words on screen. */
export type WindowIs = 'live' | 'preparing';
export const WINDOW_WORD: Record<WindowIs, string> = { live: 'Live', preparing: 'Preparing' };

/** The station's own sentence when a preparing window asks for something that changes the air
 *  (`PREPARING_REFUSAL` in the control plane). The window shows the station's answer as given;
 *  this copy is for the sandbox's simulated station and the tests. */
export const PREPARING_REFUSAL = 'This window is preparing a show, so it cannot change what is on air. Switch it to Live first.';

/** A show as these rules need it: its path, its name and the account that presents it. */
export interface ShowRef {
  slug: string;
  name: string;
  presenter_account?: string | null;
}

/** Whether this window can prepare at all: a member of staff signed in as themselves, on a
 *  station that reports the flag. The station key is always live, and a control plane older than
 *  Prepare mode has no route to ask. */
export function canPrepare(me: Me | null | undefined): boolean {
  return !!me && me.kind === 'staff' && typeof me.preparing === 'boolean';
}

/** Whether this window is preparing, as the station last said. */
export function isPreparing(me: Me | null | undefined): boolean {
  return canPrepare(me) && me!.preparing === true;
}

/** Every show this account presents (ADR-0218 ties a show to its presenter's account). */
export function showsOf(me: Me | null | undefined, shows: readonly ShowRef[]): ShowRef[] {
  const id = me?.kind === 'staff' ? me.account?.id : undefined;
  if (!id) return [];
  return shows.filter((s) => s.presenter_account === id);
}

/** What is on air by the schedule, as this account sees it: one of its own shows, someone
 *  else's, or none. `step` is the view's: the show on air now, or the one last on air. */
export type OnAir = { whose: 'mine' | 'other'; name: string } | { whose: 'none' };

export function onAirFor(me: Me | null | undefined, shows: readonly ShowRef[], step: StepView | null | undefined): OnAir {
  if (!step?.on_air || !step.show) return { whose: 'none' };
  const mine = showsOf(me, shows).some((s) => s.slug === step.show!.slug);
  return { whose: mine ? 'mine' : 'other', name: step.show.name };
}

/** The default when a window starts (ADR-0276): the presenter of a show that is not on air now
 *  starts Preparing. Everyone else starts Live, as before: an account that presents no show, the
 *  presenter whose show is on air, the station key. "On air" is the schedule's word, so a
 *  presenter who opens the studio before their slot starts Preparing, and switches to Live in
 *  one press. */
export function startsPreparing(me: Me | null | undefined, shows: readonly ShowRef[], step: StepView | null | undefined): boolean {
  if (!canPrepare(me)) return false;
  if (showsOf(me, shows).length === 0) return false;
  return onAirFor(me, shows, step).whose !== 'mine';
}

/** The show whose own cart banks this window works with, beside the station's (a cart wall per
 *  show, 1.0.2): a live window's is the show the station's step comes from (on air, or between
 *  shows the one last on air); a preparing window's is the show it prepares, which is this
 *  account's show that comes on next, else its first. A window with no such show works with
 *  the station's banks alone. */
export function cartShowFor(preparing: boolean, me: Me | null | undefined, shows: readonly ShowRef[], step: StepView | null | undefined): string | null {
  if (!preparing) return step?.show?.slug ?? null;
  const own = showsOf(me, shows);
  const next = step?.next?.slug;
  return own.find((s) => s.slug === next)?.slug ?? own[0]?.slug ?? null;
}

/** Switching to Live asks once while someone else's show is on air: the one deliberate act that
 *  takes the desk (ADR-0276). With no show on air, or this account's own, there is nobody to
 *  take it from. */
export function liveAsks(air: OnAir): boolean {
  return air.whose === 'other';
}

// ---- what changes the air ----

/** The two-press actions that change the room: arming one is the first half of doing it. The
 *  chat's own (influx, ban) stay, as the chat stays open to a preparing window. */
const ARMS_AIR: Record<TwoPress, boolean> = { dump: true, drop: true, 'fade-all': true, 'end-pass': true, influx: false, ban: false };

/** Every action, sorted once: `air` goes to a route the station refuses a preparing window (the
 *  desk's, the carts' and the screener's commands, ending a pass or the show, the station's
 *  mode); `free` stays this window's own, or goes to the chat. A record over every action type,
 *  so a new action cannot be added without being sorted. */
const AIR: Record<Action['type'], 'air' | 'free' | ((a: never) => boolean)> = {
  tab: 'free',
  arm: (a: Extract<Action, { type: 'arm' }>) => ARMS_AIR[a.what],
  disarm: 'free',
  escape: 'free', // Esc still dismisses; `run` leaves out its two sends (decline, clear the cue)
  'leave-field': 'free',
  // the global layer
  dump: 'air',
  drop: 'air',
  'end-pass': 'air',
  'end-show': 'air',
  'restart-playout': 'air',
  'restart-encoder': 'air',
  'backup-stream': 'air',
  'talk-seat': 'air', // held or let go: the Screener tab's release sends too
  cart: 'air',
  picture: 'air',
  bank: 'free', // which bank this window shows
  // the desk
  on: 'air',
  cue: 'air',
  mute: 'air',
  talk: 'air',
  handover: 'air',
  'take-caller': 'air',
  // the carts
  'cart-key': 'air',
  'cart-cue': 'air',
  'cart-stop-last': 'air',
  'fade-all': 'air',
  'cart-enter': 'air',
  'cart-move': 'free',
  // the screener's callers
  'screen-talk': 'air',
  send: 'air',
  back: 'air',
  'release-place': 'air',
  'focus-name': 'air', // the name on air is a screener command to save
  'queue-move': 'free',
  'queue-jump': 'free',
  // the chat: its own service, open to a preparing window
  influx: 'free',
  mode: 'free',
  'mod-hide': 'free',
  'mod-timeout': 'free',
  ban: 'free',
  'mod-leave': 'free',
  'mod-release': 'free',
  'release-all': 'free',
  'hide-all': 'free',
  acknowledge: 'free',
  escalate: 'free',
  feature: 'free',
  pin: 'free',
  invite: 'free',
  reply: 'free',
  pass: 'free',
  newest: 'free',
  post: 'free',
  'avatar-remove': 'free',
  'attachment-remove': 'free',
  'sor-then': 'free',
  'sor-reason': 'free',
  'sor-send': 'free',
  'sor-cancel': 'free',
  // the schedule
  week: 'free',
  today: 'free',
};

/** Whether an action changes what is on air, so a preparing window must not run it. */
export function changesAir(a: Action): boolean {
  const rule = AIR[a.type];
  return typeof rule === 'function' ? (rule as (a: Action) => boolean)(a) : rule === 'air';
}

// ---- the words ----

/** `This window: Preparing. Change it`: the frame button's accessible name. */
export function prepareLabel(is: WindowIs): string {
  return `This window: ${WINDOW_WORD[is]}. Change it`;
}

/** The frame button's tooltip, and the line beside each choice in the dialog. */
export const WINDOW_LINE: Record<WindowIs, string> = {
  live: 'Drives the desk: its keys and buttons change what is on air',
  preparing: 'Shows the live desk without driving it, while you set up your own show',
};

export const prepareTitle = (is: WindowIs): string => (is === 'live' ? 'This window is live: it drives the desk' : 'This window is preparing: it shows the live desk and cannot change what is on air');

/** The tooltip on a control a preparing window has switched off. */
export const OFF_WHY = 'Off while this window is preparing. Switch it to Live first.';

/** The dialog's lede. */
export const PREPARE_LEDE = 'A studio window is Live or Preparing. Only a live window can change what is on air, so you can set up your own show in a preparing one while someone else’s is on air.';

export interface PrepareChanges {
  heading: string;
  lines: string[];
  /** Someone else's show is on air: the box is edged, and this is the ask. */
  ask: boolean;
}

/** What the dialog says before a switch (the step dialog's grammar: picking changes nothing
 *  yet, the box says what will change, the button names the move). */
export function prepareChanges(to: WindowIs, air: OnAir): PrepareChanges {
  if (to === 'preparing') {
    return {
      heading: 'Switching to Preparing:',
      lines: [
        'The desk, the carts and the callers stay in view, with their controls off: no key or button in this window changes what is on air.',
        'The Schedule tab, Settings, invitations, the chat and your show’s step work as usual.',
        'Only this window changes. Nothing changes on air.',
      ],
      ask: false,
    };
  }
  const heading = 'Switching to Live:';
  if (air.whose === 'other') {
    return {
      heading,
      lines: [
        `${air.name} is on air. This window’s keys and buttons will act on it at once: they put people on air, mute them, fire carts and end the show.`,
        'Switch when you are taking over the desk. To get your own show ready, keep preparing.',
      ],
      ask: true,
    };
  }
  if (air.whose === 'mine') {
    return { heading, lines: [`${air.name} is on air. This window’s keys and buttons drive the desk.`], ask: false };
  }
  return { heading, lines: ['No show is on air by the schedule. This window’s keys and buttons drive the desk.'], ask: false };
}

/** The dialog's buttons: the way out keeps what is, the primary names the move. */
export function prepareButtons(now: WindowIs): { keep: string; move: string } {
  return now === 'preparing' ? { keep: 'Keep preparing', move: 'Switch to Live' } : { keep: 'Stay live', move: 'Switch to Preparing' };
}

export interface PrepareBand {
  /** The bold lead. */
  lead: string;
  /** The sentence after it. */
  rest: string;
  /** The button: `…` when it asks first; `primary` when this account's own show is on air. */
  button: string;
  primary: boolean;
}

/** The band under the frame while this window is preparing: what the window is, what that means
 *  on this surface, and the way to switch. */
export function prepareBand(air: OnAir): PrepareBand {
  if (air.whose === 'mine') {
    return { lead: `${air.name} is on air, and this window is still preparing.`, rest: 'Switch it to Live to drive the desk.', button: 'Switch to Live', primary: true };
  }
  if (air.whose === 'other') {
    return {
      lead: 'This window is preparing.',
      rest: `${air.name} is on air: follow it here. Nothing in this window changes what is on air; the Schedule tab and Settings work as usual.`,
      button: 'Switch to Live…',
      primary: false,
    };
  }
  return {
    lead: 'This window is preparing.',
    rest: 'It shows the live desk. Nothing in it changes what is on air; the Schedule tab and Settings work as usual.',
    button: 'Switch to Live',
    primary: false,
  };
}
