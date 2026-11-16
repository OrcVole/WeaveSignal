// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the week's clashes and a show's dates (week-clashes.md, release 1.0.5) in a real
// browser against the stand-in control plane (station.mjs, with a week of clashes added), as
// shoot.mjs drives New show: the studio app's ordinary build served as static files, every
// request for /api and /health answered in this process, no stack and no ports but its own
// loopback one. It checks the fold in the real calendar library (where each block sits, which
// is open, what a press does), the Day view, the clash in the editor, the list of shows, and
// the Dates tab with Draft, Skip, Put back, Move… and Undo against what the studio sent, and an
// episode's own Move… (saved at once, with Undo) and the arrows beside every schedule time.
//
//   cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/newshow/week.mjs
//   WIDTH=2560 HEIGHT=1440 TEXT=1.2 … node tests/newshow/week.mjs      # and 3840 by 2160
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs). Screenshots go to
// OUT, never the repository. The clock is fixed at Friday 13 November 2026, 10:00 in
// Europe/London, and the week driven is the one after it, Monday 16 to Sunday 22 November.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Station, clashWeek, instant } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? '/tmp/weavesignal-week-shots';
const PORT = Number(process.env.PORT ?? 27914);
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

/** A window on a station, on the Schedule tab, in the week of Monday 16 November. */
async function open(station, { width = SIZE.width, height = SIZE.height, theme = 'dark', text = SIZE.text, week = true, reduced = false, dsf = 1 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dsf, timezoneId: 'Europe/London', reducedMotion: reduced ? 'reduce' : 'no-preference' });
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
    const [status, body] = station.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  // A presenter with no show of their own is offered the setup first; this drive is not about it.
  if (station.role === 'presenter') await page.getByRole('button', { name: 'Skip for now' }).click({ timeout: 5000 }).catch(() => {});
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await sleep(800);
  if (week) {
    await page.keyboard.press('ArrowRight');
    await sleep(900);
  }
  return { ctx, page };
}

const shot = async (page, name) => {
  await sleep(250);
  await page.screenshot({ path: join(OUT, `${name}-${TAG}.png`) });
  log('shot', name);
};
const run = (name) => !ONLY || ONLY.includes(name);

/** A block of the week by its show's name and its day (0 is Monday), as the grid drew it. */
const block = (page, key) => page.locator(`.ec .ec-event[data-key="${key}"]`);
const box = async (loc) => (await loc.count()) ? loc.first().boundingBox() : null;

/** The page's zoom: the text size times the window's factor. Boxes are in the window's pixels. */
const zoomOf = (page) => page.evaluate(() => Number(getComputedStyle(document.documentElement).zoom) || 1);
/** One rem in the page's own pixels: 16, or 20 in a window wider than 2,200 (base.css). */
const remOf = (page) => page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16);
const rangeText = (page) => page.locator('[data-panel="calendar.toolbar"] .range').innerText();
const toast = (page) => page.locator('[data-testid="schedule-toast"]');
const toastText = async (page) => ((await toast(page).count()) ? (await toast(page).innerText()).replace(/\s+/g, ' ').trim() : '');
const editor = (page) => page.locator('.schedule aside.editor');
const within = (a, b, tol = 1.5) => Math.abs(a - b) <= tol;
/** The tab has read the week again and a show's dates are read: nothing is on its way. */
const settled = async (page) => {
  await sleep(250);
  await page.waitForFunction(() => !document.querySelector('.toolbar .load[role="status"]') && !document.querySelector('.fields.dates[aria-busy]'), null, { timeout: 8000 }).catch(() => {});
  await sleep(150);
};
async function inView(page, loc, what) {
  const b = await box(loc);
  const vp = page.viewportSize();
  check(`${what} is inside the window (${vp.width}×${vp.height}, text ${SIZE.text})`, !!b && b.y >= 0 && b.x >= 0 && b.y + b.height <= vp.height + 0.5 && b.x + b.width <= vp.width + 0.5, b ? `at ${Math.round(b.x)},${Math.round(b.y)} to ${Math.round(b.x + b.width)},${Math.round(b.y + b.height)}` : 'not drawn');
}
/** Scroll the grid so that an hour of the day is near its top. */
const scrollTo = async (page, hour) => {
  await page.locator('.ec-main').evaluate((el, h) => (el.scrollTop = h * 48 - 24), hour);
  await sleep(200);
};
const K = {
  tea: 'ep:the-tea-break-1@2',
  schools: 'rule:schools-hour:2026-11-18@2',
  driveThu: 'ep:kettle-drive-4@3',
  budget: 'ep:budget-night-special-1@3',
  sport: 'ep:saturday-sport-1@5',
  fair: 'ep:record-fair-live-1@5',
  choice: 'rule:listeners-choice:2026-11-21@5',
  garden: 'ep:gardeners-corner-1@6',
  classics: 'ep:kettle-classics-1@6',
  bothyFri: 'ep:the-bothy-sessions-6@4',
  lateFri: 'rule:the-late-shift:2026-11-20@4',
  lateSat: 'rule:the-late-shift:2026-11-20@5',
};
const puts = (station, slug) => station.calls.filter((c) => c.method === 'PUT' && c.path === `/api/v1/shows/${slug}/rule`);
const patches = (station) => station.calls.filter((c) => c.method === 'PATCH');

