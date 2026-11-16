// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the studio's half of calendar files (ADR-0281, release 1.0.5) in a real browser
// against the stand-in control plane (station.mjs), as week.mjs drives the week: Subscribe… in
// the toolbar and in the show editor (the link on the station's public address, never on the
// address the studio talks to; Copy; Download .ics; a station with no public address), and
// Import a calendar… (who is offered it; a file that is not a calendar, one too large, one with
// too many events, a bare 413; the proposed shows with their sentences, dates and clashes; ticks
// and names; a refusal placed under its row; Make the shows against what the studio sent; Done).
// The import's preview is the control plane's own reply, recorded (import-preview.json), and
// the file chosen is kettle-old-schedule.ics, the calendar it was recorded from.
//
//   cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/newshow/calendar.mjs
//   WIDTH=2560 HEIGHT=1440 TEXT=1.2 … node tests/newshow/calendar.mjs      # and 3840 by 2160
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs). Screenshots go to
// OUT, never the repository. The clock is fixed at Friday 13 November 2026, 10:00 in
// Europe/London. A download goes to Playwright's own folder, not the person's Downloads.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { IMPORT_PREVIEW, SAMPLE_ICS, Station, instant } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? '/tmp/weavesignal-calendar-shots';
const PORT = Number(process.env.PORT ?? 27916);
const NOW = instant('2026-11-13', 600) * 1000;
const ONLY = process.env.ONLY ? process.env.ONLY.split(',') : null;
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
const ORIGIN = `http://127.0.0.1:${PORT}`;

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
const SIZE = { width: Number(process.env.WIDTH ?? 1440), height: Number(process.env.HEIGHT ?? 900), text: Number(process.env.TEXT ?? 1) };
const TAG = `${SIZE.width}x${SIZE.height}${SIZE.text === 1 ? '' : `-text-${Math.round(SIZE.text * 100)}`}`;
/** The marker of a file the driver answers as a body limit would: 413 with no sentence. */
const BARE_413 = 'X-STAND-IN-BARE-413';

/** Kettle FM with The Late Kettle, the show the recorded preview's clashes name. */
function kettle(o = {}) {
  const st = new Station(o);
  st.add({ slug: 'the-late-kettle', name: 'The Late Kettle', colour_slot: 8, presenter: 'Shona Campbell', hosts: ['Shona Campbell'], rule: { starts_on: '2026-11-03', start: 1260, minutes: 120, repeat: { kind: 'weekly', every: 1, weekdays: [2] }, end: { kind: 'never' }, skip: [], moves: [] } });
  return st;
}

/** A window on a station, on the Schedule tab. */
async function open(station, { width = SIZE.width, height = SIZE.height, theme = 'dark', text = SIZE.text } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: 'Europe/London', acceptDownloads: true, permissions: ['clipboard-read', 'clipboard-write'] });
  await ctx.addInitScript(([t, z]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', String(z));
    } catch {}
  }, [theme, text]);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(NOW));
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.route(/\/(api\/|health)/, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (url.pathname === '/api/v1/studio/events' || url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    const body = req.postData() ? JSON.parse(req.postData()) : undefined;
    if (typeof body?.ics === 'string' && body.ics.includes(BARE_413)) {
      station.calls.push({ method: req.method(), path: url.pathname, body });
      return route.fulfill({ status: 413, contentType: 'text/plain', body: 'Failed to buffer the request body: length limit exceeded' });
    }
    const [status, out] = station.handle(req.method(), url.pathname + url.search, body);
    if (typeof out === 'string') return route.fulfill({ status, contentType: 'text/calendar; charset=utf-8', body: out });
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(out) });
  });
  await page.goto(`${ORIGIN}/`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  if (station.role === 'presenter') await page.getByRole('button', { name: 'Skip for now' }).click({ timeout: 5000 }).catch(() => {});
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await sleep(800);
  return { ctx, page };
}

const shot = async (page, name) => {
  await sleep(250);
  await page.screenshot({ path: join(OUT, `${name}-${TAG}.png`) });
  log('shot', name);
};
const run = (name) => !ONLY || ONLY.includes(name);
const box = async (loc) => ((await loc.count()) ? loc.first().boundingBox() : null);
const toolbar = (page) => page.locator('[data-panel="calendar.toolbar"]');
const rangeText = (page) => toolbar(page).locator('.range').innerText();
const sub = (page) => page.locator('dialog[data-panel="calendar.subscribe"]');
const imp = (page) => page.locator('dialog[data-panel="calendar.import"]');
const said = async (page) => (await sub(page).locator('[data-testid="sub-said"]').innerText()).trim();
const calls = (station, method, path) => station.calls.filter((c) => c.method === method && c.path.split('?')[0] === path);
const focusId = (page) => page.evaluate(() => document.activeElement?.id ?? '');
const text = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();
async function inView(page, loc, what) {
  const b = await box(loc);
  const vp = page.viewportSize();
  check(`${what} is inside the window (${vp.width}×${vp.height}, text ${SIZE.text})`, !!b && b.y >= 0 && b.x >= 0 && b.y + b.height <= vp.height + 0.5 && b.x + b.width <= vp.width + 0.5, b ? `at ${Math.round(b.x)},${Math.round(b.y)} to ${Math.round(b.x + b.width)},${Math.round(b.y + b.height)}` : 'not drawn');
}
/** Choose a file in the import dialog: its name and its text. */
const pick = (page, name, content) => imp(page).locator('input[type="file"]').setInputFiles({ name, mimeType: 'text/calendar', buffer: Buffer.from(content, 'utf8') });
const previews = (station) => calls(station, 'POST', '/api/v1/schedule/import/preview');
const imports = (station) => calls(station, 'POST', '/api/v1/schedule/import');
const item = (page, i) => imp(page).locator(`li.item[data-item="${i}"]`);
// The toolbar's two calendar-file buttons are one menu in a window too narrow for their words
// beside the legend (calendar-files.md section 1): each helper works on whichever is drawn.
const more = (page) => toolbar(page).getByRole('button', { name: 'Calendar files' });
const wideBtn = (page, name) => toolbar(page).locator('button.files-wide', { hasText: name });
const isWide = async (page) => (await wideBtn(page, 'Subscribe…').count()) > 0 && (await wideBtn(page, 'Subscribe…').isVisible());
/** Whether the toolbar offers an action, as a button or as an entry of the menu. */
const offers = async (page, name) => (await toolbar(page).locator('[role="menuitem"]', { hasText: name }).count()) === 1 && (await wideBtn(page, name).count()) === 1 && ((await isWide(page)) || (await more(page).isVisible()));
async function openFiles(page, name) {
  if (await isWide(page)) return wideBtn(page, name).click();
  await more(page).click();
  await toolbar(page).getByRole('menuitem', { name }).click();
}
/** What holds the focus after a dialog opened from the toolbar closes: the button that opened it. */
const backOn = async (page, name) => page.evaluate(([n, wide]) => (wide ? document.activeElement?.textContent?.trim() === n : document.activeElement?.getAttribute('aria-label') === 'Calendar files'), [name, await isWide(page)]);

