// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A caller on a phone whose browser blocks WebRTC's UDP, as GrapheneOS's Vanadium does (patch
// 0102: `disable_non_proxied_udp`), calls in through LiveKit's TURN over TLS and is heard by
// listeners. Runs against tests/m3/run-join.sh with TURN_TLS_PORT and SERVE_WEB set, so the
// pages come from the control plane with their real caching and Content-Security-Policy.
//
// 1. The player page, the lobby and the call at 390 px with touch (taps, not clicks), dark.
// 2. The challenge's cost, read from the page (and again with the CPU slowed 6x, for a phone).
// 3. The call: no direct path (UDP blocked, ICE over TCP off), so the page falls back to the
//    relay alone (`iceTransportPolicy: 'relay'`); getStats must show the selected pair's local
//    candidate as a relay reached over TLS.
// 4. Heard: the caller's fake microphone is a 1 kHz tone; the listener stream (after the
//    profanity delay) has 1 kHz in it while she is on air and not before.
// 5. The negative control: the same browser with TURN taken out of the page's ICE servers
//    cannot connect at all, so (3) is the relay and only the relay.
// 6. No CSP violation on any page.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core OUT=/some/dir node tests/m3/relay.mjs
import { createRequire } from 'node:module';
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:39310';
const WEB = process.env.WEB ?? CONTROL;
const STREAM = process.env.STREAM ?? 'http://127.0.0.1:39812/programme.opus';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm3-admin-test-only';
const RUN = process.env.RUN ?? new URL('./run/', import.meta.url).pathname;
const OUT = process.env.OUT ?? '/tmp/weavesignal-m3-relay';
const DELAY_MS = Number(process.env.DELAY_MS ?? 15_000);
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const evidence = {};
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail);
}
const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
const view = async () => (await fetch(`${CONTROL}/api/v1/studio`, { headers: admin })).json();
const post = (path, body) => fetch(`${CONTROL}${path}`, { method: 'POST', headers: admin, body: JSON.stringify(body) });

// ---- the served pages' headers ----
const page0 = await fetch(`${WEB}/`);
const csp = page0.headers.get('content-security-policy') ?? '';
const html = await page0.text();
const asset = html.match(/src="(\/assets\/player-[^"]+\.js)"/)?.[1];
const a0 = asset ? await fetch(`${WEB}${asset}`, { headers: { 'Accept-Encoding': 'gzip' } }) : null;
check(
  'the control plane serves the player: page no-cache, hashed assets immutable, gzip, a CSP without inline script',
  page0.headers.get('cache-control') === 'no-cache' &&
    a0?.headers.get('cache-control') === 'public, max-age=31536000, immutable' &&
    /script-src 'self';/.test(csp) &&
    !/unsafe-eval/.test(csp),
  `${csp.slice(0, 90)}… · ${asset}`,
);
evidence.csp = csp;

// ---- the caller's microphone: a warbling tone around 1 kHz, in syllables, 20 s, looped ----
// Not a steady sine: the page's noise suppression (on, as for every caller) learns a steady
// tone as noise and takes it out. This one moves like a voice: its pitch swings 900-1100 Hz
// and it comes in bursts four times a second.
const tone = join(OUT, 'warble-1k.wav');
execFileSync('ffmpeg', ['-v', 'error', '-y', '-f', 'lavfi', '-i', "aevalsrc=0.5*sin(2*PI*1000*t+33*sin(2*PI*3*t))*gt(sin(2*PI*4*t)\\,-0.2):s=48000:d=20", '-ac', '1', tone]);
const spki = readFileSync(join(RUN, 'turn-spki.txt'), 'utf8').trim();
const browser = await chromium.launch({
  args: [
    '--use-fake-device-for-media-stream',
    '--use-fake-ui-for-media-stream',
    `--use-file-for-fake-audio-capture=${tone}`,
    '--autoplay-policy=no-user-gesture-required',
    // What Vanadium does by default: no WebRTC UDP that is not proxied.
    '--force-webrtc-ip-handling-policy=disable_non_proxied_udp',
    // turn.localhost over IPv4: reached over ::1, LiveKit's TURN (pion/turn 5.0.13) refuses the
    // allocation (508) when its relay address is IPv4 (node_ip). A station's TURN name wants
    // an A record, or IPv6 tested end to end.
    '--host-resolver-rules=MAP turn.localhost 127.0.0.1',
    // The test TURN's self-signed certificate, trusted by its key alone.
    `--ignore-certificate-errors-spki-list=${spki}`,
  ],
});
const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true, permissions: ['microphone'], colorScheme: 'dark' };

