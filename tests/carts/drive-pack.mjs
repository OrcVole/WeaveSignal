// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives Load the default pack (1.2.1) in a real browser against a real control plane with
// PostgreSQL and the media worker: a station that loaded the pack at its first start, one of
// whose default carts a person has renamed and to which they have added a sound of their own.
// The dialog says what will happen to the bank before it is asked; the load replaces only the
// default carts nobody changed, and the two that are a person's are the same carts afterwards.
// Then one of the pack's banks goes into a bank nobody has made. Captured at 1,440 × 900 and
// 2,560 × 1,440 in the dark and light themes.
//
//   CARTS_PACK=$PWD/assets/carts-demo WEAVESIGNAL_TEST_PREFIX=weavesignal-carts tests/screens/run-screens.sh   # then, in another shell:
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/carts/drive-pack.mjs
//
// Whatever pack is in the tree is the one loaded: the counts are read from it, not written here.
// Playwright is not a dependency of the repository. Screenshots go to OUT, never the repository.
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:26420/';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:26310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'screens-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-cart-pack-shots';
mkdirSync(join(OUT, 'docs'), { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}
const auth = { Authorization: `Bearer ${TOKEN}` };
const api = async (method, path, body, type = 'application/json') => {
  const res = await fetch(`${CONTROL}${path}`, { method, headers: body === undefined ? auth : { ...auth, 'Content-Type': type }, body });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
const cat = async () => (await api('GET', '/api/v1/carts')).body;
const importing = async () => (await api('GET', '/api/v1/studio')).body.carts?.importing ?? null;
async function until(what, fn, ms = 60_000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) {
      check(what, false, `waited ${ms / 1000} s`);
      return null;
    }
    await sleep(200);
  }
}
/** A mono 16-bit WAV of a sine. */
function wav(seconds, hz, level = 0.4, rate = 44_100) {
  const n = Math.round(seconds * rate);
  const b = Buffer.alloc(44 + n * 2);
  b.write('RIFF', 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write('data', 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(Math.sin((2 * Math.PI * hz * i) / rate) * level * 32767), 44 + i * 2);
  return b;
}

// ---- the station as its people left it ----

const pack = (await api('GET', '/api/v1/carts/pack')).body;
check('the station has a default pack to load', pack.available === true && pack.banks.length > 0, JSON.stringify({ available: pack.available, problem: pack.problem }));
const total = pack.banks.reduce((n, b) => n + b.carts.length, 0);
await until('the new station loaded the pack by itself', async () => (await importing()) === null && (await cat()).carts.length === total, 120_000);
const first = pack.banks[0];
const before = await cat();
const inOne = before.carts.filter((c) => c.bank === first.number);
const loaded = (await api('GET', '/api/v1/carts/pack')).body;
check('every cart of the first bank is a default nobody changed', inOne.length === first.carts.length && inOne.every((c) => loaded.untouched.includes(c.id)), `${inOne.length} carts, ${loaded.untouched.length} untouched on the station`);
const renamedSlot = inOne[0].slot;
const free = Array.from({ length: 24 }, (_, i) => i + 1).find((s) => !inOne.some((c) => c.slot === s));
const CODES = '123456QWERTYASDFGHZXCVBN';
const renamed = await api('PATCH', `/api/v1/carts/banks/${first.number}/slots/${renamedSlot}`, JSON.stringify({ label: 'Our own applause' }));
const own = await api('PUT', `/api/v1/carts/banks/${first.number}/slots/${free}?label=Kettle%20FM%20ident&mode=one_shot&origin=ident.wav`, wav(1, 880), 'application/octet-stream');
check('a person renames a default and adds a sound of their own', renamed.status === 200 && own.status === 201, `${renamed.status} ${own.status}`);

// ---- the browser ----

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, locale: 'en-GB', timezoneId: 'Europe/London' });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(m.text()));
const dlg = page.locator('dialog[data-panel="carts.pack"]');
async function shots(name, pause = 250) {
  for (const theme of ['dark', 'light']) {
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), theme);
    for (const [w, h] of [
      [1440, 900],
      [2560, 1440],
    ]) {
      await page.setViewportSize({ width: w, height: h });
      await sleep(pause);
      await page.screenshot({ path: join(OUT, `${name}-${w}-${theme}.png`) });
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'));
  await sleep(150);
}

await page.goto(STUDIO);
await page.keyboard.press('Control+3');
await page.waitForSelector('.cartwall');
await until('the wall shows the pack', async () => (await page.locator('.pad:not(.empty)').count()) > 0);
await page.keyboard.press('Control+F1');
await sleep(400);
await shots('01-wall');

