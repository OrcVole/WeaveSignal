// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The one path by which the window leaves a station (ADR-0285;
// docs/reference/design/station-switcher.md, section 8), in the packaged app, with Tauri's core
// replaced by a fake that answers as main.rs's commands do. The order is the guarantee: held
// keys are let go, the links are closed, the page is sealed, the stores are emptied, the Rust
// side is told, and only then does the page load again.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StationList, StationRow, View } from './lib/api.ts';

const calls: { cmd: string; args: Record<string, unknown>; opts?: { headers: Record<string, string> } }[] = [];
const order: string[] = [];
let answer: (cmd: string, args: Record<string, unknown>) => unknown = () => null;

class FakeChannel {
  static n = 0;
  id = ++FakeChannel.n;
  onmessage: (m: unknown) => void = () => {};
}

vi.mock('@tauri-apps/api/core', () => ({
  Channel: FakeChannel,
  invoke: async (cmd: string, args: Record<string, unknown>, opts?: { headers: Record<string, string> }) => {
    calls.push({ cmd, args, opts });
    if (cmd.startsWith('stations_')) order.push(`rust: ${cmd}`);
    return answer(cmd, args);
  },
}));

const row = (id: string, o: Partial<StationRow>): StationRow => ({ id, url: `https://${id}.example`, label: null, name: null, who: null, version: '1.0.5', kept: 'keychain', ...o });
const KETTLE = row('s1', { url: 'https://kettle.example', name: 'Kettle FM' });
const ROSS = row('s2', { url: 'https://radio-ross.example', name: 'Radio Ross' });
const list = (current: string | null, stations = [KETTLE, ROSS]): StationList => ({ app_version: '1.0.5', current, stations });
const HERE = { id: 's1', url: KETTLE.url, kept: 'keychain', current: true };

const onAir = { station: 'Kettle FM', desk: { reachable: true, error: null, playout: { connected: true, for_ms: null } }, sources: [{ on_air: true, mute: false }], callers: [], notices: [] } as unknown as View;
const quiet = { ...onAir, sources: [{ on_air: false, mute: false }] } as unknown as View;

const session = new Map<string, string>();
const local = new Map<string, string>();
const storage = (m: Map<string, string>) => ({
  getItem: (k: string) => m.get(k) ?? null,
  setItem: (k: string, v: string) => void m.set(k, v),
  removeItem: (k: string) => void m.delete(k),
});

async function load() {
  const api = await import('./lib/api.ts');
  const sw = await import('./lib/stations.svelte.ts');
  const reset = await import('./lib/reset.ts');
  const { studio } = await import('./lib/studio.svelte.ts');
  const { account } = await import('./lib/account.svelte.ts');
  await api.loadStation();
  await sw.loadStations();
  sw.setReload(() => order.push('reload'));
  sw.onBeforeLeave(async () => {
    order.push('held keys let go');
  });
  reset.onLeave('studio events', () => order.push('event stream closed'));
  reset.onStationChange('probe', () => order.push('stores emptied'));
  return { api, sw, reset, studio, account };
}

beforeEach(() => {
  calls.length = 0;
  order.length = 0;
  session.clear();
  local.clear();
  answer = (cmd) => (cmd === 'station_info' ? HERE : cmd.startsWith('stations_') ? list(cmd === 'stations_switch' ? 's2' : 's1') : null);
  vi.stubGlobal('location', { protocol: 'tauri:', hostname: 'localhost', search: '', reload: () => order.push('reload') });
  vi.stubGlobal('sessionStorage', storage(session));
  vi.stubGlobal('localStorage', storage(local));
  vi.resetModules();
});
afterEach(() => vi.unstubAllGlobals());

