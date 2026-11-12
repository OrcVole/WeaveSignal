// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives pictures and documents in the chat (ADR-0262, ADR-0258) on the M6 loopback stack
// (TEST=attachments tests/chat/run-stack.sh) in Chromium:
//   A  the studio's switch (Settings > Chat pictures) turns attaching on, and a trusted chatter's
//      open page offers it at once; a member who is not trusted is not offered it;
//   B  a trusted chatter attaches a screenshot of a document with words: she sees it waiting,
//      nobody else does, and the file is not served;
//   C  the screener's Needs you shows it with its message, opens it full size
//      (screener.attachment), and approves it; the room sees the thumbnail, which opens the
//      picture, legible, at about 1,600 pixels;
//   D  an SVG is refused by its bytes, with words in the composer;
//   E  a PDF is attached and approved: the reader's card has its name, type and size, and the
//      download comes with Content-Disposition: attachment, nosniff and a sandbox;
//   F  a reader reports the document; the screener removes it from its report, and the message
//      leaves every page.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/chat/attachments.mjs
//
// Playwright is not a dependency of the repository; the screenshot is drawn with ImageMagick.
// Screenshots go to OUT, at 1,440 wide (dark) and 390 wide (light). Sample people are from the
// Scottish names list (ADR-0071).
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
const FILES = process.env.ATTACHMENTS;
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

/** A screenshot of a document: a running order in small dark type on white, 2,400 by 1,350,
 *  with a comment that must not survive. */
function screenshot() {
  const lines = [
    'Kettle FM · The Kettle Phone-In · running order, Friday',
    '20:00  News and weather, then the phones open',
    '20:12  Morag Fraser on the Crieff bus timetable',
    '20:25  Callers: what would three buses a night cost?',
    '20:40  Tide’s Out (request from Hamish Murray)',
    '20:52  Trail: Saturday’s ceilidh at the Drill Hall',
  ];
  const args = ['-size', '2400x1350', 'xc:white', '-fill', '#1d1f2a', '-pointsize', '34'];
  lines.forEach((l, i) => args.push('-annotate', `+90+${140 + i * 120}`, l));
  args.push('-set', 'comment', 'Taken at 56.3950 N, 3.4370 W', 'png:-');
  return execFileSync('magick', args, { maxBuffer: 32 * 1024 * 1024 });
}

const PDF = Buffer.from('%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n');

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

