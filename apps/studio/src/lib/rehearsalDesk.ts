// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal desk's screens (1.3.0, step R13): the words and rules its panels read, kept out
// of the components so the tests can hold them. The data's own rules are `rehearsal.ts`; the
// window's state is `rehearsalDesk.svelte.ts`. Panels: `desk.studio` (the choice of studio),
// `rehearsal.stage`, `rehearsal.tile.N`, `rehearsal.session`, `rehearsal.carts` and
// `rehearsal.move` (ADR-0070).
//
// Nothing in the rehearsal studio reaches the air, so nothing here is drawn or worded as ON AIR
// or OPEN (ADR-0062): the lamp is REHEARSAL, hollow and dashed in the accent colour, and a
// person is IN THE MIX or NOT IN THE MIX. No new colour: each borrows a shape the vocabulary
// has, as LINK LOST and NOT CONNECTED do.

import type { Action, Tab } from '../../../../packages/ui/keymap.ts';
import type { Chip } from '../../../../packages/ui/states.ts';
import { CHIP } from '../../../../packages/ui/states.ts';
import type { DeskStudio, HandoverWindow, Me, MoveResult, RehearsalPerson, RehearsalView, Source } from './api.ts';
import { firstName, hm } from './format.ts';
import { changesAir } from './prepare.ts';
import type { RehearsalPhase } from './rehearsal.ts';
import { presentPeople } from './rehearsal.ts';

// ---- the vocabulary ----

/** The rehearsal's three words. None has a tooltip sentence of the state vocabulary's, so none
 *  borrows one that speaks of the air. */