// ---- 1. The fold, in the calendar library itself ----
if (run('fold')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station);
  const z = await zoomOf(page);
  const rem = await remOf(page);
  const strip = (1.5 * rem + 2) * z;
  await scrollTo(page, 13.5);

  const heads = await page.locator('.ec .dayhd').evaluateAll((els) => els.map((e) => e.innerText.replace(/\s+/g, ' ').trim()));
  check('each day’s header counts the shows that clash on it', /Mon 16 Nov 24 h/.test(heads[0]) && /Wed 18 Nov 2 clash/.test(heads[2]) && /Thu 19 Nov 2 clash/.test(heads[3]) && /Fri 20 Nov 2 clash/.test(heads[4]) && /Sat 21 Nov 3 clash/.test(heads[5]) && /Sun 22 Nov 2 clash/.test(heads[6]), heads.join(' | '));
  check('a header says the rest aloud', (await page.locator('.ec .dayhd[data-day="5"]').getAttribute('aria-label')) === 'Saturday 21 November, 3 shows clash, open the day' && (await page.locator('.ec .dayhd[data-day="2"]').getAttribute('title')) === 'Open Wednesday 18 November');

  // Where the library puts a clashing block: the adapter's own sizes win over the library's.
  const day = await box(page.locator('.ec-body .ec-day').nth(2));
  const tea = await box(block(page, K.tea));
  const schools = await box(block(page, K.schools));
  check('one block is open at the day’s width less the strip, and the other lies behind it at the day’s width', !!tea && !!schools && within(tea.x, day.x, 2) && within(schools.x, day.x, 2) && within(schools.width - tea.width, strip, 2) && schools.width > day.width - 8 * z, `day ${Math.round(day.width)}, open ${Math.round(tea?.width ?? 0)}, behind ${Math.round(schools?.width ?? 0)}, strip ${strip}`);
  const placed = await block(page, K.tea).evaluate((el) => ({ style: el.getAttribute('style'), left: getComputedStyle(el).insetInlineStart, z: getComputedStyle(el).zIndex }));
  const zs = await block(page, K.schools).evaluate((el) => getComputedStyle(el).zIndex);
  check('the library’s own placement is overridden without !important, and the open block is in front', placed.left === '0px' && !/important/.test(placed.style) && /inline-size: calc\(100% - var\(--ec-event-col-gap\) - 1\.5rem - 2px\)/.test(placed.style) && Number(placed.z) > Number(zs), `${placed.left}, z ${placed.z} over ${zs}`);
  check('the open block is the drafted episode, and the dotted date is a tab', (await block(page, K.tea).getAttribute('class')).includes('narrow') && (await block(page, K.schools).getAttribute('class')).split(' ').includes('tab'));
  check('a tab says the rest in its tooltip and to a screen reader', (await block(page, K.schools).getAttribute('title')) === 'Schools Hour, not drafted, 15:00 to 16:00, clashes with The Tea Break. Folded: press to bring it forward.' && (await block(page, K.schools).getAttribute('aria-label')).endsWith('folded: press Enter to bring it forward'), await block(page, K.schools).getAttribute('aria-label'));
  check('every block in a clash says so: a dotted date too, with the triangle leading its time', (await page.locator('.ec .ev.rule.conflict').count()) >= 3 && (await block(page, K.tea).locator('.t .cf').count()) === 1 && (await block(page, K.tea).getAttribute('title')).includes('clashes with Schools Hour'), `${await page.locator('.ec .ev.rule.conflict').count()} dotted in a clash`);
  const grounds = await page.locator('.ec .ev.rule').evaluateAll((els) => els.map((e) => getComputedStyle(e).backgroundColor));
  check('a dotted date’s ground is opaque', grounds.length > 0 && grounds.every((g) => !/rgba\(.*,\s*0\)|transparent/.test(g)), grounds[0]);
  check('every block carries its show’s colour', (await block(page, K.tea).evaluate((el) => getComputedStyle(el).borderLeftColor)) !== (await page.locator('.ec .ev.auto').first().evaluate((el) => getComputedStyle(el).borderLeftColor)));
  await shot(page, '01-week-folded');

  // Hover moves nothing and is quieter than selection.
  const before = JSON.stringify([await box(block(page, K.tea)), await box(block(page, K.schools))]);
  await page.mouse.move(schools.x + schools.width - 6 * z, schools.y + schools.height / 2);
  await sleep(250);
  check('hovering a tab moves nothing', JSON.stringify([await box(block(page, K.tea)), await box(block(page, K.schools))]) === before && (await page.locator('.ec-host').getAttribute('data-view')) === 'week');
  await page.mouse.move(tea.x + 20, tea.y + 20);
  await sleep(200);
  check('a hovered block has no accent line: only the selected one has', (await block(page, K.tea).evaluate((el) => getComputedStyle(el).outlineStyle)) === 'none');

  // Pressing a tab opens it, selects it and turns the editor to it.
  await page.mouse.click(schools.x + schools.width - 6 * z, schools.y + schools.height / 2);
  await block(page, K.schools).and(page.locator('.narrow')).waitFor({ timeout: 3000 }).catch(() => {});
  const swapped = await block(page, K.schools).evaluate((el) => el.getAnimations().length).catch(() => 0);
  await sleep(500);
  const tea2 = await box(block(page, K.tea));
  const schools2 = await box(block(page, K.schools));
  check('the swap of widths is animated on the block that opened', swapped > 0, `${swapped} animation`);
  check('pressing a tab brings it forward and folds the other', within(tea2.width - schools2.width, strip, 2) && (await block(page, K.schools).getAttribute('class')).includes('sel') && (await block(page, K.tea).getAttribute('class')).split(' ').includes('tab'), `open ${Math.round(schools2.width)}, behind ${Math.round(tea2.width)}`);
  check('and the editor turns to it, with what it clashes with', (await editor(page).locator('h2').innerText()).includes('Schools Hour · not drafted') && (await page.locator('[data-panel="calendar.clash"]').innerText()).replace(/\s+/g, ' ').includes('The Tea Break · Ep 1 · 15:00 to 16:00 · overlaps 1 h'), (await page.locator('[data-panel="calendar.clash"]').innerText()).replace(/\s+/g, ' '));
  check('a dotted date in a clash says what drafting it would do', (await page.locator('[data-panel="calendar.clash"]').innerText()).includes('This date is not drafted, so nothing of it airs yet. Draft it and it would clash with The Tea Break.'));
  check('the week never turns into the Day view by itself', (await page.locator('.ec-host').getAttribute('data-view')) === 'week' && (await page.locator('[data-panel="calendar.week"]').count()) === 1);
  await inView(page, page.locator('[data-panel="calendar.clash"]').getByRole('button', { name: 'Open the day' }), 'Open the day, in the clash');
  await shot(page, '02-dotted-date-open-with-its-clash');

  // Show it: the keyboard's way to a folded block.
  await page.locator('[data-panel="calendar.clash"]').getByRole('button', { name: 'Show The Tea Break' }).click();
  await sleep(500);
  check('Show it selects the other block: it opens in the week and the editor turns to it', (await block(page, K.tea).getAttribute('class')).includes('sel') && (await block(page, K.schools).getAttribute('class')).split(' ').includes('tab') && (await editor(page).locator('h2').innerText()).includes('Episode 1') && (await page.locator('[data-panel="calendar.clash"]').innerText()).replace(/\s+/g, ' ').includes('Schools Hour · not drafted · 15:00 to 16:00 · overlaps 1 h'));

  // ⏎ on a focused tab, and the focus stays on the block.
  await block(page, K.schools).focus();
  check('focus alone does not open a tab', (await block(page, K.schools).getAttribute('class')).split(' ').includes('tab'));
  await page.keyboard.press('Enter');
  await sleep(500);
  check('⏎ on a focused tab brings it forward, and the focus stays on it', (await block(page, K.schools).getAttribute('class')).includes('narrow') && (await page.evaluate(() => document.activeElement?.getAttribute('data-key'))) === K.schools, await page.evaluate(() => document.activeElement?.getAttribute('data-key')));

  // Thursday: a block covered later than its first hour keeps its words above the cover.
  await scrollTo(page, 15);
  const drive = await box(block(page, K.driveThu));
  const budget = await box(block(page, K.budget));
  const driveText = (await block(page, K.driveThu).innerText()).replace(/\s+/g, ' ');
  check('a block covered after its first hour keeps its name and words above the cover, and shows beside it', driveText.includes('Kettle Drive') && driveText.includes('CONFLICT') && within(drive.width - budget.width, strip, 2) && (await block(page, K.driveThu).getAttribute('class')).split(' ').includes('under') && (await block(page, K.driveThu).locator('.evb').evaluate((el) => el.getBoundingClientRect().bottom)) <= budget.y + 1, driveText);
  await page.mouse.click(budget.x + 30 * z, budget.y + 30 * z);
  await sleep(500);
  check('two episodes in a clash: the editor says who is on air by the schedule', (await page.locator('[data-panel="calendar.clash"]').innerText()).replace(/\s+/g, ' ').includes('Where two shows overlap, the one that starts later is on air by the schedule: its presenter is named to callers and its step shapes the studio. Here that is Budget Night Special, from 18:00.'), (await page.locator('[data-panel="calendar.clash"]').innerText()).replace(/\s+/g, ' '));
  await shot(page, '03-episode-in-a-clash');

  // Sunday: a show that starts half an hour before the one over it keeps its name there.
  await scrollTo(page, 13.5);
  check('a folded block’s name is written in the half hour left clear', (await block(page, K.garden).locator('.lbl').innerText()) === 'Gardeners’ Corner' && (await block(page, K.garden).getAttribute('class')).split(' ').includes('tab') && (await box(block(page, K.garden).locator('.lbl'))).y < (await box(block(page, K.classics))).y);

  // Friday into Saturday: a clash that crosses midnight folds for each day's part.
  check('a clash that crosses midnight folds on its own day only', (await block(page, K.lateFri).getAttribute('class')).split(' ').includes('tab') && !(await block(page, K.lateSat).getAttribute('class')).split(' ').includes('tab') && (await block(page, K.bothyFri).getAttribute('class')).includes('narrow'));

  // Saturday: three at one hour are one open block and a count, which opens the day.
  const count = page.locator('.ec .ev.tab.count');
  check('three shows at one hour: one open, and a tab with the count', (await count.count()) === 1 && (await count.innerText()).trim() === '+2' && (await count.getAttribute('title')) === '2 more shows at this time: Record Fair Live, Listeners’ Choice. Press to open the day.' && (await block(page, K.sport).count()) === 1 && (await block(page, K.fair).count()) === 0 && (await block(page, K.choice).count()) === 0, await count.getAttribute('title'));
  const cb = await box(count);
  check('the count is the strip’s width, the target size or more', within(cb.width, 1.5 * rem * z, 1.5) && cb.width >= 24 * z - 1 && cb.height >= 24 * z - 1, `${Math.round(cb.width)} by ${Math.round(cb.height)}`);
  const top = await page.locator('.ec-main').evaluate((el) => el.scrollTop);
  await count.click();
  await page.locator('[data-panel="calendar.day"]').waitFor();
  await sleep(600);
  const lanes = page.locator('.ec .ev.lane');
  const lb = (await lanes.evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).map((r) => ({ x: r.x, w: r.width, y: r.y })))).sort((a, b) => a.x - b.x);
  check('the count opens the day, with a lane for each show and nothing over anything', (await rangeText(page)) === 'Sat 21 November 2026' && lb.length === 3 && lb.every((l) => within(l.w, lb[0].w, 1) && within(l.y, lb[0].y, 1)) && lb[1].x >= lb[0].x + lb[0].w - 1 && lb[2].x >= lb[1].x + lb[1].w - 1 && (await page.locator('.ec .ev.tab').count()) === 0, lb.map((l) => `${Math.round(l.x)}+${Math.round(l.w)}`).join(' '));
  check('the Day view keeps the place in the day', within(await page.locator('.ec-main').evaluate((el) => el.scrollTop), top, 6), `${await page.locator('.ec-main').evaluate((el) => el.scrollTop)} against ${top}`);
  const stripEl = page.locator('[data-panel="calendar.day.strip"]');
  const days = await stripEl.locator('button').evaluateAll((els) => els.map((e) => `${e.innerText.replace(/\s+/g, ' ').trim()}${e.getAttribute('aria-pressed') === 'true' ? '*' : ''}`));
  check('the strip is the week’s seven days, the open day pressed, each day’s clashes counted', days.length === 7 && days[5] === 'Sat 21 3 clash*' && days[0] === 'Mon 16' && days[2] === 'Wed 18 2 clash', days.join(' | '));
  check('the focus goes to the open day’s button', await page.evaluate(() => document.activeElement?.getAttribute('aria-pressed') === 'true' && !!document.activeElement?.closest('.strip')));
  const laneText = (await lanes.first().innerText()).replace(/\s+/g, ' ');
  check('in a lane a block says CONFLICT and what it is over, and has no triangle in its time row', laneText.includes('CONFLICT') && laneText.includes('over 2 shows') && (await lanes.first().locator('.t .cf').count()) === 0, laneText);
  await shot(page, '04-day-three-lanes');

  // The strip and the toolbar's arrows move between the days of the week.
  await stripEl.locator('button').nth(3).click();
  await sleep(500);
  const thu = (await page.locator('.ec .ev.lane').evaluateAll((els) => els.map((e) => e.innerText.replace(/\s+/g, ' ')))).join(' | ');
  check('a day of the strip opens that day: two lanes, each naming the other with the overlap', (await rangeText(page)) === 'Thu 19 November 2026' && (await page.locator('.ec .ev.lane').count()) === 2 && thu.includes('over Budget Night Special · 1 h') && thu.includes('over Kettle Drive · 1 h') && (await page.evaluate(() => document.activeElement?.getAttribute('aria-pressed'))) === 'true', thu);
  await page.getByRole('button', { name: 'Next day' }).click();
  await sleep(400);
  check('the toolbar’s arrows move a day in the Day view', (await rangeText(page)) === 'Fri 20 November 2026' && (await page.getByRole('button', { name: 'Previous day' }).count()) === 1 && (await page.getByRole('button', { name: 'Next week' }).count()) === 0);
  await page.keyboard.press('t');
  await sleep(900);
  check('T is today, in the Day view too', (await rangeText(page)) === 'Fri 13 November 2026' && (await page.locator('.ec-host').getAttribute('data-view')) === 'day', await rangeText(page));
  // The keys (ADR-0283): ← → a day in the Day view, W the week, D the day the toolbar's Day
  // would choose (the selected block's day when it is in the week shown, else today).
  await page.keyboard.press('ArrowRight');
  await sleep(600);
  check('→ moves a day in the Day view', (await rangeText(page)) === 'Sat 14 November 2026', await rangeText(page));
  await page.keyboard.press('ArrowLeft');
  await sleep(600);
  await page.keyboard.press('w');
  await sleep(600);
  check('W is the week that holds the day', (await page.locator('.ec-host').getAttribute('data-view')) === 'week' && (await rangeText(page)).endsWith('15 November 2026'), await rangeText(page));
  await page.keyboard.press('d');
  await sleep(600);
  check('D opens today when the selected block is in another week', (await rangeText(page)) === 'Fri 13 November 2026' && (await page.locator('.ec-host').getAttribute('data-view')) === 'day', await rangeText(page));
  await page.keyboard.press('w');
  await sleep(400);
  await page.keyboard.press('ArrowRight');
  await sleep(900);
  await page.keyboard.press('d');
  await sleep(600);
  check('D opens the selected block’s day', (await rangeText(page)) === 'Thu 19 November 2026', await rangeText(page));
  await page.keyboard.press('ArrowRight');
  await sleep(900);
  check('and → then moves on to the next day', (await rangeText(page)) === 'Fri 20 November 2026', await rangeText(page));

  // Week: back to the week that holds the day, the selection kept.
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Week', exact: true }).click();
  await page.locator('[data-panel="calendar.week"]').waitFor();
  await sleep(500);
  check('Week returns to the week that holds the day, the selection kept, the focus on that day’s header', (await rangeText(page)) === '16 – 22 November 2026' && (await block(page, K.budget).getAttribute('class')).includes('sel') && (await page.evaluate(() => document.activeElement?.className === 'dayhd' && document.activeElement?.getAttribute('data-day'))) === '4', await page.evaluate(() => `${document.activeElement?.className} ${document.activeElement?.getAttribute('data-day')}`));

  // The other ways in: a day's header, Open the day, Day in the toolbar.
  await page.locator('.ec .dayhd[data-day="2"]').click();
  await sleep(500);
  check('a day’s header opens that day', (await rangeText(page)) === 'Wed 18 November 2026' && (await page.locator('.ec .ev.lane').count()) === 2);
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Week', exact: true }).click();
  await sleep(400);
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Day', exact: true }).click();
  await sleep(500);
  check('Day in the toolbar opens the selected block’s day', (await rangeText(page)) === 'Thu 19 November 2026', await rangeText(page));
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Week', exact: true }).click();
  await sleep(400);
  await scrollTo(page, 13.5);
  await block(page, K.tea).click({ position: { x: 20, y: 20 } });
  await sleep(400);
  await page.locator('[data-panel="calendar.clash"]').getByRole('button', { name: 'Open the day' }).click();
  await sleep(500);
  check('Open the day opens it with the block still selected', (await rangeText(page)) === 'Wed 18 November 2026' && (await page.locator('.ec .ev.lane.sel').count()) === 1 && (await editor(page).locator('h2').innerText()).includes('Episode 1'));
  await shot(page, '05-day-two-lanes-selected');
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Week', exact: true }).click();
  await sleep(400);

  // The editor column shows the selected block only while the grid draws it (the 1.0.5 test run:
  // an automation block pressed on Monday, then Wednesday opened by its header, left Monday's
  // log beside a day with nothing selected in it).
  await scrollTo(page, 9.5);
  const hatch = page.locator('.ec .ec-event.auto[data-key^="auto:0:"]').filter({ hasText: '10:00' }).first();
  await hatch.click({ position: { x: 30, y: 60 } });
  await sleep(400);
  check('an automation block pressed on purpose opens its log', (await editor(page).getAttribute('data-panel')) === 'calendar.log-editor' && (await page.locator('.ec .ev.auto.sel').count()) === 1);
  await page.locator('.ec .dayhd[data-day="2"]').click();
  await sleep(500);
  check('another day opened in the Day view has nothing selected, and the editor column lists the shows as the week does', (await rangeText(page)) === 'Wed 18 November 2026' && (await page.locator('.ec .ev.sel').count()) === 0 && (await editor(page).getAttribute('data-panel')) === 'calendar.shows', await editor(page).getAttribute('data-panel'));
  await shot(page, '05b-day-nothing-selected');
  await page.locator('[data-panel="calendar.day.strip"] button').nth(0).click();
  await sleep(500);
  check('on its own day the block is selected still, and its log is back', (await page.locator('.ec .ev.auto.sel').count()) === 1 && (await editor(page).getAttribute('data-panel')) === 'calendar.log-editor');
  await page.locator('[data-panel="calendar.day.strip"] button').nth(2).click();
  await sleep(400);
  await page.locator('.ec .ec-event.auto').first().click({ position: { x: 30, y: 30 } });
  await sleep(400);
  check('an automation block pressed in the Day view opens its log there', (await editor(page).getAttribute('data-panel')) === 'calendar.log-editor' && (await page.locator('.ec .ev.auto.sel').count()) === 1);
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Week', exact: true }).click();
  await sleep(400);

  // The toolbar, and the way back to the list of shows without leaving the week.
  if (SIZE.width === 1440 && SIZE.text === 1) {
    // What is not drawn is on no line (the calendar files' two buttons are a menu at this width).
    const tops = await page.locator('[data-panel="calendar.toolbar"] > *').evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width > 0).map((e) => Math.round(e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2)));
    check('the toolbar stays on one line at 1440 by 900, with Day in it', Math.max(...tops) - Math.min(...tops) < 12, tops.join(' '));
  }
  check('the legend says what a clash looks like', (await page.locator('.legend span[title]').getAttribute('title')) === 'Where shows clash, one is open and the others fold beside it' && (await page.locator('.legend').innerText()).includes('Automation fills the gaps'));
  await editor(page).getByRole('button', { name: '‹ Shows' }).click();
  await sleep(300);
  check('‹ Shows closes what is open and clears the selection, in the same week', (await page.locator('[data-panel="calendar.shows"]').count()) === 1 && (await page.locator('.ec .ev.sel').count()) === 0 && (await rangeText(page)) === '16 – 22 November 2026');
  await ctx.close();
}

