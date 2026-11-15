// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the guided New show (ADR-0278) in a real browser against a stand-in control plane
// (station.mjs), checks each step against what the studio sent, and captures the screens. It
// needs no stack and no ports but its own loopback one: the studio app's ordinary build is
// served as static files, and every request the page makes for /api and /health is answered
// in this process. So it proves the studio half alone; the two halves together are the stack
// tests' to prove once the control plane's routes exist.
//
//   just studio-check          # or: cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/newshow/shoot.mjs
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs). Screenshots go to
// OUT, never the repository. The clock is fixed at Friday 13 November 2026, 10:00 in
// Europe/London, the design's own day, so every date in a picture is a real one.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Station, instant } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const DIST = join(here, '..', '..', 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? '/tmp/weavesignal-newshow-shots';
const PORT = Number(process.env.PORT ?? 27913);
const NOW = instant('2026-11-13', 600) * 1000;
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
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

/** A window on a station: every /api and /health request is the stand-in's to answer. */
// WIDTH, HEIGHT and TEXT run the whole drive at another window and text size: the page is zoomed
// above 1920 by 1080, and every button the steps press must then still be inside the window.
const SIZE = { width: Number(process.env.WIDTH ?? 1440), height: Number(process.env.HEIGHT ?? 900), text: Number(process.env.TEXT ?? 1) };
/** The control is inside the window, so a person can see and press it without scrolling the page. */
async function inView(page, loc, what) {
  const b = await loc.boundingBox();
  const vp = page.viewportSize();
  check(`${what} is inside the window (${vp.width}×${vp.height}, text ${SIZE.text})`, !!b && b.y >= 0 && b.x >= 0 && b.y + b.height <= vp.height && b.x + b.width <= vp.width, b ? `at ${Math.round(b.x)},${Math.round(b.y)} to ${Math.round(b.x + b.width)},${Math.round(b.y + b.height)}` : 'not drawn');
}

async function open(station, { width = SIZE.width, height = SIZE.height, theme = 'dark', tab = true, text = SIZE.text, events = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: 'Europe/London' });
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
    // The studio's view comes as server-sent events from a desk this stand-in does not have.
    // With `events`, each request is answered with the view as it stands and the stream ends;
    // the studio asks again after a second, so a notice reaches the page within about one.
    if (events && url.pathname === '/api/v1/studio/events') {
      station.calls.push({ method: 'GET', path: url.pathname });
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: `data: ${JSON.stringify(station.studioView())}\n\n` });
    }
    if (url.pathname === '/api/v1/studio/events' || url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    const [status, body] = station.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 15000 });
  if (tab) {
    await page.keyboard.press('Control+5');
    await page.waitForSelector('.schedule .ec', { timeout: 15000 });
    await sleep(800);
  }
  return { ctx, page };
}

const shot = async (page, name) => {
  await sleep(250);
  await page.screenshot({ path: join(OUT, `${name}.png`) });
  log('shot', name);
};
/** The preview has answered the questions as they stand. */
const settled = async (page) => {
  await sleep(350);
  await page.waitForFunction(() => !document.querySelector('.preview.stale') && !document.querySelector('.summary.wait'), null, { timeout: 5000 }).catch(() => {});
  await sleep(100);
};
/** How often the week is read for one Skip this date, with no notice in the way (scenario 3). */
let skipWeekReads = null;
const weekReads = (station) => station.calls.filter((c) => c.method === 'GET' && c.path.startsWith('/api/v1/schedule/episodes')).length;
const previews = (station) => station.calls.filter((c) => c.path === '/api/v1/schedule/rule/preview');
const dlg = (page, panel = 'calendar.new-show') => page.locator(`dialog[data-panel="${panel}"]`);

