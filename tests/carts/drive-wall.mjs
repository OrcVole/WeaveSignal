// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the cart wall on the M2 loopback stack (tests/m2/run-studio.sh, which loads the demo
// pack) from the keyboard alone: the Carts tab, banks on ⌃F1–⌃F5, carts on their letters, a bed
// ducked under the open microphones, Space, ⇧Space twice, F1 from the desk, FILE MISSING; checks
// each step against the control plane, and captures the wall and the desk's glance at
// 1,440 × 900 and 2,560 × 1,440 in both themes.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core CHROMIUM=/path/to/chrome-headless-shell \
//   STUDIO=http://127.0.0.1:31720/ CONTROL=http://127.0.0.1:31710 OUT=/some/dir node tests/carts/drive-wall.mjs
//
// Playwright is not a dependency of the repository. Screenshots go to OUT, never the repository.
import { createRequire } from 'node:module';
import { mkdirSync, renameSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:27420/';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:27310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm2-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-carts-shots';
const MEDIA = process.env.MEDIA ?? join(here, '..', 'm2', 'run', 'carts');
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}
const get = async (path) => (await fetch(`${CONTROL}${path}`, { headers: { Authorization: `Bearer ${TOKEN}` } })).json();
async function until(pred, ms = 5000) {
  const t0 = Date.now();
  for (;;) {
    const v = await get('/api/v1/studio');
    if (pred(v)) return { v, ms: Date.now() - t0 };
    if (Date.now() - t0 > ms) return { v, ms: null };
    await sleep(25);
  }
}
const CODES = '123456QWERTYASDFGHZXCVBN';
const keyOf = (slot) => CODES[slot - 1];
const sounding = (v, id) => v.carts.sounding.some((s) => s.cart === id && !s.stopping);

const cat = await get('/api/v1/carts');
const demo = (id) => cat.carts.find((c) => c.origin === `demo:${id}`);
check('the demo pack is the banks', cat.carts.length === 50 && cat.banks.length === 4, `${cat.carts.length} carts, ${cat.banks.length} banks`);

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
await page.goto(STUDIO);
await page.waitForSelector('.desk');
await sleep(1000);

async function shots(name) {
  for (const theme of ['dark', 'light']) {
    await page.click(`.frame .seg button:has-text("${theme === 'dark' ? 'Dark' : 'Light'}")`);
    for (const [w, h] of [[1440, 900], [2560, 1440]]) {
      await page.setViewportSize({ width: w, height: h });
      await sleep(250);
      await page.screenshot({ path: join(OUT, `${name}-${w}-${theme}.png`) });
    }
  }
  await page.click('.frame .seg button:has-text("Dark")');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.mouse.move(1, 1);
}

// ⌃3: the Carts tab.
await page.keyboard.press('Control+3');
await page.waitForSelector('.cartwall');
check('⌃3 opens the Carts tab', true);
const pads = await page.$$eval('.pad', (els) => els.length);
check('24 slots under the left hand', pads === 24, `${pads}`);

// ⌃F4, then a bed on its key: it loops, and the open microphones duck it.
const bed = demo('bed-1');
await page.keyboard.press(`Control+F${bed.bank}`);
await sleep(150);
let t0 = Date.now();
await page.keyboard.press(keyOf(bed.slot));
let r = await until((v) => sounding(v, bed.id));
check(`⌃F${bed.bank} then ${keyOf(bed.slot)} fires “${bed.label}”`, r.ms !== null, `${Date.now() - t0} ms key to the view`);
check('the bed ducks under the open microphones', r.v.carts.ducked && r.v.carts.mic_open);

// ⌃F1, then applause on 1.
const applause = demo('applause-1');
await page.keyboard.press('Control+F1');
await sleep(150);
t0 = Date.now();
await page.keyboard.press(keyOf(applause.slot));
r = await until((v) => sounding(v, applause.id));
check(`⌃F1 then ${keyOf(applause.slot)} fires “${applause.label}”`, r.ms !== null, `${Date.now() - t0} ms key to the view`);
await sleep(700);
await shots('wall-sounding');