// Every page records CSP violations and its peer connections, for getStats.
const watch = `
  window.__csp = [];
  document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI));
  window.__pcs = [];
  const Native = window.RTCPeerConnection;
  const strip = (c) => (window.__noTurn && c ? { ...c, iceServers: [] } : c);
  window.RTCPeerConnection = class extends Native {
    constructor(config, ...rest) {
      super(strip(config), ...rest);
      this.__policy = config?.iceTransportPolicy ?? 'all';
      window.__pcs.push(this);
    }
    setConfiguration(c) {
      return super.setConfiguration(strip(c));
    }
  };
`;

async function lobby(ctx, name, topic, shots) {
  const page = await ctx.newPage();
  page.on('console', (m) => m.type() === 'error' && log('page console:', m.text()));
  await page.goto(WEB + '/');
  await page.getByRole('button', { name: 'Listen live' }).waitFor();
  if (shots) await page.screenshot({ path: join(OUT, 'relay-player-390.png'), fullPage: true });
  await page.getByRole('button', { name: 'Call the studio' }).tap();
  await page.locator('#f-name').waitFor();
  await page.locator('#f-name').tap();
  await page.keyboard.type(name);
  await page.locator('#f-topic').tap();
  await page.keyboard.type(topic);
  await page.locator('#f-consent').tap();
  return page;
}

async function selectedPair(page) {
  return page.evaluate(async () => {
    const out = [];
    for (const pc of window.__pcs) {
      if (pc.connectionState === 'closed') continue;
      const stats = await pc.getStats();
      let pairId = null;
      stats.forEach((s) => s.type === 'transport' && s.selectedCandidatePairId && (pairId = s.selectedCandidatePairId));
      const pair = pairId ? stats.get(pairId) : [...stats.values()].find((s) => s.type === 'candidate-pair' && s.nominated && s.state === 'succeeded');
      if (!pair) continue;
      const local = stats.get(pair.localCandidateId);
      const remote = stats.get(pair.remoteCandidateId);
      out.push({ policy: pc.__policy, state: pc.connectionState, local: { type: local?.candidateType, relayProtocol: local?.relayProtocol, protocol: local?.protocol, url: local?.url }, remote: { type: remote?.candidateType, protocol: remote?.protocol, port: remote?.port } });
    }
    return out;
  });
}

// ---- 1-2: the phone's lobby, and the challenge's cost ----
const ctx = await browser.newContext(phone);
await ctx.addInitScript(watch);
const page = await lobby(ctx, 'Jessie Brown', 'Calling from a phone that blocks UDP', true);
let ch = null;
for (let i = 0; i < 100 && !ch; i++) {
  ch = await page.evaluate(() => window.__weavesignalChallenge ?? null);
  if (!ch) await sleep(100);
}
check('the challenge is answered in the background while the caller types', ch && ch.answer?.nonce, ch && `${ch.bits} bits · ${ch.tried} hashes in ${Math.round(ch.ms)} ms · ${Math.round((ch.tried / ch.ms) * 1000)} hashes/s`);
evidence.challenge = ch && { bits: ch.bits, tried: ch.tried, ms: Math.round(ch.ms), rate: Math.round((ch.tried / ch.ms) * 1000) };
await page.screenshot({ path: join(OUT, 'relay-lobby-390.png'), fullPage: true });

