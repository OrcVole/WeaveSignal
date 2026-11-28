// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the player with a listener's key (keyed shows, 1.3.2; apps/web) in a real browser
// against the stand-in station (station.mjs): a key's link and the key typed in, the welcome, the
// badge, the end notice, every way a key does not work with its own words, the devices and Sign
// out, Lost your key? (and at a station that sends no email, 1.3.3), Can't hear the show?, a key
// that stops mid-listen, a station with no keys at all, and the join page's line for a guest of
// a keyed show. It checks what the page shows
// and what it sent and played, and captures each state in Dark and Light (the public pages
// follow the system) at 390×844 and 1440×900. No stack: one loopback port, and Playwright (not
// a dependency of the repository; set PLAYWRIGHT_CORE to a playwright-core directory).
//
//   cd apps/web && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/keys/shoot-player.mjs
//   SIZES=390x844 THEMES=dark ONLY=welcome,devices … node tests/keys/shoot-player.mjs
//   DOCS=1 … node tests/keys/shoot-player.mjs    # also writes the how-to's pictures
//
// Screenshots go to OUT (target/shots-keys-player unless told), named
// `<width>-<theme>-<state>.png`. What this does not prove: that a keyed stream plays from a real
// edge (the stand-in serves a short recording at whatever address the page asks for, and the
// driver checks only which address that was), and any engine but Chromium.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { KeysStation, written } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = join(ROOT, 'apps', 'web', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-keys-player');
const PORT = Number(process.env.PORT ?? 28151);
const SIZES = (process.env.SIZES ?? '390x844,1440x900').split(',').map((s) => s.split('x').map(Number));
const THEMES = (process.env.THEMES ?? 'dark,light').split(',');
const ONLY = process.env.ONLY ? new Set(process.env.ONLY.split(',')) : null;
const DOCS = process.env.DOCS ? join(ROOT, 'docs', 'src', 'assets', 'screens') : null;
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'listen', 'index.html'))) throw new Error(`no build at ${DIST} with /listen/: run \`pnpm build\` in apps/web first`);
const SOUND = readFileSync(join(ROOT, 'assets', 'phone', 'phone-words-alba.opus'));

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}
const wants = (name) => !ONLY || ONLY.has(name);
const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = join(DIST, path.endsWith('/') ? `${path}index.html` : path);
  if (!file.startsWith(DIST) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));
const ORIGIN = `http://127.0.0.1:${PORT}`;

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--autoplay-policy=no-user-gesture-required'] });

/** A window on a station's player. `kept`: a key this browser already holds. `answer`: a status
 *  to give every key route (a station that is too busy, or has keys off). `slowRedeem`: the
 *  station takes this long (ms) to answer a redeem. `refusePlay`: the browser refuses to start
 *  sound this many times (a press too long ago for it), as `play()` does. */
async function open(st, [width, height], theme, { path = '/', kept = null, answer = null, slowRedeem = 0, refusePlay = 0 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: width < 600 ? 2 : 1, isMobile: width < 600, hasTouch: width < 600, colorScheme: theme === 'light' ? 'light' : 'dark', timezoneId: 'Europe/London', locale: 'en-GB' });
  await ctx.addInitScript((k) => {
    try {
      if (k && !localStorage.getItem('weavesignal.key')) localStorage.setItem('weavesignal.key', JSON.stringify(k));
    } catch {}
  }, kept);
  if (refusePlay > 0) {
    await ctx.addInitScript((n) => {
      const play = HTMLMediaElement.prototype.play;
      let left = n;
      HTMLMediaElement.prototype.play = function () {
        if (left > 0) {
          left -= 1;
          return Promise.reject(new DOMException('play() failed because the user didn’t interact with the document first.', 'NotAllowedError'));
        }
        return play.call(this);
      };
    }, refusePlay);
  }
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(st.now * 1000));
  const w = { ctx, page, streams: [], errors: [], refuseKeyed: 0 };
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  // The streams: whichever address the page asks for is noted, and answered with a short recording:
  // the public streams at the stream's own address, the keyed ones at the station's (`/listen/k/`).
  await ctx.route(/^https:\/\/stream\.kettle\.example\/|\/listen\/k\//, (route) => {
    w.streams.push(route.request().url());
    // A station that refuses the keyed stream for a moment (a change of keyed show).
    if (w.refuseKeyed > 0 && route.request().url().includes('/listen/k/')) {
      w.refuseKeyed -= 1;
      return route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ error: 'This needs a key for the show on now.', code: 'changing_show' }) });
    }
    return route.fulfill({ status: 200, contentType: 'audio/ogg', body: SOUND });
  });
  await page.route(/\/api\//, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    // A station without a chat or a look of its own: the page draws neither.
    if (url.pathname.startsWith('/api/v1/chat') || url.pathname.startsWith('/api/v1/branding') || url.pathname.startsWith('/api/v1/call-in')) return route.fulfill({ status: 404, contentType: 'application/json', body: '{"error":"not found"}' });
    if (answer && url.pathname.startsWith('/api/v1/keyed/')) return route.fulfill({ status: answer[0], headers: answer[2] ?? {}, contentType: 'application/json', body: JSON.stringify({ error: answer[1] }) });
    const [status, body] = st.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    if (slowRedeem > 0 && url.pathname === '/api/v1/keyed/redeem') await new Promise((r) => setTimeout(r, slowRedeem));
    if (status === 204 || status === 202) return route.fulfill({ status });
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) }).catch(() => {});
  });
  await page.goto(`${ORIGIN}${path}`);
  await page.locator('#show-title').waitFor({ timeout: 15000 });
  await page.waitForFunction(() => document.querySelector('#show-title')?.textContent !== 'Loading…');
  await sleep(500);
  return w;
}

