// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the studio's 1.2 screens (PLAN 17j) in a real browser against the stand-in station
// (station.mjs and, for the phone line and the presenter kit, station-12.mjs), checks each
// against what the studio sent, and captures them: the handover window on the desk's clock with
// the heads-up and the seat notice, the rota, the rules' window; the pull source and the WHIP
// details; the phone line in the screener and in Settings; the bridge and the presenter kit; and
// the desk's Picture and Callers at 1,440 by 900. Like shoot.mjs it needs no stack and no port
// but its own loopback one, and so proves the studio half alone.
//
//   cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/hosts/shoot-12.mjs
//   WIDTH=2560 HEIGHT=1440 TEXT=1.2 … node tests/hosts/shoot-12.mjs
//   ONLY=window,rota … node tests/hosts/shoot-12.mjs        # some sections only
//
// Screenshots go to OUT (target/shots-12 unless told), named for the window's width. The clock
// is fixed at Friday 20 November 2026, 13:52 in Europe/London, as in shoot.mjs.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { HostStation } from './station.mjs';
import { Station12 } from './station-12.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = process.env.DIST ?? join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-12');
const PORT = Number(process.env.PORT ?? 28120);
const SIZE = { width: Number(process.env.WIDTH ?? 1440), height: Number(process.env.HEIGHT ?? 900), text: Number(process.env.TEXT ?? 1) };
const TAG = `${SIZE.width}${SIZE.text === 1 ? '' : `-text${Math.round(SIZE.text * 100)}`}`;
const ONLY = (process.env.ONLY ?? '').split(',').filter(Boolean);
const want = (name) => !ONLY.length || ONLY.includes(name);
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/studio first`);

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = join(DIST, path === '/' ? 'index.html' : path);
  if (!file.startsWith(DIST) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

// A made-up microphone, so Record here can be run (the phone line's own recording).
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] });

/** A window on a station: every /api and /health request is the stand-in's to answer. `extra`
 *  answers first, for routes the stand-in station does not have (`(method, path, body)` giving
 *  `[status, body]` or nothing). */
async function open(station, { theme = 'dark', settings = false, schedule = false, screener = false, query = '', extra = null, init = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: SIZE.width, height: SIZE.height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
  await ctx.grantPermissions(['clipboard-read', 'clipboard-write']);
  await ctx.addInitScript(([t, z]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', String(z));
    } catch {}
    window.__opened = [];
    window.open = (url) => (window.__opened.push(String(url)), null);
  }, [theme, SIZE.text]);
  if (init) await ctx.addInitScript(init.fn, init.arg);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(station.now * 1000));
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.route(/\/(api\/|health)/, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (url.pathname === '/api/v1/studio/events') {
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: `data: ${JSON.stringify(station.studioView())}\n\n` });
    }
    if (url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    let body;
    try {
      body = req.postData() ? JSON.parse(req.postData()) : undefined;
    } catch {
      body = req.postDataBuffer();
    }
    const answer = extra?.(req.method(), url.pathname + url.search, body, req) ?? station.handle(req.method(), url.pathname + url.search, body);
    const [status, out] = answer;
    if (status === 204) return route.fulfill({ status });
    // A shipped voice's recording comes as sound, not JSON.
    if (Buffer.isBuffer(out)) return route.fulfill({ status, contentType: 'audio/ogg', body: out });
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(out) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/${query}`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  if (settings) {
    await page.locator('button.settings-tab').click();
    await page.locator('#settings-h').waitFor();
    await sleep(700);
  } else if (schedule) {
    await page.keyboard.press('Control+5');
    await page.waitForSelector('.schedule .ec', { timeout: 15000 });
    await sleep(800);
  } else if (screener) {
    await page.keyboard.press('Control+2');
    await page.locator('[data-panel="screener.queue"]').waitFor({ timeout: 15000 });
    await sleep(800);
  } else {
    await page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 15000 });
    await sleep(1300);
  }
  return { ctx, page };
}

const shot = async (page, name) => {
  await sleep(250);
  await page.screenshot({ path: join(OUT, `${TAG}-${name}.png`) });
  log('shot', `${TAG}-${name}`);
};
const shotOf = async (page, loc, name) => {
  await sleep(250);
  await loc.screenshot({ path: join(OUT, `${TAG}-${name}.png`) });
  log('shot', `${TAG}-${name}`);
};
/** Nothing in the panel is wider than the panel: no text is cut or pushed out of it. */
async function fits(loc, what) {
  const over = await loc.evaluate((el) => {
    const out = [];
    const box = el.getBoundingClientRect();
    for (const x of el.querySelectorAll('button, input, select, textarea, .state, h2, h3, th, td, li, code')) {
      const r = x.getBoundingClientRect();
      if (r.width && (r.right > box.right + 1 || r.left < box.left - 1)) out.push(`${x.tagName.toLowerCase()} “${(x.textContent || x.value || '').trim().slice(0, 30)}”`);
    }
    return out;
  });
  check(`${what}: nothing runs out of its panel`, over.length === 0, over.join(', '));
}
/** The page itself is no wider than the window. */
async function noSideScroll(page, what) {
  const w = await page.evaluate(() => [document.scrollingElement.scrollWidth, window.innerWidth]);
  check(`${what}: the page is no wider than the window`, w[0] <= w[1] + 1, w.join(' > '));
}
/** Every control in the panel that takes a press can be reached with Tab and says what it is. */
async function named(loc, what) {
  const bad = await loc.evaluate((el) =>
    [...el.querySelectorAll('button, input, select, textarea, a[href]')]
      .filter((x) => x.getClientRects().length && !x.closest('[hidden]'))
      .filter((x) => {
        const label = x.getAttribute('aria-label') || x.getAttribute('aria-labelledby') || (x.id && el.ownerDocument.querySelector(`label[for="${CSS.escape(x.id)}"]`)) || x.closest('label') || (x.tagName === 'BUTTON' || x.tagName === 'A' ? (x.textContent || '').trim() : '');
        return !label || x.tabIndex < 0;
      })
      .map((x) => `${x.tagName.toLowerCase()}#${x.id || '?'} “${(x.textContent || '').trim().slice(0, 24)}”`),
  );
  check(`${what}: every control has a name and a place in the Tab order`, bad.length === 0, bad.join(', '));
}
const calls = (station, method, re) => station.calls.filter((c) => c.method === method && re.test(c.path));
const tile = (page, n) => page.locator(`[data-panel="desk.tile.${n}"]`);
const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();
const toastOf = (page) => page.locator('[data-panel="desk.toast"]');
const win = (page) => page.locator('[data-testid="handover-window"]');
/** The station's clock moves on, and the window's with it; the next view follows within a second. */
async function tick(page, st, seconds = 0) {
  st.now += seconds;
  await page.clock.setFixedTime(new Date(st.now * 1000));
  await sleep(1400);
}
const THEMES = ['dark', 'light', 'night'];

/** Kettle FM at 13:52 as shoot.mjs has it: Duncan presenting The Long Lunch with a guest, Finlay
 *  standing by over SRT for Steeped at 14:00, Heather down for Kettle Drive at 16:00. */
function withHosts(o = {}) {
  const st = new HostStation(o);
  st.join('duncan', 'Duncan Boyd', 'presenter');
  st.join('eilidh', 'Eilidh Fraser', 'guest', { on_air: true, lufs: -24.8 });
  const finlay = st.give('steeped', 'Finlay Grant', 'srt', { made_days_ago: 21, staff_id: 'a-finlay' });
  const heather = st.give('kettle-drive', 'Heather Douglas', 'icecast', { made_days_ago: 40, last_used_days_ago: 1 });
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo');
  return { st, finlay, heather };
}