await page.getByRole('button', { name: 'Default pack…' }).click();
await dlg.locator('[data-testid="pk-plan"]').waitFor();
check('the dialog is on the pack’s bank of the tab’s number, into the station’s bank of that number', (await dlg.locator('input[name="pk-bank"]').first().isChecked()) && (await dlg.locator('#pk-into').inputValue()) === String(first.number));
const said = ((await dlg.locator('[data-testid="pk-plan"]').textContent()) ?? '').replace(/\s+/g, ' ').trim();
const gone = first.carts.length - 1;
check(
  'it says what will happen first: the defaults nobody changed are replaced, the two that are a person’s stay',
  said.includes(`holds ${first.carts.length + 1} carts.`) && said.includes(`${gone} are default sounds nobody changed and are replaced`) && said.includes(`2 are yours and stay, on ${[renamedSlot, free].sort((a, b) => a - b).map((s) => CODES[s - 1]).join(', ')}`) && said.includes('the pack’s 1 sound for those keys is left out'),
  said,
);
check('the button names it', ((await dlg.locator('#pk-load').textContent()) ?? '').trim().startsWith(`Replace ${gone} and load ${gone}`), (await dlg.locator('#pk-load').textContent()) ?? '');
await shots('02-pack-replace');
await sleep(200);
await dlg.screenshot({ path: join(OUT, 'docs', 'how-to-add-your-own-cart-pack.png') });
// All of the pack: a line for each of the station's banks.
await dlg.getByText(/^All \d+, each into/).click();
check('all of the pack is a line for each bank', (await dlg.locator('[data-testid="pk-plan"] li').count()) === pack.banks.length);
await dlg.locator('input[name="pk-bank"]').first().check();
await dlg.locator('input[name="pk-bank"]').first().focus();
await page.keyboard.press('Enter');
await until('the dialog closes and the load starts', async () => !(await dlg.evaluate((d) => d.open)), 15_000);
const counted = await until('the strip counts the sounds through the media worker', async () => ((await page.locator('[data-testid="carts-importing"]').count()) ? page.locator('[data-testid="carts-importing"]').textContent() : (await importing()) === null ? 'done' : null), 15_000);
log('   the strip:', counted);
await until('the load finishes', async () => (await importing()) === null, 120_000);
const after = await cat();
const at = (c, slot) => c.carts.find((x) => x.bank === first.number && x.slot === slot);
check('the renamed cart is the same cart, with its name and its sound', at(after, renamedSlot)?.id === renamed.body.id && at(after, renamedSlot)?.label === 'Our own applause' && at(after, renamedSlot)?.file === renamed.body.file, JSON.stringify(at(after, renamedSlot)));
check('the person’s own sound is the same cart', at(after, free)?.id === own.body.id);
const others = first.carts.filter((p) => p.slot !== renamedSlot);
check(
  'every other cart of the bank is the pack’s sound again, a new cart on the same key',
  others.every((p) => {
    const now = at(after, p.slot);
    const was = at(before, p.slot);
    return now && now.label === p.label && now.id !== was?.id && now.origin.startsWith('demo:');
  }),
  `${others.length} carts`,
);
check('nothing was added or lost', after.carts.length === before.carts.length + 1, `${before.carts.length} + 1 → ${after.carts.length}`);
await until('the wall says the carts are loaded', async () => ((await page.locator('[data-panel="carts.toast"]').count()) ? ((await page.locator('[data-panel="carts.toast"]').textContent()) ?? '').includes('Carts loaded') : false), 10_000);
await shots('03-loaded');

// ---- one of the pack's banks into a bank nobody has made ----

const spare = [5, 4, 3, 2, 1].find((n) => !after.banks.some((b) => b.number === n && !b.show));
if (spare) {
  const second = pack.banks[1] ?? first;
  await page.keyboard.press(`Control+F${spare}`);
  await page.getByRole('button', { name: 'Default pack…' }).click();
  await dlg.locator('[data-testid="pk-plan"]').waitFor();
  await dlg.locator('input[name="pk-bank"]').nth(pack.banks.indexOf(second)).check();
  await dlg.locator('#pk-into').selectOption(String(spare));
  const empty = ((await dlg.locator('[data-testid="pk-plan"]').textContent()) ?? '').replace(/\s+/g, ' ').trim();
  check('a bank nobody has made is made and loaded, and nothing is asked', empty.includes(`The station’s bank ${spare} has not been made: it is made under the pack’s name. ${second.carts.length} sounds are loaded into it.`) && ((await dlg.locator('#pk-load').textContent()) ?? '').trim().startsWith(`Load ${second.carts.length} sounds`), empty);
  await shots('04-pack-empty-bank');
  await dlg.locator('#pk-load').click();
  await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)), 15_000);
  await sleep(300);
  await until('the load finishes', async () => (await importing()) === null, 120_000);
  const made = await cat();
  check('the bank is made under the pack’s name and holds the pack’s sounds', made.banks.some((b) => b.number === spare && b.name === second.name && !b.show) && made.carts.filter((c) => c.bank === spare).length === second.carts.length, JSON.stringify(made.banks.find((b) => b.number === spare)));
} else {
  log('   every bank is made: the empty-bank load is not driven');
}

check('the page raised no error', errors.length === 0, errors.slice(0, 3).join(' | '));
await browser.close();
const failed = results.filter((r) => !r.ok);
log(failed.length ? `${failed.length} failed, ${results.length - failed.length} passed` : `all ${results.length} passed`, `· pictures in ${OUT}`);
process.exit(failed.length ? 1 : 0);
