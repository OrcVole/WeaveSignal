// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the studio app's Keys tab (keyed shows, 1.3.2) and the key beside a show (the show
// editor, the Schedule, the desk's Playout panel) in a real browser against the stand-in station
// (station.mjs), checks each state and each press against what the page shows and what it sent,
// and captures them in Dark, Light and Night at 1440×900 and 2560×1440. From 1.3.3 also: what
// the tick means on a show that has opened to everyone (`opened`), the desk's third state while
// playout does not answer (in `locks`), a key's page at a station that sends no email
// (`nomail`), a key made for the picture and a station of 1.3.2 (`media`, `older`). No stack and no port
// but its own loopback one; Playwright is not a dependency of the repository (set
// PLAYWRIGHT_CORE to a playwright-core directory).
//
//   cd apps/studio && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/keys/shoot.mjs
//   SIZES=1440x900 THEMES=dark ONLY=list,send … node tests/keys/shoot.mjs      # fewer of them
//   DOCS=1 … node tests/keys/shoot.mjs       # also writes the how-to's pictures
//                                            # (docs/src/assets/screens/how-to-keys-*.png)
//
// Screenshots go to OUT (target/shots-keys unless told), named `<width>-<theme>-<state>.png`.
// What this does not prove: anything about a real station, whose answers the stand-in copies
// (its views are checked here against docs/openapi/weavesignal-v1.json), and any engine but
// Chromium.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { KeysStation, written } from './station.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = join(ROOT, 'apps', 'studio', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-keys');
const PORT = Number(process.env.PORT ?? 28150);
const SIZES = (process.env.SIZES ?? '1440x900,2560x1440').split(',').map((s) => s.split('x').map(Number));
const THEMES = (process.env.THEMES ?? 'dark,light,night').split(',');
const ONLY = process.env.ONLY ? new Set(process.env.ONLY.split(',')) : null;
const DOCS = process.env.DOCS ? join(ROOT, 'docs', 'src', 'assets', 'screens') : null;
mkdirSync(OUT, { recursive: true });
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/studio first`);

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}
const wants = (name) => !ONLY || ONLY.has(name);

// ---- the stand-in's views against the station's own description ----

const SCHEMAS = JSON.parse(readFileSync(join(ROOT, 'docs', 'openapi', 'weavesignal-v1.json'), 'utf8')).components.schemas;
/** What is wrong with `v` as `schema` (the subset of JSON Schema the description uses). */
function wrong(schema, v, path = '$') {
  if (schema.$ref) return wrong(SCHEMAS[schema.$ref.split('/').at(-1)], v, path);
  for (const k of ['oneOf', 'anyOf']) {
    if (schema[k]) return schema[k].some((s) => wrong(s, v, path).length === 0) ? [] : [`${path}: matches none of ${k}`];
  }
  const types = [].concat(schema.type ?? []);
  const is = (t) => (t === 'null' ? v === null : t === 'integer' ? Number.isInteger(v) : t === 'number' ? typeof v === 'number' : t === 'array' ? Array.isArray(v) : t === 'object' ? typeof v === 'object' && v !== null && !Array.isArray(v) : typeof v === t);
  if (types.length && !types.some(is)) return [`${path}: ${JSON.stringify(v)} is not ${types.join(' or ')}`];
  if (schema.enum && v !== null && !schema.enum.includes(v)) return [`${path}: ${JSON.stringify(v)} is not one of ${schema.enum.join(', ')}`];
  const out = [];
  if (Array.isArray(v) && schema.items) v.slice(0, 5).forEach((x, i) => out.push(...wrong(schema.items, x, `${path}[${i}]`)));
  if (v && typeof v === 'object' && !Array.isArray(v) && schema.properties) {
    for (const k of schema.required ?? []) if (!(k in v)) out.push(`${path}.${k}: missing`);
    for (const k of Object.keys(v)) {
      if (!schema.properties[k]) out.push(`${path}.${k}: not in the description`);
      else out.push(...wrong(schema.properties[k], v[k], `${path}.${k}`));
    }
  }
  return out;
}
{
  const st = new KeysStation({ role: 'admin' });
  const as = (name, [status, body], want = 200) => check(`the stand-in answers ${name} as the station's description has it`, status === want && wrong({ $ref: `#/x/${name}` }, body).length === 0, `${status} ${wrong({ $ref: `#/x/${name}` }, body).slice(0, 4).join('; ')}`);
  as('ListenerKeyList', st.handle('GET', '/api/v1/listener-keys?limit=5'));
  as('ListenerKey', st.handle('GET', `/api/v1/listener-keys/${st.isla.id}`));
  as('ListenerKeyDevices', st.handle('GET', `/api/v1/listener-keys/${st.isla.id}/devices`));
  as('ListenerKeyLog', st.handle('GET', `/api/v1/listener-keys/${st.katie.id}/log`));
  as('ListenerKeyText', st.handle('POST', `/api/v1/listener-keys/${st.isla.id}/text`, {}));
  as('ListenerKeyFamilyList', st.handle('GET', '/api/v1/listener-keys/families'));
  as('ListenerKeyLeakReport', st.handle('GET', '/api/v1/listener-keys/leaks'));
  as('ListenerKeySigning', st.handle('GET', '/api/v1/listener-keys/signing'));
  as('KeyedShowList', st.handle('GET', '/api/v1/public/keyed-shows'));
  as('ShowAccess', st.handle('GET', '/api/v1/shows/friday-supporters/access'));
  as('ShowAccess', st.handle('GET', '/api/v1/shows/the-long-lunch/access'));
  as('ListenerKeyMade', st.handle('POST', '/api/v1/listener-keys', { label: 'Tara Douglas', shows: ['friday-supporters'] }), 201);
  as('ListenerKeyFamilyMade', st.handle('POST', '/api/v1/listener-keys/families', { label: 'Winter patrons', template: {}, recipients: [{ label: 'Oona Murray' }, { label: 'Gavin Scott', email: 'gavin@mail.example' }] }), 201);
  as('ListenerKeyReplaced', st.handle('POST', `/api/v1/listener-keys/${st.callum.id}/replace`, {}));
  as('ListenToken', st.handle('POST', '/api/v1/keyed/redeem', { key: 'ketm-7qmp-2d4x', show: 'friday-supporters' }));
}

// ---- the studio's build and the stand-in on one loopback port ----

let station = null;
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (path === '/api/v1/studio/events') {
    if (!station) return res.writeHead(503).end();
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
    const send = () => res.write(`data: ${JSON.stringify(station.studioView())}\n\n`);
    send();
    const every = setInterval(send, 1000);
    req.on('close', () => clearInterval(every));
    return;
  }
  const file = join(DIST, path === '/' ? 'index.html' : path);
  if (!file.startsWith(DIST) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });

/** A window on a station, on the Keys tab unless told. `refuse` answers every key route with a
 *  status and a sentence (a sign-in that ended, an account the station will not have). */
async function open(st, [width, height], theme, { tab = 'keys', refuse = null } = {}) {
  station = st;
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: 'Europe/London', permissions: ['clipboard-read', 'clipboard-write'] });
  await ctx.addInitScript(([t]) => {
    try {
      localStorage.setItem('weavesignal.theme', t);
      localStorage.setItem('weavesignal.zoom', '1');
    } catch {}
  }, [theme]);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(new Date(st.now * 1000));
  page.on('pageerror', (e) => check('no page errors', false, e.message));
  await page.route(/\/(api\/|health)/, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (url.pathname === '/api/v1/studio/events') return route.continue();
    if (url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"not in the stand-in"}' });
    if (refuse && url.pathname.startsWith('/api/v1/listener-keys')) return route.fulfill({ status: refuse[0], contentType: 'application/json', body: JSON.stringify({ error: refuse[1] }) });
    const [status, body] = st.handle(req.method(), url.pathname + url.search, req.postData() ? JSON.parse(req.postData()) : undefined);
    if (typeof body === 'string') return route.fulfill({ status, contentType: 'text/csv; charset=utf-8', body });
    if (status === 204 || status === 202) return route.fulfill({ status });
    await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  });
  await page.goto(`http://127.0.0.1:${PORT}/`);
  await page.locator('.frame .link[data-link="live"]').waitFor({ timeout: 15000 });
  if (tab === 'keys') {
    await page.keyboard.press('Control+6');
    await page.locator('.keys').waitFor({ timeout: 10000 });
    await page.locator('.keys .toolbar, .keys .unready').first().waitFor({ timeout: 10000 });
  }
  await sleep(400);
  return { ctx, page };
}

const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').replace(/ ([:,])/g, '$1').trim();
const sent = (st, method, re) => st.calls.filter((c) => c.method === method && re.test(c.path));

