// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the studio's screens for hosts in their own studios (ADR-0280, ADR-0284) in a real
// browser against a stand-in control plane (station.mjs), checks each flow against what the
// studio sent, and captures the screens: the source's tile on the desk, Settings › Own studios
// with the setup and its once-only details, the rows, the rules, the show editor's Comes from,
// and Your show; the post's rule on the desk (the grace with its toast and the handover's two
// presses, the late count, a lost link with Off air), `desk.next` at Solo, and the monitor link
// (the tile's line, Talk, the link in the details, Open my monitor). Like tests/newshow/shoot.mjs it needs no stack and no port but its own
// loopback one, and so proves the studio half alone; `just test-sources` proves the station's.
//
//   just studio-check          # or: cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/hosts/shoot.mjs
//   WIDTH=2560 HEIGHT=1440 TEXT=1.2 … node tests/hosts/shoot.mjs      # a larger window and text
//   WIDTH=3840 HEIGHT=2160 … node tests/hosts/shoot.mjs
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs). Screenshots go to
// OUT (target/shots-hosts unless told), named for the window's width. The clock is fixed at
// Friday 20 November 2026, 13:52 in Europe/London, the design's own day.
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
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-hosts');
const PORT = Number(process.env.PORT ?? 28100);
const SIZE = { width: Number(process.env.WIDTH ?? 1440), height: Number(process.env.HEIGHT ?? 900), text: Number(process.env.TEXT ?? 1) };
const TAG = `${SIZE.width}${SIZE.text === 1 ? '' : `-text${Math.round(SIZE.text * 100)}`}`;
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

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

/** A window on a station: every /api and /health request is the stand-in's to answer, and the
 *  studio's view is the stand-in's desk as it stands (asked again each second). */
async function open(station, { theme = 'dark', settings = false, schedule = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width: SIZE.width, height: SIZE.height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
  await ctx.grantPermissions(['clipboard-read', 'clipboard-write']);
  await ctx.addInitScript(([t, z]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', String(z));
    } catch {}
    // What the window hands to a browser (Open my monitor) is kept, and nothing is opened.
    window.__opened = [];
    window.open = (url) => (window.__opened.push(String(url)), null);
  }, [theme, SIZE.text]);
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
    const [status, body] = station.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  if (settings) {
    await page.locator('button.settings-tab').click();
    await page.locator('#settings-h').waitFor();
    await sleep(700);
  } else if (schedule) {
    await page.keyboard.press('Control+5');
    await page.waitForSelector('.schedule .ec', { timeout: 15000 });
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
/** A panel alone, with a little room round it. */
const shotOf = async (page, loc, name) => {
  await sleep(250);
  await loc.screenshot({ path: join(OUT, `${TAG}-${name}.png`) });
  log('shot', `${TAG}-${name}`);
};
/** The control is inside the window, so a person can see and press it without scrolling. */
async function inView(page, loc, what) {
  const b = await loc.boundingBox();
  const vp = page.viewportSize();
  check(`${what} is inside the window (${vp.width}×${vp.height}, text ${SIZE.text})`, !!b && b.y >= 0 && b.x >= 0 && b.y + b.height <= vp.height + 0.5 && b.x + b.width <= vp.width + 0.5, b ? `at ${Math.round(b.x)},${Math.round(b.y)} to ${Math.round(b.x + b.width)},${Math.round(b.y + b.height)}` : 'not drawn');
}
/** Nothing in the panel is wider than the panel: no text is cut or pushed out of it. */
async function fits(loc, what) {
  const over = await loc.evaluate((el) => {
    const out = [];
    const box = el.getBoundingClientRect();
    for (const x of el.querySelectorAll('button, input, select, .state, h2, h3, th, td')) {
      const r = x.getBoundingClientRect();
      if (r.width && (r.right > box.right + 1 || r.left < box.left - 1)) out.push(`${x.tagName.toLowerCase()} “${(x.textContent || x.value || '').trim().slice(0, 30)}”`);
    }
    return out;
  });
  check(`${what}: nothing runs out of its panel`, over.length === 0, over.join(', '));
}
/** Nothing in a tile is cut off by the tile's own edge: its last line is whole. */
async function whole(loc, what) {
  const cut = await loc.evaluate((el) => {
    const box = el.getBoundingClientRect();
    const inner = box.bottom - parseFloat(getComputedStyle(el).paddingBottom) + 1;
    return [...el.querySelectorAll('.lines > *, .rest > *')].filter((x) => x.getBoundingClientRect().height && x.getBoundingClientRect().bottom > inner).map((x) => (x.textContent || '').trim().slice(0, 40));
  });
  check(`${what}: none of its lines is cut off at the tile’s edge`, cut.length === 0, cut.join(' | '));
}
const calls = (station, method, re) => station.calls.filter((c) => c.method === method && re.test(c.path));
const tile = (page, n) => page.locator(`[data-panel="desk.tile.${n}"]`);
const panel = (page) => page.locator('[data-panel="settings.hosts"]');
const dlg = (page, name) => page.locator(`dialog[data-panel="settings.hosts.${name}"]`);
const clip = (page) => page.evaluate(() => navigator.clipboard.readText());
const toastOf = (page) => page.locator('[data-panel="desk.toast"]');
const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();
const clockLine = (page) => page.locator('[data-panel="desk.clock"] .post');
const postRow = (page) => page.locator('[data-panel="desk.rundown"] [data-testid="post-row"]');
/** The station's clock moves on, and the window's with it; the next view follows within a second. */
async function tick(page, st, seconds = 0) {
  st.now += seconds;
  await page.clock.setFixedTime(new Date(st.now * 1000));
  await sleep(1400);
}
const posted = (st, re) => calls(st, 'POST', re).length;

/** Kettle FM with its hosts as they stand at 13:52: Finlay standing by over SRT, his stand-in
 *  due and not connected, Heather's key refused this lunchtime, Catriona testing for Saturday,
 *  and one key ended. */
function withHosts(o = {}) {
  const st = new HostStation(o);
  st.join('duncan', 'Duncan Boyd', 'presenter');
  st.join('eilidh', 'Eilidh Fraser', 'guest', { on_air: true, lufs: -24.8 });
  const finlay = st.give('steeped', 'Finlay Grant', 'srt', { made_days_ago: 21, staff_id: 'a-finlay' });
  const elspeth = st.give('steeped', 'Elspeth Gordon', 'icecast', { made_days_ago: 2 });
  const heather = st.give('kettle-drive', 'Heather Douglas', 'icecast', { made_days_ago: 40, last_used_days_ago: 1 });
  const catriona = st.give('saturday-night', 'Catriona MacKenzie-Buchanan', 'icecast', { made_days_ago: 3 });
  const rory = st.give('kettle-drive', 'Rory Bruce', 'icecast', { made_days_ago: 60 });
  st.endKey(st.keys.find((k) => k.key_id === rory.key_id));
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo');
  st.connect(catriona.key_id, 'icecast', 'mp3 44.1 kHz stereo', { test: true, ago_s: 95, level_db: -18.3, title: 'Runrig - Loch Lomond' });
  st.refuse(heather.key_id, 'wrong_key');
  return { st, finlay, elspeth, heather, catriona, rory };
}

// ---- 1. The source's tile on the desk: standing by, On and Off, Cue, the monitor link and Talk, Disconnect ----
{
  const { st, finlay } = withHosts();
  const { ctx, page } = await open(st);
  const id = `source-${finlay.key_id.toLowerCase()}`;
  const t5 = tile(page, 5);
  await t5.waitFor();
  check('the source takes strip 5, under Not on air', (await page.locator('[data-panel="desk.waiting"] [data-panel="desk.tile.5"]').count()) === 1);
  check('its chip says STANDBY', /standby/i.test(await t5.locator('.chips .state').first().innerText()));
  check('its role line says Own studio', (await t5.locator('.role').innerText()).includes('Own studio'), await t5.locator('.role').innerText());
  check('its role tooltip names the way in, the show and the format', (await t5.locator('.role').getAttribute('title')) === 'SRT · Steeped · mpegts/aac 48 kHz stereo', await t5.locator('.role').getAttribute('title'));
  check('its link line gives the way in and the steady time', (await t5.locator('[data-testid="source-link"]').innerText()).trim() === 'SRT · steady 4:51', await t5.locator('[data-testid="source-link"]').innerText());
  const mon = t5.locator('[data-testid="source-monitor"]');
  check('it says its monitor link is not open, and what the software sends in the tooltip', (await mon.innerText()).trim() === 'Monitor not open' && ((await mon.getAttribute('title')) ?? '').endsWith('Their software sends mpegts/aac 48 kHz stereo.'), `${await mon.innerText()} / ${await mon.getAttribute('title')}`);
  check('the second readout is the leveller', /leveller\s+−7\.4\s+dB/.test(await t5.locator('.levels').innerText()), await t5.locator('.levels').innerText());
  check('the meter shows its level', /−23\.6\s+LUFS/.test(await t5.locator('.levels').innerText()));
  check('the head’s quiet button is Disconnect, not End pass', (await t5.getByRole('button', { name: 'Disconnect Finlay Grant' }).count()) === 1 && (await t5.getByText('End pass').count()) === 0);
  const talk = t5.getByRole('button', { name: /Talk to Finlay Grant/ });
  check('with the monitor link not open Talk is drawn off, with the design’s reason', (await talk.getAttribute('aria-disabled')) === 'true' && (await talk.getAttribute('title')) === 'Finlay’s monitor link is not open, so they would not hear you.', await talk.getAttribute('title'));
  check('nothing on the desk says the monitor link is not built', !/not built/i.test(await page.locator('[data-panel="desk.stage"]').innerHTML()));
  check('the foot counts own studios apart from the room', (await page.locator('[data-testid="own-studios"]').innerText()).includes('1 standing by') && (await page.locator('[data-panel="desk.status"]').innerText()).includes('2 in'), await page.locator('[data-panel="desk.status"]').innerText());
  await fits(t5, 'the source’s tile, standing by');
  await shot(page, '01-desk-standing-by');
  await shotOf(page, t5, '02-tile-standing-by');

  // Cue on the tile's own key (strip 5's is T).
  check('Cue is offered on the tile’s own key', (await t5.locator('.controls .btn').nth(1).innerText()).replace(/\s+/g, ' ').trim() === 'Cue T', await t5.locator('.controls .btn').nth(1).innerText());
  await page.keyboard.press('t');
  await sleep(300);
  check('T cues the source: the desk is told, and the chip says CUE', calls(st, 'POST', /desk\/commands/).some((c) => c.body.op === 'cue' && c.body.identity === id && c.body.on === true) && /cue/i.test(await t5.locator('.chips .state').first().innerText()));
  await shotOf(page, t5, '03-tile-cued');
  await page.keyboard.press('t');
  await sleep(300);

  // On, as for a person: the strip's own number, through the desk's command for a slot.
  await page.keyboard.press('5');
  await sleep(400);
  check('5 puts it on air through the desk’s own command', calls(st, 'POST', /desk\/commands/).some((c) => c.body.op === 'on_air' && c.body.identity === id && c.body.on === true));
  check('on air it moves to the stage and says ON AIR', (await page.locator('.tiles [data-panel="desk.tile.5"]').count()) === 1 && /on air/i.test(await t5.locator('.chips .state').first().innerText()));
  check('the foot says 1 on air', (await page.locator('[data-testid="own-studios"]').innerText()).includes('1 on air'));
  await fits(t5, 'the source’s tile, on air');
  await shot(page, '04-desk-on-air');
  // Talk's key says why nothing happens, and sends nothing.
  const before = st.calls.length;
  await page.keyboard.down('g');
  await sleep(250);
  await page.keyboard.up('g');
  check('Talk’s key sends nothing and says why', !st.calls.slice(before).some((c) => c.method === 'POST') && (await page.locator('[data-panel="desk.toast"]').innerText()).includes('would not hear you'), await page.locator('[data-panel="desk.toast"]').innerText().catch(() => 'no toast'));
  await page.keyboard.press('Escape');

  // The host opens their monitor link: the tile says so, and Talk, held, goes into it.
  st.listen(finlay.key_id);
  await sleep(1400);
  check('with the monitor link open the tile says so, and Talk is live', (await mon.innerText()).trim() === 'Monitor open' && (await talk.getAttribute('aria-disabled')) !== 'true' && !(await talk.getAttribute('title')), `${await mon.innerText()} / ${await talk.getAttribute('aria-disabled')}`);
  await page.keyboard.down('g');
  await sleep(350);
  const talked = calls(st, 'POST', /desk\/commands/).filter((c) => c.body.op === 'talk_to');
  check('G, held, talks to the host’s monitor link as the presenter in the room', talked.length === 1 && JSON.stringify(talked[0].body) === JSON.stringify({ op: 'talk_to', identity: 'duncan', to: id, on: true }), JSON.stringify(talked.map((c) => c.body)));
  check('the tile says TALK while it is held', /talk/i.test(await words(t5.locator('.chips'))) && (await talk.getAttribute('aria-pressed')) === 'true', await words(t5.locator('.chips')));
  await fits(t5, 'the source’s tile, talking');
  await shotOf(page, t5, '04b-tile-monitor-open-talk');
  await page.keyboard.up('g');
  await sleep(350);
  const let_go = calls(st, 'POST', /desk\/commands/).filter((c) => c.body.op === 'talk_to');
  check('letting go closes the talk', let_go.length === 2 && let_go[1].body.on === false && (await talk.getAttribute('aria-pressed')) === 'false');
  st.listen(finlay.key_id, false);
  await sleep(1400);
  check('the monitor link closed, Talk is off again', (await mon.innerText()).trim() === 'Monitor not open' && (await talk.getAttribute('aria-disabled')) === 'true');

  await page.keyboard.press('5');
  await sleep(400);
  check('5 again takes it off air, back under Not on air', (await page.locator('[data-panel="desk.waiting"] [data-panel="desk.tile.5"]').count()) === 1 && st.people.find((p) => p.identity === id).on_air === false);

  // Disconnect: two presses of its button, no key.
  await t5.getByRole('button', { name: 'Disconnect Finlay Grant' }).click();
  check('the first press arms Disconnect and sends nothing', (await t5.getByRole('button', { name: 'Press again to disconnect Finlay Grant' }).count()) === 1 && calls(st, 'POST', /\/disconnect$/).length === 0);
  await shotOf(page, t5, '05-tile-disconnect-armed');
  await t5.getByRole('button', { name: 'Press again to disconnect Finlay Grant' }).click();
  await sleep(1500);
  check('the second press disconnects by the key', calls(st, 'POST', new RegExp(`/api/v1/sources/${finlay.key_id}/disconnect$`)).length === 1);
  check('the tile leaves the desk', (await tile(page, 5).count()) === 0);
  check('the toast says where to let them back in', (await page.locator('[data-panel="desk.toast"]').innerText()).includes('Settings › Own studios'), await page.locator('[data-panel="desk.toast"]').innerText().catch(() => 'no toast'));
  await shot(page, '06-desk-disconnected');
  await ctx.close();
}

// ---- 2. Two sources among people at Full: one on air over SRT, one standing by over Icecast ----
{
  const st = new HostStation({ step: 'full', nowMinutes: 15 * 60 + 41 });
  st.join('duncan', 'Duncan Boyd', 'presenter', { on_air: false });
  st.join('eilidh', 'Eilidh Fraser', 'guest', { on_air: true, lufs: -24.8 });
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  const heather = st.give('kettle-drive', 'Catriona MacKenzie-Buchanan', 'icecast');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 6140, leveller_db: -3.1, lufs: -22.9 });
  st.people.find((p) => p.role === 'source').on_air = true;
  st.connect(heather.key_id, 'icecast', 'mp3 44.1 kHz stereo', { ago_s: 760, leveller_db: 12, lufs: -31.5 });
  const { ctx, page } = await open(st);
  const on = tile(page, 5);
  const by = page.locator('[data-panel="desk.waiting"] .tile.is-source');
  await by.waitFor();
  check('a second source takes the next free of 1, 2, 3', (await by.getAttribute('data-panel')) === 'desk.tile.3', await by.getAttribute('data-panel'));
  check('an Icecast source says how far behind it is', (await by.locator('[data-testid="source-link"]').innerText()).trim() === 'Icecast · steady 12:40 · about 2 s behind', await by.locator('[data-testid="source-link"]').innerText());
  check('a leveller at the end of its reach says so, and which way', (await by.locator('.extra').innerText()).includes('Too quiet for the station to make up'), await by.locator('.extra').innerText().catch(() => 'no note'));
  check('the source on air is on the stage among the people', (await page.locator('.tiles .tile').count()) === 2 && (await page.locator('.tiles [data-panel="desk.tile.5"]').count()) === 1);
  check('a long name is cut in the head, not wrapped over the chip', await by.locator('.name').evaluate((el) => el.scrollWidth >= el.clientWidth && getComputedStyle(el).whiteSpace === 'nowrap'));
  await fits(on, 'the tile on air');
  await fits(by, 'the tile standing by, with a long name');
  await shot(page, '07-desk-two-sources');
  // The Icecast note for a show with callers, when the leveller is in range.
  st.conns.get(heather.key_id).leveller_db = 4.2;
  await sleep(1400);
  check('an Icecast source on a show with callers carries the note', (await by.locator('.extra').innerText()).includes('Callers will hear Catriona about 2 s late. Use SRT for a show with callers.'), await by.locator('.extra').innerText().catch(() => 'no note'));
  await shotOf(page, page.locator('[data-panel="desk.waiting"]'), '08-not-on-air-row');
  await ctx.close();
}

// ---- 2b. A host connects while the desk is watched: said once, with Cue on the tile's own key ----
{
  const st = new HostStation();
  st.join('duncan', 'Duncan Boyd', 'presenter');
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  const { ctx, page } = await open(st);
  check('before anyone connects there is no source and nothing in the foot', (await page.locator('.tile.is-source').count()) === 0 && (await page.locator('[data-testid="own-studios"]').count()) === 0);
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 6 });
  await tile(page, 5).waitFor();
  const toast = page.locator('[data-panel="desk.toast"]');
  await toast.waitFor();
  const said = (await toast.innerText()).replace(/\s+/g, ' ').trim();
  check('the desk says the host is standing by, for which show and when', said.startsWith('Finlay is standing by From their own studio, for Steeped at 14:00.'), said);
  check('with Cue on the tile’s own key, and Dismiss', (await toast.getByRole('button', { name: /^Cue/ }).innerText()).replace(/\s+/g, ' ').trim() === 'Cue T' && (await toast.getByRole('button', { name: /^Dismiss/ }).count()) === 1);
  await inView(page, toast.getByRole('button', { name: /^Cue/ }), 'the toast’s Cue');
  await shot(page, '09-desk-arrival');
  const before = st.calls.filter((c) => c.method === 'POST').length;
  await page.keyboard.press('Escape');
  await sleep(1300);
  check('Esc dismisses it for good, and sends nothing', (await toast.count()) === 0 && st.calls.filter((c) => c.method === 'POST').length === before);
  await ctx.close();
}

