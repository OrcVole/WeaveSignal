// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The caller's page on a phone, without a stack: the player page as it ships (apps/web/dist),
// a station that is only answers given to the page's own requests, and a room that never
// answers (so the call stays Connecting) or refuses (so the line is lost). It checks what
// `just test-m3` found and could not explain: once the call panel was up, the page laid out
// 799 px wide in a 390 px window, so "Reconnect through the relay" was off the screen and the
// tap never landed. The cause was a rule for the station's banner picture (`.banner`, 3:1) that
// also matched the call panel's own banner, a `div`: a 3:1 box round its words is three times as
// wide as the words are tall. The rule is the picture's alone now (`img.banner`, web.css), and
// this keeps both sides: the pages stay as wide as the window, and the picture keeps its shape.
//
//   cd apps/web && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/m3/layout.mjs
//   just test-join-layout          # the same, with PLAYWRIGHT_CORE from the environment
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs). Screenshots go to
// OUT (target/shots-join-layout unless told); DIST is another build of the pages to check.
// Ports PORT, PORT+1 and PORT+2 (29430 to 29432 unless told), on loopback. What this does not prove: anything about a real room or a real call;
// that is `just test-m3`'s, with its stack.
//
// The join page's own states (1.3: the rehearsal studio, the move to the live studio, standing by)
// are laid out from a second build of the join page alone, with tests/m3/fake-room.js in
// livekit-client's place (apps/web/vite.joinstand.config.ts; this script builds it, into
// target/join-standin/dist, unless STANDIN names another). It proves layout and words at 390 and
// 320 px, not the move's order against a real room (the vitest suite's) or a real room (the stack's).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { createServer as createTcp } from 'node:net';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = process.env.DIST ?? join(ROOT, 'apps', 'web', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-join-layout');
const PORT = Number(process.env.PORT ?? 29430);
const STANDIN = process.env.STANDIN ?? join(ROOT, 'target', 'join-standin', 'dist');
mkdirSync(OUT, { recursive: true });
if (!process.env.STANDIN) execFileSync('pnpm', ['exec', 'vite', 'build', '-c', 'vite.joinstand.config.ts', '--logLevel', 'warn'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
if (!existsSync(join(STANDIN, 'join', 'index.html'))) throw new Error(`no stand-in build at ${STANDIN}`);
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/web first`);

const log = (...a) => console.log(...a);
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

// ---- the page as it ships, and a room that accepts a connection and never says a word ----
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.json': 'application/json', '.woff2': 'font/woff2' };
const serve = (dist) =>
  createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    const file = normalize(join(dist, path.endsWith('/') ? `${path}index.html` : path));
    if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  });
const web = serve(DIST);
await new Promise((r) => web.listen(PORT, '127.0.0.1', r));
const standin = serve(STANDIN);
await new Promise((r) => standin.listen(PORT + 2, '127.0.0.1', r));
const STAND = `http://127.0.0.1:${PORT + 2}`;
const held = new Set();
const silent = createTcp((s) => {
  held.add(s);
  s.on('error', () => {});
  s.on('close', () => held.delete(s));
});
await new Promise((r) => silent.listen(PORT + 1, '127.0.0.1', r));
const WEB = `http://127.0.0.1:${PORT}`;
const ROOM_SILENT = `ws://127.0.0.1:${PORT + 1}`;
// Nothing listens on port 9 (discard) here: the connection is refused at once, and the line is lost.
const ROOM_REFUSES = 'ws://127.0.0.1:9';

// The station's banner picture, made here so the repository carries no binary for it: 30 by 10
// of one colour (the page draws it 3:1 at the width of the page by its own rule).
function crc32(buf) {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}
function png(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) raw.set(rgba, y * (width * 4 + 1) + 1 + x * 4);
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(width, 0);
  head.writeUInt32BE(height, 4);
  head.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from('\x89PNG\r\n\x1a\n', 'latin1'), chunk('IHDR', head), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
const PNG = png(30, 10, [180, 83, 42, 255]);
const CONSENT = "You'll be live on Kettle FM, and we may keep your call in our archive and podcast for 42 days. To have it taken down, contact calls@kettle.example.";

/** The station, as answers to the page's own requests. Sample people are from the Tartan list. */
async function station(page, { room, look = false }) {
  const json = (route, body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  await page.route('**/api/v1/**', (route) => {
    const p = new URL(route.request().url()).pathname;
    if (p === '/api/v1/player') {
      return json(route, { station: 'Kettle FM', show: 'The Morning Kettle', stream_url: '', streams: [], presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, call_in: { open: true, why_not: null, consent_text: CONSENT, consent_version: 'v1', name_max: 60, topic_max: 200 }, relay: { turn_tls_port: 5349, stun_servers: [] }, video: null });
    }
    if (p === '/api/v1/branding') return json(route, { accent: null, icon: null, banner: look ? { url: '/api/v1/branding/banner?v=0123456789abcdef', width: 1200, height: 400 } : null });
    if (p === '/api/v1/branding/banner') return route.fulfill({ status: 200, contentType: 'image/png', body: PNG });
    // No challenge at this station: the lobby's answer is ready at once.
    if (p === '/api/v1/call-in/challenge') return json(route, { token: '', salt: '', bits: 0, expires_at_ms: Date.now() + 600_000 });
    // The call, and the fresh room token a rejoin asks for (Reconnect through the relay).
    if (p === '/api/v1/call-in' || p === '/api/v1/pass/token') {
      return json(route, { pass: 'layout-test-pass', token: 'layout-test-token', url: room, room: 'studio-booth-caller-jessie', identity: 'caller-jessie', role: 'caller', name: 'Jessie Brown', hear: 'desk-return-caller-jessie', see: null, pass_expires_at_ms: Date.now() + 3_600_000 });
    }
    if (p === '/api/v1/pass/events') {
      const me = { identity: 'caller-jessie', role: 'caller', phase: 'joining', open: false, muted: false, place: 1, queue: 1, screener_talking: false, talkback: false, held_until_ms: null, station: 'Kettle FM', show: 'The Morning Kettle', presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, desk_reachable: true };
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: `data: ${JSON.stringify(me)}\n\n` });
    }
    return json(route, { error: 'not found' }, 404);
  });
}

/** How wide the page lays out, and everything in it that reaches past the window's right edge. */
const measure = (page) =>
  page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const past = [...document.querySelectorAll('.page *')]
      .map((el) => ({ el, r: el.getBoundingClientRect() }))
      .filter((x) => x.r.width > 0 && x.r.right > vw + 1)
      .map((x) => `${x.el.tagName.toLowerCase()}${x.el.className && typeof x.el.className === 'string' ? `.${x.el.className.split(' ')[0]}` : ''} to ${Math.round(x.r.right)}`);
    return { inner: window.innerWidth, scroll: document.scrollingElement.scrollWidth, past: past.slice(0, 6) };
  });
