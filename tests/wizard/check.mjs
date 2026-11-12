// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The wizards and the station's look in a real browser (ADR-0237), at 1,440 x 900, 390 x 844
// (a phone) and 2,560 x 1,440, in both themes. Two stages, each against something already
// running on loopback:
//
//   station  tests/wizard/run-station.sh with OVERRIDES=1 (a fresh one: the stage sets it up).
//            The lock (nothing shown or fetched before sign-in, a wrong key refused), the setup
//            page from pending to finished, the look saved and then seen on the player.
//            BASE=<dir> also compares the unbranded player and join page, pixel for pixel,
//            with pictures of the build before this work (tests/wizard/shoot.mjs made them).
//   website  the built website served from docs/dist (any static server), the install wizard
//            at /docs/install-wizard/: every route's output, and no request while answering.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core \
//     node tests/wizard/check.mjs station http://127.0.0.1:18950 [BASE=target/wizard/base]
//     node tests/wizard/check.mjs website http://127.0.0.1:18960
//
// Pictures go to OUT (default target/wizard). It needs Node 22.18 or later (it imports the
// wizard's TypeScript as it is). Playwright is not a dependency of the repository.
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';
import { contrast, derive, mix, GROUNDS } from '../../packages/wizard/accent.ts';
import { everySize, SIZES, THEMES } from './shoot.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const OUT = process.env.OUT ?? join(root, 'target/wizard');
const [stage, base] = process.argv.slice(2);
const KEY = process.env.STATION_KEY ?? 'wizard-admin-test-only';
mkdirSync(OUT, { recursive: true });

let failed = 0;
const pass = (what) => console.log(`pass  ${what}`);
function check(ok, what, detail = '') {
  if (ok) pass(what);
  else {
    failed++;
    console.error(`FAIL  ${what}${detail ? `: ${detail}` : ''}`);
  }
}

// ---- pictures to upload: made here, so the repository carries no binary for them ----
function crc32(buf) {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}
function png(width, height, pixel) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    const row = y * (width * 4 + 1);
    for (let x = 0; x < width; x++) raw.set(pixel(x, y), row + 1 + x * 4);
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(width, 0);
  head.writeUInt32BE(height, 4);
  head.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from('\x89PNG\r\n\x1a\n', 'latin1'), chunk('IHDR', head), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
/** A round badge on a transparent ground: a kettle-orange disc with a pale ring. */
const iconPng = () =>
  png(512, 512, (x, y) => {
    const r = Math.hypot(x - 255.5, y - 255.5);
    if (r > 250) return [0, 0, 0, 0];
    if (r > 215) return [250, 236, 214, 255];
    return r < 90 ? [250, 236, 214, 255] : [180, 83, 42, 255];
  });
/** A wide picture, taller than three to one: dusk bands with a low sun in the middle. */
const bannerPng = () =>
  png(1800, 800, (x, y) => {
    const t = y / 800;
    const sun = Math.hypot(x - 900, y - 470) < 150;
    if (sun) return [250, 214, 150, 255];
    const band = Math.floor(t * 8) % 2;
    return [Math.round(40 + 150 * t + band * 10), Math.round(34 + 60 * t), Math.round(70 + 40 * (1 - t)), 255];
  });

const rgbHex = (css) => {
  const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(css);
  return m ? `#${[1, 2, 3].map((i) => Number(m[i]).toString(16).padStart(2, '0')).join('')}` : css;
};

async function shot(page, name) {
  await page.screenshot({ path: join(OUT, `${name}.png`), fullPage: true });
}

