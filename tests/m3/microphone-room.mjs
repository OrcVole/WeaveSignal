// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A change of microphone during a call, against the real room and the real desk (1.3.3): the M3
// loopback stack (tests/m3/run-join.sh), a guest's invitation opened in headless Chromium with
// its three fake microphones, and the join page as the station serves it, with the real
// livekit-client. It proves what tests/m3/microphone.mjs cannot:
//
// - a change keeps the one publication: the same track id in the room (LiveKit's own list), the
//   same RTP stream from the browser (its SSRC), bytes still rising, and the desk neither drops
//   nor makes its subscription again (its log), and goes on hearing the person (their channel's
//   level in the studio's view);
// - the new capture keeps the join pages' rules (echo cancelling and noise suppression on, gain
//   control off), as asked and as the browser reports it;
// - the room client leaves a chosen microphone alone when the browser's list changes;
// - when the microphone in use ends, the room client's own restart keeps the rules too, and the
//   page says which microphone it is now on;
// - after the room removes the person and the page rejoins under the same identity, the desk
//   hears them again on the microphone they chose (the room client stops local tracks when the
//   server ends a connection, so this rejoin captures afresh: it must ask for the choice, and
//   exactly, because Chromium gives its default microphone for an id that is only preferred).
//
// What it does not prove: a real microphone unplugged (the fake ones cannot be: the capture is
// ended from the page and `ended` fired), Firefox, or a phone.
//
//   M3_PREFIX=weavesignal-mic tests/m3/run-join.sh          # then, in another shell:
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core DESK_CONTAINER=weavesignal-mic-desk node tests/m3/microphone-room.mjs
import { createRequire } from 'node:module';
import { createHmac } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const WEB = process.env.WEB ?? 'http://127.0.0.1:32430';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:32310';
const LIVEKIT = process.env.LIVEKIT ?? 'http://127.0.0.1:32880';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm3-admin-test-only';
const API_KEY = process.env.API_KEY ?? 'm3join';
const API_SECRET = process.env.API_SECRET ?? 'm3-join-test-only-secret-not-for-use-00';
const DESK_CONTAINER = process.env.DESK_CONTAINER ?? `${process.env.M3_PREFIX ?? 'weavesignal-m3'}-desk`;
const OUT = process.env.OUT ?? new URL('../../target/shots-microphone-room', import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}

