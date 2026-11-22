// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives adding and changing carts from the cart wall (1.2.1) in a real browser, from the
// keyboard where a key exists: an empty slot's key opens the dialog, a sound is chosen, refused,
// sent, renamed, moved to another key, replaced, dropped on a pad, removed; a bank is made and
// renamed. Each step is checked against what the station holds, and the wall and its dialogs are
// captured at 1,440 × 900 and 2,560 × 1,440 in the dark and light themes.
//
// Two ways to run it, MODE says which:
//
//   stack    (the default) against a real control plane with PostgreSQL and the media worker:
//            tests/screens/run-screens.sh, a station with no cart banks yet. The upload goes
//            through the worker; the loudness and gain it stored are read back from the station.
//            No desk is in that stack, so nothing is fired here (`just test-carts` fires carts).
//
//              WEAVESIGNAL_TEST_PREFIX=weavesignal-carts tests/screens/run-screens.sh   # then, in another shell:
//              PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/carts/drive-edit.mjs
//
//   sandbox  against the sandbox build (the station simulated in the page), served as static
//            files: the same dialogs on the demo banks, a cart fired and cued around them (Space
//            on a cart's Edit stops nothing; Esc that closes the dialog clears no cue).
//
//              just sandbox-build && (cd apps/studio/dist-sandbox && python3 -m http.server 27911 --bind 127.0.0.1)
//              MODE=sandbox PLAYWRIGHT_CORE=... OUT=/some/dir node tests/carts/drive-edit.mjs
//
// Playwright is not a dependency of the repository. Screenshots and the sounds this makes go to
// OUT, never the repository.
import { createRequire } from 'node:module';
import { closeSync, ftruncateSync, mkdirSync, openSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const MODE = process.env.MODE ?? 'stack';
const STUDIO = process.env.STUDIO ?? (MODE === 'sandbox' ? 'http://127.0.0.1:27911/' : 'http://127.0.0.1:26420/');
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:26310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'screens-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-cart-edit-shots';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

// ---- the sounds: made here, so the repository's own are not this test's ----

/** A mono 16-bit WAV of a sine: `seconds` long at `hz`, at `level` of full scale. */
function wav(seconds, hz, level = 0.5, rate = 44_100) {
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
const sounds = join(OUT, 'sounds');
mkdirSync(sounds, { recursive: true });
const file = (name, bytes) => {
  const p = join(sounds, name);
  writeFileSync(p, bytes);
  return p;
};
const IDENT = file('01_Top-of-the-hour.wav', wav(2, 440, 0.5));
const SHORT = file('ident-short.wav', wav(1, 660, 0.25));
const STING = file('news sting.wav', wav(3, 330, 0.4));
const NOT_SOUND = file('minutes.wav', Buffer.from('<html>the minutes of the last meeting</html>'.repeat(40)));
// 65 MB of nothing: larger than the station's 64, and never sent.
const HUGE = join(sounds, 'the-whole-show.wav');
{
  const fd = openSync(HUGE, 'w');
  ftruncateSync(fd, 65 * 1024 * 1024);
  closeSync(fd);
}

// ---- the browser ----

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM, args: ['--autoplay-policy=no-user-gesture-required'] });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, locale: 'en-GB', timezoneId: 'Europe/London' });
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
// A refusal the test asks for is a failed request in the console, and the sandbox's sounds need
// not be served: neither is the page's error.
page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(m.text()));

const CODES = '123456QWERTYASDFGHZXCVBN';
const keyOf = (slot) => CODES[slot - 1];
const dlg = page.locator('dialog[data-panel="carts.edit"]');
const bankDlg = page.locator('dialog[data-panel="carts.bank-edit"]');
const pad = (slot) => page.locator(`[data-cart-slot="${slot}"]`);
const editBtn = (slot) => page.locator(`[data-cart-edit="${slot}"]`);
const focused = () => page.evaluate(() => document.activeElement?.getAttribute('data-cart-slot') ?? document.activeElement?.getAttribute('data-cart-edit') ?? document.activeElement?.id ?? document.activeElement?.tagName);
const toast = async () => ((await page.locator('[data-panel="carts.toast"]').count()) ? ((await page.locator('[data-panel="carts.toast"]').textContent()) ?? '') : '');

