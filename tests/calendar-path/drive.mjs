// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Import a calendar… in the real build of the studio app (pnpm tauri build --debug
// --no-bundle), driven over WebDriver: the path Download .ics said is pasted straight into the
// import; a path in quotes with ~; the refusals of `calendar_read` as the page shows them; and
// a file dropped on the window, as Tauri's own drag-and-drop event tells the page. It also
// looks at the week's fold in the app's own webview (WebKitGTK), which the browser drivers
// (Chromium) cannot: the tabs' size, that nothing is drawn over one, and an hour's block.
//
// What this does not prove: the system's own drag from a file manager. There is no pointer on
// the headless display, so the drop is given to the page as the event Tauri emits for one
// (`tauri://drag-drop`, with the path), not by dragging a file over the window.
import fs from 'node:fs';
import { open, sleep } from '../stations/wd.mjs';

const here = new URL('.', import.meta.url).pathname;
const run = here + 'run/';
const HOME = run + 'home';
const STATION = `http://127.0.0.1:${process.env.STATION_PORT}`;
const out = [];
const ok = (name, pass, detail = '') => {
  out.push(`${pass ? 'PASS' : 'FAIL'}  ${name}${detail ? '  [' + detail + ']' : ''}`);
  console.log(out.at(-1));
};
const imports = async () => (await fetch(STATION + '/__imports')).json();
const SAMPLE = fs.readFileSync(here + '../newshow/kettle-old-schedule.ics', 'utf8').replace(/\r?\n/g, '\r\n');