describe('switching to another station', () => {
  it('lets go, closes the links, empties the stores, tells the Rust side, then loads the page again, in that order', async () => {
    const { sw, studio } = await load();
    studio.view = quiet;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    expect(order).toEqual(['rust: stations_list', 'held keys let go', 'event stream closed', 'stores emptied', 'rust: stations_switch', 'reload']);
    expect(calls.find((c) => c.cmd === 'stations_switch')?.args).toEqual({ id: 's2' });
    // The window's own stores went with it.
    expect(studio.view).toBeNull();
    // The next page is told where the window came from, for its note.
    expect(JSON.parse(session.get('weavesignal.arrived') ?? 'null')).toEqual({ from: 'Kettle FM', leftLive: false });
  });

  it('seals the page once the links are closed: nothing more is asked of any station until it loads again', async () => {
    const { api, sw, studio } = await load();
    studio.view = quiet;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    const before = calls.length;
    await expect(api.desk({ op: 'dump' })).rejects.toThrow('This window is changing station.');
    await expect(api.whoAmI()).rejects.toThrow('This window is changing station.');
    expect(calls.length).toBe(before);
    expect(calls.some((c) => c.cmd === 'station_fetch')).toBe(false);
  });

  it('names its station on every request, so the Rust side can refuse one left over from before a switch', async () => {
    const { api } = await load();
    answer = (cmd) => (cmd === 'station_fetch' ? { id: 1, status: 204, status_text: 'No Content', content_type: null } : null);
    await api.netFetch(`${api.base()}/api/v1/studio`, {});
    expect(calls.find((c) => c.cmd === 'station_fetch')?.opts?.headers['x-station-id']).toBe('s1');
    expect(calls.find((c) => c.cmd === 'station_fetch')?.opts?.headers['x-station-path']).toBe('/api/v1/studio');
  });

  it('asks first while the station is on air, and does nothing until the answer', async () => {
    const { sw, studio } = await load();
    studio.view = onAir;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    expect(sw.stations.ask?.words.heading).toBe('Leave Kettle FM while it is on air?');
    expect(sw.stations.open).toBe(true);
    expect(order).toEqual(['rust: stations_list']);
    expect(studio.view).toBe(onAir);
    // Stay: nothing happened.
    sw.stations.ask = null;
    expect(order).toEqual(['rust: stations_list']);
    // Switch: the same path, and the note will say the station carries on.
    await sw.leaveNow({ kind: 'switch', id: 's2' }, true);
    expect(order.slice(1)).toEqual(['held keys let go', 'event stream closed', 'stores emptied', 'rust: stations_switch', 'reload']);
    expect(JSON.parse(session.get('weavesignal.arrived') ?? 'null')).toEqual({ from: 'Kettle FM', leftLive: true });
  });

  it('does not ask a Preparing window, which changes nothing on air', async () => {
    const { sw, studio, account } = await load();
    studio.view = onAir;
    studio.link = 'live';
    Object.assign(account, { loaded: true, me: { kind: 'staff', role: 'presenter', preparing: true, account: { id: 'a1', name: 'Eilidh Fraser-MacLeod' } } });
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    expect(sw.stations.ask).toBeNull();
    expect(order.at(-1)).toBe('reload');
  });

  it('does nothing for the station the window is already on, or while a switch is on its way', async () => {
    const { sw, studio } = await load();
    studio.view = quiet;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'switch', id: 's1' });
    expect(order).toEqual(['rust: stations_list']);
    const first = sw.requestLeave({ kind: 'switch', id: 's2' });
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    await first;
    expect(order.filter((o) => o === 'rust: stations_switch')).toHaveLength(1);
    expect(order.filter((o) => o === 'reload')).toHaveLength(1);
  });

  it('still loads the page again when the Rust side refuses, so the window comes up wherever it really is', async () => {
    const { sw, studio } = await load();
    studio.view = quiet;
    studio.link = 'live';
    answer = (cmd) => {
      if (cmd === 'stations_switch') throw 'That station is not in the list.';
      return null;
    };
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    expect(order.at(-1)).toBe('reload');
  });

  it('forgets that this window settled Live or Preparing, so the next station settles its own', async () => {
    const { api, sw, studio } = await load();
    api.settlePrepare('account-at-kettle');
    studio.view = quiet;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'switch', id: 's2' });
    expect(api.prepareSettledFor()).toBeNull();
  });
});

