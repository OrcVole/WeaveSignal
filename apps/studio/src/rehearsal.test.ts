// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal studio's data in the studio app (1.3.0, step R12): the rules in
// `lib/rehearsal.ts`, the calls in `lib/api.ts`, the store in `lib/rehearsal.svelte.ts` fed by
// the event stream, and the week's columns and rehearsal blocks in `lib/schedule.ts`. The
// station is stubbed at `fetch`. Sample people are from the Tartan National Names list
// (ADR-0071); the shows are Kettle FM's.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import type { RehearsalBlock, RehearsalPerson, RehearsalView } from './lib/api.ts';
import { bookingChangeable, bookingClashAt, holdingBooking, inTheMix, mixCommand, openingPreview, phaseOf, rehearsing, roomFull } from './lib/rehearsal.ts';
import { chooseDeskStudio, endRehearsalNow, openRehearsalFor, rehearsal, rehearsalPhase, sendRehearsal, startRehearsal, stopRehearsal } from './lib/rehearsal.svelte.ts';
import { resetStores } from './lib/reset.ts';
import * as schedApi from './lib/schedule.ts';
import { studio } from './lib/studio.svelte.ts';

const person = (identity: string, name: string, over: Partial<RehearsalPerson> = {}): RehearsalPerson => ({
  identity,
  name,
  role: 'guest',
  on: true,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present: true,
  linked: true,
  lufs: -20,
  ...over,
});

const OPEN: RehearsalView = {
  enabled: true,
  desk: { linked: true, error: null },
  session: { id: 's1', show: 'steeped', show_name: 'Steeped', opened_by: 'Eilidh', opened_at_ms: 1_000 },
  people: [
    person('eilidh', 'Eilidh Fraser-MacLeod', { role: 'presenter' }),
    person('gregor', 'Gregor Stewart', { mute: true }),
    person('kirsty', 'Kirsty Wilson', { on: false }),
    person('finlay', 'Finlay Grant', { present: false }),
  ],
  carts_sounding: [],
  max_people: 6,
};

const block = (id: string, kind: 'booking' | 'session', start_ms: number, end_ms: number, show = 'steeped'): RehearsalBlock => ({
  id,
  kind,
  show,
  show_name: show === 'steeped' ? 'Steeped' : 'The Morning Kettle',
  colour_slot: 4,
  start_ms,
  end_ms,
  by: 'Finlay',
  open: false,
  ended: null,
});

describe('the rehearsal studio’s rules', () => {
  it('says where the studio stands', () => {
    expect(phaseOf(null, false)).toBe('unknown');
    expect(phaseOf(null, true)).toBe('absent');
    expect(phaseOf({ ...OPEN, enabled: false, session: null }, false)).toBe('off');
    expect(phaseOf({ ...OPEN, session: null }, false)).toBe('closed');
    expect(phaseOf({ ...OPEN, session: null, desk: { linked: false, error: 'not answering' } }, false)).toBe('desk-down');
    expect(phaseOf(OPEN, false)).toBe('open');
    // An open session stays open while its desk starts again.
    expect(phaseOf({ ...OPEN, desk: { linked: false, error: 'not answering' } }, false)).toBe('open');
  });

  it('counts only the people present, and carries their switches as the opening state', () => {
    expect(inTheMix(OPEN).map((p) => p.identity)).toEqual(['eilidh', 'gregor']);
    expect(openingPreview(OPEN)).toEqual({ on: ['eilidh', 'gregor'], mute: ['gregor'] });
    expect(openingPreview(null)).toEqual({ on: [], mute: [] });
    expect(roomFull(OPEN)).toBe(false);
    expect(roomFull({ ...OPEN, max_people: 3 })).toBe(true);
    expect(rehearsing(OPEN, 'steeped')).toBe(true);
    expect(rehearsing(OPEN, 'the-morning-kettle')).toBe(false);
    expect(mixCommand('kirsty', true)).toEqual({ op: 'on_air', identity: 'kirsty', on: true });
  });

  it('finds the booking holding the studio and the one a new booking would clash with', () => {
    const blocks = [block('b1', 'booking', 1_000, 2_000), block('s1', 'session', 1_500, 1_800), block('b2', 'booking', 3_000, 4_000, 'the-morning-kettle')];
    expect(holdingBooking(blocks, 999)).toBeNull();
    expect(holdingBooking(blocks, 1_000)?.id).toBe('b1');
    expect(holdingBooking(blocks, 2_000)).toBeNull();
    expect(bookingClashAt(blocks, 1_500, 2_500)?.id).toBe('b1');
    expect(bookingClashAt(blocks, 2_000, 3_000)).toBeNull();
    expect(bookingClashAt(blocks, 1_200, 1_800, 'b1')).toBeNull();
    expect(bookingChangeable(blocks[0], 1_999)).toBe(true);
    expect(bookingChangeable(blocks[0], 2_000)).toBe(false);
    expect(bookingChangeable(blocks[1], 0)).toBe(false);
  });
});

