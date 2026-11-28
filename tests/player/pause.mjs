// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The player's main button on a station with a picture, in a real browser, without a stack: the
// player as it ships (apps/web/dist), a station that is only answers, and a short HLS stream
// (AV1 and Opus in fMP4) made here with ffmpeg and played by the page's own hls.js.
//
// The page knows that it plays from the media element's own events, and detaching hls.js drops a
// `pause` event that is still queued. So it checks that the button always says what a press will
// do: play, Pause, play again, with the picture and with Audio only; and with a stream that does
// not start (every segment answers 404), then does.
//
// What it does not prove: anything about a real station, a live playlist, a key, Firefox (unless
// run there) or Safari.
//
//   cd apps/web && pnpm build
//   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/player/pause.mjs
//   just test-player-pause          # the same, with PLAYWRIGHT_CORE from the environment
//
// DIST names another build (to see the check fail on an older one). Port PORT (29450 unless
// told), on loopback.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const pw = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, '..', '..');
const DIST = process.env.DIST ?? join(ROOT, 'apps', 'web', 'dist');
const OUT = process.env.OUT ?? join(ROOT, 'target', 'player-pause');
const PORT = Number(process.env.PORT ?? 29450);
if (!existsSync(join(DIST, 'index.html'))) throw new Error(`no build at ${DIST}: run \`pnpm build\` in apps/web first`);

// The stream: 40 s of a test picture and a tone, as two renditions the way the desk serves them.
const MEDIA = join(OUT, 'hls');
rmSync(MEDIA, { recursive: true, force: true });
for (const d of ['v0', 'audio']) mkdirSync(join(MEDIA, d), { recursive: true });
const hlsOut = (dir) => ['-f', 'hls', '-hls_time', '2', '-hls_playlist_type', 'vod', '-hls_segment_type', 'fmp4',
  '-hls_fmp4_init_filename', 'init.mp4', join(MEDIA, dir, 'index.m3u8')];
const ffmpeg = (args) => execFileSync('ffmpeg', ['-nostdin', '-v', 'error', '-y', ...args], { stdio: 'inherit', timeout: 120_000 });
ffmpeg(['-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=20', '-t', '40', '-c:v', 'libsvtav1', '-preset', '12', '-g', '40', '-pix_fmt', 'yuv420p', ...hlsOut('v0')]);
ffmpeg(['-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-t', '40', '-ac', '2', '-c:a', 'libopus', '-b:a', '64k', ...hlsOut('audio')]);
const rendition = 'URI="audio/index.m3u8"';
writeFileSync(join(MEDIA, 'live.m3u8'), `#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="Programme",DEFAULT=YES,AUTOSELECT=YES,CHANNELS="2",${rendition}\n#EXT-X-STREAM-INF:BANDWIDTH=500000,CODECS="av01.0.00M.08,opus",RESOLUTION=320x180,FRAME-RATE=20.000,AUDIO="aud"\nv0/index.m3u8\n`);
writeFileSync(join(MEDIA, 'audio.m3u8'), `#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="Programme",DEFAULT=YES,AUTOSELECT=YES,CHANNELS="2",${rendition}\n#EXT-X-STREAM-INF:BANDWIDTH=80000,CODECS="opus",AUDIO="aud"\naudio/index.m3u8\n`);

const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.json': 'application/json', '.woff2': 'font/woff2', '.m3u8': 'application/vnd.apple.mpegurl', '.mp4': 'video/mp4', '.m4s': 'video/iso.segment' };
let broken = false; // every segment answers 404
const web = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const media = path.startsWith('/hls/');
  if (media && broken && path.endsWith('.m4s')) return res.writeHead(404).end();
  const base = media ? MEDIA : DIST;
  const file = normalize(join(base, media ? path.slice('/hls/'.length) : path.endsWith('/') ? `${path}index.html` : path));
  if (!file.startsWith(base) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' }).end(readFileSync(file));
});
await new Promise((r) => web.listen(PORT, '127.0.0.1', r));
const WEB = `http://127.0.0.1:${PORT}`;

