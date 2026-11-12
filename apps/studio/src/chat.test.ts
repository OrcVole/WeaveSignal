// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The screener's chat (M6) without a page: Needs you in its order, the snapshot kept from the
// moderators' frames, the room keeping what was hidden, and the statement preview. Sample people
// come from the Scottish names list (ADR-0071).
import { describe, expect, it } from 'vitest';
import type { Held, Item, Msg, StudioMsg } from '../../../packages/ui/chat.ts';
import {
  EMPTY_MOD,
  EMPTY_ROOM,
  HIDE_MATCH_MS,
  ageWords,
  applyMod,
  applyRoom,
  holdLeft,
  needsList,
  rateWords,
  sorReasonOf,
  statementPreview,
  step,
  type Room,
} from './lib/chatModel.ts';

const msg = (id: number, name: string, body: string, at = 1_000 * id, over: Partial<Msg> = {}): Msg => ({
  id,
  kind: 'chat',
  body,
  author: { id: `a-${name}`, name, standing: 'member', guest: false },
  reply_to: null,
  server_ms: at,
  playhead_ms: null,
  state: 'visible',
  pinned: false,
  featured: false,
  reactions: [],
  ...over,
});

const item = (id: string, over: Partial<Item> = {}): Item => ({
  id,
  kind: 'report',
  reason: 'abuse',
  count: 1,
  message: msg(9, 'Tavish', 'aye right'),
  account: { id: 'a-Tavish', name: 'Tavish', since_ms: 0, standing: 'member', guest: false, restricted: null },
  detail: '',
  location: '',
  contact: null,
  created_ms: 5_000,
  due_ms: null,
  automated: null,
  status: 'open',
  ...over,
});

const held = (id: number, at: number, release: number | null): Held => ({
  m: msg(id, 'Jessie Brown', 'hiya, is this the show about the buses?', at),
  release_at_ms: release,
  why: 'Held: new account, 2 min old.',
  flagged: null,
  account_since_ms: at - 120_000,
});

describe('Needs you', () => {
  it('puts an intimate image first, then everything oldest first', () => {
    const snap = {
      items: [item('r-late', { created_ms: 9_000 }), item('n-1', { kind: 'notice', created_ms: 2_000 }), item('ii', { created_ms: 8_000, due_ms: 8_000 + 48 * 3_600_000 })],
      held: [held(3, 4_000, 9_000)],
      studio: [],
    };
    expect(needsList(snap).map((e) => e.key)).toEqual(['item:ii', 'item:n-1', 'held:3', 'item:r-late']);
  });

  it('keeps the snapshot from the moderators’ frames', () => {
    let s = applyMod(EMPTY_MOD, { t: 'mod_held', h: held(3, 4_000, 9_000) });
    s = applyMod(s, { t: 'mod_held', h: held(4, 4_500, 9_500) });
    s = applyMod(s, { t: 'mod_unheld', ids: [3] });
    expect(s.held.map((h) => h.m.id)).toEqual([4]);
    s = applyMod(s, { t: 'mod_item', item: item('r1', { count: 1 }) });
    s = applyMod(s, { t: 'mod_item', item: item('r1', { count: 3 }) });
    expect(s.items).toHaveLength(1);
    expect(s.items[0].count).toBe(3);
    s = applyMod(s, { t: 'mod_item_done', id: 'r1', status: 'dismissed', by: 'Elspeth Gordon' });
    expect(s.items).toEqual([]);
    // An item that arrives closed leaves the list.
    s = applyMod(s, { t: 'mod_item', item: item('r2') });
    s = applyMod(s, { t: 'mod_item', item: item('r2', { status: 'actioned' }) });
    expect(s.items).toEqual([]);
    const pm: StudioMsg = { m: msg(20, 'Iona MacIntosh', 'Is tonight recorded?'), account_since_ms: 0, passed_ms: null, reply: null, invited_ms: null };
    s = applyMod(s, { t: 'mod_studio', s: pm });
    s = applyMod(s, { t: 'mod_studio', s: { ...pm, passed_ms: 30_000 } });
    expect(s.studio).toHaveLength(1);
    expect(s.studio[0].passed_ms).toBe(30_000);
  });

  it('counts a hold down on its ring, and a word-filter hold has no countdown', () => {
    expect(holdLeft(held(3, 10_000, 15_000), 11_000)).toEqual({ s: 4, share: 0.8 });
    expect(holdLeft(held(3, 10_000, 15_000), 16_000)).toEqual({ s: 0, share: 0 });
    expect(holdLeft(held(3, 10_000, null), 11_000)).toBeNull();
  });

  it('moves a selection without leaving the list', () => {
    expect(step(['a', 'b', 'c'], null, 1)).toBe('a');
    expect(step(['a', 'b', 'c'], 'b', 1)).toBe('c');
    expect(step(['a', 'b', 'c'], 'c', 1)).toBe('c');
    expect(step(['a', 'b', 'c'], 'a', -1)).toBe('a');
    expect(step([], null, 1)).toBeNull();
  });
});