// ---- 1. The handover window on the desk's clock, the heads-up and the seat notice ----
if (want('window')) {
  const { st, finlay, heather } = withHosts({ step: 'full' });
  const t0 = st.now;
  const { ctx, page } = await open(st);
  check('before the window opens the clock says nothing of who is next', (await win(page).count()) === 0);
  check('the post line is there as it was', (await words(page.locator('[data-panel="desk.clock"] .post'))).startsWith('Steeped · the post 14:00:00'));

  // 13:55: the window opens. Finlay has stood by since 13:47.
  await tick(page, st, 180);
  const rows = win(page).locator('li');
  check('at 13:55 the window opens with one row: who is next, and ready', (await rows.count()) === 1 && (await rows.first().getAttribute('data-win')) === 'next' && (await rows.first().getAttribute('data-ready')) === 'ready', await words(win(page)));
  check('the row names the host, where they come from, and READY as a chip with its word', /^Next Finlay Grant Ready own studio$/i.test(await words(rows.first())), await words(rows.first()));
  check('the show and its time are not said twice: the post line above has them', !(await words(rows.first())).includes('Steeped'));
  check('the row’s tooltip is the whole sentence', /^Next: Finlay Grant, Steeped · 14:00, own studio, ready\.$/.test((await rows.first().getAttribute('title')) ?? ''), await rows.first().getAttribute('title'));
  await fits(page.locator('[data-panel="desk.clock"]'), 'the clock with the window open');

  // The heads-up, once: a calm toast, and the host's own tile says they are on next.
  st.headsUp();
  await tick(page, st, 1);
  const toast = toastOf(page);
  check('the heads-up is a calm toast with the station’s words', (await toast.count()) === 1 && (await words(toast)).startsWith('Next: Finlay Grant, Steeped') && (await toast.getAttribute('role')) === 'status' && (await toast.getAttribute('class')).includes('plain'), (await toast.count()) ? await words(toast) : 'no toast');
  const seat = tile(page, 5).locator('[data-testid="seat-next"]');
  check('the host’s own tile says they are on next, and when they were told', /^On next · Steeped at 14:00 · told 13:55$/.test(await words(seat)), (await seat.count()) ? await words(seat) : 'no seat notice');
  check('the row says the heads-up was given', (await words(rows.first())).endsWith('own studio · heads-up 13:55'), await words(rows.first()));
  await shot(page, 'W1-window-ready-heads-up');
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), 'W1b-clock-ready');
  await shotOf(page, tile(page, 5), 'W1c-seat-notice');
  check('a second heads-up is not given for the same airing', st.headsUp() === null);
  await tick(page, st, 20);
  check('the toast goes by itself after 20 seconds; the seat notice stays until the post', (await toastOf(page).count()) === 0 && (await seat.count()) === 1);

  // Not here yet, then connecting.
  st.drop(finlay.key_id);
  await tick(page, st, 1);
  check('a host who has not connected is NOT HERE YET', (await rows.first().getAttribute('data-ready')) === 'not_here' && /Not here yet/i.test(await words(rows.first())), await words(rows.first()));
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), 'W2-clock-not-here-yet');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 2 });
  await tick(page, st, 1);
  check('and CONNECTING once their software is heard, before it is steady', (await rows.first().getAttribute('data-ready')) === 'connecting' && /Connecting/i.test(await words(rows.first())), await words(rows.first()));
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), 'W3-clock-connecting');

  // The post comes and Duncan keeps talking: the grace counts above, the window says who is over.
  await tick(page, st, t0 + 8 * 60 + 30 - st.now);
  const over = win(page).locator('li[data-win="overrun"]');
  check('past the post with the outgoing host on air, the window says who is running over', (await over.count()) === 1 && /^Over Duncan Steeped was due 14:00 · Finlay is ready$/i.test(await words(over)), await words(win(page)));
  check('the grace keeps the line above, with Hold', (await page.locator('[data-panel="desk.clock"] .post[data-post="grace"]').count()) === 1 && (await page.locator('[data-panel="desk.clock"] .post .hold').count()) === 1);
  check('nobody is told on a tile once the post has come', (await page.locator('[data-testid="seat-next"]').count()) === 0);
  await shot(page, 'W4-window-overrun');

  // A producer holds the handover: the window stays open and says who holds it.
  await page.locator('[data-panel="desk.clock"] .post .hold').click();
  await tick(page, st, 1);
  const held = win(page).locator('li[data-win="next"]');
  check('a held handover keeps the next host in sight, ready, with who holds it', (await held.count()) === 1 && /^Next Finlay Grant Ready Steeped · 14:00 · own studio · held by shona: a press only$/i.test(await words(held)), await words(win(page)));
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), 'W5-clock-held');
  await ctx.close();

  // Both hosts: Steeped began at 14:00 and nobody came; Kettle Drive's window opens at 15:55.
  for (const theme of THEMES) {
    const b = withHosts({ step: 'full', nowMinutes: 15 * 60 + 56 });
    b.st.drop(b.finlay.key_id);
    b.st.people.find((p) => p.identity === 'duncan').on_air = false;
    b.st.people.find((p) => p.identity === 'eilidh').on_air = false;
    b.st.mode = 'auto';
    b.st.connect(b.heather.key_id, 'icecast', 'mp3 44.1 kHz stereo', { ago_s: 40 });
    const w = await open(b.st, { theme });
    const two = win(w.page).locator('li');
    if (theme === 'dark') {
      check('with the show under way still waiting and the next window open, both are said: the late one first', (await two.count()) === 2 && (await two.nth(0).getAttribute('data-win')) === 'late' && (await two.nth(1).getAttribute('data-win')) === 'next', await words(win(w.page)));
      check('the late row: who, since when, and NOT HERE YET', /^Late Finlay Grant Not here yet Steeped · since 14:00$/i.test(await words(two.nth(0))), await words(two.nth(0)));
      check('the next row: who, the show and its time, where from, and READY', /^Next Heather Douglas Ready Kettle Drive · 16:00 · own studio$/i.test(await words(two.nth(1))), await words(two.nth(1)));
      await fits(w.page.locator('[data-panel="desk.clock"]'), 'the clock with both rows');
      const clock = await w.page.locator('[data-panel="desk.clock"]').boundingBox();
      const vp = w.page.viewportSize();
      check('both rows are inside the window without scrolling', !!clock && clock.y + clock.height <= vp.height, JSON.stringify(clock));
      await noSideScroll(w.page, 'the desk with both rows');
    }
    await shot(w.page, `W6-${theme}-window-late-and-next`);
    await shotOf(w.page, w.page.locator('[data-panel="desk.clock"]'), `W6b-${theme}-clock-late-and-next`);
    await w.ctx.close();
  }

  // A station older than 1.2 sends no window: the clock is as it was.
  const old = withHosts({ step: 'full', nowMinutes: 13 * 60 + 56 });
  delete old.st.rules.window_s;
  const o = await open(old.st);
  check('a station that sends no window draws none', (await win(o.page).count()) === 0);
  await o.ctx.close();
}

