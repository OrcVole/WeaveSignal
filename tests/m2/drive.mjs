// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the studio app on the M2 loopback stack (tests/m2/run-studio.sh) from the keyboard,
// checks each step against the control plane, and captures desk B and the screener at
// 1,440 × 900 and 2,560 × 1,440 in both themes, with playout up and down.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/m2/drive.mjs
//
// Playwright is not a dependency of the repository: point PLAYWRIGHT_CORE at an installed
// playwright-core with its Chromium headless shell. Screenshots go to OUT, never the repository.
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:27420/';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:27310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm2-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-m2-shots';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

async function view() {
  const r = await fetch(`${CONTROL}/api/v1/studio`, { headers: { Authorization: `Bearer ${TOKEN}` } });
  return r.json();
}
async function until(what, pred, ms = 8000) {
  const t0 = Date.now();
  for (;;) {
    const v = await view();
    if (pred(v)) return { v, ms: Date.now() - t0 };
    if (Date.now() - t0 > ms) return { v, ms: null };
    await sleep(150);
  }
}
const src = (v, id) => v.sources.find((s) => s.identity === id);
const caller = (v, id) => v.callers.find((c) => c.identity === id);
const stack = (arg) => execFileSync(join(here, 'run-studio.sh'), [arg], { stdio: 'inherit' });
/** The desk's own status, straight from its control socket (JSON lines, ADR-0088). */
const deskStatus = () =>
  JSON.parse(
    execFileSync('python3', [
      '-c',
      `import socket
s=socket.socket(socket.AF_UNIX); s.connect(${JSON.stringify(join(here, 'run', 'desk.sock'))})
s.sendall(b'{"op":"status"}\\n'); print(s.recv(65536).decode())`,
    ]).toString(),
  );

const browser = await chromium.launch({ headless: true });

async function open(width, height, theme = 'dark') {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1 });
  await ctx.addInitScript((t) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
    } catch {}
  }, theme);
  const page = await ctx.newPage();
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.goto(STUDIO);
  await page.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
  await sleep(800);
  return { ctx, page };
}

async function shot(page, name) {
  await page.mouse.move(2, 2); // no hover state, no name plate
  await sleep(500);
  const path = join(OUT, `${name}.png`);
  await page.screenshot({ path });
  log('shot', path);
}

// ---- the keyboard run, at 1,440 × 900 ----
const { ctx, page } = await open(1440, 900);
const k = page.keyboard;

// Desk: cue Finlay on slot 5 with T (slot 5's cue on the desk tab).
await k.press('t');
check('desk T cues slot 5 (Finlay)', (await until('cue', (v) => src(v, 'finlay')?.cue)).ms !== null);

// Screener: ⌃2, jump to Niall, send him to the desk with P.
await k.press('Control+2');
await page.locator('[data-panel="screener.queue"]').waitFor();
const order = (await view()).callers.map((c) => c.identity);
await k.press(String(order.indexOf('niall') + 1));
await k.press('p');
const sent = await until('ready', (v) => caller(v, 'niall')?.state === 'ready' && caller(v, 'niall')?.slot === 4);
check('screener P sends Niall to the desk, strip 4', sent.ms !== null);

// Desk: the ready toast; Esc is "not now": DESK BUSY.
await k.press('Control+1');
await page.getByText('Niall is ready for you · strip 4').waitFor({ timeout: 5000 });
check('desk shows the ready caller in the toast slot', true);
await k.press('Escape');
const busy = await until('declined', (v) => !!caller(v, 'niall')?.declined && !v.desk_taking_callers);
check('desk Esc declines: DESK BUSY, the desk not taking callers', busy.ms !== null);

// Screener: Dougal; type into the line field: its P, T and G must not fire (rule 3).
await k.press('Control+2');
await k.press(String(order.indexOf('dougal') + 1));
await page.locator('#f-line').click();
await k.type('Please put me through, the bins');
await k.press('Escape'); // leaves the field
await sleep(1200);
const v = await view();
check('typing P, T, G in a text field fires nothing', caller(v, 'dougal').state !== 'ready' && !v.chat.influx, caller(v, 'dougal').state);
check('the line typed is saved for the presenter', caller(v, 'dougal').line === 'Please put me through, the bins', caller(v, 'dougal').line);
await page.locator('#f-note').click();
await k.type('Nervous; go gentle.');
await k.press('Escape');
await k.press('p');
check('screener P sends Dougal', (await until('dougal ready', (v) => caller(v, 'dougal')?.state === 'ready')).ms !== null);

// Screener: hold T on Jessie: SCREENING and the screener's voice to her alone, while held.
await k.press(String(order.indexOf('jessie') + 1));
await k.down('t');
const held = await until('talk', (v) => caller(v, 'jessie')?.state === 'screening');
await sleep(400);
const whileHeld = deskStatus().people.find((p) => p.identity === 'moira')?.talk_to;
await k.up('t');
await sleep(400);
const afterRelease = deskStatus().people.find((p) => p.identity === 'moira')?.talk_to;
check('screener T held: Jessie SCREENING, the screener talking to her alone', held.ms !== null && whileHeld === 'jessie', String(whileHeld));
check('T released: the screener talks to nobody', afterRelease === null, String(afterRelease));

