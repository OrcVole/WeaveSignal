// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The beginner steps (ADR-0271, ADR-0274), the rules without a page: the four steps and their
// words, what this window draws at each, which keys a step switches off, what the step dialog
// says will change, and the desk's queue of callers at the Callers step. Pure, so the rules
// are tested on their own.
//
// Two steps are in play, and every rule here names which it reads:
//  - the **station's** step (`View.step.step`): what is switched on. It decides commands and
//    their keys (Dump, the handover, the picture, taking a caller).
//  - this window's **look**: what is drawn. It is the station's step, or `full` while Show
//    everything is on, which reveals the studio and switches nothing on (ADR-0274).

import type { Caller, ShowStep, Source, StepView, View } from './api.ts';
import type { Action, Tab } from '../../../../packages/ui/keymap.ts';
import { firstName, hm } from './format.ts';
import { parts } from '../schedule/model.ts';

export const STEPS: readonly ShowStep[] = ['solo', 'guests', 'callers', 'full'];

export const STEP_WORD: Record<ShowStep, string> = { solo: 'Solo', guests: 'Guests', callers: 'Callers', full: 'Full' };

/** One line a step, for the dialog and the wizard. */
export const STEP_SHORT: Record<ShowStep, string> = {
  solo: 'Just you',
  guests: 'You and guests you invite by link',
  callers: 'Guests, and listeners who call in',
  full: 'A screener on the phones, the picture, the next DJ',
};

/** What each step adds, for the show editor. */
export const STEP_LONG: Record<ShowStep, string> = {
  solo: 'Just you: your microphone, the music and carts, the listener count, the chat to read',
  guests: 'Adds guests you invite by link',
  callers: 'Adds listeners who call in; you answer them at the desk, and Dump is on',
  full: 'Adds a screener on the phones, the picture, moderating the chat, handing over to the next DJ',
};

export const rank = (s: ShowStep): number => STEPS.indexOf(s);
export const atLeast = (s: ShowStep, need: ShowStep): boolean => rank(s) >= rank(need);

/** A step from anything: an unknown word is `full`, the studio as it was before steps. */
export function asStep(s: unknown): ShowStep {
  return STEPS.includes(s as ShowStep) ? (s as ShowStep) : 'full';
}

/** The station's step; a view without one (none yet) runs everything. */
export function stationStep(v: { step?: StepView } | null | undefined): ShowStep {
  return asStep(v?.step?.step);
}

/** What this window draws: the station's step, or everything while Show everything is on. */
export function lookOf(step: ShowStep, showAll: boolean): ShowStep {
  return showAll ? 'full' : step;
}

/** `Step: Solo, 1 of 4. Change the step`: the frame button's accessible name. */
export function stepLabel(step: ShowStep): string {
  return `Step: ${STEP_WORD[step]}, ${rank(step) + 1} of 4. Change the step`;
}

/** The frame button's tooltip: whose step it is. */
export function stepTitle(s: StepView | null | undefined): string {
  if (!s?.show) return 'No show has been on air yet: the studio shows everything';
  return s.on_air ? `The step of ${s.show.name}` : `No show is on air: the step of ${s.show.name}, the show last on air`;
}

// ---- what is drawn ----

/** The parts a step brings, each with the step it arrives at (design section 1). A part not
 *  listed is drawn at every step. */
export const ARRIVES = {
  /** The Screener tab and the Chat tab. */
  'tab.screener': 'full',
  'tab.chat': 'full',
  /** The Not on air row, with playout's tile and Invite someone…. */
  waiting: 'guests',
  invite: 'guests',
  /** The role DJ in the invitation dialog. */
  'invite.dj': 'full',
  /** The profanity delay's figure and Dump. */
  delay: 'callers',
  dump: 'callers',
  /** The stage's Screener button. */
  'screener-button': 'full',
  picture: 'full',
  handover: 'full',
  /** The status line's talk rule, A to H. */
  'talk-rule': 'guests',
  /** The status line's Talk to the screener, ⌃T. */
  'talk-screener': 'full',
} as const satisfies Record<string, ShowStep>;

export type Part = keyof typeof ARRIVES;

/** Whether this window draws a part. */
export function shown(part: Part, look: ShowStep): boolean {
  return atLeast(look, ARRIVES[part]);
}

/** A part drawn only because everything is shown: the station keeps it switched off, so it
 *  carries the dashed tag and its controls are disabled. */
export function offHere(part: Part, step: ShowStep, look: ShowStep): boolean {
  return shown(part, look) && !atLeast(step, ARRIVES[part]);
}