// ---- 2. The rota, and the rules' window ----
if (want('rota')) {
  for (const theme of THEMES) {
    const { st } = withHosts({ step: 'full' });
    const { ctx, page } = await open(st, { theme });
    if (theme === 'dark') {
      // From the desk: the button beside Coming up goes to the Schedule tab's rota.
      await page.locator('[data-testid="desk-rota"]').click();
      await page.locator('[data-panel="calendar.rota"]').waitFor({ timeout: 8000 });
      await sleep(600);
      const rota = page.locator('[data-panel="calendar.rota"]');
      const rr = rota.locator('[data-testid="rota-row"]');
      check('Rota beside Coming up opens the rota on the Schedule tab, and asks the station', (await rr.count()) === 3 && calls(st, 'GET', /\/api\/v1\/rota$/).length >= 1, `${await rr.count()} rows`);
      check('the head says the stretch and counts the shows', (await words(rota.locator('[data-testid="rota-head"]'))) === '13:52 to 01:52 · 3 shows', await words(rota.locator('[data-testid="rota-head"]')));
      check('the one on air is first and marked', (await rr.first().getAttribute('aria-current')) === 'true' && /^12:00 – 14:00 on air now The Long Lunch Duncan Boyd in the room On air$/i.test(await words(rr.first())), await words(rr.first()));
      check('the next show: its hosts, own studio, and READY', /^14:00 – 16:00 in 8 min Steeped Finlay Grant own studio Ready$/i.test(await words(rr.nth(1))), await words(rr.nth(1)));
      check('a show hours off whose host has not connected is only Not due yet', /^16:00 – 18:00 in 2 h 08 min Kettle Drive Heather Douglas own studio Not due yet$/i.test(await words(rr.nth(2))), await words(rr.nth(2)));
      check('the Rota view’s button is pressed, and Week is not', (await page.getByRole('button', { name: 'Rota', exact: true }).getAttribute('aria-pressed')) === 'true' && (await page.getByRole('button', { name: 'Week', exact: true }).getAttribute('aria-pressed')) === 'false');
      await fits(rota, 'the rota');
      await noSideScroll(page, 'the rota');
      // W goes back to the week, R comes back.
      await page.keyboard.press('w');
      await sleep(300);
      check('W goes back to the week', (await page.locator('[data-panel="calendar.rota"]').count()) === 0 && (await page.locator('[data-panel="calendar.week"]').isVisible()));
      await page.keyboard.press('r');
      await rota.waitFor();
      check('R opens the rota from the week', await rota.isVisible());
      // A show nobody is down for.
      st.rotaNobody.add('kettle-drive');
      await sleep(5600);
      check('a show nobody is down for says so, and the head counts it', /Kettle Drive Nobody down for it — —$/.test(await words(rr.nth(2))) && (await words(rota.locator('[data-testid="rota-head"]'))).endsWith('1 with nobody down'), await words(rr.nth(2)));
    } else {
      await page.keyboard.press('Control+5');
      await page.waitForSelector('.schedule .ec', { timeout: 15000 });
      st.rotaNobody.add('kettle-drive');
      await page.keyboard.press('r');
      await page.locator('[data-testid="rota-row"]').first().waitFor();
      await sleep(400);
    }
    await shot(page, `R1-${theme}-rota`);
    await ctx.close();
  }

  // Nothing in the next twelve hours; a station that cannot read its schedule; one older than 1.2.
  {
    const st = new HostStation({ nowMinutes: 23 * 60 + 5, step: 'full' });
    const { ctx, page } = await open(st, { schedule: true });
    await page.keyboard.press('r');
    const empty = page.locator('[data-testid="rota-empty"]');
    await empty.waitFor();
    check('with nothing in the next twelve hours the rota says so, and how to fill it', (await words(empty)).startsWith('Nothing on the schedule in the next twelve hours.'), await words(empty));
    await shot(page, 'R2-rota-empty');
    await page.getByRole('button', { name: /Back to the week/ }).click();
    check('Back to the week leaves the rota', (await page.locator('[data-panel="calendar.rota"]').count()) === 0);
    await ctx.close();

    const down = withHosts({ step: 'full' }).st;
    down.rotaUp = false;
    const d = await open(down, { schedule: true });
    await d.page.keyboard.press('r');
    const alert = d.page.locator('[data-panel="calendar.rota"] [role="alert"]');
    await alert.waitFor();
    check('a rota that cannot be read says why, with Try again', (await words(alert)).startsWith('The rota could not be read.') && (await alert.getByRole('button', { name: 'Try again' }).count()) === 1, await words(alert));
    down.rotaUp = true;
    await alert.getByRole('button', { name: 'Try again' }).click();
    await d.page.locator('[data-testid="rota-row"]').first().waitFor();
    check('Try again reads it', (await d.page.locator('[data-testid="rota-row"]').count()) === 3);
    await d.ctx.close();

    const old = withHosts({ step: 'full' }).st;
    delete old.rules.window_s;
    const o = await open(old, { schedule: true });
    await o.page.keyboard.press('r');
    await o.page.locator('[data-testid="rota-absent"]').waitFor();
    check('a station older than 1.2 says it has no rota yet', (await words(o.page.locator('[data-testid="rota-absent"]'))).startsWith('This station has no rota yet.'));
    await o.ctx.close();
  }

  // Settings › Own studios › The rules: the window's length.
  {
    const { st } = withHosts({ role: 'admin' });
    const { ctx, page } = await open(st, { settings: true });
    const rules = page.locator('[data-panel="settings.hosts.rules"]');
    await rules.waitFor();
    await rules.scrollIntoViewIfNeeded();
    const sel = rules.locator('#hr-window_s');
    check('the rules have the window’s length, read from the station: 5 minutes', (await sel.inputValue()) === '300' && (await sel.locator('option').allInnerTexts()).join('|') === 'Off|5 minutes before|10 minutes before|15 minutes before|30 minutes before', (await sel.locator('option').allInnerTexts()).join('|'));
    check('no hint says a rule is not acted on yet', !/This build does not/.test(await rules.innerText()));
    await sel.selectOption('600');
    await rules.getByRole('button', { name: 'Save the rules' }).click();
    await sleep(400);
    const put = calls(st, 'PUT', /settings\/sources/).at(-1);
    check('saving sends the window with every other rule', put?.body.window_s === 600 && put.body.grace_s === 120 && st.rules.window_s === 600, JSON.stringify(put?.body));
    await sel.selectOption('0');
    await rules.getByRole('button', { name: 'Save the rules' }).click();
    await sleep(400);
    check('Off is 0', st.rules.window_s === 0);
    await fits(rules, 'the rules with the window');
    await named(rules, 'the rules');
    await shotOf(page, rules, 'R3-rules-window');
    await ctx.close();

    // A length set through the API that is none of the five is shown as it is.
    const odd = withHosts({ role: 'admin' }).st;
    odd.rules.window_s = 420;
    const w = await open(odd, { settings: true });
    const s2 = w.page.locator('#hr-window_s');
    await s2.scrollIntoViewIfNeeded();
    check('a stored length that is none of the five is shown as it is, not moved', (await s2.inputValue()) === '420' && (await s2.locator('option:checked').innerText()) === '7 min before (set through the API)', await s2.locator('option:checked').innerText());
    await w.ctx.close();

    const old = withHosts({ role: 'admin' }).st;
    delete old.rules.window_s;
    const o = await open(old, { settings: true });
    await o.page.locator('[data-panel="settings.hosts.rules"]').waitFor();
    check('a station older than 1.2 has no window to set, and the rules still save without it', (await o.page.locator('#hr-window_s').count()) === 0);
    await o.ctx.close();
  }
}