async function attach(page, file, words) {
  await page.locator('[data-chat="attach-input"]').setInputFiles(file);
  await page.locator('[data-chat="attach-chosen"]').waitFor();
  await page.locator('[data-chat="composer"]').fill(words);
  await page.locator('[data-chat="send"]').click();
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
try {
  const desktop = (scheme = 'dark') => browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme });
  const ailsa = await (await desktop()).newPage();
  const hamish = await (await desktop()).newPage();
  const phone = await (await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: 'light', deviceScaleFactor: 2 })).newPage();
  for (const p of [ailsa, hamish, phone]) {
    await p.goto(WEB + '/');
    await p.locator('[data-chat="log"]').waitFor();
  }
  await signIn(ailsa, 'ailsa.grant@mail.example', 'Ailsa Grant');
  await signIn(hamish, 'hamish.murray@mail.example', 'Hamish Murray');
  sql("UPDATE chat_account SET created_at = now() - interval '3 days' WHERE kind = 'email'");
  const ailsaId = sql("SELECT id FROM chat_account WHERE name = 'Ailsa Grant'");
  const st = await fetch(`${CHAT}/api/v1/chat/mod`, { method: 'POST', headers: admin, body: JSON.stringify({ op: 'standing', account: ailsaId, standing: 'trusted' }) });
  check('setup: Ailsa is trusted', st.status === 200);
  for (const p of [ailsa, hamish]) {
    await p.reload();
    await p.locator('[data-chat="composer"]').waitFor();
  }

  // ---- A: the switch, off as installed, turned on in the studio ----
  check('A: off as installed: no attach control', (await ailsa.locator('[data-chat="attach"]').count()) === 0);
  const studio = await (await desktop()).newPage();
  await studio.goto(STUDIO);
  await studio.locator('button.settings-tab').click();
  const panel = studio.locator('[data-panel="settings.chat-pictures"]');
  await panel.waitFor();
  const sw = panel.locator('[data-chat="attachments-switch"]');
  await sw.waitFor();
  check('A: the switch says off', !(await sw.isChecked()) && (await panel.locator('[data-chat="attachments-words"]').innerText()).includes('words only'));
  await sw.check();
  check('A: on, and it says so', await until(async () => (await panel.locator('[data-chat="attachments-words"]').innerText()).includes('Trusted chatters can post')));
  await panel.locator('[data-chat="attachments-switch"]').scrollIntoViewIfNeeded();
  await shot(studio, 'A-settings-switch-on');
  check('A: the trusted chatter’s open page offers it at once', await until(async () => (await ailsa.locator('[data-chat="attach"]').count()) === 1));
  check('A: a member who is not trusted is not offered it', (await hamish.locator('[data-chat="attach"]').count()) === 0);
  const rec = await (await fetch(`${CHAT}/api/v1/chat/mod/log`, { headers: admin })).json();
  check('A: the switch is in the moderation record', rec.log.some((l) => l.action === 'attachments_on'));

  // ---- B: a screenshot with words, waiting ----
  await attach(ailsa, { name: 'running-order.png', mimeType: 'image/png', buffer: screenshot() }, 'Tonight’s running order');
  const mine = ailsa.locator('[data-chat="msg"]', { hasText: 'Tonight’s running order' }).first();
  check('B: she sees it waiting for the station', !!(await until(async () => (await mine.locator('[data-chat="held-attachment"]').count()) === 1)));
  check('B: with her own preview', (await mine.locator('[data-chat="attachment-thumb"] img').count()) === 1);
  await shot(ailsa, 'B-player-waiting');
  const picId = sql("SELECT id FROM chat_avatar WHERE kind = 'picture' AND state = 'held'");
  check('B: kept with its original, rewritten, and its thumbnail', ['full', 'thumb', 'orig'].every((e) => existsSync(join(FILES, `${picId}.${e}`))));
  check('B: the rewritten picture has no comment', !readFileSync(join(FILES, `${picId}.full`)).includes('56.3950'));
  check('B: not served to anyone else', (await fetch(`${CHAT}/api/v1/chat/pictures/${picId}`)).status === 404);
  await sleep(500);
  check('B: the reader does not see it', (await hamish.locator('[data-chat="msg"]', { hasText: 'Tonight’s running order' }).count()) === 0);

  // ---- C: the screener's queue, full size, approve ----
  await studio.keyboard.press('Control+2');
  await studio.locator('[data-panel="screener.needs"]').waitFor();
  const entry = await until(async () => ((await studio.locator('[data-chat="avatar"]').count()) > 0 ? studio.locator('[data-chat="avatar"]').first() : null));
  check('C: in Needs you as a posted picture, with its words', !!entry && (await entry.innerText()).includes('posted a picture') && (await entry.innerText()).includes('Tonight’s running order'));
  check('C: its thumbnail loads with the studio’s bearer', !!(await until(async () => (await entry.locator('[data-chat="attachment-thumb"] img').count()) === 1)));
  await shot(studio, 'C-needs-picture');
  await entry.locator('[data-chat="attachment-thumb"]').click();
  const view = studio.locator('[data-panel="screener.attachment"]');
  await view.waitFor();
  const full = await until(async () => {
    const img = view.locator('img');
    return (await img.count()) === 1 && (await img.evaluate((i) => i.complete && i.naturalWidth)) ? img.evaluate((i) => [i.naturalWidth, i.naturalHeight]) : null;
  });
  check('C: full size is 1,600 by 900', JSON.stringify(full) === '[1600,900]', JSON.stringify(full));
  await shot(studio, 'C-screener-full-size');
  await studio.keyboard.press('Escape');
  check('C: Esc closes it', await until(async () => (await view.count()) === 0));
  await entry.click();
  await studio.locator('[data-chat="act-approve"]').first().click();
  const seen = hamish.locator('[data-chat="msg"]', { hasText: 'Tonight’s running order' }).first();
  check('C: approved: the reader sees it', !!(await until(async () => (await seen.locator('[data-chat="attachment-thumb"] img').count()) === 1)));
  const thumb = await until(async () => {
    const img = seen.locator('[data-chat="attachment-thumb"] img');
    return (await img.evaluate((i) => i.complete && i.naturalWidth)) ? img.evaluate((i) => [i.naturalWidth, i.naturalHeight]) : null;
  });
  check('C: the thumbnail is 480 by 270', JSON.stringify(thumb) === '[480,270]', JSON.stringify(thumb));
  check('C: approved, only the clean copy is kept: the original is gone', !existsSync(join(FILES, `${picId}.orig`)) && existsSync(join(FILES, `${picId}.full`)));
  check('C: her waiting line is gone', await until(async () => (await ailsa.locator('[data-chat="held-attachment"]').count()) === 0));
  await shot(hamish, 'C-player-thumbnail');
  await seen.locator('[data-chat="attachment-thumb"]').click();
  const pv = hamish.locator('[data-chat="picture-view"]');
  await pv.locator('img').waitFor();
  check('C: it opens full size', (await pv.locator('img').evaluate((i) => i.src)).endsWith(`/api/v1/chat/pictures/${picId}`));
  await sleep(300);
  await shot(hamish, 'C-player-full-picture');
  await hamish.keyboard.press('Escape');
  const pub = await fetch(`${CHAT}/api/v1/chat/pictures/${picId}`);
  check(
    'C: served inline as a PNG, nosniff, sandboxed',
    pub.status === 200 &&
      pub.headers.get('content-type') === 'image/png' &&
      pub.headers.get('content-disposition') === 'inline' &&
      pub.headers.get('x-content-type-options') === 'nosniff' &&
      (pub.headers.get('content-security-policy') ?? '').includes('sandbox'),
  );
  // The phone, light: the message with its picture.
  const onPhone = phone.locator('[data-chat="msg"]', { hasText: 'Tonight’s running order' }).first();
  await onPhone.waitFor();
  await until(async () => onPhone.locator('[data-chat="attachment-thumb"] img').evaluate((i) => i.complete && i.naturalWidth > 0));
  await onPhone.scrollIntoViewIfNeeded();
  await shot(phone, 'C-phone-light-thumbnail');
  check('C: the phone page has no sideways scroll', (await phone.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)) === true);

  // ---- D: an SVG is refused by its bytes ----
  await ailsa.locator('[data-chat="attach-input"]').setInputFiles({
    name: 'logo.png',
    mimeType: 'image/png',
    buffer: Buffer.from("<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>"),
  });
  await ailsa.locator('[data-chat="send"]').click();
  const refusal = await until(async () => {
    const r = ailsa.locator('[data-chat="refusal"]');
    return (await r.count()) ? r.innerText() : null;
  });
  check('D: refused, in words', !!refusal && refusal.includes('markup'), refusal ?? '');
  await shot(ailsa, 'D-refused-svg');
  await ailsa.locator('[data-chat="attach-remove"]').click();

  // ---- E: a PDF, approved, offered only as a download ----
  await sleep(800);
  await attach(ailsa, { name: 'Ceilidh poster (final).pdf', mimeType: 'application/pdf', buffer: PDF }, 'The poster for Saturday');
  const docId = await until(() => sql("SELECT id FROM chat_avatar WHERE kind = 'document' AND state = 'held'") || null);
  check('E: kept as it arrived', !!docId && readFileSync(join(FILES, `${docId}.orig`)).equals(PDF));
  const dentry = await until(async () => {
    const e = studio.locator('[data-chat="avatar"]', { hasText: 'posted a document' });
    return (await e.count()) ? e.first() : null;
  });
  check('E: in Needs you by its name, type and size', !!dentry && (await dentry.innerText()).includes('Ceilidh poster (final).pdf') && (await dentry.innerText()).includes('PDF'));
  await shot(studio, 'E-needs-document');
  await dentry.click();
  await studio.keyboard.press('Enter');
  const card = hamish.locator('[data-chat="msg"]', { hasText: 'The poster for Saturday' }).first().locator('[data-chat="attachment-doc"]');
  check('E: the reader sees its card', !!(await until(async () => (await card.count()) === 1)));
  check('E: name, type and size', (await card.innerText()).includes('Ceilidh poster (final).pdf') && (await card.innerText()).includes('PDF ·'));
  await card.scrollIntoViewIfNeeded();
  await shot(hamish, 'E-player-document');
  const href = await card.locator('[data-chat="attachment-download"]').getAttribute('href');
  check('E: Download points at the separate route', href === `/api/v1/chat/files/${docId}`);
  const dl = await fetch(`${CHAT}${href}`);
  const short = docId.replaceAll('-', '').slice(0, 8);
  check('E: a download under the station’s name', dl.headers.get('content-disposition') === `attachment; filename="chat-document-${short}.pdf"`, dl.headers.get('content-disposition'));
  check('E: of the type its bytes showed, nosniff, sandboxed', dl.headers.get('content-type') === 'application/pdf' && dl.headers.get('x-content-type-options') === 'nosniff' && dl.headers.get('content-security-policy') === "default-src 'none'; sandbox");
  check('E: unchanged', Buffer.from(await dl.arrayBuffer()).equals(PDF));

  // ---- F: Report this document, removed from its report ----
  const msgF = hamish.locator('[data-chat="msg"]', { hasText: 'The poster for Saturday' }).first();
  await msgF.locator('[data-chat="actions"]').click();
  await msgF.locator('[data-chat="report-attachment"]').click();
  await hamish.locator('[data-chat="report-attachment-choice"]').waitFor();
  check('F: the dialog opens on the document', await hamish.locator('[data-chat="report-attachment-choice"]').isChecked());
  await hamish.locator('input[data-reason="abuse"]').check();
  await shot(hamish, 'F-report-document');
  await hamish.locator('[data-chat="report-submit"]').click();
  check('F: report sent', await until(async () => (await hamish.locator('#rp-h').innerText()).includes('Report sent')));
  await hamish.locator('dialog[aria-labelledby="rp-h"] button:has-text("Done")').click();
  const rmBtn = await until(async () => {
    const b = studio.locator('[data-chat="act-remove-attachment"]');
    return (await b.count()) ? b.first() : null;
  });
  check('F: in Needs you as a document report', !!rmBtn && (await rmBtn.innerText()).includes('Remove the reported document'));
  await shot(studio, 'F-needs-document-reported');
  await rmBtn.click();
  await studio.locator('[data-chat="sor"]').waitFor();
  check('F: the dialog names the document', (await studio.locator('#sor-t').innerText()).includes('document'));
  await studio.keyboard.press('KeyA');
  await studio.keyboard.press('Enter');
  check('F: removed from every page', await until(async () => (await hamish.locator('[data-chat="msg"]', { hasText: 'The poster for Saturday' }).count()) === 0));
  check('F: the report is closed', await until(async () => (await studio.locator('[data-chat="item"]').count()) === 0));
  check('F: its file is deleted', !!(await until(() => !existsSync(join(FILES, `${docId}.orig`)))));
  check('F: no longer served', (await fetch(`${CHAT}${href}`)).status === 404);
} finally {
  await browser.close();
}

const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