// ---- 2c. At Solo: the next show's source is one card in the guest's empty seat (`desk.next`) ----
const nextCard = (page) => page.locator('[data-panel="desk.next"]');
{
  const st = new HostStation({ step: 'solo' });
  st.join('duncan', 'Duncan Boyd', 'presenter');
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 291 });
  const id = `source-${finlay.key_id.toLowerCase()}`;
  const { ctx, page } = await open(st);
  const card = nextCard(page);
  await card.waitFor();
  check('at Solo there is no Not on air row, and the next show’s source is not a tile', (await page.locator('[data-panel="desk.waiting"]').count()) === 0 && (await tile(page, 5).count()) === 0 && (await page.locator('.tiles .tile').count()) === 1);
  check('it has the guest’s empty seat: beside the DJ’s tile, in place of the Solo note', (await page.locator('.tiles > *').evaluateAll((els) => els.map((e) => e.dataset.panel).join(' '))) === 'desk.tile.1 desk.next' && (await page.locator('[data-panel="desk.step-note"]').count()) === 0, await page.locator('.tiles > *').evaluateAll((els) => els.map((e) => e.dataset.panel).join(' ')));
  check('the card says Next, the show and its host, how it stands, and when it goes on air', /^Next Steeped · Finlay Grant Standby Own studio · SRT · steady 4:51 The post is at 14:00:00\. Or on air when you end the show\. Cue Finlay T$/i.test(await words(card)), await words(card));
  check('it has its meter, and one button: Cue, on its strip’s own key. No On.', (await card.locator('.hmeter').getAttribute('aria-label')) === 'Short-term −23.6 LUFS, target −23' && (await card.locator('button').count()) === 1 && (await words(card.locator('button'))) === 'Cue Finlay T');
  check('the seat is dashed, as every empty seat is, and reads from the left', await card.evaluate((el) => getComputedStyle(el).borderTopStyle === 'dashed' && getComputedStyle(el).textAlign === 'left'));
  check('End show says the air passes to the next host', (await page.locator('[data-testid="end-show"]').getAttribute('title')) === 'Finlay is standing by: Steeped starts as you end.');
  await fits(card, 'the next card');
  await inView(page, card.locator('button'), 'the next card’s Cue');
  await shot(page, '09b-desk-solo');
  await shotOf(page, card, '09d-next-card');

  // The plate, on its title, names the panel.
  await card.locator('[data-panel-title]').hover();
  await sleep(250);
  check('the card’s name plate says desk.next', (await page.locator('#pn-desk-next.show code').innerText().catch(() => '')) === 'desk.next');
  await page.mouse.move(2, 2);

  // Cue is the strip's own key; its number is no On key here.
  await page.keyboard.press('t');
  await sleep(350);
  check('T cues the next host: the desk is told, the chip says CUE and the button is lit', calls(st, 'POST', /desk\/commands/).some((c) => c.body.op === 'cue' && c.body.identity === id && c.body.on === true) && /cue/i.test(await words(card.locator('.state').first())) && (await card.locator('button').getAttribute('aria-pressed')) === 'true', await words(card));
  await shotOf(page, card, '09e-next-card-cued');
  await page.keyboard.press('t');
  await sleep(350);
  const before = st.calls.filter((c) => c.method !== 'GET').length;
  await page.keyboard.press('5');
  await sleep(400);
  check('5 sends nothing at Solo, and says how the air passes', st.calls.filter((c) => c.method !== 'GET').length === before && st.people.find((p) => p.identity === id).on_air === false && (await words(toastOf(page))).startsWith('Finlay has no On key at Solo The air passes to Finlay when you end the show, or by the schedule.'), await words(toastOf(page)).catch(() => 'no toast'));
  await page.keyboard.press('Escape');
  await sleep(300);

  // With everything shown, the whole studio is drawn: the source is a tile again.
  await page.getByRole('button', { name: 'Show everything' }).first().click();
  await sleep(500);
  check('with everything shown the source is a tile under Not on air, and the card is gone', (await nextCard(page).count()) === 0 && (await page.locator('[data-panel="desk.waiting"] [data-panel="desk.tile.5"]').count()) === 1);
  await ctx.close();

  // The grace at Solo: the DJ on air sees the count in the card and on the clock, and the
  // toast offers only Not now, because the handover is off below Full.
  const g = atTheGrace({ step: 'solo' });
  const w = await open(g.st);
  await nextCard(w.page).waitFor();
  check('in a grace the card gives the design’s sentence', (await words(nextCard(w.page).locator('[data-testid="next-when"]'))) === 'On air by the schedule at 16:02:00, or when you end the show.', await words(nextCard(w.page)));
  check('and the toast offers Not now alone, with End show as the other way', /^Heather goes on air in 0:42 ?at 16:02:00 By the schedule, at 16:02:00\. Or end the show, and the air passes now\. Not now Esc$/.test(await words(toastOf(w.page))), await words(toastOf(w.page)));
  await fits(nextCard(w.page), 'the next card in a grace');
  await shot(w.page, '09f-desk-solo-grace');
  await w.ctx.close();

  // A late host at Solo: the count is in the card, with Not yet.
  const late = lateHost({ step: 'solo' });
  const l = await open(late.st);
  const lc = nextCard(l.page);
  await lc.waitFor();
  check('a late host is counted in the card, with Not yet', (await words(lc.locator('[data-testid="next-when"]'))) === 'On air in 0:20 · by the schedule Last 30 seconds.' && (await lc.getByRole('button', { name: 'Not yet' }).count()) === 1, await words(lc));
  await fits(lc, 'the next card in a late count');
  await inView(l.page, lc.getByRole('button', { name: 'Not yet' }), 'the next card’s Not yet');
  await shot(l.page, '09g-desk-solo-late');
  await lc.getByRole('button', { name: 'Not yet' }).click();
  await sleep(1500);
  check('Not yet in the card stops the count, by the key', posted(late.st, new RegExp(`/api/v1/sources/${late.finlay.key_id}/not-yet$`)) === 1 && (await words(lc.locator('[data-testid="next-when"]'))) === 'Nothing is counted: Finlay goes on air by their own press.', await words(lc));
  await l.ctx.close();
}

// ---- 2d. At Full, the Screener tab with a source at the desk ----
{
  const st = new HostStation({ step: 'full' });
  st.join('duncan', 'Duncan Boyd', 'presenter');
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo');
  st.people.find((p) => p.role === 'source').on_air = true;
  const { ctx, page } = await open(st);
  await page.keyboard.press('Control+2');
  await page.locator('[data-panel="screener.queue"]').waitFor();
  await sleep(600);
  const words = await page.locator('.screener, main').first().innerText().catch(() => page.locator('body').innerText());
  check('the Screener tab does not list a host’s own studio as a caller or by a raw role word', !/\bsource\b/.test(words.replace(/sources?\s+say|a source moves/gi, '')) && (await page.locator('[data-panel="screener.queue"]').innerText()).includes('Finlay') === false, words.slice(0, 300));
  await shot(page, '09c-screener-full');
  await ctx.close();
}

// ---- The post's rule on the desk (design sections 3.4 to 3.6, 4.2 and 8) ----

// ---- 2e. Before the post: the clock names the show, the rundown says how its source stands ----
{
  const { st } = withHosts();
  const { ctx, page } = await open(st);
  await tile(page, 5).waitFor();
  check('before the post the clock names the show and counts to it', (await words(clockLine(page))) === 'Steeped · the post 14:00:00 −8:00', await words(clockLine(page)));
  const row = postRow(page);
  check('the rundown has a row for the show whose post it is: where it comes from, and STANDBY', /^14:00 Steeped Finlay Grant · own studio Standby$/i.test(await words(row)), await words(row));
  check('End show says where the air goes before the press: to the host standing by', (await page.locator('[data-testid="end-show"]').getAttribute('title')) === 'Finlay is standing by: Steeped starts as you end.', await page.locator('[data-testid="end-show"]').getAttribute('title'));
  check('nothing is counted for the schedule yet: no Hold, no late count', (await page.getByRole('button', { name: 'Hold' }).count()) === 0 && (await page.locator('[data-testid="late-count"]').count()) === 0);
  await fits(page.locator('[data-panel="desk.clock"]'), 'the clock before the post');
  await fits(page.locator('[data-panel="desk.rundown"]'), 'the rundown before the post');
  await shot(page, '80-desk-before-the-post');
  await ctx.close();
}

/** 16:01:18 on the design's day: Finlay on air from his own studio and running over, Heather
 *  standing by from hers since 15:58; Kettle Drive's post was at 16:00, and the grace counts. */
function atTheGrace(o = {}, { heatherConnected = true } = {}) {
  const st = new HostStation({ step: 'full', nowMinutes: 16 * 60 + 1, ...o });
  st.now += 18;
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  const heather = st.give('kettle-drive', 'Heather Douglas', 'icecast');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 7340, leveller_db: -3.1, lufs: -22.9 });
  st.people.find((p) => p.role === 'source').on_air = true;
  st.acted.add(finlay.key_id);
  if (heatherConnected) st.connect(heather.key_id, 'icecast', 'mp3 44.1 kHz stereo', { ago_s: 203, leveller_db: 2.4, lufs: -23.4 });
  return { st, finlay, heather };
}

// ---- 2f. The grace: the count on the clock and its toast (Not now), Hold and Held by, Let it run; End show passes the air ----
{
  const { st, heather } = atTheGrace();
  const { ctx, page } = await open(st);
  const line = clockLine(page);
  await page.locator('[data-post="grace"]').waitFor();
  check('in the grace the clock says who takes the air, by the schedule, and when', (await words(line)).startsWith('Kettle Drive takes the air −0:42 by the schedule 16:02:00'), await words(line));
  check('the grace’s bar is the warning colour’s, as designed', await line.locator('.bar i').evaluate((el) => getComputedStyle(el).backgroundColor === getComputedStyle(document.documentElement).getPropertyValue('--status-warning').trim() || getComputedStyle(el).backgroundColor !== getComputedStyle(document.querySelector('[data-panel="desk.clock"]')).backgroundColor));
  check('a producer is offered Hold, a button with no key', (await line.getByRole('button', { name: 'Hold' }).count()) === 1 && (await line.locator('kbd').count()) === 0);
  check('the show’s name has moved at the post, and the row is the one under way', (await page.locator('[data-panel="desk.stage"] h1').innerText()) === 'Kettle Drive' && /(^| )now( |$)/.test(await postRow(page).getAttribute('class')) && /^16:00 Kettle Drive Heather Douglas · own studio Standby Bring Heather on air ⇧⏎$/i.test(await words(postRow(page))), await words(postRow(page)));
  check('End show says it passes the air to the next host', (await page.locator('[data-testid="end-show"]').getAttribute('title')) === 'Heather is standing by: Kettle Drive starts as you end.');
  await inView(page, line.getByRole('button', { name: 'Hold' }), 'Hold');
  await fits(page.locator('[data-panel="desk.clock"]'), 'the clock in the grace');
  await fits(page.locator('[data-panel="desk.rundown"]'), 'the rundown in the grace');
  // The grace's toast (design section 8): who goes on air, in how long, by the schedule; On air
  // on the handover's key, Not now on Esc.
  const toast = toastOf(page);
  check('the toast offers the grace: On air on ⇧⏎, Not now on Esc', /^Heather goes on air in 0:42 ?at 16:02:00 By the schedule, at 16:02:00\. Or bring them on now\. On air ⇧⏎ Not now Esc$/.test(await words(toast)), await words(toast).catch(() => 'no toast'));
  check('it is the warning toast, and its count is kept from a screen reader, which is told when it ends', /warning/.test(await toast.getAttribute('class')) && (await toast.locator('.title [aria-hidden="true"]').innerText()) === 'in 0:42' && (await toast.locator('.title .sr-only').innerText()) === 'at 16:02:00');
  check('the next host’s tile and the rundown’s row carry the same press', (await words(tile(page, 1).locator('.lines .hand'))) === 'Bring Heather on air ⇧⏎' && /Bring Heather on air ⇧⏎$/.test(await words(postRow(page))), `${await words(tile(page, 1).locator('.lines')).catch(() => 'no tile')} / ${await words(postRow(page))}`);
  await inView(page, toast.getByRole('button', { name: /^Not now/ }), 'the toast’s Not now');
  await inView(page, toast.getByRole('button', { name: /^On air/ }), 'the toast’s On air');
  await fits(toast, 'the grace’s toast');
  await shot(page, '81-desk-grace');
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), '82-clock-grace');
  await shotOf(page, page.locator('.col.right .slot'), '82b-toast-grace');

  // Not now only puts the offer away: nothing is sent, and the grace runs on.
  const sentBefore = st.calls.filter((c) => c.method !== 'GET').length;
  await page.keyboard.press('Escape');
  await sleep(400);
  check('Esc is Not now: the toast goes, nothing is sent, and the clock goes on counting', (await toast.count()) === 0 && st.calls.filter((c) => c.method !== 'GET').length === sentBefore && (await words(line)).startsWith('Kettle Drive takes the air −0:42'), await words(line));
  await tick(page, st, 4);
  check('the offer stays away for this handover', (await toast.count()) === 0 && (await words(line)).startsWith('Kettle Drive takes the air −0:38'), await words(line));
  await tick(page, st, -4);

  // Under ten seconds the count turns amber, as the hard post's does.
  await tick(page, st, 34);
  check('under ten seconds the count is red, with its glyph and words (14a)', (await words(line)).startsWith('Kettle Drive takes the air −0:08 Last 10 seconds.') && (await line.locator('.left').getAttribute('data-hurry')) === 'red', await words(line));
  await tick(page, st, -34);

  // Hold: one POST, the button waits for the view, and the line says who holds it.
  await line.getByRole('button', { name: 'Hold' }).click();
  await page.locator('[data-post="held"]').waitFor();
  check('Hold asks the station once', posted(st, /\/api\/v1\/air\/post\/hold$/) === 1 && st.heldBy === 'shona');
  check('held, the line says by whom and that it is a press only', (await words(clockLine(page))).startsWith('Handover held by you a press only'), await words(clockLine(page)));
  check('and offers Let it run', (await clockLine(page).getByRole('button', { name: 'Let it run' }).count()) === 1);
  await fits(page.locator('[data-panel="desk.clock"]'), 'the clock, held');
  await shotOf(page, page.locator('[data-panel="desk.clock"]'), '83-clock-held');
  await clockLine(page).getByRole('button', { name: 'Let it run' }).click();
  await page.locator('[data-post="grace"]').waitFor();
  check('Let it run lets the handover go, and the count is back', calls(st, 'DELETE', /\/api\/v1\/air\/post\/hold$/).length === 1 && st.heldBy === null && (await words(clockLine(page))).startsWith('Kettle Drive takes the air −0:42'));

  // End show: the dialog says which, and after the press the desk says where the air went.
  await page.keyboard.press('Control+Shift+E');
  const ask = page.locator('dialog[data-panel="desk.end-show"]');
  await ask.waitFor();
  const asks = await words(ask);
  check('End show’s dialog says the next host starts as you end, and that nothing goes to automation', asks.includes('Heather is standing by: Kettle Drive starts as you end.') && asks.includes('End show passes the air to Heather, at once. Nothing goes to automation.') && !asks.includes('Automation takes the air'), asks);
  await inView(page, ask.getByRole('button', { name: /^End show/ }), 'End show’s confirm');
  await shot(page, '84-end-show-passes');
  await page.keyboard.press('Enter');
  await toastOf(page).getByText('Heather has the air').waitFor();
  check('End show is asked of the station once', posted(st, /\/api\/v1\/show\/end$/) === 1);
  check('afterwards the desk says where the air went', (await words(toastOf(page))).startsWith('Heather has the air 1 source is off air. The air passed to Heather; nothing went to automation.'), await words(toastOf(page)));
  const hid = `source-${heather.key_id.toLowerCase()}`;
  check('and Heather is on air on the stage', st.people.find((p) => p.identity === hid).on_air === true && (await page.locator('.tiles .tile.is-source').count()) === 1 && /on air/i.test(await page.locator('.tiles .tile.is-source .chips .state').first().innerText()));
  await shot(page, '85-desk-passed');
  await ctx.close();
}

// ---- 2f2. On air in the grace is the handover: ⇧⏎ brings the next host on, and again hands over ----
{
  const { st, finlay, heather } = atTheGrace();
  const { ctx, page } = await open(st);
  const fid = `source-${finlay.key_id.toLowerCase()}`;
  const hid = `source-${heather.key_id.toLowerCase()}`;
  const onAirCmds = () => calls(st, 'POST', /desk\/commands/).filter((c) => c.body.op === 'on_air').map((c) => `${c.body.identity}:${c.body.on}`);
  await toastOf(page).getByRole('button', { name: /^On air/ }).waitFor();
  await toastOf(page).getByRole('button', { name: /^On air/ }).click();
  await toastOf(page).getByText('Heather is on air too').waitFor();
  check('On air brings the next host on beside the one on air: one switch, and nobody is taken off', JSON.stringify(onAirCmds()) === JSON.stringify([`${hid}:true`]) && st.people.find((p) => p.identity === fid).on_air === true, onAirCmds().join(' '));
  check('the desk says it is the overlap, and what the next press does', (await words(toastOf(page))).startsWith('Heather is on air too The overlap: both sound together, each at their own level. Press ⇧⏎ again to hand Heather the air.'), await words(toastOf(page)));
  check('both are on the stage, and Heather’s tile offers Hand over', (await page.locator('.tiles .tile.is-source').count()) === 2 && (await words(tile(page, 1).locator('.lines .hand'))) === 'Hand over to Heather ⇧⏎', await words(tile(page, 1).locator('.lines')).catch(() => 'no tile'));
  await fits(tile(page, 1), 'the next host’s tile in the overlap');
  await whole(tile(page, 1), 'the next host’s tile in the overlap');
  await shot(page, '85b-desk-overlap');
  await page.keyboard.press('Escape');
  await sleep(300);
  check('the grace is over once the next host is on air: no toast, no count', (await toastOf(page).count()) === 0 && (await page.locator('[data-post="grace"]').count()) === 0);
  await page.keyboard.press('Shift+Enter');
  await toastOf(page).getByText('Handed over to Heather').waitFor();
  check('⇧⏎ again takes the outgoing host off air, and nobody else', JSON.stringify(onAirCmds()) === JSON.stringify([`${hid}:true`, `${fid}:false`]) && st.people.find((p) => p.identity === hid).on_air === true, onAirCmds().join(' '));
  check('and says who holds the air, without handing a source the talk keys', (await words(toastOf(page))) === 'Handed over to Heather Finlay off air; Heather holds the air from their own studio.', await words(toastOf(page)));
  await ctx.close();
}

// ---- 2g. Who may hold: an admin sees who holds it; a screener is offered no button ----
{
  const { st } = atTheGrace({ role: 'screener' });
  let w = await open(st);
  await w.page.locator('[data-post="grace"]').waitFor();
  check('a screener sees the grace count and is offered no Hold', (await words(clockLine(w.page))).startsWith('Kettle Drive takes the air −0:42') && (await w.page.getByRole('button', { name: 'Hold' }).count()) === 0);
  await w.ctx.close();
  st.role = 'admin';
  st.heldBy = 'shona';
  w = await open(st);
  await w.page.locator('[data-post="held"]').waitFor();
  check('an admin sees who holds it, as the station names them, and may let it run', (await words(clockLine(w.page))).startsWith('Handover held by shona a press only') && (await clockLine(w.page).getByRole('button', { name: 'Let it run' }).count()) === 1, await words(clockLine(w.page)));
  await w.ctx.close();
}

