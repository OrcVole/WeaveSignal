// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the monitor link draws from a view (ADR-0280; hosts-in-their-own-studios.md sections
// 4.5, 5 and 8). Pure, so the rules are tested.
//
// The station owns the sentences: `says`, `then.words`, each `why_not`, `end_show_says` and a
// refusal are printed as they come. What is made here is what a sentence cannot carry: the
// lamp, the counts and clock times (drawn from `at_ms` and the station's clock), and the facts
// set out in rows. The lamp is picked from `state` alone, and a state this page does not know
// gives no lamp at all: it must never be able to show ON AIR by mistake.

import { CHIP, type Chip } from '../../../../../packages/ui/states.ts';
import type { Insert, MonitorView, Refusal, RundownRow, Then } from './api.ts';
import { day, dayKey, hm, hms } from './clock.ts';

/** The lamp: the vocabulary's own chip, at the size of the desk's master lamp. */
export interface Lamp {
  /** `onair`, `open`, `standby`, `lost`, `test` or `notconn`: the card's and the lamp's class. */
  kind: 'onair' | 'open' | 'standby' | 'lost' | 'test' | 'notconn';
  chip: Chip;
}

/** TEST borrows STANDBY's lamp, dashed (section 5): only here and in Settings. */
const TEST: Chip = { cls: 'hollow hollow-standby dashed', glyph: 'g-standby', glyphCls: '', word: 'Test' };

const LAMPS: Record<string, Lamp> = {
  on_air: { kind: 'onair', chip: CHIP.onAir },
  open: { kind: 'open', chip: CHIP.open },
  standby: { kind: 'standby', chip: CHIP.standby },
  link_lost: { kind: 'lost', chip: CHIP.linkLost },
  test: { kind: 'test', chip: TEST },
  not_connected: { kind: 'notconn', chip: CHIP.notConnected },
};

/** The lamp for a state the station sent; null for one this page does not know. */
export function lampOf(state: string): Lamp | null {
  return Object.hasOwn(LAMPS, state) ? LAMPS[state] : null;
}

/** A sentence's opening, to set in bold, and what follows it: `On air.` and the rest. */
export function lead(says: string): { lead: string; rest: string } {
  const m = /^(.+?[.!?])\s+(.+)$/s.exec(says.trim());
  return m ? { lead: m[1], rest: m[2] } : { lead: says.trim(), rest: '' };
}

/** What happens next, unless the lamp's own sentence already says it (a place let go). */
export function thenWords(view: Pick<MonitorView, 'says' | 'then'>): string {
  const w = view.then?.words.trim() ?? '';
  return w && !view.says.includes(w) ? w : '';
}

/** A count: `0:42`, `1:02:07`; null from a day away, where a date says more. */
export function count(ms: number): string | null {
  const s = Math.max(0, Math.ceil(ms / 1000));
  if (s >= 86_400) return null;
  const sec = String(s % 60).padStart(2, '0');
  const min = Math.floor(s / 60) % 60;
  const h = Math.floor(s / 3600);
  return h ? `${h}:${String(min).padStart(2, '0')}:${sec}` : `${min}:${sec}`;
}

/** How long something has lasted: `4:51`, `2:04:51`. */
export function lasted(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const sec = String(s % 60).padStart(2, '0');
  const min = Math.floor(s / 60) % 60;
  const h = Math.floor(s / 3600);
  return h ? `${h}:${String(min).padStart(2, '0')}:${sec}` : `${min}:${sec}`;
}

/** The count beside the lamp: what it counts to, the figure, and one line under it. */
export interface Counting {
  label: string;
  /** The figure, or null when the instant has passed or is a day away. */
  figure: string | null;
  small: string;
  /** The instant counted to, to key the bar by. */
  at_ms: number;
  kind: string;
}

