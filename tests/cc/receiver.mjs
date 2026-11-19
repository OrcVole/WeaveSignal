// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A guest's browser for the congestion-control measurement (tests/cc/run-cc.sh): headless Chromium
// with a fake microphone joins as the join page does (livekit-client, one connection, no one may
// subscribe to its own track), hears only its own return from the desk (`desk-mm-<identity>`)
// and, with VIDEO=1, watches the desk's programme picture (`desk-pgm`). Every INTERVAL seconds it
// prints one JSON line of what Chromium's `getStats` says about the return (packets, loss,
// concealment as tests/m3/return-stats.mjs reads it, jitter) and the picture (bytes, frames per
// second, size, freezes, NACKs, PLIs), then a summary line.
//
//   PLAYWRIGHT_CORE=... LIVEKIT_UMD=.../livekit-client.umd.js URL=ws://... TOKEN=... \
//     IDENTITY=gregor SECONDS=60 VIDEO=1 node tests/cc/receiver.mjs
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const { URL: LK_URL, TOKEN, IDENTITY = 'gregor', LIVEKIT_UMD: UMD } = process.env;
const SECONDS = Number(process.env.SECONDS ?? 60);
const INTERVAL = Number(process.env.INTERVAL ?? 5);
const VIDEO = process.env.VIDEO === '1';
// With the picture on, the guest also sends a camera (Chromium's fake device: moving content), so the desk's programme picture is not a still slate.
const DESK = process.env.DESK_IDENTITY ?? 'desk';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.launch({
  args: [...(process.env.CAMERA_FILE ? [`--use-file-for-fake-video-capture=${process.env.CAMERA_FILE}`] : []), '--no-sandbox', '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
});
const page = await browser.newPage();
page.on('console', (m) => console.error('page:', m.text()));
const origin = 'http://127.0.0.1:1/';
await page.route(`${origin}**`, (r) => r.fulfill({ contentType: 'text/html', body: '<!doctype html><title>cc</title>' }));
await page.addInitScript(() => {
  const Native = window.RTCPeerConnection;
  window.__pcs = [];
  window.RTCPeerConnection = class extends Native {
    constructor(...a) {
      super(...a);
      window.__pcs.push(this);
    }
  };
});
await page.goto(origin);
await page.addScriptTag({ path: UMD });
const joined = await page.evaluate(
  async ({ url, token, hear, see, camera }) => {
    const { Room, RoomEvent, Track, createLocalAudioTrack, createLocalVideoTrack } = window.LivekitClient;
    const room = new Room({ adaptiveStream: false, dynacast: false });
    window.room = room;
    const take = (pub, who) => {
      if (pub.kind === Track.Kind.Audio && who.identity === hear) pub.setSubscribed(true);
      if (pub.kind === Track.Kind.Video && who.identity === see) pub.setSubscribed(true);
    };
    room.on(RoomEvent.TrackPublished, take).on(RoomEvent.TrackSubscribed, (track) => {
      const el = track.attach();
      el.muted = track.kind === 'video';
      document.body.appendChild(el);
    });
    await room.connect(url, token, { autoSubscribe: false });
    room.localParticipant.setTrackSubscriptionPermissions(false, []);
    const mic = await createLocalAudioTrack({ echoCancellation: false, noiseSuppression: false, autoGainControl: false });
    await room.localParticipant.publishTrack(mic, { source: Track.Source.Microphone, dtx: false, red: true });
    if (camera) {
      const cam = await createLocalVideoTrack({ resolution: { width: 640, height: 360, frameRate: 25 } });
      await room.localParticipant.publishTrack(cam, { source: Track.Source.Camera, videoCodec: 'vp8', simulcast: false, videoEncoding: { maxBitrate: 800_000, maxFramerate: 25 } });
    }
    for (const who of room.remoteParticipants.values()) for (const pub of who.trackPublications.values()) take(pub, who);
    return true;
  },
  { url: LK_URL, token: TOKEN, hear: `${DESK}-mm-${IDENTITY}`, see: VIDEO ? `${DESK}-pgm` : '', camera: VIDEO },
);
if (!joined) process.exit(1);

// What every receiving connection says about the return and the picture, cumulative.
const read = () =>
  page.evaluate(async () => {
    const out = { audio: null, video: null };
    for (const pc of window.__pcs) {
      if (pc.connectionState === 'closed') continue;
      (await pc.getStats()).forEach((r) => {
        if (r.type !== 'inbound-rtp') return;
        if (r.kind === 'audio' && r.packetsReceived > 0)
          out.audio = { recv: r.packetsReceived, lost: r.packetsLost, bytes: r.bytesReceived, jitter: r.jitter, conceal: r.concealedSamples, silent: r.silentConcealedSamples ?? 0, events: r.concealmentEvents, total: r.totalSamplesReceived, energy: r.totalAudioEnergy ?? 0, level: r.audioLevel ?? 0, jbDelay: r.jitterBufferDelay, jbEmitted: r.jitterBufferEmittedCount };
        if (r.kind === 'video' && r.packetsReceived > 0)
          out.video = { recv: r.packetsReceived, lost: r.packetsLost, bytes: r.bytesReceived, decoded: r.framesDecoded, dropped: r.framesDropped, w: r.frameWidth, h: r.frameHeight, fps: r.framesPerSecond ?? 0, freezes: r.freezeCount ?? 0, nack: r.nackCount ?? 0, pli: r.pliCount ?? 0, fir: r.firCount ?? 0 };
      });
    }
    return out;
  });

const t0 = Date.now();
while (!(await read()).audio) {
  if (Date.now() - t0 > 30_000) {
    console.log(JSON.stringify({ error: 'no return heard in 30 s' }));
    await browser.close();
    process.exit(2);
  }
  await sleep(250);
}
await sleep(3000); // past the jitter buffer's start
console.log(JSON.stringify({ ready: true }));
let was = await read();
const rows = [];
for (let t = 0; t < SECONDS; t += INTERVAL) {
  await sleep(INTERVAL * 1000);
  const r = await read();
  const row = { t: t + INTERVAL, phase: process.env.PHASE_FILE && existsSync(process.env.PHASE_FILE) ? 'after' : 'before' };
  if (was.audio && r.audio && r.audio.total > was.audio.total) {
    const a = r.audio, b = was.audio;
    row.audio = {
      kbps: Math.round(((a.bytes - b.bytes) * 8) / INTERVAL / 1000),
      packets: a.recv - b.recv,
      lost: a.lost - b.lost,
      events: a.events - b.events,
      gap: ((a.conceal - b.conceal) - (a.silent - b.silent)) / (a.total - b.total),
      energy: +(a.energy - b.energy).toFixed(4),
      level: +a.level.toFixed(3),
      jitter_ms: Math.round(a.jitter * 1000),
      jb_ms: Math.round(((a.jbDelay - b.jbDelay) / Math.max(1, a.jbEmitted - b.jbEmitted)) * 1000),
    };
  }
  if (VIDEO && was.video && r.video) {
    const v = r.video, b = was.video;
    row.video = {
      kbps: Math.round(((v.bytes - b.bytes) * 8) / INTERVAL / 1000),
      fps: Math.round((v.decoded - b.decoded) / INTERVAL),
      size: `${v.w}x${v.h}`,
      lost: v.lost - b.lost,
      freezes: v.freezes - b.freezes,
      nack: v.nack - b.nack,
      pli: v.pli - b.pli,
      dropped: v.dropped - b.dropped,
    };
  }
  rows.push(row);
  console.log(JSON.stringify(row));
  was = r;
}
const med = (xs) => (xs.length ? xs.slice().sort((a, b) => a - b)[Math.floor(xs.length / 2)] : null);
const aud = rows.map((r) => r.audio).filter(Boolean);
const vid = rows.map((r) => r.video).filter(Boolean);
console.log(
  JSON.stringify({
    summary: {
      audio: aud.length && { intervals: aud.length, kbps_median: med(aud.map((a) => a.kbps)), gap_median: med(aud.map((a) => a.gap)), gap_max: Math.max(...aud.map((a) => a.gap)), lost: aud.reduce((s, a) => s + a.lost, 0), packets: aud.reduce((s, a) => s + a.packets, 0) },
      video: vid.length && { intervals: vid.length, kbps_median: med(vid.map((v) => v.kbps)), kbps_min: Math.min(...vid.map((v) => v.kbps)), kbps_max: Math.max(...vid.map((v) => v.kbps)), fps_median: med(vid.map((v) => v.fps)), sizes: [...new Set(vid.map((v) => v.size))], freezes: vid.reduce((s, v) => s + v.freezes, 0), nack: vid.reduce((s, v) => s + v.nack, 0), pli: vid.reduce((s, v) => s + v.pli, 0) },
    },
  }),
);
await page.evaluate(() => window.room.disconnect()).catch(() => {});
await browser.close();