// ---- 1. New show: the three parts, a skipped and a moved date, Done, Open episode 1 ----
{
  const station = new Station();
  const { ctx, page } = await open(station);
  check('shows with nothing coming up are listed', (await page.locator('[data-panel="calendar.unscheduled"]').innerText()).includes('Northern Lights'));
  await page.locator('[data-panel="calendar.unscheduled"] summary').click();
  await shot(page, '01-nothing-coming-up');

  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = dlg(page);
  await d.waitFor();
  check('New show opens on part 1 with focus in Name', await page.evaluate(() => document.activeElement?.id === 'ns-name'));
  await page.keyboard.type('The Folk Club');
  await page.locator('[aria-label="Bank 11"]').click();
  await shot(page, '02-new-show-part-1');
  await page.locator('#ns-name').press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  check('⏎ in Name goes to part 2, with focus in First on', await page.evaluate(() => document.activeElement?.id === 'ns-first'));
  await settled(page);
  check('the first preview is asked for at once', previews(station).length === 1, `${previews(station).length} asked`);
  check('Once only is picked for a new show', await d.locator('input[value="once"]').isChecked());
  check('with no block selected, First on is today and nothing is said of a block', (await page.inputValue('#ns-first')) === '2026-11-13' && (await d.locator('[data-testid="rule-seeded"]').count()) === 0, await page.inputValue('#ns-first'));

  await page.fill('#ns-first', '2026-12-01');
  await page.locator('#ns-first').dispatchEvent('change');
  await settled(page);
  const rowsText = await d.locator('.choices').innerText();
  check('the Repeats rows are made from the date', rowsText.includes('Every month on the first Tuesday') && rowsText.includes('Every year on 1 December'), rowsText.replace(/\n/g, ' · '));
  await shot(page, '03-when-it-airs-once');

  const before = previews(station).length;
  await d.getByText('Every month on the first Tuesday').click();
  await d.getByText('After', { exact: false }).first().click();
  await settled(page);
  check('quick changes ask for one preview, 250 ms after the last', previews(station).length - before <= 2, `${previews(station).length - before} asked for two changes`);
  {
    const rep = await d.locator('.choices input').first().boundingBox();
    const end = await d.locator('.ends input[type="radio"]').first().boundingBox();
    check('the radios of Ends stand in line under those of Repeats', !!rep && !!end && Math.abs(rep.x - end.x) < 1.5, `${rep?.x} against ${end?.x}`);
  }
  check('the sentence is the station’s', (await d.locator('[data-panel="calendar.rule.summary"] .say').innerText()) === 'Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes.', await d.locator('[data-panel="calendar.rule.summary"] .say').innerText());

  // Skip 5 January from its row, with S on the focused row.
  await d.locator('[data-row="2027-01-05"]').focus();
  await page.keyboard.press('s');
  await settled(page);
  check('S skips the focused row, and the row stays, struck through', await d.locator('[data-row="2027-01-05"].skipped').count() === 1);
  check('a skipped date does not count: the run still has 10 episodes', (await d.locator('.summary .last').innerText()).includes('5 October 2027'), await d.locator('.summary .last').innerText());
  // Move 6 April with M, then the date and ⏎.
  await d.locator('[data-row="2027-04-06"]').focus();
  await page.keyboard.press('m');
  await page.fill('#rp-mv-date', '2027-06-08');
  await page.locator('#rp-mv-date').press('Enter');
  check('a move past its neighbour is refused in words', (await d.locator('.mv .err').innerText()).includes('Keep it between 2 March and 4 May'), await d.locator('.mv .err').innerText().catch(() => ''));
  check('and the reason is said once, not as the error and the hint both', ((await d.locator('.mv').innerText()).match(/Keep it between/g) ?? []).length === 1 && (await d.locator('#rp-mv-date').getAttribute('aria-describedby')) === 'rp-mv-hint' && (await d.locator('#rp-mv-hint').count()) === 1, (await d.locator('.mv').innerText()).replace(/\n/g, ' '));
  await page.fill('#rp-mv-date', '2027-04-13');
  await shot(page, '04-when-it-airs-move');
  await page.locator('#rp-mv-date').press('Enter');
  await settled(page);
  check('the moved row says where it was, and offers Back to the rule', (await d.locator('[data-row="2027-04-06"]').innerText()).includes('MOVED · WAS TUE 6 APR') && (await d.locator('[data-row="2027-04-06"]').innerText()).includes('Back to the rule'), (await d.locator('[data-row="2027-04-06"]').innerText()).replace(/\n/g, ' '));
  check('focus returns to the row after a move', await page.evaluate(() => document.activeElement?.getAttribute('data-row') === '2027-04-06'));
  check('one date clashes, and the button says anyway', (await d.locator('.warn').innerText()).includes('1 of the next 10 dates clashes') && (await d.getByRole('button', { name: 'Make the show anyway' }).count()) === 1, (await d.locator('.warn').innerText()).replace(/\n/g, ' '));
  await shot(page, '05-when-it-airs-monthly');

  // The small months: the arrows move a day, ⏎ on a date that airs goes to its row.
  await d.locator('[data-day="2026-12-01"]').focus();
  await page.keyboard.press('ArrowRight');
  check('→ moves a day in the small months', await page.evaluate(() => document.activeElement?.getAttribute('data-day') === '2026-12-02'));
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('Enter');
  check('⏎ on a date that airs moves focus to its row', await page.evaluate(() => document.activeElement?.getAttribute('data-row') === '2026-12-01'));

  // The rows are one stop for Tab, then the focused row's own buttons, then on: not every row's.
  {
    const at = () => page.evaluate(() => { const a = document.activeElement; return a?.getAttribute('data-row') ? `row ${a.getAttribute('data-row')}` : `${a?.closest('[data-row]')?.getAttribute('data-row') ?? ''} ${a?.textContent?.trim() ?? ''}`.trim(); });
    await page.keyboard.press('ArrowDown');
    const row = await at();
    await page.keyboard.press('Tab');
    const one = await at();
    await page.keyboard.press('Tab');
    const two = await at();
    await page.keyboard.press('Tab');
    const three = await at();
    check('Tab from a row goes to that row’s own buttons, then leaves the rows', row === 'row 2027-01-05' && one === '2027-01-05 Put back' && !/2027|2026/.test(two) && !/2027|2026/.test(three), `${row} → ${one} → ${two} → ${three}`);
    // A row's button pressed from the keyboard: the button is replaced, and the focus stays in the row.
    await d.locator('[data-row="2027-01-05"]').focus();
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');
    await settled(page);
    check('Put back from the keyboard leaves the focus on its row, not out of the dialog', (await d.locator('[data-row="2027-01-05"].skipped').count()) === 0 && (await at()) === 'row 2027-01-05', await at());
    await page.keyboard.press('s');
    await settled(page);
  }

  await inView(page, d.getByRole('button', { name: 'Make the show anyway' }), 'New show: Make the show anyway');
  await d.getByRole('button', { name: 'Make the show anyway' }).click();
  await d.getByText('is a show.').waitFor();
  const put = station.calls.find((c) => c.method === 'PUT' && c.path === '/api/v1/shows/the-folk-club');
  check('one save makes the show, with its rule and draft_first', !!put && put.body.draft_first === true && put.body.rule.repeat.kind === 'monthly_nth' && put.body.rule.skip[0] === '2027-01-05' && put.body.rule.moves[0].to === '2027-04-13T20:00:00' && put.body.rule.end.count === 10 && !('slot' in put.body), JSON.stringify(put?.body.rule));
  const done = await d.locator('div.done').innerText();
  check('Done says episode 1 is drafted, and offers the rest', done.includes('Episode 1 is drafted for Tuesday 1 December, 20:00 to 21:00.') && done.includes('or all 10 now') && done.includes('Draft all 10'), done.replace(/\n/g, ' '));
  await shot(page, '06-done');
  await d.getByRole('button', { name: 'Open episode 1' }).click();
  await page.locator('[data-panel="calendar.episode-editor"] h2').filter({ hasText: 'Episode 1' }).waitFor({ timeout: 5000 });
  check('Open episode 1 moves the week to it and selects it', (await page.locator('.toolbar .range').innerText()).includes('December 2026'), await page.locator('.toolbar .range').innerText());
  await shot(page, '07-episode-1-open');

  // Changing the rule later: Tuesday to Wednesday, with the drafted episode listed first.
  await page.getByRole('tab', { name: 'Show' }).click();
  await page.locator('[data-panel="calendar.show-rule"]').getByRole('button', { name: 'Change…' }).click();
  const rd = page.locator('dialog[data-dialog="rule"]');
  await rd.waitFor();
  await settled(page);
  await rd.getByRole('button', { name: 'More choices…' }).click();
  await rd.locator('select[aria-label="Weekday"]').selectOption('3');
  await settled(page);
  check('changing the pattern drops the skipped and moved dates it no longer gives, and says so once', (await rd.locator('.dropped').innerText().catch(() => '')).includes('no longer appl'), await rd.locator('.dropped').innerText().catch(() => 'nothing said'));
  await shot(page, '08-rule-dialog');
  await inView(page, rd.getByRole('button', { name: 'Save…' }), 'Change the rule: Save…');
  await rd.getByRole('button', { name: 'Save…' }).click();
  await rd.locator('[data-panel="calendar.rule.change"]').waitFor();
  check('Before you save lists the drafted episode’s move, with Move picked', (await rd.innerText()).includes('1 drafted, on the old dates.') && (await rd.locator('input[value="move"]').isChecked()), (await rd.locator('.groups').innerText()).replace(/\n/g, ' '));
  await shot(page, '09-before-you-save');
  await rd.getByText('Leave them where they are').click();
  check('the button follows the choice', (await rd.getByRole('button', { name: 'Save, and leave them' }).count()) === 1);
  await rd.getByText('Move them to the new dates').click();
  await inView(page, rd.getByRole('button', { name: /Save and move 1 episode/ }), 'Before you save: Save and move');
  await rd.getByRole('button', { name: /Save and move 1 episode/ }).click();
  await rd.waitFor({ state: 'detached' });
  const rulePut = station.calls.find((c) => c.method === 'PUT' && c.path === '/api/v1/shows/the-folk-club/rule');
  check('the rule is saved alone, with what the drafted episodes do', !!rulePut && rulePut.body.drafted === 'move' && rulePut.body.rule.repeat.weekday === 3, JSON.stringify(rulePut?.body));
  await sleep(600);
  check('the show editor shows the new rule in the station’s short form', (await page.locator('[data-panel="calendar.show-rule"]').innerText()).includes('First Wed monthly'), (await page.locator('[data-panel="calendar.show-rule"]').innerText()).replace(/\n/g, ' '));

  // A save of the name alone carries the rule as stored.
  await page.locator('[aria-label="Show name"]').fill('The Folk Club Live');
  await page.getByRole('button', { name: 'Save show' }).click();
  await sleep(500);
  const rename = station.calls.filter((c) => c.method === 'PUT' && c.path === '/api/v1/shows/the-folk-club').at(-1);
  check('a change of name carries the rule as stored', rename?.body.name === 'The Folk Club Live' && rename.body.rule?.repeat.weekday === 3 && !rename.body.draft_first, JSON.stringify(rename?.body.rule?.repeat));
  await ctx.close();
}

