// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The Screener tab's layout in a real browser against the stand-in station (tests/hosts): at each
// window size and text size, every panel is whole inside the window, none is narrower than it can
// be read at, no panel covers another, and nothing in a panel's head runs out of it. Written for
// the 1.2.1 fault: the Room panel, the last column, was cut off at 120 % text in a window about
// 2,000 px wide (the operator's click-through of 2026-11-20), because the first columns were
// fixed widths and the last took whatever was left, however little.
//
// It runs in two engines, because the studio's text size is `zoom` on <html> and its layouts are
// container queries, and the two engines have differed on both: WebKitGTK 4.1, which the app's
// own window uses on Linux (through tests/screens/webkit-measure.py; python3 with GTK 3 and
// WebKit2 4.1's introspection), and Chromium (Playwright, which is not a dependency of the
// repository: set PLAYWRIGHT_CORE to a playwright-core directory). ENGINES names them; an engine
// that is not on this machine is said and left out, and the run fails if none ran.
//
// No stack and no port but its own loopback one: the stand-in station answers over HTTP here.
//
//   cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/screens/screener-layout.mjs
//   ENGINES=webkitgtk SIZES=2000x1100 TEXTS=1.2 node tests/screens/screener-layout.mjs   # some only
//
// Screenshots go to OUT (target/shots-screener-layout unless told), named for the engine, the
// window and the text size. `just test-screener-layout`. Not in `ci`.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { Station12 } from '../hosts/station-12.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const DIST = process.env.DIST ?? join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-screener-layout');
const PORT = Number(process.env.PORT ?? 28121);
const ENGINES = (process.env.ENGINES ?? 'webkitgtk,chromium').split(',').filter(Boolean);
// The two sizes the screens are drawn for, the window the fault was seen in, a 2,560 px window
// that is not tall (so the text is not scaled up and the wide layout is chosen), and the widths
// either side of the wide layout's threshold.
const SIZES = (process.env.SIZES ?? '1440x900,1920x1080,2000x1100,2200x1200,2560x1080,2560x1440,3840x2160').split(',').map((s) => s.split('x').map(Number));
// 100 % and 120 % are the sizes the screens must hold at; 150 % reaches the stacked layout in a
// 1,440 px window, so that one is kept working too.
const TEXTS = (process.env.TEXTS ?? '1,1.2,1.5').split(',').map(Number);
/** The narrowest a panel may be and still be read, in rems (16 px at 100 %): the Room's lines
 *  are a time, a name and the words. */
const MIN_REM = { 'screener.queue': 20, 'screener.screening': 20, 'screener.needs': 22, 'screener.room': 26 };
/** The shortest: a head and a few lines. The studio lane is a head and a line when it is empty. */
const MIN_HIGH_REM = { 'screener.queue': 8, 'screener.screening': 12, 'screener.needs': 8, 'screener.room': 10 };
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/studio first`);

/** Kettle FM with a full queue: a caller from the player page with a long topic, a phone caller,
 *  a withheld number and one more, so every row kind is drawn. */
function station() {
  const st = new Station12({ step: 'full', role: 'screener' });
  st.join('duncan', 'Duncan Boyd', 'presenter');
  st.phone.on = true;
  st.ring('caller-jessie', { name: 'Jessie Paterson', topic: 'The bins on Bothwell Street, and what the council said about them on Tuesday', waited_s: 240 });
  st.ring('phone-x7k2', { phone: { tail: '0123' }, waited_s: 140 });
  st.ring('phone-q9m4', { phone: { withheld: true }, consent: 'not_asked', waited_s: 80 });
  st.ring('caller-dougal', { name: 'Dougal Thomson', topic: 'A request for the next hour', waited_s: 40 });
  return st;
}
const st = station();

// The studio's build and the stand-in station on one loopback port, so any engine can load it.
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://x');
  const path = decodeURIComponent(url.pathname);
  if (/^\/(api\/|health)/.test(path)) {
    if (path === '/api/v1/studio/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
      return void res.write(`data: ${JSON.stringify(st.studioView())}\n\n`);
    }
    if (path.startsWith('/api/v1/carts') || path.startsWith('/api/v1/chat')) return void res.writeHead(503, { 'content-type': 'application/json' }).end('{"error":"not in the stand-in"}');
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    return void req.on('end', () => {
      let body;
      try {
        body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : undefined;
      } catch {
        body = undefined;
      }
      const [status, out] = st.handle(req.method, path + url.search, body);
      if (status === 204) return void res.writeHead(204).end();
      if (Buffer.isBuffer(out)) return void res.writeHead(status, { 'content-type': 'audio/ogg' }).end(out);
      res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(out));
    });
  }
  const file = join(DIST, path === '/' ? 'index.html' : path);
  if (!file.startsWith(DIST) || !existsSync(file)) return void res.writeHead(404).end();
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));
const URL_ = `http://127.0.0.1:${PORT}/`;