// ---- 2h. The next host has not connected: the DJ on air keeps the air; NOT CONNECTED on the row ----
{
  const { st } = atTheGrace({}, { heatherConnected: false });
  const { ctx, page } = await open(st);
  await page.locator('[data-post="words"]').waitFor();
  check('the clock says the next host has not connected, and who keeps the air', (await words(clockLine(page))) === 'Heather has not connected. You keep the air.', await words(clockLine(page)));
  check('the rundown’s row says NOT CONNECTED, dashed', /^16:00 Kettle Drive Heather Douglas · own studio Not connected$/i.test(await words(postRow(page))) && (await postRow(page).locator('.state.dashed').count()) === 1, await words(postRow(page)));
  check('End show says automation takes the air', (await page.locator('[data-testid="end-show"]').getAttribute('title')) === 'Automation takes the air.');
  check('nothing is counted and nothing is offered', (await page.getByRole('button', { name: 'Hold' }).count()) === 0 && (await page.locator('.tile.is-kept').count()) === 0);
  await fits(page.locator('[data-panel="desk.clock"]'), 'the clock, not connected');
  await fits(page.locator('[data-panel="desk.rundown"]'), 'the rundown, not connected');
  await shot(page, '86-desk-not-connected');
  await page.keyboard.press('Control+Shift+E');
  const ask = page.locator('dialog[data-panel="desk.end-show"]');
  await ask.waitFor();
  check('End show’s dialog then says automation takes the air', (await words(ask)).includes('automation takes the air.') && !(await words(ask)).includes('passes the air'), await words(ask));
  await page.keyboard.press('Escape');
  await ctx.close();
}

/** 14:10:05: Steeped's post was at 14:00 and automation holds the air; Finlay connected at
 *  14:10:00 and has just become steady, so the station counts twenty seconds. */
function lateHost(o = {}) {
  const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10, ...o });
  st.now += 5;
  st.mode = 'auto';
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 5, leveller_db: -6.2, lufs: -23.1 });
  return { st, finlay, id: `source-${finlay.key_id.toLowerCase()}` };
}

// ---- 2i. The late count: On air in 0:20, Go on air now on ⇧⏎, Not yet; Esc on the toast stops it ----
{
  const { st, finlay, id } = lateHost();
  const { ctx, page } = await open(st);
  const t5 = tile(page, 5);
  await t5.locator('[data-testid="late-count"]').waitFor();
  check('the tile counts the host on air, by the schedule', (await words(t5.locator('[data-testid="late-count"]'))) === 'On air in 0:20 · by the schedule Last 30 seconds. Not yet', await words(t5.locator('[data-testid="late-count"]')));
  check('its handover button is Go on air now, on the handover’s key', (await words(t5.locator('.lines .hand'))) === 'Go on air now ⇧⏎', await words(t5.locator('.lines .hand')).catch(() => 'no button'));
  check('the clock counts the same twenty seconds, to the second it ends', (await words(clockLine(page))) === 'Steeped takes the air −0:20 Last 30 seconds. by the schedule 14:10:25', await words(clockLine(page)));
  check('the toast says who goes on air and why, with On air on ⇧⏎ and Not yet on Esc', /^Finlay goes on air in 0:20 ?at 14:10:25 ?Last 30 seconds\. They connected late\. Automation holds their air\. On air ⇧⏎ Not yet Esc$/.test(await words(toastOf(page))), await words(toastOf(page)));
  check('the moving count is kept from a screen reader, which is told when it ends', (await toastOf(page).locator('.title [aria-hidden="true"]').innerText()) === 'in 0:20' && (await toastOf(page).locator('.title .sr-only').first().innerText()) === 'at 14:10:25');
  check('the rundown’s row carries the same button beside how the source stands', /^14:00 Steeped Finlay Grant · own studio Standby Go on air now ⇧⏎$/i.test(await words(postRow(page))), await words(postRow(page)));
  await fits(t5, 'the tile in the late count');
  await whole(t5, 'the tile in the late count');
  await fits(page.locator('[data-panel="desk.rundown"]'), 'the rundown in the late count');
  await inView(page, toastOf(page).getByRole('button', { name: /^Not yet/ }), 'the toast’s Not yet');
  await inView(page, t5.getByRole('button', { name: 'Not yet' }), 'the tile’s Not yet');
  await shot(page, '87-desk-late-count');
  await shotOf(page, t5, '88-tile-late-count');

  // Esc answers the toast on screen: the count stops, by the source's key, and nobody goes on air.
  await page.keyboard.press('Escape');
  await sleep(1500);
  check('Esc on the toast stops the count, by the key', posted(st, new RegExp(`/api/v1/sources/${finlay.key_id}/not-yet$`)) === 1 && posted(st, /\/go$/) === 0 && st.people.find((p) => p.identity === id).on_air === false);
  check('the count leaves the tile and the clock says a press only', (await t5.locator('[data-testid="late-count"]').count()) === 0 && (await words(clockLine(page))) === 'Finlay is standing by · a press only', await words(clockLine(page)));
  check('the tile’s button is then the handover’s own', (await words(t5.locator('.lines .hand'))) === 'Bring Finlay on air ⇧⏎', await words(t5.locator('.lines .hand')).catch(() => 'no button'));
  await page.keyboard.press('Escape');
  await sleep(300);
  await shot(page, '89-desk-count-stopped');

  // ⇧⏎ is one press: Go on air now, by the key; the station takes the air back from automation.
  await page.keyboard.press('Shift+Enter');
  await sleep(1500);
  check('⇧⏎ brings the source on air in one press, by the key', posted(st, new RegExp(`/api/v1/sources/${finlay.key_id}/go$`)) === 1 && !calls(st, 'POST', /desk\/commands/).some((c) => c.body.op === 'on_air'));
  check('it is on the stage and says ON AIR, and the lamp with it', st.mode === 'live' && (await page.locator('.tiles [data-panel="desk.tile.5"]').count()) === 1 && /on air/i.test(await t5.locator('.chips .state').first().innerText()) && /on air/i.test(await page.locator('[data-panel="desk.lamp"]').innerText()));
  await ctx.close();
}

// ---- 2j. The late count's own buttons: Go on air now during the count; Not yet below Full ----
{
  let { st, finlay } = lateHost();
  let w = await open(st);
  await tile(w.page, 5).locator('.lines .hand').click();
  await sleep(1500);
  check('Go on air now on the tile is the same press as ⇧⏎', posted(st, new RegExp(`/api/v1/sources/${finlay.key_id}/go$`)) === 1 && st.mode === 'live');
  await w.ctx.close();

  // Below Full the handover is off (ADR-0271): the count and Not yet are there, the press is not.
  ({ st, finlay } = lateHost({ step: 'guests' }));
  w = await open(st);
  const t5 = tile(w.page, 5);
  await t5.locator('[data-testid="late-count"]').waitFor();
  check('below Full the tile counts and offers Not yet, and no handover button', (await t5.locator('.lines .hand').count()) === 0 && (await t5.getByRole('button', { name: 'Not yet' }).count()) === 1);
  check('and the toast offers Not yet alone', /^Finlay goes on air in 0:20 ?at 14:10:25 ?Last 30 seconds\. They connected late\. Automation holds their air\. Not yet Esc$/.test(await words(toastOf(w.page))), await words(toastOf(w.page)));
  await w.page.keyboard.press('Shift+Enter');
  await sleep(500);
  check('⇧⏎ below Full sends nothing', posted(st, /\/go$/) === 0);
  await w.page.keyboard.press('Escape');
  await sleep(300);
  await t5.getByRole('button', { name: 'Not yet' }).click();
  await sleep(1500);
  check('Not yet on the tile stops the count, by the key', posted(st, new RegExp(`/api/v1/sources/${finlay.key_id}/not-yet$`)) === 1 && (await t5.locator('[data-testid="late-count"]').count()) === 0);
  await w.ctx.close();
}

/** 14:41:00: Steeped from Finlay's own studio, on air since 14:00 with nobody in the room. */
function onAirAlone(o = {}) {
  const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 41, ...o });
  const finlay = st.give('steeped', 'Finlay Grant', 'srt');
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 2520, leveller_db: -7.4, lufs: -22.6 });
  st.people.find((p) => p.role === 'source').on_air = true;
  st.acted.add(finlay.key_id);
  return { st, finlay, id: `source-${finlay.key_id.toLowerCase()}` };
}

// ---- 2k. A dropped link: LINK LOST with its count, the place kept, the toast, and back on air ----
{
  const { st, finlay } = onAirAlone();
  const { ctx, page } = await open(st);
  const t5 = tile(page, 5);
  await t5.waitFor();
  check('before the drop the source is on air on the stage', (await page.locator('.tiles [data-panel="desk.tile.5"]').count()) === 1 && /on air/i.test(await t5.locator('.chips .state').first().innerText()));

  // 14:41:02: the link has been gone for two seconds. The tile stays where it was and says so.
  st.lose(finlay.key_id);
  await tick(page, st, 2);
  check('the tile stays on the stage, on its own strip, and says LINK LOST', (await page.locator('.tiles [data-panel="desk.tile.5"].is-kept').count()) === 1 && (await words(t5.locator('.chips .state').first())).toLowerCase() === 'link lost', await words(t5.locator('.chips')).catch(() => 'no tile'));
  check('with how long it has been lost and until when the place is kept', (await words(t5.locator('[data-testid="source-kept"]'))) === '0:02 · place kept until 14:46', await words(t5.locator('[data-testid="source-kept"]')).catch(() => 'no line'));
  check('in the first seconds there is no toast: only the tile changes', (await toastOf(page).count()) === 0);
  check('LINK LOST borrows LINK DEGRADED’s glyph, hollow, in the serious colour: no new colour', await t5.locator('.chips .state').first().evaluate((el) => el.classList.contains('hollow-serious') && el.querySelector('use').getAttribute('href') === '#g-link-degraded'));
  await shot(page, '90-desk-link-lost');

  // 14:41:07: nobody else is on air, so automation covers. Now the toast, the lamp and the foot.
  st.cover();
  await tick(page, st, 5);
  await toastOf(page).waitFor();
  check('once automation covers, the toast says so and until when the place is kept', (await words(toastOf(page))) === 'Finlay’s link is lost Automation covers. Their place is kept until 14:46. Off air 5 Dismiss Esc', await words(toastOf(page)));
  check('the toast is the serious one, and an alert', /serious/.test(await toastOf(page).getAttribute('class')) && (await toastOf(page).getAttribute('role')) === 'alert');
  check('the tile counts on', (await words(t5.locator('[data-testid="source-kept"]'))) === '0:07 · place kept until 14:46', await words(t5.locator('[data-testid="source-kept"]')));
  check('the lamp says AUTO and the subline whose place is kept', /auto/i.test(await page.locator('[data-panel="desk.lamp"]').innerText()) && (await words(page.locator('[data-testid="kept-sub"]'))) === 'Automation holds the air · Finlay’s place is kept until 14:46', await words(page.locator('[data-testid="kept-sub"]')).catch(() => 'no subline'));
  check('the foot says 1 link lost, and the rundown’s row LINK LOST', (await words(page.locator('[data-testid="own-studios"]'))) === 'Own studios 1 link lost' && /^14:00 Steeped Finlay Grant · own studio Link lost$/i.test(await words(postRow(page))), `${await words(page.locator('[data-testid="own-studios"]'))} / ${await words(postRow(page))}`);
  check('the note under the tile says what listeners hear, and until when', (await words(t5.locator('.extra'))) === 'Listeners hear automation until the stream is steady.', await words(t5.locator('.extra')).catch(() => 'no note'));
  await whole(t5, 'the kept tile');
  check('Cue and Mute have nothing to act on, and say why', (await t5.locator('.controls .btn').nth(1).getAttribute('aria-disabled')) === 'true' && (await t5.locator('.controls .btn').nth(3).getAttribute('aria-disabled')) === 'true');
  await fits(t5, 'the kept tile');
  await inView(page, toastOf(page).getByRole('button', { name: /^Dismiss/ }), 'the toast’s Dismiss');
  await shot(page, '91-desk-covered');
  await shotOf(page, t5, '92-tile-kept');

  // Off air is the strip's own On key, lit while the place is kept: live in the toast and on the tile.
  const offAir = toastOf(page).getByRole('button', { name: /^Off air/ });
  const keptOn = t5.locator('.controls .btn').first();
  check('Off air is live in the toast, and nothing says it is not built', (await offAir.getAttribute('aria-disabled')) !== 'true' && !(await offAir.getAttribute('title')) && !/not built/i.test(await page.locator('[data-panel="desk.stage"]').innerHTML()));
  check('the kept tile’s own On is lit and says it is Off air', (await keptOn.getAttribute('aria-pressed')) === 'true' && (await keptOn.getAttribute('title')) === 'Off air: lets their place go now. If they reconnect, they stand by.', await keptOn.getAttribute('title'));
  await inView(page, offAir, 'the toast’s Off air');
  const before = st.calls.filter((c) => c.method !== 'GET').length;
  await page.keyboard.press('t');
  await sleep(300);
  check('Cue’s key says there is nothing to cue', (await words(toastOf(page))).startsWith('Nothing to cue Finlay’s link is lost'), await words(toastOf(page)));
  await page.keyboard.press('Escape');
  await sleep(300);
  check('the lost-link toast is back once the note is dismissed', (await words(toastOf(page))).startsWith('Finlay’s link is lost'));
  await page.keyboard.press('Escape');
  await sleep(400);
  check('Esc dismisses it; the place stays kept, and nothing was sent', (await toastOf(page).count()) === 0 && (await page.locator('.tiles .tile.is-kept').count()) === 1 && st.calls.filter((c) => c.method !== 'GET').length === before);

  // 14:41:30: his software reconnects. The tile keeps its place on the stage and says it is coming back.
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 1, leveller_db: -7.1, lufs: -22.9 });
  await tick(page, st, 23);
  check('reconnected, it keeps its place on the stage and says it is back once steady', (await page.locator('.tiles [data-panel="desk.tile.5"].is-kept').count()) === 1 && (await page.locator('[data-panel="desk.waiting"] .tile.is-source').count()) === 0 && (await words(t5.locator('[data-testid="source-kept"]'))) === 'Back on air once steady', await words(t5.locator('[data-testid="source-kept"]')).catch(() => 'no line'));
  check('and its level shows again', /−22\.9\s+LUFS/.test(await t5.locator('.levels').innerText()));
  check('the station puts it back by itself: no press is offered, nothing is counted, and it is not said to have arrived', (await page.locator('[data-handover]').count()) === 0 && (await toastOf(page).count()) === 0 && (await words(clockLine(page))) === 'No hard post ahead' && /Link lost$/i.test(await words(postRow(page))) && (await words(page.locator('[data-testid="kept-sub"]'))).includes('Finlay’s place is kept until 14:46'), `${await words(clockLine(page))} / ${await words(postRow(page))}`);
  await whole(t5, 'the reconnected tile');
  await shot(page, '93-desk-reconnected');

  // 14:41:36: steady for five seconds, and back on air.
  st.comeBack();
  await tick(page, st, 6);
  check('back on air, the desk says so', (await words(toastOf(page))) === 'Finlay is back on air Their link held steady, and automation has handed the air back.', await words(toastOf(page)).catch(() => 'no toast'));
  check('and the tile is an ordinary one again: ON AIR, no dashed edge', (await page.locator('.tiles .tile.is-kept').count()) === 0 && /on air/i.test(await t5.locator('.chips .state').first().innerText()) && (await words(page.locator('[data-testid="own-studios"]'))) === 'Own studios 1 on air');
  await shot(page, '94-desk-back-on-air');
  await ctx.close();
}

// ---- 2k2. Off air on a kept place: the toast's button, the strip's own key, and what the station answers ----
{
  // Automation covers already: Off air in the toast lets the place go, and the air stays where it is.
  const a = onAirAlone();
  a.st.now += 7;
  a.st.lose(a.finlay.key_id, { ago_s: 7 });
  a.st.cover();
  let w = await open(a.st);
  await toastOf(w.page).getByRole('button', { name: /^Off air/ }).click();
  await toastOf(w.page).getByText('place is let go').waitFor();
  const route = (k) => new RegExp(`/api/v1/sources/${k}/off-air$`);
  check('Off air asks the station once, by the key, with no body', posted(a.st, route(a.finlay.key_id)) === 1 && calls(a.st, 'POST', route(a.finlay.key_id))[0].body === undefined && !calls(a.st, 'POST', /desk\/commands/).length);
  check('the place is let go: the tile leaves at once, and the desk says automation holds the air', (await w.page.locator('.tile.is-kept').count()) === 0 && (await words(toastOf(w.page))) === 'Finlay’s place is let go Automation holds the air. If Finlay reconnects, they stand by.', await words(toastOf(w.page)));
  check('the lamp still says AUTO, and the foot has no lost link to count', /auto/i.test(await w.page.locator('[data-panel="desk.lamp"]').innerText()) && (await w.page.locator('[data-testid="own-studios"]').count()) === 0);
  await shot(w.page, '95b-desk-off-air');
  // A second press of the strip's key is an empty strip's: nothing more is asked of the station.
  await w.page.keyboard.press('Escape');
  await w.page.keyboard.press('5');
  await sleep(400);
  check('the strip’s key afterwards sends nothing more', posted(a.st, route(a.finlay.key_id)) === 1 && (await words(toastOf(w.page))).startsWith('Nobody is on strip 5'), await words(toastOf(w.page)));
  // It reconnects: standing by, not put back on air by the station.
  await w.page.keyboard.press('Escape');
  a.st.connect(a.finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 8 });
  await tick(w.page, a.st, 20);
  check('reconnected after Off air, the source stands by under Not on air', (await w.page.locator('[data-panel="desk.waiting"] [data-panel="desk.tile.5"]').count()) === 1 && (await w.page.locator('.tile.is-kept').count()) === 0 && /standby/i.test(await tile(w.page, 5).locator('.chips .state').first().innerText()));
  await w.ctx.close();

  // In the first seconds, before automation covers: the strip's own On key is Off air, and the
  // station gives the air to automation first. The desk shows `covered`.
  const b = onAirAlone();
  w = await open(b.st);
  await tile(w.page, 5).waitFor();
  b.st.lose(b.finlay.key_id);
  await tick(w.page, b.st, 2);
  check('before automation covers there is no toast, and the lamp is not AUTO yet', (await toastOf(w.page).count()) === 0 && !/auto/i.test(await w.page.locator('[data-panel="desk.lamp"]').innerText()));
  await w.page.keyboard.press('5');
  await toastOf(w.page).getByText('place is let go').waitFor();
  check('the strip’s own On key is Off air: one ask, by the key', posted(b.st, route(b.finlay.key_id)) === 1 && b.st.mode === 'auto');
  check('the desk says automation has taken the air', (await words(toastOf(w.page))) === 'Finlay’s place is let go Automation has taken the air. If Finlay reconnects, they stand by.', await words(toastOf(w.page)));
  await tick(w.page, b.st, 1);
  check('and the lamp follows: AUTO', /auto/i.test(await w.page.locator('[data-panel="desk.lamp"]').innerText()) && (await w.page.locator('.tile.is-kept').count()) === 0);
  await shot(w.page, '95c-desk-off-air-covered');
  await w.ctx.close();

  // Someone else is on air: the kept tile's lit On button lets the place go, and they keep the air.
  const c = onAirAlone();
  c.st.join('duncan', 'Duncan Boyd', 'presenter');
  w = await open(c.st);
  await tile(w.page, 5).waitFor();
  c.st.lose(c.finlay.key_id);
  await tick(w.page, c.st, 7);
  await tile(w.page, 5).locator('.controls .btn').first().click();
  await toastOf(w.page).getByText('place is let go').waitFor();
  check('with someone else on air, Off air on the tile says they keep the air, and nothing goes to automation', posted(c.st, route(c.finlay.key_id)) === 1 && c.st.mode === 'live' && (await words(toastOf(w.page))) === 'Finlay’s place is let go Duncan keeps the air. If Finlay reconnects, they stand by.', await words(toastOf(w.page)));

  // A place the station has let go already: its refusal is shown as given.
  c.st.lose(c.finlay.key_id);
  await tick(w.page, c.st, 3);
  await tile(w.page, 5).waitFor();
  c.st.lost = null;
  await w.page.keyboard.press('Escape');
  await w.page.keyboard.press('5');
  await sleep(500);
  check('a place let go meanwhile: the station’s own sentence is shown', (await words(toastOf(w.page))).startsWith('Not done no place is being kept for that source'), await words(toastOf(w.page)).catch(() => 'no toast'));
  await w.ctx.close();

  // The real station's view follows a decision by up to a second: a view that still carries the
  // place must not bring the tile and the toast back, nor let a second press be sent.
  const e = onAirAlone();
  e.st.lag = true;
  e.st.now += 7;
  e.st.lose(e.finlay.key_id, { ago_s: 7 });
  e.st.cover();
  w = await open(e.st);
  await toastOf(w.page).getByRole('button', { name: /^Off air/ }).click();
  await toastOf(w.page).getByText('place is let go').waitFor();
  let cameBack = false;
  for (let i = 0; i < 12; i++) {
    cameBack ||= (await w.page.locator('.tile.is-kept').count()) > 0 || (await toastOf(w.page).getByText('link is lost').count()) > 0;
    await sleep(200);
  }
  check('while the station’s view still carries the place, the tile and its toast stay gone', !cameBack && e.st.lost === null);
  await w.page.keyboard.press('Escape');
  await w.page.keyboard.press('5');
  await sleep(400);
  check('and a second press is not sent to be refused', posted(e.st, route(e.finlay.key_id)) === 1);
  await w.ctx.close();

  // A preparing window sends nothing.
  const d = onAirAlone({ preparing: true });
  d.st.now += 7;
  d.st.lose(d.finlay.key_id, { ago_s: 7 });
  d.st.cover();
  w = await open(d.st);
  await w.page.locator('.tile.is-kept').waitFor();
  await w.page.keyboard.press('5');
  await sleep(400);
  check('a preparing window’s Off air sends nothing, and the place stays kept', posted(d.st, /off-air$/) === 0 && (await w.page.locator('.tile.is-kept').count()) === 1 && (await toastOf(w.page).getByRole('button', { name: /^Off air/ }).count()) === 0);
  await w.ctx.close();
}

