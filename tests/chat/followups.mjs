// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the 1.0.1 chat follow-ups on the M6 loopback stack (TEST=followups
// tests/chat/run-stack.sh) in Chromium: the studio's switch drops the upload choice from an
// open player page at once (ADR-0259); a listener reports a chatter's picture and the screener
// removes it from Needs you (ADR-0260); a picture refused with H then C is kept, not deleted,
// and Settings > Chat pictures deletes it only after asking why and for a confirmation
// (ADR-0257). The evidence's View and Export need a staff session of the named person, which
// this stack (the station key only) cannot give; the chat's own tests cover them.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/chat/followups.mjs
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


const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
try {
  const desktop = () => browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
  const mc = await desktop();
  const morag = await mc.newPage();
  const hamish = await (await desktop()).newPage();
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
  const settingsDoc = async () => (await fetch(`${CHAT}/api/v1/chat/admin/settings`, { headers: admin })).json();
  const putUploads = async (on) => {
    const d = await settingsDoc();
    d.avatar_uploads = on;
    const r = await fetch(`${CHAT}/api/v1/chat/admin/settings`, { method: 'PUT', headers: admin, body: JSON.stringify(d) });
    return r.status;
  };

  // ---- A: the switch hides the upload choice on an open page, at once ----
  await morag.locator('[data-chat="avatar-open"]').click();
  await morag.locator('[data-chat="avatar-sheet"] h2').waitFor();
  check('A: the sheet offers a picture of her own', (await morag.locator('[data-chat="avatar-pick"]').count()) === 1);
  check('A: the switch is used (200)', (await putUploads(false)) === 200);
  check('A: the open sheet drops the upload choice with no reload', await until(async () => (await morag.locator('[data-chat="avatar-pick"]').count()) === 0));
  await shot(morag, 'A-switch-off');
  await putUploads(true);
  check('A: switched back on, it returns', await until(async () => (await morag.locator('[data-chat="avatar-pick"]').count()) === 1));
  await morag.locator('[data-chat="avatar-file"]').setInputFiles({ name: 'crieff.jpg', mimeType: 'image/jpeg', buffer: picture(200) });
  await morag.locator('[data-chat="avatar-crop"]').waitFor();
  await morag.locator('[data-chat="avatar-use"]').click();
  await morag.locator('[data-chat="avatar-waiting"]').waitFor();
  await morag.locator('[data-chat="avatar-done"]').click();
  const moragPic = sql("SELECT id FROM chat_avatar WHERE state = 'held'");
  const ap = await fetch(`${CHAT}/api/v1/chat/mod`, { method: 'POST', headers: admin, body: JSON.stringify({ op: 'avatar_approve', avatar: moragPic }) });
  check('A: approved by the station key', ap.status === 200);
  await say(morag, 'Evening from Crieff');

  // ---- B: report this picture, through to Remove in Needs you ----
  const msg = hamish.locator('[data-chat="msg"]', { hasText: 'Evening from Crieff' }).first();
  await msg.waitFor();
  check('B: the sender shows her own picture', (await until(() => avatarOf(hamish, 'Evening from Crieff'))) === 'upload');
  await msg.locator('[data-chat="actions"]').click();
  check('B: Report this picture is offered', (await msg.locator('[data-chat="report-picture"]').count()) === 1);
  await msg.locator('[data-chat="report-picture"]').click();
  await hamish.locator('[data-chat="report-picture-choice"]').waitFor();
  check('B: the dialog opens on the picture', await hamish.locator('[data-chat="report-picture-choice"]').isChecked());
  await hamish.locator('input[data-reason="abuse"]').check();
  await shot(hamish, 'B-report-dialog');
  await hamish.locator('[data-chat="report-submit"]').click();
  check('B: report sent', await until(async () => (await hamish.locator('#rp-h').innerText()).includes('Report sent')));
  await hamish.locator('dialog[aria-labelledby="rp-h"] button:has-text("Done")').click();

  const sc = await desktop();
  const studio = await sc.newPage();
  await studio.goto(STUDIO);
  await studio.keyboard.press('Control+2');
  await studio.locator('[data-panel="screener.needs"]').waitFor();
  const item = await until(async () => ((await studio.locator('[data-chat="reported-picture"]').count()) > 0 ? studio.locator('[data-chat="item"]').first() : null));
  check('B: it is in Needs you as a picture report', !!item && (await item.innerText()).includes('Picture reported'));
  await shot(studio, 'B-needs-picture-reported');
  const rm = studio.locator('[data-chat="act-remove-picture"]');
  check('B: the button says the reported picture', (await rm.innerText()).includes('Remove the reported picture'));
  await rm.click();
  await studio.locator('[data-chat="sor"]').waitFor();
  check('B: removing a picture offers the suspected-CSAM reason too', (await studio.locator('[data-chat="sor-reason-suspected_csam"]').count()) === 1);
  await studio.keyboard.press('KeyA');
  await studio.keyboard.press('Enter');
  check('B: removed: her messages leave her own picture', (await until(async () => (await avatarOf(hamish, 'Evening from Crieff')) !== 'upload')) === true);
  check('B: the report is closed', await until(async () => (await studio.locator('[data-chat="item"]').count()) === 0));
  check('B: the picture file is deleted', !!(await until(() => !existsSync(join(AVATARS, `${moragPic}.png`)))));

  // ---- C: H then C, through to Delete now with the confirmation ----
  await hamish.locator('[data-chat="avatar-open"]').click();
  await hamish.locator('[data-chat="avatar-file"]').setInputFiles({ name: 'me.jpg', mimeType: 'image/jpeg', buffer: picture(20) });
  await hamish.locator('[data-chat="avatar-crop"]').waitFor();
  await hamish.locator('[data-chat="avatar-use"]').click();
  await hamish.locator('[data-chat="avatar-waiting"]').waitFor();
  await hamish.locator('[data-chat="avatar-done"]').click();
  const entry = await until(async () => ((await studio.locator('[data-chat="avatar"]').count()) > 0 ? studio.locator('[data-chat="avatar"]').first() : null));
  const hamPic = sql("SELECT id FROM chat_avatar WHERE state = 'held'");
  await entry.click();
  await studio.locator('[data-chat="avatar"] .square[data-loaded="true"]').first().waitFor();
  await studio.keyboard.press('KeyH');
  await studio.locator('[data-chat="sor"]').waitFor();
  await studio.keyboard.press('KeyC');
  check('C: C chooses the suspected-CSAM reason', await until(async () => (await studio.locator('[data-chat="sor-reason-suspected_csam"].on').count()) === 1));
  check('C: the dialog says it is not deleted', await studio.locator('[data-chat="sor-evidence"]').isVisible());
  check('C: the preview tells the uploader less (the rules on illegal content)', (await studio.locator('.sor .preview').innerText()).includes('rules on illegal content') && !(await studio.locator('.sor .preview').innerText()).includes('at least a year'));
  await shot(studio, 'C-sor-csam');
  await studio.keyboard.press('Enter');
  check('C: kept, not deleted', !!(await until(() => existsSync(join(AVATARS, `${hamPic}.png`)) && existsSync(join(AVATARS, `${hamPic}.orig`)))));
  check('C: withheld with evidence', sql(`SELECT state || ',' || (evidence IS NOT NULL) FROM chat_avatar WHERE id = '${hamPic}'`) === 'withheld,true');
  check('C: not served to the studio route', (await fetch(`${CHAT}/api/v1/chat/mod/avatars/${hamPic}`, { headers: admin })).status === 404);

  await studio.locator('button.settings-tab').click();
  const panel = studio.locator('[data-panel="settings.chat-pictures"]');
  await panel.waitFor();
  const kept = panel.locator('[data-chat="kept"]');
  await kept.first().waitFor();
  check('C: Settings lists it as kept as evidence', (await kept.first().innerText()).includes('Suspected child sexual abuse material'));
  check('C: View and Export are not offered to the station key', (await panel.locator('[data-chat="evidence-view"]').count()) === 0);
  await shot(studio, 'C-settings-kept');
  await panel.locator('[data-chat="kept-delete"]').first().click();
  const del = studio.locator('[data-panel="settings.chat-pictures.delete"]');
  await del.waitFor();
  check('C: the dialog says what the law asks', (await del.locator('[data-chat="delete-law"]').innerText()).includes('report it and then keep it'));
  await del.locator('[data-chat="delete-go"]').click();
  check('C: it asks why first', await until(async () => (await del.locator('.err').innerText()).includes('Say why')));
  await del.locator('[data-chat="delete-why"]').fill('Reported to the NCA; reference noted.');
  await del.locator('[data-chat="delete-go"]').click();
  check('C: it asks to confirm', await until(async () => (await del.locator('.err').innerText()).includes('confirm')));
  await shot(studio, 'C-delete-dialog');
  await del.locator('[data-chat="delete-confirm"]').check();
  await del.locator('[data-chat="delete-go"]').click();
  check('C: deleted: the list is empty', await until(async () => (await panel.locator('[data-chat="kept-empty"]').count()) === 1));
  check('C: both files are gone', !!(await until(() => !existsSync(join(AVATARS, `${hamPic}.png`)) && !existsSync(join(AVATARS, `${hamPic}.orig`)))));
  const logRes = await (await fetch(`${CHAT}/api/v1/chat/mod/log`, { headers: admin })).json();
  const del1 = logRes.log.find((l) => l.action === 'avatar_delete');
  check('C: the record says who deleted it and why', !!del1 && del1.note.includes('Reported to the NCA') && del1.note.includes('kept as evidence'), JSON.stringify(del1));
} finally {
  await browser.close();
}

const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
