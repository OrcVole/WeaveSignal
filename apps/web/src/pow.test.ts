// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { fresh } from './lib/challenge.ts';
import { sha256Hex, solve } from './lib/pow.ts';

describe('the call-in challenge', () => {
  it('hashes as SHA-256 does', () => {
    expect(sha256Hex('abc')).toBe('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
    expect(sha256Hex('')).toBe('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
  });

  it('finds the nonce the control plane expects (crates/control/src/challenge.rs)', async () => {
    const r = await solve('0123456789abcdef0123456789abcdef', 12);
    expect(r?.nonce).toBe('6092');
    expect(r?.tried).toBe(6093);
  });

  it('can be stopped between batches', async () => {
    const r = await solve('0123456789abcdef0123456789abcdef', 40, { batch: 1000, onBatch: (p) => p.tried < 3000 });
    expect(r).toBeNull();
  });

  it('sends only an answer with time left on it', () => {
    const s = { answer: { token: 't', nonce: '1' }, expires_at_ms: 200_000, bits: 1, tried: 1, ms: 1 };
    expect(fresh(s, 100_000)).toBe(true);
    expect(fresh(s, 150_000)).toBe(false);
    expect(fresh(null)).toBe(false);
  });
});
