// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// G0, the two-person test (docs 0011), with one person: two invited guests in a station's room,
// one in Chromium on this machine and one in a phone's browser driven over the Chrome DevTools
// protocol (an Android WebView forwarded by adb). Neither page uses a microphone: each page's
// getUserMedia returns a tone source the script plays, and each page listens to what the desk
// sends it. Guest A sounds a 1 kHz burst every few seconds; the phone, hearing it, answers at
// once with 1.5 kHz; A times the answer. That round trip is two mouth-to-ear paths plus a few
// milliseconds of detection, and the clocks of the two ends are compared over CDP to split it.
// A hearing its own 1 kHz, or the phone its own 1.5 kHz, is an echo through the desk. Each
// connection's ICE pair, round-trip time, jitter and loss are sampled from getStats.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core \
//   CONTROL=https://station WEB=https://public-pages KEY_FILE=/path/to/station-key \
//   PHONE_CDP=http://127.0.0.1:9333 MINUTES=10 PHONE_RELAY=1 OUT=/some/dir node tests/g0/two-person.mjs
//
// PHONE_CDP=local puts the second guest in a page here too, a baseline without the phone.
// PHONE_CDP: `adb forward tcp:9333 localabstract:webview_devtools_remote_<pid>` for a WebView
// browser (LineageOS's Jelly). PHONE_RELAY=1 ticks the phone page's own "Connect through the
// station's relay" (TURN only), the done-when's "one of them through TURN"; A_RELAY=1 does the
// same for the guest here (on staging, 2026-11-03, TURN refused a phone that reached it over
// IPv6 with 508, so the relayed guest was the IPv4 one here). The station key
// makes the two invites and ends both passes afterwards. Results go to OUT, never the
// repository. Not part of `just ci`: it needs a phone and a station on a real network.
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const CONTROL = process.env.CONTROL;
const WEB = process.env.WEB ?? CONTROL;
const KEY = readFileSync(process.env.KEY_FILE, 'utf8').trim();
const PHONE_CDP = process.env.PHONE_CDP ?? 'http://127.0.0.1:9333';
const MINUTES = Number(process.env.MINUTES ?? 10);
const PHONE_RELAY = process.env.PHONE_RELAY === '1';
const A_RELAY = process.env.A_RELAY === '1';
const OUT = process.env.OUT ?? '/tmp/weavesignal-g0';
const PERIOD_MS = 4000;
mkdirSync(OUT, { recursive: true });

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const admin = { Authorization: `Bearer ${KEY}`, 'Content-Type': 'application/json' };
const post = async (path, body) => {
  const r = await fetch(`${CONTROL}${path}`, { method: 'POST', headers: admin, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(`${path}: ${r.status} ${await r.text()}`);
  return r.json().catch(() => ({}));
};

// ---- the script each page runs before its own: a tone source for a microphone, an ear on
// every received audio track, and getStats on every peer connection ----
const pageScript = (me, other) => `(() => {
  const ME = ${me}, OTHER = ${other};
  const AC = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' });
  const dest = AC.createMediaStreamDestination();
  const osc = AC.createOscillator(); osc.frequency.value = ME;
  const gain = AC.createGain(); gain.gain.value = 0;
  osc.connect(gain).connect(dest); osc.start();
  const g0 = window.__g0 = { me: ME, sent: [], heard: [], pcs: [], tracks: 0, answer: false, ctx: AC };
  // A burst of ME Hz, 150 ms at -12 dBFS; the time is the moment it enters the stream.
  g0.burst = () => {
    const t = AC.currentTime + 0.005;
    gain.gain.setValueAtTime(0.25, t); gain.gain.setValueAtTime(0, t + 0.15);
    const at = performance.timeOrigin + performance.now() + 5 + (AC.outputLatency || 0) * 0;
    g0.sent.push(at); return at;
  };
  g0.now = () => performance.timeOrigin + performance.now();
  const md = navigator.mediaDevices;
  const orig = md.getUserMedia.bind(md);
  md.getUserMedia = async (c) => {
    if (c && c.audio) {
      await AC.resume().catch(() => {});
      const s = new MediaStream(dest.stream.getAudioTracks().map((t) => t.clone()));
      if (c.video) { try { const v = await orig({ video: c.video }); v.getVideoTracks().forEach((t) => s.addTrack(t)); } catch (e) {} }
      return s;
    }
    return orig(c);
  };
  // A subclass, so the page's own wrapper (turn.ts moves LiveKit's TURN port to the station's,
  // in the constructor and in setConfiguration) still runs on top of it.
  const Orig = window.RTCPeerConnection;
  g0.ice = [];
  class PC extends Orig {
    constructor(cfg, ...rest) {
      super(cfg, ...rest);
      const pc = this;
      g0.pcs.push(pc);
      pc.addEventListener('track', (e) => { if (e.track.kind === 'audio') ear(e.track); });
      // What the page was told to use (no credentials), and how each connection went.
      const entry = { servers: [], policy: null, states: [] };
      g0.ice.push(entry);
      const conf = () => { const c = pc.getConfiguration(); entry.servers = (c.iceServers || []).map((x) => [].concat(x.urls)); entry.policy = c.iceTransportPolicy; };
      conf(); entry.conf = conf;
      const mark = () => { conf(); entry.states.push([Math.round(performance.now()), pc.iceConnectionState, pc.connectionState]); };
      pc.addEventListener('iceconnectionstatechange', mark);
      pc.addEventListener('connectionstatechange', mark);
      pc.addEventListener('icecandidateerror', (e) => entry.states.push([Math.round(performance.now()), 'candidate-error', e.url, e.errorCode, e.errorText]));
    }
  }
  window.RTCPeerConnection = PC;
  // Goertzel power at f over 10 ms windows, as a share of the window's power.
  function share(buf, f) {
    const k = 2 * Math.cos(2 * Math.PI * f / AC.sampleRate);
    let s1 = 0, s2 = 0, e = 0;
    for (let i = 0; i < buf.length; i++) { const s0 = buf[i] + k * s1 - s2; s2 = s1; s1 = s0; e += buf[i] * buf[i]; }
    const p = s1 * s1 + s2 * s2 - k * s1 * s2;
    return { p: p / (buf.length * buf.length / 4), e: e / buf.length };
  }
  function ear(track) {
    g0.tracks++;
    const ms = new MediaStream([track]);
    const el = new Audio(); el.srcObject = ms; el.muted = true; el.play().catch(() => {});
    const src = AC.createMediaStreamSource(ms);
    const an = AC.createAnalyser(); an.fftSize = 512; src.connect(an);
    const buf = new Float32Array(480);
    // A tone counts once from its onset; the ear re-arms after 50 ms without it.
    const on = { [ME]: false, [OTHER]: false }, quiet = { [ME]: 0, [OTHER]: 0 };
    setInterval(() => {
      an.getFloatTimeDomainData(buf);
      const now = performance.timeOrigin + performance.now();
      for (const f of [ME, OTHER]) {
        const { p, e } = share(buf, f);
        const loud = e > 1e-5 && p > 0.5 * e;
        if (loud) {
          quiet[f] = 0;
          if (!on[f]) {
            on[f] = true;
            g0.heard.push({ f, at: now, rms: Math.sqrt(e) });
            if (f === OTHER && g0.answer) g0.burst();
          }
        } else if (on[f] && ++quiet[f] >= 10) on[f] = false;
      }
    }, 5);
  }
  g0.stats = async () => {
    const out = [];
    for (const pc of g0.pcs) {
      if (pc.connectionState === 'closed') continue;
      const r = await pc.getStats();
      let pair, local, remote, inb = [], outb = [];
      r.forEach((s) => {
        if (s.type === 'transport' && s.selectedCandidatePairId) pair = r.get(s.selectedCandidatePairId);
      });
      if (!pair) r.forEach((s) => { if (s.type === 'candidate-pair' && s.nominated && s.state === 'succeeded') pair = s; });
      if (pair) { local = r.get(pair.localCandidateId); remote = r.get(pair.remoteCandidateId); }
      r.forEach((s) => {
        if (s.type === 'inbound-rtp' && s.kind === 'audio') inb.push({ jitter: s.jitter, lost: s.packetsLost, got: s.packetsReceived, jbDelay: s.jitterBufferEmittedCount ? s.jitterBufferDelay / s.jitterBufferEmittedCount : null, concealed: s.concealedSamples, samples: s.totalSamplesReceived });
        if (s.type === 'outbound-rtp' && s.kind === 'audio') outb.push({ sent: s.packetsSent });
      });
      out.push({ state: pc.connectionState, rtt: pair ? pair.currentRoundTripTime : null,
        local: local ? { type: local.candidateType, protocol: local.protocol, relay: local.relayProtocol || null, network: local.networkType || null } : null,
        remote: remote ? { type: remote.candidateType, protocol: remote.protocol } : null, inb, outb });
    }
    return out;
  };
})();`;

// ---- a small CDP client for the phone's page ----
async function cdpPage(base) {
  const list = await (await fetch(`${base}/json`)).json();
  const target = list.find((t) => t.type === 'page');
  if (!target) throw new Error('no page on the phone: open the browser first');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0; const wait = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && wait.has(d.id)) { wait.get(d.id)(d); wait.delete(d.id); } };
  const send = (method, params = {}) => new Promise((res, rej) => {
    const n = ++id; wait.set(n, (d) => (d.error ? rej(new Error(`${method}: ${d.error.message}`)) : res(d.result)));
    ws.send(JSON.stringify({ id: n, method, params }));
  });
  const evaluate = async (expr, gesture = false) => {
    const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true, userGesture: gesture });
    if (r.exceptionDetails) throw new Error(`phone: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
    return r.result.value;
  };
  return { send, evaluate, close: () => ws.close() };
}

// The offset of a page's clock against this machine's (positive: the page is ahead), from the
// exchange with the shortest round trip, as NTP does.
async function offset(evalFn) {
  let best = null;
  for (let i = 0; i < 15; i++) {
    const t0 = Date.now(); const p = await evalFn('performance.timeOrigin + performance.now()'); const t1 = Date.now();
    if (!best || t1 - t0 < best.rtt) best = { rtt: t1 - t0, off: p - (t0 + t1) / 2 };
  }
  return best;
}

const phase = `document.querySelector('.banner')?.getAttribute('data-phase') ?? null`;
const relayBox = `(() => { const i = [...document.querySelectorAll('label.check')].find((l) => /station's relay/.test(l.textContent))?.querySelector('input'); if (!i) return false; if (!i.checked) i.click(); return i.checked; })()`;
const joinBtn = `(() => { const b = [...document.querySelectorAll('button')].find((b) => /Join the studio/.test(b.textContent)); if (!b) return false; b.click(); return true; })()`;
const leaveBtn = `(async () => { const f = () => [...document.querySelectorAll('button')].find((b) => /Leave the studio|Press again/.test(b.textContent)); for (let i = 0; i < 2; i++) { const b = f(); if (b) b.click(); await new Promise((r) => setTimeout(r, 400)); } return true; })()`;

// ---- go ----
const results = { started: new Date().toISOString(), control: CONTROL, web: WEB, minutes: MINUTES, phone_relay: PHONE_RELAY, a_relay: A_RELAY };
const [ia, ib] = [await post('/api/v1/invites', { role: 'guest', name: 'Gregor Stewart' }), await post('/api/v1/invites', { role: 'guest', name: 'Finlay Grant' })];
log('invites', ia.identity, ib.identity);
let browser, phone;
try {
  browser = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] });
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await ctx.addInitScript(pageScript(1000, 1500));
  const a = await ctx.newPage();
  a.on('console', (m) => m.type() === 'error' && log('A console:', m.text()));
  await a.goto((/^https?:/.test(ia.link) ? ia.link : `${WEB}${ia.link}`));

  if (PHONE_CDP === 'local') {
    // A baseline without the phone: the second guest is a page here too.
    const bctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    await bctx.addInitScript(pageScript(1500, 1000));
    const b = await bctx.newPage();
    await b.goto((/^https?:/.test(ib.link) ? ib.link : `${WEB}${ib.link}`));
    phone = { evaluate: (e) => b.evaluate(e), close: () => {} };
    results.phone_user_agent = 'local Chromium';
  } else {
    phone = await cdpPage(PHONE_CDP);
    await phone.send('Page.enable');
    await phone.send('Runtime.enable');
    await phone.send('Page.addScriptToEvaluateOnNewDocument', { source: pageScript(1500, 1000) });
    // Through a blank page: a link that differs from the open page only in its fragment (the
    // pass) would not load a new document, and the script above would not run.
    await phone.send('Page.navigate', { url: 'about:blank' });
    await sleep(1000);
    await phone.send('Page.navigate', { url: (/^https?:/.test(ib.link) ? ib.link : `${WEB}${ib.link}`) });
    results.phone_user_agent = (await (await fetch(`${PHONE_CDP}/json/version`)).json())['User-Agent'];
  }

  if (A_RELAY) {
    for (let i = 0; i < 60 && !(await a.evaluate(relayBox).catch(() => false)); i++) await sleep(500);
    results.a_relay_ticked = await a.evaluate(relayBox).catch(() => false);
  }
  for (let i = 0; i < 60; i++) {
    const okA = await a.evaluate(joinBtn).catch(() => false);
    if (okA) break; await sleep(500);
  }
  if (PHONE_RELAY) {
    for (let i = 0; i < 60 && !(await phone.evaluate(relayBox, true).catch(() => false)); i++) await sleep(500);
    results.phone_relay_ticked = await phone.evaluate(relayBox, true).catch(() => false);
  }
  for (let i = 0; i < 60; i++) {
    const okB = await phone.evaluate(joinBtn, true).catch(() => false);
    if (okB) break; await sleep(500);
  }
  log('both pressed Join');
  const onAir = async (ev) => { for (let i = 0; i < 120; i++) { if ((await ev(phase).catch(() => null)) === 'on_air') return true; await sleep(250); } return false; };
  results.a_on_air = await onAir((e) => a.evaluate(e));
  results.b_on_air = await onAir((e) => phone.evaluate(e));
  log('on air:', results.a_on_air, results.b_on_air);
  if (!results.a_on_air || !results.b_on_air) throw new Error('a guest did not get on air');
  await phone.evaluate('window.__g0.answer = true; window.__g0.ctx.resume(); true', true);
  await sleep(3000);

  const offA = await offset((e) => a.evaluate(e));
  const offB = await offset((e) => phone.evaluate(e));
  results.clock = { a: offA, phone: offB };
  log('clock offsets (ms, best of 15):', offA, offB);

  const trials = []; const stats = [];
  const end = Date.now() + MINUTES * 60_000;
  let next = Date.now();
  while (Date.now() < end) {
    const sentAt = await a.evaluate('window.__g0.burst()');
    await sleep(PERIOD_MS - 500);
    const ha = await a.evaluate('window.__g0.heard.splice(0)');
    const hb = await phone.evaluate('window.__g0.heard.splice(0)');
    const back = ha.find((h) => h.f === 1500 && h.at > sentAt);
    const there = hb.find((h) => h.f === 1000);
    const t = {
      at: new Date().toISOString(),
      rtt_ms: back ? back.at - sentAt : null,
      // One way, A to the phone, on this machine's clock: both times moved by their offsets.
      a_to_phone_ms: there ? (there.at - offB.off) - (sentAt - offA.off) : null,
      echo_a: ha.some((h) => h.f === 1000), echo_phone: hb.some((h) => h.f === 1500),
    };
    if (t.rtt_ms !== null && t.a_to_phone_ms !== null) t.phone_to_a_ms = t.rtt_ms - t.a_to_phone_ms;
    trials.push(t);
    if (trials.length % 5 === 1) {
      const s = { at: t.at, a: await a.evaluate('window.__g0.stats()'), phone: await phone.evaluate('window.__g0.stats()') };
      stats.push(s);
      const pl = s.phone.find((p) => p.local)?.local;
      log(`trial ${trials.length}: rtt ${t.rtt_ms?.toFixed(0)} ms, A→phone ${t.a_to_phone_ms?.toFixed(0)} ms, phone via ${pl?.type}/${pl?.protocol}${pl?.relay ? '/' + pl.relay : ''}, echo ${t.echo_a || t.echo_phone}`);
    }
    next += PERIOD_MS; const wait = next - Date.now(); if (wait > 0) await sleep(wait);
  }
  results.trials = trials; results.stats = stats;
  const ice = "window.__g0.ice.map((e) => { e.conf(); const { conf, ...rest } = e; return rest; })";
  results.ice = { a: await a.evaluate(ice), phone: await phone.evaluate(ice) };
  const ok = trials.filter((t) => t.rtt_ms !== null).map((t) => t.rtt_ms).sort((x, y) => x - y);
  const oneway = trials.filter((t) => t.a_to_phone_ms !== null).map((t) => t.a_to_phone_ms).sort((x, y) => x - y);
  const q = (xs, p) => (xs.length ? xs[Math.min(xs.length - 1, Math.floor(p * xs.length))] : null);
  results.summary = {
    trials: trials.length, answered: ok.length,
    rtt_ms: { p50: q(ok, 0.5), p90: q(ok, 0.9), max: ok.at(-1) ?? null },
    a_to_phone_ms: { p50: q(oneway, 0.5), p90: q(oneway, 0.9) },
    echoes: trials.filter((t) => t.echo_a || t.echo_phone).length,
    phone_path: stats.at(-1)?.phone.find((p) => p.local)?.local ?? null,
    a_path: stats.at(-1)?.a.find((p) => p.local)?.local ?? null,
  };
  log('summary', JSON.stringify(results.summary));
  await a.screenshot({ path: join(OUT, 'a-on-air.png') }).catch(() => {});
  await a.evaluate(leaveBtn).catch(() => {});
  await phone.evaluate(leaveBtn, true).catch(() => {});
} catch (e) {
  results.error = String(e?.stack ?? e);
  log('ERROR', results.error);
} finally {
  for (const i of [ia, ib]) await post('/api/v1/passes/end', { identity: i.identity }).catch((e) => log('end pass:', String(e)));
  log('passes ended');
  writeFileSync(join(OUT, `g0-${Date.now()}.json`), JSON.stringify(results, null, 1));
  phone?.close(); await browser?.close();
}
process.exit(results.error ? 1 : 0);
