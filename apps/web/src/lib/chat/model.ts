// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The listener chat's pure rules (M6), apart from the browser so they can be tested: the
// socket's address and backoff, the position to recover from, which messages a reader sees
// (sync to the stream, pause), the composer's checks and the room-rules line, names, times and
// mentions. The wire types and the shared helpers are packages/ui/chat.ts.

import { dueForViewer, type Msg, type Position, type RoomRules, type ServerFrame, type Statement } from '../../../../../packages/ui/chat.ts';

/** The socket on the page's own origin: ws: beside http:, wss: beside https:. */
export function socketUrl(loc: { protocol: string; host: string }): string {
  return `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/v1/chat/socket`;
}

/** The wait before the n-th reconnect (0-based): 1, 2, 4, 8 s, then 15 s at most. */
export function chatBackoff(attempt: number): number {
  return Math.min(15_000, 1000 * 2 ** Math.max(0, attempt));
}

/** The position after a frame: only the room's public events carry an offset. */
export function positionAfter(pos: Position | null, f: ServerFrame): Position | null {
  if (!pos) return pos;
  switch (f.t) {
    case 'msg':
    case 'hide':
    case 'pin':
    case 'feature':
    case 'react':
      return f.offset > pos.offset ? { epoch: pos.epoch, offset: f.offset } : pos;
    default:
      return pos;
  }
}

export interface ViewFilter {
  /** "Sync chat to my stream" is on. */
  sync: boolean;
  nowLocalMs: number;
  skewMs: number;
  delayMs: number;
  /** The reader's own account: their messages show at once, synced or not. */
  meId: string | null;
  /** While paused, the ids that were on screen when the reader paused. */
  pausedIds: ReadonlySet<number> | null;
}

/** Whether a message is due for this reader (sync on: once they hear its moment). */
export function due(m: Msg, v: Pick<ViewFilter, 'sync' | 'nowLocalMs' | 'skewMs' | 'delayMs' | 'meId'>): boolean {
  if (!v.sync || (v.meId !== null && m.author.id === v.meId)) return true;
  return dueForViewer(m, v.nowLocalMs, v.skewMs, v.delayMs);
}

/** What the log shows: due messages, and while paused only those already on screen. */
export function shownMessages(list: readonly Msg[], v: ViewFilter): Msg[] {
  return list.filter((m) => due(m, v) && (v.pausedIds === null || v.pausedIds.has(m.id)));
}

/** How many due messages wait behind a pause. */
export function waitingBehindPause(list: readonly Msg[], v: ViewFilter): number {
  if (v.pausedIds === null) return 0;
  let n = 0;
  for (const m of list) if (due(m, v) && !v.pausedIds.has(m.id)) n++;
  return n;
}

/** How many messages arrived below the one the reader last saw at the bottom. */
export function newBelow(shown: readonly Msg[], lastSeenId: number | null): number {
  if (lastSeenId === null) return 0;
  const i = shown.findIndex((m) => m.id === lastSeenId);
  return i < 0 ? 0 : shown.length - 1 - i;
}

/** A message's length as the server counts it: characters, outer spaces trimmed. */
export function charCount(body: string): number {
  return [...body.trim()].length;
}

/** The sender's playhead (ADR-0038). This player plays an Icecast stream through an <audio>
 *  element, which carries no EXT-X-PROGRAM-DATE-TIME, so the best estimate is the studio's
 *  clock less the listener delay while the stream plays, and nothing when it does not. HLS's
 *  real playhead arrives with M5. */
export function playheadMs(playing: boolean, nowLocalMs: number, skewMs: number, delayMs: number): number | undefined {
  return playing ? Math.round(nowLocalMs + skewMs - delayMs) : undefined;
}

/** The room's rules that apply now, in one short line; empty when none does. */
export function rulesLine(r: RoomRules): string {
  const parts: string[] = [];
  if (r.slow || r.influx) parts.push(`Slow mode: one message every ${r.slow_s} s`);
  if (r.comments_only) parts.push('Comments on the show only, no replies');
  if (r.accounts_only || r.influx) parts.push('Signed-in chatters only');
  if (r.min_age_24h || r.influx) parts.push('New accounts post after 24 hours');
  if (r.hold_new || r.influx) parts.push('New chatters wait a few seconds for a moderator');
  return parts.join(' · ');
}

/** Why this chatter cannot post now, or null. */
export function postingBlocked(me: { banned: boolean; timed_out_until_ms: number | null } | null, nowServerMs: number): 'banned' | 'timed_out' | null {
  if (!me) return null;
  if (me.banned) return 'banned';
  if (me.timed_out_until_ms !== null && me.timed_out_until_ms > nowServerMs) return 'timed_out';
  return null;
}