/** `off at Solo`: the dashed tag's words. */
export const offTag = (step: ShowStep): string => `off at ${STEP_WORD[step]}`;

/** The tabs this window draws: Desk, Carts and Schedule at every step, the rest at Full. */
export function tabShown(tab: Tab, look: ShowStep): boolean {
  return tab === 'screener' ? shown('tab.screener', look) : tab === 'chat' ? shown('tab.chat', look) : true;
}

/** `desk.step-note`, the guest's empty seat: at Solo only. */
export const stepNoteShown = (look: ShowStep): boolean => look === 'solo';

/** `desk.callers`: at the Callers step only; at Full the queue is the Screener tab's. */
export const deskCallersShown = (look: ShowStep): boolean => look === 'callers';

/** At Solo nothing moves: every tile stays on the stage, on air or off, and the Not on air row
 *  is not drawn (the design sets ADR-0061's move rule aside for this one step). Someone left
 *  from a higher step keeps their tile the same way. */
export function stageOf(sources: readonly Source[], look: ShowStep): { stage: Source[]; waiting: Source[] } {
  if (!shown('waiting', look)) return { stage: [...sources], waiting: [] };
  return { stage: sources.filter((s) => s.on_air), waiting: sources.filter((s) => !s.on_air) };
}

/** The stage offers the DJ their own microphone link while nobody holds the presenter's seat (no
 *  presenter in the room, and no DJ on air, who holds it after a handover: ADR-0231), at every step
 *  (the operator, 2026-11-12; first built for Solo only, ADR-0274). Above Solo the station lets any
 *  member of staff make one, so the stage offers it only to those who may change the show's step
 *  (an admin, a producer, the show's presenter), not to a screener's window. */
export function needsOwnLink(sources: readonly Source[], look: ShowStep, mayChange: boolean, signedIn = true): boolean {
  // The link is made for the signed-in account: a window on the station key has none to make it
  // for (the operator's 1.0.2 run: the station refused it), so it is not offered there.
  if (!signedIn) return false;
  if (sources.some((s) => s.role === 'presenter' || (s.role === 'dj' && s.on_air))) return false;
  return look === 'solo' || mayChange;
}

// ---- keys of hidden parts ----

/** What a key press meets: it goes ahead, or its part is off and the press does nothing; `told`
 *  names the part for the one note a session gets. */
export type Gate = { allow: true } | { allow: false; told: OffPart };

export type OffPart = 'dump' | 'drop' | 'screener-tab' | 'chat-tab' | 'talk-screener' | 'handover' | 'picture' | 'take-caller';

const GO: Gate = { allow: true };
const no = (told: OffPart): Gate => ({ allow: false, told });

/** Whether a caller is still in the room on a strip: Drop works on them at any step. */
function callerOnDesk(v: View | null): boolean {
  return !!v && (v.sources.some((s) => s.role === 'caller') || v.callers.some((c) => c.on_air));
}

/** A key's action against the steps (design section 4). `step` is the station's and decides
 *  what is switched on; `look` is this window's and decides which tabs there are. A key that
 *  acts on a person still in the room from an earlier step works until they have left. */
export function gate(a: Action, step: ShowStep, look: ShowStep, v: View | null): Gate {
  switch (a.type) {
    case 'tab':
      if (a.tab === 'screener' && !tabShown('screener', look)) return no('screener-tab');
      if (a.tab === 'chat' && !tabShown('chat', look)) return no('chat-tab');
      return GO;
    case 'dump':
      return atLeast(step, 'callers') ? GO : no('dump');
    case 'drop':
      return atLeast(step, 'callers') || callerOnDesk(v) ? GO : no('drop');
    case 'arm':
      if (a.what === 'dump') return atLeast(step, 'callers') ? GO : no('dump');
      if (a.what === 'drop') return atLeast(step, 'callers') || callerOnDesk(v) ? GO : no('drop');
      return GO;
    case 'talk-seat':
      // Letting go always goes through, so a talk held across a step change still closes.
      return !a.held || atLeast(step, 'full') ? GO : no('talk-screener');
    case 'handover':
      return atLeast(step, 'full') ? GO : no('handover');
    case 'picture':
      return atLeast(step, 'full') ? GO : no('picture');
    case 'take-caller':
      // The Desk tab's P belongs to `desk.callers`, which is drawn at the Callers step only
      // (ADR-0275). At Full the queue is the Screener tab's and the key does nothing, silently
      // (`offNote` has no words for it there).
      return deskCallersShown(look) ? GO : no('take-caller');
    default:
      return GO;
  }
}

