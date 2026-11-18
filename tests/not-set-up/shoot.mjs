// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The page `/` shows on a station's own address until the station is set up (1.1:
// apps/web/not-set-up/), in a real browser: what it says, where its five links go, that it
// breaks no part of the pages' Content-Security-Policy and does not scroll sideways at a phone's
// width; and screenshots in Dark and Light (the public pages follow the system's setting; Night
// is the studio app's and the monitor link's) at 1440×900 and 390×844.
// Which page `/` serves before and after setup is the control plane's (`just test-db`,
// `web::tests` and `setup_db.rs`) and nginx's (`just wizard-check`, `deploy/cloudron/test-local.sh`).
// It needs no stack: one loopback port, and Playwright (not a dependency of the repository; see
// tests/m2/drive.mjs).
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/not-set-up/shoot.mjs
//   just test-not-set-up            # the same, with PLAYWRIGHT_CORE from the environment
//
// It builds the pages first (apps/web, `pnpm build`); SKIP_BUILD=1 uses the build that is there.
// Screenshots go to OUT (target/shots-not-set-up unless told), named `<width>-<theme>.png`.
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = join(ROOT, 'apps', 'web', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-not-set-up');
const PORT = Number(process.env.PORT ?? 30140);
mkdirSync(OUT, { recursive: true });

if (!process.env.SKIP_BUILD) {
  execFileSync('pnpm', ['install', '--frozen-lockfile'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
  execFileSync('pnpm', ['build'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
}
if (!existsSync(join(DIST, 'not-set-up', 'index.html'))) throw new Error(`no build at ${DIST}`);

let failed = 0;
function check(name, ok, detail = '') {
  if (!ok) failed++;
  console.log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

// The pages as the control plane serves them: the same Content-Security-Policy (web.rs's `csp`
// with no other origins), the page at `/`, a branding route that answers nothing (a station that
// has set no look; a station older than ADR-0237 answers 404).
const CSP =
  "default-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; media-src 'self' blob:; worker-src 'self'; manifest-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'";
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png' };
const setupHits = [];
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  const headers = { 'Content-Security-Policy': CSP, 'Cache-Control': 'no-cache' };
  if (path.startsWith('/api/')) {
    res.writeHead(404, headers).end('{}');
    return;
  }
  if (path === '/setup/') setupHits.push(path);
  // `/` is the not-set-up page, as the station serves it until the station is set up.
  const file = path === '/' ? '/not-set-up/index.html' : path.endsWith('/') ? `${path}index.html` : path;
  const full = normalize(join(DIST, file));
  if (!full.startsWith(DIST) || !existsSync(full)) {
    res.writeHead(404, headers).end('Not found\n');
    return;
  }
  res.writeHead(200, { ...headers, 'Content-Type': TYPES[extname(full)] ?? 'application/octet-stream' }).end(readFileSync(full));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
const base = `http://127.0.0.1:${PORT}`;
const SIZES = [
  { width: 1440, height: 900 },
  { width: 390, height: 844 },
];
for (const size of SIZES) {
  for (const theme of ['dark', 'light']) {
    const ctx = await browser.newContext({ viewport: size, colorScheme: theme, hasTouch: size.width < 500 });
    const page = await ctx.newPage();
    const tag = `${size.width}-${theme}`;
    const problems = [];
    page.on('pageerror', (e) => problems.push(e.message));
    page.on('console', (m) => m.type() === 'error' && !/branding/.test(m.text()) && !/404/.test(m.text()) && problems.push(m.text()));
    await page.addInitScript(() => {
      globalThis.__csp = [];
      document.addEventListener('securitypolicyviolation', (e) => globalThis.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
    });
    await page.goto(`${base}/`);
    await page.locator('h1').waitFor({ timeout: 10000 });
    if (tag === '1440-dark') {
      check('the page says the station is not set up', (await page.locator('h1').innerText()) === 'This station is not set up yet');
      check('the tab is titled Not set up yet', (await page.title()) === 'Not set up yet');
      const steps = await page.locator('ol.steps > li').allInnerTexts();
      check('three steps', steps.length === 3, JSON.stringify(steps));
      check('step 1 names the station key and where each route keeps it', /CONTROL_ADMIN_TOKEN/.test(steps[0]) && /\/app\/data\/secrets\.env/.test(steps[0]) && /\.env/.test(steps[0]), steps[0]);
      const where = await page.locator('ol.steps > li:first-child ul.where > li').allInnerTexts();
      check('step 1 says where to find the key on Cloudron, Compose or Quadlets, and any other install', where.length === 3 && /Cloudron/.test(where[0]) && /Compose or Quadlets/.test(where[1]) && /deploy\/compose\/\.env/.test(where[1]) && /WEAVESIGNAL_CONTROL_ADMIN_TOKEN/.test(where[2]), JSON.stringify(where));
      check('step 1 names this address', steps[0].includes(`${base}/setup/`), steps[0]);
      check('step 3 names this address', steps[2].includes(base), steps[2]);
      const links = await page.locator('a').evaluateAll((as) => as.map((a) => [a.textContent.trim(), a.getAttribute('href')]));
      const hrefs = Object.fromEntries(links.map(([t, h]) => [t, h]));
      check('Windows guide', hrefs.Windows === 'https://weavesignal.top/docs/how-to/install-studio-windows/', JSON.stringify(links));
      check('Linux guide', hrefs.Linux === 'https://weavesignal.top/docs/how-to/install-studio-linux/', JSON.stringify(links));
      check('the sign-in guide', hrefs['The guide to signing in'] === 'https://weavesignal.top/docs/how-to/install-on-cloudron/#sign-in', JSON.stringify(links));
      check('the setup page is linked twice, relative', links.filter(([, h]) => h === '/setup/').length === 2, JSON.stringify(links));
      check('the page asks nothing of the station but its look', !(await page.evaluate(() => performance.getEntriesByType('resource').map((e) => e.name))).some((n) => /\/api\/v1\/(setup|front)/.test(n)));
      await page.locator('a.btn.primary').click();
      await page.waitForURL(`${base}/setup/`);
      check('Open the setup page goes to /setup/ on this address', setupHits.length === 1, `${setupHits}`);
      await page.goto(`${base}/`);
      await page.locator('h1').waitFor();
    }
    const scrollsSideways = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    check(`${tag}: no sideways scroll`, !scrollsSideways);
    check(`${tag}: no Content-Security-Policy violation`, (await page.evaluate(() => globalThis.__csp)).length === 0, JSON.stringify(await page.evaluate(() => globalThis.__csp)));
    check(`${tag}: no script error`, problems.length === 0, problems.join('; '));
    check(`${tag}: the button is a 44 px target`, (await page.locator('a.btn.primary').boundingBox()).height >= 44);
    await page.screenshot({ path: join(OUT, `${tag}.png`), fullPage: true });
    await ctx.close();
  }
}
await browser.close();
server.close();
console.log(failed ? `${failed} FAILED` : `all passed; screenshots in ${OUT}`);
process.exit(failed ? 1 : 0);