// ---- 1. Subscribe…, the schedule ----
if (run('subscribe')) {
  const station = kettle();
  const { ctx, page } = await open(station);
  const bar = toolbar(page);
  const wide = await isWide(page);
  check('the toolbar offers Subscribe… and, to a producer, Import a calendar…', (await offers(page, 'Subscribe…')) && (await offers(page, 'Import a calendar…')));
  // Two buttons with their words where the window has the room, one menu where it has not.
  const room = await bar.evaluate((el) => el.clientWidth / (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16));
  check(`they are ${wide ? 'two buttons' : 'one menu'} in a toolbar ${Math.round(room)} rem wide`, wide === room >= 118 && (wide ? !(await more(page).isVisible()) && (await wideBtn(page, 'Import a calendar…').isVisible()) : (await more(page).isVisible()) && !(await wideBtn(page, 'Import a calendar…').isVisible())), `${room.toFixed(1)} rem`);
  const tops = await bar.locator(':scope > *').evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width > 0).map((e) => Math.round(e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2)));
  if (wide || (SIZE.width === 1440 && SIZE.text === 1)) check(`the toolbar stays on one line, the legend beside it (${SIZE.width} by ${SIZE.height})`, Math.max(...tops) - Math.min(...tops) < 12, tops.join(' '));
  if (SIZE.width === 1440 && SIZE.text === 1) {
    const spare = await bar.evaluate((el) => el.querySelector('.legend').getBoundingClientRect().left - el.querySelector('.files-menu').getBoundingClientRect().right);
    check('with room to spare beside the legend at 1440 by 900', spare >= 12 + 12, `${Math.round(spare)} px between the menu and the legend`);
    // The week after, 16 – 22 November, has a longer range: the toolbar week-clashes.md holds to one line.
    await page.keyboard.press('ArrowRight');
    await sleep(700);
    const next = await bar.locator(':scope > *').evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width > 0).map((e) => Math.round(e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2)));
    const nextSpare = await bar.evaluate((el) => el.querySelector('.legend').getBoundingClientRect().left - el.querySelector('.files-menu').getBoundingClientRect().right);
    check('and in the week of 16 – 22 November too', (await rangeText(page)) === '16 – 22 November 2026' && Math.max(...next) - Math.min(...next) < 12 && nextSpare >= 12 + 4, `${Math.round(nextSpare)} px between the menu and the legend`);
    await page.keyboard.press('ArrowLeft');
    await sleep(700);
  }
  const overflow = await bar.evaluate((el) => el.scrollWidth - el.clientWidth);
  check('the toolbar does not run off the window', overflow <= 1, `${overflow} px over`);
  await shot(page, '01-toolbar');

  if (!wide) {
    // The menu: a dialog, so the keys are its own; ↑ ↓ move, Esc closes, a press elsewhere closes.
    const range0 = await rangeText(page);
    await more(page).focus();
    await page.keyboard.press('Enter');
    const menu = toolbar(page).getByRole('menu', { name: 'Calendar files' });
    check('the menu opens from the keyboard with its first entry focused', (await menu.isVisible()) && (await more(page).getAttribute('aria-expanded')) === 'true' && (await page.evaluate(() => document.activeElement?.querySelector('b')?.textContent)) === 'Import a calendar…');
    await inView(page, menu, 'the calendar-files menu');
    await shot(page, '01b-toolbar-menu');
    await page.keyboard.press('ArrowDown');
    const second = await page.evaluate(() => document.activeElement?.querySelector('b')?.textContent);
    for (const k of ['t', 'd', 'ArrowRight']) await page.keyboard.press(k);
    await sleep(250);
    check('↓ moves to Subscribe…, and T, D and → do nothing to the week while it is open', second === 'Subscribe…' && (await rangeText(page)) === range0 && (await menu.isVisible()), `${second} · ${await rangeText(page)}`);
    await page.keyboard.press('Escape');
    check('Esc closes it and the focus is back on its button', !(await menu.isVisible()) && (await more(page).getAttribute('aria-expanded')) === 'false' && (await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))) === 'Calendar files');
    await more(page).click();
    await toolbar(page).locator('.tz').click();
    await sleep(150);
    check('a press elsewhere closes it', !(await menu.isVisible()));
    await page.keyboard.press('Escape');
    await sleep(150);
  }

  await openFiles(page, 'Subscribe…');
  await sub(page).waitFor({ timeout: 5000 });
  check('Subscribe… opens a dialog named for its panel, titled for the schedule', (await sub(page).locator('h2').innerText()) === 'Subscribe to the schedule');
  const webcal = await sub(page).locator('#sub-webcal').inputValue();
  const web = await sub(page).locator('#sub-web').inputValue();
  check('the link is on the station’s public address, as webcal and as https', webcal === 'webcal://kettle.example/api/v1/public/schedule.ics' && web === 'https://kettle.example/api/v1/public/schedule.ics', `${webcal} | ${web}`);
  check('and never on the address the studio talks to', !webcal.includes('127.0.0.1') && !web.includes('127.0.0.1') && !(await sub(page).innerText()).includes('127.0.0.1'));
  check('the focus opens on Copy link', (await focusId(page)) === 'sub-copy', await focusId(page));
  await inView(page, sub(page).getByRole('button', { name: /^Done/ }), 'Subscribe’s Done');
  await inView(page, sub(page).getByRole('button', { name: 'Download .ics' }), 'Download .ics');
  await inView(page, sub(page).getByRole('button', { name: 'Copy link' }), 'Copy link');
  check('the dialog itself does not scroll: its foot is always in the window', await sub(page).evaluate((el) => el.scrollHeight <= el.clientHeight + 1 && getComputedStyle(el).overflowY === 'hidden'));
  await shot(page, '02-subscribe-schedule');

  await page.keyboard.press('Enter');
  await sleep(200);
  check('Copy link puts the webcal link on the clipboard and says so', (await page.evaluate(() => navigator.clipboard.readText())) === webcal && (await said(page)) === 'Copied the calendar link.', await said(page));
  await sub(page).getByRole('button', { name: 'Copy', exact: true }).click();
  await sleep(200);
  check('Copy puts the web address on the clipboard and says so', (await page.evaluate(() => navigator.clipboard.readText())) === web && (await said(page)) === 'Copied the web address.', await said(page));

  // The clipboard refused (a webview may): the link is selected in its box and copied the old way.
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: () => Promise.reject(new Error('refused')), readText: () => Promise.reject(new Error('refused')) } }));
  await sub(page).getByRole('button', { name: 'Copy link' }).click();
  await sleep(200);
  const selected = await page.evaluate(() => { const b = document.getElementById('sub-webcal'); return document.activeElement === b && b.selectionStart === 0 && b.selectionEnd === b.value.length; });
  check('with the clipboard refused, the link is selected in its box and what happened is said', selected && /^(Copied the calendar link\.|It could not be copied from here\. The link is selected in its box: copy it from there\.)$/.test(await said(page)), await said(page));

  // The keys behind the dialog are the dialog's: the week does not move.
  const range = await rangeText(page);
  await sub(page).locator('h2').click();
  for (const k of ['t', 'd', 'ArrowRight', 'ArrowLeft', 'w']) await page.keyboard.press(k);
  await sleep(300);
  check('T, D, W and the arrows behind the dialog do nothing to the week', (await rangeText(page)) === range && (await sub(page).count()) === 1, await rangeText(page));

  const before = calls(station, 'GET', '/api/v1/public/schedule.ics').length;
  const [download] = await Promise.all([page.waitForEvent('download', { timeout: 8000 }), sub(page).getByRole('button', { name: 'Download .ics' }).click()]);
  const saved = readFileSync(await download.path(), 'utf8');
  check('Download .ics reads the file through the studio’s own address and saves it as schedule.ics', download.suggestedFilename() === 'schedule.ics' && saved.startsWith('BEGIN:VCALENDAR') && saved.includes('SUMMARY:The Late Kettle') && calls(station, 'GET', '/api/v1/public/schedule.ics').length === before + 1, download.suggestedFilename());
  await sleep(200);
  check('and says where it went', (await said(page)) === 'schedule.ics is with your browser’s downloads.', await said(page));

  await page.keyboard.press('Escape');
  await sleep(300);
  check('Esc closes it, and the focus is back on the button that opened it', (await sub(page).count()) === 0 && (await backOn(page, 'Subscribe…')));
  await ctx.close();
}

