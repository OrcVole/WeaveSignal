// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { shouldReload } from './lib/stale.ts';

describe('a page whose code files went with an update', () => {
  it('reloads the first time', () => {
    expect(shouldReload(1_000_000, null)).toBe(true);
    expect(shouldReload(1_000_000, 'not a time')).toBe(true);
  });
  it('does not reload again within a minute', () => {
    expect(shouldReload(1_000_000, String(1_000_000 - 59_000))).toBe(false);
  });
  it('reloads again after a minute, or when the clock went back', () => {
    expect(shouldReload(1_000_000, String(1_000_000 - 60_000))).toBe(true);
    expect(shouldReload(1_000_000, String(2_000_000))).toBe(true);
  });
});