const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  console.log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Before the page's own scripts: the theme, the text size, and the station's clock (Friday
 *  20 November 2026, 13:52 in Europe/London, as tests/hosts fix it). */
const init = (text) => `(() => {
  try { localStorage.setItem('weavesignal.theme', 'dark'); localStorage.setItem('weavesignal.zoom', '${text}'); } catch {}
  const T = ${st.now * 1000}, D = Date;
  class Fixed extends D { constructor(...a) { if (a.length) super(...a); else super(T); } static now() { return T; } }
  window.Date = Fixed;
})();`;

/** In the page: first go to the Screener tab, then measure. Every measure is in the page's own
 *  layout pixels, whichever way the engine counts under zoom on <html> (Chromium's rects are in
 *  the window's pixels, WebKitGTK's in the page's): a 100 px box says which. */
const MEASURE = `(() => {
  if (window.__step === 'open') {
    const tab = [...document.querySelectorAll('button')].find((b) => /^\\s*Screener/.test(b.textContent || '') && b.closest('[data-panel="studio.frame"]'));
    if (tab) tab.click();
    return 'opened';
  }
  const z = Number(getComputedStyle(document.documentElement).zoom) || 1;
  const probe = document.createElement('div');
  probe.style.cssText = 'position:fixed;left:0;top:0;width:100px;height:1px';
  document.body.append(probe);
  const scale = probe.getBoundingClientRect().width / 100;
  probe.remove();
  const win = { w: (window.innerWidth / z) * scale, h: (window.innerHeight / z) * scale };
  const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) * scale;
  const box = (el) => { const r = el.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom, w: r.width, h: r.height }; };
  const app = document.querySelector('.screener .app');
  if (!app) return JSON.stringify({ error: 'the Screener tab did not open' });
  const panels = [...app.querySelectorAll('.panel')].map((el) => {
    const head = el.querySelector(':scope > .head');
    const h = head ? head.getBoundingClientRect() : null;
    return {
      name: el.getAttribute('data-panel') || el.className,
      ...box(el),
      over: !h ? [] : [...head.children]
        .filter((x) => !x.classList.contains('spacer'))
        .filter((x) => { const r = x.getBoundingClientRect(); return r.width && (r.right > h.right + 1 || r.left < h.left - 1 || r.bottom > h.bottom + 1); })
        .map((x) => (x.textContent || '').trim().replace(/\\s+/g, ' ').slice(0, 40)),
    };
  });
  for (const [i, el] of [...app.querySelectorAll('.panel')].entries()) {
    const p = el.getBoundingClientRect();
    panels[i].out = [...el.querySelectorAll('*')]
      .filter((x) => { const r = x.getBoundingClientRect(); return r.width > 0 && r.height > 0 && (r.right > p.right + 1 || r.left < p.left - 1); })
      .slice(0, 4)
      .map((x) => x.tagName.toLowerCase() + '.' + String(x.getAttribute('class') || '').split(' ')[0] + ' “' + (x.textContent || '').trim().replace(/\\s+/g, ' ').slice(0, 24) + '”');
  }
  const scr = document.querySelector('.screener');
  const bar = document.querySelector('.screener .statusbar');
  const container = document.querySelector('[data-panel="studio.frame"]').parentElement.getBoundingClientRect().width / scale;
  return JSON.stringify({ z, scale, win, rem, container, cols: getComputedStyle(app).gridTemplateColumns, panels, app: box(app), bar: box(bar), talk: box(bar.querySelector('button')), scroll: [document.scrollingElement.scrollWidth, document.scrollingElement.clientWidth], screener: { sw: scr.scrollWidth, cw: scr.clientWidth, sh: scr.scrollHeight, ch: scr.clientHeight, overflowY: getComputedStyle(scr).overflowY } });
})()`;

