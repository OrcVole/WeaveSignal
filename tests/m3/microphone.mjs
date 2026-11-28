// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The microphone chooser (1.3.3) and the bare /join/ page in a real browser, without a stack: the
// join page built with tests/m3/fake-room.js in livekit-client's place (as tests/m3/layout.mjs
// builds it), the player and the echo test as they ship (apps/web/dist), and a station that is
// only answers. The microphones are the browser's own fake ones (Chromium lists three), captured
// by the browser's own getUserMedia with the page's own constraints, which this driver reads
// back from each capture.
//
// It checks: no list before the browser has given a microphone; the names after; a choice before
// joining taken at once, kept on the device and asked for at the join; a change during a call
// waiting for its button, then carried on the same track with the rules kept (echo cancelling
// and noise suppression on, gain control off); a refusal leaving the microphone in use and the
// list back on it; microphones unplugged and plugged in; a remembered microphone that is gone;
// one microphone (a name, no list); the call page's lobby with consent sent as before; the echo
// test page untouched; and the bare /join/ page's words. At 390 and 320 px.
//
// What it does not prove: anything about a real room. That a change keeps the publication and
// the desk goes on hearing is the stack's (tests/m3/join.mjs on tests/m3/run-join.sh). Unplugging
// is simulated here (the browser's fake microphones cannot be unplugged): the page's own view of
// the device list is narrowed, the capture is ended, and `devicechange` is fired. A refusal is
// simulated the same way (a headless browser asks nobody).
//
//   cd apps/web && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/m3/microphone.mjs
//   just test-microphone            # the same, with PLAYWRIGHT_CORE from the environment
//
// BROWSER=firefox runs it in Firefox (Playwright's own build; or a stock Firefox over WebDriver
// BiDi with FIREFOX=<executable>, which Playwright calls the channel `moz-firefox`). Firefox's
// fake microphone is a single device, so there the list is filled out with two more names that
// lead to the same fake capture: that run proves the page in Firefox's engine and Firefox's
// handling of the constraints, not Firefox's own change of device.
// Screenshots go to OUT (target/shots-microphone unless told). Ports PORT to PORT+2 (29440 to
// 29442 unless told), on loopback.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { createServer as createTcp } from 'node:net';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const pw = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = process.env.DIST ?? join(ROOT, 'apps', 'web', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'shots-microphone');
const PORT = Number(process.env.PORT ?? 29440);
const STANDIN = process.env.STANDIN ?? join(ROOT, 'target', 'join-standin', 'dist');
const WHICH = process.env.BROWSER ?? 'chromium';
const FIREFOX = WHICH === 'firefox';
mkdirSync(OUT, { recursive: true });
if (!process.env.STANDIN) execFileSync('pnpm', ['exec', 'vite', 'build', '-c', 'vite.joinstand.config.ts', '--logLevel', 'warn'], { cwd: join(ROOT, 'apps', 'web'), stdio: 'inherit' });
if (!existsSync(join(STANDIN, 'join', 'index.html'))) throw new Error(`no stand-in build at ${STANDIN}`);
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/web first`);

const log = (...a) => console.log(...a);
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.json': 'application/json', '.woff2': 'font/woff2' };
const serve = (dist) =>
  createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    const file = normalize(join(dist, path.endsWith('/') ? `${path}index.html` : path));
    if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  });
const web = serve(DIST);
await new Promise((r) => web.listen(PORT, '127.0.0.1', r));
const standin = serve(STANDIN);
await new Promise((r) => standin.listen(PORT + 2, '127.0.0.1', r));
// A room that accepts a connection and never says a word: the player's call stays Connecting.
const held = new Set();
const silent = createTcp((s) => {
  held.add(s);
  s.on('error', () => {});
  s.on('close', () => held.delete(s));
});
await new Promise((r) => silent.listen(PORT + 1, '127.0.0.1', r));
const WEB = `http://127.0.0.1:${PORT}`;
const STAND = `http://127.0.0.1:${PORT + 2}`;
const ROOM = `ws://127.0.0.1:${PORT + 1}`;

const CONSENT = "You'll be live on Kettle FM, and we may keep your call in our archive and podcast for 42 days. To have it taken down, contact calls@kettle.example.";
const PLAYER = { station: 'Kettle FM', show: 'The Morning Kettle', stream_url: '', streams: [], presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, call_in: { open: true, why_not: null, consent_text: CONSENT, consent_version: 'v1', name_max: 60, topic_max: 200 }, relay: { turn_tls_port: 5349, stun_servers: [] }, video: null };

/** The station, as answers. Sample people are from the Tartan list. `sent` collects the bodies
 *  the page posts, so the driver can see consent is sent as it always was. */