export const REHEARSAL_CHIP = {
  /** The lamp: drawn dashed, in the accent colour (`.lamp.rehearsal`, styles/rehearsal.css). */
  lamp: { cls: 'hollow hollow-accent dashed', glyph: 'g-loop', glyphCls: 'g-accent', word: 'Rehearsal' },
  inMix: { cls: 'hollow hollow-accent', glyph: 'g-good', glyphCls: 'g-accent', word: 'In the mix' },
  notInMix: { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Not in the mix' },
} as const satisfies Record<string, Chip>;

/** What the lamp's tooltip says: the one fact the whole screen rests on. */
export const REHEARSAL_LAMP_WHY = 'Nothing here reaches listeners. The rehearsal desk has no way to the air: only the people in the rehearsal studio hear each other and the carts.';

/** The one word a person's tile shows at the rehearsal desk. Muted and Cue are the desk's own
 *  words and keep their shapes; the other two are the rehearsal's. */
export function personChip(p: Pick<RehearsalPerson, 'on' | 'mute' | 'cue'>): Chip {
  if (p.on && p.mute) return CHIP.muted;
  if (p.on) return REHEARSAL_CHIP.inMix;
  if (p.cue) return CHIP.cue;
  return REHEARSAL_CHIP.notInMix;
}

// ---- slots ----

/** The rehearsal desk has six strips with keys, as the live desk has (keymap section 3). */
export const REHEARSAL_SLOTS = 6;

/** Which strip each person has in this window. The station gives a rehearsal person no slot, so
 *  the window keeps its own: a person keeps their strip for as long as the station lists them
 *  (their keys never change, desk.md rule 4), and nobody is renumbered when someone leaves. A
 *  newcomer takes the lowest free strip, presenters before the rest and then by identity, so two
 *  windows that open on the same room agree. Beyond the sixth, a person has a tile and no keys
 *  (slot 0 is never given; they are simply not in the answer). */
export function assignSlots(prev: Readonly<Record<string, number>>, people: readonly Pick<RehearsalPerson, 'identity' | 'role'>[]): Record<string, number> {
  const next: Record<string, number> = {};
  const taken = new Set<number>();
  for (const p of people) {
    const s = prev[p.identity];
    if (s && !taken.has(s)) {
      next[p.identity] = s;
      taken.add(s);
    }
  }
  const lead = (r: string) => (r === 'presenter' ? 0 : r === 'dj' ? 1 : 2);
  const fresh = people.filter((p) => !next[p.identity]).sort((a, b) => lead(a.role) - lead(b.role) || a.identity.localeCompare(b.identity));
  for (const p of fresh) {
    let s = 1;
    while (taken.has(s)) s += 1;
    if (s > REHEARSAL_SLOTS) break;
    next[p.identity] = s;
    taken.add(s);
  }
  return next;
}

/** A rehearsal person in the shape the tile draws. `on_air` is the desk's own field name for
 *  "in the mix" (`mixCommand`); the tile's rehearsal context never words it as the air. */
export function asSource(p: RehearsalPerson, slot: number): Source {
  return {
    identity: p.identity,
    name: p.name,
    role: p.role,
    on_air: p.on,
    mute: p.mute,
    cue: p.cue,
    talk: p.talk,
    talk_to: p.talk_to ?? null,
    present: p.present,
    away_ms: null,
    linked: p.linked,
    lufs: p.lufs ?? null,
    lufs_peak: null,
    slot,
    source: null,
    caller: null,
  } as Source;
}

// ---- the keys ----

/** Where a key or a button goes while the Desk tab shows the rehearsal desk:
 *  - `rehearsal`: to the rehearsal desk, through its own route;
 *  - `swallow`: it would change the live air, which this screen does not show: nothing is sent;
 *  - `live`: the window's own (tabs, Esc, the bank shown), as ever. */
export type KeyRoute = 'live' | 'rehearsal' | 'swallow';

/** The actions the rehearsal desk takes: a strip's four keys, and F1 to F6. */
const toRehearsal = (t: Action['type']): boolean => t === 'on' || t === 'cue' || t === 'mute' || t === 'talk' || t === 'cart';

/** Route one action. Only the Desk tab has a rehearsal desk: on every other tab the keys are
 *  the live studio's, as those tabs show it. On the Desk tab with the rehearsal desk shown, the
 *  slot keys and F1 to F6 drive the rehearsal, and every other key that changes the air (Dump,
 *  the handover, End show, Restart playout, the picture, the callers' keys) does nothing. */
export function routeKey(a: Action, tab: Tab, deskStudio: DeskStudio): KeyRoute {
  if (tab !== 'desk' || deskStudio !== 'rehearsal') return 'live';
  if (toRehearsal(a.type)) return 'rehearsal';
  return changesAir(a) ? 'swallow' : 'live';
}

/** Said once when a live key is pressed at the rehearsal desk. */
export const LIVE_KEY_HERE = {
  title: 'That key drives the live studio',
  message: 'This is the rehearsal studio, so it did nothing. Choose Live studio, at the top left of the Desk tab, to use it.',
} as const;

// ---- who may drive ----

/** Whether this window's account may run a show's rehearsal, as the station decides it
 *  (`steps::may_change`): an admin, a producer, the station key, or the show's own presenter.
 *  The station's answer is final: a 403 on any press makes the screen read-only whatever this
 *  says. `presenterAccount` is the show's, null when it has none or the shows are not read. */
export function mayRun(me: Me | null | undefined, presenterAccount: string | null | undefined): boolean {
  if (!me) return false;
  if (me.kind === 'station') return true;
  if (me.role === 'admin' || me.role === 'producer') return true;
  return !!me.account && !!presenterAccount && me.account.id === presenterAccount;
}

// ---- the studio choice (`desk.studio`) ----

export interface StudioChoice {
  /** Draw the choice at all: a station older than 1.3.0 has no rehearsal studio to offer. */
  shown: boolean;
  /** The Rehearsal studio segment can be chosen. */
  can: boolean;
  /** A short mark beside the segment's word: `off`, `starting`, or a show's name. */
  mark: string;
  /** The segment's tooltip: what it is, or why it cannot be chosen. */
  why: string;
}

export const REHEARSAL_OFF_WHY = 'The rehearsal studio is off at this station. An admin turns it on in the installation’s settings: REHEARSAL=true.';
export const REHEARSAL_WHAT = 'A second desk that cannot reach the air: run through your show with your guests and carts, then move everyone to the live studio.';

/** The choice in the Desk tab's head, from where the rehearsal studio stands. Off is drawn and
 *  cannot be chosen, with the setting that turns it on; a desk that is not answering can be
 *  chosen, because the screen then says what is wrong. */
export function studioChoice(phase: RehearsalPhase, view: RehearsalView | null): StudioChoice {
  switch (phase) {
    case 'absent':
    case 'unknown':
      return { shown: false, can: false, mark: '', why: '' };
    case 'off':
      return { shown: true, can: false, mark: 'off', why: REHEARSAL_OFF_WHY };
    case 'desk-down':
      return { shown: true, can: true, mark: 'not answering', why: `The rehearsal desk is not answering${view?.desk.error ? `: ${view.desk.error}` : ''}. It starts again by itself.` };
    case 'open':
      return { shown: true, can: true, mark: view?.session?.show_name ?? '', why: `${view?.session?.opened_by || 'Someone'} is rehearsing ${view?.session?.show_name ?? 'a show'}. ${REHEARSAL_WHAT}` };
    default:
      return { shown: true, can: true, mark: '', why: REHEARSAL_WHAT };
  }
}

// ---- the read-only line in Settings (screen 9) ----

/** Whether the rehearsal studio is on at this station, and the setting that turns it on. The
 *  setting is the installation's (`REHEARSAL`), so the line is to read: nothing here changes it.
 *  Null where there is nothing true to say yet (not read, or a station older than 1.3.0). */
export function settingsLine(phase: RehearsalPhase, view: RehearsalView | null): { state: string; rest: string } | null {
  if (phase === 'unknown' || phase === 'absent') return null;
  const how = 'It is the installation’s setting REHEARSAL (true or false), changed where the station was installed, not here.';
  if (phase === 'off') return { state: 'off', rest: `at this station. ${how}` };
  const room = `It takes ${view?.max_people ?? 6} people at once and has no way to the air.`;
  if (!view?.desk.linked) return { state: 'on', rest: `at this station, and its desk is not answering: it starts again by itself. ${how}` };
  return { state: 'on', rest: `at this station. ${room} ${how}` };
}

// ---- the stage's words ----

/** The line under the show's name on the rehearsal stage. */
export function stageSub(view: RehearsalView | null): string {
  if (!view?.session) return '';
  const here = presentPeople(view);
  const mix = here.filter((p) => p.on && !p.mute).length;
  const people = here.length === 0 ? 'Nobody is here yet' : `${mix} of ${here.length} in the mix`;
  return `${people} · nothing here reaches listeners`;
}

/** The window line (build plan section 1.6): when the rehearsed show is on air, and that the
 *  move is a press. Null while the live studio's window is about another show, or not open. */
export function windowLine(w: HandoverWindow | null | undefined, now: number): { text: string; due: boolean } | null {
  if (!w) return null;
  const at = hm(w.start_ms);
  if (now >= w.start_ms) return { text: `${w.show_name} was due on air at ${at}: move now`, due: true };
  return { text: `On air at ${at}: move by then`, due: false };
}

// ---- the move (`rehearsal.move`) ----

export const MOVE = {
  button: 'Move to the live studio…',
  title: (show: string) => `Move ${show} to the live studio?`,
  /** The plan's sentence, word for word. */
  lede: 'Nobody goes on air until the handover. Their links keep working.',
  then: 'The rehearsal ends. Everyone arrives at the live desk standing by, under Not on air.',
  confirm: 'Move',
  stay: 'Stay here',
  nobody: 'Nobody is in the rehearsal studio, so there is nobody to move.',
} as const;

export interface MoveRow {
  identity: string;
  name: string;
  /** `Presenter`, `Guest`, `Remote DJ`. */
  role: string;
  /** What the handover does with them: the opening state, in words. */
  opening: string;
  /** For the mark beside the words. */
  kind: 'on' | 'muted' | 'off';
}

const ROLE_WORD: Record<string, string> = { presenter: 'Presenter', dj: 'Remote DJ', guest: 'Guest' };

/** Who moves, each with their opening: on at the handover, on and muted, or standing by until
 *  put on. Only the people present move; someone who has left is not in the list. */
export function moveRows(view: RehearsalView | null): MoveRow[] {
  return presentPeople(view).map((p) => ({
    identity: p.identity,
    name: p.name || p.identity,
    role: ROLE_WORD[p.role] ?? p.role,
    opening: p.on ? (p.mute ? 'On at the handover, muted' : 'On at the handover') : 'Stands by until you put them on',
    kind: p.on ? (p.mute ? 'muted' : 'on') : 'off',
  }));
}

// ---- after the move, at the live desk (`desk.stage`) ----

/** What this window remembers of a move it pressed: the station's `/studio` view does not say
 *  who came from rehearsal, so only the window that pressed Move can. */
export interface Moved extends MoveResult {
  show: string;
  show_name: string;
}

/** Whether a person at the live desk still stands by from the move: they were moved, they are
 *  at the desk and not on air, the opening has not been applied by this window's knowledge, and
 *  the standing-by time is still ahead. */
export function fromRehearsal(moved: Moved | null, s: Pick<Source, 'identity' | 'on_air' | 'present'>, now: number): boolean {
  return !!moved && now < moved.opening.standby_until_ms && moved.moved.includes(s.identity) && s.present && !s.on_air;
}

/** The note on a moved person's tile, in the place of the tile's own note (one line). */
export function fromRehearsalNote(moved: Moved, identity: string): string {
  const on = moved.opening.on.includes(identity);
  const muted = moved.opening.mute.includes(identity);
  return `From rehearsal · ${on ? (muted ? 'on at the handover, muted' : 'on at the handover') : 'stands by until you put them on'}`;
}

/** `Eilidh`, `Eilidh and Gregor`, `Eilidh, Gregor and Kirsty`. */
function listNames(names: string[]): string {
  return names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}` : (names[0] ?? '');
}

/** The post line's sentence on the opening state: who the handover puts on. Null once nobody
 *  moved still stands by (the opening was applied, they left, or the time passed). */
export function openingLine(moved: Moved | null, sources: readonly Pick<Source, 'identity' | 'name' | 'on_air' | 'present'>[], now: number): string | null {
  if (!moved) return null;
  const waiting = sources.filter((s) => fromRehearsal(moved, s, now));
  if (!waiting.length) return null;
  const on = waiting.filter((s) => moved.opening.on.includes(s.identity)).map((s) => firstName(s.name, s.identity));
  if (!on.length) return `From rehearsal: ${waiting.length === 1 ? `${firstName(waiting[0].name, waiting[0].identity)} stands` : `${waiting.length} people stand`} by until you put them on`;
  return `The handover puts ${listNames(on)} on air · from rehearsal`;
}

/** The toast after a move, at the live desk. */
export function movedWords(m: Moved): { title: string; message: string } {
  const n = m.moved.length;
  return {
    title: `${m.show_name} is in the live studio`,
    message: `${n === 1 ? 'One person is' : `${n} people are`} standing by under Not on air. Nobody is on air until the handover at ${hm(m.airing_start_ms)}.`,
  };
}
