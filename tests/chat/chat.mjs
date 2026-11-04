// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the chat on the M6 loopback stack (tests/chat/run-stack.sh) in Chromium: two listeners
// sign in on phone-sized player pages by emailed code (read from the outbox), post, are held by
// the word filter, are reported, hidden with a statement of reasons and timed out from the
// studio app's screener; a listener messages the studio and gets a reply and an invitation to
// call; someone without an account sends a notice; influx goes on and off with G G. Each step
// is checked on the pages and against the chat's routes, and the pages are captured.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/chat/chat.mjs
//
// Playwright is not a dependency of the repository. Screenshots go to OUT.
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const WEB = process.env.WEB ?? 'http://127.0.0.1:37430';
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:37420/';
const CHAT = process.env.CHAT ?? 'http://127.0.0.1:37311';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:37310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'chat-browser-admin-only';
const OUTBOX = process.env.OUTBOX;
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

/** The newest sign-in code mailed to an address. */
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

async function say(page, text, kind = 'chat') {
  await page.locator(`[data-chat="kind-${kind}"]`).check();
  await page.locator('[data-chat="composer"]').fill(text);
  await page.locator('[data-chat="send"]').click();
}

const logText = (page) => page.locator('[data-chat="log"]').innerText();

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const phone = (colorScheme) => browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, colorScheme, isMobile: true, hasTouch: true });
try {
  // ---- two listeners sign in ----
  const jc = await phone('dark');
  const ac = await phone('light');
  const jessie = await jc.newPage();
  const alastair = await ac.newPage();
  for (const p of [jessie, alastair]) {
    await p.goto(WEB + '/');
    await p.locator('[data-chat="log"]').waitFor();
  }
  check('the chat is on the player page, readable without an account', await jessie.locator('[data-chat="log"]').isVisible());
  await shot(jessie, '01-player-390-dark-reading');
  await signIn(jessie, 'jessie.brown@mail.example', 'Jessie Brown');
  await signIn(alastair, 'alastair.buchanan@mail.example', 'Alastair Buchanan');
  check('two listeners signed in by emailed code, with the age declaration and the terms', true);
  // Accounts must be 10 minutes old to post (ADR-0036): age them, then reload.
  sql("UPDATE chat_account SET created_at = now() - interval '3 days' WHERE kind = 'email'");
  for (const p of [jessie, alastair]) {
    await p.reload();
    await p.locator('[data-chat="composer"]').waitFor();
  }

  // ---- the screener ----
  const sc = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
  const studio = await sc.newPage();
  await studio.goto(STUDIO);
  await studio.keyboard.press('Control+2');
  await studio.locator('[data-panel="screener.needs"]').waitFor();
  await sleep(1500);

  // ---- posting ----
  await say(jessie, 'The N22 was a lifeline, two an hour till three');
  check('a listener posts and the other sees it', await until(async () => (await logText(alastair)).includes('two an hour till three')));
  check('the screener sees it in the room in real time', await until(async () => (await studio.locator('[data-panel="screener.room"]').innerText()).includes('two an hour till three')));
  await shot(alastair, '02-player-390-light-posted');

  // ---- the word filter holds a message for a moderator ----
  await say(alastair, 'the councillor is a fucking idiot');
  check('a filtered message waits for a moderator on the sender\'s page', await until(() => alastair.locator('[data-chat="held"]').isVisible()));
  const held = await until(async () => ((await studio.locator('[data-chat="held"]').count()) > 0 ? studio.locator('[data-chat="held"]').first() : null));
  check('the screener sees the held message in Needs you', !!held);
  check('nobody else sees it', !(await logText(jessie)).includes('fucking'));
  await shot(studio, '03-screener-1440-held');
  await held.locator('[data-chat="act-hide"]').click();
  await studio.locator('[data-chat="sor"]').waitFor();
  await studio.locator('[data-chat="sor-reason-abuse"]').click();
  await shot(studio, '04-screener-1440-statement');
  await studio.locator('[data-chat="sor-send"]').click();
  const st1 = await until(async () => {
    const t = await alastair.locator('[data-chat="statement"]').first().innerText();
    return t.includes('did not show your message') ? t : null;
  });
  check('the sender receives a statement of reasons naming the automated step', st1 && st1.includes('An automatic filter flagged it'), st1?.slice(0, 160));
  check('the turned-down message leaves his page', await until(async () => (await alastair.locator('[data-chat="held"]').count()) === 0));

  // ---- a report, hidden with a time-out ----
  await sleep(800);
  await say(alastair, 'Rory the N22 is gone, get over it, nobody rides buses');
  const line = await until(async () => {
    const l = jessie.locator('[data-chat="msg"]', { hasText: 'nobody rides buses' });
    return (await l.count()) ? l.first() : null;
  });
  await line.locator('[data-chat="actions"]').click();
  await line.locator('[data-chat="report"]').click();
  await jessie.locator('[data-chat="report-reason"] input[value="abuse"]').check();
  await shot(jessie, '05-player-390-dark-report');
  await jessie.locator('[data-chat="report-submit"]').click();
  await jessie.getByRole('button', { name: 'Done' }).click();
  const item = await until(async () => {
    const i = studio.locator('[data-chat="item"][data-kind="report"]');
    return (await i.count()) ? i.first() : null;
  });
  check('the report lands in the screener\'s Needs you', !!item);
  await shot(studio, '06-screener-1440-report');
  await item.locator('[data-chat="act-hide"]').click();
  await studio.locator('[data-chat="sor"]').waitFor();
  await studio.locator('[data-chat="sor-then-timeout"]').click();
  await studio.locator('[data-chat="sor-reason-abuse"]').click();
  await studio.locator('[data-chat="sor-send"]').click();
  check('the hidden message leaves every listener\'s page', await until(async () => !(await logText(jessie)).includes('nobody rides buses')));
  const st2 = await until(async () => {
    const t = (await alastair.locator('[data-chat="statement"]').allInnerTexts()).join('\n');
    return t.includes('stopped you posting for 10 minutes') ? t : null;
  });
  check('he is told why and for how long', !!st2);
  await sleep(800);
  check('his composer says he is timed out', await until(async () => /timed out/i.test(await alastair.locator('main').innerText())));
  await shot(alastair, '07-player-390-light-statement');

  // ---- message the studio ----
  await say(jessie, 'Rory, my dad drove the N22 in the nineties. He is listening tonight.', 'studio');
  const lane = await until(async () => {
    const m = studio.locator('[data-chat="studio-msg"]');
    return (await m.count()) ? m.first() : null;
  });
  check('a private message reaches the studio lane', !!lane);
  check('the room never sees it', !(await logText(alastair)).includes('my dad drove'));
  await lane.locator('[data-chat="studio-reply"]').click();
  await lane.locator('input').first().fill('Rory will read it after the news.');
  await studio.locator('[data-chat="studio-reply-send"]').click();
  check('the reply reaches the listener privately', await until(async () => (await jessie.locator('[data-chat="private"]').allInnerTexts()).join(' ').includes('after the news')));
  await lane.locator('[data-chat="studio-invite"]').click();
  check('an invitation to call in reaches her with a call button', await until(() => jessie.locator('[data-chat="invite"]').isVisible()));
  await shot(jessie, '08-player-390-dark-studio-lane');
  await shot(studio, '09-screener-1440-studio-lane');

  // ---- a notice from someone without an account ----
  const kc = await phone('dark');
  const k = await kc.newPage();
  await k.goto(WEB + '/');
  await k.locator('[data-chat="notice-open"]').first().click();
  await k.locator('[data-chat="notice-reason"] input[value="illegal"]').check();
  await k.locator('#nt-location').fill("Last Saturday's replay at 1:12:40");
  await k.locator('#nt-detail').fill('A message gives my street and house number.');
  await k.locator('#nt-name').fill('Kirsty Ferguson');
  await k.locator('#nt-email').fill('k.ferguson@mail.example');
  await k.locator('#nt-good_faith').check();
  await k.locator('[data-chat="notice-submit"]').click();
  check('the notice is sent', await until(async () => (await k.locator('main').innerText()).includes('Notice sent')));
  await shot(k, '10-player-390-dark-notice');
  check('the notice lands in Needs you with its contact', await until(async () => (await studio.locator('[data-chat="item"][data-kind="notice"]').count()) > 0));

  // ---- influx: one press of G twice, with the undo on the same key ----
  await studio.evaluate(() => document.activeElement?.blur());
  await studio.keyboard.press('KeyG');
  await studio.keyboard.press('KeyG');
  check('influx reaches the listeners\' rules', await until(async () => /slow mode/i.test(await jessie.locator('main').innerText())));
  const ctl = await (await fetch(`${CONTROL}/api/v1/studio`, { headers: admin })).json();
  check('the control plane holds influx (the chat follows it)', ctl.chat.influx === true);
  await shot(studio, '11-screener-1440-influx');
  await shot(jessie, '12-player-390-dark-influx');
  await studio.keyboard.press('KeyG');
  await studio.keyboard.press('KeyG');
  check('the undo turns it off', await until(async () => !/slow mode/i.test(await jessie.locator('main').innerText())));

  // ---- the record ----
  const lg = await (await fetch(`${CHAT}/api/v1/chat/mod/log`, { headers: admin })).json();
  const acts = lg.log.map((e) => e.action);
  check('every act is in the moderation log with who did it', acts.includes('reject') && acts.includes('hide') && acts.includes('timeout'), acts.join(','));
  check('the log names the screener, not just the studio', lg.log.some((e) => e.actor !== 'The studio'), [...new Set(lg.log.map((e) => e.actor))].join(','));
  const playheads = Number(sql('SELECT count(*) FROM chat_message WHERE playhead_at IS NOT NULL'));
  const all = Number(sql('SELECT count(*) FROM chat_message'));
  log(`messages stored ${all}, with a playhead ${playheads} (the stream was not playing, so none is expected)`);
} catch (e) {
  check('the run finished', false, e.stack ?? String(e));
} finally {
  await browser.close();
}
const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed`);
process.exit(failed.length ? 1 : 0);
