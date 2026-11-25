// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A QR code for a short text, such as a key's link (keyed shows, 1.3.2): byte mode, error
// correction level M, versions 1 to 10 (up to 213 bytes), written from the standard
// (ISO/IEC 18004) so that the pages carry no dependency for it. The result is a square of
// modules, true for dark; `qrPath` draws it as one SVG path with the quiet zone round it.
//
// What proves it: qr.test.ts checks the fixed parts against the standard's own figures, and
// tests/keys/qr-decode.py reads the drawn codes back with an independent decoder.

/** Level M, versions 1 to 10: error-correction codewords a block, then the blocks as
 *  [how many, data codewords each] pairs. */
const BLOCKS: readonly { ec: number; groups: readonly (readonly [number, number])[] }[] = [
  { ec: 10, groups: [[1, 16]] },
  { ec: 16, groups: [[1, 28]] },
  { ec: 26, groups: [[1, 44]] },
  { ec: 18, groups: [[2, 32]] },
  { ec: 24, groups: [[2, 43]] },
  { ec: 16, groups: [[4, 27]] },
  { ec: 18, groups: [[4, 31]] },
  { ec: 22, groups: [[2, 38], [2, 39]] },
  { ec: 22, groups: [[3, 36], [2, 37]] },
  { ec: 26, groups: [[4, 43], [1, 44]] },
];
/** Where the alignment patterns' centres are, by version. */
const ALIGN: readonly (readonly number[])[] = [[], [6, 18], [6, 22], [6, 26], [6, 30], [6, 34], [6, 22, 38], [6, 24, 42], [6, 26, 46], [6, 28, 50]];

export const QR_MAX_BYTES = 213;

/** How many data codewords a version holds at level M. */
function dataCodewords(version: number): number {
  return BLOCKS[version - 1].groups.reduce((n, [blocks, each]) => n + blocks * each, 0);
}

/** How many bytes of text a version holds: its data codewords less the mode, the count and
 *  nothing for the terminator (which may be cut short). */
export function qrCapacity(version: number): number {
  return Math.floor((dataCodewords(version) * 8 - 4 - (version < 10 ? 8 : 16)) / 8);
}

// ---- Reed-Solomon over GF(256), polynomial x^8 + x^4 + x^3 + x^2 + 1 ----

const EXP = new Uint8Array(512);
const LOG = new Uint8Array(256);
{
  let x = 1;
  for (let i = 0; i < 255; i++) {
    EXP[i] = x;
    LOG[x] = i;
    x <<= 1;
    if (x & 0x100) x ^= 0x11d;
  }
  for (let i = 255; i < 512; i++) EXP[i] = EXP[i - 255];
}
const mul = (a: number, b: number): number => (a === 0 || b === 0 ? 0 : EXP[LOG[a] + LOG[b]]);

/** The generator polynomial of `degree`, highest power first, without its leading 1. */
function generator(degree: number): Uint8Array {
  let g = [1];
  for (let i = 0; i < degree; i++) {
    const next = new Array<number>(g.length + 1).fill(0);
    for (let j = 0; j < g.length; j++) {
      next[j] ^= g[j];
      next[j + 1] ^= mul(g[j], EXP[i]);
    }
    g = next;
  }
  return Uint8Array.from(g.slice(1));
}

/** The error-correction codewords of one block. */
export function rsRemainder(data: Uint8Array, degree: number): Uint8Array {
  const gen = generator(degree);
  const rem = new Uint8Array(degree);
  for (const b of data) {
    const factor = b ^ rem[0];
    rem.copyWithin(0, 1);
    rem[degree - 1] = 0;
    for (let i = 0; i < degree; i++) rem[i] ^= mul(gen[i], factor);
  }
  return rem;
}

// ---- the codewords ----

/** The text as the final sequence of codewords: mode, count, bytes, terminator, padding, cut
 *  into blocks, each with its error correction, and interleaved. */
function codewords(bytes: Uint8Array, version: number): Uint8Array {
  const capacity = dataCodewords(version);
  const bits: number[] = [];
  const put = (value: number, length: number) => {
    for (let i = length - 1; i >= 0; i--) bits.push((value >>> i) & 1);
  };
  put(0b0100, 4);
  put(bytes.length, version < 10 ? 8 : 16);
  for (const b of bytes) put(b, 8);
  put(0, Math.min(4, capacity * 8 - bits.length));
  while (bits.length % 8) bits.push(0);
  const data = new Uint8Array(capacity);
  for (let i = 0; i < bits.length; i++) data[i >> 3] |= bits[i] << (7 - (i & 7));
  for (let i = bits.length / 8, pad = 0xec; i < capacity; i++, pad ^= 0xec ^ 0x11) data[i] = pad;

  const { ec, groups } = BLOCKS[version - 1];
  const blocks: { data: Uint8Array; ec: Uint8Array }[] = [];
  let at = 0;
  for (const [n, each] of groups) {
    for (let i = 0; i < n; i++) {
      const d = data.subarray(at, at + each);
      at += each;
      blocks.push({ data: d, ec: rsRemainder(d, ec) });
    }
  }
  const out: number[] = [];
  const longest = Math.max(...blocks.map((b) => b.data.length));
  for (let i = 0; i < longest; i++) for (const b of blocks) if (i < b.data.length) out.push(b.data[i]);
  for (let i = 0; i < ec; i++) for (const b of blocks) out.push(b.ec[i]);
  return Uint8Array.from(out);
}