for (const size of SIZES) {
  for (const theme of THEMES) {
    const tag = `${size[0]}-${theme}`;
    const first = process.env.CHECKS !== '0' && size === SIZES[0] && theme === THEMES[0];
    const shot = async (page, name, { docs = null } = {}) => {
      await sleep(150);
      await page.screenshot({ path: join(OUT, `${tag}-${name}.png`) });
      if (DOCS && docs && size[0] === 1440 && theme === 'light') await page.screenshot({ path: join(DOCS, docs) });
    };

    // ---- the list, a key's page, the families, the leak report ----
    if (wants('list')) {
      const st = new KeysStation();
      const { ctx, page } = await open(st, size, theme);
      if (first) {
        check('the frame has a Keys tab with its key, for a producer', (await words(page.locator('.frame .tabs'))).includes('Keys ⌃6'));
        const f = await page.locator('.frame').boundingBox();
        check('the frame is still one row with the Keys tab', f.height < 60, `frame height ${f.height}`);
        check('the list counts the keys and says how many are shown', /137 keys, the newest 60 shown/.test(await words(page.locator('[data-testid="keys-count"]'))), await words(page.locator('[data-testid="keys-count"]')));
        check('nothing on the tab says token or access code', !/token|access code/i.test(await words(page.locator('.keys'))));
      }
      await shot(page, 'list', { docs: 'how-to-keys-list.png' });

      // A key's page: Isla Fraser, ends soon, two devices.
      await page.locator('.keys .search input').fill('Isla');
      await page.locator('.rowbtn', { hasText: 'Isla Fraser' }).click();
      await page.locator('[data-panel="keys.key"] .log').waitFor();
      if (first) {
        const pg = await words(page.locator('[data-panel="keys.key"]'));
        check('a key’s page says where it stands, what it opens, its window, limits, use, devices and log', ['ENDS SOON', 'Friday Supporters’ Hour and The Archive Hour', 'Ends 25 Nov 2026, 22:00', '2 devices at once', 'Listens 9', 'Firefox on Linux', 'Safari on iPhone', 'Made'].every((w) => pg.includes(w)), pg);
        check('the key’s page never shows its text', !pg.includes('KETM'));
      }
      await shot(page, 'key-page', { docs: 'how-to-keys-key.png' });

      // Extend by a week: one press, nothing to send again.
      if (first) {
        await page.locator('[data-panel="keys.key"] button', { hasText: /^Extend$/ }).click();
        await page.locator('.kmenu button', { hasText: '+1 week' }).click();
        await page.locator('[data-testid="keys-note"]').waitFor();
        check('+1 week asks the station to extend by 7 days', sent(st, 'POST', /\/extend$/).at(-1)?.body?.by_days === 7);
        check('and the page says the new end', (await words(page.locator('[data-panel="keys.key"]'))).includes('Ends 2 Dec 2026, 22:00'));
      }

      // Pause asks first.
      await page.locator('.keys .search input').fill('Callum');
      await page.locator('.rowbtn', { hasText: 'Callum Reid' }).click();
      await page.locator('[data-panel="keys.key"] .log').waitFor();
      await page.locator('[data-panel="keys.key"] button', { hasText: 'Revoke…' }).click();
      await page.locator('[data-panel="keys.ask"]').waitFor();
      await page.locator('#ka-reason').fill('Asked to leave the supporters’ list');
      await shot(page, 'ask-revoke');
      if (first) {
        check('Revoke asks first, and nothing is sent until it is confirmed', sent(st, 'POST', /\/revoke$/).length === 0);
        await page.locator('[data-panel="keys.ask"] button', { hasText: 'Revoke the key' }).click();
        await page.locator('[data-testid="key-standing"]', { hasText: 'Revoked' }).waitFor();
        check('confirmed, it sends the reason', sent(st, 'POST', /\/revoke$/).at(-1)?.body?.reason === 'Asked to leave the supporters’ list');
        check('a revoked key offers nothing that would change it', (await page.locator('[data-panel="keys.key"] .actions button').allInnerTexts()).join('|') === '');
      } else await page.keyboard.press('Escape');

      // Change, in place.
      await page.locator('.keys .search input').fill('Choir');
      await page.locator('.rowbtn', { hasText: 'Dunfermline Gaelic Choir' }).click();
      await page.locator('[data-panel="keys.key"] .log').waitFor();
      await page.locator('[data-panel="keys.key"] button', { hasText: 'Change…' }).click();
      await page.locator('[data-panel="keys.change"]').waitFor();
      await page.locator('[data-panel="keys.change"]').evaluate((el) => (el.scrollTop = 0));
      await shot(page, 'change');
      if (first) {
        await page.locator('#ch-people').fill('45');
        await page.locator('[data-panel="keys.change"] button[type=submit]').click();
        await page.locator('[data-testid="keys-note"]', { hasText: 'is changed' }).waitFor();
        const body = sent(st, 'PATCH', /listener-keys\//).at(-1)?.body;
        check('a change sends only what changed', JSON.stringify(body) === '{"max_people":45}', JSON.stringify(body));
      } else await page.keyboard.press('Escape');

      // Families.
      await page.locator('.keys .seg button', { hasText: 'Families' }).click();
      await page.locator('[data-panel="keys.families"] table').waitFor();
      await shot(page, 'families', { docs: 'how-to-keys-families.png' });
      if (first) {
        const down = page.waitForEvent('download');
        await page.locator('[data-panel="keys.families"] tr', { hasText: 'Autumn patrons' }).locator('button', { hasText: 'Export CSV' }).click();
        const file = await (await down).path();
        const csv = readFileSync(file, 'utf8');
        check('a family’s export is the station’s own CSV, a row a key with its link', csv.startsWith('name,email,note,key,link,state\r\n') && csv.includes('Isla Fraser,isla.fraser@mail.example,,KETM-7QMP-2D4X,https://kettle.example/listen/#key=KETM-7QMP-2D4X,active'), csv.slice(0, 200));
      }

      // The leak report, and the key it points at.
      await page.locator('.keys .seg button', { hasText: 'Leak report' }).click();
      await page.locator('[data-panel="keys.leaks"] table').waitFor();
      if (first) {
        const rows = await page.locator('[data-panel="keys.leaks"] tbody tr').evaluateAll((trs) => trs.slice(0, 3).map((tr) => tr.querySelector('.rowbtn')?.textContent));
        check('the leak report puts the key most likely passed on first', rows[0] === 'Hugh Wallace', rows.join(', '));
        const row = await words(page.locator('[data-panel="keys.leaks"] tbody tr').first());
        check('and says why, in the station’s words, with a burst marked', /likely passed on/i.test(row) && row.includes('41 addresses in the last 7 days, where about 2 is usual') && row.includes('17 new addresses within one hour') && /17\s*burst/.test(row), row);
        check('the report never shows an address', !/\d+\.\d+\.\d+\.\d+/.test(await words(page.locator('[data-panel="keys.leaks"]'))));
      }
      await shot(page, 'leaks', { docs: 'how-to-keys-leaks.png' });
      await page.locator('[data-panel="keys.leaks"] .rowbtn', { hasText: 'Hugh Wallace' }).click();
      await page.locator('[data-panel="keys.key"] .log').waitFor();
      await shot(page, 'leaks-key');
      await page.locator('[data-panel="keys.key"] button', { hasText: 'Replace…' }).click();
      await page.locator('[data-panel="keys.ask"]').waitFor();
      await shot(page, 'ask-replace');
      if (first) {
        await page.locator('[data-panel="keys.ask"] button', { hasText: 'Replace the key' }).click();
        await page.locator('[data-panel="keys.send"] [data-testid="key-text"]').waitFor();
        const fresh = st.lkeys[0];
        check('Replace ends the old key and opens Send with the new one, without asking for its text again', st.hugh.state === 'revoked' && fresh.replaces_id === st.hugh.id && (await words(page.locator('[data-testid="key-text"]'))) === written(fresh.text) && sent(st, 'POST', /\/text$/).length === 0);
        await shot(page, 'replaced');
      }
      await ctx.close();
    }

    // ---- make a key, and send it ----
    if (wants('make')) {
      const st = new KeysStation();
      const { ctx, page } = await open(st, size, theme);
      await page.keyboard.press('n');
      await page.locator('[data-panel="keys.make"]').waitFor();
      if (first) {
        check('N opens Make a key, with the common case chosen', (await page.locator('#mk-devices').inputValue()) === '2' && (await page.locator('input[name="mk-kind"][value="personal"]').isChecked()) && (await page.locator('input[name="mk-opens"][value="all"]').isChecked()));
        await page.locator('[data-panel="keys.make"] button[type=submit]').click();
        check('a key with no name is caught before the station is asked', (await page.locator('#mk-label-err').count()) === 1 && sent(st, 'POST', /listener-keys$/).length === 0);
      }
      await page.locator('#mk-label').fill('Tara Douglas');
      await page.locator('#mk-email').fill('tara.douglas@mail.example');
      await page.locator('input[name="mk-opens"][value="some"]').check();
      await page.locator('[data-panel="keys.make"] .shows label', { hasText: 'Friday Supporters’ Hour' }).locator('input').check();
      await page.locator('[data-panel="keys.make"] .quick button', { hasText: 'in a month' }).click();
      await page.locator('[data-panel="keys.make"]').evaluate((el) => (el.scrollTop = 0));
      await shot(page, 'make', { docs: 'how-to-keys-make.png' });
      await page.locator('[data-panel="keys.make"] button[type=submit]').click();
      await page.locator('[data-panel="keys.send"] [data-testid="key-qr"]').waitFor();
      if (first) {
        const body = sent(st, 'POST', /listener-keys$/).at(-1)?.body;
        check('Make a key sends the form as the station takes it', body.label === 'Tara Douglas' && body.email === 'tara.douglas@mail.example' && body.all_shows === false && JSON.stringify(body.shows) === '["id-friday-supporters"]' && body.max_devices === 2 && body.valid_until_ms === st.now * 1000 + 30 * 86_400_000 && body.kind === 'personal', JSON.stringify(body));
        const send = await words(page.locator('[data-panel="keys.send"]'));
        const key = st.lkeys[0];
        check('Send shows the link, the key and a QR code, without a second request for the text', send.includes(written(key.text)) && (await page.locator('#ks-link').inputValue()) === `https://kettle.example/listen/#key=${written(key.text)}` && sent(st, 'POST', /\/text$/).length === 0);
        const msg = await page.locator('#ks-body').inputValue();
        check('the message is ready to send: a greeting, the link, the key, the end, and that it is theirs alone', msg.startsWith('Hello Tara,') && msg.includes(`https://kettle.example/listen/#key=${written(key.text)}`) && msg.includes(written(key.text)) && msg.includes('It works until 20 Dec 2026, 13:52') && msg.includes('This key is yours alone') && !/token|access code/i.test(msg), msg);
        // The QR code, for an independent decoder (tests/keys/qr-decode.py).
        await page.locator('[data-testid="key-qr"]').evaluate((el) => (el.style.cssText = 'position:fixed;left:0;top:0;z-index:99;width:24rem;height:24rem;border:0;border-radius:0'));
        await page.locator('[data-testid="key-qr"]').screenshot({ path: join(OUT, 'qr-made.png'), scale: 'css' });
        await page.locator('[data-testid="key-qr"]').evaluate((el) => (el.style.cssText = ''));
        writeFileSync(join(OUT, 'qr-made.txt'), `https://kettle.example/listen/#key=${written(key.text)}\n`);
        await page.locator('[data-panel="keys.send"] button', { hasText: 'Copy the message' }).click();
        const clip = await page.evaluate(() => navigator.clipboard.readText());
        check('Copy the message copies the subject and the words', clip.startsWith('Your key for Kettle FM\n\nHello Tara,'));
      }
      await shot(page, 'send', { docs: 'how-to-keys-send.png' });
      await ctx.close();
    }

    // ---- make many ----
    if (wants('many')) {
      const st = new KeysStation();
      const { ctx, page } = await open(st, size, theme);
      await page.locator('.keys .toolbar button', { hasText: 'Make many…' }).click();
      await page.locator('[data-panel="keys.make-many"]').waitFor();
      await page.locator('#mm-label').fill('Winter patrons');
      await page.locator('#mm-list').fill('name,email,note\nOona Murray, oona.murray@mail.example\nGavin Scott, gavin.scott@mail.example, joined at the fair\nWendy Hamilton\nnot an email, wendy@\nOona Murray, oona.murray@mail.example');
      if (first) {
        check('a pasted list is counted, its column names left out', (await words(page.locator('[data-testid="mm-count-line"]'))) === '3 people, 2 with an email address · the first line was column names', await words(page.locator('[data-testid="mm-count-line"]')));
        const probs = await words(page.locator('[data-testid="mm-problems"]'));
        check('and every line that cannot be a key is said', probs.includes('Line 5: “wendy@” is not an email address.') && probs.includes('Line 6 repeats Oona Murray'), probs);
        await page.locator('[data-panel="keys.make-many"] button[type=submit]').click();
        check('a list with a bad line is not sent', sent(st, 'POST', /families$/).length === 0 && (await page.locator('#mm-list-err').count()) === 1);
      }
      await shot(page, 'make-many-problems');
      await page.locator('#mm-list').fill('Oona Murray, oona.murray@mail.example\nGavin Scott, gavin.scott@mail.example, joined at the fair\nWendy Hamilton');
      await page.locator('[data-panel="keys.make-many"] .quick button', { hasText: 'in a year' }).click();
      await page.locator('[data-panel="keys.make-many"]').evaluate((el) => (el.scrollTop = 0));
      await shot(page, 'make-many', { docs: 'how-to-keys-make-many.png' });
      await page.locator('[data-panel="keys.make-many"] button[type=submit]').click();
      await page.locator('[data-panel="keys.made-many"]').waitFor();
      if (first) {
        const body = sent(st, 'POST', /families$/).at(-1)?.body;
        check('Make many sends the family, its template and a key a person', body.label === 'Winter patrons' && body.recipients.length === 3 && body.recipients[1].note === 'joined at the fair' && body.template.max_devices === 2 && body.template.all_shows === true, JSON.stringify(body));
        const down = page.waitForEvent('download');
        await page.locator('[data-panel="keys.made-many"] button', { hasText: 'Save as CSV' }).click();
        const csv = readFileSync(await (await down).path(), 'utf8');
        check('the keys made are saved as CSV with their links, a row a key', csv.split('\r\n').length === 5 && csv.includes('Gavin Scott,gavin.scott@mail.example,joined at the fair,') && csv.includes('https://kettle.example/listen/#key='), csv);
      }
      await shot(page, 'made-many');
      await page.locator('[data-panel="keys.made-many"] button', { hasText: 'Done' }).click();
      // Tickets: keys without names.
      await page.locator('.keys .toolbar button', { hasText: 'Make many…' }).click();
      await page.locator('#mm-label').fill('Burns Night tickets');
      await page.locator('input[name="mm-from"][value="count"]').check();
      await page.locator('#mm-count').fill('200');
      await page.locator('#mm-prefix').fill('Ticket');
      await page.locator('#mm-devices').fill('1');
      await shot(page, 'make-many-tickets');
      if (first) {
        await page.locator('[data-panel="keys.make-many"] button[type=submit]').click();
        await page.locator('[data-panel="keys.made-many"]').waitFor();
        const body = sent(st, 'POST', /families$/).at(-1)?.body;
        check('keys without names are a number and a word in front', body.count === 200 && body.prefix === 'Ticket' && body.template.max_devices === 1 && (await words(page.locator('[data-panel="keys.made-many"] h2'))) === '200 keys made in Burns Night tickets', JSON.stringify(body));
      }
      await ctx.close();
    }

    // ---- find a key ----
    if (wants('find')) {
      const st = new KeysStation();
      const { ctx, page } = await open(st, size, theme);
      await page.locator('.keys .toolbar button', { hasText: 'Find a key…' }).click();
      await page.locator('#kfi-text').fill(`found on a forum: https://kettle.example/listen/#key=${written(st.hugh.text).toLowerCase()}`);
      await page.locator('[data-panel="keys.find"] button[type=submit]').click();
      await page.locator('[data-testid="found-key"]').waitFor();
      if (first) check('Find a key says whose a pasted key was, from a link', (await words(page.locator('[data-testid="found-key"]'))).startsWith('It is Hugh Wallace’s key, of the family Autumn patrons.'), await words(page.locator('[data-testid="found-key"]')));
      await shot(page, 'find');
      await ctx.close();
    }

    // ---- the key beside a show: the Schedule, the show editor, the desk's Playout panel ----
    if (wants('locks')) {
      const st = new KeysStation();
      const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
      await page.keyboard.press('Control+5');
      await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
      await sleep(600);
      if (first) {
        const keyedBlocks = await page.locator('[data-panel="calendar.week"] .ev:has(.rep.keyed)').evaluateAll((els) => [...new Set(els.map((e) => e.querySelector('b')?.textContent))]);
        check('the week marks the blocks of the shows kept for people with a key, and no others', JSON.stringify(keyedBlocks.sort()) === JSON.stringify(['Friday Supporters’ Hour', 'The Archive Hour']), JSON.stringify(keyedBlocks));
        check('the legend says what the key means', /keyed/i.test(await words(page.locator('.schedule .legend'))) && (await page.locator('.schedule .legend use[href="#g-key"]').count()) === 1);
        const listed = await page.locator('[data-panel="calendar.shows"] .showlist li:has([data-testid="keyed-tick"][data-listen="key"]) b').allInnerTexts();
        check('the list of shows marks them too', listed.length === 3 && listed.some((t) => t.includes('Ceilidh Night Live')), listed.join(' | '));
      }
      await shot(page, 'schedule', { docs: 'how-to-keys-schedule.png' });
      // The show editor: who can listen.
      await page.locator('[data-panel="calendar.shows"] .showrow[data-show="friday-supporters"]').click();
      const details = page.locator('[role="tab"]', { hasText: 'Details' });
      if (await details.count()) await details.click();
      await page.locator('[data-panel="calendar.show-access"]').waitFor({ timeout: 10000 });
      await page.locator('[data-panel="calendar.show-access"]').scrollIntoViewIfNeeded();
      if (first) {
        const acc = await words(page.locator('[data-panel="calendar.show-access"]'));
        check('the show editor says who can listen, what the public hears and who set it', (await page.locator('input[name="access-friday-supporters"][value="key"]').isChecked()) && /the public hears/i.test(acc) && acc.includes('by Shona Campbell') && !/token|access code/i.test(acc), acc);
      }
      await shot(page, 'show-access', { docs: 'how-to-keys-show.png' });
      if (first) {
        // A public show, kept for people with a key from here.
        await page.locator('[data-panel="calendar.shows"], .schedule .editor').first().waitFor();
        await page.keyboard.press('Escape');
        await page.locator('[data-panel="calendar.shows"] .showrow[data-show="steeped"]').click();
        const d2 = page.locator('[role="tab"]', { hasText: 'Details' });
        if (await d2.count()) await d2.click();
        await page.locator('[data-panel="calendar.show-access"]').waitFor();
        await page.locator('input[name="access-steeped"][value="key"]').check();
        await page.locator('[data-panel="calendar.show-access"] select').selectOption('fallback');
        await page.locator('[data-panel="calendar.show-access"] button', { hasText: 'Save who can listen' }).click();
        await page.locator('[data-testid="access-note"]').waitFor();
        const body = sent(st, 'PUT', /shows\/steeped\/access$/).at(-1)?.body;
        check('keeping a show for people with a key sends its rule and what the public hears', body?.rule === 'key' && body.slate === 'fallback' && body.opens_at_ms === null, JSON.stringify(body));
        check('and says what to do next', (await words(page.locator('[data-testid="access-note"]'))) === 'Steeped is now for people with a key. Make keys for its listeners on the Keys tab.');
        await page.locator('[data-panel="calendar.show-access"]').scrollIntoViewIfNeeded();
        await shot(page, 'show-access-saved');
      }
      await ctx.close();

      // A presenter reads who can listen, and cannot change it.
      if (first) {
        const pst = new KeysStation({ role: 'presenter' });
        const p2 = await open(pst, size, theme, { tab: 'schedule' });
        await p2.page.keyboard.press('Control+5');
        await p2.page.locator('[data-panel="calendar.shows"] .showrow[data-show="friday-supporters"]').click();
        const d3 = p2.page.locator('[role="tab"]', { hasText: 'Details' });
        if (await d3.count()) await d3.click();
        await p2.page.locator('[data-testid="access-read"]').waitFor({ timeout: 10000 });
        check('a presenter reads who can listen and is offered nothing to change it', (await words(p2.page.locator('[data-testid="access-read"]'))).startsWith('People with a key.') && (await p2.page.locator('[data-panel="calendar.show-access"] input').count()) === 0);
        await p2.page.locator('[data-panel="calendar.show-access"]').scrollIntoViewIfNeeded();
        await shot(p2.page, 'show-access-presenter');
        await p2.ctx.close();
      }

      // The desk, with a keyed show on air: the Playout panel says what the public hears.
      const air = new KeysStation({ onAir: 'friday-supporters' });
      air.join('fiona', 'Fiona Kennedy', 'presenter');
      const d = await open(air, size, theme, { tab: 'desk' });
      await d.page.locator('[data-testid="keyed-now"]').waitFor({ timeout: 10000 });
      if (first) {
        const said = await words(d.page.locator('[data-testid="keyed-now"]'));
        check('the Playout panel says what the public hears now, as playout says it, and that the show is kept from them', /what the public hears now/i.test(said) && said.includes('Friday Supporters’ Hour is for people with a key, and the station is keeping it from the public: they hear the station’s fallback music until it ends, and people with a key hear the show.'), said);
        check('and the lamp stays the air’s: the keyed line is not a lamp', (await d.page.locator('[data-testid="keyed-now"] .fill-onair').count()) === 0);
      }
      await shot(d.page, 'desk-keyed', { docs: 'how-to-keys-desk.png' });
      await d.ctx.close();
      if (first) {
        // Playout refused to shut its gate: the panel says the show is not kept from the public.
        const bad = new KeysStation({ onAir: 'friday-supporters' });
        bad.gateKept = false;
        bad.join('fiona', 'Fiona Kennedy', 'presenter');
        const b = await open(bad, size, theme, { tab: 'desk' });
        await b.page.locator('[data-testid="keyed-now"][data-kept="false"]').waitFor({ timeout: 10000 });
        const alarm = await words(b.page.locator('[data-testid="keyed-now"]'));
        check('a keyed show playout does not keep from the public is said as an alarm', alarm.includes('but playout is not keeping it from the public: everyone hears the show. Restart playout') && (await b.page.locator('[data-testid="keyed-now"] [role=alert]').count()) === 1, alarm);
        await shot(b.page, 'desk-keyed-not-kept');
        await b.ctx.close();
      }
      // Playout does not answer the station: the third state, not confirmed (1.3.2; the picture 1.3.3).
      {
        const away = new KeysStation({ onAir: 'friday-supporters' });
        away.gateConfirmed = false;
        away.join('fiona', 'Fiona Kennedy', 'presenter');
        const n = await open(away, size, theme, { tab: 'desk' });
        const now = n.page.locator('[data-testid="keyed-now"][data-standing="not-confirmed"]');
        await now.waitFor({ timeout: 10000 });
        if (first) {
          const said = await words(now);
          check('playout not answering: the panel says not confirmed beside its heading, what should be so and what to do', said.includes('What the public hears now · not confirmed') && said.includes('Friday Supporters’ Hour is for people with a key. Playout is not answering the station, so this is not confirmed: the listeners’ stream should be playing the station’s own fallback, not the show. It is asked again every two seconds. If this stays, restart playout, and see its log.'), said);
          check('it is a status and not an alarm, and the chip is drawn dashed', (await now.locator('[role=alert]').count()) === 0 && (await now.locator('[role=status]').count()) === 1 && (await now.locator('.state.dashed').count()) === 1 && (await now.locator('.hollow-warning').count()) === 0);
        }
        await now.scrollIntoViewIfNeeded();
        check(`${tag}: the not-confirmed panel is whole inside the window`, await now.evaluate((el) => { const r = el.getBoundingClientRect(); return r.top >= 0 && r.left >= 0 && r.right <= innerWidth + 0.5 && r.bottom <= innerHeight + 0.5; }));
        await shot(n.page, 'desk-keyed-not-confirmed', { docs: 'how-to-keys-desk-not-confirmed.png' });
        await n.ctx.close();
      }
      if (first) {
        const pub = new KeysStation({ onAir: 'the-long-lunch' });
        pub.join('duncan', 'Duncan Boyd', 'presenter');
        const e = await open(pub, size, theme, { tab: 'desk' });
        await sleep(800);
        check('a public show on air has no keyed line', (await e.page.locator('[data-testid="keyed-now"]').count()) === 0);
        await e.ctx.close();
      }
    }


    // ---- what the tick means on a show that has opened to everyone (1.3.3) ----
    // In 1.3.2 a show kept until a time that had passed stayed ticked in the list, though
    // everyone could listen. The tick now means one thing, that a key is needed now.
    if (wants('opened')) {
      const vp = { width: size[0], height: size[1] };
      const inside = (b) => !!b && b.x >= -0.5 && b.y >= -0.5 && b.x + b.width <= vp.width + 0.5 && b.y + b.height <= vp.height + 0.5;
      const OPENED = 'Opened to everyone 15 Nov 2026, 15:00: no key is needed now. Tick to keep it for people with a key again.';
      /** Kettle FM, where The Archive Hour was kept until Sunday 15 November at 15:00. */
      const withOpened = (o = {}) => {
        const st = new KeysStation(o);
        st.access.get('id-the-archive-hour').opens_at_ms = Date.UTC(2026, 10, 15, 15, 0);
        return st;
      };
      const st = withOpened();
      const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
      await page.keyboard.press('Control+5');
      await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
      await sleep(600);
      const list = page.locator('[data-panel="calendar.shows"]');
      const item = list.locator('.showitem:has(.showrow[data-show="the-archive-hour"])');
      const tick = item.locator('[data-testid="keyed-tick"]');
      if (first) {
        check('opened: the station’s list says the show has a key rule and is open to everyone now', st.keyedShows().shows.find((x) => x.slug === 'the-archive-hour')?.now === 'public');
        check('opened: its tick is not ticked, and the two shows still kept are', (await tick.getAttribute('data-listen')) === 'opened' && !(await tick.locator('input').isChecked()) && (await list.locator('.showitem [data-testid="keyed-tick"] input:checked').count()) === 2 && (await list.locator('.showitem [data-testid="keyed-tick"][data-listen="key"] input:checked').count()) === 2);
        check('opened: a line under its row says when it opened and that ticking keeps it again', (await words(tick.locator('[data-testid="keyed-tick-opened"]'))) === OPENED, await words(tick));
        check('opened: the tick’s tooltip says the same, and is not the keyed show’s', (await tick.locator('label').getAttribute('title')) === 'Not ticked: the show opened to everyone 15 Nov 2026, 15:00, so no key is needed now. Tick to keep it for people with a key again.' && (await list.locator('.showitem:has(.showrow[data-show="friday-supporters"]) [data-testid="keyed-tick"] label').getAttribute('title')).startsWith('Ticked: only people with a key can listen now.'));
        const keyedBlocks = await page.locator('[data-panel="calendar.week"] .ev:has(.rep.keyed)').evaluateAll((els) => [...new Set(els.map((e) => e.querySelector('b')?.textContent))]);
        check('opened: its blocks in the week carry no key, and a screen reader is not told it is keyed', !keyedBlocks.includes('The Archive Hour') && keyedBlocks.includes('Friday Supporters’ Hour') && !(await words(item.locator('.showrow b'))).includes('keyed'), JSON.stringify(keyedBlocks));
      }
      await item.scrollIntoViewIfNeeded();
      check(`${tag} opened: the row, its tick and the line under it are inside the list, and the line is not cut`, inside(await item.boundingBox()) && (await tick.locator('[data-testid="keyed-tick-opened"]').evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.getBoundingClientRect().height > 10)), JSON.stringify(await item.boundingBox()));
      await shot(page, 'opened-1-list', { docs: 'how-to-keys-opened.png' });

      // Make a show keyed, on the Keys tab: the row says when it opened, once.
      if (first) {
        await page.keyboard.press('Control+6');
        await page.locator('[data-testid="about-keep"]').waitFor({ timeout: 10000 });
        const about = await words(page.locator('[data-panel="keys.about"] .showlist li', { hasText: 'The Archive Hour' }));
        check('opened: the Keys tab’s list of shows says when it opened and that no key is needed now', about === 'The Archive Hour Opened to everyone 15 Nov 2026, 15:00: no key is needed now', about);
        await page.locator('[data-testid="about-keep"]').click();
        const keep = page.locator('dialog[data-panel="keys.keep-show"][open]');
        const row = keep.locator('.keeplist li[data-show="the-archive-hour"]');
        await row.waitFor({ timeout: 10000 });
        check('opened: in Make a show keyed its row is not ticked and says when it opened, once', (await row.getAttribute('data-listen')) === 'opened' && !(await row.locator('input').isChecked()) && (await words(row)).includes('Opened to everyone 15 Nov 2026, 15:00') && (await row.locator('[data-testid="keyed-tick-opened"]').count()) === 0, await words(row));
        await row.scrollIntoViewIfNeeded();
        await shot(page, 'opened-2-keep-show');
        await keep.getByRole('button', { name: 'Done' }).click();
        await page.keyboard.press('Control+5');
        await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
        await sleep(400);
      }

      // The show's pane, and Who can listen in its editor.
      await list.locator('.showrow[data-show="the-archive-hour"]').click();
      const pane = page.locator('[data-testid="pane-listen"]');
      await pane.waitFor();
      if (first) check('opened: its pane has the tick unticked, and says once when it opened and that no key is needed now', (await pane.getAttribute('data-listen')) === 'opened' && !(await pane.locator('input').isChecked()) && (await words(pane)) === `Keyed show: The Archive Hour ${OPENED} Who can listen…`, await words(pane));
      check(`${tag} opened: the pane’s tick and its words are in view`, inside(await pane.boundingBox()));
      await shot(page, 'opened-3-pane');
      await pane.getByRole('button', { name: 'Who can listen…' }).click();
      const access = page.locator('[data-panel="calendar.show-access"]');
      await access.locator('[data-testid="access-opened"]').waitFor({ timeout: 10000 });
      await sleep(400);
      if (first) check('opened: Who can listen says it opened, that no key is needed now, and how to keep it again', (await words(access.locator('[data-testid="access-opened"]'))) === 'This show opened to everyone on 15 Nov 2026, 15:00, so no key is needed now. To keep it for people with a key again, clear Opens to everyone or give it a later time, and save.', await words(access.locator('[data-testid="access-opened"]')));
      check(`${tag} opened: that line is inside the window, above the choice it explains`, inside(await access.locator('[data-testid="access-opened"]').boundingBox()) && (await access.locator('[data-testid="access-opened"]').boundingBox()).y < (await access.locator('.choice').boundingBox()).y);
      await shot(page, 'opened-4-who-can-listen');
      if (first) {
        // Ticking it keeps the show again from now: the past time is left out, the rest of the rule kept,
        // and the form open under the tick follows, so a later save cannot send the past time back.
        await pane.locator('input').check();
        await page.locator('[data-testid="pane-listen"][data-listen="key"]').waitFor({ timeout: 10000 });
        const body = sent(st, 'PUT', /shows\/the-archive-hour\/access$/).at(-1)?.body;
        check('opened: ticked, the show is kept again from now, without the past time and with what the public hears as it was', JSON.stringify(body) === '{"rule":"key","slate":"fallback","slate_note":"","opens_at_ms":null}' && (await pane.locator('input').isChecked()), JSON.stringify(body));
        check('opened: the tick says what it did, with what becomes of the feed and the files', (await words(pane.locator('[data-testid="keyed-tick-said"]'))) === 'The Archive Hour is kept for people with a key, from now. Its podcast feed and episode files are held back meanwhile, earlier episodes included. Make keys for its listeners on the Keys tab.', await words(pane));
        await access.locator('[data-testid="access-opened"]').waitFor({ state: 'detached', timeout: 10000 }).catch(() => {});
        check('opened: Who can listen, open under the tick, follows: it no longer says the show opened, its time is empty, and Save has nothing to send', (await access.locator('[data-testid="access-opened"]').count()) === 0 && (await access.locator('input[type="datetime-local"]').inputValue()) === '' && (await access.getByRole('button', { name: 'Save who can listen' }).isDisabled()), await access.locator('input[type="datetime-local"]').inputValue());
        await shot(page, 'opened-5-kept-again');
      }
      await ctx.close();

      if (first) {
        // Anyone who may not change it sees no Keyed mark on a show everyone can hear.
        const p = await open(withOpened({ role: 'presenter' }), size, theme, { tab: 'schedule' });
        await p.page.keyboard.press('Control+5');
        await p.page.locator('[data-panel="calendar.shows"] .showrow[data-show="the-archive-hour"]').waitFor({ timeout: 15000 });
        await sleep(600);
        const pl = p.page.locator('[data-panel="calendar.shows"]');
        check('opened: a presenter sees the word Keyed on the shows still kept, and none on the one that opened', (await pl.locator('.showitem:has(.showrow[data-show="the-archive-hour"]) .keyedmark').count()) === 0 && (await pl.locator('.showitem:has(.showrow[data-show="friday-supporters"]) .keyedmark').count()) === 1 && (await pl.locator('input[type=checkbox]').count()) === 0);
        await shot(p.page, 'opened-6-presenter');
        await p.ctx.close();
      }
    }

    // ---- a key's page at a station that sends no email (1.3.2; the picture 1.3.3) ----
    if (wants('nomail')) {
      const st = new KeysStation({ mail: false });
      // Callum asked for his key again on the player: the station sent nothing, and noted it.
      st.handle('POST', '/api/v1/keyed/lost', { email: 'callum.reid@mail.example' });
      const { ctx, page } = await open(st, size, theme);
      await page.locator('.keys .search input').fill('Callum');
      await page.locator('.rowbtn', { hasText: 'Callum Reid' }).click();
      const note = page.locator('[data-testid="key-lost-waiting"]');
      await note.waitFor({ timeout: 10000 });
      const keyPage = page.locator('[data-panel="keys.key"]');
      if (first) {
        check('no mail: the key’s page says its holder asked for it again, that the station sent nothing, and offers Send…', (await words(note)).startsWith('Its holder asked for it again') && (await words(note)).includes('with Lost your key? on the player. The station sent nothing, because it has no outgoing mail: send it to them yourself.') && (await note.getByRole('button', { name: 'Send…' }).count()) === 1, await words(note));
        check('no mail: the Email line says an address is kept and that the request is only noted', (await words(keyPage.locator('dl.facts').first())).includes('An address is kept, sealed. This station does not send email, so Lost your key? only notes the request here: you send the key yourself, with Send.'), await words(keyPage.locator('dl.facts').first()));
        check('no mail: the change log says nothing was sent', (await words(keyPage.locator('.log'))).includes('Its holder asked for it again (Lost your key?): nothing was sent'), await words(keyPage.locator('.log')));
      }
      check(`${tag} no mail: the note is whole inside the key’s page`, await note.evaluate((el) => { const r = el.getBoundingClientRect(); const p = el.closest('[data-panel="keys.key"]').getBoundingClientRect(); return r.left >= p.left - 0.5 && r.right <= p.right + 0.5 && r.top >= 0 && r.bottom <= innerHeight + 0.5; }));
      await shot(page, 'no-mail-key', { docs: 'how-to-keys-key-no-mail.png' });
      if (first) {
        // Send… answers the request: the key's text is shown, the station notes it, and the note goes.
        await note.getByRole('button', { name: 'Send…' }).click();
        await page.locator('dialog[data-panel="keys.send"][open] [data-testid="send-link"], dialog[data-panel="keys.send"][open] .linkbox, dialog[data-panel="keys.send"][open]').first().waitFor({ timeout: 10000 });
        await sleep(600);
        await page.keyboard.press('Escape');
        await sleep(600);
        await page.locator('[data-testid="key-lost-waiting"]').waitFor({ state: 'detached', timeout: 10000 }).catch(() => {});
        check('no mail: once the key’s text has been shown with Send…, nothing waits, without the page being opened again', (await page.locator('[data-testid="key-lost-waiting"]').count()) === 0 && sent(st, 'POST', /\/text$/).length === 1, String(sent(st, 'POST', /\/text$/).length));
        // Make a key says what an address is for at such a station.
        await page.keyboard.press('n');
        await page.locator('dialog[data-panel="keys.make"][open]').waitFor({ timeout: 10000 });
        check('no mail: Make a key says an address only lets a request be noted', (await words(page.locator('#mk-email-hint'))) === 'Kept sealed. This station does not send email: with an address, a Lost your key? request on the player is noted on the key, for you to send it again.', await words(page.locator('#mk-email-hint')));
        await shot(page, 'no-mail-make');
      }
      await ctx.close();
    }

    // ---- what a key is for: Picture, with its sound is no longer offered (1.3.3) ----
    if (wants('media') && first) {
      const st = new KeysStation();
      // A key made for the picture before 1.3.3, or through the API: it goes on working, and is shown as it is.
      const old = st.put({ label: 'Rhona Sinclair', media: 'video', shows: ['id-friday-supporters'], all_shows: false, note: 'Made for the picture in 1.3.2' });
      const { ctx, page } = await open(st, size, theme);
      await page.keyboard.press('n');
      const make = page.locator('dialog[data-panel="keys.make"][open]');
      await make.waitFor({ timeout: 10000 });
      check('media: Make a key offers Sound and picture or Sound only, and no third', (await make.locator('#mk-media option').allInnerTexts()).join('|') === 'Sound and picture|Sound only' && (await make.locator('#mk-media').inputValue()) === 'both', (await make.locator('#mk-media option').allInnerTexts()).join('|'));
      await page.keyboard.press('Escape');
      await page.locator('.keys .search input').fill('Rhona');
      await page.locator('.rowbtn', { hasText: 'Rhona Sinclair' }).click();
      const keyPage = page.locator('[data-panel="keys.key"]');
      await keyPage.locator('.log').waitFor({ timeout: 10000 });
      check('media: a key made for the picture is still shown as it is on its page', (await words(keyPage.locator('dl.facts').first())).includes('Friday Supporters’ Hour · picture, with its sound'), await words(keyPage.locator('dl.facts').first()));
      await keyPage.getByRole('button', { name: 'Change…' }).click();
      const change = page.locator('dialog[data-panel="keys.change"][open]');
      await change.waitFor({ timeout: 10000 });
      check('media: Change keeps the choice in that key’s own list, chosen, and says what it opens', (await change.locator('#ch-media option').allInnerTexts()).join('|') === 'Sound and picture|Sound only|Picture, with its sound' && (await change.locator('#ch-media').inputValue()) === 'video' && (await words(change.locator('[data-testid="media-old"]'))) === 'This key was made for the picture, with its sound. It opens the same as Sound and picture, and goes on working as it is. The choice is no longer offered for new keys.', await words(change.locator('[data-testid="media-old"]')));
      await change.locator('#ch-media').scrollIntoViewIfNeeded();
      await shot(page, 'media-change-video-key');
      // Another field changed and saved: the key stays a key for the picture.
      await change.locator('#ch-note').fill('Made for the picture in 1.3.2; still in use');
      await change.locator('button[type=submit]').click();
      await change.waitFor({ state: 'hidden', timeout: 10000 });
      const patch = sent(st, 'PATCH', new RegExp(`listener-keys/${old.id}$`)).at(-1)?.body;
      check('media: saving another field sends that field alone, and the key is still for the picture', !!patch && !('media' in patch) && patch.note === 'Made for the picture in 1.3.2; still in use' && st.lkeys.find((k) => k.id === old.id).media === 'video', JSON.stringify(patch));
      // Changed to Sound and picture, the old choice leaves its list.
      await keyPage.getByRole('button', { name: 'Change…' }).click();
      await change.waitFor({ timeout: 10000 });
      await change.locator('#ch-media').selectOption('both');
      await change.locator('button[type=submit]').click();
      await change.waitFor({ state: 'hidden', timeout: 10000 });
      check('media: changed to Sound and picture, that is what is sent', sent(st, 'PATCH', new RegExp(`listener-keys/${old.id}$`)).at(-1)?.body?.media === 'both');
      await keyPage.getByRole('button', { name: 'Change…' }).click();
      await change.waitFor({ timeout: 10000 });
      check('media: and the old choice is no longer in its list', (await change.locator('#ch-media option').allInnerTexts()).join('|') === 'Sound and picture|Sound only' && (await change.locator('[data-testid="media-old"]').count()) === 0, (await change.locator('#ch-media option').allInnerTexts()).join('|'));
      await ctx.close();
    }

    // ---- a station of 1.3.2, which holds no episode file back: the studio says that instead ----
    // The released app goes by the station's version. A build made before the release's version is
    // set says 1.3.2 itself, as its own station does while it holds the files back, so there the
    // app says what 1.3.3 does: until the version is 1.3.3 this checks that, and the other after.
    if (wants('older') && first) {
      const APP = JSON.parse(readFileSync(join(ROOT, 'apps', 'studio', 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
      const [maj, min, pat] = APP.split('.').map((n) => parseInt(n, 10));
      const released = maj > 1 || (maj === 1 && (min > 3 || (min === 3 && pat >= 3)));
      const st = new KeysStation({ version: '1.3.2' });
      st.draft('the-long-lunch', '2026-11-19', { aired: true });
      const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
      await page.keyboard.press('Control+5');
      await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
      await sleep(600);
      const OLDER = 'This station runs WeaveSignal 1.3.2, which leaves a keyed show’s podcast feed and episode files open to anyone. Update the station to hold them back.';
      await page.locator('[data-panel="calendar.week"] .ev.past', { hasText: 'The Long Lunch' }).first().click();
      const line = page.locator('[data-testid="listen-line"]');
      await line.waitFor({ timeout: 10000 });
      if (released) check('older: on a station of 1.3.2 an aired episode says its files stay open and to update, and promises no hold-back', (await words(line)).includes(`The tick keeps the episodes to come for people with a key. ${OLDER}`) && !(await words(line)).includes('held back'), await words(line));
      else check(`older: this build says ${APP} itself, so at a station that says 1.3.2 it words the tick as 1.3.3 does`, (await words(line)).includes('and holds back the show’s podcast feed and episode files, this episode’s too') && !(await words(line)).includes('This station runs WeaveSignal 1.3.2'), await words(line));
      await line.locator('input').check();
      await page.locator('[data-testid="listen-line"][data-listen="key"]').waitFor({ timeout: 10000 });
      const said = await words(line.locator('[data-testid="keyed-tick-said"]'));
      if (released) check('older: and the tick says the same when it is pressed', said === `The Long Lunch is kept for people with a key, from now. ${OLDER} Make keys for its listeners on the Keys tab.`, said);
      else check('older: and the tick says the same when it is pressed', said === 'The Long Lunch is kept for people with a key, from now. Its podcast feed and episode files are held back meanwhile, earlier episodes included. Make keys for its listeners on the Keys tab.', said);
      await shot(page, 'older-station-aired-episode');
      await ctx.close();
    }

    // ---- the way to a keyed show (the operator's click-through, 2026-11-25) ----
    // From where a DJ is: the Keys tab's Make a show keyed…, the three steps and the hints in
    // the make-a-key dialogs, the word Keyed on the Schedule's cards, who can listen on a
    // show's pane and on an episode, and New show's tick. Geometry is checked at every size
    // and theme (a 600 px window is the smallest the packaged app goes); presses once.
    if (wants('route')) {
      const vp = { width: size[0], height: size[1] };
      const inside = (b) => !!b && b.x >= -0.5 && b.y >= -0.5 && b.x + b.width <= vp.width + 0.5 && b.y + b.height <= vp.height + 0.5;
      const KEEP_BODY = '{"rule":"key","slate":"announcement","slate_note":"","opens_at_ms":null}';

      // -- the Keys tab, with no show keyed yet --
      {
        const st = new KeysStation({ shows: false });
        const { ctx, page } = await open(st, size, theme);
        const callout = page.locator('[data-testid="no-keyed-shows"]');
        if (first) check('route: with no keyed show, the Keys tab offers Make a show keyed…, first', (await callout.locator('button').allInnerTexts()).join('|') === 'Make a show keyed…|Open the Schedule' && (await page.locator('[data-testid="about-keep"]').innerText()) === 'Make a show keyed…', (await callout.locator('button').allInnerTexts()).join('|'));
        await shot(page, 'route-1-keys-no-keyed-show');
        await callout.getByRole('button', { name: 'Make a show keyed…' }).click();
        const keep = page.locator('dialog[data-panel="keys.keep-show"][open]');
        await keep.locator('.keeplist li').first().waitFor({ timeout: 10000 });
        const steps = keep.locator('[data-testid="key-steps"]');
        check(`${tag} route: Make a show keyed is inside the window, with its Done`, inside(await keep.boundingBox()) && inside(await keep.getByRole('button', { name: 'Done' }).boundingBox()), JSON.stringify(await keep.boundingBox()));
        if (first) {
          check('route: it lists the station’s shows, each open to everyone, each with the tick Keyed show, unticked', (await keep.locator('.keeplist li').count()) === 7 && (await keep.locator('.keeplist li [data-testid="keyed-tick"] input:not(:checked)').count()) === 7 && (await words(keep.locator('.keeplist li[data-show="steeped"]'))).includes('Everyone can listen.') && (await words(keep.locator('.keeplist li[data-show="steeped"] [data-testid="keyed-tick"] label'))) === 'Keyed show: Steeped', await words(keep.locator('.keeplist')));
          check('route: the three steps stand at its top, the first marked as where you are', (await steps.locator('li[aria-current="step"]').getAttribute('data-step')) === '1' && (await steps.locator('li').count()) === 3 && (await words(steps)).startsWith('1 Step 1, where you are: Keep a show for people with a key You are here'), await words(steps));
          check('route: and a show that does not exist yet is a press away: New keyed show…', (await keep.getByRole('button', { name: 'New keyed show…' }).count()) === 1);
          // 1.3.3: before anyone ticks, the dialog says what a tick does to the show's published episodes.
          check('route: it says, before a tick, that the show’s podcast feed and episode files are held back too, and that a downloaded copy cannot be called back', (await words(keep.locator('p.lead'))).endsWith('Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again. Copies already downloaded cannot be called back.'), await words(keep.locator('p.lead')));
          check('route: and the tick’s own tooltip says what its state means', (await keep.locator('.keeplist li[data-show="steeped"] [data-testid="keyed-tick"] label').getAttribute('title')).startsWith('Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment.'));
        }
        await shot(page, 'route-2-make-a-show-keyed', { docs: 'how-to-keys-keep-show.png' });
        const tickS = keep.locator('.keeplist li[data-show="steeped"] [data-testid="keyed-tick"] input');
        await tickS.check();
        await keep.locator('.keeplist li[data-show="steeped"][data-listen="key"]').waitFor({ timeout: 10000 });
        if (first) {
          const put = sent(st, 'PUT', /shows\/steeped\/access$/);
          check('route: one tick reads the show’s rule and saves it as key, keeping what the public hears', sent(st, 'GET', /shows\/steeped\/access$/).length === 1 && put.length === 1 && JSON.stringify(put[0].body) === KEEP_BODY, JSON.stringify(put.map((c) => c.body)));
          check('route: the row says what the tick did, and that only people with a key can listen', (await words(keep.locator('.keeplist li[data-show="steeped"]'))).includes('Only people with a key can listen.') && (await words(keep.locator('.keeplist li[data-show="steeped"] [data-testid="keyed-tick-said"]'))) === 'Steeped is kept for people with a key, from now. Its podcast feed and episode files are held back meanwhile, earlier episodes included. Make keys for its listeners on the Keys tab.' && (await tickS.isChecked()), await words(keep.locator('.keeplist li[data-show="steeped"]')));
          await tickS.uncheck().catch(() => {});
          const ask = keep.locator('.keeplist li[data-show="steeped"] .ask');
          await ask.waitFor();
          check('route: unticking a keyed show asks first, and nothing is sent until it is answered', (await words(ask)).startsWith('Open Steeped to everyone again? Its keys are kept, and its episodes return to the podcast feed.') && (await tickS.isChecked()) && sent(st, 'PUT', /shows\/steeped\/access$/).length === 1, await words(ask));
          await ask.getByRole('button', { name: 'Open to everyone' }).click();
          await keep.locator('.keeplist li[data-show="steeped"][data-listen="public"]').waitFor();
          check('route: answered, it opens the show to everyone again', JSON.stringify(sent(st, 'PUT', /shows\/steeped\/access$/).at(-1).body) === '{"rule":"public"}' && !(await tickS.isChecked()));
          await tickS.check();
          await keep.locator('.keeplist li[data-show="steeped"][data-listen="key"]').waitFor();
        }
        await shot(page, 'route-3-show-kept');
        // Step 2 from here: Make a key, with the steps and the hints.
        await keep.getByRole('button', { name: 'Next: make a key…' }).click();
        const make = page.locator('dialog[data-panel="keys.make"][open]');
        await make.waitFor();
        await sleep(300);
        const msteps = make.locator('[data-testid="key-steps"]');
        check(`${tag} route: Make a key, with the steps above it, is inside the window`, inside(await make.boundingBox()), JSON.stringify(await make.boundingBox()));
        if (size[1] >= 900) check(`${tag} route: and still fits a 900 px window without a scroll, its button in view`, (await make.evaluate((d) => d.scrollHeight <= d.clientHeight + 1)) && inside(await make.getByRole('button', { name: 'Make the key' }).boundingBox()), String(await make.evaluate((d) => `${d.scrollHeight} against ${d.clientHeight}`)));
        if (first) {
          check('route: Next: make a key… opens Make a key, its second step marked', (await page.locator('dialog[data-panel="keys.keep-show"][open]').count()) === 0 && (await msteps.locator('li[aria-current="step"]').getAttribute('data-step')) === '2');
          check('route: step 1 is a button, and Send is words that say it opens by itself', (await msteps.locator('li[data-step="1"] button').count()) === 1 && (await msteps.locator('li[data-step="3"] button').count()) === 0 && (await words(msteps.locator('li[data-step="3"]'))).includes('Send opens by itself the moment the key is made'), await words(msteps));
          check('route: the show just keyed is there to choose', (await make.locator('input[name="mk-opens"][value="some"]').isEnabled()));
        }
        // The hints: a ? beside the less obvious fields, with the studio's tooltip, inside the dialog.
        const hint = make.locator('[data-hint="mk-kind-tip"]');
        await hint.scrollIntoViewIfNeeded();
        await sleep(200);
        const hb = await hint.boundingBox();
        await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2, { steps: 3 });
        await sleep(450);
        const tipBox = await page.locator('#mk-kind-tip > span').boundingBox();
        check(`${tag} route: pointing at the ? beside Kind shows its hint, inside the window and not over the ?`, (await page.locator('#mk-kind-tip').evaluate((e) => e.classList.contains('show'))) && inside(tipBox) && !(tipBox.x < hb.x + hb.width && hb.x < tipBox.x + tipBox.width && tipBox.y < hb.y + hb.height && hb.y < tipBox.y + tipBox.height), JSON.stringify({ tipBox, hb }));
        if (first) {
          check('route: the hint says what each kind of key is for', (await words(page.locator('#mk-kind-tip'))).startsWith('Personal: one listener, named') && (await hint.getAttribute('aria-label')) === 'About the kind of key' && (await hint.getAttribute('aria-describedby')) === 'mk-kind-tip', await words(page.locator('#mk-kind-tip')));
          check('route: the kind, what it opens, sound or picture, the window and the limits each have one', (await make.locator('.hintbtn').count()) === 5, String(await make.locator('.hintbtn').count()));
        }
        await shot(page, 'route-4-make-a-key-steps-and-hint');
        await page.mouse.move(2, 2);
        if (first) {
          // From the keyboard: Tab reaches a hint and its tooltip shows; Esc hides it and the dialog stays.
          await make.locator('input[name="mk-opens"][value="all"]').focus();
          await page.keyboard.press('Shift+Tab');
          let reached = false;
          for (let i = 0; i < 12 && !reached; i++) {
            reached = await page.evaluate(() => document.activeElement?.getAttribute('data-hint') === 'mk-opens-tip');
            if (!reached) await page.keyboard.press(i < 6 ? 'Shift+Tab' : 'Tab');
          }
          await sleep(300);
          // In a short window the focus scrolls the dialog to the hint, and a scroll hides a tooltip (on
          // purpose): leave and come back by the keyboard, now that the hint is in view.
          if (reached && !(await page.locator('#mk-opens-tip').evaluate((e) => e.classList.contains('show')))) {
            await page.keyboard.press('Shift+Tab');
            await page.keyboard.press('Tab');
            await sleep(400);
          }
          check('route: Tab reaches a hint, and its tooltip shows on the keyboard’s focus', reached && (await page.locator('#mk-opens-tip').evaluate((e) => e.classList.contains('show'))), String(reached));
          await page.keyboard.press('Escape');
          await sleep(200);
          check('route: Esc hides the hint and leaves the dialog open', !(await page.locator('#mk-opens-tip').evaluate((e) => e.classList.contains('show'))) && (await make.count()) === 1);
          // Step 1 from Make a key lies over it: what is typed stays.
          await page.locator('#mk-label').fill('Tara Douglas');
          await msteps.locator('li[data-step="1"] button').click();
          const over = page.locator('dialog[data-panel="keys.keep-show"][open]');
          await over.locator('.keeplist li').first().waitFor();
          check('route: step 1 from Make a key opens Make a show keyed over it, with no second way to Make a key', (await make.count()) === 1 && (await over.getByRole('button', { name: 'Next: make a key…' }).count()) === 0);
          await shot(page, 'route-5-step-1-over-make-a-key');
          await over.getByRole('button', { name: 'Done' }).click();
          await sleep(200);
          check('route: and what was typed in Make a key is still there', (await page.locator('dialog[data-panel="keys.keep-show"][open]').count()) === 0 && (await page.locator('#mk-label').inputValue()) === 'Tara Douglas');
          await make.getByRole('button', { name: 'Make the key' }).click();
          const send = page.locator('dialog[data-panel="keys.send"][open]');
          await send.locator('[data-testid="key-text"]').waitFor();
          const ssteps = send.locator('[data-testid="key-steps"]');
          check('route: Send stands at the third step, and its second makes another key', (await ssteps.locator('li[aria-current="step"]').getAttribute('data-step')) === '3' && (await ssteps.locator('li[data-step="2"] button').count()) === 1 && (await ssteps.locator('li[data-step="1"] button').count()) === 1);
          await shot(page, 'route-6-send-steps');
          await ssteps.locator('li[data-step="2"] button').click();
          await page.locator('dialog[data-panel="keys.make"][open]').waitFor();
          check('route: step 2 from Send closes it and opens Make a key afresh', (await page.locator('dialog[data-panel="keys.send"][open]').count()) === 0 && (await page.locator('#mk-label').inputValue()) === '');
          await page.keyboard.press('Escape');
          // Make many carries the steps too, and the hint on what a family is.
          await page.locator('.keys .toolbar button', { hasText: 'Make many…' }).click();
          const many = page.locator('dialog[data-panel="keys.make-many"][open]');
          await many.waitFor();
          check('route: Make many has the steps and says what a family is for', (await many.locator('[data-testid="key-steps"] li[aria-current="step"]').getAttribute('data-step')) === '2' && (await many.locator('[data-hint="mm-family-tip"]').count()) === 1);
          await shot(page, 'route-7-make-many-steps');
        }
        await ctx.close();
      }

      // -- the Schedule: the tick on the list of shows, a show's pane, an episode, a date not drafted --
      {
        const st = new KeysStation();
        st.draft('the-long-lunch', '2026-11-19', { aired: true });
        st.draft('friday-supporters', '2026-11-20');
        const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
        await page.keyboard.press('Control+5');
        await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
        await sleep(600);
        const list = page.locator('[data-panel="calendar.shows"]');
        const tickOf = (scope, slug) => scope.locator(`[data-testid="keyed-tick"][data-show="${slug}"]`);
        if (first) {
          check('route: every row of the list of shows ends with the tick Keyed show, ticked on the three keyed shows', (await list.locator('.showitem [data-testid="keyed-tick"]').count()) === 7 && (await list.locator('.showitem [data-testid="keyed-tick"] input:checked').count()) === 3 && (await words(tickOf(list, 'steeped').locator('label'))) === 'Keyed show: Steeped' && (await list.locator('.showitem [data-testid="keyed-tick"] label svg use[href="#g-key"]').count()) === 7, await words(list.locator('.showlist')));
          check('route: and its tooltip says what the tick’s state means, and what a press does to the show’s feed and files', (await tickOf(list, 'steeped').locator('label').getAttribute('title')) === 'Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment. Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again. Copies already downloaded cannot be called back.' && (await tickOf(list, 'friday-supporters').locator('label').getAttribute('title')) === 'Ticked: only people with a key can listen now. Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included. Untick to open the show to everyone again.', await tickOf(list, 'steeped').locator('label').getAttribute('title'));
          const blockTitles = await page.locator('[data-panel="calendar.week"] .ev .rep.keyed').evaluateAll((els) => [...new Set(els.map((e) => e.getAttribute('title')))]);
          check('route: every keyed block in the week carries the key beside its time, with the same tooltip', blockTitles.length === 1 && blockTitles[0].startsWith('Keyed: advertised to everyone'), JSON.stringify(blockTitles));
          check('route: the legend names the mark Keyed', /keyed/i.test(await words(page.locator('.schedule .legend'))));
          check('route: New keyed show stands beside New show', (await page.locator('.schedule .btn.newkeyed').innerText()).trim() === 'New keyed show');
        }
        const cut = await list.locator('.showitem').evaluateAll((els) => els.filter((e) => { const t = e.querySelector('.keyedtick label').getBoundingClientRect(); const p = e.getBoundingClientRect(); const b = e.querySelector('.showrow b').getBoundingClientRect(); return t.right > p.right - 2 || t.width < 60 || b.width < 60; }).length);
        check(`${tag} route: the tick is whole on every row, and leaves the show its name`, cut === 0, String(cut));
        const bar = await page.evaluate(() => { const nk = document.querySelector('.schedule .btn.newkeyed').getBoundingClientRect(); const ns = [...document.querySelectorAll('.schedule button')].find((b) => b.textContent.trim() === 'New show').getBoundingClientRect(); return { sameRow: Math.abs(nk.top - ns.top) < 2, right: nk.right, w: window.innerWidth }; });
        check(`${tag} route: New keyed show is on New show’s row, inside the window`, bar.sameRow && bar.right <= bar.w, JSON.stringify(bar));
        await shot(page, 'route-8-schedule-list-ticks', { docs: 'how-to-keys-list-ticks.png' });
        if (first) {
          // Tick a show in the list: it is keyed from that moment, and its blocks carry the key.
          await tickOf(list, 'kettle-drive').locator('input').check();
          await list.locator('[data-testid="keyed-tick"][data-show="kettle-drive"][data-listen="key"]').waitFor({ timeout: 10000 });
          await sleep(500);
          check('route: ticking a show in the list keys it, says so under its row, and its blocks in the week gain the key', JSON.stringify(sent(st, 'PUT', /shows\/kettle-drive\/access$/).at(-1)?.body) === KEEP_BODY && (await words(tickOf(list, 'kettle-drive').locator('[data-testid="keyed-tick-said"]'))).startsWith('Kettle Drive is kept for people with a key, from now.') && (await page.locator('[data-panel="calendar.week"] .ev:has(.rep.keyed)', { hasText: 'Kettle Drive' }).count()) > 0, await words(tickOf(list, 'kettle-drive')));
          check('route: a block tall enough says Keyed on a line of its own, under the kind', (await page.locator('[data-panel="calendar.week"] .ev .rest .kind.kk').first().isVisible()) && /^keyed$/i.test((await page.locator('[data-panel="calendar.week"] .ev .rest .kind.kk').first().innerText()).trim()));
          await shot(page, 'route-8b-schedule-list-ticked');
          // The arrows still move between the rows, and Tab reaches the tick of the row in focus.
          await list.locator('.showrow[data-show="kettle-drive"]').focus();
          await page.keyboard.press('Tab');
          check('route: Tab from a row goes to that row’s tick', await page.evaluate(() => document.activeElement?.closest('[data-testid="keyed-tick"]')?.getAttribute('data-show') === 'kettle-drive'));
        }
        // The Day view has the room: the word stands beside the key on the block.
        await page.keyboard.press('Escape');
        await page.locator('body').click({ position: { x: 5, y: size[1] - 5 } }).catch(() => {});
        await page.keyboard.press('d');
        await sleep(700);
        const dayWord = page.locator('.schedule .ev .evb.wide .rep.keyed .kw');
        check(`${tag} route: in the Day view a keyed block says Keyed beside its key`, (await dayWord.count()) > 0 && (await dayWord.first().isVisible()) && /^keyed$/i.test((await dayWord.first().innerText()).trim()), String(await dayWord.count()));
        await shot(page, 'route-9-schedule-cards-day');
        await page.keyboard.press('w');
        await sleep(500);

        // A public show's pane: the tick under its name, on the Dates tab.
        await list.locator('.showrow[data-show="steeped"]').click();
        const paneListen = page.locator('[data-testid="pane-listen"]');
        await paneListen.waitFor();
        if (first) check('route: a public show’s pane has the tick under its name, on its Dates, and says everyone can listen', (await words(paneListen)) === 'Keyed show: Steeped Everyone can listen. Who can listen…' && !(await paneListen.locator('input').isChecked()) && (await page.locator('[role="tab"][aria-selected="true"]', { hasText: 'Dates' }).count()) === 1, await words(paneListen));
        check(`${tag} route: the pane’s tick and its words are in view`, inside(await paneListen.boundingBox()));
        await shot(page, 'route-10-pane-public');
        if (first) {
          await paneListen.locator('input').check();
          await page.locator('[data-testid="pane-listen"][data-listen="key"]').waitFor({ timeout: 10000 });
          check('route: ticked there, the show is keyed and the pane says so', JSON.stringify(sent(st, 'PUT', /shows\/steeped\/access$/).at(-1)?.body) === KEEP_BODY && (await words(paneListen.locator('[data-testid="keyed-tick-said"]'))).startsWith('Steeped is kept for people with a key, from now.'), await words(paneListen));
          await shot(page, 'route-10b-pane-ticked');
        }
        // Who can listen… leads to the rest of the choice, right under the show's name.
        await paneListen.getByRole('button', { name: 'Who can listen…' }).click();
        const access = page.locator('[data-panel="calendar.show-access"]');
        await access.waitFor({ timeout: 10000 });
        await sleep(400);
        const [ab, sb] = [await access.boundingBox(), await page.locator('[data-panel="calendar.show-step"]').boundingBox()];
        const nameY = await page.locator('.schedule .editor input[aria-label="Show name"]').evaluate((e) => e.getBoundingClientRect().top);
        check(`${tag} route: Who can listen… opens Details with Who can listen in view, right under the name and above the step`, inside(await access.locator('legend').boundingBox()) && !!sb && ab.y > nameY && ab.y < sb.y, JSON.stringify({ ab, nameY, sb }));
        if (first) {
          check('route: the focus is in it, and it is outlined for a moment', (await page.evaluate(() => !!document.activeElement?.closest('[data-panel="calendar.show-access"]'))) && (await access.evaluate((e) => e.classList.contains('jumped'))));
          const es = access.locator('[data-testid="key-steps"]');
          check('route: the steps stand in it on one line, the first marked, and the second goes to Make a key now the show is keyed', (await es.locator('li[aria-current="step"]').getAttribute('data-step')) === '1' && (await es.locator('li[data-step="2"] button').count()) === 1 && (await es.evaluate((o) => o.getBoundingClientRect().height)) < 80 && (await es.locator('li').evaluateAll((ls) => ls.every((l) => l.getBoundingClientRect().height > 10))), String(await es.evaluate((o) => o.getBoundingClientRect().height)));
        }
        await shot(page, 'route-11-who-can-listen');
        if (first) {
          // The tick above changes the rule while Who can listen is open under it: the form follows, so a
          // save there cannot send the old rule back.
          await paneListen.locator('input').click();
          await paneListen.locator('.ask').getByRole('button', { name: 'Open to everyone' }).click();
          await page.locator('[data-testid="pane-listen"][data-listen="public"]').waitFor({ timeout: 10000 });
          await page.locator('input[name="access-steeped"][value="public"]:checked').waitFor({ timeout: 10000 }).catch(() => {});
          const followedOpen = await page.locator('input[name="access-steeped"][value="public"]').isChecked();
          await paneListen.locator('input').check();
          await page.locator('[data-testid="pane-listen"][data-listen="key"]').waitFor({ timeout: 10000 });
          await page.locator('input[name="access-steeped"][value="key"]:checked').waitFor({ timeout: 10000 }).catch(() => {});
          check('route: with Who can listen open, the tick above it changes the form too, both ways, and Save has nothing to send', followedOpen && (await page.locator('input[name="access-steeped"][value="key"]').isChecked()) && (await access.getByRole('button', { name: 'Save who can listen' }).isDisabled()), String(followedOpen));
          await access.locator('[data-testid="key-steps"] li[data-step="2"] button').click();
          await page.locator('dialog[data-panel="keys.make"][open]').waitFor({ timeout: 10000 });
          check('route: and it does: the Keys tab, with Make a key open', (await page.locator('.frame .tab[aria-current="page"]').innerText()).includes('Keys'));
          await page.keyboard.press('Escape');
          await page.keyboard.press('Control+5');
          await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
          await sleep(500);
        }
        await page.keyboard.press('Escape');
        await sleep(200);

        // An episode of a keyed show, still to come: the tick, the show's rule in words, the way to the rest.
        const line = page.locator('[data-testid="listen-line"]');
        await page.locator('[data-panel="calendar.week"] .ev:has(.rep.keyed)', { hasText: 'Friday Supporte' }).first().click();
        await line.waitFor({ timeout: 10000 });
        if (first) check('route: an episode of a keyed show has the tick, ticked, and says the show is kept for people with a key, this episode too', (await line.getAttribute('data-listen')) === 'key' && (await line.locator('input').isChecked()) && (await words(line)) === 'Keyed show: Friday Supporters’ Hour This show is kept for people with a key. Every episode of Friday Supporters’ Hour, this one too: it is advertised to everyone and heard only with a key. Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included. Who can listen…', await words(line));
        check(`${tag} route: the line is in view at the top of the episode, with its tick and its link`, inside(await line.boundingBox()) && inside(await line.getByRole('button', { name: 'Who can listen…' }).boundingBox()));
        await shot(page, 'route-13-episode-keyed');
        // An aired episode: a key keeps the whole show, not that episode.
        await page.keyboard.press('Escape');
        await sleep(200);
        await page.locator('[data-panel="calendar.week"] .ev.past', { hasText: 'The Long Lunch' }).first().click();
        await line.waitFor({ timeout: 10000 });
        if (first) check('route: an aired episode says plainly that a key keeps the whole show, not that episode, with the show’s tick', (await line.getAttribute('data-aired')) === 'true' && (await words(line)) === 'Keyed show: The Long Lunch A key keeps the whole show, not one episode. This one has aired, so nothing here changes who heard it on air. The Long Lunch is open to everyone. The tick keeps the episodes to come for people with a key, and holds back the show’s podcast feed and episode files, this episode’s too, until it is open to everyone again. Copies already downloaded cannot be called back. Who can listen…', await words(line));
        await shot(page, 'route-14-episode-aired');
        if (first) {
          await line.locator('input').check();
          await page.locator('[data-testid="listen-line"][data-listen="key"]').waitFor({ timeout: 10000 });
          check('route: ticked on an episode, it is the show that is keyed', JSON.stringify(sent(st, 'PUT', /shows\/the-long-lunch\/access$/).at(-1)?.body) === KEEP_BODY && (await words(line)).includes('The Long Lunch is kept for people with a key now'), await words(line));
          // 1.3.3: the aired episode's own file is held back with the rest, and the line says so.
          check('route: and the aired episode says its own file is held back with the show’s, and that a downloaded copy cannot be called back', (await words(line)).includes('so its podcast feed lists no episode and its episode files are held back, this episode’s too, until the show is open to everyone. Copies already downloaded cannot be called back.'), await words(line));
          check(`${tag} route: the longer line on an aired episode is in view, with its tick and its link`, inside(await line.boundingBox()) && inside(await line.getByRole('button', { name: 'Who can listen…' }).boundingBox()));
          await shot(page, 'route-14b-episode-aired-keyed');
          await line.getByRole('button', { name: 'Who can listen…' }).click();
          await access.waitFor({ timeout: 10000 });
          await sleep(300);
          check('route: its link opens the show’s Who can listen', (await page.locator('input[name="access-the-long-lunch"][value="key"]').isChecked()) && inside(await access.locator('legend').boundingBox()));
          await page.keyboard.press('Escape');
          await sleep(200);
        }
        // A date not drafted yet says it too.
        await page.keyboard.press('Escape');
        await sleep(200);
        const dotted = page.locator('[data-panel="calendar.week"] .ev.rule:has(.rep.keyed)').first();
        if (await dotted.count()) {
          await dotted.click();
          await line.waitFor({ timeout: 10000 });
          if (first) check('route: a date not drafted yet has the tick and says whose rule it follows', (await line.getAttribute('data-listen')) === 'key' && (await line.locator('input').isChecked()) && (await words(line)).includes('This show is kept for people with a key.'), await words(line));
          await shot(page, 'route-15-date-not-drafted');
        } else if (first) check('route: a date not drafted yet has the tick and says whose rule it follows', false, 'no dotted keyed block in the week');
        await ctx.close();
      }

      // -- where a show is made: New keyed show, New show's tick, and what Done says --
      {
        const st = new KeysStation();
        const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
        await page.keyboard.press('Control+5');
        await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
        const ns = page.locator('dialog[data-panel="calendar.new-show"][open]');
        const tick = ns.locator('[data-testid="ns-listen"]');
        if (first) {
          await page.locator('.schedule button', { hasText: /^New show$/ }).first().click();
          await tick.waitFor({ timeout: 10000 });
          check('route: New show has the same tick, Keyed show, with its line, unticked', (await words(tick)).startsWith('Who can listen Keyed show Kept for people with a key: advertised to everyone, heard only with a key you give out. Its podcast feed and episode files are held back while it is keyed. You make the keys on the Keys tab.') && !(await tick.locator('input').isChecked()) && (await words(ns.locator('h2'))) === 'New show' && (await tick.locator('.keyedtick label svg use[href="#g-key"]').count()) === 1, await words(tick));
          await ns.getByRole('button', { name: 'Cancel' }).click();
          await sleep(300);
        }
        await page.locator('.schedule .btn.newkeyed').click();
        await tick.waitFor({ timeout: 10000 });
        if (first) check('route: New keyed show opens the same dialog with the tick ticked, and its heading says so', (await tick.locator('input').isChecked()) && (await words(ns.locator('h2'))) === 'New keyed show', await words(ns.locator('h2')));
        await page.locator('#ns-name').fill('Crofters’ Hour');
        check(`${tag} route: New keyed show’s first part is inside the window, the tick and its buttons within reach`, inside(await ns.boundingBox()) && (inside(await ns.getByRole('button', { name: 'Next: when it airs' }).boundingBox()) || (await ns.evaluate((d) => d.scrollHeight > d.clientHeight))), JSON.stringify(await ns.boundingBox()));
        await tick.scrollIntoViewIfNeeded();
        await shot(page, 'route-16-new-keyed-show');
        await ns.getByRole('button', { name: 'Next: when it airs' }).click();
        await ns.getByRole('button', { name: 'No dates yet' }).click();
        const kept = ns.locator('[data-testid="ns-kept"]');
        await kept.waitFor({ timeout: 10000 });
        if (first) {
          const put = sent(st, 'PUT', /shows\/crofters-hour\/access$/);
          check('route: made with the tick, the show’s rule is saved as key the moment it exists', put.length === 1 && put[0].body.rule === 'key', JSON.stringify(st.calls.filter((c) => /access/.test(c.path)).map((c) => `${c.method} ${c.path}`)));
          check('route: Done says it is kept for people with a key, and offers Make a key…', (await kept.getAttribute('data-kept')) === 'yes' && (await words(kept)).startsWith('It is kept for people with a key. Listeners need a key from you to hear Crofters’ Hour') && (await kept.getByRole('button', { name: 'Make a key…' }).count()) === 1, await words(kept));
        }
        await shot(page, 'route-17-new-show-done-kept');
        if (first) {
          await kept.getByRole('button', { name: 'Make a key…' }).click();
          await page.locator('dialog[data-panel="keys.make"][open]').waitFor({ timeout: 10000 });
          check('route: Make a key… goes to the Keys tab with Make a key open', (await page.locator('dialog[data-panel="calendar.new-show"][open]').count()) === 0 && (await page.locator('.frame .tab[aria-current="page"]').innerText()).includes('Keys'));
          await page.locator('input[name="mk-opens"][value="some"]').check();
          check('route: and the new show is among the keyed shows there', (await page.locator('[data-panel="keys.make"] .shows label', { hasText: 'Crofters’ Hour' }).count()) === 1);
          // From the Keys tab's list, New keyed show… comes back to the Schedule with the dialog open and ticked.
          await page.keyboard.press('Escape');
          await page.locator('[data-testid="about-keep"]').click();
          await page.locator('dialog[data-panel="keys.keep-show"][open]').getByRole('button', { name: 'New keyed show…' }).click();
          await tick.waitFor({ timeout: 10000 });
          check('route: New keyed show… on the Keys tab opens New show on the Schedule, ticked', (await tick.locator('input').isChecked()) && (await page.locator('.frame .tab[aria-current="page"]').innerText()).includes('Schedule'));
        }
        await ctx.close();
      }
      // The station refuses (playout has keyed shows off): the show is made, and Done says it is not kept.
      {
        const st = new KeysStation({ playoutKeyed: false });
        const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
        await page.keyboard.press('Control+5');
        await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
        await page.locator('.schedule .btn.newkeyed').click();
        const ns = page.locator('dialog[data-panel="calendar.new-show"][open]');
        await page.locator('#ns-name').fill('Crofters’ Hour');
        await ns.getByRole('button', { name: 'Next: when it airs' }).click();
        await ns.getByRole('button', { name: 'No dates yet' }).click();
        const kept = ns.locator('[data-testid="ns-kept"]');
        await kept.waitFor({ timeout: 10000 });
        if (first) check('route: where the station will not key it, Done says the show is open to everyone, why, and where to set it', (await kept.getAttribute('data-kept')) === 'no' && (await words(kept)).startsWith('It is not kept for people with a key: everyone can hear it. Playout has keyed shows off, so nothing would keep this show from the public.') && (await kept.getByRole('button', { name: 'Who can listen…' }).count()) === 1 && st.shows.some((x) => x.slug === 'crofters-hour'), await words(kept));
        check(`${tag} route: Done is inside the window with its buttons`, inside(await ns.boundingBox()) && inside(await ns.getByRole('button', { name: 'Close' }).boundingBox()));
        await shot(page, 'route-18-new-show-done-not-kept');
        if (first) {
          await kept.getByRole('button', { name: 'Who can listen…' }).click();
          const access = page.locator('[data-panel="calendar.show-access"]');
          await access.waitFor({ timeout: 10000 });
          await sleep(300);
          check('route: Who can listen… there opens the show’s editor with People with a key chosen and nothing saved', (await page.locator('input[name="access-crofters-hour"][value="key"]').isChecked()) && sent(st, 'PUT', /shows\/crofters-hour\/access$/).length === 1 && (await words(access)).includes('From the moment you save, new listeners need a key to hear this show.'), await words(access));
          // A tick the station refuses goes back, with the reason beside it.
          await page.keyboard.press('Escape');
          await sleep(200);
          const t = page.locator('[data-panel="calendar.shows"] [data-testid="keyed-tick"][data-show="steeped"]');
          await t.locator('input').click();
          await t.locator('.err').waitFor({ timeout: 10000 });
          check('route: a tick the station refuses goes back, with the reason under the row', !(await t.locator('input').isChecked()) && (await words(t.locator('.err'))).startsWith('Playout has keyed shows off, so nothing would keep this show from the public.'), await words(t));
          await shot(page, 'route-18b-tick-refused');
        }
        await ctx.close();
      }
      // A presenter is offered none of it: no tick anywhere, the mark on a keyed show, and who sets it.
      if (first) {
        const st = new KeysStation({ role: 'presenter' });
        st.draft('friday-supporters', '2026-11-20');
        const { ctx, page } = await open(st, size, theme, { tab: 'schedule' });
        await page.keyboard.press('Control+5');
        await page.locator('[data-panel="calendar.week"] .ev').first().waitFor({ timeout: 15000 });
        await sleep(500);
        check('route: a presenter’s list of shows has no tick, and the word Keyed with the key on the keyed shows', (await page.locator('[data-panel="calendar.shows"] input[type="checkbox"]').count()) === 0 && (await page.locator('[data-panel="calendar.shows"] .showitem .keyedmark').allInnerTexts()).map((x) => x.trim().toLowerCase()).join('|') === 'keyed|keyed|keyed' && (await page.locator('.schedule .btn.newkeyed').count()) === 0);
        await shot(page, 'route-19-presenter-list');
        await page.locator('[data-panel="calendar.week"] .ev:has(.rep.keyed)', { hasText: 'Friday Supporte' }).first().click();
        const line = page.locator('[data-testid="listen-line"]');
        await line.waitFor({ timeout: 10000 });
        check('route: a presenter reads who can listen on an episode, and is told who sets it', (await words(line)).endsWith('A producer or an admin sets who can listen.') && (await line.locator('button, input').count()) === 0 && (await line.locator('.keyedmark').count()) === 1, await words(line));
        await page.locator('.schedule .editor .crumbs .back', { hasText: 'Shows' }).first().click();
        await page.locator('[data-panel="calendar.shows"] .showrow[data-show="friday-supporters"]').click();
        await page.locator('[data-testid="pane-listen"]').waitFor();
        check('route: and on the show’s pane, with no tick and no button', (await page.locator('[data-testid="pane-listen"] button, [data-testid="pane-listen"] input').count()) === 0 && /^keyed only people with a key can listen\.$/i.test(await words(page.locator('[data-testid="pane-listen"]'))), await words(page.locator('[data-testid="pane-listen"]')));
        await page.locator('.schedule button', { hasText: /^New show$/ }).first().click();
        const ns = page.locator('dialog[data-panel="calendar.new-show"][open]');
        await ns.locator('#ns-name').waitFor();
        await sleep(400);
        check('route: nor the tick in New show', (await ns.locator('[data-testid="ns-listen"]').count()) === 0);
        await ctx.close();
      }
    }

    // ---- every state the screen can be in ----
    if (wants('states')) {
      const states = [
        ['state-no-keyed-shows', () => new KeysStation({ shows: false }), {}, async (page) => check('with no keyed show, the screen says so and where to set one', (await words(page.locator('[data-testid="no-keyed-shows"]'))).startsWith('No show needs a key yet.') && (await page.locator('[data-testid="no-keys"]').count()) === 1)],
        ['state-no-keys', () => new KeysStation({ seed: false }), {}, async (page) => check('with no keys, the list offers Make a key and Make many', (await words(page.locator('[data-testid="no-keys"]'))).startsWith('No keys yet.'))],
        ['state-not-set-up', () => new KeysStation({ keysUp: 'no-station-key' }), {}, async (page) => check('keys not set up: the screen says so, with the station’s own sentence', (await page.locator('[data-unready="not-set-up"]').count()) === 1 && (await words(page.locator('[data-testid="station-said"]'))).startsWith('Keys need a station key to seal them under'))],
        ['state-no-database', () => new KeysStation({ keysUp: 'no-database' }), {}, async (page) => check('no database: the same screen with the other sentence', (await words(page.locator('[data-testid="station-said"]'))) === 'Keys are kept in the database.')],
        ['state-older-station', () => new KeysStation({ keysUp: 'older' }), {}, async (page) => check('a station older than 1.3.2: the screen says it has no keys yet', (await page.locator('[data-unready="older"]').count()) === 1)],
        ['state-signed-out', () => new KeysStation(), { refuse: [401, 'Your sign-in has ended. Sign in again.'] }, async (page) => check('a sign-in that ended: Sign in to see the keys', (await words(page.locator('.keys .unready h2'))) === 'Sign in to see the keys')],
        ['state-not-allowed', () => new KeysStation(), { refuse: [403, "Only a producer or an admin can manage keys. Ask the station's admin."] }, async (page) => check('an account the station will not have: said, with its sentence', (await page.locator('[data-unready="not-allowed"]').count()) === 1 && (await words(page.locator('[data-testid="station-said"]'))).startsWith('Only a producer or an admin'))],
      ];
      for (const [name, make, o, verify] of states) {
        const { ctx, page } = await open(make(), size, theme, o);
        if (first) await verify(page);
        await shot(page, name);
        await ctx.close();
      }
      // Keys made and none used: the list, a key's page and the leak report each say so.
      {
        const st = new KeysStation({ seed: false });
        for (const label of ['Tara Douglas', 'Oona Murray', 'Gavin Scott']) st.put({ label, shows: ['id-friday-supporters'], all_shows: false });
        const { ctx, page } = await open(st, size, theme);
        await page.locator('.rowbtn', { hasText: 'Oona Murray' }).click();
        await page.locator('[data-testid="key-unused"]').waitFor();
        await shot(page, 'state-unused-key');
        await page.locator('.keys .seg button', { hasText: 'Leak report' }).click();
        await page.locator('[data-testid="leaks-unused"]').waitFor();
        if (first) check('nothing used yet: the leak report says there is nothing to compare', (await words(page.locator('[data-testid="leak-summary"]'))).startsWith('No key has been used yet') && (await words(page.locator('[data-testid="key-unused"]'))).startsWith('Not used yet.'));
        await shot(page, 'state-unused-leaks');
        await ctx.close();
      }
      // A key whose text can no longer be read.
      {
        const st = new KeysStation();
        st.unseal(st.isla);
        const { ctx, page } = await open(st, size, theme);
        await page.locator('.keys .search input').fill('Isla');
        await page.locator('.rowbtn', { hasText: 'Isla Fraser' }).click();
        await page.locator('[data-panel="keys.key"] button', { hasText: 'Send…' }).click();
        await page.locator('[data-panel="keys.send"] .callout').waitFor();
        if (first) check('a key whose text cannot be read says so and offers Replace', (await words(page.locator('[data-panel="keys.send"] .callout'))).includes('can no longer be read') && !/token key/.test(await words(page.locator('[data-panel="keys.send"]'))));
        await shot(page, 'state-text-unreadable');
        await ctx.close();
      }
      // A presenter has no Keys tab, and ⌃6 says why.
      if (first) {
        const st = new KeysStation({ role: 'presenter' });
        const { ctx, page } = await open(st, size, theme, { tab: 'desk' });
        check('a presenter’s frame has no Keys tab', !(await words(page.locator('.frame .tabs'))).includes('Keys'));
        await page.keyboard.press('Control+6');
        await sleep(300);
        check('and ⌃6 does not open one', (await page.locator('.keys').count()) === 0 && sent(st, 'GET', /listener-keys/).length === 0);
        await shot(page, 'state-presenter');
        await ctx.close();
      }
      // An admin may erase; a producer is not offered it.
      if (first) {
        const admin = await open(new KeysStation({ role: 'admin' }), size, theme);
        await admin.page.locator('.keys .search input').fill('Katie');
        await admin.page.locator('.rowbtn', { hasText: 'Katie Ross' }).click();
        await admin.page.locator('[data-panel="keys.key"] .log').waitFor();
        check('an admin is offered Erase', (await admin.page.locator('[data-panel="keys.key"] button', { hasText: 'Erase…' }).count()) === 1);
        await admin.page.locator('[data-panel="keys.key"] button', { hasText: 'Erase…' }).click();
        await admin.page.locator('[data-panel="keys.ask"]').waitFor();
        await shot(admin.page, 'ask-erase');
        await admin.ctx.close();
        const prod = await open(new KeysStation({ role: 'producer' }), size, theme);
        await prod.page.locator('.keys .search input').fill('Katie');
        await prod.page.locator('.rowbtn', { hasText: 'Katie Ross' }).click();
        await prod.page.locator('[data-panel="keys.key"] .log').waitFor();
        check('a producer is not', (await prod.page.locator('[data-panel="keys.key"] button', { hasText: 'Erase…' }).count()) === 0);
        check('a paused key says why and offers Resume', (await words(prod.page.locator('[data-panel="keys.key"]'))).includes('“Membership payment is late”') && (await prod.page.locator('[data-panel="keys.key"] button', { hasText: 'Resume' }).count()) === 1);
        await shot(prod.page, 'key-paused');
        await prod.ctx.close();
      }
    }
  }
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
