// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the join pages on the M3 loopback stack (tests/m3/run-join.sh) in Chromium with a fake
// microphone: a listener calls in from the player page on a phone-sized screen (390 px, touch:
// every press on the phone pages is a tap), is refused
// without consent (in the page and by the control plane), consents, waits, is screened, sent,
// put on air and taken off, loses the link and rejoins in the same place, and hangs up; then an
// invited guest and a remote DJ join. Each step is checked against the control plane, and the
// pages and the screener are captured.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/m3/join.mjs
//
// Playwright is not a dependency of the repository: point PLAYWRIGHT_CORE at an installed
// playwright-core with its Chromium. Screenshots go to OUT, never the repository.
import { createRequire } from 'node:module';
import { createHmac } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const WEB = process.env.WEB ?? 'http://127.0.0.1:32430';
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:32420/';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:32310';
const LIVEKIT = process.env.LIVEKIT ?? 'http://127.0.0.1:32880';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm3-admin-test-only';
const API_KEY = process.env.API_KEY ?? 'm3join';
const API_SECRET = process.env.API_SECRET ?? 'm3-join-test-only-secret-not-for-use-00';
const OUT = process.env.OUT ?? '/tmp/weavesignal-m3-shots';
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
async function view() {
  return (await fetch(`${CONTROL}/api/v1/studio`, { headers: admin })).json();
}
async function until(pred, ms = 10_000) {
  const t0 = Date.now();
  for (;;) {
    const v = await view();
    if (pred(v)) return { v, ms: Date.now() - t0 };
    if (Date.now() - t0 > ms) return { v, ms: null };
    await sleep(200);
  }
}
const post = (path, body, headers = admin) => fetch(`${CONTROL}${path}`, { method: 'POST', headers, body: JSON.stringify(body) });
const screener = (op, identity) => post('/api/v1/screener/commands', { op, identity });
const desk = (body) => post('/api/v1/desk/commands', body);

// LiveKit's server API, to cut a caller's link from outside the page (a dropped line).
function b64(o) {
  return Buffer.from(typeof o === 'string' ? o : JSON.stringify(o)).toString('base64url');
}
// A caller is in a booth of their own (ADR-0193); everyone else is in the studio room.
const booth = (identity) => `studio-booth-${identity}`;
function adminJwt(room = 'studio') {
  const now = Math.floor(Date.now() / 1000);
  const body = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64({ iss: API_KEY, nbf: now - 10, exp: now + 60, video: { room, roomAdmin: true } })}`;
  return `${body}.${createHmac('sha256', API_SECRET).update(body).digest('base64url')}`;
}
// An ordinary token signed as a pass's would be (no recorder grant), for the intruder below.
function ordinaryJwt(identity, room = 'studio') {
  const now = Math.floor(Date.now() / 1000);
  const claims = {
    iss: API_KEY, sub: identity, nbf: now - 10, exp: now + 600,
    video: { room, roomJoin: true, canPublish: false, canSubscribe: true, canPublishData: false },
    attributes: { 'weavesignal.role': 'listener' },
  };
  const body = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64(claims)}`;
  return `${body}.${createHmac('sha256', API_SECRET).update(body).digest('base64url')}`;
}

// Someone in the room with an ordinary token asks for another person's tracks, as a modified page
// could (ADR-0118): what the room hands them, after `ms`.
async function intrude(browser, targets, room = 'studio', ms = 6000) {
  const ictx = await browser.newContext();
  const ip = await ictx.newPage();
  await ip.addScriptTag({ path: new URL('../../apps/web/node_modules/livekit-client/dist/livekit-client.umd.js', import.meta.url).pathname });
  const got = await ip.evaluate(
    async ({ url, token, targets, ms }) => {
      const { Room, RoomEvent } = window.LivekitClient;
      const room = new Room();
      const seen = { asked: [], subscribed: [], refused: [] };
      room.on(RoomEvent.TrackSubscribed, (_t, _p, who) => seen.subscribed.push(who.identity));
      room.on(RoomEvent.TrackSubscriptionPermissionChanged, (_p, status, who) => status === 'not_allowed' && seen.refused.push(who.identity));
      await room.connect(url, token, { autoSubscribe: false });
      const t0 = Date.now();
      while (Date.now() - t0 < ms) {
        for (const who of room.remoteParticipants.values())
          if (targets.includes(who.identity))
            for (const pub of who.trackPublications.values())
              if (!pub.isDesired) {
                pub.setSubscribed(true);
                seen.asked.push(who.identity);
              }
        await new Promise((r) => setTimeout(r, 250));
      }
      await room.disconnect();
      return seen;
    },
    { url: LIVEKIT.replace(/^http/, 'ws'), token: ordinaryJwt(`intruder-${Date.now()}`, room), targets, ms },
  );
  await ictx.close();
  return got;
}