/** The show whose own banks the window works with (stack: the show the station's step comes
 *  from, which the seeded week may or may not have on air when this runs). */
let SHOW = null;
let SHOW_NAME = '';
/** What the station holds: the catalogue as this window sees it. */
async function cat() {
  if (MODE === 'sandbox') return page.evaluate(() => JSON.parse(JSON.stringify(globalThis.__kettle.catalogue)));
  const q = SHOW ? `?show=${encodeURIComponent(SHOW)}` : '';
  return (await fetch(`${CONTROL}/api/v1/carts${q}`, { headers: { Authorization: `Bearer ${TOKEN}` } })).json();
}
/** The cart in a slot of the tab's bank. In the sandbox tab 1 is the show's own bank. */
async function cartAt(bank, slot) {
  const c = await cat();
  const own = c.banks.some((b) => b.number === bank && b.show);
  return c.carts.find((x) => x.bank === bank && x.slot === slot && !!x.show === own) ?? null;
}
async function until(what, fn, ms = 10_000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) {
      check(what, false, `waited ${ms / 1000} s`);
      return null;
    }
    await sleep(100);
  }
}

/** The window at both sizes in both themes; the theme is set on the page, so a dialog can stay open. */
async function shots(name, pause = 250) {
  const was = await page.evaluate(() => document.documentElement.dataset.theme ?? 'dark');
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
  await page.evaluate((t) => (document.documentElement.dataset.theme = t), was);
  await sleep(150);
}

/** One panel as the documentation shows it: the app's default theme at 1,440 × 900, into
 *  OUT/docs. The pictures under docs/src/assets/screens/how-to-add-your-own-cart-*.png are
 *  these, from the sandbox run, compressed with pngquant (65-90). */
mkdirSync(join(OUT, 'docs'), { recursive: true });
async function docShot(locator, name) {
  await sleep(200);
  await locator.screenshot({ path: join(OUT, 'docs', `${name}.png`) });
}

/** Whether anything is on air: a show by the schedule, or anyone on air at the desk. */
async function onAir() {
  const v = MODE === 'sandbox' ? await page.evaluate(() => JSON.parse(JSON.stringify(globalThis.__kettle.view(Date.now())))) : await (await fetch(`${CONTROL}/api/v1/studio`, { headers: { Authorization: `Bearer ${TOKEN}` } })).json();
  return !!(v.step?.on_air || v.sources?.some((s) => s.on_air));
}
/** Open an empty slot. Off air its key does; on air the key must not (a stray key never covers
 *  the wall), and the pad's own press does. */
async function openEmpty(slot) {
  const air = await onAir();
  await page.keyboard.press(keyOf(slot).toLowerCase());
  if (air) {
    await sleep(500);
    check('on air an empty slot’s key opens no dialog', (await page.locator('dialog[open]').count()) === 0);
    await page.keyboard.press('Escape');
    await pad(slot).click();
  } else {
    log('   nothing is on air: the empty slot’s key opens the dialog');
  }
}

/** Drag a file over a pad, and (with `drop`) let it go there. */
async function drag(slot, path, name, drop) {
  const bytes = [...require('node:fs').readFileSync(path)];
  const dt = await page.evaluateHandle(
    ({ bytes, name }) => {
      const d = new DataTransfer();
      d.items.add(new File([new Uint8Array(bytes)], name, { type: 'audio/wav' }));
      return d;
    },
    { bytes, name },
  );
  await pad(slot).dispatchEvent('dragover', { dataTransfer: dt });
  if (drop) await pad(slot).dispatchEvent('drop', { dataTransfer: dt });
}

// A closed dialog is not drawn: nothing of either lies over the wall.
const hiddenDialogs = async () => page.evaluate(() => [...document.querySelectorAll('.cartwall dialog')].every((d) => d.open || getComputedStyle(d).display === 'none'));

// ---- open the Carts tab ----

