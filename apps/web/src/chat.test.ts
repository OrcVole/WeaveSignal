// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { mentions, type Msg, type RoomRules, type Statement } from '../../../packages/ui/chat.ts';
import {
  charCount,
  chatBackoff,
  checkName,
  due,
  hhmm,
  markMentions,
  mentionName,
  newBelow,
  noticeLocation,
  parsePrefs,
  playheadMs,
  positionAfter,
  postingBlocked,
  reviewOpen,
  rulesLine,
  shownMessages,
  socketUrl,
  tidyCode,
  waitingBehindPause,
  type ViewFilter,
} from './lib/chat/model.ts';

function msg(id: number, over: Partial<Msg> = {}): Msg {
  return {
    id,
    kind: 'chat',
    body: 'Morning all',
    author: { id: 'a-elspeth', name: 'Elspeth Gordon', standing: 'member', guest: false },
    reply_to: null,
    server_ms: 1_000_000 + id * 1000,
    playhead_ms: null,
    state: 'visible',
    pinned: false,
    featured: false,
    reactions: [],
    ...over,
  };
}

const rules = (over: Partial<RoomRules> = {}): RoomRules => ({
  slow: false,
  hold_new: false,
  links_trusted: true,
  accounts_only: false,
  min_age_24h: false,
  influx: false,
  comments_only: false,
  guests: false,
  slow_s: 10,
  min_age_minutes: 10,
  max_chars: 500,
  hold_ms: 5000,
  ...over,
});

const view = (over: Partial<ViewFilter> = {}): ViewFilter => ({ sync: false, nowLocalMs: 0, skewMs: 0, delayMs: 15_000, meId: null, pausedIds: null, ...over });

describe('the socket', () => {
  it('lives on the page’s own origin, wss beside https', () => {
    expect(socketUrl({ protocol: 'https:', host: 'radio.example' })).toBe('wss://radio.example/api/v1/chat/socket');
    expect(socketUrl({ protocol: 'http:', host: '127.0.0.1:37401' })).toBe('ws://127.0.0.1:37401/api/v1/chat/socket');
  });
  it('backs off 1, 2, 4, 8 s, then 15 s at most', () => {
    expect([0, 1, 2, 3, 4, 9].map(chatBackoff)).toEqual([1000, 2000, 4000, 8000, 15_000, 15_000]);
  });
  it('remembers the last public offset, and only public events move it', () => {
    const p = { epoch: 'e1', offset: 4 };
    expect(positionAfter(p, { t: 'msg', offset: 5, m: msg(1) })).toEqual({ epoch: 'e1', offset: 5 });
    expect(positionAfter(p, { t: 'hide', offset: 9, ids: [1] })).toEqual({ epoch: 'e1', offset: 9 });
    expect(positionAfter(p, { t: 'count', n: 3 })).toBe(p);
    expect(positionAfter(p, { t: 'msg', offset: 3, m: msg(1) })).toBe(p);
    expect(positionAfter(null, { t: 'msg', offset: 5, m: msg(1) })).toBeNull();
  });
});

describe('what the reader sees', () => {
  const list = [msg(1), msg(2), msg(3, { author: { id: 'a-rory', name: 'Rory Bruce', standing: 'member', guest: false } })];

  it('shows everything in real time by default (ADR-0038)', () => {
    expect(shownMessages(list, view()).map((m) => m.id)).toEqual([1, 2, 3]);
  });
  it('synced, holds a message until the reader hears its moment', () => {
    // Message 2 was written at 1_002_000; with a 15 s delay it is due at 1_017_000 server time.
    const v = view({ sync: true, nowLocalMs: 1_016_500, skewMs: 0 });
    expect(shownMessages(list, v).map((m) => m.id)).toEqual([1]);
    expect(shownMessages(list, { ...v, nowLocalMs: 1_017_000 }).map((m) => m.id)).toEqual([1, 2]);
    // A clock 2 s slow is corrected by the skew.
    expect(shownMessages(list, { ...v, nowLocalMs: 1_015_000, skewMs: 2000 }).map((m) => m.id)).toEqual([1, 2]);
  });
  it('synced, still shows the reader’s own messages at once', () => {
    const v = view({ sync: true, nowLocalMs: 1_000_000, meId: 'a-rory' });
    expect(shownMessages(list, v).map((m) => m.id)).toEqual([3]);
    expect(due(list[2], v)).toBe(true);
  });
  it('paused, adds nothing and counts what waits', () => {
    const v = view({ pausedIds: new Set([1, 2]) });
    expect(shownMessages(list, v).map((m) => m.id)).toEqual([1, 2]);
    expect(waitingBehindPause(list, v)).toBe(1);
    expect(waitingBehindPause(list, view())).toBe(0);
  });
  it('a hidden message leaves a paused log too', () => {
    const v = view({ pausedIds: new Set([1, 2]) });
    expect(shownMessages(list.filter((m) => m.id !== 2), v).map((m) => m.id)).toEqual([1]);
  });
  it('counts new messages below where the reader stopped', () => {
    expect(newBelow(list, 1)).toBe(2);
    expect(newBelow(list, 3)).toBe(0);
    expect(newBelow(list, null)).toBe(0);
    expect(newBelow(list, 99)).toBe(0);
  });
});