describe('the room', () => {
  const two = { ...EMPTY_ROOM, lines: [msg(1, 'Niall Anderson', 'two an hour till three'), msg(2, 'Alastair Buchanan', 'spam link')] };

  it('keeps a hidden line in place, marked by whoever hid it', () => {
    let r = applyRoom(two, { t: 'hide', offset: 7, ids: [2] }, 50_000);
    expect(r.lines.map((m) => m.id)).toEqual([1, 2]);
    expect(r.hidden[2]).toEqual({ at_ms: 50_000, by: null, why: null });
    r = applyRoom(r, { t: 'mod_done', by: 'Elspeth Gordon', what: 'hid a message: spam', at_ms: 50_100 }, 50_100);
    expect(r.hidden[2]).toMatchObject({ by: 'Elspeth Gordon', why: 'hid a message: spam' });
    // A later act is not taken to be about it.
    const late = applyRoom(applyRoom(two, { t: 'hide', offset: 7, ids: [1] }, 0), { t: 'mod_done', by: 'Rory Bruce', what: 'pinned', at_ms: 0 }, HIDE_MATCH_MS + 1);
    expect(late.hidden[1].by).toBeNull();
  });

  it('says "by you" for a hide this window sent', () => {
    const r = applyRoom(two, { t: 'hide', offset: 7, ids: [2] }, 1, { 2: 'spam or scam' });
    expect(r.hidden[2]).toEqual({ at_ms: 1, by: 'you', why: 'spam or scam' });
  });

  it('adds, features and pins', () => {
    let r = applyRoom(two, { t: 'msg', offset: 8, m: msg(3, 'Jessie Brown', 'play it again Rory') }, 0);
    expect(r.lines).toHaveLength(3);
    r = applyRoom(r, { t: 'feature', offset: 9, m: { ...r.lines[2], featured: true } }, 77);
    expect(r.lines[2].featured).toBe(true);
    expect(r.featuredAt[3]).toBe(77);
    r = applyRoom(r, { t: 'pin', offset: 10, m: r.lines[0] }, 0);
    expect(r.pinned?.id).toBe(1);
    expect(r.lines[0].pinned).toBe(true);
    r = applyRoom(r, { t: 'hide', offset: 11, ids: [1] }, 0);
    expect(r.pinned).toBeNull();
  });

  it('words the pace and an account’s age', () => {
    const now = 100_000;
    expect(rateWords(Array.from({ length: 70 }, (_, i) => now - i * 100), now)).toBe('7 a second');
    expect(rateWords([now - 30_000, now - 20_000], now)).toBe('2 a minute');
    expect(rateWords([], now)).toBe('quiet');
    expect(ageWords(40_000)).toBe('40 s');
    expect(ageWords(3 * 86_400_000)).toBe('3 days');
  });
});