describe('the calls', () => {
  const sent: { url: string; method: string; body: unknown }[] = [];
  let reply: { status: number; body: unknown } = { status: 200, body: {} };
  beforeEach(() => {
    sent.length = 0;
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      sent.push({ url, method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (reply.status === 204) return new Response(null, { status: 204 });
      return new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'content-type': 'application/json' } });
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('books, changes and deletes, and keeps the booking a refusal was over', async () => {
    reply = { status: 201, body: block('b1', 'booking', 1_000, 2_000) };
    expect((await api.makeBooking({ show: 'steeped', start_ms: 1_000, end_ms: 2_000 })).id).toBe('b1');
    expect(sent[0]).toEqual({ url: '/api/v1/rehearsal/bookings', method: 'POST', body: { show: 'steeped', start_ms: 1_000, end_ms: 2_000 } });
    reply = { status: 200, body: block('b1', 'booking', 1_500, 2_000) };
    await api.changeBooking('b1', { start_ms: 1_500 });
    expect(sent[1]).toEqual({ url: '/api/v1/rehearsal/bookings/b1', method: 'PATCH', body: { start_ms: 1_500 } });
    reply = { status: 204, body: null };
    await api.deleteBooking('b1');
    expect(sent[2].method).toBe('DELETE');
    reply = { status: 200, body: { enabled: true, bookings: [] } };
    await api.listBookings(0, 10);
    expect(sent[3].url).toBe('/api/v1/rehearsal/bookings?from_ms=0&to_ms=10');

    reply = { status: 409, body: { error: 'That time is booked for The Morning Kettle from 13:30 until 14:30 (Kirsty): bookings never share time.', clash: block('b2', 'booking', 0, 1, 'the-morning-kettle') } };
    const e = await api.makeBooking({ show: 'steeped', start_ms: 0, end_ms: 1 }).catch((x: unknown) => x);
    expect(api.bookingClash(e)?.id).toBe('b2');
    expect((e as Error).message).toContain('booked for The Morning Kettle');
    reply = { status: 409, body: { error: 'That booking for Steeped is over.' } };
    expect(api.bookingClash(await api.deleteBooking('b1').catch((x: unknown) => x))).toBeNull();
    reply = { status: 422, body: { error: 'A booking ends after it starts.', field: 'end_ms' } };
    const bad = await api.makeBooking({ show: 'steeped', start_ms: 1, end_ms: 1 }).catch((x: unknown) => x);
    expect((bad as api.FieldError).field).toBe('end_ms');
    expect(api.bookingClash(bad)).toBeNull();
  });

  it('takes the week’s columns, each episode’s studio and the rehearsals, and none from an older station', async () => {
    const ep = { show: { slug: 'steeped', name: 'Steeped' }, series: {}, episode: {}, resolved: {} };
    reply = {
      status: 200,
      body: {
        zone: 'Europe/London',
        episodes: [{ ...ep, studio: 'studio', from: 'own' }],
        dates: [],
        studios: [
          { id: 'studio', name: 'Studio' },
          { id: 'rehearsal', name: 'Rehearsal' },
        ],
        rehearsals: [block('b1', 'booking', 1_000, 2_000), { kind: 'nonsense' }, block('s1', 'session', 3_000, 2_000)],
      },
    };
    const w = await schedApi.weekEpisodes(0, 1);
    expect(w.studios?.map((s) => s.id)).toEqual(['studio', 'rehearsal']);
    expect(w.rehearsals.map((b) => b.id)).toEqual(['b1']);
    expect(schedApi.studioOf(w.episodes[0])).toBe('studio');
    expect(schedApi.fromOf(w.episodes[0])).toBe('own');

    reply = { status: 200, body: { zone: 'Europe/London', episodes: [ep], dates: [] } };
    const old = await schedApi.weekEpisodes(0, 1);
    expect(old.studios).toBeNull();
    expect(old.rehearsals).toEqual([]);
    expect(schedApi.studioOf(old.episodes[0])).toBe('studio');
    expect(schedApi.fromOf(old.episodes[0])).toBe('room');
  });
});

/** A response whose body is an event stream with these views, then held open. */
function stream(views: RehearsalView[]): Response {
  const text = views.map((v) => `event: state\ndata: ${JSON.stringify(v)}\n\n`).join('');
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      c.enqueue(new TextEncoder().encode(text));
    },
  });
  return new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } });
}