// ---- 3. Sources: a stream the station pulls, WHIP's details, and the tile's name for each way in ----
if (want('sources')) {
  const { st } = withHosts({ step: 'full' });
  const { ctx, page } = await open(st, { schedule: true });
  const openShow = async (name) => {
    if (await page.getByRole('button', { name: '‹ Shows' }).count()) await page.getByRole('button', { name: '‹ Shows' }).click();
    await page.locator('[data-panel="calendar.shows"] .showrow', { hasText: name }).first().click();
    await page.locator('aside.editor').getByRole('tab', { name: 'Details' }).click();
    await page.locator('[data-panel="calendar.show-source"]').waitFor();
  };
  const src = page.locator('[data-panel="calendar.show-source"]');
  await openShow('Kettle Drive');
  await src.scrollIntoViewIfNeeded();
  await src.getByRole('button', { name: 'A stream the station pulls' }).click();
  check('a show with a host’s key says a show comes from one place, and offers no form', (await words(src.locator('[data-testid="pull-blocked"]'))).includes('A show comes from one place') && (await src.locator('[data-testid="pull-form"]').count()) === 0);

  await openShow('The Long Lunch');
  await src.scrollIntoViewIfNeeded();
  check('Comes from offers three places', (await src.locator('.seg button').allInnerTexts()).join('|') === 'The station’s room|The host’s own studio|A stream the station pulls', (await src.locator('.seg button').allInnerTexts()).join('|'));
  await src.getByRole('button', { name: 'A stream the station pulls' }).click();
  const form = src.locator('[data-testid="pull-form"]');
  await form.waitFor();
  check('the form asks for the address and a name for the desk, which is the show’s to start with', (await page.inputValue('#ps-label')) === 'The Long Lunch' && (await page.inputValue('#ps-url')) === '');
  await fits(src, 'Comes from, with the pull’s form');
  await named(src, 'Comes from, with the pull’s form');
  await shotOf(page, src, 'S1-pull-form');
  await form.getByRole('button', { name: 'Pull this stream' }).click();
  check('an empty address is said under the field, and the station is not asked', (await words(form.locator('#ps-url-err'))).startsWith('Give the stream’s address') && calls(st, 'POST', /the-long-lunch\/keys$/).length === 0 && (await page.evaluate(() => document.activeElement?.id)) === 'ps-url');
  await page.fill('#ps-url', 'radio.example.org/live');
  await form.getByRole('button', { name: 'Pull this stream' }).click();
  check('an address with no http://, https:// or srt:// is said so', (await words(form.locator('#ps-url-err'))) === 'Start the address with http://, https:// or srt://.');
  await page.fill('#ps-url', 'https://partick.example/hls/live.m3u8');
  check('the address is read as it is typed: an HLS playlist', (await words(form.locator('[data-testid="pull-kind"]'))).startsWith('HLS (a playlist).'), await words(form.locator('[data-testid="pull-kind"]')));
  await page.fill('#ps-url', 'srt://partick.example:9000');
  check('or SRT, which the station calls', (await words(form.locator('[data-testid="pull-kind"]'))).startsWith('SRT: the station calls it.'));
  await page.fill('#ps-url', 'http://192.168.1.20:8000/live');
  await form.getByRole('button', { name: 'Pull this stream' }).click();
  await sleep(400);
  check('the station’s refusal of an address inside its own network lands under the field, in plain words', (await words(form.locator('#ps-url-err'))) === 'That address is inside the station’s own network, which the station does not open. Use the stream’s public address.', (await form.locator('#ps-url-err').count()) ? await words(form.locator('#ps-url-err')) : 'no error');
  await shotOf(page, src, 'S2-pull-refused');
  await page.fill('#ps-url', 'https://stream.partick.example/live.mp3');
  await page.fill('#ps-label', 'Radio Partick');
  await form.getByRole('button', { name: 'Pull this stream' }).click();
  const pulled = src.locator('[data-testid="pulled"]');
  await pulled.waitFor();
  const body = calls(st, 'POST', /the-long-lunch\/keys$/).at(-1)?.body;
  check('Pull this stream makes the show’s key with the address', JSON.stringify(body) === JSON.stringify({ label: 'Radio Partick', hint: 'pull', url: 'https://stream.partick.example/live.mp3' }), JSON.stringify(body));
  check('the show now reads A stream the station pulls, with its address and what kind it is', (await src.getByRole('button', { name: 'A stream the station pulls' }).getAttribute('aria-pressed')) === 'true' && (await words(pulled)).includes('Pulled by the station from https://stream.partick.example/live.mp3') && (await words(pulled)).includes('Icecast or other audio over the web.'), await words(pulled));
  check('it is in its slot and nothing has arrived: NOT CONNECTED, and until when the station keeps trying', /Not connected/i.test(await words(pulled)) && (await words(pulled)).includes('Nothing arrives from the address yet. It keeps trying until 16:00.'), await words(pulled));
  const mon = pulled.locator('[data-testid="pull-monitor"]');
  check('the monitor link is given this once, whole, with Copy', (await mon.locator('input').inputValue()).startsWith('https://kettle.example/monitor/#') && (await mon.getByRole('button', { name: 'Copy' }).count()) === 1, (await mon.count()) ? await mon.locator('input').inputValue() : 'no link');
  await fits(src, 'Comes from, pulled');
  await shotOf(page, src, 'S3-pull-made');
  await mon.getByRole('button', { name: 'Done' }).click();
  // The stream opens: it stands by on the desk like any source.
  const key = st.keys.find((k) => k.hint === 'pull');
  st.connect(key.key_id, 'pull', 'mp3 44.1 kHz stereo', { ago_s: 40 });
  await page.reload();
  await page.locator('[data-panel="studio.frame"]').waitFor();
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await openShow('The Long Lunch');
  await src.scrollIntoViewIfNeeded();
  check('once it opens: STANDBY, and what the stream sends', /Standby/i.test(await words(pulled)) && (await words(pulled)).includes('Open and standing by on the desk · mp3 44.1 kHz stereo'), await words(pulled));
  check('its key row has no Details of a way in to give, and can be ended', (await src.locator('li[data-key] button', { hasText: 'End key' }).count()) === 1);
  await shotOf(page, src, 'S4-pull-standing-by');
  await ctx.close();

  // The desk: each way in has its own name on the tile.
  for (const [way, kind, link, long] of [
    ['pull', 'Pulled stream', 'Pull · steady 0:40', 'Pulled by the station · Steeped'],
    ['vban', 'Studio PC', 'VBAN · steady 0:40', 'VBAN, on the station’s network · Steeped'],
    ['bridge', 'Own studio', 'Bridge · steady 0:40 · about 2 s behind', 'The studio app’s bridge · Steeped'],
    ['whip', 'Own studio', 'WHIP · steady 0:40', 'WHIP (sound only) · Steeped'],
  ]) {
    const d = new HostStation({ step: 'full' });
    d.join('duncan', 'Duncan Boyd', 'presenter');
    const k = d.give('steeped', way === 'pull' ? 'Radio Partick' : 'Finlay Grant', way === 'pull' ? 'pull' : way);
    d.connect(k.key_id, way, way === 'whip' ? 'opus 48 kHz stereo' : 'mp3 44.1 kHz stereo', { ago_s: 40 });
    const w = await open(d);
    const t5 = tile(w.page, 5);
    const role = t5.locator('.role');
    check(`${way}: the tile’s role line says ${kind}, and its tooltip the way in full`, (await words(role)).startsWith(kind) && ((await role.getAttribute('title')) ?? '').startsWith(long), `${await words(role)} | ${await role.getAttribute('title')}`);
    check(`${way}: the link line is “${link}”`, (await words(t5.locator('[data-testid="source-link"]'))) === link, await words(t5.locator('[data-testid="source-link"]')));
    await fits(t5, `the ${way} tile`);
    if (way === 'pull' || way === 'vban') await shotOf(w.page, t5, `S5-tile-${way}`);
    await w.ctx.close();
  }

  // WHIP's details say it is sound only, in one line.
  {
    const { st: s2 } = withHosts({ role: 'admin' });
    const w = await open(s2, { settings: true });
    const p = w.page.locator('[data-panel="settings.hosts"]');
    await p.getByRole('button', { name: 'Set up a host…' }).click();
    const setup = w.page.locator('dialog[data-panel="settings.hosts.setup"]');
    await setup.waitFor();
    await w.page.selectOption('#hk-show', 'kettle-drive');
    await w.page.fill('#hk-label', 'Isla Robertson');
    await setup.locator('input[name="hk-hint"][value="whip"]').check({ force: true });
    check('the setup’s line on WHIP says a picture OBS sends is dropped', (await words(setup)).includes('Sound only: a picture OBS sends with it is dropped.'));
    await setup.getByRole('button', { name: 'Make the key' }).click();
    const det = w.page.locator('dialog[data-panel="settings.hosts.details"]');
    await det.waitFor();
    const line = det.locator('[data-testid="whip-sound-only"]');
    check('the details say WHIP is sound only, in one line', (await words(line)) === 'Sound only. If OBS sends its picture too, the station drops it: nothing of it is kept or shown.', (await line.count()) ? await words(line) : 'no line');
    await shotOf(w.page, det, 'S6-whip-details');
    await det.getByRole('button', { name: 'I have sent them' }).click();
    await w.ctx.close();
  }
}

