// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the monitor link's page (ADR-0280; apps/web/monitor/) in a real browser against a
// stand-in station (station.mjs), checks each state and each press against what the page shows
// and what it sent, and captures the page in Dark, Light and Night at 1440×900 and 390×844.
// It needs no stack: one loopback port, and Playwright (not a dependency of the repository; see
// tests/m2/drive.mjs).
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/monitor-page/shoot.mjs
//   just test-monitor-page          # the same, with PLAYWRIGHT_CORE from the environment
//   DOCS=1 … node tests/monitor-page/shoot.mjs   # also writes the how-to's two pictures
//                                                # (docs/src/assets/screens/how-to-monitor-*.png)
//   ONLY=grace,dump … node tests/monitor-page/shoot.mjs   # some scenarios only
//
// It builds the page first (apps/web/vite.standin.config.ts: the page as it ships, with
// fake-livekit.js in livekit-client's place) into target/monitor-page/dist; SKIP_BUILD=1 uses
// the build that is there. Screenshots go to OUT (target/shots-monitor-page unless told), named
// `<width>-<theme>-<state>.png`. What this does not prove: anything about a real room, which is
// `just test-monitor`'s, and any browser but Chromium.
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MonitorStation, at, serve } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = join(ROOT, 'target', 'monitor-page', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-monitor-page');
const PORT = Number(process.env.PORT ?? 28140);
const ONLY = process.env.ONLY ? new Set(process.env.ONLY.split(',')) : null;
const DOCS = process.env.DOCS ? join(ROOT, 'docs', 'src', 'assets', 'screens') : null;
mkdirSync(OUT, { recursive: true });

if (!process.env.SKIP_BUILD) {
  execFileSync('pnpm', ['exec', 'vite', 'build', '--config', 'vite.standin.config.ts', '--logLevel', 'warn'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
}
if (!existsSync(join(DIST, 'monitor', 'index.html'))) throw new Error(`no build at ${DIST}`);

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** Wait for what the stand-in sees to be so (the page's own audio output can take seconds to start in a headless browser, so the stream's own connection is the sign). */
async function until(what, ok, ms = 8000) {
  const end = Date.now() + ms;
  while (!ok()) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(50);
  }
}
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

// ---- the stand-in's views against the station's own description ----

const SCHEMAS = JSON.parse(readFileSync(join(ROOT, 'docs', 'openapi', 'weavesignal-v1.json'), 'utf8')).components.schemas;
/** What is wrong with `v` as `schema` (the subset of JSON Schema the description uses). */
function wrong(schema, v, path = '$') {
  if (schema.$ref) return wrong(SCHEMAS[schema.$ref.split('/').at(-1)], v, path);
  for (const k of ['oneOf', 'anyOf']) {
    if (schema[k]) return schema[k].some((s) => wrong(s, v, path).length === 0) ? [] : [`${path}: matches none of ${k}`];
  }
  const types = [].concat(schema.type ?? []);
  const is = (t) => (t === 'null' ? v === null : t === 'integer' ? Number.isInteger(v) : t === 'number' ? typeof v === 'number' : t === 'array' ? Array.isArray(v) : t === 'object' ? typeof v === 'object' && v !== null && !Array.isArray(v) : typeof v === t);
  if (types.length && !types.some(is)) return [`${path}: ${JSON.stringify(v)} is not ${types.join(' or ')}`];
  if (schema.enum && v !== null && !schema.enum.includes(v)) return [`${path}: ${JSON.stringify(v)} is not one of ${schema.enum.join(', ')}`];
  const out = [];
  if (v && typeof v === 'object' && !Array.isArray(v) && schema.properties) {
    for (const k of schema.required ?? []) if (!(k in v)) out.push(`${path}.${k}: missing`);
    for (const k of Object.keys(v)) {
      if (!schema.properties[k]) out.push(`${path}.${k}: not in the description`);
      else out.push(...wrong(schema.properties[k], v[k], `${path}.${k}`));
    }
  }
  if (Array.isArray(v) && schema.items) v.forEach((x, i) => out.push(...wrong(schema.items, x, `${path}[${i}]`)));
  return out;
}
const described = new Set();
function describedAs(name, schema, v) {
  const bad = wrong(SCHEMAS[schema], v);
  if (bad.length || !described.has(name)) check(`the stand-in's ${name} is a ${schema} as docs/openapi describes it`, bad.length === 0, bad.join('; '));
  described.add(name);
}

// ---- the browser ----

const server = await serve(DIST, PORT);
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
const THEMES = ['dark', 'light', 'night'];
const SIZES = [
  { width: 1440, height: 900 },
  { width: 390, height: 844 },
];

/** One window: a size, a theme, and what it saw go wrong. */
async function open(size, theme, o = {}) {
  const ctx = await browser.newContext({
    viewport: size,
    deviceScaleFactor: 1,
    timezoneId: o.zone ?? 'Europe/London',
    colorScheme: theme === 'light' ? 'light' : 'dark',
    hasTouch: size.width < 500,
  });
  await ctx.addInitScript((night) => {
    try {
      if (night) localStorage.setItem('weavesignal.monitor.night', '1');
    } catch {}
    globalThis.__csp = [];
    document.addEventListener('securitypolicyviolation', (e) => globalThis.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  }, theme === 'night');
  const page = await ctx.newPage();
  if (o.deviceClock) await page.clock.setFixedTime(o.deviceClock);
  const w = { ctx, page, size, theme, tag: `${size.width}-${theme}`, urls: [], errors: [] };
  page.on('pageerror', (e) => w.errors.push(e.message));
  page.on('request', (r) => w.urls.push(r.url()));
  return w;
}
const windows = [];
for (const size of SIZES) for (const theme of THEMES) windows.push(await open(size, theme));
const main = windows[0];

const panel = (page, name) => page.locator(`[data-panel="monitor.${name}"]`);
const act = (page, name) => page.locator(`[data-act="${name}"]`);
const lampIs = (page, state, ms = 8000) => page.locator(`[data-panel="monitor.lamp"][data-state="${state}"]`).waitFor({ timeout: ms });
/** What an element says, as written (not as CSS capitals), with a space between its parts. */
const text = (loc) =>
  loc.evaluate((el) => {
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const out = [];
    while (walk.nextNode()) out.push(walk.currentNode.textContent);
    return out.join(' ').replace(/\s+/g, ' ').replace(/ ([.,:])/g, '$1').trim();
  });
const posts = (st, re) => st.calls.filter((c) => c.method === 'POST' && re.test(c.path));

/** Every window on the scenario's station, each waiting for its first view. */
async function everyone(st, { secret = st.secret, wait = true } = {}) {
  await Promise.all(
    windows.map(async (w) => {
      await w.page.goto('about:blank');
      await w.page.goto(`http://127.0.0.1:${PORT}/monitor/#${secret}`);
      if (wait) await w.page.locator('[data-panel="monitor.lamp"]').waitFor({ timeout: 15000 });
    }),
  );
}

/** Nothing on the page is wider than the window, and nothing runs out of its card. */
async function fits(w, what) {
  const bad = await w.page.evaluate(() => {
    const out = [];
    const vw = document.documentElement.clientWidth;
    if (document.scrollingElement.scrollWidth > vw + 1) out.push(`the page is ${document.scrollingElement.scrollWidth} px wide in a ${vw} px window`);
    for (const card of document.querySelectorAll('.mon .card, .mon .top')) {
      const box = card.getBoundingClientRect();
      for (const x of card.querySelectorAll('button, .state, .biglamp, h1, h2, b, p, dd, input, .cnt')) {
        const r = x.getBoundingClientRect();
        if (r.width && (r.right > box.right + 1 || r.left < box.left - 1)) out.push(`“${(x.textContent || '').trim().slice(0, 28)}” runs out of ${card.dataset.panel ?? card.className}`);
      }
    }
    return out;
  });
  check(`${what} (${w.tag}): nothing is cut off or runs out of its card`, bad.length === 0, bad.join('; '));
}

/** A picture for the product's documentation, when asked for (DOCS=1). */
async function docShot(w, name, fullPage = false) {
  if (!DOCS) return;
  await sleep(350);
  await w.page.screenshot({ path: join(DOCS, `${name}.png`), fullPage });
  log('docs picture', name);
}

/** A screenshot of every window, and the layout checked in each. `window` takes only what the
 *  window shows (a question over the page), not the whole page. */
async function shotAll(name, { quiet = false, window = false } = {}) {
  await sleep(350);
  for (const w of windows) {
    await w.page.screenshot({ path: join(OUT, `${w.tag}-${name}.png`), fullPage: !window });
    if (!quiet) await fits(w, name);
  }
  log('shot', name);
}

/** After a scenario: no page errors, no CSP violation, and the secret in no address. */
async function clean(st, what) {
  for (const w of windows) {
    const csp = await w.page.evaluate(() => globalThis.__csp ?? []).catch(() => []);
    if (csp.length || w.errors.length) check(`${what} (${w.tag}): no page errors and nothing the Content-Security-Policy refused`, false, [...w.errors, ...csp].join('; '));
    w.errors.length = 0;
    const leaked = w.urls.filter((u) => u.includes(st.secret));
    if (leaked.length) check(`${what} (${w.tag}): the secret is in no address the page asked for`, false, leaked.join(', '));
    w.urls.length = 0;
  }
}

const row = (show_name, host, state, start, end) => ({ show_name, host, state, start_ms: at(start), end_ms: at(end) });
const HEATHER = { label: 'Heather Douglas', show: 'kettle-drive', show_name: 'Kettle Drive', start: at('16:00'), end: at('18:00'), way: 'icecast', format: 'mp3 44.1 kHz stereo', level: -22.9, leveller_db: -3.9, steady_ms: 552_000 };
const heatherNext = () => ({ name: 'Heather Douglas', show_name: 'Kettle Drive', start: at('16:00'), state: 'standing_by', at_desk: true, on_air: false, cued: false, passes: true });

/** Finlay on air at 16:01:18, running over, with Heather standing by and the grace counting. */
function grace(o = {}) {
  const st = new MonitorStation({ now: at('16:01:18') });
  Object.assign(st, { on_air: true, level: -22.6, leveller_db: -7.0, steady_ms: 7_491_000, next: heatherNext() });
  st.post = { mine: false, show_name: 'Kettle Drive', start_ms: at('16:00'), host: 'Heather Douglas', host_state: 'standing_by', grace_ends_ms: at('16:02:00') };
  st.rows = { before: row('The Long Lunch', 'Duncan Boyd', 'in_room', '12:00', '14:00'), after: row('Kettle Drive', 'Heather Douglas', 'standing_by', '16:00', '18:00') };
  st.insert = { label: 'Station ID', at_ms: at('17:00'), duration_ms: 7000, plays: 'unknown' };
  return Object.assign(st, o);
}
/** Finlay on air at 14:41:07, alone, nobody after him connected yet. */
function onAir(o = {}) {
  const st = new MonitorStation({ now: at('14:41:07') });
  Object.assign(st, { on_air: true, steady_ms: 2_452_000 });
  st.rows = { before: row('The Long Lunch', 'Duncan Boyd', 'in_room', '12:00', '14:00'), after: row('Kettle Drive', 'Heather Douglas', 'not_connected', '16:00', '18:00') };
  st.insert = { label: 'Station ID', at_ms: at('15:00'), duration_ms: 7000, plays: 'unknown' };
  return Object.assign(st, o);
}

const scenarios = {
  // 13:57:12, standing by before the show: the lamp, the count, the clock, the monitor's sound.
  async standby() {
    const st = server.use(new MonitorStation({ now: at('13:57:12') }));
    st.accent = '#2f6fed';
    st.post = { mine: true, show_name: 'Steeped', start_ms: at('14:00'), host: 'Finlay Grant', host_state: 'standing_by', on_air_at_ms: at('14:00') };
    st.rows = { before: row('The Long Lunch', 'Duncan Boyd', 'in_room', '12:00', '14:00'), after: row('Kettle Drive', 'Heather Douglas', 'not_connected', '16:00', '18:00') };
    st.insert = { label: 'Station ID', at_ms: at('15:00'), duration_ms: 7000, plays: 'unknown' };
    describedAs('view standing by', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'standby');
    const lamp = panel(p, 'lamp');
    check('standing by: the lamp says STANDBY, hollow, and nothing on the page is filled as on air', (await text(lamp.locator('.biglamp'))) === 'Standby' && (await p.locator('.biglamp.onair, .fill-onair').count()) === 0);
    check('standing by: the sentence is the station’s own', (await text(lamp.locator('.say'))).includes('Standing by. The station hears you. Listeners do not.') && (await text(lamp.locator('.say'))).includes('You go on air at the post, by the schedule.'), await text(lamp.locator('.say')));
    const cnt = await text(lamp.locator('.cnt'));
    check('standing by: the count is to the post, on the station’s clock', /On air at 14:00:00/.test(cnt) && /2:[0-4]\d/.test(cnt), cnt);
    check('the title is the show and the host', (await p.title()) === 'Steeped · Finlay Grant', await p.title());
    const clock = await text(panel(p, 'clock'));
    check('the clock is the station’s, with its zone, the same as the device’s here', /13:57:[1-5]\d/.test(clock) && clock.includes('Europe/London · the same as yours'), clock);
    check('the header names the station', (await text(p.locator('.top .station'))) === 'Kettle FM');
    const ctl = panel(p, 'controls');
    check('standing by: nothing to press, and the page says so', (await ctl.locator('button').count()) === 0 && (await text(ctl)).includes('Nothing to press. You go on air at 14:00:00. Start when the lamp turns red.'), await text(ctl));
    const next = await text(panel(p, 'next'));
    check('before you, you and after you', /Before you The Long Lunch Duncan Boyd · in the station's room/.test(next) && /You Steeped 14:00 to 16:00 ?Standby/.test(next) && /After you Kettle Drive Heather Douglas · 16:00 · not connected yet/.test(next), next);
    check('the next insert, as far as the station can say', (await text(panel(p, 'insert'))).startsWith('Station ID at 15:00:00') && /In 1:0[12]:\d\d\. It plays for 0:07\./.test(await text(panel(p, 'insert'))), await text(panel(p, 'insert')));
    const src = await text(panel(p, 'source'));
    check('the station hears you: the way in, the level as it arrived and what the leveller does, how long steady', src.includes('SRT · mpegts/aac 48 kHz stereo') && src.includes('−15.2 LUFS · turned down 7.4 dB to match the station') && /Steady for (4:5\d|5:\d\d)/.test(src), src);
    check('the details are closed until opened', !(await panel(p, 'details').evaluate((d) => d.open)));

    // The monitor's sound: a press, a token asked for with the secret as a bearer, the room joined.
    const listen = panel(p, 'listen');
    check('your monitor: Start listening, and the note on headphones', (await act(p, 'listen').count()) === 1 && (await text(listen)).includes('Listen on headphones, and do not send this back into your mix.'));
    check('nothing is asked of the room before the press', posts(st, /\/token$/).length === 0);
    await act(p, 'listen').click();
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 8000 });
    const tok = posts(st, /\/token$/);
    check('Start listening asks the station for a token, with the secret as a bearer and no body', tok.length === 1 && tok[0].bearer === st.secret && tok[0].body === undefined, JSON.stringify(tok));
    describedAs('token', 'MonitorToken', st.lastToken);
    const room = await p.evaluate(() => globalThis.__rooms.map((r) => r.log));
    check('it joins the room the station named, listen only, with the token it was given', room.length === 1 && room[0].connects.length === 1 && room[0].connects[0].token === 'standin-token-1' && room[0].connects[0].url.includes(`:${PORT}/room`) && room[0].joined, JSON.stringify(room));
    check('the sound goes through an audio graph made in the press (so the volume works where an element’s does not)', room[0].webAudio === true);
    await sleep(500);
    check('listening: the chip says so', (await text(listen.locator('.state'))) === 'Listening');
    const vol = listen.locator('input[type="range"]');
    await vol.focus();
    for (let i = 0; i < 10; i++) await p.keyboard.press('ArrowLeft');
    await sleep(100);
    const after = await p.evaluate(() => globalThis.__rooms.at(-1).log.volume);
    check('the volume control answers the keyboard and turns the monitor down', Math.abs(after - 0.7) < 0.001 && (await vol.inputValue()) === '70', `volume ${after}`);
    check('…and the stream follows it', Math.abs((await p.locator('audio[data-monitor="stream"]').evaluate((a) => a.volume)) - 0.7) < 0.001);
    for (let i = 0; i < 10; i++) await p.keyboard.press('ArrowRight');

    // Nobody on air at the desk, so its mix is silent: the host hears the listeners' stream.
    const heard = p.locator('audio[data-monitor="stream"]');
    await heard.waitFor({ state: 'attached', timeout: 8000 });
    await p.waitForFunction(() => document.querySelector('audio[data-monitor="stream"]')?.paused === false, null, { timeout: 8000 });
    await until('the stream to be asked for', () => st.listenerStream.open === 1);
    check('standing by with nobody on air: the listeners’ stream plays, from the press', st.listenerStream.hits === 1 && st.listenerStream.open === 1, JSON.stringify(st.listenerStream));
    const vs = await heard.evaluate((a) => a.volume);
    check('…and the volume control turns it down with the rest', Math.abs(vs - 0.8) < 0.001, `volume ${vs}`);
    const says = await text(listen.locator('.small'));
    check('…and the card says it is the listeners’ stream, how far behind, and what takes over on air', says === 'You are hearing what listeners hear, about 23 s behind, while you stand by. When you are on air: everyone but you · cue · talkback, live.', says);
    check('…and no level bar is drawn beside it (the room’s mix is silent, and a flat bar would say the host’s sound is broken)', (await listen.locator('.level').count()) === 0);

    // Night: a switch beside the device's own light or dark, and the station's accent kept out of it.
    const theme = () => p.evaluate(() => [document.documentElement.dataset.theme ?? 'dark', getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()]);
    const [t0, a0] = await theme();
    await p.locator('.seg button', { hasText: 'Night' }).click();
    const [t1, a1] = await theme();
    await p.locator('.seg button', { hasText: 'Device' }).click();
    const [t2] = await theme();
    check('the Night switch turns Night on and Device turns it off', t0 === 'dark' && t1 === 'night' && t2 === 'dark', `${t0} ${t1} ${t2}`);
    check('Night keeps its own accent whatever the station’s is; Dark takes the station’s', a1 === '#d0912f' && a0 !== '#7aa2f7' && a0 !== a1, `dark ${a0}, night ${a1}`);
    const light = await windows[1].page.evaluate(() => document.documentElement.dataset.theme);
    check('Device follows the device: light where it asks for light', light === 'light', String(light));

    for (const w of windows.slice(1)) {
      await act(w.page, 'listen').click();
      await w.page.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 8000 });
      await w.page.waitForFunction(() => document.querySelector('audio[data-monitor="stream"]')?.paused === false, null, { timeout: 8000 });
    }
    await shotAll('standby');
    await docShot(windows[3], 'how-to-monitor-standing-by', true);
    await clean(st, 'standing by');
  },

  // Heather in Berlin, behind a DJ who is running over, on a device whose clock is years wrong.
  async behind() {
    const st = server.use(new MonitorStation({ now: at('16:01:18') }));
    Object.assign(st, HEATHER, { others: ['Finlay Grant'] });
    st.post = { mine: true, show_name: 'Kettle Drive', start_ms: at('16:00'), host: 'Heather Douglas', host_state: 'standing_by', grace_ends_ms: at('16:02:00'), on_air: ['Finlay Grant'] };
    st.rows = { before: row('Steeped', 'Finlay Grant', 'on_air', '14:00', '16:00'), after: row('Firth Sessions', 'Rory Bruce', 'in_room', '18:00', '20:00') };
    describedAs('view behind a DJ', 'MonitorView', st.view());
    await everyone(st);
    const far = await open({ width: 390, height: 844 }, 'dark', { zone: 'Europe/Berlin', deviceClock: new Date('2031-03-03T03:03:03Z') });
    await far.page.goto(`http://127.0.0.1:${PORT}/monitor/#${st.secret}`);
    await lampIs(far.page, 'standby');
    await sleep(2300);
    const device = await far.page.evaluate(() => new Date().toISOString());
    const clock = await text(panel(far.page, 'clock'));
    const cnt = await text(panel(far.page, 'lamp').locator('.cnt'));
    check('a device whose clock is years wrong still shows the station’s time', device.startsWith('2031-03-03T03:03:03') && /16:01:[2-5]\d/.test(clock), `device ${device}; page ${clock}`);
    check('…and still counts to the second on the station’s clock', /On air at 16:02:00/.test(cnt) && /0:([0-3]\d|4[0-2])/.test(cnt), cnt);
    check('a host in another zone sees their own time beside the station’s', clock.includes('17:01 your time · Europe/Berlin'), clock);
    const a = Number(/0:(\d\d)/.exec(cnt)[1]);
    await sleep(3000);
    const b = Number(/0:(\d\d)/.exec(await text(panel(far.page, 'lamp').locator('.cnt')))[1]);
    check('the count moves with the station’s seconds, not the device’s (which stand still)', a - b >= 2 && a - b <= 4, `${a} then ${b}`);
    await far.page.screenshot({ path: join(OUT, '390-dark-behind-berlin.png'), fullPage: true });
    await far.ctx.close();
    const say = await text(panel(main.page, 'lamp').locator('.say'));
    check('behind a DJ: the station’s words for who holds the air and how it passes', say.includes('Finlay Grant is still on air. The handover is by a press or by the schedule.'), say);
    check('behind a DJ: Icecast’s delay is said', (await text(panel(main.page, 'source'))).includes('your sound reaches the station about 2 s after you play it'));
    const next = await text(panel(main.page, 'next'));
    check('the show before, running over, says so and carries ON AIR', /Before you Steeped Finlay Grant · running over ?On air/.test(next), next);
    await shotAll('behind');
    await clean(st, 'behind a DJ');
  },

  // Arriving late onto automation: the count, Not yet, Go on air now.
  async late() {
    const st = server.use(new MonitorStation({ now: at('16:10:41') }));
    Object.assign(st, HEATHER, { auto_mode: true, steady_ms: 11_000 });
    st.post = { mine: true, show_name: 'Kettle Drive', start_ms: at('16:00'), host: 'Heather Douglas', host_state: 'standing_by', late_ends_ms: at('16:10:55'), on_air_at_ms: at('16:10:55') };
    st.rows = { before: row('Steeped', 'Finlay Grant', 'not_connected', '14:00', '16:00'), after: row('Firth Sessions', 'Rory Bruce', 'in_room', '18:00', '20:00') };
    describedAs('view arriving late', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'standby');
    const cnt = await text(panel(p, 'lamp').locator('.cnt'));
    check('late: the lamp says STANDBY and counts down to air', /On air in/.test(cnt) && /0:(0[1-9]|1[0-4])/.test(cnt) && (await text(panel(p, 'lamp').locator('.say'))).includes('Automation holds your air.'), cnt);
    check('late: Go on air now and Not yet, and nothing else to press', (await text(act(p, 'go'))) === 'Go on air now' && (await text(act(p, 'not-yet'))) === 'Not yet' && (await panel(p, 'controls').locator('button').count()) === 2);
    await shotAll('late');
    await act(p, 'not-yet').click();
    await p.waitForFunction(() => !document.querySelector('[data-act="not-yet"]'));
    const ny = posts(st, /\/not-yet$/);
    check('Not yet sends POST /api/v1/monitor/not-yet, once, with no body', ny.length === 1 && ny[0].body === undefined && ny[0].bearer === st.secret, JSON.stringify(ny));
    check('after Not yet the count is gone and Go on air now stays', (await panel(p, 'lamp').locator('.cnt b').count()) === 0 && (await act(p, 'go').count()) === 1);
    await shotAll('late-not-yet');
    const before = st.view();
    await act(p, 'go').click();
    await lampIs(p, 'on_air');
    // A view made before the press and arriving after its answer must not turn the lamp back.
    st.pushOld(before);
    await sleep(150);
    check('a view made before the press, arriving after its answer, does not turn the lamp back', (await panel(p, 'lamp').getAttribute('data-state')) === 'on_air');
    const go = posts(st, /\/go$/);
    check('Go on air now sends POST /api/v1/monitor/go, once', go.length === 1 && go[0].body === undefined, JSON.stringify(go));
    check('…and the lamp turns red: ON AIR, filled', (await text(panel(p, 'lamp').locator('.biglamp.onair'))) === 'On air' && (await text(panel(p, 'lamp').locator('.say'))).includes('On air. Listeners hear you, about 23 s behind.'));
    describedAs('answer to a press', 'MonitorDone', st.lastDone);
    await clean(st, 'arriving late');
  },

  // On air with the grace counting: cue, the handover's two presses.
  async grace() {
    const st = server.use(grace());
    describedAs('view on air in the grace', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    const lamp = panel(p, 'lamp');
    const cnt = await text(lamp.locator('.cnt'));
    check('on air in the grace: ON AIR filled, the station’s sentence, and the grace counted', (await lamp.locator('.biglamp.onair').count()) === 1 && (await text(lamp.locator('.say'))).includes('Kettle Drive takes the air by the schedule. Heather Douglas is standing by.') && /Kettle Drive takes the air/.test(cnt) && /0:[0-4]\d/.test(cnt) && /by the schedule at 16:02:00/.test(cnt), cnt);
    const names = await panel(p, 'controls').locator('button:not(dialog button)').allTextContents();
    check('the controls are what he may do now, in order: Cue, Bring, End my show, Dump', JSON.stringify(names.map((n) => n.trim())) === JSON.stringify(['Cue Heather', 'Bring Heather on air', 'End my show', 'Dump']), JSON.stringify(names));
    check('the overlap is explained once, beside the button', (await text(panel(p, 'controls'))).includes('You and Heather sound together until you hand over. Fade your own music. Do not try to match beats across the link.'));
    const next = await text(panel(p, 'next'));
    check('running over, with the next host standing by', /Before you The Long Lunch ended 14:00/.test(next) && /You Steeped 14:00 to 16:00 · running over ?On air/.test(next) && /After you Kettle Drive Heather Douglas · 16:00 ?Standby/.test(next), next);
    check('an insert after his time asks nothing of him', (await text(panel(p, 'insert'))).includes('After your time. Nothing for you to do.'));
    for (const w of windows) await act(w.page, 'listen').click();
    await act(p, 'cue').click();
    await p.locator('[data-act="cue"][aria-pressed="true"]').waitFor();
    let cue = posts(st, /\/cue$/);
    check('Cue Heather sends POST /api/v1/monitor/cue {"on": true}', cue.length === 1 && JSON.stringify(cue[0].body) === '{"on":true}', JSON.stringify(cue));
    st.push();
    await sleep(300);
    check('…the button lights as CUE, and the monitor says she is in his cue', (await act(p, 'cue').evaluate((b) => b.classList.contains('lit-cue'))) && (await text(panel(p, 'listen'))).includes('Heather is in your cue'));
    await shotAll('grace');
    await docShot(main, 'how-to-monitor-on-air');
    check('at 1440×900 every control is in the window without scrolling', await act(p, 'dump').evaluate((b) => b.getBoundingClientRect().bottom <= innerHeight));
    await act(p, 'cue').click();
    await p.locator('[data-act="cue"][aria-pressed="false"]').waitFor();
    cue = posts(st, /\/cue$/);
    check('pressing it again takes her out: {"on": false}', cue.length === 2 && JSON.stringify(cue[1].body) === '{"on":false}', JSON.stringify(cue));

    await act(p, 'bring').click();
    await act(p, 'hand-over').waitFor();
    let ho = posts(st, /\/handover$/);
    check('Bring Heather on air sends POST /api/v1/monitor/handover {"step": "bring"}', ho.length === 1 && JSON.stringify(ho[0].body) === '{"step":"bring"}', JSON.stringify(ho));
    check('…then the button is Hand over to Heather, Bring is gone, and he is still ON AIR', (await text(act(p, 'hand-over'))) === 'Hand over to Heather' && (await act(p, 'bring').count()) === 0 && (await panel(p, 'lamp').locator('.biglamp.onair').count()) === 1);
    check('the focus stays in the controls when the pressed button goes', await p.evaluate(() => !!document.activeElement?.closest('[data-panel="monitor.controls"]')));
    st.push();
    await shotAll('overlap');
    await act(p, 'hand-over').click();
    await lampIs(p, 'standby');
    ho = posts(st, /\/handover$/);
    check('Hand over to Heather sends {"step": "hand_over"}, and the lamp leaves ON AIR', ho.length === 2 && JSON.stringify(ho[1].body) === '{"step":"hand_over"}' && (await p.locator('.biglamp.onair').count()) === 0, JSON.stringify(ho));
    check('…with nothing left for him to press', (await panel(p, 'controls').locator('button:not(dialog button)').count()) === 0);
    st.push();
    await shotAll('handed-over');
    await clean(st, 'the grace and the handover');
  },

  // End my show: asked once; Keep going sends nothing; a held key is one press.
  async end() {
    const st = server.use(grace());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    await act(p, 'end').click();
    const dlg = p.locator('dialog[open]');
    await dlg.waitFor();
    check('End my show asks once, with what follows in the station’s words', (await text(dlg)).includes('End Steeped now?') && (await text(dlg)).includes('Heather Douglas is standing by: the air passes to them as you leave.'), await text(dlg));
    check('the question has sent nothing yet', posts(st, /\/end-show$/).length === 0);
    for (const w of windows.slice(1)) await act(w.page, 'end').click();
    await shotAll('end-ask', { quiet: true, window: true });
    for (const w of windows.slice(1)) await act(w.page, 'end-no').click();
    await act(p, 'end-no').click();
    await p.waitForFunction(() => !document.querySelector('dialog[open]'));
    await sleep(200);
    check('Keep going closes the question and sends nothing', posts(st, /\/end-show$/).length === 0 && (await panel(p, 'lamp').locator('.biglamp.onair').count()) === 1);
    await act(p, 'end').click();
    await dlg.waitFor();
    await p.keyboard.press('Escape');
    await sleep(200);
    check('Esc is Keep going', (await dlg.count()) === 0 && posts(st, /\/end-show$/).length === 0);
    // A key held down on End my show opens the question and must not answer it.
    await act(p, 'end').focus();
    await p.keyboard.down('Enter');
    for (let i = 0; i < 6; i++) await p.keyboard.down('Enter');
    await p.keyboard.up('Enter');
    await sleep(300);
    check('Enter held down on End my show opens the question and does not answer it', (await dlg.count()) === 1 && posts(st, /\/end-show$/).length === 0);
    await p.keyboard.press('Enter');
    await lampIs(p, 'standby');
    const end = posts(st, /\/end-show$/);
    check('End my show in the question sends POST /api/v1/monitor/end-show, once, and the lamp leaves ON AIR', end.length === 1 && end[0].body === undefined && (await panel(p, 'lamp').locator('.biglamp.onair').count()) === 0, JSON.stringify(end));
    await clean(st, 'End my show');
  },

  // Dump: two presses within 3 s; one press lapses; a held key is one press; a refusal is said.
  async dump() {
    const st = server.use(onAir({ step: 'callers' }));
    describedAs('view on air', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    const names = (await panel(p, 'controls').locator('button:not(dialog button)').allTextContents()).map((n) => n.trim());
    check('on air at Callers with nobody after him: End my show and Dump, no handover', JSON.stringify(names) === JSON.stringify(['End my show', 'Dump']), JSON.stringify(names));
    await shotAll('on-air');
    const dump = act(p, 'dump');
    await dump.click();
    check('one press arms Dump: it says Press again, and nothing is sent', (await text(dump)) === 'Press again' && (await dump.evaluate((b) => b.classList.contains('armed'))) && posts(st, /\/dump$/).length === 0);
    for (const w of windows.slice(1)) await act(w.page, 'dump').click();
    await shotAll('dump-armed');
    await sleep(3000);
    check('after 3 s it is Dump again, and still nothing was sent', (await text(dump)) === 'Dump' && posts(st, /\/dump$/).length === 0);
    await dump.focus();
    await p.keyboard.down('Enter');
    for (let i = 0; i < 8; i++) await p.keyboard.down('Enter');
    await p.keyboard.up('Enter');
    await sleep(200);
    check('Enter held down on Dump is one press, not two: armed, nothing sent', (await text(dump)) === 'Press again' && posts(st, /\/dump$/).length === 0);
    await sleep(3100);
    await dump.click();
    await dump.click();
    await p.locator('[data-says="dumped"]').waitFor();
    const d = posts(st, /\/dump$/);
    check('two presses within 3 s send POST /api/v1/monitor/dump, once, with no body', d.length === 1 && d[0].body === undefined && d[0].bearer === st.secret, JSON.stringify(d));
    check('…and the page says DUMPED, what listeners lost, and to carry on', /Dumped ?Listeners lost 8 s\. Carry on, and do not repeat the words\./.test(await text(p.locator('[data-says="dumped"]'))), await text(p.locator('[data-says="dumped"]')));
    for (const w of windows.slice(1)) {
      await act(w.page, 'dump').click();
      await act(w.page, 'dump').click();
      await w.page.locator('[data-says="dumped"]').waitFor();
    }
    await shotAll('dumped');
    // A press the view allowed, refused at the press: the station's words, as they are.
    st.refuseNext = 'You are not on air.';
    await dump.click();
    await dump.click();
    await p.locator('[data-says="refusal"]').waitFor();
    check('a press the station refuses says why, in the station’s words', (await text(p.locator('[data-says="refusal"]'))) === 'You are not on air.');
    // A press the station never answers.
    st.dropPresses = true;
    await act(p, 'end').click();
    await act(p, 'end-yes').click();
    await p.locator('[data-says="refusal"]', { hasText: 'did not answer' }).waitFor();
    check('a press the station does not answer says so, and to look at the lamp first', (await text(p.locator('[data-says="refusal"]'))) === 'The station did not answer. Look at the lamp before you press again.');
    st.dropPresses = false;
    // A press on a connection that has stalled: taken, never answered. The controls must come back.
    st.holdPresses = true;
    await dump.click();
    await dump.click();
    const held = Date.now();
    await sleep(1500);
    check('while a press waits for its answer the button says so, and a second press is not sent', (await dump.getAttribute('aria-busy')) === 'true' && st.calls.filter((c) => c.held).length === 1);
    await p.locator('[data-says="refusal"]', { hasText: 'did not answer' }).waitFor({ timeout: 12000 });
    const waited = Date.now() - held;
    check('a press that is never answered gives up after about 8 s and says so', waited > 6000 && waited < 11000, `${waited} ms`);
    st.holdPresses = false;
    const sent = posts(st, /\/dump$/).filter((c) => !c.held).length;
    await dump.click();
    await dump.click();
    await p.waitForFunction(() => !document.querySelector('[data-says="refusal"]'));
    check('…and the controls work again afterwards', posts(st, /\/dump$/).filter((c) => !c.held).length === sent + 1);
    await clean(st, 'Dump');
  },

  // Dump at Solo: in sight, off, with the reason; pressing it sends nothing.
  async solo() {
    const st = server.use(onAir({ step: 'solo' }));
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    const dump = act(p, 'dump');
    check('at Solo, Dump is in sight and off', (await dump.getAttribute('aria-disabled')) === 'true' && (await text(dump)) === 'Dump');
    check('…with the station’s reason beside it', (await text(panel(p, 'controls'))).includes('Dump is off at Solo. It comes on at Callers.'));
    await dump.click({ force: true });
    await dump.click({ force: true });
    await dump.focus();
    await p.keyboard.press('Enter');
    await p.keyboard.press('Enter');
    await sleep(400);
    check('pressing it, twice, by pointer and by key, sends nothing', posts(st, /\/dump$/).length === 0 && (await text(dump)) === 'Dump');
    await shotAll('solo');
    await clean(st, 'Dump at Solo');
  },

  // The other sentences a host on air reads: the next host absent, a held handover, OPEN.
  async others() {
    let st = server.use(onAir());
    st.setNow(at('16:01:18'));
    st.next = { ...heatherNext(), state: 'not_connected', at_desk: false, passes: false };
    st.post = { mine: false, show_name: 'Kettle Drive', start_ms: at('16:00'), host: 'Heather Douglas', host_state: 'not_connected' };
    describedAs('view with the next host absent', 'MonitorView', st.view());
    await everyone(st);
    await lampIs(main.page, 'on_air');
    check('the next host absent: You keep the air, with no count', (await text(panel(main.page, 'lamp').locator('.say'))).includes('Heather Douglas has not connected. You keep the air.') && (await panel(main.page, 'lamp').locator('.cnt').count()) === 0);
    await shotAll('keeps-air');

    st = server.use(grace());
    st.post = { ...st.post, grace_ends_ms: null, held_by: 'Shona' };
    describedAs('view with the handover held', 'MonitorView', st.view());
    await everyone(st);
    await lampIs(main.page, 'on_air');
    check('a held handover says who holds it, with no count', (await text(panel(main.page, 'lamp').locator('.say'))).includes('Handover held by Shona. It is by a press only.') && (await panel(main.page, 'lamp').locator('.cnt').count()) === 0);
    await shotAll('held');

    st = server.use(onAir({ playout_unknown: true }));
    describedAs('view open', 'MonitorView', st.view());
    await everyone(st);
    await lampIs(main.page, 'open');
    let lamp = panel(main.page, 'lamp');
    check('OPEN is the white lamp and never the red one', (await text(lamp.locator('.biglamp.open'))) === 'Open' && (await main.page.locator('.biglamp.onair, .fill-onair').count()) === 0 && (await text(lamp.locator('.say'))).includes('Open. The station hears you. Listeners hear the fallback.'));
    await shotAll('open');

    // The station's own clock is set back a minute (a correction, a restart) and the host is taken
    // off air: the page follows at once, and does not hold the old state as if it were newer.
    st = server.use(grace());
    await everyone(st);
    await lampIs(main.page, 'on_air');
    await sleep(1200);
    st.setNow(st.now() - 60_000);
    st.on_air = false;
    st.post = null;
    await lampIs(main.page, 'standby', 4000).catch(() => {});
    await sleep(1300);
    const stepped = await text(panel(main.page, 'clock'));
    check('the station’s clock set back a minute: the lamp follows the station, and so does the time', (await panel(main.page, 'lamp').getAttribute('data-state')) === 'standby' && (await main.page.locator('.biglamp.onair').count()) === 0 && /16:00:[12]\d/.test(stepped), stepped);

    st = server.use(onAir({ playout_unknown: true }));
    await everyone(st);
    await lampIs(main.page, 'open');
    // A state this page does not know gives no lamp at all, and the station's sentence.
    st.lamp = () => 'rehearsing';
    st.says = () => ['Rehearsing. Nobody hears you.', null];
    await main.page.locator('[data-panel="monitor.lamp"][data-state="rehearsing"]').waitFor();
    check('a state the page does not know shows no lamp, only the station’s sentence', (await main.page.locator('.biglamp').count()) === 0 && (await text(lamp.locator('.say'))).includes('Rehearsing. Nobody hears you.'));
    await clean(st, 'the other sentences');
  },

  // The station has lost his stream; then the place is let go.
  async linkLost() {
    const st = server.use(onAir());
    Object.assign(st, { conn: 'lost', kept_until: at('14:46:00') });
    describedAs('view with the link lost', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'link_lost');
    const lamp = panel(p, 'lamp');
    const cnt = await text(lamp.locator('.cnt'));
    check('LINK LOST: hollow, in the serious colour, never the red lamp', (await text(lamp.locator('.biglamp.lost'))) === 'Link lost' && (await p.locator('.biglamp.onair, .fill-onair').count()) === 0);
    check('…the station’s sentence, and the place kept, counted', (await text(lamp.locator('.say'))).includes('The station has lost your stream. Listeners hear automation. Reconnect your software: you are back on air 5 s after it is steady.') && /Your place is kept until 14:46:00/.test(cnt) && /4:[0-5]\d/.test(cnt), cnt);
    check('The station hears you says where the fault is', (await text(panel(p, 'source'))).includes('Nothing is arriving from your software. This page is still connected, so the fault is between your software and the station.'));
    const ctl = panel(p, 'controls');
    check('Dump stays in sight, off, with the station’s reason; nothing else to press', (await act(p, 'dump').getAttribute('aria-disabled')) === 'true' && (await text(ctl)).includes('You are not on air.') && (await ctl.locator('button:not(dialog button)').count()) === 1);
    await shotAll('link-lost');
    Object.assign(st, { conn: 'none', kept_until: null, place_gone: true });
    describedAs('view with the place gone', 'MonitorView', st.view());
    await lampIs(p, 'not_connected');
    const say = await text(lamp.locator('.say'));
    check('the place let go is said once, not twice', say.split('Connect again and you will be standing by.').length === 2 && say.includes('Your place was kept for 5 minutes.'), say);
    await sleep(1200);
    await shotAll('place-gone');
    await clean(st, 'a lost link');
  },

  // A private test, the evening before; and a refusal of the software.
  async test() {
    let st = server.use(new MonitorStation({ now: at('20:14:36', '2026-11-18') }));
    Object.assign(st, HEATHER, { conn: 'test', level: -19.0, steady_ms: 18_000 });
    st.rows = { before: null, after: null };
    describedAs('view in a test', 'MonitorView', st.view());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'test');
    const lamp = panel(p, 'lamp');
    check('TEST: the dashed lamp, the station’s sentence, nothing that can put her on air', (await text(lamp.locator('.biglamp.test'))) === 'Test' && (await text(lamp.locator('.say'))).includes('Test. The station hears you. Nobody else does.') && (await text(panel(p, 'controls'))).includes('Nothing here can put you on air.') && (await panel(p, 'controls').locator('button:not(dialog button)').count()) === 0);
    const marks = await panel(p, 'source').locator('.checks li').evaluateAll((l) => l.map((x) => `${x.dataset.mark}:${x.querySelector('b').firstChild.textContent}`));
    check('the checks: connected, the level as a reading with no verdict, steady not yet', JSON.stringify(marks) === JSON.stringify(['ok:Connected', 'read:Level', 'wait:Steady for 30 s']) && (await text(panel(p, 'source'))).includes('−19.0 dBFS as the station reads it'), JSON.stringify(marks));
    check('the monitor says nothing plays in a test', (await text(panel(p, 'listen'))).includes('Nothing plays here until you are standing by.'));
    await shotAll('test');
    st.steady_ms = 31_000;
    await p.locator('[data-panel="monitor.source"] .checks li:nth-child(3)[data-mark="ok"]').waitFor();
    check('steady for 30 s gets its tick', true);

    st = server.use(new MonitorStation({ now: at('13:41:52') }));
    Object.assign(st, HEATHER, { conn: 'none' });
    st.refusal = { at_ms: at('13:41:20'), reason: 'wrong_key', words: 'The key is wrong.' };
    st.rows = { before: row('Steeped', 'Finlay Grant', 'not_connected', '14:00', '16:00'), after: null };
    describedAs('view not connected, refused', 'MonitorView', st.view());
    await everyone(st);
    await lampIs(p, 'not_connected');
    const cnt = await text(panel(p, 'lamp').locator('.cnt'));
    check('NOT CONNECTED: the dashed lamp, and the next show counted', (await text(panel(p, 'lamp').locator('.biglamp.notconn'))) === 'Not connected' && /Your show starts in/.test(cnt) && /2:1[78]:\d\d/.test(cnt) && /today at 16:00/.test(cnt), cnt);
    check('a refusal of her software is said in words, with its time', (await text(panel(p, 'source'))).includes('The station refused your software at 13:41: the key is wrong.'), await text(panel(p, 'source')));
    check('nobody after her: the row says so', /After you Nobody Nothing is scheduled after you\./.test(await text(panel(p, 'next'))));
    for (const w of windows) await w.page.locator('[data-panel="monitor.details"] summary').click();
    await shotAll('refused');
    await clean(st, 'a test and a refusal');
  },

  // The page loses the station: no stale ON AIR, anywhere, and no press goes out.
  async lostStation() {
    const st = server.use(grace());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    await act(p, 'listen').click();
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor();
    // The stream stays open and says nothing: the page must notice by itself.
    st.silent = true;
    const t = Date.now();
    await lampIs(p, 'away', 9000);
    const took = Date.now() - t;
    const lamp = panel(p, 'lamp');
    check('a stream that goes quiet without closing is noticed within about 4 s', took < 5500, `${took} ms`);
    check('then the page shows no lamp and nothing filled as on air, anywhere', (await p.locator('.biglamp').count()) === 0 && (await p.locator('.fill-onair, .fill-open, .lit-cue').count()) === 0);
    check('…and says so: This page has lost the station. Trying again…', (await text(lamp.locator('.say'))).includes('This page has lost the station. Trying again…') && /Last heard at 16:01:\d\d\./.test(await text(lamp.locator('.say'))), await text(lamp.locator('.say')));
    check('…with no count, no chip in the rundown, no controls, and no claim about what the station hears', (await lamp.locator('.cnt').count()) === 0 && (await panel(p, 'next').locator('.state').count()) === 0 && (await panel(p, 'controls').locator('button:not(dialog button)').count()) === 0 && (await text(panel(p, 'source'))).includes('This page cannot say what the station hears'));
    const before = posts(st, /\/(go|not-yet|cue|handover|end-show|dump)$/).length;
    await p.keyboard.press('Enter');
    await sleep(200);
    check('nothing can be pressed while it is lost', posts(st, /\/(go|not-yet|cue|handover|end-show|dump)$/).length === before);
    for (const w of windows.slice(1)) await lampIs(w.page, 'away', 9000);
    await shotAll('lost-station');
    st.silent = false;
    await lampIs(p, 'on_air', 12000);
    check('when the station is back the lamp is back: ON AIR', (await lamp.locator('.biglamp.onair').count()) === 1);
    // The connection itself fails, and the station refuses new ones for a while.
    st.down = true;
    st.cut();
    await lampIs(p, 'away', 9000);
    check('a dropped connection is the same: no lamp, the sentence', (await p.locator('.biglamp').count()) === 0 && (await p.locator('.fill-onair').count()) === 0);
    st.down = false;
    await lampIs(p, 'on_air', 12000);
    check('…and it comes back by itself', true);
    await clean(st, 'the page losing the station');
  },

  // The link's own refusals: an ended key, a wrong secret, no secret, too many pages.
  async link() {
    let st = server.use(grace());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    await act(p, 'listen').click();
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor();
    st.endKey();
    await p.locator('[data-says="gone"]').waitFor();
    check('an ended key: only the sentence is shown', (await text(p.locator('main'))) === 'This link no longer works. Ask the station for a new one.' && (await p.locator('[data-panel]').count()) === 0, await text(p.locator('main')));
    check('…and the page leaves the monitor room', await p.evaluate(() => globalThis.__rooms.at(-1).log.left));
    const n = st.calls.length;
    await sleep(2500);
    check('…and stops asking the station', st.calls.length === n, `${st.calls.length - n} more requests`);
    await shotAll('ended', { quiet: true });

    st = server.use(grace());
    await everyone(st, { secret: 'notTheSecret1234', wait: false });
    await p.locator('[data-says="gone"]').waitFor();
    check('a wrong secret: only the station’s sentence', (await text(p.locator('main'))) === 'This link no longer works. Ask the station for a new one.');
    check('…sent as a bearer, never in an address', st.calls.every((c) => c.bearer === 'notTheSecret1234') && main.urls.every((u) => !u.includes('notTheSecret1234')));
    for (const w of windows) w.urls.length = 0;

    st.down = true;
    await p.goto('about:blank');
    await p.goto(`http://127.0.0.1:${PORT}/monitor/#${st.secret}`);
    await p.locator('[data-says="opening"]', { hasText: 'cannot reach the station' }).waitFor({ timeout: 8000 });
    check('a station that cannot serve the page from the first moment: the page says so, and keeps trying', (await text(p.locator('main'))) === 'This page cannot reach the station. Trying again…');
    st.down = false;
    await lampIs(p, 'on_air', 12000);
    check('…and opens by itself when the station can', true);
    main.urls.length = 0;

    await p.goto('about:blank');
    await sleep(300);
    const asked = st.calls.length;
    await p.goto(`http://127.0.0.1:${PORT}/monitor/`);
    await p.locator('[data-says="gone"]').waitFor();
    await sleep(500);
    check('no secret at all: the page asks for the whole link, and asks the station nothing', (await text(p.locator('main'))).includes('This page needs the link the station sent you.') && st.calls.length === asked, `${st.calls.length - asked} requests`);

    st = server.use(grace());
    st.tooMany = 'Too many pages are open on this link. Close one.';
    await everyone(st, { wait: false });
    await p.locator('[data-says="refused"]').waitFor();
    check('too many pages on the link is its own sentence, not “no longer works”', (await text(p.locator('main'))).includes('Too many pages are open on this link. Close one.') && (await text(p.locator('main'))).includes('This page tries again by itself.'));
    await shotAll('too-many', { quiet: true });
    st.tooMany = null;
    await lampIs(p, 'on_air', 12000);
    check('…and the page gets in by itself when there is room', true);
    await clean(st, 'the link’s refusals');
  },

  // The listeners' stream while standing by: only while the desk's mix is silent, and cut at once
  // when it need not play (on air, someone else on air, views gone, no stream set up).
  async stream() {
    const st = server.use(new MonitorStation({ now: at('13:57:12') }));
    st.post = { mine: true, show_name: 'Steeped', start_ms: at('14:00'), host: 'Finlay Grant', host_state: 'standing_by', on_air_at_ms: at('14:00') };
    st.rows = { before: row('The Long Lunch', 'Duncan Boyd', 'in_room', '12:00', '14:00'), after: null };
    const p = main.page;
    const w = main;
    await p.goto('about:blank');
    await p.goto(`http://127.0.0.1:${PORT}/monitor/#${st.secret}`);
    await lampIs(p, 'standby');
    const audio = p.locator('audio[data-monitor="stream"]');
    const playing = async () => {
      await p.waitForFunction(() => document.querySelector('audio[data-monitor="stream"]')?.paused === false, null, { timeout: 8000 });
      await until('the stream to be asked for', () => st.listenerStream.open === 1);
    };
    check('before the press nothing is fetched from the listener stream', st.listenerStream.hits === 0 && (await audio.count()) === 0);
    check('before the press the card says what Start listening will bring', (await text(panel(p, 'listen').locator('.small'))).startsWith('You will hear what listeners hear, about 23 s behind'), await text(panel(p, 'listen').locator('.small')));
    await act(p, 'listen').click();
    await playing();
    check('the press starts the stream', st.listenerStream.open === 1);

    // Someone on air at the desk: their voice is in the mix, live, so the stream stops.
    st.others = ['Duncan Boyd'];
    st.push();
    await audio.waitFor({ state: 'detached', timeout: 6000 });
    await sleep(300);
    check('someone else on air at the desk: the stream stops and its connection is dropped', st.listenerStream.open === 0, JSON.stringify(st.listenerStream));
    check('…and the card goes back to the desk’s own mix', (await text(panel(p, 'listen').locator('.small'))).startsWith('Everyone but you · cue · talkback.'), await text(panel(p, 'listen').locator('.small')));
    await sleep(500);
    const bar = await panel(p, 'listen').locator('.level i').evaluate((i) => i.getBoundingClientRect().width);
    check('…and the level bar is back, showing what arrives', bar > 4, `bar ${bar}px`);
    st.others = [];
    st.push();
    await playing();
    check('they go: the stream comes back', st.listenerStream.hits === 2 && st.listenerStream.open === 1, JSON.stringify(st.listenerStream));

    // The host goes on air: the stream is cut within a view.
    Object.assign(st, { on_air: true });
    st.push();
    await audio.waitFor({ state: 'detached', timeout: 3000 });
    await lampIs(p, 'on_air');
    await sleep(300);
    check('on air: the stream is gone at once, so the host never hears themselves late', st.listenerStream.open === 0 && (await audio.count()) === 0);
    check('…and the card is the desk’s own mix', (await text(panel(p, 'listen').locator('.small'))).startsWith('Everyone but you · cue · talkback.'), await text(panel(p, 'listen').locator('.small')));
    Object.assign(st, { on_air: false });
    st.push();
    await playing();

    // The station stops telling the page: it does not know the host's state, so it plays nothing.
    st.silent = true;
    await audio.waitFor({ state: 'detached', timeout: 8000 });
    await sleep(300);
    check('no views: the stream is cut', st.listenerStream.open === 0);
    st.silent = false;
    await playing();
    check('views again: it comes back', st.listenerStream.open === 1, JSON.stringify(st.listenerStream));
    await clean(st, 'the listeners’ stream');

    // OPEN: the host is live to the room and automation holds the air, so listeners do not hear
    // them. They hear the station's own output, with the room's voices not in it, and the card
    // says that plainly; it never says "while you stand by".
    const open = server.use(onAir({ auto_mode: true, others: ['Duncan Boyd'] }));
    await p.goto('about:blank');
    await p.goto(`http://127.0.0.1:${PORT}/monitor/#${open.secret}`);
    await lampIs(p, 'open');
    const openSays = async () => text(panel(p, 'listen').locator('.small'));
    check('OPEN, before the press: the card says what Start listening will bring, not that it plays', (await openSays()) === "You will hear the station's output, about 23 s behind. The room's voices are not in it. Cue and talkback still come through, live.", await openSays());
    check('…and it does not say while you stand by', !(await openSays()).includes('stand by'));
    check('OPEN, before the press: nothing is fetched from the listener stream', open.listenerStream.hits === 0 && (await audio.count()) === 0);
    await act(p, 'listen').click();
    await p.waitForFunction(() => document.querySelector('audio[data-monitor="stream"]')?.paused === false, null, { timeout: 8000 });
    await until('the stream to be asked for', () => open.listenerStream.open === 1);
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 8000 });
    check('OPEN, after the press: the station’s output plays, though someone is on air in the room', open.listenerStream.open === 1);
    check('…and the card says it is the station’s output, how far behind, and that the room is not in it', (await openSays()) === "You are hearing the station's output, about 23 s behind. The room's voices are not in it. Cue and talkback still come through, live.", await openSays());
    check('…and no level bar is drawn beside it', (await panel(p, 'listen').locator('.level').count()) === 0);
    await sleep(350);
    await p.screenshot({ path: join(OUT, 'open-listening.png'), fullPage: true });
    // The air passes back to the room (automation lets go): the mix is the programme again.
    Object.assign(open, { auto_mode: false });
    open.push();
    await audio.waitFor({ state: 'detached', timeout: 6000 });
    await lampIs(p, 'on_air');
    check('on air again: the stream stops and the card is the desk’s own mix', open.listenerStream.open === 0 && (await openSays()).startsWith('Everyone but you · cue · talkback.'), await openSays());
    await clean(open, 'the stream in OPEN');

    // A station with no listener stream set up: today's silence and today's words.
    const bare = server.use(new MonitorStation({ now: at('13:57:12') }));
    bare.noStream = true;
    await p.goto('about:blank');
    await p.goto(`http://127.0.0.1:${PORT}/monitor/#${bare.secret}`);
    await lampIs(p, 'standby');
    await act(p, 'listen').click();
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 8000 });
    await sleep(500);
    check('no stream set up: nothing plays and the words are the desk’s own mix', (await audio.count()) === 0 && bare.listenerStream.hits === 0 && (await text(panel(p, 'listen').locator('.small'))).startsWith('Everyone but you · cue · talkback.'), await text(panel(p, 'listen').locator('.small')));
    await clean(bare, 'no stream');
    void w;
  },

  // The monitor's sound when it does not simply work.
  async listening() {
    const st = server.use(grace());
    await everyone(st);
    const p = main.page;
    await lampIs(p, 'on_air');
    const listen = panel(p, 'listen');
    st.tokenRefusal = 'Two browsers are listening already. Close one, or wait a few seconds if you have just reloaded.';
    await act(p, 'listen').click();
    await p.locator('[data-says="listen-problem"]').waitFor();
    check('a refused seat: Start listening stays, with the station’s sentence under it', (await act(p, 'listen').count()) === 1 && (await text(p.locator('[data-says="listen-problem"]'))) === st.tokenRefusal);
    for (const w of windows.slice(1)) {
      await act(w.page, 'listen').click();
      await w.page.locator('[data-says="listen-problem"]').waitFor();
    }
    await shotAll('listen-refused');
    st.tokenRefusal = null;

    st.roomMode = 'fail';
    await act(p, 'listen').click();
    await p.locator('[data-says="listen-problem"]', { hasText: 'could not join' }).waitFor({ timeout: 10000 });
    let rooms = await p.evaluate(() => globalThis.__rooms.map((r) => r.log.connects[0]));
    check('a room that cannot be reached is tried directly, then through the relay, each with a token of its own', rooms.length === 2 && rooms[0].relay === false && rooms[1].relay === true && rooms[0].token !== rooms[1].token, JSON.stringify(rooms));
    check('…and then Start listening stays, with the reason', (await act(p, 'listen').count()) === 1);

    st.roomMode = 'relayonly';
    await act(p, 'listen').click();
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 10000 });
    rooms = await p.evaluate(() => globalThis.__rooms.map((r) => r.log.connects[0]));
    check('a network that blocks the room: the next press tries directly again, then gets in through the relay', rooms.length === 4 && rooms[2].relay === false && rooms[3].relay === true, JSON.stringify(rooms));

    // The room drops under the page: it joins again, with a fresh token.
    st.roomMode = 'ok';
    const had = st.tokens;
    await p.evaluate(() => globalThis.__rooms.at(-1).drop());
    await p.locator('[data-panel="monitor.listen"][data-hearing="rejoining"]').waitFor();
    check('a dropped room says Reconnecting', (await text(listen.locator('.state'))) === 'Reconnecting');
    await p.locator('[data-panel="monitor.listen"][data-sounding="true"]').waitFor({ timeout: 10000 });
    check('…and joins again by itself with a fresh token', st.tokens === had + 1 && (await text(listen.locator('.state'))) === 'Listening', `${st.tokens - had} tokens`);
    await clean(st, 'the monitor’s sound');
  },

  // 14a: every count turns amber in its last 30 s and red in its last 10 s, and says so in words.
  // Each count is moved on to 45 s, 25 s and 5 s from its end (the page follows the station's
  // clock forward at once), and photographed in every window.
  async hurry() {
    const colour = (page, token) => page.evaluate((t) => {
      const probe = document.createElement('i');
      probe.style.color = `var(${t})`;
      document.body.append(probe);
      const c = getComputedStyle(probe).color;
      probe.remove();
      return c;
    }, token);
    const counts = [
      {
        name: 'grace',
        label: /takes the air/,
        station: () => {
          const st = grace();
          return [st, at('16:02:00'), 'on_air'];
        },
      },
      {
        name: 'late',
        label: /On air in/,
        station: () => {
          const st = new MonitorStation({ now: at('16:10:41') });
          Object.assign(st, HEATHER, { auto_mode: true, steady_ms: 11_000 });
          st.post = { mine: true, show_name: 'Kettle Drive', start_ms: at('16:00'), host: 'Heather Douglas', host_state: 'standing_by', late_ends_ms: at('16:10:55'), on_air_at_ms: at('16:10:55') };
          st.rows = { before: row('Steeped', 'Finlay Grant', 'not_connected', '14:00', '16:00'), after: row('Firth Sessions', 'Rory Bruce', 'in_room', '18:00', '20:00') };
          return [st, at('16:10:55'), 'standby'];
        },
      },
      {
        name: 'post',
        label: /On air at 14:00:00/,
        station: () => {
          const st = new MonitorStation({ now: at('13:57:12') });
          st.post = { mine: true, show_name: 'Steeped', start_ms: at('14:00'), host: 'Finlay Grant', host_state: 'standing_by', on_air_at_ms: at('14:00') };
          st.rows = { before: row('The Long Lunch', 'Duncan Boyd', 'in_room', '12:00', '14:00'), after: row('Kettle Drive', 'Heather Douglas', 'not_connected', '16:00', '18:00') };
          return [st, at('14:00'), 'standby'];
        },
      },
      {
        name: 'kept',
        label: /Your place is kept until 14:46:00/,
        station: () => {
          const st = onAir();
          Object.assign(st, { conn: 'lost', kept_until: at('14:46:00') });
          return [st, at('14:46:00'), 'link_lost'];
        },
      },
    ];
    // What the figure's colour must be, by the seconds left when it is read.
    const level = (s) => (s > 30 ? null : s > 10 ? 'amber' : 'red');
    for (const k of counts) {
      const [st, end, lamp] = k.station();
      server.use(st);
      await everyone(st);
      const p = main.page;
      await lampIs(p, lamp);
      for (const off of [45, 25, 5]) {
        // The page reads the station's time at its next view; 2.5 s over the offset leaves it
        // about the offset by the time the figure is read, and a little less by the last window.
        st.setNow(end - off * 1000 - 2500);
        st.push();
        await sleep(1100);
        const want = level(off - 1);
        const tag = `${k.name} at ${off} s`;
        for (const w of windows) {
          const cnt = panel(w.page, 'lamp').locator('.cnt');
          const said = await text(cnt);
          const m = /^(\d+):(\d\d)$/.exec(await text(cnt.locator('b')));
          const left = m ? Number(m[1]) * 60 + Number(m[2]) : -1;
          check(`${tag} (${w.tag}): the count is ${off - 4} to ${off + 2} s and says what it counts to`, k.label.test(said) && left >= off - 4 && left <= off + 2, said);
          const got = await cnt.getAttribute('data-hurry');
          check(`${tag} (${w.tag}): ${want ? `${want}, with its word` : 'plain, with no word'}`, got === want && (await cnt.locator('.hurry').count()) === (want ? 1 : 0) && (!want || (await text(cnt.locator('.hurry'))) === (want === 'red' ? 'Last 10 seconds' : 'Last 30 seconds')), `${got}; ${said}`);
          const fig = await cnt.locator('b').evaluate((b) => ({ color: getComputedStyle(b).color, weight: getComputedStyle(b).fontWeight }));
          const wantColour = want ? await colour(w.page, want === 'red' ? '--status-critical' : '--status-warning') : await colour(w.page, '--ink');
          // WCAG: the figure is 56 px, large text, so 3:1 on the card it sits on is AA.
          const ratio = await w.page.evaluate(() => {
            const lum = (rgb) => {
              const [r, g, b] = rgb.match(/[\d.]+/g).slice(0, 3).map((v) => { const c = Number(v) / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; });
              return 0.2126 * r + 0.7152 * g + 0.0722 * b;
            };
            const fg = lum(getComputedStyle(document.querySelector('.cnt b')).color);
            const bg = lum(getComputedStyle(document.querySelector('[data-panel="monitor.lamp"]')).backgroundColor);
            return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
          });
          check(`${tag} (${w.tag}): the figure is at least 3:1 against its card (AA for large text), ${ratio.toFixed(2)}:1`, ratio >= 3, String(ratio));
          check(`${tag} (${w.tag}): the figure is ${want ? (want === 'red' ? 'the critical' : 'the warning') : 'the ink'} token's colour${want ? ', one weight heavier' : ''}`, fig.color === wantColour && (want ? Number(fig.weight) > 600 : Number(fig.weight) === 600), JSON.stringify(fig) + ' vs ' + wantColour);
          // The picture is taken in the same breath, so the figure in it is the one just checked.
          await w.page.screenshot({ path: join(OUT, `${w.tag}-hurry-${k.name}-${off}.png`), fullPage: true });
          check(`${tag} (${w.tag}): …and in the picture it is still ${want ?? 'plain'}`, (await cnt.getAttribute('data-hurry')) === want, String(await cnt.getAttribute('data-hurry')));
          await fits(w, tag);
        }
        log('shot', `hurry-${k.name}-${off}`);
      }
      await clean(st, `the ${k.name} count's last seconds`);
    }
  },

  // 14a: Start listening is off, in look and in behaviour, where nothing can play.
  async listenOff() {
    const noToken = (st) => posts(st, /\/token$/).length === 0;
    const off = async (w, st, what) => {
      const b = act(w.page, 'listen');
      const reason = (await text(panel(w.page, 'listen').locator('[data-says="listen-off"]'))) || '';
      check(`${what} (${w.tag}): Start listening is off, and the reason is beside it`, (await b.getAttribute('aria-disabled')) === 'true' && (await b.getAttribute('aria-describedby')) === 'listen-off' && reason.length > 20, reason);
      const opacity = await b.evaluate((x) => getComputedStyle(x).opacity);
      const fill = await b.evaluate((x) => [getComputedStyle(x).backgroundColor, getComputedStyle(document.querySelector('.card .btn:not(.primary)') ?? x).backgroundColor]);
      const accent = await w.page.evaluate(() => { const i = document.createElement('i'); i.style.color = 'var(--accent)'; document.body.append(i); const c = getComputedStyle(i).color; i.remove(); return c; });
      check(`${what} (${w.tag}): …and looks off, in the existing disabled style`, Number(opacity) < 0.6, opacity);
      check(`${what} (${w.tag}): …with no accent fill, as a plain button looks`, fill[0] !== accent && fill[0] === fill[1], JSON.stringify(fill) + ' accent ' + accent);
    };
    // The link lost: the station says so, and nothing is asked of it.
    let st = server.use(onAir());
    Object.assign(st, { conn: 'lost', kept_until: at('14:46:00') });
    await everyone(st);
    let p = main.page;
    await lampIs(p, 'link_lost');
    for (const w of windows) await off(w, st, 'link lost');
    for (const w of windows) await act(w.page, 'listen').click({ force: true }); // a tap on an off button, which Playwright would otherwise refuse to make
    await sleep(400);
    check('link lost: pressing it asks for no token and joins no room', noToken(st) && (await p.evaluate(() => globalThis.__rooms.length)) === 0 && (await panel(p, 'listen').getAttribute('data-hearing')) === 'idle', JSON.stringify(st.calls.filter((c) => /token/.test(c.path))));
    await p.locator('[data-act="listen"]').focus();
    await p.keyboard.press('Enter');
    await sleep(300);
    check('…and Enter on it does nothing either', noToken(st));
    check('link lost: the card does not say the reason twice', (await panel(p, 'listen').locator('p.small').count()) === 1);
    await shotAll('listen-off-link-lost');
    // The link comes back: the station is steady and the button wakes.
    Object.assign(st, { conn: 'desk', kept_until: null });
    st.push();
    await lampIs(p, 'on_air');
    check('the link back, standing at the desk again: Start listening is on again', (await act(p, 'listen').getAttribute('aria-disabled')) === null && (await panel(p, 'listen').locator('[data-says="listen-off"]').count()) === 0);
    await clean(st, 'Start listening, link lost');

    // The page loses the station: nothing is known, so nothing is offered.
    st = server.use(grace());
    await everyone(st);
    p = main.page;
    await lampIs(p, 'on_air');
    check('on air, with the station telling the page: Start listening is on', (await act(p, 'listen').getAttribute('aria-disabled')) === null);
    st.silent = true;
    for (const w of windows) await lampIs(w.page, 'away', 9000);
    for (const w of windows) await off(w, st, 'the page has lost the station');
    for (const w of windows) await act(w.page, 'listen').click({ force: true }); // a tap on an off button, which Playwright would otherwise refuse to make
    await sleep(400);
    check('the page has lost the station: pressing it asks for no token', noToken(st));
    await shotAll('listen-off-lost-station');
    st.silent = false;
    await lampIs(p, 'on_air', 12000);
    check('the station back: Start listening is on again', (await act(p, 'listen').getAttribute('aria-disabled')) === null);
    await clean(st, 'Start listening, the page losing the station');

    // A test, and not connected: not at the desk, so no mix to hear. Standing by is on.
    st = server.use(new MonitorStation({ now: at('20:14:36', '2026-11-18') }));
    Object.assign(st, HEATHER, { conn: 'test', level: -19.0, steady_ms: 18_000 });
    st.rows = { before: null, after: null };
    await everyone(st);
    p = main.page;
    await lampIs(p, 'test');
    for (const w of windows) await off(w, st, 'a test');
    check('a test: the reason is the sentence the card already had', (await text(panel(p, 'listen').locator('[data-says="listen-off"]'))).startsWith('Nothing plays here until you are standing by.'));
    await shotAll('listen-off-test');
    st = server.use(new MonitorStation({ now: at('13:57:12') }));
    st.post = { mine: true, show_name: 'Steeped', start_ms: at('14:00'), host: 'Finlay Grant', host_state: 'standing_by', on_air_at_ms: at('14:00') };
    st.rows = { before: null, after: null };
    await everyone(st);
    await lampIs(main.page, 'standby');
    check('standing by with no mix yet: Start listening is on (the press is what asks for the mix)', (await act(main.page, 'listen').getAttribute('aria-disabled')) === null);
    await clean(st, 'Start listening, a test');
  },
};

