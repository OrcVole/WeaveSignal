// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The screener's chat, as pure functions (M6): the moderation snapshot kept up to date from the
// socket's frames, "Needs you" in its order, the room with the lines hidden while the screener
// watched, the rate, and the statement of reasons the chatter receives. No DOM and no runes, so
// it is tested on its own (src/chat.test.ts).

import { applyEvent, type ChatInfo, type Held, type Item, type ModSnapshot, type Msg, type Reason, type ServerFrame } from '../../../../packages/ui/chat.ts';
import type { SorReason, SorThen } from '../../../../packages/ui/keymap.ts';

// ---- the statement dialog's reasons ----

export interface SorReasonRow {
  reason: SorReason;
  key: string;
  /** The screener's words on the radio (the design's). */
  label: string;
  /** The chatter's words in the statement, when the service gives none. */
  words: string;
}

export const SOR_REASONS: readonly SorReasonRow[] = [
  { reason: 'abuse', key: 'A', label: 'Abuse or harassment', words: 'abuse or harassment' },
  { reason: 'spam', key: 'S', label: 'Spam or scam', words: 'spam or a scam' },
  { reason: 'off_topic', key: 'T', label: 'Off topic, repeated', words: 'off topic, repeated' },
  { reason: 'illegal', key: 'I', label: 'Illegal content: notify police', words: 'illegal content' },
  { reason: 'intimate_image', key: 'M', label: 'Intimate image: 48 h path', words: 'an intimate image shared without consent' },
  { reason: 'other', key: 'E', label: 'Something else', words: 'another breach of our terms' },
];

export const SOR_THENS: readonly { then: SorThen; key: string; label: string }[] = [
  { then: 'none', key: '1', label: 'Hide only' },
  { then: 'timeout', key: '2', label: 'Hide and time out' },
  { then: 'ban', key: '3', label: 'Hide and ban' },
];

/** A report's reason as the dialog's: the two reasons only chatters and notices give are
 *  illegal content (the police path). */
export function sorReasonOf(r: Reason): SorReason {
  return r === 'threat_to_life' || r === 'child_safety' ? 'illegal' : r;
}

/** A reason in words, from the service's own list when it gave one. */
export function reasonWords(r: Reason, info: Pick<ChatInfo, 'reasons'> | null): string {
  const given = info?.reasons.find((x) => x.reason === r)?.words;
  if (given) return given.toLowerCase();
  const row = SOR_REASONS.find((x) => x.reason === r);
  if (row) return row.words;
  return r === 'threat_to_life' ? 'a threat to life' : 'child safety';
}

/** A reason as the screener reads it on an item: the dialog's label, or the service's words. */
export function reasonLabel(r: Reason, info: Pick<ChatInfo, 'reasons'> | null): string {
  const row = SOR_REASONS.find((x) => x.reason === r);
  if (row) return row.label.replace(/:.*$/, '').toLowerCase();
  return reasonWords(r, info);
}

/** How long a review may be asked for (the service's REVIEW_DAYS). */
export const REVIEW_DAYS = 14;
/** The time-out the screener's `O` and the dialog's `2` give. */
export const TIMEOUT_MIN = 10;

export interface PreviewFacts {
  station: string;
  show: string;
  /** The message's time, already as the chatter's clock prints it (`20:13`); null for many. */
  at: string | null;
  reason: Reason;
  then: SorThen;
  automated: boolean;
  info: Pick<ChatInfo, 'reasons' | 'retention'> | null;
}

/** The statement the chatter receives, as the dialog previews it (ADR-0032: terse). The
 *  service writes the real one from the same facts. */
export function statementPreview(f: PreviewFacts): string {
  const where = f.show ? ` in ${f.show}` : '';
  const which = f.at ? `your message of ${f.at}${where}` : `your message${where}`;
  const out = [`${f.station || 'The station'} hid ${which} because it broke our terms (${reasonWords(f.reason, f.info)}).`];
  out.push(f.automated ? 'An automatic filter flagged it and a moderator decided.' : 'A moderator decided.');
  if (f.then === 'timeout') out.push(`You cannot post for ${TIMEOUT_MIN} minutes.`);
  if (f.then === 'ban') out.push('You can no longer post here.');
  out.push(`You can ask for a review within ${REVIEW_DAYS} days.`);
  const keep = f.info?.retention.moderation.trim();
  if (keep) out.push(keep);
  return out.join(' ');
}

// ---- the moderation snapshot ----

export const EMPTY_MOD: ModSnapshot = { items: [], held: [], studio: [] };

/** Apply a moderators-only frame to the snapshot. Returns the same object when nothing changed. */
export function applyMod(s: ModSnapshot, f: ServerFrame): ModSnapshot {
  switch (f.t) {
    case 'mod_held':
      return { ...s, held: [...s.held.filter((h) => h.m.id !== f.h.m.id), f.h] };
    case 'mod_unheld': {
      const gone = new Set(f.ids);
      return { ...s, held: s.held.filter((h) => !gone.has(h.m.id)) };
    }
    case 'mod_item':
      if (f.item.status !== 'open') return { ...s, items: s.items.filter((i) => i.id !== f.item.id) };
      return s.items.some((i) => i.id === f.item.id)
        ? { ...s, items: s.items.map((i) => (i.id === f.item.id ? f.item : i)) }
        : { ...s, items: [...s.items, f.item] };
    case 'mod_item_done':
      return { ...s, items: s.items.filter((i) => i.id !== f.id) };
    case 'mod_studio':
      return s.studio.some((x) => x.m.id === f.s.m.id)
        ? { ...s, studio: s.studio.map((x) => (x.m.id === f.s.m.id ? f.s : x)) }
        : { ...s, studio: [...s.studio, f.s] };
    default:
      return s;
  }
}