// ---- 2. Subscribe…, a station with no public address ----
if (run('no-address')) {
  const station = kettle({ publicUrl: null });
  const { ctx, page } = await open(station);
  check('a station with no public address still offers Subscribe…', await offers(page, 'Subscribe…'));
  await openFiles(page, 'Subscribe…');
  await sub(page).waitFor({ timeout: 5000 });
  const words = await text(sub(page).locator('[data-testid="sub-no-address"]'));
  check('the dialog shows no link and says why and who sets one', (await sub(page).locator('input').count()) === 0 && words.startsWith('This station has no public address, so there is no link to give out.') && words.includes('publishing.public_url'), words);
  check('nothing in it is made from the studio’s own address', !(await sub(page).innerText()).includes('127.0.0.1') && !(await sub(page).innerText()).includes('webcal:'));
  check('the focus opens on Download .ics', (await focusId(page)) === 'sub-download', await focusId(page));
  const [download] = await Promise.all([page.waitForEvent('download', { timeout: 8000 }), page.keyboard.press('Enter')]);
  check('and the file can still be downloaded', download.suggestedFilename() === 'schedule.ics' && readFileSync(await download.path(), 'utf8').startsWith('BEGIN:VCALENDAR'));
  await inView(page, sub(page).getByRole('button', { name: /^Done/ }), 'Done, with no address');
  await shot(page, '03-subscribe-no-address');
  await ctx.close();

  // A station older than 1.0.5 sends no public address with the week: it has no calendar files.
  const old = new Station({ version: '1.0.3' });
  const o = await open(old);
  check('a station that does not send its public address is offered neither', (await toolbar(o.page).locator('.files-wide, .files-menu, [role="menuitem"]').count()) === 0 && !(await toolbar(o.page).innerText()).includes('Subscribe'));
  await o.ctx.close();
}

// ---- 3. Subscribe…, one show, from the show editor ----
if (run('show')) {
  const station = kettle({ role: 'presenter' });
  const { ctx, page } = await open(station);
  check('a presenter is offered Subscribe… but not Import a calendar…', (await offers(page, 'Subscribe…')) && (await toolbar(page).locator('button', { hasText: 'Import a calendar…' }).count()) === 0);
  check('and the station refuses a presenter’s import anyway', station.handle('POST', '/api/v1/schedule/import/preview', { ics: SAMPLE_ICS })[0] === 403);
  const openShow = async (name) => {
    await page.locator('[data-panel="calendar.shows"], [data-panel="calendar.unscheduled"]').getByRole('button', { name, exact: false }).first().click();
    await page.getByRole('tab', { name: 'Details' }).click();
    await page.locator('[data-panel="calendar.show-rule"]').waitFor({ timeout: 5000 });
  };
  await openShow('The Late Kettle');
  const rule = page.locator('[data-panel="calendar.show-rule"]');
  check('the show editor offers Subscribe… beside When it airs', (await rule.getByRole('button', { name: 'Subscribe…' }).count()) === 1);
  await rule.getByRole('button', { name: 'Subscribe…' }).click();
  await sub(page).waitFor({ timeout: 5000 });
  const webcal = await sub(page).locator('#sub-webcal').inputValue();
  check('it opens the show’s own link, on the public address', (await sub(page).locator('h2').innerText()) === 'Subscribe to The Late Kettle' && webcal === 'webcal://kettle.example/api/v1/public/shows/the-late-kettle.ics' && (await sub(page).locator('#sub-web').inputValue()) === 'https://kettle.example/api/v1/public/shows/the-late-kettle.ics', webcal);
  check('a show with dates does not say its calendar is empty', (await sub(page).locator('[data-testid="sub-empty"]').count()) === 0);
  const [download] = await Promise.all([page.waitForEvent('download', { timeout: 8000 }), sub(page).getByRole('button', { name: 'Download .ics' }).click()]);
  const saved = readFileSync(await download.path(), 'utf8');
  check('Download .ics saves that show alone, under its path', download.suggestedFilename() === 'the-late-kettle.ics' && saved.includes('SUMMARY:The Late Kettle') && !saved.includes('Kettle Breakfast'), download.suggestedFilename());
  await inView(page, sub(page).getByRole('button', { name: /^Done/ }), 'Done, for one show');
  await shot(page, '04-subscribe-show');
  await sub(page).getByRole('button', { name: /^Done/ }).click();
  await sleep(300);
  check('Done closes it and the focus is back on the editor’s Subscribe…', (await sub(page).count()) === 0 && (await page.evaluate(() => document.activeElement?.textContent?.trim())) === 'Subscribe…');

  // A show with no dates: its calendar is empty until it has.
  await page.locator('.schedule aside.editor').getByRole('button', { name: '‹ Shows' }).click();
  await openShow('Northern Lights');
  await page.locator('[data-panel="calendar.show-rule"]').getByRole('button', { name: 'Subscribe…' }).click();
  await sub(page).waitFor({ timeout: 5000 });
  check('a show with no dates says its calendar is empty until it has', (await text(sub(page).locator('[data-testid="sub-empty"]'))) === 'Northern Lights has no dates yet, so its calendar is empty until it has.', await text(sub(page).locator('.carries').first()));
  await ctx.close();
}

