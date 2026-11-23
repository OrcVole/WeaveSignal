// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the Schedule tab's Studios view and the rehearsal studio's booking editor (1.3.0,
// `calendar.studios` and `rehearsal.booking`) on the screens loopback stack, checks each step
// against the control plane, and captures both at 1,440 × 900 and 2,560 × 1,440 in Dark, Light
// and Night. The stack is started with the view's own day seeded:
//
//   STUDIOS_DAY=1 tests/screens/run-screens.sh
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/screens/drive-studios.mjs
//
// Against a stack started with REHEARSAL=false it checks and captures the one state that has:
// the one column, and the line that says the rehearsal studio is off and how it is turned on.
//
// It runs as the station key, which may book for every show: a window with no show of its own
// (the column to read) and a booking for another presenter's show are covered without a
// browser, in apps/studio/src/studios.test.ts. Playwright is not a dependency of the
// repository. Screenshots go to OUT, never the repository.
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
  results.push({ name, ok: !!ok, detail });
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
const DAY = 86_400_000;
const bookings = async () => (await get(`/api/v1/rehearsal/bookings?from_ms=${Date.now() - DAY}&to_ms=${Date.now() + 4 * DAY}`)).bookings ?? [];
const hm = (ms) => new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, hourCycle: 'h23', hour: '2-digit', minute: '2-digit' }).format(new Date(ms));

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

/** Open the Studios view from the toolbar. */
async function studios(page) {
  await page.getByRole('button', { name: 'Studios', exact: true }).click();
  await page.waitForSelector('.ec-host[data-view="studios"]', { timeout: 10000 });
  await sleep(1200);
}
const editor = (page) => page.locator('[data-panel="rehearsal.booking"]');
const heading = async (page) => ((await editor(page).locator('h2').first().textContent()) ?? '').trim();
/** A column's left and right edges, from its head. */
const column = async (page, id) => {
  const b = await page.locator(`.studiohd[data-studio="${id}"]`).boundingBox();
  return b ? [b.x - 4, b.x + b.width + 4] : null;
};
/** Whether every block the locator finds stands inside the column. */
async function allIn(page, locator, id) {
  const col = await column(page, id);
  const boxes = await locator.evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).filter((r) => r.width > 0).map((r) => [r.left, r.right]));
  return { ok: !!col && boxes.length > 0 && boxes.every(([l, r]) => l >= col[0] && r <= col[1] + 12), n: boxes.length };
}