const sent = (st, re) => st.calls.filter((c) => re.test(c.path));
const KEY = 'KETM-7QMP-2D4X';
/** Kettle FM with Friday Supporters' Hour on air. */
const friday = (o = {}) => new KeysStation({ onAir: 'friday-supporters', ...o });

for (const size of SIZES) {
  for (const theme of THEMES) {
    const tag = `${size[0]}-${theme}`;
    const first = process.env.CHECKS !== '0' && size === SIZES[0] && theme === THEMES[0];
    const shot = async (page, name, { docs = null, full = true } = {}) => {
      await sleep(150);
      await page.screenshot({ path: join(OUT, `${tag}-${name}.png`), fullPage: full });
      if (DOCS && docs && size[0] === 390 && theme === 'light') await page.screenshot({ path: join(DOCS, docs), fullPage: full });
    };
    const panel = (page) => page.locator('[data-panel="player.key"]');

    // ---- a keyed show, and no key ----
    if (wants('nokey')) {
      const st = friday();
      const w = await open(st, size, theme);
      await panel(w.page).waitFor();
      if (first) {
        const all = await words(w.page.locator('.page'));
        check('a keyed show says it is for people with a key, and offers Enter a key', (await words(w.page.locator('[data-testid="keyed-line"]'))) === 'For people with a key' && (await words(panel(w.page).locator('h2'))) === 'Friday Supporters’ Hour needs a key' && (await w.page.locator('#f-key').count()) === 1);
        check('it says what someone without a key hears, and how to get one in the station’s words', all.includes('Without a key you hear what Kettle FM plays for everyone else until the public programme returns.') && (await words(w.page.locator('[data-testid="key-get"]'))).includes('Kettle FM’s supporters get a key by email') && (await w.page.locator('[data-testid="key-get"] a').getAttribute('href')) === 'https://kettle.example/supporters');
        check('the page never says token or access code', !/token|access code/i.test(all));
        await w.page.locator('.now button.primary').click();
        await sleep(600);
        check('without a key, Listen plays the public stream', w.streams.length > 0 && w.streams.every((u) => u === 'https://stream.kettle.example/programme.opus'), w.streams.join(' '));
        await w.page.locator('.now button.primary').click();
      }
      await shot(w.page, 'keyed-show-no-key', { docs: 'how-to-listen-enter-a-key.png' });
      await w.ctx.close();
    }

    // ---- the key's link: the welcome, the badge, the end notice, the keyed stream ----
    if (wants('welcome')) {
      const st = friday();
      const w = await open(st, size, theme, { path: `/listen/#key=${KEY}` });
      await w.page.locator('[data-testid="key-welcome"]').waitFor();
      if (first) {
        check('a key’s link is taken out of the address', (await w.page.evaluate(() => location.href)) === `${ORIGIN}/listen/`);
        check('the welcome says what the key opens and until when, and never the name staff gave the key', (await words(w.page.locator('[data-testid="key-welcome"]'))) === 'Welcome. This key opens Friday Supporters’ Hour and The Archive Hour, until 25 November.', await words(w.page.locator('[data-testid="key-welcome"]')));
        check('the badge is quiet and says a key is in use here, not whose', (await words(w.page.locator('[data-testid="keyed-badge"]'))) === 'Your key is on this device');
        check('the key’s name is nowhere on the page, nor kept in this browser', !(await words(w.page.locator('.page'))).includes('Isla') && !(await w.page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }))).includes('Isla'));
        check('the end is said in its last week', (await words(w.page.locator('[data-testid="key-ends"]'))).startsWith('Your key ends on 25 November at 22:00.'));
        const redeem = sent(st, /keyed\/redeem$/).at(-1)?.body;
        check('the key is redeemed for the show that is on', redeem?.key === KEY && redeem.show === 'id-friday-supporters', JSON.stringify(redeem));
        const kept = await w.page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.key')));
        check('the key is kept on this device with its device, and no token is stored', kept.text === KEY && typeof kept.device === 'string' && !JSON.stringify(kept).includes('ks1') && (await w.page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }))).includes('ks1') === false);
        await w.page.locator('.now button.primary').click();
        await sleep(600);
        const token = [...st.tokens.keys()].at(-1);
        check('with a key, Listen plays the station’s keyed stream, with the show and the token in its address', w.streams.at(-1) === `${ORIGIN}/listen/k/id-friday-supporters/${token}/programme-keyed.opus`, w.streams.join(' '));
        await w.page.locator('.now button.primary').click();
      }
      await shot(w.page, 'welcome', { docs: 'how-to-listen-welcome.png' });
      // The devices that hold the key, and Can't hear the show?
      await panel(w.page).locator('button', { hasText: 'Devices' }).click();
      await w.page.locator('[data-testid="key-devices"] li').first().waitFor();
      if (first) check('Devices lists the browsers that hold the key, and marks this one', (await words(w.page.locator('[data-testid="key-devices"]'))).includes('this device') && (await w.page.locator('[data-testid="key-devices"] li').count()) === 3, await words(w.page.locator('[data-testid="key-devices"]')));
      await shot(w.page, 'devices');
      await panel(w.page).locator('button', { hasText: 'Can’t hear the show?' }).click();
      await w.page.locator('[data-testid="key-help"]').waitFor();
      if (first) check('Can’t hear the show? starts with the browser, for a good key whose show is on', (await words(w.page.locator('[data-testid="key-help"] p').first())).startsWith('Your browser may be holding the sound back.'));
      await shot(w.page, 'help', { docs: 'how-to-listen-help.png' });
      await w.ctx.close();
    }

    // ---- the key typed in ----
    if (wants('typed')) {
      const st = friday();
      const w = await open(st, size, theme);
      await w.page.locator('#f-key').fill('ketm 7qmp 2d4x');
      await shot(w.page, 'typing');
      await w.page.locator('.enter button[type=submit]').click();
      await w.page.locator('[data-testid="key-welcome"]').waitFor();
      if (first) check('a key typed in lower case with spaces is welcomed the same', (await words(w.page.locator('[data-testid="key-welcome"]'))).startsWith('Welcome. This key opens') && sent(st, /keyed\/redeem$/).at(-1)?.body.key === 'ketm 7qmp 2d4x');
      await w.ctx.close();
    }

    // ---- `/listen/` opened without its key: the box to type or paste one ----
    if (wants('bare')) {
      const LINE = 'Have the link Kettle FM sent you? Open that link, all of it, on this device, and your key comes with it. Or type or paste the key itself here; the whole link works too.';
      // A public show on: at `/` the box is a press away; at `/listen/` it is open, with the line.
      {
        const st = new KeysStation({ onAir: 'the-long-lunch' });
        const w = await open(st, size, theme, { path: '/listen/' });
        await panel(w.page).waitFor();
        if (first) {
          check('bare /listen/: the page is the player, headed Listen with a key, and the box is open', (await words(panel(w.page).locator('h2'))) === 'Listen with a key' && (await w.page.locator('#f-key').count()) === 1 && (await w.page.locator('.enter button[type=submit]').count()) === 1, await words(panel(w.page)));
          check('bare /listen/: it keeps the line for whoever has the link, then says the box takes the key or the link', (await words(w.page.locator('[data-testid="key-entry"]'))) === LINE, await words(w.page.locator('[data-testid="key-entry"]')));
          const field = await w.page.locator('#f-key').evaluate((el) => ({ label: document.querySelector(`label[for="${el.id}"]`)?.textContent?.trim(), hint: document.getElementById((el.getAttribute('aria-describedby') ?? '').split(' ')[0])?.textContent?.trim(), type: el.getAttribute('type'), auto: el.getAttribute('autocomplete') }));
          check('bare /listen/: the box has its label and its hint, and is a plain text field', field.label === 'Your key' && field.hint === 'It looks like KETM-7QMP-2D4X. Capitals, spaces and dashes do not matter.' && field.type === 'text' && field.auto === 'off', JSON.stringify(field));
          check('bare /listen/: nothing was asked of the station about a key before one is typed', sent(st, /keyed\/redeem$/).length === 0);
          // Empty, and Enter: the box says what it wants and keeps the focus.
          await w.page.locator('#f-key').focus();
          await w.page.keyboard.press('Enter');
          await w.page.locator('#key-err').waitFor();
          check('bare /listen/: Enter on an empty box says what to type, and the box keeps the focus', (await words(w.page.locator('#key-err'))) === 'Type or paste the key you were sent.' && (await w.page.locator('#f-key').evaluate((el) => el === document.activeElement && el.getAttribute('aria-invalid') === 'true')));
        }
        await shot(w.page, 'bare-listen', { docs: 'how-to-listen-bare.png' });
        if (first) {
          // A wrong key: the player's own words, the key left in the box to put right.
          await w.page.locator('#f-key').fill('KETM-7QMP-2D4Y');
          await w.page.keyboard.press('Enter');
          await w.page.locator('[data-testid="key-problem"]').waitFor();
          const said = await words(w.page.locator('[data-testid="key-problem"]'));
          check('bare /listen/: a wrong key is refused in the player’s own words, and stays in the box', said.startsWith('That key is not recognised.') && said.includes('no 0, 1, I, L or O') && (await w.page.locator('#f-key').inputValue()) === 'KETM-7QMP-2D4Y', said);
          // The whole link pasted, and Enter: the key in it is used.
          await w.page.locator('#f-key').fill(`https://kettle.example/listen/#key=${KEY.toLowerCase()}`);
          await w.page.keyboard.press('Enter');
          await w.page.locator('[data-testid="key-welcome"]').waitFor();
          check('bare /listen/: a pasted link works as the key does, sent with Enter', sent(st, /keyed\/redeem$/).at(-1)?.body.key === KEY.toLowerCase() && (await words(w.page.locator('[data-testid="key-welcome"]'))).startsWith('Welcome. This key opens'));
          await sleep(400);
          check('bare /listen/: a key for a show that is not on starts nothing, and says its show is not on', w.streams.length === 0 && (await words(panel(w.page))).includes('Nothing on at the moment needs a key.'), w.streams.join(' '));
        }
        await w.ctx.close();
      }
      // The same page at `/` is as it was: one quiet offer, the box a press away.
      if (first) {
        const w = await open(new KeysStation({ onAir: 'the-long-lunch' }), size, theme);
        await panel(w.page).waitFor();
        check('at / the player still only asks whether you have a key', (await words(panel(w.page).locator('h2'))) === 'Have a key?' && (await w.page.locator('#f-key').count()) === 0 && (await w.page.locator('[data-testid="key-entry"]').count()) === 0);
        await w.ctx.close();
      }
      // A station with no keyed show listed, and one older than keys: `/listen/` still offers the
      // box (the link was sent by someone), and an old station says keys are not in use.
      if (first) {
        const w = await open(new KeysStation({ shows: false, onAir: 'the-long-lunch' }), size, theme, { path: '/listen/' });
        await panel(w.page).waitFor();
        check('bare /listen/ at a station that lists no keyed show still offers the box', (await w.page.locator('#f-key').count()) === 1 && (await words(panel(w.page).locator('h2'))) === 'Listen with a key');
        await w.ctx.close();
        const old = await open(new KeysStation({ keysUp: 'older', onAir: 'the-long-lunch' }), size, theme, { path: '/listen/' });
        await old.page.locator('#f-key').fill(KEY);
        await old.page.locator('.enter button[type=submit]').click();
        await old.page.locator('[data-testid="key-problem"]').waitFor();
        check('bare /listen/ at a station older than keys says keys are not in use there', (await words(old.page.locator('[data-testid="key-problem"]'))).startsWith('Keys are not in use at this station.'), await words(old.page.locator('[data-testid="key-problem"]')));
        await old.ctx.close();
      }
      // The keyed show on: the key typed at `/listen/` is welcomed and its show plays at once.
      {
        const st = friday();
        const w = await open(st, size, theme, { path: '/listen/' });
        await panel(w.page).waitFor();
        if (first) check('bare /listen/ with the keyed show on: the show’s own heading, what the public hears, and the line', (await words(panel(w.page).locator('h2'))) === 'Friday Supporters’ Hour needs a key' && (await words(panel(w.page))).includes('Without a key you hear what Kettle FM plays for everyone else') && (await words(w.page.locator('[data-testid="key-entry"]'))) === LINE);
        await shot(w.page, 'bare-listen-keyed-show');
        await w.page.locator('#f-key').fill('ketm 7qmp 2d4x');
        await w.page.keyboard.press('Enter');
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        await sleep(700);
        if (first) {
          const token = [...st.tokens.keys()].at(-1);
          check('bare /listen/: the key accepted, its show plays at once from the keyed stream', w.streams.at(-1) === `${ORIGIN}/listen/k/id-friday-supporters/${token}/programme-keyed.opus` && (await w.page.locator('.now button.primary').getAttribute('aria-pressed')) === 'true', `${w.streams.join(' ')} pressed=${await w.page.locator('.now button.primary').getAttribute('aria-pressed')}`);
          check('bare /listen/: and no word of a stream that did not start', (await w.page.locator('.now .problem').count()) === 0 && (await w.page.locator('[data-testid="key-press"]').count()) === 0);
        }
        await shot(w.page, 'bare-listen-playing');
        await w.ctx.close();
      }
      // Someone who did open the link, all of it, is never asked for the link while the station
      // is still answering; nor is someone whose key this browser kept.
      if (first) {
        for (const [what, o] of [['the link', { path: `/listen/#key=${KEY}` }], ['a kept key', { path: '/listen/', kept: { text: KEY, device: null } }]]) {
          const st = friday();
          const w = await open(st, size, theme, { ...o, slowRedeem: 2500 });
          check(`/listen/ with ${what}, the station slow to answer: Checking your key…, with no box and no line about the link meanwhile`, sent(st, /keyed\/redeem$/).length === 1 && (await w.page.locator('[data-testid="key-welcome"]').count()) === 0 && (await words(w.page.locator('[data-testid="key-checking"]'))) === 'Checking your key…' && (await w.page.locator('#f-key').count()) === 0 && (await w.page.locator('[data-testid="key-entry"]').count()) === 0 && !(await words(panel(w.page))).includes('Enter a key'), await words(panel(w.page)));
          await w.page.locator('[data-testid="key-welcome"]').waitFor();
          check(`/listen/ with ${what}: then the welcome, and still no line about the link`, (await w.page.locator('[data-testid="key-entry"]').count()) === 0 && (await w.page.locator('#f-key').count()) === 0);
          await w.ctx.close();
        }
      }
      // A browser that will not start the sound after the key is accepted: no alarm, the page
      // says what to press, and that press plays the show.
      if (first) {
        const st = friday();
        const w = await open(st, size, theme, { path: '/listen/', refusePlay: 1 });
        await w.page.locator('#f-key').fill(KEY);
        await w.page.keyboard.press('Enter');
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        await w.page.locator('[data-testid="key-press"]').waitFor({ timeout: 5000 }).catch(() => {});
        check('bare /listen/, the browser holding the sound back: the page says what to press, calmly', (await w.page.locator('[data-testid="key-press"]').count()) === 1 && (await words(w.page.locator('[data-testid="key-press"]'))) === 'Your key is accepted. Press Listen live to hear Friday Supporters’ Hour.' && (await w.page.locator('.now .problem').count()) === 0 && (await w.page.locator('.now button.primary').getAttribute('aria-pressed')) === 'false', await words(w.page.locator('.now')));
        await shot(w.page, 'bare-listen-press');
        await w.page.locator('.now button.primary').click();
        await sleep(700);
        const token = [...st.tokens.keys()].at(-1);
        check('and that press plays the show from the keyed stream, the line gone', w.streams.at(-1) === `${ORIGIN}/listen/k/id-friday-supporters/${token}/programme-keyed.opus` && (await w.page.locator('.now button.primary').getAttribute('aria-pressed')) === 'true' && (await w.page.locator('[data-testid="key-press"]').count()) === 0, w.streams.join(' '));
        await w.ctx.close();
      }
      // At `/` a key typed in the box does not start the stream by itself, as before.
      if (first) {
        const st = friday();
        const w = await open(st, size, theme);
        await w.page.locator('#f-key').fill(KEY);
        await w.page.keyboard.press('Enter');
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        await sleep(600);
        check('at / a key typed in starts nothing by itself: Listen is the listener’s press', w.streams.length === 0 && (await w.page.locator('.now button.primary').getAttribute('aria-pressed')) === 'false', w.streams.join(' '));
        await w.ctx.close();
      }
    }

    // ---- every way a key does not work ----
    if (wants('errors')) {
      const cases = [
        ['not-recognised', (st) => 'KETM-7QMP-2D4Y', 'That key is not recognised.', 'no 0, 1, I, L or O'],
        ['not-yet-valid', (st) => written(st.put({ label: 'Rosie MacIntosh', shows: ['id-friday-supporters'], all_shows: false, valid_from_ms: Date.UTC(2026, 11, 3, 19, 0) }).text), 'This key opens on 3 December at 19:00.', 'come back then'],
        ['ended', (st) => written(st.margaret.text), 'Your key ended on 15 November at 22:00.', 'Ask Kettle FM for a new one.'],
        ['paused', (st) => written(st.katie.text), 'This key is switched off for now.', 'Kettle FM has paused it'],
        ['revoked', (st) => written(st.lkeys.find((k) => k.state === 'revoked').text), 'This key was ended by Kettle FM.', 'Ask them for a new one.'],
        ['other-show', (st) => written(st.put({ label: 'Wendy Armstrong', shows: ['id-the-archive-hour'], all_shows: false }).text), 'This key does not open Friday Supporters’ Hour.', 'It is for other shows.'],
        ['people-limit', (st) => { const k = st.put({ kind: 'group', label: 'Fife Folk Club', shows: ['id-friday-supporters'], all_shows: false, max_people: 1 }); st.device(k, 'Firefox on Windows', { last: st.now * 1000 - 86_400_000 }); return written(k.text); }, 'This key has reached the number of people it is for.', 'the group is full'],
        ['listens-used-up', (st) => { const k = st.put({ label: 'Lewis Gordon', shows: ['id-friday-supporters'], all_shows: false, max_listens: 2 }); k.listens_used = 2; return written(k.text); }, 'This key’s listens are used up.', 'Ask Kettle FM for more.'],
      ];
      for (const [name, make, title, body] of cases) {
        const st = friday();
        const text = make(st);
        const w = await open(st, size, theme);
        await w.page.locator('#f-key').fill(text);
        await w.page.locator('.enter button[type=submit]').click();
        await w.page.locator('[data-testid="key-problem"]').waitFor();
        if (first) {
          const said = await words(w.page.locator('[data-testid="key-problem"]'));
          check(`${name}: the page says “${title}” and what to do`, said.startsWith(title) && said.includes(body) && !/token|access code/i.test(await words(panel(w.page))), said);
        }
        await shot(w.page, `error-${name}`, name === 'not-recognised' ? { docs: 'how-to-listen-not-recognised.png' } : {});
        await w.ctx.close();
      }
    }

    // ---- in use on too many devices: the list, and Sign out ----
    if (wants('devices')) {
      const st = friday();
      const now = st.now * 1000;
      for (const d of st.devs.get(st.isla.id)) d.last_seen_ms = now - 30_000;
      const w = await open(st, size, theme);
      await w.page.locator('#f-key').fill(KEY);
      await w.page.locator('.enter button[type=submit]').click();
      await w.page.locator('[data-testid="key-devices"] li').first().waitFor();
      if (first) {
        check('too many devices: the page counts them and lists them by name', (await words(w.page.locator('[data-testid="key-problem"]'))).startsWith('This key is already in use on 2 devices.') && (await w.page.locator('[data-testid="key-devices"] li').allInnerTexts()).join('|').includes('Firefox on Linux') && (await words(w.page.locator('[data-testid="key-devices"]'))).includes('Safari on iPhone'), await words(panel(w.page)));
      }
      await shot(w.page, 'error-too-many-devices', { docs: 'how-to-listen-devices.png' });
      if (first) {
        await w.page.locator('[data-testid="key-devices"] li', { hasText: 'Safari on iPhone' }).locator('button').click();
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        check('signing one out uses the key here at once', sent(st, /devices\/end$/).at(-1)?.body.key === KEY && (await words(w.page.locator('[data-testid="key-welcome"]'))).startsWith('Welcome. This key opens'));
        await shot(w.page, 'devices-signed-out');
      }
      await w.ctx.close();
    }

    // ---- a keyed stream refused for a moment at a change of show: the page waits, quietly ----
    if (wants('cut') && first) {
      const st = friday();
      const w = await open(st, size, theme, { path: `/listen/#key=${KEY}` });
      await w.page.locator('[data-testid="key-welcome"]').waitFor();
      w.refuseKeyed = 3;
      await w.page.locator('.now button.primary').click();
      const played = await w.page
        .waitForFunction(() => {
          const a = document.querySelector('audio');
          return a && a.currentSrc.includes('/listen/k/') && !a.paused && a.currentTime > 0.2;
        }, null, { timeout: 30000 })
        .then(() => true)
        .catch(() => false);
      check('a keyed stream refused for a change of show is waited for, then played', played && w.refuseKeyed === 0 && (await w.page.locator('[data-testid="key-cut"]').count()) === 0, `${w.refuseKeyed} refusals left; ${w.streams.length} asks`);
      await w.ctx.close();
    }

    // ---- a key that stops while it is in use: never silence without words ----
    if (wants('cut') && first) {
      const st = friday();
      st.renewIn = 1;
      const w = await open(st, size, theme, { path: `/listen/#key=${KEY}` });
      await w.page.locator('[data-testid="key-welcome"]').waitFor();
      await w.page.locator('.now button.primary').click();
      await sleep(500);
      st.handle('POST', `/api/v1/listener-keys/${st.isla.id}/revoke`, { reason: 'Passed on' });
      await w.page.locator('[data-testid="key-cut"]').waitFor({ timeout: 15000 });
      check('a key revoked mid-listen stops the show with words, within one renewal', (await words(w.page.locator('[data-testid="key-cut"]'))).startsWith('The show has stopped here because your key stopped working.') && (await words(w.page.locator('[data-testid="key-problem"]'))).startsWith('This key was ended by Kettle FM.') && (await w.page.locator('.now button.primary').getAttribute('aria-pressed')) === 'false');
      check('and the key is let go of on this device', (await w.page.evaluate(() => localStorage.getItem('weavesignal.key'))) === null);
      await shot(w.page, 'cut-mid-listen');
      await w.ctx.close();

      // Signed out from another device: this one is told, and may take the key up again.
      const st2 = friday();
      st2.renewIn = 1;
      const w2 = await open(st2, size, theme, { path: `/listen/#key=${KEY}` });
      await w2.page.locator('[data-testid="key-welcome"]').waitFor();
      const mine = st2.devs.get(st2.isla.id).at(-1);
      st2.handle('POST', '/api/v1/keyed/devices/end', { key: KEY, device: mine.id });
      await w2.page.locator('[data-testid="key-problem"][data-code="device_signed_out"]').waitFor({ timeout: 15000 });
      check('a device signed out is told so, and offered the key again', (await words(w2.page.locator('[data-testid="key-problem"]'))).startsWith('This device was signed out of the key.') && (await panel(w2.page).locator('button', { hasText: 'Use the key here again' }).count()) === 1);
      await shot(w2.page, 'error-device-signed-out');
      await panel(w2.page).locator('button', { hasText: 'Use the key here again' }).click();
      await w2.page.locator('[data-testid="key-welcome"]').waitFor();
      check('and taking it up again works', true);
      await w2.ctx.close();
    }

    // ---- lost your key? ----
    if (wants('lost')) {
      const st = friday();
      const w = await open(st, size, theme);
      await panel(w.page).locator('button', { hasText: 'Lost your key?' }).click();
      await w.page.locator('#f-lost').fill('isla.fraser@mail.example');
      await shot(w.page, 'lost');
      await w.page.locator('.lost button[type=submit]').click();
      await w.page.locator('[data-testid="key-lost"] [role=status]').waitFor();
      const answer = await words(w.page.locator('[data-testid="key-lost"] [role=status]'));
      if (first) {
        check('Lost your key? sends the address and answers without saying whether one matched', sent(st, /keyed\/lost$/).at(-1)?.body.email === 'isla.fraser@mail.example' && answer.startsWith('If Kettle FM has a key for that address, the key is on its way there.'), answer);
        const st2 = friday();
        const w2 = await open(st2, size, theme);
        await panel(w2.page).locator('button', { hasText: 'Lost your key?' }).click();
        await w2.page.locator('#f-lost').fill('nobody@mail.example');
        await w2.page.locator('.lost button[type=submit]').click();
        await w2.page.locator('[data-testid="key-lost"] [role=status]').waitFor();
        check('and says the very same for an address with no key', (await words(w2.page.locator('[data-testid="key-lost"] [role=status]'))) === answer);
        await w2.ctx.close();
      }
      await shot(w.page, 'lost-sent', { docs: 'how-to-listen-lost.png' });
      await w.ctx.close();

      // A station that sends no email (1.3.2; the pictures 1.3.3): the words say so before the
      // address is typed, the button asks and does not promise to send, and the answer says a
      // person at the station sends the key.
      {
        const nm = friday({ mail: false });
        const n = await open(nm, size, theme);
        await panel(n.page).locator('button', { hasText: 'Lost your key?' }).click();
        await n.page.locator('#f-lost').fill('isla.fraser@mail.example');
        if (first) check('no mail: the words under the field say the station does not send email by itself, and the button asks', (await words(n.page.locator('#lost-hint'))) === 'The one Kettle FM has for you. Kettle FM does not send email by itself: if a key was made with that address, its staff see your request and send the key to you again.' && (await words(n.page.locator('.lost button[type=submit]'))) === 'Ask for my key again', `${await words(n.page.locator('#lost-hint'))} / ${await words(n.page.locator('.lost button[type=submit]'))}`);
        await shot(n.page, 'lost-no-mail');
        await n.page.locator('.lost button[type=submit]').click();
        await n.page.locator('[data-testid="key-lost"] [role=status]').waitFor();
        if (first) {
          const said = await words(n.page.locator('[data-testid="key-lost"]'));
          check('no mail: the answer says its staff can see the request and send the key, never that an email is on its way', said.startsWith('Asked. If Kettle FM has a key for that address, its staff can see your request and will send the key to you again.') && said.includes('Nothing is emailed by itself here, so it may take a while: a person at Kettle FM sends it.') && !/on its way|inbox/i.test(said) && (await n.page.locator('[data-testid="key-lost"] [role=status]').getAttribute('data-mail')) === 'false', said);
          check('no mail: the request reaches the station all the same, to be noted on the key', sent(nm, /keyed\/lost$/).at(-1)?.body.email === 'isla.fraser@mail.example' && nm.lostAsked.length === 1);
        }
        await shot(n.page, 'lost-no-mail-asked', { docs: 'how-to-listen-lost-no-mail.png' });
        await n.ctx.close();
      }
    }

    // ---- the quieter states ----
    if (wants('states')) {
      // A public show on, at a station with keyed shows: one quiet offer.
      {
        const w = await open(new KeysStation({ onAir: 'the-long-lunch' }), size, theme);
        await panel(w.page).waitFor();
        if (first) check('with a public show on, the player only asks whether you have a key', (await words(panel(w.page).locator('h2'))) === 'Have a key?' && (await w.page.locator('#f-key').count()) === 0 && (await w.page.locator('[data-testid="keyed-line"]').count()) === 0);
        await shot(w.page, 'public-show');
        await w.ctx.close();
      }
      // A key kept from last time, and a public show on.
      {
        const st = new KeysStation({ onAir: 'the-long-lunch' });
        const w = await open(st, size, theme, { kept: { text: KEY, device: null } });
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        if (first) {
          check('a kept key is used again without being typed, and says nothing on now needs it', (await words(panel(w.page))).includes('Nothing on at the moment needs a key.') && (await words(w.page.locator('[data-testid="keyed-badge"]'))) === 'Your key is on this device');
          await w.page.locator('.now button.primary').click();
          await sleep(500);
          check('and a public show plays from the public stream, key or not', w.streams.at(-1) === 'https://stream.kettle.example/programme.opus', w.streams.join(' '));
          await w.page.locator('.now button.primary').click();
          await panel(w.page).locator('button', { hasText: 'Remove the key from this device' }).click();
          await sleep(200);
          check('Remove the key from this device forgets it here', (await w.page.evaluate(() => localStorage.getItem('weavesignal.key'))) === null && (await w.page.locator('[data-testid="keyed-badge"]').count()) === 0);
        }
        await shot(w.page, 'kept-key-public-show');
        await w.ctx.close();
      }
      // Nothing on air, a key held.
      {
        const st = new KeysStation();
        const w = await open(st, size, theme, { kept: { text: KEY, device: null } });
        await w.page.locator('[data-testid="key-welcome"]').waitFor();
        await panel(w.page).locator('button', { hasText: 'Can’t hear the show?' }).click();
        if (first) check('off air, Can’t hear the show? says the station is not on air yet', (await words(w.page.locator('[data-testid="key-help"] p').first())).startsWith('Kettle FM is not on air yet.'));
        await shot(w.page, 'help-off-air');
        await w.ctx.close();
      }
      // A station with no keyed show and no key kept, and one older than keys: nothing about keys.
      if (first) {
        for (const [what, st] of [['a station with no keyed show', new KeysStation({ shows: false, onAir: 'the-long-lunch' })], ['a station older than 1.3.2', new KeysStation({ keysUp: 'older', onAir: 'the-long-lunch' })]]) {
          const w = await open(st, size, theme);
          await sleep(500);
          check(`${what} shows nothing about keys`, (await panel(w.page).count()) === 0 && !/\bkey\b/i.test(await words(w.page.locator('.page'))));
          await w.ctx.close();
        }
      }
      // A station too busy, and one with keys off.
      {
        const w = await open(friday(), size, theme, { answer: [429, 'Too many requests.', { 'Retry-After': '90' }] });
        await w.page.locator('#f-key').fill(KEY);
        await w.page.locator('.enter button[type=submit]').click();
        await w.page.locator('[data-testid="key-problem"]').waitFor();
        if (first) check('too many tries: the page says how long to wait', (await words(w.page.locator('[data-testid="key-problem"]'))) === 'Too many tries from this connection. Wait 2 minutes and try again.', await words(w.page.locator('[data-testid="key-problem"]')));
        await shot(w.page, 'error-slow-down');
        await w.ctx.close();
      }
      if (first) {
        const w = await open(friday(), size, theme, { answer: [503, 'Keys are kept in the database.'] });
        await w.page.locator('#f-key').fill(KEY);
        await w.page.locator('.enter button[type=submit]').click();
        await w.page.locator('[data-testid="key-problem"]').waitFor();
        check('keys off at the station: said plainly, without the station’s own sentence', (await words(w.page.locator('[data-testid="key-problem"]'))).startsWith('Keys are not in use at this station.') && !(await words(panel(w.page))).includes('database'));
        await shot(w.page, 'error-not-set-up');
        await w.ctx.close();
      }
    }

    // ---- the join page, for a guest of a keyed show ----
    if (wants('join')) {
      const st = friday();
      st.inviteFor = 'friday-supporters';
      const ctx = await browser.newContext({ viewport: { width: size[0], height: size[1] }, deviceScaleFactor: size[0] < 600 ? 2 : 1, colorScheme: theme === 'light' ? 'light' : 'dark', timezoneId: 'Europe/London', permissions: ['microphone'] });
      const page = await ctx.newPage();
      await page.clock.setFixedTime(new Date(st.now * 1000));
      await page.route(/\/api\//, async (route) => {
        const req = route.request();
        const url = new URL(req.url());
        if (url.pathname.startsWith('/api/v1/chat') || url.pathname.startsWith('/api/v1/branding') || url.pathname.startsWith('/api/v1/pass/')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
        const [status, body] = st.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
        await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
      });
      await page.goto(`${ORIGIN}/join/#rsp1-stand-in-pass`);
      await page.locator('[data-testid="join-keyed"]').waitFor({ timeout: 15000 });
      if (first) check('a guest of a keyed show is told who hears it, and that they need no key', (await words(page.locator('[data-testid="join-keyed"]'))).startsWith('A show for people with a key. Only listeners who were given a key hear Friday Supporters’ Hour') && (await words(page.locator('[data-testid="join-keyed"]'))).includes('you need no key'));
      await sleep(150);
      await page.screenshot({ path: join(OUT, `${tag}-join-keyed.png`), fullPage: true });
      await ctx.close();
      if (first) {
        const st2 = new KeysStation({ onAir: 'the-long-lunch' });
        st2.inviteFor = 'the-long-lunch';
        const ctx2 = await browser.newContext({ viewport: { width: size[0], height: size[1] }, timezoneId: 'Europe/London' });
        const p2 = await ctx2.newPage();
        await p2.route(/\/api\//, async (route) => {
          const req = route.request();
          const url = new URL(req.url());
          if (url.pathname.startsWith('/api/v1/chat') || url.pathname.startsWith('/api/v1/branding') || url.pathname.startsWith('/api/v1/pass/')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
          const [status, body] = st2.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
          await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
        });
        await p2.goto(`${ORIGIN}/join/#rsp1-stand-in-pass`);
        await p2.locator('#join-h').waitFor();
        await sleep(800);
        check('a guest of a public show sees no such line', (await p2.locator('[data-testid="join-keyed"]').count()) === 0);
        await ctx2.close();
      }
    }
  }
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