// ---- 4. Import a calendar…: the file ----
if (run('import-file')) {
  const station = kettle();
  const { ctx, page } = await open(station);
  await openFiles(page, 'Import a calendar…');
  await imp(page).waitFor({ timeout: 5000 });
  check('Import a calendar… opens on its first part, the file, with the limits said', (await imp(page).getAttribute('data-part')) === '1' && (await text(imp(page).locator('#ic-limits'))) === 'Up to 1 MB and 200 events.' && (await focusId(page)) === 'ic-choose', await focusId(page));
  await inView(page, imp(page).getByRole('button', { name: /^Cancel/ }), 'the import’s Cancel');
  await shot(page, '10-import-choose');

  await pick(page, 'notes.txt', 'Shows we used to run:\n- Folk Night, Tuesdays\n');
  await imp(page).locator('[data-testid="ic-problem"]').waitFor({ timeout: 5000 });
  check('a file that is not a calendar is refused in the station’s sentence, named', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'notes.txt was not read. This is not a calendar file: it does not begin with BEGIN:VCALENDAR.' && (await imp(page).getAttribute('data-part')) === '1', await text(imp(page).locator('[data-testid="ic-problem"]')));
  await shot(page, '11-import-not-a-calendar');

  // Not UTF-8: read loosely, its letters would be spoilt and the shows made under wrong names.
  const before8 = previews(station).length;
  await imp(page).locator('input[type="file"]').setInputFiles({ name: 'latin1.ics', mimeType: 'text/calendar', buffer: Buffer.concat([Buffer.from('BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:C', 'latin1'), Buffer.from([0xe8]), Buffer.from('ilidh\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n', 'latin1')]) });
  await page.waitForFunction(() => document.querySelector('[data-testid="ic-problem"]')?.textContent?.includes('latin1.ics'), null, { timeout: 5000 }).catch(() => {});
  check('a file that is not UTF-8 is refused in a sentence and is not sent', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'latin1.ics was not read. The file is not text in UTF-8, so it is not a calendar file we can read.' && previews(station).length === before8, await text(imp(page).locator('[data-testid="ic-problem"]')));

  const sent = previews(station).length;
  await pick(page, 'everything.ics', `BEGIN:VCALENDAR\r\n${'X-PAD:' + 'x'.repeat(70) + '\r\n'}`.repeat(1) + 'x'.repeat(1024 * 1024));
  await sleep(400);
  check('a file over 1 MB is said at once and is not sent', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'everything.ics is larger than 1 MB. Export a shorter range of dates, or one calendar at a time.' && previews(station).length === sent, await text(imp(page).locator('[data-testid="ic-problem"]')));

  const event = 'BEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261201T200000Z\r\nSUMMARY:x\r\nEND:VEVENT\r\n';
  await pick(page, 'ten-years.ics', `BEGIN:VCALENDAR\r\n${event.repeat(201)}END:VCALENDAR\r\n`);
  await page.waitForFunction(() => document.querySelector('[data-testid="ic-problem"]')?.textContent?.includes('201 events'), null, { timeout: 5000 }).catch(() => {});
  check('a file of 201 events is refused (413) in the station’s sentence', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'ten-years.ics was not read. The file holds 201 events; at most 200 can be read at once. Export a shorter range of dates.', await text(imp(page).locator('[data-testid="ic-problem"]')));

  await pick(page, 'proxied.ics', `BEGIN:VCALENDAR\r\n${BARE_413}:1\r\n${event}END:VCALENDAR\r\n`);
  await page.waitForFunction(() => document.querySelector('[data-testid="ic-problem"]')?.textContent?.includes('proxied.ics'), null, { timeout: 5000 }).catch(() => {});
  check('a 413 with no sentence (a body limit’s) is said plainly, not as a status', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'proxied.ics was not read. That file is too large. A calendar file can be up to 1 MB and hold up to 200 events: export a shorter range of dates, or one calendar at a time.', await text(imp(page).locator('[data-testid="ic-problem"]')));
  check('nothing was made by any of that', imports(station).length === 0);

  await page.keyboard.press('Escape');
  await sleep(300);
  check('Esc closes the first part without asking: nothing was chosen', (await imp(page).count()) === 0 && (await backOn(page, 'Import a calendar…')));
  await ctx.close();
}

