// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the chat's avatars (ADR-0235) on the M6 loopback stack (TEST=avatars
// tests/chat/run-stack.sh) in Chromium: a listener chooses a shipped avatar, uploads a picture
// of her own through the crop step (keys, the slider, a drag), and nobody else sees it until the
// screener approves it; a second listener's picture is refused with a statement of reasons; an
// approved picture is removed from the Room. Each step is checked on the pages and against the
// chat's routes, and the crop step and the screener's queue are captured at 1,440 by 900 and
// 2,560 by 1,440 in both themes.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/chat/avatars.mjs
//
// Playwright is not a dependency of the repository; the test pictures are drawn with
// ImageMagick. Screenshots go to OUT. Sample people are from the Scottish names list (ADR-0071).
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const WEB = process.env.WEB ?? 'http://127.0.0.1:37430';
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:37420/';
const CHAT = process.env.CHAT ?? 'http://127.0.0.1:37311';
const TOKEN = process.env.ADMIN_TOKEN ?? 'chat-browser-admin-only';
const OUTBOX = process.env.OUTBOX;
const AVATARS = process.env.AVATARS;
const PG = process.env.PG ?? 'weavesignal-chat-browser-pg';
const OUT = process.env.OUT ?? 'tests/chat/run/shots';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}
const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
const shot = (page, name) => page.screenshot({ path: join(OUT, `${name}.png`), fullPage: false });
const sql = (q) => execFileSync('podman', ['exec', PG, 'psql', '-U', 'postgres', '-d', 'weavesignal', '-tAc', q]).toString().trim();

/** A test picture: a landscape photo stand-in with a face-like disc off to one side, so a good
 *  crop has to move and zoom. Carries a comment, which must not survive. */
function picture(hue) {
  return execFileSync('magick', [
    '-size', '1200x800', `gradient:hsl(${hue},60%,70%)-hsl(${(hue + 40) % 360},50%,30%)`,
    '-fill', '#f2d2b6', '-draw', 'circle 820,330 820,470',
    '-fill', '#3a2a22', '-draw', 'circle 775,300 775,314', '-draw', 'circle 865,300 865,314',
    '-stroke', '#3a2a22', '-strokewidth', '8', '-fill', 'none', '-draw', 'arc 770,340 870,410 20,160',
    '-set', 'comment', 'Taken at 56.4907 N, 4.2026 W', 'jpg:-',
  ], { maxBuffer: 16 * 1024 * 1024 });
}

function codeFor(email) {
  let best = null;
  for (const f of readdirSync(OUTBOX)) {
    const p = join(OUTBOX, f);
    const text = readFileSync(p, 'utf8');
    if (!text.includes(`To: ${email}`)) continue;
    const t = statSync(p).mtimeMs;
    if (!best || t >= best.t) best = { t, code: /chat is (\d{6})/.exec(text)[1] };
  }
  return best?.code;
}

async function until(fn, ms = 8000) {
  const t0 = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // not yet
    }
    if (Date.now() - t0 > ms) return null;
    await sleep(150);
  }
}

async function signIn(page, email, name) {
  await page.locator('[data-chat="signin-open"]').first().click();
  await page.locator('[data-chat="signin-email"]').fill(email);
  await page.locator('[data-chat="signin-submit"]').click();
  await page.locator('[data-chat="signin-code"]').waitFor();
  const code = await until(() => codeFor(email));
  await page.locator('[data-chat="signin-code"]').fill(code);
  await page.locator('[data-chat="signin-submit"]').click();
  await page.locator('[data-chat="signin-name"]').waitFor();
  await page.locator('[data-chat="signin-name"]').fill(name);
  await page.locator('[data-chat="signin-age"]').check();
  await page.locator('[data-chat="signin-terms"]').check();
  await page.locator('[data-chat="signin-submit"]').click();
  await page.locator('[data-chat="composer"]').waitFor();
}

async function say(page, text) {
  await page.locator('[data-chat="composer"]').fill(text);
  await page.locator('[data-chat="send"]').click();
}

/** What a message's avatar is on a page: 'default', 'stock', 'upload'. */
const avatarOf = (page, text) => page.locator('[data-chat="msg"]', { hasText: text }).first().locator('[data-avatar]').getAttribute('data-avatar');

const SIZES = [
  { w: 1440, h: 900 },
  { w: 2560, h: 1440 },
];
const THEMES = ['dark', 'light'];