// ---- 4. The phone line: in the screener's queue and on a caller's card, and in Settings ----
if (want('phone')) {
  const kettle = (o = {}) => {
    const st = new Station12({ step: 'full', ...o });
    st.join('duncan', 'Duncan Boyd', 'presenter');
    return st;
  };
  // The screener, with the line off: one quiet line says so.
  {
    const st = kettle({ role: 'screener' });
    st.ring('caller-jessie', { name: 'Jessie Paterson', topic: 'The bins on Bothwell Street' });
    const { ctx, page } = await open(st, { screener: true });
    const line = page.locator('[data-testid="screener-phone"]');
    check('with no trunk set the queue says the phone line is off', (await words(line)) === 'Phone line off · no trunk is set' && (await line.getAttribute('data-phone')) === 'off', (await line.count()) ? await words(line) : 'no line');
    check('and the queue is from the player page alone', (await words(page.locator('[data-panel="screener.queue"] .head .sub'))) === 'from the player page');
    await shotOf(page, page.locator('[data-panel="screener.queue"]'), 'P1-screener-line-off');
    await ctx.close();
    const old = kettle({ role: 'screener' });
    old.phone.routes = false;
    const o = await open(old, { screener: true });
    check('a station older than 1.2 says nothing of a phone line', (await o.page.locator('[data-testid="screener-phone"]').count()) === 0);
    await o.ctx.close();
  }
  // The line open: a phone caller, a withheld number, and a caller from the player page.
  {
    const st = kettle({ role: 'screener' });
    st.phone.on = true;
    st.phone.atGate = 1;
    st.ring('phone-x7k2', { phone: { tail: '0123' }, waited_s: 140 });
    st.ring('phone-q9m4', { phone: { withheld: true }, consent: 'not_asked', waited_s: 80 });
    st.ring('caller-jessie', { name: 'Jessie Paterson', topic: 'The bins on Bothwell Street', waited_s: 40 });
    const { ctx, page } = await open(st, { screener: true });
    const q = page.locator('[data-panel="screener.queue"]');
    const rows = q.locator('.qrow');
    check('the queue says callers come from the player page and the phone line', (await words(q.locator('.head .sub'))) === 'from the player page and the phone line');
    check('a phone caller is the last four digits, never a key or a number', (await words(rows.nth(0).locator('.name'))) === 'Phone ···0123' && !(await q.innerText()).includes('phone-x7k2'), await words(rows.nth(0)));
    check('a withheld number says so', (await words(rows.nth(1).locator('.name'))) === 'Withheld number' && !(await q.innerText()).includes('phone-q9m4'));
    check('the line’s state is under the queue: open, its lines, who is hearing the words', /^Phone line open · 8 lines · 1 hearing the words · 2 in the queue$/.test(await words(page.locator('[data-testid="screener-phone"]'))), await words(page.locator('[data-testid="screener-phone"]')));
    const card = page.locator('[data-panel="screener.screening"]');
    check('the card’s head and Calling from say the line, and the consent was a key on the phone', (await words(card.locator('h2 .nm'))) === 'Phone ···0123' && (await words(card.locator('[data-testid="caller-phone"]'))) === 'Phone ···0123' && /with a key on the phone/.test(await words(card.locator('[data-consent="given"]'))), await words(card));
    check('a caller with no name yet is asked for one', (await card.locator('#f-name').getAttribute('placeholder')) === 'Ask their name');
    const ban = card.locator('[data-testid="ban-number"]');
    check('their card offers Ban this number…', (await words(ban)) === 'Ban this number…');
    await fits(card, 'a phone caller’s card');
    await shot(page, 'P2-screener-phone-caller');
    await ban.click();
    const dlg = page.locator('dialog[data-panel="screener.ban-number"]');
    await dlg.waitFor();
    check('the ban asks why and for how long, and says what the station keeps', (await words(dlg.locator('h2'))) === 'Ban Phone ···0123?' && (await words(dlg)).includes('never the number') && (await page.evaluate(() => document.activeElement?.id)) === 'bn-reason');
    await dlg.getByRole('button', { name: 'Ban this number' }).click();
    check('a ban without a reason is said under the field, and nothing is sent', (await words(dlg.locator('#bn-reason-err'))).startsWith('Say why') && calls(st, 'POST', /phone\/bans$/).length === 0);
    await shot(page, 'P3-ban-number');
    await page.fill('#bn-reason', 'Abusive to the screener');
    await dlg.getByRole('button', { name: 'Ban this number' }).click();
    await sleep(500);
    const sent = calls(st, 'POST', /phone\/bans$/).at(-1)?.body;
    check('Ban this number sends the caller’s key in the queue, the reason and 30 days', JSON.stringify(sent) === JSON.stringify({ identity: 'phone-x7k2', reason: 'Abusive to the screener', days: 30 }), JSON.stringify(sent));
    check('and says it is banned, and that dropping is its own act', (await words(page.locator('[data-panel="screener.toast"]'))).includes('Phone ···0123 is banned'), (await page.locator('[data-panel="screener.toast"]').count()) ? await words(page.locator('[data-panel="screener.toast"]')) : 'no toast');
    // The withheld caller: the ban is drawn off, in the words the station uses.
    await rows.nth(1).click();
    await sleep(300);
    const no = card.locator('[data-testid="ban-number"]');
    check('a withheld number cannot be banned: the button says so and is off, with the reason', (await words(no)) === 'Cannot ban a withheld number' && (await no.getAttribute('aria-disabled')) === 'true' && ((await no.getAttribute('title')) ?? '').startsWith('Every caller who withholds their number looks the same'));
    check('and their consent says the words were not asked', /not asked: the station's phone words are off/.test(await words(card.locator('[data-consent="given"]'))), await words(card.locator('.chks')));
    await shotOf(page, card, 'P4-card-withheld');
    await rows.nth(2).click();
    await sleep(300);
    check('a caller from the player page is as before: Signed in as, and no ban of a number', /signed in as/i.test(await words(card)) && (await card.locator('[data-testid="ban-number"]').count()) === 0);
    await ctx.close();
  }

  // Settings › Phone line.
  const phonePanel = (page) => page.locator('[data-panel="settings.phone"]');
  for (const theme of THEMES) {
    const st = kettle({ role: 'admin' });
    const { ctx, page } = await open(st, { settings: true, theme });
    const p = phonePanel(page);
    await p.waitFor();
    await p.scrollIntoViewIfNeeded();
    if (theme === 'dark') {
      check('with no trunk the panel says the line is off, and what a trunk is', /^Off$/i.test(await words(p.locator('.ph .state'))) && (await words(p.locator('[data-testid="phone-line"]'))).endsWith('The phone line is off until a trunk is set.') && (await words(p.locator('[data-testid="phone-fix"]'))).startsWith('A trunk is the phone provider’s connection'), await words(p.locator('.ph')));
      const sw = p.locator('[data-testid="phone-words-switch"]');
      check('the words’ switch shows the setting and is not this screen’s to change', (await sw.getAttribute('role')) === 'switch' && (await sw.getAttribute('aria-disabled')) === 'true' && (await words(p.locator('.sw-state'))) === 'Off with the line');
      await sw.click({ force: true });
      check('pressing it says where the setting is, by name', (await words(p.locator('.part').first())).includes('Its name is PHONE_CONSENT_WORDS.'));
      const tip = page.locator('#ph-words-tip');
      const info = p.getByRole('button', { name: 'Does my station need the consent words?' });
      const tipWords = ((await tip.textContent()) ?? '').trim();
      check('the tooltip beside it says whether a station needs the words', tipWords === 'An internet-only station may not need it. Stations that broadcast over radio frequencies, or that record calls, often should; check your own requirements.' && (await info.getAttribute('aria-describedby')) === 'ph-words-tip' && (await tip.getAttribute('role')) === 'tooltip', tipWords);
      await info.hover();
      await sleep(250);
      const hovered = await tip.evaluate((el) => getComputedStyle(el).visibility);
      await page.mouse.move(5, 5);
      await sleep(250);
      // By the keyboard: Tab from the switch lands on it, and its words show.
      await sw.focus();
      await page.keyboard.press('Tab');
      await sleep(250);
      const focused = (await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))) === 'Does my station need the consent words?';
      check('it shows on hover, and Tab reaches its button and shows it too', hovered === 'visible' && focused && (await tip.evaluate((el) => getComputedStyle(el).visibility)) === 'visible', `${hovered} ${focused} ${await tip.evaluate((el) => getComputedStyle(el).visibility)}`);
      await shotOf(page, p, 'P5b-words-tooltip');
      check('nothing is offered to record until the line is on', (await words(p.locator('[data-testid="phone-recording"]'))).startsWith('Nothing to choose yet.') && (await p.locator('input[type="file"]').count()) === 0);
      check('the station’s takedown contact is there, from its settings file', (await page.inputValue('#ph-contact')) === 'calls@kettle.example' && (await words(p)).includes('It comes from the station’s settings file now.'));
      check('no number is banned, and no form is offered with the line off', (await p.locator('[data-testid="phone-bans-none"]').count()) === 1 && (await p.locator('#ph-number').count()) === 0);
      await fits(p, 'the phone line, off');
      await named(p, 'the phone line, off');
    }
    await shotOf(page, p, `P5-${theme}-settings-line-off`);
    await ctx.close();
  }
  {
    // On, with the words on and the voices that come with WeaveSignal missing from the
    // installation (an image built without them).
    const st = kettle({ role: 'admin' });
    st.phone.on = true;
    st.phone.hasDefault = false;
    const { ctx, page } = await open(st, { settings: true });
    const p = phonePanel(page);
    await p.waitFor();
    await p.scrollIntoViewIfNeeded();
    check('on with nothing to play, the line is CLOSED and says what puts it right', /^Closed$/i.test(await words(p.locator('.ph .state'))) && (await words(p.locator('[data-testid="phone-fix"]'))).startsWith('Record the station’s own words below, or install the station again'), await words(p.locator('.ph')));
    const voices = p.locator('[data-testid="phone-voices"]');
    check('the recording is a choice of three: Alba, the default; Edinburgh; the station’s own', (await voices.locator('.voice').count()) === 3 && (await words(voices.locator('[data-voice="alba"] .pick'))) === 'Alba a Scottish woman · the default' && (await words(voices.locator('[data-voice="edinburgh"] .pick'))) === 'Edinburgh a man' && (await words(voices.locator('[data-voice="own"] .pick'))).startsWith('The station’s own'), await words(voices));
    check('each voice has Play', (await voices.getByRole('button', { name: 'Play Alba' }).count()) === 1 && (await voices.getByRole('button', { name: 'Play Edinburgh' }).count()) === 1);
    check('the station says which voice it plays: Alba, the default, is marked', (await voices.locator('[data-voice="alba"] input:checked').count()) === 1 && (await p.locator('[data-testid="phone-voice-unknown"]').count()) === 0);
    await voices.getByRole('button', { name: 'Play Alba' }).click();
    await sleep(500);
    check('Play asks the station for the voice, and says plainly when it has no copy', calls(st, 'GET', /consent\/default\/alba\.opus$/).length === 1 && (await words(p)).includes('Alba cannot be played here: this station has no copy of it.'));
    check('the words to read aloud name the station’s own contact, with Copy', (await words(p.locator('[data-testid="phone-to-record"]'))).includes('contact calls@kettle.example') && (await p.getByRole('button', { name: 'Copy' }).count()) >= 1);
    await fits(p, 'the phone line, closed');
    await shotOf(page, p, 'P6-settings-line-closed');
    // Upload the station's own.
    await p.locator('input[type="file"]').setInputFiles({ name: 'words.wav', mimeType: 'audio/wav', buffer: Buffer.alloc(48_000, 1) });
    await sleep(700);
    const put = calls(st, 'PUT', /phone\/words$/).at(-1);
    check('Upload a recording… sends the file to the station', !!put && (put.body?.length ?? 0) === 48_000, String(put?.body?.length));
    check('the recording is kept: how long, by whom and when; the line opens and plays it', (await words(p.locator('[data-testid="phone-recording"]'))).startsWith('7 s recorded by morag, today 13:52') && /^Taking calls$/i.test(await words(p.locator('.ph .state'))) && (await voices.locator('[data-voice="own"] input[type="radio"]:checked').count()) === 1, await words(p.locator('[data-testid="phone-recording"]')));
    await shotOf(page, p, 'P7-settings-own-recording');
    // With the station's own recording, a voice chosen is what plays if it is removed.
    await voices.locator('[data-voice="edinburgh"] input').click();
    await sleep(400);
    check('choosing Edinburgh with the station’s own recording says the recording still plays', JSON.stringify(calls(st, 'PUT', /phone\/consent$/).at(-1)?.body) === JSON.stringify({ default_voice: 'edinburgh' }) && (await voices.locator('[data-voice="own"] input[type="radio"]:checked').count()) === 1 && (await words(p)).includes('The station’s own recording still plays in place of it; Edinburgh plays if that recording is removed.') && (await words(p.locator('[data-testid="phone-fallback"]'))) === 'If the station’s recording is removed, Edinburgh plays.', await words(p));
    await p.getByRole('button', { name: 'Remove' }).click();
    await sleep(500);
    // Record here, with the browser's made-up microphone: a WAV of what was said goes to the station.
    await ctx.grantPermissions(['microphone', 'clipboard-read', 'clipboard-write']);
    await p.getByRole('button', { name: 'Record here' }).click();
    await p.getByRole('button', { name: 'Stop and keep' }).waitFor({ timeout: 5000 });
    check('Record here says it is recording, with Stop and keep and Cancel', /^Recording · \d+ s$/.test(await words(p.locator('.recording'))) && (await p.getByRole('button', { name: 'Cancel' }).count()) === 1, (await p.locator('.recording').count()) ? await words(p.locator('.recording')) : 'not recording');
    await sleep(2300);
    await p.getByRole('button', { name: 'Stop and keep' }).click();
    await sleep(900);
    const rec = calls(st, 'PUT', /phone\/words$/).at(-1)?.body;
    const riff = rec ? Buffer.from(rec).subarray(0, 4).toString('latin1') + Buffer.from(rec).subarray(8, 12).toString('latin1') : '';
    const seconds = rec ? (rec.length - 44) / 2 / Buffer.from(rec).readUInt32LE(24) : 0;
    check('Stop and keep sends a WAV of what was said, over two seconds of it', calls(st, 'PUT', /phone\/words$/).length === 2 && riff === 'RIFFWAVE' && seconds > 2 && seconds < 4, `${riff} ${seconds.toFixed(2)} s`);
    await p.getByRole('button', { name: 'Remove' }).click();
    await sleep(500);
    check('Remove takes the station’s recording away', calls(st, 'DELETE', /phone\/words$/).length === 2 && (await words(p.locator('[data-testid="phone-recording"]'))).startsWith('None yet.'));
    // The contact.
    await page.fill('#ph-contact', 'takedown@kettle.example');
    await p.getByRole('button', { name: 'Save the contact' }).click();
    await sleep(500);
    const setup = calls(st, 'PUT', /\/api\/v1\/setup$/).at(-1)?.body;
    check('saving the contact sends the name set on the setup page with it, so neither is handed back', JSON.stringify(setup) === JSON.stringify({ name: 'Kettle FM', contact: 'takedown@kettle.example' }), JSON.stringify(setup));
    check('and the words to read aloud follow it', (await words(p.locator('[data-testid="phone-to-record"]'))).includes('contact takedown@kettle.example'));
    // Ban a number, lift it, and find a number's calls.
    await p.locator('#ph-number').scrollIntoViewIfNeeded();
    await p.getByRole('button', { name: 'Ban this number' }).click();
    check('a ban needs the number and a reason', (await words(p)).includes('Type the number as the caller or the provider gave it.') && calls(st, 'POST', /phone\/bans$/).length === 0);
    await page.fill('#ph-number', '0131 496 0123');
    await page.fill('#ph-reason', 'Threats, 19 November');
    await page.selectOption('#ph-days', '90');
    await p.getByRole('button', { name: 'Ban this number' }).click();
    await sleep(500);
    const ban = calls(st, 'POST', /phone\/bans$/).at(-1)?.body;
    const bans = p.locator('[data-testid="phone-bans"] li');
    check('Ban this number sends it once, with the reason and the length', JSON.stringify(ban) === JSON.stringify({ number: '0131 496 0123', reason: 'Threats, 19 November', days: 90 }), JSON.stringify(ban));
    check('the list shows the last four digits, the reason and until when; the fields are cleared', (await bans.count()) === 1 && /^···0123 Threats, 19 November banned 2026-11-20 · until 18 February 2027 Lift/.test(await words(bans.first())) && (await page.inputValue('#ph-number')) === '', await words(bans.first()));
    await shotOf(page, p, 'P8-settings-banned');
    await page.fill('#ph-find', '0131 496 0123');
    await p.getByRole('button', { name: 'Find their calls' }).click();
    const found = p.locator('[data-testid="phone-found"]');
    await found.waitFor();
    check('Find their calls says how many, and what erasing removes and keeps', (await words(found)).startsWith('2 calls found.') && (await words(found)).includes('The date and the words agreed stay as the station’s record.'));
    await found.getByRole('button', { name: 'Erase these 2 calls…' }).click();
    check('erasing asks once more, and has sent nothing yet', (await words(found)).startsWith('Erase these 2 calls? This cannot be undone.') && st.erased.length === 0);
    await found.getByRole('button', { name: 'Erase 2 calls' }).click();
    await sleep(600);
    check('Erase erases each call’s consent record, signed with the admin’s name', JSON.stringify(st.erased) === JSON.stringify([{ id: 41, by: 'morag' }, { id: 37, by: 'morag' }]), JSON.stringify(st.erased));
    await bans.first().getByRole('button', { name: /Lift/ }).click();
    await sleep(400);
    check('Lift lifts the ban', calls(st, 'DELETE', /phone\/bans\/pb_/).length === 1 && (await p.locator('[data-testid="phone-bans-none"]').count()) === 1);
    await named(p, 'the phone line, on');
    await ctx.close();
  }
  {
    // The line on with the shipped voices: the voice and the words are changed from here.
    const st = kettle({ role: 'admin' });
    st.phone.on = true;
    const { ctx, page } = await open(st, { settings: true });
    const p = phonePanel(page);
    await p.waitFor();
    await p.scrollIntoViewIfNeeded();
    const voices = p.locator('[data-testid="phone-voices"]');
    check('a station that says its voice: Alba is marked, and the switch is this screen’s', (await voices.locator('[data-voice="alba"] input:checked').count()) === 1 && (await p.locator('[data-testid="phone-words-switch"]').getAttribute('aria-disabled')) === null && (await p.locator('[data-testid="phone-words-switch"]').getAttribute('aria-checked')) === 'true');
    await voices.locator('[data-voice="edinburgh"] input').click();
    await sleep(400);
    check('choosing Edinburgh asks the station, and it is marked', JSON.stringify(calls(st, 'PUT', /phone\/consent$/).at(-1)?.body) === JSON.stringify({ default_voice: 'edinburgh' }) && (await voices.locator('[data-voice="edinburgh"] input:checked').count()) === 1);
    await voices.getByRole('button', { name: 'Play Edinburgh' }).click();
    await sleep(600);
    check('Play fetches the shipped voice and plays it', calls(st, 'GET', /consent\/default\/edinburgh\.opus$/).length === 1 && !(await words(p)).includes('cannot be played') && !(await words(p)).includes('could not be played'), await words(p));
    await voices.locator('[data-voice="own"] input[type="radio"]').click({ force: true });
    await sleep(300);
    check('the station’s own cannot be chosen before it is recorded', calls(st, 'PUT', /phone\/consent$/).length === 1 && (await words(p)).includes('Upload or record the station’s own words first.'));
    await p.locator('[data-testid="phone-words-switch"]').click();
    await sleep(400);
    check('the switch turns the words off, and says what that means', JSON.stringify(calls(st, 'PUT', /phone\/consent$/).at(-1)?.body) === JSON.stringify({ consent_words: false }) && (await p.locator('[data-testid="phone-words-switch"]').getAttribute('aria-checked')) === 'false' && (await words(p)).includes('callers go straight into the queue'));
    await shotOf(page, p, 'P9-settings-chooser');
    await ctx.close();
  }
  {
    const st = kettle({ role: 'admin' });
    st.phone.routes = false;
    const { ctx, page } = await open(st, { settings: true });
    await page.locator('[data-panel="settings.hosts"]').waitFor();
    check('a station older than 1.2 has no Phone line panel', (await phonePanel(page).count()) === 0);
    await ctx.close();
    const prod = await open(kettle({ role: 'producer' }), { settings: true });
    check('the panel is an admin’s: a producer is not shown it', (await phonePanel(prod.page).count()) === 0);
    await prod.ctx.close();
  }
}

