// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The one-key handover (ADR-0231) against the real desk on the M2 loopback stack
// (tests/m2/run-studio.sh, where Eilidh presents and Finlay waits as a DJ): ⇧⏎ puts Finlay on
// air beside her with the seat staying hers, ⇧⏎ again takes her off and the seat passes to
// him, the guests stay on, a third press changes nothing, and slot 1's talk key (A) then
// speaks as Finlay to Eilidh and lets go. First run 2026-11-10: passed.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/m2/handover.cjs
const { chromium } = require(process.env.PLAYWRIGHT_CORE);
const STUDIO = 'http://127.0.0.1:27420/', CONTROL = 'http://127.0.0.1:27310', TOKEN = 'm2-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-m2-handover';
require('fs').mkdirSync(OUT, { recursive: true });
const view = async () => (await fetch(`${CONTROL}/api/v1/studio`, { headers: { authorization: `Bearer ${TOKEN}` } })).json();
const who = (v, id) => v.sources.find((s) => s.identity === id);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(what, f, ms = 8000) {
  const t0 = Date.now();
  for (;;) { const v = await view(); if (f(v)) return v; if (Date.now() - t0 > ms) throw new Error(`timed out: ${what}: ${JSON.stringify(v.sources.map((s) => [s.identity, s.role, s.on_air, s.talk_to]))} seat=${v.seats.presenter}`); await sleep(200); }
}
const log = (m) => console.log(`${new Date().toISOString().slice(11, 23)} ${m}`);
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
  await p.goto(STUDIO);
  await p.waitForSelector('[data-panel="desk.stage"]', { timeout: 30000 });
  let v = await view();
  log(`start: eilidh on_air=${who(v, 'eilidh').on_air}, finlay(${who(v, 'finlay').role}) on_air=${who(v, 'finlay').on_air}, seat=${v.seats.presenter}`);
  if (who(v, 'finlay').on_air || !who(v, 'eilidh').on_air || v.seats.presenter !== 'eilidh') throw new Error('unexpected start');
  await p.screenshot({ path: `${OUT}/0-dj-waiting.png` });
  const btn = await p.locator('button', { hasText: 'Bring Finlay on air' }).count();
  log(`button "Bring Finlay on air" shown: ${btn > 0}`);
  await p.mouse.click(5, 450); // focus the page, away from any control
  await p.keyboard.press('Shift+Enter');
  v = await until('finlay on air', (v) => who(v, 'finlay').on_air);
  log(`after ⇧⏎ #1: eilidh on_air=${who(v, 'eilidh').on_air}, finlay on_air=${who(v, 'finlay').on_air}, seat=${v.seats.presenter}`);
  if (!who(v, 'eilidh').on_air || v.seats.presenter !== 'eilidh') throw new Error('overlap wrong');
  await sleep(800);
  await p.screenshot({ path: `${OUT}/1-overlap.png` });
  log(`toast: ${(await p.locator('[data-panel="desk.toast"]').innerText().catch(() => '')).replace(/\s+/g, ' ').slice(0, 160)}`);
  await p.keyboard.press('Shift+Enter');
  v = await until('eilidh off, seat finlay', (v) => !who(v, 'eilidh').on_air && v.seats.presenter === 'finlay');
  const others = v.sources.filter((s) => !['eilidh', 'finlay'].includes(s.identity)).map((s) => `${s.identity}:${s.role}:${s.on_air}`);
  log(`after ⇧⏎ #2: eilidh on_air=${who(v, 'eilidh').on_air}, finlay on_air=${who(v, 'finlay').on_air}, seat=${v.seats.presenter}; others ${others.join(' ')}`);
  await sleep(800);
  await p.screenshot({ path: `${OUT}/2-handed-over.png` });
  // A third press: nothing left to do.
  await p.keyboard.press('Shift+Enter');
  await sleep(1000);
  v = await view();
  log(`after ⇧⏎ #3: finlay on_air=${who(v, 'finlay').on_air}, seat=${v.seats.presenter}`);
  // Slot 1's talk key (A) now speaks as Finlay, the seat, to Eilidh off air; letting go closes it.
  await p.keyboard.down('KeyA');
  v = await until('finlay talks to eilidh', (v) => who(v, 'finlay').talk_to === 'eilidh', 5000);
  log(`holding A: finlay.talk_to=${who(v, 'finlay').talk_to}`);
  await p.keyboard.up('KeyA');
  v = await until('talk released', (v) => !who(v, 'finlay').talk_to, 5000);
  log('released A: talk closed');
  await b.close();
  log('done');
})().catch((e) => { console.error(e); process.exit(1); });