// ---- 1a. Esc, and a press on the grid where no block is (the 1.0.5 test run) ----
// Once a block or a show was open in the editor column, the only ways out were another block,
// ‹ Shows, or a change of view. Esc now closes what the column holds and clears the selection,
// after a text field, a menu or a dialog has had the key, and never over changes not saved.
if (run('esc')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station);
  const z = await zoomOf(page);
  const panel = () => editor(page).getAttribute('data-panel');
  const selected = () => page.locator('.ec .ev.sel').count();
  const pick = async () => {
    await scrollTo(page, 13.5);
    await block(page, K.tea).click({ position: { x: 20, y: 20 } });
    await sleep(400);
  };
  await pick();
  check('a block pressed is selected, and the editor shows it', (await selected()) === 1 && (await panel()) === 'calendar.episode-editor');
  await shot(page, '01h-esc-before');
  await page.keyboard.press('Escape');
  await sleep(400);
  check('Esc clears the selection: nothing is selected and the editor column lists the shows, in the same week', (await selected()) === 0 && (await panel()) === 'calendar.shows' && (await rangeText(page)) === '16 – 22 November 2026' && (await page.locator('.ec-host').getAttribute('data-view')) === 'week');
  await shot(page, '01i-esc-cleared');
  await page.keyboard.press('Escape');
  await sleep(200);
  check('Esc with nothing open changes nothing', (await panel()) === 'calendar.shows' && (await rangeText(page)) === '16 – 22 November 2026');
  check('‹ Shows names the key', await (async () => { await pick(); return (await editor(page).getByRole('button', { name: '‹ Shows' }).getAttribute('title')) === 'Back to the list of shows (Esc)'; })());

  // A text field has the key first; then the editor's own changes hold the column.
  const title = editor(page).locator('input[type="text"]').first();
  await title.click();
  await page.keyboard.press('Escape');
  await sleep(300);
  check('in a text field the first Esc leaves the field, and the editor stays', (await panel()) === 'calendar.episode-editor' && (await selected()) === 1 && (await page.evaluate(() => document.activeElement?.tagName)) !== 'INPUT');
  await title.click();
  await page.keyboard.type(' at four');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await sleep(300);
  check('over changes that are not saved a second Esc closes nothing, and the tab says why', (await panel()) === 'calendar.episode-editor' && (await selected()) === 1 && (await toastText(page)) === 'Not closed. The episode has changes that are not saved. Save or discard them first.' && (await title.inputValue()).endsWith(' at four'), await toastText(page));
  await shot(page, '01j-esc-not-saved');
  await editor(page).getByRole('button', { name: 'Discard changes' }).click();
  await editor(page).locator('h2').first().focus();
  await page.keyboard.press('Escape');
  await sleep(500);
  check('discarded, Esc closes it, and the focus goes back to the block in the week', (await panel()) === 'calendar.shows' && (await selected()) === 0 && (await page.evaluate(() => document.activeElement?.getAttribute('data-key'))) === K.tea, await page.evaluate(() => `${document.activeElement?.tagName} ${document.activeElement?.getAttribute('data-key')}`));

  // The refusal is said once: leaving the tab and coming back does not say it again.
  await pick();
  await title.click();
  await page.keyboard.type(' again');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await sleep(300);
  const said = await toastText(page);
  await page.keyboard.press('Control+1');
  await sleep(400);
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await sleep(700);
  check('a refusal is said once: it is not said again when the tab is opened again', said.startsWith('Not closed.') && (await toastText(page)) === '' && (await panel()) === 'calendar.episode-editor', `then “${await toastText(page)}”`);
  await page.keyboard.press('Escape');
  await sleep(400);
  check('and with the draft gone with the tab, Esc closes the editor', (await panel()) === 'calendar.shows' && (await selected()) === 0);

  // Move… in the episode editor is the key's first: Esc cancels it, and the editor stays.
  await pick();
  await editor(page).getByRole('button', { name: 'Move…' }).click();
  await sleep(200);
  // The 1.0.5 click-through: one date-and-time box showed its date and cut its time off. Both
  // fields now show whole, inside the editor column, and hold the episode's own date and time.
  const fits = await editor(page).evaluate((ed) => {
    const col = ed.getBoundingClientRect();
    const inside = (el) => { const r = el?.getBoundingClientRect(); return !!r && r.width > 40 && r.left >= col.left - 1 && r.right <= col.right + 1; };
    const d = ed.querySelector('.row.move input.date'), t = ed.querySelector('.row.move input.time');
    const arrows = [...ed.querySelectorAll('.row.move .timefield .steps button')];
    const within = (el) => { const r = el.getBoundingClientRect(); return r.left >= col.left - 1 && r.right <= col.right + 1; };
    return { ok: inside(d) && inside(t) && arrows.length === 2 && arrows.every(within) && /^\d{4}-\d\d-\d\d$/.test(d.value) && /^\d\d:\d\d$/.test(t.value), d: d?.value, t: t?.value, dw: d?.getBoundingClientRect().width, tw: t?.getBoundingClientRect().width, arrows: arrows.length };
  });
  check('the episode’s Move… shows a date field and a time field with its two arrows, all whole inside the editor column, holding its date and time', fits.ok, JSON.stringify(fits));
  await shot(page, '05-episode-move');
  await editor(page).locator('input[type="date"]').focus();
  await page.keyboard.press('Escape');
  await sleep(300);
  check('Esc in the episode’s Move… cancels it and returns the focus to its button; the editor stays', (await editor(page).locator('input[type="date"]').count()) === 0 && (await panel()) === 'calendar.episode-editor' && (await selected()) === 1 && (await page.evaluate(() => document.activeElement?.textContent)) === 'Move…', await page.evaluate(() => `${document.activeElement?.tagName} ${document.activeElement?.textContent}`));
  await page.keyboard.press('Escape');
  await sleep(400);
  check('and the next Esc closes the editor', (await panel()) === 'calendar.shows' && (await selected()) === 0);

  // A show's own pane, opened from the list.
  await page.locator('[data-panel="calendar.shows"] [data-show="the-tea-break"]').click();
  await sleep(500);
  check('a show opened from the list is in the column', (await panel()) === 'calendar.show-dates');
  await editor(page).getByRole('tab', { name: 'Dates' }).focus();
  await page.keyboard.press('Escape');
  await sleep(500);
  check('Esc closes a show’s pane, and the focus goes back to its row in the list', (await panel()) === 'calendar.shows' && (await page.evaluate(() => document.activeElement?.getAttribute('data-show'))) === 'the-tea-break', await page.evaluate(() => `${document.activeElement?.tagName} ${document.activeElement?.getAttribute('data-show')}`));

  // A menu and a dialog take the key for themselves; the selection is behind them still.
  await pick();
  const more = page.locator('.files-menu .more');
  if (await more.isVisible()) {
    await more.click();
    await sleep(200);
    await page.keyboard.press('Escape');
    await sleep(300);
    check('an open menu takes the first Esc: it closes, and the selection stays', (await page.locator('#cal-files-menu[open]').count()) === 0 && (await selected()) === 1 && (await panel()) === 'calendar.episode-editor');
    await page.keyboard.press('Escape');
    await sleep(300);
    check('and a second Esc clears the selection', (await selected()) === 0 && (await panel()) === 'calendar.shows');
    await pick();
  }
  await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'New show', exact: true }).click();
  await page.locator('[data-panel="calendar.new-show"]').waitFor();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await sleep(400);
  check('a dialog takes its own Esc: New show closes, and the selection behind it stays', (await page.locator('[data-panel="calendar.new-show"]').count()) === 0 && (await selected()) === 1 && (await panel()) === 'calendar.episode-editor');

  // The grid where no block is. Automation fills every gap in time, so a press on those hours
  // opens their log, as before; what is left is the gap the library keeps beside a day's blocks.
  const gap = await page.evaluate(() => {
    const host = document.querySelector('.ec-host').getBoundingClientRect();
    for (let y = host.top + 140; y < host.bottom - 20; y += 7) for (let x = host.left + 70; x < host.right - 14; x += 1) if (document.elementFromPoint(x, y)?.classList.contains('ec-day')) return { x, y };
    return null;
  });
  check('there is a gap beside a day’s blocks to press', !!gap, JSON.stringify(gap));
  if (gap) {
    await page.mouse.click(gap.x, gap.y);
    await sleep(400);
    check('a press on the grid where no block is clears the selection', (await selected()) === 0 && (await panel()) === 'calendar.shows');
  }
  await pick();
  const main = await box(page.locator('.ec-main'));
  await page.mouse.click(main.x + 20 * z, main.y + main.height / 2);
  await sleep(300);
  check('a press on the time axis does not', (await selected()) === 1 && (await panel()) === 'calendar.episode-editor');
  await page.locator('.ec .dayhd[data-day="2"]').hover();
  await page.locator('.ec .ec-event.auto[data-key^="auto:2:"]').first().click({ position: { x: 30, y: 16 } });
  await sleep(400);
  check('and a press on the hours automation fills opens their log, as before', (await panel()) === 'calendar.log-editor' && (await page.locator('.ec .ev.auto.sel').count()) === 1);
  await page.keyboard.press('Escape');
  await sleep(300);
  check('Esc closes an automation block’s log too', (await panel()) === 'calendar.shows' && (await selected()) === 0);

  // The Day view: the same key, and the view stays.
  await pick();
  await page.keyboard.press('d');
  await sleep(600);
  check('in the Day view the selected block is in its lane', (await page.locator('.ec .ev.lane.sel').count()) === 1 && (await panel()) === 'calendar.episode-editor');
  await page.keyboard.press('Escape');
  await sleep(400);
  check('Esc clears it there, and the Day view stays', (await selected()) === 0 && (await panel()) === 'calendar.shows' && (await page.locator('.ec-host').getAttribute('data-view')) === 'day' && (await rangeText(page)) === 'Wed 18 November 2026');
  if (SIZE.width >= 2560) {
    await page.locator('.ec .ev.lane').first().click({ position: { x: 20, y: 12 } });
    await sleep(400);
    const beside = await page.evaluate(() => { const m = document.querySelector('.ec-main').getBoundingClientRect(); const d = document.querySelector('.ec-body .ec-day').getBoundingClientRect(); return { room: m.right - d.right, x: d.right + (m.right - d.right) / 2, y: m.top + m.height / 2 }; });
    if (beside.room > 60 * z) {
      await page.mouse.click(beside.x, beside.y);
      await sleep(400);
      check('in a window wider than the day, a press on the plane beside it clears the selection', (await selected()) === 0 && (await panel()) === 'calendar.shows', `room ${Math.round(beside.room)}`);
    }
  }
  await ctx.close();
}

// ---- 1b. The tab: a folded block reads as a thing to press (the 1.0.5 test run) ----
// "We can't see an open or folded block": an hour's show over the middle of a three-hour one
// left the long show as a thin colour line at the day's right edge. Each stretch a block lies
// behind another is now a tab: the strip's width, a ground filled with the show's colour, a
// chevron, the name down it where it is tall enough, and a tooltip; and the open block says
// what it is over even when it is an hour tall.
const K2 = { lunch: 'ep:the-long-lunch-1@1', steeped: 'ep:steeped-1@1', morning: 'ep:the-morning-kettle-1@0', archive: 'rule:the-archive-hour:2026-11-16@0', requests: 'ep:sunday-requests-1@6', evensong: 'ep:evensong-1@6', play: 'ep:the-sunday-play-1@6', mix: 'rule:monday-mix:2026-11-16@0' };
/** Kettle FM's week of clashes with two two-hour shows at one time on Monday, for a tall tab;
 *  three in a chain on Sunday evening (a long show with a short one at each end), where the long
 *  show is covered and itself leaves the strip to the one behind it; and on Monday evening an
 *  hour's show, a second from its half hour and a long dotted date behind both, where a count
 *  stands on part of a tab. */
function tabWeek() {
  const station = clashWeek(new Station());
  const weekly = (weekdays, start, minutes, o = {}) => ({ starts_on: '2026-09-07', start, minutes, repeat: { kind: 'weekly', every: 1, weekdays }, end: { kind: 'never' }, skip: [], moves: [], ...o });
  station.add({ slug: 'the-morning-kettle', name: 'The Morning Kettle', colour_slot: 4, presenter: 'Tara Buchanan', hosts: ['Tara Buchanan'], rule: weekly([1], 600, 180) });
  station.add({ slug: 'the-archive-hour', name: 'The Archive Hour', colour_slot: 1, presenter: '', rule: weekly([1], 600, 120, { starts_on: '2026-11-16' }) });
  station.draft('the-morning-kettle', '2026-11-16');
  const once = (on, start, minutes) => ({ starts_on: on, start, minutes, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] });
  station.add({ slug: 'sunday-requests', name: 'Sunday Requests', colour_slot: 7, presenter: 'Callum Murray', hosts: ['Callum Murray'], rule: once('2026-11-22', 1080, 180) });
  station.add({ slug: 'evensong', name: 'Evensong', colour_slot: 11, presenter: '', rule: once('2026-11-22', 1080, 60) });
  station.add({ slug: 'the-sunday-play', name: 'The Sunday Play', colour_slot: 5, presenter: '', rule: once('2026-11-22', 1200, 60) });
  for (const s of ['sunday-requests', 'evensong', 'the-sunday-play']) station.draft(s, '2026-11-22');
  station.add({ slug: 'quiz-night', name: 'Quiz Night', colour_slot: 8, presenter: 'Magnus Henderson', hosts: ['Magnus Henderson'], rule: once('2026-11-16', 1140, 60) });
  station.add({ slug: 'the-late-kettle', name: 'The Late Kettle', colour_slot: 12, presenter: 'Shona Campbell', hosts: ['Shona Campbell'], rule: once('2026-11-16', 1170, 60) });
  station.add({ slug: 'monday-mix', name: 'Monday Mix', colour_slot: 9, presenter: '', rule: weekly([1], 1140, 180, { starts_on: '2026-11-16' }) });
  station.draft('quiz-night', '2026-11-16');
  station.draft('the-late-kettle', '2026-11-16');
  return station;
}
/** What a tab looks like, read from the page: its colours as numbers, and the contrast of its ink on its ground. */
const tabLook = (loc) =>
  loc.evaluate((el) => {
    const probe = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
    const rgb = (c) => { probe.clearRect(0, 0, 1, 1); probe.fillStyle = '#000'; probe.fillStyle = c; probe.fillRect(0, 0, 1, 1); return [...probe.getImageData(0, 0, 1, 1).data]; };
    const lum = ([r, g, b]) => [r, g, b].map((v) => v / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)).reduce((n, v, i) => n + v * [0.2126, 0.7152, 0.0722][i], 0);
    const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    const cs = getComputedStyle(el);
    const block = getComputedStyle(el.closest('.ec-event'));
    const ground = rgb(cs.backgroundColor);
    // How much of the show's colour the ground holds over the surface: read back from the three
    // colours as painted, on the channel where the colour and the surface differ most.
    const [band, surface] = [rgb(cs.borderLeftColor), rgb(getComputedStyle(document.documentElement).getPropertyValue('--surface').trim())];
    const ch = [0, 1, 2].sort((a, b) => Math.abs(band[b] - surface[b]) - Math.abs(band[a] - surface[a]))[0];
    const share = (ground[ch] - surface[ch]) / (band[ch] - surface[ch]);
    return { ground, share, stands: ratio(ground, surface), inkColour: cs.color, opaque: ground[3] === 255, ink: ratio(rgb(cs.color), ground), band: rgb(cs.borderLeftColor).join(), bandWidth: cs.borderLeftWidth, blockBand: rgb(block.borderLeftColor).join(), differs: ratio(ground, rgb(block.backgroundColor)), cursor: cs.cursor };
  });