/** The note a hidden part's key shows, once a session, without a tone colour: the title, one
 *  sentence, and nothing else. Null when the press should say nothing (P on the Desk tab at
 *  Full, where the queue is the Screener tab's). */
export function offNote(part: OffPart, step: ShowStep): { title: string; message: string } | null {
  const at = STEP_WORD[step];
  const runs = `This show runs at ${at}`;
  switch (part) {
    case 'dump':
      return { title: `Dump is off at ${at}`, message: `${runs}, so there is nothing for Dump to do. It comes on at Callers.` };
    case 'drop':
      return { title: `Drop is off at ${at}`, message: `${runs}, so there is no caller to drop. Call-in comes with Callers.` };
    case 'take-caller':
      if (atLeast(step, 'callers')) return null;
      return { title: `Taking a caller is off at ${at}`, message: `${runs}, so the lines are closed. Call-in comes with Callers.` };
    case 'screener-tab':
      return { title: `The Screener tab is off at ${at}`, message: `${runs}, so nobody is on the phones. The Screener tab comes with Full.` };
    case 'chat-tab':
      return { title: `The Chat tab is off at ${at}`, message: `${runs}. The Chat tab comes with Full.` };
    case 'talk-screener':
      return { title: `Talk to the screener is off at ${at}`, message: `${runs}, so the screener's seat is empty. It comes with Full.` };
    case 'handover':
      return { title: `The handover is off at ${at}`, message: `${runs}, so there is no next DJ to hand over to. It comes on at Full.` };
    case 'picture':
      return { title: `The picture is held on the slate at ${at}`, message: `${runs}, so the slate holds the picture. Switching it comes with Full.` };
  }
}

// ---- the step dialog ----

/** A sentence with its bold parts, as the dialog draws it. */
export type Rich = (string | { b: string })[];
const b = (s: string) => ({ b: s });

/** The plain words of a rich line (tests, and a screen reader's label). */
export const plain = (r: Rich): string => r.map((p) => (typeof p === 'string' ? p : p.b)).join('');