// ---- 4b. Import a calendar…: a file dropped on the dialog (the 1.0.5 test run) ----
// This is the browser build: a page cannot read a path, so the field for a pasted path is not
// drawn here. The packaged app's field and its drop go through `calendar_read`, tested in Rust
// (apps/studio/src-tauri/src/calendar.rs) and, with Tauri faked, in calendar-path.test.ts.
if (run('import-drop')) {
  const station = kettle();
  const { ctx, page } = await open(station);
  await openFiles(page, 'Import a calendar…');
  await imp(page).waitFor({ timeout: 5000 });
  const transfer = (name, content) => page.evaluateHandle(([n, c]) => { const d = new DataTransfer(); d.items.add(new File([c], n, { type: n.endsWith('.ics') ? 'text/calendar' : 'text/plain' })); return d; }, [name, content]);
  const drag = async (what, dataTransfer, on = 'dialog[data-panel="calendar.import"]') => page.dispatchEvent(on, what, { dataTransfer });
  check('a browser build offers the chooser and the drop, and no field for a path', (await imp(page).locator('#ic-path').count()) === 0 && (await imp(page).locator('[data-testid="ic-pathrow"]').count()) === 0 && (await text(imp(page).locator('[data-testid="ic-drop"]'))) === 'Or drop the file anywhere on this dialog.' && (await imp(page).locator('#ic-choose').getAttribute('aria-describedby')) === 'ic-limits ic-drop');
  await inView(page, imp(page).locator('[data-testid="ic-drop"]'), 'the drop’s line');

  const notes = await transfer('notes.txt', 'Shows we used to run');
  await drag('dragover', notes);
  await sleep(150);
  const zone = imp(page).locator('.pickfile');
  check('a file dragged over the dialog shows where it will land', (await zone.getAttribute('class')).split(' ').includes('over') && (await imp(page).getAttribute('class')).split(' ').includes('over') && (await text(imp(page).locator('[data-testid="ic-drop"]'))) === 'Let go to read the file.' && (await zone.evaluate((el) => getComputedStyle(el).borderTopStyle)) === 'solid');
  await shot(page, '10b-import-drag-over');
  await drag('drop', notes);
  await sleep(200);
  check('a dropped file that is not named as a calendar is said, and is not sent', (await text(imp(page).locator('[data-testid="ic-problem"]'))) === 'notes.txt is not a calendar file: its name does not end in .ics.' && previews(station).length === 0 && !(await zone.getAttribute('class')).split(' ').includes('over'), await text(imp(page).locator('[data-testid="ic-problem"]')));
  // A file that misses the dialog is still not opened in place of the studio.
  const stray = await page.evaluate(() => { const d = new DataTransfer(); d.items.add(new File(['x'], 'stray.txt')); const over = new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: d }); const drop = new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: d }); document.body.dispatchEvent(over); document.body.dispatchEvent(drop); return over.defaultPrevented && drop.defaultPrevented; });
  check('a file dropped beside the dialog does not take the window away', stray && (await imp(page).count()) === 1 && page.url().startsWith(`http://127.0.0.1:${PORT}/`));

  const good = await transfer('kettle-old-schedule.ics', SAMPLE_ICS);
  await drag('dragover', good);
  await drag('drop', good);
  await imp(page).locator('li.item').first().waitFor({ timeout: 5000 });
  check('a dropped calendar file is read as a chosen one is: sent as it is, once, and its shows listed', previews(station).length === 1 && previews(station)[0].body.ics === SAMPLE_ICS && (await imp(page).getAttribute('data-part')) === '2' && (await text(imp(page).locator('h2'))) === 'Import a calendar · kettle-old-schedule.ics' && (await imp(page).locator('li.item').count()) === 7 && !(await imp(page).getAttribute('class')).split(' ').includes('over'));
  // With the shows listed, another file may be dropped in its place, as Choose another file… does.
  const second = await transfer('second.ics', SAMPLE_ICS);
  await drag('drop', second);
  await page.waitForFunction(() => document.querySelector('dialog[data-panel="calendar.import"] h2')?.textContent?.includes('second.ics'), null, { timeout: 5000 }).catch(() => {});
  check('another file dropped over the listed shows takes their place', previews(station).length === 2 && (await text(imp(page).locator('h2'))) === 'Import a calendar · second.ics');
  check('nothing was made by a drop', imports(station).length === 0);
  await ctx.close();
}