function fits(name, m, width) {
  // On a phone a page wider than the screen is laid out at its own width: `innerWidth` grows with it.
  check(`${name}: the page is as wide as the window (${width} px), and nothing reaches past its edge`, m.inner === width && m.scroll <= width + 1 && m.past.length === 0, JSON.stringify(m));
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] });

/** The caller's way in, as tests/m3/relay.mjs taps it: the player, Call the studio, the lobby, Call. */
async function call(width, { room, look = false }) {
  const ctx = await browser.newContext({ viewport: { width, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true, permissions: ['microphone'], colorScheme: 'dark' });
  const page = await ctx.newPage();
  await station(page, { room, look });
  await page.goto(`${WEB}/`);
  await page.getByRole('button', { name: 'Call the studio' }).tap();
  await page.locator('#f-name').waitFor();
  fits(`${width} px, the lobby${look ? ', with the station’s banner' : ''}`, await measure(page), width);
  await page.locator('#f-name').tap();
  await page.keyboard.type('Jessie Brown');
  await page.locator('#f-topic').tap();
  await page.keyboard.type('Calling from a phone that blocks UDP');
  await page.locator('#f-consent').tap();
  await page.locator('form button[type="submit"]').tap();
  return { ctx, page };
}

/** The station as a join link sees it (1.3). `studio` is where the link leads, `states` the
 *  frames the pass's events send in turn (the last repeats), `later` the live pass's answers. */
async function joinStation(page, { studio, role, not_now = null, room = `ws://127.0.0.1:${PORT + 1}`, frames, liveFrames, tokenDelayMs = 0, show = 'Steeped' }) {
  const json = (route, body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  const me = (over) => ({ identity: 'gregor', role, phase: 'off_air', open: false, muted: false, place: null, queue: 0, screener_talking: false, talkback: false, held_until_ms: null, station: 'Kettle FM', show, presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, desk_reachable: true, studio, ...over });
  const info = (s, pass) => ({ kind: 'invite', role, identity: 'gregor', name: 'Gregor Stewart', station: 'Kettle FM', show: s === 'rehearsal' ? show : 'The Morning Kettle', expires_at_ms: Date.now() + 3_600_000, ended: false, relay: { turn_tls_port: 5349, stun_servers: [] }, not_now: s === studio ? not_now : null, studio: s, pass });
  await page.route('**/api/v1/**', async (route) => {
    const req = route.request();
    const p = new URL(req.url()).pathname;
    const bearer = (req.headers().authorization ?? '').replace(/^Bearer /, '');
    const live = bearer === 'live-pass';
    if (p === '/api/v1/player') return json(route, { station: 'Kettle FM', show: 'The Morning Kettle', stream_url: '', streams: [], presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, call_in: { open: true, why_not: null, consent_text: CONSENT, consent_version: 'v1', name_max: 60, topic_max: 200 }, relay: { turn_tls_port: 5349, stun_servers: [] }, video: null });
    if (p === '/api/v1/branding') return json(route, { accent: null, icon: null, banner: null });
    if (p === '/api/v1/pass') return json(route, info(live ? 'live' : studio));
    if (p === '/api/v1/pass/token') {
      if (live && tokenDelayMs) await new Promise((r) => setTimeout(r, tokenDelayMs));
      return json(route, { pass: bearer, token: 'x', url: live ? `${room}?live` : room, room: live ? 'studio' : 'studio-rehearsal', identity: 'gregor', role, name: 'Gregor Stewart', hear: 'desk-mm-gregor', see: null, pass_expires_at_ms: Date.now() + 3_600_000 }, 201);
    }
    if (p === '/api/v1/pass/events') {
      const list = (live ? (liveFrames ?? frames) : frames) ?? [];
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: list.map((f) => `event: me\ndata: ${JSON.stringify(me(f))}\n\n`).join('') });
    }
    return json(route, { error: 'not found' }, 404);
  });
}