// ---- 3: the call, UDP blocked: only TURN over TLS gets through, then the relay alone ----
const t0 = Date.now();
await page.locator('form button[type="submit"]').tap();
let link = null;
for (; Date.now() - t0 < 60_000; await sleep(250)) {
  link = await page.locator('.banner').getAttribute('data-link').catch(() => null);
  if (link === 'connected' || link === 'lost') break;
}
const joinMs = Date.now() - t0;
const relayed = (ps) => ps.length > 0 && ps.every((p) => p.local.type === 'relay' && p.local.relayProtocol === 'tls');
const first = await selectedPair(page);
check(
  "UDP blocked: the caller's first join gets through, and only by TURN over TLS (selected pair: a local relay candidate over TLS)",
  link === 'connected' && relayed(first),
  `${link} after ${joinMs} ms · ${JSON.stringify(first)}`,
);
const mine = await page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.call') ?? 'null'));
// The page's own relay option ("Trouble hearing or being heard?"): the relay alone.
await page.locator('details.trouble > summary', { hasText: 'Trouble hearing' }).tap();
await page.getByRole('button', { name: 'Reconnect through the relay' }).tap();
const t1 = Date.now();
await sleep(500);
for (; Date.now() - t1 < 45_000; await sleep(250)) {
  link = await page.locator('.banner').getAttribute('data-link').catch(() => null);
  if (link === 'connected' || link === 'lost') break;
}
const forced = (await selectedPair(page)).filter((p) => p.policy === 'relay');
const viaRelay = await page.getByText(/connected through the station.s relay/i).count();
check(
  'forced to the relay (iceTransportPolicy: relay): the caller rejoins, selected pair a relay over TLS, and the page says so',
  link === 'connected' && relayed(forced) && viaRelay > 0,
  `${link} after ${Date.now() - t1} ms · ${JSON.stringify(forced)}`,
);
evidence.join = { first_ms: joinMs, first, forced_ms: Date.now() - t1, forced };
await page.screenshot({ path: join(OUT, 'relay-call-390-waiting.png'), fullPage: true });

// ---- 4: heard by listeners ----
// The listener stream for `seconds`, as 48 kHz mono samples; again if it came back short (a
// listener connecting while the listener process is between sources gets nothing at first).
async function record(seconds) {
  for (let i = 0; i < 3; i++) {
    const pcm = await recordOnce(seconds);
    if (pcm.length >= seconds * 48000) return pcm;
    await sleep(1000);
  }
  throw new Error('the listener stream gave no audio');
}
function recordOnce(seconds) {
  return new Promise((resolve) => {
    const ff = spawn('ffmpeg', ['-v', 'error', '-t', String(seconds), '-i', STREAM, '-f', 's16le', '-ac', '1', '-ar', '48000', '-']);
    const chunks = [];
    ff.stdout.on('data', (c) => chunks.push(c));
    ff.on('close', () => resolve(Buffer.concat(chunks)));
  });
}
function goertzel(pcm, freq) {
  const n = pcm.length >> 1;
  const k = (2 * Math.PI * freq) / 48000;
  const c = 2 * Math.cos(k);
  let s1 = 0, s2 = 0;
  for (let i = 0; i < n; i++) {
    const s = pcm.readInt16LE(i * 2) / 32768 + c * s1 - s2;
    s2 = s1;
    s1 = s;
  }
  return Math.sqrt(s1 * s1 + s2 * s2 - c * s1 * s2) / n;
}
// The band the warble sweeps (900-1100 Hz) against bands it never reaches.
const band = (pcm, fs) => fs.reduce((a, f) => a + goertzel(pcm, f), 0) / fs.length;
const tonality = (pcm) => {
  const at = band(pcm, [920, 960, 1000, 1040, 1080]);
  const near = band(pcm, [600, 700, 1400, 1600, 1800]);
  return { band: at, outside: near, ratio: at / Math.max(near, 1e-9) };
};
const id = mine?.identity;
await until((v) => v.callers.some((c) => c.identity === id && c.present));
const before = tonality(await record(5));
await post('/api/v1/screener/commands', { op: 'send', identity: id });
await sleep(800);
const onAir = await post('/api/v1/desk/commands', { op: 'on_air', identity: id, on: true });
// The desk's own meter on her channel, while the profanity delay holds the stream back.
const levels = [];
for (const t = Date.now(); Date.now() - t < DELAY_MS + 2000; await sleep(1000)) {
  const src = (await view()).sources.find((x) => x.identity === id);
  levels.push(src ? Math.round(src.lufs ?? -99) : null);
}
const during = tonality(await record(6));
await post('/api/v1/desk/commands', { op: 'on_air', identity: id, on: false });
check(
  "heard: the caller's warble is in the listener stream while she is on air, and not before",
  onAir.status === 200 && during.ratio > 3 && during.ratio > 3 * before.ratio,
  `on air ${onAir.status} · her channel ${JSON.stringify(levels.slice(-5))} LUFS · 900-1100 Hz against the bands beside it: before ${before.ratio.toFixed(1)}, on air ${during.ratio.toFixed(1)}`,
);
evidence.heard = { before, during, levels };
await page.screenshot({ path: join(OUT, 'relay-call-390-heard.png'), fullPage: true });
const csp1 = await page.evaluate(() => window.__csp);
await page.getByRole('button', { name: 'Hang up' }).tap();
await page.getByRole('button', { name: /Press again to hang up/ }).tap();
await sleep(500);