const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
const view = async () => (await fetch(`${CONTROL}/api/v1/studio`, { headers: admin })).json();
const post = (path, body) => fetch(`${CONTROL}${path}`, { method: 'POST', headers: admin, body: JSON.stringify(body) });
const b64 = (o) => Buffer.from(typeof o === 'string' ? o : JSON.stringify(o)).toString('base64url');
function adminJwt(room = 'studio') {
  const now = Math.floor(Date.now() / 1000);
  const body = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64({ iss: API_KEY, nbf: now - 10, exp: now + 60, video: { room, roomAdmin: true, roomList: true } })}`;
  return `${body}.${createHmac('sha256', API_SECRET).update(body).digest('base64url')}`;
}
const twirp = async (method, body) => (await fetch(`${LIVEKIT}/twirp/livekit.RoomService/${method}`, { method: 'POST', headers: { Authorization: `Bearer ${adminJwt()}`, 'Content-Type': 'application/json' }, body: JSON.stringify(body) })).json();
/** The person's microphone track in the room, as LiveKit lists it. */
async function published(identity) {
  const r = await twirp('ListParticipants', { room: 'studio' });
  const who = (r.participants ?? []).find((p) => p.identity === identity);
  const t = (who?.tracks ?? []).find((x) => x.type === 'AUDIO' || x.type === 0 || x.source === 'MICROPHONE');
  return t ? { sid: t.sid, muted: !!t.muted } : null;
}
/** The desk's level for the person, read afresh: null is silence, or nobody there. */
async function heard(identity, ms = 12_000) {
  let lufs = null;
  for (const t0 = Date.now(); Date.now() - t0 < ms && lufs == null; await sleep(500)) lufs = (await view()).sources.find((x) => x.identity === identity)?.lufs ?? null;
  return lufs;
}
/** What the desk's log says of the person's subscription (the desk logs to stderr). */
function deskLines(identity) {
  const r = spawnSync('podman', ['logs', DESK_CONTAINER], { encoding: 'utf8', maxBuffer: 256 << 20 });
  const text = `${r.stdout ?? ''}${r.stderr ?? ''}`;
  const mine = text.split('\n').filter((l) => l.includes(identity));
  return { subscribing: mine.filter((l) => /subscribing/.test(l)).length, dropped: mine.filter((l) => /subscription dropped/.test(l)).length, errors: mine.filter((l) => /ERROR|not-linked/.test(l)).length, known: mine.length > 0 };
}

/** Before any page script: every capture is recorded as asked, the page's peer connections are
 *  kept (a subclass, so the page's own ICE settings stay: AGENTS.md), and the driver can put a
 *  name at the top of the browser's list and end a capture as an unplugged microphone ends. */
function hooks() {
  const md = navigator.mediaDevices;
  const realList = md.enumerateDevices.bind(md);
  const realGet = md.getUserMedia.bind(md);
  const t = (window.__t = { captures: [], pcs: [], top: null });
  md.enumerateDevices = async () => {
    const list = await realList();
    return t.top ? [{ kind: 'audioinput', deviceId: t.top.id, groupId: 'g-top', label: t.top.label, toJSON() {} }, ...list] : list;
  };
  md.getUserMedia = async (c) => {
    const stream = await realGet(c);
    for (const track of stream.getAudioTracks()) t.captures.push({ asked: JSON.parse(JSON.stringify(c.audio)), track });
    return stream;
  };
  const Real = window.RTCPeerConnection;
  window.RTCPeerConnection = class extends Real {
    constructor(...a) {
      super(...a);
      t.pcs.push(this);
    }
  };
  t.changed = () => md.dispatchEvent(new Event('devicechange'));
  t.end = (label) => {
    for (const c of t.captures) {
      if (c.track.label === label && c.track.readyState === 'live') {
        c.track.stop();
        c.track.dispatchEvent(new Event('ended'));
      }
    }
  };
}
/** What the browser is sending as the microphone: the sender's track and its RTP stream. */
const sending = (page) =>
  page.evaluate(async () => {
    for (const pc of window.__t.pcs) {
      if (pc.connectionState === 'closed') continue;
      const sender = pc.getSenders().find((s) => s.track?.kind === 'audio');
      if (!sender) continue;
      const s = sender.track.getSettings();
      let rtp = null;
      for (const r of (await sender.getStats()).values()) if (r.type === 'outbound-rtp') rtp = { ssrc: r.ssrc, bytes: r.bytesSent };
      return { label: sender.track.label, live: sender.track.readyState === 'live', ec: s.echoCancellation, ns: s.noiseSuppression, agc: s.autoGainControl, ...rtp };
    }
    return null;
  });
const captures = (page) => page.evaluate(() => window.__t.captures.map((c) => ({ asked: c.asked, label: c.track.label, live: c.track.readyState === 'live' })));
const rules = (x) => x.ec === true && x.ns === true && x.agc === false;
const askedRules = (a) => a?.echoCancellation === true && a?.noiseSuppression === true && a?.autoGainControl === false;
const note = (page) => page.locator('.io [data-mic-note]').innerText();
const selected = (page) => page.locator('.io [data-panel="join.microphone"] select').evaluate((el) => el.selectedOptions[0]?.textContent ?? '');
/** Bytes go on leaving: two readings a second and a half apart. */
async function flowing(page) {
  const a = await sending(page);
  await sleep(1500);
  const b = await sending(page);
  return { ok: !!a && !!b && b.bytes > a.bytes && a.ssrc === b.ssrc, a, b };
}

const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM, args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'] });
try {
  await post('/api/v1/setup/finish', {});
  const inv = await (await post('/api/v1/invites', { role: 'guest', name: 'Gregor Stewart' })).json();
  const id = inv.identity;
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, permissions: ['microphone'], locale: 'en-GB', colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.on('console', (m) => m.type() === 'error' && log('page error:', m.text().slice(0, 300)));
  await page.addInitScript(hooks);
  await page.goto(/^https?:/.test(inv.link) ? inv.link : `${WEB}${inv.link}`);
  await page.getByRole('button', { name: 'Join the studio' }).tap();
  await page.locator('.banner[data-link="connected"][data-phase="off_air"]').waitFor({ timeout: 30_000 });
  await page.locator('.io [data-panel="join.microphone"] select').waitFor({ timeout: 10_000 });
  const names = await page.locator('.io select option').allTextContents();
  const [FIRST, , THIRD] = names;
  check('joined: the call names the microphone in use and lists the browser’s three', names.length === 3 && (await selected(page)) === FIRST, JSON.stringify(names));

  // ---- as joined ----
  const lufs0 = await heard(id);
  const pub0 = await published(id);
  const send0 = await sending(page);
  const desk0 = deskLines(id);
  check('before the change: the desk hears the guest, on the browser’s own microphone with the rules', lufs0 != null && send0?.label === FIRST && rules(send0) && !!pub0?.sid, `${lufs0} LUFS · ${JSON.stringify(send0)} · ${JSON.stringify(pub0)} · desk ${JSON.stringify(desk0)}`);

  // ---- the change ----
  await page.locator('.io select').selectOption({ label: THIRD });
  await page.getByRole('button', { name: `Use ${THIRD}` }).tap();
  await page.locator('.io [data-mic-note]').waitFor({ timeout: 10_000 });
  const said = await note(page);
  const send1 = await sending(page);
  const caps1 = await captures(page);
  check(`the page says: "Now using ${THIRD}. The studio hears you through it."`, said === `Now using ${THIRD}. The studio hears you through it.` && (await selected(page)) === THIRD, said);
  check('the browser now sends the chosen microphone, captured exactly and with the rules: echo cancelling and noise suppression on, gain control off', send1?.label === THIRD && send1.live && rules(send1) && typeof caps1.at(-1).asked.deviceId?.exact === 'string' && askedRules(caps1.at(-1).asked), `${JSON.stringify(send1)} · asked ${JSON.stringify(caps1.at(-1).asked)}`);
  check('the old capture is let go: one microphone open', caps1.filter((c) => c.live).length === 1, JSON.stringify(caps1.map((c) => [c.label, c.live])));
  const flow1 = await flowing(page);
  check('the same RTP stream carries on (one SSRC before and after), and bytes go on leaving', flow1.ok && send0.ssrc === send1.ssrc, `ssrc ${send0.ssrc} → ${send1.ssrc} · bytes ${flow1.a?.bytes} → ${flow1.b?.bytes}`);
  const pub1 = await published(id);
  check('the room lists the same published track: nothing was unpublished', !!pub1 && pub1.sid === pub0.sid && !pub1.muted, `${pub0.sid} → ${pub1?.sid}`);
  await sleep(3500); // past the desk's 3 s meter window, so the level read is the new microphone's
  const lufs1 = await heard(id, 4000);
  const desk1 = deskLines(id);
  check('the desk goes on hearing the guest, and neither dropped nor made its subscription again', lufs1 != null && desk1.known && desk1.subscribing === desk0.subscribing && desk1.dropped === desk0.dropped && desk1.errors === desk0.errors, `${lufs1} LUFS · desk ${JSON.stringify(desk0)} → ${JSON.stringify(desk1)}`);
  await page.screenshot({ path: join(OUT, 'changed.png'), fullPage: true });

  // ---- the browser's list changes: the room client leaves a chosen microphone alone ----
  const n = caps1.length;
  for (const top of [{ id: 'top-1', label: 'A webcam' }, { id: 'top-2', label: 'A second webcam' }, null]) {
    await page.evaluate((v) => ((window.__t.top = v), window.__t.changed()), top);
    await sleep(1200);
  }
  const send2 = await sending(page);
  check('a new device at the top of the browser’s list, twice, moves nothing: no capture, the same microphone, the same stream', (await captures(page)).length === n && send2?.label === THIRD && send2.ssrc === send1.ssrc && (await selected(page)) === THIRD, JSON.stringify(send2));

  // ---- a rejoin under the same identity (the trap of M3) keeps the chosen microphone ----
  await twirp('RemoveParticipant', { room: 'studio', identity: id });
  await page.locator('.banner[data-link="reconnecting"]').waitFor({ timeout: 10_000 });
  await page.locator('.banner[data-link="connected"]').waitFor({ timeout: 40_000 });
  await sleep(3500);
  const lufs3 = await heard(id, 15_000);
  const send3 = await sending(page);
  const caps3 = await captures(page);
  const flow3 = await flowing(page);
  const again = caps3.slice(n);
  check(
    'after a rejoin the desk hears the guest again, on the microphone they chose, with the rules, and one microphone open',
    lufs3 != null && send3?.label === THIRD && rules(send3) && flow3.ok && (await selected(page)) === THIRD && caps3.filter((c) => c.live).length === 1 && again.every((c) => askedRules(c.asked) && typeof c.asked.deviceId?.exact === 'string' && c.label === THIRD),
    `${lufs3} LUFS · ${JSON.stringify(send3)} · ${again.length} fresh capture(s) ${JSON.stringify(again.map((c) => c.asked.deviceId))}`,
  );

  // ---- the microphone in use ends: the room client's own restart, and what the page says ----
  const pubA = await published(id);
  await page.evaluate((l) => window.__t.end(l), THIRD);
  await page.waitForFunction((l) => document.querySelector('.io [data-mic-note]')?.textContent?.startsWith(`${l} stopped`), THIRD, { timeout: 15_000 });
  const moved = await note(page);
  const send4 = await sending(page);
  const caps4 = await captures(page);
  check(`the page says which microphone it is on now: "${moved}"`, moved === `${THIRD} stopped, so this page is now using ${send4?.label}.` && (await selected(page)) === send4?.label, moved);
  check('the restart keeps the rules: echo cancelling and noise suppression on, gain control off', !!send4 && send4.live && rules(send4) && askedRules(caps4.at(-1).asked), `${JSON.stringify(send4)} · asked ${JSON.stringify(caps4.at(-1).asked)}`);
  await sleep(3500);
  const lufs4 = await heard(id, 6000);
  const pubB = await published(id);
  const flow4 = await flowing(page);
  check('and the desk hears the guest on it, on the same published track', lufs4 != null && pubB?.sid === pubA?.sid && !pubB.muted && flow4.ok, `${lufs4} LUFS · ${pubA?.sid} → ${pubB?.sid}`);
  const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.mic') ?? 'null'));
  check('the choice stays remembered for the next visit', kept?.label === THIRD, JSON.stringify(kept));
  await page.screenshot({ path: join(OUT, 'restarted.png'), fullPage: true });

  await page.getByRole('button', { name: 'Leave the studio' }).tap();
  await page.getByRole('button', { name: /Press again/ }).tap();
  await page.locator('.banner[data-link="ended"]').waitFor({ timeout: 10_000 });
  check('leaving lets the microphone go', (await captures(page)).every((c) => !c.live));
  await ctx.close();
} finally {
  await browser.close();
}

const failed = results.filter((r) => !r.ok);
log(`${results.length - failed.length} of ${results.length} checks passed; screenshots in ${OUT}`);
if (failed.length) {
  for (const f of failed) log('FAILED', f.name, f.detail);
  process.exit(1);
}
