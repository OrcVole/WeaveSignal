// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Call-in's challenge (ADR-0006), fetched and answered while the caller fills in the lobby:
// nothing to see, tick or solve. The work runs in a Web Worker; where workers are missing it
// runs on the page in short slices instead. Its cost is kept for the page's tests.
import * as api from './api.ts';
import { solve } from './pow.ts';

export interface Solved {
  answer: api.ChallengeAnswer;
  expires_at_ms: number;
  bits: number;
  tried: number;
  ms: number;
}

function work(salt: string, bits: number): Promise<{ nonce: string; tried: number; ms: number }> {
  let worker: Worker;
  try {
    worker = new Worker(new URL('./pow.worker.ts', import.meta.url), { type: 'module' });
  } catch {
    return solve(salt, bits, { batch: 20_000 }).then((r) => r!);
  }
  return new Promise((resolve, reject) => {
    worker.onmessage = (e) => {
      worker.terminate();
      resolve(e.data);
    };
    worker.onerror = (e) => {
      worker.terminate();
      e.preventDefault();
      // A worker the page's policy or the browser refused: do the work here.
      solve(salt, bits, { batch: 20_000 }).then((r) => resolve(r!), reject);
    };
    worker.postMessage({ salt, bits });
  });
}

/** Fetch a challenge and start answering it; the promise settles when the answer is ready. */
export async function prepare(): Promise<Solved | null> {
  const c = await api.challenge();
  if (c.bits === 0 && !c.token) return null;
  const r = await work(c.salt, c.bits);
  const solved: Solved = { answer: { token: c.token, nonce: r.nonce }, expires_at_ms: c.expires_at_ms, bits: c.bits, tried: r.tried, ms: r.ms };
  (globalThis as { __weavesignalChallenge?: Solved }).__weavesignalChallenge = solved;
  return solved;
}

/** Whether a prepared answer is still worth sending (a minute's margin for clocks). */
export function fresh(s: Solved | null, now = Date.now()): boolean {
  return !!s && s.expires_at_ms - now > 60_000;
}
