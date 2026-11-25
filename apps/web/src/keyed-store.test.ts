// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A listener's key on this device (lib/keyed/keyed.svelte.ts) against a station that answers as
// the control plane does: what is kept and what is let go, the token renewed while the page
// lives and never stored, a key that stops mid-listen, a page that slept past its token, the
// devices and Sign out, and a station that does not answer.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Keyed } from './lib/keyed/keyed.svelte.ts';
import { ROUTES, type Device, type ListenToken } from './lib/keyed/routes.ts';

const KEY = 'KETM-7QMP-2D4X';
const NOW = Date.UTC(2026, 10, 20, 13, 52);
const store = new Map<string, string>();
let calls: { path: string; body: Record<string, unknown> }[] = [];
/** What the station answers next, by route; a function sees the body. */
let answers: Record<string, (body: Record<string, unknown>) => [number, unknown]> = {};
let n = 0;

/** The show a token is for: the one asked for, else the Friday show. */
const forShow = (id: unknown) => ({ id: typeof id === 'string' ? id : 's-fri', slug: typeof id === 'string' ? id : 'friday-supporters', name: 'A keyed show' });
const token = (o: Partial<ListenToken> = {}): ListenToken => ({
  token: `ks1.claims${++n}.sig`,
  device: 'dev-1',
  starts_ms: Date.now() - 3_000,
  expires_ms: Date.now() + 3_600_000,
  show: forShow(null),
  urls: { hls: `/hls/k/s-fri/ks1.claims${n}.sig/live.m3u8`, opus: `/listen/k/s-fri/ks1.claims${n}.sig/programme-keyed.opus`, mp3: null },
  renew_in_s: 60,
  key: { id: 'k1', kind: 'personal', label: 'Isla Fraser', media: 'both', all_shows: true, shows: [], valid_from_ms: null, valid_until_ms: null, ends_soon: false },
  ...o,
});
const dev = (id: string, label: string): Device => ({ id, label, first_seen_ms: NOW, last_seen_ms: NOW, ended_ms: null });
const refuse = (status: number, code: string, extra: object = {}): [number, unknown] => [status, { error: 'a sentence for a program', code, ...extra }];