// ---- 2l. The place is let go, and the tile leaves; Disconnect lets it go at once; a window opened during the loss ----
{
  const { st, finlay } = onAirAlone();
  st.now += 7;
  st.lose(finlay.key_id, { ago_s: 7 });
  st.cover();
  // This window never saw the source on its strip: the place is drawn where a source goes.
  let w = await open(st);
  await w.page.locator('.tiles .tile.is-kept').waitFor();
  check('a window opened during the loss draws the place on a source’s strip', (await w.page.locator('.tiles [data-panel="desk.tile.5"].is-kept').count()) === 1 && (await words(tile(w.page, 5).locator('[data-testid="source-kept"]'))) === '0:07 · place kept until 14:46');
  // Five minutes pass: the station lets the place go.
  st.letGo();
  await tick(w.page, st, 293);
  check('when the place is let go the tile leaves, and the toast with it', (await w.page.locator('.tile.is-kept').count()) === 0 && (await tile(w.page, 5).count()) === 0 && (await toastOf(w.page).count()) === 0);
  check('automation still holds the air, and the foot has no own studio left to count', /auto/i.test(await w.page.locator('[data-panel="desk.lamp"]').innerText()) && (await w.page.locator('[data-testid="own-studios"]').count()) === 0);
  await shot(w.page, '95-desk-place-let-go');
  await w.ctx.close();

  // Disconnect on the kept tile: the built way to let a place go at once (it also turns the key away).
  const again = onAirAlone();
  again.st.now += 7;
  again.st.lose(again.finlay.key_id, { ago_s: 7 });
  again.st.cover();
  w = await open(again.st);
  const t5 = tile(w.page, 5);
  await t5.waitFor();
  await t5.getByRole('button', { name: 'Disconnect Finlay Grant' }).click();
  await t5.getByRole('button', { name: 'Press again to disconnect Finlay Grant' }).click();
  await sleep(1500);
  check('Disconnect on a kept tile lets the place go, by the key', posted(again.st, new RegExp(`/api/v1/sources/${again.finlay.key_id}/disconnect$`)) === 1 && (await w.page.locator('.tile.is-kept').count()) === 0 && (await words(toastOf(w.page))).startsWith('Finlay is disconnected'), await words(toastOf(w.page)).catch(() => 'no toast'));
  await w.ctx.close();
}

// ---- 2m. With someone else on air only the tile changes; a station without playout says the fallback ----
{
  const { st, finlay } = onAirAlone();
  st.join('duncan', 'Duncan Boyd', 'presenter');
  let w = await open(st);
  await tile(w.page, 5).waitFor();
  st.lose(finlay.key_id);
  await tick(w.page, st, 7);
  check('with someone else on air there is no toast and no automation: the tile alone says LINK LOST', (await toastOf(w.page).count()) === 0 && (await w.page.locator('.tiles .tile.is-kept').count()) === 1 && /on air/i.test(await w.page.locator('[data-panel="desk.lamp"]').innerText()) && (await tile(w.page, 5).locator('.extra').count()) === 0);
  check('and the stage keeps slot order: Duncan on 1, the kept place on 5', (await w.page.locator('.tiles .tile').evaluateAll((els) => els.map((e) => e.dataset.panel).join(' '))) === 'desk.tile.1 desk.tile.5');
  await shot(w.page, '96-desk-lost-with-someone-on-air');
  // It reconnects while Duncan has the air: the station does not put it back by itself.
  st.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 9 });
  await tick(w.page, st, 20);
  check('reconnected with someone else on air, it stands by and says it is not put back by itself', (await words(tile(w.page, 5).locator('[data-testid="source-kept"]'))) === 'Reconnected · standing by' && (await words(tile(w.page, 5).locator('.extra'))) === 'Not put back by itself while someone else has the air.', await words(tile(w.page, 5).locator('.lines')).catch(() => 'no tile'));
  await whole(tile(w.page, 5), 'the tile reconnected with someone else on air');
  await w.ctx.close();

  const bare = onAirAlone();
  bare.st.playout = false;
  bare.st.now += 7;
  bare.st.lose(bare.finlay.key_id, { ago_s: 7 });
  bare.st.cover();
  w = await open(bare.st);
  await toastOf(w.page).waitFor();
  check('a station without the playout layer says the fallback covers', (await words(toastOf(w.page))).startsWith('Finlay’s link is lost The fallback covers. Their place is kept until 14:46.') && (await words(tile(w.page, 5).locator('.extra'))) === 'Listeners hear the fallback until the stream is steady.', await words(toastOf(w.page)));
  await w.ctx.close();
}

// ---- 2n. A station older than 1.1 sends no post: the desk is as it was ----
{
  const st = new HostStation({ routes: false });
  st.join('duncan', 'Duncan Boyd', 'presenter');
  const { ctx, page } = await open(st);
  await tile(page, 1).waitFor();
  check('with no post in the view the clock and the rundown are as before', (await words(clockLine(page))) === 'No hard post ahead' && (await postRow(page).count()) === 0 && (await page.locator('[data-panel="desk.rundown"]').innerText()).includes('Nothing scheduled') && (await page.locator('[data-testid="end-show"]').getAttribute('title')) === 'Automation takes the air.');
  await ctx.close();
  // A 1.1 station whose next show comes from the room: the station names its post, with no host
  // to bring on, and the desk is as it was too.
  const room = new HostStation();
  room.join('duncan', 'Duncan Boyd', 'presenter');
  const w = await open(room);
  await tile(w.page, 1).waitFor();
  check('a next show from the room changes nothing on the clock or the rundown', room.studioView().post?.show?.slug === 'steeped' && (await words(clockLine(w.page))) === 'No hard post ahead' && (await postRow(w.page).count()) === 0);
  await w.ctx.close();
}

// ---- 2o. The post's screens in Light and Night ----
for (const theme of ['light', 'night']) {
  let w = await open(atTheGrace().st, { theme });
  await w.page.locator('[data-post="grace"]').waitFor();
  await shot(w.page, `97-${theme}-desk-grace`);
  await w.ctx.close();

  w = await open(lateHost().st, { theme });
  await tile(w.page, 5).locator('[data-testid="late-count"]').waitFor();
  await shot(w.page, `98-${theme}-desk-late-count`);
  await w.ctx.close();

  const lost = onAirAlone();
  lost.st.now += 7;
  lost.st.lose(lost.finlay.key_id, { ago_s: 7 });
  lost.st.cover();
  w = await open(lost.st, { theme });
  await toastOf(w.page).waitFor();
  await fits(tile(w.page, 5), `${theme}: the kept tile`);
  await whole(tile(w.page, 5), `${theme}: the kept tile`);
  await shot(w.page, `99-${theme}-desk-covered`);
  await w.ctx.close();

  // `desk.next` at Solo, and the next card in a grace beside the toast.
  const solo = new HostStation({ step: 'solo' });
  solo.join('duncan', 'Duncan Boyd', 'presenter');
  solo.connect(solo.give('steeped', 'Finlay Grant', 'srt').key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 291 });
  w = await open(solo, { theme });
  await nextCard(w.page).waitFor();
  await fits(nextCard(w.page), `${theme}: the next card`);
  await shot(w.page, `97b-${theme}-desk-solo-next`);
  await w.ctx.close();
  w = await open(atTheGrace({ step: 'solo' }).st, { theme });
  await nextCard(w.page).waitFor();
  await shot(w.page, `97c-${theme}-desk-solo-grace`);
  await w.ctx.close();

  // The tile with its monitor link open, Talk held.
  const talk = withHosts({ step: 'full' });
  talk.st.listen(talk.finlay.key_id);
  w = await open(talk.st, { theme });
  await tile(w.page, 5).locator('.mon.open').waitFor();
  await w.page.keyboard.down('g');
  await sleep(350);
  await shotOf(w.page, tile(w.page, 5), `97d-${theme}-tile-monitor-open-talk`);
  await w.page.keyboard.up('g');
  await sleep(200);
  await w.ctx.close();

  w = await open(atTheGrace({}, { heatherConnected: false }).st, { theme });
  await w.page.locator('[data-post="words"]').waitFor();
  await shotOf(w.page, w.page.locator('.col.left'), `99b-${theme}-not-connected`);
  await w.ctx.close();
}

// ---- 2p. The counts' last seconds (14a): amber in the last 30 s, red in the last 10 s, never by colour alone ----

/** What a figure's colour is in the window, as the browser draws it, and what the design's tokens say. */
const colours = (page, loc, over) =>
  loc.evaluate((el, o) => {
    const rgb = (c) => c.match(/[\d.]+/g).slice(0, 3).map(Number);
    const lum = ([r, g, b]) => {
      const f = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
    };
    const token = (name) => {
      const probe = document.createElement('i');
      probe.style.color = `var(--${name})`;
      document.body.append(probe);
      const c = getComputedStyle(probe).color;
      probe.remove();
      return c;
    };
    let bg = el;
    while (bg && (!getComputedStyle(bg).backgroundColor || getComputedStyle(bg).backgroundColor === 'rgba(0, 0, 0, 0)')) bg = bg.parentElement;
    const own = getComputedStyle(o.of ? el.querySelector(o.of) ?? el : el).color;
    const back = getComputedStyle(bg ?? document.body).backgroundColor;
    const a = lum(rgb(own)), b = lum(rgb(back));
    return { own, back, ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05), warning: token('status-warning'), critical: token('status-critical'), ink: token('ink'), weight: Number(getComputedStyle(o.of ? el.querySelector(o.of) ?? el : el).fontWeight) };
  }, { of: over });

/** The count's mark, as a count in its last seconds draws it (review findings 1 and 8): one shape at both levels, the clock glyph
 *  and never FAULT's triangle, in the level's colour, and directly before the figure it marks: nothing but white space between
 *  them, on one line. `fig` is the figure's own element where it has one; where the figure is bare text (the clock's), it is the
 *  text after the mark. */
const markOf = (count, fig) =>
  count.evaluate((el, figSel) => {
    const marks = [...el.querySelectorAll('svg.hurry')];
    const m = marks[0];
    if (!m) return { marks: 0, fault: el.querySelectorAll('use[href="#g-fault"]').length };
    const f = figSel ? el.querySelector(figSel) : null;
    // What follows the mark, past the renderer's own comments and empty text.
    let next = m.nextSibling;
    while (next && next !== f && (next.nodeType === 8 || (next.nodeType === 3 && !next.textContent.trim()))) next = next.nextSibling;
    const range = document.createRange();
    if (f) range.selectNodeContents(f);
    else range.selectNodeContents(next ?? m);
    const fr = range.getBoundingClientRect();
    const mr = m.getBoundingClientRect();
    return {
      marks: marks.length,
      fault: el.querySelectorAll('use[href="#g-fault"]').length,
      shape: m.querySelector('use')?.getAttribute('href'),
      colour: getComputedStyle(m).color,
      // The mark's own sibling after it is the figure (or the figure's text), and the two share a line, the mark on the left.
      before: f ? next === f : !!next && next.nodeType === 3 && /^[−\d]/.test(next.textContent.trim()),
      gap: Math.round(fr.left - mr.right),
      line: mr.top < fr.bottom && mr.bottom > fr.top,
    };
  }, fig ?? null);
const markOk = (m, c, want) => m.marks === 1 && m.fault === 0 && m.shape === '#g-delay' && m.colour === (want === 'amber' ? c.warning : c.critical) && m.before && m.line && m.gap >= 0 && m.gap <= 8;
/** The weights on the design system's scale: 800 is the on-air lamp's alone, so nothing in a count in its last seconds is drawn at it. */
const heaviest = (page) => page.evaluate(() => Math.max(0, ...[...document.querySelectorAll('[data-hurry], [data-hurry] *')].map((el) => Number(getComputedStyle(el).fontWeight) || 0)));