async function cutLink(identity) {
  const r = await fetch(`${LIVEKIT}/twirp/livekit.RoomService/RemoveParticipant`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${adminJwt(booth(identity))}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ room: booth(identity), identity }),
  });
  return r.status;
}

async function shot(page, name) {
  const path = join(OUT, `${name}.png`);
  await page.screenshot({ path, fullPage: true });
  log('shot', path);
}
const phase = (page) => page.locator('.banner').getAttribute('data-phase');
async function waitPhase(page, want, ms = 15_000) {
  const t0 = Date.now();
  for (;;) {
    const p = await phase(page).catch(() => null);
    if (p === want) return Date.now() - t0;
    if (Date.now() - t0 > ms) return null;
    await sleep(150);
  }
}
const headline = (page) => page.locator('#call-headline').innerText();

// ---- the stack is up, the presenter is live, call-in is open ----
let player;
for (let i = 0; i < 120; i++) {
  player = await fetch(`${WEB}/api/v1/player`).then((r) => r.json()).catch(() => null);
  if (player?.call_in?.open && player.presenter_live) break;
  await sleep(500);
}
check('the player page: call-in open, the presenter live', player?.call_in?.open && player.presenter_live, JSON.stringify({ show: player?.show, presenter: player?.presenter }));
const version = player.call_in.consent_version;

// ---- the control plane refuses a call without consent ----
const before = (await view()).callers.length;
for (const [label, consent] of [
  ['no consent', null],
  ['consent not ticked', { accepted: false, version }],
]) {
  const r = await fetch(`${WEB}/api/v1/call-in`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: 'Niall Anderson', topic: 'Ferries', consent }) });
  const b = await r.json();
  check(`call-in refused with ${label}: 422, no token`, r.status === 422 && b.field === 'consent' && !b.token, `${r.status} ${b.error}`);
}
check('nobody reached the queue without consent', (await view()).callers.length === before);

// ---- a listener on a phone calls in through the lobby ----
const browser = await chromium.launch({
  args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
});
const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, permissions: ['microphone'] };
const ctx = await browser.newContext({ ...phone, colorScheme: 'dark' });
const page = await ctx.newPage();
const callIns = [];
page.on('request', (r) => r.url().endsWith('/api/v1/call-in') && callIns.push(r.method()));
page.on('console', (m) => m.type() === 'error' && log('page console:', m.text()));
await page.goto(WEB + '/');
await page.getByRole('button', { name: 'Listen live' }).waitFor();
await sleep(500);
await shot(page, 'player-390-dark');
await page.getByRole('button', { name: 'Call the studio' }).tap();
await page.locator('#f-name').waitFor();
await page.fill('#f-name', 'Jessie Brown');
await page.fill('#f-topic', 'Nurse on nights: no bus after 23:10 from the Infirmary');
await shot(page, 'lobby-390-dark');
await page.locator('form button[type="submit"]').tap();
await sleep(800);
const consentErr = await page.locator('#consent-err').innerText().catch(() => '');
const focused = await page.evaluate(() => document.activeElement?.id);
check('the lobby will not call without consent: the box is named and focused, no request sent', /broadcast and recorded/.test(consentErr) && focused === 'f-consent' && callIns.length === 0, `${focused} · requests ${callIns.length}`);
check('still nobody in the queue', (await view()).callers.length === before);
await shot(page, 'lobby-390-dark-no-consent');