/** The count for what happens next; null when the sentence counts to nothing. */
export function counting(view: Pick<MonitorView, 'then' | 'post' | 'state'>, now: number, zone: string): Counting | null {
  const t: Then | null = view.then;
  if (!t || t.at_ms === null) return null;
  const at = t.at_ms;
  const left = at - now;
  const figure = left > 0 ? count(left) : null;
  const base = { at_ms: at, kind: t.kind, figure };
  switch (t.kind) {
    case 'at_post':
      return left > 0 ? { ...base, label: `On air at ${hms(at, zone)}`, small: 'by the schedule, to the second' } : null;
    case 'late_count':
      return left > 0 ? { ...base, label: 'On air in', small: "the station's count for a late start" } : null;
    case 'grace':
      if (left <= 0) return null;
      return view.post && !view.post.mine
        ? { ...base, label: `${view.post.show_name} takes the air`, small: `by the schedule at ${hms(at, zone)}` }
        : { ...base, label: `On air at ${hms(at, zone)}`, small: 'by the schedule, unless a press comes first' };
    case 'waits':
      return left > 0 ? { ...base, label: `Your show starts at ${hms(at, zone)}`, small: '' } : null;
    case 'link_lost':
      return left > 0 ? { ...base, label: `Your place is kept until ${hms(at, zone)}`, small: 'after that you come back standing by, not on air' } : null;
    case 'slot': {
      if (left <= 0) return { ...base, label: `Your show started at ${hm(at, zone)}`, small: '' };
      const today = dayKey(at, zone) === dayKey(now, zone);
      const when = today ? `today at ${hm(at, zone)}` : `${day(at, zone)} at ${hm(at, zone)}`;
      return figure ? { ...base, label: 'Your show starts in', small: when } : { ...base, label: 'Your show starts', small: when };
    }
    default:
      return null;
  }
}

/** A count turns amber in its last 30 s and red in its last 10 s (the figure shown, so `0:30` is the first amber). */
export const AMBER_S = 30;
export const RED_S = 10;

/** How pressing a count is: none until its last 30 s. The word goes beside the colour, so the
 *  colour is never the only sign. */
export interface Hurry {
  level: 'amber' | 'red';
  word: string;
}

/** The count's last seconds; null while it has more, or no figure to show. */
export function hurry(c: Pick<Counting, 'figure' | 'at_ms'>, now: number): Hurry | null {
  if (!c.figure) return null;
  const s = Math.ceil((c.at_ms - now) / 1000);
  if (s > AMBER_S || s <= 0) return null;
  return s <= RED_S ? { level: 'red', word: 'Last 10 seconds' } : { level: 'amber', word: 'Last 30 seconds' };
}

/** How far a count's bar has run, 0 to 100. The run-up to the post is the last five minutes;
 *  any other count runs from when this page first saw it (`since`, the station's time). */
export function barPct(c: Pick<Counting, 'kind' | 'at_ms'>, now: number, since: number): number {
  const left = c.at_ms - now;
  if (left <= 0) return 100;
  const span = c.kind === 'at_post' || c.kind === 'waits' || c.kind === 'slot' ? 5 * 60_000 : c.at_ms - since;
  if (span <= 0) return 0;
  return Math.round(Math.min(100, Math.max(0, (1 - left / span) * 100)));
}

/** `−15.2`: one decimal, with a true minus. */
export function signed(n: number): string {
  const s = Math.abs(n).toFixed(1);
  return n < -0.05 ? `−${s}` : s;
}

/** The station's leveller reaches 12 dB either way (apps/studio/src/lib/hosts.ts). */
const LEVELLER_REACH_DB = 12;

/** `SRT`, `Icecast`: the two ways in, as plain words. */
export function wayWord(way: string | null): string {
  return way === 'srt' ? 'SRT' : way === 'icecast' ? 'Icecast' : (way ?? '');
}

