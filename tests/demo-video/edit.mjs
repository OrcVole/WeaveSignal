// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Edits the demo video from what tests/demo-video/run.mjs recorded: the introduction (each
// character steps forward on a card over a still of where they sit, saying who they are), the
// show as it happened (the desk or the screener full frame, Gregor's invite page and Niall's
// player page, both on phones, as insets, a title for each moment), a replay of the dump against the listener stream
// ("the studio heard it; listeners did not"), and the outro. Captions are burned in and written
// as WebVTT. Cards and overlays are drawn as HTML in the product's dark palette
// (packages/ui/tokens.css) and rendered with Playwright; everything else is ffmpeg.
//
// The soundtrack is what the room really carried: the studio mix (the mix-minus of Finlay, the
// DJ standing by, who says nothing on the show: every voice on air or cued, and the carts),
// with the booth (Shona's and Niall's microphones, which only they hear) while Shona screens
// him, and Eilidh's microphone while she talks to Niall alone. The swear is bleeped here for
// the website; the replay plays the listener stream as it was recorded.
//
//   PLAYWRIGHT_CORE=... CHROMIUM=... node tests/demo-video/edit.mjs [RUN dir]
//
// Out, in target/demo-video/out/ (OUT): demo.webm (AV1 and Opus), demo.mp4 (H.264 and AAC),
// demo.vtt, poster.jpg, clips/<name>.webm with clips/<name>.vtt, CREDITS.txt. Work files in
// target/demo-video/edit/.
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const RUN = resolve(process.argv[2] ?? process.env.RUN ?? join(root, 'target/demo-video/run/show'));
const STEMS = process.env.STEMS ?? join(root, 'target/demo-video/stems');
const WORK = process.env.WORK ?? join(root, 'target/demo-video/edit');
const OUT = process.env.OUT ?? join(root, 'target/demo-video/out');
const ONLY = (process.env.ONLY ?? '').split(',').filter(Boolean);
const script = JSON.parse(readFileSync(join(here, 'script.json'), 'utf8'));
const tl = JSON.parse(readFileSync(join(RUN, 'timeline.json'), 'utf8'));
const W = 1920;
const H = 1080;
const FPS = 30;
for (const d of [WORK, OUT, join(OUT, 'clips'), join(WORK, 'cards'), join(WORK, 'seg')]) mkdirSync(d, { recursive: true });
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
const ff = (args) => execFileSync('ffmpeg', ['-v', 'error', '-y', ...args], { stdio: ['ignore', 'inherit', 'inherit'], maxBuffer: 1 << 30 });
const probe = (f) => Number(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', f]).toString());
const want = (stage) => !ONLY.length || ONLY.includes(stage);
const s3 = (x) => x.toFixed(3);

// ------------------------------------------------------------------ the timeline
const ev = tl.events;
const first = (pred) => ev.find(pred);
const ids = tl.ids;
const who = Object.fromEntries(Object.entries(ids).filter(([k]) => !k.startsWith('_')).map(([k, v]) => [v, k]));
const moments = ev.filter((e) => e.kind === 'moment');
const momentAt = (id) => moments.find((m) => m.id === id).t;
const untilAt = (what) => first((e) => e.kind === 'until' && e.what === what).t;
const lines = ev.filter((e) => e.kind === 'line');
const lineOf = (id) => lines.find((l) => l.id === id);
const S = momentAt('m1-open') - 800; // the show's first frame, wall-clock ms
const E = untilAt('auto') + 4500; // its last
const DUMP = untilAt('dumped');
const CUT = lineOf('l-e-sorry').at + lineOf('l-e-sorry').ms + 300; // the replay goes in here, after Eilidh's apology
const track = (identity) => first((e) => e.kind === 'track' && e.identity === identity);
const streamAt = first((e) => e.kind === 'stream').at;
const dumpLine = lineOf('l-j');
const sorryLine = lineOf('l-e-sorry');
const showLen = (E - S) / 1000;
const partA = (CUT - S) / 1000;
const partB = (E - CUT) / 1000;

// ------------------------------------------------------------------ audio helpers
function pcm(file, rate = 8000) {
  const buf = execFileSync('ffmpeg', ['-v', 'error', '-i', file, '-ac', '1', '-ar', String(rate), '-f', 'f32le', '-'], { maxBuffer: 1 << 30 });
  return new Float32Array(buf.buffer, buf.byteOffset, buf.byteLength / 4);
}
/** dB envelope, one value per 10 ms. */
function envelope(x, rate = 8000) {
  const n = Math.floor(x.length / (rate / 100));
  const e = new Float32Array(n);
  const k = rate / 100;
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let j = 0; j < k; j++) s += x[i * k + j] ** 2;
    e[i] = 10 * Math.log10(s / k + 1e-10);
  }
  return e;
}
/** The stream's lag behind the studio mix over a wall-clock window, searched in [lo, hi] s. */
function lag(mixEnv, mixStart, streamEnv, w0, w1, lo, hi) {
  const a0 = Math.round((w0 - mixStart) / 10);
  const a1 = Math.round((w1 - mixStart) / 10);
  const seg = Array.from(mixEnv.slice(a0, a1));
  const mean = seg.reduce((p, c) => p + c, 0) / seg.length;
  const x = seg.map((v) => v - mean);
  const xx = Math.sqrt(x.reduce((p, c) => p + c * c, 0));
  let best = { c: -2, lag: null };
  for (let l = Math.round(lo * 100); l <= Math.round(hi * 100); l++) {
    const b0 = Math.round((w0 - streamAt) / 10) + l;
    if (b0 < 0 || b0 + x.length > streamEnv.length) continue;
    let m = 0;
    for (let i = 0; i < x.length; i++) m += streamEnv[b0 + i];
    m /= x.length;
    let c = 0;
    let yy = 0;
    for (let i = 0; i < x.length; i++) {
      const y = streamEnv[b0 + i] - m;
      c += x[i] * y;
      yy += y * y;
    }
    c /= xx * Math.sqrt(yy) + 1e-9;
    if (c > best.c) best = { c, lag: l / 100 };
  }
  return best;
}
/** Where the swear is in Jessie's stem: the last word, back from the end of her speech to the
 *  first dip under -45 dB (20 ms or more) that parts it from the word before. */