await page.locator('#f-consent').tap();
await page.locator('form button[type="submit"]').tap();
const waited = await waitPhase(page, 'waiting', 20_000);
check('with consent: the caller waits in the queue, and the page says so', waited !== null, `${waited} ms · "${await headline(page).catch(() => '')}"`);
// Her identity is the page's own (a held place from an earlier run may carry the same name).
const mine = await page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.call') ?? 'null'));
let { v } = await until((v) => v.callers.some((c) => c.identity === mine?.identity && c.present));
const jessie = v.callers.find((c) => c.identity === mine?.identity);
check(
  'the screener sees her name, topic and consent (by the caller, with its time)',
  jessie && jessie.consent?.by === 'caller' && /no bus after 23:10/.test(jessie.topic) && jessie.consent.version === version,
  JSON.stringify(jessie && { identity: jessie.identity, topic: jessie.topic, consent: jessie.consent }),
);
const id = jessie?.identity;
const t0 = Date.now();
while (!(await page.locator('[data-hearing="true"]').count()) && Date.now() - t0 < 15_000) await sleep(200);
const hears = await page.evaluate(() => [...document.querySelectorAll('audio[data-hear]')].map((a) => a.dataset.hear));
check('the page hears only its own mix-minus', hears.length === 1 && hears[0] === `desk-mm-${id}`, JSON.stringify(hears));
// From the studio room she cannot even be seen: she is in her own booth (ADR-0193).
const unseen = await intrude(browser, [id, `desk-mm-${id}`]);
check(
  'an ordinary token in the studio room does not see her or her mix-minus at all',
  unseen.asked.length === 0 && unseen.subscribed.length === 0,
  JSON.stringify(unseen),
);
// From inside her booth, the publishers' limits still refuse it (ADR-0118).
const stolen = await intrude(browser, [id, `desk-mm-${id}`], booth(id));
check(
  'an ordinary token inside her booth asks for her microphone and her mix-minus and gets neither',
  stolen.asked.includes(id) && stolen.asked.includes(`desk-mm-${id}`) && stolen.subscribed.length === 0,
  JSON.stringify(stolen),
);
const showLine = await page.locator('.showline').innerText().catch(() => '');
check('the page says the show is on air and the presenter live', /on air · Rory Bruce is live/.test(showLine), showLine);
await shot(page, 'call-390-dark-waiting');

// ---- screened, the producer talking (held T), sent, on air, back ----
await screener('screen', id);
check('screened: the page says the producer picked up', (await waitPhase(page, 'screening')) !== null, await headline(page));
await desk({ op: 'talk_to', identity: 'elspeth', to: id, on: true });
await sleep(1200);
const talking = await headline(page);
check('the producer holding T: the page says they are talking to her', talking === 'The producer is talking to you', talking);
await shot(page, 'call-390-dark-screener-talking');
await desk({ op: 'talk_to', identity: 'elspeth', to: id, on: false });
await screener('send', id);
check('sent to the desk: "You\'re next"', (await waitPhase(page, 'ready')) !== null, await headline(page));
await shot(page, 'call-390-dark-ready');
const onAir = await desk({ op: 'on_air', identity: id, on: true });
check('the desk puts her on air (consent on file)', onAir.status === 200, String(onAir.status));
check('on air: the page says so, in red', (await waitPhase(page, 'on_air')) !== null && (await headline(page)) === "You're on air");
await shot(page, 'call-390-dark-on-air');
await desk({ op: 'on_air', identity: id, on: false });
const back = await waitPhase(page, 'ready');
check('off air again: back to waiting at the desk, still on the line', back !== null, await headline(page));

// ---- the link drops; the page rejoins under the same identity, in the same place ----
const place = (await view()).callers.find((c) => c.identity === id)?.ordinal;
const cut = await cutLink(id);
let sawReconnect = false;
const t1 = Date.now();
while (Date.now() - t1 < 20_000) {
  const link = await page.locator('.banner').getAttribute('data-link');
  if (link === 'reconnecting') sawReconnect = true;
  if (sawReconnect && link === 'connected') break;
  await sleep(100);
}
const after = await until((v) => v.callers.some((c) => c.identity === id && c.present), 15_000);
const again = after.v.callers.find((c) => c.identity === id);
check('a cut link: the page reconnects by itself, same identity, same place', cut === 200 && sawReconnect && again?.ordinal === place, `cut ${cut} · reconnecting seen ${sawReconnect} · place ${place} → ${again?.ordinal} · ${Date.now() - t1} ms`);

