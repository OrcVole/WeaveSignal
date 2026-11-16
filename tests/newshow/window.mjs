// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives calendar files (ADR-0281) in the built studio window itself, not a browser: the Tauri
// app's own webview (WebKitGTK on Linux), its requests going out from Rust with the station's
// key, as an installed app's do. calendar.mjs proves the screens in Chromium; this proves the
// three things only the window can: that Import a calendar…'s POST, with the file as its body,
// reaches the station (the app's CSP once blocked every request with a body); that a file
// chosen in the window is read; and that Download .ics lands a file on the computer, through
// the app's `calendar_save` (a webview has no download the page can count on).
//
//   cd apps/studio && pnpm tauri build --debug --no-bundle      # `tauri dev` serves from Vite and behaves as the browser
//   APP=target/debug/weavesignal-studio OUT=/some/dir node tests/newshow/window.mjs
//
// It needs `tauri-driver` (cargo install tauri-driver) and WebKitWebDriver (webkit2gtk-driver)
// on the PATH, and `cage` for a compositor nobody sees (HEADLESS=0 puts the window on this
// desktop instead); with `wlr-randr` the hidden screen is 1440 by 900 (SIZE=2560x1440 for
// another), and the toolbar is then measured in the window's own typeface. Nothing is a
// dependency of the repository.
//
// The window is given a home of its own under OUT (settings, the station's key in a file,
// Downloads), so it never reads the station an installed app is signed in to and never writes
// to this person's Downloads. By default the station is the stand-in (station.mjs) behind a
// loopback HTTP server. STATION_URL and STATION_KEY name a real control plane instead, on
// loopback or a private network only: Make the shows then makes real shows there.
import { execFileSync, spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Station } from './station.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const APP = resolve(process.env.APP ?? join(here, '..', '..', 'target', 'debug', 'weavesignal-studio'));
const OUT = resolve(process.env.OUT ?? '/tmp/weavesignal-window-shots');
const PORT = Number(process.env.PORT ?? 27920);
const WD = Number(process.env.WD_PORT ?? 27924);
const HEADLESS = process.env.HEADLESS !== '0';
// The hidden screen's size, set with wlr-randr when it is installed (else the compositor's own).
const SIZE = process.env.SIZE ?? '1440x900';
const REAL = process.env.STATION_URL ?? '';
const KEY = process.env.STATION_KEY ?? 'stand-in-key';
const SAMPLE = join(here, 'kettle-old-schedule.ics');
if (!existsSync(APP)) throw new Error(`no studio app at ${APP}: build it with \`pnpm tauri build --debug --no-bundle\` in apps/studio`);