beforeEach(() => {
  store.clear();
  calls = [];
  n = 0;
  answers = { [ROUTES.redeem]: (b) => [200, token({ show: forShow(b.show) })], [ROUTES.renew]: () => [200, token()] };
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
  vi.stubGlobal('fetch', async (path: string, init?: { body?: string }) => {
    const body = init?.body ? (JSON.parse(init.body) as Record<string, unknown>) : {};
    calls.push({ path, body });
    const a = answers[path];
    if (!a) throw new TypeError('Failed to fetch');
    const [status, data] = a(body);
    return new Response(status === 204 || status === 202 ? null : JSON.stringify(data), { status, headers: { 'content-type': 'application/json' } });
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const kept = () => (store.has('weavesignal.key') ? JSON.parse(store.get('weavesignal.key')!) : null);
const to = (path: string) => calls.filter((c) => c.path === path);

describe('a key that is accepted', () => {
  it('is kept with this browser’s id, and its token is held in memory alone', async () => {
    const k = new Keyed(null);
    expect(await k.enter(` ${KEY} `, 's-fri')).toBe(true);
    expect(to(ROUTES.redeem)[0].body).toEqual({ key: KEY, show: 's-fri' });
    expect(k.fresh).toBe(true);
    // The name staff gave the key is theirs: this device does not keep it.
    expect(kept()).toEqual({ text: KEY, device: 'dev-1' });
    expect([...store.values()].join()).not.toContain('Isla');
    expect([...store.values()].join()).not.toContain('wk1e');
    k.stop();
  });

  it('is used again next time with the device it had, and is not a new welcome', async () => {
    const k = new Keyed({ text: KEY, device: 'dev-1' });
    expect(await k.resume(null)).toBe(true);
    expect(to(ROUTES.redeem)[0].body).toEqual({ key: KEY, device: 'dev-1' });
    expect(k.fresh).toBe(false);
    k.stop();
  });

  it('is renewed when the station says, with the token it holds', async () => {
    const k = new Keyed(null);
    await k.enter(KEY);
    const first = k.session!.token;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(to(ROUTES.renew)).toHaveLength(1);
    expect(to(ROUTES.renew)[0].body).toEqual({ token: first });
    expect(k.session!.token).not.toBe(first);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(to(ROUTES.renew)).toHaveLength(2);
    k.stop();
    await vi.advanceTimersByTimeAsync(600_000);
    expect(to(ROUTES.renew)).toHaveLength(2);
  });

  it('asks again for the show that is on when the show changes, and not otherwise', async () => {
    const k = new Keyed(null);
    await k.enter(KEY, 's-fri');
    await k.follow('s-fri');
    expect(to(ROUTES.redeem)).toHaveLength(1);
    await k.follow('s-arc');
    expect(to(ROUTES.redeem)[1].body).toEqual({ key: KEY, show: 's-arc', device: 'dev-1' });
    expect(k.session?.show.id).toBe('s-arc');
    // No keyed show on: the token held is for its show's next window, and renewing moves it on.
    await k.follow(null);
    expect(to(ROUTES.redeem)).toHaveLength(2);
    k.stop();
  });
});

describe('a key that does not work', () => {
  it('is not kept when it is not a key, has ended for good, or was ended', async () => {
    for (const code of ['not_recognised', 'ended', 'revoked']) {
      store.clear();
      answers[ROUTES.redeem] = () => refuse(code === 'not_recognised' ? 404 : 403, code, { until_ms: 5 });
      const k = new Keyed(null);
      expect(await k.enter(KEY)).toBe(false);
      expect(k.refusal?.code).toBe(code);
      expect(k.kept, code).toBeNull();
      expect(kept(), code).toBeNull();
    }
  });

  it('is kept when it may work later', async () => {
    for (const code of ['not_yet_valid', 'paused', 'does_not_open_show', 'too_many_devices', 'people_limit', 'listens_used_up']) {
      store.clear();
      answers[ROUTES.redeem] = () => refuse(403, code, { from_ms: NOW + 1000 });
      const k = new Keyed(null);
      await k.enter(KEY, 's-fri');
      expect(k.refusal?.code).toBe(code);
      expect(kept()?.text, code).toBe(KEY);
      expect(k.refusedFor).toBe('s-fri');
    }
  });

  it('carries when a key starts, and the devices that hold it', async () => {
    answers[ROUTES.redeem] = () => refuse(403, 'not_yet_valid', { from_ms: 1234 });
    const k = new Keyed(null);
    await k.enter(KEY);
    expect(k.refusal?.at_ms).toBe(1234);
    answers[ROUTES.redeem] = () => refuse(409, 'too_many_devices', { devices: [dev('d1', 'Firefox on Linux'), dev('d2', 'Safari on iPhone')] });
    await k.enter(KEY);
    expect(k.devices?.map((d) => d.label)).toEqual(['Firefox on Linux', 'Safari on iPhone']);
  });

  it('a wrong key typed over a kept one leaves the kept one alone', async () => {
    const k = new Keyed({ text: KEY, device: 'dev-1' });
    store.set('weavesignal.key', JSON.stringify(k.kept));
    answers[ROUTES.redeem] = () => refuse(404, 'not_recognised');
    await k.enter('AAAA-BBBB-CCCC');
    expect(k.kept?.text).toBe(KEY);
    expect(kept()?.text).toBe(KEY);
  });

  it('a station that does not answer keeps the key for Try again, on this page only', async () => {
    delete answers[ROUTES.redeem];
    const k = new Keyed(null);
    expect(await k.enter(KEY)).toBe(false);
    expect(k.refusal?.code).toBe('no_answer');
    expect(k.kept?.text).toBe(KEY);
    expect(kept()).toBeNull();
    answers[ROUTES.redeem] = () => [200, token()];
    expect(await k.resume(null)).toBe(true);
    k.stop();
  });
});

describe('a key that stops while it is in use', () => {
  it('revoked: the page is told at the next renewal, the stream is cut with words, and the key is let go', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY, 's-fri');
    answers[ROUTES.renew] = () => refuse(403, 'revoked');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(cut).toHaveBeenCalledTimes(1);
    expect(k.session).toBeNull();
    expect(k.refusal?.code).toBe('revoked');
    expect(kept()).toBeNull();
    // And it stops renewing.
    await vi.advanceTimersByTimeAsync(300_000);
    expect(to(ROUTES.renew)).toHaveLength(1);
  });

  it('paused: cut the same way, and kept for when it is switched back on', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY);
    answers[ROUTES.renew] = () => refuse(403, 'paused');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(cut).toHaveBeenCalledTimes(1);
    expect(k.refusal?.code).toBe('paused');
    expect(kept()?.text).toBe(KEY);
  });

  it('a page that slept past its token asks with the key again, and says nothing', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY, 's-fri');
    answers[ROUTES.renew] = () => [401, { error: 'That token has ended. Enter the key again.', code: 'token_ended' }];
    await vi.advanceTimersByTimeAsync(60_000);
    expect(to(ROUTES.redeem)).toHaveLength(2);
    expect(to(ROUTES.redeem)[1].body).toEqual({ key: KEY, show: 's-fri', device: 'dev-1' });
    expect(k.session).not.toBeNull();
    expect(k.refusal).toBeNull();
    expect(cut).not.toHaveBeenCalled();
    expect(k.fresh).toBe(false);
    k.stop();
  });

  it('a stream that stops is asked about at once: a revoked key is said then, not a minute later', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY, 's-fri');
    expect(await k.check()).toBe(true);
    answers[ROUTES.renew] = () => refuse(403, 'revoked');
    expect(await k.check()).toBe(false);
    expect(cut).toHaveBeenCalledTimes(1);
    expect(k.refusal?.code).toBe('revoked');
  });

  it('a show that has ended, with nothing of the key’s on the schedule, is no alarm', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY, 's-fri');
    answers[ROUTES.renew] = () => refuse(403, 'not_scheduled');
    answers[ROUTES.redeem] = () => refuse(403, 'not_scheduled');
    await vi.advanceTimersByTimeAsync(60_000);
    // The key is asked for any show it opens, then kept quietly for later.
    expect(to(ROUTES.redeem)[1].body).toEqual({ key: KEY, device: 'dev-1' });
    expect(cut).not.toHaveBeenCalled();
    expect(k.refusal?.code).toBe('not_scheduled');
    expect(kept()?.text).toBe(KEY);
  });

  it('a show that has ended moves the key on to another show it opens', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY, 's-fri');
    answers[ROUTES.renew] = () => refuse(403, 'not_scheduled');
    answers[ROUTES.redeem] = () => [200, token({ show: forShow('s-arc'), starts_ms: Date.now() + 3_600_000 })];
    await vi.advanceTimersByTimeAsync(60_000);
    expect(k.session?.show.id).toBe('s-arc');
    expect(cut).not.toHaveBeenCalled();
    k.stop();
  });

  it('a dropped connection is tried again while the token lives, and is not a refusal', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY);
    delete answers[ROUTES.renew];
    await vi.advanceTimersByTimeAsync(60_000);
    expect(k.session).not.toBeNull();
    expect(cut).not.toHaveBeenCalled();
    answers[ROUTES.renew] = () => [200, token()];
    await vi.advanceTimersByTimeAsync(5_000);
    expect(k.session).not.toBeNull();
    expect(k.refusal).toBeNull();
    k.stop();
  });

  it('a station gone for the whole of a token’s life ends the keyed stream with words', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY);
    delete answers[ROUTES.renew];
    await vi.advanceTimersByTimeAsync(3_700_000);
    expect(cut).toHaveBeenCalledTimes(1);
    expect(k.refusal?.code).toBe('no_answer');
    expect(kept()?.text).toBe(KEY);
  });
});