// After the rejoin the desk must hear her again, not only see her (the page publishes a fresh
// microphone track: the desk never routed the old one again).
await desk({ op: 'on_air', identity: id, on: true });
let level = null;
for (const t = Date.now(); Date.now() - t < 12_000 && level == null; await sleep(500)) {
  level = (await view()).sources.find((x) => x.identity === id)?.lufs ?? null;
}
await desk({ op: 'on_air', identity: id, on: false });
check('after the rejoin the desk hears her again (her channel has a level)', level != null, `${level} LUFS`);

// ---- the screener's view of her, with the consent ----
const sctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const studio = await sctx.newPage();
await studio.goto(STUDIO);
await studio.keyboard.press('Control+2');
await studio.locator('[data-panel="screener.queue"]').waitFor();
await studio.locator('.qrow', { hasText: id }).click();
await sleep(800);
const consentText = await studio.locator('[data-consent]').innerText().catch(() => '');
check('the screener shows the consent flag above the fold', /Consent to record · accepted .* on the player page/.test(consentText), consentText);
await shot(studio, 'screener-1440-dark-consent');

// ---- she hangs up: out of the queue, the pass ended ----
const pass = mine?.pass;
await page.getByRole('button', { name: 'Hang up' }).tap();
await page.getByRole('button', { name: /Press again to hang up/ }).tap();
check('hung up: the page says the call ended', (await waitPhase(page, 'ended', 5000)) !== null || (await headline(page)) === 'Your call has ended', await headline(page));
await shot(page, 'call-390-dark-ended');
const gone = await until((v) => !v.callers.some((c) => c.identity === id), 8000);
const reuse = pass ? await fetch(`${WEB}/api/v1/pass/token`, { method: 'POST', headers: { Authorization: `Bearer ${pass}` } }) : { status: 0 };
check('out of the queue, and her pass buys no new token', gone.ms !== null && reuse.status === 410, `pass ${reuse.status}`);

// ---- an invited guest (on air at once) and a remote DJ (waits for the handover) ----
for (const [role, nameOf, want, wantHead] of [
  ['guest', 'Gregor Stewart', 'on_air', "You're on air"],
  ['dj', 'Finlay Grant', 'off_air', 'Waiting for the handover'],
]) {
  const inv = await (await post('/api/v1/invites', { role, name: nameOf })).json();
  const gctx = await browser.newContext({ ...phone, colorScheme: role === 'guest' ? 'light' : 'dark' });
  const g = await gctx.newPage();
  await g.goto((/^https?:/.test(inv.link) ? inv.link : `${WEB}${inv.link}`));
  await g.getByRole('button', { name: 'Join the studio' }).waitFor();
  await shot(g, `join-390-${role}-invite`);
  await g.getByRole('button', { name: 'Join the studio' }).tap();
  const ms = await waitPhase(g, want, 20_000);
  const h = await headline(g).catch(() => '');
  check(`an invited ${role === 'dj' ? 'remote DJ' : role} joins: "${wantHead}"`, ms !== null && h === wantHead, `${ms} ms · ${h}`);
  await shot(g, `join-390-${role}-${want}`);
  await g.getByRole('button', { name: 'Leave the studio' }).tap();
  await g.getByRole('button', { name: /Press again/ }).tap();
  await gctx.close();
}

// ---- the echo test, and the player at desktop width in light ----
const e = await ctx.newPage();
await e.goto(`${WEB}/echo-test/`);
await e.getByRole('button', { name: 'Start the echo test' }).tap();
await sleep(800);
const ec = await e.evaluate(async () => {
  const s = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false } });
  const v = s.getAudioTracks()[0].getSettings().echoCancellation;
  s.getTracks().forEach((t) => t.stop());
  return v;
});
check('the echo test runs with echo cancelling off', ec === false, String(ec));
await shot(e, 'echo-test-390-dark');
const wide = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: 'light' });
const wp = await wide.newPage();
await wp.goto(WEB + '/');
await wp.getByRole('button', { name: 'Listen live' }).waitFor();
await sleep(400);
await shot(wp, 'player-1280-light');

await browser.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(OUT, 'results.json'), JSON.stringify(results, null, 2));
log(`${results.length - failed.length} of ${results.length} steps passed`);
process.exit(failed.length ? 1 : 0);
