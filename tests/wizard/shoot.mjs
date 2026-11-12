// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Screenshots of one page at the three sizes the wizards are checked at, in both themes
// (ADR-0237): 1,440 x 900, 390 x 844 (a phone) and 2,560 x 1,440.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core \
//     node tests/wizard/shoot.mjs http://127.0.0.1:18950/ target/wizard/player
//
// writes target/wizard/player-1440-dark.png and so on. Playwright is not a dependency of the
// repository (see tests/m2/drive.mjs).
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');

export const SIZES = [
  { name: '1440', width: 1440, height: 900 },
  { name: '390', width: 390, height: 844, mobile: true },
  { name: '2560', width: 2560, height: 1440 },
];
export const THEMES = ['dark', 'light'];

/** Run `each(page, {size, theme})` in a fresh context per size and theme. */
export async function everySize(browser, each) {
  for (const size of SIZES) {
    for (const theme of THEMES) {
      const context = await browser.newContext({
        viewport: { width: size.width, height: size.height },
        deviceScaleFactor: 1,
        isMobile: !!size.mobile,
        hasTouch: !!size.mobile,
        colorScheme: theme,
        reducedMotion: 'reduce',
      });
      const page = await context.newPage();
      await each(page, { size, theme });
      await context.close();
    }
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [url, out] = process.argv.slice(2);
  if (!url || !out) {
    console.error('usage: shoot.mjs <url> <output prefix>');
    process.exit(2);
  }
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
  await everySize(browser, async (page, { size, theme }) => {
    await page.goto(url, { waitUntil: 'networkidle' });
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${out}-${size.name}-${theme}.png`, fullPage: true });
  });
  await browser.close();
}