async function until(pred, ms = 15_000) {
  const t = Date.now();
  for (;;) {
    const v = await view();
    if (pred(v)) return v;
    if (Date.now() - t > ms) return v;
    await sleep(200);
  }
}

// ---- 2b: the challenge with the CPU slowed six times (a mid-range phone, roughly) ----
// CDP slows the page's main thread only, so this page has no Worker: the challenge runs in the
// page's own fallback (short slices on the main thread), which the slowdown then applies to.
const sctx = await browser.newContext(phone);
await sctx.addInitScript(watch + '\ndelete window.Worker;');
const slow = await sctx.newPage();
const cdp = await ctx.newCDPSession(slow);
await cdp.send('Emulation.setCPUThrottlingRate', { rate: 6 });
const costs = [];
await slow.goto(WEB + '/');
await slow.getByRole('button', { name: 'Listen live' }).waitFor();
for (let i = 0; i < 5; i++) {
  // The page's own path: open the lobby, which starts the challenge, read its cost, go back.
  await slow.getByRole('button', { name: 'Call the studio' }).tap();
  let c = null;
  for (let j = 0; j < 300 && !c; j++) {
    c = await slow.evaluate(() => window.__weavesignalChallenge ?? null);
    if (!c) await sleep(100);
  }
  if (c) costs.push({ tried: c.tried, ms: Math.round(c.ms) });
  await slow.evaluate(() => delete window.__weavesignalChallenge);
  await slow.getByRole('button', { name: 'Not now' }).tap();
}
const rate6 = costs.reduce((a, c) => a + c.tried, 0) / Math.max(1, costs.reduce((a, c) => a + c.ms, 0)) * 1000;
check('the challenge with the CPU slowed 6x: answered every time', costs.length === 5, `${JSON.stringify(costs)} · ${Math.round(rate6)} hashes/s`);
evidence.challengeSlow = { costs, rate: Math.round(rate6) };
await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });

// ---- 5: the negative control: no TURN in the page's ICE servers, so nothing gets through ----
const nctx = await browser.newContext(phone);
await nctx.addInitScript('window.__noTurn = true;\n' + watch);
const neg = await lobby(nctx, 'Niall Anderson', 'The control: no relay', false);
await neg.locator('form button[type="submit"]').tap();
const tn = Date.now();
let nlink = null;
for (; Date.now() - tn < 75_000; await sleep(500)) {
  nlink = await neg.locator('.banner').getAttribute('data-link').catch(() => null);
  if (nlink === 'connected' || nlink === 'lost') break;
}
check('the control: without TURN the same browser cannot connect at all (UDP really is blocked)', nlink === 'lost', `${nlink} after ${Date.now() - tn} ms`);
await neg.screenshot({ path: join(OUT, 'relay-control-390-lost.png'), fullPage: true });
const nme = await neg.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.call') ?? 'null'));
if (nme?.pass) await fetch(`${CONTROL}/api/v1/pass/leave`, { method: 'POST', headers: { Authorization: `Bearer ${nme.pass}` } });

// ---- 6: no CSP violation anywhere ----
const csp2 = [...(await slow.evaluate(() => window.__csp)), ...(await neg.evaluate(() => window.__csp))];
check('no Content-Security-Policy violation on the player, the lobby or the call', csp1.length === 0 && csp2.length === 0, JSON.stringify([...csp1, ...csp2]));

await browser.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(OUT, 'relay-results.json'), JSON.stringify({ results, evidence }, null, 2));
log(`${results.length - failed.length} of ${results.length} steps passed`);
process.exit(failed.length ? 1 : 0);