async function station(page, { role = 'guest', sent = [] } = {}) {
  const json = (route, body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
  const me = { identity: 'gregor', role, phase: 'off_air', open: false, muted: false, place: role === 'caller' ? 1 : null, queue: role === 'caller' ? 1 : 0, screener_talking: false, talkback: false, held_until_ms: null, station: 'Kettle FM', show: 'The Morning Kettle', presenter: 'Eilidh Fraser-MacLeod', presenter_live: true, show_on_air: true, desk_reachable: true, studio: 'live' };
  await page.route('**/api/v1/**', (route) => {
    const req = route.request();
    const p = new URL(req.url()).pathname;
    if (req.method() === 'POST') sent.push({ path: p, body: req.postData() });
    if (p === '/api/v1/player') return json(route, PLAYER);
    if (p === '/api/v1/branding') return json(route, { accent: null, icon: null, banner: null });
    if (p === '/api/v1/call-in/challenge') return json(route, { token: '', salt: '', bits: 0, expires_at_ms: Date.now() + 600_000 });
    if (p === '/api/v1/pass') return json(route, { kind: 'invite', role, identity: 'gregor', name: 'Gregor Stewart', station: 'Kettle FM', show: 'The Morning Kettle', expires_at_ms: Date.now() + 3_600_000, ended: false, relay: { turn_tls_port: 5349, stun_servers: [] }, not_now: null, studio: 'live' });
    if (p === '/api/v1/call-in') return json(route, { pass: 'call-pass', token: 'x', url: ROOM, room: 'studio-booth-caller-jessie', identity: 'caller-jessie', role: 'caller', name: 'Jessie Brown', hear: 'desk-mm-caller-jessie', see: null, pass_expires_at_ms: Date.now() + 3_600_000 });
    if (p === '/api/v1/pass/token') return json(route, { pass: 'join-pass', token: 'x', url: ROOM, room: 'studio', identity: 'gregor', role, name: 'Gregor Stewart', hear: 'desk-mm-gregor', see: null, pass_expires_at_ms: Date.now() + 3_600_000 }, 201);
    if (p === '/api/v1/pass/events') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: `event: me\ndata: ${JSON.stringify(role === 'caller' ? { ...me, identity: 'caller-jessie', phase: 'waiting' } : me)}\n\n` });
    return json(route, { error: 'not found' }, 404);
  });
}

/**
 * What the driver puts between the page and the browser's devices, before any page script runs.
 * Every capture is the browser's real one; this records each (what was asked, and the track),
 * and lets the driver: hide a microphone (unplugged: gone from the list, a capture of it ends,
 * `devicechange` fires), bring it back, refuse one (as a person pressing Block would), give one
 * a longer name, and, where the browser has a single fake microphone (Firefox), add names that
 * lead to that same capture.
 */