// ---- 2. A clash on every date, the offered time; the whole rule; chosen dates; Esc ----
{
  const station = new Station();
  const { ctx, page } = await open(station);
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = dlg(page);
  await page.keyboard.type('Late Tales');
  await page.keyboard.press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  await page.fill('#ns-first', '2026-11-17');
  await page.locator('#ns-first').dispatchEvent('change');
  await page.fill('#ns-from', '21:30');
  await page.locator('#ns-from').dispatchEvent('change');
  await page.fill('#ns-until', '22:30');
  await page.locator('#ns-until').dispatchEvent('change');
  await d.getByText('Every week on Tuesday').click();
  await settled(page);
  const warn = await d.locator('.warn').innerText();
  check('a show wholly in another’s way is named once, with a clear time offered', warn.includes('Every date overlaps Firth Sessions (Tuesdays, 21:00 to 22:00) by 30 minutes.') && warn.includes('Try 22:00 to 23:00'), warn.replace(/\n/g, ' '));
  await shot(page, '10-clash-every-date');
  await d.getByRole('button', { name: 'Try 22:00 to 23:00' }).click();
  await settled(page);
  check('the offered time clears it', (await d.locator('.warn').count()) === 0 && (await page.inputValue('#ns-from')) === '22:00' && (await d.getByRole('button', { name: 'Make the show', exact: true }).count()) === 1);

  await d.getByRole('button', { name: 'More choices…' }).click();
  await d.getByRole('button', { name: 'Friday' }).click();
  await d.locator('input[aria-label="Every how many weeks"]').fill('2');
  await d.locator('input[aria-label="Every how many weeks"]').dispatchEvent('change');
  await settled(page);
  check('the whole rule takes two weekdays every second week', (await d.locator('.summary .say').innerText()).startsWith('Every 2 weeks on Tuesday and Friday'), await d.locator('.summary .say').innerText());
  await shot(page, '11-whole-rule-weekly');
  // ← → along the five kinds, as a radio group.
  await d.getByRole('radio', { name: 'Weekly' }).focus();
  await page.keyboard.press('ArrowRight');
  check('→ moves along the five kinds and picks', await d.getByRole('radio', { name: 'Monthly' }).getAttribute('aria-checked') === 'true');
  await d.locator('select[aria-label="Day of the month"]').selectOption('31');
  await settled(page);
  check('day 31 says which months are skipped, and offers the last day', (await d.locator('.rulebox').innerText()).includes('Months without a 31st are skipped.') && (await d.getByRole('button', { name: 'Use the last day instead' }).count()) === 1);
  await shot(page, '12-whole-rule-monthly-day');
  await d.getByRole('radio', { name: 'Chosen dates' }).click();
  await settled(page);
  await d.locator('[data-day="2026-11-24"]').click();
  await d.locator('[data-day="2026-12-01"]').focus();
  await page.keyboard.press(' ');
  await settled(page);
  check('the calendar picks chosen dates, with a click or Space', (await d.locator('.picked .chip').count()) === 3 && (await d.locator('.summary .say').innerText()).startsWith('On 3 dates you chose'), await d.locator('.summary .say').innerText());
  await shot(page, '13-chosen-dates');
  {
    // A chip's cross is named and removes its date; the focus goes to the chip that takes its place.
    const chip = d.locator('.picked .chip').nth(1).locator('button');
    check('a chip’s cross says what it does, to the eye and to a screen reader', (await chip.getAttribute('title')) === 'Remove Tuesday 24 November' && (await chip.getAttribute('aria-label')) === 'Remove Tuesday 24 November');
    const box = await chip.boundingBox();
    check('and is a target of 24 px or more', !!box && box.width >= 24 && box.height >= 24, `${Math.round(box?.width ?? 0)}×${Math.round(box?.height ?? 0)}`);
    await chip.hover();
    await shot(page, '13b-chosen-dates-remove');
    await chip.focus();
    await page.keyboard.press('Enter');
    await settled(page);
    check('removing a date from its chip leaves the focus on the next chip', (await d.locator('.picked .chip').count()) === 2 && (await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))) === 'Remove Tuesday 1 December', await page.evaluate(() => document.activeElement?.getAttribute('aria-label') ?? document.activeElement?.tagName));
    // Remove on a row does the same from the preview.
    await d.locator('[data-row="2026-12-01"]').getByRole('button', { name: /^Remove/ }).click();
    await settled(page);
    check('Remove on a row takes the date out and leaves the focus on a row', (await d.locator('.picked .chip').count()) === 1 && (await page.evaluate(() => document.activeElement?.getAttribute('data-row'))) === '2026-11-17', await page.evaluate(() => document.activeElement?.getAttribute('data-row') ?? document.activeElement?.tagName));
    await d.locator('[data-day="2026-11-24"]').click();
    await d.locator('[data-day="2026-12-01"]').click();
    await settled(page);
  }

  // Esc leaves a text field first; the second asks before discarding.
  await page.locator('#ns-from').focus();
  await page.keyboard.press('Escape');
  check('Esc leaves a text field first', (await d.isVisible()) && (await page.evaluate(() => document.activeElement?.tagName === 'DIALOG')));
  await page.keyboard.press('Escape');
  const confirm = page.locator('dialog.confirm');
  await confirm.waitFor();
  check('a second Esc asks once, with Keep editing in focus', (await confirm.innerText()).includes('Discard this show?') && (await page.evaluate(() => document.activeElement?.textContent === 'Keep editing')));
  await shot(page, '14-discard');
  await page.keyboard.press('Enter');
  check('Keep editing returns to the questions', (await d.isVisible()) && (await confirm.count()) === 0);
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.locator('dialog.confirm').getByRole('button', { name: 'Discard this show' }).click();
  await d.waitFor({ state: 'detached' });
  check('nothing was saved', !station.calls.some((c) => c.method === 'PUT'));
  await ctx.close();
}