const GRACE_LEFT = 42;
const levelOf = (s) => (s > 30 ? null : s > 10 ? 'amber' : 'red');
for (const theme of ['dark', 'light', 'night']) {
  // The grace on the clock and in its toast, at 45, 25 and 5 s (the grace is 42 s long at 16:01:18, so 45 s is a minute-eighteen with the rule at 3 minutes).
  for (const left of [45, 25, 5]) {
    const { st } = atTheGrace();
    st.rules.grace_s = 120 + (left - GRACE_LEFT);
    const w = await open(st, { theme });
    const line = clockLine(w.page);
    await line.waitFor();
    const fig = line.locator('.left');
    const want = levelOf(left);
    const txt = await words(line);
    check(`${theme}: the grace at ${left} s says −0:${String(left).padStart(2, '0')}`, txt.includes(`−0:${String(left).padStart(2, '0')}`), txt);
    check(`${theme}: the grace at ${left} s is ${want ?? 'plain'} on the clock`, ((await fig.getAttribute('data-hurry')) ?? null) === want, `${await fig.getAttribute('data-hurry')}`);
    const c = await colours(w.page, fig);
    if (want) {
      check(`${theme}: the figure at ${left} s is the ${want === 'amber' ? 'warning' : 'danger'} token, at weight 700, with the words`, c.own === (want === 'amber' ? c.warning : c.critical) && c.weight === 700 && txt.includes(want === 'amber' ? 'Last 30 seconds.' : 'Last 10 seconds.'), JSON.stringify(c));
      const m = await markOf(fig);
      check(`${theme}: its mark at ${left} s is the clock glyph in the level’s colour, directly before the figure, and no fault triangle`, markOk(m, c, want), JSON.stringify(m));
      check(`${theme}: the 24 px figure at ${left} s meets 3:1 against its panel (large text)`, c.ratio >= 3, `${c.ratio.toFixed(2)}`);
      check(`${theme}: nothing in a count at ${left} s is drawn at 800, the on-air lamp’s weight`, (await heaviest(w.page)) <= 700, `${await heaviest(w.page)}`);
    } else {
      check(`${theme}: the figure at ${left} s is plain: no glyph, no words, the ink`, (await fig.locator('svg.hurry').count()) === 0 && !/Last (10|30) seconds/.test(txt) && c.own === c.ink, JSON.stringify(c));
    }
    const toast = toastOf(w.page);
    await toast.waitFor();
    check(`${theme}: the grace’s toast at ${left} s is ${want ?? 'plain'}, and keeps its own glyph, the clock, at every level: no fault triangle, no second mark`, ((await toast.getAttribute('data-hurry')) ?? null) === want && (await toast.locator('.body > svg use').getAttribute('href')) === '#g-delay' && (await toast.locator('use[href="#g-fault"]').count()) === 0 && (await toast.locator('svg.hurry').count()) === 0, `${await toast.getAttribute('data-hurry')} ${await toast.locator('.body > svg use').getAttribute('href')}`);
    const stripe = await toast.locator('.stripe').evaluate((el) => getComputedStyle(el).backgroundColor);
    const glyph = await toast.locator('.body > svg').evaluate((el) => getComputedStyle(el).color);
    // The grace's toast is a warning's from the start, so its stripe is amber at every level; it never takes a critical alert's stripe.
    check(`${theme}: its stripe at ${left} s is the warning token, never the danger one`, stripe === c.warning && stripe !== c.critical, `${stripe}`);
    if (want) check(`${theme}: its glyph at ${left} s takes the level’s colour`, glyph === (want === 'amber' ? c.warning : c.critical), `${glyph}`);
    await fits(w.page.locator('[data-panel="desk.clock"]'), `${theme}: the clock at ${left} s`);
    await shotOf(w.page, w.page.locator('[data-panel="desk.clock"]'), `A1-${theme}-grace-${left}s-clock`);
    await shotOf(w.page, w.page.locator('.col.right .slot'), `A2-${theme}-grace-${left}s-toast`);
    if (left === 5) await shot(w.page, `A3-${theme}-desk-grace-5s`);
    await w.ctx.close();
  }

  // The run-up to the next show's post (Steeped, 14:00:00), 45, 25 and 5 s before it.
  for (const left of [45, 25, 5]) {
    const { st } = withHosts();
    st.now += 8 * 60 - left;
    const w = await open(st, { theme });
    const line = clockLine(w.page);
    await line.locator('.left').waitFor();
    const want = levelOf(left);
    const txt = await words(line);
    check(`${theme}: the run-up to the post at ${left} s is ${want ?? 'plain'}, with the words for a screen reader`, ((await line.locator('.left').getAttribute('data-hurry')) ?? null) === want && txt.includes(`−0:${String(left).padStart(2, '0')}`) && (want ? txt.includes(want === 'amber' ? 'Last 30 seconds.' : 'Last 10 seconds.') : !/Last (10|30) seconds/.test(txt)), txt);
    if (want) {
      const c = await colours(w.page, line.locator('.left'));
      check(`${theme}: the run-up figure at ${left} s is the ${want === 'amber' ? 'warning' : 'danger'} token and meets 3:1`, c.own === (want === 'amber' ? c.warning : c.critical) && c.ratio >= 3, JSON.stringify(c));
      // An ordinary post, going to plan: the clock's mark, never a fault's triangle on the clock.
      const m = await markOf(line.locator('.left'));
      check(`${theme}: the run-up’s mark at ${left} s is the clock glyph before the figure, and the clock shows no fault triangle`, markOk(m, c, want) && (await w.page.locator('[data-panel="desk.clock"] use[href="#g-fault"]').count()) === 0, JSON.stringify(m));
    }
    if (left !== 45) await shotOf(w.page, w.page.locator('[data-panel="desk.clock"]'), `A7-${theme}-run-up-${left}s-clock`);
    await w.ctx.close();
  }

  // The late count on the tile, in the next card at Solo, and in the toast: 45, 25 and 5 s.
  for (const left of [45, 25, 5]) {
    const { st } = lateHost();
    st.rules.late_count_s = 60;
    st.now += 60 - left;
    const w = await open(st, { theme });
    const count = tile(w.page, 5).locator('[data-testid="late-count"]');
    await count.waitFor();
    const want = levelOf(left);
    check(`${theme}: the tile’s late count at ${left} s is ${want ?? 'plain'}`, ((await count.getAttribute('data-hurry')) ?? null) === want, await words(count));
    const c = await colours(w.page, count, 'b');
    check(`${theme}: its figure at ${left} s keeps the ink (4.5:1) and is at ${want ? '700' : '600'}, on the design system’s scale`, c.ratio >= 4.5 && c.weight === (want ? 700 : 600), JSON.stringify(c));
    if (want) {
      check(`${theme}: its edge is the ${want === 'amber' ? 'warning' : 'danger'} token, with the words`, (await count.evaluate((el) => getComputedStyle(el).boxShadow)).includes(want === 'amber' ? c.warning : c.critical) && (await words(count)).includes(want === 'amber' ? 'Last 30 seconds.' : 'Last 10 seconds.'), (await count.evaluate((el) => getComputedStyle(el).boxShadow)));
      const m = await markOf(count, 'b');
      check(`${theme}: its mark at ${left} s is the clock glyph in the level’s colour, directly before the figure, and no fault triangle`, markOk(m, c, want), JSON.stringify(m));
    } else check(`${theme}: with more than 30 s the tile’s count has no mark`, (await count.locator('svg').count()) === 0);
    // The late count's toast: its own glyph, the clock, in the level's colour; a stripe that stops at warning.
    const toast = toastOf(w.page);
    await toast.waitFor();
    const stripe = await toast.locator('.stripe').evaluate((el) => getComputedStyle(el).backgroundColor);
    const glyph = await toast.locator('.body > svg').evaluate((el) => getComputedStyle(el).color);
    check(`${theme}: the late count’s toast at ${left} s keeps the clock glyph${want ? `, in the ${want === 'amber' ? 'warning' : 'danger'} token, and its stripe is the warning token` : ''}`, (await toast.locator('.body > svg use').getAttribute('href')) === '#g-delay' && (await toast.locator('use[href="#g-fault"]').count()) === 0 && (!want || (glyph === (want === 'amber' ? c.warning : c.critical) && stripe === c.warning)) && stripe !== c.critical, `${glyph} ${stripe}`);
    check(`${theme}: nothing in the late count at ${left} s is drawn at 800`, (await heaviest(w.page)) <= 700, `${await heaviest(w.page)}`);
    if (left !== 45) await shotOf(w.page, w.page.locator('.col.right .slot'), `A8-${theme}-late-${left}s-toast`);
    await whole(tile(w.page, 5), `${theme}: the tile at ${left} s`);
    if (left !== 45) await shotOf(w.page, tile(w.page, 5), `A4-${theme}-late-${left}s-tile`);
    await w.ctx.close();
  }
  {
    const solo = lateHost({ step: 'solo' });
    solo.st.rules.late_count_s = 60;
    solo.st.now += 55;
    solo.st.join('duncan', 'Duncan Boyd', 'presenter');
    const w = await open(solo.st, { theme });
    await nextCard(w.page).waitFor();
    const when = nextCard(w.page).locator('[data-testid="next-when"]');
    check(`${theme}: the next card’s count at 5 s is red, with the words`, (await when.getAttribute('data-hurry')) === 'red' && (await words(when)).includes('Last 10 seconds.'), await words(when));
    const nc = await colours(w.page, when, 'b');
    const nm = await markOf(when, 'b');
    check(`${theme}: its mark is the clock glyph in the danger token, directly before the figure, and no fault triangle; the figure is at 700`, markOk(nm, nc, 'red') && nc.weight === 700, JSON.stringify({ nm, nc }));
    await fits(nextCard(w.page), `${theme}: the next card at 5 s`);
    await shotOf(w.page, nextCard(w.page), `A5-${theme}-next-card-5s`);
    await w.ctx.close();
  }

  // A kept place running out: 25 and 5 s before the station lets it go.
  for (const left of [25, 5]) {
    const lost = onAirAlone();
    lost.st.now += 7;
    lost.st.lose(lost.finlay.key_id, { ago_s: 300 - left });
    lost.st.cover();
    const w = await open(lost.st, { theme });
    const kept = tile(w.page, 5).locator('[data-testid="source-kept"]');
    await kept.waitFor();
    const want = levelOf(left);
    check(`${theme}: the kept place at ${left} s left is ${want} and counts what is left`, (await kept.getAttribute('data-hurry')) === want && (await words(kept)).includes(`0:${String(left).padStart(2, '0')} left`), await words(kept));
    const kc = await colours(w.page, kept, '.left b');
    const km = await markOf(kept, '.left b');
    check(`${theme}: its mark at ${left} s is the clock glyph in the level’s colour, directly before the figure it marks, and no fault triangle; the figure alone is at 700`, markOk(km, kc, want) && kc.weight === 700 && (await kept.locator('.num').evaluate((el) => Number(getComputedStyle(el).fontWeight))) < 700, JSON.stringify({ km, kc }));
    await whole(tile(w.page, 5), `${theme}: the kept tile at ${left} s left`);
    await shotOf(w.page, tile(w.page, 5), `A6-${theme}-kept-${left}s-tile`);
    await w.ctx.close();
  }
}

// ---- 2q. Leaving the air from the GUI: Leave on the presenter's own tile, and Hand to automation under the lamp, as big as End show ----
{
  const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10 });
  // The window runs as Shona (a producer): her own microphone link's identity is the station's, `presenter-me-` and her account's id.
  st.join(`presenter-me-${st.me().account.id.replaceAll('-', '')}`, 'Shona Campbell', 'presenter');
  st.join('duncan', 'Duncan Boyd', 'presenter', { on_air: false });
  const { ctx, page } = await open(st);
  const mine = tile(page, 1);
  await mine.waitFor();
  check('Leave is the quiet button on the presenter’s own tile, and End pass is gone from it', (await mine.getByRole('button', { name: 'Leave the room' }).count()) === 1 && (await mine.getByText('End pass').count()) === 0, await words(mine));
  check('the tile says You’re in the room, beside the link', (await words(mine.locator('[data-testid="in-room"]'))) === 'You’re in the room · linked', await words(mine.locator('[data-testid="in-room"]')));
  check('another presenter’s tile keeps End pass', (await tile(page, 2).getByRole('button', { name: 'End pass for Duncan Boyd' }).count()) === 1);
  check('Leave says what it does, in its tooltip', ((await mine.getByRole('button', { name: 'Leave the room' }).getAttribute('title')) ?? '').startsWith('Takes you off air and out of the room'));
  await inView(page, mine.getByRole('button', { name: 'Leave the room' }), 'Leave');
  await fits(mine, 'the presenter’s own tile with Leave');
  await shotOf(page, mine, 'B1-tile-leave');
  await mine.getByRole('button', { name: 'Leave the room' }).click();
  check('the first press arms Leave and sends nothing', (await mine.getByRole('button', { name: 'Press again to leave the room' }).count()) === 1 && calls(st, 'POST', /passes\/end/).length === 0);
  await shotOf(page, mine, 'B2-tile-leave-armed');
  await mine.getByRole('button', { name: 'Press again to leave the room' }).click();
  await sleep(1500);
  check('the second press ends your own pass', calls(st, 'POST', /passes\/end/).length === 1 && calls(st, 'POST', /passes\/end/)[0].body.identity === st.me().account.id.replaceAll('-', '') .replace(/^/, 'presenter-me-'));
  check('the tile has gone, and the toast says where you are', (await page.locator('[data-panel="desk.tile.1"]').count()) === 0 && (await words(toastOf(page))).startsWith('You have left the room You are off air and your microphone link is closed. Nobody is on air.') && !(await words(toastOf(page))).includes('Join with my microphone'), await words(toastOf(page)).catch(() => 'no toast'));
  await shot(page, 'B3-desk-left-the-room');
  await ctx.close();
}
{
  const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10 });
  st.join('duncan', 'Duncan Boyd', 'presenter');
  for (const theme of ['dark', 'light', 'night']) {
    const { ctx, page } = await open(st, { theme });
    const head = page.locator('.stage-head');
    const air = head.getByRole('button', { name: 'Hand to automation' });
    const end = head.locator('[data-testid="end-show"]');
    await air.waitFor();
    const a = await air.boundingBox();
    const e = await end.boundingBox();
    const l = await page.locator('[data-panel="desk.lamp"]').boundingBox();
    check(`${theme}: Hand to automation is under the lamp, about as wide as it, and as tall as End show`, !!a && !!e && !!l && Math.abs(a.x - l.x) < 2 && Math.abs(a.width - l.width) < 12 && a.y > l.y && Math.abs(a.height - e.height) < 2, JSON.stringify({ a, e, l }));
    check(`${theme}: it is no longer the 12 px button it was`, (await air.evaluate((el) => parseFloat(getComputedStyle(el).fontSize))) >= 12.9, `${a?.height}`);
    // Its words are the studio's own tooltip (hover and the keyboard's focus), not the browser's: what the press does, then what automation is. shoot-frame.mjs points at it.
    check(`${theme}: it says what it does, and what automation is, in a tooltip it names`, (await air.getAttribute('title')) === null && ((await air.getAttribute('aria-describedby')) ?? '').split(' ').includes('air-tip') && (await page.locator('#air-tip[role="tooltip"]').textContent()).replace(/\s+/g, ' ').trim().startsWith('Automation takes the air now. Whoever is open stays in the room. Automation is the station playing by itself.'), await page.locator('#air-tip').textContent().catch(() => 'no tooltip'));
    // The look's review, question 7: End show is drawn as Hand to automation is, so the proper way to end is not the quieter-looking.
    const look = (loc) => loc.evaluate((el) => { const c = getComputedStyle(el); return { size: c.fontSize, weight: c.fontWeight, ink: c.color, border: `${c.borderTopWidth} ${c.borderTopStyle} ${c.borderTopColor}`, fill: c.backgroundColor, radius: c.borderTopLeftRadius }; });
    const [la, le] = [await look(air), await look(end)];
    const token = (name) => page.evaluate((n) => { const probe = document.createElement('i'); probe.style.color = `var(--${n})`; document.body.append(probe); const c = getComputedStyle(probe).color; probe.remove(); return c; }, name);
    // The outline's width is the window's own 1 px, which a larger text size scales with the rest, so it is compared and not fixed here.
    check(`${theme}: End show has the same outlined button as Hand to automation: 13 px, the ink, a solid outline in the second ink, no fill`, JSON.stringify(la) === JSON.stringify(le) && parseFloat(le.size) >= 12.9 && le.ink === (await token('ink')) && parseFloat(le.border) > 0 && le.border.endsWith(` solid ${await token('ink-2')}`) && le.fill === 'rgba(0, 0, 0, 0)', JSON.stringify({ la, le }));
    check(`${theme}: End show keeps its key cap, and Screener beside it stays a quiet button with none of the outline`, (await words(end)) === 'End show ⌃⇧E' && (await end.locator('kbd').count()) === 1 && (await head.getByRole('button', { name: /^Screener/ }).evaluate((el) => getComputedStyle(el).borderTopColor)) === 'rgba(0, 0, 0, 0)', await words(end));
    await inView(page, air, `${theme}: Hand to automation`);
    await fits(head, `${theme}: the stage’s head`);
    if (theme === 'dark') {
      await air.click();
      await page.locator('dialog[data-panel="desk.end-show"][open]').waitFor();
      await shot(page, 'B4-dark-hand-to-automation-dialog');
      await page.keyboard.press('Escape');
    }
    await shotOf(page, head, `B5-${theme}-stage-head`);
    await ctx.close();
  }
}

// ---- 2r. Join with my microphone…: the browser tab carries the voice ----
{
  const st = new HostStation({ step: 'solo', nowMinutes: 14 * 60 + 10 });
  const { ctx, page } = await open(st);
  const own = page.locator('[data-testid="own-link"]');
  await own.waitFor();
  const said = await words(own);
  check('the empty Solo stage says the browser tab carries the voice and must be kept open', said.includes('Keep that tab open: it carries your voice.'), said);
  await fits(own, 'the join offer');
  await shotOf(page, own, 'C1-join-offer');
  await own.getByRole('button', { name: 'Join with my microphone…' }).click().catch(() => {});
  await sleep(600);
  const dialog = page.locator('dialog[data-panel="desk.invite"]');
  if (await dialog.evaluate((d) => d.open).catch(() => false)) {
    check('the dialog’s lede says it too', (await words(dialog)).includes('Keep that tab open: it carries your voice.'), await words(dialog));
    await shot(page, 'C2-join-dialog');
  } else check('the dialog opens (the stand-in makes the link)', false, 'it did not open');
  await ctx.close();
}

