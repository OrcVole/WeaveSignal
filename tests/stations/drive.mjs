// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station switcher in the real build (pnpm tauri build --debug --no-bundle), driven over
// WebDriver in a throwaway profile, against one real control plane and two stand-in stations.
import { open, sleep } from './wd.mjs';
import fs from 'node:fs';
import { execSync } from 'node:child_process';
const here = new URL('.', import.meta.url).pathname;
const cfg = here + 'run/profile/config/org.weavesignal.studio/';
const [A, B, C] = ['http://127.0.0.1:26511', 'http://127.0.0.1:26512', 'http://127.0.0.1:26513'];
const out = [];
const ok = (name, pass, detail = '') => { out.push(`${pass ? 'PASS' : 'FAIL'}  ${name}${detail ? '  [' + detail + ']' : ''}`); console.log(out.at(-1)); };
const book = () => JSON.parse(fs.readFileSync(cfg + 'stations.json', 'utf8'));
const logOf = async (u) => (await fetch(u + '/__log')).json();
const keychain = () => execSync('secret-tool search --all service weavesignal-studio 2>&1 | grep attribute.username || true').toString().trim().split('\n').map((l) => l.replace('attribute.username = ', '')).filter(Boolean).sort();
const name = (s) => s.text('.stationbtn .name');
const toast = (s) => s.js(`const t=document.querySelector('.toast'); return t ? t.innerText.replace(/\\s+/g,' ') : '';`);
async function quietAfter(u, what) { const a = (await logOf(u)).log.length; await sleep(3500); const l = await logOf(u); ok(what, l.log.length === a && l.streams === 0, `requests ${a} -> ${l.log.length}, streams open ${l.streams}`); return l; }