const MOVE = { pass: 'live-pass', link: `${STAND}/join/#live-pass`, show: 'Steeped', airing_start_ms: Date.now() + 25 * 60_000 };

/** The join page at its 1.3 states, at 390 and 320 px: each fits the window, and is shot. */
async function joinStates() {
  const connected = { phase: 'on_air', studio: 'rehearsal' };
  const cases = [
    // [name, options, what to wait for, extra checks]
    ['rehearsal-card', { studio: 'rehearsal', role: 'guest', frames: [] }, async (page) => page.getByRole('button', { name: 'Join the rehearsal studio' }).waitFor()],
    ['rehearsal-closed', { studio: 'rehearsal', role: 'guest', frames: [], not_now: 'The rehearsal studio for Steeped is not open. Ask whoever invited you to open it again.' }, async (page) => page.getByRole('heading', { name: 'Not just now' }).waitFor()],
    ['live-card-presenter', { studio: 'live', role: 'presenter', frames: [] }, async (page) => page.getByRole('button', { name: 'Join the studio' }).waitFor()],
    ['rehearsal-in-the-mix', { studio: 'rehearsal', role: 'guest', frames: [connected] }, async (page) => page.locator('.banner[data-phase="on_air"][data-link="connected"]').waitFor({ timeout: 10_000 })],
    ['rehearsal-not-in-the-mix', { studio: 'rehearsal', role: 'presenter', frames: [{ ...connected, phase: 'off_air' }] }, async (page) => page.locator('.banner[data-phase="off_air"][data-link="connected"]').waitFor({ timeout: 10_000 })],
    ['live-standing-by-guest', { studio: 'live', role: 'guest', frames: [{ studio: 'live', phase: 'off_air' }] }, async (page) => page.locator('.banner[data-phase="off_air"][data-link="connected"]').waitFor({ timeout: 10_000 })],
    ['live-standing-by-dj', { studio: 'live', role: 'dj', frames: [{ studio: 'live', phase: 'off_air' }] }, async (page) => page.locator('.banner[data-phase="off_air"][data-link="connected"]').waitFor({ timeout: 10_000 })],
    // The move: the live pass's token is slow, so the page is seen on "Moving you…", then standing by.
    ['moving', { studio: 'rehearsal', role: 'guest', frames: [{ ...connected, move: MOVE }], liveFrames: [{ studio: 'live', phase: 'off_air' }], tokenDelayMs: 2500 }, async (page) => page.locator('.banner[data-link="moving"]').waitFor({ timeout: 10_000 })],
    ['moved-standing-by', { studio: 'rehearsal', role: 'guest', frames: [{ ...connected, move: MOVE }], liveFrames: [{ studio: 'live', phase: 'off_air' }] }, async (page) => page.locator('.banner[data-link="connected"][data-phase="off_air"]').waitFor({ timeout: 15_000 })],
  ];
  for (const width of [390, 320]) {
    for (const [name, opts, ready] of cases) {
      const ctx = await browser.newContext({ viewport: { width, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true, permissions: ['microphone'], colorScheme: 'dark' });
      const page = await ctx.newPage();
      await joinStation(page, { ...opts, room: `ws://127.0.0.1:${PORT + 1}` });
      await page.goto(`${STAND}/join/#${opts.studio === 'rehearsal' ? 'rehearsal-pass' : 'live-pass'}`);
      const joinButton = page.getByRole('button', { name: /^Join the (rehearsal )?studio$/ });
      if (/card|closed/.test(name) === false) {
        await joinButton.waitFor();
        await joinButton.tap();
      }
      await ready(page);
      const where = `${width} px, the join page: ${name}`;
      fits(where, await measure(page), width);
      const text = (await page.locator('main').innerText()).replace(/\s+/g, ' ');
      if (name === 'moving') check(`${where}: says Moving you to the live studio…, not on air`, /Moving you to the live studio…/.test(text) && !/You.re on air/.test(text), text);
      if (name === 'moved-standing-by') {
        check(`${where}: standing by, told where from, and the fragment holds the live pass`, /Standing by/.test(text) && /moved here from the rehearsal for Steeped/.test(text) && (await page.evaluate(() => location.hash)) === '#live-pass', `${text} · ${await page.evaluate(() => location.hash)}`);
      }
      if (name.startsWith('rehearsal-') && name !== 'rehearsal-closed' && name !== 'rehearsal-card') check(`${where}: in the rehearsal studio, only the people there hear, and nothing says on air`, /You are in the rehearsal studio/.test(text) && /Only the people here hear you/.test(text) && !/on air|open/i.test(text.replace(/nobody listening/i, '')), text);
      await page.screenshot({ path: join(OUT, `${width}-join-${name}.png`), fullPage: true });
      await ctx.close();
    }
  }
}

try {
  for (const width of [390, 320]) {
    // Connecting: the call panel is up, with the microphone's level, what you hear and the way out.
    {
      const { ctx, page } = await call(width, { room: ROOM_SILENT });
      await page.locator('.banner[data-link="connecting"]').waitFor({ timeout: 10_000 });
      const where = `${width} px, the call panel while connecting`;
      fits(where, await measure(page), width);
      const banner = await page.locator('.banner').evaluate((el) => ({ w: el.getBoundingClientRect().width, h: el.getBoundingClientRect().height, ratio: getComputedStyle(el).aspectRatio }));
      check(`${where}: the call’s banner is as tall as its words, not a 3:1 picture`, banner.ratio === 'auto' && banner.w <= width, JSON.stringify(banner));
      // The step that failed: the relay for networks that block calls, opened and tapped.
      await page.locator('details.trouble > summary', { hasText: 'Trouble hearing' }).tap();
      const relay = page.getByRole('button', { name: 'Reconnect through the relay' });
      const box = await relay.boundingBox();
      check(`${where}: Reconnect through the relay is inside the window`, !!box && box.x >= 0 && box.x + box.width <= width + 1, JSON.stringify(box));
      fits(`${where}, the help open`, await measure(page), width);
      await page.screenshot({ path: join(OUT, `${width}-call-connecting.png`), fullPage: true });
      // The tap itself is what timed out in test-m3. What the page does next needs a room that
      // answers (this one never does, so the first join is still under way), and is test-m3's.
      const tapped = await relay.tap({ timeout: 5000 }).then(
        () => true,
        (e) => String(e).split('\n')[0],
      );
      check(`${where}: Reconnect through the relay takes a tap`, tapped === true, String(tapped));
      await ctx.close();
    }
    // The line lost: the longest words the banner has, and the way back.
    {
      const { ctx, page } = await call(width, { room: ROOM_REFUSES });
      await page.locator('.banner[data-link="lost"]').waitFor({ timeout: 30_000 });
      const where = `${width} px, the line lost`;
      fits(where, await measure(page), width);
      const back = await page.getByRole('button', { name: 'Back to the show' }).boundingBox();
      check(`${where}: Back to the show is inside the window`, !!back && back.x >= 0 && back.x + back.width <= width + 1, JSON.stringify(back));
      await page.screenshot({ path: join(OUT, `${width}-call-lost.png`), fullPage: true });
      await ctx.close();
    }
  }
  // The other side of the same rule: the station's banner picture keeps its 3:1 shape, and the
  // call panel under it is still as wide as the window.
  {
    const { ctx, page } = await call(390, { room: ROOM_SILENT, look: true });
    await page.locator('.banner[data-link="connecting"]').waitFor({ timeout: 10_000 });
    const pic = await page
      .locator('img.banner')
      .evaluate((el) => ({ w: el.getBoundingClientRect().width, h: el.getBoundingClientRect().height }))
      .catch(() => null);
    check('the station’s banner picture is still drawn three to one, the width of the page', !!pic && pic.w > 300 && Math.abs(pic.w / pic.h - 3) < 0.05, JSON.stringify(pic));
    fits('390 px, the call panel under the station’s banner', await measure(page), 390);
    await page.screenshot({ path: join(OUT, '390-call-with-banner.png'), fullPage: true });
    await ctx.close();
  }
  await joinStates();
} finally {
  await browser.close();
  for (const s of held) s.destroy();
  await new Promise((r) => silent.close(r));
  await new Promise((r) => web.close(r));
  await new Promise((r) => standin.close(r));
}

const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
