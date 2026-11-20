// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The desk's room (1.2.0): drives the desk in a real browser against the hosts' stand-in control
// plane (station.mjs) in the states where 1.1.1 drew one thing over another or out of sight,
// measures them, and captures them. Like shoot.mjs it needs no stack and no port but its own
// loopback one.
//
// What it holds, at each size:
//
//   1. The stage's head. End show lay over the Dump box at 1,440 × 900 whenever playout was down:
//      the title kept 12 rem and the lamp, the delay and End show shared the rest. Checked with
//      the widest lamp beside the widest delay, with Dump armed, with the tag that says Dump is
//      off, and with the frame in two rows: no part of the head is narrower than its content,
//      no two controls touch, and the head is one height throughout.
//   2. The stage's height. Not on air took its height first and the on-air tiles shared what was
//      left: a caller's note on strip 4 was cut when playout dropped (its tile grew the row by
//      39 px), and with three on air and two waiting the on-air tiles lost their four buttons.
//      Checked: nothing on the stage moves when playout drops; no line of any tile is cut; the
//      live buttons are whole in every on-air tile with two, three, four and five on air; Not on
//      air scrolls inside itself and its last tile is reached, and a control the keys reach comes
//      out from under its name; a note arriving on one tile cuts nothing; and Solo's seat, the
//      next show's card, the join offer and a kept place are whole.
//   3. One column (a narrow window or large text). The toast was at the foot of a page 2,000 px
//      long. Checked: a toast is inside the window wherever the page is scrolled, above the
//      status bar and over nothing; its arrival moves nothing; it is drawn once; it goes back to
//      the slot when the window is three columns again.
//
//   just studio-check          # or: cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/hosts/shoot-desk.mjs
//   SIZES=2560x1440@1.2 … node tests/hosts/shoot-desk.mjs      # one size: width x height @ text size
//   DIST=/path/to/another/build/dist … node tests/hosts/shoot-desk.mjs      # an earlier build, to see the checks fail on it
//
// By default: 1440×900 at text 1 (three columns, the window the app opens at) and at text 1.2
// (one column), so both layouts are driven in one run. Screenshots go to OUT (target/shots-desk
// unless told). The clock is shoot.mjs's. Sample people are the designs' (ADR-0071).
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HostStation } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = process.env.DIST ?? join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-desk');
const PORT = Number(process.env.PORT ?? 28120);
const SIZES = (process.env.SIZES ?? '1440x900@1,1440x900@1.2').split(',').map((s) => {
  const [wh, text = '1'] = s.split('@');
  const [width, height] = wh.split('x').map(Number);
  return { width, height, text: Number(text) };
});
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/studio first`);

/** The station the open window is on. Its event stream is held open from here, as a real
 *  station's is, so the frame says Connected and is as tall as it is in use. */
let station = null;
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (path === '/api/v1/studio/events') {
    if (!station) return res.writeHead(503, { 'content-type': 'application/json' }).end('{"error":"the control plane is not answering"}');
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
    const send = () => station && res.write(`data: ${JSON.stringify(station.studioView())}\n\n`);
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
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

// ---- the station: the M2 stack's desk, as 1.1.1's run captured it ----
const cart = (slot, label, ms) => ({ id: `c${slot}`, bank: 1, slot, label, detail: '', mode: 'one_shot', file: `c${slot}.flac`, duration_ms: ms, gain_db: 0, loudness_lufs: -16, true_peak_dbtp: -1, ramp_ms: null, origin: 'demo pack', licence: 'CC0-1.0', missing: false, show: null });
const CATALOGUE = { version: 1, show: null, banks: [{ number: 1, name: 'Reactions and comedy', colour_slot: 3, show: null }], carts: [cart(1, 'Applause after a joke', 12000), cart(2, 'Applause, short', 11000), cart(3, 'Crowd hurray', 4000), cart(4, 'Clapping and hurray', 12000), cart(5, 'Crowd ooh-aah', 5000), cart(6, 'Audience laugh, small', 3000)], loudness_lufs: -16 };
const RUNDOWN = [
  { at: '13:55', title: 'Phone-in', detail: 'callers through Moira', hard_post: false, key: null },
  { at: '14:00', title: 'News · 3:00', detail: 'hard post · playout fires it', hard_post: true, key: 'F2' },
  { at: '14:03', title: 'Weather', detail: 'Eilidh reads · Weather bed', hard_post: false, key: 'F4' },
];
/** Playout's link lost three seconds ago; and the delay as it is while it builds, the widest it says. */
const down = (v) => ({ ...v, desk: { ...v.desk, playout: { connected: false, for_ms: 3_000 } } });
const building = (v) => ({ ...v, desk: { ...v.desk, delay: { delay_ms: 6000, target_ms: 8000, rebuilding: true, dumps: 0, dumped_at_ms: null } } });
const DEAD_AIR = (st) => ({ id: 9001, at_ms: st.now * 1000 - 1000, surface: 'desk', tone: 'serious', kind: 'playout.dead_air', subject: null, title: 'Dead air', message: 'The desk was silent too long while live; Kettle Overnight is on air until it sounds again.' });

/** Kettle FM at Full with five people at the desk: `onAir` of them on the stage, the rest under
 *  Not on air beside playout's own tile. Dougal is a caller with a note; Finlay a DJ, cued.
 *  `over` changes the view it gives; `setup` is what the station says of its setup page. */
function desk({ onAir = ['eilidh', 'gregor', 'kirsty', 'dougal'], ...o } = {}) {
  const st = new HostStation({ step: 'full', ...o });
  st.join('eilidh', 'Eilidh Fraser-MacLeod', 'presenter', { lufs: -21.5 });
  st.join('gregor', 'Gregor Stewart', 'guest', { lufs: -35 });
  st.join('kirsty', 'Kirsty Wilson', 'guest', { lufs: -37.5 });
  st.join('dougal', 'Dougal Thomson', 'caller', { lufs: -43.2 });
  st.join('finlay', 'Finlay Grant', 'dj', { lufs: -23.2 });
  for (const p of st.people) p.on_air = onAir.includes(p.identity);
  st.people.find((p) => p.identity === 'finlay').cue = !onAir.includes('finlay');
  st.audience = { listeners: null, by_mount: [], error: 'the listener process does not answer on its status address', edges: false, scope: 'origin' };
  st.over = building;
  st.setup = null;
  const view = st.viewNow.bind(st);
  st.viewNow = () => {
    const v = view();
    v.show = { title: 'The Morning Kettle', rundown: RUNDOWN };
    v.sources = v.sources.map((s) => (s.identity === 'dougal' ? { ...s, caller: { line: 'Please put me through, the bins', note: 'Nervous: go gentle', name_on_air: 'Dougal', sent: true, taken: true } } : s));
    return st.over(v);
  };
  return st;
}

/** A window on a station, at a size and a text size. */
async function open(st, size, { theme = 'dark' } = {}) {
  station = st;
  const ctx = await browser.newContext({ viewport: { width: size.width, height: size.height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
  await ctx.addInitScript(([t, z]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', String(z));
    } catch {}
  }, [theme, size.text]);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(st.now * 1000));
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.route(/\/(api\/|health)/, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (url.pathname === '/api/v1/studio/events') return route.fallback();
    if (url.pathname === '/api/v1/setup' && st.setup) return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(st.setup) });
    if (/^\/api\/v1\/carts($|\?)/.test(url.pathname + url.search)) return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(CATALOGUE) });
    if (url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    const [status, body] = st.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 15000 });
  await page.locator('[data-panel="desk.carts"] .cart:not(.empty)').first().waitFor({ timeout: 8000 });
  await sleep(1500);
  return { ctx, page };
}
/** The view changes, and the window follows within a second. */
async function turn(page, st, over, lamp) {
  st.over = over;
  await page.locator(`[data-panel="desk.lamp"][data-state="${lamp}"]`).waitFor({ timeout: 6000 });
  await sleep(500);
}
const columns = (page) => page.evaluate(() => getComputedStyle(document.querySelector('.desk .cols')).gridTemplateColumns.split(' ').length);

/** The stage's head as drawn: its height, which of its parts are narrower than their content,
 *  which controls touch, and which reach out of the head or the stage. */
const headNow = (page) =>
  page.evaluate(() => {
    const head = document.querySelector('.stage-head');
    const hb = head.getBoundingClientRect();
    const stage = document.querySelector('[data-panel="desk.stage"]').getBoundingClientRect();
    const say = (e) => (e.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 18);
    const parts = [...head.children].filter((c) => c.getClientRects().length);
    const controls = [...head.querySelectorAll('button, .lamp, .state, .offtag')].filter((c) => c.getClientRects().length && !c.closest('dialog'));
    const touching = [];
    for (let i = 0; i < controls.length; i++)
      for (let j = i + 1; j < controls.length; j++) {
        if (controls[i].contains(controls[j]) || controls[j].contains(controls[i])) continue;
        const a = controls[i].getBoundingClientRect();
        const b = controls[j].getBoundingClientRect();
        if (a.left < b.right - 0.5 && b.left < a.right - 0.5 && a.top < b.bottom - 0.5 && b.top < a.bottom - 0.5) touching.push(`${say(controls[i])} × ${say(controls[j])}`);
      }
    const title = head.querySelector('.title').getBoundingClientRect();
    return {
      height: Math.round(hb.height * 10) / 10,
      rows: new Set(parts.map((c) => Math.round(c.getBoundingClientRect().top / 20))).size > 1 && parts.some((c) => c.getBoundingClientRect().top >= parts[0].getBoundingClientRect().bottom - 1) ? 2 : 1,
      squeezed: parts.filter((c) => c.scrollWidth > c.clientWidth + 1).map((c) => `${c.className.split(' ')[0]}: ${c.scrollWidth} in ${c.clientWidth}`),
      touching,
      out: controls.filter((c) => {
        const r = c.getBoundingClientRect();
        return r.right > hb.right + 0.5 || r.left < hb.left - 0.5 || r.right > stage.right + 0.5 || r.bottom > hb.bottom + 0.5 || r.top < hb.top - 0.5;
      }).map(say),
      title: Math.round(title.width),
      dump: say(head.querySelector('.delay .btn.big') ?? head),
    };
  });
function headSound(h, what) {
  check(`${what}: no part of the stage’s head is narrower than what it holds`, h.squeezed.length === 0, h.squeezed.join(', '));
  check(`${what}: no two controls of the head touch (the lamp, Dump, End show, Screener, the delay’s chip)`, h.touching.length === 0, h.touching.join(', '));
  check(`${what}: every control of the head is inside it, and inside the stage`, h.out.length === 0, h.out.join(', '));
}

/** The stage as drawn, in window pixels: each tile's box, whether its four buttons are whole
 *  inside it, and which of its lines are cut at its edge; the box under the head; Not on air. */
const stageNow = (page) =>
  page.evaluate(() => {
    const r1 = (n) => Math.round(n * 10) / 10;
    const box = (e) => {
      if (!e || !e.getClientRects().length) return null;
      const b = e.getBoundingClientRect();
      return { top: r1(b.top), bottom: r1(b.bottom), left: r1(b.left), right: r1(b.right) };
    };
    const q = (s) => document.querySelector(s);
    const tiles = [...document.querySelectorAll('.tiles > .tile, .waiting > .tile')].map((t) => {
      const tb = t.getBoundingClientRect();
      const inner = tb.bottom - parseFloat(getComputedStyle(t).paddingBottom) + 1;
      const controls = t.querySelector('.controls').getBoundingClientRect();
      return {
        panel: t.dataset.panel,
        onStage: !!t.closest('.tiles'),
        box: box(t),
        buttons: controls.height > 20 && controls.bottom <= inner && controls.top >= tb.top,
        buttonsAt: [r1(controls.top), r1(controls.bottom)],
        // a button of 24 px in a row of 16 reaches 4 px past it on purpose (the handover's)
        cut: [...t.querySelectorAll('.lines > *:not(.hand), .rest > *')].filter((x) => x.getBoundingClientRect().height && x.getBoundingClientRect().bottom > inner).map((x) => (x.textContent || '').trim().slice(0, 40)),
      };
    });
    const body = q('.stage-body') ?? q('[data-panel="desk.stage"]');   // a build before 1.2.0 has no box under the head
    const waiting = q('.waiting');
    return {
      tiles,
      body: { box: box(body), scrolls: body.scrollHeight - body.clientHeight > 1 },
      waiting: waiting ? { box: box(waiting), scrolls: waiting.scrollHeight - waiting.clientHeight > 1, name: box(waiting.querySelector('[data-panel-title]')) } : null,
      head: box(q('.stage-head')),
      status: box(q('.statusbar')),
      cols: box(q('.desk .cols')),
    };
  });
const boxes = (s) => s.tiles.map((t) => [t.panel, t.box, t.buttonsAt]);
/** What every state of the stage must hold, in three columns. */
function stageSound(s, what) {
  const j = JSON.stringify(s);
  const on = s.tiles.filter((t) => t.onStage);
  check(`${what}: every on-air tile has its four live buttons whole inside it`, on.length > 0 && on.every((t) => t.buttons), j);
  check(`${what}: no line or note of any tile is cut at the tile’s edge`, s.tiles.every((t) => t.cut.length === 0), s.tiles.filter((t) => t.cut.length).map((t) => `${t.panel}: ${t.cut.join(' | ')}`).join(' · '));
  check(`${what}: the stage ends above the status bar`, s.body.box.bottom <= s.status.top + 0.5, j);
  if (s.waiting) check(`${what}: Not on air keeps its name in sight, under the on-air tiles`, !!s.waiting.name && s.waiting.name.top >= Math.max(...on.map((t) => t.box.bottom)) - 0.5 && s.waiting.name.bottom <= s.status.top, JSON.stringify(s.waiting));
}

const shot = async (page, tag, name) => {
  await sleep(250);
  await page.screenshot({ path: join(OUT, `${tag}-${name}.png`) });
  log('shot', `${tag}-${name}`);
};

for (const size of SIZES) {
  const tag = `${size.width}${size.text === 1 ? '' : `-text${Math.round(size.text * 100)}`}`;
  const at = `${size.width}×${size.height}, text ${size.text}`;

  // ---- 1. The stage's head: Dump and End show are never the part that gives way ----
  let three;
  {
    const st = desk();
    const { ctx, page } = await open(st, size);
    three = (await columns(page)) === 3;
    const up = await headNow(page);
    headSound(up, `${at}, on air, the delay building`);
    await turn(page, st, (v) => down(building(v)), 'playout-down');
    const lost = await headNow(page);
    headSound(lost, `${at}, Playout down, the delay building`);
    check(`${at}: the head is as tall with Playout down as up, so nothing under it moves`, lost.rows > 1 || Math.abs(lost.height - up.height) <= 0.5, `${up.height} then ${lost.height}`);
    await shot(page, tag, 'H1-head-playout-down');
    // Dump armed: "Press again" is the widest the button says.
    await page.keyboard.press('Delete');
    await page.locator('.stage-head .delay .btn.big.armed').waitFor({ timeout: 4000 });
    await sleep(300);
    const armed = await headNow(page);
    check(`${at}: Delete arms Dump, which says Press again`, armed.dump.startsWith('Press again'), armed.dump);
    headSound(armed, `${at}, Playout down, Dump armed`);
    check(`${at}: arming Dump does not change the head’s height, and the title still has room for its name’s first word`, (armed.rows > 1 || Math.abs(armed.height - up.height) <= 0.5) && armed.title >= 80, JSON.stringify(armed));
    await shot(page, tag, 'H2-head-dump-armed');
    await ctx.close();
  }
  {
    // The frame in two rows (an admin, with the station's setup unfinished), as the M2 stack's is.
    const st = desk({ role: 'admin' });
    st.setup = { status: 'pending', missing: ['a takedown contact'] };
    st.over = (v) => down(building(v));
    const { ctx, page } = await open(st, size);
    await page.locator('[data-panel="desk.lamp"][data-state="playout-down"]').waitFor({ timeout: 6000 });
    headSound(await headNow(page), `${at}, Playout down, the setup reminder in the frame`);
    if (three) stageSound(await stageNow(page), `${at}, Playout down, the setup reminder in the frame`);
    await shot(page, tag, 'H3-playout-down-setup-reminder');
    await ctx.close();
  }
  {
    // Show everything at Guests: Dump is drawn switched off, with the tag that says so.
    const st = desk({ step: 'guests' });
    st.over = (v) => down(building(v));
    const { ctx, page } = await open(st, size);
    await page.locator('button.showall').click();
    await page.locator('.stage-head .delay .offtag').waitFor({ timeout: 4000 });
    await sleep(400);
    const h = await headNow(page);
    headSound(h, `${at}, everything shown at Guests, Playout down`);
    const under = await page.evaluate(() => {
      const d = document.querySelector('.stage-head .delay .btn.big').getBoundingClientRect();
      const t = document.querySelector('.stage-head .delay .offtag').getBoundingClientRect();
      return { under: t.top >= d.bottom, beside: t.left >= d.right };
    });
    check(`${at}: the tag that says Dump is off stands clear of Dump, under it or beside it`, under.under || under.beside, JSON.stringify(under));
    if (three) stageSound(await stageNow(page), `${at}, everything shown at Guests, Playout down`);
    await shot(page, tag, 'H4-head-dump-off-tag');
    await ctx.close();
  }

  if (three) {
    // ---- 2. The stage's height: the tiles on air keep theirs, and Not on air gives way ----
    {
      // Four on air, a caller's note on strip 4, a DJ and playout under Not on air: the M2 stack's desk.
      const st = desk();
      const { ctx, page } = await open(st, size);
      const up = await stageNow(page);
      stageSound(up, `${at}, four on air`);
      check(`${at}, four on air: the caller’s note on strip 4 is drawn`, (await page.locator('[data-panel="desk.tile.4"] .note').innerText()).includes('Nervous: go gentle'));
      await shot(page, tag, 'S1-four-on-air');
      await turn(page, st, (v) => down(building(v)), 'playout-down');
      const lost = await stageNow(page);
      stageSound(lost, `${at}, four on air, Playout down`);
      check(`${at}: when playout drops no tile moves and none changes height, playout’s own included`, same(boxes(up), boxes(lost)), `${JSON.stringify(boxes(up))} then ${JSON.stringify(boxes(lost))}`);
      const six = page.locator('[data-panel="desk.tile.6"]');
      check(`${at}: playout’s tile says what its loss means, in the place of what playout does`, (await six.locator('[data-testid="playout-down-line"]').count()) === 1 && (await six.locator('[data-testid="playout-down-line"]').innerText()).trim() === 'Listeners hear nothing of the room' && (await six.locator('.extra').count()) === 0 && !(await six.innerText()).includes('Takes the desk'), await six.innerText());
      await shot(page, tag, 'S2-four-on-air-playout-down');
      await ctx.close();
    }
    {
      // A note arrives on one on-air tile (a guest leaves the room: their tile says so for 30 s).
      // The row it is in grows for it and the rows share what is to spare, so the other row may
      // move, by no more than the note's height; nothing is cut, and no tile leaves the stage.
      const st = desk();
      const { ctx, page } = await open(st, size);
      const before = await stageNow(page);
      const gregor = st.people.find((p) => p.identity === 'gregor');
      Object.assign(gregor, { present: false, away_ms: 5000, linked: false });
      await page.locator('[data-panel="desk.tile.2"] .rest .extra').waitFor({ timeout: 6000 });
      await sleep(400);
      const after = await stageNow(page);
      stageSound(after, `${at}, four on air, a guest has left the room`);
      const note = (await page.locator('[data-panel="desk.tile.2"] .rest .extra').boundingBox()).height;
      const moved = after.tiles.map((t, i) => Math.abs(t.buttonsAt[0] - before.tiles[i].buttonsAt[0]));
      check(`${at}: the note on strip 2 moves no button in its own row, and none in another by more than the note’s height (${Math.round(note)} px)`, moved[0] === 0 && moved[1] === 0 && Math.max(...moved) <= note + 6.5, JSON.stringify(moved));
      log(`note ${at}: the buttons of strips 1 to 6 moved ${JSON.stringify(moved)} px for a note of ${Math.round(note)} px`);
      await shot(page, tag, 'S5-four-on-air-a-guest-left');
      await ctx.close();
    }
    {
      // The stage's other pictures, drawn through the same rows: Solo with the guest's seat and with
      // the next show's card; the join offer over a late host; a lost link's kept place among people.
      const solo = new HostStation({ step: 'solo' });
      solo.join('duncan', 'Duncan Boyd', 'presenter');
      const next = new HostStation({ step: 'solo' });
      next.join('duncan', 'Duncan Boyd', 'presenter');
      const steeped = next.give('steeped', 'Finlay Grant', 'srt');
      next.connect(steeped.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 291 });
      const late = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10 });
      late.now += 5;
      late.mode = 'auto';
      const key = late.give('steeped', 'Finlay Grant', 'srt');
      late.connect(key.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 5, leveller_db: -6.2, lufs: -23.1 });
      const lost = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 41 });
      lost.join('duncan', 'Duncan Boyd', 'presenter');
      lost.join('eilidh', 'Eilidh Fraser', 'guest', { on_air: true });
      const kept = lost.give('steeped', 'Finlay Grant', 'srt');
      lost.connect(kept.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 2520, leveller_db: -7.4, lufs: -22.6 });
      lost.people.find((p) => p.role === 'source').on_air = true;
      lost.acted.add(kept.key_id);
      lost.lose(kept.key_id);
      for (const [what, st, has] of [
        ['Solo, the guest’s seat', solo, '[data-panel="desk.step-note"]'],
        ['Solo, the next show’s card', next, '[data-panel="desk.next"]'],
        ['the join offer over a late host', late, '.tiles .empty.join'],
        ['a lost link’s kept place among people', lost, '.tiles .tile.is-kept'],
      ]) {
        const { ctx, page } = await open(st, size);
        await page.locator(has).waitFor({ timeout: 6000 });
        const drawn = await page.evaluate(() => {
          const r1 = (n) => Math.round(n * 10) / 10;
          const body = document.querySelector('.stage-body').getBoundingClientRect();
          const foot = document.querySelector('.statusbar').getBoundingClientRect();
          const parts = [...document.querySelectorAll('.tiles > *, .waiting > .tile')].map((e) => {
            const b = e.getBoundingClientRect();
            const inner = b.bottom - parseFloat(getComputedStyle(e).paddingBottom) + 1;
            // the handover's button reaches 4 px past its row on purpose
            const cut = [...e.querySelectorAll(':scope > *, .lines > *:not(.hand), .rest > *')].filter((x) => x.getBoundingClientRect().height && x.getBoundingClientRect().bottom > inner).map((x) => (x.textContent || '').trim().slice(0, 30));
            return { what: e.dataset.panel ?? e.className, top: r1(b.top), bottom: r1(b.bottom), left: r1(b.left), right: r1(b.right), cut };
          });
          const over = [];
          for (let i = 0; i < parts.length; i++) for (let j = i + 1; j < parts.length; j++) if (parts[i].left < parts[j].right - 0.5 && parts[j].left < parts[i].right - 0.5 && parts[i].top < parts[j].bottom - 0.5 && parts[j].top < parts[i].bottom - 0.5) over.push(`${parts[i].what} × ${parts[j].what}`);
          return { parts, over, inside: parts.filter((p) => p.what !== 'desk.tile.6').every((p) => p.bottom <= body.bottom + 0.5 && p.bottom <= foot.top), body: r1(body.bottom) };
        });
        check(`${at}, ${what}: every part of the stage is whole, inside it, and over no other`, drawn.parts.length > 0 && drawn.parts.every((p) => p.cut.length === 0) && drawn.over.length === 0 && drawn.inside, JSON.stringify(drawn));
        await ctx.close();
      }
    }
    {
      // Three on air and two waiting: a caller standing by and a DJ, beside playout's tile.
      const st = desk({ onAir: ['eilidh', 'gregor', 'kirsty'] });
      st.over = (v) => down(building(v));
      const { ctx, page } = await open(st, size);
      await page.locator('[data-panel="desk.lamp"][data-state="playout-down"]').waitFor({ timeout: 6000 });
      const s = await stageNow(page);
      stageSound(s, `${at}, three on air and two waiting`);
      const first = s.tiles.filter((t) => !t.onStage).slice(0, 2);
      check(`${at}, three on air and two waiting: Not on air is drawn under them with its first row’s names in sight`, !!s.waiting && first.length === 2 && first.every((t) => t.box.top + 40 <= s.waiting.box.bottom), JSON.stringify(s.waiting));
      if (s.waiting.scrolls) {
        // What gave way is reached by scrolling inside Not on air; nothing on air moves for it.
        await page.locator('.waiting').evaluate((el) => el.scrollTo(0, el.scrollHeight));
        await sleep(300);
        const end = await stageNow(page);
        const six = end.tiles.find((t) => t.panel === 'desk.tile.6');
        check(`${at}: scrolled to its end, Not on air shows playout’s tile whole, its name staying`, six.box.bottom <= end.waiting.box.bottom + 0.5 && six.box.top >= end.waiting.name.bottom - 0.5 && same(end.waiting.name, s.waiting.name), JSON.stringify({ six, waiting: end.waiting }));
        check(`${at}: and no on-air tile moved for it`, same(boxes(s).slice(0, 3), boxes(end).slice(0, 3)));
        // The keys reach a control that has scrolled up out of sight: it comes out from under the name that stays.
        const drop = page.locator('[data-panel="desk.tile.4"] .chips .btn').first();
        await drop.focus();
        await sleep(300);
        const focused = await page.evaluate(() => {
          const b = document.activeElement.getBoundingClientRect();
          const name = document.querySelector('.waiting .wh').getBoundingClientRect();
          const box = document.querySelector('.waiting').getBoundingClientRect();
          return { button: (document.activeElement.textContent || '').trim().slice(0, 12), top: Math.round(b.top), bottom: Math.round(b.bottom), name: Math.round(name.bottom), box: Math.round(box.bottom) };
        });
        check(`${at}: a control of Not on air that takes the focus is brought into sight, under its name and not behind it`, focused.top >= focused.name - 0.5 && focused.bottom <= focused.box, JSON.stringify(focused));
        await page.locator('.waiting').evaluate((el) => el.scrollTo(0, 0));
      } else check(`${at}, three on air and two waiting: everything fits, so Not on air does not scroll`, s.tiles.every((t) => t.box.bottom <= s.body.box.bottom + 0.5), JSON.stringify(s.waiting));
      await shot(page, tag, 'S3-three-on-air-two-waiting');
      await ctx.close();
    }
    {
      // Five on air: three rows of tiles. Where they and the floor do not fit, the part under the head scrolls.
      const st = desk({ onAir: ['eilidh', 'gregor', 'kirsty', 'dougal', 'finlay'] });
      const { ctx, page } = await open(st, size);
      const s = await stageNow(page);
      stageSound(s, `${at}, five on air`);
      const on = s.tiles.filter((t) => t.onStage);
      check(`${at}, five on air: all five tiles are in sight, whole, before anything is scrolled`, on.length === 5 && on.every((t) => t.box.bottom <= s.body.box.bottom + 0.5 && t.box.bottom <= s.status.top), JSON.stringify(on.map((t) => t.box)));
      await shot(page, tag, 'S4-five-on-air');
      await ctx.close();
    }
    {
      // Two on air: a single row on the stage and two rows under Not on air.
      const st = desk({ onAir: ['eilidh', 'gregor'] });
      st.over = (v) => down(building(v));
      const { ctx, page } = await open(st, size);
      await page.locator('[data-panel="desk.lamp"][data-state="playout-down"]').waitFor({ timeout: 6000 });
      stageSound(await stageNow(page), `${at}, two on air and three waiting`);
      await ctx.close();
    }
    {
      // In three columns the toast has the slot at the right column's foot: it is not moved.
      const st = desk();
      st.over = (v) => down(building(v));
      const { ctx, page } = await open(st, size);
      const where = await page.evaluate(() => ({ count: document.querySelectorAll('[data-panel="desk.toast"]').length, inSlot: !!document.querySelector('.col.right .slot > [data-panel="desk.toast"]'), row: !!document.querySelector('.toast-row') }));
      check(`${at}: in three columns the toast is drawn once, in the right column’s slot`, where.count === 1 && where.inSlot && !where.row, JSON.stringify(where));
      await ctx.close();
    }
  } else {
    // ---- 3. One column: a toast is in view wherever the page is scrolled ----
    const st = desk();
    const { ctx, page } = await open(st, size);
    /** Where the things at the top of the page are, to see that a toast's arrival moves none. */
    const top = () =>
      page.evaluate(() => {
        const y = (s) => Math.round(document.querySelector(s).getBoundingClientRect().top * 10) / 10;
        return { clock: y('[data-panel="desk.clock"]'), rundown: y('[data-panel="desk.rundown"]'), playout: y('[data-panel="desk.playout"]'), head: y('.stage-head'), scrolled: Math.round(document.querySelector('.desk .cols').scrollTop) };
      });
    /** The toast against the window, the scrolling part and the status bar. */
    const toastNow = () =>
      page.evaluate(() => {
        const r1 = (n) => Math.round(n * 10) / 10;
        const box = (e) => {
          if (!e || !e.getClientRects().length) return null;
          const b = e.getBoundingClientRect();
          return { top: r1(b.top), bottom: r1(b.bottom), left: r1(b.left), right: r1(b.right) };
        };
        const all = document.querySelectorAll('[data-panel="desk.toast"]');
        const toast = all[0];
        const msg = toast?.querySelector('.msg');
        return {
          count: all.length,
          toast: box(toast),
          answer: box(toast?.querySelector('.acts .btn')),
          // all its words; or, in a studio under 920 px wide, one line of them with the rest in its own title attribute
          whole: !!msg && ((msg.scrollHeight - msg.clientHeight <= 1 && msg.scrollWidth - msg.clientWidth <= 1) || (innerWidth / (Number(document.documentElement.style.zoom) || 1) <= 920 && msg.getAttribute('title') === msg.textContent)),
          row: !!toast?.closest('.toast-row'),
          role: toast?.getAttribute('role'),
          cols: box(document.querySelector('.desk .cols')),
          status: box(document.querySelector('.statusbar')),
          window: { width: innerWidth, height: innerHeight },
          chat: !!document.querySelector('.col.right .slot [data-panel="desk.chat"]')?.getClientRects().length,
        };
      });
    const inView = (t) => !!t.toast && t.toast.top >= t.cols.bottom - 0.5 && t.toast.bottom <= t.status.top + 0.5 && t.toast.top >= 0 && t.toast.bottom <= t.window.height && t.toast.left >= 0 && t.toast.right <= t.window.width + 0.5;
    const quiet = await top();
    const before = await toastNow();
    check(`${at}, one column: with nothing to say there is no toast and no row for one`, before.count === 0, JSON.stringify(before));
    // A notice arrives and nothing else changes (the station's Dead air, which has no answer): it
    // is in view, with all its words, and nothing in the page has moved for it.
    st.notices.push(DEAD_AIR(st));
    await page.locator('[data-panel="desk.toast"]').getByText('Dead air', { exact: true }).waitFor({ timeout: 6000 });
    await sleep(400);
    const notice = await toastNow();
    check(`${at}, one column: a notice (Dead air) is inside the window without scrolling, between the page and the status bar`, inView(notice), JSON.stringify(notice));
    check(`${at}, one column: it is drawn once, an alert, in its own row, with all its words`, notice.count === 1 && notice.row && notice.role === 'alert' && notice.whole && !notice.answer, JSON.stringify(notice));
    const after = await top();
    check(`${at}, one column: its arrival moves nothing in the page`, same(quiet, after), `${JSON.stringify(quiet)} then ${JSON.stringify(after)}`);
    await shot(page, tag, 'T3-one-column-dead-air');
    st.notices.length = 0;
    await page.locator('[data-panel="desk.toast"]').waitFor({ state: 'detached', timeout: 6000 });
    await sleep(300);
    check(`${at}, one column: the toast goes, and its row with it, and still nothing has moved`, (await page.locator('.toast-row').count()) === 0 && same(quiet, await top()));
    // Playout drops: its toast has an answer.
    await turn(page, st, (v) => down(building(v)), 'playout-down');
    await page.locator('[data-panel="desk.toast"]').waitFor({ timeout: 6000 });
    await sleep(400);
    const told = await toastNow();
    check(`${at}, one column: Playout down’s toast is inside the window without scrolling, between the page and the status bar`, inView(told), JSON.stringify(told));
    check(`${at}, one column: it is drawn once, an alert, in its own row, with all its words and its answer whole`, told.count === 1 && told.row && told.role === 'alert' && told.whole && !!told.answer && told.answer.bottom <= told.toast.bottom && told.answer.right <= told.toast.right, JSON.stringify(told));
    check(`${at}, one column: the chat glance stays at the column’s foot`, told.chat, JSON.stringify(told));
    await shot(page, tag, 'T1-one-column-playout-down');
    for (const [where, to] of [['half way down', 0.5], ['at the foot', 1]]) {
      await page.locator('.desk .cols').evaluate((el, f) => el.scrollTo(0, (el.scrollHeight - el.clientHeight) * f), to);
      await sleep(300);
      const t = await toastNow();
      check(`${at}, one column, the page ${where}: the toast is where it was, in view, over nothing`, inView(t) && same(t.toast, told.toast), JSON.stringify(t));
      if (to === 1) await shot(page, tag, 'T2-one-column-playout-down-foot');
    }
    // The keys reach its answer after everything in the page and before the status bar's own Restart.
    const order = await page.evaluate(() => {
      const all = [...document.querySelectorAll('.desk button:not([disabled])')];
      const mine = all.findIndex((b) => b.closest('[data-panel="desk.toast"]'));
      return { before: mine > 0 && !!all[mine - 1].closest('.cols'), after: !!all[mine + 1]?.closest('.statusbar') };
    });
    check(`${at}, one column: the toast’s answer keeps its place in the order of the keys, after the columns and before the status bar`, order.before && order.after, JSON.stringify(order));
    // The window made three columns again (the text size back to 100 %): the toast is in the slot.
    if (size.width > 1400) {
      await page.getByRole('button', { name: 'Reset text size' }).click();
      await page.locator('.col.right .slot > [data-panel="desk.toast"]').waitFor({ timeout: 4000 }).catch(() => {});
      const moved = await page.evaluate(() => ({ columns: getComputedStyle(document.querySelector('.desk .cols')).gridTemplateColumns.split(' ').length, count: document.querySelectorAll('[data-panel="desk.toast"]').length, inSlot: !!document.querySelector('.col.right .slot > [data-panel="desk.toast"]'), row: !!document.querySelector('.toast-row') }));
      check(`${at}: with the text size reset the desk is three columns and the toast is in the slot, once`, moved.columns === 3 && moved.count === 1 && moved.inSlot && !moved.row, JSON.stringify(moved));
      for (let i = 0; i < Math.round((size.text - 1) * 10); i++) await page.getByRole('button', { name: 'Larger text' }).click();
      await page.locator('.toast-row [data-panel="desk.toast"]').waitFor({ timeout: 4000 }).catch(() => {});
      const back = await page.evaluate(() => ({ columns: getComputedStyle(document.querySelector('.desk .cols')).gridTemplateColumns.split(' ').length, count: document.querySelectorAll('[data-panel="desk.toast"]').length, row: !!document.querySelector('.toast-row [data-panel="desk.toast"]') }));
      check(`${at}: and at the larger text again it is back in its row, once`, back.columns === 1 && back.count === 1 && back.row, JSON.stringify(back));
    }
    await ctx.close();
  }
}

await browser.close();
server.close();
server.closeAllConnections?.();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} passed`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