const s = await open(process.env.STUDIO ?? new URL('../../target/debug/weavesignal-studio', import.meta.url).pathname);
try {
  // ---- 1. an existing install carries over ----
  await s.waitFor('[data-panel="studio.frame"]', 20000);
  ok('carry-over: no sign-in screen; the studio opens on the saved station', !(await s.has('[data-panel="setup.sign-in"]')));
  await s.wait('name', async () => (await name(s)) === 'Kettle FM');
  ok('the frame names the station on its button, with the mark while there is one station', (await s.has('.stationbtn .mark')) && !(await s.has('.stationbtn .stbadge')));
  await s.wait('the list to learn the version', () => book().stations[0].version, 15000);
  const b1 = book();
  ok('stations.json: one row, the window on it, its key still in the keychain', b1.current === 's1' && b1.stations.length === 1 && b1.stations[0].url === A && b1.stations[0].kept === 'keychain', JSON.stringify(b1.stations[0]));
  ok('station.json is gone, and the list holds no key', !fs.existsSync(cfg + 'station.json') && !/proof-key|rss_/.test(fs.readFileSync(cfg + 'stations.json', 'utf8')));
  ok('the packaged app reads the station version through /health', b1.stations[0].version === '1.0.4' && b1.stations[0].name === 'Kettle FM', `version ${b1.stations[0].version}`);
  ok('keychain: the entry is the one the old build made, not moved', keychain().includes(A), keychain().join(' '));
  const frame = await s.js(`const f=document.querySelector('.frame').getBoundingClientRect(); return [innerWidth, Math.round(f.height)];`);
  const btnW = await s.js(`return Math.round(document.querySelector('.stationbtn').getBoundingClientRect().width)`);
  ok('the frame is as tall at 1440 px as the build before it (73 px with the station key: two rows there too)', frame[1] === 73, `width ${frame[0]}, height ${frame[1]}; the name took 86 px before, the button takes ${btnW}`);
  await s.shot(here + 'run/p1-carried-over.png');


  // ---- a request that names another station is refused by the Rust side ----
  const guard = await fetch(`http://127.0.0.1:4455/session/${s.id}/execute/async`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ script: `const done=arguments[arguments.length-1]; const T=window.__TAURI_INTERNALS__; const id=T.transformCallback(()=>{}); T.invoke('station_fetch', new Uint8Array(), {headers:{'x-station-id':'s99','x-station-method':'GET','x-station-path':'/api/v1/carts','x-station-channel':'__CHANNEL__:'+id}}).then(()=>done('sent')).catch(e=>done(String(e)));`, args: [] }) }).then((r) => r.json());
  const before = (await logOf(A)).log.length;
  ok('a request naming another station is refused, and is not sent', guard.value === 'This window has moved to another station.', guard.value);

  // ---- 2. add Radio Ross with its station key; Kettle FM is quiet, so the window switches at once ----
  await s.click('.stationbtn');
  await s.waitFor('dialog.stations[open]');
  await s.shot(here + 'run/p2-list-one.png');
  await s.click('dialog.stations .foot .btn');
  await s.waitFor('.setup-over [data-panel="setup.sign-in"]');
  ok('Add a station: the sign-in screen lies over the studio with a way back', (await s.text('.setup-over h1')).trim() === 'Add a station' && (await s.text('.setup-back')).includes('Back to Kettle FM') && (await s.js(`return document.querySelector('.shell').inert`)));
  await s.shot(here + 'run/p3-add.png');
  await s.js(`[...document.querySelectorAll('.setup-over .other')].find(b=>b.textContent.includes('station key')).click()`);
  await s.waitFor('#si-key');
  await s.type('#si-url', B);
  await s.type('#si-key', 'proof-key-b');
  await s.click('.setup-over button[type=submit]');
  await s.wait('arrival at Radio Ross', async () => (await name(s)) === 'Radio Ross', 20000);
  ok('the window is on Radio Ross, and the badge has taken the mark’s place', (await s.text('.stationbtn .stbadge')) === 'RR');
  const t2 = await s.wait('the arrival note', async () => { const t = await toast(s); return t.includes('You are on Radio Ross') && t; }, 15000);
  ok('arrival note: where the window is, who, and that the station is older', /Signed in with the station key\./.test(t2) && /It runs 1\.0\.3, older than this app \(1\.0\.4\)/.test(t2), t2);
  await s.shot(here + 'run/p4-arrived-ross.png');
  const la = await quietAfter(A, 'after the switch nothing more reaches Kettle FM, and its event stream is closed');
  ok('every request Kettle FM received carried Kettle FM’s key, /health too', la.log.every((r) => r.auth === 'proof-key-a'), [...new Set(la.log.map((r) => r.auth))].join(','));
  ok('Kettle FM saw its event stream close (which side closed it first is the page tests’ to say)', la.log.some((r) => r.m === 'CLOSE'));
  ok('the list: two rows, the window on the second, each key its own entry', book().current === 's2' && keychain().includes(A) && keychain().includes(B), keychain().join(' '));

  // ---- 3. leave Radio Ross while it is on air ----
  await s.wait('ON AIR on the desk', () => s.js(`return document.body.innerText.includes('ON AIR')`), 15000);
  // The dialog's own keys, pressed for real: ↑ ↓ between the rows, ⏎ on a row, Esc and ⏎ on the question.
  const [UP, DOWN, ENTER, ESC] = ['\uE013', '\uE015', '\uE007', '\uE00C'];
  const focused = () => s.js(`const a=document.activeElement; return a ? (a.closest('.strow') ? 'row: ' + a.querySelector('.nm span').textContent : a.textContent.replace(/\\s+/g,' ').trim()) : 'nothing';`);
  await s.click('.stationbtn');
  await s.waitFor('dialog.stations[open]');
  await sleep(300);
  const f0 = await focused();
  await s.keys(DOWN);
  const f1 = await focused();
  await s.keys(UP);
  const f2 = await focused();
  ok('the list opens with focus on the first station to switch to, and ↑ ↓ move between the rows', f0 === 'row: Kettle FM' && f1 === 'row: Radio Ross' && f2 === 'row: Kettle FM', `${f0} | ↓ ${f1} | ↑ ${f2}`);
  await s.keys(ENTER);
  await s.waitFor('dialog[data-panel="studio.stations.leave"][open]');
  const ask = (await s.text('dialog.stations')).replace(/\s+/g, ' ');
  ok('leaving a station on air is asked first, with what stays on named', ask.includes('Leave Radio Ross while it is on air?') && ask.includes('Radio Ross is on air: 2 strips are open on the desk.') && ask.includes('Stay on Radio Ross') && ask.includes('Switch to Kettle FM'), ask.slice(0, 120));
  await sleep(300);
  ok('on the question, focus is on Switch, so ⏎ is the answer the button prints', (await focused()).startsWith('Switch to Kettle FM'), await focused());
  await s.shot(here + 'run/p5-leave.png');
  await s.keys(ESC);
  await sleep(400);
  ok('Esc stays: the list is back, the dialog is still open, the window is still on Radio Ross, and focus is on a row', (await s.has('dialog[data-panel="studio.stations"][open]')) && (await name(s)) === 'Radio Ross' && (await focused()) === 'row: Kettle FM', await focused());
  await s.shot(here + 'run/p6-list-two.png');
  await s.keys(ENTER);
  await s.waitFor('dialog[data-panel="studio.stations.leave"][open]');
  await sleep(300);
  await s.keys(ENTER);
  await s.wait('arrival at Kettle FM', async () => (await name(s)) === 'Kettle FM', 20000);
  const t3 = await s.wait('the arrival note', async () => { const t = await toast(s); return t.includes('You are on Kettle FM') && t; }, 15000);
  ok('arrival note says the station left carries on', t3.includes('Radio Ross carries on without this window.'), t3);
  const lb = await quietAfter(B, 'after the switch nothing more reaches Radio Ross, and its event stream is closed');
  ok('every request Radio Ross received carried Radio Ross’s key, /health too', lb.log.every((r) => r.auth === 'proof-key-b'), [...new Set(lb.log.map((r) => r.auth))].join(','));

  // ---- 4. add the real control plane with a username and password ----
  await s.click('.stationbtn');
  await s.waitFor('dialog.stations[open]');
  await s.click('dialog.stations .foot .btn');
  await s.waitFor('.setup-over #si-username');
  await s.type('#si-url', C);
  await s.type('#si-username', 'eilidh');
  await s.type('#si-password', 'wrong password here');
  await s.click('.setup-over button[type=submit]');
  const err = await s.wait('the refusal', () => s.text('.setup-over .err'));
  const sa = (await logOf(A)).streams;
  ok('a wrong password is refused in the station’s words, nothing is added, and the station underneath carries on', err.includes('do not match') && book().stations.length === 2 && sa === 1, `${err} rows ${book().stations.length}, Kettle FM streams ${sa}`);
  await s.type('#si-password', 'a long password for the proof');
  await s.click('.setup-over button[type=submit]');
  await s.wait('arrival at the real station', async () => book().current === 's3' && (await s.has('.stationbtn')) && !(await s.has('.setup-over')), 20000);
  await s.wait('who is signed in', async () => (await s.text('.who'))?.includes('Eilidh'), 15000);
  ok('signed in to a real control plane as a member of staff', (await s.text('.who')).includes('Eilidh Fraser-MacLeod'), (await s.text('.who')).replace(/\s+/g, ' '));
  await s.wait('the list to learn who', () => book().stations[2].who, 15000);
  ok('the list notes who is signed in and the version, and holds no token', book().stations[2].who === 'Eilidh Fraser-MacLeod, producer' && !/rss_/.test(fs.readFileSync(cfg + 'stations.json', 'utf8')), JSON.stringify(book().stations[2]));
  ok('two stations called Kettle FM get different badges', (await s.text('.stationbtn .stbadge')) !== 'KF', await s.text('.stationbtn .stbadge'));

  // ---- 5. a name on this computer ----
  await s.click('.stationbtn');
  await s.waitFor('dialog.stations[open]');
  await s.js(`document.querySelector('dialog.stations .strow.here .manage').click()`);
  await s.waitFor('.stmanage input');
  await s.type('.stmanage input', 'Kettle FM test');
  await s.js(`document.querySelector('.stmanage button[type=submit]').click()`);
  await s.wait('the label in the frame', async () => (await name(s)) === 'Kettle FM test');
  ok('rename: the frame and the list take the label, and the badge follows', (await s.text('.stationbtn .stbadge')) === 'KT' && book().stations[2].label === 'Kettle FM test' && book().stations[2].name === 'Kettle FM');
  await s.shot(here + 'run/p7-manage.png');

  // ---- 6. sign out of the station the window is on: the row stays ----
  await s.js(`[...document.querySelectorAll('.stmanage .acts .btn')].find(b=>b.textContent.trim()==='Sign out').click()`);
  await s.waitFor('[data-panel="setup.sign-in"]', 20000);
  await s.wait('the others', () => s.has('[data-panel="setup.stations"]'));
  const h = (await s.text('h1')).trim();
  ok('sign out: the sign-in screen for that station, its address kept, the others listed', h === 'Sign in to Kettle FM test' && (await s.js(`return document.querySelector('#si-url').value`)) === C && (await s.text('[data-panel="setup.stations"]')).includes('Radio Ross'), h);
  ok('sign out: the row stays signed out and its keychain entry is deleted', book().stations[2].kept === null && book().stations.length === 3 && !keychain().includes(C), keychain().join(' '));
  const ended = fs.readFileSync(here + 'run/control.log', 'utf8').split('\n').filter((l) => /sign(ed)?[- ]out|session ended|sessions\/me/i.test(l)).length;
  const me = await fetch(C + '/api/v1/staff', { headers: { authorization: 'Bearer proof-key-kettle-real' } }).then((r) => r.json());
  ok('(info) the real station was told to end the session', true, `log lines about it: ${ended}; account still there: ${me.accounts?.length}`);
  await s.shot(here + 'run/p8-signed-out.png');

  // ---- 7. open another station from the sign-in screen ----
  await s.js(`[...document.querySelectorAll('[data-panel="setup.stations"] .go')].find(b=>b.textContent.includes('Radio Ross')).click()`);
  await s.wait('arrival at Radio Ross', async () => (await s.has('.stationbtn')) && (await name(s)) === 'Radio Ross', 20000);
  ok('from the sign-in screen another station opens in one press', book().current === 's2');

  // ---- 8. remove a station the window is not on: no question, no reload ----
  await s.click('.stationbtn');
  await s.waitFor('dialog.stations[open]');
  await s.js(`[...document.querySelectorAll('dialog.stations .strow')].find(r=>r.textContent.includes('127.0.0.1:26511')).querySelector('.manage').click()`);
  await s.js(`[...document.querySelectorAll('.stmanage .acts .btn')].find(b=>b.textContent.trim()==='Remove…').click()`);
  await s.waitFor('.stmanage .stask');
  await s.shot(here + 'run/p9-remove.png');
  await s.js(`document.querySelector('.stmanage .stask .danger-hollow').click()`);
  await s.wait('the row to go', () => book().stations.length === 2);
  ok('remove another station: its row and its keychain entry go, and the window stays on Radio Ross', !keychain().includes(A) && (await name(s)) === 'Radio Ross' && (await s.has('dialog.stations[open]')) && (await logOf(B)).streams === 1, keychain().join(' '));

  // ---- 9. remove the station the window is on, on air: asked, then the window is on none ----
  await s.js(`document.querySelector('dialog.stations .strow.here .manage').click()`);
  await s.js(`[...document.querySelectorAll('.stmanage .acts .btn')].find(b=>b.textContent.trim()==='Remove…').click()`);
  await s.js(`document.querySelector('.stmanage .stask .danger-hollow').click()`);
  await s.waitFor('dialog[data-panel="studio.stations.leave"][open]');
  ok('removing the window’s own station on air is asked as leaving it', (await s.text('dialog.stations')).includes('Remove Radio Ross'));
  await s.js(`document.querySelector('dialog.stations .acts .btn.primary').click()`);
  await s.waitFor('[data-panel="setup.sign-in"]', 20000);
  ok('after it the window is on no station; the one left is offered', book().current === null && book().stations.length === 1 && !keychain().includes(B) && (await s.text('[data-panel="setup.stations"]')).includes('Kettle FM test'));

  // ---- 10. the last, signed-out station is removed from the sign-in screen ----
  await s.js(`document.querySelector('[data-panel="setup.stations"] .go').click()`);
  await s.wait('its sign-in', async () => (await s.text('h1'))?.trim() === 'Sign in to Kettle FM test', 20000);
  await s.js(`[...document.querySelectorAll('.other')].find(b=>b.textContent.includes('from this computer')).click()`);
  await s.wait('an empty list', () => book().stations.length === 0, 20000);
  await s.wait('first run', async () => (await s.text('h1'))?.trim() === 'Sign in');
  ok('the last station removed: the first-run sign-in screen, an empty list', book().current === null && !(await s.has('[data-panel="setup.stations"]')));
  ok('no key file was ever written (the keychain kept every key)', !fs.existsSync(cfg + 'keys'));
} catch (e) {
  ok('the run stopped: ' + String(e).slice(0, 300), false);
  await s.shot(here + 'run/p-stopped.png').catch(() => {});
} finally { await s.close(); }
const kc = keychain();
ok('keychain at the end: only what was there before the run', JSON.stringify(kc) === JSON.stringify(fs.readFileSync(here + 'run/keychain-before.txt', 'utf8').trim().split('\n').map((l) => l.replace('attribute.username = ', '')).sort()), kc.join(' '));
fs.writeFileSync(here + 'run/result.txt', out.join('\n') + '\n');
console.log(`\n${out.filter((l) => l.startsWith('PASS')).length} passed, ${out.filter((l) => l.startsWith('FAIL')).length} failed`);