/** Capture a page at both sizes in both themes; `theme` switches it. */
async function fourShots(page, name, theme) {
  for (const s of SIZES) {
    for (const t of THEMES) {
      await page.setViewportSize({ width: s.w, height: s.h });
      await theme(t);
      await sleep(250);
      await shot(page, `${name}-${s.w}-${t}`);
    }
  }
  await page.setViewportSize({ width: SIZES[0].w, height: SIZES[0].h });
  await theme('dark');
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
try {
  const desktop = () => browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
  const mc = await desktop();
  const hc = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, colorScheme: 'light', isMobile: true, hasTouch: true });
  const morag = await mc.newPage();
  const hamish = await hc.newPage();
  const webTheme = (page) => (t) => page.emulateMedia({ colorScheme: t });
  for (const p of [morag, hamish]) {
    await p.goto(WEB + '/');
    await p.locator('[data-chat="log"]').waitFor();
  }
  await signIn(morag, 'morag.fraser@mail.example', 'Morag Fraser');
  await signIn(hamish, 'hamish.murray@mail.example', 'Hamish Murray');
  sql("UPDATE chat_account SET created_at = now() - interval '3 days' WHERE kind = 'email'");
  for (const p of [morag, hamish]) {
    await p.reload();
    await p.locator('[data-chat="composer"]').waitFor();
  }

  // ---- the plain default, then a shipped avatar ----
  await say(morag, 'Evening from Crieff');
  check('a message shows its sender\'s avatar; with none chosen, their initial', (await until(() => avatarOf(hamish, 'Evening from Crieff'))) === 'default');
  await morag.locator('[data-chat="avatar-open"]').click();
  await morag.locator('[data-chat="avatar-sheet"] h2').waitFor();
  check('the sheet says what others see, in words', (await morag.locator('[data-chat="avatar-others"]').innerText()).includes('your initial, M'));
  check('all 72 shipped avatars are offered, and none', (await morag.locator('[data-chat="avatar-set"] input[type=radio]').count()) === 73);
  await fourShots(morag, '01-player-avatar-sheet', webTheme(morag));
  await morag.locator('[data-chat="avatar-set"] label:has(input[data-avatar-id="fox"])').click();
  check('choosing one applies itself and everyone sees it at once', (await until(async () => (await avatarOf(hamish, 'Evening from Crieff')) === 'stock')) === true);
  check('the sheet now says the fox', await until(async () => (await morag.locator('[data-chat="avatar-others"]').innerText()).includes('the fox')));

  // ---- her own picture: the crop step ----
  await morag.locator('[data-chat="avatar-file"]').setInputFiles({ name: 'crieff.jpg', mimeType: 'image/jpeg', buffer: picture(200) });
  await morag.locator('[data-chat="avatar-crop"]').waitFor();
  const stage = morag.locator('[data-chat="avatar-stage"]');
  const zoom = morag.locator('[data-chat="avatar-zoom"]');
  check('the crop step opens centred at 100%', (await zoom.inputValue()) === '1');
  await fourShots(morag, '02-player-crop-start', webTheme(morag));
  // The keyboard alone: zoom in five steps, move right and up.
  await stage.focus();
  for (let i = 0; i < 8; i++) await morag.keyboard.press('+');
  for (let i = 0; i < 6; i++) await morag.keyboard.press('ArrowRight');
  for (let i = 0; i < 2; i++) await morag.keyboard.press('ArrowUp');
  check('plus zooms from the keyboard', Math.abs(Number(await zoom.inputValue()) - 1.8) < 0.01, await zoom.inputValue());
  check('a screen reader is told where the picture is', await until(async () => /Zoom 180%\. Showing the/.test(await morag.locator('[data-chat="avatar-crop"] [role=status]').innerText())));
  // The mouse: drag the picture a little left, the wheel zooms.
  const box = await stage.boundingBox();
  await morag.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await morag.mouse.down();
  await morag.mouse.move(box.x + box.width / 2 - 30, box.y + box.height / 2 + 10, { steps: 6 });
  await morag.mouse.up();
  await morag.mouse.wheel(0, -200);
  check('the wheel zooms too', Number(await zoom.inputValue()) > 1.8, await zoom.inputValue());
  await morag.locator('[data-chat="avatar-centre"]').click();
  check('Centre puts it back', (await zoom.inputValue()) === '1');
  for (let i = 0; i < 8; i++) await morag.keyboard.press('+');
  await stage.focus();
  for (let i = 0; i < 8; i++) await morag.keyboard.press('+');
  for (let i = 0; i < 7; i++) await morag.keyboard.press('ArrowRight');
  for (let i = 0; i < 2; i++) await morag.keyboard.press('ArrowUp');
  await fourShots(morag, '03-player-crop-fitted', webTheme(morag));
  await morag.locator('[data-chat="avatar-use"]').click();
  await morag.locator('[data-chat="avatar-waiting"]').waitFor();
  check('after sending, the sheet says it waits and what others see meanwhile', (await morag.locator('[data-chat="avatar-others"]').innerText()).includes('the fox'));
  await until(async () => (await morag.locator('[data-chat="avatar-waiting"] [data-avatar="own"]').count()) > 0);
  await fourShots(morag, '04-player-avatar-waiting', webTheme(morag));
  await morag.locator('[data-chat="avatar-done"]').click();
  check('the chat panel says her picture is waiting', await morag.locator('[data-chat="avatar-pending"]').isVisible());

  // ---- held: nobody else sees it ----
  const heldId = sql("SELECT id FROM chat_avatar WHERE state = 'held'");
  check('one picture is held', /^[0-9a-f-]{36}$/.test(heldId), heldId);
  const kept = readFileSync(join(AVATARS, `${heldId}.png`));
  check('what the station keeps is a 128-pixel PNG with nothing but pixels', kept.readUInt32BE(16) === 128 && kept.readUInt32BE(20) === 128 && !kept.includes('56.4907') && !kept.includes('Exif'), `${kept.length} bytes`);
  check('the public address of a held picture answers 404', (await fetch(`${CHAT}/api/v1/chat/avatars/${heldId}`)).status === 404);
  await say(morag, 'Is the phone-in open yet?');
  check('her messages still show the fox to everyone else', (await until(() => avatarOf(hamish, 'phone-in open'))) === 'stock');

  // ---- the screener's queue ----
  const sc = await desktop();
  const studio = await sc.newPage();
  await studio.goto(STUDIO);
  await studio.keyboard.press('Control+2');
  await studio.locator('[data-panel="screener.needs"]').waitFor();
  const studioTheme = async (t) => {
    await studio.evaluate((t) => localStorage.setItem('weavesignal.theme', t), t);
    await studio.reload();
    await studio.keyboard.press('Control+2');
    await studio.locator('[data-chat="avatar"] .square[data-loaded="true"]').first().waitFor();
  };
  const entry = await until(async () => ((await studio.locator('[data-chat="avatar"]').count()) > 0 ? studio.locator('[data-chat="avatar"]').first() : null));
  check('the picture waits in the screener\'s Needs you, with the chatter\'s name', !!entry && (await entry.innerText()).includes('Morag Fraser'));
  await studio.locator('[data-chat="avatar"] .square[data-loaded="true"]').first().waitFor();
  check('the screener sees the picture itself', true);
  await fourShots(studio, '05-screener-queue', studioTheme);

  // A second picture, to refuse.
  await hamish.locator('[data-chat="avatar-open"]').click();
  await hamish.locator('[data-chat="avatar-file"]').setInputFiles({ name: 'me.jpg', mimeType: 'image/jpeg', buffer: picture(20) });
  await hamish.locator('[data-chat="avatar-crop"]').waitFor();
  await shot(hamish, '06-player-crop-390-light');
  await hamish.locator('[data-chat="avatar-use"]').click();
  await hamish.locator('[data-chat="avatar-waiting"]').waitFor();
  await shot(hamish, '07-player-avatar-waiting-390-light');
  await hamish.locator('[data-chat="avatar-done"]').click();
  check('a second picture joins the queue, oldest first', await until(async () => (await studio.locator('[data-chat="avatar"]').count()) === 2));
  const names = await studio.locator('[data-chat="avatar"] .kind').allInnerTexts();
  check('Morag\'s is first', names[0]?.includes('Morag Fraser') && names[1]?.includes('Hamish Murray'), names.join(' | '));

  // Refuse Hamish's with H and a reason.
  const second = studio.locator('[data-chat="avatar"]').nth(1);
  await second.locator('[data-chat="act-refuse"]').click();
  await studio.locator('[data-chat="sor"]').waitFor();
  check('the statement is about the picture, with no time-out or ban offered', (await studio.locator('#sor-t').innerText()).includes("Refuse Hamish Murray's picture") && (await studio.locator('[data-chat^="sor-then-"]').count()) === 0);
  await studio.keyboard.press('KeyT');
  await studio.locator('.sor .square[data-loaded="true"]').waitFor();
  await fourShotsDialog();
  async function fourShotsDialog() {
    for (const s of SIZES) {
      await studio.setViewportSize({ width: s.w, height: s.h });
      await sleep(250);
      await shot(studio, `08-screener-refuse-${s.w}-dark`);
    }
    await studio.setViewportSize({ width: 1440, height: 900 });
  }
  await studio.keyboard.press('Enter');
  const st = await until(async () => {
    const t = await hamish.locator('[data-chat="statement"]').first().innerText();
    return t.includes('did not approve the picture you uploaded') ? t : null;
  });
  check('he receives a statement of reasons with the reason and a review', st && st.includes('off topic') && st.includes('review'), st?.slice(0, 200));
  check('his picture is gone from the queue and the disk', (await until(async () => (await studio.locator('[data-chat="avatar"]').count()) === 1)) && readdirSync(AVATARS).filter((f) => f.endsWith('.png')).length === 1);
  check('his page no longer says a picture is waiting', await until(async () => (await hamish.locator('[data-chat="avatar-pending"]').count()) === 0));

  // Approve Morag's with the keyboard: ⏎ on the selected entry.
  await studio.locator('[data-chat="avatar"]').first().locator('[data-chat="act-approve"]').focus();
  await studio.keyboard.press('Enter');
  check('approved: every page shows her own picture beside her messages', (await until(async () => (await avatarOf(hamish, 'Evening from Crieff')) === 'upload')) === true);
  check('the queue is empty', await until(async () => (await studio.locator('[data-chat="avatar"]').count()) === 0));
  check('the public address now serves it', (await fetch(`${CHAT}/api/v1/chat/avatars/${heldId}`)).status === 200);
  check('her page says so', await until(async () => (await morag.locator('[data-chat="avatar-pending"]').count()) === 0));
  await until(async () => hamish.locator('[data-chat="msg"] [data-avatar="upload"] img').first().evaluate((i) => i.complete && i.naturalWidth === 128));
  await shot(hamish, '09-player-approved-390-light');
  await shot(morag, '10-player-approved-1440-dark');
  await studio.locator('[data-chat="room-line"] [data-avatar="own"]').first().waitFor();
  await shot(studio, '11-screener-room-approved-1440-dark');

  // ---- removed later, from the Room ----
  const line = studio.locator('[data-chat="room-line"]', { hasText: 'Evening from Crieff' }).first();
  await line.click();
  await line.locator('[data-chat="room-remove-picture"]').click();
  await studio.locator('[data-chat="sor"]').waitFor();
  check('removing asks why, as any decision a chatter is told about', (await studio.locator('#sor-t').innerText()).includes("Remove Morag Fraser's picture"));
  await studio.keyboard.press('Enter');
  check('⏎ without a reason does nothing and says so', await until(() => studio.locator('.sor .problem').isVisible()));
  await studio.locator('[data-chat="sor-reason-other"]').click();
  await shot(studio, '12-screener-remove-1440-dark');
  await studio.locator('[data-chat="sor-send"]').click();
  check('removed: her messages go back to the fox everywhere', (await until(async () => (await avatarOf(hamish, 'Evening from Crieff')) === 'stock')) === true);
  check('the file is deleted and its address answers 404', (await until(() => !existsSync(join(AVATARS, `${heldId}.png`)))) && (await fetch(`${CHAT}/api/v1/chat/avatars/${heldId}`)).status === 404);
  const st2 = await until(async () => {
    const t = await morag.locator('[data-chat="statement"]').first().innerText();
    return t.includes('removed your picture') ? t : null;
  });
  check('she is told why', !!st2, st2?.slice(0, 160));
  const logRes = await (await fetch(`${CHAT}/api/v1/chat/mod/log`, { headers: admin })).json();
  const acts = logRes.log.map((l) => l.action);
  check('the moderation log has the approval, the refusal and the removal', ['avatar_approve', 'avatar_refuse', 'avatar_remove'].every((a) => acts.includes(a)), acts.join(','));
} finally {
  await browser.close();
}

const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