// Desk: Dougal's toast; R (slot 4's cue) takes him, then 4 puts him on air.
await k.press('Control+1');
await page.getByText(/Dougal is ready for you|Please put me through/).first().waitFor({ timeout: 5000 });
await k.press('r');
check('desk R cues Dougal: taken', (await until('taken', (v) => src(v, 'dougal')?.cue)).ms !== null);
await k.press('4');
check('desk 4 puts Dougal on air', (await until('on air', (v) => src(v, 'dougal')?.on_air)).ms !== null);
await k.press('r'); // cue off again
// Mute and unmute Kirsty (slot 3, C).
await k.press('c');
check('desk C mutes Kirsty', (await until('mute', (v) => src(v, 'kirsty')?.mute)).ms !== null);
await k.press('c');
await until('unmute', (v) => !src(v, 'kirsty')?.mute);
// Talk to Gregor (slot 2, S held).
await k.down('s');
await sleep(400);
const eilidhTalk = deskStatus().people.find((p) => p.identity === 'eilidh')?.talk_to;
await k.up('s');
check('desk S held: the presenter talks to Gregor alone', eilidhTalk === 'gregor', String(eilidhTalk));

// Influx on the screener: G G on, its toast, G G off restores the modes.
await k.press('Control+2');
const before = (await view()).chat;
await k.press('g');
await k.press('g');
const on = await until('influx', (v) => v.chat.influx && v.chat.slow && v.chat.accounts_only);
check('screener G G: influx on, five settings', on.ms !== null);
await page.getByText(/Influx mode on since/).waitFor({ timeout: 4000 });
await shot(page, 'screener-1440-dark-influx');
await k.press('g');
await k.press('g');
const off = await until('influx off', (v) => !v.chat.influx);
check('screener G G again: influx off, the earlier modes back', off.ms !== null && JSON.stringify({ ...off.v.chat, influx_since_ms: null }) === JSON.stringify({ ...before, influx_since_ms: null }));

// ---- captures with playout up ----
await k.press('Control+1');
await sleep(1500);
await shot(page, 'desk-1440-dark');
await page.getByRole('button', { name: 'Light' }).click();
await shot(page, 'desk-1440-light');
await k.press('Control+2');
await shot(page, 'screener-1440-light');
await page.getByRole('button', { name: 'Dark' }).click();
await shot(page, 'screener-1440-dark');
await k.press('Control+1');
for (const theme of ['dark', 'light']) {
  const big = await open(2560, 1440, theme);
  await shot(big.page, `desk-2560-${theme}`);
  await big.page.keyboard.press('Control+2');
  await shot(big.page, `screener-2560-${theme}`);
  await big.ctx.close();
}
// The name plates: every plate pinned, and the audit of which (if any) lies over a control.
const audit = await (await ctx.newPage());
await audit.setViewportSize({ width: 1440, height: 900 });
await audit.goto(STUDIO + '?audit=1');
await audit.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
await sleep(1500);
await audit.screenshot({ path: join(OUT, 'desk-1440-dark-plates.png') });
const auditText = await audit.locator('#pn-audit').textContent();
check('every desk name plate clear of controls', !/OVER/.test(auditText ?? ''), (auditText ?? '').split('\n')[0]);
await audit.close();

// ---- playout down: the lamp, the strips, and back ----
stack('down');
const down = await until('playout down', (v) => !v.desk.playout.connected, 15000);
check('stopping Liquidsoap: the desk reports playout down', down.ms !== null, `${down.ms} ms`);
await sleep(1200);
const lampDown = await page.locator('[data-panel="desk.lamp"]').textContent();
const openTiles = await page.locator('.tile.is-open').count();
const onairTiles = await page.locator('.tile.is-onair').count();
check('lamp PLAYOUT DOWN, open tiles OPEN, none ON AIR', /Playout down/i.test(lampDown) && openTiles > 0 && onairTiles === 0, `${lampDown?.trim()} · ${openTiles} open · ${onairTiles} on air`);
await shot(page, 'desk-1440-dark-playout-down');
await page.getByRole('button', { name: 'Light' }).click();
await shot(page, 'desk-1440-light-playout-down');
await page.getByRole('button', { name: 'Dark' }).click();
await k.press('Control+2');
await shot(page, 'screener-1440-dark-playout-down');
await k.press('Control+1');
for (const theme of ['dark', 'light']) {
  const big = await open(2560, 1440, theme);
  await shot(big.page, `desk-2560-${theme}-playout-down`);
  await big.ctx.close();
}
const t0 = Date.now();
stack('up');
const back = await until('playout back', (v) => v.desk.playout.connected, 60000);
await page.locator('[data-panel="desk.lamp"]', { hasText: /On air/i }).waitFor({ timeout: 10000 });
check('starting Liquidsoap: ON AIR returns', back.ms !== null, `${Date.now() - t0} ms after the start`);
await shot(page, 'desk-1440-dark-back');

// ---- drop: ⌃D ⌃D on the screener's selected caller, last (it takes them out of the room) ----
await k.press('Control+2');
await k.press(String(((await view()).callers.findIndex((c) => c.identity === 'tavish')) + 1));
await k.press('Control+d');
await sleep(300);
const armed = await page.getByText('Press again').count();
await k.press('Control+d');
const dropped = await until('dropped', (v) => !caller(v, 'tavish'));
check('screener ⌃D ⌃D: armed, then Tavish dropped from the queue and the room', armed > 0 && dropped.ms !== null);

await ctx.close();
await browser.close();
const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