function devices(virtual) {
  const md = navigator.mediaDevices;
  const realList = md.enumerateDevices.bind(md);
  const realGet = md.getUserMedia.bind(md);
  const t = (window.__t = { captures: [], hidden: new Set(), refused: new Set(), renamed: new Map(), virtual: virtual ?? [] });
  try {
    for (const l of JSON.parse(sessionStorage.getItem('test-hidden') ?? '[]')) t.hidden.add(l);
  } catch {}
  const label = (d) => t.renamed.get(d.label) ?? d.label;
  const all = async () => {
    const real = (await realList()).map((d) => ({ kind: d.kind, deviceId: d.deviceId, groupId: d.groupId, label: label(d) }));
    const named = real.some((d) => d.kind === 'audioinput' && d.label);
    const extra = named ? t.virtual.map((v) => ({ kind: 'audioinput', deviceId: v.id, groupId: `g-${v.id}`, label: label(v) })) : [];
    return [...real, ...extra];
  };
  md.enumerateDevices = async () => (await all()).filter((d) => !t.hidden.has(d.label));
  md.getUserMedia = async (c) => {
    if (!c?.audio) return realGet(c);
    const audio = typeof c.audio === 'object' ? c.audio : null;
    const exact = audio?.deviceId?.exact;
    const want = exact ?? audio?.deviceId?.ideal;
    const real = (await realList()).filter((d) => d.kind === 'audioinput').map((d) => ({ deviceId: d.deviceId, groupId: d.groupId, label: label(d) }));
    // Until the browser names its microphones (Firefox, before the first capture) the driver
    // cannot tell which one an id means: then the browser itself answers.
    const named = real.some((d) => d.label);
    const added = t.virtual.map((v) => ({ deviceId: v.id, groupId: `g-${v.id}`, label: label(v), virtual: true }));
    // The names behind ids seen on an earlier page of this visit are kept, so a remembered id
    // can be known as unplugged before this page's list is named.
    let known = {};
    try {
      known = JSON.parse(localStorage.getItem('test-ids') ?? '{}');
      for (const d of real) if (d.label && d.deviceId) known[d.deviceId] = d.label;
      localStorage.setItem('test-ids', JSON.stringify(known));
    } catch {}
    const asked = want ? ([...added, ...real].find((d) => d.deviceId === want) ?? (known[want] ? { deviceId: want, label: known[want] } : undefined)) : undefined;
    const gone = (d) => t.hidden.has(d.label);
    if (exact && asked && gone(asked)) throw new DOMException('The microphone is not there', 'OverconstrainedError');
    if (exact && !asked && named) throw new DOMException('The microphone is not there', 'OverconstrainedError');
    if (asked && t.refused.has(asked.label)) throw new DOMException('Refused', 'NotAllowedError');
    // The browser is asked exactly what the page asked, unless that would reach a microphone the
    // driver has unplugged or one of its added names: then the first one still plugged in.
    let target = asked && !gone(asked) ? asked : null;
    if (!target && t.hidden.size) {
      target = [...real.filter((d) => d.label), ...added].find((d) => !gone(d)) ?? (named ? null : (added.find((d) => !gone(d)) ?? real[0] ?? null));
      if (!target) throw new DOMException('No microphone', 'NotFoundError');
    }
    const { deviceId: _asked, ...rest } = audio ?? {};
    const pass = !target || (target === asked && !target.virtual) ? c : { ...c, audio: target.virtual || !target.deviceId ? rest : { ...rest, deviceId: { exact: target.deviceId } } };
    const stream = await realGet(pass);
    for (const track of stream.getAudioTracks()) {
      const settings = track.getSettings.bind(track);
      const now = (await realList()).find((d) => d.kind === 'audioinput' && d.deviceId === settings().deviceId);
      let as = target?.virtual ? target : { deviceId: settings().deviceId, groupId: settings().groupId, label: now ? label(now) : track.label };
      // The browser's own microphone is unplugged, and the browser gave it anyway (it could not
      // be told apart before it was named): the capture stands for the first added name.
      if (gone(as)) as = added.find((d) => !gone(d)) ?? as;
      if (as.label !== track.label || as.deviceId !== settings().deviceId) {
        Object.defineProperty(track, 'label', { get: () => as.label });
        track.getSettings = () => ({ ...settings(), deviceId: as.deviceId, groupId: as.groupId });
      }
      t.captures.push({ asked: audio ? JSON.parse(JSON.stringify(audio)) : c.audio, track });
    }
    return stream;
  };
  t.changed = () => md.dispatchEvent(new Event('devicechange'));
  t.unplug = (l) => {
    t.hidden.add(l);
    for (const c of t.captures) {
      if (c.track.label === l && c.track.readyState === 'live') {
        c.track.stop();
        c.track.dispatchEvent(new Event('ended'));
      }
    }
    t.changed();
  };
  t.plug = (l) => {
    t.hidden.delete(l);
    t.changed();
  };
}

/** The captures the page has made, as plain facts. */
const captures = (page) =>
  page.evaluate(() =>
    window.__t.captures.map((c) => {
      const s = c.track.getSettings();
      return { asked: c.asked, label: c.track.label, live: c.track.readyState === 'live', ec: s.echoCancellation, ns: s.noiseSuppression, agc: s.autoGainControl };
    }),
  );
/** The join pages' rules, as asked and as the browser says the capture runs. */
// Firefox's fake microphone reports no echo cancelling or noise suppression whatever is asked (a
// real one does), so there only what the page asked for, and the gain control, are read.
const keepsRules = (c) => c.asked?.echoCancellation === true && c.asked?.noiseSuppression === true && c.asked?.autoGainControl === false && c.agc !== true && (FIREFOX || (c.ec !== false && c.ns !== false));
const kept = (page) => page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.mic') ?? 'null'));
const options = (page) => page.locator('[data-panel="join.microphone"] select option').allTextContents();
const selected = (page) => page.locator('[data-panel="join.microphone"] select').evaluate((el) => el.selectedOptions[0]?.textContent ?? '');
const note = (page) => page.locator('[data-mic-note]').innerText();

const measure = (page) =>
  page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const past = [...document.querySelectorAll('.page *')]
      .map((el) => ({ el, r: el.getBoundingClientRect() }))
      .filter((x) => x.r.width > 0 && x.r.right > vw + 1)
      .map((x) => `${x.el.tagName.toLowerCase()} to ${Math.round(x.r.right)}`);
    return { inner: window.innerWidth, scroll: document.scrollingElement.scrollWidth, past: past.slice(0, 6) };
  });
function fits(name, m, width) {
  check(`${name}: the page is as wide as the window (${width} px), and nothing reaches past its edge`, m.inner === width && m.scroll <= width + 1 && m.past.length === 0, JSON.stringify(m));
}