/** *The station hears you*: rows of facts, the three checks in a test, and what is wrong. */
export interface SourceWords {
  /** Whether anything arrives from the host's software now. */
  connected: boolean;
  rows: [string, string][];
  /** In a test: each check, `ok` (a tick), `wait` (not yet) or `read` (a reading, no verdict). */
  checks: { mark: 'ok' | 'wait' | 'read'; title: string; detail: string }[];
  /** A sentence that needs the host to act: a refusal, a lost link, a level out of reach. */
  problem: string | null;
  /** The level's place on the bar, 0 to 100; null when there is none to draw. */
  meter: number | null;
  /** The line about Icecast's delay, when that is the way in. */
  icecast: boolean;
}

export function sourceWords(view: MonitorView, now: number, zone: string): SourceWords {
  const connected = view.way !== null && view.state !== 'link_lost' && view.state !== 'not_connected';
  const wayLine = [wayWord(view.way), view.format ?? ''].filter(Boolean).join(' · ');
  const out: SourceWords = { connected, rows: [], checks: [], problem: null, meter: null, icecast: connected && view.way === 'icecast' };
  const refusal = view.last_refusal;
  // A refusal still matters when nothing is connected, or when it came after this connection began.
  const since = view.steady_ms !== null ? now - view.steady_ms : null;
  const refused = refusal && (!connected || (since !== null && refusal.at_ms > since)) ? refusalWords(refusal, zone) : null;

  if (view.state === 'link_lost') {
    out.problem = 'Nothing is arriving from your software. This page is still connected, so the fault is between your software and the station.';
    if (wayLine) out.rows.push(['Way in', wayLine]);
    return out;
  }
  if (!connected) {
    out.problem = refused;
    if (!refused) out.rows.push(['Your software', 'Nothing is arriving from it.']);
    return out;
  }
  if (view.state === 'test') {
    const steady = view.steady_ms ?? 0;
    out.checks = [
      { mark: 'ok', title: 'Connected', detail: wayLine },
      view.level === null
        ? { mark: 'wait', title: 'Level', detail: 'No sound yet.' }
        : { mark: 'read', title: 'Level', detail: `${signed(view.level)} dBFS as the station reads it. It is matched to the station once you are standing by.` },
      steady >= 30_000
        ? { mark: 'ok', title: 'Steady for 30 s', detail: `${lasted(steady)} so far` }
        : { mark: 'wait', title: 'Steady for 30 s', detail: `${Math.floor(steady / 1000)} s so far` },
    ];
    out.meter = view.level === null ? null : meterPct(view.level);
    out.problem = refused;
    return out;
  }
  if (wayLine) out.rows.push(['Way in', wayLine]);
  if (view.level !== null && view.leveller_db !== null) {
    const lev = view.leveller_db;
    const arrived = `${signed(view.level - lev)} LUFS`;
    const does = Math.abs(lev) < 0.05 ? 'left as it is' : `turned ${lev < 0 ? 'down' : 'up'} ${Math.abs(lev).toFixed(1)} dB to match the station`;
    out.rows.push(['Level', `${arrived} · ${does}`]);
    if (Math.abs(lev) >= LEVELLER_REACH_DB - 0.05) {
      out.problem =
        lev > 0
          ? `Too quiet for the station to make up: it has turned you up ${LEVELLER_REACH_DB} dB, as far as it goes. Raise your software's output.`
          : `Too loud for the station to bring down: it has turned you down ${LEVELLER_REACH_DB} dB, as far as it goes. Lower your software's output.`;
    }
  } else if (view.level !== null) {
    out.rows.push(['Level', `${signed(view.level)} LUFS`]);
  }
  if (view.steady_ms !== null) out.rows.push(['Steady for', lasted(view.steady_ms)]);
  if (view.title) out.rows.push(['Playing', view.title]);
  out.meter = view.level === null ? null : meterPct(view.level);
  out.problem ??= refused;
  return out;
}

/** A level's place on a −60 to 0 bar. */
function meterPct(level: number): number {
  return Math.round(Math.min(100, Math.max(0, ((level + 60) / 60) * 100)));
}

