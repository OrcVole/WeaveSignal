// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A call's pass kept on this device, so a reload or a closed tab can rejoin while the place is
// held (ADR-0101). Browser storage can be refused (private windows, blocked site data), so
// every read and write is guarded and the page works without it.

import { PLACE_HELD_MS } from './rules.ts';

const KEY = 'weavesignal.call';

export interface Kept {
  pass: string;
  identity: string;
  name: string;
  /** The last time this page was on the call. */
  seen_ms: number;
  pass_expires_at_ms: number;
}

export function keep(k: Kept): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(k));
  } catch {
    // Refused: a reload starts afresh.
  }
}

export function forget(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Nothing kept.
  }
}

/** The kept call if its place may still be held at `now`. */
export function kept(now: number, raw?: string | null): Kept | null {
  let text = raw;
  if (text === undefined) {
    try {
      text = localStorage.getItem(KEY);
    } catch {
      return null;
    }
  }
  if (!text) return null;
  try {
    const k = JSON.parse(text) as Kept;
    if (typeof k.pass !== 'string' || typeof k.seen_ms !== 'number') return null;
    if (now - k.seen_ms > PLACE_HELD_MS || now >= k.pass_expires_at_ms) return null;
    return k;
  } catch {
    return null;
  }
}