describe('signing out of, and removing, a station', () => {
  it('changes another station at once, without leaving the one the window is on', async () => {
    const { sw, studio } = await load();
    studio.view = onAir;
    studio.link = 'live';
    answer = (cmd) => (cmd === 'stations_remove' ? list('s1', [KETTLE]) : list('s1', [KETTLE, { ...ROSS, kept: null }]));
    await sw.requestLeave({ kind: 'sign-out', id: 's2' });
    expect(sw.stations.list?.stations[1].kept).toBeNull();
    await sw.requestLeave({ kind: 'remove', id: 's2' });
    expect(sw.stations.list?.stations.map((s) => s.id)).toEqual(['s1']);
    // The window never left Kettle FM: nothing was let go, closed, emptied or loaded again.
    expect(order).toEqual(['rust: stations_list', 'rust: stations_sign_out', 'rust: stations_remove']);
    expect(studio.view).toBe(onAir);
    expect(sw.stations.ask).toBeNull();
  });

  it('treats the window’s own station as leaving it: asked while on air, then the same path', async () => {
    const { sw, studio } = await load();
    studio.view = onAir;
    studio.link = 'live';
    await sw.requestLeave({ kind: 'remove', id: 's1' });
    expect(sw.stations.ask?.leave).toEqual({ kind: 'remove', id: 's1' });
    await sw.leaveNow({ kind: 'remove', id: 's1' }, true);
    expect(order.slice(1)).toEqual(['held keys let go', 'event stream closed', 'stores emptied', 'rust: stations_remove', 'reload']);
    // No arrival note: the window is on no station now.
    expect(session.has('weavesignal.arrived')).toBe(false);
  });

  it('says a refusal in the dialog and leaves the list as it was', async () => {
    const { sw } = await load();
    answer = (cmd) => {
      if (cmd === 'stations_rename') throw 'A name here is at most 60 characters.';
      return list('s1');
    };
    expect(await sw.rename('s2', 'x'.repeat(61))).toBe(false);
    expect(sw.stations.problem).toBe('A name here is at most 60 characters.');
    expect(sw.stations.list?.stations).toHaveLength(2);
  });
});

describe('adding a station', () => {
  it('keeps the page on its own station when the new one only joined the list, then switches by the same path', async () => {
    const { api, sw, studio } = await load();
    studio.view = quiet;
    studio.link = 'live';
    answer = (cmd) => {
      if (cmd === 'station_sign_in') return { station: { id: 's2', url: ROSS.url, kept: 'keychain', current: false }, account: { id: 'a2' }, expires_ms: 0 };
      if (cmd === 'stations_switch') return list('s2');
      return list('s1');
    };
    sw.stations.adding = true;
    const r = await api.signIn(ROSS.url, 'eilidh', 'a long password');
    // The page still names Kettle FM on its requests until the switch has run.
    expect(api.savedStation()?.id).toBe('s1');
    await sw.afterSignIn(r.station);
    expect(sw.stations.adding).toBe(false);
    expect(order.slice(-5)).toEqual(['held keys let go', 'event stream closed', 'stores emptied', 'rust: stations_switch', 'reload']);
  });

  it('asks before it switches when the station underneath is on air, and Stay leaves the new one in the list', async () => {
    const { sw, studio } = await load();
    studio.view = onAir;
    studio.link = 'live';
    sw.stations.adding = true;
    await sw.afterSignIn({ id: 's2', url: ROSS.url, kept: 'keychain', current: false });
    expect(sw.stations.ask?.leave).toEqual({ kind: 'switch', id: 's2' });
    expect(order).not.toContain('reload');
    expect(sw.stations.list?.stations.map((s) => s.id)).toEqual(['s1', 's2']);
  });

  it('only loads the page again when the station took the window (the first one, or its own signed in again)', async () => {
    const { sw } = await load();
    await sw.afterSignIn({ id: 's1', url: KETTLE.url, kept: 'keychain', current: true });
    expect(order).toEqual(['rust: stations_list', 'reload']);
  });
});