if (run('tab')) {
  const station = tabWeek();
  const { ctx, page } = await open(station);
  const z = await zoomOf(page);
  const rem = await remOf(page);
  await scrollTo(page, 10);
  const tabOf = (key) => block(page, key).locator('.ftab');
  const cls = async (key) => (await block(page, key).getAttribute('class')).split(' ');

  // Wednesday: an hour's dotted date behind an episode. The tab is the strip's width and an hour tall.
  const st = await box(tabOf(K.schools));
  const tea = await box(block(page, K.tea));
  check('a folded block shows as a tab the strip’s width: the target size (24 px at the usual size) or more each way', !!st && within(st.width, 1.5 * rem * z, 1.5) && st.width >= 24 * z - 1 && st.height >= 24 * z - 1, st ? `${Math.round(st.width)} by ${Math.round(st.height)} at zoom ${z}` : 'no tab');
  check('the tab stands beside the open block, clear of it: nothing is drawn through anything', !!st && st.x >= tea.x + tea.width + 1 * z && within(st.y, tea.y, 2 * z) && within(st.height, tea.height, 3 * z), st ? `open to ${Math.round(tea.x + tea.width)}, tab from ${Math.round(st.x)}` : '');
  const look = await tabLook(tabOf(K.schools));
  check('its ground is filled with the show’s colour, with the colour itself as its band', look.opaque && look.differs >= 1.15 && look.band === look.blockBand && parseFloat(look.bandWidth) >= 3.5 && look.cursor === 'pointer', JSON.stringify(look));
  check('its chevron and name are readable on that ground (4.5:1 or more)', look.ink >= 4.5, `${look.ink.toFixed(2)}:1`);
  const chev = await box(tabOf(K.schools).locator('svg.chev'));
  check('it carries a chevron, inside the tab', !!chev && chev.width >= 10 * z && chev.x >= st.x && chev.x + chev.width <= st.x + st.width + 0.5 && chev.y >= st.y && chev.y + chev.height <= st.y + st.height);
  check('an hour’s tab has the chevron alone: the name would be cut to a few letters', (await tabOf(K.schools).getAttribute('class')).split(' ').includes('bare') && !(await tabOf(K.schools).locator('.fn').isVisible()));
  check('the tab’s tooltip says which show it is and that a press brings it forward', (await tabOf(K.schools).getAttribute('title')) === 'Schools Hour, not drafted, 15:00 to 16:00, clashes with The Tea Break. Folded: press to bring it forward.');
  check('a dotted date’s tab keeps its dashes', (await tabOf(K.schools).evaluate((el) => getComputedStyle(el).borderTopStyle)) === 'dashed' && (await tabOf(K2.lunch).evaluate((el) => getComputedStyle(el).borderTopStyle)) === 'solid');

  // Monday: two hours behind another show. The name is written down the tab, inside it.
  const tall = tabOf(K2.archive);
  const tb = await box(tall);
  const fn = await box(tall.locator('.fn'));
  check('a tab of an hour and a half or more has the show’s name down it, inside the tab', !!tb && !!fn && within(tb.height, 96 * z, 3 * z) && !(await tall.getAttribute('class')).split(' ').includes('bare') && (await tall.locator('.fn').innerText()) === 'The Archive Hour' && (await tall.locator('.fn').evaluate((el) => getComputedStyle(el).writingMode)) === 'vertical-rl' && fn.height > fn.width && fn.x >= tb.x && fn.x + fn.width <= tb.x + tb.width + 0.5 && fn.y >= tb.y && fn.y + fn.height <= tb.y + tb.height + 0.5, fn ? `tab ${Math.round(tb.width)} by ${Math.round(tb.height)}, name ${Math.round(fn.width)} by ${Math.round(fn.height)}` : 'no name');

  // Tuesday, the test run's own shape: an hour's show over the middle of a three-hour one.
  const steeped = await box(block(page, K2.steeped));
  const lt = await box(tabOf(K2.lunch));
  check('an hour over the middle of a long show: the long show keeps its words above and has a tab beside the hour', (await cls(K2.lunch)).includes('under') && (await cls(K2.steeped)).includes('narrow') && (await tabOf(K2.lunch).count()) === 1 && !!lt && lt.width >= 24 * z - 1 && within(lt.y, steeped.y, 2 * z) && within(lt.height, 48 * z, 3 * z) && lt.x >= steeped.x + steeped.width + 1 * z && (await block(page, K2.lunch).locator('.evb > b').innerText()) === 'The Long Lunch', lt ? `tab ${Math.round(lt.width)} by ${Math.round(lt.height)}` : 'no tab');
  const lbl = await box(block(page, K2.lunch).locator('.lbl'));
  check('and its name again where it comes out below the hour, under nothing', !!lbl && (await block(page, K2.lunch).locator('.lbl').innerText()) === 'The Long Lunch' && lbl.y >= steeped.y + steeped.height - 1);
  check('its tab says it is folded there', (await tabOf(K2.lunch).getAttribute('title')) === 'The Long Lunch, live, 12:00 to 15:00, clashes with Steeped. Folded here: press to bring it forward.' && (await block(page, K2.lunch).getAttribute('aria-label')).endsWith('partly folded: press Enter to bring it forward'), await block(page, K2.lunch).getAttribute('aria-label'));
  // The open block, an hour tall: its time with the triangle, its name, and what it is over.
  const over = block(page, K2.steeped).locator('.over');
  const ob = await box(over);
  const lines = await block(page, K2.steeped).evaluate((el) => ['.t', '.evb > b', '.over'].map((q) => { const r = el.querySelector(q).getBoundingClientRect(); return [r.top, r.bottom]; }));
  check('an open block an hour tall says what it is over, on a line of its own inside the block', (await cls(K2.steeped)).includes('short') && !!ob && ob.height >= 11 * z && ob.y + ob.height <= steeped.y + steeped.height + 0.5 && (await over.innerText()).trim() === 'over The Long Lunch' && lines[0][1] <= lines[1][0] + 0.5 && lines[1][1] <= lines[2][0] + 0.5 && (await block(page, K2.steeped).locator('.t .cf').count()) === 1, ob ? `over at ${Math.round(ob.y)} to ${Math.round(ob.y + ob.height)} of ${Math.round(steeped.y)} to ${Math.round(steeped.y + steeped.height)}; lines ${lines.map((l) => l.map(Math.round).join('–')).join(' ')}` : 'not drawn');
  const shown = await over.evaluate((el) => { const r = document.createRange(); r.selectNodeContents(el); return { text: el.clientWidth, all: el.scrollWidth, square: getComputedStyle(el.querySelector('i')).backgroundColor }; });
  check('with a square of that show’s colour, the colour of its tab', shown.square === (await tabOf(K2.lunch).evaluate((el) => getComputedStyle(el).borderLeftColor)) && shown.text >= 60, JSON.stringify(shown));
  // How much of the line is in view is the window's to say: said here, not judged (a name is cut with an ellipsis where the day is narrow).
  log(`  the hour’s “over” line shows ${Math.min(shown.text, shown.all)} of its ${shown.all} px at ${TAG}${shown.all > shown.text ? ' (cut with an ellipsis)' : ' (in full)'}`);
  await shot(page, '01b-tabs');

  // Hover and focus move nothing; a press on the tab itself brings the block forward.
  const still = JSON.stringify([await box(block(page, K2.steeped)), await box(block(page, K2.lunch)), lt]);
  await page.mouse.move(lt.x + lt.width / 2, lt.y + lt.height / 2);
  await sleep(250);
  check('pointing at a tab rings it and moves nothing', JSON.stringify([await box(block(page, K2.steeped)), await box(block(page, K2.lunch)), await box(tabOf(K2.lunch))]) === still && (await tabOf(K2.lunch).evaluate((el) => getComputedStyle(el).boxShadow)) !== 'none');
  await shot(page, '01c-tab-hovered');
  await page.mouse.down();
  await page.mouse.up();
  await settled(page);
  await sleep(300);
  const s2 = await box(tabOf(K2.steeped));
  const lunch2 = await box(block(page, K2.lunch));
  check('a press on the tab brings its block forward, selected, and the hour over it folds to a tab in its place', (await cls(K2.lunch)).includes('sel') && (await cls(K2.lunch)).includes('narrow') && (await tabOf(K2.lunch).count()) === 0 && (await cls(K2.steeped)).includes('tab') && !!s2 && s2.width >= 24 * z - 1 && within(s2.height, 48 * z, 3 * z) && s2.x >= lunch2.x + lunch2.width + 1 * z && (await editor(page).innerText()).includes('The Long Lunch'), s2 ? `tab ${Math.round(s2.width)} by ${Math.round(s2.height)}` : 'no tab');
  check('the block now open says what it is over, and the folded hour’s tab its own tooltip', (await block(page, K2.lunch).locator('.over').innerText()).trim() === 'over Steeped' && (await tabOf(K2.steeped).getAttribute('title')) === 'Steeped, live, 13:00 to 14:00, clashes with The Long Lunch. Folded: press to bring it forward.');
  await shot(page, '01d-tab-pressed');
  await page.mouse.click(s2.x + s2.width / 2, s2.y + s2.height / 2);
  await settled(page);
  await sleep(300);
  check('and a press on that tab brings the hour back', (await cls(K2.steeped)).includes('sel') && (await cls(K2.lunch)).includes('under') && (await tabOf(K2.lunch).count()) === 1);

  // Sunday evening, three in a chain: the long show is covered at its end and is itself the open
  // block's width (the hour at its start lies behind it), so its tab stands out in the strip.
  await scrollTo(page, 15);
  const play = await box(block(page, K2.play));
  const rt = await box(tabOf(K2.requests));
  const et = await box(tabOf(K2.evensong));
  const sunday = await box(page.locator('.ec-body .ec-day').nth(6));
  check('three in a chain: the covered long show’s tab stands in the strip beside the block over it, not under it', (await cls(K2.requests)).includes('under') && (await cls(K2.requests)).includes('narrow') && (await cls(K2.requests)).includes('spill') && !!rt && rt.width >= 24 * z - 1 && rt.x >= play.x + play.width + 1 * z && rt.x + rt.width <= sunday.x + sunday.width + 1 && within(rt.y, play.y, 2 * z) && within(rt.height, 48 * z, 3 * z), rt ? `open to ${Math.round(play.x + play.width)}, tab ${Math.round(rt.x)} to ${Math.round(rt.x + rt.width)}, day to ${Math.round(sunday.x + sunday.width)}` : 'no tab');
  check('and the hour behind it has its own tab, at its own time, in the same strip', (await cls(K2.evensong)).includes('tab') && !!et && within(et.x, rt.x, 2 * z) && et.y + et.height <= rt.y + 1 && (await block(page, K2.requests).locator('.evb > b').innerText()) === 'Sunday Requests');
  await shot(page, '01f-tabs-chain');
  await page.mouse.click(rt.x + rt.width / 2, rt.y + rt.height / 2);
  await settled(page);
  await sleep(300);
  check('pressing the tab that stands out brings the long show forward, and both hours are tabs beside it', (await cls(K2.requests)).includes('sel') && (await cls(K2.play)).includes('tab') && (await cls(K2.evensong)).includes('tab') && (await tabOf(K2.play).count()) === 1 && (await tabOf(K2.evensong).count()) === 1);

  // Monday evening: a count stands where three share a time, and a tab gives way to it there.
  await scrollTo(page, 16);
  const cnt = await box(page.locator('.ec .ev.tab.count').first());
  const mixTabs = await tabOf(K2.mix).evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return { y: r.top, b: r.bottom, x: r.left }; }));
  check('where a count stands on the strip the tab gives way to it: a tab above the count and one below, none under it', (await page.locator('.ec .ev.tab.count').count()) === 2 && !!cnt && mixTabs.length === 2 && mixTabs[0].b <= cnt.y + 1.5 * z && mixTabs[1].y >= cnt.y + cnt.height - 1.5 * z && within(mixTabs[0].x, cnt.x, 2 * z), `count ${Math.round(cnt?.y ?? 0)} to ${Math.round((cnt?.y ?? 0) + (cnt?.height ?? 0))}; tabs ${mixTabs.map((t) => `${Math.round(t.y)}–${Math.round(t.b)}`).join(' ')}`);
  await shot(page, '01g-tabs-count');
  // Everywhere in the week: nothing is drawn over a tab. What is at each tab's middle is the tab.
  for (const hour of [9, 13.5, 16]) {
    await scrollTo(page, hour);
    const hidden = await page.locator('.ec .ftab').evaluateAll((els) => { const main = document.querySelector('.ec-main').getBoundingClientRect(); const head = document.querySelector('.ec-header').getBoundingClientRect().bottom; return els.map((e) => { const r = e.getBoundingClientRect(); const [x, y] = [r.left + r.width / 2, r.top + r.height / 2]; if (y < head + 2 || y > main.bottom - 2) return null; const at = document.elementFromPoint(x, y); return at && e.contains(at) ? null : `${e.closest('.ec-event').dataset.key} under ${at?.closest('.ec-event')?.dataset.key ?? at?.className}`; }).filter(Boolean); });
    check(`nothing is drawn over any tab (the week from ${hour}:00)`, hidden.length === 0, hidden.join('; '));
  }

  // The count is a filled tab too.
  const count = page.locator('.ec .ev.tab.count').last();
  await scrollTo(page, 13.5);
  const cl = await count.evaluate((el) => ({ image: getComputedStyle(el).backgroundImage, cursor: getComputedStyle(el).cursor }));
  check('the count stands on its shows’ colours side by side', /linear-gradient/.test(cl.image) && (cl.image.match(/color-mix|rgb|color\(/g) ?? []).length >= 2 && cl.cursor === 'pointer' && (await count.innerText()).trim() === '+2', cl.image.slice(0, 120));
  await ctx.close();

  // Every tab's ink on its ground, in each theme (the show colours differ in each).
  for (const theme of ['dark', 'light', 'night']) {
    const { ctx, page } = await open(tabWeek(), { theme });
    await scrollTo(page, 10);
    const tabs = page.locator('.ec .ftab');
    const n = await tabs.count();
    const looks = [];
    for (let i = 0; i < n; i++) looks.push(await tabLook(tabs.nth(i)));
    check(`${theme}: every tab is filled, banded in its show’s colour and readable`, n >= 6 && looks.every((l) => l.opaque && l.ink >= 4.5 && l.differs >= 1.1 && l.band === l.blockBand), `${n} tabs, ink ${Math.min(...looks.map((l) => l.ink)).toFixed(2)}:1 at least, ground ${Math.min(...looks.map((l) => l.differs)).toFixed(2)}:1 from its block at least`);
    // The ground reads as the show's colour (the 1.0.5 test run found 40 % of it too faint): 60 %
    // of it over the surface, 50 % in Night, whose colours are lighter. Aired tabs are dimmed on purpose.
    const want = theme === 'night' ? 0.5 : 0.6;
    const live = [];
    for (let i = 0; i < n; i++) if (!(await tabs.nth(i).evaluate((el) => el.closest('.ec-event').classList.contains('past')))) live.push(looks[i]);
    check(`${theme}: a tab’s ground holds ${want * 100} % of its show’s colour`, live.length >= 6 && live.every((l) => within(l.share, want, 0.03)), `${live.length} tabs, ${Math.min(...live.map((l) => l.share)).toFixed(2)} to ${Math.max(...live.map((l) => l.share)).toFixed(2)}; ${Math.min(...live.map((l) => l.stands)).toFixed(2)}:1 from the surface at least`);
    const plus = page.locator('.ec .ev.tab.count .plus').first();
    check(`${theme}: the count’s figure is in the tabs’ ink`, (await plus.count()) === 1 && (await plus.evaluate((el) => getComputedStyle(el).color)) === live[0].inkColour, live[0].inkColour);
    await shot(page, `01e-tabs-${theme}`);
    await ctx.close();
  }
}

