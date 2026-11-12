// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// How much of a guest's return their browser conceals, on the M3 loopback stack
// (tests/m3/run-join.sh): an invited guest joins in Chromium with a fake microphone, hears the
// synthetic presenter's tone through their return, and the inbound audio's figures are read
// every 5 s: packets, loss, and the share of the interval's samples concealed while sound was
// due (`gap`, as tests/g0/phone-stats.mjs reads it on a phone). A clean return conceals next to
// nothing; the desk's uneven timestamps (fixed 2026-11-03) made it 26-44 %.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core SECONDS=60 node tests/m3/return-stats.mjs
//   (a station: WEB=https://station.example CONTROL=https://station.example ADMIN_TOKEN=... SPEAKER=1)
const { chromium } = await import(process.env.PLAYWRIGHT_CORE + '/index.mjs');
const WEB = process.env.WEB ?? 'http://127.0.0.1:32430';
const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:32310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'm3-admin-test-only';
const SECONDS = Number(process.env.SECONDS ?? 60);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const admin = { Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' };

const invite = async (name) => (await (await fetch(`${CONTROL}/api/v1/invites`, { method: 'POST', headers: admin, body: JSON.stringify({ role: 'guest', name }) })).json()).link;
const browser = await chromium.launch({
  args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
});
async function onAir(page, link) {
  await page.goto(/^https?:/.test(link) ? link : `${WEB}${link}`);
  await page.getByRole('button', { name: 'Join the studio' }).click();
  const t0 = Date.now();
  while ((await page.locator('.banner').getAttribute('data-phase').catch(() => null)) !== 'on_air') {
    if (Date.now() - t0 > 20_000) { console.log('a guest never reached on air'); process.exit(1); }
    await sleep(200);
  }
}
// SPEAKER=1: a second guest whose fake microphone (Chromium's beeps) is what the measured guest
// hears, for a station with nobody else at the desk (the off-site test station).
let speaker = null;
if (process.env.SPEAKER) {
  speaker = await browser.newContext({ permissions: ['microphone'] });
  await onAir(await speaker.newPage(), await invite('Iona Campbell'));
}
const ctx = await browser.newContext({ permissions: ['microphone'] });
await ctx.addInitScript(() => {
  const Native = window.RTCPeerConnection;
  window.__pcs = [];
  window.RTCPeerConnection = class extends Native { constructor(...a) { super(...a); window.__pcs.push(this); } };
});
const g = await ctx.newPage();
await onAir(g, await invite('Gregor Stewart'));
await sleep(3000); // past the jitter buffer's start

const read = () => g.evaluate(async () => {
  let r0 = null;
  for (const pc of window.__pcs) {
    if (pc.connectionState === 'closed') continue;
    (await pc.getStats()).forEach((r) => {
      if (r.type === 'inbound-rtp' && r.kind === 'audio' && r.packetsReceived > 0) r0 = { recv: r.packetsReceived, lost: r.packetsLost, conceal: r.concealedSamples, silent: r.silentConcealedSamples ?? 0, events: r.concealmentEvents, total: r.totalSamplesReceived, level: r.audioLevel };
    });
  }
  return r0;
});
const gaps = [];
let was = await read();
for (let t = 0; t < SECONDS; t += 5) {
  await sleep(5000);
  const r = await read();
  if (was && r && r.total > was.total) {
    const gap = ((r.conceal - was.conceal) - (r.silent - was.silent)) / (r.total - was.total);
    gaps.push(gap);
    console.log(new Date().toISOString().slice(11, 19), `packets ${r.recv - was.recv} lost ${r.lost - was.lost} events ${r.events - was.events} level ${r.level?.toFixed(3)} gap ${(gap * 100).toFixed(1)} %`);
  }
  was = r;
}
gaps.sort((a, b) => a - b);
const med = gaps.length ? gaps[Math.floor(gaps.length / 2)] : NaN;
console.log(`median concealed while sound was due: ${(med * 100).toFixed(1)} % over ${gaps.length} intervals (max ${(gaps.at(-1) * 100).toFixed(1)} %)`);
await browser.close();