// ---- the square ----

/** The 15 format bits for level M and a mask: BCH(15,5), then the standard's mask. */
export function formatBits(mask: number): number {
  const data = (0b00 << 3) | mask;
  let rem = data;
  for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  return ((data << 10) | rem) ^ 0x5412;
}

/** The 18 version bits (versions 7 and up): BCH(18,6). */
export function versionBits(version: number): number {
  let rem = version;
  for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
  return (version << 12) | rem;
}

const MASKS: readonly ((r: number, c: number) => boolean)[] = [
  (r, c) => (r + c) % 2 === 0,
  (r) => r % 2 === 0,
  (_r, c) => c % 3 === 0,
  (r, c) => (r + c) % 3 === 0,
  (r, c) => (Math.floor(r / 2) + Math.floor(c / 3)) % 2 === 0,
  (r, c) => ((r * c) % 2) + ((r * c) % 3) === 0,
  (r, c) => (((r * c) % 2) + ((r * c) % 3)) % 2 === 0,
  (r, c) => (((r + c) % 2) + ((r * c) % 3)) % 2 === 0,
];

interface Grid {
  size: number;
  dark: Uint8Array;
  /** Function modules: never masked, never data. */
  fixed: Uint8Array;
}

function blank(version: number): Grid {
  const size = 17 + 4 * version;
  const g: Grid = { size, dark: new Uint8Array(size * size), fixed: new Uint8Array(size * size) };
  const set = (r: number, c: number, on: boolean) => {
    if (r < 0 || c < 0 || r >= size || c >= size) return;
    g.dark[r * size + c] = on ? 1 : 0;
    g.fixed[r * size + c] = 1;
  };
  // The three finder patterns with their separators.
  for (const [r0, c0] of [[3, 3], [3, size - 4], [size - 4, 3]]) {
    for (let dr = -4; dr <= 4; dr++) {
      for (let dc = -4; dc <= 4; dc++) {
        const d = Math.max(Math.abs(dr), Math.abs(dc));
        set(r0 + dr, c0 + dc, d !== 2 && d !== 4);
      }
    }
  }
  // The alignment patterns, but not on the finders.
  const centres = ALIGN[version - 1];
  for (const r0 of centres) {
    for (const c0 of centres) {
      if ((r0 === 6 && c0 === 6) || (r0 === 6 && c0 === size - 7) || (r0 === size - 7 && c0 === 6)) continue;
      for (let dr = -2; dr <= 2; dr++) for (let dc = -2; dc <= 2; dc++) set(r0 + dr, c0 + dc, Math.max(Math.abs(dr), Math.abs(dc)) !== 1);
    }
  }
  // The timing patterns.
  for (let i = 8; i < size - 8; i++) {
    if (!g.fixed[6 * size + i]) set(6, i, i % 2 === 0);
    if (!g.fixed[i * size + 6]) set(i, 6, i % 2 === 0);
  }
  // The dark module, and room kept for the format bits (written per mask) and the version bits.
  set(size - 8, 8, true);
  for (let i = 0; i < 9; i++) {
    if (!g.fixed[8 * size + i]) set(8, i, false);
    if (!g.fixed[i * size + 8]) set(i, 8, false);
  }
  for (let i = 0; i < 8; i++) {
    set(8, size - 1 - i, false);
    if (i < 7) set(size - 1 - i, 8, false);
  }
  if (version >= 7) {
    const bits = versionBits(version);
    for (let i = 0; i < 18; i++) {
      const on = ((bits >>> i) & 1) === 1;
      const a = size - 11 + (i % 3);
      const b = Math.floor(i / 3);
      set(a, b, on);
      set(b, a, on);
    }
  }
  return g;
}

function writeFormat(g: Grid, mask: number): void {
  const { size } = g;
  const bits = formatBits(mask);
  const bit = (i: number) => ((bits >>> i) & 1) as 0 | 1;
  const put = (r: number, c: number, i: number) => {
    g.dark[r * size + c] = bit(i);
  };
  // Round the top-left finder: bits 0 to 7 down column 8 from the bottom of the corner up...
  for (let i = 0; i <= 5; i++) put(i, 8, i);
  put(7, 8, 6);
  put(8, 8, 7);
  put(8, 7, 8);
  for (let i = 9; i < 15; i++) put(8, 14 - i, i);
  // ...and the second copy: along row 8 at the right, and up column 8 at the bottom.
  for (let i = 0; i < 8; i++) put(8, size - 1 - i, i);
  for (let i = 8; i < 15; i++) put(size - 15 + i, 8, i);
}

/** The codewords laid in the square: two columns at a time from the right, snaking up and down,
 *  past the function modules and never in the vertical timing column. */