// Its key again fades it.
await page.keyboard.press(keyOf(applause.slot));
r = await until((v) => !sounding(v, applause.id));
check('its key again fades it', r.ms !== null);

// Space stops the cart started last (the bed is the only one left).
await page.keyboard.press(keyOf(applause.slot));
await until((v) => sounding(v, applause.id));
await page.keyboard.press('Space');
r = await until((v) => !sounding(v, applause.id) && sounding(v, bed.id));
check('Space fades the last cart and leaves the bed', r.ms !== null);

// ⇧Space once arms; twice fades everything.
await page.keyboard.press('Shift+Space');
await sleep(300);
const armed = await page.$eval('.now .btn', (b) => b.classList.contains('armed'));
let v = await get('/api/v1/studio');
check('⇧Space once only arms', armed && sounding(v, bed.id));
await page.keyboard.press('Shift+Space');
r = await until((v) => v.carts.sounding.length === 0, 3000);
check('⇧Space twice fades everything', r.ms !== null);

// ⏎ on a bank button is the button's, never the last cart's: it selects the bank, fires nothing.
await page.focus('[data-panel="carts.bank.2"]');
await page.keyboard.press('Enter');
await sleep(400);
v = await get('/api/v1/studio');
const bank2 = await page.$eval('[data-panel="carts.bank.2"]', (b) => b.getAttribute('aria-pressed'));
check('⏎ on a bank button selects the bank and fires nothing', v.carts.sounding.length === 0 && bank2 === 'true');
// The arrows reach a cart without firing it; ⏎ then fires it.
await page.keyboard.press('Control+F1');
await page.keyboard.press('ArrowRight');
await sleep(300);
v = await get('/api/v1/studio');
const second = cat.carts.find((c) => c.bank === 1 && c.slot === 2);
check('an arrow moves to a cart without firing', v.carts.sounding.length === 0);
await page.keyboard.press('Enter');
r = await until((v) => sounding(v, second.id));
check(`⏎ then fires “${second.label}”`, r.ms !== null);
await page.keyboard.press('Space');
await until((v) => v.carts.sounding.length === 0);

// The desk: F1 fires the same top row; the glance shows it.
await page.keyboard.press('Control+1');
await page.waitForSelector('.desk');
const first = cat.carts.find((c) => c.bank === 1 && c.slot === 1);
await page.keyboard.press('F1');
r = await until((v) => sounding(v, first.id));
check(`F1 on the desk fires “${first.label}”`, r.ms !== null);
await sleep(600);
await shots('desk-glance');
await page.keyboard.press('F1');
await until((v) => !sounding(v, first.id));

// FILE MISSING: a file goes; the slot says so and its key does nothing.
const ding = demo('ding-1');
renameSync(join(MEDIA, ding.file), join(MEDIA, `${ding.file}.aside`));
r = await until((v) => v.carts.missing.includes(ding.id), 8000);
check('a cart whose file went says FILE MISSING', r.ms !== null);
await page.keyboard.press('Control+3');
await page.keyboard.press(`Control+F${ding.bank}`);
await sleep(300);
await page.keyboard.press(keyOf(ding.slot));
await sleep(500);
v = await get('/api/v1/studio');
const note = await page.textContent('.cartwall').catch(() => '');
check('its key does nothing', !sounding(v, ding.id));
await shots('wall-missing');
const padText = await page.$eval(`[data-cart-slot="${ding.slot}"]`, (e) => e.textContent);
check('the slot says FILE MISSING and the fix', /File missing/i.test(padText) && padText.includes('Re-link'), padText.slice(0, 80));
renameSync(join(MEDIA, `${ding.file}.aside`), join(MEDIA, ding.file));
r = await until((v) => !v.carts.missing.includes(ding.id), 8000);
check('the file back, the cart is back', r.ms !== null);
void note;

await browser.close();
const failed = results.filter((x) => !x.ok);
log(`${results.length - failed.length} of ${results.length} steps passed; screenshots in ${OUT}`);
process.exit(failed.length ? 1 : 0);