describe('the devices', () => {
  it('signing another device out of a full key uses the key here at once', async () => {
    answers[ROUTES.redeem] = () => refuse(409, 'too_many_devices', { devices: [dev('d1', 'Firefox on Linux'), dev('d2', 'Safari on iPhone')] });
    answers[ROUTES.endDevice] = () => [204, null];
    answers[ROUTES.devices] = () => [200, { devices: [dev('d1', 'Firefox on Linux'), dev('dev-1', 'Chrome on Android')] }];
    const k = new Keyed(null);
    await k.enter(KEY, 's-fri');
    answers[ROUTES.redeem] = () => [200, token()];
    await k.signOut('d2');
    expect(to(ROUTES.endDevice)[0].body).toEqual({ key: KEY, device: 'd2' });
    expect(k.session).not.toBeNull();
    expect(k.refusal).toBeNull();
    expect(k.fresh).toBe(true);
    expect(k.devices?.map((d) => d.id)).toEqual(['d1', 'dev-1']);
    k.stop();
  });

  it('signing this device out lets go of the key here', async () => {
    answers[ROUTES.endDevice] = () => [204, null];
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY);
    await k.signOut('dev-1');
    expect(k.session).toBeNull();
    expect(k.kept).toBeNull();
    expect(kept()).toBeNull();
    expect(cut).toHaveBeenCalledTimes(1);
  });

  it('says when the sign-out did not go through, and changes nothing', async () => {
    const k = new Keyed(null);
    await k.enter(KEY);
    await k.signOut('d2');
    expect(k.devicesProblem).toBe(true);
    expect(k.session).not.toBeNull();
    k.stop();
  });
});

describe('letting go of a key', () => {
  it('forgets it on this device, and stops the keyed stream', async () => {
    const k = new Keyed(null);
    const cut = vi.fn();
    k.onCut = cut;
    await k.enter(KEY);
    k.forget();
    expect([k.session, k.kept, k.refusal, k.devices]).toEqual([null, null, null, null]);
    expect(kept()).toBeNull();
    expect(cut).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(300_000);
    expect(to(ROUTES.renew)).toHaveLength(0);
  });
});

describe('the keyed shows', () => {
  it('are read, and a station that has none of this reads as nothing', async () => {
    const k = new Keyed(null);
    answers[ROUTES.keyedShows] = () => [200, { shows: [{ id: 's-fri', slug: 'friday-supporters', name: 'Friday Supporters’ Hour', now: 'key', opens_at_ms: null, slate: 'announcement', slate_note: '' }], get_a_key_text: '', get_a_key_url: '' }];
    await k.loadShows();
    expect(k.shows?.shows[0].slug).toBe('friday-supporters');
    answers[ROUTES.keyedShows] = () => [404, { error: 'not found' }];
    await k.loadShows();
    expect(k.shows).toBeNull();
    expect(k.asked).toBe(true);
  });
});