// ---- 3. A dotted block: Skip this date and Undo; New show from a selected block ----
{
  const station = new Station();
  const { ctx, page } = await open(station);
  await page.keyboard.press('ArrowRight');
  await sleep(700);
  check('the week draws the dates the station sent as dotted blocks', (await page.locator('.ec .ev.rule').count()) === station.week(instant('2026-11-16', 0) * 1000, instant('2026-11-23', 0) * 1000).dates.length, `${await page.locator('.ec .ev.rule').count()} dotted`);
  const dotted = page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' }).first();
  await dotted.click();
  await page.getByRole('button', { name: 'Skip this date' }).waitFor();
  await inView(page, page.getByRole('button', { name: 'Skip this date' }), 'a not-drafted block: Skip this date');
  await shot(page, '15-dotted-block');
  const n = await page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' }).count();
  const weekBefore = weekReads(station);
  await page.getByRole('button', { name: 'Skip this date' }).click();
  await page.locator('[data-testid="schedule-toast"]').waitFor();
  await sleep(1200);
  skipWeekReads = weekReads(station) - weekBefore;
  const skip = station.calls.filter((c) => c.method === 'PUT' && c.path === '/api/v1/shows/firth-sessions/rule').at(-1);
  check('Skip this date reads the rule and saves it with the date skipped', skip?.body.rule.skip.includes('2026-11-17') && skip.body.drafted === 'leave' && station.calls.some((c) => c.method === 'GET' && c.path === '/api/v1/shows/firth-sessions'), JSON.stringify(skip?.body.rule.skip));
  check('the block goes and the toast says so, with Undo', (await page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' }).count()) === n - 1 && (await page.locator('[data-testid="schedule-toast"]').innerText()).includes('Firth Sessions will not air on Tuesday 17 November.'), await page.locator('[data-testid="schedule-toast"]').innerText());
  await shot(page, '16-skipped-undo');
  await page.getByRole('button', { name: 'Undo' }).click();
  await sleep(700);
  check('Undo puts the date back', (await page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' }).count()) === n && station.shows.find((s) => s.slug === 'firth-sessions').rule.skip.length === 0);

  // Draft this episode: the station drafts the series' next date.
  await page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' }).first().click();
  await page.getByRole('button', { name: 'Draft this episode' }).click();
  await page.locator('[data-panel="calendar.episode-editor"] h2').filter({ hasText: 'Episode 1' }).waitFor({ timeout: 5000 });
  check('Draft this episode drafts it and opens it', station.episodes.some((e) => e.slug === 'firth-sessions'));

  // With a block selected, its day and hour start First on and from.
  await page.locator('.ec .ev.auto').last().click();
  await page.getByRole('button', { name: 'New show' }).first().click();
  await page.keyboard.type('Sunday Service');
  await page.keyboard.press('Enter');
  await dlg(page).locator('[data-panel="calendar.rule"]').waitFor();
  check('a selected block’s day starts First on', (await page.inputValue('#ns-first')) >= '2026-11-16' && (await page.inputValue('#ns-first')) <= '2026-11-22', `${await page.inputValue('#ns-first')} ${await page.inputValue('#ns-from')}`);

  // The date and time nobody typed are said under First on, in words, with the way back to today
  // (the 1.0.4 test run made shows on the selected block's day and hour without seeing either).
  {
    const d = dlg(page);
    const seeded = d.locator('[data-testid="rule-seeded"]');
    const first = await page.inputValue('#ns-first');
    const from = await page.inputValue('#ns-from');
    const until = await page.inputValue('#ns-until');
    const day = new Intl.DateTimeFormat('en-GB', { weekday: 'long', day: 'numeric', month: 'long', timeZone: 'UTC' }).format(new Date(`${first}T12:00:00Z`)).replace(',', '');
    const said = (await seeded.innerText().catch(() => '')).replace(/\s+/g, ' ').trim();
    check('the block’s day and time are said under First on, with Use today instead', said === `From the block selected in the week: ${day}, ${from} to ${until}. Use today instead`, said || 'nothing said');
    check('the date field is described by that line', ((await page.locator('#ns-first').getAttribute('aria-describedby')) ?? '').split(' ').includes('ns-seeded'));
    await inView(page, seeded.getByRole('button', { name: 'Use today instead' }), 'Use today instead');
    await settled(page);
    await shot(page, '23-first-on-from-a-block');
    // A time of the person's own: the line still names the day, which is still the block's.
    await page.fill('#ns-from', '18:00');
    await page.locator('#ns-from').dispatchEvent('change');
    const dayOnly = (await seeded.innerText().catch(() => '')).replace(/\s+/g, ' ').trim();
    check('once the time is the person’s own, the line names the day alone', dayOnly === `From the block selected in the week: ${day}. Use today instead`, dayOnly || 'nothing said');
    await page.fill('#ns-from', from);
    await page.locator('#ns-from').dispatchEvent('change');
    await d.getByText(/^Every week on /).click();
    await seeded.getByRole('button', { name: 'Use today instead' }).click();
    await settled(page);
    check('Use today instead goes back to today and the usual hour, and the line goes', (await page.inputValue('#ns-first')) === '2026-11-13' && (await page.inputValue('#ns-from')) === '20:00' && (await page.inputValue('#ns-until')) === '21:00' && (await seeded.count()) === 0, `${await page.inputValue('#ns-first')} ${await page.inputValue('#ns-from')}–${await page.inputValue('#ns-until')}`);
    check('the other answers are kept, and follow the date', (await d.locator('.choices input:checked + span').innerText()) === 'Every week on Friday' && (await page.evaluate(() => document.activeElement?.id)) === 'ns-first', await d.locator('.choices input:checked + span').innerText());
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.locator('dialog.confirm').getByRole('button', { name: 'Discard this show' }).click();
    await d.waitFor({ state: 'detached' });
  }
  // A date the person changes themselves needs no telling.
  {
    await page.getByRole('button', { name: 'New show' }).first().click();
    const d = dlg(page);
    await page.keyboard.type('Sunday Service');
    await page.keyboard.press('Enter');
    await d.locator('[data-panel="calendar.rule"]').waitFor();
    await page.fill('#ns-first', '2026-12-06');
    await page.locator('#ns-first').dispatchEvent('change');
    check('a date the person picks themselves takes the line away', (await d.locator('[data-testid="rule-seeded"]').count()) === 0);
  }
  await ctx.close();
}

// ---- 4. The DJ's setup: four parts, a first date refused, Save my show ----
{
  const station = new Station({ role: 'presenter' });
  const { ctx, page } = await open(station, { tab: false });
  const d = dlg(page, 'studio.show-setup');
  await d.waitFor({ timeout: 10000 });
  check('the setup opens by itself for a presenter with no show, in four parts', (await d.locator('.crumbs').innerText()).replace(/\s+/g, ' ').toUpperCase().includes('3 WHEN IT IS ON'), (await d.locator('.crumbs').innerText()).replace(/\s+/g, ' '));
  await page.locator('#sw-presenter').fill('Ailsa Drummond');
  await page.locator('#sw-presenter').press('Enter');
  await page.locator('#sw-show').fill('The Wee Small Hours');
  await page.locator('#sw-show').press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  check('the third part opens on the list', (await d.locator('.choices').count()) === 1 && (await d.locator('.seg5').count()) === 0);
  await page.fill('#sw-first', '2026-11-14');
  await page.locator('#sw-first').dispatchEvent('change');
  await page.fill('#sw-from', '21:00');
  await page.locator('#sw-from').dispatchEvent('change');
  await page.fill('#sw-until', '23:00');
  await page.locator('#sw-until').dispatchEvent('change');
  await d.getByText('Every week on Saturday').click();
  await settled(page);
  const refused = await d.locator('.warn.refused').innerText();
  check('a first date that is another show’s is refused, with the other show named', refused.includes('Saturday Night Kettle has that time.') && refused.includes('Saturdays, 20:00 to 23:00, with Shona Campbell and Catriona MacKenzie-Buchanan.') && (await d.getByRole('button', { name: 'Save my show' }).isDisabled()), refused.replace(/\n/g, ' '));
  await shot(page, '17-dj-setup-refused');
  await d.getByRole('button', { name: /^Try / }).click();
  await settled(page);
  check('the offered time lets it be saved', await d.getByRole('button', { name: 'Save my show' }).isEnabled(), `${await page.inputValue('#sw-from')} to ${await page.inputValue('#sw-until')}`);
  await shot(page, '18-dj-setup-when-it-is-on');
  await d.getByRole('button', { name: 'Save my show' }).click();
  await d.getByText('is set up as your show.').waitFor();
  const put = station.calls.find((c) => c.method === 'PUT' && c.path === '/api/v1/shows/the-wee-small-hours');
  check('Save my show saves the show and drafts the first episode in one request', put?.body.draft_first === true && put.body.rule.repeat.kind === 'weekly' && put.body.presenter === 'Ailsa Drummond' && put.body.presenter_account === 'a-ailsa', JSON.stringify(put?.body.rule?.repeat));
  const done = await d.locator('div.done').innerText();
  check('Done says episode 1 and the rule as the station says it', done.includes('Episode 1 is drafted for') && /It is on every week on Saturday, \d\d:\d\d to \d\d:\d\d/.test(done), done.replace(/\n/g, ' '));
  await shot(page, '19-dj-setup-done');
  await ctx.close();
}

// ---- 5. A station older than 1.0.4: a weekly time only, and nothing it would refuse ----
{
  const station = new Station({ version: '1.0.3' });
  const { ctx, page } = await open(station);
  check('an older station’s week has no dotted blocks, and the list offers the next episode', (await page.locator('.ec .ev.rule').count()) === 0 && (await page.locator('[data-panel="calendar.unscheduled"]').innerText()).includes('This station runs an older WeaveSignal'));
  await shot(page, '20-older-station-list');
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = dlg(page);
  await page.keyboard.type('Bothy Ballads');
  await page.keyboard.press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  check('only Every week on… can be picked', (await d.locator('.note').innerText()).includes('This station keeps a weekly time only.') && (await d.locator('input[value="once"]').isDisabled()) && (await d.getByRole('button', { name: 'Make the show', exact: true }).isDisabled()));
  await d.getByText(/^Every week on /).click();
  await shot(page, '21-older-station-new-show');
  await d.getByRole('button', { name: 'Make the show', exact: true }).click();
  await d.getByText('is a show.').waitFor();
  const put = station.calls.find((c) => c.method === 'PUT' && c.path === '/api/v1/shows/bothy-ballads');
  check('an older station is sent the weekly slot, and neither rule nor draft_first', !!put?.body.slot && !('rule' in put.body) && !('draft_first' in put.body) && previews(station).length === 0, JSON.stringify(put?.body.slot));
  await ctx.close();
}

// ---- 6. Light and Night, a wide window and a narrow one ----
// The window factor and the text size zoom the page, and under zoom `vh` grows with it: a dialog
// held to `100vh` ran off the bottom of a large window, its Make the show out of reach (the
// 1.0.4 test run). So the foot must be in view at every size, zoomed or not.
for (const [name, o] of [['light', { theme: 'light' }], ['night', { theme: 'night' }], ['2560', { width: 2560, height: 1440 }], ['2560-text-120', { width: 2560, height: 1440, text: 1.2 }], ['4k', { width: 3840, height: 2160 }], ['narrow', { width: 900, height: 700 }]]) {
  const station = new Station();
  const { ctx, page } = await open(station, o);
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = dlg(page);
  await page.keyboard.type('The Folk Club');
  await page.keyboard.press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  await page.fill('#ns-first', '2026-12-01');
  await page.locator('#ns-first').dispatchEvent('change');
  await d.getByText('Every month on the first Tuesday').click();
  await settled(page);
  await d.locator('[data-row="2027-01-05"]').getByRole('button', { name: 'Skip' }).click();
  await settled(page);
  {
    const h = page.viewportSize().height;
    const box = await d.boundingBox();
    const acts = await d.locator('.acts').last().boundingBox();
    const make = await d.locator('.acts button[type="submit"]').last().boundingBox();
    check(`${name}: the dialog and its foot (Make the show) are inside the window`, !!box && !!acts && !!make && box.y + box.height <= h && acts.y + acts.height <= h && make.y + make.height <= h, `window ${h} px, dialog ${Math.round(box?.y ?? 0)}–${Math.round((box?.y ?? 0) + (box?.height ?? 0))}, Make the show ends at ${Math.round((make?.y ?? 0) + (make?.height ?? 0))}`);
  }
  await shot(page, `22-when-it-airs-${name}`);
  await ctx.close();
}

// ---- 6b. A short, wide window: the questions scroll beside the preview, and their edges say so ----
{
  const station = new Station();
  const { ctx, page } = await open(station, { width: 1440, height: 640, text: 1 });
  await page.getByRole('button', { name: 'New show' }).first().click();
  const d = dlg(page);
  await page.keyboard.type('The Folk Club');
  await page.keyboard.press('Enter');
  await d.locator('[data-panel="calendar.rule"]').waitFor();
  await page.fill('#ns-first', '2026-12-01');
  await page.locator('#ns-first').dispatchEvent('change');
  await d.getByText('Every month on the first Tuesday').click();
  await settled(page);
  const qs = d.locator('.qs');
  await qs.evaluate((el) => (el.scrollTop = 0));
  await sleep(150);
  check('questions that do not all fit say there is more below', (await qs.getAttribute('data-more')) === 'below', `data-more="${await qs.getAttribute('data-more')}"`);
  await qs.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await sleep(150);
  check('and, scrolled down, that First on is above', (await qs.getAttribute('data-more')) === 'above', `data-more="${await qs.getAttribute('data-more')}"`);
  await shot(page, '24-short-window-edges');
  await ctx.close();
}

// ---- 7. Delete show (ADR-0279): the confirmation, the delete, a refusal, who is offered it ----
{
  const station = new Station();
  const { ctx, page } = await open(station);
  const delDialog = page.locator('dialog[data-panel="calendar.show-delete"]');
  const toast = page.locator('[data-testid="schedule-toast"]');
  const deletes = () => station.calls.filter((c) => c.method === 'DELETE');

  // From the Unscheduled list: Northern Lights has no dates, no series and no episodes.
  await page.locator('[data-panel="calendar.unscheduled"] summary').click();
  await page.locator('[data-panel="calendar.unscheduled"]').getByRole('button', { name: 'Northern Lights' }).click();
  const open1 = page.getByRole('button', { name: 'Delete show…' });
  await open1.waitFor();
  await inView(page, open1, 'Delete show… in the show editor');
  const save = await page.getByRole('button', { name: 'Save show' }).boundingBox();
  const del = await open1.boundingBox();
  check('Delete show… is at the foot, left of Save show', !!save && !!del && del.x + del.width <= save.x && Math.abs(del.y - save.y) < 40, `${Math.round(del?.x ?? 0)} against ${Math.round(save?.x ?? 0)}`);
  await open1.click();
  await delDialog.waitFor();
  await sleep(500);
  check('the dialog is headed Delete Northern Lights? and says what goes and that it cannot be undone', (await delDialog.locator('h2').innerText()) === 'Delete Northern Lights?' && (await delDialog.locator('#sd-p').innerText()) === 'Its cart banks and cards go with it. This cannot be undone.', await delDialog.innerText());
  check('Keep it has the focus', await page.evaluate(() => document.activeElement?.textContent === 'Keep it'));
  await inView(page, delDialog.getByRole('button', { name: 'Delete show', exact: true }), 'Delete show in the dialog');
  await inView(page, delDialog.getByRole('button', { name: 'Keep it' }), 'Keep it in the dialog');
  await shot(page, '30-delete-show-confirm');
  await page.keyboard.press('Enter');
  await sleep(300);
  check('Keep it closes it and deletes nothing', (await delDialog.count()) === 0 && deletes().length === 0);
  await open1.click();
  await delDialog.waitFor();
  await delDialog.getByRole('button', { name: 'Delete show', exact: true }).click();
  await toast.waitFor();
  await sleep(700);
  check('Delete show sends DELETE for that show', deletes().length === 1 && deletes()[0].path === '/api/v1/shows/northern-lights', JSON.stringify(deletes().map((c) => c.path)));
  check('the toast says it is deleted, with no Undo', (await toast.innerText()).includes('Northern Lights is deleted.') && (await toast.getByRole('button', { name: 'Undo' }).count()) === 0, await toast.innerText());
  check('the editor closes and the show leaves the list and the station', (await page.getByRole('button', { name: 'Delete show…' }).count()) === 0 && !(await page.locator('[data-panel="calendar.unscheduled"]').innerText()).includes('Northern Lights') && !station.shows.some((x) => x.slug === 'northern-lights'));
  check('the shows and the week were read again', station.calls.filter((c) => c.method === 'GET' && c.path === '/api/v1/shows').length >= 2);
  await shot(page, '31-delete-show-done');

  // From an episode's Show level: Saturday Night Kettle has one series and one drafted episode.
  await page.keyboard.press('ArrowRight');
  await sleep(500);
  await page.keyboard.press('ArrowLeft');
  await sleep(500);
  await page.locator('.ec .ev:not(.auto):not(.rule)').filter({ hasText: 'Saturday Night Kettle' }).first().click();
  await page.getByRole('tab', { name: 'Show' }).click();
  const open2 = page.getByRole('button', { name: 'Delete show…' });
  await open2.waitFor();
  await open2.click();
  await delDialog.waitFor();
  await sleep(500);
  check('the counts are the show’s own', (await delDialog.locator('#sd-p').innerText()) === 'Its 1 series and 1 drafted episode go with it, and its cart banks and cards. This cannot be undone.', await delDialog.locator('#sd-p').innerText());
  await page.keyboard.press('Escape');
  await sleep(300);
  check('Esc keeps the show too', (await delDialog.count()) === 0 && deletes().length === 1);

  // The station refuses a show that has aired, in its own sentence, and the show stays.
  await page.locator('.ec .ev:not(.auto):not(.rule)').filter({ hasText: 'Kettle Breakfast' }).first().click();
  await page.getByRole('tab', { name: 'Show' }).click();
  await page.getByRole('button', { name: 'Delete show…' }).click();
  await delDialog.waitFor();
  await delDialog.getByRole('button', { name: 'Delete show', exact: true }).click();
  await delDialog.getByRole('alert').waitFor();
  const refusal = await delDialog.getByRole('alert').innerText();
  check('a 409 shows the station’s sentence in the dialog and keeps the show', refusal === 'Kettle Breakfast has aired 4 episodes, so it stays as the station\'s record: only a show that has never aired can be deleted.' && station.shows.some((x) => x.slug === 'kettle-breakfast') && (await delDialog.count()) === 1 && (await delDialog.getByRole('button', { name: 'Delete show', exact: true }).isDisabled()), refusal);
  await inView(page, delDialog.getByRole('button', { name: 'Keep it' }), 'Keep it after a refusal');
  await shot(page, '32-delete-show-refused');
  await ctx.close();
}
{
  // A presenter, and a station older than 1.0.4, are not offered it.
  for (const [what, opts] of [['a presenter', { role: 'presenter' }], ['a station older than 1.0.4', { version: '1.0.3' }]]) {
    const station = new Station(opts);
    // Even the presenter of this show (a presenter with no show is met by the DJ's setup).
    station.shows.find((x) => x.slug === 'saturday-night').presenter_account = 'a-ailsa';
    const { ctx, page } = await open(station);
    await page.locator('.ec .ev:not(.auto):not(.rule)').filter({ hasText: 'Saturday Night Kettle' }).first().click();
    await page.getByRole('tab', { name: 'Show' }).click();
    await page.getByRole('button', { name: 'Save show' }).waitFor();
    check(`${what} is not offered Delete show…`, (await page.getByRole('button', { name: 'Delete show…' }).count()) === 0);
    await ctx.close();
  }
}

// ---- 8. A rule changed in another window: the tab says so and reads the week again ----
// The station tells every window (`rule.changed`), this one too. The tab says another person's
// change in its own line and reads the week and the shows again; its own change it has already
// said and drawn. Here the studio's view is answered, with the notices as the station holds them.
{
  const weekly = (weekdays, start, minutes) => ({ starts_on: '2026-09-07', start, minutes, repeat: { kind: 'weekly', every: 1, weekdays }, end: { kind: 'never' }, skip: [], moves: [] });
  const station = new Station();
  station.draft('saturday-night', '2026-11-21');
  // Something from before this window looked: it is the past, not news.
  station.ruleChanged('Callum', 'Firth Sessions');
  const { ctx, page } = await open(station, { events: true });
  const toast = page.locator('[data-testid="schedule-toast"]');
  const heard = page.locator('[data-testid="schedule-heard"]');
  const views = () => station.calls.filter((c) => c.path === '/api/v1/studio/events').length;
  const reads = (path) => station.calls.filter((c) => c.method === 'GET' && c.path.startsWith(path)).length;
  const seen = async (n) => {
    const from = views();
    await page.waitForFunction(() => true, null, { timeout: 100 }).catch(() => {});
    for (let i = 0; i < 80 && views() < from + n; i++) await sleep(100);
    await sleep(300);
  };
  await page.keyboard.press('ArrowRight');
  await sleep(700);
  await seen(2);
  check('a notice from before the window looked is not said', (await heard.count()) === 0 && (await toast.count()) === 0);

  // Someone is typing an episode's title when Moira moves Firth Sessions to Wednesdays at 19:00.
  await page.locator('.ec .ev:not(.auto):not(.rule)').filter({ hasText: 'Saturday Night Kettle' }).first().click();
  await page.locator('[aria-label="Title"]').fill('Hogmanay warm-up');
  const before = { shows: reads('/api/v1/shows'), week: reads('/api/v1/schedule/episodes') };
  station.changeElsewhere('Moira', 'firth-sessions', weekly([3], 1140, 60));
  await heard.waitFor({ timeout: 5000 });
  await sleep(700);
  check('another person’s change is said on the Schedule tab, in the station’s sentence', (await heard.innerText()).replace(/\s+/g, ' ').trim() === 'Moira changed when Firth Sessions airs. The week is up to date.', (await heard.innerText()).replace(/\s+/g, ' '));
  check('the week and the shows are read again', reads('/api/v1/shows') > before.shows && reads('/api/v1/schedule/episodes') > before.week, `shows ${before.shows} → ${reads('/api/v1/shows')}, week ${before.week} → ${reads('/api/v1/schedule/episodes')}`);
  const firth = page.locator('.ec .ev.rule').filter({ hasText: 'Firth Sessions' });
  check('the week draws the new dates', (await firth.count()) === 1 && (await firth.first().innerText()).includes('19:00'), (await firth.first().innerText().catch(() => 'no block')).replace(/\n/g, ' '));
  check('what was being typed in the editor is kept', (await page.locator('[aria-label="Title"]').inputValue()) === 'Hogmanay warm-up', await page.locator('[aria-label="Title"]').inputValue().catch(() => 'the editor closed'));
  await inView(page, heard, 'the line about another person’s change');
  await shot(page, '40-rule-changed-elsewhere');

  // The show editor open on a show whose rule changes elsewhere: a save must not send the old one back.
  await page.getByRole('tab', { name: 'Show' }).click();
  await page.locator('[data-panel="calendar.show-rule"]').waitFor();
  await sleep(400);
  station.changeElsewhere('Moira', 'saturday-night', weekly([7], 1200, 180));
  await page.waitForFunction(() => document.querySelector('[data-testid="schedule-heard"]')?.textContent?.includes('Saturday Night Kettle'), null, { timeout: 5000 }).catch(() => {});
  await sleep(900);
  check('an open show editor shows the rule as changed elsewhere', (await page.locator('[data-panel="calendar.show-rule"]').innerText()).includes('Sun'), (await page.locator('[data-panel="calendar.show-rule"]').innerText()).replace(/\n/g, ' '));
  await page.locator('[aria-label="Show name"]').fill('Saturday Night Kettle Live');
  await page.getByRole('button', { name: 'Save show' }).click();
  await sleep(600);
  const resave = station.calls.filter((c) => c.method === 'PUT' && c.path === '/api/v1/shows/saturday-night').at(-1);
  check('and its save carries the new rule, not the one it opened with', resave?.body.name === 'Saturday Night Kettle Live' && resave.body.rule?.repeat.weekdays?.[0] === 7, JSON.stringify(resave?.body.rule?.repeat));

  // This window's own change: Skip this date. Its toast and Undo stay; the station's notice of
  // it (Shona is who is signed in) arrives with the next views and is passed over.
  await page.waitForFunction(() => !document.querySelector('[data-testid="schedule-heard"]'), null, { timeout: 15000 }).catch(() => {});
  await firth.first().click();
  const weekBefore = weekReads(station);
  await page.getByRole('button', { name: 'Skip this date' }).click();
  await toast.waitFor();
  await seen(4);
  check('the station told this window of its own change too', station.notices.at(-1).title === 'Shona changed when Firth Sessions airs.', station.notices.at(-1).title);
  check('this window’s own change is not said back to it: Skipped and its Undo stay', (await toast.innerText()).includes('Firth Sessions will not air on Wednesday 18 November.') && (await toast.getByRole('button', { name: 'Undo' }).count()) === 1 && (await heard.count()) === 0, (await toast.innerText()).replace(/\n/g, ' '));
  check('and the week is not read again for it', weekReads(station) - weekBefore === skipWeekReads, `${weekReads(station) - weekBefore} reads of the week for the skip, ${skipWeekReads} without a notice`);

  // Another person's change while Undo is offered: both are said, and Undo is not lost.
  await page.locator('.ec .ev.rule').filter({ hasText: 'Kettle Breakfast' }).first().click();
  await page.getByRole('button', { name: 'Skip this date' }).click();
  await page.getByRole('button', { name: 'Undo' }).waitFor();
  station.changeElsewhere('Moira', 'six-nations-preview', { starts_on: '2027-03-02', start: 1200, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] });
  await heard.waitFor({ timeout: 5000 });
  check('another person’s change does not take Undo away', (await toast.innerText()).includes('Kettle Breakfast will not air on') && (await toast.innerText()).includes('Moira changed when Six Nations Preview airs.') && (await toast.getByRole('button', { name: 'Undo' }).count()) === 1, (await toast.innerText()).replace(/\n/g, ' '));
  await inView(page, toast.getByRole('button', { name: 'Undo' }), 'Undo beside another person’s change');
  await shot(page, '41-rule-changed-with-undo');
  await ctx.close();
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
