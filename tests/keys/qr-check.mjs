// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Draws QR codes with the pages' own encoder (packages/ui/qr.ts) for texts of every version it
// makes (1 to 10), each under every mask, as plain bitmaps with the quiet zone, and writes a
// list of what each should say. tests/keys/qr-decode.py then reads them back with a decoder
// that shares no code with the encoder (OpenCV's), which is what proves the codes scan: a test
// of the encoder against itself would prove nothing.
//
//   node --experimental-strip-types tests/keys/qr-check.mjs      # into target/qr-check
//   uvx --with opencv-python-headless python tests/keys/qr-decode.py target/qr-check
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { qr, qrCapacity } from '../../packages/ui/qr.ts';

const OUT = process.env.OUT ?? join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'target', 'qr-check');
mkdirSync(OUT, { recursive: true });
const SCALE = 6;
const QUIET = 4;

/** A code as a plain PBM (P1): 1 is dark. */
function pbm(code) {
  const n = (code.size + 2 * QUIET) * SCALE;
  const rows = [];
  for (let y = 0; y < n; y++) {
    let row = '';
    for (let x = 0; x < n; x++) {
      const r = Math.floor(y / SCALE) - QUIET;
      const c = Math.floor(x / SCALE) - QUIET;
      row += r >= 0 && c >= 0 && r < code.size && c < code.size && code.modules[r][c] ? '1' : '0';
    }
    rows.push(row);
  }
  return `P1\n${n} ${n}\n${rows.join('\n')}\n`;
}

const link = 'https://kettle.example/listen/#key=KETM-7QMP-2D4X';
const filler = 'The Firth of Forth Community Broadcasting Trust keeps Friday Supporters’ Hour for people with a key. Cèilidh, 20:00. ';
const texts = [link, 'KETM-7QMP-2D4X', 'https://station.example/listen/#key=ZZZZ-9999-QQQQ'];
// One text at the very top of each version's room, so every version and block layout is drawn.
for (let v = 1; v <= 10; v++) {
  let t = '';
  while (new TextEncoder().encode(t + filler[t.length % filler.length]).length <= qrCapacity(v)) t += filler[t.length % filler.length];
  texts.push(t);
}
const list = [];
texts.forEach((text, i) => {
  // Every mask for the link, the chosen one for the rest.
  const masks = i === 0 ? [0, 1, 2, 3, 4, 5, 6, 7] : [undefined];
  for (const m of masks) {
    const code = qr(text, m);
    const file = `qr-${String(list.length).padStart(2, '0')}-v${code.version}-m${code.mask}.pbm`;
    writeFileSync(join(OUT, file), pbm(code));
    list.push({ file, text, version: code.version, mask: code.mask });
  }
});
writeFileSync(join(OUT, 'expected.json'), JSON.stringify(list, null, 1));
console.log(`${list.length} codes in ${OUT}: versions ${[...new Set(list.map((l) => l.version))].join(', ')}`);