function swearSpan() {
  const e = envelope(pcm(join(STEMS, 'raw', `${dumpLine.id}.wav`), 8000));
  let end = e.length - 1;
  while (end > 0 && e[end] < -45) end--;
  let i = end;
  while (i > 1 && !(e[i] < -45 && e[i - 1] < -45)) i--;
  return { from: (i + 1) / 100, to: end / 100 + 0.08 };
}

const audioDir = join(RUN, 'audio');
const mixId = `desk-mm-${ids.finlay}`;
const mixFile = join(audioDir, `${mixId}.webm`);
const mixStart = track(mixId).at;
let lags;
if (existsSync(join(WORK, 'lags.json')) && !want('align-again')) lags = JSON.parse(readFileSync(join(WORK, 'lags.json'), 'utf8'));
if (!lags || want('align')) {
  log('aligning the listener stream');
  const mixEnv = envelope(pcm(mixFile));
  const streamEnv = envelope(pcm(join(audioDir, 'stream.ogg')));
  const before = lag(mixEnv, mixStart, streamEnv, momentAt('m1-open'), DUMP - 3000, 10, 40);
  let after = lag(mixEnv, mixStart, streamEnv, DUMP + 300, momentAt('m8-down') + 800, before.lag - 9.5, before.lag - 5);
  // Too little sound after the dump to be sure: the delay it threw away, by the desk's own figure.
  if (after.c < 0.5) after = { c: after.c, lag: before.lag - 8, assumed: true };
  lags = { before, after, swear: swearSpan() };
  writeFileSync(join(WORK, 'lags.json'), JSON.stringify(lags, null, 1));
}
log('stream lag before the dump', lags.before, 'after', lags.after, 'swear in the stem', lags.swear);

