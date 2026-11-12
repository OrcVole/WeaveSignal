// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The chat socket client against a fake WebSocket: hello, welcome, recovery, the held copy of
// one's own message, and the reconnect rules.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Me, Msg, RoomRules, ServerFrame } from '../../../packages/ui/chat.ts';
import { ChatClient } from './lib/chat/client.svelte.ts';

class FakeSocket {
  static OPEN = 1;
  static all: FakeSocket[] = [];
  readyState = 0;
  sent: unknown[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onclose: ((ev: { code: number }) => void) | null = null;
  constructor(readonly url: string) {
    FakeSocket.all.push(this);
  }
  send(s: string) {
    this.sent.push(JSON.parse(s));
  }
  close() {
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  get(f: ServerFrame) {
    this.onmessage?.({ data: JSON.stringify(f) });
  }
  drop(code = 1006) {
    this.readyState = 3;
    this.onclose?.({ code });
  }
}

const rules: RoomRules = {
  slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false,
  comments_only: false, guests: true, slow_s: 10, min_age_minutes: 10, max_chars: 500, hold_ms: 5000,
  hours: { shows_only: false, open: true, opens_at_ms: null, zone: '' },
};
const me: Me = { id: 'a-jessie', name: 'Jessie Brown', kind: 'guest', standing: 'member', created_ms: 0, timed_out_until_ms: null, banned: false, moderator: false };
const msg = (id: number, body = 'Hiya'): Msg => ({
  id, kind: 'chat', body, author: { id: 'a-alastair', name: 'Alastair Buchanan', standing: 'member', guest: false }, reply_to: null,
  server_ms: 1000 * id, playhead_ms: null, state: 'visible', pinned: false, featured: false, reactions: [],
});
function welcome(over: Partial<Extract<ServerFrame, { t: 'welcome' }>> = {}): ServerFrame {
  return {
    t: 'welcome', epoch: 'e1', offset: 2, recovered: false, missed: [], history: [msg(1), msg(2)], station: 'Kettle FM', show: 'The Morning Kettle',
    me: null, rules, listener_delay_ms: 15_000, server_now_ms: Date.now() + 1500, pinned: null, count: 7, statements: [], moderation: null, ...over,
  };
}

beforeEach(() => {
  FakeSocket.all = [];
  vi.useFakeTimers();
  vi.stubGlobal('WebSocket', FakeSocket);
  vi.stubGlobal('location', { protocol: 'https:', host: 'radio.example' });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the chat socket', () => {
  it('says hello with the session, then takes the welcome', () => {
    const c = new ChatClient('tok-1');
    c.start();
    const ws = FakeSocket.all[0];
    expect(ws.url).toBe('wss://radio.example/api/v1/chat/socket');
    ws.open();
    expect(ws.sent[0]).toEqual({ t: 'hello', session: 'tok-1' });
    ws.get(welcome());
    expect(c.link).toBe('open');
    expect(c.list.map((m) => m.id)).toEqual([1, 2]);
    expect(c.count).toBe(7);
    expect(Math.abs(c.skewMs - 1500)).toBeLessThan(5);
    c.stop();
  });

  it('reconnects with backoff and recovers from the last offset', () => {
    const c = new ChatClient(null);
    c.start();
    let ws = FakeSocket.all[0];
    ws.open();
    ws.get(welcome());
    ws.get({ t: 'msg', offset: 3, m: msg(3) });
    ws.drop();
    expect(c.link).toBe('reconnecting');
    vi.advanceTimersByTime(999);
    expect(FakeSocket.all).toHaveLength(1);
    vi.advanceTimersByTime(1);
    ws = FakeSocket.all[1];
    ws.open();
    expect(ws.sent[0]).toEqual({ t: 'hello', since: { epoch: 'e1', offset: 3 } });
    ws.get(welcome({ offset: 5, recovered: true, history: [], missed: [{ t: 'msg', offset: 4, m: msg(4) }, { t: 'hide', offset: 5, ids: [1] }] }));
    expect(c.list.map((m) => m.id)).toEqual([2, 3, 4]);
    // The next drop starts the backoff at 1 s again.
    ws.drop();
    vi.advanceTimersByTime(1000);
    expect(FakeSocket.all).toHaveLength(3);
    c.stop();
  });

  it('reconnects at once when closed for falling behind (4008)', () => {
    const c = new ChatClient(null);
    c.start();
    FakeSocket.all[0].open();
    FakeSocket.all[0].get(welcome());
    FakeSocket.all[0].drop(4008);
    expect(FakeSocket.all).toHaveLength(2);
    c.stop();
  });

  it('forgets the session when signed out by the server (4001)', () => {
    const c = new ChatClient('tok-1');
    let told = false;
    c.onSignedOut = () => (told = true);
    c.start();
    FakeSocket.all[0].open();
    FakeSocket.all[0].get(welcome({ me }));
    FakeSocket.all[0].drop(4001);
    expect(told).toBe(true);
    expect(c.session).toBeNull();
    expect(c.me).toBeNull();
    FakeSocket.all[1].open();
    expect(FakeSocket.all[1].sent[0]).toMatchObject({ t: 'hello' });
    expect(FakeSocket.all[1].sent[0]).not.toHaveProperty('session');
    c.stop();
  });

  it('keeps a copy of its own held message until the room has it', async () => {
    const c = new ChatClient('tok-1');
    c.start();
    const ws = FakeSocket.all[0];
    ws.open();
    ws.get(welcome({ me }));
    const sent = c.post('chat', 'Play Tide’s Out', null, undefined);
    const post = ws.sent[1] as { nonce: string; body: string };
    expect(post).toMatchObject({ t: 'post', kind: 'chat', body: 'Play Tide’s Out' });
    expect(post).not.toHaveProperty('playhead_ms');
    ws.get({ t: 'ack', nonce: post.nonce, id: 9, state: 'held', code: null, words: null, retry_ms: null });
    expect(await sent).toEqual({ state: 'held', id: 9 });
    expect(c.held.map((m) => [m.id, m.state, m.author.name])).toEqual([[9, 'held', 'Jessie Brown']]);
    ws.get({ t: 'released', ids: [9] });
    expect(c.held[0].state).toBe('visible');
    ws.get({ t: 'msg', offset: 3, m: msg(9, 'Play Tide’s Out') });
    expect(c.held).toEqual([]);
    expect(c.list.at(-1)?.id).toBe(9);
    c.stop();
  });

  it('keeps its own message with a picture waiting in the room, not in the lane to the studio', () => {
    const c = new ChatClient('tok-1');
    c.start();
    const ws = FakeSocket.all[0];
    ws.open();
    ws.get(welcome({ me }));
    const attachment = { id: 'p-1', kind: 'picture' as const, name: 'map.png', mime: 'image/png', label: 'PNG', bytes: 9000, width: 800, height: 600 };
    ws.get({ t: 'mine', m: { ...msg(11, 'Where we are'), state: 'held', attachment } });
    expect(c.held.map((m) => [m.id, m.state, m.attachment?.id])).toEqual([[11, 'held', 'p-1']]);
    expect(c.lane).toEqual([]);
    ws.get({ t: 'rejected', ids: [11] });
    expect(c.held).toEqual([]);
    c.stop();
  });

  it('answers a refusal with its words and wait, and a lost link as not sent', async () => {
    const c = new ChatClient('tok-1');
    c.start();
    const ws = FakeSocket.all[0];
    ws.open();
    ws.get(welcome({ me }));
    const a = c.post('chat', 'again', null, 123);
    const post = ws.sent[1] as { nonce: string };
    expect(ws.sent[1]).toMatchObject({ playhead_ms: 123 });
    ws.get({ t: 'ack', nonce: post.nonce, id: null, state: 'refused', code: 'slow', words: 'Slow mode is on: one message every 10 seconds.', retry_ms: 6000 });
    expect(await a).toEqual({ state: 'refused', code: 'slow', words: 'Slow mode is on: one message every 10 seconds.', retry_ms: 6000 });
    const b = c.post('studio', 'Hello studio', null, undefined);
    ws.drop();
    expect((await b).state).toBe('refused');
    c.stop();
  });

  it('pings every 25 s, and keeps the studio lane and statements', () => {
    const c = new ChatClient('tok-1');
    c.start();
    const ws = FakeSocket.all[0];
    ws.open();
    ws.get(welcome({ me }));
    vi.advanceTimersByTime(25_000);
    expect(ws.sent.at(-1)).toEqual({ t: 'ping' });
    ws.get({ t: 'private', id: 'p1', at_ms: 5000, from: 'The studio', body: 'Fancy calling in?', invite: true });
    ws.get({ t: 'mine', m: { ...msg(8, 'Can I say hello on air?'), kind: 'studio', server_ms: 4000 } });
    expect(c.lane.map((e) => [e.mine, e.invite])).toEqual([[true, false], [false, true]]);
    ws.get({ t: 'statement', s: { id: 's1', at_ms: 1, text: 'Hidden: spam.', automated: true, review_by_ms: 2, review_requested: false } });
    expect(c.statements).toHaveLength(1);
    ws.get({ t: 'feature', offset: 3, m: { ...msg(2), featured: true } });
    expect(c.featured?.m.id).toBe(2);
    ws.get({ t: 'hide', offset: 4, ids: [2] });
    expect(c.featured).toBeNull();
    c.stop();
  });
});