describe('arriving at a station', () => {
  it('tells the list what the window sees here, and says once where the window is', async () => {
    session.set('weavesignal.arrived', JSON.stringify({ from: 'Kettle FM', leftLive: true }));
    answer = (cmd) => (cmd === 'station_info' ? { id: 's2', url: ROSS.url, kept: 'keychain', current: true } : list('s2'));
    const { sw, studio, account } = await load();
    studio.view = { ...quiet, station: 'Radio Ross' } as View;
    Object.assign(account, { loaded: true, me: { kind: 'staff', role: 'producer', account: { id: 'a2', name: 'Eilidh Fraser-MacLeod' } } });
    await sw.settleArrival('1.0.3');
    expect(calls.find((c) => c.cmd === 'stations_note')?.args).toEqual({ id: 's2', name: 'Radio Ross', who: 'Eilidh Fraser-MacLeod, producer', version: '1.0.3' });
    expect(studio.note?.title).toBe('You are on Radio Ross');
    expect(studio.note?.message).toBe(
      'Signed in as Eilidh Fraser-MacLeod, producer. Kettle FM carries on without this window. It runs 1.0.3, older than this app (1.0.5): what it does not have yet is hidden or switched off here.',
    );
    // Once: a reload of the same station says nothing.
    expect(session.has('weavesignal.arrived')).toBe(false);
    studio.note = null;
    await sw.settleArrival('1.0.3');
    expect(studio.note).toBeNull();
  });
});

describe('a preference kept for each station apart', () => {
  it('reads and writes under the station’s own key, and the first station finds what an older build kept', async () => {
    const { api } = await load();
    const scoped = await import('./lib/scoped.ts');
    local.set('weavesignal.cart-bank', '3');
    expect(api.savedStation()?.id).toBe('s1');
    expect(scoped.stationKey('weavesignal.cart-bank')).toBe('weavesignal.cart-bank@s1');
    // The station an install carried over as: what the older build kept is found.
    expect(scoped.readScoped('weavesignal.cart-bank')).toBe('3');
    scoped.writeScoped('weavesignal.cart-bank', '5');
    expect(local.get('weavesignal.cart-bank@s1')).toBe('5');
    expect(scoped.readScoped('weavesignal.cart-bank')).toBe('5');
  });

  it('shows no other station the first one’s bank', async () => {
    answer = (cmd) => (cmd === 'station_info' ? { id: 's2', url: ROSS.url, kept: 'keychain', current: true } : list('s2'));
    local.set('weavesignal.cart-bank', '3');
    local.set('weavesignal.cart-bank@s1', '5');
    const { studio } = await load();
    const scoped = await import('./lib/scoped.ts');
    const { settleBank } = await import('./lib/studio.svelte.ts');
    expect(scoped.readScoped('weavesignal.cart-bank')).toBeNull();
    settleBank();
    expect(studio.bank).toBe(1);
  });
});

describe('⌃⇧S opens the Stations list (ADR-0287)', () => {
  it('is a chord on every tab, never from a text field, and never switches by itself', async () => {
    const { KeyEngine } = await import('../../../packages/ui/keymap.ts');
    const key = { code: 'KeyS', ctrl: true, shift: true, alt: false, meta: false, repeat: false };
    for (const tab of ['desk', 'screener', 'carts', 'chat', 'schedule'] as const) {
      expect(new KeyEngine().down(key, { tab, inTextField: false }, 0).action).toEqual({ type: 'stations' });
    }
    const field = new KeyEngine().down(key, { tab: 'desk', inTextField: true }, 0);
    expect([field.handled, field.action]).toEqual([true, null]);
    // ⌃S alone and a bare S are not it (on the Desk, S is a talk key).
    expect(new KeyEngine().down({ ...key, shift: false }, { tab: 'desk', inTextField: false }, 0).action).toBeNull();
    expect(new KeyEngine().down({ ...key, ctrl: false, shift: false }, { tab: 'desk', inTextField: false }, 0).action?.type).not.toBe('stations');
  });
});