// ---- needs you ----

export type Entry =
  | { key: string; kind: 'report' | 'notice' | 'review'; at: number; due: number | null; item: Item }
  | { key: string; kind: 'held'; at: number; due: null; held: Held };

/** Everything that needs a person, in one list: anything on a fixed clock (an intimate image's
 *  48 hours) first, soonest due first; then the rest oldest first. */
export function needsList(s: ModSnapshot): Entry[] {
  const items: Entry[] = s.items
    .filter((i) => i.status === 'open')
    .map((item) => ({ key: `item:${item.id}`, kind: item.kind, at: item.created_ms, due: item.due_ms, item }));
  const held: Entry[] = s.held.map((h) => ({ key: `held:${h.m.id}`, kind: 'held' as const, at: h.m.server_ms, due: null, held: h }));
  return [...items, ...held].sort((a, b) => {
    if (a.due !== null || b.due !== null) {
      if (a.due === null) return 1;
      if (b.due === null) return -1;
      if (a.due !== b.due) return a.due - b.due;
    }
    return a.at - b.at;
  });
}

/** A held message's countdown: seconds left (never below 0) and the ring's share still to run. */
export function holdLeft(h: Held, now: number): { s: number; share: number } | null {
  if (h.release_at_ms === null) return null;
  const total = Math.max(1, h.release_at_ms - h.m.server_ms);
  const left = Math.max(0, h.release_at_ms - now);
  return { s: Math.ceil(left / 1000), share: Math.min(1, left / total) };
}

// ---- the room ----

export interface HiddenMark {
  at_ms: number;
  by: string | null;
  why: string | null;
}

export interface Room {
  /** Newest last, hidden lines kept in place. */
  lines: Msg[];
  hidden: Record<number, HiddenMark>;
  /** When this window saw a message featured. */
  featuredAt: Record<number, number>;
  pinned: Msg | null;
}

export const EMPTY_ROOM: Room = { lines: [], hidden: {}, featuredAt: {}, pinned: null };

/** How long after a `hide` its `mod_done` is taken to be about it. */
export const HIDE_MATCH_MS = 3000;

/** Apply a public frame to the room. Unlike the player page, a hidden line stays, marked, so the
 *  screener sees what went; `mine` names the hides this window sent, by message id. */
export function applyRoom(r: Room, f: ServerFrame, now: number, mine: Record<number, string> = {}, cap = 500): Room {
  switch (f.t) {
    case 'msg':
    case 'react':
      return { ...r, lines: applyEvent(r.lines, f, cap) };
    case 'feature':
      return { ...r, lines: applyEvent(r.lines, f, cap), featuredAt: { ...r.featuredAt, [f.m.id]: now } };
    case 'pin':
      return { ...r, lines: applyEvent(r.lines, f, cap), pinned: f.m };
    case 'hide': {
      const hidden = { ...r.hidden };
      for (const id of f.ids) {
        if (!r.lines.some((m) => m.id === id) || hidden[id]) continue;
        hidden[id] = mine[id] ? { at_ms: now, by: 'you', why: mine[id] } : { at_ms: now, by: null, why: null };
      }
      const pinned = r.pinned && f.ids.includes(r.pinned.id) ? null : r.pinned;
      return { ...r, hidden, pinned };
    }
    case 'mod_done': {
      // Who hid the lines that went just before: the frame names the moderator and the act.
      let hidden: Record<number, HiddenMark> | null = null;
      for (const [id, h] of Object.entries(r.hidden)) {
        if (h.by === null && now - h.at_ms <= HIDE_MATCH_MS) {
          hidden ??= { ...r.hidden };
          hidden[Number(id)] = { ...h, by: f.by, why: f.what };
        }
      }
      return hidden ? { ...r, hidden } : r;
    }
    default:
      return r;
  }
}

/** The room from a fresh `welcome` that could not recover: its history, nothing marked. */
export function roomFrom(history: Msg[], pinned: Msg | null): Room {
  return { lines: history, hidden: {}, featuredAt: {}, pinned };
}

/** The room's pace: "7 a second", "12 a minute", from the arrival times of the last minute. */
export function rateWords(arrivals: number[], now: number): string {
  const tenS = arrivals.filter((t) => now - t <= 10_000).length;
  if (tenS >= 10) return `${Math.round(tenS / 10)} a second`;
  const minute = arrivals.filter((t) => now - t <= 60_000).length;
  return minute ? `${minute} a minute` : 'quiet';
}

/** An account's age as the service words it: "40 s", "2 min", "3 h", "3 days". */
export function ageWords(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h`;
  const d = Math.floor(s / 86_400);
  return d === 1 ? '1 day' : `${d} days`;
}

/** Move a selection by `delta` in a list of keys, clamped; from nothing, the first. */
export function step(keys: readonly string[], at: string | null, delta: number): string | null {
  if (!keys.length) return null;
  const i = at === null ? -1 : keys.indexOf(at);
  if (i < 0) return keys[0];
  return keys[Math.max(0, Math.min(keys.length - 1, i + delta))];
}
