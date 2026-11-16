// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The cart bank a window last showed comes back when the app starts (it always opened on bank
// 1: the store read the bank before the key's name was defined).
import { afterEach, describe, expect, it, vi } from 'vitest';

describe('the cart bank at start', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it('opens on the bank kept on this machine', async () => {
    const kept = new Map([['weavesignal.cart-bank', '3']]);
    vi.stubGlobal('localStorage', { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => void kept.set(k, v) });
    vi.resetModules();
    const { studio, setBank } = await import('./lib/studio.svelte.ts');
    expect(studio.bank).toBe(3);
    setBank(5);
    expect(kept.get('weavesignal.cart-bank')).toBe('5');
  });

  it('opens on bank 1 when nothing, or nonsense, is kept', async () => {
    vi.stubGlobal('localStorage', { getItem: () => '9', setItem: () => {} });
    vi.resetModules();
    const { studio } = await import('./lib/studio.svelte.ts');
    expect(studio.bank).toBe(1);
  });
});