/** *The station refused your software at 13:41: the key is wrong.* */
export function refusalWords(r: Refusal, zone: string): string {
  const why = r.words.trim();
  const at = `The station refused your software at ${hm(r.at_ms, zone)}`;
  return why ? `${at}: ${why.charAt(0).toLowerCase()}${why.slice(1)}` : `${at}.`;
}

/** One row of *Before you, you, after you*. */
export interface NextRow {
  where: string;
  title: string;
  detail: string;
  chip: Chip | null;
}

const ROW_CHIP: Record<string, Chip> = {
  on_air: CHIP.onAir,
  open: CHIP.open,
  standby: CHIP.standby,
  standing_by: CHIP.standby,
  link_lost: CHIP.linkLost,
  test: TEST,
  not_connected: CHIP.notConnected,
};

const AFTER_WORDS: Record<string, string> = {
  on_air: 'on air',
  open: 'open',
  standing_by: 'standing by',
  connecting: 'connecting',
  not_connected: 'not connected yet',
  link_lost: 'link lost',
  test: 'testing',
  in_room: "in the station's room",
};

function span(r: RundownRow, zone: string): string {
  return `${hm(r.start_ms, zone)} to ${hm(r.end_ms, zone)}`;
}

/** Before you, you, after you (`monitor.next`). `known` is false while the page has lost the
 *  station: then no row carries a chip, because none can be vouched for. */
export function nextRows(view: MonitorView, now: number, zone: string, known = true): NextRow[] {
  const { before, you, after } = view.rundown;
  if (!you) return [];
  const rows: NextRow[] = [];
  const live = (s: string) => s === 'on_air' || s === 'open';
  if (before) {
    const over = live(before.state) && now >= before.end_ms;
    const ended = !live(before.state) && now >= before.end_ms;
    const where = before.state === 'in_room' ? "in the station's room" : 'own studio';
    rows.push({
      where: 'Before you',
      title: before.show_name,
      detail: ended ? `ended ${hm(before.end_ms, zone)}` : [before.host, over ? 'running over' : where].filter(Boolean).join(' · '),
      chip: known && live(before.state) ? ROW_CHIP[before.state] : null,
    });
  }
  // A show on another day (a test the evening before) says which.
  const mine = [dayKey(you.start_ms, zone) === dayKey(now, zone) ? span(you, zone) : `${day(you.start_ms, zone)}, ${span(you, zone)}`];
  if (live(you.state) && now >= you.end_ms) mine.push('running over');
  if (you.state === 'link_lost') mine.push('your place is kept');
  rows.push({ where: 'You', title: you.show_name, detail: mine.join(' · '), chip: known ? (ROW_CHIP[you.state] ?? null) : null });
  if (after) {
    const chip = known && (after.state === 'standing_by' || live(after.state)) ? ROW_CHIP[after.state] : null;
    // The chip says the state where there is one; otherwise the words do.
    const words = known && !chip ? (AFTER_WORDS[after.state] ?? '') : '';
    rows.push({ where: 'After you', title: after.show_name, detail: [after.host, hm(after.start_ms, zone), words].filter(Boolean).join(' · '), chip });
  } else {
    rows.push({ where: 'After you', title: 'Nobody', detail: 'Nothing is scheduled after you.', chip: null });
  }
  return rows;
}

/** The station's next insert (`monitor.insert`): its title and one line. Whether it plays over
 *  the host or instead of them is not in what the station sends yet, so the line does not say. */
export function insertWords(insert: Insert, view: MonitorView, now: number, zone: string): { title: string; detail: string } {
  const title = `${insert.label} at ${hms(insert.at_ms, zone)}`;
  const mineEnds = view.rundown.you?.end_ms ?? null;
  if (mineEnds !== null && insert.at_ms >= mineEnds) return { title, detail: 'After your time. Nothing for you to do.' };
  const left = insert.at_ms - now;
  const c = left > 0 ? count(left) : null;
  const lasts = `It plays for ${lasted(insert.duration_ms)}.`;
  return { title, detail: c ? `In ${c}. ${lasts}` : lasts };
}