/** What every page must hold at every size: no sideways scroll, named controls, big enough targets on a phone. */
async function soundPage(page, where, phone) {
  const facts = await page.evaluate(() => {
    const seen = (el) => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return r.width > 1 && r.height > 1 && s.visibility !== 'hidden' && s.display !== 'none';
    };
    const named = (el) => {
      if (el.getAttribute('aria-label') || el.getAttribute('aria-labelledby')) return true;
      if (el.labels && el.labels.length && [...el.labels].some((l) => l.textContent.trim())) return true;
      return ['BUTTON', 'A', 'SUMMARY'].includes(el.tagName) && el.textContent.trim() !== '';
    };
    const controls = [...document.querySelectorAll('input, select, textarea, button, a[href], summary')].filter((el) => el.type !== 'hidden' && !el.closest('dialog, site-search')); // Starlight's search is its own
    const unnamed = controls.filter((el) => !named(el)).map((el) => el.outerHTML.slice(0, 90));
    // A control's target is itself, or the label that stands for it (a file input's button, a radio's segment).
    const small = controls
      .filter((el) => !el.closest('[aria-hidden="true"]'))
      .map((el) => {
        const boxed = el.type === 'checkbox' || el.type === 'radio';
        const target = !boxed && seen(el) && !el.classList.contains('sr-only') && getComputedStyle(el).opacity !== '0' ? el : (el.labels?.[0] ?? el);
        const r = target.getBoundingClientRect();
        return { html: el.outerHTML.slice(0, 70), w: Math.round(r.width), h: Math.round(r.height), inline: getComputedStyle(target).display === 'inline', seen: seen(target) };
      })
      .filter((t) => t.seen && !t.inline && (t.h < 44 || t.w < 44));
    return { scroll: document.documentElement.scrollWidth - document.documentElement.clientWidth, unnamed, small, h1: document.querySelectorAll('h1').length };
  });
  check(facts.scroll <= 0, `${where}: no sideways scroll`, `${facts.scroll}px`);
  check(facts.unnamed.length === 0, `${where}: every control has a name`, facts.unnamed.join(' | '));
  check(facts.h1 === 1, `${where}: one h1`, String(facts.h1));
  if (phone) check(facts.small.length === 0, `${where}: targets are at least 44 px`, JSON.stringify(facts.small));
}

