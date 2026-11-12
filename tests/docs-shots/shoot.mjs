// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Writes the product documentation's screenshots (docs/src/assets/screens/*.png) from the
// running interface, so they can be made again after the interface changes. The stacks are the
// repository's own loopback stacks, one at a time (each stage names its stack):
//
//   screens  the Schedule tab, the editors and Settings: tests/screens/run-screens.sh (:26420)
//   desk     the desk, screener and carts against the real desk: tests/m2/run-studio.sh (:27420)
//   public   the player, join page and echo test at phone width: tests/m3/run-join.sh (:32430)
//   chat     the moderators' Needs you and statement, the player's chat: WAIT=1 tests/chat/run-stack.sh (:37420, :37430)
//   signin   the packaged app's sign-in screens, on the `screens` stack
//   steps    the beginner steps (ADR-0271): the step control, its dialog, Show everything, the desk
//            at Solo and at Callers. No stack: the sandbox build, served as static files (:27911)
//   prepare  Prepare mode (ADR-0276): the Live or Preparing control, its dialog, the band and the
//            desk of a preparing window. No stack: the sandbox build, as `steps`
//
// `desk` restarts playout, sets playout's auto mode and writes a fallback into tests/m2/run/playout.json
// (a fresh stack each time; it runs last through a Dump, which the delay takes minutes to rebuild from).
//
//   WEAVESIGNAL_TEST_PREFIX=weavesignal-shots tests/screens/run-screens.sh      # then, in another shell:
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/docs-shots/shoot.mjs screens
//
// `steps` and `prepare` need no stack. Their pictures come from the sandbox build of the studio
// app (the same interface against a station simulated in the page, with Kettle FM's people),
// because a show on air at a chosen step with callers waiting is a state no loopback stack sets
// up yet; the sandbox's own guide bar is hidden, so each picture is the studio app's window:
//
//   just sandbox-build && (cd apps/studio/dist-sandbox && python3 -m http.server 27911 --bind 127.0.0.1)
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/docs-shots/shoot.mjs steps
//
// SANDBOX names another address. The sounds (/media/sandbox/) need not be there.
//
// Start a stack, run its stage, stop the stack (Ctrl+C), start the next. A stage first puts the
// stack in the state the pages describe (staff, a token, a hold, callers ready), all through
// the control plane's routes with the stack's own test-only bearer, and uses sample names from
// the seeds (ADR-0071). Pictures are cropped to the panel the page is about (`data-panel`,
// ADR-0070) or are a whole 1,440 × 900 window, in the app's default theme, with no browser
// chrome; each is compressed with pngquant (65-90). OUT defaults to docs/src/assets/screens.
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs).
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const OUT = process.env.OUT ?? join(root, 'docs/src/assets/screens');
const ZONE = 'Europe/London';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