// ---- 2. Dragging, with the fold in place ----
if (run('drag')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station);
  const z = await zoomOf(page);
  await scrollTo(page, 13.5);
  // A tab does not drag: the press brings it forward.
  const schools = await box(block(page, K.schools));
  await page.mouse.move(schools.x + schools.width - 6 * z, schools.y + schools.height / 2);
  await page.mouse.down();
  await page.mouse.move(schools.x + schools.width - 6 * z, schools.y + schools.height / 2 + 60 * z, { steps: 6 });
  await page.mouse.up();
  await settled(page);
  check('a tab does not drag: the first press brings it forward', patches(station).length === 0 && (await block(page, K.schools).getAttribute('class')).includes('narrow'));
  // An episode dragged over another show: allowed for a producer, and said, with Undo.
  const drive = await box(block(page, 'ep:kettle-drive-3@2'));
  await page.mouse.move(drive.x + 30 * z, drive.y + 40 * z);
  await page.mouse.down();
  await page.mouse.move(drive.x + 30 * z, drive.y + 40 * z - 24 * z, { steps: 5 });
  await page.mouse.move(drive.x + 30 * z, drive.y + 40 * z - 48 * z, { steps: 5 });
  await sleep(150);
  await page.mouse.up();
  await toast(page).waitFor({ timeout: 5000 }).catch(() => {});
  await settled(page);
  const moved = patches(station).at(-1);
  check('a block dragged with the fold in place is moved by what the library says', moved?.path === '/api/v1/shows/kettle-drive/series/1/episodes/3' && moved.body.air_at.startsWith('2026-11-18T15:00') && moved.body.minutes === 180, JSON.stringify(moved?.body));
  check('a drop that makes a clash is allowed, and the line says so with Undo', (await toastText(page)) === 'Moved. It now clashes with The Tea Break and Schools Hour on Wednesday. Undo', await toastText(page));
  check('and the day’s header counts three', (await page.locator('.ec .dayhd[data-day="2"]').innerText()).includes('3 clash'));
  await shot(page, '06-dropped-into-a-clash');
  await toast(page).getByRole('button', { name: 'Undo' }).click();
  await settled(page);
  check('Undo puts the episode back', patches(station).at(-1).body.air_at.startsWith('2026-11-18T16:00') && (await page.locator('.ec .dayhd[data-day="2"]').innerText()).includes('2 clash'), JSON.stringify(patches(station).at(-1).body));

  // The open block of a clash, dragged to another day: the block the library drags is in front
  // of everything while it moves, and what it reports is the block's own new time.
  await scrollTo(page, 15);
  const budget = await box(block(page, K.budget));
  const tue = await box(page.locator('.ec-body .ec-day').nth(1));
  await page.mouse.move(budget.x + 30 * z, budget.y + 30 * z);
  await page.mouse.down();
  await page.mouse.move(budget.x + 30 * z, budget.y + 40 * z, { steps: 3 });
  await page.mouse.move(tue.x + 40 * z, budget.y + 30 * z, { steps: 8 });
  await sleep(200);
  const ghost = await page.evaluate(() => {
    const els = [...document.querySelectorAll('.ec .ec-event')];
    const top = Math.max(...els.filter((e) => !e.classList.contains('ec-preview')).map((e) => Number(getComputedStyle(e).zIndex) || 0));
    const p = document.querySelector('.ec .ec-event.ec-preview');
    return p ? { z: Number(getComputedStyle(p).zIndex), top, classes: p.className } : null;
  });
  check('while a block of a fold is dragged, what moves is in front of every other block', !!ghost && ghost.z > ghost.top, JSON.stringify(ghost));
  await shot(page, '06b-dragging-the-open-block');
  await page.mouse.up();
  await settled(page);
  const dropped = patches(station).at(-1);
  // Tuesday had two in a clash already (the hour over the long lunch); the special lands on Kettle Drive there: four.
  check('the open block of a clash dragged to another day is moved there, and the clash it left is gone', dropped?.path === '/api/v1/shows/budget-night-special/series/1/episodes/1' && dropped.body.air_at.startsWith('2026-11-17T18:00') && dropped.body.minutes === 90 && !(await page.locator('.ec .dayhd[data-day="3"]').innerText()).includes('clash') && (await page.locator('.ec .dayhd[data-day="1"]').innerText()).includes('4 clash'), JSON.stringify(dropped?.body));
  await ctx.close();

  // Under reduced motion the swap takes no time.
  {
    const st = clashWeek(new Station());
    const { ctx, page } = await open(st, { reduced: true });
    await scrollTo(page, 13.5);
    const tab = await box(block(page, K.schools));
    await page.mouse.click(tab.x + tab.width - 6 * z, tab.y + tab.height / 2);
    await block(page, K.schools).and(page.locator('.narrow')).waitFor({ timeout: 3000 }).catch(() => {});
    check('under reduced motion the swap is not animated', (await block(page, K.schools).evaluate((el) => el.getAnimations().length)) === 0 && (await block(page, K.schools).getAttribute('class')).includes('narrow'));
    await ctx.close();
  }
}

// ---- 3. Room for lanes: nothing folds when each show has 10 rem ----
if (run('lanes') && SIZE.width === 1440 && SIZE.text === 1) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station, { width: 3840, height: 1080 });
  await scrollTo(page, 13.5);
  const rem = Number(await page.locator('.ec-host').getAttribute('data-day-rem'));
  const wed = await page.locator(`.ec .ec-event[data-key="${K.tea}"], .ec .ec-event[data-key="${K.schools}"]`).evaluateAll((els) => els.map((e) => ({ lane: e.classList.contains('lane'), x: e.getBoundingClientRect().x, w: e.getBoundingClientRect().width })));
  check('with 10 rem or more for each lane, two clashing shows stand side by side in the week and nothing of them folds', rem >= 20 && wed.length === 2 && wed.every((b) => b.lane) && Math.abs(wed[0].x - wed[1].x) >= wed[0].w - 1 && (await page.locator('.ec .ev.tab:not(.count)').count()) === 0, `a day is ${rem} rem`);
  check('three at one hour need 30 rem: under that they still fold, with the count', rem >= 30 ? (await page.locator('.ec .ev.tab.count').count()) === 0 : (await page.locator('.ec .ev.tab.count').count()) === 1 && !(await block(page, K.sport).getAttribute('class')).split(' ').includes('lane'), `a day is ${rem} rem`);
  await page.screenshot({ path: join(OUT, '07-week-lanes-3840x1080.png') });
  log('shot', '07-week-lanes');
  // The Day view in a very wide window: the day is at most 80 rem beside the axis.
  await page.locator('.ec .dayhd[data-day="2"]').click();
  await page.locator('[data-panel="calendar.day"]').waitFor();
  await sleep(500);
  const wide = await page.evaluate(() => ({ day: document.querySelector('.ec-body .ec-day').getBoundingClientRect().width, grid: document.querySelector('.ec-main').getBoundingClientRect().width, rem: parseFloat(getComputedStyle(document.documentElement).fontSize), z: Number(getComputedStyle(document.documentElement).zoom) || 1 }));
  check('in a very wide window the Day view’s day is at most 80 rem, and the rest is left to the plane', Math.abs(wide.day - 80 * wide.rem * wide.z) <= 2 && wide.grid > wide.day + 100, `${Math.round(wide.day)} px of ${Math.round(wide.grid)}`);
  await page.screenshot({ path: join(OUT, '08-day-80rem-3840x1080.png') });
  await ctx.close();
}