await page.goto(MODE === 'sandbox' ? `${STUDIO}#carts` : STUDIO);
if (MODE === 'sandbox') {
  await page.getByRole('button', { name: 'Start with sound' }).click();
  await page.waitForFunction(() => globalThis.__kettle?.begun === true, null, { timeout: 30_000 });
  // The sandbox's own guide bar is not the studio app's window.
  await page.addStyleTag({
    content: '[data-panel="sandbox.guide"], [data-panel="sandbox.caption"] { display: none !important; } .shell.with-guide { grid-template-rows: auto minmax(0, 1fr) !important; } .shell.with-guide.with-all { grid-template-rows: auto auto minmax(0, 1fr) !important; }',
  });
}
await page.keyboard.press('Control+3');
await page.waitForSelector('.cartwall');
await until('the catalogue is read', async () => (await page.locator('.cartwall .bank .count').first().textContent()) !== null);
await sleep(600);
check('the wall has 24 pads, each still a single button with its slot', (await page.locator('[data-cart-slot]').count()) === 24);
check('the closed dialogs are not drawn over the wall', await hiddenDialogs());

const BANK = 1;
/** Three empty slots of the tab: one to add to, one to move to, one to drop on. */
let [ADD, MOVE, DROP] = [7, 8, 9];

if (MODE === 'stack') {
  // A station with no banks: the tab says so, and an empty slot's key makes the bank first.
  const before = await cat();
  check('the station starts with no cart banks', before.banks.length === 0, `${before.banks.length} banks`);
  check('the strip offers to make the bank', await page.getByRole('button', { name: 'Make bank 1…' }).isVisible());
  check('an empty slot invites a cart', ((await pad(ADD).textContent()) ?? '').includes('Add a cart'));
  await shots('01-empty-station');
  await openEmpty(ADD);
  await bankDlg.waitFor();
  check('an empty slot on a tab with no bank opens the bank dialog first', ((await bankDlg.textContent()) ?? '').includes('This tab has no bank yet'));
  // With a show on air the window has a show whose banks this key may change: the dialog asks
  // whose the bank is to be, the show's own first. Otherwise it is the station's.
  SHOW = (await (await fetch(`${CONTROL}/api/v1/studio`, { headers: { Authorization: `Bearer ${TOKEN}` } })).json()).step?.show?.slug ?? null;
  if (SHOW) {
    check('with a show on air the bank dialog asks whose the bank is to be, the show’s own first', (await bankDlg.getByText('Whose bank').count()) === 1 && (await bankDlg.locator('input[name="be-owner"]').first().isChecked()));
    SHOW_NAME = ((await bankDlg.locator('.choices b').first().textContent()) ?? '').replace(/’s own$/, '');
  } else {
    check('the bank dialog says whose bank it makes', ((await bankDlg.locator('[data-testid="be-whose"]').textContent()) ?? '').includes('The station’s bank, shared by every show'));
  }
  await bankDlg.locator('#be-name').focus();
  await page.keyboard.press('Enter');
  check('a bank needs a name', ((await bankDlg.textContent()) ?? '').includes('Give the bank a name'));
  await page.keyboard.type('Jingles');
  await bankDlg.getByRole('button', { name: 'Colour 4' }).click();
  await shots('02-make-bank');
  await bankDlg.locator('#be-name').focus();
  await page.keyboard.press('Enter');
  await dlg.waitFor();
  const made = await cat();
  check('the bank is made on the station, the show’s own or the station’s as the dialog said', made.banks.length === 1 && made.banks[0].name === 'Jingles' && made.banks[0].colour_slot === 4 && (made.banks[0].show ?? null) === SHOW, JSON.stringify(made.banks));
  const whose = SHOW ? `${SHOW_NAME}’s own bank 1, Jingles` : 'the station’s bank 1, Jingles';
  check('the cart dialog follows, on the slot whose key was pressed, and names whose bank it is', ((await dlg.locator('.where').textContent()) ?? '').includes(`${keyOf(ADD)}in ${whose}`), (await dlg.locator('.where').textContent()) ?? '');
} else {
  // The demo banks are loaded: tab 1 is the show's own. Find three empty slots on it.
  const c = await cat();
  const taken = new Set(c.carts.filter((x) => x.bank === 1 && x.show).map((x) => x.slot));
  [ADD, MOVE, DROP] = Array.from({ length: 24 }, (_, i) => i + 1).filter((s) => !taken.has(s));
  check('an idle cart has its Edit beside its pad', (await editBtn(1).count()) === 1);
  check('an empty slot invites a cart', ((await pad(ADD).textContent()) ?? '').includes('Add a cart'));
  await pad(2).hover();
  await shots('01-wall');
  await docShot(page.locator('[data-panel="carts.wall"]'), 'how-to-add-your-own-cart-wall');
  // Space on a cart's Edit presses it and stops nothing: a bed sounds through it.
  await page.keyboard.press(keyOf(2).toLowerCase());
  await until('a bed sounds', () => page.evaluate(() => globalThis.__kettle.sounding.some((s) => !s.stopping)));
  check('a sounding cart is not offered for change', (await editBtn(2).count()) === 0);
  await pad(3).focus();
  await page.keyboard.press('Tab');
  check('Tab from a pad reaches its Edit', (await focused()) === '3', String(await focused()));
  await page.keyboard.press('Space');
  await dlg.waitFor();
  check('Space on Edit opens the dialog', ((await dlg.locator('h2').textContent()) ?? '') === 'Edit cart');
  check('and stops nothing: the bed still sounds', await page.evaluate(() => globalThis.__kettle.sounding.some((s) => !s.stopping)));
  // Esc: the first leaves the name's field, the second closes; neither is the wall's Esc.
  await page.keyboard.press(`Shift+${keyOf(4)}`).catch(() => {});
  await page.keyboard.press('Escape');
  check('the first Esc leaves the field and the dialog stays', await dlg.evaluate((d) => d.open));
  await page.keyboard.press('Escape');
  await until('the second Esc closes the dialog', async () => !(await dlg.evaluate((d) => d.open)));
  check('the focus is back on the cart’s pad', (await focused()) === '3', String(await focused()));
  // A cue stays through a dialog closed with Esc.
  await page.keyboard.press(`Shift+${keyOf(4)}`);
  await until('a cart is cued', () => page.evaluate(() => globalThis.__kettle.sounding.some((s) => s.cue && !s.stopping)));
  await editBtn(3).click();
  await dlg.waitFor();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)));
  check('Esc that closed the dialog cleared no cue', await page.evaluate(() => globalThis.__kettle.sounding.some((s) => s.cue && !s.stopping)));
  await page.keyboard.press('Escape');
  await page.keyboard.press('Shift+Space');
  await page.keyboard.press('Shift+Space');
  await until('everything is faded', () => page.evaluate(() => !globalThis.__kettle.sounding.some((s) => !s.stopping)));
  // The empty slot's key opens the dialog to add.
  await openEmpty(ADD);
  await dlg.waitFor();
}