/** Under this width (the three columns' floors and the gaps) the columns stack and the tab
 *  scrolls: the panels need not fit the window then, but each keeps a height and none is cut at
 *  the side. The same figure as screener.css's narrow layout. */
const STACK_UNDER = 1140;

function judge(tag, m) {
  if (m.error) return check(`${tag}: the Screener tab opens`, false, m.error);
  const names = m.panels.map((p) => p.name);
  const stacked = m.container <= STACK_UNDER;
  const at = `(zoom ${m.z}, the studio ${Math.round(m.container)} px wide, columns ${m.cols})`;
  check(`${tag}: the queue, the caller's card, the chat's moderation and the Room are all drawn`, ['screener.queue', 'screener.screening', 'screener.needs', 'screener.room'].every((n) => names.includes(n)), names.join(', '));
  if (stacked) {
    const side = m.panels.filter((p) => p.l < -0.5 || p.r > m.win.w + 0.5);
    check(`${tag}: stacked, no panel is cut at the side`, side.length === 0, `${side.map((p) => `${p.name} ${Math.round(p.l)}–${Math.round(p.r)} of ${Math.round(m.win.w)}`).join('; ')} ${at}`);
    check(`${tag}: stacked, the tab scrolls to what is below`, m.screener.overflowY === 'auto' && m.screener.sh > m.screener.ch, JSON.stringify(m.screener));
  } else {
    const out = m.panels.filter((p) => p.l < -0.5 || p.t < -0.5 || p.r > m.win.w + 0.5 || p.b > m.win.h + 0.5);
    check(`${tag}: every panel is whole inside the window`, out.length === 0, `${out.map((p) => `${p.name} ${Math.round(p.l)}–${Math.round(p.r)} of ${Math.round(m.win.w)} wide, ${Math.round(p.t)}–${Math.round(p.b)} of ${Math.round(m.win.h)} high`).join('; ')} ${at}`);
  }
  const thin = m.panels.filter((p) => MIN_REM[p.name] && p.w < MIN_REM[p.name] * m.rem - 0.5);
  check(`${tag}: no panel is narrower than it can be read at`, thin.length === 0, `${thin.map((p) => `${p.name} is ${(p.w / m.rem).toFixed(1)} rem, under ${MIN_REM[p.name]}`).join('; ')} ${at}`);
  const short = m.panels.filter((p) => MIN_HIGH_REM[p.name] && p.h < MIN_HIGH_REM[p.name] * m.rem - 0.5);
  check(`${tag}: no panel is squeezed to a strip`, short.length === 0, `${short.map((p) => `${p.name} is ${(p.h / m.rem).toFixed(1)} rem high, under ${MIN_HIGH_REM[p.name]}`).join('; ')} ${at}`);
  const lapped = [];
  for (let i = 0; i < m.panels.length; i++)
    for (let j = i + 1; j < m.panels.length; j++) {
      const [a, b] = [m.panels[i], m.panels[j]];
      if (Math.min(a.r, b.r) - Math.max(a.l, b.l) > 1 && Math.min(a.b, b.b) - Math.max(a.t, b.t) > 1) lapped.push(`${a.name} and ${b.name}`);
    }
  check(`${tag}: no panel covers another`, lapped.length === 0, lapped.join('; '));
  const over = m.panels.filter((p) => p.over.length);
  check(`${tag}: nothing in a panel's head runs out of it`, over.length === 0, over.map((p) => `${p.name}: ${p.over.join(' | ')}`).join('; '));
  const outside = m.panels.filter((p) => p.out.length);
  check(`${tag}: nothing in a panel runs out of its sides`, outside.length === 0, outside.map((p) => `${p.name}: ${p.out.join(' | ')}`).join('; '));
  check(`${tag}: the status bar's Talk button is whole`, m.talk.r <= m.bar.r + 0.5 && m.talk.l >= m.bar.l - 0.5 && m.talk.r <= m.win.w + 0.5, `${Math.round(m.talk.l)}–${Math.round(m.talk.r)} in a bar to ${Math.round(m.bar.r)}`);
  check(`${tag}: the page is no wider than the window`, m.scroll[0] <= m.scroll[1] + 1 && m.screener.sw <= m.screener.cw + 1, `${m.scroll.join(' > ')}; the tab ${m.screener.sw} > ${m.screener.cw}`);
}