/** What the controls say when there is nothing to press. */
export function nothingToPress(view: MonitorView, now: number, zone: string): string {
  switch (view.state) {
    case 'standby': {
      // At the post it is to the second; in a grace a press may come before the schedule does.
      const t = view.then;
      const due = t && t.at_ms !== null && t.at_ms > now ? t.kind : '';
      const at = due === 'at_post' ? ` You go on air at ${hms(t!.at_ms!, zone)}.` : due === 'grace' ? ` You go on air by ${hms(t!.at_ms!, zone)}.` : '';
      return `Nothing to press.${at} Start when the lamp turns red.`;
    }
    case 'test':
      return "Nothing here can put you on air. On the day you go on air at your show's time.";
    case 'link_lost':
      return 'Nothing to press. Reconnect your software.';
    case 'not_connected':
      return 'Nothing to press until your software reaches the station.';
    default:
      return 'Nothing to press just now.';
  }
}

/** After a dump: *Listeners lost 8 s. Carry on, and do not repeat the words.* */
export function dumpedWords(lostMs: number | null): string {
  const lost = lostMs === null ? '' : `Listeners lost ${Math.round(lostMs / 1000)} s. `;
  return `${lost}Carry on, and do not repeat the words.`;
}

/** The page's title: *Steeped · Finlay Grant*. */
export function titleOf(view: Pick<MonitorView, 'show_name' | 'label'>): string {
  return [view.show_name, view.label].filter(Boolean).join(' · ');
}

/** "Heather" from "Heather Douglas". */
export function first(name: string): string {
  return name.trim().split(/\s+/)[0] || name;
}

/** What the Listen card says while the host stands by and hears the listeners' stream (the
 *  desk's mix is silent then): how far behind it is, and what takes over on air. Before Start
 *  listening is pressed nothing plays yet, so the card says what the press will bring. */
export function standbyStreamWords(behindMs: number | null, playing = true): string {
  const behind = behindMs && behindMs > 0 ? `about ${Math.round(behindMs / 1000)} s behind` : 'a little behind';
  return `${playing ? 'You are hearing' : 'You will hear'} what listeners hear, ${behind}, while you stand by. When you are on air: everyone but you · cue · talkback, live.`;
}

/** What the Listen card says while the host is OPEN (live to the room, automation or the
 *  fallback holds the air) and hears the station's own output: the room's voices are not in
 *  it, so the card says so plainly. Before Start listening is pressed it says what the press
 *  will bring. */
export function openStreamWords(behindMs: number | null, playing = true): string {
  const behind = behindMs && behindMs > 0 ? `about ${Math.round(behindMs / 1000)} s behind` : 'a little behind';
  return `${playing ? 'You are hearing' : 'You will hear'} the station's output, ${behind}. The room's voices are not in it. Cue and talkback still come through, live.`;
}

/** Why Start listening is off, or null when it may be pressed. Nothing can play while the page
 *  has lost the station, while the station has lost the host's stream, or before the host is
 *  at the desk (a test, or not connected): the desk makes a monitor mix only for a source it
 *  holds. A host standing by, on air or open may press it even before the mix exists: the
 *  press is what asks for it. */
export function listenOff(view: Pick<MonitorView, 'state'>, known: boolean): string | null {
  if (!known) return 'This page has lost the station. Start listening comes back when it reaches the station again.';
  if (view.state === 'link_lost') return 'Nothing plays here while the station has lost your stream.';
  if (view.state === 'standby' || view.state === 'on_air' || view.state === 'open') return null;
  return 'Nothing plays here until you are standing by. Then: everyone but you · cue · talkback, live.';
}