// ---- add a cart ----

check('the dialog is Add a cart', ((await dlg.locator('h2').textContent()) ?? '') === 'Add a cart');
check('the chooser has the focus', (await focused()) === 'ce-choose', String(await focused()));
check('it says what a slot takes', ((await dlg.locator('#ce-takes').textContent()) ?? '').includes('up to 64 MB and 10 minutes'), (await dlg.locator('#ce-takes').textContent()) ?? '');
await shots('03-add-empty');
await dlg.getByRole('button', { name: /Add cart/ }).click();
check('a cart needs a sound', ((await dlg.locator('[data-testid="ce-file-err"]').textContent()) ?? '').includes('Choose the sound first'));
const input = dlg.locator('input[type=file]');
await input.setInputFiles(HUGE);
check('a file larger than the station takes is refused before it is sent', ((await dlg.locator('[data-testid="ce-file-err"]').textContent()) ?? '').includes('is 65.0 MB. This station takes sounds up to 64 MB.'), (await dlg.locator('[data-testid="ce-file-err"]').textContent()) ?? '');
await input.setInputFiles(NOT_SOUND);
check('a file chosen offers its name as the cart’s', (await dlg.locator('#ce-label').inputValue()) === 'minutes');
await dlg.getByRole('button', { name: /Add cart/ }).click();
const refusal = await until('the station refuses what is not a sound', async () => ((await dlg.locator('[data-testid="ce-problem"]').count()) ? dlg.locator('[data-testid="ce-problem"]').textContent() : null), 20_000);
check('the refusal is the station’s own sentence', (refusal ?? '').includes('not an audio file'), refusal ?? '');
check('nothing was added', (await cartAt(BANK, ADD)) === null);
await shots('04-add-refused');

