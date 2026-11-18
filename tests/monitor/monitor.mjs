// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The monitor link's sound, driven (tests/monitor/run-monitor.sh brings the stack up): host A's
// monitor page is headless Chromium with livekit-client, joined with the token the control plane
// gives the monitor secret. Two hosts' software send tones (A 1 kHz, B 2 kHz) over Icecast; the
// page measures what the monitor mix carries with a Goertzel filter on the received track, and
// reads the receiver's own figures (`getStats`: concealedSamples minus silentConcealedSamples
// over totalSamplesReceived, as tests/m3/return-stats.mjs does for a caller's return).
import { createRequire } from 'node:module';
import { createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE);
const here = dirname(fileURLToPath(import.meta.url));
const UMD = join(here, '../../apps/web/node_modules/livekit-client/dist/livekit-client.umd.js');
const { CONTROL, ADMIN_TOKEN, RUN, LK_URL, LK_API_KEY, LK_API_SECRET } = process.env;
const SECONDS = Number(process.env.MONITOR_SECONDS ?? 30);
const keys = JSON.parse(readFileSync(join(RUN, 'keys.json'), 'utf8'));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);
let failed = 0;
const check = (ok, what) => {
  console.log(ok ? '   ok:' : '   FAIL:', what);
  if (!ok) failed++;
};