// ---- 5. Settings › Presenter kit and Settings › Bridge ----
if (want('kit')) {
  const kitPanel = (page) => page.locator('[data-panel="settings.kit"]');
  const withKeys = (o = {}) => {
    const st = new Station12({ role: 'admin', ...o });
    st.join('duncan', 'Duncan Boyd', 'presenter');
    const finlay = st.give('steeped', 'Finlay Grant', 'vban', { made_days_ago: 3 });
    st.give('kettle-drive', 'Heather Douglas', 'icecast', { made_days_ago: 40 });
    return { st, finlay };
  };
  for (const theme of THEMES) {
    const { st, finlay } = withKeys();
    st.connect(finlay.key_id, 'vban', 'pcm 48 kHz stereo', { ago_s: 600 });
    st.clock = { rate: 48_000, sources: [{ identity: `source-${finlay.key_id.toLowerCase()}`, name: 'Finlay Grant', way: 'vban', ppm: 41.3, replaced: 1, lost: 2 }], vban_dropped: { address: 1200, late: 3 }, vban_out: [{ name: 'Programme', address: '192.168.1.30', source: 'programme', packets: 182_400, dropped: 0, problem: null }] };
    st.kit = { preset: 'custom', bind: '192.168.1.10', port: 6980, allow_public: false, vban_in: [{ address: '192.168.1.20', name: 'Stream1', key_id: finlay.key_id, channels: [1, 2] }], vban_out: [] };
    const { ctx, page } = await open(st, { settings: true, theme });
    const p = kitPanel(page);
    await p.waitFor();
    await p.scrollIntoViewIfNeeded();
    await sleep(500);
    if (theme === 'dark') {
      check('the clock badge counts drop-outs, and the table says each source’s drift in words', (await words(p.locator('[data-testid="kit-clock"]'))) === 'One clock, 48 kHz · 3 drop-outs' && /Finlay Grant VBAN \+41 ppm 1 2/.test(await words(p.locator('table tbody'))), await words(p.locator('table tbody')));
      check('dropped VBAN packets are said by reason, in words', (await words(p.locator('[data-testid="kit-dropped"]'))) === 'VBAN packets not taken: 1,200 from an address that is not bound; 3 arrived late.');
      const presets = p.locator('.preset');
      check('the four presets are buttons named in plain words, the one in use marked', (await presets.count()) === 4 && (await presets.locator('b').allInnerTexts()).join('|') === 'Studio PC on the network|Transmitter feed|Local guest|Custom' && (await p.locator('.preset[aria-pressed="true"]').getAttribute('data-preset')) === 'custom', (await presets.locator('b').allInnerTexts()).join('|'));
      check('a VBAN in is bound to a show key by name, not a typed id', (await p.locator('#kit-in-0-key option:checked').innerText()) === 'Finlay Grant · Steeped');
      await presets.nth(1).click();
      await sleep(400);
      check('a preset with nowhere to send to says so, and puts the caret in Send to', (await words(p)).includes('Transmitter feed needs an address to send to.') && (await page.evaluate(() => document.activeElement?.id)) === 'kit-addr');
      await page.fill('#kit-addr', '192.168.1.30');
      await presets.nth(1).click();
      await sleep(500);
      const preset = calls(st, 'POST', /kit\/preset$/).at(-1)?.body;
      check('Transmitter feed is one press: the preset and the address', JSON.stringify(preset) === JSON.stringify({ preset: 'transmitter', address: '192.168.1.30' }) && (await p.locator('.preset[aria-pressed="true"]').getAttribute('data-preset')) === 'transmitter', JSON.stringify(preset));
      const out = p.locator('[data-testid="kit-out-row"]');
      check('its row is below: the programme after the delay, in words, and that it is sending', (await out.count()) === 1 && (await out.locator('#kit-out-0-source option:checked').innerText()) === 'The programme, after the delay (what listeners hear)' && (await words(out.locator('.now'))) === 'Sending: 182,400 packets.', (await out.count()) ? await words(out) : 'no row');
      await page.fill('#kit-out-0-address', '192.168.1.255');
      await p.getByRole('button', { name: 'Save the kit' }).click();
      await sleep(500);
      check('the station’s refusal of a field lands under that field, in plain words, with the caret there', (await words(out)).includes('Give one receiver’s address: VBAN is never sent to everyone on the network.') && (await page.evaluate(() => document.activeElement?.id)) === 'kit-out-0-address', await words(out));
      await page.fill('#kit-out-0-address', '192.168.1.31');
      await p.getByRole('button', { name: 'Save the kit' }).click();
      await sleep(500);
      check('Save sends the kit whole', calls(st, 'PUT', /\/api\/v1\/kit$/).at(-1)?.body.vban_out[0].address === '192.168.1.31' && st.kit.vban_in.length === 1 && (await words(p.locator('.foot'))).startsWith('Saved.'));
      await fits(p, 'the presenter kit');
      await named(p, 'the presenter kit');
      await noSideScroll(page, 'Settings with the presenter kit');
    }
    await shotOf(page, p, `K1-${theme}-kit`);
    await ctx.close();
  }
  {
    // Cloudron: VBAN is never offered, and none of its settings are drawn. The clock still is.
    const { st } = withKeys();
    st.kitCloudron = true;
    const { ctx, page } = await open(st, { settings: true });
    const p = kitPanel(page);
    await p.waitFor();
    await p.scrollIntoViewIfNeeded();
    const off = p.locator('[data-testid="kit-vban-off"]');
    check('on Cloudron VBAN is not offered, and the screen says why and what to use instead', (await words(off)).startsWith('VBAN is not offered on Cloudron.') && (await words(off)).includes('with a show key instead') && (await off.getAttribute('data-cloudron')) === 'true', await words(off));
    check('and none of it is drawn: no address, no presets, no rows, no Save', (await p.locator('#kit-bind, .preset, [data-testid="kit-in-row"], [data-testid="kit-out-row"], form').count()) === 0);
    check('the clock is still read there', (await p.locator('[data-testid="kit-clock"]').count()) === 1 && (await p.locator('[data-testid="kit-clock-none"]').count()) === 1);
    await shotOf(page, p, 'K2-kit-cloudron');
    await ctx.close();
    const quiet = withKeys().st;
    quiet.kitAllowed = false;
    const q = await open(quiet, { settings: true });
    const o2 = kitPanel(q.page).locator('[data-testid="kit-vban-off"]');
    await o2.waitFor();
    check('on Compose with VBAN off it says how it is turned on', (await words(o2)).startsWith('VBAN is off for this station.') && (await words(o2)).includes('KIT_VBAN=true') && (await kitPanel(q.page).locator('.preset').count()) === 0);
    await q.ctx.close();
  }
  // The bridge is the packaged app's alone. Its panel's markup in each state comes from the
  // unit test (which also checks it), and is drawn here in a real browser beside the built
  // app's own styles: a picture of the panel, not a run of it.
  {
    const dir = join(OUT, 'bridge-html');
    let wrote = true;
    try {
      execFileSync('pnpm', ['vitest', 'run', 'src/bridge.test.ts'], { cwd: join(here, '..', '..', 'apps', 'studio'), env: { ...process.env, SHOT_HTML_DIR: dir }, stdio: 'pipe' });
    } catch (e) {
      wrote = false;
      check('the bridge’s unit tests pass and write its states', false, String(e.stdout ?? e));
    }
    if (wrote) {
      for (const theme of THEMES) {
        const { ctx, page } = await open(new Station12({ role: 'admin' }), { settings: true, theme });
        for (const state of theme === 'dark' ? ['new', 'off', 'waiting', 'standing', 'toofast', 'retrying'] : ['standing']) {
          const html = readFileSync(join(dir, `bridge-${state}.html`), 'utf8');
          await page.evaluate((h) => {
            document.querySelector('[data-shot="bridge"]')?.remove();
            const host = document.createElement('div');
            host.dataset.shot = 'bridge';
            host.innerHTML = h;
            document.querySelector('main.settings .inner header').after(host);
          }, html);
          const b = page.locator('[data-shot="bridge"] [data-panel="settings.bridge"]');
          await b.scrollIntoViewIfNeeded();
          if (theme === 'dark') {
            const styled = await b.evaluate((el) => getComputedStyle(el).borderTopLeftRadius !== '0px' && getComputedStyle(el.querySelector('.now') ?? el).display === (el.querySelector('.now') ? 'grid' : getComputedStyle(el).display));
            check(`the bridge, ${state}: the panel is drawn with the app’s own styles`, styled);
            await fits(b, `the bridge, ${state}`);
          }
          await shotOf(page, b, `B-${theme}-bridge-${state}`);
        }
        await ctx.close();
      }
    }
  }
}