await input.setInputFiles(IDENT);
check('another file takes the name’s place while it is untouched', (await dlg.locator('#ce-label').inputValue()) === '01 Top of the hour');
check('the file chosen is named with its size', /01_Top-of-the-hour\.wav\s*· 172 KB/.test((await dlg.locator('[data-testid="ce-file"]').textContent()) ?? ''), (await dlg.locator('[data-testid="ce-file"]').textContent()) ?? '');
await dlg.locator('#ce-label').fill('Top of the hour');
await dlg.locator('#ce-detail').fill('Kettle FM');
await dlg.getByText('Sweeper', { exact: true }).click();
await dlg.locator('#ce-ramp').fill('9');
await shots('05-add-chosen');
await docShot(dlg, 'how-to-add-your-own-cart-add');
await dlg.locator('#ce-label').focus();
await page.keyboard.press('Enter');
check('a ramp longer than the sound is refused by the station', ((await until('the refusal', async () => ((await dlg.locator('[data-testid="ce-problem"]').count()) ? dlg.locator('[data-testid="ce-problem"]').textContent() : null), 20_000)) ?? '').includes('a ramp ends before its cart does'));
await dlg.locator('#ce-ramp').fill('1.5');
// Hold the request a moment, so the sending state can be seen and captured.
if (MODE === 'stack') {
  await page.route('**/carts/banks/*/slots/*', async (route) => {
    if (route.request().method() === 'PUT') await sleep(2500);
    await route.continue();
  });
} else {
  await page.evaluate(() => {
    const real = AudioContext.prototype.decodeAudioData;
    AudioContext.prototype.decodeAudioData = function (...a) {
      return new Promise((r) => setTimeout(r, 2500)).then(() => real.apply(this, a));
    };
    globalThis.__realDecode = real;
  });
}
await dlg.locator('#ce-label').focus();
await page.keyboard.press('Enter');
await dlg.locator('[data-testid="ce-working"]').waitFor();
check('a file on its way says so, and offers Stop', ((await dlg.locator('[data-testid="ce-working"]').textContent()) ?? '').includes('Sending “01_Top-of-the-hour.wav” (172 KB)') && (await dlg.getByRole('button', { name: /Stop/ }).isEnabled()));
await shots('06-add-sending', 100);
await until('the dialog closes when the cart is added', async () => !(await dlg.evaluate((d) => d.open)), 30_000);
if (MODE === 'stack') await page.unroute('**/carts/banks/*/slots/*');
else await page.evaluate(() => (AudioContext.prototype.decodeAudioData = globalThis.__realDecode));
const added = await until('the cart is on the station', () => cartAt(BANK, ADD));
check('the cart is as the dialog said', !!added && added.label === 'Top of the hour' && added.detail === 'Kettle FM' && added.mode === 'sweeper' && added.ramp_ms === 1500 && added.origin === '01_Top-of-the-hour.wav', JSON.stringify(added));
if (MODE === 'stack' && added) {
  const c = await cat();
  // A sine at half scale in mono is about −9 LUFS; the gain brings it to the station's target.
  check('the media worker measured it and stored the gain to the station’s loudness', Math.abs(added.loudness_lufs + added.gain_db - c.loudness_lufs) < 0.1 && added.duration_ms === 2000 && /^[0-9a-f-]{36}\.flac$/.test(added.file), `measured ${added.loudness_lufs} LUFS, gain ${added.gain_db} dB, target ${c.loudness_lufs}`);
}
await until('the pad shows the cart', async () => ((await pad(ADD).textContent()) ?? '').includes('Top of the hour'));
check('the toast says what was measured', /Cart added/.test(await toast()) && (MODE === 'sandbox' || /measured −?\d+\.\d LUFS, plays at/.test(await toast())), await toast());
check('the focus is on the new cart’s pad', (await focused()) === String(ADD), String(await focused()));
await shots('07-added');