// ------------------------------------------------------------------ the pages, as video
function pageVideo(name, t0, t1, out) {
  const dir = join(RUN, 'frames', name);
  const frames = readFileSync(join(dir, 'frames.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  let i = frames.findLastIndex((f) => f.t <= t0);
  if (i < 0) i = 0;
  const list = [];
  let t = t0;
  for (; i < frames.length && t < t1; i++) {
    const next = Math.min(frames[i + 1]?.t ?? t1, t1);
    const d = (next - t) / 1000;
    if (d > 0) list.push(`file '${join(dir, frames[i].file)}'\nduration ${d.toFixed(4)}`);
    t = next;
  }
  list.push(`file '${join(dir, frames[Math.min(i, frames.length) - 1].file)}'`);
  writeFileSync(`${out}.txt`, list.join('\n') + '\n');
  ff(['-f', 'concat', '-safe', '0', '-i', `${out}.txt`, '-vf', `fps=${FPS},format=yuv420p`, '-t', s3((t1 - t0) / 1000), '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '12', out]);
}

// ------------------------------------------------------------------ cards and overlays
const tokens = readFileSync(join(root, 'packages/ui/tokens.css'), 'utf8');
const mark = readFileSync(join(root, 'packages/ui/icon/weavesignal-icon-broadcast.svg'), 'utf8').replace(/<\?xml[^>]*>/, '');
const css = `${tokens}
  *{box-sizing:border-box;margin:0}
  html,body{width:${W}px;height:${H}px;background:transparent;font-family:var(--font-ui);color:var(--ink);-webkit-font-smoothing:antialiased}
  .full{width:${W}px;height:${H}px;background:var(--plane);position:relative;overflow:hidden}
  .mark svg{width:100%;height:100%;fill:currentColor;color:var(--accent)}
  .caps{font-size:18px;letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted);font-weight:600}`;
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;');
const img = (file) => `data:image/png;base64,${readFileSync(file).toString('base64')}`;
let browser;
async function render(name, body, { transparent = false } = {}) {
  if (!browser) {
    const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');
    browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM });
  }
  const page = await browser.newPage({ viewport: { width: W, height: H } });
  await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head><body>${body}</body></html>`);
  await page.evaluate(() => document.fonts.ready);
  const file = join(WORK, 'cards', `${name}.png`);
  await page.screenshot({ path: file, omitBackground: transparent });
  await page.close();
  return file;
}
const still = (tag, page) => join(RUN, 'stills', `${tag}-${page}.png`);

/** A character's card: their name and role, over a still of where they sit. */
function castCard(id, shot, { phone = false } = {}) {
  const c = script.cast[id];
  const frame = phone
    ? `<div style="position:absolute;left:250px;top:90px;width:420px;height:900px;border-radius:44px;background:#0b0c12;padding:14px;box-shadow:var(--e-3);border:1px solid var(--hairline)"><img src="${img(shot)}" style="width:100%;height:100%;object-fit:cover;object-position:top;border-radius:32px"></div>`
    : `<div style="position:absolute;left:80px;top:150px;width:1000px;height:${phone ? 900 : 780}px;border-radius:16px;overflow:hidden;box-shadow:var(--e-3);border:1px solid var(--hairline)"><img src="${img(shot)}" style="width:100%;height:100%;object-fit:cover;object-position:left top"></div>`;
  return `<div class="full">
    <img src="${img(shot)}" style="position:absolute;inset:-40px;width:${W + 80}px;height:${H + 80}px;object-fit:cover;filter:blur(28px) brightness(.35) saturate(.8)">
    ${frame}
    <div style="position:absolute;left:${phone ? 780 : 1160}px;right:90px;top:0;bottom:0;display:flex;flex-direction:column;justify-content:center;gap:22px">
      <div class="caps">${esc(c.where)}</div>
      <div style="font-size:72px;font-weight:700;line-height:1.05;color:#fff">${esc(c.name)}</div>
      <div style="font-size:34px;color:var(--ink-2);line-height:1.3">${esc(c.role)}</div>
      <div style="margin-top:26px;font-size:20px;color:var(--muted)">Sample name · designed voice</div>
    </div></div>`;
}
const titleCard = () => `<div class="full" style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:28px">
    <div class="mark" style="width:96px;height:96px">${mark}</div>
    <div style="font-size:84px;font-weight:700;color:#fff;letter-spacing:-.01em">One morning on Kettle FM</div>
    <div style="font-size:32px;color:var(--ink-2)">A show through WeaveSignal's real desk, recorded as it ran</div>
    <div style="margin-top:30px;font-size:22px;color:var(--muted)">Kettle FM is not a real station · every voice is designed, not a person's</div></div>`;
const outroCard = () => {
  const o = script.outro_card;
  return `<div class="full" style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:30px">
    <div style="display:flex;align-items:center;gap:26px"><div class="mark" style="width:92px;height:92px">${mark}</div><div style="font-size:92px;font-weight:700;color:#fff">WeaveSignal</div></div>
    <div style="font-size:44px;color:var(--ink)">${esc(o.line)}</div>
    <div style="display:flex;gap:18px;margin-top:10px">${o.routes.map((r) => `<span style="font-size:28px;padding:12px 26px;border-radius:999px;border:1px solid var(--axis);color:var(--ink-2);background:var(--surface)">${esc(r)}</span>`).join('')}</div>
    <div style="font-size:56px;font-weight:600;color:var(--accent);margin-top:34px">${esc(o.url)}</div>
    <div style="font-size:22px;color:var(--muted);margin-top:8px">${esc(o.licence)}</div></div>`;
};
/** The band across the top that names each moment (it covers only the studio app's tab bar). */
const band = (n, m) => `<div style="position:absolute;left:0;top:0;width:${W}px;height:64px;background:rgba(19,20,28,.94);border-bottom:1px solid var(--axis);display:flex;align-items:center;gap:22px;padding:0 34px">
    <span style="font-size:20px;font-weight:700;color:var(--accent-ink);background:var(--accent);border-radius:999px;padding:4px 14px">${n}</span>
    <span style="font-size:28px;font-weight:700;color:#fff">${esc(m.title)}</span>
    <span style="font-size:24px;color:var(--ink-2)">${esc(m.caption)}</span></div>`;
/** Who hears whom, for the guest's arrival. */
const mixminus = () => `<div style="position:absolute;left:1240px;top:330px;width:630px;padding:26px 30px;border-radius:14px;background:rgba(26,27,38,.97);border:1px solid var(--axis);box-shadow:var(--e-3)">
    <div class="caps" style="margin-bottom:14px">Who hears whom · mix-minus</div>
    <div style="font-size:28px;font-weight:700;color:#fff;margin-bottom:16px">Gregor's headphones</div>
    ${[['Eilidh', true], ['the carts', true], ['Gregor himself', false]].map(([n, y]) => `<div style="display:flex;align-items:center;gap:14px;font-size:26px;margin:10px 0;color:${y ? 'var(--ink)' : 'var(--muted)'}"><span style="width:30px;height:30px;border-radius:50%;display:inline-flex;align-items:center;justify-content:center;font-size:20px;font-weight:700;background:${y ? 'var(--status-good)' : 'var(--surface-3)'};color:#fff">${y ? '✓' : '✕'}</span>${n}${y ? '' : ': never his own voice back'}</div>`).join('')}
    <div style="font-size:20px;color:var(--muted);margin-top:14px">The desk makes one mix for each person, so nobody hears their own echo.</div></div>`;
/** The replay of the dump: labels over two waveforms. */
const replay = (swearX, cutX) => `<div style="position:absolute;inset:0;background:rgba(19,20,28,.86)"></div>
    <div style="position:absolute;left:160px;top:120px;font-size:48px;font-weight:700;color:#fff">${esc(script.dump_caption)}</div>
    <div style="position:absolute;left:160px;top:190px;font-size:26px;color:var(--ink-2)">The same seconds of Jessie's call, twice: as the room heard them, and as the listener stream carried them.</div>
    <div class="caps" style="position:absolute;left:160px;top:300px">In the studio · real time</div>
    <div style="position:absolute;left:${160 + swearX}px;top:330px;width:90px;height:200px;background:rgba(214,59,88,.30);border:2px solid var(--live-onair);border-radius:6px"></div>
    <div style="position:absolute;left:${160 + swearX - 40}px;top:540px;font-size:22px;font-weight:700;color:var(--live-onair)">the swear (bleeped here)</div>
    <div class="caps" style="position:absolute;left:160px;top:620px">On the listener stream · what listeners heard, playing now</div>
    <div style="position:absolute;left:${160 + cutX}px;top:650px;width:4px;height:200px;background:var(--status-warning)"></div>
    <div style="position:absolute;left:${160 + cutX + 14}px;top:860px;font-size:22px;font-weight:700;color:var(--status-warning)">Del Del: the delay is thrown away, and listeners jump to the live programme</div>`;

// ------------------------------------------------------------------ stage: cards
const introCast = [
  ['eilidh', still('m5-air', 'desk')],
  ['gregor', still('m2-guest', 'gregor'), true],
  ['shona', still('m4-screen', 'shona')],
  ['niall', still('m3-call', 'niall'), true],
  ['jessie', still('m5-air', 'jessie'), true],
  ['finlay', still('pre', 'mic-finlay')],
];
const introLines = Object.fromEntries(script.intro.map((l) => [l.who, l]));
const TITLE_S = 3.6;
const LEAD = 0.45;
const TAIL = 0.75;
const cardLen = (whoId) => LEAD + probe(join(STEMS, `${introLines[whoId].id}.wav`)) + TAIL;
const OUTRO_S = probe(join(STEMS, `${script.outro.id}.wav`)) + 3.4;
// The replay: the studio's seconds from just before Jessie's line to the end of Eilidh's
// apology, and the listener stream's for the same moments, which are shorter by what the dump
// threw away. Both strips are drawn at one scale; the stream's plays.
const REPLAY = { lead: 0.6, tail: 1.2, studio0: dumpLine.at - 1200, len: 0, streamLen: 0 };
REPLAY.len = (sorryLine.at + sorryLine.ms - REPLAY.studio0) / 1000 + 0.6;
REPLAY.streamLen = REPLAY.len - (lags.before.lag - lags.after.lag);
const replayLen = REPLAY.lead + REPLAY.streamLen + REPLAY.tail;

if (want('cards')) {
  log('cards');
  await render('title', titleCard());
  for (const [id, shot, phone] of introCast) await render(`cast-${id}`, castCard(id, shot, { phone }));
  await render('outro', outroCard());
  let n = 0;
  for (const m of moments) await render(`band-${m.id}`, band(++n, m), { transparent: true });
  await render('mixminus', mixminus(), { transparent: true });
  // Both strips are the same length of listening: the studio's from Jessie's line on, the
  // stream's from when listeners got that moment. The stream jumps when the dump reaches it,
  // which is (before - after) seconds (the delay thrown away) ahead of the dump on its strip.
  const px = 1600 / REPLAY.len;
  const swearX = ((dumpLine.at - REPLAY.studio0) / 1000 + lags.swear.from) * px;
  const cutX = ((DUMP - REPLAY.studio0) / 1000 - (lags.before.lag - lags.after.lag)) * px;
  await render('replay', replay(swearX, cutX), { transparent: true });
}
if (browser) await browser.close();

// ------------------------------------------------------------------ stage: pages
const pageFile = (name) => join(WORK, 'seg', `page-${name}.mp4`);
if (want('pages')) {
  for (const name of ['desk', 'shona', 'gregor', 'niall']) {
    log('page', name);
    pageVideo(name, S, E, pageFile(name));
  }
}

// ------------------------------------------------------------------ stage: the show's picture
const at = (t) => (t - S) / 1000; // wall-clock ms to seconds into the show
const shows = ev.filter((e) => e.kind === 'show');
// The main picture: the desk, or Shona's screener from the moment it is shown until the desk is again.
const mainSpans = [];
{
  let cur = 'desk';
  let from = 0;
  for (const s of shows) {
    if (!['desk', 'shona'].includes(s.page)) continue;
    if (s.page !== cur) {
      if (cur === 'shona') mainSpans.push([from, at(s.t)]);
      cur = s.page;
      from = at(s.t);
    }
  }
  if (cur === 'shona') mainSpans.push([from, showLen]);
}
const gregorIn = at(first((e) => e.kind === 'show' && e.page === 'gregor').t);
const gregorOutAt = at(untilAt('gregor_on_air')) + 3.5;
const niallIn = at(first((e) => e.kind === 'show' && e.page === 'niall').t);
const niallOut = at(untilAt('niall_gone')) + 2.0;
const overlayOn = ev.filter((e) => e.kind === 'overlay');
const mmIn = at(overlayOn.find((e) => e.overlay === 'mixminus').t);
const mmOut = at(overlayOn.find((e) => e.overlay === null).t);
const gregorOut = Math.min(gregorOutAt, mmIn - 0.1);
const between = (a, b) => `between(t,${s3(a)},${s3(b)})`;

if (want('show')) {
  log('the show, composed');
  const inputs = ['desk', 'shona', 'gregor', 'niall'].flatMap((n) => ['-i', pageFile(n)]);
  const pngs = [...moments.map((m) => join(WORK, 'cards', `band-${m.id}.png`)), join(WORK, 'cards', 'mixminus.png')];
  for (const p of pngs) inputs.push('-loop', '1', '-t', s3(showLen), '-i', p);
  const g = [];
  g.push(`[0:v]setpts=PTS-STARTPTS,scale=${W}:${H}:flags=lanczos[desk]`);
  g.push(`[1:v]setpts=PTS-STARTPTS,scale=${W}:${H}:flags=lanczos[shona]`);
  g.push(`[desk][shona]overlay=0:0:enable='${mainSpans.map(([a, b]) => between(a, b)).join('+') || 0}'[main]`);
  // Gregor's laptop (1,920 × 1,200) and Niall's phone (780 × 1,688), framed.
  // The phones sit over the desk's left column (the clock and the rundown), so its toasts on the
  // right stay in sight; over the screener, on the right, over the chat, so the queue does.
  const onShona = mainSpans.map(([a, b]) => between(a, b)).join('+') || '0';
  // (Over the desk a little smaller: its left column is 350 px wide at the app's 120 %.)
  g.push(`[2:v]scale=312:676:flags=lanczos,pad=326:690:7:7:color=0x3b3f57[greg]`);
  g.push(`[3:v]split[pl][pr];[pl]scale=312:676:flags=lanczos,pad=326:690:7:7:color=0x3b3f57[phoneL];[pr]scale=388:840,pad=404:856:8:8:color=0x3b3f57[phoneR]`);
  g.push(`[main][greg]overlay=14:150:enable='${between(gregorIn, gregorOut)}'[v0]`);
  g.push(`[v0][phoneL]overlay=14:150:enable='${between(niallIn, niallOut)}*not(${onShona})'[v1]`);
  g.push(`[v1][phoneR]overlay=${W - 404 - 52}:110:enable='${between(niallIn, niallOut)}*(${onShona})'[v2]`);
  let last = 'v2';
  moments.forEach((m, i) => {
    const a = at(m.t);
    g.push(`[${last}][${4 + i}:v]overlay=0:0:enable='${between(a, a + 5.5)}'[b${i}]`);
    last = `b${i}`;
  });
  g.push(`[${last}][${4 + moments.length}:v]overlay=0:0:enable='${between(mmIn, mmOut)}'[v]`);
  ff([...inputs, '-filter_complex', g.join(';'), '-map', '[v]', '-t', s3(showLen), '-r', String(FPS), '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '14', '-pix_fmt', 'yuv420p', join(WORK, 'seg', 'show.mp4')]);
}

// ------------------------------------------------------------------ stage: the show's sound
const offsetOf = (identity) => (track(identity).at - S) / 1000; // where a recording starts, in show seconds
const holdSpan = (page, keyLike) => {
  const h = ev.find((e) => e.kind === 'hold' && e.page === page && keyLike(e));
  const r = ev.find((e) => e.kind === 'release' && e.page === page && e.t > h.t);
  return [at(h.t), at(r.t)];
};
const screening = holdSpan('shona', (e) => e.moment === 'm4-screen');
const talking = holdSpan('desk', (e) => e.moment === 'm5-air');
const swear = [at(dumpLine.at) + lags.swear.from, at(dumpLine.at) + lags.swear.to];
if (want('sound')) {
  log('the show, its sound; screening', screening, 'talk', talking, 'swear', swear);
  const src = [
    [mixFile, offsetOf(mixId)],
    [join(audioDir, `${ids.shona}.webm`), offsetOf(ids.shona)],
    [join(audioDir, `${ids.niall}.webm`), offsetOf(ids.niall)],
    [join(audioDir, `${ids.eilidh}.webm`), offsetOf(ids.eilidh)],
    [join(root, 'assets/carts-demo/bleep-1.opus'), 0],
    [join(audioDir, 'stream.ogg'), (streamAt - S) / 1000],
  ];
  // A recording placed on the show's clock: trimmed where it starts before the show, delayed where after.
  const place = (i, off) => (off >= 0 ? `[${i}:a]aresample=48000,aformat=channel_layouts=stereo,adelay=${Math.round(off * 1000)}:all=1` : `[${i}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=start=${s3(-off)},asetpts=PTS-STARTPTS`);
  const g = [];
  const autoAt = at(untilAt('auto'));
  // The studio mix, with the swear cut out (the bleep goes there).
  g.push(`${place(0, src[0][1])},volume=0:enable='${between(swear[0], swear[1])}'[mix]`);
  // The booth while Shona screens Niall, and Eilidh while she talks to him alone.
  g.push(`${place(1, src[1][1])},volume=0:enable='not(${between(screening[0] - 0.1, screening[1] + 0.4)})'[shona]`);
  g.push(`${place(2, src[2][1])},volume=0:enable='not(${between(screening[0] - 0.1, screening[1] + 0.4)})'[niall]`);
  g.push(`${place(3, src[3][1])},volume=0:enable='not(${between(talking[0] - 0.1, talking[1] + 0.3)})'[eilidh]`);
  g.push(`[4:a]aresample=48000,aformat=channel_layouts=stereo,atrim=0:${s3(swear[1] - swear[0])},volume=0.35,afade=t=out:st=${s3(Math.max(0, swear[1] - swear[0] - 0.05))}:d=0.05,adelay=${Math.round(swear[0] * 1000)}:all=1[bleep]`);
  // Automation on the air at the end: the stream's default loop, faded in under the close.
  const loopFrom = (streamAt - S) / 1000;
  const lagEnd = lags.after.lag;
  g.push(`${place(5, loopFrom - lagEnd)},volume=0:enable='lt(t,${s3(autoAt + 0.2)})',afade=t=in:st=${s3(autoAt + 0.2)}:d=1.5,volume=0.55[loop]`);
  g.push(`[mix][shona][niall][eilidh][bleep][loop]amix=inputs=6:normalize=0:duration=longest,atrim=0:${s3(showLen)},afade=t=out:st=${s3(showLen - 1.2)}:d=1.2[a]`);
  ff([...src.flatMap(([f]) => ['-i', f]), '-filter_complex', g.join(';'), '-map', '[a]', '-ar', '48000', '-ac', '2', '-c:a', 'pcm_s24le', join(WORK, 'seg', 'show.wav')]);
}

// ------------------------------------------------------------------ stage: the replay of the dump
if (want('replay')) {
  log('the replay');
  const len = REPLAY.len;
  const streamLen = REPLAY.streamLen;
  const studio0 = at(REPLAY.studio0);
  // The studio side: the show's own sound (the swear already bleeped); the stream side: what
  // listeners got for the same seconds, which arrived lags.before later.
  ff(['-i', join(WORK, 'seg', 'show.wav'), '-ss', s3(studio0), '-t', s3(len), '-c:a', 'pcm_s24le', join(WORK, 'seg', 'replay-studio.wav')]);
  const stream0 = (REPLAY.studio0 + lags.before.lag * 1000 - streamAt) / 1000;
  ff(['-i', join(audioDir, 'stream.ogg'), '-ss', s3(stream0), '-t', s3(streamLen), '-ar', '48000', '-ac', '2', '-c:a', 'pcm_s24le', join(WORK, 'seg', 'replay-stream.wav')]);
  for (const [n, colour, width] of [['studio', '0xc0caf5', 1600], ['stream', '0xfab219', Math.round((1600 * streamLen) / len)]]) {
    ff(['-i', join(WORK, 'seg', `replay-${n}.wav`), '-filter_complex', `aformat=channel_layouts=mono,compand=gain=6,showwavespic=s=${width}x200:colors=${colour}:draw=full`, '-frames:v', '1', join(WORK, 'cards', `wave-${n}.png`)]);
  }
  const bg = join(WORK, 'cards', 'replay-bg.png');
  ff(['-ss', s3(at(DUMP) + 0.3), '-i', join(WORK, 'seg', 'show.mp4'), '-frames:v', '1', '-vf', 'gblur=sigma=14,eq=brightness=-0.08', bg]);
  const T = replayLen;
  const L = REPLAY.lead;
  const g = [
    `[0:v]scale=${W}:${H},setsar=1[bg]`,
    `[bg][1:v]overlay=0:0[b1]`,
    `[b1][2:v]overlay=160:330[b2]`,
    `[b2][3:v]overlay=160:650[b3]`,
    // the playhead: across the studio strip and the stream strip as their sound plays
    `[b3]drawbox=x='160+${s3(1600 / len)}*clip(t-${L},0,${s3(streamLen)})':y=640:w=4:h=220:color=white@0.9:t=fill[v]`,
  ];
  ff(['-loop', '1', '-t', s3(T), '-i', bg, '-loop', '1', '-t', s3(T), '-i', join(WORK, 'cards', 'replay.png'), '-loop', '1', '-t', s3(T), '-i', join(WORK, 'cards', 'wave-studio.png'), '-loop', '1', '-t', s3(T), '-i', join(WORK, 'cards', 'wave-stream.png'),
    '-i', join(WORK, 'seg', 'replay-stream.wav'),
    '-filter_complex', `${g.join(';')};[4:a]adelay=${Math.round(L * 1000)}:all=1,apad,atrim=0:${s3(T)}[a]`,
    '-map', '[v]', '-map', '[a]', '-t', s3(T), '-r', String(FPS), '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '14', '-pix_fmt', 'yuv420p', '-c:a', 'pcm_s24le', join(WORK, 'seg', 'replay.mkv')]);
}

// ------------------------------------------------------------------ stage: intro and outro
if (want('intro')) {
  log('the introduction and the outro');
  const parts = [['title', TITLE_S, null], ...introCast.map(([id]) => [`cast-${id}`, cardLen(id), introLines[id]])];
  for (const [name, len, line] of parts) {
    const args = ['-loop', '1', '-t', s3(len), '-i', join(WORK, 'cards', `${name}.png`)];
    const fade = `fade=t=in:st=0:d=0.35,fade=t=out:st=${s3(len - 0.35)}:d=0.35`;
    if (line) {
      args.push('-i', join(STEMS, `${line.id}.wav`));
      args.push('-filter_complex', `[0:v]fps=${FPS},format=yuv420p,${fade}[v];[1:a]aresample=48000,adelay=${Math.round(LEAD * 1000)}:all=1,apad,atrim=0:${s3(len)}[a]`, '-map', '[v]', '-map', '[a]');
    } else {
      args.push('-f', 'lavfi', '-t', s3(len), '-i', 'anullsrc=r=48000:cl=stereo', '-filter_complex', `[0:v]fps=${FPS},format=yuv420p,${fade}[v]`, '-map', '[v]', '-map', '1:a');
    }
    ff([...args, '-t', s3(len), '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '14', '-c:a', 'pcm_s24le', '-ar', '48000', '-ac', '2', join(WORK, 'seg', `${name}.mkv`)]);
  }
  const len = OUTRO_S;
  ff(['-loop', '1', '-t', s3(len), '-i', join(WORK, 'cards', 'outro.png'), '-i', join(STEMS, `${script.outro.id}.wav`),
    '-filter_complex', `[0:v]fps=${FPS},format=yuv420p,fade=t=in:st=0:d=0.5,fade=t=out:st=${s3(len - 0.8)}:d=0.8[v];[1:a]aresample=48000,adelay=900:all=1,apad,atrim=0:${s3(len)}[a]`,
    '-map', '[v]', '-map', '[a]', '-t', s3(len), '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '14', '-c:a', 'pcm_s24le', '-ar', '48000', '-ac', '2', join(WORK, 'seg', 'outro.mkv')]);
}

// ------------------------------------------------------------------ the whole timeline
const introParts = [['title', TITLE_S], ...introCast.map(([id]) => [`cast-${id}`, cardLen(id)])];
const introLen = introParts.reduce((p, [, l]) => p + l, 0);
const showA0 = introLen;
const replay0 = showA0 + partA;
const showB0 = replay0 + replayLen;
const outro0 = showB0 + partB;
const total = outro0 + OUTRO_S;
/** A moment of the show (wall-clock ms) on the final video's clock. */
const vt = (t) => {
  const s = at(t);
  return s < partA ? showA0 + s : showB0 + (s - partA);
};

// Captions: who says what, when it was said.
const cues = [];
const speaker = (id) => script.cast[id].name.split(' ')[0];
function addLine(start, len, whoId, text) {
  // Long lines in two cues, split at a sentence (or comma) near the middle, timed by length.
  const words = text.split(' ');
  if (text.length <= 84) return cues.push({ start, end: start + len, who: speaker(whoId), text });
  let best = null;
  for (let i = 3; i < words.length - 2; i++) {
    const left = words.slice(0, i).join(' ');
    const score = Math.abs(left.length - text.length / 2) - (/[.?!]$/.test(left) ? 30 : /,$/.test(left) ? 12 : 0);
    if (!best || score < best.score) best = { i, score };
  }
  const a = words.slice(0, best.i).join(' ');
  const b = words.slice(best.i).join(' ');
  const mid = start + len * (a.length / text.length);
  cues.push({ start, end: mid, who: speaker(whoId), text: a });
  cues.push({ start: mid, end: start + len, who: speaker(whoId), text: b });
}
{
  let t = TITLE_S;
  for (const [id] of introCast) {
    const l = introLines[id];
    addLine(t + LEAD, probe(join(STEMS, `${l.id}.wav`)), id, l.text);
    t += cardLen(id);
  }
}
for (const l of lines) {
  const text = l.swear ? l.text.replace(new RegExp(`\\b${l.swear}\\b`), '[bleep]') : l.text;
  addLine(vt(l.at), l.ms / 1000, l.who, text);
}
cues.push({ start: replay0 + 0.3, end: showB0 - 0.2, who: '', text: "[The listener stream: Jessie's call, then a jump to Eilidh's apology. The swear never reached listeners.]", vttOnly: true });
addLine(outro0 + 0.9, probe(join(STEMS, `${script.outro.id}.wav`)), 'eilidh', script.outro.text);
cues.sort((a, b) => a.start - b.start);
// Every line on the video's clock, whole, for check.sh.
const spoken = [];
{
  let t = TITLE_S;
  for (const [id] of introCast) {
    const l = introLines[id];
    spoken.push({ id: l.id, who: id, at: t + LEAD, len: probe(join(STEMS, `${l.id}.wav`)), text: l.text });
    t += cardLen(id);
  }
  for (const l of lines) spoken.push({ id: l.id, who: l.who, at: vt(l.at), len: l.ms / 1000, text: l.text, ...(l.swear ? { swear: l.swear, swear_from: lags.swear.from } : {}) });
  spoken.push({ id: script.outro.id, who: 'eilidh', at: outro0 + 0.9, len: probe(join(STEMS, `${script.outro.id}.wav`)), text: script.outro.text });
}
for (let i = 0; i + 1 < cues.length; i++) if (cues[i].end > cues[i + 1].start - 0.02 && !cues[i].vttOnly) cues[i].end = cues[i + 1].start - 0.02;

const vtime = (s) => {
  const ms = Math.max(0, Math.round(s * 1000));
  const h = Math.floor(ms / 3600000);
  const m = Math.floor(ms / 60000) % 60;
  const sec = Math.floor(ms / 1000) % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}.${String(ms % 1000).padStart(3, '0')}`;
};
function vtt(list, from = 0, to = Infinity) {
  const out = ['WEBVTT', ''];
  for (const c of list) {
    if (Math.min(c.end, to) - Math.max(c.start, from) < 0.3) continue; // nothing, or a sliver of a line cut by a clip's edge
    out.push(`${vtime(Math.max(c.start, from) - from)} --> ${vtime(Math.min(c.end, to) - from)}`);
    out.push(c.who ? `<v ${c.who}>${c.who}: ${c.text}` : c.text, '');
  }
  return out.join('\n');
}
const ass = () => {
  const t = (s) => {
    const cs = Math.max(0, Math.round(s * 100));
    return `${Math.floor(cs / 360000)}:${String(Math.floor(cs / 6000) % 60).padStart(2, '0')}:${String(Math.floor(cs / 100) % 60).padStart(2, '0')}.${String(cs % 100).padStart(2, '0')}`;
  };
  const head = `[Script Info]\nScriptType: v4.00+\nPlayResX: ${W}\nPlayResY: ${H}\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Line,Noto Sans,40,&H00FFFFFF,&H00FFFFFF,&H00000000,&H28120E0B,0,0,0,0,100,100,0,0,3,14,0,2,470,470,128,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n`;
  return head + cues.filter((c) => !c.vttOnly).map((c) => `Dialogue: 0,${t(c.start)},${t(c.end)},Line,,0,0,0,,{\\c&HF7A27A&\\b1}${c.who}{\\c&HFFFFFF&\\b0}  ${c.text.replace(/\n/g, ' ')}`).join('\n') + '\n';
};

if (want('final')) {
  log('the whole video,', total.toFixed(1), 's');
  writeFileSync(join(WORK, 'captions.ass'), ass());
  writeFileSync(join(OUT, 'demo.vtt'), vtt(cues));
  // Picture and sound of the show, cut at the replay.
  ff(['-i', join(WORK, 'seg', 'show.mp4'), '-i', join(WORK, 'seg', 'show.wav'), '-t', s3(partA), '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'pcm_s24le', join(WORK, 'seg', 'show-a.mkv')]);
  ff(['-ss', s3(partA), '-i', join(WORK, 'seg', 'show.mp4'), '-ss', s3(partA), '-i', join(WORK, 'seg', 'show.wav'), '-map', '0:v', '-map', '1:a', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '14', '-c:a', 'pcm_s24le', join(WORK, 'seg', 'show-b.mkv')]);
  const order = [...introParts.map(([n]) => `${n}.mkv`), 'show-a.mkv', 'replay.mkv', 'show-b.mkv', 'outro.mkv'];
  const inputs = order.flatMap((f) => ['-i', join(WORK, 'seg', f)]);
  const concat = `${order.map((_, i) => `[${i}:v][${i}:a]`).join('')}concat=n=${order.length}:v=1:a=1[cv][ca]`;
  // One master: captions burned in; the sound to -16 LUFS integrated, peaks under -1.5 dBTP.
  const assPath = join(WORK, 'captions.ass').replace(/:/g, '\\:');
  ff([...inputs, '-filter_complex', `${concat};[cv]subtitles='${assPath}'[v];[ca]aresample=48000[a]`, '-map', '[v]', '-map', '[a]', '-c:v', 'libx264', '-preset', 'medium', '-crf', '12', '-pix_fmt', 'yuv420p', '-c:a', 'pcm_s24le', join(WORK, 'master-raw.mkv')]);
  const out = execFileSync('sh', ['-c', `ffmpeg -hide_banner -i '${join(WORK, 'master-raw.mkv')}' -map 0:a -af loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json -f null - 2>&1 | sed -n '/^{/,/^}/p'`]).toString();
  const m = JSON.parse(out);
  const ln = `loudnorm=I=-16:TP=-1.5:LRA=11:measured_I=${m.input_i}:measured_TP=${m.input_tp}:measured_LRA=${m.input_lra}:measured_thresh=${m.input_thresh}:offset=${m.target_offset}:linear=true`;
  log('loudness before', m.input_i, 'LUFS');
  ff(['-i', join(WORK, 'master-raw.mkv'), '-map', '0:v', '-map', '0:a', '-c:v', 'copy', '-af', `${ln},aresample=48000`, '-c:a', 'pcm_s24le', join(WORK, 'master.mkv')]);
  if (process.env.MASTER_ONLY) {
    writeFileSync(join(WORK, 'timeline-video.json'), JSON.stringify({ total, introLen, partA, replay0, showB0, outro0, moments: moments.map((m) => ({ id: m.id, at: vt(m.t) })), lines: spoken, stream: { file: join(audioDir, 'stream.ogg'), dump: { from: (REPLAY.studio0 + lags.before.lag * 1000 - streamAt) / 1000 - 4, len: REPLAY.streamLen + 8 } } }, null, 1));
    process.exit(0);
  }
  log('demo.webm');
  ff(['-i', join(WORK, 'master.mkv'), '-c:v', 'libsvtav1', '-preset', '5', '-crf', '30', '-svtav1-params', 'tune=0:enable-overlays=1', '-g', '240', '-pix_fmt', 'yuv420p10le', '-maxrate', '4M', '-bufsize', '8M', '-c:a', 'libopus', '-b:a', '128k', join(OUT, 'demo.webm')]);
  log('demo.mp4');
  ff(['-i', join(WORK, 'master.mkv'), '-c:v', 'libx264', '-preset', 'slow', '-crf', '20', '-maxrate', '4M', '-bufsize', '8M', '-profile:v', 'high', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '160k', '-movflags', '+faststart', join(OUT, 'demo.mp4')]);
  const posterAt = vt(untilAt('niall_on_air')) + 5.5;
  writeFileSync(join(WORK, 'timeline-video.json'), JSON.stringify({ total, introLen, partA, replay0, showB0, outro0, posterAt, moments: moments.map((m) => ({ id: m.id, at: vt(m.t) })), lines: spoken, stream: { file: join(audioDir, 'stream.ogg'), dump: { from: (REPLAY.studio0 + lags.before.lag * 1000 - streamAt) / 1000 - 4, len: REPLAY.streamLen + 8 } } }, null, 1));
}

// ------------------------------------------------------------------ the poster
// The desk on air with a caller, a few seconds after Niall goes on air, without captions.
if (want('final') || want('poster')) ff(['-ss', s3(at(untilAt('niall_on_air')) + 5.5), '-i', join(WORK, 'seg', 'show.mp4'), '-frames:v', '1', '-q:v', '2', join(OUT, 'poster.jpg')]);

// ------------------------------------------------------------------ clips for the site
if (want('clips')) {
  const tv = JSON.parse(readFileSync(join(WORK, 'timeline-video.json'), 'utf8'));
  const mAt = (id) => tv.moments.find((m) => m.id === id).at;
  const clips = [
    ['on-air', mAt('m1-open') + 0.2, mAt('m2-guest') + 0.3],
    ['guest-joins', mAt('m2-guest'), mAt('m3-call')],
    ['call-in', mAt('m3-call'), vt(lineOf('l-n2').at + lineOf('l-n2').ms) + 0.5],
    ['dump', vt(lineOf('l-j').at) - 0.4, tv.showB0 + 0.3],
    ['playout-down', mAt('m8-down'), mAt('m7-end') + 0.5],
  ];
  for (const [name, a, b] of clips) {
    const len = Math.min(b - a, 26);
    log('clip', name, len.toFixed(1), 's');
    ff(['-ss', s3(a), '-i', join(WORK, 'master.mkv'), '-t', s3(len), '-c:v', 'libsvtav1', '-preset', '5', '-crf', '30', '-svtav1-params', 'tune=0', '-g', '240', '-pix_fmt', 'yuv420p10le', '-c:a', 'libopus', '-b:a', '128k', '-af', `loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000,afade=t=in:d=0.2,afade=t=out:st=${s3(len - 0.4)}:d=0.4`, '-vf', `fade=t=in:d=0.2,fade=t=out:st=${s3(len - 0.4)}:d=0.4`, join(OUT, 'clips', `${name}.webm`)]);
    writeFileSync(join(OUT, 'clips', `${name}.vtt`), vtt(cues, a, a + len));
  }
}

if (want('credits')) {
  const cartsUsed = ['jingle-1', 'bed-1', 'bleep-1'];
  const credits = readFileSync(join(root, 'assets/carts-demo/CREDITS.md'), 'utf8').split('\n').filter((l) => l.startsWith('| `') && cartsUsed.some((c) => l.startsWith(`| \`${c}.opus\``)));
  writeFileSync(join(OUT, 'CREDITS.txt'), [
    'WeaveSignal demo video: credits',
    '',
    'Kettle FM and The Morning Kettle are fictional. The names are sample data from the project\'s seeds; no real person is portrayed.',
    'Every voice is designed with OmniVoice\'s voice design (a description of gender, age, pitch and accent, and a seed), not cloned from any person, and read through VoiceLab\'s tts-serve.',
    'Filmed through WeaveSignal\'s own desk, playout and listener process on one machine (tests/demo-video/ in the repository).',
    '',
    'Sounds, from the repository\'s demo cart pack (assets/carts-demo/CREDITS.md):',
    ...credits.map((l) => {
      const c = l.split('|').map((x) => x.trim());
      const src = /^\[([^\]]+)\]\(([^)]+)\)/.exec(c[4]);
      return `- ${c[2]} (${c[1].replace(/`/g, '')}): ${c[5]}, ${c[6]} ${c[7]}; from ${src ? `${src[1]} <${src[2]}>` : c[4]}`;
    }),
    '- The default loop (assets/default-loop), heard as automation takes the air: made by the repository\'s assets/default-loop/make.sh.',
    '',
    'WeaveSignal is free software under the AGPL-3.0. https://weavesignal.top',
    '',
  ].join('\n'));
}
log('done');
