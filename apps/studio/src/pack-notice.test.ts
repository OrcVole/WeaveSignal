// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// "New sounds are available" (1.3.1): how many of the default pack's sounds a station lacks,
// who is told, and that a dismissal is kept per pack and asks again for another. Sample people
// are from the Tartan National Names list (ADR-0071).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Cart, Catalogue } from '../../../packages/ui/carts.ts';
import type { PackView } from './lib/cartEdit.ts';
import { PACK_NOTICE, mayLoadPack, packFingerprint, packNewSounds } from './lib/packNotice.ts';

const cart = (id: string, bank: number, slot: number, label: string, origin: string): Cart => ({
  id, bank, slot, label, detail: '', mode: 'one_shot', file: `${id}.opus`, duration_ms: 2000, gain_db: 0, loudness_lufs: null, true_peak_dbtp: null,
  ramp_ms: null, origin, licence: '', missing: false, show: null,
});
const catalogue = (carts: Cart[]): Catalogue => ({ version: 1, banks: [], carts, loudness_lufs: -16 });
const pack = (over: Partial<PackView> = {}): PackView => ({
  available: true,
  loading: false,
  banks: [
    { number: 1, name: 'Stings', carts: [{ slot: 1, label: 'Fanfare', mode: 'one_shot' }, { slot: 2, label: 'Kettle whistle', mode: 'one_shot' }, { slot: 3, label: 'Bagpipe sting', mode: 'one_shot' }] },
    { number: 2, name: 'Beds', carts: [{ slot: 1, label: 'Calm bed', mode: 'bed' }] },
  ],
  untouched: [],
  ...over,
});

describe('how many new sounds', () => {
  it('counts every sound on a station with no carts', () => {
    expect(packNewSounds(pack(), catalogue([]))).toBe(4);
  });

  it('does not count a sound the station already has by name from the pack', () => {
    const cat = catalogue([cart('c1', 1, 1, 'Fanfare', 'demo:fanfare'), cart('c2', 1, 2, 'KETTLE WHISTLE ', 'demo:kettle')]);
    expect(packNewSounds(pack(), cat)).toBe(2);
  });

  it('leaves out a sound whose key holds a cart of the person’s own, which a load keeps', () => {
    const cat = catalogue([cart('mine', 1, 3, 'Fiona’s jingle', 'upload')]);
    expect(packNewSounds(pack(), cat)).toBe(3);
  });

  it('counts a sound whose key holds a default nobody changed: the load puts the pack’s in its place', () => {
    const cat = catalogue([cart('old', 1, 3, 'Old sting', 'demo:old-sting')]);
    expect(packNewSounds(pack({ untouched: ['old'] }), cat)).toBe(4);
  });

  it('counts only the station’s banks, not a show’s, and only banks 1 to 5', () => {
    const own = { ...cart('s1', 1, 1, 'Show jingle', 'upload'), show: 'the-morning-kettle' };
    expect(packNewSounds(pack(), catalogue([own]))).toBe(4);
    const odd = pack({ banks: [{ number: 9, name: 'Extra', carts: [{ slot: 1, label: 'X', mode: 'one_shot' }] }] });
    expect(packNewSounds(odd, catalogue([]))).toBe(0);
  });

  it('is nought when the station cannot say: no pack, or no catalogue', () => {
    expect(packNewSounds(pack({ available: false }), catalogue([]))).toBe(0);
    expect(packNewSounds(pack(), null)).toBe(0);
  });
});

describe('the pack’s fingerprint', () => {
  it('is the same for the same pack and changes with a sound, a key or a name', () => {
    const a = packFingerprint(pack());
    expect(packFingerprint(pack())).toBe(a);
    const moved = pack();
    moved.banks[0].carts[0].slot = 7;
    expect(packFingerprint(moved)).not.toBe(a);
    const renamed = pack();
    renamed.banks[1].name = 'Under beds';
    expect(packFingerprint(renamed)).not.toBe(a);
  });
});

