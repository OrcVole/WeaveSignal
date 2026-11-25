// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives two things the operator asked for after clicking through 1.1.0 (2026-11-17), in a real
// browser against the hosts' stand-in control plane (station.mjs), checks them and captures
// them: the studio app's version in the frame (and the station's beside it when they differ),
// and what automation is, said where it is named (the tooltip of the button under the lamp, the
// AUTO lamp's tooltip, the air dialog). Like shoot.mjs it needs no stack and no port but its
// own loopback one.
//
// Unlike shoot.mjs it holds the studio's event stream open from its own server, so the frame
// says Connected, as it does against a real station: the frame's room is what is tested here,
// and "Control plane not connected" takes 200 px of it.
//
//   just studio-check          # or: cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/hosts/shoot-frame.mjs
//   SIZES=1440x900 THEMES=dark … node tests/hosts/shoot-frame.mjs      # fewer of them
//   SIZES=2560x1440 TEXT=1.2 … node tests/hosts/shoot-frame.mjs        # a larger window and text
//
// By default: 1440×900 (the window the app opens at) and 1280×600 (the smallest the packaged
// window goes, tauri.conf.json), each in Dark and Light. Screenshots go to OUT
// (target/shots-frame unless told). The clock is shoot.mjs's.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HostStation } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-frame');
const PORT = Number(process.env.PORT ?? 28110);
const SIZES = (process.env.SIZES ?? '1440x900,1280x600').split(',').map((s) => s.split('x').map(Number));
const THEMES = (process.env.THEMES ?? 'dark,light').split(',');
const TEXT = Number(process.env.TEXT ?? 1);
/** The page's zoom in a window this size: the text size times the window's factor (zoom.svelte.ts). */
const zoomOf = ([w, h]) => TEXT * (Math.round(Math.min(2.5, Math.max(1, Math.min(w / 1920, h / 1080))) * 20) / 20);
const APP = JSON.parse(readFileSync(join(here, '..', '..', 'apps', 'studio', 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/studio first`);

/** The station the open window is on, and whether its event stream answers. */
let station = null;
let streamUp = true;

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  // The studio's event stream, held open: the view as the stand-in has it, each second.
  if (path === '/api/v1/studio/events') {
    if (!streamUp || !station) return res.writeHead(503, { 'content-type': 'application/json' }).end('{"error":"the control plane is not answering"}');
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
    const send = () => res.write(`data: ${JSON.stringify(station.studioView())}\n\n`);
    send();
    const every = setInterval(send, 1000);
    req.on('close', () => clearInterval(every));
    return;
  }
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
const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();
/** The version as drawn, without the standing's word kept for a screen reader (1.3.2: ", up to date"). */
const shown = async (loc) => (await words(loc)).replace(/\s*,\s*(up to date|behind)$/, '');

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

/** A window on a station. `setup`: the station's setup is not finished, so an admin's frame
 *  carries its button too (the fullest frame a browser draws). */
async function open(st, [width, height], theme, { setup = false, up = true } = {}) {
  station = st;
  streamUp = up;
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
  await ctx.addInitScript(([t, z]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', String(z));
    } catch {}
  }, [theme, TEXT]);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(st.now * 1000));
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.route(/\/(api\/|health)/, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (url.pathname === '/api/v1/studio/events') return route.continue();
    if (url.pathname === '/api/v1/setup' && setup) return route.fulfill({ status: 200, contentType: 'application/json', body: '{"status":"pending","missing":["the station’s name","the public address"]}' });
    if (url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    const [status, body] = st.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  // With the stream up, the first view has arrived once the frame says the link is good.
  if (up) await page.locator('.frame .link[data-link="live"]').waitFor({ timeout: 15000 });
  if (up) await page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 15000 });
  await sleep(1300);
  return { ctx, page };
}

/** The frame's controls, each with where it is: everything but the spare room and its words. */
const frameBoxes = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('.frame > *')]
      .filter((el) => !el.matches('.spacer, [role=tooltip], dialog'))
      .map((el) => {
        const r = el.getBoundingClientRect();
        return [el.className.toString().split(' ')[0] || el.tagName, Math.round(r.x), Math.round(r.y), Math.round(r.width)];
      }),
  );
const frameHeight = (page) => page.evaluate(() => Math.round(document.querySelector('.frame').getBoundingClientRect().height));
const shownTip = (page, id) => page.evaluate((i) => document.getElementById(i)?.classList.contains('show') ?? false, id);
/** Whether two boxes share any of the window. */
const overlap = (a, b) => !!a && !!b && a.x < b.x + b.width - 0.5 && b.x < a.x + a.width - 0.5 && a.y < b.y + b.height - 0.5 && b.y < a.y + a.height - 0.5;
/** The tooltip's own box: the drawn part, without the gap that lets the pointer cross to it. */
const tipBox = (page, id) => page.locator(`#${id} > span`).boundingBox();
const inside = (b, vp) => !!b && b.x >= 0 && b.y >= 0 && b.x + b.width <= vp.width + 0.5 && b.y + b.height <= vp.height + 0.5;

/** Point at `loc`. The page is scrolled to it first and left to settle, and the pointer is then
 *  moved by its coordinates: a scroll hides a tooltip, as it should, and the driver's own
 *  scrolling on a hover (it scrolls again in a window as short as 600 px) would hide the one
 *  being asked for. A person's pointer scrolls nothing. */
async function point(page, loc, rest = 300) {
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await loc.scrollIntoViewIfNeeded();
  await sleep(300);
  const b = await loc.boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 4 });
  await sleep(rest);
}

/** Press Tab until the focus is on `loc`, as a person at the keyboard would reach it. */
async function tabTo(page, loc, limit = 90) {
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await page.mouse.move(2, 400);
  for (let i = 0; i < limit; i++) {
    await page.keyboard.press('Tab');
    if (await loc.evaluate((el) => el === document.activeElement)) return true;
  }
  return false;
}

for (const size of SIZES) {
  const [W, H] = size;
  // The frame's widths below are in the page's own pixels, which the zoom scales.
  const Z = zoomOf(size);
  for (const theme of THEMES) {
    const tag = `${W}${TEXT === 1 ? '' : `-text${Math.round(TEXT * 100)}`}-${theme}`;
    const shot = async (page, name, clip) => {
      await sleep(250);
      await page.screenshot({ path: join(OUT, `${tag}-${name}.png`), ...(clip ? { clip } : {}) });
      log('shot', `${tag}-${name}`);
    };
    const top = async (page) => ({ x: 0, y: 0, width: W, height: Math.min(H, (await frameHeight(page)) + 96 * Z) });

    // ---- 1. The version in the frame: an admin's window, the link good ----
    {
      const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10, role: 'admin' });
      st.join('duncan', 'Duncan Boyd', 'presenter');
      const { ctx, page } = await open(st, size, theme);
      const ver = page.locator('[data-testid="app-version"]');
      check(`${tag}: the link to the station is good, so the frame is as a real station draws it`, (await page.locator('.frame .link').getAttribute('data-link')) === 'live');
      const said = await shown(ver);
      // Station and app on the same release: the version is up to date (1.3.2), so its glyph stands
      // before the v (the operator, 2026-11-25: "a tick then v1.3.1"). An admin's frame at 1,440 px
      // leaves about 65 px: the tick, the v and the number. With 10.25 rem or more, the name too.
      check(`${tag}: the frame says the app’s version, v${APP}, the one Tauri packages, and it is drawn`, (await ver.isVisible()) && (said === `WeaveSignal v${APP}` || said === `v${APP}`), said);
      const room = await page.evaluate(() => document.querySelector('.frame .spacer').getBoundingClientRect().width);
      check(`${tag}: with the name where the spare room holds it (${Math.round(room)} px here), the v and the number where it does not`, said === (room >= 164 * Z ? `WeaveSignal v${APP}` : `v${APP}`), said);
      const drawn = await ver.evaluate((el) => { const g = el.querySelector('svg.vg').getBoundingClientRect(); const v = el.querySelector('.v').getBoundingClientRect(); const n = el.querySelector('.name').getBoundingClientRect(); return { cut: el.scrollWidth > el.clientWidth + 1, gap: Math.round((n.width ? n : v).left - g.right), v: Math.round(v.width), after: v.left >= g.right && (!n.width || v.left >= n.right - 1) }; });
      // A version longer than five characters gives up the gap after the tick, never its v, until
      // the spare room holds both (4 rem for six characters, 4.5 rem for seven).
      const close = APP.length > 5 && room < (APP.length === 6 ? 64 : 72) * Z;
      check(`${tag}: the tick stands first, a little apart, the v before the number (after the name where there is room for it), and nothing is cut short`, !drawn.cut && drawn.v > 0 && drawn.after && (close ? drawn.gap >= 0 && drawn.gap <= 2 * Z : drawn.gap >= 3 * Z - 1 && drawn.gap <= 6 * Z), JSON.stringify(drawn));
      check(`${tag}: up to date, it says so in words for a screen reader, and carries the tick`, (await words(ver)).endsWith(', up to date') && (await ver.getAttribute('data-standing')) === 'current' && (await ver.locator('svg.vg').count()) === 1, await words(ver));
      const [vb, nav, acc] = [await ver.boundingBox(), await page.locator('.frame nav.tabs').boundingBox(), await page.locator('[data-panel="studio.account"]').first().boundingBox()];
      check(`${tag}: it sits between the tabs and who is signed in, touching neither`, !!vb && !!nav && !!acc && vb.x >= nav.x + nav.width + 8 && vb.x + vb.width <= acc.x - 8, JSON.stringify({ vb, nav, acc }));
      const look = await ver.evaluate((el) => { const c = getComputedStyle(el); const f = getComputedStyle(el.closest('.frame')); return { size: c.fontSize, weight: c.fontWeight, ink: c.color, frameInk: f.color, tag: el.tagName }; });
      check(`${tag}: words, not a button: 13 px, semibold, in the green of being up to date (the frame’s own ink only while the standing is not known)`, look.tag === 'SPAN' && look.ink !== look.frameInk && parseFloat(look.size) >= 12.9 && parseFloat(look.size) <= 13.1 && Number(look.weight) === 600, JSON.stringify(look));
      // The words take only spare room: with them gone, nothing else in the frame moves.
      const withIt = await frameBoxes(page);
      const hWith = await frameHeight(page);
      await page.addStyleTag({ content: '[data-testid="app-version"] { display: none !important; }' }).then((h) => h.evaluate((el) => el.setAttribute('data-probe', '1')));
      const without = await frameBoxes(page);
      const hWithout = await frameHeight(page);
      await page.evaluate(() => document.querySelector('style[data-probe]')?.remove());
      check(`${tag}: it moves nothing: every control in the frame is where it is without it, and the frame is as tall`, JSON.stringify(withIt) === JSON.stringify(without) && hWith === hWithout, `${hWith} px against ${hWithout} px`);
      // At the text size the app opens with; larger text takes a second row, as it did before.
      if (W >= 1440 && TEXT === 1) check(`${tag}: the frame is one row`, hWith <= 44 * Z, `${hWith} px`);
      await shot(page, '1-frame', await top(page));
      await shot(page, '1-desk');
      // Its tooltip: on hover, and from the keyboard.
      await point(page, ver);
      check(`${tag}: pointing at it says whose version it is, and the station’s`, (await shownTip(page, 'ver-tip')) && (await words(page.locator('#ver-tip'))) === `This studio app is WeaveSignal ${APP}. The station runs the same version. This app is up to date.`, await words(page.locator('#ver-tip')));
      check(`${tag}: the tooltip is inside the window and not over the words`, inside(await tipBox(page, 'ver-tip'), { width: W, height: H }) && !overlap(await tipBox(page, 'ver-tip'), await ver.boundingBox()));
      await shot(page, '2-version-tooltip', await top(page));
      await page.mouse.move(2, 400);
      await sleep(300);
      check(`${tag}: it goes when the pointer leaves`, !(await shownTip(page, 'ver-tip')));
      check(`${tag}: Tab reaches it, and the tooltip shows on the keyboard’s focus`, (await tabTo(page, ver)) && (await shownTip(page, 'ver-tip')));
      await page.keyboard.press('Escape');
      check(`${tag}: Esc hides it`, !(await shownTip(page, 'ver-tip')));
      await ctx.close();
    }

    // ---- 1b. A version written longer (1.3.10, 1.10.10): the tick, the v and the number, always ----
    // The operator, 2026-11-25, of the tight form ✓1.3.10: "we want to see the tick, the v for
    // version and then the version number". The build's own version is five characters, so a
    // longer one is written into the page here (its text and the length the styles go by), in
    // the frame as it is at this size and then in every room the spare room could have.
    {
      const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10, role: 'admin' });
      st.join('duncan', 'Duncan Boyd', 'presenter');
      const { ctx, page } = await open(st, size, theme);
      const ver = page.locator('[data-testid="app-version"]');
      const before = await frameBoxes(page);
      const hBefore = await frameHeight(page);
      /** Write `v` as the app's version, in `room` px of spare room (the frame's own when null), and read what is drawn. */
      const drawnAs = (v, room) =>
        ver.evaluate(
          (el, [v, room]) => {
            const text = [...el.childNodes].find((n) => n.nodeType === 3 && /\d/.test(n.textContent ?? ''));
            text.textContent = v;
            el.setAttribute('data-len', String(Math.min(7, Math.max(5, v.length))));
            document.querySelector('style[data-room]')?.remove();
            if (room !== null) {
              const probe = document.createElement('style');
              probe.setAttribute('data-room', '1');
              probe.textContent = `.frame .spacer { flex: none !important; width: ${room}px !important; }`;
              document.head.appendChild(probe);
            }
            const shown = getComputedStyle(el).display !== 'none';
            if (!shown) return { shown, room: el.parentElement.clientWidth };
            const g = el.querySelector('svg.vg').getBoundingClientRect();
            const vb = el.querySelector('.v').getBoundingClientRect();
            // What follows the tick: the name where the room holds it, else the v.
            const nm = el.querySelector('.name').getBoundingClientRect();
            const num = document.createRange();
            num.selectNode(text);
            const nb = num.getBoundingClientRect();
            const box = el.getBoundingClientRect();
            const host = el.parentElement.getBoundingClientRect();
            return {
              shown,
              room: el.parentElement.clientWidth,
              tick: g.width > 0,
              v: vb.width > 0,
              order: g.right <= vb.left + 0.5 && vb.right <= nb.left + 0.5,
              gap: Math.round(((nm.width ? nm : vb).left - g.right) * 10) / 10,
              whole: el.scrollWidth <= el.clientWidth + 1 && nb.right <= box.right + 0.5 && box.left >= host.left - 0.5 && box.right <= host.right + 0.5,
              said: el.innerText.replace(/\s+/g, ' ').trim(),
            };
          },
          [v, room],
        );
      for (const [v, bare, full] of [['1.3.10', 56, 64], ['1.10.10', 64, 72]]) {
        // In the frame as it is: an admin's 1,440 px leaves about 65 px, and 1,280 px two rows.
        const here = await drawnAs(v, null);
        check(`${tag}: ${v} in the frame as it is (${here.room} px of spare room): the tick, the v and the number, whole`, here.shown && here.tick && here.v && here.order && here.whole && here.said.startsWith(here.room >= 164 ? `WeaveSignal v${v}` : `v${v}`), JSON.stringify(here));
        check(`${tag}: ${v} moves nothing: every control in the frame is where it was, and the frame is as tall`, JSON.stringify(await frameBoxes(page)) === JSON.stringify(before) && (await frameHeight(page)) === hBefore);
        if (v === '1.3.10') check(`${tag}: ${v} has the room for its gap here, as ${APP} has`, here.gap >= 3 * Z - 1 && here.gap <= 6 * Z, JSON.stringify(here));
        // Every room from none to plenty: never the tick and the number without the v.
        const seen = [];
        for (let room = 40; room <= 110; room += 1) seen.push(await drawnAs(v, room));
        const drawn = seen.filter((d) => d.shown);
        check(`${tag}: ${v} is never drawn without its v, its tick first, in any room from 40 to 110 px`, drawn.length > 0 && drawn.every((d) => d.tick && d.v && d.order && d.said.startsWith(`v${v}`)), JSON.stringify(drawn.find((d) => !(d.tick && d.v && d.order && d.said.startsWith(`v${v}`))) ?? {}));
        check(`${tag}: ${v} is whole wherever it is drawn, never cut short`, drawn.every((d) => d.whole), JSON.stringify(drawn.find((d) => !d.whole) ?? {}));
        check(`${tag}: ${v} is drawn from ${bare} px of room, and not in less`, seen.every((d) => d.shown === d.room >= bare), JSON.stringify(seen.find((d) => d.shown !== d.room >= bare) ?? {}));
        check(`${tag}: ${v} gives up the gap after the tick under ${full} px, and has it from there`, drawn.every((d) => (d.room >= full ? d.gap >= 3 * Z - 1 && d.gap <= 6 * Z : d.gap >= 0 && d.gap <= 2 * Z)), JSON.stringify(drawn.find((d) => !(d.room >= full ? d.gap >= 3 * Z - 1 && d.gap <= 6 * Z : d.gap >= 0 && d.gap <= 2 * Z)) ?? {}));
        if (theme === THEMES[0]) {
          await drawnAs(v, bare);
          const b = await page.locator('.frame .spacer').boundingBox();
          await shot(page, `1b-${v}-least-room`, { x: Math.max(0, b.x - 120), y: 0, width: Math.min(W, b.width + 240), height: Math.min(H, (await frameHeight(page)) + 8) });
          await drawnAs(v, null);
          await shot(page, `1b-${v}-frame`, await top(page));
        }
      }
      await ctx.close();
    }

    // ---- 2. The fullest frame a browser draws: the setup button too, and the link down ----
    {
      const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10, role: 'admin' });
      const { ctx, page } = await open(st, size, theme, { setup: true, up: false });
      await page.getByRole('button', { name: 'Finish the station’s setup' }).waitFor();
      const withIt = await frameBoxes(page);
      const hWith = await frameHeight(page);
      await page.addStyleTag({ content: '[data-testid="app-version"] { display: none !important; }' }).then((h) => h.evaluate((el) => el.setAttribute('data-probe', '1')));
      const without = await frameBoxes(page);
      const hWithout = await frameHeight(page);
      await page.evaluate(() => document.querySelector('style[data-probe]')?.remove());
      check(`${tag}: with the setup button and the link down, the version still moves nothing and adds no row`, JSON.stringify(withIt) === JSON.stringify(without) && hWith === hWithout, `${hWith} px against ${hWithout} px`);
      const ver = page.locator('[data-testid="app-version"]');
      const said = (await ver.isVisible()) ? await shown(ver) : '';
      check(`${tag}: there it says what it has room for, whole or not at all`, ['', `v${APP}`, `WeaveSignal ${APP}`, `WeaveSignal v${APP}`].includes(said), said);
      const cut = (await ver.isVisible()) ? await ver.evaluate((el) => el.scrollWidth > el.clientWidth + 1) : false;
      check(`${tag}: and never cut short`, !cut);
      await shot(page, '3-frame-fullest', { x: 0, y: 0, width: W, height: Math.min(H, hWith + 24) });
      await ctx.close();
    }

    // ---- 3. A station on another version: said beside the app's, in words ----
    {
      const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10, role: 'admin', routes: false });
      st.join('duncan', 'Duncan Boyd', 'presenter');
      const { ctx, page } = await open(st, size, theme);
      const ver = page.locator('[data-testid="app-version"]');
      const said = await words(ver);
      const room = await page.evaluate(() => document.querySelector('.frame .spacer').getBoundingClientRect().width);
      const want = room >= 212 * Z ? `WeaveSignal ${APP} · station ${st.version}` : room >= 140 * Z ? `v${APP} · station ${st.version}` : room >= 96 * Z ? `${APP} ≠ ${st.version}` : `v${APP}`;
      check(`${tag}: a station on ${st.version} is said beside the app’s version, as far as the spare room goes (${Math.round(room)} px here)`, said === want, `${said} against ${want}`);
      check(`${tag}: and never cut short`, !(await ver.evaluate((el) => el.scrollWidth > el.clientWidth + 1)));
      check(`${tag}: in words alone: no state glyph, no status colour`, (await ver.locator('svg').count()) === 0 && (await ver.evaluate((el) => !/state|hollow|fill-/.test(el.className))));
      const hWith = await frameHeight(page);
      await page.addStyleTag({ content: '[data-testid="app-version"] { display: none !important; }' }).then((h) => h.evaluate((el) => el.setAttribute('data-probe', '1')));
      const hWithout = await frameHeight(page);
      await page.evaluate(() => document.querySelector('style[data-probe]')?.remove());
      check(`${tag}: and it adds no row`, hWith === hWithout, `${hWith} px against ${hWithout} px`);
      await point(page, ver);
      check(`${tag}: the tooltip says which way the difference runs, and what follows`, (await words(page.locator('#ver-tip'))) === `This studio app is WeaveSignal ${APP}. The station runs ${st.version}, older than this app: what it does not have yet is hidden or switched off here.`, await words(page.locator('#ver-tip')));
      await shot(page, '4-station-older-tooltip', await top(page));
      await ctx.close();
    }

    // ---- 4. What automation is: the button under the lamp, the dialog, the AUTO lamp ----
    {
      const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10 });
      st.join('duncan', 'Duncan Boyd', 'presenter');
      const { ctx, page } = await open(st, size, theme);
      const head = page.locator('.stage-head');
      const air = head.locator('[data-panel="desk.air"]');
      const lamp = page.locator('[data-panel="desk.lamp"]');
      const PLAYS = 'Automation is the station playing by itself. It plays the log: the music planned for this hour from the station’s library, with carts such as idents, stingers or adverts between the tracks. With nothing in the log, listeners hear the station’s fallback files, or the loop WeaveSignal ships.';
      check(`${tag}: the button still says Hand to automation, and has no tooltip of the browser’s`, (await words(air)) === 'Hand to automation' && (await air.getAttribute('title')) === null && ((await air.getAttribute('aria-describedby')) ?? '').split(' ').includes('air-tip'), `${await words(air)} / ${await air.getAttribute('title')} / ${await air.getAttribute('aria-describedby')}`);
      check(`${tag}: nothing shows until it is pointed at`, !(await shownTip(page, 'air-tip')));
      await point(page, air, 700);
      check(`${tag}: pointing at it says what the press does, then what automation is, with an example`, (await shownTip(page, 'air-tip')) && (await words(page.locator('#air-tip'))) === `Automation takes the air now. Whoever is open stays in the room. ${PLAYS}`, await words(page.locator('#air-tip')));
      check(`${tag}: the tooltip is inside the window, and over neither the button nor the lamp`, inside(await tipBox(page, 'air-tip'), { width: W, height: H }) && !overlap(await tipBox(page, 'air-tip'), await air.boundingBox()) && !overlap(await tipBox(page, 'air-tip'), await lamp.boundingBox()), JSON.stringify(await tipBox(page, 'air-tip')));
      // The button is its panel's title, so its name plate (ADR-0070) shows there too.
      const plate = page.locator('#pn-desk-air.show');
      check(`${tag}: the panel’s name plate shows with it, and neither lies over the other`, (await plate.count()) === 1 && !overlap(await tipBox(page, 'air-tip'), await plate.boundingBox()), JSON.stringify([await tipBox(page, 'air-tip'), await plate.boundingBox().catch(() => null)]));
      await shot(page, '5-hand-to-automation-tooltip');
      await page.mouse.move(2, H - 40);
      await sleep(300);
      check(`${tag}: it goes when the pointer leaves`, !(await shownTip(page, 'air-tip')));
      const reached = await tabTo(page, air);
      // Where the button is below the fold (the one-column desk in a short window), the focus scrolls it
      // into view, and a scroll hides a tooltip (Tip.svelte, on purpose): so look once, and if it was
      // hidden by that, leave and come back by the keyboard, now that the button is in view.
      await sleep(400);
      let early = await shownTip(page, 'air-tip');
      if (reached && !early) {
        await page.keyboard.press('Shift+Tab');
        await page.keyboard.press('Tab');
        await sleep(400);
        early = await shownTip(page, 'air-tip');
      }
      check(`${tag}: Tab reaches the button, and the tooltip shows on the keyboard’s focus`, reached && early, JSON.stringify({ reached, early }));
      await page.keyboard.press('Escape');
      check(`${tag}: Esc hides it, and the button keeps the focus`, !(await shownTip(page, 'air-tip')) && (await air.evaluate((el) => el === document.activeElement)));
      // A press does not leave the tooltip up: with Duncan open, the dialog asks.
      await point(page, air, 100);
      await air.click();
      const dialog = page.locator('dialog[data-panel="desk.end-show"][open]');
      await dialog.waitFor();
      check(`${tag}: a press opens the dialog and the tooltip is gone`, !(await shownTip(page, 'air-tip')));
      check(`${tag}: Hand to automation’s dialog says what automation is, in the same words, under what the press does`, (await words(dialog.locator('[data-testid="air-about"]'))) === PLAYS && (await dialog.locator('ul ~ [data-testid="air-about"]').count()) === 1);
      check(`${tag}: the dialog is inside the window with both its answers`, inside(await dialog.boundingBox(), { width: W, height: H }) && inside(await dialog.getByRole('button', { name: /^Hand to automation/ }).boundingBox(), { width: W, height: H }));
      await shot(page, '6-hand-to-automation-dialog');
      await page.keyboard.press('Escape');
      await sleep(300);
      await page.mouse.move(2, H - 40);
      await page.keyboard.press('Control+Shift+E');
      await dialog.waitFor();
      check(`${tag}: End show’s dialog says it too, where the air goes to automation`, (await words(dialog.locator('h2'))) === 'End the show?' && (await words(dialog.locator('[data-testid="air-about"]'))) === PLAYS);
      check(`${tag}: End show’s dialog is inside the window with both its answers`, inside(await dialog.boundingBox(), { width: W, height: H }) && inside(await dialog.getByRole('button', { name: /^End show/ }).boundingBox(), { width: W, height: H }));
      await shot(page, '7-end-show-dialog');
      await page.keyboard.press('Escape');
      await sleep(300);

      // Automation holds the air: the lamp says AUTO, and its tooltip adds the same sentence.
      st.mode = 'auto';
      await sleep(1500);
      check(`${tag}: in auto mode the lamp says AUTO and the button Take the air back`, /auto/i.test(await words(lamp)) && (await words(air)) === 'Take the air back');
      await point(page, lamp);
      const lampTip = page.locator('.air [role="tooltip"]').first();
      check(`${tag}: the AUTO lamp’s tooltip keeps the word’s own sentence and adds what automation is`, (await words(lampTip)) === `Automation is playing the log and the desk never reaches listeners; pressing On opens you to the room only, and you leave auto mode before you go on air. ${PLAYS}`, await words(lampTip));
      check(`${tag}: the lamp’s tooltip is inside the window, and over neither the lamp nor the button under it`, inside(await lampTip.locator('> span').boundingBox(), { width: W, height: H }) && !overlap(await lampTip.locator('> span').boundingBox(), await air.boundingBox()) && !overlap(await lampTip.locator('> span').boundingBox(), await lamp.boundingBox()), JSON.stringify(await lampTip.locator('> span').boundingBox()));
      await shot(page, '8-auto-lamp-tooltip');
      await page.mouse.move(2, H - 40);
      await sleep(300);
      await point(page, air, 700);
      check(`${tag}: Take the air back says what the press does, then what automation is`, (await words(page.locator('#air-tip'))) === `The air comes back to the desk. Whoever is open reaches listeners. ${PLAYS}`, await words(page.locator('#air-tip')));
      await shot(page, '9-take-the-air-back-tooltip');
      await ctx.close();
    }
  }
}

await browser.close();
server.close();
server.closeAllConnections?.();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
