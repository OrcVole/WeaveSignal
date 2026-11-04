// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Call-in's challenge, the page's half (crates/control/src/challenge.rs): find a nonce, in
// decimal digits, such that SHA-256(salt ":" nonce) starts with `bits` zero bits. It runs in a
// Web Worker (pow.worker.ts) while the caller types, so nobody waits for it or sees it.
//
// A plain SHA-256 of one 64-byte block: the salt (32 hex digits), the colon and a nonce of up
// to 20 digits always fit in one block, so each try is one compression. This is several times
// faster than crypto.subtle.digest, whose promise per hash dominates at this size.

const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);
const H0 = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

const W = new Uint32Array(64);
const OUT = new Uint32Array(8);

/** SHA-256 of a message of at most 55 bytes, as eight big-endian words in OUT. */
function sha256Block(msg: Uint8Array, len: number): Uint32Array {
  W.fill(0, 0, 16);
  for (let i = 0; i < len; i++) W[i >> 2] |= msg[i] << (24 - 8 * (i & 3));
  W[len >> 2] |= 0x80 << (24 - 8 * (len & 3));
  W[15] = len * 8;
  for (let i = 16; i < 64; i++) {
    const a = W[i - 15],
      b = W[i - 2];
    const s0 = ((a >>> 7) | (a << 25)) ^ ((a >>> 18) | (a << 14)) ^ (a >>> 3);
    const s1 = ((b >>> 17) | (b << 15)) ^ ((b >>> 19) | (b << 13)) ^ (b >>> 10);
    W[i] = (W[i - 16] + s0 + W[i - 7] + s1) | 0;
  }
  let [a, b, c, d, e, f, g, h] = H0;
  for (let i = 0; i < 64; i++) {
    const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
    const t1 = (h + S1 + ((e & f) ^ (~e & g)) + K[i] + W[i]) | 0;
    const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
    const t2 = (S0 + ((a & b) ^ (a & c) ^ (b & c))) | 0;
    h = g;
    g = f;
    f = e;
    e = (d + t1) | 0;
    d = c;
    c = b;
    b = a;
    a = (t1 + t2) | 0;
  }
  OUT[0] = H0[0] + a;
  OUT[1] = H0[1] + b;
  OUT[2] = H0[2] + c;
  OUT[3] = H0[3] + d;
  OUT[4] = H0[4] + e;
  OUT[5] = H0[5] + f;
  OUT[6] = H0[6] + g;
  OUT[7] = H0[7] + h;
  return OUT;
}

/** SHA-256 of a short ASCII string, as hex (for tests). */
export function sha256Hex(text: string): string {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > 55) throw new Error('one block only');
  return [...sha256Block(bytes, bytes.length)].map((w) => w.toString(16).padStart(8, '0')).join('');
}

/** How many zero bits the digest starts with. */
function leadingZeroBits(h: Uint32Array): number {
  let n = 0;
  for (const w of h) {
    if (w === 0) n += 32;
    else return n + Math.clz32(w);
  }
  return n;
}

export interface Progress {
  tried: number;
  ms: number;
}

/**
 * Find the first nonce from `start` that meets `bits`, trying `batch` at a time and yielding
 * between batches (`onBatch` may stop the search by returning false). Resolves to the nonce
 * and how many were tried, or null if stopped.
 */
export async function solve(
  salt: string,
  bits: number,
  { start = 0, batch = 50_000, onBatch }: { start?: number; batch?: number; onBatch?: (p: Progress) => boolean | void } = {},
): Promise<{ nonce: string; tried: number; ms: number } | null> {
  const prefix = new TextEncoder().encode(`${salt}:`);
  if (prefix.length + 20 > 55) throw new Error('the salt is too long for one block');
  const msg = new Uint8Array(64);
  msg.set(prefix);
  const t0 = performance.now();
  let n = start;
  for (;;) {
    const end = n + batch;
    for (; n < end; n++) {
      // The nonce's digits after the prefix.
      let len = prefix.length;
      const digits = String(n);
      for (let i = 0; i < digits.length; i++) msg[len++] = digits.charCodeAt(i);
      if (leadingZeroBits(sha256Block(msg, len)) >= bits) return { nonce: digits, tried: n - start + 1, ms: performance.now() - t0 };
    }
    if (onBatch?.({ tried: n - start, ms: performance.now() - t0 }) === false) return null;
    await new Promise((r) => setTimeout(r, 0));
  }
}
