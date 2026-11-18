// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Hosts in their own studios in the real build of the studio app (pnpm tauri build --debug
// --no-bundle), driven over WebDriver in the app's own webview (WebKitGTK), which the browser
// driver (Chromium) cannot stand in for: the source's tile under Not on air with its lines, On
// from the tile through the app's own requests, and Settings › Own studios with the setup and
// the details shown once (the dialog laid out with its way out inside the window, Esc leaving
// it open, Copy everything reaching the clipboard or else offering the text to copy by hand).
//
// What this does not prove: a real host's software (that is `just test-sources`), or a real
// keyboard: keys are sent by WebDriver.
import fs from 'node:fs';
import { open, sleep } from '../../stations/wd.mjs';

const run = `${process.env.RUN}/`;
const STATION = `http://127.0.0.1:${process.env.STATION_PORT}`;
const out = [];
const ok = (name, pass, detail = '') => {
  out.push(`${pass ? 'PASS' : 'FAIL'}  ${name}${detail && !pass ? '  [' + detail + ']' : ''}`);
  console.log(out.at(-1));
};
const station = async (what) => (await fetch(`${STATION}/__${what}`)).json();

const s = await open(process.env.STUDIO ?? new URL('../../../target/debug/weavesignal-studio', import.meta.url).pathname);
const text = (css) => s.js(`const e=document.querySelector(arguments[0]); return e ? e.innerText.replace(/\\s+/g,' ').trim() : null;`, css);
const value = (css) => s.js(`return document.querySelector(arguments[0])?.value ?? null;`, css);
const pressIn = (css, label) => s.js(`const b=[...document.querySelectorAll(arguments[0]+' button')].find(b=>b.textContent.replace(/\\s+/g,' ').trim().startsWith(arguments[1])); if(b) b.click(); return !!b;`, css, label);
/** Inside the window, in the page's own pixels. */
const inside = (css) => s.js(`const e=document.querySelector(arguments[0]); if(!e) return false; const r=e.getBoundingClientRect(); return r.width>0 && r.top>=0 && r.left>=0 && r.bottom<=innerHeight+0.5 && r.right<=innerWidth+0.5;`, css);
const TILE = '[data-panel="desk.waiting"] [data-panel="desk.tile.5"]';
const HOSTS = '[data-panel="settings.hosts"]';
const DET = 'dialog[data-panel="settings.hosts.details"]';
try {
  // Sign in with the station key: the desk is the first tab.
  await s.waitFor('[data-panel="setup.sign-in"]', 30000);
  await s.js(`[...document.querySelectorAll('.other')].find(b=>b.textContent.includes('station key')).click()`);
  await s.waitFor('#si-key');
  await s.type('#si-url', STATION);
  await s.type('#si-key', 'proof-key-hosts-screens');
  await s.click('button[type=submit]');
  await s.waitFor('[data-panel="studio.frame"]', 30000);

  // ---- 1. The source's tile, in this webview ----
  await s.waitFor(TILE, 20000);
  await sleep(1200);
  const t = await text(TILE);
  ok('in the app’s own webview a host’s own studio stands by under Not on air', /Finlay Grant/.test(t) && /Own studio/.test(t) && /STANDBY|Standby/i.test(t), t);
  ok('with its level, the leveller, the format and the steady time', /LUFS/.test(t) && /leveller −7\.4 dB/.test(t) && /mpegts\/aac 48 kHz stereo/.test(t) && /SRT · steady \d+:\d\d/.test(t), t);
  ok('the leveller is drawn on the fader’s track, left of the middle, inside the tile', await s.js(`const t=document.querySelector(arguments[0]); const f=t.querySelector('.hfader.leveller'); const k=f.querySelector('.knob').getBoundingClientRect(); const z=f.querySelector('.z').getBoundingClientRect(); const r=f.getBoundingClientRect(); const b=t.getBoundingClientRect(); return r.width>50 && k.left<z.left && r.right<=b.right && r.bottom<=b.bottom;`, TILE));
  ok('nothing in the tile runs out of it', await s.js(`const t=document.querySelector(arguments[0]); const b=t.getBoundingClientRect(); return [...t.querySelectorAll('button,.state,.rd,.lines > span')].every(e=>{const r=e.getBoundingClientRect(); return !r.width || (r.right<=b.right+1 && r.left>=b.left-1 && r.bottom<=b.bottom+1);});`, TILE));
  await s.shot(run + 'a1-desk-standing-by.png');
  // On, from the tile: the app's own request goes to the desk's command, and the tile moves.
  await pressIn(TILE + ' .controls', 'On');
  await s.wait('the source on the stage', () => s.has('.tiles [data-panel="desk.tile.5"].is-onair'), 8000);
  const sent = await station('calls');
  ok('On goes through the desk’s own command, from the app itself', sent.some((c) => c.path === '/api/v1/desk/commands' && c.body.op === 'on_air' && c.body.on === true && c.body.identity.startsWith('source-')), JSON.stringify(sent.slice(-2)));
  ok('and the tile is on the stage, ON AIR', /On air/i.test(await text('.tiles [data-panel="desk.tile.5"] .chips')));
  await s.shot(run + 'a2-desk-on-air.png');

  // ---- 2. Settings › Own studios ----
  await s.click('button.settings-tab');
  await s.waitFor(`${HOSTS} table`, 15000);
  await s.js(`document.querySelector(arguments[0]).scrollIntoView({block:'start'})`, HOSTS);
  await sleep(500);
  const row = await text(`${HOSTS} tbody tr`);
  ok('the key’s row says ON AIR, the way in and the format', /Finlay Grant/.test(row) && /On air/i.test(row) && /SRT · mpegts\/aac 48 kHz stereo · strip 5/.test(row), row);
  ok('an admin has the rules', await s.has('[data-panel="settings.hosts.rules"] #hr-port_srt'));
  await s.shot(run + 'a3-settings-rows.png');

  await pressIn(`${HOSTS} .ph`, 'Set up a host');
  await s.waitFor('dialog[data-panel="settings.hosts.setup"][open]');
  await s.js(`const e=document.querySelector('#hk-show'); e.value='kettle-drive'; e.dispatchEvent(new Event('change',{bubbles:true}));`);
  await s.type('#hk-label', 'Heather Douglas');
  await s.shot(run + 'a4-setup.png');
  await pressIn('dialog[data-panel="settings.hosts.setup"]', 'Make the key');
  await s.waitFor(`${DET}[open]`, 8000);
  await sleep(400);
  const key = (await station('keys')).find((k) => k.label === 'Heather Douglas');
  ok('the key is made with the fields the station takes', JSON.stringify((await station('calls')).find((c) => c.path === '/api/v1/shows/kettle-drive/keys')?.body) === JSON.stringify({ label: 'Heather Douglas', hint: 'icecast' }));
  ok('the details hold the secret, and the address this app reaches the station at', (await value('#hd-icecast-4')) === `${key.key_id}-${key.secret}` && (await value('#hd-srt-3')) === key.secret && (await value('#hd-icecast-0')) === '127.0.0.1');
  ok('the details are a modal dialog laid out in this webview: heading, both ways and the way out inside the window', (await inside(`${DET} h2`)) && (await inside(`${DET} .acts .btn.primary`)) && (await s.js(`const b=[...document.querySelectorAll(arguments[0]+' .waybox')].map(e=>e.getBoundingClientRect()); return b.length===3 && Math.abs(b[0].top-b[1].top)<2 && b[1].left>b[0].right;`, DET)), JSON.stringify(await s.js(`return [...document.querySelectorAll(arguments[0]+' .waybox')].map(e=>{const r=e.getBoundingClientRect(); return [Math.round(r.left),Math.round(r.top),Math.round(r.right)]})`, DET)));
  ok('first focus is on Copy everything, not on the way out', await s.js(`return document.activeElement?.id === 'hk-all'`));
  // Esc, twice: what cannot be shown again stays.
  await s.keys('');
  await sleep(200);
  await s.keys('').catch(() => {});
  await sleep(400);
  ok('Esc twice leaves the details open, the secret still in them', (await s.has(`${DET}[open]`)) && (await value('#hd-srt-3')) === key.secret);
  // Copy everything: the clipboard, or the text to copy by hand where this webview has none.
  await pressIn(`${DET} .acts`, 'Copy everything');
  await sleep(600);
  const copied = /Copied everything/.test(await text(`${DET} .acts`));
  const byHand = await value('#hk-all-text');
  ok('Copy everything reaches the clipboard, or offers the text to copy by hand', copied || (byHand ?? '').includes(`Password: ${key.key_id}-${key.secret}`), `copied=${copied} byHand=${!!byHand}`);
  console.log(`      (in this webview Copy everything ${copied ? 'reached the clipboard' : 'could not reach the clipboard: the text is drawn to copy by hand'})`);
  await s.shot(run + 'a5-details-once.png');
  await pressIn(`${DET} .acts`, 'I have sent them');
  await s.wait('the details closed', async () => !(await s.has(`${DET}[open]`)), 5000);
  await sleep(600);
  ok('after I have sent them the secret is nowhere on the page, and the key has its row', !(await s.js(`return document.documentElement.innerHTML.includes(arguments[0])`, key.secret)) && (await s.has(`${HOSTS} tr[data-key="${key.key_id}"]`)));
  await s.js(`document.querySelector(arguments[0]).scrollIntoView({block:'start'})`, HOSTS);
  await s.shot(run + 'a6-settings-after.png');
} catch (e) {
  ok('the run reached its end', false, String(e).slice(0, 300));
  await s.shot(run + 'a-failed.png').catch(() => {});
} finally {
  await s.close();
  fs.writeFileSync(run + 'result.txt', out.join('\n') + '\n');
}