/** Make a stack's `api`: JSON calls with its test-only bearer. */
function apiFor(control, token) {
  return async (method, path, body) => {
    const res = await fetch(`${control}${path}`, {
      method,
      headers: { Authorization: `Bearer ${token}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await res.text();
    let json = {};
    try {
      json = JSON.parse(text);
    } catch {}
    return { status: res.status, body: json };
  };
}

/** A new page in a fresh context: the app's default theme (dark), station time. */
async function open(url, { width = 1440, height = 900, ready, mobile = false, args } = {}) {
  const ctx = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: mobile ? 2 : 1,
    isMobile: mobile,
    hasTouch: mobile,
    timezoneId: ZONE,
    locale: 'en-GB',
    permissions: [],
  });
  await ctx.addInitScript(() => {
    try {
      localStorage.setItem('weavesignal.theme', 'dark');
    } catch {}
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => log('page error', e.message));
  await page.goto(url);
  if (ready) await ready(page);
  return { ctx, page };
}

/** Capture a panel (a locator) or the whole window (a page) to OUT/name.png and compress it. */
async function shot(target, name, { pause = 300, blur = false } = {}) {
  const page = target.page ? target.page() : target;
  await page.mouse.move(2, 2); // no hover state, no name plate
  if (blur) await page.evaluate(() => document.activeElement?.blur()); // nor a focused panel's plate
  await sleep(pause);
  const path = join(OUT, `${name}.png`);
  await target.screenshot({ path });
  try {
    execFileSync('pngquant', ['--quality', '65-90', '--skip-if-larger', '--force', '--ext', '.png', path]);
  } catch (e) {
    if (e.status !== 98 && e.status !== 99) throw e; // 98/99: nothing smaller, kept as it is
  }
  log('shot', name);
}
/** One picture of several panels side by side: the box that holds them all. */
async function shotUnion(page, names, name, opts) {
  const boxes = [];
  for (const n of names) boxes.push(await panel(page, n).boundingBox());
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  const x2 = Math.max(...boxes.map((b) => b.x + b.width));
  const y2 = Math.max(...boxes.map((b) => b.y + b.height));
  const clip = { x, y, width: x2 - x, height: y2 - y };
  const pageish = { page: () => page, screenshot: (o) => page.screenshot({ ...o, clip }) };
  await shot(pageish, name, opts);
}
const panel = (page, name) => page.locator(`[data-panel="${name}"]`).first();

// ====================================================================================
// screens: the Schedule tab, the editors, Settings (tests/screens/run-screens.sh)
// ====================================================================================
async function screens() {
  const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:26420/';
  const api = apiFor(process.env.CONTROL ?? 'http://127.0.0.1:26310', process.env.ADMIN_TOKEN ?? 'screens-admin-test-only');
  // The pictures show a station that is set up, not the setup notice (ADR-0267).
  await api('POST', '/api/v1/setup/finish');

  // Staff for the Settings pages: names from the seeds, roles mixed (the passwords are never shown).
  for (const [username, name, role] of [
    ['eilidh', 'Eilidh Fraser-MacLeod', 'admin'],
    ['ailsa', 'Ailsa MacMillan', 'producer'],
    ['moira', 'Moira Campbell', 'screener'],
  ]) {
    const r = await api('POST', '/api/v1/staff', { username, name, role, password: 'a-long-test-only-phrase' });
    if (![201, 409].includes(r.status)) log('staff', username, r.status);
  }

  const { page } = await open(STUDIO, {
    ready: async (p) => {
      await p.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
    },
  });

  // --- Settings, before the zone is chosen: the page on scheduling starts here ---
  await page.getByRole('button', { name: 'Settings' }).click();
  await panel(page, 'settings.zone').waitFor();
  await sleep(800);
  const use = page.getByRole('button', { name: /^Use / });
  if (await use.count()) {
    // A fresh station: the zone is unchosen. (On a stack where it is chosen already, the earlier picture stands.)
    await shot(panel(page, 'settings.zone'), 'how-to-schedule-the-week-time-zone');
    await use.click();
    await sleep(1200);
  }

  // --- Staff ---
  await shot(panel(page, 'settings.staff'), 'how-to-add-staff-list');
  await page.setViewportSize({ width: 1440, height: 1250 }); // the dialogs are tall
  const dlg = page.locator('dialog[open]');
  // Give a person an address for their sign-in codes: the dialog, an address being typed.
  await page.getByRole('button', { name: /^Email… for Ailsa MacMillan/ }).click();
  await dlg.waitFor();
  await dlg.locator('#se-email').fill('ailsa.macmillan@kettle.example');
  await sleep(500);
  await shot(dlg, 'how-to-add-staff-email');
  await dlg.getByRole('button', { name: /^Cancel/ }).click();
  await sleep(500);
  await page.getByRole('button', { name: 'Add someone…' }).click();
  await dlg.waitFor();
  await dlg.locator('#ad-name').fill('Gregor Stewart');
  await dlg.locator('#ad-name').press('Tab'); // the form offers the username
  await dlg.locator('#ad-role').selectOption({ label: 'Presenter' });
  await sleep(500);
  await shot(dlg, 'how-to-add-staff-add-form');
  await dlg.getByRole('button', { name: /^Cancel/ }).click();
  await sleep(500);

  // --- API tokens: make one, as the page says, and show the form and the list ---
  await page.getByRole('button', { name: 'Make a token…' }).click();
  await dlg.waitFor();
  await dlg.locator('#tk-name').fill('Stream deck');
  const carts = dlg.locator('.group', { has: page.locator('.gname', { hasText: /^carts$/i }) });
  for (const action of ['read', 'fire']) await carts.locator('label.scope', { has: page.locator('.mono', { hasText: new RegExp(`^${action}$`) }) }).locator('input').check();
  await sleep(500);
  await shot(dlg, 'how-to-connect-an-integration-make-token');
  await dlg.getByRole('button', { name: 'Make the token' }).click();
  await page.getByRole('button', { name: 'I have copied it' }).click(); // the secret is shown once: not in a picture
  await sleep(800);
  await shot(panel(page, 'settings.tokens'), 'how-to-connect-an-integration-tokens');
  await page.setViewportSize({ width: 1440, height: 900 });

  // --- Retention: the UK, a hold ---
  await page.getByLabel('Jurisdiction').selectOption({ label: 'UK, internet-only' });
  await sleep(800);
  await shot(panel(page, 'settings.jurisdiction'), 'how-to-set-retention-jurisdiction');
  await shot(panel(page, 'settings.periods'), 'how-to-set-retention-periods');
  const hold = panel(page, 'settings.holds');
  const now = Date.now();
  const local = (t) => new Date(t).toISOString().slice(0, 16);
  await hold.getByLabel('From').fill(local(now - 4 * 3600_000));
  await hold.getByLabel('To').fill(local(now - 2 * 3600_000));
  await hold.getByLabel('Reason').fill('Complaint about the 17:00 phone-in, received by email');
  await hold.getByLabel('Your name').fill('Eilidh Fraser-MacLeod');
  await hold.getByRole('button', { name: 'Place hold' }).click();
  await sleep(1500);
  await shot(hold, 'how-to-set-retention-holds');

  // --- The Schedule tab ---
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await sleep(1500);
  await shot(page, 'reference-calendar-week');
  await shot(panel(page, 'calendar.toolbar'), 'reference-calendar-toolbar');

  // A dotted weekly-rule block: not drafted, with the button that drafts it
  // (the seed drafts a few weeks ahead: page forward to the first week with an undrafted instance)
  for (let i = 0; i < 8 && !(await page.locator('.ev.rule').count()); i++) {
    await page.keyboard.press('ArrowRight');
    await sleep(800);
  }
  await page.locator('.ev.rule').first().focus();
  await page.keyboard.press('Enter');
  await page.getByRole('button', { name: 'Draft this episode' }).waitFor({ timeout: 10000 });
  await sleep(800);
  await page.setViewportSize({ width: 1440, height: 520 }); // a short window: the editor is not a tall empty column
  await sleep(600);
  await shot(page.locator('aside.editor'), 'how-to-schedule-the-week-draft-from-rule');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('button', { name: /^Today/ }).click();
  await sleep(1000);

  // New show
  await page.getByRole('button', { name: 'New show' }).click();
  await sleep(800);
  await shot(page, 'how-to-schedule-the-week-new-show');
  await page.keyboard.press('Escape');
  await page.locator('.ev', { hasText: 'Saturday Night' }).first().focus();
  await page.keyboard.press('Enter');
  await panel(page, 'calendar.episode-editor').waitFor();
  await sleep(1200);
  const editor = page.locator('aside.editor');
  await shot(editor, 'reference-calendar-episode-editor');

  // The card editor: Poster, then Slate
  await page.getByRole('button', { name: 'Card…' }).click();
  await page.locator('.card-preview img').waitFor({ timeout: 20000 });
  await sleep(1500);
  await shot(editor, 'reference-cards-poster-editor');
  await panel(page, 'calendar.card-editor').getByRole('button', { name: 'Slate', exact: true }).click();
  await sleep(3000);
  await shot(editor, 'reference-cards-slate-editor');
  await page.getByRole('button', { name: 'Back to the editor' }).click();
  await sleep(600);

  // Series and show levels
  await page.getByRole('tab', { name: /Series/ }).click();
  await sleep(1200);
  await shot(editor, 'reference-calendar-series-editor');

  // The show level: a slate picture chosen (the station's demo still), with its size
  await page.getByRole('tab', { name: /^Show/ }).click();
  await panel(page, 'calendar.show-still').waitFor({ timeout: 10000 });
  await page.locator('input[type=file][aria-label^="Choose the show"]').setInputFiles(join(root, 'assets/stills-demo/kettle-fm.png'));
  await page.locator('.still-preview img').waitFor({ timeout: 20000 });
  await page.getByText(/\d+ × \d+, /).waitFor({ timeout: 20000 });
  await sleep(1200);
  await shot(panel(page, 'calendar.show-still'), 'reference-calendar-show-still');

  // The log editor: the first automation block still ahead
  await page.getByRole('tab', { name: 'Episode' }).click();
  await page.locator('.ev.auto:not(.past)').first().focus();
  await page.keyboard.press('Enter');
  await panel(page, 'calendar.log-editor').waitFor({ timeout: 10000 });
  await sleep(1500);
  await shot(editor, 'reference-calendar-log-editor');

  // An aired episode: put tonight's on air now, then read it frozen
  await page.locator('.ev', { hasText: 'Saturday Night' }).first().focus();
  await page.keyboard.press('Enter');
  await sleep(800);
  await page.getByRole('button', { name: 'On air now' }).click();
  await sleep(3000);
  await shot(editor, 'reference-calendar-aired-episode');

  // A correction to the aired episode: one is logged, a second is being made with its reason
  await page.getByRole('button', { name: 'Correct the public text…' }).click();
  await editor.getByLabel('Title').fill('Ghost Train Night');
  await editor.getByLabel('Reason for the correction').fill('Title capitalised as on the poster');
  await editor.getByRole('button', { name: 'Publish correction' }).click();
  await editor.locator('[data-correction]').first().waitFor({ timeout: 15000 });
  await sleep(800);
  await page.getByRole('button', { name: 'Correct the public text…' }).click();
  await editor.getByLabel('Guests, separated by commas').fill('Firth Lights, Dr Una Robertson, Gregor Stewart');
  await editor.getByLabel('Reason for the correction').fill('Added the studio guest');
  await editor.locator('.fields').evaluate((el) => (el.scrollTop = el.scrollHeight)); // down to Corrections: the form and the log
  await sleep(500);
  await shot(editor, 'how-to-schedule-the-week-correction');

  // Last, because it changes the rules: the station webcasts under the US statutory licence, and
  // a replacement that breaks a separation is refused, with the choice to accept the break.
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.locator('#lic-us').check();
  await page.locator('#who').fill('Eilidh Fraser-MacLeod');
  await page.getByRole('button', { name: 'Save changes' }).click();
  await sleep(2000);
  await page.keyboard.press('Control+5');
  await page.waitForSelector('.schedule .ec', { timeout: 15000 });
  await page.locator('.ev.auto:not(.past)').first().focus();
  await page.keyboard.press('Enter');
  await panel(page, 'calendar.log-editor').waitFor({ timeout: 10000 });
  await sleep(1500);
  // The generated log differs from run to run: try tracks for the first item until one is refused
  // for a separation only (that refusal can be accepted; the US complement rule's cannot).
  const choose = page.locator('select[aria-label="Another track"]');
  const labels = await (async () => {
    await page.getByRole('button', { name: 'Another track…' }).first().click();
    const l = (await choose.locator('option').allTextContents()).slice(1);
    return l;
  })();
  let refused = false;
  for (const label of labels) {
    if (!(await choose.count())) await page.getByRole('button', { name: 'Another track…' }).first().click();
    await choose.selectOption({ label });
    await page.getByRole('button', { name: 'Use it' }).click();
    await sleep(900);
    if (await page.getByRole('button', { name: 'Accept the break' }).count()) {
      refused = true;
      break;
    }
  }
  if (!refused) throw new Error('no replacement was refused with the choice to accept the break');
  await sleep(500);
  await shot(editor, 'reference-calendar-log-editor-refused');
}

/** The packaged app's sign-in, on the same stack as `screens`. */
async function signin() {
  const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:26420/';
  // The packaged app's sign-in: the app treats the host name tauri.localhost as its own
  // window (as on Windows), so a browser told that name is this machine shows the real screen ---
  const packaged = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--host-resolver-rules=MAP tauri.localhost 127.0.0.1'] });
  const pctx = await packaged.newContext({ viewport: { width: 1440, height: 900 }, locale: 'en-GB' });
  await pctx.addInitScript(() => {
    try {
      localStorage.setItem('weavesignal.theme', 'dark');
    } catch {}
  });
  const sign = await pctx.newPage();
  await sign.goto(new URL(STUDIO).href.replace('127.0.0.1', 'tauri.localhost'));
  await panel(sign, 'setup.sign-in').waitFor();
  await sign.getByLabel('Station address').fill('https://kettle.example');
  await sign.getByLabel('Username').fill('eilidh');
  await sleep(400);
  await shot(sign.locator('.setup .card'), 'how-to-install-studio-linux-sign-in');
  await sign.getByRole('button', { name: 'Email me a code instead' }).click();
  await sign.getByLabel('Email address').waitFor();
  await sleep(500);
  await shot(sign.locator('.setup .card'), 'how-to-add-staff-code');
  await sign.getByRole('button', { name: 'Sign in with a password instead' }).click();
  await sign.getByRole('button', { name: 'Use the station key instead' }).click();
  await sleep(500);
  await shot(sign.locator('.setup .card'), 'how-to-add-staff-station-key');
  await packaged.close();
}

// ====================================================================================
// desk: desk B, the screener, the carts, against the real desk (tests/m2/run-studio.sh)
// ====================================================================================
async function desk() {
  const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:27420/';
  const api = apiFor(process.env.CONTROL ?? 'http://127.0.0.1:27310', process.env.ADMIN_TOKEN ?? 'm2-admin-test-only');
  // The pictures show a station that is set up, not the setup notice (ADR-0267).
  await api('POST', '/api/v1/setup/finish');
  const view = async () => (await api('GET', '/api/v1/studio')).body;
  const until = async (pred, ms = 8000) => {
    const t0 = Date.now();
    for (;;) {
      const v = await view();
      if (pred(v)) return v;
      if (Date.now() - t0 > ms) throw new Error('timed out waiting for the desk');
      await sleep(150);
    }
  };
  const src = (v, id) => v.sources.find((x) => x.identity === id);
  const caller = (v, id) => v.callers.find((c) => c.identity === id);
  const { page } = await open(STUDIO, {
    ready: async (p) => {
      await p.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
      await p.locator('[data-panel="desk.stage"]').waitFor();
    },
  });
  const k = page.keyboard;
  await until((v) => v.sources.length >= 4 && v.callers.length >= 4, 30000);
  await sleep(2500); // the meters settle

  // --- Desk B as it is when the show is running: lamp, stage, the rest of the room waiting ---
  await shot(page, 'reference-desk-window');
  await shot(panel(page, 'desk.tile.1'), 'how-to-get-ready-your-tile');
  await shot(panel(page, 'desk.clock'), 'how-to-set-the-show-clock');
  await shot(panel(page, 'desk.rundown'), 'how-to-set-the-show-rundown');
  await shot(panel(page, 'studio.frame'), 'reference-studio-frame');

  // --- Tutorial steps 3 and 4 and the how-to: Finlay on air, Kirsty muted ---
  await k.press('5');
  await until((v) => src(v, 'finlay')?.on_air);
  await k.press('c');
  await until((v) => src(v, 'kirsty')?.mute);
  await sleep(1200);
  await shot(panel(page, 'desk.stage'), 'how-to-present-at-desk-b-on-air-and-muted');
  await k.press('c');
  await k.press('5');
  await until((v) => !src(v, 'finlay')?.on_air && !src(v, 'kirsty')?.mute);
  // Talk to one person off air: Gregor's talk key (S) held, the TALK chip on his tile
  await k.down('s');
  await panel(page, 'desk.tile.2').locator('.state', { hasText: /talk/i }).waitFor({ timeout: 5000 });
  await sleep(800);
  await shot(panel(page, 'desk.tile.2'), 'how-to-present-at-desk-b-talk');
  await k.up('s');
  await sleep(500);
  await k.press('t'); // Finlay's cue key: in the presenter's headphones only
  await until((v) => src(v, 'finlay')?.cue);
  await sleep(1000);
  await shot(panel(page, 'desk.waiting'), 'how-to-present-at-desk-b-cued');
  await k.press('t');
  await until((v) => !src(v, 'finlay')?.cue);

  // --- The screener: the queue, then Niall screened with a line for the presenter ---
  await k.press('Control+2');
  await panel(page, 'screener.queue').waitFor();
  await sleep(1500);
  await shotUnion(page, ['screener.queue', 'screener.screening'], 'reference-screener-queue', { blur: true });
  const order = (await view()).callers.map((c) => c.identity);
  await k.press(String(order.indexOf('niall') + 1));
  await k.down('t');
  await until((v) => caller(v, 'niall')?.state === 'screening');
  // `t` is held (talk) while the fields are filled: key presses would let the held key eat the first
  // "t" of "Leith Walk", so the text is inserted (an edit as a person's, not a programmatic fill: the
  // field commits its value when it loses focus).
  await page.locator('#f-line').click();
  await k.insertText('Bus lanes on Leith Walk');
  await k.press('Escape');
  await page.locator('#f-note').click();
  await k.insertText('Has rung in three weeks running; keep it short.');
  await k.press('Escape');
  await sleep(1000);
  await shotUnion(page, ['screener.queue', 'screener.screening'], 'how-to-screen-callers-screening', { blur: true });
  await k.up('t');
  // The count-in: the screener holds ⌃T and talks to the presenter only
  await k.down('Control');
  await k.down('t');
  await page.locator('[data-panel="screener.status"] .lit-talk').waitFor({ timeout: 5000 });
  await sleep(600);
  await shot(panel(page, 'screener.status'), 'how-to-count-in-talkback');
  await k.up('t');
  await k.up('Control');
  await sleep(500);

  // --- Send him to the desk; the presenter's toast and his READY tile ---
  await k.press('p');
  await until((v) => caller(v, 'niall')?.state === 'ready' && caller(v, 'niall')?.slot === 4);
  await k.press('Control+1');
  await page.getByText('Niall is ready for you · strip 4').waitFor({ timeout: 5000 });
  await sleep(1200);
  await shot(page, 'how-to-take-a-caller-ready');
  // Not now: DESK BUSY on the screener
  await k.press('Escape');
  await until((v) => !!caller(v, 'niall')?.declined && !v.desk_taking_callers);
  await k.press('Control+2');
  await sleep(1200);
  await shot(panel(page, 'screener.status'), 'tutorial-first-show-desk-busy');
  await shot(panel(page, 'screener.screening'), 'how-to-screen-callers-desk-busy', { blur: true });
  // Send again, take him: cued, then on air
  await k.press('p');
  await until((v) => v.desk_taking_callers !== false && caller(v, 'niall')?.state === 'ready', 8000).catch(() => {});
  await k.press('Control+1');
  await sleep(800);
  await k.press('r');
  await until((v) => src(v, 'niall')?.cue);
  await k.press('4');
  await until((v) => src(v, 'niall')?.on_air);
  await sleep(1500);
  await shot(panel(page, 'desk.stage'), 'how-to-take-a-caller-on-air');
  await k.press('4');
  await k.press('r');

  // --- Playout down: PLAYOUT DOWN and OPEN tiles, then back ---
  const stack = (arg) => execFileSync(join(root, 'tests/m2/run-studio.sh'), [arg], { stdio: 'inherit', env: process.env });
  stack('down');
  await until((v) => !v.desk.playout.connected, 15000);
  await sleep(1500);
  // (the stage's tiles clip their controls in a short window while sources say OPEN: a taller one)
  await page.setViewportSize({ width: 1440, height: 1100 });
  await sleep(600);
  await shot(page, 'tutorial-first-show-playout-down');
  await shot(panel(page, 'desk.toast'), 'how-to-playout-down-toast');
  await page.setViewportSize({ width: 1440, height: 900 });
  stack('up');
  await until((v) => v.desk.playout.connected, 60000);
  await panel(page, 'desk.lamp').getByText(/on air/i).waitFor({ timeout: 15000 });

  // --- Carts: the demo pack loads in about a minute ---
  await k.press('Control+3');
  await page.locator('[data-panel="carts.wall"]').waitFor();
  await sleep(2000);
  await shot(page, 'reference-carts-window');

  // --- A bed playing under the open microphones (ducked), another cart cued in the headphones ---
  await k.press('Control+F4');
  await sleep(1000);
  const keyOf = async (pad) => (await pad.locator('kbd').first().textContent()).trim();
  await k.press((await keyOf(page.locator('.pad.bed').first())).toLowerCase());
  await page.locator('.pad.bed.sounding, .pad.bed .duck').first().waitFor({ timeout: 10000 });
  const cueKey = await keyOf(page.locator('.pad', { hasText: 'Thunder storm' }).first()); // 14 s: it stays cued for the picture
  await k.press(`Shift+${cueKey.toUpperCase()}`);
  await page.locator('.pad.cued').first().waitFor({ timeout: 10000 });
  await sleep(2500);
  await shotUnion(page, ['carts.wall', 'carts.sounding'], 'reference-carts-wall-cued-and-ducked');
  await k.press('Escape'); // the cue out of the headphones
  await k.press('Shift+Space');
  await k.press('Shift+Space'); // everything sounding, faded
  await k.press('Control+F1');
  await sleep(1200);

  // --- A caller on air whose link stops: the toast offers to mute them or keep them on ---
  await k.press('Control+1');
  await k.press('4');
  await until((v) => src(v, 'niall')?.on_air);
  await sleep(1000);
  const people = `${process.env.M2_PREFIX ?? 'weavesignal-m2'}-people`;
  const publisher = execFileSync('podman', [
    'exec', people, 'sh', '-c',
    'for p in /proc/[0-9]*; do c=$(tr "\\0" " " < $p/cmdline 2>/dev/null); case "$c" in "gst-launch-1.0 "*pub-niall*) echo ${p#/proc/};; esac; done',
  ]).toString().trim().split('\n')[0];
  if (!/^\d+$/.test(publisher)) throw new Error(`no publisher process for niall: ${publisher}`);
  const signal = (sig) => execFileSync('podman', ['exec', people, 'kill', `-${sig}`, publisher]);
  signal('STOP');
  try {
    await panel(page, 'desk.toast').getByText(/link is degraded|left the room/).waitFor({ timeout: 45000 });
    await sleep(800);
    await shot(panel(page, 'desk.toast'), 'how-to-take-a-caller-link-lost');
  } finally {
    signal('CONT');
  }
  await k.press('Escape');
  await k.press('4'); // off air again
  await until((v) => !src(v, 'niall')?.on_air);

  // --- AUTO, and the default loop: playout in auto mode with an empty fallback folder ---
  const runDir = join(root, 'tests/m2/run');
  const data = JSON.parse(readFileSync(join(runDir, 'playout.json'), 'utf8'));
  mkdirSync(join(runDir, 'fallback'), { recursive: true });
  copyFileSync(join(root, 'assets/default-loop/default-loop.flac'), join(runDir, 'default-loop.flac'));
  data.fallback = { dir: '/r/fallback', default_file: '/r/default-loop.flac' };
  writeFileSync(join(runDir, 'playout.json'), JSON.stringify(data, null, 2));
  stack('down');
  stack('up');
  await until((v) => v.desk.playout.connected, 60000);
  const settings = (await api('GET', '/api/v1/playout/settings')).body;
  const set = await api('PUT', '/api/v1/playout/settings', { ...settings, mode: 'auto' });
  if (set.status !== 200) throw new Error(`could not set auto mode: ${set.status}`);
  await until((v) => v.default_fallback, 60000);
  await sleep(2500);
  await page.setViewportSize({ width: 1440, height: 1100 }); // OPEN tiles clip their controls in a short window
  await sleep(800);
  await shot(panel(page, 'desk.stage'), 'explanation-on-air-auto');
  await shot(panel(page, 'desk.stage').locator('header').first(), 'reference-desk-stage-auto');
  await page.setViewportSize({ width: 1440, height: 900 });
  await sleep(500);
  await shot(panel(page, 'desk.playout'), 'reference-playout-default-loop');
  await api('PUT', '/api/v1/playout/settings', { ...settings, mode: 'live' });
  await sleep(1500);

  // --- Dump, last: the delay thrown away takes minutes to rebuild, and every picture after would show it ---
  await k.press('Control+1');
  await sleep(800);
  await k.press('Delete');
  await k.press('Delete');
  await until((v) => v.desk.delay?.dumps > 0 && v.desk.delay?.rebuilding, 10000);
  await sleep(1200);
  await shot(panel(page, 'desk.delay'), 'reference-desk-delay-dumped');
}

// ====================================================================================
// public: the player with call-in, the invite page and the echo test, at phone width
// (tests/m3/run-join.sh)
// ====================================================================================
async function publicPages() {
  const WEB = process.env.WEB ?? 'http://127.0.0.1:32430';
  const api = apiFor(process.env.CONTROL ?? 'http://127.0.0.1:32310', process.env.ADMIN_TOKEN ?? 'm3-admin-test-only');
  // The pictures show a station that is set up, not the setup notice (ADR-0267).
  await api('POST', '/api/v1/setup/finish');
  for (let i = 0; i < 120; i++) {
    const p = await fetch(`${WEB}/api/v1/player`).then((r) => r.json()).catch(() => null);
    if (p?.call_in?.open && p.presenter_live) break;
    await sleep(500);
  }
  // A phone: 390 × 844, the page's own dark scheme, a fake microphone for the call.
  const phoneBrowser = await chromium.launch({
    headless: true,
    executablePath: process.env.CHROMIUM,
    args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
  });
  const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, locale: 'en-GB', colorScheme: 'dark' };
  const full = (page) => ({ page: () => page, screenshot: (o) => page.screenshot({ ...o, fullPage: true }) });
  // The player down to where its chat begins (the chat has no service behind it on this stack).
  const aboveChat = (page) => ({
    page: () => page,
    screenshot: async (o) => {
      const h = await page.getByRole('heading', { name: 'Chat', exact: true }).boundingBox();
      const y = h ? Math.max(0, h.y + (await page.evaluate(() => window.scrollY)) - 12) : 800;
      return page.screenshot({ ...o, fullPage: true, clip: { x: 0, y: 0, width: 390, height: Math.round(y) } });
    },
  });

  const ctx = await phoneBrowser.newContext({ ...phone, permissions: ['microphone'] });
  const page = await ctx.newPage();
  await page.goto(WEB + '/');
  await page.getByRole('button', { name: 'Listen live' }).waitFor();
  await sleep(800);
  await shot(aboveChat(page), 'reference-public-pages-player');
  await page.getByRole('button', { name: 'Call the studio' }).tap();
  await page.locator('#f-name').waitFor();
  await page.fill('#f-name', 'Jessie Brown');
  await page.fill('#f-topic', 'No bus after 23:10 from the Infirmary');
  await page.locator('#f-topic').blur();
  await sleep(500);
  await shot(aboveChat(page), 'reference-public-pages-call-in');
  await page.locator('#f-consent').tap();
  await page.locator('form button[type="submit"]').tap();
  await page.locator('.banner[data-phase="waiting"]').waitFor({ timeout: 25000 });
  await sleep(1200);
  await shot(aboveChat(page), 'reference-public-pages-call-waiting');
  await page.getByText('Trouble hearing or being heard?').tap();
  await sleep(600);
  await shot(aboveChat(page), 'reference-public-pages-trouble');
  await page.getByRole('button', { name: /Press again to hang up/ }).tap().catch(() => {});

  // The invite page: a guest's link, before they join
  const inv = (await api('POST', '/api/v1/invites', { role: 'guest', name: 'Gregor Stewart' })).body;
  const g = await (await phoneBrowser.newContext({ ...phone, permissions: ['microphone'] })).newPage();
  await g.goto((/^https?:/.test(inv.link) ? inv.link : `${WEB}${inv.link}`));
  await g.getByRole('button', { name: 'Join the studio' }).waitFor();
  await sleep(800);
  await shot(full(g), 'reference-public-pages-join');

  // The echo test, as it opens
  const e = await (await phoneBrowser.newContext({ ...phone, permissions: ['microphone'] })).newPage();
  await e.goto(`${WEB}/echo-test/`);
  await e.getByRole('button', { name: 'Start the echo test' }).waitFor();
  await sleep(800);
  await shot(full(e), 'reference-public-pages-echo-test');
  await phoneBrowser.close();

  // A microphone the browser will not give: no fake device prompt is granted, nothing is allowed
  const deniedBrowser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--use-fake-device-for-media-stream'] });
  const inv2 = (await api('POST', '/api/v1/invites', { role: 'guest', name: 'Gregor Stewart' })).body;
  const d = await (await deniedBrowser.newContext({ ...phone, permissions: [] })).newPage();
  await d.goto((/^https?:/.test(inv2.link) ? inv2.link : `${WEB}${inv2.link}`));
  await d.getByRole('button', { name: 'Join the studio' }).tap();
  await d.getByText('Your microphone is blocked').waitFor({ timeout: 20000 });
  await sleep(800);
  await shot(full(d), 'reference-public-pages-no-mic');
  await deniedBrowser.close();
}

// ====================================================================================
// chat: the moderators' screens and the player's chat (tests/chat/run-stack.sh, with WAIT=1)
// ====================================================================================
async function chat() {
  const WEB = process.env.WEB ?? 'http://127.0.0.1:37430';
  const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:37420/';
  const CHAT = process.env.CHAT ?? 'http://127.0.0.1:37311';
  const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:37310';
  const TOKEN = process.env.ADMIN_TOKEN ?? 'chat-browser-admin-only';
  const OUTBOX = process.env.OUTBOX ?? join(root, 'tests/chat/run/outbox');
  const PG = process.env.PG ?? `${process.env.WEAVESIGNAL_TEST_PREFIX ?? 'weavesignal-chat'}-browser-pg`;
  const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
  const sql = (q) => execFileSync('podman', ['exec', PG, 'psql', '-U', 'postgres', '-d', 'weavesignal', '-tAc', q]).toString().trim();
  const codeFor = (email) => {
    let best = null;
    for (const f of readdirSync(OUTBOX)) {
      const text = readFileSync(join(OUTBOX, f), 'utf8');
      if (!text.includes(`To: ${email}`)) continue;
      const t = statSync(join(OUTBOX, f)).mtimeMs;
      if (!best || t >= best.t) best = { t, code: /chat is (\d{6})/.exec(text)[1] };
    }
    return best?.code;
  };
  const withCode = async (email) => {
    for (let i = 0; i < 50; i++) {
      const c = codeFor(email);
      if (c) return c;
      await sleep(200);
    }
    throw new Error(`no code for ${email}`);
  };

  // The station's own blocked word (a mild one), so a held message needs no swearing in a picture.
  const settings = await (await fetch(`${CHAT}/api/v1/chat/admin/settings`, { headers: admin })).json();
  const put = await fetch(`${CHAT}/api/v1/chat/admin/settings`, { method: 'PUT', headers: admin, body: JSON.stringify({ ...settings, blocked_words: ['numpty'] }) });
  if (!put.ok) throw new Error(`could not set the blocked word: ${put.status}`);

  const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, locale: 'en-GB', colorScheme: 'dark' };
  const signIn = async (page, email, name) => {
    await page.locator('[data-chat="signin-open"]').first().click();
    await page.locator('[data-chat="signin-email"]').fill(email);
    await page.locator('[data-chat="signin-submit"]').click();
    await page.locator('[data-chat="signin-code"]').waitFor();
    await page.locator('[data-chat="signin-code"]').fill(await withCode(email));
    await page.locator('[data-chat="signin-submit"]').click();
    await page.locator('[data-chat="signin-name"]').waitFor();
    await page.locator('[data-chat="signin-name"]').fill(name);
    await page.locator('[data-chat="signin-age"]').check();
    await page.locator('[data-chat="signin-terms"]').check();
    await page.locator('[data-chat="signin-submit"]').click();
    await page.locator('[data-chat="composer"]').waitFor();
  };
  const say = async (page, text) => {
    await page.locator('[data-chat="kind-chat"]').check();
    await page.locator('[data-chat="composer"]').fill(text);
    await page.locator('[data-chat="send"]').click();
  };
  const logText = (page) => page.locator('[data-chat="log"]').innerText();
  const waitText = async (page, text) => {
    for (let i = 0; i < 60; i++) {
      if ((await logText(page)).includes(text)) return;
      await sleep(250);
    }
    throw new Error(`"${text}" never appeared`);
  };
  // From the chat's heading to the foot of the player page.
  const fromChat = (page) => ({
    page: () => page,
    screenshot: async (o) => {
      const h = await page.getByRole('heading', { name: 'Chat', exact: true }).boundingBox();
      const top = h ? Math.max(0, h.y + (await page.evaluate(() => window.scrollY)) - 12) : 0;
      const bottom = await page.evaluate(() => document.documentElement.scrollHeight);
      return page.screenshot({ ...o, fullPage: true, clip: { x: 0, y: top, width: 390, height: Math.min(bottom - top, 900) } });
    },
  });

  const cb = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
  const jessie = await (await cb.newContext(phone)).newPage();
  const alastair = await (await cb.newContext(phone)).newPage();
  for (const p of [jessie, alastair]) {
    await p.goto(WEB + '/');
    await p.locator('[data-chat="log"]').waitFor();
  }
  await signIn(jessie, 'jessie.brown@mail.example', 'Jessie Brown');
  await signIn(alastair, 'alastair.buchanan@mail.example', 'Alastair Buchanan');
  sql("UPDATE chat_account SET created_at = now() - interval '3 days' WHERE kind = 'email'"); // old enough to post (ADR-0036)
  for (const p of [jessie, alastair]) {
    await p.reload();
    await p.locator('[data-chat="composer"]').waitFor();
  }
  const studio = await (await cb.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark', locale: 'en-GB' })).newPage();
  await studio.addInitScript(() => {
    try {
      localStorage.setItem('weavesignal.theme', 'dark');
    } catch {}
  });
  await studio.goto(STUDIO);
  await studio.keyboard.press('Control+2');
  await panel(studio, 'screener.needs').waitFor();
  await sleep(1500);

  // A few messages, then one held by the station's word filter
  await say(jessie, 'The N22 was a lifeline, two an hour till three');
  await waitText(alastair, 'two an hour till three');
  await sleep(1500);
  await say(alastair, 'Agreed, and the 4 to the Infirmary stopped at ten');
  await waitText(jessie, 'stopped at ten');
  await sleep(1500);
  await say(alastair, 'Rory the N22 is gone, get over it, nobody rides buses');
  await waitText(jessie, 'nobody rides buses');
  await sleep(1500);
  await say(alastair, 'the councillor is a numpty');
  await alastair.locator('[data-chat="held"]').waitFor({ timeout: 15000 });

  // A report on the third message
  const line = jessie.locator('[data-chat="msg"]', { hasText: 'nobody rides buses' }).first();
  await line.locator('[data-chat="actions"]').click();
  await line.locator('[data-chat="report"]').click();
  await jessie.locator('[data-chat="report-reason"] input[value="abuse"]').check();
  await jessie.locator('[data-chat="report-submit"]').click();
  await jessie.getByRole('button', { name: 'Done' }).click();
  await studio.locator('[data-chat="item"][data-kind="report"]').first().waitFor({ timeout: 15000 });
  await studio.locator('[data-chat="held"]').first().waitFor({ timeout: 15000 });
  await sleep(1200);
  await shot(panel(studio, 'screener.needs'), 'how-to-moderate-the-chat-needs', { blur: true });

  // Slow mode on (G twice), the held message still waiting: the player's chat as a listener sees it
  await studio.evaluate(() => document.activeElement?.blur());
  await studio.keyboard.press('KeyG');
  await studio.keyboard.press('KeyG');
  for (let i = 0; i < 40 && !/slow mode/i.test(await alastair.locator('main').innerText()); i++) await sleep(250);
  await sleep(800);
  await shot(fromChat(alastair), 'reference-chat-player', { blur: true });
  await studio.keyboard.press('KeyG');
  await studio.keyboard.press('KeyG');

  // The statement of reasons on the held message
  await studio.locator('[data-chat="held"]').first().locator('[data-chat="act-hide"]').click();
  await studio.locator('[data-chat="sor"]').waitFor();
  await studio.locator('[data-chat="sor-reason-abuse"]').click();
  await sleep(800);
  await shot(panel(studio, 'screener.statement'), 'how-to-moderate-the-chat-statement');
  await studio.keyboard.press('Escape');
  await cb.close();
}

// ====================================================================================
// steps: the beginner steps, from the sandbox build (no stack)
// ====================================================================================
async function steps() {
  const SITE = process.env.SANDBOX ?? 'http://127.0.0.1:27911';
  /** Open a scenario and start it; hide the sandbox's own bar, so the window is the studio app's. */
  async function scenario(id) {
    const { ctx, page } = await open(`${SITE}/#${id}`);
    await page.getByRole('button', { name: 'Start with sound' }).click();
    await page.waitForFunction(() => globalThis.__kettle?.begun === true, null, { timeout: 30_000 });
    await page.addStyleTag({
      content: '[data-panel="sandbox.guide"], [data-panel="sandbox.caption"] { display: none !important; } .shell.with-guide { grid-template-rows: auto minmax(0, 1fr) !important; } .shell.with-guide.with-all { grid-template-rows: auto auto minmax(0, 1fr) !important; }',
    });
    return { ctx, page };
  }
  const st = (page, fn) => page.evaluate(fn);

  // Solo: one tile, the guest's empty seat, no Not on air row; the frame's control and its dialog.
  const solo = await scenario('carts');
  await solo.page.keyboard.press('1');
  await solo.page.waitForFunction(() => globalThis.__kettle.person('eilidh')?.on_air === true);
  await sleep(600);
  await shot(solo.page, 'how-to-choose-a-step-solo');
  await shot(panel(solo.page, 'studio.frame'), 'reference-studio-frame-step');
  await panel(solo.page, 'studio.step').locator('.stepbtn').click();
  await panel(solo.page, 'studio.step.menu').waitFor();
  await solo.page.keyboard.press('ArrowDown');
  await shot(panel(solo.page, 'studio.step.menu'), 'how-to-choose-a-step-menu', { pause: 500 });
  await solo.page.keyboard.press('Escape');
  // A hidden part's key: the note in the toast slot.
  await solo.page.keyboard.press('Delete');
  await panel(solo.page, 'desk.toast').waitFor();
  await shot(panel(solo.page, 'desk.toast'), 'how-to-choose-a-step-key-off');
  await solo.page.keyboard.press('Escape');
  // Show everything: the banner, and the parts the step keeps off.
  await solo.page.getByRole('button', { name: 'Show everything' }).click();
  await panel(solo.page, 'studio.step.banner').waitFor();
  await shot(solo.page, 'how-to-choose-a-step-show-everything', { pause: 500 });
  await solo.ctx.close();

  // Callers: the queue on the desk, the delay with Dump, one caller taken to strip 4.
  const callers = await scenario('alone');
  await callers.page.keyboard.press('1');
  await callers.page.keyboard.press('F1');
  await callers.page.waitForFunction(() => globalThis.__kettle.callerState('niall') === 'waiting', null, { timeout: 60_000 });
  await st(callers.page, () => globalThis.__kettle.arrive('jessie', Date.now()));
  await sleep(1500);
  await shot(panel(callers.page, 'desk.callers'), 'reference-desk-callers');
  await callers.page.keyboard.press('p');
  await callers.page.waitForFunction(() => globalThis.__kettle.callerState('niall') === 'ready');
  await sleep(600);
  await shot(callers.page, 'how-to-choose-a-step-callers');
  await callers.ctx.close();

  // Guests: the invitation dialog without the DJ's role.
  const guests = await scenario('guest');
  await guests.page.keyboard.press('1');
  await guests.page.getByRole('button', { name: 'Invite someone…' }).click();
  await panel(guests.page, 'desk.invite').waitFor();
  await guests.page.locator('#inv-name').fill('Gregor Stewart');
  await shot(panel(guests.page, 'desk.invite'), 'how-to-choose-a-step-invitation', { blur: true });
  await guests.ctx.close();
}

// ====================================================================================
// prepare: Prepare mode (ADR-0276), from the sandbox build (no stack)
// ====================================================================================
async function prepare() {
  const SITE = process.env.SANDBOX ?? 'http://127.0.0.1:27911';
  // The queue scenario: The Morning Kettle on air at Full, and the window is Shona's.
  const { ctx, page } = await open(`${SITE}/#queue`);
  await page.getByRole('button', { name: 'Start with sound' }).click();
  await page.waitForFunction(() => globalThis.__kettle?.begun === true, null, { timeout: 30_000 });
  // Hide the sandbox's own bar, so the window is the studio app's; the rows above the surface
  // are then the frame and, while preparing, the band.
  await page.addStyleTag({
    content:
      '[data-panel="sandbox.guide"], [data-panel="sandbox.caption"] { display: none !important; } .shell { grid-template-rows: auto minmax(0, 1fr) !important; } .shell.with-prepare { grid-template-rows: auto auto minmax(0, 1fr) !important; }',
  });
  await page.keyboard.press('Control+1');
  await sleep(1500);
  // Live: the control in the frame, after who is signed in.
  await shot(panel(page, 'studio.frame'), 'reference-studio-frame-prepare');
  // The dialog, with Preparing picked: what the switch changes.
  await panel(page, 'studio.prepare').locator('button').click();
  await panel(page, 'studio.prepare.menu').waitFor();
  await page.keyboard.press('ArrowDown');
  await shot(panel(page, 'studio.prepare.menu'), 'how-to-prepare-your-show-switch', { pause: 500 });
  await page.keyboard.press('Enter');
  await panel(page, 'studio.prepare.banner').waitFor();
  // Preparing: the band, and the live desk with its controls off.
  await shot(page, 'how-to-prepare-your-show-desk', { pause: 600, blur: true });
  // Switch to Live… while The Morning Kettle is on air: the one ask.
  await panel(page, 'studio.prepare.banner').locator('button').click();
  await panel(page, 'studio.prepare.menu').waitFor();
  await shot(panel(page, 'studio.prepare.menu'), 'how-to-prepare-your-show-ask', { pause: 500 });
  await page.keyboard.press('Escape');
  await ctx.close();
}

const stages = { screens, signin, desk, public: publicPages, chat, steps, prepare };
const wanted = process.argv.slice(2);
if (!wanted.length || wanted.some((w) => !stages[w])) {
  console.error(`usage: shoot.mjs ${Object.keys(stages).join(' | ')}`);
  process.exit(2);
}
for (const w of wanted) await stages[w]();
await browser.close();