// ---------------------------------------------------------------------------------------------
async function station(browser) {
  const api = async (method, path, body, bearer = KEY) => {
    const res = await fetch(`${base}${path}`, { method, headers: { Authorization: `Bearer ${bearer}`, ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
    return { status: res.status, body: await res.json().catch(() => ({})) };
  };
  const fresh = await api('GET', '/api/v1/setup');
  check(fresh.status === 200 && fresh.body.status === 'pending', 'the station starts not set up', JSON.stringify(fresh.body).slice(0, 120));
  check((await api('GET', '/api/v1/setup', undefined, 'wrong')).status === 401, 'a wrong key is refused by the route');

  // 1. Unbranded: as before this work, to the pixel.
  const baseDir = (process.argv.find((a) => a.startsWith('BASE=')) ?? '').slice(5) || process.env.BASE;
  if (baseDir) {
    mkdirSync(join(OUT, 'unbranded'), { recursive: true });
    for (const [name, path] of [['player', '/'], ['join', '/join/']]) {
      await everySize(browser, async (page, { size, theme }) => {
        await page.goto(`${base}${path}`, { waitUntil: 'networkidle' });
        await page.waitForTimeout(300);
        const file = `${name}-${size.name}-${theme}.png`;
        await page.screenshot({ path: join(OUT, 'unbranded', file), fullPage: true });
        const before = join(baseDir, file);
        if (!existsSync(before)) return check(false, `unbranded ${file}: a picture from before to compare with`, before);
        check(readFileSync(before).equals(readFileSync(join(OUT, 'unbranded', file))), `unbranded ${name} at ${size.name} ${theme} is as before, to the pixel`);
      });
    }
  }

  // 2. The lock, as a visitor meets it.
  {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
    const page = await context.newPage();
    const asked = [];
    page.on('request', (r) => asked.push(new URL(r.url()).pathname));
    await page.goto(`${base}/setup/`, { waitUntil: 'networkidle' });
    const text = await page.locator('body').innerText();
    check(!asked.some((p) => p.startsWith('/api/')), 'the setup page asks the station nothing before sign-in', asked.filter((p) => p.startsWith('/api/')).join(' '));
    check(!/Kettle FM|7882|station\.example\b.*wss/.test(text), 'the setup page shows nothing of the station before sign-in');
    await page.getByRole('textbox', { name: 'Station key' }).fill('not-the-key');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByRole('alert').waitFor();
    check((await page.getByRole('alert').innerText()).includes('not the station key'), 'a wrong key says so, and opens nothing');
    check((await page.getByRole('heading', { level: 1 }).innerText()) === 'Set up your station', 'a wrong key leaves the sign-in in place');
    await shot(page, 'setup-signin-refused-1440-dark');
    await page.getByRole('radio', { name: 'As a member of staff' }).check({ force: true });
    await page.getByLabel('Username').fill('nobody');
    await page.getByLabel('Password').fill('not a password at all');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByRole('alert').waitFor();
    check(true, 'a wrong username and password are refused');
    await context.close();
  }

  // 3. Sign-in at every size, and the page not yet set up.
  await everySize(browser, async (page, { size, theme }) => {
    await page.goto(`${base}/setup/`, { waitUntil: 'networkidle' });
    await soundPage(page, `setup sign-in ${size.name} ${theme}`, !!size.mobile);
    await shot(page, `setup-signin-${size.name}-${theme}`);
    await page.getByRole('textbox', { name: 'Station key' }).fill(KEY);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByRole('heading', { name: 'This station is not set up yet' }).waitFor();
    await soundPage(page, `setup pending ${size.name} ${theme}`, !!size.mobile);
    await shot(page, `setup-pending-${size.name}-${theme}`);
  });

  // 4. The whole of setup, once, by keyboard and mouse as an admin would.
  {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
    const page = await context.newPage();
    await page.goto(`${base}/setup/`, { waitUntil: 'networkidle' });
    await page.getByRole('textbox', { name: 'Station key' }).fill(KEY);
    await page.keyboard.press('Enter');
    const h1 = page.getByRole('heading', { level: 1 });
    await page.getByRole('heading', { name: 'This station is not set up yet' }).waitFor();
    check(await h1.evaluate((el) => el === document.activeElement), 'after sign-in the focus is on the page’s heading');
    check((await page.locator('.missing li').count()) === 2, 'what is missing is listed');

    // an answers file from the website's wizard fills the page in, and saves nothing
    const answers = join(OUT, 'weavesignal-setup.json');
    writeFileSync(answers, JSON.stringify({ weavesignal_setup: 1, name: 'Radio Ceilidh', contact: 'calls@station.example', jurisdiction: 'uk_internet', accent: '#b4532a' }));
    await page.locator('#answers').setInputFiles(answers);
    await page.getByText('Filled in from the file').waitFor();
    check((await page.getByLabel('Station name').inputValue()) === 'Radio Ceilidh', 'an answers file fills in the name');
    check((await page.getByLabel('Where the station answers to').inputValue()) === 'uk_internet', 'an answers file chooses the jurisdiction');
    check((await page.getByLabel('Accent colour', { exact: true }).inputValue()) === '#b4532a', 'an answers file fills in the colour');
    check((await api('GET', '/api/v1/setup')).body.name.value === 'Kettle FM', 'an answers file saves nothing by itself');
    const notJson = join(OUT, 'not-answers.json');
    writeFileSync(notJson, '{"hello": "world"}');
    await page.locator('#answers').setInputFiles(notJson);
    await page.getByText('not an answers file').waitFor();
    check(true, 'a file that is not an answers file is refused with a sentence');

    // name and contact
    const identity = page.locator('form', { has: page.getByRole('heading', { name: 'Name and takedown contact' }) });
    await identity.getByRole('button', { name: 'Save' }).click();
    await identity.getByRole('status').waitFor();
    check((await page.getByRole('heading', { level: 1 }).innerText()) === 'Radio Ceilidh is set up', 'with a name and a contact, setup is no longer pending');
    const player = await (await fetch(`${base}/api/v1/player`)).json();
    check(player.station === 'Radio Ceilidh' && player.call_in.consent_text.includes('calls@station.example'), 'the player and the consent words carry the new name and contact at once');

    // jurisdiction
    const jur = page.locator('form', { has: page.getByRole('heading', { name: 'Jurisdiction' }) });
    check((await jur.locator('#preset-sum').innerText()).length > 40, 'the chosen jurisdiction says what it keeps');
    await jur.getByRole('button', { name: 'Save' }).click();
    await jur.getByRole('status').waitFor();
    check((await api('GET', '/api/v1/retention')).body.setting.preset === 'uk_internet', 'the jurisdiction is saved where it always was');

    // the look
    const lookCard = page.locator('section', { has: page.getByRole('heading', { name: 'The look' }) });
    await lookCard.getByRole('button', { name: 'Save colour' }).click();
    await lookCard.getByText('The public pages use it now').waitFor();
    await page.locator('#icon-file').setInputFiles({ name: 'badge.png', mimeType: 'image/png', buffer: iconPng() });
    await lookCard.getByText('It is the station’s icon now').waitFor();
    await page.locator('#banner-file').setInputFiles({ name: 'dusk.png', mimeType: 'image/png', buffer: bannerPng() });
    await lookCard.getByText('It is on the player now').waitFor();
    // refused before it is sent, and by the station
    await page.locator('#icon-file').setInputFiles({ name: 'logo.svg', mimeType: 'image/svg+xml', buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>') });
    await lookCard.getByText('SVG files are not taken').waitFor();
    check(true, 'an SVG is refused on the page');
    await page.locator('#icon-file').setInputFiles({ name: 'logo.png', mimeType: 'image/png', buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>') });
    await lookCard.getByText('not a picture this worker reads').waitFor();
    check(true, 'an SVG named .png is refused by the station');
    await page.locator('#banner-file').setInputFiles({ name: 'small.png', mimeType: 'image/png', buffer: png(300, 100, () => [9, 9, 9, 255]) });
    await lookCard.getByText('it is at least 600 wide').waitFor();
    check(true, 'a banner too small says how big it must be');
    const look = (await api('GET', '/api/v1/setup')).body.branding;
    check(look.accent === '#b4532a' && look.icon?.width === 256 && look.banner?.width === 1200 && look.banner?.height === 400, 'the look is stored: colour, a 256 px icon, a 1,200 by 400 banner', JSON.stringify(look));
    await lookCard.scrollIntoViewIfNeeded();
    await lookCard.screenshot({ path: join(OUT, 'setup-look-card-1440-dark.png') });

    // the stream password (this station runs as the Cloudron package does, with an overrides file)
    const stream = page.locator('form', { has: page.getByRole('heading', { name: 'Stream password' }) });
    await stream.getByRole('checkbox').check();
    await stream.getByLabel('New password').fill('bad pass');
    await stream.getByRole('button', { name: 'Save' }).click();
    await stream.getByRole('alert').waitFor();
    check((await stream.getByLabel('New password').getAttribute('aria-invalid')) === 'true', 'a password the start script would not take is refused, on its field');
    await stream.getByLabel('New password').fill('kettle-fm-2026');
    await stream.getByRole('button', { name: 'Save' }).click();
    await stream.getByText('Restart the app').waitFor();
    check(!(await page.content()).includes('kettle-fm-2026'), 'the saved password is not shown again');

    // finish
    await page.getByRole('button', { name: 'Finish setup' }).click();
    await page.getByText('Setup was finished on').waitFor();
    check((await api('GET', '/api/v1/setup')).body.done_by === 'page', 'setup is finished');
    check((await page.getByRole('button', { name: 'Finish setup' }).count()) === 0, 'once finished, finishing is no longer offered');

    // lock
    await page.getByRole('button', { name: 'Sign out' }).click();
    await page.getByRole('heading', { name: 'Set up your station' }).waitFor();
    check(!(await page.locator('body').innerText()).includes('Radio Ceilidh'), 'Sign out forgets the sign-in and shows nothing of the station');
    await context.close();
  }

  // 5. The finished page at every size.
  await everySize(browser, async (page, { size, theme }) => {
    await page.goto(`${base}/setup/`, { waitUntil: 'networkidle' });
    await page.getByRole('textbox', { name: 'Station key' }).fill(KEY);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByRole('heading', { name: 'Radio Ceilidh is set up' }).waitFor();
    await page.waitForLoadState('networkidle');
    await soundPage(page, `setup done ${size.name} ${theme}`, !!size.mobile);
    await shot(page, `setup-done-${size.name}-${theme}`);
  });

  // 6. The branded player and join page at every size, and what the colours measure.
  for (const [name, path] of [['player', '/'], ['join', '/join/'], ['echo', '/echo-test/']]) {
    await everySize(browser, async (page, { size, theme }) => {
      await page.goto(`${base}${path}`, { waitUntil: 'networkidle' });
      await page.waitForTimeout(300);
      const where = `branded ${name} ${size.name} ${theme}`;
      const got = await page.evaluate(() => {
        const css = getComputedStyle(document.documentElement);
        const btn = document.querySelector('.btn.primary');
        const icon = document.querySelector('.brand-icon');
        const banner = document.querySelector('.banner');
        return {
          accent: css.getPropertyValue('--accent').trim(),
          ink: css.getPropertyValue('--accent-ink').trim(),
          plane: getComputedStyle(document.body).backgroundColor,
          btnBg: btn ? getComputedStyle(btn).backgroundColor : null,
          btnInk: btn ? getComputedStyle(btn).color : null,
          icon: icon ? { w: icon.naturalWidth, shown: icon.getBoundingClientRect().width } : null,
          banner: banner ? { w: banner.naturalWidth, h: banner.naturalHeight, shown: banner.getBoundingClientRect().width, ratio: banner.getBoundingClientRect().width / banner.getBoundingClientRect().height } : null,
          favicon: document.querySelector('link[rel="icon"]')?.getAttribute('href') ?? '',
          mark: !!document.querySelector('svg.mark'),
        };
      });
      const want = derive('#b4532a', theme);
      const g = GROUNDS[theme];
      check(got.accent === want.accent && got.ink === want.ink, `${where}: the accent is the one derived for this theme`, `${got.accent} with ${got.ink}`);
      check(rgbHex(got.plane) === g.plane, `${where}: the page's ground is the theme's own`);
      const onGrounds = Math.min(contrast(got.accent, g.plane), contrast(got.accent, g.surface), contrast(got.accent, g.surface2));
      check(onGrounds >= 4.5, `${where}: the accent reads on every ground`, onGrounds.toFixed(2));
      if (got.btnBg) {
        const ratio = contrast(rgbHex(got.btnInk), rgbHex(got.btnBg));
        const hover = contrast(rgbHex(got.btnInk), mix(rgbHex(got.btnBg), g.ink, 0.88));
        check(ratio >= 4.5 && hover >= 4.5, `${where}: the main button's words read on it, hovered too`, `${ratio.toFixed(2)}, ${hover.toFixed(2)}`);
      }
      check(got.icon?.w === 256 && got.icon.shown === 28 && !got.mark, `${where}: the station's icon stands where the mark stood`, JSON.stringify(got.icon));
      check(/^\/api\/v1\/branding\/icon\?v=[0-9a-f]{16}$/.test(got.favicon), `${where}: the favicon is the station's icon`, got.favicon);
      if (name === 'player') check(got.banner?.w === 1200 && got.banner.h === 400 && Math.abs(got.banner.ratio - 3) < 0.02, `${where}: the banner is shown, three to one`, JSON.stringify(got.banner));
      else check(got.banner === null, `${where}: only the player carries the banner`);
      await soundPage(page, where, !!size.mobile);
      await shot(page, `${name}-branded-${size.name}-${theme}`);
    });
  }

  // 7. The look comes off again, and the player is the product's own once more.
  for (const path of ['/api/v1/branding/icon', '/api/v1/branding/banner']) await api('DELETE', path);
  await api('PUT', '/api/v1/branding/accent', { accent: '' });
  {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: 'dark' });
    const page = await context.newPage();
    await page.goto(`${base}/`, { waitUntil: 'networkidle' });
    const got = await page.evaluate(() => ({ accent: document.documentElement.style.getPropertyValue('--accent'), mark: !!document.querySelector('svg.mark'), banner: !!document.querySelector('.banner'), kept: localStorage.getItem('weavesignal.look.v1') }));
    check(got.accent === '' && got.mark && !got.banner && got.kept === null, 'with the look removed, the player is the product’s own again', JSON.stringify(got));
    await context.close();
  }
}

// ---------------------------------------------------------------------------------------------
async function website(browser) {
  const url = `${base}/docs/install-wizard/`;
  const routes = [
    { key: 'compose', label: 'Compose', file: '.env' },
    { key: 'quadlets', label: 'Quadlets', file: 'station.env' },
    { key: 'cloudron', label: 'Cloudron', file: 'weavesignal.env' },
  ];
  // 1. Answering sends nothing anywhere, and keeps nothing.
  {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });
    const page = await context.newPage();
    await page.goto(url, { waitUntil: 'networkidle' });
    const sent = [];
    page.on('request', (r) => sent.push(`${r.method()} ${r.url()}`));
    await page.getByLabel('Station name').fill('Radio Ceilidh');
    await page.getByLabel('Takedown contact').fill('calls@station.example');
    await page.getByLabel('The station’s name on the internet').fill('radio.station.example');
    await page.getByLabel('Jurisdiction').selectOption('uk_internet');
    await page.getByLabel('Accent colour', { exact: true }).fill('#b4532a');
    for (const r of routes) {
      await page.getByRole('radio', { name: new RegExp(`^${r.label}`) }).check({ force: true });
      await page.waitForTimeout(100);
    }
    await page.waitForTimeout(500);
    check(sent.length === 0, 'the website wizard makes no request while answering', sent.join(' | '));
    // (Starlight keeps its own theme choice in localStorage; nothing of the answers may be there.)
    const kept = await page.evaluate(() => ({ stored: JSON.stringify([Object.entries(localStorage), Object.entries(sessionStorage)]), cookie: document.cookie, url: location.search + location.hash }));
    check(!/Ceilidh|station\.example|b4532a|uk_internet/i.test(kept.stored) && kept.cookie === '' && kept.url === '', 'the answers are kept nowhere: no storage, no cookie, nothing in the address', JSON.stringify(kept));
    await context.close();
  }

  // 2. Each route's output, read back from the page.
  {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', acceptDownloads: true });
    const page = await context.newPage();
    await page.goto(url, { waitUntil: 'networkidle' });
    const out = page.locator('#wizard-output');
    check((await out.innerText()).includes('Answer the questions'), 'before the answers are complete the output says what is missing, not a file');
    await page.getByLabel('Station name').fill('Kettle "FM"');
    await page.getByLabel('Station name').blur();
    check((await page.getByLabel('Station name').getAttribute('aria-invalid')) === 'true', 'a name a settings file cannot hold is refused on its field');
    await page.getByLabel('Station name').fill('Radio Ceilidh');
    await page.getByLabel('Takedown contact').fill('calls@station.example');
    await page.getByLabel('The station’s name on the internet').fill('radio.station.example');
    await page.getByLabel('Jurisdiction').selectOption('uk_internet');
    await page.getByLabel('Accent colour', { exact: true }).fill('#b4532a');
    let text = await out.innerText();
    check(text.includes('STATION_NAME=Radio Ceilidh') && text.includes('CONSENT_CONTACT=calls@station.example'), 'Compose: the settings file carries the answers');
    check(text.includes('deploy/compose/weavesignal.sh --public radio.station.example'), 'Compose: the command names the station');
    check(/radio\.station\.example\s+A\b/.test(text) && /no AAAA|No AAAA|must have no AAAA/i.test(text), 'Compose: an A record, and no AAAA on the TURN name');
    check(!/^CONTROL_ADMIN_TOKEN=/m.test(text) && !/POSTGRES_PASSWORD|LIVEKIT_API_SECRET/.test(text), 'no secret is made or shown');
    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: /Download \.env/ }).click();
    const file = await (await download).path();
    check(readFileSync(file, 'utf8').includes('STATION_NAME=Radio Ceilidh'), 'the settings file downloads as shown');
    const answersDl = page.waitForEvent('download');
    await page.getByRole('button', { name: /Download weavesignal-setup\.json/ }).click();
    const answers = JSON.parse(readFileSync(await (await answersDl).path(), 'utf8'));
    check(answers.weavesignal_setup === 1 && answers.jurisdiction === 'uk_internet' && answers.accent === '#b4532a', 'the answers file carries the jurisdiction and the colour for the setup page');

    await page.getByRole('radio', { name: /^Quadlets/ }).check({ force: true });
    text = await out.innerText();
    check(text.includes('--no-start --env-file ~/.config/weavesignal/station.env --public radio.station.example') && text.includes('deploy/quadlets/generate.sh'), 'Quadlets: the settings are completed, then the units generated');

    await page.getByRole('radio', { name: /^Cloudron/ }).check({ force: true });
    await page.getByLabel('The studio (the app’s own domain)').fill('studio.station.example');
    await page.getByLabel('Public pages', { exact: true }).fill('station.example');
    await page.getByLabel('Listener stream', { exact: true }).fill('listen.station.example');
    await page.getByLabel('Live room', { exact: true }).fill('room.station.example');
    await page.getByLabel('The server has an IPv6 address').check();
    text = await out.innerText();
    check(text.includes('STATION_NAME="Radio Ceilidh"') && text.includes('ROOM_NAME=studio'), 'Cloudron: the settings file is whole and quoted');
    check(!/^(PUBLIC|ROOM|LISTENERS)_DOMAIN=/m.test(text.split('cloudron install')[0]), 'Cloudron: the domains are not in the settings file');
    check(text.includes('--location studio.station.example') && text.includes('ROOM_DOMAIN=room.station.example'), 'Cloudron: the install command carries the names');
    check(text.includes('Delete the studio name’s AAAA record'), 'Cloudron with IPv6: the studio name loses its AAAA record after the install');
    await context.close();
  }

  // 3. At every size, in both themes: empty, and answered for Cloudron (the longest output).
  await everySize(browser, async (page, { size, theme }) => {
    await page.goto(url, { waitUntil: 'networkidle' });
    await page.waitForTimeout(200);
    const where = `website wizard ${size.name} ${theme}`;
    await soundPage(page, `${where}, empty`, false);
    await shot(page, `website-empty-${size.name}-${theme}`);
    await page.getByLabel('Station name').fill('Radio Ceilidh');
    await page.getByLabel('Takedown contact').fill('calls@station.example');
    await page.getByLabel('Jurisdiction').selectOption('uk_internet');
    await page.getByLabel('Accent colour', { exact: true }).fill('#b4532a');
    await page.getByLabel('The station’s name on the internet').fill('radio.station.example');
    await page.waitForTimeout(100);
    await shot(page, `website-compose-${size.name}-${theme}`);
    await page.getByRole('radio', { name: /^Cloudron/ }).check({ force: true });
    await page.getByLabel('The studio (the app’s own domain)').fill('studio.station.example');
    await page.getByLabel('Public pages', { exact: true }).fill('station.example');
    await page.getByLabel('Listener stream', { exact: true }).fill('listen.station.example');
    await page.getByLabel('Live room', { exact: true }).fill('room.station.example');
    await page.getByLabel('The server has an IPv6 address').check();
    await page.waitForTimeout(100);
    await soundPage(page, `${where}, Cloudron`, false);
    // The wizard's own controls on a phone (Starlight's chrome is its own).
    if (size.mobile) {
      const small = await page.evaluate(() =>
        [...document.querySelectorAll('#install-wizard :is(input, select, button, summary)')]
          .map((el) => {
            const t = el.type === 'radio' || el.type === 'checkbox' ? (el.closest('label') ?? el) : el;
            const r = t.getBoundingClientRect();
            return { html: el.outerHTML.slice(0, 60), h: Math.round(r.height), w: Math.round(r.width) };
          })
          .filter((t) => t.h > 0 && (t.h < 44 || t.w < 44)),
      );
      check(small.length === 0, `${where}: the wizard's targets are at least 44 px`, JSON.stringify(small));
    }
    await shot(page, `website-cloudron-${size.name}-${theme}`);
  });
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
try {
  if (stage === 'station' && base) await station(browser);
  else if (stage === 'website' && base) await website(browser);
  else {
    console.error('usage: check.mjs station|website <base url> [BASE=<dir of pictures from before>]');
    process.exitCode = 2;
  }
} finally {
  await browser.close();
}
if (failed) {
  console.error(`\n${failed} check${failed === 1 ? '' : 's'} failed`);
  process.exitCode = 1;
} else if (process.exitCode !== 2) {
  console.log(`\nall checks passed; pictures in ${OUT} (sizes: ${SIZES.map((s) => s.name).join(', ')}; themes: ${THEMES.join(', ')})`);
}
