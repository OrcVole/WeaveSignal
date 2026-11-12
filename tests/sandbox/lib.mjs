// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// What the sandbox's browser tests share (drive.mjs, scenarios.mjs): a headless Chromium with
// autoplay allowed, the checks and their count, waiting on the simulated station
// (`globalThis.__kettle`), the keys as a visitor presses them, screenshots into OUT. Playwright
// is not a dependency of the repository (see tests/m2/drive.mjs): PLAYWRIGHT_CORE points at an
// installed playwright-core with its Chromium headless shell.
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');

export const SITE = process.env.SITE ?? 'http://127.0.0.1:27911';
export const OUT = process.env.OUT ?? '';
if (OUT) mkdirSync(OUT, { recursive: true });

export const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
let failed = 0;
let passed = 0;
export const check = (ok, what) => {
  log(ok ? 'PASS' : 'FAIL', what);
  if (ok) passed++;
  else failed++;
};

/** The browser and a page, with every page error, console error and failed request kept. */
export async function open() {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--autoplay-policy=no-user-gesture-required'] });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'en-GB', timezoneId: 'Europe/London' });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  page.on('requestfailed', (r) => errors.push(`request failed: ${r.url()}`));
  let shot = 0;
  const d = {
    browser,
    page,
    errors,
    st: (fn) => page.evaluate(fn),
    async until(what, fn, ms = 30_000) {
      const end = Date.now() + ms;
      while (Date.now() < end) {
        if (await page.evaluate(fn)) return check(true, what);
        await page.waitForTimeout(200);
      }
      check(false, `${what} (waited ${ms / 1000} s)`);
    },
    snap: async (name) => OUT && page.screenshot({ path: `${OUT}/${String(++shot).padStart(2, '0')}-${name}.png` }),
    async press(key) {
      await page.keyboard.press(key);
      await page.waitForTimeout(150);
    },
    async hold(key, ms) {
      await page.keyboard.down(key);
      await page.waitForTimeout(ms);
      await page.keyboard.up(key);
    },
    /** Open a scenario by its hash and press Start; the sound and the director start then. */
    async start(id) {
      errors.length = 0;
      // A link that differs only in its hash loads no new document: leave the page first.
      await page.goto('about:blank');
      await page.goto(`${SITE}/#${id}`);
      await page.getByRole('button', { name: 'Start with sound' }).waitFor();
      await page.getByRole('button', { name: 'Start with sound' }).click();
      await d.until(`${id}: the sound starts`, () => !!globalThis.__kettle?.begun, 20_000);
      // STEPLOG=1: log each change of the guide's step counter (n/total), to find a step the
      // guide skips because it was already done when its turn came (2026-11-10).
      if (process.env.STEPLOG) {
        clearInterval(d.stepTimer);
        let last = '';
        d.stepTimer = setInterval(async () => {
          const c = await page.evaluate(() => document.querySelector('.count')?.textContent?.trim() ?? '').catch(() => '');
          if (c && c !== last) {
            log(`STEP ${id} ${c}`);
            last = c;
          }
        }, 100);
      }
    },
    /** Close: report the count and exit with it. */
    async done() {
      clearInterval(d.stepTimer);
      await browser.close();
      log(failed ? `${failed} failed, ${passed} passed` : `all ${passed} passed`);
      process.exit(failed ? 1 : 0);
    },
  };
  return d;
}