// ---- 5. Import a calendar…: the shows, and making them ----
if (run('import')) {
  const station = kettle();
  const { ctx, page } = await open(station);
  await openFiles(page, 'Import a calendar…');
  await imp(page).waitFor({ timeout: 5000 });
  await pick(page, 'kettle-old-schedule.ics', SAMPLE_ICS);
  await imp(page).locator('li.item').first().waitFor({ timeout: 5000 });
  check('the file goes to the station as it is, once, and the dialog moves to its second part', previews(station).length === 1 && previews(station)[0].body.ics === SAMPLE_ICS && (await imp(page).getAttribute('data-part')) === '2');
  check('the head says the file, what was read and whose clock', (await text(imp(page).locator('h2'))) === 'Import a calendar · kettle-old-schedule.ics' && (await text(imp(page).locator('.head .small'))).includes('10 events read · 7 shows proposed, 6 can be made. Every time is on the station’s clock, Europe/London.') && (await text(imp(page).locator('[data-testid="ic-note"]'))) === '1 cancelled event was left out.', await text(imp(page).locator('.head .small')));
  check('seven shows are proposed, each marked: four exact, two chosen dates, one not a show', (await imp(page).locator('li.item').count()) === 7 && (await imp(page).locator('li.item[data-mapping="exact"]').count()) === 4 && (await imp(page).locator('li.item[data-mapping="dates"]').count()) === 2 && (await imp(page).locator('li.item[data-mapping="refused"]').count()) === 1);
  check('each is said back in New show’s words: the sentence', (await text(item(page, 0).locator('.summary .say'))) === 'Every week on Tuesday, 20:00 to 22:00, from 17 November. Not on 29 December.' && (await text(item(page, 1).locator('.summary'))) === 'Every month on the first Thursday, 19:00 to 20:00, from 3 December, for 6 episodes. The last is Thursday 6 May 2027.', await text(item(page, 1).locator('.summary')));
  check('its next dates as rows', (await item(page, 0).locator('.nd li.r').count()) === 4 && (await text(item(page, 0).locator('.nd li.r').first())).startsWith('Tue 17 Nov 20:00–22:00') && (await text(item(page, 0).locator('.after'))) === 'and so on, with no end', await text(item(page, 0).locator('.nd li.r').first()));
  check('and its clashes: every clashing row is marked, and a clash they all share is said once, in New show’s sentence', (await item(page, 0).locator('.nd li.r.clash .flag').count()) === 4 && (await text(item(page, 0).locator('[data-clash]'))).toLowerCase() === 'conflict every date overlaps the late kettle (tue, 21:00–23:00) by an hour.' && (await item(page, 0).locator('.nd li.r.clash .flag').first().getAttribute('title')) === 'The Late Kettle, 21:00 to 23:00 · overlaps 1 hour', await text(item(page, 0).locator('[data-clash]')));
  check('what a clash costs is said once for the file, and that it does not stop the import', (await text(imp(page).locator('[data-testid="ic-cost"]'))).startsWith('One ticked show clashes with shows the station has. Where two shows overlap, the one that starts later is on air by the schedule') && (await text(imp(page).locator('[data-testid="ic-cost"]'))).includes('A clash does not stop the import'), await text(imp(page).locator('[data-testid="ic-cost"]')));
  check('chosen dates say why', (await text(item(page, 2).locator('.map'))).toLowerCase() === 'chosen dates' && (await text(item(page, 2).locator('.reason'))).includes('It repeats every 2 days, which a show\'s rule cannot say. These are its dates for the next twelve months.'), await text(item(page, 2).locator('.reason')));
  check('a date at another time is marked moved', (await text(item(page, 4).locator('.nd li.r').nth(2))).toLowerCase().includes('moved · was sat 30 jan'), await text(item(page, 4).locator('.nd li.r').nth(2)));
  check('what cannot be a show has its reason and nothing to tick or rename', (await text(item(page, 5).locator('.map'))).toLowerCase() === 'not a show' && (await text(item(page, 5).locator('.reason'))).endsWith('It is an all-day event: a show needs a time to start.') && (await item(page, 5).locator('input').count()) === 0);
  check('five are ticked, and the button counts them', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '5 of 6 ticked' && (await text(imp(page).locator('#ic-make'))) === 'Make 5 shows');
  // A show the station already has (the 1.0.5 test run: a station's own calendar read back was
  // five shows, each a CONFLICT with itself). The file's last event is the old system's own
  // Kettle drive, at Kettle Drive's times.
  const there = item(page, 6);
  check('a show the station already has starts unticked', (await there.getAttribute('data-already')) === 'kettle-drive' && !(await there.locator('input[type="checkbox"]').isChecked()) && (await there.getAttribute('class')).split(' ').includes('off'));
  check('it says Already on the station, and which show it is, in place of a clash', (await text(there.locator('[data-there]'))).toLowerCase() === 'already on the station it is there as kettle drive, with the same dates and times. left unticked, so it is not made twice.' && (await there.locator('[data-clash]').count()) === 0 && (await there.locator('.nd li.r.clash').count()) === 0 && (await there.locator('.nd li.r').count()) === 4, await text(there.locator('[data-there]')));
  check('the reason it is unticked is not dimmed with the rest of its row', (await there.locator('[data-there]').evaluate((el) => getComputedStyle(el).opacity)) === '1' && (await there.locator('.summary').evaluate((el) => getComputedStyle(el).opacity)) !== '1');
  check('it is said once for the file, and the clashes’ warning does not count it', (await text(imp(page).locator('[data-testid="ic-there"]'))) === 'One of these shows is already on the station, with the same dates and times. It starts unticked, so it is not made twice.' && (await text(imp(page).locator('[data-testid="ic-cost"]'))).startsWith('One ticked show clashes with shows the station has.'), await text(imp(page).locator('[data-testid="ic-there"]')));
  await imp(page).getByRole('button', { name: 'Untick all' }).click();
  await imp(page).getByRole('button', { name: 'Tick all', exact: true }).click();
  check('Tick all ticks what is new and leaves it unticked', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '5 of 6 ticked' && !(await there.locator('input[type="checkbox"]').isChecked()));
  await there.locator('input[type="checkbox"]').check();
  check('ticked by hand, its row says a second show is made, and the warning counts it', (await text(there.locator('[data-there]'))).endsWith('Ticked, a second show is made, and the two clash on every date.') && (await text(imp(page).locator('[data-testid="ic-count"]'))) === '6 of 6 ticked' && (await text(imp(page).locator('[data-testid="ic-cost"]'))).startsWith('2 ticked shows clash with shows the station has.') && (await text(imp(page).locator('#ic-make'))) === 'Make 6 shows', await text(there.locator('[data-there]')));
  await imp(page).getByRole('button', { name: 'Tick all', exact: true }).click();
  check('and Tick all leaves it as the person set it', await there.locator('input[type="checkbox"]').isChecked());
  await there.locator('input[type="checkbox"]').uncheck();
  check('unticked again, it is as it began', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '5 of 6 ticked' && (await text(imp(page).locator('[data-testid="ic-cost"]'))).startsWith('One ticked show clashes'));
  await inView(page, imp(page).locator('#ic-make'), 'Make 5 shows');
  await inView(page, imp(page).getByRole('button', { name: 'Cancel', exact: true }), 'the import’s Cancel, with the shows listed');
  await inView(page, imp(page).getByRole('button', { name: 'Tick all', exact: true }), 'Tick all');
  const dlg = await box(imp(page));
  const vp = page.viewportSize();
  check('the dialog is inside the window, and only its list scrolls', dlg.y >= 0 && dlg.y + dlg.height <= vp.height + 0.5 && (await imp(page).evaluate((el) => el.scrollHeight <= el.clientHeight + 1)), `${Math.round(dlg.y)} to ${Math.round(dlg.y + dlg.height)} of ${vp.height}; ${await imp(page).evaluate((el) => `${el.scrollHeight}/${el.clientHeight} ` + [...el.children].map((c) => `${c.className || c.tagName}:${c.offsetTop}+${c.offsetHeight}`).join(' '))}`);
  await shot(page, '12b-import-already-on-the-station');
  await imp(page).locator('.scroll').evaluate((el) => (el.scrollTop = 0));
  await shot(page, '12-import-shows');

  // Keys: a name is typed, not acted on.
  const range = await rangeText(page);
  await item(page, 0).locator('input.name').fill('The Folk Hour');
  await page.keyboard.press('t');
  await page.keyboard.press('Backspace');
  await page.keyboard.press('Enter');
  await sleep(300);
  check('⏎ in a name does not make the shows, and letters typed there do not reach the week', imports(station).length === 0 && (await imp(page).getAttribute('data-part')) === '2' && (await item(page, 0).locator('input.name').inputValue()) === 'The Folk Hour' && (await rangeText(page)) === range);
  check('the tick is named for the show as it is now', (await item(page, 0).locator('input[type="checkbox"]').getAttribute('aria-label')) === 'Make The Folk Hour');

  // Ticks.
  await item(page, 2).locator('input[type="checkbox"]').uncheck();
  await item(page, 3).locator('input[type="checkbox"]').press('Space');
  check('unticking leaves a show out, from the pointer and from the keyboard', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '3 of 6 ticked' && (await text(imp(page).locator('#ic-make'))) === 'Make 3 shows', await text(imp(page).locator('[data-testid="ic-count"]')));
  await imp(page).getByRole('button', { name: 'Untick all' }).click();
  check('with nothing ticked, Make the shows is held and says why', (await imp(page).locator('#ic-make').isDisabled()) && (await imp(page).locator('#ic-make').getAttribute('title')) === 'Tick at least one show.');
  await imp(page).getByRole('button', { name: 'Tick all', exact: true }).click();
  check('Tick all ticks the five that are new to the station', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '5 of 6 ticked');
  await item(page, 2).locator('input[type="checkbox"]').uncheck();
  await item(page, 3).locator('input[type="checkbox"]').uncheck();

  // Names.
  await item(page, 1).locator('input.name').fill('');
  check('an empty name is marked on its row and holds the import', (await text(item(page, 1).locator('.err'))) === 'Give the show a name.' && (await item(page, 1).locator('input.name').getAttribute('aria-invalid')) === 'true' && (await imp(page).locator('#ic-make').isDisabled()) && (await imp(page).locator('#ic-make').getAttribute('title')) === 'Put right the names marked first.');
  await shot(page, '13-import-name-problem');
  await item(page, 1).locator('input.name').fill('the folk hour');
  check('two ticked shows of one name are both allowed, and the second says how they are told apart', (await text(item(page, 1).locator('[data-same-name]'))) === 'Another ticked show is named the folk hour too. Both are made; their paths are told apart by a number. Rename one to tell them apart in the lists.' && (await item(page, 1).locator('.err').count()) === 0 && !(await imp(page).locator('#ic-make').isDisabled()));
  await item(page, 1).locator('input.name').fill('The The Kettle Book Club');

  // Esc: out of the field first, then the dialog asks before it throws the changes away.
  await page.keyboard.press('Escape');
  await sleep(150);
  check('Esc in a name leaves the field and keeps the dialog', (await imp(page).count()) === 1 && (await page.evaluate(() => document.activeElement?.tagName)) === 'DIALOG');
  await page.keyboard.press('Escape');
  await page.locator('dialog.wz.confirm').waitFor({ timeout: 3000 });
  check('a second Esc asks before discarding the ticks and names', (await text(page.locator('dialog.wz.confirm h2'))) === 'Discard this import?');
  await page.getByRole('button', { name: 'Keep going' }).click();
  await sleep(200);
  check('Keep going keeps them', (await page.locator('dialog.wz.confirm').count()) === 0 && (await item(page, 1).locator('input.name').inputValue()) === 'The The Kettle Book Club');

  // A refusal names a show by its place among those sent; it lands under that show's own row.
  await imp(page).locator('#ic-make').click();
  await item(page, 1).locator('.err').waitFor({ timeout: 5000 });
  check('the station’s refusal of one name lands under that show, and nothing is made', (await text(item(page, 1).locator('.err'))) === '“The The Kettle Book Club”: The name does not fit the card at its smallest size: shorten it.' && (await text(imp(page).locator('[data-testid="ic-refusal"]'))) === 'Nothing was made. See The The Kettle Book Club above.' && imports(station).length === 1 && !station.shows.some((s) => s.name === 'The Folk Hour') && (await focusId(page)) === 'ic-name-1', await text(item(page, 1).locator('.err')));
  await shot(page, '14-import-refused-name');
  await item(page, 1).locator('input.name').fill('The Kettle Book Club');
  check('typing in the name takes the refusal away', (await item(page, 1).locator('.err').count()) === 0 && (await imp(page).locator('[data-testid="ic-refusal"]').count()) === 0);

  // Make the shows.
  const weeks = calls(station, 'GET', '/api/v1/schedule/episodes').length;
  await imp(page).locator('#ic-make').click();
  await imp(page).locator('[data-testid="ic-made"]').waitFor({ timeout: 5000 });
  const sent = imports(station).at(-1).body;
  check('Make the shows sends the ticked shows only, renamed, each a name, a description and its rule as it came', sent.items.length === 3 && sent.items.map((i) => i.name).join('|') === 'The Folk Hour|The Kettle Book Club|Outside Broadcast' && sent.items.every((i) => Object.keys(i).sort().join() === 'description,name,rule') && JSON.stringify(sent.items[0].rule) === JSON.stringify(IMPORT_PREVIEW.items[0].rule) && JSON.stringify(sent.items[2].rule) === JSON.stringify(IMPORT_PREVIEW.items[4].rule) && sent.items[0].description === IMPORT_PREVIEW.items[0].description, sent.items.map((i) => i.name).join('|'));
  check('Done reports what was made', (await imp(page).getAttribute('data-part')) === '3' && (await text(imp(page).locator('[data-testid="ic-made"]'))) === '3 shows are made.' && (await imp(page).locator('.madelist li').count()) === 3 && (await text(imp(page).locator('.madelist li').first())).startsWith('The Folk Hour /the-folk-hour Tue, 20:00–22:00 · next Tue 17 Nov'), await text(imp(page).locator('.madelist li').first()));
  check('and the warnings: which show overlaps which', (await text(imp(page).locator('[data-testid="ic-warnings"]'))).includes('One of them overlaps another show.') && (await text(imp(page).locator('[data-testid="ic-warnings"] li'))).startsWith('The Folk Hour overlaps Firth Sessions, The Late Kettle and Six Nations Preview on '), await text(imp(page).locator('[data-testid="ic-warnings"]')));
  check('the week and the shows are read again', calls(station, 'GET', '/api/v1/schedule/episodes').length > weeks && station.shows.filter((s) => ['the-folk-hour', 'the-kettle-book-club', 'outside-broadcast'].includes(s.slug)).length === 3 && station.shows.find((s) => s.slug === 'the-folk-hour').step === 'solo');
  await inView(page, imp(page).getByRole('button', { name: /^Done/ }), 'the import’s Done');
  await shot(page, '15-import-done');

  await imp(page).getByRole('button', { name: 'Open The Folk Hour' }).click();
  await page.locator('[data-panel="calendar.show-dates"]').waitFor({ timeout: 5000 });
  await sleep(300);
  check('Open on a made show closes the dialog and opens that show at its Dates', (await imp(page).count()) === 0 && (await page.locator('[data-panel="calendar.show-dates"] h2').innerText()) === 'The Folk Hour');
  await shot(page, '16-import-opened-show');
  await ctx.close();
}