function place(g: Grid, words: Uint8Array): void {
  const { size } = g;
  let i = 0;
  const total = words.length * 8;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let v = 0; v < size; v++) {
      for (let j = 0; j < 2; j++) {
        const c = right - j;
        const upward = ((right + 1) & 2) === 0;
        const r = upward ? size - 1 - v : v;
        if (g.fixed[r * size + c]) continue;
        if (i < total) g.dark[r * size + c] = (words[i >> 3] >>> (7 - (i & 7))) & 1;
        i += 1;
      }
    }
  }
}

function applyMask(g: Grid, mask: number): void {
  const { size } = g;
  const m = MASKS[mask];
  for (let r = 0; r < size; r++) for (let c = 0; c < size; c++) if (!g.fixed[r * size + c] && m(r, c)) g.dark[r * size + c] ^= 1;
}

/** The standard's penalty for a masked square: runs, blocks, finder-like patterns, balance. */
function penalty(g: Grid): number {
  const { size, dark } = g;
  let score = 0;
  const at = (r: number, c: number) => dark[r * size + c];
  for (let pass = 0; pass < 2; pass++) {
    for (let a = 0; a < size; a++) {
      let run = 1;
      for (let b = 1; b < size; b++) {
        const same = pass === 0 ? at(a, b) === at(a, b - 1) : at(b, a) === at(b - 1, a);
        if (same) {
          run += 1;
          if (run === 5) score += 3;
          else if (run > 5) score += 1;
        } else run = 1;
      }
      // 1:1:3:1:1 with four light modules on either side.
      for (let b = 0; b + 10 < size; b++) {
        const v = (k: number) => (pass === 0 ? at(a, b + k) : at(b + k, a));
        const core = (o: number) => v(o) && !v(o + 1) && v(o + 2) && v(o + 3) && v(o + 4) && !v(o + 5) && v(o + 6);
        if (core(0) && !v(7) && !v(8) && !v(9) && !v(10)) score += 40;
        if (!v(0) && !v(1) && !v(2) && !v(3) && core(4)) score += 40;
      }
    }
  }
  for (let r = 0; r < size - 1; r++) {
    for (let c = 0; c < size - 1; c++) {
      const v = at(r, c);
      if (v === at(r, c + 1) && v === at(r + 1, c) && v === at(r + 1, c + 1)) score += 3;
    }
  }
  let darkCount = 0;
  for (const d of dark) darkCount += d;
  score += Math.floor(Math.abs((darkCount * 100) / (size * size) - 50) / 5) * 10;
  return score;
}

export interface Qr {
  version: number;
  mask: number;
  size: number;
  /** Row by row, true for a dark module. */
  modules: boolean[][];
}

/** The QR code for `text` (as UTF-8). Throws when it is longer than a version 10 code holds
 *  at level M: a key's link is about 50 bytes. `mask` fixes the mask, for a test. */
export function qr(text: string, mask?: number): Qr {
  const bytes = new TextEncoder().encode(text);
  let version = 1;
  while (version <= 10 && qrCapacity(version) < bytes.length) version += 1;
  if (version > 10) throw new Error(`That is too long for a QR code here (${bytes.length} bytes; ${QR_MAX_BYTES} at most).`);
  const words = codewords(bytes, version);
  let best: Grid | null = null;
  let bestMask = 0;
  let bestScore = Infinity;
  for (let m = 0; m < 8; m++) {
    if (mask !== undefined && m !== mask) continue;
    const g = blank(version);
    place(g, words);
    applyMask(g, m);
    writeFormat(g, m);
    const s = penalty(g);
    if (s < bestScore) {
      best = g;
      bestMask = m;
      bestScore = s;
    }
  }
  const g = best!;
  const modules: boolean[][] = [];
  for (let r = 0; r < g.size; r++) {
    const row: boolean[] = [];
    for (let c = 0; c < g.size; c++) row.push(g.dark[r * g.size + c] === 1);
    modules.push(row);
  }
  return { version, mask: bestMask, size: g.size, modules };
}

/** The code as one SVG path in module units, with a quiet zone of `quiet` modules (the standard
 *  asks for four): `viewBox` is `0 0 box box`. Draw it dark on a light ground, in every theme:
 *  a reversed code is not read by every scanner. */
export function qrPath(code: Qr, quiet = 4): { d: string; box: number } {
  let d = '';
  for (let r = 0; r < code.size; r++) {
    let c = 0;
    while (c < code.size) {
      if (!code.modules[r][c]) {
        c += 1;
        continue;
      }
      let run = 1;
      while (c + run < code.size && code.modules[r][c + run]) run += 1;
      d += `M${c + quiet} ${r + quiet}h${run}v1h-${run}z`;
      c += run;
    }
  }
  return { d, box: code.size + 2 * quiet };
}

/** The code as a whole SVG file, dark on white, for saving or printing. */
export function qrSvg(code: Qr, pixels = 512): string {
  const { d, box } = qrPath(code);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${box} ${box}" width="${pixels}" height="${pixels}" shape-rendering="crispEdges"><rect width="${box}" height="${box}" fill="#fff"/><path d="${d}" fill="#000"/></svg>\n`;
}