/** HH:MM in the reader's own time zone. */
export function hhmm(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** A day and time for notices: "26 Oct, 20:14". */
export function dayTime(ms: number): string {
  const d = new Date(ms);
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  return `${d.getDate()} ${months[d.getMonth()]}, ${hhmm(ms)}`;
}

/** The name check the server makes (crates/chat/src/rules.rs `check_name`), less its word
 *  lists: 1 to 40 characters, letters, numbers, spaces and _ - . ' only. Returns the tidied
 *  name, or the sentence to show. */
export function checkName(raw: string): { name: string } | { problem: string } {
  const name = raw.split(/\s+/u).filter(Boolean).join(' ');
  if (!name) return { problem: 'Choose a name to show beside your messages.' };
  if ([...name].length > 40) return { problem: 'Keep your name to 40 characters.' };
  if (!/^[\p{L}\p{N} _\-.']+$/u.test(name)) return { problem: "Use letters, numbers, spaces and _ - . ' only." };
  return { name };
}

/** An email address worth sending a code to (the server judges the rest). */
export function looksLikeEmail(s: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(s.trim());
}

/** A sign-in code: six digits, spaces allowed while typing. */
export function tidyCode(s: string): string {
  return s.replace(/\D/gu, '').slice(0, 6);
}

/**
 * The name whose @mentions the page marks: the reader's, but none in comments-only mode, where a
 * mention is plain text (ADR-0147): nothing is highlighted or announced.
 */
export function mentionName(name: string, rules: Pick<RoomRules, 'comments_only'> | null): string {
  return rules?.comments_only ? '' : name;
}

/** A body split into plain text and @mentions of `name`, for highlighting. */
export function markMentions(body: string, name: string): { text: string; mention: boolean }[] {
  if (!name) return [{ text: body, mention: false }];
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = new RegExp(`(^|[^\\p{L}\\p{N}_])(@${esc})(?![\\p{L}\\p{N}_])`, 'giu');
  const out: { text: string; mention: boolean }[] = [];
  let last = 0;
  for (const hit of body.matchAll(re)) {
    const start = (hit.index ?? 0) + hit[1].length;
    if (start > last) out.push({ text: body.slice(last, start), mention: false });
    out.push({ text: hit[2], mention: true });
    last = start + hit[2].length;
  }
  if (last < body.length) out.push({ text: body.slice(last), mention: false });
  return out.length ? out : [{ text: body, mention: false }];
}

/** Where a reported message is, for the notice form of someone without an account. */
export function noticeLocation(m: Pick<Msg, 'id' | 'body' | 'server_ms' | 'author'>): string {
  const words = [...m.body];
  const excerpt = words.length > 80 ? `${words.slice(0, 80).join('')}…` : m.body;
  return `Chat message ${m.id} from ${m.author.name} at ${hhmm(m.server_ms)}: "${excerpt}"`;
}

/** Whether a chatter may still ask for a review of a statement. */
export function reviewOpen(s: Statement, nowServerMs: number): boolean {
  return !s.review_requested && nowServerMs < s.review_by_ms;
}

/** A local copy of the chatter's own held message: the server sends held messages to
 *  moderators only, so the sender's page keeps what it sent until it is released or hidden. */
export function heldCopy(id: number, body: string, kind: Msg['kind'], author: Msg['author'], replyTo: Msg['reply_to'], serverMs: number): Msg {
  return {
    id,
    kind,
    body,
    author,
    reply_to: replyTo,
    server_ms: serverMs,
    playhead_ms: null,
    state: 'held',
    pinned: false,
    featured: false,
    reactions: [],
  };
}

/** Announcement levels for screen readers (WCAG 2.2.2; 0006 section 7). */
export type Announce = 'mentions' | 'everything' | 'nothing';

/** The reader's own settings, kept on this device. */
export interface Prefs {
  sync: boolean;
  announce: Announce;
}

export const DEFAULT_PREFS: Prefs = { sync: false, announce: 'mentions' };

export function parsePrefs(raw: string | null): Prefs {
  if (!raw) return { ...DEFAULT_PREFS };
  try {
    const p = JSON.parse(raw) as Partial<Prefs>;
    return {
      sync: p.sync === true,
      announce: p.announce === 'everything' || p.announce === 'nothing' ? p.announce : 'mentions',
    };
  } catch {
    return { ...DEFAULT_PREFS };
  }
}