/** Press at one point, move to another, let go: a block dragged or resized, or a stretch dragged out. */
async function drag(page, from, to) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + (to.x - from.x) / 4, from.y + (to.y - from.y) / 4, { steps: 3 });
  await page.mouse.move(to.x, to.y, { steps: 6 });
  await page.mouse.up();
  await sleep(1500);
}
/** Whether the library gives the first block found a hand to drag it by; null when there is none. */
const hand = async (locator) => ((await locator.count()) ? ((await locator.first().getAttribute('class')) ?? '').includes('ec-draggable') : null);
const toastSaid = async (page) => ((await page.locator('[data-testid="schedule-toast"]').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ').trim();

const week = await get(`/api/v1/schedule/episodes?from_ms=${Date.now() - DAY}&to_ms=${Date.now() + DAY}`);
const columns = (week.studios ?? []).map((s) => s.id);
check('the station sends the Studios view’s columns with the week', columns[0] === 'studio', JSON.stringify(week.studios ?? null));

if (!columns.includes('rehearsal')) {
  // ---- the rehearsal studio is off at this station: the one column, and the line that says so ----
  for (const [w, h] of [
    [1440, 900],
    [2560, 1440],
  ]) {
    for (const theme of ['dark', 'light', 'night']) {
      const { ctx, page } = await open(w, h, theme);
      await studios(page);
      if (w === 1440 && theme === 'dark') {
        check('with the rehearsal studio off there is the one column, with no studio names under the days', (await page.locator('.studiohd').count()) === 0 && (await page.locator('.ec-host[data-columns="1"]').count()) === 1);
        const line = ((await page.locator('[data-testid="studios-line"]').textContent()) ?? '').trim();
        check('and the view says it is off, and how it is turned on', /off at this station/.test(line) && /REHEARSAL=true/.test(line), line);
        check('the strip of days is the Day view’s', (await page.locator('.ec-host[data-view="studios"] .strip button').count()) === 7);
      }
      await shot(page, `studios-off-${w}-${theme}`);
      await ctx.close();
    }
  }
} else {
  const seeded = (week.rehearsals ?? []).some((r) => r.kind === 'booking' && r.show === 'steeped') && (week.rehearsals ?? []).some((r) => r.kind === 'session');
  check('the Studios view’s day is seeded (STUDIOS_DAY=1 tests/screens/run-screens.sh)', seeded, `${(week.rehearsals ?? []).length} rehearsal blocks`);
  if (!seeded) {
    await browser.close();
    writeFileSync(join(OUT, 'results-studios.json'), JSON.stringify(results, null, 2));
    process.exit(1);
  }
  const steeped = week.episodes.find((e) => e.show.slug === 'steeped');

  // ---- the view at 1,440 × 900, dark ----
  const { ctx, page } = await open(1440, 900, 'dark');
  await studios(page);
  const names = await page.locator('.studiohd .nm b').allTextContents();
  check('Studios opens one day with a column for each studio', names.join(', ') === 'Studio, Rehearsal', names.join(', '));
  const caps = await page.locator('.studiohd .cap').allTextContents();
  check('each column says what it is', caps.join(' | ') === 'On air by the schedule | Never on air', caps.join(' | '));
  check('the view has its own panel name', (await page.locator('[data-panel="calendar.studios"]').count()) === 1);
  const strip = page.locator('.ec-host[data-view="studios"] .strip button');
  check('it shares the Day view’s strip of days, today pressed', (await strip.count()) === 7 && (await page.locator('.strip button[aria-pressed="true"] .today-word').count()) === 1);
  const inStudio = await allIn(page, page.locator('.ec-event.ev:not(.reh)'), 'studio');
  check('everything on air is in the Studio column', inStudio.ok, `${inStudio.n} blocks`);
  const inRehearsal = await allIn(page, page.locator('.ec-event.reh'), 'rehearsal');
  check('bookings and sessions are in the Rehearsal column', inRehearsal.ok, `${inRehearsal.n} blocks`);
  const booked = page.locator('.ec-event.reh.booking', { hasText: 'Steeped' });
  check('Steeped’s booking is drawn there', (await booked.count()) === 1, (await booked.first().textContent().catch(() => '')) ?? '');
  const hatch = await booked.first().evaluate((el) => getComputedStyle(el).backgroundImage).catch(() => '');
  check('a booking’s ground is hatched', /repeating-linear-gradient/.test(hatch), hatch.slice(0, 60));
  const session = page.locator('.ec-event.reh.session.past', { hasText: 'The Morning Kettle' });
  check('a session as it happened is drawn muted', (await session.count()) === 1, (await session.first().textContent().catch(() => '')) ?? '');
  const air = page.locator('.ec-event.ev:not(.reh)', { hasText: 'The Morning Kettle' });
  check('The Morning Kettle is on air beside it', (await air.locator('.flag.onair').count()) >= 1);
  const said = (await page.locator('.ec-event.reh').allTextContents()).join(' | ');
  check('nothing in the Rehearsal column says on air or open, or is flagged', !/on air|\bopen\b/i.test(said) && (await page.locator('.ec-event.reh .flag').count()) === 0 && (await page.locator('.ec-event.reh.conflict').count()) === 0, said.slice(0, 160));
  const withBooking = (await bookings()).find((b) => b.show === 'steeped');
  const overlapsAir = !!withBooking && week.episodes.some((e) => e.show.slug === 'the-morning-kettle' && e.episode.air_at * 1000 < withBooking.end_ms);
  check('the booking shares time with the show on air, and is no clash', overlapsAir && (await page.locator('.ec-event.reh.conflict').count()) === 0, `overlaps the air: ${overlapsAir}`);
  check('the now line crosses both columns, with its time once', (await page.locator('.ec-bg-event.now-line').count()) === 2 && (await page.locator('.now-line .now-label').count()) === 1);
  if (steeped?.from === 'own') check('a show whose host is in their own studio says Own studio', (await page.locator('.ec-event.ev:not(.reh) .kind.own').count()) >= 1);
  else log('note', 'the station did not say Steeped comes from its own studio: no Own studio mark to look for', JSON.stringify(steeped?.from ?? null));
  const oneLine = await page.locator('[data-panel="calendar.toolbar"] > *').evaluateAll((els) => {
    const t = els.filter((e) => e.getBoundingClientRect().width > 0).map((e) => e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2);
    return Math.max(...t) - Math.min(...t) < 12;
  });
  check('the toolbar is still one line at 1440 by 900', oneLine);
  const fits = await page.evaluate(() => {
    const m = document.querySelector('.ec-host[data-view="studios"] .ec-main');
    return m ? m.scrollWidth <= m.clientWidth + 1 : false;
  });
  check('both columns fit at 1440 with nothing to scroll sideways', fits);
  // What may be moved has the library's hand to drag it by, and nothing else has.
  const hands = {
    show: await hand(page.locator('.ec-event.ev:not(.reh):not(.auto):not(.past)', { hasText: 'Steeped' })),
    booking: await hand(booked),
    onAir: await hand(air),
    automation: await hand(page.locator('.ec-event.ev.auto')),
    session: await hand(session),
  };
  check('a show and a booking still to come can be dragged; the show on air, automation and a session cannot', hands.show === true && hands.booking === true && hands.onAir === false && hands.automation === false && hands.session === false, JSON.stringify(hands));
  await shot(page, 'studios-day-1440-dark');

  // A drag keeps to its own column: Steeped, still to come, does not go to the rehearsal studio.
  {
    const show = page.locator('.ec-event.ev.ec-draggable:not(.reh)', { hasText: 'Steeped' });
    const e0 = await show.first().boundingBox();
    const rehearsalCol = await column(page, 'rehearsal');
    const airAt = async () => (await get(`/api/v1/schedule/episodes?from_ms=${Date.now() - DAY}&to_ms=${Date.now() + DAY}`)).episodes.find((e) => e.show.slug === 'steeped')?.episode.air_at;
    const was = await airAt();
    if (e0 && rehearsalCol) {
      await drag(page, { x: e0.x + e0.width / 2, y: e0.y + 10 }, { x: (rehearsalCol[0] + rehearsalCol[1]) / 2, y: e0.y + 10 });
      const still = await allIn(page, page.locator('.ec-event.ev:not(.reh)'), 'studio');
      check('a show cannot be dragged into the Rehearsal column', (await airAt()) === was && still.ok, `${still.n} blocks still in Studio`);
      await page.keyboard.press('Escape');
      await sleep(400);
    } else check('a show cannot be dragged into the Rehearsal column', false, 'no show still to come to drag');
  }

  // ---- a session is only to read ----
  await session.first().click();
  await editor(page).waitFor({ timeout: 8000 });
  check('a session opens in the editor column, to read', /The Morning Kettle · rehearsed/.test(await heading(page)) && (await editor(page).locator('[data-testid="booking-read-only"]').count()) === 1 && (await editor(page).locator('input, select, button[type="submit"]').count()) === 0, await heading(page));
  await shot(page, 'booking-read-only-1440-dark');
  await page.keyboard.press('Escape');
  await sleep(400);
  check('Esc closes it, back to the list of shows', (await editor(page).count()) === 0 && (await page.locator('[data-panel="calendar.shows"]').count()) === 1);

  // ---- tomorrow: a new booking, a refusal, a save, a change, a delete ----
  await page.getByRole('button', { name: 'Next day' }).click();
  await sleep(900);
  const drive = page.locator('.ec-event.reh.booking', { hasText: 'Kettle Drive' });
  check('→ is the next day: Kettle Drive has the studio at 15:00', (await drive.count()) === 1);
  await page.locator('.studiohd .bookbtn').click();
  await editor(page).waitFor({ timeout: 8000 });
  check('Book… begins a booking in the editor column', (await heading(page)) === 'Book the rehearsal studio', await heading(page));
  check('and draws it on the grid before it is saved', (await page.locator('.ec-event.reh.draft').count()) === 1);
  const before = (await bookings()).length;
  await shot(page, 'booking-new-1440-dark');
  await page.getByLabel('The show the rehearsal studio is held for').selectOption('steeped');
  await page.getByLabel('From, station time').fill('15:15');
  await page.getByLabel('Until, station time').fill('15:45');
  await sleep(500);
  const shares = ((await editor(page).locator('[data-testid="booking-shares"]').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');
  check('a booking in the way is said before the station is asked', /Kettle Drive has the studio from 15:00 to 16:00/.test(shares), shares.trim());
  await page.getByRole('button', { name: 'Book the studio' }).click();
  await sleep(1200);
  const refused = ((await editor(page).locator('[data-testid="booking-refused"]').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');
  check('the station refuses two bookings at one time, and the editor names the one in the way', /Not saved\. Kettle Drive has the studio from 15:00 to 16:00/.test(refused) && (await bookings()).length === before, refused.trim());
  await shot(page, 'booking-refused-1440-dark');
  await page.getByLabel('From, station time').fill('11:00');
  await page.getByLabel('Until, station time').fill('12:00');
  await sleep(300);
  await page.getByRole('button', { name: 'Book the studio' }).click();
  await sleep(1500);
  let made = (await bookings()).find((b) => b.show === 'steeped' && hm(b.start_ms) === '11:00' && b.start_ms > Date.now());
  check('Book the studio books it', !!made && hm(made.end_ms) === '12:00', made ? `${hm(made.start_ms)} to ${hm(made.end_ms)} by ${made.by}` : 'no such booking');
  check('the saved booking stays open, to change', (await heading(page)) === 'Steeped · rehearsal booked' && (await page.locator('.ec-event.reh.draft').count()) === 0, await heading(page));
  const toast = ((await page.locator('[data-testid="schedule-toast"]').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');
  check('and the tab says so, with Undo', /Booked\. The rehearsal studio is held for Steeped/.test(toast) && /Undo/.test(toast), toast.trim());
  await shot(page, 'booking-change-1440-dark');
  await page.getByLabel('Until, station time').fill('12:30');
  await sleep(300);
  await page.getByRole('button', { name: 'Save booking' }).click();
  await sleep(1500);
  made = (await bookings()).find((b) => b.id === made?.id);
  check('Save booking sends the change', !!made && hm(made.end_ms) === '12:30', made ? `${hm(made.start_ms)} to ${hm(made.end_ms)}` : 'gone');

  // ---- a booking dragged, resized, dropped on another, and held to its own column ----
  const mine = page.locator('.ec-event.reh.booking', { hasText: 'Steeped' });
  const at = async () => (await bookings()).find((b) => b.id === made?.id);
  const times = (b) => (b ? `${hm(b.start_ms)} to ${hm(b.end_ms)}` : 'gone');
  let b0 = await mine.first().boundingBox();
  if (b0 && made) {
    // An hour is 48 px: down an hour.
    await drag(page, { x: b0.x + b0.width / 2, y: b0.y + 10 }, { x: b0.x + b0.width / 2, y: b0.y + 10 + 48 });
    made = await at();
    check('a booking dragged an hour down is an hour later, saved at once', times(made) === '12:00 to 13:30', times(made));
    check('and the tab says where it now is, with Undo', /^Moved\. Steeped has the rehearsal studio on .* 12:00 to 13:30\./.test(await toastSaid(page)) && /Undo/.test(await toastSaid(page)), await toastSaid(page));
    // Its lower edge, half an hour down.
    b0 = await mine.first().boundingBox();
    await drag(page, { x: b0.x + b0.width / 2, y: b0.y + b0.height - 3 }, { x: b0.x + b0.width / 2, y: b0.y + b0.height - 3 + 24 });
    made = await at();
    check('its lower edge dragged half an hour down makes it half an hour longer', times(made) === '12:00 to 14:00', times(made));
    // Two hours down would put it on Kettle Drive's 15:00 to 16:00: refused, and it snaps back.
    // Where it stands is measured from Kettle Drive's block: the tab's line above the grid comes and goes.
    const above = async () => ((await drive.first().boundingBox())?.y ?? 0) - ((await mine.first().boundingBox())?.y ?? 0);
    const gap = await above();
    b0 = await mine.first().boundingBox();
    await drag(page, { x: b0.x + b0.width / 2, y: b0.y + 10 }, { x: b0.x + b0.width / 2, y: b0.y + 10 + 96 });
    const kept = await at();
    check('dropped on another booking it is refused, in the tab’s line, and goes back where it was', times(kept) === '12:00 to 14:00' && /^Not moved Kettle Drive has the studio from 15:00 to 16:00/.test(await toastSaid(page)) && Math.abs((await above()) - gap) < 2, `${times(kept)} | ${await toastSaid(page)} | ${gap} px above Kettle Drive before, ${await above()} after`);
    // Across into the Studio column: a booking does not go on air.
    const studioCol = await column(page, 'studio');
    b0 = await mine.first().boundingBox();
    await drag(page, { x: b0.x + b0.width / 2, y: b0.y + 10 }, { x: (studioCol[0] + studioCol[1]) / 2, y: b0.y + 10 });
    const stayed = await allIn(page, mine, 'rehearsal');
    check('a booking cannot be dragged into the Studio column', times(await at()) === '12:00 to 14:00' && stayed.ok);
    await page.keyboard.press('Escape');
    await sleep(300);
  }

  // A drag across empty Rehearsal time: an hour, starting half an hour after Kettle Drive's booking.
  const box = await drive.first().boundingBox();
  if (box) {
    const x = box.x + box.width / 2;
    await page.keyboard.press('Escape');
    await sleep(300);
    await page.mouse.move(x, box.y + box.height + 30);
    await page.mouse.down();
    await page.mouse.move(x, box.y + box.height + 50, { steps: 4 });
    await page.mouse.move(x, box.y + box.height + 66, { steps: 4 });
    await page.mouse.up();
    await sleep(800);
    const from = await page.getByLabel('From, station time').inputValue().catch(() => '');
    const until = await page.getByLabel('Until, station time').inputValue().catch(() => '');
    check('a drag across empty Rehearsal time begins a booking for that time', (await page.locator('.ec-event.reh.draft').count()) === 1 && from === '16:30' && until === '17:30', `${from} to ${until}`);
    await page.getByRole('button', { name: 'Discard', exact: true }).click().catch(() => {});
    await sleep(300);
  }
  // A drag never takes a show to the rehearsal studio.
  const moved = await bookings();
  check('nothing was booked by the drag until it is saved', moved.length === before + 1, `${moved.length} bookings`);

  await page.locator('.ec-event.reh.booking', { hasText: 'Steeped' }).first().click();
  await editor(page).waitFor({ timeout: 8000 });
  await page.getByRole('button', { name: 'Delete booking' }).click();
  await sleep(1500);
  check('Delete booking frees the studio', !(await bookings()).some((b) => b.id === made?.id) && (await editor(page).count()) === 0);
  const gone = ((await page.locator('[data-testid="schedule-toast"]').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');
  check('and the tab offers it back', /Booking deleted\./.test(gone) && /Undo/.test(gone), gone.trim());
  await page.locator('[data-testid="schedule-toast"]').getByRole('button', { name: 'Undo' }).click();
  await sleep(1500);
  const back = (await bookings()).find((b) => b.show === 'steeped' && b.start_ms === made?.start_ms);
  check('Undo books it again', !!back && back.end_ms === made?.end_ms, back ? `${hm(back.start_ms)} to ${hm(back.end_ms)}` : 'no such booking');
  if (back) await api('DELETE', `/api/v1/rehearsal/bookings/${back.id}`);

  // ---- a day with nothing booked ----
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Next day' }).click();
  await sleep(900);
  const line = ((await page.locator('[data-testid="studios-line"]').textContent().catch(() => '')) ?? '').trim();
  check('a day with nothing booked says so, and how to begin', /^Nothing is booked in the rehearsal studio on /.test(line) && /Book…/.test(line), line);
  check('and its Rehearsal column is empty', (await page.locator('.ec-event.reh').count()) === 0);
  await shot(page, 'studios-empty-1440-dark');

  // ---- the name plates (ADR-0070): none over a control in the Studios view ----
  const audit = await ctx.newPage();
  await audit.goto(`${STUDIO}?audit=1`);
  await audit.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
  await audit.keyboard.press('Control+5');
  await audit.waitForSelector('.schedule .ec', { timeout: 15000 });
  await studios(audit);
  const plates = await audit.evaluate(() => document.getElementById('pn-audit')?.textContent ?? '');
  check('no name plate lies over a control in the Studios view', /plates \d+/.test(plates) && !/OVER/.test(plates), plates.split('\n')[0]);
  log('note', plates.includes('calendar.studios') ? 'calendar.studios has its plate' : 'calendar.studios has no plate: it is not in the registry yet (packages/ui/panels.ts)');
  await audit.screenshot({ path: join(OUT, 'studios-1440-dark-plates.png') });
  await ctx.close();

  // ---- the other sizes and themes ----
  for (const [w, h] of [
    [1440, 900],
    [2560, 1440],
  ]) {
    for (const theme of ['dark', 'light', 'night']) {
      const { ctx: c, page: p } = await open(w, h, theme);
      await studios(p);
      if (!(w === 1440 && theme === 'dark')) await shot(p, `studios-day-${w}-${theme}`);
      if (w === 2560 && theme === 'dark') {
        const wide = await allIn(p, p.locator('.ec-event.reh'), 'rehearsal');
        check('at 2,560 the two columns share the width, each block in its own', wide.ok, `${wide.n} blocks`);
      }
      // The booking editor on Steeped's booking, while it can still be changed; else as it reads once over.
      await p.locator('.ec-event.reh.booking', { hasText: 'Steeped' }).first().click();
      await editor(p).waitFor({ timeout: 8000 });
      await sleep(400);
      await shot(p, `booking-${w}-${theme}`);
      await p.keyboard.press('Escape');
      await sleep(300);
      // A session, to read.
      await p.locator('.ec-event.reh.session').first().click();
      await editor(p).waitFor({ timeout: 8000 });
      await sleep(300);
      if (!(w === 1440 && theme === 'dark')) await shot(p, `booking-read-only-${w}-${theme}`);
      await p.keyboard.press('Escape');
      await p.getByRole('button', { name: 'Next day' }).click();
      await sleep(600);
      await p.locator('.studiohd .bookbtn').click();
      await editor(p).waitFor({ timeout: 8000 });
      await sleep(300);
      if (!(w === 1440 && theme === 'dark')) await shot(p, `booking-new-${w}-${theme}`);
      await p.getByLabel('From, station time').fill('15:15');
      await p.getByLabel('Until, station time').fill('15:45');
      await p.getByRole('button', { name: 'Book the studio' }).click();
      await sleep(1000);
      await shot(p, `booking-refused-${w}-${theme}`);
      await p.getByRole('button', { name: 'Discard', exact: true }).click();
      await p.getByRole('button', { name: 'Next day' }).click();
      await sleep(700);
      await shot(p, `studios-empty-${w}-${theme}`);
      await c.close();
    }
  }
}

await browser.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(OUT, 'results-studios.json'), JSON.stringify(results, null, 2));
log(`${results.length - failed.length} of ${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