const log = (...a) => console.log(...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

// ---- a home of its own ----
const home = join(OUT, 'home');
rmSync(home, { recursive: true, force: true });
const config = join(home, '.config');
const downloads = join(home, 'Downloads');
mkdirSync(join(config, 'org.weavesignal.studio'), { recursive: true });
mkdirSync(downloads, { recursive: true });
const stationUrl = REAL || `http://127.0.0.1:${PORT}`;
writeFileSync(join(config, 'org.weavesignal.studio', 'station.json'), JSON.stringify({ url: stationUrl, kept: 'file' }));
writeFileSync(join(config, 'org.weavesignal.studio', 'station.key'), KEY, { mode: 0o600 });
writeFileSync(join(config, 'user-dirs.dirs'), `XDG_DOWNLOAD_DIR="${downloads}"\n`);

// ---- the station: the stand-in behind real HTTP, with what it was sent kept ----
const station = new Station();
station.now = Math.floor(Date.now() / 1000);
station.add({ slug: 'the-late-kettle', name: 'The Late Kettle', colour_slot: 8, presenter: 'Shona Campbell', hosts: ['Shona Campbell'], rule: { starts_on: '2026-11-03', start: 1260, minutes: 120, repeat: { kind: 'weekly', every: 1, weekdays: [2] }, end: { kind: 'never' }, skip: [], moves: [] } });
/** Every request as it arrived: the method, the path, the headers and the body's bytes. */
const seen = [];
const server = createServer(async (req, res) => {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const raw = Buffer.concat(chunks);
  const url = new URL(req.url, 'http://x');
  seen.push({ method: req.method, path: url.pathname, type: req.headers['content-type'] ?? '', bearer: req.headers.authorization ?? '', bytes: raw.length, text: raw.toString('utf8') });
  if (url.pathname === '/api/v1/studio/events' || url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return res.writeHead(503, { 'content-type': 'application/json' }).end('{"error":"not in the stand-in"}');
  let body;
  try {
    body = raw.length ? JSON.parse(raw.toString('utf8')) : undefined;
  } catch {
    body = undefined;
  }
  const [status, out] = station.handle(req.method, url.pathname + url.search, body);
  if (typeof out === 'string') return res.writeHead(status, { 'content-type': 'text/calendar; charset=utf-8' }).end(out);
  res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(out));
});
if (!REAL) await new Promise((r) => server.listen(PORT, '127.0.0.1', r));

// ---- tauri-driver, in a compositor nobody sees ----
const env = { ...process.env, HOME: home, XDG_CONFIG_HOME: config, XDG_DATA_HOME: join(home, '.local', 'share'), XDG_CACHE_HOME: join(home, '.cache'), WEBKIT_DISABLE_DMABUF_RENDERER: '1', DBUS_SESSION_BUS_ADDRESS: 'unix:path=/nonexistent' };
const driverArgs = ['--port', String(WD), '--native-port', String(WD + 1)];
const runtime = process.env.XDG_RUNTIME_DIR ?? '';
const sockets = () => (runtime && existsSync(runtime) ? readdirSync(runtime).filter((f) => /^wayland-\d+$/.test(f)) : []);
const socketsBefore = sockets();
// Its own process group, so the compositor, the driver and the window all end with the drive.
const driver = HEADLESS
  ? spawn('cage', ['--', 'tauri-driver', ...driverArgs], { env: { ...env, WLR_BACKENDS: 'headless', WLR_LIBINPUT_NO_DEVICES: '1', WLR_RENDERER: 'pixman' }, stdio: 'ignore', detached: true })
  : spawn('tauri-driver', driverArgs, { env, stdio: 'ignore', detached: true });
const stop = () => {
  try {
    process.kill(-driver.pid, 'SIGTERM');
  } catch {
    /* already gone */
  }
};
driver.on('error', (e) => {
  console.error(`could not start ${HEADLESS ? 'cage and ' : ''}tauri-driver: ${e.message}`);
  process.exit(2);
});

async function wd(method, path, body) {
  const res = await fetch(`http://127.0.0.1:${WD}${path}`, { method, headers: { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${JSON.stringify(data.value ?? data).slice(0, 300)}`);
  return data.value;
}
let sid = null;
const el = async (css) => Object.values(await wd('POST', `/session/${sid}/element`, { using: 'css selector', value: css }))[0];
const click = async (css) => wd('POST', `/session/${sid}/element/${await el(css)}/click`, {});
const exec = (script, args = []) => wd('POST', `/session/${sid}/execute/sync`, { script, args });
const shot = async (name) => writeFileSync(join(OUT, `${name}.png`), Buffer.from(await wd('GET', `/session/${sid}/screenshot`), 'base64'));
const textOf = (css) => exec('const e = document.querySelector(arguments[0]); return e ? e.innerText.replace(/\\s+/g, " ").trim() : null;', [css]);
async function until(what, fn, ms = 10000) {
  const t0 = Date.now();
  for (;;) {
    const v = await fn().catch(() => null);
    if (v) return v;
    if (Date.now() - t0 > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(150);
  }
}
/** A button of the toolbar's calendar files, as a button or as an entry of the menu. */
async function openFiles(name) {
  const wide = await exec('const b = [...document.querySelectorAll(".toolbar button.files-wide")].find((x) => x.textContent.trim() === arguments[0]); return !!b && b.getBoundingClientRect().width > 0;', [name]);
  if (wide) return exec('[...document.querySelectorAll(".toolbar button.files-wide")].find((x) => x.textContent.trim() === arguments[0]).click();', [name]);
  await click('.toolbar .more');
  await exec('[...document.querySelectorAll(".toolbar [role=menuitem]")].find((x) => x.querySelector("b").textContent.trim() === arguments[0]).click();', [name]);
}

let code = 0;
try {
  await until('tauri-driver', async () => (await fetch(`http://127.0.0.1:${WD}/status`)).ok, 15000);
  if (HEADLESS) {
    // The compositor's socket is the one that was not there before it started.
    const mine = sockets().find((f) => !socketsBefore.includes(f));
    try {
      if (mine) execFileSync('wlr-randr', ['--output', 'HEADLESS-1', '--custom-mode', SIZE], { env: { ...process.env, WAYLAND_DISPLAY: mine }, stdio: 'ignore' });
    } catch {
      log('wlr-randr did not set the screen to', SIZE, '(not installed, or refused): the compositor’s own size is used');
    }
  }
  sid = (await wd('POST', '/session', { capabilities: { alwaysMatch: { 'tauri:options': { application: APP } } } })).sessionId;
  await until('the studio frame', () => exec('return !!document.querySelector(\'[data-panel="studio.frame"]\')'), 20000);
  const where = await exec('return location.protocol + "//" + location.host');
  check('this is the packaged window, not a browser', where === 'tauri://localhost' || where.includes('tauri.localhost'), where);
  await exec('[...document.querySelectorAll(\'[data-panel="studio.frame"] [role=tab], [data-panel="studio.frame"] button\')].find((b) => /^Schedule/.test(b.textContent.trim())).click();');
  await until('the Schedule tab', () => exec('return !!document.querySelector(".schedule .toolbar .files-menu, .schedule .toolbar .files-wide")'), 20000);
  const size = await exec('return innerWidth + "×" + innerHeight');
  log('window', size);
  // The toolbar in the window's own typeface, said and not judged: the interface is set in the
  // system's own face (`system-ui`), and the week shown is this week, so whether the legend
  // stays beside the buttons at 1440 by 900 (week-clashes.md) differs by desktop and by date.
  // calendar.mjs judges it in Chromium, with the clock fixed.
  await sleep(1200);
  const bar = await exec('const bar = document.querySelector(\'[data-panel="calendar.toolbar"]\'); const kids = [...bar.children].filter((e) => e.getBoundingClientRect().width > 0); const tops = kids.map((e) => Math.round(e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2)); const menu = bar.querySelector(".files-menu").getBoundingClientRect(); return { oneLine: Math.max(...tops) - Math.min(...tops) < 12, menu: menu.width > 0, spare: Math.round(bar.querySelector(".legend").getBoundingClientRect().left - menu.right - 12), range: bar.querySelector(".range").textContent };');
  log('toolbar', size, bar.range, bar.menu ? 'the calendar files as one menu' : 'the calendar files as two buttons', bar.oneLine ? `on one line, ${bar.spare} px to spare beside the legend` : 'the legend on a second line');

  // ---- Subscribe…, and Download .ics ----
  await openFiles('Subscribe…');
  await until('Subscribe…', () => exec('return !!document.querySelector(\'dialog[data-panel="calendar.subscribe"][open]\')'));
  const webcal = await exec('return document.getElementById("sub-webcal")?.value ?? null');
  check('Subscribe… shows the link on the station’s public address, not the address the window talks to', typeof webcal === 'string' && webcal.startsWith('webcal://') && webcal.endsWith('/api/v1/public/schedule.ics') && !webcal.includes(new URL(stationUrl).host), String(webcal));
  await click('#sub-copy');
  await sleep(400);
  const copied = await textOf('[data-testid="sub-said"]');
  check('Copy link says what happened (copied, or selected for the person to copy)', /^(Copied the calendar link\.|It could not be copied from here\. The link is selected in its box: copy it from there\.)$/.test(copied ?? ''), copied ?? '');
  await shot('window-subscribe');
  await click('#sub-download');
  const first = await until('the file to be saved', async () => {
    const t = await textOf('[data-testid="sub-said"]');
    return t && (t.startsWith('Saved as ') || /not saved|did not/i.test(t)) ? t : null;
  });
  const path1 = join(downloads, 'schedule.ics');
  check('Download .ics writes schedule.ics into the Downloads folder and says where', first === `Saved as ${path1}` && existsSync(path1) && readFileSync(path1, 'utf8').startsWith('BEGIN:VCALENDAR'), first);
  if (!REAL) check('the file is what the station answered, read with the station’s key', readFileSync(path1, 'utf8') === station.ics() && seen.some((r) => r.method === 'GET' && r.path === '/api/v1/public/schedule.ics' && r.bearer === `Bearer ${KEY}`));
  await click('#sub-download');
  const path2 = join(downloads, 'schedule (2).ics');
  const second = await until('the second file', async () => ((await textOf('[data-testid="sub-said"]')) === `Saved as ${path2}` ? path2 : null));
  check('a second download never writes over the first', existsSync(second) && readdirSync(downloads).sort().join('|') === 'schedule (2).ics|schedule.ics', readdirSync(downloads).join('|'));
  // This picture shows where the file went: a path under OUT, so it is for this run's eyes only.
  await shot('window-subscribe-saved');
  await exec('document.querySelector(\'dialog[data-panel="calendar.subscribe"]\').close();');
  await sleep(300);

  // ---- Import a calendar…: a file chosen in the window, the POST with a body, Make the shows ----
  await openFiles('Import a calendar…');
  await until('Import a calendar…', () => exec('return !!document.querySelector(\'dialog[data-panel="calendar.import"][open]\')'));
  // WebDriver hands the file to the input as the file chooser would: the page reads it itself.
  await wd('POST', `/session/${sid}/element/${await el('dialog[data-panel="calendar.import"] input[type=file]')}/value`, { text: SAMPLE });
  await until('the proposed shows', () => exec('return document.querySelectorAll(\'dialog[data-panel="calendar.import"] li.item\').length > 0'), 15000);
  const items = await exec('return document.querySelectorAll(\'dialog[data-panel="calendar.import"] li.item\').length');
  const fileLine = await textOf('[data-testid="ic-file"]');
  check('the file chosen in the window is read and its shows proposed', items >= 5 && /events read · \d+ shows proposed/.test(fileLine ?? ''), `${items} shows; ${fileLine}`);
  if (!REAL) {
    const sent = seen.filter((r) => r.method === 'POST' && r.path === '/api/v1/schedule/import/preview');
    const ics = readFileSync(SAMPLE, 'utf8');
    check('the preview’s POST reached the station with the file as its body, as JSON, with the key', sent.length === 1 && sent[0].type === 'application/json' && sent[0].bearer === `Bearer ${KEY}` && JSON.parse(sent[0].text).ics === ics && sent[0].bytes > ics.length, sent.map((r) => `${r.type} ${r.bytes} bytes`).join('; '));
  }
  const foot = await exec('const b = document.getElementById("ic-make").getBoundingClientRect(); return b.bottom <= innerHeight && b.top >= 0 && b.right <= innerWidth;');
  check(`Make the shows is inside the window (${size})`, foot);
  await shot('window-import-shows');
  const label = await textOf('#ic-make');
  await click('#ic-make');
  const made = await until('the shows to be made', async () => (await textOf('[data-testid="ic-made"]')) ?? (await textOf('[data-testid="ic-refusal"]')), 20000);
  check(`${label} makes them, and Done says so`, /^\d+ shows? (is|are) made\.$/.test(made), made);
  if (!REAL) {
    const sent = seen.filter((r) => r.method === 'POST' && r.path === '/api/v1/schedule/import');
    const body = sent.length ? JSON.parse(sent[0].text) : { items: [] };
    check('the import’s POST reached the station once, with the chosen shows as its body', sent.length === 1 && sent[0].type === 'application/json' && sent[0].bearer === `Bearer ${KEY}` && body.items.length === 5 && body.items.every((i) => i.name && i.rule) && station.shows.some((s) => s.slug === 'folk-night'), `${body.items.length} shows, ${sent[0]?.bytes} bytes`);
  }
  await shot('window-import-done');
} catch (e) {
  check('the drive ran to its end', false, e.message);
  if (sid) await shot('window-failed').catch(() => {});
} finally {
  if (sid) await wd('DELETE', `/session/${sid}`).catch(() => {});
  stop();
  server.close();
  const failed = results.filter((r) => !r.ok);
  log(`\n${results.length - failed.length} of ${results.length} passed`);
  for (const f of failed) log('FAILED', f.name, f.detail);
  code = failed.length ? 1 : 0;
}
process.exit(code);
