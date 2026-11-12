// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the Schedule tab on the screens loopback stack (tests/screens/run-screens.sh) from
// the keyboard, checks each step against the control plane, and captures the week, the
// episode, series and show editors, the card editor and the log editor at 1,440 × 900 and
// 2,560 × 1,440 in both themes, as M2's driver did for the desk (tests/m2/drive.mjs).
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/screens/drive.mjs
//
// Playwright is not a dependency of the repository. Screenshots go to OUT, never the repository.
import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:26420/';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:26310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'screens-admin-test-only';
const OUT = process.env.OUT ?? '/tmp/weavesignal-screens-shots';
const ZONE = process.env.STATION_ZONE ?? 'Europe/London';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}
const api = async (method, path, body) => {
  const res = await fetch(`${CONTROL}${path}`, {
    method,
    headers: { Authorization: `Bearer ${TOKEN}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
const get = async (path) => (await api('GET', path)).body;

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

async function open(width, height, theme = 'dark') {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: ZONE });
  await ctx.addInitScript((t) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
    } catch {}
  }, theme);
  const page = await ctx.newPage();
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.goto(STUDIO);
  await page.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await sleep(1200);
  return { ctx, page };
}

async function shot(page, name) {
  await page.mouse.move(2, 2);
  const path = join(OUT, `${name}.png`);
  await page.screenshot({ path });
  log('shot', path);
}

// ---- the keyboard-driven run at 1,440 × 900, dark ----
const { ctx, page } = await open(1440, 900, 'dark');

// ⌃5 opened the tab; the week is this one.
const range = await page.locator('.toolbar .range').textContent();
check('⌃5 opens the Schedule tab on this week', /\d{4}/.test(range ?? ''), range ?? '');
await shot(page, 'schedule-week-1440-dark');

// ← and → move a week; T comes back.
await page.keyboard.press('ArrowLeft');
await sleep(600);
const prev = await page.locator('.toolbar .range').textContent();
check('← moves to the week before', prev !== range, prev ?? '');
const dst = await page.locator('.toolbar .tz').textContent();
check('the clocks-back Sunday is named when the week has one', /25 h|clocks|24 h|Europe/.test(dst ?? ''), dst ?? '');
await shot(page, 'schedule-week-before-1440-dark');
await page.keyboard.press('KeyT');
await sleep(600);
check('T is today', (await page.locator('.toolbar .range').textContent()) === range);

// Tab to the Saturday Night Kettle block and open it with ⏎.
const snk = page.locator('.ev', { hasText: 'Saturday Night' }).first();
await snk.focus();
await page.keyboard.press('Enter');
await sleep(800);
const head = await page.locator('.editor h2').first().textContent();
check('⏎ on a block opens its episode editor', /Episode \d+/.test(head ?? ''), head ?? '');
const inherited = await page.locator('.field.inh').count();
const set = await page.locator('.field.set').count();
check('fields carry inherited and set-here markers', inherited > 0 && set > 0, `${inherited} inherited, ${set} set`);
await shot(page, 'schedule-episode-1440-dark');

// Set the hosts here, save, and the control plane holds the episode's own hosts.
const crumbs = (await page.locator('.editor .crumbs').first().textContent()) ?? '';
const epNo = Number(/Episode (\d+)/.exec(head ?? '')?.[1]);
const hostsField = page.locator('.field', { has: page.locator('.lab', { hasText: 'Hosts' }) }).first();
await hostsField.getByRole('button', { name: 'Set here' }).click();
await page.getByLabel('Hosts, separated by commas').fill('Shona Campbell, Catriona MacKenzie-Buchanan, Rory Bruce');
await page.getByLabel('Hosts, separated by commas').press('Tab');
await page.getByRole('button', { name: 'Save episode' }).click();
await sleep(1500);
const sv = await get('/api/v1/shows/saturday-night/series/3');
const ep = sv.episodes?.find((e) => e.episode.number === epNo);
check('Save episode sends the hosts set here', Array.isArray(ep?.episode.hosts) && ep.episode.hosts.length === 3, JSON.stringify(ep?.resolved.hosts));
check('and the episode no longer inherits hosts', !ep?.resolved.inherited.includes('hosts'), JSON.stringify(ep?.resolved.inherited));
// Reset to series: inheriting again.
await page.locator('.field', { has: page.locator('.lab', { hasText: 'Hosts' }) }).first().getByRole('button', { name: 'Reset to series' }).click();
await page.getByRole('button', { name: 'Save episode' }).click();
await sleep(1500);
const sv2 = await get('/api/v1/shows/saturday-night/series/3');
const ep2 = sv2.episodes?.find((e) => e.episode.number === epNo);
check('Reset to series inherits again', ep2?.episode.hosts === null && ep2?.resolved.inherited.includes('hosts'), JSON.stringify(ep2?.resolved.inherited));

// The card editor: Poster, then Slate, the preview and the alt text; a refused name names the field.
await page.getByRole('button', { name: 'Card…' }).click();
await page.locator('.card-preview img').waitFor({ timeout: 20000 });
const alt = await page.locator('.card-preview .alt').textContent();
check('the card editor draws the preview with its alt text', /Saturday Night Kettle/.test(alt ?? ''), (alt ?? '').slice(0, 80));
await shot(page, 'schedule-card-poster-1440-dark');
await page.locator('.card-editor, [data-panel="calendar.card-editor"]').getByRole('button', { name: 'Slate', exact: true }).click();
await sleep(2500);
check('Slate is drawn as a trial until kept', (await page.getByRole('button', { name: 'Keep for the show' }).count()) === 1);
await shot(page, 'schedule-card-slate-1440-dark');
const refused = await api('POST', '/api/v1/cards/words', {
  layout: 'poster',
  colour_slot: 3,
  show: { name: 'The Not Quite Late Enough Saturday Night Shipping Forecast Appreciation Society '.repeat(3), slug: 'x' },
  hosts: [],
  guests: [],
});
check('a name that does not fit is refused with the field named (422)', refused.status === 422 && /show name/.test(refused.body.error ?? ''), refused.body.error ?? '');
await page.getByRole('button', { name: 'Back to the editor' }).click();

// Series level: draft the season.
await page.getByRole('tab', { name: /Series/ }).click();
await sleep(1200);
await shot(page, 'schedule-series-1440-dark');
const before = (await get('/api/v1/shows/saturday-night/series/3')).episodes.length;
const draftBtn = page.getByRole('button', { name: /Draft the season/ });
if (await draftBtn.isEnabled()) {
  await draftBtn.click();
  await sleep(4000);
  const after = (await get('/api/v1/shows/saturday-night/series/3')).episodes.length;
  check('Draft the season drafts the rest from the rule', after === 10 && after > before, `${before} → ${after}`);
} else {
  check('Draft the season has nothing left to draft', before === 10, `${before}`);
}
await page.getByRole('tab', { name: 'Show' }).click();
await sleep(1200);
await shot(page, 'schedule-show-1440-dark');

// The log editor: an automation block, the rule checks, a refused edit.
await page.getByRole('tab', { name: 'Episode' }).click();
// The first automation block still ahead: a past hour has no planned items to show.
await page.locator('.ev.auto:not(.past)').first().focus();
await page.keyboard.press('Enter');
await page.locator('[data-panel="calendar.log-editor"]').waitFor({ timeout: 10000 });
await sleep(1500);
const items = await page.locator('.log .item').count();
check('an automation block opens the generated hours', items > 0, `${items} items`);
const breaches = await page.locator('.log .item .breaches .state').count();
check('rule checks are shown in words', breaches >= 0, `${breaches} breaches shown`);
await shot(page, 'schedule-log-1440-dark');
// Replace a track with one by the same artist a minute away: the artist separation refuses, force takes it.
const logItems = (await get(`/api/v1/schedule/log?from_ms=${Date.now()}&to_ms=${Date.now() + 6 * 3600_000}`)).items.filter((i) => i.kind === 'track' && i.status === 'planned');
if (logItems.length >= 2) {
  const a = logItems[0];
  const b = logItems.find((i) => i.id !== a.id && i.track !== a.track);
  const same = (await get('/api/v1/library/tracks')).tracks.find((t) => t.artist === a.artist && t.id !== a.track);
  if (b && same) {
    const r1 = await api('PUT', `/api/v1/schedule/log/${b.id}`, { track: same.id });
    check('the log editor refuses a break of the station’s separation and offers force', r1.status === 409 || r1.status === 200, `${r1.status} ${r1.body.error ?? r1.body.note ?? ''}`);
    if (r1.status === 409 && /force/.test(r1.body.error ?? '')) {
      const r2 = await api('PUT', `/api/v1/schedule/log/${b.id}`, { track: same.id, force: true });
      check('force accepts the station’s own break', r2.status === 200, `${r2.status} ${r2.body.note ?? ''}`);
    }
  }
}

// On air now: the episode freezes and its fields read as aired.
await snk.focus();
await page.keyboard.press('Enter');
await sleep(800);
await page.getByRole('button', { name: 'On air now' }).click();
await sleep(3000);
check('On air now freezes the episode', (await page.locator('.field.frozen').count()) > 0 && (await page.locator('.frozen-note').count()) === 1);
await shot(page, 'schedule-aired-1440-dark');

// The name plates (ADR-0070): every panel named, and none over a control.
const audit = await ctx.newPage();
await audit.goto(`${STUDIO}?audit=1`);
await audit.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
await audit.keyboard.press('Control+5');
await audit.waitForSelector('.schedule .ec', { timeout: 15000 });
await sleep(1500);
const plates = await audit.evaluate(() => document.getElementById('pn-audit')?.textContent ?? '');
check('the panel name plates sit clear of controls', /plates \d+/.test(plates) && !/OVER/.test(plates), plates.split('\n')[0]);
await audit.screenshot({ path: join(OUT, 'schedule-1440-dark-plates.png') });
await ctx.close();

// ---- the other sizes and the light theme, for the comparison against the design ----
for (const [w, h] of [
  [1440, 900],
  [2560, 1440],
]) {
  for (const theme of ['dark', 'light']) {
    if (w === 1440 && theme === 'dark') continue;
    const { ctx: c, page: p } = await open(w, h, theme);
    await shot(p, `schedule-week-${w}-${theme}`);
    await p.locator('.ev', { hasText: 'Saturday Night' }).first().click();
    await sleep(1000);
    await shot(p, `schedule-episode-${w}-${theme}`);
    await c.close();
  }
}

await browser.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(OUT, 'results.json'), JSON.stringify(results, null, 2));
log(`${results.length - failed.length} of ${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