const each = [];
for (const [width, height] of SIZES) for (const text of TEXTS) each.push({ width, height, text, tag: `${width}x${height}-text${Math.round(text * 100)}` });
let ran = 0;

if (ENGINES.includes('webkitgtk')) {
  const run = promisify(execFile);
  // Drawn in software: an offscreen window has no GL context on some desktops, and a layout
  // check should not take the graphics card from whatever else is using it.
  const WEBKIT_ENV = { ...process.env, WEBKIT_DISABLE_COMPOSITING_MODE: '1', WEBKIT_DISABLE_DMABUF_RENDERER: '1', LIBGL_ALWAYS_SOFTWARE: '1' };
  const tool = join(here, 'webkit-measure.py');
  const dir = mkdtempSync(join(tmpdir(), 'ws-screener-layout-'));
  const has = await run('python3', ['-c', "import gi; gi.require_version('Gtk', '3.0'); gi.require_version('WebKit2', '4.1')"]).then(() => true, () => false);
  if (!has) console.log('SKIP webkitgtk: python3 with GTK 3 and WebKit2 4.1 is not on this machine');
  else {
    ran++;
    writeFileSync(join(dir, 'measure.js'), MEASURE);
    for (const c of each) {
      const tag = `webkitgtk-${c.tag}`;
      writeFileSync(join(dir, 'init.js'), init(c.text));
      try {
        const { stdout } = await run('python3', [tool, URL_, join(OUT, `${tag}.png`), String(c.width), String(c.height), join(dir, 'init.js'), join(dir, 'measure.js'), '1800'], { timeout: 90_000, env: WEBKIT_ENV });
        const line = stdout.split('\n').find((l) => l.startsWith('RESULT '));
        judge(tag, JSON.parse(line.slice(7)));
      } catch (e) {
        check(`${tag}: the page was measured`, false, String(e.stderr || e.message).slice(0, 300));
      }
    }
  }
  rmSync(dir, { recursive: true, force: true });
}

if (ENGINES.includes('chromium')) {
  let chromium = null;
  try {
    ({ chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT_CORE ?? 'playwright-core'));
  } catch {
    console.log('SKIP chromium: set PLAYWRIGHT_CORE to a playwright-core directory');
  }
  if (chromium) {
    ran++;
    const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
    for (const c of each) {
      const tag = `chromium-${c.tag}`;
      const ctx = await browser.newContext({ viewport: { width: c.width, height: c.height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
      await ctx.addInitScript(init(c.text));
      const page = await ctx.newPage();
      page.on('pageerror', (e) => check(`${tag}: no page errors`, false, e.message));
      await page.goto(URL_);
      await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
      await page.evaluate(`window.__step = 'open';\n${MEASURE}`);
      await page.locator('[data-panel="screener.queue"]').waitFor({ timeout: 15000 });
      await sleep(900);
      const m = JSON.parse(await page.evaluate(`window.__step = 'measure';\n${MEASURE}`));
      await page.screenshot({ path: join(OUT, `${tag}.png`) });
      judge(tag, m);
      await ctx.close();
    }
    await browser.close();
  }
}

server.closeAllConnections();
server.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} of ${results.length} checks passed (${SIZES.length} windows, ${TEXTS.length} text sizes, ${ran} of ${ENGINES.length} engines)`);
if (!ran) {
  console.log('FAILED no engine ran');
  process.exit(1);
}
if (failed.length) {
  for (const f of failed) console.log('FAILED', f.name, f.detail);
  process.exit(1);
}