const s = await open(process.env.STUDIO ?? new URL('../../target/debug/weavesignal-studio', import.meta.url).pathname);
const press = (label) => s.js(`const b=[...document.querySelectorAll('button,[role=menuitem]')].find(b=>b.textContent.replace(/\\s+/g,' ').includes(arguments[0])); if(b) b.click(); return !!b;`, label);
const text = (css) => s.js(`const e=document.querySelector(arguments[0]); return e ? e.innerText.replace(/\\s+/g,' ').trim() : null;`, css);
const DLG = 'dialog[data-panel="calendar.import"]';
const openImport = async () => {
  await press('Import a calendar…');
  await s.waitFor(`${DLG}[open]`);
};
const closeImport = async () => {
  await s.js(`document.querySelector(arguments[0])?.close()`, DLG);
  await s.wait('the import closed', async () => !(await s.has(DLG)));
};
/** Put a path in the field as a paste would, and read the file. */
const paste = async (path) => {
  await s.type('#ic-path', path);
  await s.click(`${DLG} .pathrow button[type=submit]`);
};
try {
  // Sign in with the station key, and go to the Schedule tab.
  await s.waitFor('[data-panel="setup.sign-in"]', 30000);
  await s.js(`[...document.querySelectorAll('.other')].find(b=>b.textContent.includes('station key')).click()`);
  await s.waitFor('#si-key');
  await s.type('#si-url', STATION);
  await s.type('#si-key', 'proof-key-calendar-path');
  await s.click('button[type=submit]');
  await s.waitFor('[data-panel="studio.frame"]', 30000);
  await s.wait('the Schedule tab', async () => (await press('Schedule')) && (await s.has('.schedule .ec')), 20000);
  await sleep(800);

  // ---- 0. The week's fold, in this webview: the week of Monday 16 November 2026 ----
  const monday = await s.js(`const d=new Date(); d.setHours(12,0,0,0); d.setDate(d.getDate()-((d.getDay()+6)%7)); return d.getTime();`);
  const weeks = Math.round((new Date(2026, 10, 16, 12).getTime() - monday) / (7 * 86_400_000));
  for (let i = 0; i < Math.abs(weeks); i++) {
    await s.click(`button[aria-label="${weeks > 0 ? 'Next week' : 'Previous week'}"]`);
    await sleep(700);
  }
  await s.wait('the week of 16 November', async () => ((await text('[data-panel="calendar.toolbar"] .range')) ?? '').startsWith('16 – 22 November 2026'));
  await s.js(`document.querySelector('.ec-main').scrollTop = 11 * 48 - 24`);
  await sleep(600);
  const fold = await s.js(`
    const zoom = Number(getComputedStyle(document.documentElement).zoom) || 1;
    const tabs = [...document.querySelectorAll('.ec .ftab')].map((e) => { const r = e.getBoundingClientRect(); const at = [0.15, 0.5, 0.85].map((f) => document.elementFromPoint(r.left + r.width * f, r.top + r.height / 2)).find((x) => !x || !e.contains(x)) ?? e; const c = e.querySelector('.chev').getBoundingClientRect(); return { w: r.width / zoom, h: r.height / zoom, top: e.contains(at), over: at && !e.contains(at) ? (at.closest('.ec-event')?.dataset.key ?? at.className ?? at.tagName) + ' at ' + Math.round(r.left) + ',' + Math.round(r.top) + ' of ' + e.closest('.ec-event').dataset.key : '', chev: c.width / zoom, ground: getComputedStyle(e).backgroundColor }; });
    const hour = document.querySelector('.ec .ec-event[data-key="ep:steeped-1@1"]');
    const box = hour.getBoundingClientRect();
    const lines = ['.t', '.evb > b', '.over'].map((q) => { const r = hour.querySelector(q).getBoundingClientRect(); return [r.top, r.bottom, r.height]; });
    return { tabs, over: hour.querySelector('.over').innerText.trim(), inside: lines[2][1] <= box.bottom + 0.5 && lines[2][2] > 8, apart: lines[0][1] <= lines[1][0] + 0.5 && lines[1][1] <= lines[2][0] + 0.5 };`);
  ok('in the app’s own webview every folded block is a tab the strip’s width, filled, with its chevron, and nothing takes the pointer over it, the week’s scrollbar on Sunday included', fold.tabs.length >= 5 && fold.tabs.every((t) => t.w >= 23.5 && t.h >= 23.5 && t.top && t.chev >= 10 && !/rgba\(.*,\s*0\)|transparent/.test(t.ground)), JSON.stringify(fold.tabs.map((t) => `${Math.round(t.w)}x${Math.round(t.h)}${t.top ? '' : ` covered by ${t.over}`}`)));
  ok('and an hour’s block over a long show says what it is over, on three lines inside the block', fold.over === 'over The Long Lunch' && fold.inside && fold.apart, JSON.stringify([fold.over, fold.inside, fold.apart]));
  await s.shot(run + 'p0-week-fold.png');

  // ---- 1. Download .ics, then paste what it said straight into the import ----
  await press('Subscribe…');
  await s.waitFor('dialog[data-panel="calendar.subscribe"][open]');
  await s.click('#sub-download');
  const said = await s.wait('where it was saved', async () => { const t = await text('[data-testid="sub-said"]'); return t && t.startsWith('Saved as ') && t; });
  const saved = said.replace(/^Saved as /, '');
  ok('Download .ics saves into the Downloads folder and says where', fs.existsSync(saved) && fs.realpathSync(saved) === fs.realpathSync(`${HOME}/Downloads/schedule.ics`), said);
  await s.js(`document.querySelector('dialog[data-panel="calendar.subscribe"]')?.close()`);
  await openImport();
  ok('the packaged app’s import has the field for a path, with Read the file waiting for one', (await s.has('#ic-path')) && (await text(`${DLG} label[for="ic-path"]`)) === 'Or paste the file’s path' && (await s.js(`return document.querySelector(arguments[0]).disabled`, `${DLG} .pathrow button[type=submit]`)) === true);
  await s.shot(run + 'p1-import-path-field.png');
  await paste(said);
  await s.wait('the shows the file proposes', () => s.has(`${DLG}[data-part="2"] li.item`), 15000);
  const first = await imports();
  ok('what Download .ics said pastes straight in: the file is read by the app and sent to the station as it is', first.length === 1 && first[0].path === '/api/v1/schedule/import/preview' && first[0].body.ics === fs.readFileSync(saved, 'utf8') && (await text(`${DLG} h2`)) === 'Import a calendar · schedule.ics', await text(`${DLG} h2`));
  await s.shot(run + 'p2-import-from-pasted-path.png');
  await closeImport();

  // ---- 2. A path in quotes, with ~ and a space, as a file manager copies it ----
  fs.writeFileSync(`${HOME}/Downloads/schedule (2).ics`, SAMPLE);
  await openImport();
  await paste(` '~/Downloads/schedule (2).ics' `);
  await s.wait('the second file read', async () => (await text(`${DLG} h2`)) === 'Import a calendar · schedule (2).ics', 15000);
  const second = await imports();
  ok('a path in quotes with ~ and a space is read', second.length === 2 && second[1].body.ics === SAMPLE);
  await closeImport();

  // ---- 3. Only a calendar file is read: the refusals, as the page shows them ----
  fs.writeFileSync(`${HOME}/Downloads/notes.txt`, SAMPLE);
  fs.writeFileSync(`${HOME}/Downloads/settings.ics`, 'station_key = "hunter2-the-station-key"\n');
  fs.writeFileSync(`${HOME}/Downloads/large.ics`, SAMPLE + 'X'.repeat(1024 * 1024));
  await openImport();
  const refused = async (path, want) => {
    await paste(path);
    const got = await s.wait(`the refusal of ${path}`, async () => { const t = await text('[data-testid="ic-problem"]'); return t === want && t; }, 8000).catch(async () => await text('[data-testid="ic-problem"]'));
    ok(`refused: ${path}`, got === want && (await s.has(`${DLG}[data-part="1"]`)), got);
  };
  await refused('~/Downloads/notes.txt', 'Only a calendar file is read: its name must end in .ics.');
  await refused('/etc/passwd', 'Only a calendar file is read: its name must end in .ics.');
  await refused('~/Downloads/settings.ics', 'That is not a calendar file: it does not begin with BEGIN:VCALENDAR.');
  await refused('~/Downloads/large.ics', 'That file is larger than 1 MB. Export a shorter range of dates, or one calendar at a time.');
  await refused('~/Downloads/gone.ics', 'No file is at that path.');
  await refused('Downloads/schedule.ics', 'Give the whole path of the file, from the top folder or from ~.');
  ok('nothing refused was sent to the station, and nothing of a refused file is on the page', (await imports()).length === 2 && !(await s.js(`return document.body.innerText.includes('hunter2')`)));
  await s.shot(run + 'p3-import-path-refused.png');

  // ---- 4. A file dropped on the window, as Tauri tells the page ----
  const emit = (event, payload) => s.js(`window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: arguments[0], payload: arguments[1] }); return true;`, event, payload);
  const dropPath = `${HOME}/Downloads/schedule (2).ics`;
  await emit('tauri://drag-enter', { paths: [dropPath], position: { x: 400, y: 300 } });
  await s.wait('the dialog to show the drop', async () => (await text('[data-testid="ic-drop"]')) === 'Let go to read the file.', 5000).catch(() => {});
  ok('a file dragged over the window shows where it will land', (await text('[data-testid="ic-drop"]')) === 'Let go to read the file.' && (await s.js(`return document.querySelector(arguments[0]).classList.contains('over')`, DLG)));
  await s.shot(run + 'p4-import-drag-over.png');
  await emit('tauri://drag-drop', { paths: [`${HOME}/Downloads/notes.txt`], position: { x: 400, y: 300 } });
  await sleep(500);
  ok('a dropped file that is not named as a calendar is said and not read', (await text('[data-testid="ic-problem"]')) === 'notes.txt is not a calendar file: its name does not end in .ics.' && (await imports()).length === 2);
  await emit('tauri://drag-drop', { paths: [dropPath], position: { x: 400, y: 300 } });
  await s.wait('the dropped file read', async () => (await text(`${DLG} h2`)) === 'Import a calendar · schedule (2).ics', 15000);
  const third = await imports();
  ok('a dropped calendar file is read through the same command and its shows are listed', third.length === 3 && third[2].body.ics === SAMPLE && (await s.has(`${DLG}[data-part="2"] li.item`)));
  await s.shot(run + 'p5-import-from-drop.png');
} catch (e) {
  ok('the run reached its end', false, String(e).slice(0, 300));
  await s.shot(run + 'p-failed.png').catch(() => {});
} finally {
  await s.close();
  fs.writeFileSync(run + 'result.txt', out.join('\n') + '\n');
}