/** `Niall`, `Niall and Jessie`, `Niall, Jessie and Calum`. */
export function listNames(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

export interface StepChanges {
  /** `Moving to Guests, at once:`, or `Moving to Guests:` when the show is not on air. */
  heading: string;
  lines: Rich[];
  /** People are affected (a move down with someone waiting or present): the box is edged. */
  people: boolean;
  /** `Saved to The Morning Kettle. Each show keeps its own step.` */
  last: string;
}

const UP: Record<Exclude<ShowStep, 'solo'>, Rich[]> = {
  guests: [[b('Invite someone…'), ' appears on the desk, and a guest can join by link.'], ['The Not on air row and talkback appear.']],
  callers: [['The call-in page opens: listeners can call from now.'], ['Callers wait in a new panel on the desk, and Dump comes on.']],
  full: [['The Screener tab opens, and its seat can be taken.'], ['The picture can be switched, and the next DJ can be invited.']],
};

/** What stays off after a move up that stops short of Full. */
function staysOff(to: ShowStep): Rich | null {
  if (to === 'guests') return ['Call-in stays closed. The screener’s seat, the picture and the handover stay off.'];
  if (to === 'callers') return ['The screener’s seat, the picture and the handover stay off.'];
  return null;
}

const short = (p: { name: string; identity: string; caller?: Source['caller'] }): string => p.caller?.name_on_air || firstName(p.name, p.identity);

/** Callers who are only waiting: in the queue, on no strip, not on air or in cue. A falling
 *  step lets them go (`releasable` in the control plane's studio.rs). */
export function onlyWaiting(v: View | null): Caller[] {
  return (v?.callers ?? []).filter((c) => c.slot === null && !c.on_air && !c.cue);
}

/** What the dialog says before a move: what appears, what stays off, and what happens to the
 *  people present (design section 2). `from` is the show's step now. */
export function stepChanges(from: ShowStep, to: ShowStep, v: View | null, show: string, onAir: boolean): StepChanges {
  const heading = `Moving to ${STEP_WORD[to]}${onAir ? ', at once' : ''}:`;
  const last = `Saved to ${show}. Each show keeps its own step.`;
  const lines: Rich[] = [];
  let people = false;
  if (rank(to) > rank(from)) {
    for (const s of STEPS.slice(rank(from) + 1, rank(to) + 1)) lines.push(...UP[s as Exclude<ShowStep, 'solo'>]);
    const off = staysOff(to);
    if (off) lines.push(off);
    return { heading, lines, people, last };
  }
  // Down. The people named are the ones this window's view holds, so only while the show is the
  // one on air: off air its step closes no door yet.
  const sources = onAir ? (v?.sources ?? []) : [];
  if (atLeast(from, 'full') && !atLeast(to, 'full')) {
    lines.push(['The Screener tab closes, and its seat cannot be taken. The picture is held on the slate, and the handover goes off.']);
  }
  if (atLeast(from, 'callers') && !atLeast(to, 'callers')) {
    const waiting = onAir ? onlyWaiting(v).map((c) => c.name_on_air || firstName(c.name, c.identity)) : [];
    const closed: Rich = ['The call-in page says the lines are closed.'];
    if (waiting.length) {
      people = true;
      closed.push(' ', b(listNames(waiting)), waiting.length === 1 ? ' is waiting: they are told the lines have closed.' : ' are waiting: they are told the lines have closed.');
    }
    lines.push(closed);
  }
  if (atLeast(from, 'guests') && !atLeast(to, 'guests')) {
    lines.push([b('Invite someone…'), ' and the Not on air row go: no guest can join by link.']);
  }
  // Who is in a seat the new step closes: they stay until they leave.
  const stays: Rich[] = [];
  if (!atLeast(to, 'callers')) {
    for (const s of sources.filter((x) => x.role === 'caller')) stays.push([b(short(s)), ` stays on strip ${s.slot} until you drop the call`]);
  }
  if (!atLeast(to, 'guests')) {
    for (const s of sources.filter((x) => x.role === 'guest')) stays.push([b(short(s)), ' stays until you end their pass']);
  }
  if (!atLeast(to, 'full')) {
    for (const s of sources.filter((x) => x.role === 'dj')) stays.push([b(short(s)), ' stays until you end their pass']);
  }
  if (stays.length) {
    people = true;
    const line: Rich = [];
    stays.forEach((r, i) => line.push(...(i ? [', and '] : []), ...r));
    line.push('. Nobody new can join.');
    lines.push(line);
  }
  if (atLeast(from, 'callers') && !atLeast(to, 'callers')) lines.push(['Dump goes off.']);
  return { heading, lines, people, last };
}

/** The dialog's lede: whose step, whether it is on air, and who may change it. `presenter` is
 *  the show's presenter by name, when the window knows it. */
export function stepLede(s: StepView | null | undefined, presenter: string, zone = ''): Rich {
  if (!s?.show)
    return [
      'Steps belong to a show, and no show has been on air yet, so the studio shows everything and there is no step to change. Make a show and give it an episode on the ',
      b('Schedule'),
      ' tab: a new show starts at Solo, and its step applies while it is on air.',
    ];
  const at = STEP_WORD[asStep(s.step)];
  if (!s.may_change) {
    const who: Rich = presenter ? [b(presenter)] : ['The show’s presenter'];
    return [s.on_air ? `On air now, at ${at}. ` : `No show is on air; the station keeps ${s.show.name}’s step, ${at}. `, ...who, ', a producer or an admin can change the step. You can look at the whole studio without changing anything.'];
  }
  if (s.on_air) return ['On air now. A step shows the parts of the studio this show uses and switches the rest off, so nothing can wait unseen.'];
  const next: Rich = s.next
    ? [' until ', b(s.next.name), ` starts at ${stationHm(s.next.start_ms, zone)}, which runs at ${STEP_WORD[asStep(s.next.step)]}`]
    : [];
  return ['No show is on air. The station keeps the step of the show last on air', ...next, `. A change here changes ${s.show.name}.`];
}

/** `10:05` on the station's wall clock; this machine's when the zone is not known. */
function stationHm(ms: number, zone: string): string {
  if (zone) {
    try {
      const p = parts(ms, zone);
      return `${String(p.hh).padStart(2, '0')}:${String(p.mm).padStart(2, '0')}`;
    } catch {
      // An unknown zone: this machine's clock.
    }
  }
  return hm(ms);
}

/** The dialog's title. */
export function stepHeading(s: StepView | null | undefined): string {
  if (!s?.show) return 'Step';
  return s.on_air ? `Step for ${s.show.name}` : `Step from ${s.show.name}`;
}

/** What a key does in the step dialog, which owns the keyboard while open (keymap rule 5):
 *  ↑ ↓ move between steps, ⏎ confirms a move, Esc keeps the step. Null leaves the key to the
 *  browser (Tab, Space on a button). `locked`: this account may not change the step. */
export type MenuKey = { type: 'pick'; step: ShowStep } | { type: 'confirm' } | { type: 'close' } | null;

export function menuKey(key: string, picked: ShowStep, now: ShowStep, locked: boolean): MenuKey {
  if (key === 'Escape') return { type: 'close' };
  if (locked) return null;
  if (key === 'ArrowDown' || key === 'ArrowUp') {
    const i = Math.max(0, Math.min(STEPS.length - 1, rank(picked) + (key === 'ArrowDown' ? 1 : -1)));
    return { type: 'pick', step: STEPS[i] };
  }
  if (key === 'Enter') return picked === now ? null : { type: 'confirm' };
  return null;
}

/** Show everything ends when another show takes the air (design section 2): the look goes back
 *  to the step. True when `next` has a show on air that `before` did not name. */
export function showTookAir(before: StepView | null | undefined, next: StepView | null | undefined): boolean {
  if (!before || !next?.on_air || !next.show) return false;
  return next.show.slug !== before.show?.slug;
}

// ---- desk.callers ----

export interface DeskCallers {
  /** `2 waiting · lines open`, `2 waiting · strip 4 taken`, `lines open`. */
  head: string;
  /** At most three rows, the longest wait first. */
  rows: { identity: string; n: number; name: string; topic: string; since_ms: number; dropped: boolean }[];
  /** How many wait beyond the rows drawn. */
  more: number;
  /** The caller the button and `P` take: the one who has waited longest and can go to the desk. */
  next: { identity: string; name: string } | null;
  /** The caller who holds the strip, so the next cannot be taken yet. */
  holds: { name: string; slot: number } | null;
  /** The button's tooltip. */
  title: string;
}

const ROWS = 3;

/** The desk's queue at the Callers step (design sections 1 and 5): who waits, the longest wait
 *  first; the one caller the strip holds; and who Take would bring. A caller can go to the desk
 *  with their consent to record on file and their link up (studio.rs `send`). */
export function deskCallers(v: View | null): DeskCallers {
  const waiting = [...onlyWaiting(v)].sort((a, c) => a.arrived_ms - c.arrived_ms);
  const strip = (v?.sources ?? []).find((s) => s.role === 'caller') ?? null;
  const holds = strip ? { name: short(strip), slot: strip.slot } : null;
  const can = waiting.find((c) => c.dropped_ms === null && c.consent !== null) ?? null;
  const next = can ? { identity: can.identity, name: can.name_on_air || firstName(can.name, can.identity) } : null;
  const count = waiting.length;
  const head = [count ? `${count} waiting` : '', holds ? `strip ${holds.slot} taken` : 'lines open'].filter(Boolean).join(' · ');
  const title = holds
    ? `Strip ${holds.slot} holds one caller. Put ${holds.name} on air or drop the call, then take the next.`
    : 'They join strip 4 off air. Cue to listen, hold Talk to speak to them, then On.';
  return {
    head,
    rows: waiting.slice(0, ROWS).map((c, i) => ({ identity: c.identity, n: i + 1, name: c.name, topic: c.topic || c.line, since_ms: c.arrived_ms, dropped: c.dropped_ms !== null })),
    more: Math.max(0, count - ROWS),
    next,
    holds,
    title,
  };
}

// ---- desk.chat ----

/** `1 report, 2 held messages`: what waits for a moderator, or '' when nothing does (or the
 *  station has not counted). */
export function chatNeeds(chat: { held?: number | null; open_reports?: number | null } | null | undefined): string {
  const reports = chat?.open_reports ?? 0;
  const held = chat?.held ?? 0;
  const parts = [];
  if (reports > 0) parts.push(`${reports} ${reports === 1 ? 'report' : 'reports'}`);
  if (held > 0) parts.push(`${held} held ${held === 1 ? 'message' : 'messages'}`);
  return parts.join(', ');
}

/** Where the moderator goes: at Full, and while everything is shown, the Screener tab is one
 *  press away; below, Show everything brings it. */
export function chatNeedsWhere(look: ShowStep, n: { held?: number | null; open_reports?: number | null } | null | undefined): string {
  const one = (n?.open_reports ?? 0) + (n?.held ?? 0) === 1;
  const verb = one ? 'needs' : 'need';
  return tabShown('screener', look) ? `${verb} a moderator, on the Screener tab.` : `${verb} a moderator: Show everything, then the Screener tab.`;
}

/** The step a station is at before any show has been on air: everything, and nothing to change. */
export const NO_SHOW: StepView = { step: 'full', show: null, on_air: false, next: null, may_change: false };