// ---- 4. The list of shows and a show's dates ----
if (run('dates')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station, { week: false });
  const shows = page.locator('[data-panel="calendar.shows"]');
  const names = await shows.locator('.showrow b').allInnerTexts();
  check('with nothing open the editor column lists every show with dates to come, by name', (await shows.locator('h2').innerText()) === 'Shows' && names[0] === 'The Bothy Sessions' && names[1] === 'Budget Night Special' && names.includes('St Andrew’s Weekend') && names.indexOf('The Late Shift') < names.indexOf('Listeners’ Choice') && (await shows.locator('.head .when').innerText()) === `${names.length} with dates to come · 1 with none yet · 1 finished`, `${names.length}: ${names.slice(0, 4).join(', ')}…`);
  check('a row says the rule in short and the next date', (await shows.locator('.showrow').first().innerText()).replace(/\s+/g, ' ') === 'The Bothy Sessions Wed and Fri, 21:00–23:00 · next today', (await shows.locator('.showrow').first().innerText()).replace(/\s+/g, ' '));
  check('the shows that draw nothing in the week are still at its foot, and above twelve a field finds one', (await shows.locator('[data-panel="calendar.unscheduled"]').innerText()).includes('Northern Lights') && (await shows.getByPlaceholder('Find a show').count()) === 1);
  await shows.getByPlaceholder('Find a show').fill('kettle');
  check('Find a show narrows the list', (await shows.locator('.showrow b').allInnerTexts()).join(', ') === 'Kettle Breakfast, Kettle Classics, Kettle Drive, Saturday Night Kettle', (await shows.locator('.showrow b').allInnerTexts()).join(', '));
  await shows.getByPlaceholder('Find a show').fill('zzz');
  check('and says when no show has that in its name', (await shows.innerText()).includes('No show has that in its name.'));
  await shows.getByPlaceholder('Find a show').fill('');
  await shot(page, '10-shows');

  // The rows are one stop: ↓ moves, ⏎ opens the show at Dates.
  await shows.locator('.showrow').first().focus();
  await page.keyboard.press('ArrowDown');
  check('↓ moves along the rows, without moving the week', (await page.evaluate(() => document.activeElement?.querySelector('b')?.textContent)) === 'Budget Night Special' && (await rangeText(page)) === '9 – 15 November 2026');
  await page.keyboard.press('Home');
  await page.keyboard.press('Enter');
  const dates = page.locator('[data-panel="calendar.show-dates"]');
  await dates.locator('.nd.dl li.r').first().waitFor({ timeout: 5000 });
  const head = (await dates.locator('.sum').innerText()).replace(/\s+/g, ' ');
  check('⏎ opens the show at Dates: when it airs, in the station’s sentence, with its last date and its counts', (await dates.locator('h2').innerText()) === 'The Bothy Sessions' && (await dates.locator('[role="tab"][aria-selected="true"]').innerText()) === 'Dates' && head.includes('Every week on Wednesday and Friday, 21:00 to 23:00, until 26 February 2027. Not on 9 December.') && head.includes('The last is Friday 26 February 2027.') && head.includes('30 to come: 3 drafted, 27 not drafted. 1 skipped. 1 clashes with another show.'), head);
  const row = (on) => dates.locator(`li.r[data-row="${on}"]`);
  const rowText = async (on) => (await row(on).innerText()).replace(/\s+/g, ' ').trim();
  const all = await dates.locator('.nd.dl.coming > li.r').evaluateAll((els) => els.map((e) => `${e.getAttribute('data-row')}:${e.getAttribute('data-kind')}`));
  check('the rows are the dates to come in order: drafted, not drafted, moved, skipped', all.join(' ') === '2026-11-13:drafted 2026-11-18:drafted 2026-11-20:drafted 2026-11-25:open 2026-11-27:open 2026-12-02:open 2026-12-04:open 2026-12-09:skipped 2026-12-11:open 2026-12-16:open 2026-12-18:open', all.join(' '));
  check('a drafted row says so, with its title; the next date to draft offers Draft, Skip and Move…', (await rowText('2026-11-20')) === 'Ep 6 Fri 20 Nov 21:00–23:00 Open › drafted Fiddle night Move…' && (await rowText('2026-11-25')) === 'Ep 7 Wed 25 Nov 21:00–23:00 Open › not drafted Draft Skip Move…' && (await rowText('2026-12-02')) === 'Ep 9 Wed 2 Dec 21:00–23:00 Open › not drafted Skip Move…', `${await rowText('2026-11-20')} | ${await rowText('2026-11-25')}`);
  check('a clash is a third line that names the other show; a moved date says where it was; a skipped one stays, struck through', (await rowText('2026-11-27')) === 'Ep 8 Fri 27 Nov 21:00–23:00 Open › not drafted CONFLICT Skip Move… St Andrew’s Weekend, 22:00 to 23:30 · overlaps 1 h' && (await rowText('2026-12-04')) === 'Ep 10 Sat 5 Dec 17:00–19:00 Open › not drafted MOVED · WAS FRI 4 DEC Back to the rule' && (await rowText('2026-12-09')) === '– Wed 9 Dec 21:00–23:00 SKIPPED Put back' && (await row('2026-12-09').locator('.on').evaluate((el) => getComputedStyle(el).textDecorationLine)) === 'line-through', `${await rowText('2026-11-27')} | ${await rowText('2026-12-04')} | ${await rowText('2026-12-09')}`);
  check('a row is said aloud in full', (await row('2026-11-25').getAttribute('aria-label')) === 'Episode 7, Wednesday 25 November, 21:00 to 23:00, not drafted. Space shows it in the week.', await row('2026-11-25').getAttribute('aria-label'));
  const foot = (await editor(page).locator('.foot').innerText()).replace(/\s+/g, ' ');
  check('the foot drafts the next, or all that are left, and says changes are saved as they are made', foot.includes('Draft the next · Ep 7, Wed 25 Nov') && foot.includes('Draft all 27') && foot.includes('Changes here are saved as you make them.'), foot);
  await inView(page, editor(page).getByRole('button', { name: /Draft the next/ }), 'Draft the next, at the foot of a show’s dates');
  await shot(page, '11-show-dates');

  // Pressing a row shows it in the week and keeps the list.
  await row('2026-11-18').locator('.on').click();
  await settled(page);
  check('pressing a row moves the week to it and selects its block; the list stays, the row marked', (await rangeText(page)) === '16 – 22 November 2026' && (await block(page, 'ep:the-bothy-sessions-5@2').getAttribute('class')).includes('sel') && (await dates.count()) === 1 && (await row('2026-11-18').getAttribute('aria-current')) === 'true');
  const b5 = await box(block(page, 'ep:the-bothy-sessions-5@2'));
  const main = await box(page.locator('.ec-main'));
  check('and the grid scrolls to the block', !!b5 && b5.y >= main.y && b5.y + b5.height <= main.y + main.height + 1, `${Math.round(b5?.y ?? 0)}–${Math.round((b5?.y ?? 0) + (b5?.height ?? 0))} in ${Math.round(main.y)}–${Math.round(main.y + main.height)}`);
  await settled(page);
  check('a drafted episode’s clash is known once its block is in the week on screen', (await rowText('2026-11-20')).includes('CONFLICT') && (await rowText('2026-11-20')).includes('The Late Shift, 22:30 to 00:30 · overlaps 30 min') && (await dates.locator('.sum').innerText()).includes('2 clash with another show.'), await rowText('2026-11-20'));
  await shot(page, '12-a-row-shown-in-the-week');
  await row('2026-11-20').focus();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press(' ');
  await settled(page);
  check('↓ moves along the rows and Space shows one in the week: a dotted date in the week after', (await rangeText(page)) === '23 – 29 November 2026' && (await block(page, 'rule:the-bothy-sessions:2026-11-25@2').getAttribute('class')).includes('sel') && (await page.evaluate(() => document.activeElement?.getAttribute('data-row'))) === '2026-11-25');

  // Skip, and Undo. Each change reads the rule first and is saved as it is made.
  let reads = station.calls.filter((c) => c.method === 'GET' && c.path === '/api/v1/shows/the-bothy-sessions').length;
  await row('2026-12-02').getByRole('button', { name: 'Skip' }).click();
  await toast(page).waitFor();
  await settled(page);
  let put = puts(station, 'the-bothy-sessions').at(-1);
  check('Skip reads the rule and saves it with the date skipped, drafted episodes left', put?.body.rule.skip.join() === '2026-12-02,2026-12-09' && put.body.drafted === 'leave' && station.calls.filter((c) => c.method === 'GET' && c.path === '/api/v1/shows/the-bothy-sessions').length > reads, JSON.stringify(put?.body.rule.skip));
  check('the line says so with Undo, the row stays struck through with Put back, and the focus stays on the row', (await toastText(page)) === 'Skipped. The Bothy Sessions will not air on Wednesday 2 December. Undo' && (await rowText('2026-12-02')) === '– Wed 2 Dec 21:00–23:00 SKIPPED Put back' && (await page.evaluate(() => document.activeElement?.getAttribute('data-row'))) === '2026-12-02' && (await dates.locator('.sum').innerText()).includes('2 skipped.'), `${await toastText(page)} | ${await rowText('2026-12-02')}`);
  await shot(page, '13-skipped-with-undo');
  await toast(page).getByRole('button', { name: 'Undo' }).click();
  await settled(page);
  check('Undo puts the date back', station.shows.find((s) => s.slug === 'the-bothy-sessions').rule.skip.join() === '2026-12-09' && (await rowText('2026-12-02')).includes('not drafted'), `${station.shows.find((s) => s.slug === 'the-bothy-sessions').rule.skip.join()} | ${await rowText('2026-12-02')} | ${await toastText(page)}`);

  // Put back, and Undo.
  await row('2026-12-09').getByRole('button', { name: 'Put back' }).click();
  await settled(page);
  check('Put back takes the date out of the skipped ones, and says so', station.shows.find((s) => s.slug === 'the-bothy-sessions').rule.skip.length === 0 && (await toastText(page)) === 'Put back. The Bothy Sessions airs on Wednesday 9 December. Undo' && (await rowText('2026-12-09')).includes('not drafted'), await toastText(page));
  await toast(page).getByRole('button', { name: 'Undo' }).click();
  await settled(page);
  check('and Undo skips it again', station.shows.find((s) => s.slug === 'the-bothy-sessions').rule.skip.join() === '2026-12-09' && (await rowText('2026-12-09')).includes('SKIPPED'));

  // Move…: a date and a time, Esc cancels, the neighbours bound it.
  await row('2026-12-11').getByRole('button', { name: 'Move…' }).click();
  const mv = dates.locator('li.mv');
  await mv.waitFor();
  check('Move… opens the row as a date and a time, and says between which dates it must stay', (await page.locator('#sd-mv-date').inputValue()) === '2026-12-11' && (await page.locator('#sd-mv-time').inputValue()) === '21:00' && (await mv.innerText()).includes('Keep it between 5 December and 16 December, so the episodes stay in order.') && (await page.evaluate(() => document.activeElement?.id)) === 'sd-mv-date', (await mv.innerText()).replace(/\s+/g, ' '));
  await shot(page, '14-move-open');
  await page.keyboard.press('Escape');
  await sleep(200);
  check('Esc cancels Move… and returns the focus to the row', (await mv.count()) === 0 && (await page.evaluate(() => document.activeElement?.getAttribute('data-row'))) === '2026-12-11' && (await dates.count()) === 1);
  await row('2026-12-11').getByRole('button', { name: 'Move…' }).click();
  await page.locator('#sd-mv-date').fill('2026-12-12');
  await page.locator('#sd-mv-time').fill('17:30');
  await mv.getByRole('button', { name: 'Move', exact: true }).click();
  await toast(page).waitFor();
  await settled(page);
  put = puts(station, 'the-bothy-sessions').at(-1);
  check('Move saves the rule with the date moved', JSON.stringify(put?.body.rule.moves.find((m) => m.from === '2026-12-11')) === '{"from":"2026-12-11","to":"2026-12-12T17:30:00","minutes":null}' && put.body.drafted === 'leave', JSON.stringify(put?.body.rule.moves));
  check('the line says where it is now, and the row says MOVED with Back to the rule', (await toastText(page)) === 'Moved. Episode 11 is on Saturday 12 December, 17:30 to 19:30. Undo' && (await rowText('2026-12-11')) === 'Ep 11 Sat 12 Dec 17:30–19:30 Open › not drafted MOVED · WAS FRI 11 DEC Back to the rule', `${await toastText(page)} | ${await rowText('2026-12-11')}`);
  await row('2026-12-11').getByRole('button', { name: 'Back to the rule' }).click();
  await settled(page);
  check('Back to the rule takes the move out again', !station.shows.find((s) => s.slug === 'the-bothy-sessions').rule.moves.some((m) => m.from === '2026-12-11') && (await rowText('2026-12-11')).startsWith('Ep 11 Fri 11 Dec 21:00–23:00'), await rowText('2026-12-11'));

  // Move… on a drafted episode is the episode's own time.
  await row('2026-11-18').getByRole('button', { name: 'Move…' }).click();
  await page.locator('#sd-mv-time').fill('20:30');
  await mv.getByRole('button', { name: 'Move', exact: true }).click();
  await toast(page).waitFor();
  await settled(page);
  const patch = patches(station).at(-1);
  check('Move… on a drafted episode sets the episode’s own time, and its row says MOVED', patch?.path === '/api/v1/shows/the-bothy-sessions/series/1/episodes/5' && patch.body.air_at.startsWith('2026-11-18T20:30') && (await toastText(page)) === 'Moved. Episode 5 is on Wednesday 18 November, 20:30 to 22:30. Undo' && (await rowText('2026-11-18')).includes('MOVED · WAS 21:00'), `${JSON.stringify(patch?.body)} | ${await rowText('2026-11-18')}`);
  await toast(page).getByRole('button', { name: 'Undo' }).click();
  await settled(page);
  check('and Undo gives it its time back', patches(station).at(-1).body.air_at.startsWith('2026-11-18T21:00') && !(await rowText('2026-11-18')).includes('MOVED'));

  // Draft: the next date, in date order; the line leads to the episode.
  const eps = station.episodes.length;
  await row('2026-11-25').getByRole('button', { name: 'Draft', exact: true }).click();
  await toast(page).waitFor();
  await settled(page);
  check('Draft drafts the next date, and Draft moves to the one after', station.episodes.length === eps + 1 && station.episodes.at(-1).occurrence === '2026-11-25' && (await rowText('2026-11-25')) === 'Ep 7 Wed 25 Nov 21:00–23:00 Open › drafted Move…' && (await rowText('2026-11-27')).includes('Draft Skip Move…') && (await toastText(page)) === 'Drafted. Episode 7, Wednesday 25 November. Open it', `${await toastText(page)} | ${await rowText('2026-11-25')}`);
  await toast(page).getByRole('button', { name: 'Open it' }).click();
  await editor(page).locator('h2').filter({ hasText: 'Episode 7' }).waitFor({ timeout: 5000 });
  const crumbs = (await editor(page).locator('.crumbs').innerText()).replace(/\s+/g, ' ');
  check('Open it opens the episode, and its crumbs lead back: ‹ Shows, then the show', crumbs === '‹ Shows › The Bothy Sessions › Series 1 › Episode 7' && (await rangeText(page)) === '23 – 29 November 2026', crumbs);
  check('the episode’s date field has All dates beside Move…', (await editor(page).getByRole('button', { name: 'All dates' }).count()) === 1);
  await shot(page, '15-episode-with-the-way-back');
  await editor(page).locator('.crumbs').getByRole('button', { name: 'The Bothy Sessions' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor();
  check('the show’s name in the crumbs leads to its dates', (await dates.locator('[role="tab"][aria-selected="true"]').innerText()) === 'Dates');

  // Open on a row; Show on the level switch; a dotted date's pane.
  await row('2026-11-27').getByRole('button', { name: /^Open/ }).click();
  await editor(page).locator('h2').filter({ hasText: 'not drafted' }).waitFor({ timeout: 5000 });
  check('Open on a row that is not drafted opens the dotted date’s pane, with All its dates and its clash', (await editor(page).getByRole('button', { name: 'All its dates' }).count()) === 1 && (await page.locator('[data-panel="calendar.clash"]').innerText()).includes('St Andrew’s Weekend') && (await editor(page).locator('.caption').innerText()).includes('A skipped date is put back from the show’s Dates'));
  await editor(page).getByRole('button', { name: 'All its dates' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor();
  await row('2026-11-25').getByRole('button', { name: /^Open/ }).click();
  await editor(page).locator('h2').filter({ hasText: 'Episode 7' }).waitFor({ timeout: 5000 });
  await editor(page).getByRole('tab', { name: 'Show' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor();
  check('Show on an episode’s level switch opens the show at Dates', (await dates.locator('[role="tab"][aria-selected="true"]').innerText()) === 'Dates');

  // Show more dates, and what has aired.
  const asked = station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview').length;
  check('what follows the rows says how many more there are, and to when', (await dates.locator('.after').innerText()).replace(/\s+/g, ' ') === 'Show more dates and 20 more, to Friday 26 February 2027', (await dates.locator('.after').innerText()).replace(/\s+/g, ' '));
  await dates.getByRole('button', { name: 'Show more dates' }).click();
  await sleep(300);
  const last = station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview').at(-1);
  check('Show more dates lists ten more of what the station already sent: the stored rule is sent once, only to be read, for every date at once', station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview').length === asked && last.body.limit === 60 && last.body.show === 'the-bothy-sessions' && last.body.series === 1 && (await dates.locator('.nd.dl.coming > li.r').count()) === 21, `limit ${last.body.limit}, ${await dates.locator('.nd.dl.coming > li.r').count()} rows`);
  const aired = dates.locator('details.aired');
  check('Aired is a closed group with its count', (await aired.locator('summary').innerText()).replace(/\s+/g, ' ') === 'Aired · 3' && !(await aired.evaluate((el) => el.open)));
  await aired.locator('summary').click();
  const airedRows = await aired.locator('li.r').allInnerTexts();
  check('open, it lists what has aired, newest first', airedRows.length === 3 && airedRows[0].replace(/\s+/g, ' ').startsWith('Ep 3 Wed 11 Nov 2026 21:00–23:00') && airedRows[2].replace(/\s+/g, ' ').startsWith('Ep 1 Wed 4 Nov 2026'), airedRows.map((r) => r.replace(/\s+/g, ' ')).join(' | '));
  await aired.locator('li.r').last().locator('.on').click();
  await settled(page);
  check('and a row shows it in its week', (await rangeText(page)) === '2 – 8 November 2026' && (await page.locator('.ec .ev.sel.past').count()) === 1);

  // Details is the show editor as it was; Dates is where the pane opens.
  await editor(page).getByRole('tab', { name: 'Details' }).click();
  await editor(page).getByLabel('Show name').waitFor();
  check('Details is the show editor as it was', (await editor(page).getByLabel('Show name').inputValue()) === 'The Bothy Sessions' && (await editor(page).getByRole('button', { name: 'Save show' }).count()) === 1 && (await page.locator('aside.editor[data-panel="calendar.episode-editor"]').count()) === 1);
  await shot(page, '16-show-details');
  await editor(page).getByRole('tab', { name: 'Dates' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor();

  // Draft all that are left.
  const left = (await editor(page).locator('.foot').innerText()).match(/Draft all (\d+)/)?.[1];
  await editor(page).getByRole('button', { name: /Draft all/ }).click();
  await toast(page).waitFor();
  await settled(page);
  const count = station.calls.filter((c) => c.method === 'POST' && c.path === '/api/v1/shows/the-bothy-sessions/series/1/episodes').at(-1);
  check('Draft all drafts every date that is left, in one ask', left === '26' && count?.body.count === 26 && (await toastText(page)).startsWith('Drafted. Episodes 8 to 33, the last on Friday 26 February.') && (await dates.locator('.sum').innerText()).includes('30 to come: 30 drafted, 0 not drafted.'), `${left} | ${await toastText(page)}`);

  // The other doors: a show with no dates, a finished one.
  await editor(page).getByRole('button', { name: '‹ Shows' }).click();
  await shows.locator('[data-panel="calendar.unscheduled"]').getByRole('button', { name: 'Northern Lights' }).click();
  await dates.locator('.sum').waitFor();
  check('a show with no dates opens at Dates, which says so and offers Give it dates…', (await dates.locator('.sum').innerText()).replace(/\s+/g, ' ').includes('No dates yet. Give it dates and they are listed here.') && (await dates.getByRole('button', { name: 'Give it dates…' }).count()) === 1);
  await editor(page).getByRole('button', { name: '‹ Shows' }).click();
  await shows.locator('[data-panel="calendar.unscheduled"] summary').click();
  await shows.locator('[data-panel="calendar.unscheduled"]').getByRole('button', { name: 'Harvest Home Ceilidh' }).click();
  await dates.locator('.sum').waitFor();
  check('a finished show says Finished, with Give it new dates…', (await dates.locator('.sum').innerText()).replace(/\s+/g, ' ').includes('Finished: was once, on 31 October.') && (await dates.getByRole('button', { name: 'Give it new dates…' }).count()) === 1, (await dates.locator('.sum').innerText()).replace(/\s+/g, ' '));
  await ctx.close();
}

// ---- 4a. An episode's Move…, saved at once; the arrows beside a time (the 1.0.5 click-through) ----
// Move… used to put the new time in the editor's draft, and the move was kept only by Save
// episode at the foot of the column. Now Move saves the time at once, as a show's dates do,
// the tab's line offers Undo, and whatever else is typed in the editor stays a draft.
if (run('move')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station);
  const z = await zoomOf(page);
  const DRIVE = 'ep:kettle-drive-1@0';
  const path = '/api/v1/shows/kettle-drive/series/1/episodes/1';
  const ep = () => station.episodes.find((e) => e.id === 'kettle-drive-1');
  const when = async () => (await editor(page).locator('.head .when .num').innerText()).trim();
  const field = editor(page).locator('[data-field="date-and-time"]');
  const mrow = field.locator('.row.move');
  const date = mrow.locator('input.date');
  const time = mrow.locator('input.time');
  const length = mrow.getByLabel('Length in minutes');
  const later = mrow.getByRole('button', { name: '15 minutes later' });
  const earlier = mrow.getByRole('button', { name: '15 minutes earlier' });
  const title = editor(page).getByLabel('Title');
  const focused = () => page.evaluate(() => { const a = document.activeElement; return a ? (a.getAttribute('aria-label') ?? a.textContent?.trim() ?? a.tagName) : ''; });
  const openMove = async () => {
    await field.getByRole('button', { name: 'Move…' }).click();
    await mrow.waitFor();
  };
  const move = async () => {
    await mrow.getByRole('button', { name: 'Move', exact: true }).click();
    await toast(page).waitFor({ timeout: 5000 });
    await settled(page);
  };
  const undo = async () => {
    await toast(page).getByRole('button', { name: 'Undo' }).click();
    await settled(page);
  };

  await scrollTo(page, 15);
  await block(page, DRIVE).click({ position: { x: 20, y: 20 } });
  await editor(page).locator('h2').filter({ hasText: 'Episode 1' }).waitFor({ timeout: 5000 });
  await title.fill('Monday’s drive');
  check('the episode is open, on its rule, with a title typed and not saved', (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00' && (await field.locator('.lab .src').innerText()).trim() === 'from Series 1' && (await editor(page).getByRole('button', { name: 'Save episode' }).isEnabled()));
  check('the date field says a move is saved as it is made', (await field.locator('.hint').innerText()).replace(/\s+/g, ' ').endsWith('Europe/London. A move is saved as you make it.'), await field.locator('.hint').innerText());

  // The arrows beside the time.
  await openMove();
  check('Move… opens with the episode’s date, time and length, and the focus in the date', (await date.inputValue()) === '2026-11-16' && (await time.inputValue()) === '16:00' && (await length.inputValue()) === '180' && (await focused()) === 'New date, station time', `${await date.inputValue()} ${await time.inputValue()} ${await length.inputValue()} | ${await focused()}`);
  check('the row’s buttons are Move, the primary one, and Cancel: nothing is called Set', (await mrow.getByRole('button', { name: 'Move', exact: true }).getAttribute('class')).includes('primary') && (await mrow.getByRole('button', { name: 'Cancel' }).count()) === 1 && (await mrow.getByRole('button', { name: 'Set', exact: true }).count()) === 0);
  const sizes = await mrow.locator('.timefield .steps button').evaluateAll((els) => els.map((el) => { const r = el.getBoundingClientRect(), i = el.closest('.timefield').querySelector('input').getBoundingClientRect(); return { w: r.width, h: r.height, ih: i.height, top: r.top - i.top, gap: r.left - i.right, tab: el.tabIndex, type: el.type, name: el.getAttribute('aria-label'), tip: el.title }; }));
  check('the time has two arrows, real buttons with their names, each 24 px by 24 px or more, as tall as the field and joined to it', sizes.length === 2 && sizes.every((b) => b.w / z >= 23.9 && b.h / z >= 23.9 && within(b.h, b.ih, 1) && within(b.top, 0, 1) && b.type === 'button' && b.tab === -1 && b.tip === b.name) && sizes[0].name === '15 minutes earlier' && sizes[1].name === '15 minutes later' && sizes[0].gap <= 1, JSON.stringify(sizes));
  await shot(page, '17a-episode-move-idle');
  await later.click();
  check('a press on the later arrow steps the time a quarter of an hour, and the focus is in the time field', (await time.inputValue()) === '16:15' && (await focused()) === 'New time, station time', `${await time.inputValue()} | ${await focused()}`);
  await shot(page, '17b-episode-move-stepped');
  await earlier.click();
  await earlier.click();
  check('the earlier arrow steps back', (await time.inputValue()) === '15:45');
  await time.fill('16:07');
  await later.click();
  const up = await time.inputValue();
  await time.fill('16:07');
  await earlier.click();
  check('from an odd minute a press goes to the quarter that way: 16:07 to 16:15 later, to 16:00 earlier', up === '16:15' && (await time.inputValue()) === '16:00', `${up} | ${await time.inputValue()}`);
  await time.fill('23:50');
  await later.click();
  const over = `${await date.inputValue()} ${await time.inputValue()}`;
  await earlier.click();
  check('past midnight the date goes with the time: 23:50 on the 16th steps to 00:00 on the 17th, and back to 23:45 on the 16th', over === '2026-11-17 00:00' && `${await date.inputValue()} ${await time.inputValue()}` === '2026-11-16 23:45', `${over} | ${await date.inputValue()} ${await time.inputValue()}`);
  // A time field is three stops in Chromium (hour, minute, and none for a 24-hour clock's
  // am or pm) or one in WebKitGTK; either way Tab reaches the length without an arrow between.
  const stops = [];
  await time.focus();
  for (let i = 0; i < 4 && (await focused()) !== 'Length in minutes'; i++) {
    await page.keyboard.press('Tab');
    stops.push(await focused());
  }
  check('Tab passes over the arrows: after the time comes the length', stops.at(-1) === 'Length in minutes' && !stops.some((n) => /minutes (later|earlier)/.test(n)), stops.join(' › '));
  check('nothing was sent while the arrows were pressed', patches(station).length === 0);

  // Move: one press saves it.
  await time.fill('17:30');
  await move();
  const first = patches(station);
  check('Move saves the move at once: one request, to the episode, with its time and nothing else', first.length === 1 && first[0].path === path && JSON.stringify(Object.keys(first[0].body)) === '["air_at"]' && first[0].body.air_at === '2026-11-16T17:30:00+00:00[Europe/London]', JSON.stringify(first.map((c) => [c.path, c.body])));
  check('the line says where the episode is now, with Undo', (await toastText(page)) === 'Moved. Episode 1 is on Monday 16 November, 17:30 to 20:30. Undo', await toastText(page));
  check('the week is up to date: the block is at its new time, still selected, and the editor says the time', (await when()) === 'Mon 16 Nov 2026 · 17:30–20:30' && (await block(page, DRIVE).getAttribute('class')).includes('sel') && (await block(page, DRIVE).innerText()).includes('17:30') && ep().air_at === instant('2026-11-16', 1050), `${await when()} | ${(await block(page, DRIVE).innerText()).replace(/\s+/g, ' ')}`);
  check('the row has closed, the focus is on Move…, and the field says set here, with Back to the rule', (await mrow.count()) === 0 && (await focused()) === 'Move…' && (await field.locator('.lab .src').innerText()).trim() === 'set here' && (await field.getByRole('button', { name: 'Back to the rule' }).count()) === 1 && !(await field.innerText()).includes('unsaved'), `${await focused()} | ${(await field.innerText()).replace(/\s+/g, ' ')}`);
  check('the title typed before is untouched, still a draft: Save episode waits for it', (await title.inputValue()) === 'Monday’s drive' && ep().title === '' && (await editor(page).getByRole('button', { name: 'Save episode' }).isEnabled()) && (await editor(page).getByRole('button', { name: 'Discard changes' }).isEnabled()));
  await inView(page, toast(page), 'the line with Undo');
  await shot(page, '17c-episode-moved-undo');
  await undo();
  const back = patches(station).at(-1);
  check('Undo puts the episode back: its time alone is sent again, and the week and the editor say 16:00', patches(station).length === 2 && JSON.stringify(back.body) === '{"air_at":"2026-11-16T16:00:00+00:00[Europe/London]"}' && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00' && (await field.locator('.lab .src').innerText()).trim() === 'from Series 1' && (await title.inputValue()) === 'Monday’s drive', `${JSON.stringify(back.body)} | ${await when()}`);

  // A move that changes nothing sends nothing; Cancel and Esc send nothing.
  await openMove();
  await mrow.getByRole('button', { name: 'Move', exact: true }).click();
  await sleep(300);
  check('Move with nothing changed closes the row and sends nothing', (await mrow.count()) === 0 && patches(station).length === 2);
  await openMove();
  await time.fill('18:45');
  await mrow.getByRole('button', { name: 'Cancel' }).click();
  await sleep(200);
  check('Cancel closes the row, sends nothing, and returns the focus to Move…', (await mrow.count()) === 0 && patches(station).length === 2 && (await focused()) === 'Move…' && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00');

  // The length, when it is changed; ⏎ in a field moves.
  await openMove();
  await length.fill('120');
  await length.press('Enter');
  await toast(page).waitFor({ timeout: 5000 });
  await settled(page);
  check('a changed length is sent with the time, and ⏎ in a field is Move', JSON.stringify(patches(station).at(-1).body) === '{"air_at":"2026-11-16T16:00:00+00:00[Europe/London]","minutes":120}' && (await toastText(page)) === 'Moved. Episode 1 is on Monday 16 November, 16:00 to 18:00. Undo' && (await when()) === 'Mon 16 Nov 2026 · 16:00–18:00', `${JSON.stringify(patches(station).at(-1).body)} | ${await toastText(page)}`);
  await undo();
  check('and Undo gives the length back to the rule', JSON.stringify(patches(station).at(-1).body) === '{"air_at":"2026-11-16T16:00:00+00:00[Europe/London]","minutes":null}' && ep().minutes === null && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00', JSON.stringify(patches(station).at(-1).body));
  await openMove();
  await length.fill('');
  await mrow.getByRole('button', { name: 'Move', exact: true }).click();
  await sleep(200);
  const before = patches(station).length;
  check('a length that is not a number is refused in the row, and nothing is sent', (await mrow.locator('[role="alert"]').innerText()) === 'Give a length in whole minutes, from 1 to 1,440.' && (await length.getAttribute('aria-describedby')) === 'ee-mv-err' && (await length.getAttribute('aria-invalid')) === 'true' && (await date.getAttribute('aria-invalid')) === null && patches(station).length === before, (await mrow.innerText()).replace(/\s+/g, ' '));
  await length.fill('180');

  // A move that makes a clash is allowed, and said.
  await date.fill('2026-11-19');
  await time.fill('19:15');
  await move();
  check('a move onto another show is allowed for a producer, and the line says what it clashes with', patches(station).length === before + 1 && (await toastText(page)) === 'Moved. Episode 1 is on Thursday 19 November, 19:15 to 22:15. It now clashes with Budget Night Special. Undo' && (await editor(page).locator('[data-panel="calendar.clash"]').count()) === 1, await toastText(page));
  await shot(page, '17d-episode-moved-clash');

  // Back to the rule is saved at once too.
  await field.getByRole('button', { name: 'Back to the rule' }).click();
  await settled(page);
  check('Back to the rule is saved at once: the rule’s time, and the length given back to it', JSON.stringify(patches(station).at(-1).body) === '{"air_at":"2026-11-16T16:00:00+00:00[Europe/London]","minutes":null}' && (await toastText(page)) === 'Moved. Episode 1 is back on Monday 16 November, as the rule has it. Undo' && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00' && (await field.getByRole('button', { name: 'Back to the rule' }).count()) === 0 && (await focused()) === 'Move…', `${JSON.stringify(patches(station).at(-1).body)} | ${await toastText(page)} | ${await focused()}`);
  await undo();
  check('and its Undo puts the episode where it was', patches(station).at(-1).body.air_at === '2026-11-19T19:15:00+00:00[Europe/London]' && (await when()) === 'Thu 19 Nov 2026 · 19:15–22:15', JSON.stringify(patches(station).at(-1).body));
  await field.getByRole('button', { name: 'Back to the rule' }).click();
  await settled(page);

  // To another week: the week follows the episode, and the editor keeps what was typed.
  await openMove();
  await date.fill('2026-11-28');
  await time.fill('10:00');
  await move();
  check('a move to another week takes the week there: the editor stays on the episode, with the title still typed', (await rangeText(page)) === '23 – 29 November 2026' && (await editor(page).getAttribute('data-panel')) === 'calendar.episode-editor' && (await when()) === 'Sat 28 Nov 2026 · 10:00–13:00' && (await title.inputValue()) === 'Monday’s drive' && (await page.locator('.ec .ev.sel').count()) === 1 && (await toastText(page)) === 'Moved. Episode 1 is on Saturday 28 November, 10:00 to 13:00. Undo', `${await rangeText(page)} | ${await when()} | ${await title.inputValue()} | ${await toastText(page)}`);
  await undo();
  check('Undo brings the week back with it, and the title is still there', (await rangeText(page)) === '16 – 22 November 2026' && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00' && (await title.inputValue()) === 'Monday’s drive' && (await page.locator('.ec .ev.sel').count()) === 1, `${await rangeText(page)} | ${await when()} | ${await title.inputValue()}`);

  // In the Day view, to another day.
  await page.keyboard.press('Escape');
  await block(page, DRIVE).focus();
  await page.keyboard.press('d');
  await sleep(500);
  await openMove();
  await date.fill('2026-11-17');
  await time.fill('10:00');
  await move();
  check('in the Day view a move to another day opens that day, with the editor still on the episode', (await page.locator('.ec-host').getAttribute('data-view')) === 'day' && (await rangeText(page)) === 'Tue 17 November 2026' && (await when()) === 'Tue 17 Nov 2026 · 10:00–13:00' && (await title.inputValue()) === 'Monday’s drive', `${await rangeText(page)} | ${await when()}`);
  await undo();
  check('and Undo opens its own day again', (await rangeText(page)) === 'Mon 16 November 2026' && (await when()) === 'Mon 16 Nov 2026 · 16:00–19:00' && (await title.inputValue()) === 'Monday’s drive', `${await rangeText(page)} | ${await when()}`);
  await page.keyboard.press('Escape');
  await page.keyboard.press('w');
  await sleep(400);

  // Save episode sends what is left in the draft, and no time.
  const sent = patches(station).length;
  await editor(page).getByRole('button', { name: 'Save episode' }).click();
  await settled(page);
  check('Save episode then sends the title alone: the time is no part of the draft', patches(station).length === sent + 1 && JSON.stringify(patches(station).at(-1).body) === '{"title":"Monday’s drive"}' && !(await editor(page).getByRole('button', { name: 'Save episode' }).isEnabled()), JSON.stringify(patches(station).at(-1).body));

  // Undo after another block was selected still puts back the episode that was moved.
  await openMove();
  await time.fill('16:30');
  await move();
  await scrollTo(page, 13.5);
  await block(page, K.tea).click({ position: { x: 20, y: 20 } });
  await editor(page).locator('h2').filter({ hasText: 'The Tea Break' }).or(editor(page).locator('.crumbs').filter({ hasText: 'The Tea Break' })).first().waitFor({ timeout: 5000 });
  await undo();
  check('Undo pressed with another episode open puts back the one that was moved, and leaves the other open', patches(station).at(-1).path === path && patches(station).at(-1).body.air_at === '2026-11-16T16:00:00+00:00[Europe/London]' && ep().air_at === instant('2026-11-16', 960) && (await editor(page).locator('.crumbs').innerText()).includes('The Tea Break'), `${patches(station).at(-1).path} ${JSON.stringify(patches(station).at(-1).body)}`);

  // An aired episode is not moved.
  await page.keyboard.press('Escape');
  await page.keyboard.press('ArrowLeft');
  await settled(page);
  await scrollTo(page, 20);
  await block(page, 'ep:the-bothy-sessions-3@2').click({ position: { x: 20, y: 20 } });
  await editor(page).locator('.frozen-note').waitFor({ timeout: 5000 });
  check('an aired episode has no Move…: its date is as aired', (await editor(page).locator('[data-field="date-and-time"]').getByRole('button').count()) === 0 && (await editor(page).locator('[data-field="date-and-time"] .lab .src').innerText()).trim() === 'as aired');
  await page.keyboard.press('Escape');
  await page.keyboard.press('ArrowRight');
  await settled(page);

  // The arrows in a show's dates.
  await page.locator('[data-panel="calendar.shows"] .showrow').filter({ hasText: 'The Bothy Sessions' }).click();
  const dates = page.locator('[data-panel="calendar.show-dates"]');
  await dates.locator('.nd.dl li.r').first().waitFor();
  await dates.locator('li.r[data-row="2026-12-11"]').getByRole('button', { name: 'Move…' }).click();
  const mv = dates.locator('li.mv');
  await mv.waitFor();
  await mv.getByRole('button', { name: '15 minutes later' }).click();
  const stepped = await page.locator('#sd-mv-time').inputValue();
  await page.locator('#sd-mv-time').fill('00:10');
  await mv.getByRole('button', { name: '15 minutes earlier' }).click();
  await mv.getByRole('button', { name: '15 minutes earlier' }).click();
  check('a date’s Move… in a show’s dates has the arrows too, and the date goes with the time past midnight', stepped === '21:15' && (await page.locator('#sd-mv-time').inputValue()) === '23:45' && (await page.locator('#sd-mv-date').inputValue()) === '2026-12-10' && (await page.evaluate(() => document.activeElement?.id)) === 'sd-mv-time', `${stepped} | ${await page.locator('#sd-mv-date').inputValue()} ${await page.locator('#sd-mv-time').inputValue()}`);
  const fit = await mv.evaluate((el) => { const c = el.getBoundingClientRect(); return [...el.querySelectorAll('input, button')].every((x) => { const r = x.getBoundingClientRect(); return r.left >= c.left && r.right <= c.right + 0.5; }); });
  check('and the row still holds its fields and buttons inside itself', fit);
  await shot(page, '17e-dates-move-arrows');
  await page.keyboard.press('Escape');
  await sleep(200);
  check('Esc with the focus in the time still cancels a date’s Move…', (await mv.count()) === 0 && puts(station, 'the-bothy-sessions').length === 0);

  // The arrows in the rule dialog: from and until, and a date's Move….
  await dates.getByRole('button', { name: 'Change…' }).click();
  const rd = page.locator('dialog:has([data-panel="calendar.rule"])').last();
  await rd.locator('[data-panel="calendar.rule"]').waitFor();
  await settled(page);
  await page.waitForFunction(() => !document.querySelector('.preview.stale') && !document.querySelector('.summary.wait'), null, { timeout: 5000 }).catch(() => {});
  const from = rd.locator('input[type="time"]').nth(0);
  const until = rd.locator('input[type="time"]').nth(1);
  const asked = station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview').length;
  check('the rule dialog’s from and until each have the two arrows', (await rd.locator('.qs .timefield .steps button').count()) === 4 && (await from.inputValue()) === '21:00' && (await until.inputValue()) === '23:00', `${await from.inputValue()} ${await until.inputValue()}`);
  await shot(page, '17f-rule-dialog-times');
  await rd.locator('.qs .timefield').nth(0).getByRole('button', { name: '15 minutes later' }).click();
  await rd.locator('.qs .timefield').nth(1).getByRole('button', { name: '15 minutes earlier' }).click();
  await settled(page);
  await page.waitForFunction(() => !document.querySelector('.preview.stale') && !document.querySelector('.summary.wait'), null, { timeout: 5000 }).catch(() => {});
  const asks = station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview');
  check('a press there changes the rule as typing does: the length and the preview follow', (await from.inputValue()) === '21:15' && (await until.inputValue()) === '22:45' && (await rd.locator('[data-testid="rule-length"]').innerText()).trim() === '1 h 30 min' && asks.length > asked && asks.at(-1).body.rule.start === 1275 && asks.at(-1).body.rule.minutes === 90, `${await from.inputValue()} ${await until.inputValue()} | ${await rd.locator('[data-testid="rule-length"]').innerText()} | ${JSON.stringify([asks.at(-1).body.rule.start, asks.at(-1).body.rule.minutes])}`);
  const sz = await rd.locator('.qs .timefield .steps button').evaluateAll((els) => els.map((el) => { const r = el.getBoundingClientRect(), i = el.closest('.timefield').querySelector('input').getBoundingClientRect(); return { w: r.width, h: r.height, ih: i.height }; }));
  check('and its arrows are 24 px by 24 px or more, as tall as their fields', sz.every((b) => b.w / z >= 23.9 && b.h / z >= 23.9 && within(b.h, b.ih, 1)), JSON.stringify(sz));
  await shot(page, '17g-rule-dialog-times-stepped');
  await rd.getByRole('button', { name: 'Move…' }).first().click();
  await sleep(200);
  if (await page.locator('#rp-mv-time').count()) {
    const was = await page.locator('#rp-mv-time').inputValue();
    await rd.locator('li.mv').getByRole('button', { name: '15 minutes later' }).click();
    check('a date’s Move… in the rule dialog has the arrows', (await page.locator('#rp-mv-time').inputValue()) !== was && (await rd.locator('li.mv .timefield .steps button').count()) === 2, `${was} to ${await page.locator('#rp-mv-time').inputValue()}`);
    await shot(page, '17h-rule-dialog-move');
  } else check('a date’s Move… in the rule dialog has the arrows', false, 'no Move… row opened');
  await ctx.close();

  // A presenter: the station refuses a move onto another show, and the row says so.
  {
    const st = clashWeek(new Station({ role: 'presenter' }));
    const { ctx, page } = await open(st);
    const field = editor(page).locator('[data-field="date-and-time"]');
    const mrow = field.locator('.row.move');
    await scrollTo(page, 15);
    await block(page, DRIVE).click({ position: { x: 20, y: 20 } });
    await editor(page).locator('h2').filter({ hasText: 'Episode 1' }).waitFor({ timeout: 5000 });
    await field.getByRole('button', { name: 'Move…' }).click();
    await mrow.locator('input.date').fill('2026-11-19');
    await mrow.locator('input.time').fill('18:30');
    await mrow.getByRole('button', { name: 'Move', exact: true }).click();
    await mrow.locator('[role="alert"]').waitFor({ timeout: 5000 });
    const asked = patches(st);
    check('a presenter’s move onto another show is refused by the station: the row stays open with what was typed and the station’s words, and nothing moved', asked.length === 1 && (await mrow.locator('[role="alert"]').innerText()) === 'Budget Night Special has that time.' && (await mrow.locator('input.date').getAttribute('aria-describedby')) === 'ee-mv-err' && (await mrow.locator('input.date').inputValue()) === '2026-11-19' && (await mrow.locator('input.time').inputValue()) === '18:30' && (await toastText(page)) === '' && st.episodes.find((e) => e.id === 'kettle-drive-1').air_at === instant('2026-11-16', 960) && (await editor(page).locator('.head .when .num').innerText()).trim() === 'Mon 16 Nov 2026 · 16:00–19:00', `${(await mrow.innerText()).replace(/\s+/g, ' ')} | ${await toastText(page)}`);
    await shot(page, '17i-presenter-refused');
    await mrow.locator('input.date').fill('2026-11-16');
    await mrow.locator('input.time').fill('13:00');
    await mrow.getByRole('button', { name: 'Move', exact: true }).click();
    await toast(page).waitFor({ timeout: 5000 });
    check('and a time that is free is saved', (await toastText(page)) === 'Moved. Episode 1 is on Monday 16 November, 13:00 to 16:00. Undo' && (await mrow.count()) === 0, await toastText(page));
    await ctx.close();
  }

  // Light and Night, for the eye: the row, a press on an arrow, the line with Undo, the rule dialog.
  for (const theme of ['light', 'night']) {
    const st = clashWeek(new Station());
    const { ctx, page } = await open(st, { theme });
    const field = editor(page).locator('[data-field="date-and-time"]');
    const mrow = field.locator('.row.move');
    await scrollTo(page, 15);
    await block(page, DRIVE).click({ position: { x: 20, y: 20 } });
    await editor(page).locator('h2').filter({ hasText: 'Episode 1' }).waitFor({ timeout: 5000 });
    await field.getByRole('button', { name: 'Move…' }).click();
    await mrow.waitFor();
    await shot(page, `17a-episode-move-idle-${theme}`);
    await mrow.getByRole('button', { name: '15 minutes later' }).click();
    await mrow.getByRole('button', { name: '15 minutes later' }).hover();
    await shot(page, `17b-episode-move-stepped-${theme}`);
    await mrow.getByRole('button', { name: 'Move', exact: true }).click();
    await toast(page).waitFor({ timeout: 5000 });
    await settled(page);
    check(`${theme}: Move saves at once and the line offers Undo`, (await toastText(page)) === 'Moved. Episode 1 is on Monday 16 November, 16:15 to 19:15. Undo' && patches(st).length === 1);
    await shot(page, `17c-episode-moved-undo-${theme}`);
    await page.keyboard.press('Escape');
    await page.locator('[data-panel="calendar.shows"] .showrow').filter({ hasText: 'The Bothy Sessions' }).click();
    await page.locator('[data-panel="calendar.show-dates"]').getByRole('button', { name: 'Change…' }).click();
    const rd = page.locator('dialog:has([data-panel="calendar.rule"])').last();
    await rd.locator('[data-panel="calendar.rule"]').waitFor();
    await settled(page);
    await shot(page, `17f-rule-dialog-times-${theme}`);
    await ctx.close();
  }
}

// ---- 5. The doors from New show and Give it dates…; a presenter; an older station ----
if (run('doors')) {
  const station = clashWeek(new Station());
  const { ctx, page } = await open(station, { week: false });
  const dates = page.locator('[data-panel="calendar.show-dates"]');
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = page.locator('dialog[data-panel="calendar.new-show"]');
  await page.keyboard.type('The Folk Club');
  await page.keyboard.press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  await page.fill('#ns-first', '2026-12-01');
  await page.locator('#ns-first').dispatchEvent('change');
  await d.getByText('Every month on the first Tuesday').click();
  await settled(page);
  await page.waitForFunction(() => !document.querySelector('.preview.stale') && !document.querySelector('.summary.wait'), null, { timeout: 5000 }).catch(() => {});
  check('the rule dialog’s two columns still stand side by side under their new class', (await d.locator('.rulepart').count()) === 1 && (await d.locator('.rulepart').evaluate((el) => getComputedStyle(el).display)) === 'grid' && (await page.locator('.rule:not(.ev)').count()) === 0);
  await d.locator('.acts button[type="submit"]').last().click();
  await d.getByText('is a show.').waitFor();
  check('New show’s Done says where the dates are listed, and offers See its dates', (await d.innerText()).replace(/\s+/g, ' ').includes('The other dates wait in the week as dotted blocks. Draft each when you are ready. They are listed under the show’s Dates.') && (await d.getByRole('button', { name: 'See its dates' }).count()) === 1, (await d.innerText()).replace(/\s+/g, ' ').slice(200));
  await shot(page, '20-new-show-done');
  await d.getByRole('button', { name: 'See its dates' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor({ timeout: 5000 });
  check('See its dates opens the new show at Dates', (await dates.locator('h2').innerText()) === 'The Folk Club' && (await dates.locator('.nd.dl.coming > li.r').first().innerText()).replace(/\s+/g, ' ').startsWith('Ep 1 Tue 1 Dec 20:00–21:00 Open › drafted'));
  check('a rule with no end says its dates go on', (await dates.locator('.after').innerText()).replace(/\s+/g, ' ') === 'Show more dates and more after, with no end' && (await dates.locator('.sum').innerText()).includes('1 drafted of the next 60; the dates go on with no end.'), (await dates.locator('.sum').innerText()).replace(/\s+/g, ' '));

  // Give it dates…: the line offers See them.
  await editor(page).getByRole('button', { name: '‹ Shows' }).click();
  await page.locator('[data-panel="calendar.unscheduled"]').getByRole('button', { name: 'Give it dates…' }).first().click();
  const rd = page.locator('dialog[data-panel="calendar.rule"], dialog:has([data-panel="calendar.rule"])').last();
  await rd.locator('[data-panel="calendar.rule"]').waitFor();
  await settled(page);
  await page.waitForFunction(() => !document.querySelector('.preview.stale') && !document.querySelector('.summary.wait'), null, { timeout: 5000 }).catch(() => {});
  await rd.locator('.acts button[type="submit"]').last().click();
  await toast(page).waitFor({ timeout: 5000 });
  check('after Give it dates…, the line offers See them', (await toastText(page)).startsWith('Saved. Northern Lights has its dates; they are in the week as dotted blocks.') && (await toastText(page)).endsWith('See them'), await toastText(page));
  await toast(page).getByRole('button', { name: 'See them' }).click();
  await dates.locator('.nd.dl li.r').first().waitFor({ timeout: 5000 });
  check('and See them opens that show’s dates', (await dates.locator('h2').innerText()) === 'Northern Lights');
  await ctx.close();

  // A presenter: the station marks what it would refuse, and the list draws it.
  {
    const st = clashWeek(new Station({ role: 'presenter' }));
    const { ctx, page } = await open(st, { week: false });
    await page.locator('[data-panel="calendar.shows"] .showrow').first().click();
    const dates = page.locator('[data-panel="calendar.show-dates"]');
    await dates.locator('.nd.dl li.r').first().waitFor({ timeout: 5000 });
    check('a presenter sees the list, and a clash the station would refuse says so in its words', (await dates.locator('li.r[data-row="2026-11-27"] .why2').innerText()).replace(/\s+/g, ' ') === 'St Andrew’s Weekend has that time. St Andrew’s Weekend, 22:00 to 23:30 · overlaps 1 h', await dates.locator('li.r[data-row="2026-11-27"] .why2').innerText());
    await ctx.close();
  }

  // A station older than 1.0.4 keeps a weekly time only: drafted episodes are listed, and why.
  {
    const st = new Station({ version: '1.0.3' });
    const { ctx, page } = await open(st, { week: false });
    await page.locator('[data-panel="calendar.unscheduled"]').getByRole('button', { name: 'Saturday Night Kettle' }).click();
    const dates = page.locator('[data-panel="calendar.show-dates"]');
    await dates.locator('.sum').waitFor({ timeout: 5000 });
    await settled(page);
    check('an older station’s Dates lists drafted episodes only, says why, and asks the station for no preview', (await dates.locator('.sum').innerText()).includes('This station keeps a weekly time only, so only drafted episodes are listed. Update it to see every date.') && (await dates.locator('.nd.dl.coming > li.r').count()) === 1 && !(await dates.locator('.nd.dl.coming').innerText()).includes('made by hand') && st.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview').length === 0, `${await dates.locator('.nd.dl.coming > li.r').count()} rows`);
    check('the fold and the Day view need only the blocks, so they work there too', (await page.locator('.ec .dayhd').count()) === 7 && (await page.locator('[data-panel="calendar.toolbar"]').getByRole('button', { name: 'Day', exact: true }).count()) === 1);
    await ctx.close();
  }
}

// ---- 6. Light and Night ----
if (run('themes')) {
  for (const theme of ['light', 'night']) {
    const station = clashWeek(new Station());
    const { ctx, page } = await open(station, { theme });
    await scrollTo(page, 13.5);
    await block(page, K.schools).click({ position: { x: (await box(block(page, K.schools))).width - 6, y: 10 } }).catch(() => {});
    await sleep(500);
    const ground = await block(page, K.schools).evaluate((el) => getComputedStyle(el).backgroundColor);
    check(`${theme}: the fold stands, and a selected dotted date’s ground is opaque`, (await block(page, K.schools).getAttribute('class')).includes('sel') && !/rgba\(.*,\s*0\)|transparent/.test(ground), ground);
    await shot(page, `30-week-${theme}`);
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