describe('the statement of reasons', () => {
  const base = { station: 'Kettle FM', show: 'The Kettle Phone-In', at: '20:13', reason: 'abuse' as const, then: 'none' as const, automated: true, info: null };

  it('says what was hidden, why, who decided, and how to ask for a review', () => {
    expect(statementPreview(base)).toBe(
      'Kettle FM hid your message of 20:13 in The Kettle Phone-In because it broke our terms (abuse or harassment). An automatic filter flagged it and a moderator decided. You can ask for a review within 14 days.',
    );
  });

  it('adds the time-out or the ban, and the station’s own retention words', () => {
    const info = {
      reasons: [{ reason: 'spam' as const, words: 'Spam or a scam', hint: '' }],
      retention: { messages: '', moderation: 'Reports and this notice are kept for 12 months.', addresses: '' },
    };
    const t = statementPreview({ ...base, reason: 'spam', then: 'timeout', automated: false, info });
    expect(t).toContain('(spam or a scam)');
    expect(t).toContain('A moderator decided. You cannot post for 10 minutes.');
    expect(t.endsWith('Reports and this notice are kept for 12 months.')).toBe(true);
    expect(statementPreview({ ...base, then: 'ban' })).toContain('You can no longer post here.');
    expect(statementPreview({ ...base, at: null })).toContain('hid your message in The Kettle Phone-In');
  });

  it('never calls the station compliant', () => {
    expect(statementPreview(base).toLowerCase()).not.toContain('compliant');
  });

  it('takes a report’s reason, with the police path for the two only chatters give', () => {
    expect(sorReasonOf('spam')).toBe('spam');
    expect(sorReasonOf('threat_to_life')).toBe('illegal');
    expect(sorReasonOf('child_safety')).toBe('illegal');
  });
});

describe("chatters' pictures (ADR-0235)", () => {
  const waiting = (id: string, name: string, at: number) => ({
    id,
    account: { id: `a-${name}`, name, since_ms: 0, standing: 'member' as const, guest: false, restricted: null, avatar: { kind: 'stock' as const, id: 'fox' } },
    uploaded_ms: at,
    bytes: 21_000,
  });

  it('a picture waiting for approval takes its place in Needs you by its age, and leaves when decided', () => {
    let s = applyMod(EMPTY_MOD, { t: 'mod_held', h: held(3, 4_000, 9_000) });
    s = applyMod(s, { t: 'mod_avatar', a: waiting('p-1', 'Morag Fraser', 2_000) });
    s = applyMod(s, { t: 'mod_avatar', a: waiting('p-2', 'Rory MacLeod', 6_000) });
    expect(needsList(s).map((e) => e.key)).toEqual(['avatar:p-1', 'held:3', 'avatar:p-2']);
    // The same picture sent twice is one entry; a decision elsewhere removes it.
    s = applyMod(s, { t: 'mod_avatar', a: waiting('p-1', 'Morag Fraser', 2_000) });
    expect(s.avatars).toHaveLength(2);
    s = applyMod(s, { t: 'mod_avatar_done', id: 'p-1' });
    expect(needsList(s).map((e) => e.key)).toEqual(['held:3', 'avatar:p-2']);
    expect(applyMod(s, { t: 'mod_avatar_done', id: 'never' })).toBe(s);
  });

  it('a snapshot from a chat without pictures still lists what it has', () => {
    expect(needsList({ items: [], held: [held(3, 4_000, null)], studio: [] }).map((e) => e.key)).toEqual(['held:3']);
  });

  it("an avatar frame repaints that chatter's lines and the pinned one, nobody else's", () => {
    const mine = msg(1, 'Morag Fraser', 'Evening from Crieff');
    const other = msg(2, 'Hamish Murray', 'Hello');
    let r: Room = { ...EMPTY_ROOM, lines: [mine, other], pinned: mine };
    r = applyRoom(r, { t: 'avatar', offset: 9, account: 'a-Morag Fraser', avatar: { kind: 'upload', id: 'p-1' } }, 10_000);
    expect(r.lines[0].author.avatar).toEqual({ kind: 'upload', id: 'p-1' });
    expect(r.lines[1].author.avatar).toBeUndefined();
    expect(r.pinned?.author.avatar).toEqual({ kind: 'upload', id: 'p-1' });
    r = applyRoom(r, { t: 'avatar', offset: 10, account: 'a-Morag Fraser', avatar: null }, 11_000);
    expect(r.lines[0].author.avatar).toBeNull();
  });

  it('the statement for a picture says what was done and that nobody can see it now', () => {
    const facts = { station: 'Kettle FM', show: 'The Late Line', at: null, reason: 'abuse' as const, then: 'none' as const, automated: false, info: null };
    const refused = statementPreview({ ...facts, subject: 'avatar-refuse' });
    expect(refused).toContain('Kettle FM did not approve the picture you uploaded because it broke our terms (abuse or harassment).');
    expect(refused).toContain('You can ask for a review within 14 days.');
    expect(refused).not.toContain('The Late Line');
    expect(statementPreview({ ...facts, subject: 'avatar-remove' })).toContain('removed your picture from its chat');
  });
});