const PLAYER = { station: 'Kettle FM', show: 'The Morning Kettle', stream_url: '', streams: [], presenter: null, presenter_live: false, show_on_air: false, call_in: { open: false, why_not: null, consent_text: '', consent_version: 'v1', name_max: 60, topic_max: 200 }, relay: { turn_tls_port: 5349, stun_servers: [] }, video: { live: '/hls/live.m3u8', audio: '/hls/audio.m3u8' } };

const results = [];
function check(name, ok, detail = '') {
  results.push(!!ok);
  console.log(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail);
}

const browser = await pw.chromium.launch({ headless: true });
try {
  console.log(`Chromium ${browser.version()}, ${DIST}`);
  const state = (page) => page.evaluate(() => {
    const b = [...document.querySelectorAll('button')].find((x) => /^(Pause|Listen)/.test(x.innerText.trim()));
    const v = document.querySelector('video');
    return { button: b?.innerText.trim() ?? null, paused: v?.paused ?? null, time: v?.currentTime ?? 0,
      note: [...document.querySelectorAll('p,[role=status],[role=alert]')].map((e) => e.innerText.trim()).find((s) => /stream (stopped|didn)/i.test(s)) ?? null };
  });
  const until = async (page, wanted, ms = 20_000) => {
    const end = Date.now() + ms;
    let s = await state(page);
    while (!wanted(s) && Date.now() < end) {
      await page.waitForTimeout(250);
      s = await state(page);
    }
    return s;
  };
  const sounding = (s) => s.button === 'Pause' && s.paused === false && s.time > 1;
  const resting = (s) => /^Listen/.test(s.button ?? '') && s.paused === true;
  async function open() {
    const page = await browser.newPage();
    await page.route('**/api/v1/**', (route) => {
      const p = new URL(route.request().url()).pathname;
      const body = p === '/api/v1/player' ? PLAYER : p === '/api/v1/branding' ? { accent: null, icon: null, banner: null } : null;
      return route.fulfill({ status: body ? 200 : 404, contentType: 'application/json', body: JSON.stringify(body ?? { error: 'not found' }) });
    });
    await page.goto(WEB);
    const main = page.locator('button', { hasText: /^\s*(Pause|Listen)/ }).first();
    await main.waitFor();
    return { page, main };
  }

  for (const mode of ['picture', 'audio only']) {
    const { page, main } = await open();
    if (mode === 'audio only') await page.getByRole('button', { name: 'Audio only' }).click();
    await main.click();
    let s = await until(page, sounding);
    check(`${mode}: a press plays, and the button says Pause`, sounding(s), JSON.stringify(s));
    for (const round of [1, 2]) {
      await main.click();
      s = await until(page, resting, 5_000);
      await page.waitForTimeout(500);
      s = await state(page);
      check(`${mode}, round ${round}: Pause stops it, and the button offers to play`, resting(s), JSON.stringify(s));
      await main.click();
      s = await until(page, sounding);
      check(`${mode}, round ${round}: the next press plays again`, sounding(s), JSON.stringify(s));
    }
    await page.close();
  }

  {
    broken = true;
    const { page, main } = await open();
    await main.click();
    let s = await until(page, (x) => !!x.note, 60_000);
    await page.waitForTimeout(1_000);
    s = await state(page);
    check('a stream that does not start: the page says so', !!s.note, JSON.stringify(s));
    check('a stream that does not start: the button offers to play, not Pause', /^Listen/.test(s.button ?? ''), JSON.stringify(s));
    broken = false;
    await main.click();
    s = await until(page, sounding);
    check('once the stream is there, one press plays it', sounding(s), JSON.stringify(s));
    await page.close();
  }
} finally {
  await browser.close();
  web.close();
}
const failed = results.filter((ok) => !ok).length;
console.log(`${results.length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