// ---- 2r2. While a host in their own studio has the air, the offer is one quiet line beside Invite someone… (the look's review, finding 2) ----
{
  // Full, in the grace: Finlay on air from his own studio, Heather standing by, nobody in the room.
  const { st } = atTheGrace();
  const { ctx, page } = await open(st);
  const own = page.locator('[data-testid="own-link"]');
  await own.waitFor();
  const row = page.locator('[data-panel="desk.waiting"] .wh');
  check('with a host on air the stage does not headline the offer: no “You are not in the room yet”, and no filled button over the tiles', (await page.locator('.tiles .empty.join').count()) === 0 && !(await words(page.locator('.tiles'))).includes('You are not in the room yet') && (await page.locator('.tiles .btn.primary', { hasText: 'Join with my microphone' }).count()) === 0, await words(page.locator('.tiles')));
  check('the offer is one quiet line in the Not on air row: what joining is for, and that it puts you on air', (await own.evaluate((el) => !!el.closest('[data-panel="desk.waiting"] .wh'))) && (await words(own)) === 'Join to talk to Finlay or to hear a cue. Joining puts you on air.', await words(own));
  const join = row.getByRole('button', { name: 'Join with my microphone…' });
  const invite = row.getByRole('button', { name: 'Invite someone…' });
  const [o, j, i] = [await own.boundingBox(), await join.boundingBox(), await invite.boundingBox()];
  const same = await join.evaluate((el) => {
    const other = [...el.parentElement.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Invite someone…');
    const a = getComputedStyle(el), b = getComputedStyle(other);
    return a.backgroundColor === b.backgroundColor && a.borderTopColor === b.borderTopColor && a.fontSize === b.fontSize && a.color === b.color;
  });
  check('its button is beside Invite someone…, on its line and as quiet as it: the same outline, size and ink, not the filled one', !!j && !!i && Math.abs(j.y - i.y) < 2 && j.x + j.width <= i.x && i.x - (j.x + j.width) <= 0.6 * j.height && Math.abs(j.height - i.height) < 1 && same && !/primary/.test(await join.getAttribute('class')), JSON.stringify({ j, i, same }));
  check('the sentence is whole: nothing of it is cut, and the part that says joining puts you on air is drawn', (await own.evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && getComputedStyle(el).textOverflow !== 'ellipsis' && getComputedStyle(el).whiteSpace !== 'nowrap')), JSON.stringify(o));
  // One line of text: the sentence's own height against its own line height, both as the page lays them out (a larger text size scales both).
  check(`in this window (${SIZE.width} wide, text ${SIZE.text}) the sentence and its buttons are one line`, !!o && !!j && (await own.evaluate((el) => el.offsetHeight < 1.6 * (parseFloat(getComputedStyle(el).lineHeight) || 16))) && o.y + o.height / 2 > j.y && o.y + o.height / 2 < j.y + j.height, JSON.stringify({ o, j }));
  check('the button is named by the sentence beside it', (await join.getAttribute('aria-describedby')) === 'own-beside' && (await own.getAttribute('id')) === 'own-beside');
  check('the row’s own note steps aside for it', (await row.locator('.rule').count()) === 0);
  await inView(page, join, 'the quiet Join with my microphone…');
  await fits(page.locator('[data-panel="desk.waiting"]'), 'the Not on air row with the quiet offer');
  await shot(page, 'C3-join-beside-host');
  await shotOf(page, row, 'C4-join-beside-row');
  await join.click();
  await sleep(600);
  const dialog = page.locator('dialog[data-panel="desk.invite"][open]');
  check('the quiet button makes the same link, and its dialog says again that joining is on air', (await dialog.count()) === 1 && (await words(dialog)).includes('You are on air the moment you join.') && calls(st, 'POST', /invites/).length === 1, (await dialog.count()) ? await words(dialog) : 'no dialog');
  // The dialog is drawn inside the row it is opened from: none of the row's capitals, small type or muted ink reach it.
  const asDialog = (d) => d.locator('.lede').evaluate((el) => { const c = getComputedStyle(el); return { align: c.textAlign, upper: c.textTransform, size: c.fontSize, weight: c.fontWeight, spacing: c.letterSpacing }; });
  const dl = await asDialog(dialog);
  check('the dialog opened from the row reads as the dialog it is: from the start, in sentence case, at its own size', ['start', 'left'].includes(dl.align) && dl.upper === 'none' && parseFloat(dl.size) >= 13.9 && dl.weight === '400' && ['normal', '0px'].includes(dl.spacing), JSON.stringify(dl));
  await inView(page, dialog.getByRole('button', { name: 'Open in browser' }), 'the dialog’s Open in browser, from the row');
  await shot(page, 'C4b-join-beside-dialog');
  await ctx.close();
}
{
  // The host's link is lost: the place is kept, so the offer stays quiet, with the words for it. In the first seconds the desk
  // still holds the air, and whoever joins holds it. Once automation covers, whoever joins is open to the room and not heard.
  const lost = onAirAlone();
  lost.st.lose(lost.finlay.key_id);
  lost.st.now += 2;
  const { ctx, page } = await open(lost.st);
  const own = page.locator('[data-testid="own-link"]');
  await own.waitFor();
  check('while the host’s place is kept the offer stays the quiet line; with the desk holding the air it says joining puts you on air, and what the host then does', (await page.locator('.tiles .empty.join').count()) === 0 && (await own.evaluate((el) => !!el.closest('.wh'))) && (await words(own)) === 'Joining puts you on air. If Finlay reconnects, they then stand by.', await words(own));
  await shotOf(page, page.locator('[data-panel="desk.waiting"] .wh'), 'C5a-join-beside-kept-row-desk-holds');
  lost.st.cover();
  await tick(page, lost.st, 5);
  check('once automation covers it does not say on air: you join open to the room, and are on air when the host is back', (await page.locator('.tiles .empty.join').count()) === 0 && (await words(own)) === 'You join open to the room, and are on air when Finlay is back.' && (await own.evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && getComputedStyle(el).textOverflow !== 'ellipsis')), await words(own));
  check(`under automation too the offer is one line with its buttons in this window (${SIZE.width} wide, text ${SIZE.text})`, await own.evaluate((el) => { const row = el.closest('.wh'); const tops = [...row.querySelectorAll(':scope > button')].map((b) => Math.round(b.getBoundingClientRect().top)); return el.offsetHeight < 1.6 * (parseFloat(getComputedStyle(el).lineHeight) || 16) && new Set(tops).size === 1; }));
  await fits(page.locator('[data-panel="desk.waiting"]'), 'the Not on air row with the quiet offer under automation');
  await shotOf(page, page.locator('[data-panel="desk.waiting"] .wh'), 'C5-join-beside-kept-row');
  await shot(page, 'C5b-join-beside-kept');
  await ctx.close();
}
{
  // Solo has no Not on air row: the quiet line takes the guest's seat.
  const { st } = onAirAlone({ step: 'solo' });
  const { ctx, page } = await open(st);
  const own = page.locator('[data-testid="own-link"]');
  await own.waitFor();
  const join = own.getByRole('button', { name: 'Join with my microphone…' });
  check('at Solo, with a host on air, the offer is the guest’s seat: the sentence and an outlined button, no headline', (await page.locator('[data-panel="desk.waiting"]').count()) === 0 && (await page.locator('.tiles > *').evaluateAll((els) => els.map((e) => e.dataset.panel ?? e.dataset.testid).join(' '))) === 'desk.tile.5 own-link' && (await words(own)) === 'Join to talk to Finlay or to hear a cue. Joining puts you on air. Join with my microphone…' && !(await words(page.locator('.tiles'))).includes('You are not in the room yet') && !/primary/.test(await join.getAttribute('class')), `${await page.locator('.tiles > *').evaluateAll((els) => els.map((e) => e.dataset.panel ?? e.dataset.testid).join(' '))} | ${await words(own)}`);
  check('its button is outlined, not filled with the accent', await join.evaluate((el) => { const c = getComputedStyle(el); const probe = document.createElement('i'); probe.style.color = 'var(--accent)'; document.body.append(probe); const accent = getComputedStyle(probe).color; probe.remove(); return c.backgroundColor !== accent && c.borderTopStyle === 'solid' && c.borderTopColor !== 'rgba(0, 0, 0, 0)'; }));
  await inView(page, join, 'the seat’s Join with my microphone…');
  await fits(own, 'the quiet offer in the guest’s seat');
  await shot(page, 'C6-join-beside-host-solo');
  // The seat centres its words; the dialog its button opens does not take that from it.
  await join.click();
  await sleep(600);
  const dialog = page.locator('dialog[data-panel="desk.invite"][open]');
  const dl = (await dialog.count()) ? await dialog.locator('.lede').evaluate((el) => { const c = getComputedStyle(el); return { align: c.textAlign, upper: c.textTransform, size: c.fontSize }; }) : null;
  const box = (await dialog.count()) ? await dialog.evaluate((el) => { const input = el.querySelector('#inv-link').getBoundingClientRect(), open = el.querySelector('#inv-open').getBoundingClientRect(); return { centres: Math.abs(input.top + input.height / 2 - (open.top + open.height / 2)) }; }) : null;
  check('the seat’s button makes the link, and its dialog reads from the start, with Open in browser level with the link', !!dl && ['start', 'left'].includes(dl.align) && dl.upper === 'none' && calls(st, 'POST', /invites/).length === 1 && !!box && box.centres < 3, JSON.stringify({ dl, box }));
  await shot(page, 'C6b-join-beside-host-solo-dialog');
  await ctx.close();
}
{
  // Unchanged when no source holds the air: automation has it and a host only stands by.
  const { st } = lateHost();
  const { ctx, page } = await open(st);
  const own = page.locator('[data-testid="own-link"]');
  await own.waitFor();
  check('with no host on air the stage’s offer is as it was: the headline, its filled button, and the row’s own note', (await own.evaluate((el) => el.classList.contains('join') && !!el.closest('.tiles'))) && (await words(own)).startsWith('You are not in the room yet') && /primary/.test(await own.getByRole('button', { name: 'Join with my microphone…' }).getAttribute('class')) && (await page.locator('[data-panel="desk.waiting"] .wh .rule').count()) === 1 && (await page.locator('[data-panel="desk.waiting"] .wh').getByRole('button', { name: 'Join with my microphone…' }).count()) === 0, await words(own));
  await ctx.close();
}

// ---- 2s. The Listeners panel and the desk's delay (issue 17) ----
{
  const states = [
    ['counted', {}, {}],
    ['not-counted-error', { audience: { listeners: null, by_mount: [], error: 'the listener process did not answer', edges: false, scope: 'origin' } }, {}],
    ['not-counted-waiting', { audience: { listeners: null, by_mount: [], error: null, edges: false, scope: 'origin' } }, {}],
    ['edges', { audience: { listeners: 412, by_mount: [{ mount: '/programme.opus', listeners: 280 }, { mount: '/programme.mp3', listeners: 132 }], error: null, edges: true, scope: 'origin' } }, {}],
    ['measured', { listenerDelay: { ms: 21_200, measured: true, basis: 'measured', desk_ms: 8000, process_ms: 500, rest_ms: 0, read_ms: 21_200, set_ms: 0 } }, {}],
    ['desk-only', { listenerDelay: { ms: 21_000, measured: false, basis: 'estimate', desk_ms: 8000, process_ms: null, rest_ms: 13_000, read_ms: 8000, set_ms: 13_000 } }, {}],
  ];
  const want = {
    counted: ['138 listeners now', 'programme.opus 138', 'about 23 s behind 8.0 s profanity delay, 0.5 s buffer, about 14.5 s in listeners’ players (estimated)'],
    'not-counted-error': ['Not counted: the listener process did not answer.'],
    'not-counted-waiting': ['Waiting for the first answer from the listener process.'],
    edges: ['412 listeners now', 'programme.opus 280 · programme.mp3 132', 'This counts this station’s own stream only'],
    measured: ['· 21 s behind 8.0 s profanity delay, 0.5 s buffer, 12.7 s in listeners’ players'],
    'desk-only': ['about 21 s behind 8.0 s profanity delay, about 13 s in listeners’ players (estimated)'],
  };
  for (const [name, over] of states) {
    const st = new HostStation({ step: 'full', nowMinutes: 14 * 60 + 10 });
    st.join('duncan', 'Duncan Boyd', 'presenter');
    Object.assign(st, over);
    const { ctx, page } = await open(st);
    const aud = page.locator('[data-panel="desk.audience"]');
    const clock = page.locator('[data-panel="desk.clock"] .listen');
    const said = `${await words(aud)} | ${await words(clock)}`;
    check(`the Listeners panel and the clock, ${name}: ${want[name].join(' · ')}`, want[name].every((x) => said.includes(x)), said);
    if (name === 'measured') check('measured: the delay carries no word of estimate on the clock', !/estimate|about/.test(await words(clock)), await words(clock));
    // The look's review, finding 3: each panel states the listener delay once, in a presenter's words.
    const clockSays = await words(clock);
    const head = page.locator('.stage-head');
    const headSays = await words(head);
    check(`${name}: the clock states the delay once, with its parts in one place: the line under it, and no tooltip`, (clockSays.match(/behind/g) ?? []).length === 1 && (clockSays.match(/profanity delay/g) ?? []).length === 1 && (await clock.locator('[data-testid="delay-detail"]').count()) === 1 && (await page.locator('[data-panel="desk.clock"] [title*="profanity"]').count()) === 0, clockSays);
    check(`${name}: no line of the clock says read or set`, !/\b(read|set)\b/i.test(clockSays), clockSays);
    check(`${name}: the stage’s head states it once too, and DELAY says what it holds, not where listeners are`, (headSays.match(/behind/g) ?? []).length === 1 && (await words(page.locator('[data-panel="desk.delay"] .lbl .small'))) === 'full · 8.0 s held', `${headSays} | ${await words(page.locator('[data-panel="desk.delay"] .lbl'))}`);
    await fits(aud, `the Listeners panel, ${name}`);
    await fits(page.locator('[data-panel="desk.clock"]'), `the clock, ${name}`);
    await shotOf(page, aud, `D1-${name}-listeners`);
    await shotOf(page, page.locator('[data-panel="desk.clock"]'), `D2-${name}-clock`);
    if (name === 'counted') await shotOf(page, head, 'D3-counted-stage-head');
    await ctx.close();
  }
}

// ---- 3. Settings › Own studios: empty, the setup, the once-only details, the row ----
{
  const st = new HostStation();
  const { ctx, page } = await open(st, { settings: true });
  const p = panel(page);
  await p.waitFor();
  await p.scrollIntoViewIfNeeded();
  check('with no key, the empty state says what a key is for', (await p.locator('[data-testid="hosts-empty"]').innerText()).startsWith('No host has a key yet.'));
  check('a producer is not shown the rules, and the studio does not ask for them', (await page.locator('[data-panel="settings.hosts.rules"]').count()) === 0 && calls(st, 'GET', /settings\/sources/).length === 0);
  await shotOf(page, p, '10-settings-empty');

  await p.getByRole('button', { name: 'Set up a host…' }).click();
  const setup = dlg(page, 'setup');
  await setup.waitFor();
  check('the setup opens on Show', await page.evaluate(() => document.activeElement?.id === 'hk-show'));
  await setup.getByRole('button', { name: 'Make the key' }).click();
  check('nothing is asked of the station until the form is whole', calls(st, 'POST', /\/keys$/).length === 0 && (await setup.locator('#hk-show-err').count()) === 1);
  await page.selectOption('#hk-show', 'steeped');
  await setup.getByRole('button', { name: 'Make the key' }).click();
  check('a key with no name is caught under its field', (await setup.locator('#hk-label-err').innerText()).startsWith('Say who the key is for') && (await page.evaluate(() => document.activeElement?.id)) === 'hk-label');
  await page.fill('#hk-label', 'Finlay Grant');
  await setup.getByText('SRT.', { exact: true }).click();
  await inView(page, setup.getByRole('button', { name: 'Make the key' }), 'Make the key');
  await shot(page, '11-setup');
  await page.locator('#hk-label').press('Enter');
  const det = dlg(page, 'details');
  await det.waitFor();
  const made = calls(st, 'POST', /\/shows\/steeped\/keys$/);
  check('⏎ makes the key, with exactly the fields the station takes', made.length === 1 && JSON.stringify(made[0].body) === JSON.stringify({ label: 'Finlay Grant', hint: 'srt' }), JSON.stringify(made[0]?.body));
  const key = st.keys[0];
  check('the details are titled for the host and the show', (await det.locator('h2').innerText()) === 'Finlay’s details for Steeped');
  check('first focus is on Copy everything, not on the way out', await page.evaluate(() => document.activeElement?.id === 'hk-all'));
  check('the way they said they would send comes first', (await det.locator('.waybox h3').first().innerText()).startsWith('SRT'), await det.locator('.waybox h3').first().innerText());
  check('every field a host’s software asks for is there, with the secret', (await det.locator('#hd-srt-3').inputValue()) === key.secret && (await det.locator('#hd-icecast-4').inputValue()) === `${key.key_id}-${key.secret}` && (await det.locator('#hd-srt-0').inputValue()) === 'kettle.example' && (await det.locator('#hd-srt-1').inputValue()) === '9710' && (await det.locator('#hd-icecast-1').inputValue()) === '8005');
  check('each field that can be typed has Copy', (await det.locator('.f input').count()) === (await det.locator('.f .btn').count()), `${await det.locator('.f input').count()} fields, ${await det.locator('.f .btn').count()} Copy buttons`);
  // The monitor link, whole: the station's public address, then /monitor/# and the link's secret.
  const link = `https://kettle.example/monitor/#${key.monitor}`;
  const monBox = det.locator('[data-testid="monitor-link"]');
  check('the details give the host’s whole monitor link, with Copy', (await monBox.locator('h3').innerText()).trim() === 'Monitor link' && (await det.locator('#hd-mon-0').inputValue()) === link && (await monBox.getByRole('button', { name: 'Copy the monitor link' }).count()) === 1, await det.locator('#hd-mon-0').inputValue().catch(() => 'no field'));
  check('the public address was asked of the station, where this window had not read a week', calls(st, 'GET', /\/api\/v1\/schedule\/episodes/).length >= 1);
  check('nothing in the details says the monitor link is not built', !/not built|later in 1\.1|for later/i.test(await det.innerText()));
  check('the key is someone else’s: no Open my monitor here', (await det.getByRole('button', { name: 'Open my monitor' }).count()) === 0);
  // A second ⏎ lands on Copy everything; Esc, twice, does not close what cannot be shown again.
  await page.keyboard.press('Enter');
  await sleep(200);
  await page.keyboard.press('Escape');
  await sleep(150);
  await page.keyboard.press('Escape');
  await sleep(300);
  check('⏎ again and Esc twice leave the details open', (await det.evaluate((d) => d.open)) && (await det.locator('#hd-srt-3').inputValue()) === key.secret);
  const all = await clip(page);
  check('Copy everything gives one block of text: the way they will use first, both ways, the secret, the whole monitor link', all.startsWith('Finlay Grant’s details for Steeped\n\nSRT (') && all.indexOf('SRT (') < all.indexOf('Icecast (') && all.includes(`Passphrase: ${key.secret}`) && all.includes(`Password: ${key.key_id}-${key.secret}`) && all.includes(`Monitor link (open it in a browser, and keep it open beside your software)\n  ${link}\n`) && all.includes('Server: kettle.example'), all);
  check('the text sends the host to the monitor link, and says they go on air at their show’s time', all.includes('open the monitor link, and press connect') && all.includes('You go on air at your show’s time, not when you connect.') && !/not built/i.test(all));
  await monBox.getByRole('button', { name: 'Copy the monitor link' }).click();
  check('Copy on the monitor link copies the whole link', (await clip(page)) === link);
  await det.getByRole('button', { name: 'Copy the SRT passphrase' }).click();
  check('Copy on a field copies that field', (await clip(page)) === key.secret && (await det.getByRole('button', { name: 'Copy the SRT passphrase' }).innerText()) === 'Copied');
  await det.getByRole('button', { name: 'Copy the Icecast password' }).click();
  check('Copy on another field moves the tick', (await clip(page)) === `${key.key_id}-${key.secret}` && (await det.getByRole('button', { name: 'Copy the SRT passphrase' }).innerText()) === 'Copy');
  check('I have sent them is the only way out', (await det.locator('.acts button').count()) === 2 && (await det.getByRole('button', { name: /Close|Cancel/ }).count()) === 0);
  await inView(page, det.getByRole('button', { name: 'I have sent them' }), 'I have sent them');
  await inView(page, det.locator('h2'), 'the details’ heading');
  await fits(det, 'the details');
  // The global chords do not reach the desk from under the dialog (it would unmount the secret).
  await page.keyboard.press('Control+1');
  await sleep(200);
  check('⌃1 does not leave Settings from under the details', (await det.evaluate((d) => d.open)) && (await page.locator('#settings-h').count()) === 1);
  await shot(page, '12-details-once');
  // A webview that refuses the clipboard: the value is selected in its box, and the dialog says so.
  await page.evaluate(() => (navigator.clipboard.writeText = () => Promise.reject(new Error('refused'))));
  await det.getByRole('button', { name: 'Copy the SRT stream id' }).click();
  check('where the clipboard is refused, the field is selected and the dialog says to copy it from there', (await det.locator('.copy-note').innerText()).startsWith('Could not copy from here.') && (await page.evaluate(() => document.activeElement?.id === 'hd-srt-2' && document.activeElement.selectionEnd === document.activeElement.value.length)));
  await det.getByRole('button', { name: 'Copy everything as text' }).click();
  check('and Copy everything draws the text to copy by hand', (await det.locator('#hk-all-text').inputValue()).includes(`Passphrase: ${key.secret}`) && (await page.evaluate(() => document.activeElement?.id === 'hk-all-text')));
  await inView(page, det.getByRole('button', { name: 'I have sent them' }), 'I have sent them, with the text drawn');
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(600);
  check('the details close, and the secret is nowhere on the page', !(await det.evaluate((d) => d.open)) && !(await page.content()).includes(key.secret) && !(await page.content()).includes(key.monitor));
  check('no panel name plate is left showing after the dialogs close', (await page.locator('.pn-tag.show').count()) === 0, await page.locator('.pn-tag.show').allInnerTexts().then((t) => t.join(' | ')));
  check('the secret was never put in storage', await page.evaluate((s) => !JSON.stringify({ ...localStorage, ...sessionStorage }).includes(s), key.secret));
  const row = p.locator(`tr[data-key="${key.key_id}"]`);
  check('the key has its row: the show, the host, the way in', (await row.innerText()).includes('Steeped') && (await row.innerText()).includes('Finlay Grant') && (await row.locator('td').nth(1).innerText()).trim() === 'SRT', await row.innerText().catch(() => 'no row'));
  check('its slot is open and nobody has connected: NOT CONNECTED', /not connected/i.test(await row.locator('.state').innerText()) && (await row.innerText()).includes('never connected yet'), await row.innerText().catch(() => ''));
  check('the panel says the key is ready and that its secret is gone', (await p.locator('[role=status]').first().innerText()).includes('Finlay Grant’s key for Steeped is ready'));
  await shotOf(page, p, '13-settings-one-key');

  // Details… on the row: everything but the secret, no Copy on what cannot be copied.
  await row.getByRole('button', { name: /Details…/ }).click();
  await det.waitFor();
  check('Details… has everything but the secret', !(await det.innerText()).includes(key.secret) && (await det.innerText()).includes('shown once, when the key was made') && (await det.locator('#hd-srt-2').inputValue()) === key.key_id);
  check('and no monitor link: it was shown once too, and nobody is connected to say whether it is open', !(await det.innerHTML()).includes(key.monitor) && (await det.locator('[data-testid="monitor-link"], [data-testid="monitor-state"]').count()) === 0);
  check('no Copy is offered on a field that holds no secret to copy', (await det.getByRole('button', { name: /Copy the (SRT passphrase|Icecast password)/ }).count()) === 0 && (await det.getByRole('button', { name: 'Copy everything as text' }).count()) === 0);
  await shot(page, '14-details-later');
  await page.keyboard.press('Escape');
  await sleep(200);
  check('these details close on Esc: nothing in them is lost', !(await det.evaluate((d) => d.open)));

  // A second key for the same show says what two hosts cannot do.
  await p.getByRole('button', { name: 'Set up a host…' }).click();
  check('a show that has a host says so in the list', (await page.locator('#hk-show option[value="steeped"]').innerText()) === 'Steeped · Finlay Grant has a key');
  await page.selectOption('#hk-show', 'steeped');
  check('a second key for a show says each is one person’s', (await setup.locator('[data-testid="second-key"]').innerText()).includes('Each key is one person’s'));
  await setup.getByRole('button', { name: 'Cancel' }).click();
  await ctx.close();
}