// ---- change it: its name and its key, without its sound ----

await page.keyboard.press('Tab');
check('Tab from the pad reaches its Edit', (await focused()) === String(ADD), String(await focused()));
await page.keyboard.press('Enter');
await dlg.waitFor();
check('the dialog is Edit cart, on its name', ((await dlg.locator('h2').textContent()) ?? '') === 'Edit cart' && (await focused()) === 'ce-label');
check('it says what the sound is', /0:02/.test((await dlg.locator('[data-testid="ce-has"]').textContent()) ?? '') && ((await dlg.locator('[data-testid="ce-has"]').textContent()) ?? '').includes('from 01_Top-of-the-hour.wav'), (await dlg.locator('[data-testid="ce-has"]').textContent()) ?? '');
await shots('08-edit');
await docShot(dlg, 'how-to-add-your-own-cart-edit');
await dlg.locator('#ce-label').fill('Top of the hour ID');
await dlg.locator('#ce-key').selectOption(String(MOVE));
await dlg.locator('#ce-label').focus();
await page.keyboard.press('Enter');
await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)));
const moved = await until('the cart is on its new key', () => cartAt(BANK, MOVE));
check('renamed and moved, the same cart with the same sound', !!moved && !!added && moved.id === added.id && moved.file === added.file && moved.label === 'Top of the hour ID' && (await cartAt(BANK, ADD)) === null, JSON.stringify(moved));
check('the focus follows it to its new pad', (await focused()) === String(MOVE), String(await focused()));

// ---- a sound dropped on a pad ----

await drag(DROP, STING, 'news sting.wav', false);
check('a sound dragged over the wall says where it can go', await page.locator('.wall.dragging').isVisible() && ((await page.locator('.banks .right').textContent()) ?? '').includes('Drop on a pad'));
await shots('09-dragging', 100);
await drag(DROP, STING, 'news sting.wav', true);
await dlg.waitFor();
check('a sound dropped on an empty pad opens Add with the sound in hand', ((await dlg.locator('h2').textContent()) ?? '') === 'Add a cart' && (await dlg.locator('#ce-label').inputValue()) === 'news sting' && ((await dlg.locator('.where').textContent()) ?? '').startsWith(keyOf(DROP)));
check('and adds nothing by itself', (await cartAt(BANK, DROP)) === null);
await page.keyboard.press('Escape');
await page.keyboard.press('Escape');
await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)));
await drag(MOVE, SHORT, 'ident-short.wav', true);
await dlg.waitFor();
check('a sound dropped on a cart opens Edit with the sound as its replacement', ((await dlg.locator('h2').textContent()) ?? '') === 'Edit cart' && ((await dlg.locator('[data-testid="ce-file"]').textContent()) ?? '').includes('replaces the sound when you save') && (await dlg.locator('#ce-label').inputValue()) === 'Top of the hour ID');
check('and replaces nothing by itself', (await cartAt(BANK, MOVE))?.file === moved?.file);
await shots('10-replace');
await dlg.locator('#ce-label').focus();
await page.keyboard.press('Enter');
// The cart's ramp is 1.5 s and the new sound 1 s long: the station says so, and the old sound stays.
const short = await until('the station refuses a ramp longer than the new sound', async () => ((await dlg.locator('[data-testid="ce-problem"]').count()) ? dlg.locator('[data-testid="ce-problem"]').textContent() : null), 20_000);
check('a replacement shorter than the cart’s ramp is refused, and nothing is replaced', (short ?? '').includes('a ramp ends before its cart does') && (await cartAt(BANK, MOVE))?.file === moved?.file, short ?? '');
await dlg.locator('#ce-ramp').fill('');
await dlg.locator('#ce-label').focus();
await page.keyboard.press('Enter');
await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)), 30_000);
const replaced = await until('the sound is replaced', async () => {
  const c = await cartAt(BANK, MOVE);
  return c && c.file !== moved?.file ? c : null;
});
check('the sound is the new one, the name kept', !!replaced && replaced.label === 'Top of the hour ID' && replaced.duration_ms === 1000 && replaced.origin === 'ident-short.wav', JSON.stringify(replaced));

