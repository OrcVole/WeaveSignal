// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The challenge's search, off the page's main thread (pow.ts). One message in, one out.
import { solve } from './pow.ts';

self.onmessage = async (e: MessageEvent<{ salt: string; bits: number }>) => {
  const r = await solve(e.data.salt, e.data.bits, { batch: 200_000 });
  self.postMessage(r);
};