describe('the words and who is told', () => {
  it('says how many, with the singular right, and where to go', () => {
    expect(PACK_NOTICE.lead(50)).toBe('50 new sounds are available:');
    expect(PACK_NOTICE.lead(1)).toBe('1 new sound is available:');
    expect(PACK_NOTICE.action).toBe('Default pack…');
    expect(PACK_NOTICE.rest).toContain('Carts');
  });

  it('is for an admin, a producer or the station key; not a presenter or a screener', () => {
    expect(mayLoadPack({ kind: 'station', role: 'admin' })).toBe(true);
    expect(mayLoadPack({ kind: 'staff', role: 'admin' })).toBe(true);
    expect(mayLoadPack({ kind: 'staff', role: 'producer' })).toBe(true);
    expect(mayLoadPack({ kind: 'staff', role: 'presenter' })).toBe(false);
    expect(mayLoadPack({ kind: 'staff', role: 'screener' })).toBe(false);
    expect(mayLoadPack(null)).toBe(false);
  });
});

describe('the notice', () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    vi.resetModules();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.doUnmock('./lib/cartEdit.ts');
    vi.doUnmock('./lib/api.ts');
  });

  /** The notice's store, with the station answering as given. */
  async function station(view: PackView, cat: Catalogue, me: { kind: 'staff' | 'station'; role: 'admin' | 'producer' | 'presenter' }) {
    vi.resetModules(); // each station is a fresh page
    vi.doMock('./lib/cartEdit.ts', async (orig) => ({ ...(await orig<typeof import('./lib/cartEdit.ts')>()), packView: vi.fn(async () => view) }));
    vi.doMock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof import('./lib/api.ts')>()), catalogue: vi.fn(async () => cat) }));
    const { account } = await import('./lib/account.svelte.ts');
    account.me = me;
    return import('./lib/packNotice.svelte.ts');
  }

  it('is shown to a producer once, and a dismissal is kept for this pack', async () => {
    const m = await station(pack(), catalogue([]), { kind: 'staff', role: 'producer' });
    await m.checkPackNotice();
    expect(m.packNotice.shown).toBe(true);
    expect(m.packNotice.count).toBe(4);
    m.dismissPackNotice();
    expect(m.packNotice.shown).toBe(false);
    // The next sign-in, with the same pack: still quiet.
    m.packNotice.asked = false;
    await m.checkPackNotice();
    expect(m.packNotice.shown).toBe(false);
  });

  it('asks again when the pack is a different one', async () => {
    const m = await station(pack(), catalogue([]), { kind: 'staff', role: 'admin' });
    await m.checkPackNotice();
    m.dismissPackNotice();
    const other = pack();
    other.banks[0].carts.push({ slot: 4, label: 'Ceilidh sting', mode: 'one_shot' });
    const m2 = await station(other, catalogue([]), { kind: 'staff', role: 'admin' });
    await m2.checkPackNotice();
    expect(m2.packNotice.shown).toBe(true);
  });

  it('waits while the pack is loading, and decides when it has loaded', async () => {
    const m = await station(pack({ loading: true }), catalogue([]), { kind: 'staff', role: 'admin' });
    await m.checkPackNotice();
    // Some of its sounds are on the carts and the rest are on their way: nothing is decided yet.
    expect(m.packNotice.asked).toBe(false);
    expect(m.packNotice.shown).toBe(false);
  });

  it('is not asked of a presenter, and is quiet when nothing is new', async () => {
    const p = await station(pack(), catalogue([]), { kind: 'staff', role: 'presenter' });
    await p.checkPackNotice();
    expect(p.packNotice.asked).toBe(false);
    expect(p.packNotice.shown).toBe(false);
    const full = catalogue([cart('c1', 1, 1, 'Fanfare', 'demo:a'), cart('c2', 1, 2, 'Kettle whistle', 'demo:b'), cart('c3', 1, 3, 'Bagpipe sting', 'demo:c'), cart('c4', 2, 1, 'Calm bed', 'demo:d')]);
    const q = await station(pack(), full, { kind: 'staff', role: 'admin' });
    await q.checkPackNotice();
    expect(q.packNotice.shown).toBe(false);
  });
});
