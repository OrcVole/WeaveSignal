// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A browser's camera for the desk's stack test TEST=browsercam (tests/room/run-stack.sh): headless
// Chromium with a fake microphone and camera publishes both into the room as the join page does
// (livekit-client, the page's CAMERA_PUBLISH: AV1 with VP8 as the backup codec, one layer), so
// the desk receives what Chromium really sends (the talk station's Brave fault, 2026-11-11: the
// desk had no AV1 depayloader and resubscribed that person three times a second, and its AV1
// decoder stopped on the two picture sizes of Chromium's default SVC).
//
// It waits for the Rust test to write $RUN/browser-join.json ({ url, token }), joins, writes what
// it sends to $RUN/browser-camera.json (the outbound codec from getStats), and stays until
// $RUN/browser-leave appears or ten minutes pass.
//
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core RUN=tests/room/run node tests/room/browser-camera.mjs
import { createRequire } from 'node:module';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const RUN = process.env.RUN ?? join(here, 'run');
const UMD = join(here, '../../apps/web/node_modules/livekit-client/dist/livekit-client.umd.js');
const joinFile = join(RUN, 'browser-join.json');
const leaveFile = join(RUN, 'browser-leave');
const outFile = join(RUN, 'browser-camera.json');
// The page's own settings, from the page's own module (Node strips its types): what is published
// here is what the join page publishes.
const { CAMERA_PUBLISH, cameraCapture } = await import(join(here, '../../apps/web/src/lib/rules.ts'));
const CAPTURE = cameraCapture();

const log = (...a) => console.log(new Date().toISOString().slice(11, 23), 'browser-camera:', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

for (const f of [joinFile, leaveFile, outFile]) rmSync(f, { force: true });
const t0 = Date.now();
while (!existsSync(joinFile)) {
  if (Date.now() - t0 > 600_000) {
    log('no join file after ten minutes');
    process.exit(1);
  }
  await sleep(200);
}
await sleep(100);
const { url, token } = JSON.parse(readFileSync(joinFile, 'utf8'));
log('joining', url);

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
});
const page = await browser.newPage();
page.on('console', (m) => log('page:', m.text()));
// A page on a loopback origin (a secure context, so getUserMedia works), served from here.
const origin = 'http://127.0.0.1:1/';
await page.route(`${origin}**`, (r) => r.fulfill({ contentType: 'text/html', body: '<!doctype html><title>camera</title>' }));
await page.goto(origin);
await page.addScriptTag({ path: UMD });
const sent = await page.evaluate(
  async ({ url, token, publish, capture }) => {
    const { Room, Track, createLocalAudioTrack, createLocalVideoTrack } = window.LivekitClient;
    const room = new Room({ adaptiveStream: false, dynacast: false });
    window.room = room;
    await room.connect(url, token, { autoSubscribe: false });
    const mic = await createLocalAudioTrack();
    await room.localParticipant.publishTrack(mic, { source: Track.Source.Microphone, dtx: false, red: true });
    const cam = await createLocalVideoTrack(capture);
    await room.localParticipant.publishTrack(cam, { source: Track.Source.Camera, ...publish });
    // What the camera's sender really sends, once it has sent something.
    for (let i = 0; i < 50; i++) {
      await new Promise((r) => setTimeout(r, 200));
      const stats = await cam.sender.getStats();
      const out = [...stats.values()].find((s) => s.type === 'outbound-rtp' && s.kind === 'video' && s.bytesSent > 0);
      if (out) {
        const codec = stats.get(out.codecId);
        return { codec: codec?.mimeType ?? null, encoder: out.encoderImplementation ?? null, scalability: out.scalabilityMode ?? null };
      }
    }
    return { codec: null };
  },
  { url, token, publish: CAMERA_PUBLISH, capture: CAPTURE },
);
log('sending', JSON.stringify(sent));
writeFileSync(outFile, JSON.stringify(sent));
while (!existsSync(leaveFile) && Date.now() - t0 < 600_000) await sleep(200);
await page.evaluate(() => window.room.disconnect()).catch(() => {});
await browser.close();
log('left');