// ---- 6. 200 shows: the foot stays in the window ----
if (run('many')) {
  const station = kettle();
  const base = IMPORT_PREVIEW.items.filter((i) => i.rule);
  station.importPreview = { ...IMPORT_PREVIEW, events: 200, notes: [], ready: 200, items: Array.from({ length: 200 }, (_, n) => ({ ...base[n % base.length], name: `${base[n % base.length].name} ${n + 1}`, slug: `show-${n + 1}`, overlaps: [], already: null })) };
  const { ctx, page } = await open(station);
  await openFiles(page, 'Import a calendar…');
  await pick(page, 'two-hundred.ics', SAMPLE_ICS);
  await imp(page).locator('li.item').first().waitFor({ timeout: 8000 });
  check('200 proposed shows are listed', (await imp(page).locator('li.item').count()) === 200 && (await text(imp(page).locator('#ic-make'))) === 'Make 200 shows');
  await inView(page, imp(page).locator('#ic-make'), 'Make 200 shows');
  await inView(page, imp(page).getByRole('button', { name: 'Choose another file…' }), 'Choose another file…');
  const list = await imp(page).locator('.scroll').evaluate((el) => ({ scrolls: el.scrollHeight > el.clientHeight + 10, style: getComputedStyle(el).overflowY }));
  check('the list scrolls inside the dialog; the dialog itself does not', list.scrolls && list.style === 'auto' && (await imp(page).evaluate((el) => el.scrollHeight <= el.clientHeight + 1)));
  check('the count and Tick all stay in the head while the list scrolls', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '200 of 200 ticked');
  await item(page, 199).locator('input.name').focus();
  await sleep(200);
  await inView(page, item(page, 199).locator('input.name'), 'the last show’s name, once focused');
  await inView(page, imp(page).locator('#ic-make'), 'Make 200 shows, with the list at its end');
  await shot(page, '17-import-200');
  await imp(page).getByRole('button', { name: 'Untick all' }).click();
  check('Untick all unticks all 200', (await text(imp(page).locator('[data-testid="ic-count"]'))) === '0 of 200 ticked' && (await imp(page).locator('#ic-make').isDisabled()));
  // All 200 made: Done's list scrolls too, and its button stays in the window.
  await imp(page).getByRole('button', { name: 'Tick all', exact: true }).click();
  await imp(page).locator('#ic-make').click();
  await imp(page).locator('[data-testid="ic-made"]').waitFor({ timeout: 15000 });
  check('200 shows are made in one request', (await text(imp(page).locator('[data-testid="ic-made"]'))) === '200 shows are made.' && imports(station).length === 1 && imports(station)[0].body.items.length === 200 && (await imp(page).locator('.madelist li').count()) === 200);
  await inView(page, imp(page).getByRole('button', { name: /^Done/ }), 'Done, after 200 shows');
  check('and Done’s list scrolls inside the dialog', (await imp(page).locator('.scroll').evaluate((el) => el.scrollHeight > el.clientHeight + 10)) && (await imp(page).evaluate((el) => el.scrollHeight <= el.clientHeight + 1)));
  await shot(page, '18-import-200-done');
  await ctx.close();
}

