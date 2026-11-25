// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The Keys tab and the player's key against the real control plane (tests/keys/run-real.sh
// starts it with a throwaway PostgreSQL, the studio's and the public pages' dev servers): a show
// is kept for people with a key in the show editor, a key is made and sent, a family is made, the
// key is used from the player by its link, a wrong key and a paused key are told so in the
// station's own codes, and the key's page and the leak report show the use. The stand-in drivers
// (shoot.mjs, shoot-player.mjs) prove the screens' every state; this proves the screens and the
// control plane agree. What it does not prove: a keyed stream, which needs the listener mounts
// and an edge.
//
//   STUDIO=http://127.0.0.1:26420 WEB=http://127.0.0.1:26430 CONTROL=http://127.0.0.1:26310 \
//   ADMIN_TOKEN=… PLAYWRIGHT_CORE=… node tests/keys/drive-real.mjs
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const OUT = process.env.OUT ?? join(here, '..', '..', 'target', 'shots-keys-real');
const { STUDIO, WEB, CONTROL, ADMIN_TOKEN } = process.env;
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}
const words = async (loc) => (await loc.innerText()).replace(/\s+/g, ' ').trim();
const api = async (method, path, body) => {
  const res = await fetch(`${CONTROL}${path}`, { method, headers: { Authorization: `Bearer ${ADMIN_TOKEN}`, ...(body ? { 'content-type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
  const text = await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  return [res.status, data];
};

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
await ctx.addInitScript(() => {
  try {
    localStorage.setItem('weavesignal.theme', 'light');
  } catch {}
});
const page = await ctx.newPage();
page.on('pageerror', (e) => check('no page errors in the studio', false, e.message));
// What the station refused, said in the log: a screen that waits for ever is then explained.
page.on('response', async (r) => {
  if (r.status() >= 400 && /\/api\/v1\/(listener-keys|shows\/[^/]+\/access|public\/keyed)/.test(r.url())) log('the station answered', r.status(), r.request().method(), new URL(r.url()).pathname, (await r.text().catch(() => '')).slice(0, 200));
});
process.on('unhandledRejection', async (e) => {
  await page.screenshot({ path: join(OUT, 'failed.png') }).catch(() => {});
  log('FAIL the drive stopped:', e?.message?.split('\n')[0] ?? e);
  process.exit(1);
});
const shot = (p, name) => p.screenshot({ path: join(OUT, `${name}.png`) });

await page.goto(STUDIO);
await page.locator('[data-panel="studio.frame"]').waitFor({ timeout: 30000 });
await sleep(1500);
check('the station key’s frame has the Keys tab', (await words(page.locator('.frame .tabs'))).includes('Keys'));

// ---- the Keys tab on a station with nothing yet ----
await page.keyboard.press('Control+6');
await page.locator('.keys .toolbar').waitFor({ timeout: 15000 });
await sleep(500);
check('a new station: no keyed show and no key, each said', (await page.locator('[data-testid="no-keyed-shows"]').count()) === 1 && (await page.locator('[data-testid="no-keys"]').count()) === 1, await words(page.locator('.keys .main')));
await shot(page, '01-keys-empty');

// ---- keep a show for people with a key, in the show editor ----
await page.locator('[data-testid="no-keyed-shows"] button').click();
await page.locator('[data-panel="calendar.shows"] .showlist button', { hasText: 'The Archive Hour' }).click({ timeout: 15000 });
const details = page.locator('[role="tab"]', { hasText: 'Details' });
if (await details.count()) await details.click();
await page.locator('[data-panel="calendar.show-access"]').waitFor({ timeout: 15000 });
check('a show with no rule reads as Everyone', await page.locator('input[name="access-the-archive-hour"][value="public"]').isChecked());
await page.locator('input[name="access-the-archive-hour"][value="key"]').check();
await page.locator('[data-panel="calendar.show-access"] input[type="text"]').fill('Supporters hear this show. Join at kettle.example/supporters.');
await page.locator('[data-panel="calendar.show-access"] button', { hasText: 'Save who can listen' }).click();
await page.locator('[data-testid="access-note"]').waitFor({ timeout: 10000 });
{
  const [status, a] = await api('GET', '/api/v1/shows/the-archive-hour/access');
  check('the station has the rule the editor saved', status === 200 && a.rule === 'key' && a.slate === 'announcement' && a.slate_note.startsWith('Supporters hear this show.'), JSON.stringify(a));
  const pub = await (await fetch(`${CONTROL}/api/v1/public/keyed-shows`)).json();
  check('and lists the show as keyed for anyone who asks', pub.shows.length === 1 && pub.shows[0].slug === 'the-archive-hour' && pub.shows[0].now === 'key', JSON.stringify(pub));
}
await page.locator('[data-panel="calendar.show-access"]').scrollIntoViewIfNeeded();
await shot(page, '02-show-access-saved');
await page.locator('[data-panel="calendar.show-access"] button', { hasText: 'Its keys…' }).click();
await page.locator('.keys .toolbar').waitFor();

// ---- make a key, and send it ----
await page.keyboard.press('n');
await page.locator('[data-panel="keys.make"]').waitFor();
await page.locator('#mk-label').fill('Isla Fraser');
await page.locator('#mk-email').fill('isla.fraser@mail.example');
await page.locator('[data-panel="keys.make"] .quick button', { hasText: 'in a month' }).click();
await page.locator('[data-panel="keys.make"] button[type=submit]').click();
await page.locator('[data-panel="keys.send"] [data-testid="key-text"]').waitFor({ timeout: 10000 });
const text = await words(page.locator('[data-testid="key-text"]'));
const link = await page.locator('#ks-link').inputValue();
check('the station made the key: its text is twelve of its own letters in three groups, and the link carries it', /^[ABCDEFGHJKMNPQRSTUVWXYZ23456789]{4}-[ABCDEFGHJKMNPQRSTUVWXYZ23456789]{4}-[ABCDEFGHJKMNPQRSTUVWXYZ23456789]{4}$/.test(text) && link.endsWith(`/listen/#key=${text}`), `${text} ${link}`);
check('the message names the station and carries the link and the key', (await page.locator('#ks-body').inputValue()).includes(link) && (await page.locator('#ks-subject').inputValue()) === 'Your key for Kettle FM');
await page.locator('[data-testid="key-qr"]').screenshot({ path: join(OUT, 'qr-real.png') });
await shot(page, '03-send');
await page.locator('[data-panel="keys.send"] button', { hasText: 'Done' }).click();

// ---- make many ----
await page.locator('.keys .toolbar button', { hasText: 'Make many…' }).click();
await page.locator('#mm-label').fill('Autumn patrons');
await page.locator('#mm-list').fill('Callum Reid, callum.reid@mail.example\nKatie Ross, katie.ross@mail.example, renewal due in December\nHugh Wallace');
await page.locator('[data-panel="keys.make-many"] button[type=submit]').click();
await page.locator('[data-panel="keys.made-many"]').waitFor({ timeout: 10000 });
check('the station made the family, a key a person', (await words(page.locator('[data-panel="keys.made-many"] h2'))) === '3 keys made in Autumn patrons');
await shot(page, '04-made-many');
await page.locator('[data-panel="keys.made-many"] button', { hasText: 'Done' }).click();
await sleep(500);
check('the list is narrowed to the new family', (await words(page.locator('[data-testid="keys-count"]'))).startsWith('3 keys'), await words(page.locator('[data-testid="keys-count"]')));
{
  await page.locator('.keys .seg button', { hasText: 'Families' }).click();
  const down = page.waitForEvent('download');
  await page.locator('[data-panel="keys.families"] button', { hasText: 'Export CSV' }).click();
  const { readFileSync } = await import('node:fs');
  const csv = readFileSync(await (await down).path(), 'utf8');
  check('the family’s export is the station’s CSV with a link a key', csv.startsWith('name,email,note,key,link,state') && csv.split('\n').filter((l) => l.includes('/listen/#key=')).length === 3, csv.slice(0, 160));
  await page.locator('.keys .seg button', { hasText: /^Keys$/ }).click();
}

// ---- the listener: the key's link in the player ----
const lctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, colorScheme: 'light' });
const player = await lctx.newPage();
player.on('pageerror', (e) => check('no page errors in the player', false, e.message));
await player.goto(`${WEB}/listen/#key=${text}`);
await player.locator('[data-testid="key-welcome"]').waitFor({ timeout: 20000 });
check('the real station welcomes the key from its link', (await words(player.locator('[data-testid="key-welcome"]'))).startsWith('Welcome. This key opens every show Kettle FM keeps for key holders, until '), await words(player.locator('[data-testid="key-welcome"]')));
check('and the key is out of the address', !(await player.evaluate(() => location.href)).includes('key='));
await player.screenshot({ path: join(OUT, '05-player-welcome.png'), fullPage: true });
await player.locator('[data-panel="player.key"] button', { hasText: 'Devices' }).click();
await player.locator('[data-testid="key-devices"] li').first().waitFor({ timeout: 10000 });
check('the station names this browser among the key’s devices', /this device/.test(await words(player.locator('[data-testid="key-devices"]'))), await words(player.locator('[data-testid="key-devices"]')));
await player.screenshot({ path: join(OUT, '06-player-devices.png'), fullPage: true });

// A wrong key, in the station's own code.
const l2 = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, colorScheme: 'light' });
const p2 = await l2.newPage();
await p2.goto(`${WEB}/listen/#key=AAAA-BBBB-CCCC`);
await p2.locator('[data-testid="key-problem"]').waitFor({ timeout: 20000 });
check('a wrong key: the station’s not_recognised, in the page’s words', (await p2.locator('[data-testid="key-problem"]').getAttribute('data-code')) === 'not_recognised' && (await words(p2.locator('[data-testid="key-problem"]'))).startsWith('That key is not recognised.'));
await p2.screenshot({ path: join(OUT, '07-player-not-recognised.png'), fullPage: true });
// Lost your key?: the real station answers 202 either way.
await p2.locator('[data-panel="player.key"] button', { hasText: 'Lost your key?' }).click();
await p2.locator('#f-lost').fill('isla.fraser@mail.example');
await p2.locator('.lost button[type=submit]').click();
await p2.locator('[data-testid="key-lost"] [role=status]').waitFor({ timeout: 10000 });
check('Lost your key? is answered by the real station', (await words(p2.locator('[data-testid="key-lost"] [role=status]'))).startsWith('If Kettle FM has a key for that address'));
await l2.close();

// ---- back in the studio: the key's page shows the use, and the leak report reads ----
await page.locator('[data-panel="keys.list"] button', { hasText: 'Clear the filter' }).click();
// The key's page is still open from when it was made, before anyone listened: a press on its
// name closes it, and the next opens it with what the station has now.
if (await page.locator('[data-panel="keys.key"]').count()) await page.locator('[data-panel="keys.key"] button', { hasText: 'Close' }).click();
await page.locator('.rowbtn', { hasText: 'Isla Fraser' }).click();
await page.locator('[data-panel="keys.key"] .log').waitFor({ timeout: 10000 });
{
  const pg = await words(page.locator('[data-panel="keys.key"]'));
  check('the key’s page shows the listen, the device and the log the station kept', /Listens 1/.test(pg) && /on Linux|Chrome|Chromium/.test(pg) && pg.includes('Made') && /lost your key/i.test(pg), pg);
}
await shot(page, '08-key-page-used');
await page.locator('.keys .seg button', { hasText: 'Leak report' }).click();
await page.locator('[data-testid="leak-summary"]').waitFor({ timeout: 10000 });
check('the leak report reads from the real station: one key used, no sign', (await words(page.locator('[data-testid="leak-summary"]'))).startsWith('1 key has been used, and none shows a sign'), await words(page.locator('[data-testid="leak-summary"]')));
await shot(page, '09-leaks');

// ---- pause it: the listener is told, in the station's code ----
await page.locator('[data-panel="keys.key"] button', { hasText: 'Pause…' }).click();
await page.locator('#ka-reason').fill('Payment late');
await page.locator('[data-panel="keys.ask"] button', { hasText: 'Pause the key' }).click();
await page.locator('[data-testid="key-standing"]', { hasText: /paused/i }).waitFor({ timeout: 10000 });
await player.reload();
await player.locator('[data-testid="key-problem"]').waitFor({ timeout: 20000 });
check('a paused key is refused by the real station, and the player says it is switched off for now', (await player.locator('[data-testid="key-problem"]').getAttribute('data-code')) === 'paused' && (await words(player.locator('[data-testid="key-problem"]'))).startsWith('This key is switched off for now.'));
await player.screenshot({ path: join(OUT, '10-player-paused.png'), fullPage: true });
await page.locator('[data-panel="keys.key"] button', { hasText: 'Resume' }).click();
await page.locator('[data-testid="key-standing"]', { hasText: /works|ends soon/i }).waitFor({ timeout: 10000 });
await player.locator('[data-panel="player.key"] button', { hasText: 'Check again' }).click();
await player.locator('[data-testid="key-welcome"]').waitFor({ timeout: 10000 });
check('resumed, the same key works again without being sent again', true);

// ---- extend, change, replace ----
await page.locator('[data-panel="keys.key"] button', { hasText: /^Extend$/ }).click();
await page.locator('.kmenu button', { hasText: '+1 week' }).click();
await page.locator('[data-testid="keys-note"]', { hasText: 'extended' }).waitFor({ timeout: 10000 });
await page.locator('[data-panel="keys.key"] button', { hasText: 'Change…' }).click();
await page.locator('#ch-devices').fill('1');
await page.locator('[data-panel="keys.change"] button[type=submit]').click();
await page.locator('[data-testid="keys-note"]', { hasText: 'is changed' }).waitFor({ timeout: 10000 });
check('the station took the extend and the change', (await words(page.locator('[data-panel="keys.key"]'))).includes('1 device at once'));
await page.locator('[data-panel="keys.key"] button', { hasText: 'Replace…' }).click();
await page.locator('[data-panel="keys.ask"] button', { hasText: 'Replace the key' }).click();
await page.locator('[data-panel="keys.send"] [data-testid="key-text"]').waitFor({ timeout: 10000 });
const fresh = await words(page.locator('[data-testid="key-text"]'));
check('Replace gives the same person a new text', fresh !== text && /^[A-Z2-9]{4}-[A-Z2-9]{4}-[A-Z2-9]{4}$/.test(fresh), fresh);
await shot(page, '11-replaced');
await page.locator('[data-panel="keys.send"] button', { hasText: 'Done' }).click();
{
  const [status, found] = await api('POST', '/api/v1/listener-keys/find', { text });
  check('and the station has ended the old one as replaced', status === 200 && found.state === 'revoked' && !!found.replaced_by_id, JSON.stringify(found));
}
// Find a key: the old text, found on a forum.
await page.locator('.keys .toolbar button', { hasText: 'Find a key…' }).click();
await page.locator('#kfi-text').fill(`https://kettle.example/listen/#key=${text.toLowerCase()}`);
await page.locator('[data-panel="keys.find"] button[type=submit]').click();
await page.locator('[data-testid="found-key"]').waitFor({ timeout: 10000 });
check('Find a key asks the real station whose a key was', (await words(page.locator('[data-testid="found-key"]'))).startsWith('It is Isla Fraser’s key'), await words(page.locator('[data-testid="found-key"]')));
await shot(page, '12-find');

await browser.close();
const failed = results.filter((r) => !r.ok);
log(`\n${results.length - failed.length} of ${results.length} checks passed against the real control plane; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED:', f.name, f.detail);
  process.exit(1);
}
