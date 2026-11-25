// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The player against the real control plane in the keyed stack (tests/keyed/run-stack.sh, run by
// crates/control/tests/keyed_stack.rs while a keyed show's window is open): a real key's link is
// opened in headless Chromium, the page redeems it, plays the station's keyed stream (a 200 on
// `/listen/k/<show>/…`, the audio's clock moving), and when the stack test revokes the key the
// page stops and says the key was ended, in words. Prints PASS and FAIL lines; exits 1 on a
// FAIL. Needs Playwright (PLAYWRIGHT_CORE, a playwright-core directory).
//
//   BASE=http://127.0.0.1:26410 KEY=KETM-7QMP-2D4X SHOW=<id> REVOKED=<file> node tests/keyed/player.mjs
//
// REVOKED is a file the stack test writes the moment it revokes the key; the time from then to
// the words is reported.
import { createRequire } from 'node:module';
import { existsSync, statSync } from 'node:fs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const { BASE, KEY, SHOW, REVOKED } = process.env;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
function check(name, ok, detail = '') {
  if (!ok) failed = true;
  console.log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}
async function until(what, ms, f) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await f().catch(() => null);
    if (v) return v;
    if (Date.now() > end) return null;
    await sleep(250);
  }
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--autoplay-policy=no-user-gesture-required'] });
try {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'en-GB' });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  const keyed = [];
  page.on('response', (r) => {
    if (r.url().includes('/listen/k/')) keyed.push({ url: r.url(), status: r.status() });
  });

  await page.goto(`${BASE}/listen/#key=${encodeURIComponent(KEY)}`);
  const welcome = await until('the welcome', 20_000, async () => (await page.locator('[data-testid="key-welcome"]').count()) > 0);
  check('the real key is redeemed from its link, and welcomed', !!welcome, await page.locator('body').innerText().catch(() => ''));
  const line = await until('the keyed show on now', 20_000, async () => (await page.locator('[data-keyed-show]').count()) > 0);
  check('the player finds the keyed show on now by the station’s id', !!line);

  await page.locator('.now button.primary').click();
  const heard = await until('the keyed stream playing', 25_000, () =>
    page.evaluate(() => {
      const a = document.querySelector('audio');
      return a && a.currentSrc.includes('/listen/k/') && !a.paused && a.currentTime > 2 ? { src: a.currentSrc, t: a.currentTime } : null;
    }),
  );
  check('it plays the station’s keyed stream, its clock moving', !!heard && heard.src.includes(`/listen/k/${SHOW}/ks1.`), JSON.stringify(heard));
  check('the keyed stream was answered 200', keyed.some((k) => k.status === 200 && k.url.includes(`/listen/k/${SHOW}/`)), JSON.stringify(keyed.map((k) => k.status)));
  console.log('playing');

  // The stack test revokes the key; the page must say so in words and stop.
  const said = await until('the revoked words', 45_000, async () => {
    const p = page.locator('[data-testid="key-problem"][data-code="revoked"]');
    return (await p.count()) > 0 ? (await p.innerText()).replace(/\s+/g, ' ').trim() : null;
  });
  const foundAt = Date.now();
  const revokedAt = REVOKED && existsSync(REVOKED) ? statSync(REVOKED).mtimeMs : null;
  check('a revoked key is said in the player’s words', !!said && said.startsWith('This key was ended by Kettle FM.'), said ?? (await page.locator('body').innerText()));
  check('the show stops here with words, not silence', (await page.locator('[data-testid="key-cut"]').count()) === 1 && (await page.evaluate(() => document.querySelector('audio')?.paused)) === true);
  check('and the key is let go of on this device', (await page.evaluate(() => localStorage.getItem('weavesignal.key'))) === null);
  if (revokedAt) console.log(`the words came within ${Math.round(foundAt - revokedAt)} ms of the revoke`);
} finally {
  await browser.close();
}
process.exit(failed ? 1 : 0);