const staff = (extra = {}) => ({ Authorization: `Bearer ${ADMIN_TOKEN}`, 'Content-Type': 'application/json', ...extra });
const admin = async (method, path, body) => {
  const r = await fetch(`${CONTROL}${path}`, { method, headers: staff(), body: body ? JSON.stringify(body) : undefined });
  return { code: r.status, body: await r.json().catch(() => null) };
};
const asHost = async (who, method, path) => {
  const r = await fetch(`${CONTROL}${path}`, { method, headers: { Authorization: `Bearer ${keys[who].monitor}` } });
  return { code: r.status, body: await r.json().catch(() => null) };
};
const press = async (who, route, body) => {
  const r = await fetch(`${CONTROL}/api/v1/monitor/${route}`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${keys[who].monitor}`, 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { code: r.status, body: await r.json().catch(() => null) };
};
// The monitor link's event stream, read frame by frame.
async function events(who) {
  const ac = new AbortController();
  const r = await fetch(`${CONTROL}/api/v1/monitor/events`, { headers: { Authorization: `Bearer ${keys[who].monitor}` }, signal: ac.signal });
  const reader = r.body?.getReader();
  const dec = new TextDecoder();
  let buf = '';
  return {
    code: r.status,
    // The next whole frame as { event, data }, or null after `ms`.
    async next(ms = 5000) {
      const until = Date.now() + ms;
      for (;;) {
        const at = buf.indexOf('\n\n');
        if (at >= 0) {
          const frame = buf.slice(0, at);
          buf = buf.slice(at + 2);
          const event = /^event: (.*)$/m.exec(frame)?.[1];
          const data = /^data: (.*)$/m.exec(frame)?.[1];
          if (!event) continue; // a keep-alive comment
          return { event, data: data ? JSON.parse(data) : null };
        }
        const left = until - Date.now();
        if (left <= 0) return null;
        const got = await Promise.race([reader.read(), sleep(left).then(() => 'late')]);
        if (got === 'late' || got.done) return null;
        buf += dec.decode(got.value);
      }
    },
    close: () => ac.abort(),
  };
}
const waitFor = async (what, secs, fn) => {
  const t0 = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - t0 > secs * 1000) throw new Error(`timed out: ${what}`);
    await sleep(300);
  }
};
const idOf = (who) => `source-${keys[who].key_id}`;
const desk = (op, identity, on, to) => admin('POST', '/api/v1/desk/commands', { op, identity, on, ...(to ? { to } : {}) });

// A browser page that joins a room and measures what it hears.
const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
});
async function listener(token, url = LK_URL) {
  const ctx = await browser.newContext();
  await ctx.addInitScript(() => {
    const Native = window.RTCPeerConnection;
    window.__pcs = [];
    window.RTCPeerConnection = class extends Native {
      constructor(...a) {
        super(...a);
        window.__pcs.push(this);
      }
    };
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => log('page error:', e.message));
  const origin = 'http://127.0.0.1:1/';
  await page.route(`${origin}**`, (r) => r.fulfill({ contentType: 'text/html', body: '<!doctype html><title>monitor</title>' }));
  await page.goto(origin);
  await page.addScriptTag({ path: UMD });
  await page.evaluate(
    async ({ url, token }) => {
      const { Room, RoomEvent, Track } = window.LivekitClient;
      const room = new Room({ adaptiveStream: false, dynacast: false });
      window.__room = room;
      window.__subscribed = 0;
      window.__sample = { frames: 0, p1000: 0, p2000: 0, energy: 0 };
      // The Goertzel power of a Hann-windowed frame at one frequency.
      const goertzel = (x, hz, rate) => {
        const w = (2 * Math.PI * hz) / rate;
        const c = 2 * Math.cos(w);
        let s1 = 0, s2 = 0;
        for (let i = 0; i < x.length; i++) {
          const hann = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (x.length - 1));
          const s = x[i] * hann + c * s1 - s2;
          s2 = s1;
          s1 = s;
        }
        return (s1 * s1 + s2 * s2 - c * s1 * s2) / (x.length * x.length);
      };
      room.on(RoomEvent.TrackSubscribed, (track) => {
        if (track.kind !== Track.Kind.Audio) return;
        window.__subscribed++;
        const el = track.attach();
        el.muted = false;
        el.volume = 0.01;
        document.body.append(el);
        el.play().catch(() => {});
        const ac = new AudioContext();
        ac.resume();
        const src = ac.createMediaStreamSource(new MediaStream([track.mediaStreamTrack]));
        const an = ac.createAnalyser();
        an.fftSize = 2048;
        src.connect(an);
        const buf = new Float32Array(an.fftSize);
        setInterval(() => {
          an.getFloatTimeDomainData(buf);
          const s = window.__sample;
          s.frames++;
          s.p1000 += goertzel(buf, 1000, ac.sampleRate);
          s.p2000 += goertzel(buf, 2000, ac.sampleRate);
          s.energy += buf.reduce((a, v) => a + v * v, 0) / buf.length;
        }, 30);
      });
      await room.connect(url, token, { autoSubscribe: true });
    },
    { url, token },
  );
  const l = {
    page,
    ctx,
    subscribed: () => page.evaluate(() => window.__subscribed),
    roomState: () => page.evaluate(() => window.__room.state),
    // dB of each tone's power averaged over `ms`, and the signal's energy.
    async hear(ms) {
      await page.evaluate(() => (window.__sample = { frames: 0, p1000: 0, p2000: 0, energy: 0 }));
      await sleep(ms);
      const s = await page.evaluate(() => window.__sample);
      const db = (p) => 10 * Math.log10(Math.max(p / Math.max(s.frames, 1), 1e-20));
      return { frames: s.frames, own: db(s.p1000), other: db(s.p2000), energy: 10 * Math.log10(Math.max(s.energy / Math.max(s.frames, 1), 1e-20)) };
    },
    stats: () =>
      page.evaluate(async () => {
        let r0 = null;
        for (const pc of window.__pcs) {
          if (pc.connectionState === 'closed') continue;
          (await pc.getStats()).forEach((r) => {
            if (r.type === 'inbound-rtp' && r.kind === 'audio' && r.packetsReceived > 0) {
              r0 = { recv: r.packetsReceived, lost: r.packetsLost, conceal: r.concealedSamples, silent: r.silentConcealedSamples ?? 0, events: r.concealmentEvents, total: r.totalSamplesReceived };
            }
          });
        }
        return r0;
      }),
    close: () => ctx.close(),
  };
  return l;
}

// A token for a seat that is not the link's, signed as LiveKit's own key would: no recorder grant.
const jwt = (claims) => {
  const b = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
  const input = `${b({ alg: 'HS256', typ: 'JWT' })}.${b(claims)}`;
  return `${input}.${createHmac('sha256', LK_API_SECRET).update(input).digest('base64url')}`;
};

try {
  log('1. both hosts connect and stand by');
  await waitFor('both sources standing by', 40, async () => {
    const { body } = await admin('GET', '/api/v1/sources');
    return ['a', 'b'].every((w) => body?.sources?.some((s) => s.key_id === keys[w].key_id && s.state === 'standby'));
  });
  // Host A goes on air from their own monitor link (Go on air now: the show's time has come and
  // nobody else is on air); host B, whose show is later, is put on air by the desk.
  let pressed = null;
  await waitFor('host A on air by their own press', 30, async () => {
    const a = await press('a', 'go');
    if (a.code === 200) pressed = a;
    return a.code === 200 || (await asHost('a', 'GET', '/api/v1/monitor')).body?.state === 'on_air';
  });
  check(pressed?.code === 200 && pressed.body.done === 'go' && pressed.body.view.state === 'on_air', "host A is on air, by their own press on the monitor link: the press answered 200 with A's view");
  const goes = (await admin('GET', '/api/v1/sources')).body?.sources?.find((s) => s.key_id === keys.a.key_id);
  check(goes?.state === 'on_air', "the desk's own list agrees");
  check((await press('b', 'go')).code === 409, "host B's Go on air now is refused: their show has not started, and A holds the air");
  check((await desk('on_air', idOf('b'), true)).code === 200, 'host B is put on air by the desk');
  const view0 = await asHost('a', 'GET', '/api/v1/monitor');
  check(view0.code === 200 && view0.body.monitor_open === false, `no one listens, so no monitor mix is open: ${JSON.stringify(view0.body)}`);

  log('2. the monitor token: two seats, a third refused');
  const t1 = await asHost('a', 'POST', '/api/v1/monitor/token');
  const t2 = await asHost('a', 'POST', '/api/v1/monitor/token');
  const t3 = await asHost('a', 'POST', '/api/v1/monitor/token');
  check(t1.code === 201 && t2.code === 201, 'two browsers may listen');
  check(t3.code === 409, `a third is refused (${t3.code}: ${t3.body?.error})`);
  check(t1.body.room === `studio-monitor-${keys.a.key_id}` && t1.body.identity === `monitor-${keys.a.key_id}-1`, `the room and seat: ${t1.body.room} ${t1.body.identity}`);
  check((await fetch(`${CONTROL}/api/v1/monitor/token`, { method: 'POST', headers: { Authorization: `Bearer ${keys.b.monitor}` } })).status === 201, "host B's own link gives B a token of their own");

  log('3. host A listens; the desk makes the monitor mix');
  const first = await listener(t1.body.token, t1.body.url);
  await waitFor("the monitor's audio track", 45, async () => (await first.subscribed()) > 0);
  check(true, 'the audio track reached the page');
  await waitFor('the monitor mix open at the desk', 15, async () => (await asHost('a', 'GET', '/api/v1/monitor')).body?.monitor_open === true);
  check(true, 'the host\'s view says the monitor mix is open');
  await sleep(4000); // the jitter buffer, the leveller, the ring
  const second = await listener(t2.body.token, t2.body.url);
  await waitFor("the second browser's audio", 30, async () => (await second.subscribed()) > 0);
  check(true, 'a second browser hears it too');

  log('4. both on air: the other host and never your own');
  let h = await first.hear(3000);
  log(`   1 kHz (own) ${h.own.toFixed(1)} dB, 2 kHz (other) ${h.other.toFixed(1)} dB, level ${h.energy.toFixed(1)} dB`);
  const full = h.other;
  check(h.frames > 20 && h.other - h.own > 30, `the monitor carries host B's tone and not host A's own (${(h.other - h.own).toFixed(0)} dB apart)`);
  check(full > -60, `and it is a real signal (${full.toFixed(1)} dB)`);
  const second2 = await second.hear(2000);
  check(second2.other - second2.own > 30, 'the second browser hears the same');

  log(`5. concealment over ${SECONDS} s of listening`);
  const gaps = [];
  let was = await first.stats();
  const startTotals = was;
  for (let t = 0; t < SECONDS; t += 5) {
    await sleep(5000);
    const r = await first.stats();
    if (was && r && r.total > was.total) {
      const gap = (r.conceal - was.conceal - (r.silent - was.silent)) / (r.total - was.total);
      gaps.push(gap);
      log(`   packets ${r.recv - was.recv} lost ${r.lost - was.lost} events ${r.events - was.events} gap ${(gap * 100).toFixed(1)} %`);
    }
    was = r;
  }
  const endTotals = was;
  const overall = startTotals && endTotals ? (endTotals.conceal - startTotals.conceal - (endTotals.silent - startTotals.silent)) / (endTotals.total - startTotals.total) : NaN;
  gaps.sort((a, b) => a - b);
  log(`   overall concealed while sound was due: ${(overall * 100).toFixed(2)} % over ${gaps.length} intervals (max ${(gaps.at(-1) * 100).toFixed(1)} %)`);
  check(gaps.length >= SECONDS / 5 - 1 && overall < 0.02 && (gaps.at(-1) ?? 1) < 0.05, 'the receiver conceals near nothing: every block stamped from its place in the mix');

  log('6. cue: host B off air, cued');
  await desk('on_air', idOf('b'), false);
  await desk('cue', idOf('b'), true);
  await sleep(2000);
  h = await first.hear(3000);
  log(`   1 kHz ${h.own.toFixed(1)} dB, 2 kHz ${h.other.toFixed(1)} dB`);
  check(h.other > full - 12 && h.other - h.own > 30, 'cued, host B is in the monitor');
  await desk('cue', idOf('b'), false);
  await sleep(2000);
  h = await first.hear(3000);
  log(`   uncued: 1 kHz ${h.own.toFixed(1)} dB, 2 kHz ${h.other.toFixed(1)} dB`);
  check(h.other < full - 40, 'off air and uncued, host B is gone from the monitor');

  log('7. talk addressed to host A');
  check((await desk('talk_to', idOf('b'), true, idOf('a'))).code === 200, 'host B holds talk to host A');
  await sleep(2000);
  h = await first.hear(3000);
  log(`   1 kHz ${h.own.toFixed(1)} dB, 2 kHz ${h.other.toFixed(1)} dB`);
  check(h.other > full - 12 && h.other - h.own > 30, 'talk addressed to host A is in their monitor');
  await desk('talk_to', idOf('b'), false, idOf('a'));
  await sleep(2000);
  h = await first.hear(3000);
  check(h.other < full - 40, 'and gone when it is let go');

  log('8. a stranger in the monitor room hears nothing (ADR-0118)');
  const now = Math.floor(Date.now() / 1000);
  const stranger = jwt({
    iss: LK_API_KEY,
    sub: `monitor-${keys.a.key_id}-3`,
    nbf: now - 10,
    exp: now + 300,
    name: 'stranger',
    video: { room: t1.body.room, roomJoin: true, canPublish: false, canSubscribe: true, canPublishData: false },
    attributes: { 'weavesignal.role': 'listener' },
  });
  const s = await listener(stranger);
  await sleep(10000);
  const heard = await s.stats();
  check((await s.subscribed()) === 0 && !heard, `a third seat joined with LiveKit's own key subscribes to nothing (tracks ${await s.subscribed()}, audio ${JSON.stringify(heard)})`);
  await s.close();

  log('9. the mix goes when the last listener does');
  await first.close();
  await second.close();
  await waitFor('the monitor mix closed at the desk', 45, async () => (await asHost('a', 'GET', '/api/v1/monitor')).body?.monitor_open === false);
  check(true, 'no listener, no monitor mix');
  // A seat is free again once its page has gone.
  const again = await asHost('a', 'POST', '/api/v1/monitor/token');
  check(again.code === 201, `a seat is free again after both pages left (${again.code})`);

  log('10. the handover from the monitor link');
  // B is off air and uncued here; A is on air, with Heather (B) standing by after them.
  await waitFor("host A's view names the next host", 20, async () => (await asHost('a', 'GET', '/api/v1/monitor')).body?.next_host?.name === 'Heather Douglas');
  const mine = (await asHost('a', 'GET', '/api/v1/monitor')).body;
  check(mine.state === 'on_air' && mine.controls.bring.may && mine.controls.cue.may && mine.controls.end_show.may, `A on air with B after them may cue, bring on and end: ${JSON.stringify(mine.controls)}`);
  check(!mine.controls.hand_over.may && /Bring Heather Douglas on air first/.test(mine.controls.hand_over.why_not), 'the second press waits for the first');
  check(mine.rundown?.after?.show_name === 'Kettle Drive' && mine.rundown?.after?.host === 'Heather Douglas', `after you: ${JSON.stringify(mine.rundown?.after)}`);
  const stream = await events('a');
  check(stream.code === 200, 'the event stream opens with the host secret in the header');
  const firstFrame = await stream.next(5000);
  check(firstFrame?.event === 'view' && firstFrame.data.state === 'on_air' && firstFrame.data.station?.now_ms > 0, 'the stream says the view at once');
  check((await press('a', 'handover', { step: 'hand_over' })).code === 409, 'the second press first is refused');
  const bring = await press('a', 'handover', { step: 'bring' });
  check(bring.code === 200 && bring.body.view.next_host?.state === 'on_air', `the first press brings B on air beside A: ${bring.code}`);
  let seen = null;
  for (let i = 0; i < 6 && !seen; i++) {
    const f = await stream.next(3000);
    if (f?.event === 'view' && f.data.controls.hand_over.may) seen = f.data;
  }
  check(!!seen, 'the stream carries it: the second press is on offer');
  const both = (await admin('GET', '/api/v1/sources')).body.sources;
  check(both.find((x) => x.key_id === keys.a.key_id)?.state === 'on_air' && both.find((x) => x.key_id === keys.b.key_id)?.state === 'on_air', 'both are on air at the desk: the overlap');
  const handed = await press('a', 'handover', { step: 'hand_over' });
  check(handed.code === 200 && handed.body.view.state === 'standby', `the second press takes A off air: ${handed.code}`);
  const after = (await admin('GET', '/api/v1/sources')).body.sources;
  check(after.find((x) => x.key_id === keys.a.key_id)?.state === 'standby' && after.find((x) => x.key_id === keys.b.key_id)?.state === 'on_air', 'B holds the air, A is standing by');
  check((await press('b', 'end-show')).code === 200, 'B ends their show from their own link');
  await waitFor('B off air at the desk', 10, async () => (await admin('GET', '/api/v1/sources')).body.sources.find((x) => x.key_id === keys.b.key_id)?.state === 'standby');
  check(true, 'B is off air: End my show, for B alone');
  stream.close();

  log('11. a key ends: its page is taken out of the monitor room');
  const tk = await asHost('a', 'POST', '/api/v1/monitor/token');
  check(tk.code === 201, `a seat for a new page (${tk.code})`);
  const page = await listener(tk.body.token, tk.body.url);
  await waitFor('the page in the monitor room', 45, async () => (await page.subscribed()) > 0);
  check((await page.roomState()) === 'connected', 'the page is in the monitor room');
  check((await admin('DELETE', `/api/v1/shows/steeped/keys/${keys.a.key_id}`)).code === 200, "host A's key ends");
  await waitFor('the page out of the room', 20, async () => (await page.roomState()) === 'disconnected');
  check(true, 'the seat was removed from the room, not only refused a new token');
  check((await asHost('a', 'GET', '/api/v1/monitor')).code === 410, 'and the link says so');
  await page.close();
} catch (e) {
  console.log('   FAIL:', e.message);
  failed++;
}
await browser.close();
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