describe('the composer', () => {
  it('counts characters as the server does', () => {
    expect(charCount('  hello  ')).toBe(5);
    expect(charCount('📻📻')).toBe(2);
  });
  it('sends a playhead only while the stream plays', () => {
    expect(playheadMs(true, 100_000, 500, 15_000)).toBe(85_500);
    expect(playheadMs(false, 100_000, 500, 15_000)).toBeUndefined();
  });
  it('says the room’s rules in one line, and nothing when none applies', () => {
    expect(rulesLine(rules())).toBe('');
    expect(rulesLine(rules({ slow: true }))).toBe('Slow mode: one message every 10 s');
    expect(rulesLine(rules({ comments_only: true }))).toBe('Comments on the show only, no replies');
    const influx = rulesLine(rules({ influx: true }));
    expect(influx).toContain('Slow mode');
    expect(influx).toContain('Signed-in chatters only');
    expect(influx).toContain('24 hours');
  });
  it('blocks a banned chatter, and a timed-out one until the time-out ends', () => {
    const me = { banned: false, timed_out_until_ms: 5000 };
    expect(postingBlocked(null, 0)).toBeNull();
    expect(postingBlocked({ banned: true, timed_out_until_ms: null }, 0)).toBe('banned');
    expect(postingBlocked(me, 4999)).toBe('timed_out');
    expect(postingBlocked(me, 5000)).toBeNull();
  });
});

describe('names, codes, times and mentions', () => {
  it('checks a name as the server does', () => {
    expect(checkName('  Iona   MacIntosh ')).toEqual({ name: 'Iona MacIntosh' });
    expect(checkName("Niall O'Neill-Anderson")).toEqual({ name: "Niall O'Neill-Anderson" });
    expect(checkName('   ')).toEqual({ problem: 'Choose a name to show beside your messages.' });
    expect(checkName('x'.repeat(41))).toEqual({ problem: 'Keep your name to 40 characters.' });
    expect(checkName('Jessie <b>')).toEqual({ problem: "Use letters, numbers, spaces and _ - . ' only." });
  });
  it('tidies a code to six digits', () => {
    expect(tidyCode('123 456')).toBe('123456');
    expect(tidyCode('12-34-56-7')).toBe('123456');
  });
  it('writes the time as HH:MM', () => {
    expect(hhmm(new Date(2026, 9, 26, 7, 5).getTime())).toBe('07:05');
    expect(hhmm(new Date(2026, 9, 26, 20, 14).getTime())).toBe('20:14');
  });
  it('marks @mentions of the reader, any case, not inside a longer word', () => {
    expect(markMentions('Morning @fiona, and @Fiona_F', 'Fiona')).toEqual([
      { text: 'Morning ', mention: false },
      { text: '@fiona', mention: true },
      { text: ', and @Fiona_F', mention: false },
    ]);
    expect(markMentions('@Lesley Kerr: that was ace', 'Lesley Kerr')).toEqual([
      { text: '@Lesley Kerr', mention: true },
      { text: ': that was ace', mention: false },
    ]);
    expect(markMentions('no one', '')).toEqual([{ text: 'no one', mention: false }]);
  });
  it('treats a mention as plain text in comments-only mode (ADR-0147)', () => {
    expect(mentionName('Fiona', rules())).toBe('Fiona');
    expect(mentionName('Fiona', null)).toBe('Fiona');
    const name = mentionName('Fiona', rules({ comments_only: true }));
    expect(name).toBe('');
    expect(markMentions('Morning @Fiona', name)).toEqual([{ text: 'Morning @Fiona', mention: false }]);
    expect(mentions('Morning @Fiona', name)).toBe(false);
  });
});

describe('the legal toolset, the chatter’s side', () => {
  it('names a message for the notice form of someone without an account', () => {
    const at = new Date(2026, 9, 26, 20, 14).getTime();
    const loc = noticeLocation(msg(42, { server_ms: at, body: 'x'.repeat(100) }));
    expect(loc).toMatch(/^Chat message 42 from Elspeth Gordon at 20:14: "x{80}…"$/);
  });
  it('offers a review until its time ends, and once', () => {
    const s: Statement = { id: 's1', at_ms: 0, text: 'Hidden: abuse.', automated: false, review_by_ms: 1000, review_requested: false };
    expect(reviewOpen(s, 999)).toBe(true);
    expect(reviewOpen(s, 1000)).toBe(false);
    expect(reviewOpen({ ...s, review_requested: true }, 0)).toBe(false);
  });
});

describe('the reader’s settings', () => {
  it('default to real time and mentions only, and survive bad storage', () => {
    expect(parsePrefs(null)).toEqual({ sync: false, announce: 'mentions' });
    expect(parsePrefs('{not json')).toEqual({ sync: false, announce: 'mentions' });
    expect(parsePrefs('{"sync":true,"announce":"nothing"}')).toEqual({ sync: true, announce: 'nothing' });
    expect(parsePrefs('{"sync":"yes","announce":"shout"}')).toEqual({ sync: false, announce: 'mentions' });
  });
});
