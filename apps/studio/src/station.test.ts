// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The packaged app's link to its station (ADR-0121): requests go out from the Rust side, which
// adds the key; the page never holds it. Tauri's core is replaced by a fake that answers as
// main.rs's commands do.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const calls: { cmd: string; args: unknown; opts?: { headers: Record<string, string> } }[] = [];
let channels: FakeChannel[] = [];
let answer: (cmd: string, args: unknown) => unknown = () => null;

class FakeChannel {
  id = channels.length + 1;
  onmessage: (m: unknown) => void = () => {};
  constructor() {
    channels.push(this);
  }
}

vi.mock('@tauri-apps/api/core', () => ({
  Channel: FakeChannel,
  invoke: async (cmd: string, args: unknown, opts?: { headers: Record<string, string> }) => {
    calls.push({ cmd, args, opts });
    return answer(cmd, args);
  },
}));

const store = new Map<string, string>();
beforeEach(() => {
  calls.length = 0;
  channels = [];
  store.clear();
  vi.stubGlobal('location', { protocol: 'tauri:', hostname: 'localhost', search: '' });
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
  vi.resetModules();
});
afterEach(() => vi.unstubAllGlobals());

const STATION = { url: 'https://radio.example.org', kept: 'keychain' };

describe('the packaged app and its station', () => {
  it('moves a key an older build kept in local storage into the keychain, and wipes it', async () => {
    store.set('weavesignal.station', JSON.stringify({ url: STATION.url, token: 'old-admin-key' }));
    answer = (cmd) => (cmd === 'station_info' ? STATION : cmd === 'station_save' ? STATION : null);
    const api = await import('./lib/api.ts');
    await api.loadStation();
    expect(calls.find((c) => c.cmd === 'station_save')?.args).toEqual({ url: STATION.url, key: 'old-admin-key' });
    expect(store.has('weavesignal.station')).toBe(false);
    expect(api.savedStation()).toEqual(STATION);
    expect(JSON.stringify(api.savedStation())).not.toContain('old-admin-key');
  });

  it('sends a command through Rust with no key in the page, and reads the answer', async () => {
    answer = (cmd) => {
      if (cmd === 'station_info') return STATION;
      if (cmd === 'station_fetch') {
        const ch = channels.at(-1)!;
        setTimeout(() => {
          ch.onmessage(new TextEncoder().encode('{"station":').buffer);
          ch.onmessage(new TextEncoder().encode('"Kettle FM"}').buffer);
          ch.onmessage({ end: true });
        });
        return { id: 7, status: 200, status_text: 'OK', content_type: 'application/json' };
      }
      return null;
    };
    const api = await import('./lib/api.ts');
    await api.loadStation();
    expect(api.headers(true)).toEqual({ 'Content-Type': 'application/json' });
    const view = await api.desk({ op: 'dump' });
    expect(view).toEqual({ station: 'Kettle FM' });
    const sent = calls.find((c) => c.cmd === 'station_fetch')!;
    expect(sent.opts?.headers['x-station-method']).toBe('POST');
    expect(sent.opts?.headers['x-station-path']).toBe('/api/v1/desk/commands');
    expect(sent.opts?.headers['x-station-content-type']).toBe('application/json');
    expect(JSON.stringify(sent.opts?.headers)).not.toMatch(/authorization|bearer/i);
    expect(new TextDecoder().decode(sent.args as Uint8Array)).toBe('{"op":"dump"}');
  });

  it('a refusal from Rust (no station answered) is a failed fetch; a 204 has no body', async () => {
    let n = 0;
    answer = (cmd) => {
      if (cmd === 'station_info') return STATION;
      if (cmd === 'station_fetch') {
        n += 1;
        if (n === 1) throw 'The station did not answer.';
        return { id: 8, status: 204, status_text: 'No Content', content_type: null };
      }
      return null;
    };
    const api = await import('./lib/api.ts');
    await api.loadStation();
    await expect(api.netFetch(`${STATION.url}/api/v1/carts`)).rejects.toBe('The station did not answer.');
    const res = await api.netFetch(`${STATION.url}/api/v1/schedule/x`, { method: 'DELETE' });
    expect(res.status).toBe(204);
    expect(res.body).toBeNull();
  });

  it('a 401 from the station tells the sign-in watch once, with the station’s sentence; the chat’s do not', async () => {
    answer = (cmd) => {
      if (cmd === 'station_info') return STATION;
      if (cmd === 'station_fetch') {
        const ch = channels.at(-1)!;
        setTimeout(() => {
          ch.onmessage(new TextEncoder().encode('{"error":"Your sign-in has ended. Sign in again."}').buffer);
          ch.onmessage({ end: true });
        });
        return { id: 10, status: 401, status_text: 'Unauthorized', content_type: 'application/json' };
      }
      return null;
    };
    const api = await import('./lib/api.ts');
    await api.loadStation();
    const told: string[] = [];
    api.setUnauthorizedHandler((why) => {
      told.push(why);
      return true;
    });
    await expect(api.whoAmI()).rejects.toMatchObject({ status: 401, message: 'Your sign-in has ended. Sign in again.' });
    await expect(api.desk({ op: 'dump' })).rejects.toMatchObject({ status: 401 });
    expect(told).toEqual(['Your sign-in has ended. Sign in again.', 'Your sign-in has ended. Sign in again.']);
    const res = await api.netFetch(`${STATION.url}/api/v1/chat/info`);
    expect(res.status).toBe(401);
    expect(told).toHaveLength(2);
  });

  it('an aborted stream asks Rust to stop reading it', async () => {
    answer = (cmd) => (cmd === 'station_info' ? STATION : cmd === 'station_fetch' ? { id: 9, status: 200, status_text: 'OK', content_type: 'text/event-stream' } : null);
    const api = await import('./lib/api.ts');
    await api.loadStation();
    const ctrl = new AbortController();
    const res = await api.netFetch(`${STATION.url}/api/v1/studio/events`, { signal: ctrl.signal });
    const reading = res.body!.getReader().read();
    ctrl.abort();
    await expect(reading).rejects.toThrow('stopped');
    expect(calls.some((c) => c.cmd === 'station_stop' && (c.args as { id: number }).id === 9)).toBe(true);
  });
});