// ---- 6. The desk at 1,440 by 900: a compact Picture and Callers (16d), and a phone caller in the desk's queue ----
if (want('compact')) {
  const VIDEO = { mode: { mode: 'auto' }, shot: 'person', identity: 'eilidh', cameras: ['eilidh', 'gregor'], hls: true, room: true, still: true };
  const inside = async (page, sel, panel) => {
    const [a, b] = await Promise.all([page.locator(sel).first().boundingBox(), page.locator(panel).boundingBox()]);
    return !!a && !!b && a.y >= b.y - 0.5 && a.y + a.height <= b.y + b.height + 0.5;
  };
  {
    // Full with video on: the cameras are in sight under the switch.
    const st = new Station12({ step: 'full' });
    st.join('eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
    st.join('gregor', 'Gregor Stewart', 'guest', { on_air: true });
    const view = st.viewNow.bind(st);
    st.viewNow = () => ({ ...view(), desk: { ...view().desk, picture: VIDEO } });
    const { ctx, page } = await open(st);
    const pic = page.locator('[data-panel="desk.picture"]');
    await pic.waitFor();
    const big = SIZE.width / SIZE.text >= 2200; // the large layout is the studio's own width, which the text size divides
    const line = await pic.locator('.pic').boundingBox();
    check(big ? 'in a large window the picture keeps its box' : 'what is on the picture is one line, not a box, and whole', !!line && (big ? line.height > 60 * SIZE.text : line.height >= 24 && line.height < 40 * SIZE.text), JSON.stringify(line));
    check('it still says who is on camera and that the picture follows the audio', /^Eilidh on camera follows the audio$/.test(await words(pic.locator('.pic'))), await words(pic.locator('.pic')));
    check('the switch is in sight, inside the panel', await inside(page, '[data-panel="desk.picture"] .sw', '[data-panel="desk.picture"]'));
    check('both cameras are in sight under it, without scrolling', (await pic.locator('.cams li').count()) === 2 && (await inside(page, '[data-panel="desk.picture"] .cams', '[data-panel="desk.picture"] .fold')), JSON.stringify(await pic.locator('.cams').boundingBox()));
    check('the count of cameras is not said twice: the cameras say it', !(await words(pic)).includes('2 cameras'));
    await fits(pic, 'the Picture panel');
    await noSideScroll(page, 'the desk with video on');
    await shot(page, 'C1-desk-picture-compact');
    await shotOf(page, pic, 'C1b-picture');
    await ctx.close();
  }
  {
    // Callers: the one Take would bring keeps two lines, the ones behind a line each; a phone
    // caller is their line, never their key.
    const st = new Station12({ step: 'callers' });
    st.phone.on = true;
    st.join('eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
    st.ring('phone-x7k2', { phone: { tail: '0123' }, waited_s: 400 });
    st.ring('caller-dougal', { name: 'Dougal Thomson', topic: 'the bins on Bothwell Street, again', waited_s: 380 });
    st.ring('caller-jessie', { name: 'Jessie Robertson', topic: 'the ferry timetable', waited_s: 200 });
    st.ring('caller-tavish', { name: 'Tavish Murray', topic: 'a request', waited_s: 90 });
    const { ctx, page } = await open(st);
    const cq = page.locator('[data-panel="desk.callers"]');
    await cq.waitFor();
    const rows = cq.locator('.cq li');
    const hs = await rows.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().height)));
    const big = SIZE.width / SIZE.text >= 2200; // the large layout is the studio's own width, which the text size divides
    check(big ? 'in a large window every row keeps two lines' : 'the first row keeps two lines, and those behind are a line each', hs.length === 3 && (big ? hs[1] >= hs[0] - 1 : hs[1] < hs[0] - 6 && hs[2] < hs[0] - 6), hs.join(','));
    check('all three rows are in sight with Take, without scrolling', (await inside(page, '[data-panel="desk.callers"] .cq li:nth-child(3)', '[data-panel="desk.callers"] .fold')) && (await inside(page, '[data-panel="desk.callers"] .acts', '[data-panel="desk.callers"]')), hs.join(','));
    check('a phone caller is their line in the queue and on Take, never their key', (await words(rows.first())).startsWith('1 Phone ···0123') && (await words(cq.locator('.acts .btn'))).startsWith('Take Phone ···0123 to strip 4') && !(await cq.innerText()).includes('phone-x7k2'), `${await words(rows.first())} | ${await words(cq.locator('.acts .btn'))}`);
    check('a row’s tooltip gives its topic whole', (await rows.nth(1).locator('.who').getAttribute('title')) === 'Dougal Thomson: the bins on Bothwell Street, again');
    await fits(cq, 'the Callers panel');
    await shot(page, 'C2-desk-callers-compact');
    await shotOf(page, cq, 'C2b-callers');
    await ctx.close();
  }
  {
    // The Schedule tab's Studios stays off, and says so in plain words (issue 24: not modelled yet).
    const { ctx, page } = await open(new Station12({ step: 'full' }), { schedule: true });
    const b = page.getByRole('button', { name: 'Studios', exact: true });
    check('Studios is drawn off, and its reason names no build', (await b.getAttribute('aria-disabled')) === 'true' && (await b.getAttribute('title')) === 'Not in this version: a show has no studio of its own yet');
    await ctx.close();
  }
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed (${SIZE.width}×${SIZE.height}, text ${SIZE.text})`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
