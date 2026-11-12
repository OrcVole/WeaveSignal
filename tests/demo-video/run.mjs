// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Films the demo video's show (tests/demo-video/script.json) on the demo stack
// (tests/demo-video/run-stack.sh), through the real desk. Every character is a person in the
// room, joined the way a real one would (Eilidh, Gregor and Finlay by invite links, Niall and
// Jessie from the player page's call-in, Shona with a screener's room token), and their
// microphone is their designed-voice stem (tests/demo-video/synth.sh), played into the page's
// microphone at the step that says the line. Eilidh's desk and Shona's screener are the
// studio app's browser build (1,920 × 1,080, its text size at 120 %), each signed in as that member of staff, driven by keys and clicks
// at the script's steps, with an injected cursor, a ring on every click and a keycap for every
// key; Gregor's invite page and Niall's player page (390 px, touch) are filmed too.
//
// Recorded into target/demo-video/run/ (OUT): each filmed page as JPEG frames from Chromium's
// screencast with their wall-clock times (frames/<page>/), every room track the desk publishes
// or hears (a `listener` monitor in the studio room and one in each caller's booth, by
// MediaRecorder: audio/<identity>.webm), the listener stream as listeners get it
// (audio/stream.ogg), stills of every page at the end of each moment (stills/), and
// timeline.json: when each step, line, key and click really happened. edit.sh makes the video.
//
//   tests/demo-video/run-stack.sh        # in one shell; then, in another:
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core CHROMIUM=/path/to/chrome node tests/demo-video/run.mjs
//
// Playwright is not a dependency of the repository (see tests/m2/drive.mjs).
import { createRequire } from 'node:module';
import { execFileSync, spawn } from 'node:child_process';
import { appendFileSync, createWriteStream, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const OUT = process.env.OUT ?? join(root, 'target/demo-video/run/show');
const STEMS = process.env.STEMS ?? join(root, 'target/demo-video/stems');
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:28310';
const WEB = process.env.WEB ?? 'http://127.0.0.1:28430';
const STUDIO = process.env.STUDIO ?? 'http://127.0.0.1:28420/';
const STREAM = process.env.STREAM ?? 'http://127.0.0.1:28812/programme.opus';
const TOKEN = process.env.ADMIN_TOKEN ?? 'demo-admin-test-only';
const PASSWORD = 'a-long-test-only-phrase';
const script = JSON.parse(readFileSync(join(here, 'script.json'), 'utf8'));

rmSync(OUT, { recursive: true, force: true });
for (const d of ['frames', 'audio', 'stills']) mkdirSync(join(OUT, d), { recursive: true });
const t0 = Date.now();
const log = (...a) => console.log(((Date.now() - t0) / 1000).toFixed(2).padStart(7), ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const timeline = { started: t0, events: [] };
const mark = (e) => {
  const ev = { t: Date.now(), ...e };
  timeline.events.push(ev);
  return ev;
};
const save = () => writeFileSync(join(OUT, 'timeline.json'), JSON.stringify(timeline, null, 1));

// ---------------------------------------------------------------- the control plane
const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };
async function api(method, path, body, headers = admin) {
  const res = await fetch(`${CONTROL}${path}`, { method, headers, body: body ? JSON.stringify(body) : undefined });
  const text = await res.text();
  let json = {};
  try {
    json = JSON.parse(text);
  } catch {}
  return { status: res.status, body: json };
}
const view = async () => (await api('GET', '/api/v1/studio')).body;
const ids = {}; // character -> room identity
const src = (v, who) => v.sources?.find((s) => s.identity === ids[who]);
const caller = (v, who) => v.callers?.find((c) => c.identity === ids[who]);
let dumpsBefore = 0;
const predicates = {
  eilidh_on_air: (v) => src(v, 'eilidh')?.on_air,
  gregor_on_air: (v) => src(v, 'gregor')?.on_air,
  niall_waiting: (v) => caller(v, 'niall')?.present,
  niall_screening: (v) => caller(v, 'niall')?.state === 'screening',
  niall_ready: (v) => caller(v, 'niall')?.state === 'ready',
  niall_cued: (v) => src(v, 'niall')?.cue,
  niall_on_air: (v) => src(v, 'niall')?.on_air,
  niall_gone: (v) => !caller(v, 'niall') && !src(v, 'niall'),
  jessie_waiting: (v) => caller(v, 'jessie')?.present,
  jessie_screening: (v) => caller(v, 'jessie')?.state === 'screening',
  jessie_ready: (v) => caller(v, 'jessie')?.state === 'ready',
  jessie_cued: (v) => src(v, 'jessie')?.cue,
  jessie_on_air: (v) => src(v, 'jessie')?.on_air,
  jessie_gone: (v) => !caller(v, 'jessie') && !src(v, 'jessie'),
  dumped: (v) => (v.desk?.delay?.dumps ?? 0) > dumpsBefore,
  playout_down: (v) => v.desk?.playout && !v.desk.playout.connected,
  playout_up: (v) => v.desk?.playout?.connected,
  auto: (v) => v.mode === 'auto',
};
async function until(name, ms = 25_000) {
  const pred = predicates[name];
  if (!pred) throw new Error(`no predicate ${name}`);
  const start = Date.now();
  for (;;) {
    const v = await view();
    if (pred(v)) return Date.now() - start;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${name}`);
    await sleep(100);
  }
}

// ---------------------------------------------------------------- the browser
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.CHROMIUM,
  args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-ui-for-media-stream', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows'],
});

// The microphone: the page's getUserMedia gets a track from an AudioContext, into which
// __say(id) plays that line's stem (decoded in advance, so it starts at once).
function voiceInit() {
  const ac = new AudioContext({ sampleRate: 48000 });
  const dest = ac.createMediaStreamDestination();
  const buffers = {};
  // A real microphone's room tone, about -66 dBFS, playing throughout: with nothing playing
  // into it, a Web Audio destination gives the track no samples at all, Chromium sends no
  // packets, and the desk cannot tell that from a dead link (issue 13). Digital silence would
  // do as well (a ConstantSourceNode at 0 sends a packet every 20 ms); the tone sounds real.
  const tone = ac.createBuffer(1, ac.sampleRate * 3, ac.sampleRate);
  const data = tone.getChannelData(0);
  for (let i = 0; i < data.length; i++) data[i] = (Math.random() * 2 - 1) * 0.0009;
  const bed = ac.createBufferSource();
  bed.buffer = tone;
  bed.loop = true;
  const soft = ac.createBiquadFilter();
  soft.type = 'lowpass';
  soft.frequency.value = 3000;
  bed.connect(soft).connect(dest);
  bed.start();
  window.__voice = {
    async load(list) {
      for (const id of list) {
        const r = await fetch(`/__stems/${id}.wav`);
        buffers[id] = await ac.decodeAudioData(await r.arrayBuffer());
      }
      return Object.keys(buffers).length;
    },
    say(id) {
      return new Promise((resolve) => {
        const s = ac.createBufferSource();
        s.buffer = buffers[id];
        s.connect(dest);
        const at = Date.now();
        s.onended = () => resolve({ at, ms: Date.now() - at });
        ac.resume().then(() => s.start());
      });
    },
    track: () => dest.stream.getAudioTracks()[0],
  };
  navigator.mediaDevices.getUserMedia = async () => {
    await ac.resume();
    return new MediaStream([dest.stream.getAudioTracks()[0].clone()]);
  };
}

// What a viewer of the video needs to see: a cursor that moves, a ring where it clicks or a
// finger taps, and each key pressed as a keycap at the bottom centre for about a second (a
// held key stays up until it is released). Keys typed into a text field are not shown, only
// Esc and the Ctrl chords that still act from one.
function overlayInit({ touch }) {
  const names = { Control: '⌃', Shift: '⇧', Alt: '⌥', Delete: 'Del', Escape: 'Esc', ' ': 'Space', Enter: '⏎', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→' };
  const style = `
    #__ov-cursor{position:fixed;left:0;top:0;width:22px;height:22px;pointer-events:none;z-index:2147483647;transform:translate(-3px,-2px);filter:drop-shadow(0 1px 2px rgba(0,0,0,.6))}
    .__ov-ring{position:fixed;width:44px;height:44px;margin:-22px 0 0 -22px;border-radius:50%;border:3px solid #ffd166;pointer-events:none;z-index:2147483646;animation:__ovring .6s ease-out forwards}
    @keyframes __ovring{from{transform:scale(.4);opacity:1}to{transform:scale(1.5);opacity:0}}
    #__ov-keys{position:fixed;left:50%;bottom:28px;transform:translateX(-50%);display:flex;gap:10px;pointer-events:none;z-index:2147483647}
    .__ov-key{font:600 26px/1 system-ui,'Noto Sans',sans-serif;color:#f4f4f6;background:rgba(18,20,28,.88);border:1px solid rgba(255,255,255,.35);border-bottom-width:4px;border-radius:9px;padding:12px 18px;min-width:30px;text-align:center;box-shadow:0 6px 18px rgba(0,0,0,.45);transition:opacity .25s}
    .__ov-key small{display:block;font-size:12px;font-weight:500;opacity:.75;margin-top:4px;letter-spacing:.04em}`;
  const install = () => {
    if (document.getElementById('__ov-keys')) return;
    const s = document.createElement('style');
    s.textContent = style;
    document.documentElement.appendChild(s);
    const keys = document.createElement('div');
    keys.id = '__ov-keys';
    document.documentElement.appendChild(keys);
    if (!touch) {
      const c = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      c.id = '__ov-cursor';
      c.setAttribute('viewBox', '0 0 22 22');
      c.innerHTML = '<path d="M3 2 L3 18 L7.5 13.8 L10.5 20.5 L13.4 19.2 L10.5 12.6 L16.5 12.6 Z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/>';
      c.style.left = '-40px';
      document.documentElement.appendChild(c);
      addEventListener('mousemove', (e) => { c.style.left = `${e.clientX / z()}px`; c.style.top = `${e.clientY / z()}px`; }, true);
    }
  };
  // The studio app zooms its root (its text size, ⌃+): what is placed at a pointer's
  // viewport position is divided by that zoom.
  const z = () => parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
  const ring = (x, y) => {
    x /= z();
    y /= z();
    const r = document.createElement('div');
    r.className = '__ov-ring';
    r.style.left = `${x}px`;
    r.style.top = `${y}px`;
    document.documentElement.appendChild(r);
    setTimeout(() => r.remove(), 700);
  };
  addEventListener('mousedown', (e) => ring(e.clientX, e.clientY), true);
  addEventListener('touchstart', (e) => { for (const t of e.changedTouches) ring(t.clientX, t.clientY); }, true);
  const held = new Map();
  const label = (e) => {
    let k = names[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key);
    if (e.code?.startsWith('Digit')) k = e.code.slice(5);
    if (e.code?.startsWith('Key')) k = e.code.slice(3);
    return (e.ctrlKey && e.key !== 'Control' ? '⌃' : '') + (e.shiftKey && e.key !== 'Shift' && !e.ctrlKey ? '⇧' : '') + k;
  };
  addEventListener('keydown', (e) => {
    install();
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key) || e.repeat) return;
    const f = document.activeElement;
    const inField = f && (f.tagName === 'INPUT' || f.tagName === 'TEXTAREA' || f.isContentEditable);
    if (inField && !e.ctrlKey && e.key !== 'Escape') return;
    const box = document.getElementById('__ov-keys');
    const el = document.createElement('div');
    el.className = '__ov-key';
    el.textContent = label(e);
    box.appendChild(el);
    held.set(e.code, { el, at: Date.now() });
    setTimeout(() => {
      const h = held.get(e.code);
      if (h?.el === el && h.up) fade(el);
      if (h?.el === el && !h.up) {
        el.innerHTML = `${el.textContent}<small>HELD</small>`;
      }
      if (h?.el !== el) fade(el);
    }, 1000);
  }, true);
  const fade = (el) => { el.style.opacity = '0'; setTimeout(() => el.remove(), 260); };
  addEventListener('keyup', (e) => {
    const h = held.get(e.code);
    if (!h) return;
    h.up = true;
    if (Date.now() - h.at >= 1000) fade(h.el);
  }, true);
  document.addEventListener('DOMContentLoaded', install);
}

const stemRoute = async (route) => {
  const id = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop().replace(/\.wav$/, ''));
  try {
    await route.fulfill({ status: 200, contentType: 'audio/wav', body: readFileSync(join(STEMS, `${id}.wav`)) });
  } catch {
    await route.fulfill({ status: 404, body: '' });
  }
};

const pages = {}; // name -> { page, ctx, touch, mouse }
async function newPage(name, { width = 1920, height = 1080, scale = 1, zoom = null, phone = false, voice = false, overlay = false } = {}) {
  const ctx = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: phone ? 2 : scale,
    isMobile: phone,
    hasTouch: phone,
    locale: 'en-GB',
    timezoneId: 'Europe/London',
    colorScheme: 'dark',
    permissions: voice ? ['microphone'] : [],
  });
  await ctx.addInitScript((z) => {
    try {
      localStorage.setItem('weavesignal.theme', 'dark');
      if (z) localStorage.setItem('weavesignal.zoom', String(z)); // the studio app's own text size (⌃+)
    } catch {}
  }, zoom);
  if (voice) {
    await ctx.addInitScript(voiceInit);
    await ctx.route('**/__stems/*', stemRoute);
  }
  if (overlay) await ctx.addInitScript(overlayInit, { touch: phone });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => log(`[${name}] page error`, e.message));
  page.on('console', (m) => m.type() === 'error' && !/favicon|ERR_ABORTED/.test(m.text()) && log(`[${name}] console`, m.text().slice(0, 160)));
  pages[name] = { name, page, ctx, phone, scale: phone ? 2 : scale, mouse: { x: width / 2, y: height / 2 } };
  return pages[name];
}

// Chromium's screencast, every frame with its wall-clock time.
const casts = [];
async function film(p) {
  const dir = join(OUT, 'frames', p.name);
  mkdirSync(dir, { recursive: true });
  const index = createWriteStream(join(dir, 'frames.jsonl'));
  const cdp = await p.ctx.newCDPSession(p.page);
  let n = 0;
  cdp.on('Page.screencastFrame', (f) => {
    const file = `${String(n++).padStart(6, '0')}.jpg`;
    writeFileSync(join(dir, file), Buffer.from(f.data, 'base64'));
    index.write(`${JSON.stringify({ file, t: f.metadata.timestamp * 1000 })}\n`);
    cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  });
  const vp = p.page.viewportSize();
  // The screencast sends CSS pixels whatever the device scale: the page's own size.
  await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 92, maxWidth: vp.width, maxHeight: vp.height, everyNthFrame: 1 });
  casts.push({ cdp, index, p, frames: () => n });
}

// The monitors: a `listener` token hears every track (ADR-0118); each track to a file.
const monitors = [];
async function monitor(name, booth) {
  const tok = await api('POST', '/api/v1/room/tokens', { identity: `demo-monitor-${name}`, name: 'Demo monitor', role: 'listener', ...(booth ? { booth } : {}) });
  if (tok.status !== 201) throw new Error(`monitor token ${tok.status}`);
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(`${WEB}/`);
  await page.addScriptTag({ path: join(root, 'apps/web/node_modules/livekit-client/dist/livekit-client.umd.js') });
  await page.exposeFunction('__chunk', (who, b64) => appendFileSync(join(OUT, 'audio', `${who}.webm`), Buffer.from(b64, 'base64')));
  await page.exposeFunction('__started', (who, at) => mark({ kind: 'track', monitor: name, identity: who, at }));
  await page.evaluate(async ({ url, token }) => {
    const { Room, RoomEvent } = window.LivekitClient;
    const room = new Room();
    window.__recs = [];
    const seen = new Set();
    room.on(RoomEvent.TrackSubscribed, (track, _pub, who) => {
      if (track.kind !== 'audio' || seen.has(who.identity)) return;
      seen.add(who.identity);
      const el = track.attach();
      el.volume = 0.001;
      document.body.appendChild(el);
      const rec = new MediaRecorder(new MediaStream([track.mediaStreamTrack]), { mimeType: 'audio/webm;codecs=opus', audioBitsPerSecond: 160000 });
      const queue = [];
      let busy = Promise.resolve();
      rec.ondataavailable = (e) => {
        busy = busy.then(async () => {
          const b = new Uint8Array(await e.data.arrayBuffer());
          let s = '';
          for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
          await window.__chunk(who.identity, btoa(s));
        });
        queue.push(busy);
      };
      rec.onstart = () => window.__started(who.identity, Date.now());
      rec.start(1000);
      window.__recs.push({ rec, done: () => busy });
    });
    await room.connect(url, token, { autoSubscribe: true });
    window.__room = room;
  }, { url: tok.body.url, token: tok.body.token });
  monitors.push({ name, page, ctx });
  log('monitor', name, booth ?? 'studio');
}
async function stopMonitors() {
  for (const m of monitors) {
    await m.page.evaluate(async () => {
      for (const r of window.__recs) {
        await new Promise((res) => {
          r.rec.addEventListener('stop', res, { once: true });
          r.rec.stop();
        });
        await r.done();
      }
    }).catch((e) => log('monitor stop', e.message));
  }
}

// ---------------------------------------------------------------- driving
const KEYROWS = { on: '123456', cue: 'qwerty', talk: 'asdfgh' };
async function resolveKey(spec, pageName) {
  const m = /^(caller|cue|on|talk):(\w+)$/.exec(spec);
  if (!m) return spec;
  const v = await view();
  if (m[1] === 'caller') {
    const i = v.callers.findIndex((c) => c.identity === ids[m[2]]);
    if (i < 0) throw new Error(`${m[2]} is not in the queue`);
    return String(i + 1);
  }
  const s = src(v, m[2]) ?? v.sources.find((x) => x.identity === ids[m[2]]);
  if (!s) throw new Error(`${m[2]} has no slot`);
  return KEYROWS[m[1]][s.slot - 1];
}
async function moveTo(p, x, y) {
  const from = p.mouse;
  const steps = Math.max(8, Math.round(Math.hypot(x - from.x, y - from.y) / 30));
  await p.page.mouse.move(x, y, { steps });
  p.mouse = { x, y };
}
function target(p, what) {
  if (what === 'submit') return p.page.locator('form button[type="submit"]');
  if (/^[#.[]/.test(what)) return p.page.locator(what).first();
  return p.page.getByRole('button', { name: what }).first();
}
async function line(step) {
  const p = pages[`mic-${step.who}`] ?? pages[step.who];
  const ev = mark({ kind: 'line', id: step.line, who: step.who, text: step.text, ...(step.swear ? { swear: step.swear } : {}) });
  const played = p.page.evaluate((id) => window.__voice.say(id), step.line);
  ev.done = played.then((r) => {
    ev.at = r.at;
    ev.ms = r.ms;
    return r;
  });
  return ev.done;
}
const pendingLines = {};
async function run(step, moment) {
  const p = step.page ? pages[step.page] : null;
  if (step.show) mark({ kind: 'show', page: step.show, moment });
  if (step.overlay !== undefined) mark({ kind: 'overlay', overlay: step.overlay, moment });
  if (step.line) {
    const done = line(step);
    pendingLines[step.line] = done;
    if (!step.async) await done;
  }
  if (step.wait_line) await pendingLines[step.wait_line];
  if (step.key) {
    const key = await resolveKey(step.key, step.page);
    mark({ kind: 'key', key, page: step.page, moment });
    await p.page.keyboard.press(key);
  }
  if (step.hold) {
    const key = await resolveKey(step.hold, step.page);
    mark({ kind: 'hold', key, page: step.page, moment });
    step._held = key;
    held[step.hold] = key;
    await p.page.keyboard.down(key);
  }
  if (step.release) {
    const key = held[step.release];
    mark({ kind: 'release', key, page: step.page, moment });
    await p.page.keyboard.up(key);
  }
  if (step.type) {
    mark({ kind: 'type', text: step.type, page: step.page, moment });
    await p.page.keyboard.type(step.type, { delay: 55 });
  }
  if (step.fill) {
    const el = p.page.locator(step.fill);
    await el.tap();
    mark({ kind: 'type', text: step.text, page: step.page, moment });
    await el.pressSequentially(step.text, { delay: 60 });
  }
  if (step.click || step.tap) {
    const el = target(p, step.click ?? step.tap);
    await el.waitFor({ timeout: 20_000 });
    await el.scrollIntoViewIfNeeded();
    const b = await el.boundingBox();
    const x = b.x + b.width / 2;
    const y = b.y + b.height / 2;
    if (p.phone) {
      mark({ kind: 'tap', what: step.tap ?? step.click, page: step.page, x, y, moment });
      await p.page.touchscreen.tap(x, y);
      // Once the call is placed, the phone keeps its call banner in view, as a person would
      // scroll back to it: the page itself leaves the form's scroll where it was.
      if ((step.tap ?? step.click) === 'submit') await p.page.evaluate(() => {
        const show = () => {
          const b = document.querySelector('.banner');
          if (b) window.scrollTo({ top: Math.max(0, b.getBoundingClientRect().top + window.scrollY - 150), behavior: 'smooth' });
        };
        setTimeout(show, 500);
        new MutationObserver(() => setTimeout(show, 200)).observe(document.body, { attributes: true, subtree: true, attributeFilter: ['data-phase'] });
      });
    } else {
      await moveTo(p, x, y);
      await sleep(150);
      mark({ kind: 'click', what: step.click, page: step.page, x, y, moment });
      await p.page.mouse.down();
      await sleep(60);
      await p.page.mouse.up();
    }
  }
  if (step.stack) {
    mark({ kind: 'stack', op: step.stack, moment });
    execFileSync(join(here, 'run-stack.sh'), [step.stack], { stdio: 'inherit' });
  }
  if (step.mode) {
    const settings = (await api('GET', '/api/v1/playout/settings')).body;
    const r = await api('PUT', '/api/v1/playout/settings', { ...settings, mode: step.mode });
    mark({ kind: 'mode', mode: step.mode, status: r.status, moment });
    if (r.status !== 200) throw new Error(`mode ${step.mode}: ${r.status}`);
  }
  if (step.until) {
    const ms = await until(step.until);
    mark({ kind: 'until', what: step.until, ms, moment });
  }
  if (step.background) {
    const bg = (async () => {
      for (const s of step.background) await run(s, moment);
    })().catch((e) => log('background step failed', e.message));
    background.push(bg);
  }
  if (step.gap) await sleep(step.gap * 1000);
}
const held = {};
const background = [];
async function stills(tag) {
  for (const p of Object.values(pages)) {
    await p.page.screenshot({ path: join(OUT, 'stills', `${tag}-${p.name}.png`) }).catch(() => {});
  }
}

// ---------------------------------------------------------------- the people
for (const [username, who, role] of [['eilidh', 'eilidh', 'presenter'], ['shona', 'shona', 'screener']]) {
  const r = await api('POST', '/api/v1/staff', { username, name: script.cast[who].name, role, password: PASSWORD });
  if (![201, 409].includes(r.status)) throw new Error(`staff ${username}: ${r.status}`);
}
const session = async (username) => (await api('POST', '/api/v1/sessions', { username, password: PASSWORD }, { 'Content-Type': 'application/json' })).body.token;
const linesOf = (who) => [...script.moments.flatMap((m) => m.steps.flatMap((s) => [s, ...(s.background ?? [])]))].filter((s) => s.line && s.who === who).map((s) => s.line);

async function invite(who, role, filmIt) {
  const inv = (await api('POST', '/api/v1/invites', { role, name: script.cast[who].name })).body;
  const name = filmIt ? who : `mic-${who}`;
  const p = await newPage(name, filmIt ? { width: 390, height: 844, phone: true, voice: true, overlay: true } : { width: 1280, height: 800, scale: 1.5, voice: true });
  await p.page.goto((/^https?:/.test(inv.link) ? inv.link : `${WEB}${inv.link}`));
  await p.page.getByRole('button', { name: 'Join the studio' }).waitFor({ timeout: 20_000 });
  log('voice', who, await p.page.evaluate((l) => window.__voice.load(l), linesOf(who)), 'lines');
  return { p, identity: JSON.parse(Buffer.from(inv.link.split('.')[1], 'base64url').toString()).id };
}
async function joinRoom(p) {
  await p.page.getByRole('button', { name: 'Join the studio' }).click();
  for (let i = 0; i < 100; i++) {
    const ph = await p.page.locator('.banner').getAttribute('data-phase').catch(() => null);
    if (ph && ph !== 'none' && ph !== 'joining') return ph;
    await sleep(200);
  }
  throw new Error(`${p.name} did not join`);
}

// The listener stream, from before the show: what listeners heard.
const stream = spawn('curl', ['-sN', '-o', join(OUT, 'audio', 'stream.ogg'), STREAM], { stdio: 'ignore' });
mark({ kind: 'stream', at: Date.now() });

// Eilidh's microphone and Finlay's, by invite; Shona's, by a screener's token.
const e = await invite('eilidh', 'presenter', false);
ids.eilidh = e.identity;
await joinRoom(e.p);
const f = await invite('finlay', 'dj', false);
ids.finlay = f.identity;
await joinRoom(f.p);
{
  const p = await newPage('mic-shona', { width: 900, height: 900, voice: true });
  await p.page.goto(`${WEB}/`);
  await p.page.addScriptTag({ path: join(root, 'apps/web/node_modules/livekit-client/dist/livekit-client.umd.js') });
  log('voice', 'shona', await p.page.evaluate((l) => window.__voice.load(l), linesOf('shona')), 'lines');
  const tok = await api('POST', '/api/v1/room/tokens', { identity: 'shona', name: script.cast.shona.name, role: 'screener' });
  if (tok.status !== 201) throw new Error(`screener token ${tok.status}`);
  ids.shona = 'shona';
  await p.page.evaluate(async ({ url, token }) => {
    const { Room } = window.LivekitClient;
    const room = new Room();
    await room.connect(url, token, { autoSubscribe: false });
    await room.localParticipant.publishTrack(window.__voice.track(), { name: 'microphone', source: 'microphone' });
    window.__room = room;
  }, { url: tok.body.url, token: tok.body.token });
}
const g = await invite('gregor', 'guest', true);
ids.gregor = g.identity;
for (const who of ['niall', 'jessie']) {
  const p = await newPage(who, { width: 390, height: 844, phone: true, voice: true, overlay: true });
  await p.page.goto(`${WEB}/`);
  await p.page.getByRole('button', { name: 'Listen live' }).waitFor({ timeout: 20_000 });
  log('voice', who, await p.page.evaluate((l) => window.__voice.load(l), linesOf(who)), 'lines');
}

// The studio app twice: Eilidh at the desk, Shona at the screener.
const desk = await newPage('desk', { overlay: true, zoom: 1.2 });
await desk.page.goto(`${STUDIO}?token=${await session('eilidh')}`);
await desk.page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 20_000 });
const shona = await newPage('shona', { overlay: true, zoom: 1.2 });
await shona.page.goto(`${STUDIO}?token=${await session('shona')}`);
await shona.page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 20_000 });
await shona.page.keyboard.press('Control+2');
await shona.page.locator('[data-panel="screener.queue"]').waitFor();

// Off air before the show (a presenter is on air from the moment they join), and the
// delay full: quiet passages rebuild it at twice their speed.
await api('POST', '/api/v1/desk/commands', { op: 'on_air', identity: ids.eilidh, on: false });
for (let i = 0; i < 120; i++) {
  const v = await view();
  if (v.desk?.delay && !v.desk.delay.rebuilding && v.desk.playout?.connected && v.seats?.screener) break;
  await sleep(500);
}
dumpsBefore = (await view()).desk?.delay?.dumps ?? 0;
await monitor('studio');
// Niall listens on his phone before he calls.
await pages.niall.page.getByRole('button', { name: 'Listen live' }).tap();
await sleep(4000);
await stills('pre');

for (const p of [desk, shona, g.p, pages.niall]) await film(p);
await sleep(1500);
mark({ kind: 'roll' });
log('rolling');

// The booths: a monitor joins each caller's booth as soon as the page knows who it is.
const boothWatch = setInterval(async () => {
  for (const who of ['niall', 'jessie']) {
    if (ids[who] || ids[`_${who}`]) continue;
    const call = await pages[who].page.evaluate(() => JSON.parse(localStorage.getItem('weavesignal.call') ?? 'null')).catch(() => null);
    if (call?.identity) {
      ids[`_${who}`] = true;
      ids[who] = call.identity;
      mark({ kind: 'identity', who, identity: call.identity });
      monitor(`booth-${who}`, call.identity).catch((err) => log('booth monitor', err.message));
    }
  }
}, 150);

let failed = null;
try {
  for (const m of script.moments) {
    mark({ kind: 'moment', id: m.id, title: m.title, caption: m.caption });
    log('moment', m.id);
    for (const s of m.steps) await run(s, m.id);
    await Promise.all(background.splice(0));
    await stills(m.id);
    save();
  }
} catch (err) {
  failed = err;
  log('FAILED', err.message);
  await stills('failed');
}
mark({ kind: 'end' });
await sleep(6000); // the stream runs on behind the desk
clearInterval(boothWatch);
for (const c of casts) {
  await c.cdp.send('Page.stopScreencast').catch(() => {});
  c.index.end();
  log('frames', c.p.name, c.frames());
}
await stopMonitors();
await sleep(500);
stream.kill('SIGTERM');
mark({ kind: 'stopped' });
timeline.ids = ids;
timeline.dumpsBefore = dumpsBefore;
for (const ev of timeline.events) delete ev.done;
save();
await browser.close();
log(failed ? 'failed' : 'done', OUT);
process.exit(failed ? 1 : 0);