// Firefox's fake microphone is one device: two more names, leading to the same capture.
const VIRTUAL = FIREFOX ? [{ id: 'virtual-headset', label: 'USB headset' }, { id: 'virtual-studio', label: 'Studio microphone' }] : [];
const prefs = { 'media.navigator.streams.fake': true, 'media.navigator.permission.disabled': true };
const launch = (ui = true) =>
  FIREFOX
    ? pw.firefox.launch({ headless: true, firefoxUserPrefs: ui ? prefs : { 'media.navigator.streams.fake': true }, ...(process.env.FIREFOX ? { channel: 'moz-firefox', executablePath: process.env.FIREFOX } : {}) })
    : pw.chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--use-fake-device-for-media-stream', ...(ui ? ['--use-fake-ui-for-media-stream'] : [])] });
const phone = (width) => (FIREFOX ? { viewport: { width, height: 844 }, colorScheme: 'dark' } : { viewport: { width, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, colorScheme: 'dark' });
const press = (page, loc) => (FIREFOX ? loc.click() : loc.tap());
const perms = FIREFOX ? {} : { permissions: ['microphone'] };

const browser = await launch();
log(`browser: ${WHICH} ${browser.version()}`);
let FIRST, SECOND, THIRD;

async function joinPage(ctx, { role = 'guest', hidden = [] } = {}) {
  const page = await ctx.newPage();
  // Unplugged before the page opens: set first, so the devices' stand-in reads it.
  if (hidden.length) await page.addInitScript((h) => sessionStorage.setItem('test-hidden', JSON.stringify(h)), hidden);
  await page.addInitScript(devices, VIRTUAL);
  await station(page, { role });
  await page.goto(`${STAND}/join/#join-pass`);
  await page.getByRole('button', { name: 'Join the studio' }).waitFor();
  return page;
}

try {
  // ---- before the browser has given a microphone: no names, so no list ----
  // Headless Chromium either allows every capture and names its microphones from the start
  // (--use-fake-ui-for-media-stream), or supports no capture at all: here, the second, to see the
  // page before anything is allowed. Firefox names nothing until the first capture, so there
  // the same is read on the way (below), followed by the names.
  if (!FIREFOX) {
    const asking = await launch(false);
    const ctx = await asking.newContext({ ...phone(390), permissions: [] });
    const page = await joinPage(ctx);
    const before = await page.evaluate(async () => (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput').map((d) => d.label));
    check('before the browser allows a microphone it names none, and the page draws no list', before.every((l) => l === '') && (await page.locator('[data-panel="join.microphone"]').count()) === 0, JSON.stringify(before));
    await asking.close();
  }

  // ---- a microphone remembered from an earlier visit that the browser no longer has ----
  // Where the browser names nothing yet (Firefox) the page cannot know it is gone: it asks for it
  // exactly, is told it is not there, and takes the browser's own. Where the list is already
  // named (headless Chromium) it asks once, for the browser's own. Either way it says so.
  {
    const ctx = await browser.newContext({ ...phone(390), ...perms });
    const page = await joinPage(ctx);
    const before = await page.evaluate(async () => (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput').map((d) => d.label));
    if (FIREFOX) check('before the first capture Firefox names no microphone, and the page draws no list', before.every((l) => l === '') && (await page.locator('[data-panel="join.microphone"]').count()) === 0, JSON.stringify(before));
    await page.evaluate(() => localStorage.setItem('weavesignal.mic', JSON.stringify({ id: 'a-microphone-that-is-gone', label: 'Webcam microphone' })));
    await page.evaluate(() => (window.__asked = []));
    await page.evaluate(() => {
      const get = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
      navigator.mediaDevices.getUserMedia = (c) => (window.__asked.push(JSON.parse(JSON.stringify(c.audio))), get(c));
    });
    await press(page, page.getByRole('button', { name: 'Check your microphone' }));
    await page.locator('[data-mic-note]').waitFor({ timeout: 10_000 });
    const asked = await page.evaluate(() => window.__asked);
    const caps = await captures(page);
    const named = await page.evaluate(async () => (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput').map((d) => d.label));
    const using = (await page.locator('[data-panel="join.microphone"]').getAttribute('data-mic')) ?? '';
    check(
      'a remembered microphone the browser no longer has: the browser’s own is taken with the rules, and the page says which',
      caps.length === 1 && caps[0].asked.deviceId === undefined && keepsRules(caps[0]) && (await note(page)) === `The microphone you chose before, Webcam microphone, is not connected. This page is using ${using} instead.` && using !== '' && (await kept(page))?.id === 'a-microphone-that-is-gone',
      `${JSON.stringify(asked)} · ${await note(page)}`,
    );
    check(
      FIREFOX ? 'in Firefox it was asked for exactly first, then the browser’s own: two questions, the first refused as not there' : 'with the list already named it is not asked for at all: one question',
      FIREFOX ? asked.length === 2 && asked[0].deviceId?.exact === 'a-microphone-that-is-gone' && asked[1].deviceId === undefined : asked.length === 1 && asked[0].deviceId === undefined,
      JSON.stringify(asked),
    );
    check('after the capture the browser names its microphones', named.length >= 1 && named.every((l) => l !== ''), JSON.stringify(named));
    await ctx.close();
  }

  for (const width of [390, 320]) {
    const ctx = await browser.newContext({ ...phone(width), ...perms });

    // ---- before joining: the check names the microphone and takes a choice at once ----
    let page = await joinPage(ctx);
    check(`${width} px: no list is drawn before Check your microphone`, (await page.locator('[data-panel="join.microphone"]').count()) === 0);
    await press(page, page.getByRole('button', { name: 'Check your microphone' }));
    await page.locator('[data-panel="join.microphone"] select').waitFor();
    const names = await options(page);
    [FIRST, SECOND, THIRD] = names;
    check(`${width} px: the check lists the microphones by name, with the one in use selected`, names.length === 3 && (await selected(page)) === FIRST, `${JSON.stringify(names)} · ${await selected(page)}`);
    let caps = await captures(page);
    check(`${width} px: the first capture asks for no microphone by name (the browser’s own) and keeps the rules`, caps.length === 1 && caps[0].asked.deviceId === undefined && keepsRules(caps[0]), JSON.stringify(caps));
    check(`${width} px: nothing is remembered until the person chooses`, (await kept(page)) === null);
    fits(`${width} px, the check with its list`, await measure(page), width);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-check-list.png`), fullPage: true });

    await page.locator('[data-panel="join.microphone"] select').selectOption({ label: THIRD });
    await page.locator('[data-mic-note]').waitFor();
    caps = await captures(page);
    check(
      `${width} px: choosing takes that microphone exactly, with the rules, and lets the old one go`,
      caps.length === 2 && typeof caps[1].asked.deviceId?.exact === 'string' && keepsRules(caps[1]) && caps[1].label === THIRD && caps[1].live && !caps[0].live,
      JSON.stringify(caps),
    );
    check(`${width} px: the page says which is in use and that it is kept for next time`, (await note(page)) === `Now using ${THIRD}. This page will use it next time too.` && (await selected(page)) === THIRD, await note(page));
    check(`${width} px: the choice is kept on the device`, (await kept(page))?.label === THIRD, JSON.stringify(await kept(page)));

    // A microphone the browser refuses: the check stays on the one it had, and the list goes back.
    await page.evaluate((l) => window.__t.refused.add(l), SECOND);
    await page.locator('[data-panel="join.microphone"] select').selectOption({ label: SECOND });
    await page.getByRole('alert').waitFor();
    const refusal = await page.getByRole('alert').innerText();
    caps = await captures(page);
    check(
      `${width} px: a refused microphone leaves the check on the one it had, says so, and the list shows it again`,
      refusal === `The browser did not allow ${SECOND}, so this page is still using ${THIRD}. Allow it when the browser asks, then choose it again.` && (await selected(page)) === THIRD && caps.length === 2 && caps[1].live && (await kept(page))?.label === THIRD,
      `${refusal} · ${await selected(page)}`,
    );
    await page.evaluate((l) => window.__t.refused.delete(l), SECOND);

    // ---- joining: the call asks for the remembered microphone, and names it ----
    await press(page, page.getByRole('button', { name: 'Join the studio' }));
    await page.locator('.banner[data-link="connected"]').waitFor({ timeout: 10_000 });
    await page.locator('.io [data-panel="join.microphone"] select').waitFor();
    caps = await captures(page);
    const joinCap = caps.at(-1);
    check(`${width} px: the call asks for the remembered microphone exactly, with the rules, and the check’s capture is let go`, typeof joinCap.asked.deviceId?.exact === 'string' && keepsRules(joinCap) && joinCap.label === THIRD && caps.filter((c) => c.live).length === 1, JSON.stringify(caps));
    check(`${width} px: in the call, Your microphone names the one in use`, (await selected(page)) === THIRD && (await page.locator('.io label.caps').innerText()).toLowerCase() === 'your microphone', await selected(page));
    fits(`${width} px, the call with its list`, await measure(page), width);

    // ---- during the call: a pick changes nothing until its button is pressed ----
    const micBefore = await page.evaluate(() => (window.__micWas = window.__mic.mediaStreamTrack).label);
    await page.locator('.io select').selectOption({ label: FIRST });
    const use = page.getByRole('button', { name: `Use ${FIRST}` });
    await use.waitFor();
    caps = await captures(page);
    check(`${width} px: picking in the list during a call changes nothing yet: the button says what it will do`, caps.at(-1) === undefined || (caps.at(-1).label === THIRD && (await page.evaluate(() => window.__mic.mediaStreamTrack === window.__micWas))), micBefore);
    fits(`${width} px, the call with a microphone picked`, await measure(page), width);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-call-picked.png`), fullPage: true });
    await press(page, use);
    await page.locator('[data-mic-note]').waitFor();
    caps = await captures(page);
    const swapped = await page.evaluate(() => ({ sameTrackObject: !!window.__mic, label: window.__mic.mediaStreamTrack.label, live: window.__mic.mediaStreamTrack.readyState, oldLive: window.__micWas.readyState }));
    check(
      `${width} px: the button changes the microphone on the same track: exactly the new one, the rules kept, the old capture let go`,
      swapped.label === FIRST && swapped.live === 'live' && swapped.oldLive === 'ended' && typeof caps.at(-1).asked.deviceId?.exact === 'string' && keepsRules(caps.at(-1)) && caps.filter((c) => c.live).length === 1,
      `${JSON.stringify(swapped)} ${JSON.stringify(caps.at(-1))}`,
    );
    check(`${width} px: the page says the studio hears the new one, and the device remembers it`, (await note(page)) === `Now using ${FIRST}. The studio hears you through it.` && (await kept(page))?.label === FIRST && (await selected(page)) === FIRST && (await use.count()) === 0, await note(page));
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-call-changed.png`), fullPage: true });

    // The keyboard: an arrow key in the closed list picks, and still changes nothing by itself.
    if (width === 390) {
      const n = (await captures(page)).length;
      await page.locator('.io select').focus();
      await page.keyboard.press('ArrowDown');
      await page.waitForTimeout(300);
      check('an arrow key over the list during a call captures nothing: the change waits for its button', (await captures(page)).length === n && (await page.evaluate(() => window.__mic.mediaStreamTrack.label)) === FIRST);
      await page.locator('.io select').selectOption({ label: FIRST });
      check('picking the microphone in use again takes the button away', (await page.getByRole('button', { name: /^Use / }).count()) === 0);
    }

    // A refusal during the call: the one in use carries on, said as a problem, the list back on it.
    await page.evaluate((l) => window.__t.refused.add(l), SECOND);
    await page.locator('.io select').selectOption({ label: SECOND });
    await press(page, page.getByRole('button', { name: `Use ${SECOND}` }));
    await page.locator('.io [role="alert"]').waitFor();
    const said = await page.locator('.io [role="alert"]').innerText();
    const still = await page.evaluate(() => ({ label: window.__mic.mediaStreamTrack.label, live: window.__mic.mediaStreamTrack.readyState }));
    check(
      `${width} px: a refusal during the call leaves the microphone in use running, says so, and the list shows it again`,
      said === `The browser did not allow ${SECOND}, so this page is still using ${FIRST}. Allow it when the browser asks, then choose it again.` && still.label === FIRST && still.live === 'live' && (await selected(page)) === FIRST && (await kept(page))?.label === FIRST,
      `${said} · ${JSON.stringify(still)} · ${await selected(page)}`,
    );
    fits(`${width} px, the call with a refusal`, await measure(page), width);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-call-refused.png`), fullPage: true });
    await page.evaluate((l) => window.__t.refused.delete(l), SECOND);

    // ---- unplugged and plugged in during the call ----
    await page.evaluate((l) => window.__t.unplug(l), THIRD);
    await page.waitForFunction((l) => ![...document.querySelectorAll('.io select option')].some((o) => o.textContent === l), THIRD);
    check(`${width} px: a microphone unplugged during the call leaves the list, and the one in use carries on`, (await options(page)).length === 2 && (await selected(page)) === FIRST && (await page.evaluate(() => window.__mic.mediaStreamTrack.readyState)) === 'live', JSON.stringify(await options(page)));
    await page.evaluate((l) => window.__t.plug(l), THIRD);
    await page.waitForFunction((l) => [...document.querySelectorAll('.io select option')].some((o) => o.textContent === l), THIRD);
    check(`${width} px: plugged in again it is offered, and nothing is switched`, (await options(page)).length === 3 && (await selected(page)) === FIRST && (await captures(page)).filter((c) => c.live).length === 1);
    // Down to one microphone: a name, not a list.
    await page.evaluate(([a, b]) => (window.__t.unplug(a), window.__t.unplug(b)), [SECOND, THIRD]);
    await page.locator('.io [data-panel="join.microphone"] .one').waitFor();
    check(`${width} px: with one microphone the call shows its name and no list`, (await page.locator('.io .one').innerText()) === FIRST && (await page.locator('.io select').count()) === 0, await page.locator('.io .one').innerText());

    // The microphone in use stops and nothing takes its place (the room client mutes the track):
    // the page says the studio cannot hear, and takes the first microphone that appears.
    await page.evaluate((l) => {
      window.__t.unplug(l);
      window.__mic.isMuted = true;
      window.__mic.emit('muted');
    }, FIRST);
    await page.locator('.io [role="alert"]').waitFor();
    const stopped = await page.locator('.io [role="alert"]').innerText();
    check(`${width} px: with no microphone left the page says the studio cannot hear, and shows none connected`, stopped === 'Your microphone has stopped, so the studio cannot hear you. Plug it back in, or choose another one here.' && (await page.locator('.io .one').innerText()) === 'None connected', `${stopped} · ${await page.locator('.io .one').innerText()}`);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-call-stopped.png`), fullPage: true });
    await page.evaluate((l) => window.__t.plug(l), SECOND);
    await page.waitForFunction(() => window.__mic.mediaStreamTrack.readyState === 'live' && !window.__mic.isMuted, null, { timeout: 10_000 });
    await page.locator('.io .says [data-mic-note]').waitFor();
    caps = await captures(page);
    check(`${width} px: the first microphone to appear is taken, with the rules, and the page says which`, (await note(page)) === `This page is now using ${SECOND}.` && caps.at(-1).label === SECOND && keepsRules(caps.at(-1)) && (await kept(page))?.label === FIRST, `${await note(page)} ${JSON.stringify(caps.at(-1))}`);
    await page.close();

    // ---- the next visit: the remembered microphone is asked for; gone, the page says so ----
    page = await joinPage(ctx);
    await press(page, page.getByRole('button', { name: 'Check your microphone' }));
    await page.locator('[data-panel="join.microphone"] select').waitFor();
    caps = await captures(page);
    check(`${width} px: on the next visit the check asks for the remembered microphone and gets it, with nothing said`, typeof caps[0].asked.deviceId?.exact === 'string' && caps[0].label === FIRST && (await selected(page)) === FIRST && (await page.locator('[data-mic-note]').count()) === 0, JSON.stringify(caps));
    await page.close();
    page = await joinPage(ctx, { hidden: [FIRST] });
    await press(page, page.getByRole('button', { name: 'Check your microphone' }));
    await page.locator('[data-mic-note]').waitFor();
    const fallback = await note(page);
    const using = await selected(page);
    check(`${width} px: a remembered microphone that is gone falls back to the browser’s own, and the page says which`, fallback === `The microphone you chose before, ${FIRST}, is not connected. This page is using ${using} instead.` && using !== FIRST && (await kept(page))?.label === FIRST, fallback);
    fits(`${width} px, the check with the fallback line`, await measure(page), width);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-check-fallback.png`), fullPage: true });
    // The same at the join, without a check first.
    await press(page, page.getByRole('button', { name: 'Done' }));
    await press(page, page.getByRole('button', { name: 'Join the studio' }));
    await page.locator('.banner[data-link="connected"]').waitFor({ timeout: 10_000 });
    await page.locator('.io [data-mic-note]').waitFor();
    check(`${width} px: and the call says the same when it joins on the fallback`, (await note(page)) === fallback, await note(page));
    await page.close();

    // A long name: the list keeps inside the page.
    page = await joinPage(ctx);
    const LONG = 'Microphone Array (Intel® Smart Sound Technology for Digital Microphones)';
    await page.evaluate(([from, to]) => window.__t.renamed.set(from, to), [THIRD, LONG]);
    await press(page, page.getByRole('button', { name: 'Check your microphone' }));
    await page.locator('[data-panel="join.microphone"] select').waitFor();
    await page.locator('[data-panel="join.microphone"] select').selectOption({ label: LONG });
    await page.locator('[data-mic-note]').waitFor();
    fits(`${width} px, a microphone with a long name`, await measure(page), width);
    await page.screenshot({ path: join(OUT, `${WHICH}-${width}-check-long-name.png`), fullPage: true });
    await page.close();

    // ---- the call page: the player's lobby has the same check, and consent is sent as before ----
    {
      const sent = [];
      const p = await ctx.newPage();
      await p.addInitScript(devices, VIRTUAL);
      await station(p, { role: 'caller', sent });
      // What the page posts, read in the page: Playwright's Firefox over WebDriver BiDi gives the
      // stand-in station no request body.
      await p.addInitScript(() => {
        const real = window.fetch;
        window.__posted = [];
        window.fetch = (input, init) => {
          if (init?.method === 'POST' && typeof init.body === 'string') window.__posted.push({ path: new URL(input.url ?? input, location.href).pathname, body: init.body });
          return real.call(window, input, init);
        };
      });
      await p.goto(`${WEB}/`);
      await p.evaluate(() => localStorage.removeItem('weavesignal.mic'));
      await press(p, p.getByRole('button', { name: 'Call the studio' }));
      await p.locator('#f-name').waitFor();
      await press(p, p.getByRole('button', { name: 'Check your microphone' }));
      await p.locator('[data-panel="join.microphone"] select').waitFor();
      await p.locator('[data-panel="join.microphone"] select').selectOption({ label: SECOND });
      await p.locator('[data-mic-note]').waitFor();
      check(`${width} px, the call page: the lobby’s check lists the microphones and takes a choice`, (await note(p)) === `Now using ${SECOND}. This page will use it next time too.` && (await kept(p))?.label === SECOND, await note(p));
      fits(`${width} px, the call page’s lobby with its list`, await measure(p), width);
      await p.screenshot({ path: join(OUT, `${WHICH}-${width}-lobby-list.png`), fullPage: true });
      // Without the tick the call is not placed, as before.
      await p.fill('#f-name', 'Jessie Brown');
      await p.fill('#f-topic', 'The bus timetable');
      await press(p, p.locator('form button[type="submit"]'));
      await p.locator('#consent-err').waitFor();
      check(`${width} px, the call page: no call is placed without the consent tick`, !sent.some((s) => s.path === '/api/v1/call-in'));
      await press(p, p.locator('#f-consent'));
      await press(p, p.locator('form button[type="submit"]'));
      await p.locator('.banner[data-link="connecting"]').waitFor({ timeout: 10_000 });
      await p.waitForFunction(() => window.__t.captures.length >= 3, null, { timeout: 10_000 });
      const posted = await p.evaluate(() => window.__posted);
      const body = JSON.parse(sent.find((s) => s.path === '/api/v1/call-in')?.body ?? posted.find((s) => s.path === '/api/v1/call-in')?.body ?? '{}');
      const keys = Object.keys(body).sort();
      check(
        `${width} px, the call page: the call carries the name, the topic and the consent with its version, as before, and nothing about the microphone`,
        keys.every((k) => ['challenge', 'consent', 'name', 'topic'].includes(k)) && body.name === 'Jessie Brown' && JSON.stringify(body.consent) === JSON.stringify({ accepted: true, version: 'v1' }) && !/mic|device/i.test(JSON.stringify(body)),
        JSON.stringify(body),
      );
      const c = await captures(p);
      check(`${width} px, the call page: the call asks for the chosen microphone, with the rules`, c.at(-1).label === SECOND && keepsRules(c.at(-1)), JSON.stringify(c.at(-1)));
      await p.close();
    }

    // ---- the echo test keeps its own settings, and has no list ----
    {
      const p = await ctx.newPage();
      await p.addInitScript(devices, VIRTUAL);
      await station(p);
      await p.goto(`${WEB}/echo-test/`);
      await press(p, p.getByRole('button', { name: 'Start the echo test' }));
      await p.getByRole('button', { name: 'Stop' }).waitFor();
      const c = await captures(p);
      check(
        `${width} px, the echo test: echo cancelling, noise suppression and gain control all off, no microphone asked for by name, and no list`,
        c.length === 1 && JSON.stringify(c[0].asked) === JSON.stringify({ echoCancellation: false, noiseSuppression: false, autoGainControl: false }) && (await p.locator('select').count()) === 0,
        JSON.stringify(c),
      );
      await p.close();
    }

    // ---- the bare /join/ page ----
    {
      const p = await ctx.newPage();
      await station(p);
      await p.goto(`${WEB}/join/`);
      await p.getByRole('heading', { name: 'Join with an invitation' }).waitFor();
      await p.getByText('Kettle FM has invited').waitFor();
      const text = (await p.locator('main').innerText()).replace(/\s+/g, ' ').trim();
      check(
        `${width} px, the bare /join/ page: says who it is for, what to do with the link, who to ask, and where listeners go`,
        text ===
          'Join with an invitation This page is for people Kettle FM has invited to its studio: guests, remote DJs and presenters. Have the link the station sent you? Open that link, all of it, on this device, and your invitation comes with it. No link, or only part of one? Ask whoever invited you to send it again. Here to listen, or to call the show? Go to Kettle FM’s player.',
        text,
      );
      check(`${width} px, the bare /join/ page: not worded or announced as a fault, no microphone asked for, and the player one press away`, (await p.getByRole('alert').count()) === 0 && !/doesn't work|problem|error/i.test(text) && (await p.getByRole('button').count()) === 0 && (await p.getByRole('link', { name: 'Go to Kettle FM’s player' }).getAttribute('href')) === '/' && (await p.title()) === 'Join Kettle FM', await p.title());
      fits(`${width} px, the bare /join/ page`, await measure(p), width);
      await p.screenshot({ path: join(OUT, `${WHICH}-${width}-join-bare.png`), fullPage: true });
      await p.close();
      // A link whose pass the station does not know is still a link that does not work.
      const bad = await ctx.newPage();
      await bad.route('**/api/v1/**', (route) => route.fulfill({ status: new URL(route.request().url()).pathname === '/api/v1/pass' ? 401 : 404, contentType: 'application/json', body: JSON.stringify({ error: 'This invitation is not one this station made.' }) }));
      await bad.goto(`${WEB}/join/#not-a-pass`);
      await bad.getByRole('heading', { name: "This link doesn't work" }).waitFor();
      check(`${width} px: a link the station refuses still says This link doesn't work, with the station's reason`, (await bad.getByRole('alert').count()) === 1);
      await bad.close();
    }
    await ctx.close();
  }
} finally {
  await browser.close();
  for (const s of held) s.destroy();
  await new Promise((r) => silent.close(r));
  await new Promise((r) => web.close(r));
  await new Promise((r) => standin.close(r));
}

const failed = results.filter((r) => !r.ok);
log(`\n${WHICH}: ${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
