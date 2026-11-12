// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rundown's times are on the station's clock, not this machine's (issue 10).
import { describe, expect, it } from 'vitest';
import { atToday } from './lib/format.ts';

describe("a rundown row's time", () => {
  const now = Date.UTC(2026, 10, 10, 14, 0); // 14:00 in London, 09:00 in New York

  it("is read in the station's zone", () => {
    expect(atToday('10:07', now, 'America/New_York')).toBe(Date.UTC(2026, 10, 10, 15, 7));
    expect(atToday('10:07:30', now, 'Europe/London')).toBe(Date.UTC(2026, 10, 10, 10, 7, 30));
  });

  it("falls back to this machine's clock with no zone or an unknown one", () => {
    const local = atToday('10:07', now);
    expect(atToday('10:07', now, 'Not/AZone')).toBe(local);
  });
});