const settle = () => new Promise((r) => setTimeout(r, 10));

describe('the store', () => {
  const sent: { url: string; method: string; body: unknown }[] = [];
  afterEach(() => {
    stopRehearsal();
    resetStores();
    vi.unstubAllGlobals();
    sent.length = 0;
  });

  it('reads the view, follows its events, and sends the presses', async () => {
    const closed = { ...OPEN, session: null, people: [] };
    let answer: { status: number; body: unknown } = { status: 200, body: OPEN };
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      sent.push({ url, method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (url === '/api/v1/rehearsal') return new Response(JSON.stringify(closed), { status: 200 });
      if (url === '/api/v1/rehearsal/events') return stream([closed, OPEN]);
      return new Response(JSON.stringify(answer.body), { status: answer.status });
    });
    expect(rehearsalPhase()).toBe('unknown');
    startRehearsal();
    await settle();
    expect(rehearsal.link).toBe('live');
    expect(rehearsalPhase()).toBe('open');
    expect(rehearsal.view?.session?.show_name).toBe('Steeped');

    expect(await sendRehearsal(mixCommand('kirsty', true))).toBe(true);
    expect(sent.at(-1)).toEqual({ url: '/api/v1/rehearsal/commands', method: 'POST', body: { op: 'on_air', identity: 'kirsty', on: true } });

    answer = { status: 409, body: { error: 'Eilidh is rehearsing Steeped.' } };
    expect(await openRehearsalFor('the-morning-kettle')).toBe(false);
    expect(sent.at(-1)?.body).toEqual({ show: 'the-morning-kettle' });
    expect(rehearsal.error).toBe('Eilidh is rehearsing Steeped.');
    expect(rehearsal.errorStatus).toBe(409);
    expect(rehearsal.busy).toBe(false);

    answer = { status: 200, body: closed };
    expect(await endRehearsalNow()).toBe(true);
    expect(sent.at(-1)?.url).toBe('/api/v1/rehearsal/end');
    expect(rehearsal.error).toBeNull();
    expect(rehearsalPhase()).toBe('closed');
  });

  it('settles on absent at a station older than 1.3.0, without reconnecting', async () => {
    vi.stubGlobal('fetch', async (url: string) => {
      sent.push({ url, method: 'GET', body: undefined });
      return new Response('{}', { status: 404 });
    });
    startRehearsal();
    await settle();
    await settle();
    expect(rehearsalPhase()).toBe('absent');
    expect(sent.map((s) => s.url)).toEqual(['/api/v1/rehearsal']);
  });

  it('is emptied, and the desk’s studio is live again, on a change of station', async () => {
    vi.stubGlobal('fetch', async (url: string) => (url === '/api/v1/rehearsal/events' ? stream([OPEN]) : new Response(JSON.stringify(OPEN), { status: 200 })));
    startRehearsal();
    chooseDeskStudio('rehearsal');
    await settle();
    expect(studio.deskStudio).toBe('rehearsal');
    expect(rehearsal.view).not.toBeNull();
    resetStores();
    expect(rehearsal.view).toBeNull();
    expect(rehearsalPhase()).toBe('unknown');
    expect(studio.deskStudio).toBe('live');
  });
});