/**
 * The build a station serves (apps/web/dist, with the real livekit-client), opened once through
 * the same stand-in under the same Content-Security-Policy: the page loads, shows its first
 * view, and Start listening takes the real room client as far as it can go without a room. Two
 * rooms that are not there: a port where nothing listens (a direct attempt, the relay, then the
 * sentence), and an address that answers and is not a room, which the room client alone would
 * try for ever. Not a real room: that is `just test-monitor`'s.
 */
async function shipped() {
  const dist = join(ROOT, 'apps', 'web', 'dist');
  if (!process.env.SKIP_BUILD) execFileSync('pnpm', ['exec', 'vite', 'build', '--logLevel', 'warn'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
  if (!existsSync(join(dist, 'monitor', 'index.html'))) return check('the build a station serves holds monitor/index.html', false, dist);
  const real = await serve(dist, PORT + 1, PORT + 2);
  const st = real.use(grace());
  st.roomUrl = `ws://127.0.0.1:${PORT + 2}`;
  const w = await open({ width: 1440, height: 900 }, 'dark');
  try {
    await w.page.goto(`http://127.0.0.1:${PORT + 1}/monitor/#${st.secret}`);
    await lampIs(w.page, 'on_air', 15000);
    check('the build a station serves: the page opens and shows its first view', (await text(panel(w.page, 'lamp').locator('.biglamp.onair'))) === 'On air' && (await text(w.page.locator('.top .station'))) === 'Kettle FM');
    check('…with its own noindex', (await w.page.locator('meta[name="robots"]').getAttribute('content')) === 'noindex, nofollow');
    let t = Date.now();
    await act(w.page, 'listen').click();
    await w.page.locator('[data-says="listen-problem"]', { hasText: 'could not join' }).waitFor({ timeout: 70000 });
    let tokens = posts(st, /\/token$/).length;
    check('…Start listening takes the real room client to a room that is not there: a direct attempt, then the relay, each with its own token', tokens === 2, `${tokens} tokens`);
    check('…and then says so, with Start listening still there', (await act(w.page, 'listen').count()) === 1, `${Date.now() - t} ms`);
    const csp = await w.page.evaluate(() => globalThis.__csp ?? []);
    check('…with no page error and nothing the Content-Security-Policy refused', csp.length === 0 && w.errors.length === 0, [...w.errors, ...csp].join('; '));
    check('…and the secret in no address', w.urls.every((u) => !u.includes(st.secret)));
    await w.page.screenshot({ path: join(OUT, '1440-dark-shipped-build.png'), fullPage: true });
  } finally {
    await w.ctx.close();
    await real.close();
  }
  // An address that answers every request with 404: the room client keeps trying; the page stops it.
  const odd = await serve(dist, PORT + 1);
  const st2 = odd.use(grace());
  const w2 = await open({ width: 1440, height: 900 }, 'dark');
  try {
    await w2.page.goto(`http://127.0.0.1:${PORT + 1}/monitor/#${st2.secret}`);
    await lampIs(w2.page, 'on_air', 15000);
    const t = Date.now();
    await act(w2.page, 'listen').click();
    await w2.page.locator('[data-says="listen-problem"]', { hasText: 'could not join' }).waitFor({ timeout: 80000 });
    check('an address that answers and is not a room does not leave the page Connecting for ever', (await act(w2.page, 'listen').count()) === 1, `${Date.now() - t} ms`);
    log(`   (gave up after ${Math.round((Date.now() - t) / 1000)} s)`);
  } finally {
    await w2.ctx.close();
    await odd.close();
  }
}

try {
  for (const [name, run] of Object.entries({ ...scenarios, shipped })) {
    if (ONLY && !ONLY.has(name)) continue;
    log(`\n== ${name}`);
    await run();
  }
} finally {
  await browser.close();
  await server.close();
}

const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