// ---- 6b. A station's own calendar read back: every show is already there ----
if (run('own')) {
  const station = kettle();
  const drive = IMPORT_PREVIEW.items[6];
  const names = ['Kettle Breakfast', 'Kettle Drive', 'The Bothy Sessions', 'Budget Night Special', 'Saturday Night Kettle'];
  station.importPreview = { ...IMPORT_PREVIEW, events: 5, notes: [], ready: 5, items: names.map((name, n) => ({ ...drive, name, slug: `show-${n}-2`, overlaps: [], already: { slug: `show-${n}`, name } })) };
  const { ctx, page } = await open(station);
  await openFiles(page, 'Import a calendar…');
  await pick(page, 'schedule.ics', SAMPLE_ICS);
  await imp(page).locator('li.item').first().waitFor({ timeout: 5000 });
  check('a station’s own calendar: five shows, none ticked, none a clash', (await imp(page).locator('li.item').count()) === 5 && (await imp(page).locator('li.item input[type="checkbox"]:checked').count()) === 0 && (await imp(page).locator('[data-there]').count()) === 5 && (await imp(page).locator('[data-clash]').count()) === 0 && (await imp(page).locator('.nd li.r.clash').count()) === 0 && (await text(imp(page).locator('[data-testid="ic-count"]'))) === '0 of 5 ticked', `${await imp(page).locator('li.item').count()} rows, ${await imp(page).locator('li.item input[type="checkbox"]:checked').count()} ticked, ${await imp(page).locator('[data-there]').count()} already, ${await imp(page).locator('[data-clash]').count()} clash lines, ${await imp(page).locator('.nd li.r.clash').count()} clash rows, ${await text(imp(page).locator('[data-testid="ic-count"]'))}`);
  check('the file says there is nothing to make, and no clash is warned of', (await text(imp(page).locator('[data-testid="ic-there"]'))) === 'All 5 shows in this file are already on the station, with the same dates and times. There is nothing to make.' && (await imp(page).locator('[data-testid="ic-cost"]').count()) === 0, await text(imp(page).locator('[data-testid="ic-there"]')));
  check('Make the shows is held, and says why in the station’s case', (await imp(page).locator('#ic-make').isDisabled()) && (await imp(page).locator('#ic-make').getAttribute('title')) === 'Every show in this file is already on the station.' && (await imp(page).getByRole('button', { name: 'Tick all', exact: true }).isDisabled()), await imp(page).locator('#ic-make').getAttribute('title'));
  await inView(page, imp(page).locator('#ic-make'), 'Make the shows, held');
  await shot(page, '19-import-own-calendar');
  await item(page, 2).locator('input[type="checkbox"]').check();
  check('one may still be ticked on its own row, and is then warned of', (await text(imp(page).locator('#ic-make'))) === 'Make the show' && !(await imp(page).locator('#ic-make').isDisabled()) && (await text(imp(page).locator('[data-testid="ic-cost"]'))).startsWith('One ticked show clashes with shows the station has.') && imports(station).length === 0);
  await ctx.close();
}

// ---- 7. Light and Night ----
if (run('themes')) {
  for (const theme of ['light', 'night']) {
    const station = kettle();
    const { ctx, page } = await open(station, { theme });
    await openFiles(page, 'Subscribe…');
    await sub(page).waitFor({ timeout: 5000 });
    await shot(page, `20-subscribe-${theme}`);
    await page.keyboard.press('Escape');
    await openFiles(page, 'Import a calendar…');
    await pick(page, 'kettle-old-schedule.ics', SAMPLE_ICS);
    await imp(page).locator('li.item').first().waitFor({ timeout: 5000 });
    check(`${theme}: the import lists its shows, the one already on the station unticked`, (await imp(page).locator('li.item').count()) === 7 && (await imp(page).locator('li.item.there.off').count()) === 1);
    await shot(page, `21-import-shows-${theme}`);
    await ctx.close();
  }
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} passed`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