// ---- 4. The rows as they stand, New key…, End key…, Let back in ----
{
  const { st, finlay, elspeth, heather, catriona } = withHosts();
  // Finlay is on air, with his monitor link open; Duncan has stepped back.
  st.people.find((x) => x.role === 'source').on_air = true;
  st.listen(finlay.key_id);
  const { ctx, page } = await open(st, { settings: true });
  const p = panel(page);
  await p.locator('table').waitFor();
  await p.scrollIntoViewIfNeeded();
  const row = (k) => p.locator(`tr[data-key="${k.key_id}"]`);
  check('the heading counts what is connected', (await p.locator('.ph .small').innerText()) === '4 keys work · 1 on air · 1 testing', await p.locator('.ph .small').innerText());
  check('a source on air says ON AIR, since when it connected, that its level is in range, and that its monitor is open', /on air/i.test(await row(finlay).locator('.state').innerText()) && (await row(finlay).innerText()).includes('on air · connected since 13:47 · level in range · monitor open') && (await row(finlay).innerText()).includes('SRT · mpegts/aac 48 kHz stereo · strip 5'), await row(finlay).innerText());
  check('a test with no monitor open does not say so', !(await row(catriona).innerText()).includes('monitor open'));
  check('a key whose slot is open says NOT CONNECTED', /not connected/i.test(await row(elspeth).locator('.state').innerText()) && (await row(elspeth).innerText()).includes('its slot is open until 18:00'), await row(elspeth).innerText());
  check('a test outside the slot says TEST, with the station’s own reading and what is playing', /test/i.test(await row(catriona).locator('.state').innerText()) && (await row(catriona).innerText()).includes('a private test since 13:50 · level −18.3 dBFS') && (await row(catriona).innerText()).includes('playing: Runrig - Loch Lomond'), await row(catriona).innerText());
  check('a key that is not due has no chip, and says when it last connected', (await row(heather).locator('.state').count()) === 0 && (await row(heather).innerText()).includes('last connected yesterday 15:40'), await row(heather).innerText());
  check('the last refusal is on the key’s row, in words', (await row(heather).innerText()).includes('refused today 13:41: the key is wrong'), await row(heather).innerText());
  check('the live rows are in order: on air, a test, due, the rest', JSON.stringify(await p.locator('tbody tr[data-key]').evaluateAll((rs) => rs.map((r) => r.dataset.key))) === JSON.stringify([finlay, catriona, elspeth, heather].map((k) => k.key_id)));
  check('ended keys are kept apart, folded', (await p.locator('details.ended summary').innerText()).replace(/\s+/g, ' ').trim() === 'Ended keys 1' && !(await p.locator('details.ended').evaluate((d) => d.open)));
  await fits(p, 'the rows');
  await shotOf(page, p, '20-settings-rows');
  await shot(page, '21-settings-rows-window');

  // Details… on a connected key says how its monitor link stands, and never gives the link again.
  await row(finlay).getByRole('button', { name: /Details…/ }).click();
  await dlg(page, 'details').waitFor();
  const state = dlg(page, 'details').locator('[data-testid="monitor-state"]');
  check('Details… says the monitor link is open now, and that the link was shown once', (await words(state.locator('h3'))) === 'Monitor link open now' && (await words(state)).includes('Finlay has the monitor link open') && (await words(state)).includes('The link was shown once, with the secret.') && !(await dlg(page, 'details').innerHTML()).includes(finlay.monitor), await words(state).catch(() => 'no section'));
  await fits(dlg(page, 'details'), 'the details with the monitor’s state');
  await shot(page, '21b-details-monitor-open');
  await page.keyboard.press('Escape');
  await sleep(200);
  await row(catriona).getByRole('button', { name: /Details…/ }).click();
  await dlg(page, 'details').waitFor();
  check('for a host connected without it, Details… says the monitor link is not open', (await words(dlg(page, 'details').locator('[data-testid="monitor-state"] h3'))) === 'Monitor link not open', await words(dlg(page, 'details').locator('[data-testid="monitor-state"]')).catch(() => 'no section'));
  await page.keyboard.press('Escape');
  await sleep(200);

  // New key… while connected: asked when the old one ends; *when they disconnect* is picked.
  await row(finlay).getByRole('button', { name: /New key…/ }).click();
  const nk = dlg(page, 'new-key');
  await nk.waitFor();
  check('New key… asks, and names the host', (await nk.locator('h2').innerText()) === 'Make a new key for Finlay?');
  check('connected, it asks when the old key ends, with When he disconnects picked', (await nk.locator('input[name="hn-when"]').count()) === 2 && (await nk.locator('input[name="hn-when"]').first().isChecked()));
  await nk.getByText('Now.', { exact: true }).click();
  check('Now says what listeners hear, because he is on air', (await nk.innerText()).includes('Finlay is on air: listeners hear whoever else is on air, or silence until automation covers.'));
  await shot(page, '22-new-key');
  await nk.getByText('When Finlay disconnects.', { exact: true }).click();
  await nk.getByRole('button', { name: 'Make a new key' }).click();
  const det = dlg(page, 'details');
  await det.waitFor();
  const renew = calls(st, 'POST', new RegExp(`/shows/steeped/keys/${finlay.key_id}/renew$`));
  check('the station is asked to end the old key on disconnect, in its own spelling', renew.length === 1 && JSON.stringify(renew[0].body) === JSON.stringify({ old: 'on_disconnect' }), JSON.stringify(renew[0]?.body));
  check('the new key’s details are shown once, as a first key’s are', (await det.locator('#hd-srt-3').inputValue()) === st.keys[0].secret && st.keys[0].key_id !== finlay.key_id);
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(600);
  check('the old key stays until its connection closes, and says so', (await row(finlay).innerText()).includes('ends when this connection closes') && (await row(finlay).getByRole('button', { name: /New key…/ }).isDisabled()));

  // New key… with nothing connected: no question of when.
  await row(heather).getByRole('button', { name: /New key…/ }).click();
  await nk.waitFor();
  check('not connected, New key… asks nothing about when', (await nk.locator('input[name="hn-when"]').count()) === 0);
  await nk.getByRole('button', { name: 'Make a new key' }).click();
  await det.waitFor();
  check('and the old key ends now', JSON.stringify(calls(st, 'POST', new RegExp(`/kettle-drive/keys/${heather.key_id}/renew$`))[0]?.body) === JSON.stringify({ old: 'now' }));
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(600);
  check('the old key joins the ended ones', (await row(heather).count()) === 0 && (await p.locator('details.ended summary').innerText()).includes('2'));

  // End key… on air: says so first, and the button says what it does.
  await row(finlay).getByRole('button', { name: /End key…/ }).click();
  const end = dlg(page, 'end');
  await end.waitFor();
  check('End key… on air says so first', (await end.locator('h2').innerText()) === 'End Finlay’s key for Steeped?' && (await end.locator('[data-testid="end-on-air"]').innerText()).startsWith('Finlay is on air with this key.'));
  check('and its button says what it does', (await end.getByRole('button', { name: 'End the key and take Finlay off air' }).count()) === 1);
  await shot(page, '23-end-key-on-air');
  await page.keyboard.press('Escape');
  await sleep(200);
  check('Esc keeps the key', calls(st, 'DELETE', /\/keys\//).length === 0 && !(await end.evaluate((d) => d.open)));
  await row(finlay).getByRole('button', { name: /End key…/ }).click();
  await end.getByRole('button', { name: 'End the key and take Finlay off air' }).click();
  await sleep(700);
  check('the key is ended by its own route', calls(st, 'DELETE', new RegExp(`/shows/steeped/keys/${finlay.key_id}$`)).length === 1 && (await row(finlay).count()) === 0);
  check('the panel says what ended', (await p.locator(':scope > [role=status]').innerText()).includes('Finlay Grant’s key for Steeped has ended'), await p.locator(':scope > [role=status]').innerText());

  // A key the desk turned away: who, until when, and Let back in.
  const k = st.keys.find((x) => x.key_id === elspeth.key_id);
  k.turned_away_until_ms = st.now * 1000 + 4 * 3600_000 + 8 * 60_000;
  k.turned_away_by = 'Shona Campbell';
  // The row follows at the next read of the keys (once a minute): the window is opened again.
  await page.reload();
  await page.locator('[data-panel="studio.frame"]').waitFor();
  await page.locator('button.settings-tab').click();
  await p.locator('table').waitFor();
  check('a key turned away says by whom and until when', (await row(elspeth).innerText()).includes('turned away by Shona Campbell until 18:00'), await row(elspeth).innerText());
  await p.scrollIntoViewIfNeeded();
  await shotOf(page, p, '24-settings-turned-away');
  await row(elspeth).getByRole('button', { name: /Let back in/ }).click();
  await sleep(700);
  check('Let back in asks the station, and the row is clear again', calls(st, 'DELETE', new RegExp(`/sources/${elspeth.key_id}/disconnect$`)).length === 1 && !(await row(elspeth).innerText()).includes('turned away'));
  await ctx.close();
}

// ---- 5. The rules, for an admin; what a producer and a screener see ----
{
  const { st } = withHosts({ role: 'admin' });
  const { ctx, page } = await open(st, { settings: true });
  const rules = page.locator('[data-panel="settings.hosts.rules"]');
  await rules.waitFor();
  await rules.scrollIntoViewIfNeeded();
  check('an admin has the rules, read from the station', (await rules.locator('#hr-door_before_min').inputValue()) === '30' && (await rules.locator('#hr-grace_s').inputValue()) === '120' && (await rules.locator('#hr-port_icecast').innerText()) === '8005' && (await rules.locator('#hr-port_srt').innerText()) === '9710');
  check('the two ports are the installation’s: shown, not edited', (await rules.locator('input#hr-port_icecast, input#hr-port_srt').count()) === 0);
  check('the grace and the late count are no longer marked as not acted on (the post\'s rule acts on them)', (await rules.locator('.tag.notyet').count()) === 0);
  check('Save is off until something changes', await rules.getByRole('button', { name: 'Save the rules' }).isDisabled());
  await fits(rules, 'the rules');
  await shotOf(page, rules, '30-rules');
  await rules.locator('#hr-door_before_min').fill('59');
  await rules.getByRole('button', { name: 'Save the rules' }).click();
  await sleep(400);
  check('the station’s own refusal lands under the field it names', (await rules.innerText()).includes('the station refuses this (a test)') && st.rules.door_before_min === 30);
  await rules.locator('#hr-grace_s').selectOption('300');
  await rules.locator('#hr-door_before_min').fill('45');
  await rules.getByText('Outside the slot, take a connection as a private test').click();
  await rules.getByRole('button', { name: 'Save the rules' }).click();
  await sleep(400);
  const put = calls(st, 'PUT', /settings\/sources/).at(-1);
  check('saving sends every rule at once, the ones this panel does not draw as they were', JSON.stringify(put?.body) === JSON.stringify({ door_before_min: 45, grace_s: 300, late_count_s: 20, test_outside: false, test_max_min: 15, steady_s: 5, silence_s: 4, place_kept_s: 300, fade_ms: 2000, port_icecast: 8005, port_srt: 9710 }), JSON.stringify(put?.body));
  check('and says it is saved', (await rules.locator('.foot [role=status]').innerText()) === 'The rules are saved.');
  check('an admin’s setup offers the members of staff', await (async () => {
    await panel(page).getByRole('button', { name: 'Set up a host…' }).click();
    const has = (await page.locator('#hk-staff option').count()) === 5;
    await page.selectOption('#hk-staff', 'a-finlay');
    const filled = (await page.inputValue('#hk-label')) === 'Finlay Grant';
    await page.selectOption('#hk-show', 'the-long-lunch');
    await dlg(page, 'setup').getByRole('button', { name: 'Make the key' }).click();
    await dlg(page, 'details').waitFor();
    const body = calls(st, 'POST', /the-long-lunch\/keys$/)[0]?.body;
    await dlg(page, 'details').getByRole('button', { name: 'I have sent them' }).click();
    return has && filled && JSON.stringify(body) === JSON.stringify({ label: 'Finlay Grant', hint: 'icecast', staff_id: 'a-finlay' });
  })());
  await ctx.close();

  const screener = await open(new HostStation({ role: 'screener' }), { settings: true });
  check('a screener is not shown Own studios', (await panel(screener.page).count()) === 0);
  await screener.ctx.close();
}

// ---- 6. What the station cannot do: not listening, no database, older than 1.1, no address ----
{
  const { st } = withHosts();
  st.listening = false;
  let w = await open(st, { settings: true });
  await panel(w.page).locator('table').waitFor();
  await panel(w.page).scrollIntoViewIfNeeded();
  check('a station that is not listening says so, above the rows', (await panel(w.page).locator('[data-testid="hosts-not-listening"]').innerText()).startsWith('The station is not listening for hosts.'));
  await shotOf(w.page, panel(w.page), '40-not-listening');
  await w.ctx.close();

  const none = new HostStation();
  none.database = false;
  w = await open(none, { settings: true });
  await panel(w.page).locator('[data-testid="hosts-unavailable"]').waitFor();
  check('a station that cannot keep keys says why, in its own words, and offers no setup', (await panel(w.page).locator('[data-testid="hosts-unavailable"]').innerText()).includes('show keys are kept in the database') && (await panel(w.page).getByRole('button', { name: 'Set up a host…' }).isDisabled()));
  await shotOf(w.page, panel(w.page), '41-cannot-keep-keys');
  await w.ctx.close();

  const old = new HostStation({ routes: false });
  w = await open(old, { settings: true });
  await sleep(600);
  check('a station older than 1.1 draws no Own studios, and asks once', (await panel(w.page).count()) === 0 && calls(old, 'GET', /\/api\/v1\/sources$/).length === 1 && calls(old, 'GET', /\/keys$/).length === 0);
  await w.ctx.close();

  // A station whose settings give no address: the studio fills in the one it uses.
  const bare = new HostStation({ host: null });
  w = await open(bare, { settings: true });
  await panel(w.page).getByRole('button', { name: 'Set up a host…' }).click();
  await w.page.selectOption('#hk-show', 'kettle-drive');
  await w.page.fill('#hk-label', 'Heather Douglas');
  await dlg(w.page, 'setup').getByRole('button', { name: 'Make the key' }).click();
  await dlg(w.page, 'details').waitFor();
  check('with no address from the station, the details give the one this studio uses', (await dlg(w.page, 'details').locator('#hd-icecast-0').inputValue()) === '127.0.0.1' && (await dlg(w.page, 'details').locator('.waybox h3').first().innerText()).startsWith('Icecast'));
  await dlg(w.page, 'details').getByRole('button', { name: 'Copy everything as text' }).click();
  check('and so does the text', (await clip(w.page)).includes('Server: 127.0.0.1') && !(await clip(w.page)).includes("(the station's address)"));
  await w.ctx.close();

  // A station with no public address: no link is made from the address this window uses (on
  // Cloudron that one is behind a login). The secret is given, and how the link is formed.
  const priv = new HostStation({ publicUrl: null });
  w = await open(priv, { settings: true });
  await panel(w.page).getByRole('button', { name: 'Set up a host…' }).click();
  await w.page.selectOption('#hk-show', 'kettle-drive');
  await w.page.fill('#hk-label', 'Heather Douglas');
  await dlg(w.page, 'setup').getByRole('button', { name: 'Make the key' }).click();
  await dlg(w.page, 'details').waitFor();
  const pk = priv.keys[0];
  const byHand = dlg(w.page, 'details').locator('[data-testid="monitor-by-hand"]');
  check('with no public address the details give the monitor link’s secret and how the link is formed, never a link', (await dlg(w.page, 'details').locator('#hd-mon-0').inputValue()) === pk.monitor && (await words(byHand)).startsWith('The monitor link is the station’s public address, then /monitor/# and this secret.') && !(await dlg(w.page, 'details').innerHTML()).includes('/monitor/#' + pk.monitor), await words(byHand).catch(() => 'no note'));
  await dlg(w.page, 'details').getByRole('button', { name: 'Copy everything as text' }).click();
  check('and so does the text', (await clip(w.page)).includes(`Monitor link\n  The monitor link is the station’s public address, then /monitor/# and this secret.\n  Secret: ${pk.monitor}\n`) && !(await clip(w.page)).includes('127.0.0.1/monitor'), await clip(w.page));
  await fits(dlg(w.page, 'details'), 'the details with no public address');
  await shot(w.page, '42-details-no-public-address');
  await w.ctx.close();
}

// ---- 7. The show editor's Comes from ----
{
  const { st, finlay } = withHosts();
  const { ctx, page } = await open(st, { schedule: true });
  const openShow = async (name) => {
    if (await page.getByRole('button', { name: '‹ Shows' }).count()) await page.getByRole('button', { name: '‹ Shows' }).click();
    await page.locator('[data-panel="calendar.shows"] .showrow', { hasText: name }).first().click();
    await page.locator('aside.editor').getByRole('tab', { name: 'Details' }).click();
    await page.locator('[data-panel="calendar.show-source"]').waitFor();
  };
  const src = page.locator('[data-panel="calendar.show-source"]');
  await openShow('Steeped');
  await src.scrollIntoViewIfNeeded();
  check('a show with a key reads The host’s own studio', (await src.getByRole('button', { name: 'The host’s own studio' }).getAttribute('aria-pressed')) === 'true');
  check('under it: each host, the way in, and how it stands', (await src.locator(`li[data-key="${finlay.key_id}"]`).innerText()).includes('Finlay Grant') && (await src.locator(`li[data-key="${finlay.key_id}"]`).innerText()).includes('SRT · standing by since 13:47'), await src.innerText());
  check('with Details…, New key… and End key… on each', (await src.locator(`li[data-key="${finlay.key_id}"] button`).count()) === 3);
  await fits(src, 'Comes from, with keys');
  await shotOf(page, src, '50-comes-from-own-studio');
  await shot(page, '51-show-editor');
  await src.getByRole('button', { name: 'The station’s room' }).click();
  check('choosing the room on a show with keys says they are to end, and ends nothing', (await src.locator('[data-testid="back-to-room"]').innerText()).includes('comes from the room again when its last key ends') && calls(st, 'DELETE', /\/keys\//).length === 0);

  await openShow('The Long Lunch');
  await src.scrollIntoViewIfNeeded();
  check('a show with no key reads The station’s room', (await src.getByRole('button', { name: 'The station’s room' }).getAttribute('aria-pressed')) === 'true' && (await src.locator('li').count()) === 0);
  await src.getByRole('button', { name: 'The host’s own studio' }).click();
  check('setting it to the host’s own studio says No key yet, and offers the setup', (await src.locator('[data-testid="no-key"]').innerText()).startsWith('No key yet.') && (await src.getByRole('button', { name: 'Set up a host…' }).count()) === 1);
  await shotOf(page, src, '52-comes-from-no-key');
  await src.getByRole('button', { name: 'Set up a host…' }).click();
  const setup = dlg(page, 'setup');
  await setup.waitFor();
  check('the same setup, with the show given', (await setup.locator('output#hk-show').innerText()) === 'The Long Lunch' && (await page.evaluate(() => document.activeElement?.id)) === 'hk-label');
  await page.fill('#hk-label', 'Duncan Boyd');
  await setup.getByRole('button', { name: 'Make the key' }).click();
  const det = dlg(page, 'details');
  await det.waitFor();
  await inView(page, det.getByRole('button', { name: 'I have sent them' }), 'I have sent them, in the Schedule tab');
  check('the Schedule tab’s field styles do not reach the dialog', await det.locator('.f input').first().evaluate((el) => getComputedStyle(el).fontFamily.toLowerCase().includes('mono')));
  await shot(page, '53-details-from-the-show-editor');
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(600);
  check('the show now comes from the host’s own studio', (await src.getByRole('button', { name: 'The host’s own studio' }).getAttribute('aria-pressed')) === 'true' && (await src.locator('li').count()) === 1 && (await src.innerText()).includes('Duncan Boyd'));
  await ctx.close();
}

// ---- 8. Your show: the presenter's own key, and Open my monitor ----
{
  const st = new HostStation({ role: 'presenter' });
  const { ctx, page } = await open(st, { settings: true });
  const mine = page.locator('[data-panel="settings.my-show"]');
  await mine.locator('[data-testid="my-show-own"]').waitFor();
  await mine.scrollIntoViewIfNeeded();
  check('a presenter has no Own studios panel', (await panel(page).count()) === 0);
  check('Your show offers Broadcast from your own software…', (await mine.getByRole('button', { name: 'Broadcast from your own software…' }).count()) === 1);
  check('with no key there is no monitor to open, and nothing says it is not built', (await mine.getByRole('button', { name: 'Open my monitor' }).count()) === 0 && !/not built/i.test(await mine.innerText()));
  await mine.getByRole('button', { name: 'Broadcast from your own software…' }).click();
  const setup = dlg(page, 'setup');
  await setup.waitFor();
  check('their setup is for their show and themselves', (await setup.locator('output#hk-show').innerText()) === 'Steeped' && (await page.inputValue('#hk-label')) === 'Finlay Grant');
  await setup.getByRole('button', { name: 'Make the key' }).click();
  const det = dlg(page, 'details');
  await det.waitFor();
  check('their key is tied to their account', JSON.stringify(calls(st, 'POST', /steeped\/keys$/)[0]?.body) === JSON.stringify({ label: 'Finlay Grant', hint: 'icecast', staff_id: 'a-finlay' }));
  const link = `https://kettle.example/monitor/#${st.keys[0].monitor}`;
  // The key is their own: the details offer Open my monitor, which hands the link to their browser.
  check('their own details offer Open my monitor beside the link', (await det.locator('#hd-mon-0').inputValue()) === link && (await det.getByRole('button', { name: 'Open my monitor' }).count()) === 1);
  await det.getByRole('button', { name: 'Open my monitor' }).scrollIntoViewIfNeeded();
  await shot(page, '61-details-own-monitor');
  await det.getByRole('button', { name: 'Open my monitor' }).click();
  await sleep(300);
  check('Open my monitor opens the whole link in the browser, and says so', JSON.stringify(await page.evaluate(() => window.__opened)) === JSON.stringify([link]) && (await det.locator('.copy-note').innerText()).startsWith('Your monitor is open in your browser.'), JSON.stringify(await page.evaluate(() => window.__opened)));
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(600);
  check('no panel name plate is left showing in Your show', (await page.locator('.pn-tag.show').count()) === 0);
  check('and then their details are in Your show', (await mine.locator('[data-testid="my-show-own"] li').count()) === 1 && (await mine.innerText()).includes('Steeped comes from your own studio.'));
  // This window made the key, so it holds the link in memory: Open my monitor works from Your show.
  const openMine = mine.getByRole('button', { name: 'Open my monitor' });
  check('Your show offers Open my monitor, live in the window that made the key', (await openMine.count()) === 1 && (await openMine.getAttribute('aria-disabled')) !== 'true');
  await openMine.click();
  await sleep(300);
  check('it opens the same link again', JSON.stringify(await page.evaluate(() => window.__opened)) === JSON.stringify([link, link]) && (await mine.innerText()).includes('Your monitor is open in your browser.'));
  check('the link is in memory only: not on the page, and not in storage', !(await page.content()).includes(st.keys[0].monitor) && (await page.evaluate((x) => !JSON.stringify({ ...localStorage, ...sessionStorage }).includes(x), st.keys[0].monitor)));
  await mine.scrollIntoViewIfNeeded();
  await fits(mine, 'Your show');
  await shotOf(page, mine, '60-your-show');
  await ctx.close();

  // Another window, or the same one opened again: the station cannot give the link back, so the
  // button is drawn off and says where the link is.
  const again = await open(st, { settings: true });
  const theirs = again.page.locator('[data-panel="settings.my-show"]');
  await theirs.locator('[data-testid="my-monitor"]').waitFor();
  await theirs.scrollIntoViewIfNeeded();
  const off = theirs.getByRole('button', { name: 'Open my monitor' });
  check('a window that did not make the key draws Open my monitor off, with why', (await off.getAttribute('aria-disabled')) === 'true' && ((await off.getAttribute('title')) ?? '').startsWith('This window does not hold your monitor link') && (await words(theirs.locator('[data-testid="my-monitor"]'))).includes('Your monitor link came with your details, shown once.'), await words(theirs.locator('[data-testid="my-monitor"]')));
  await off.click({ force: true });
  await sleep(200);
  check('and pressing it opens nothing', JSON.stringify(await again.page.evaluate(() => window.__opened)) === '[]');
  await shotOf(again.page, theirs, '62-your-show-no-link');
  await again.ctx.close();
}

// ---- 9. The show editor's presenter, picked from the staff accounts ----
/** The show editor of one show, on its Details tab. */
async function editShow(page, name) {
  if (await page.getByRole('button', { name: '‹ Shows' }).count()) await page.getByRole('button', { name: '‹ Shows' }).click();
  await page.locator('[data-panel="calendar.shows"] .showrow', { hasText: name }).first().click();
  await page.locator('aside.editor').getByRole('tab', { name: 'Details' }).click();
  const g = page.locator('[data-panel="calendar.show-presenter"]');
  await g.waitFor();
  await g.scrollIntoViewIfNeeded();
  await sleep(250);
  return g;
}
const saves = (st, slug) => calls(st, 'PUT', new RegExp(`/api/v1/shows/${slug}$`));
const optionsOf = (g) => g.locator('select option').evaluateAll((os) => os.map((o) => ({ value: o.value, text: o.textContent, disabled: o.disabled })));
for (const theme of ['dark', 'light', 'night']) {
  const { st } = withHosts({ role: 'admin' });
  const { ctx, page } = await open(st, { theme, schedule: true });
  const g = await editShow(page, 'Steeped');
  const pick = g.getByLabel("The presenter's staff account");
  const opts = await optionsOf(g);
  check(`${theme}: an admin is offered no account and all four of the team, the show’s own picked`, opts.length === 5 && opts[0].value === '' && opts.slice(1).every((o) => !o.disabled) && (await pick.inputValue()) === 'a-finlay', JSON.stringify(opts));
  check(`${theme}: it is the group’s first field, above the on-air name`, (await g.locator('.field').first().getAttribute('data-field')) === 'staff-account');
  check(`${theme}: it names the staff by role, and the admin themselves (you)`, opts.some((o) => o.text === 'Morag Campbell · admin (you)') && opts.some((o) => o.text === 'Finlay Grant · presenter'));
  await fits(g, `${theme}: the presenter group`);
  await shotOf(page, g, `s8-1-${theme}-presenter-admin`);
  if (theme === 'dark') {
    // A save that changes only the name leaves the account out: the station would refuse it from a producer.
    await page.getByLabel("Presenter's on-air name").fill('Finlay G');
    await page.getByRole('button', { name: 'Save show' }).click();
    await sleep(500);
    check('a save that left the account as it was does not send it', saves(st, 'steeped').length === 1 && !('presenter_account' in saves(st, 'steeped')[0].body), JSON.stringify(saves(st, 'steeped')[0]?.body));
    await pick.selectOption('a-shona');
    await page.getByRole('button', { name: 'Save show' }).click();
    await sleep(500);
    check('an admin who picked Shona sends her id, and is let', saves(st, 'steeped').at(-1).body.presenter_account === 'a-shona' && st.shows.find((x) => x.slug === 'steeped').presenter_account === 'a-shona', JSON.stringify(saves(st, 'steeped').at(-1)?.body.presenter_account));
    // A show with no account: picking one offers its name when the on-air name is empty.
    const g2 = await editShow(page, 'The Long Lunch');
    const name = page.getByLabel("Presenter's on-air name");
    await name.fill('');
    await g2.getByLabel("The presenter's staff account").selectOption('a-isla');
    check('picking an account fills an empty on-air name with theirs', (await name.inputValue()) === 'Isla Robertson', await name.inputValue());
    await name.fill('Duncan');
    await g2.getByLabel("The presenter's staff account").selectOption('a-finlay');
    check('and never writes over a name that was typed', (await name.inputValue()) === 'Duncan', await name.inputValue());
  }
  await ctx.close();
}
{
  const { st } = withHosts({ role: 'producer' });
  const { ctx, page } = await open(st, { schedule: true });
  const g = await editShow(page, 'Steeped');
  const pick = g.getByLabel("The presenter's staff account");
  const opts = await optionsOf(g);
  check('a producer is offered no account, themselves, and the account the show has, which they cannot pick', opts.map((o) => `${o.value || '-'}${o.disabled ? '!' : ''}`).join(' ') === '- a-finlay! a-shona' && opts[2].text === 'Shona Campbell · producer (you)', JSON.stringify(opts));
  check('and is told only an admin names another', (await g.locator('.field').first().innerText()).includes('Only an admin names another member of staff.'));
  await shotOf(page, g, 's8-2-presenter-producer');
  await page.getByLabel('Description').fill('Weekday teatime, from Finlay’s own studio.');
  await page.getByRole('button', { name: 'Save show' }).click();
  await sleep(500);
  check('a producer’s save (a description: theirs to change) of a show tied to someone else’s account sends no account, so it is let', saves(st, 'steeped').length === 1 && !('presenter_account' in saves(st, 'steeped')[0].body) && !(await page.locator('aside.editor .problem').count()), JSON.stringify(saves(st, 'steeped')[0]?.body));
  await pick.selectOption('');
  await page.getByRole('button', { name: 'Save show' }).click();
  await sleep(500);
  check('clearing it sends null, which a producer may', saves(st, 'steeped').at(-1).body.presenter_account === null && st.shows.find((x) => x.slug === 'steeped').presenter_account === null, JSON.stringify(saves(st, 'steeped').at(-1)?.body.presenter_account));
  const g2 = await editShow(page, 'The Long Lunch');
  await g2.getByLabel("The presenter's staff account").selectOption('a-shona');
  await page.getByRole('button', { name: 'Save show' }).click();
  await sleep(500);
  check('naming themselves sends their id, and is let', saves(st, 'the-long-lunch').at(-1).body.presenter_account === 'a-shona' && !(await page.locator('aside.editor .problem').count()), JSON.stringify(saves(st, 'the-long-lunch').at(-1)?.body.presenter_account));
  await ctx.close();
}
{
  // A station before 1.1 has no directory: an admin still reads the staff list, a producer sees no picker.
  for (const role of ['admin', 'producer']) {
    const { st } = withHosts({ role, routes: false });
    const { ctx, page } = await open(st, { schedule: true });
    const g = await editShow(page, 'Steeped');
    await sleep(400);
    const has = (await g.getByLabel("The presenter's staff account").count()) === 1;
    check(`before 1.1, ${role === 'admin' ? 'an admin reads the staff list for it' : 'a producer has no picker, and the account is kept as it is'}`, has === (role === 'admin'), `picker ${has}`);
    await ctx.close();
  }
}

{
  // The directory is a producer's and an admin's (ADR-0289): the editor does not ask the station
  // for it from a presenter's or a screener's window, which is offered only itself (Steeped is tied
  // to Finlay, so only Finlay has a picker; the screener's has none, since it could not name him).
  for (const [role, picker] of [['presenter', true], ['screener', false]]) {
    const { st } = withHosts({ role });
    const { ctx, page } = await open(st, { schedule: true });
    const g = await editShow(page, 'Steeped');
    await sleep(500);
    const asked = st.calls.filter((c) => c.path === '/api/v1/staff/directory').length;
    const has = (await g.getByLabel("The presenter's staff account").count()) === 1;
    check(`a ${role} ${picker ? 'is offered the picker (themselves)' : 'has no picker for a show tied to someone else'} and the studio never asks for the directory`, has === picker && asked === 0, `picker ${has}, asked ${asked}`);
    await ctx.close();
  }
}

// ---- 10. New show: Comes from, and Done's Set up a host… ----
const nsDlg = (page) => page.locator('dialog[data-panel="calendar.new-show"]');
for (const theme of ['dark', 'light', 'night']) {
  const { st } = withHosts({ role: 'producer' });
  const { ctx, page } = await open(st, { theme, schedule: true });
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = nsDlg(page);
  await d.waitFor();
  const src = d.locator('[data-testid="ns-comes"]');
  await src.waitFor();
  check(`${theme}: Comes from is on part 1, with the station’s room picked`, (await src.getByRole('button', { name: 'The station’s room' }).getAttribute('aria-pressed')) === 'true');
  await page.keyboard.type('Bothy Ballads');
  await src.getByRole('button', { name: 'The host’s own studio' }).click();
  check(`${theme}: picking the host’s own studio says the key is made once the show is`, (await words(src)).includes('You make the key when the show is made.'));
  await fits(d, `${theme}: New show part 1`);
  await inView(page, d.getByRole('button', { name: 'Next: when it airs' }), `${theme}: Next, with Comes from drawn`);
  await shot(page, `s8-3-${theme}-new-show-comes-from`);
  if (theme !== 'dark') {
    await ctx.close();
    continue;
  }
  // A show with callers wants SRT from its host: the step picks the way offered.
  await d.locator('input[data-step="callers"]').check({ force: true });
  await d.getByRole('button', { name: 'Next: when it airs' }).click();
  await d.getByRole('button', { name: 'No dates yet' }).click();
  const done = d.locator('[data-testid="ns-host"]');
  await done.waitFor();
  check('Done says it comes from the host’s own studio once a key is made, and offers Set up a host…', (await words(done)).startsWith('It comes from the host’s own studio once a key is made.') && (await done.getByRole('button', { name: 'Set up a host…' }).count()) === 1, await words(done));
  await shot(page, 's8-4-new-show-done-set-up-a-host');
  await done.getByRole('button', { name: 'Set up a host…' }).click();
  const setup = dlg(page, 'setup');
  await setup.waitFor();
  check('the setup is for the new show, given, and offers SRT for a show with callers', (await setup.locator('output#hk-show').innerText()) === 'Bothy Ballads' && (await setup.locator('input[name="hk-hint"]:checked').getAttribute('value')) === 'srt', await setup.locator('output#hk-show').innerText());
  await page.fill('#hk-label', 'Rhona Beaton');
  await shot(page, 's8-5-new-show-set-up-a-host-dialog');
  await page.locator('#hk-label').press('Enter');
  const det = dlg(page, 'details');
  await det.waitFor();
  check('the key is made for the new show, SRT', posted(st, /\/shows\/bothy-ballads\/keys$/) === 1 && calls(st, 'POST', /\/shows\/bothy-ballads\/keys$/)[0].body.hint === 'srt', JSON.stringify(calls(st, 'POST', /\/keys$/).at(-1)?.body));
  await det.getByRole('button', { name: 'I have sent them' }).click();
  await sleep(500);
  check('New show is still open under it, and says the host has a key', (await nsDlg(page).count()) === 1 && (await words(nsDlg(page).locator('[data-testid="ns-host"]'))).startsWith('Its host has a key.'), await words(nsDlg(page).locator('[data-testid="ns-host"]')));
  await shot(page, 's8-6-new-show-done-keyed');
  await ctx.close();
}
{
  // Where it is not offered: a presenter, a station before 1.1, and a show from the room.
  for (const [what, o, expect] of [['a presenter', { role: 'presenter' }, false], ['a station before 1.1', { role: 'producer', routes: false }, false], ['a producer on 1.1', { role: 'producer' }, true]]) {
    const { st } = withHosts(o);
    const { ctx, page } = await open(st, { schedule: true });
    await page.getByRole('button', { name: 'New show' }).first().click();
    await nsDlg(page).waitFor();
    await sleep(500);
    const has = (await nsDlg(page).locator('[data-testid="ns-comes"]').count()) === 1;
    check(`New show ${expect ? 'offers' : 'does not offer'} Comes from to ${what}`, has === expect, `offered ${has}`);
    await ctx.close();
  }
  const { st } = withHosts({ role: 'producer' });
  const { ctx, page } = await open(st, { schedule: true });
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = nsDlg(page);
  await d.waitFor();
  await page.keyboard.type('Tuesday Tunes');
  await d.getByRole('button', { name: 'Next: when it airs' }).click();
  await d.getByRole('button', { name: 'No dates yet' }).click();
  await d.locator('.done').waitFor();
  check('a show left at the station’s room says nothing of hosts on Done', (await d.locator('[data-testid="ns-host"]').count()) === 0 && !(await words(d)).includes('Set up a host'));
  await ctx.close();
}

// ---- 9. The same screens in Light and Night ----
for (const theme of ['light', 'night']) {
  const { st } = withHosts({ role: 'admin' });
  let w = await open(st, { theme });
  await tile(w.page, 5).waitFor();
  check(`${theme}: the theme is on the page`, (await w.page.evaluate(() => document.documentElement.dataset.theme)) === theme);
  await shot(w.page, `70-${theme}-desk`);
  await shotOf(w.page, tile(w.page, 5), `71-${theme}-tile`);
  await w.ctx.close();

  w = await open(st, { theme, settings: true });
  await panel(w.page).locator('table').waitFor();
  await panel(w.page).scrollIntoViewIfNeeded();
  await fits(panel(w.page), `${theme}: the rows`);
  await shotOf(w.page, panel(w.page), `72-${theme}-settings`);
  await panel(w.page).getByRole('button', { name: 'Set up a host…' }).click();
  await w.page.selectOption('#hk-show', 'the-long-lunch');
  await w.page.fill('#hk-label', 'Duncan Boyd');
  await shot(w.page, `73-${theme}-setup`);
  await dlg(w.page, 'setup').getByRole('button', { name: 'Make the key' }).click();
  await dlg(w.page, 'details').waitFor();
  await inView(w.page, dlg(w.page, 'details').getByRole('button', { name: 'I have sent them' }), `${theme}: I have sent them`);
  await shot(w.page, `74-${theme}-details`);
  await w.ctx.close();
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed (${SIZE.width}×${SIZE.height}, text ${SIZE.text})`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