// ---- remove it ----

await editBtn(MOVE).click();
await dlg.waitFor();
await dlg.getByRole('button', { name: 'Remove cart…' }).click();
check('Remove asks first, with the focus on Keep it', (await focused()) === 'ce-keep' && ((await dlg.locator('.ask').textContent()) ?? '').includes(`Remove “Top of the hour ID” from ${keyOf(MOVE)}?`), String(await focused()));
await shots('11-remove-ask');
await page.keyboard.press('Enter');
check('⏎ keeps it', (await dlg.evaluate((d) => d.open)) && (await cartAt(BANK, MOVE)) !== null);
await dlg.getByRole('button', { name: 'Remove cart…' }).click();
await dlg.getByRole('button', { name: 'Remove cart', exact: true }).click();
await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)));
await until('the slot is empty on the station', async () => (await cartAt(BANK, MOVE)) === null);
check('the pad invites a cart again', ((await pad(MOVE).textContent()) ?? '').includes('Add a cart'));

// ---- the bank's own name and colour ----

await page.getByRole('button', { name: 'Edit bank…' }).click();
await bankDlg.waitFor();
check('the bank dialog names the bank it changes', ((await bankDlg.locator('h2').textContent()) ?? '') === 'Edit bank 1');
const bankWas = await bankDlg.locator('#be-name').inputValue();
await bankDlg.locator('#be-name').fill(`${bankWas} and stings`);
await bankDlg.getByRole('button', { name: 'Colour 9' }).click();
await shots('12-edit-bank');
await docShot(bankDlg, 'how-to-add-your-own-cart-bank');
await bankDlg.locator('#be-name').focus();
await page.keyboard.press('Enter');
await until('the dialog closes', async () => !(await bankDlg.evaluate((d) => d.open)));
const renamed = await until('the bank is renamed', async () => (await cat()).banks.find((b) => b.number === 1 && b.name === `${bankWas} and stings`));
check('the bank has its new name and colour, and its tab says so', !!renamed && renamed.colour_slot === 9 && ((await page.locator('[data-panel="carts.bank.1"]').textContent()) ?? '').includes(`${bankWas} and stings`), JSON.stringify(renamed));
await shots('13-wall-after');

// ---- the station's own bank, when the first was a show's ----

if (MODE === 'stack' && SHOW) {
  await page.keyboard.press('Control+F2');
  await openEmpty(ADD);
  await bankDlg.waitFor();
  await bankDlg.getByText('The station’s', { exact: true }).click();
  await bankDlg.locator('#be-name').fill('Idents');
  await bankDlg.locator('#be-name').focus();
  await page.keyboard.press('Enter');
  await dlg.waitFor();
  check('a station bank is made when the dialog is told so', ((await dlg.locator('.where').textContent()) ?? '').includes('in the station’s bank 2, Idents'), (await dlg.locator('.where').textContent()) ?? '');
  await dlg.locator('input[type=file]').setInputFiles(STING);
  await dlg.locator('#ce-label').focus();
  await page.keyboard.press('Enter');
  await until('the dialog closes', async () => !(await dlg.evaluate((d) => d.open)), 30_000);
  const station = await (await fetch(`${CONTROL}/api/v1/carts`, { headers: { Authorization: `Bearer ${TOKEN}` } })).json();
  const c = station.carts.find((x) => x.bank === 2 && x.slot === ADD);
  check('and the cart is the station’s, for every show', !!c && (c.show ?? null) === null && c.label === 'news sting' && station.banks.some((b) => b.number === 2 && b.name === 'Idents' && (b.show ?? null) === null), JSON.stringify(c));
}

check('the page raised no error', errors.length === 0, errors.slice(0, 3).join(' | '));
await browser.close();
const failed = results.filter((r) => !r.ok);
log(failed.length ? `${failed.length} failed, ${results.length - failed.length} passed` : `all ${results.length} passed`, `· pictures in ${OUT}`);
process.exit(failed.length ? 1 : 0);
