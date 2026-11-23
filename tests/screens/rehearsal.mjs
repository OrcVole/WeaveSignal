// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The rehearsal desk's screens (1.3.0, step R13; rehearsal-mode-build-plan.md, screens 1 to 5
// and 9) on the screens loopback stack (tests/screens/run-screens.sh), called by drive.mjs.
//
// The stack has a control plane with the rehearsal studio on and NO rehearsal desk (and no live
// desk, room or playout). So two kinds of check, and each says which it is:
//
// - `real`: against the control plane as it stands. The choice of studio, the rehearsal desk
//   not answering, opening a session, a second show refused (409), keys that send nothing to
//   the live desk, End rehearsal, and the line in Settings.
// - `fixture`: the page's own `fetch` is answered here for `/api/v1/rehearsal*` (and, for the
//   live desk after a move, `/api/v1/studio/events`), because no stack without a desk and a
//   room can put three people in a rehearsal. These are checks of the screens, not of the
//   station: the people, the move dialog, another show's session, off at this station, the
//   live desk with people standing by, and the invitation's choice of studio. Each is shot at
//   1,440 × 900 and 2,560 × 1,440 in Dark, Light and Night.
//
// The event stream in a fixture is a body that never ends, as the station's is: a fulfilled
// route would close it, and the page would be shot while it reconnects.

import { join } from 'node:path';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const SIZES = [
  [1440, 900],
  [2560, 1440],
];
const THEMES = ['dark', 'light', 'night'];

const person = (identity, name, over = {}) => ({ identity, name, role: 'guest', on: true, mute: false, cue: false, talk: false, talk_to: null, present: true, linked: true, lufs: -21.5, ...over });
const PEOPLE = [person('presenter-eilidh', 'Eilidh Fraser-MacLeod', { role: 'presenter', lufs: -19.8 }), person('guest-gregor', 'Gregor Stewart', { mute: true }), person('guest-kirsty', 'Kirsty Wilson', { on: false, lufs: -27.1 })];

/** The rehearsal view with three people, for Steeped, on air at the next :05 past a quarter. */
function fixtures(now) {
  const start = Math.ceil((now + 9 * 60_000) / 300_000) * 300_000;
  const window = { state: 'announced', show: 'steeped', show_name: 'Steeped', start_ms: start, opens_ms: start - 15 * 60_000, host: 'Eilidh Fraser-MacLeod', from: 'room', heads_up_ms: null, late: null, rehearsing: true };
  const session = { id: '7c1b1d5e-5b7a-4d0e-9a55-2f6f1d3c9a01', show: 'steeped', show_name: 'Steeped', opened_by: 'Eilidh', opened_at_ms: now - 22 * 60_000 };
  const three = { enabled: true, desk: { linked: true, error: null }, session, people: PEOPLE, carts_sounding: [], max_people: 6, window };
  const other = { ...three, session: { ...session, show: 'kettle-drive', show_name: 'Kettle Drive', opened_by: 'Heather' }, people: [person('presenter-heather', 'Heather Douglas', { role: 'presenter' }), person('guest-rory', 'Rory Bruce', { on: false })], window: null };
  const off = { enabled: false, desk: { linked: false, error: null }, session: null, people: [], carts_sounding: [], max_people: 6, window: null };
  const move = { moved: PEOPLE.map((p) => p.identity), opening: { airing_start_ms: start, airing_end_ms: start + 3_600_000, standby_until_ms: start + 5 * 60_000, applied_at_ms: null, cleared_at_ms: null, host: 'presenter-eilidh', on: ['guest-gregor', 'presenter-eilidh'], mute: ['guest-gregor'] }, airing_start_ms: start };
  return { three, other, off, move, start, window };
}

/** Four rundown rows before the post, so the live desk's left column is as full as on a show. */
function rundown(start) {
  const at = (ms) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false });
  return [
    { at: at(start - 9 * 60_000), title: 'Travel and weather', detail: 'Fife and the bridges', hard_post: false, key: null },
    { at: at(start - 6 * 60_000), title: 'Listeners’ letters', detail: 'Two, then the ident', hard_post: false, key: null },
    { at: at(start - 2 * 60_000), title: 'Handover', detail: 'Steeped is next', hard_post: false, key: null },
    { at: at(start), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
  ];
}

/** The page's `fetch`, answered for the rehearsal's routes; everything else is the station's. */
function installFixture(fx) {
  const real = window.fetch.bind(window);
  const enc = new TextEncoder();
  const stream = (obj) => new Response(new ReadableStream({ start: (c) => c.enqueue(enc.encode(`event: state\ndata: ${JSON.stringify(obj)}\n\n`)) }), { status: 200, headers: { 'content-type': 'text/event-stream' } });
  const json = (obj, status = 200) => new Response(JSON.stringify(obj), { status, headers: { 'content-type': 'application/json' } });
  window.__sent = [];
  window.fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const path = new URL(url, location.href).pathname;
    const method = (init?.method ?? 'GET').toUpperCase();
    if (method !== 'GET') window.__sent.push({ method, path, body: init?.body ? String(init.body) : null });
    if (path === '/api/v1/rehearsal') return json(fx.rehearsal);
    if (path === '/api/v1/rehearsal/events') return stream(fx.rehearsal);
    if (path === '/api/v1/rehearsal/commands') return fx.refuse ? json({ error: fx.refuse }, 403) : json(fx.rehearsal);
    if (path === '/api/v1/rehearsal/move' && fx.move) return json(fx.move);
    if (path === '/api/v1/studio/events' && fx.studio) return stream({ ...fx.studio, now_ms: Date.now() });
    return real(input, init);
  };
}

/** Record every request that is not a GET, on a page with no fixture. */
function installRecorder() {
  const real = window.fetch.bind(window);
  window.__sent = [];
  window.fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const method = (init?.method ?? 'GET').toUpperCase();
    if (method !== 'GET') window.__sent.push({ method, path: new URL(url, location.href).pathname, body: init?.body ? String(init.body) : null });
    return real(input, init);
  };
}

export async function rehearsalScreens({ browser, check, log, OUT, STUDIO, CONTROL, TOKEN, ZONE }) {
  const api = async (method, path, body) => {
    const res = await fetch(`${CONTROL}${path}`, { method, headers: { Authorization: `Bearer ${TOKEN}`, ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
    return { status: res.status, body: await res.json().catch(() => ({})) };
  };
  async function open(width, height, theme, init, arg) {
    const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, timezoneId: ZONE });
    await ctx.addInitScript((t) => {
      try {
        localStorage.setItem('weavesignal.theme', t);
      } catch {}
    }, theme);
    if (init) await ctx.addInitScript(init, arg);
    const page = await ctx.newPage();
    page.on('pageerror', (e) => check('rehearsal: no page errors', false, e.message));
    await page.goto(STUDIO);
    await page.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
    await page.keyboard.press('Control+1');
    await page.locator('[data-panel="desk.studio"]').waitFor({ timeout: 15000 });
    await sleep(600);
    return { ctx, page };
  }
  async function shot(page, name) {
    await page.mouse.move(2, 2);
    const path = join(OUT, `${name}.png`);
    await page.screenshot({ path });
    log('shot', path);
  }
  const text = (page, sel) => page.locator(sel).first().innerText().catch(() => '');
  const toRehearsal = async (page) => {
    await page.locator('[data-panel="desk.studio"] button').nth(1).click();
    await page.locator('[data-panel="rehearsal.stage"]').waitFor({ timeout: 10000 });
    await sleep(500);
  };
  /** The marks of the air a rehearsal desk never carries. */
  const airMarks = (page) =>
    page.evaluate(() => {
      const root = document.querySelector('.desk.rehearsal');
      if (!root) return ['no rehearsal desk drawn'];
      const found = [];
      for (const sel of ['.fill-onair', '.fill-open', '.is-onair', '.is-open', '.lit-onair', '.lit-open', '.lamp.onair', '[data-panel^="desk.tile"]', '[data-panel="desk.lamp"]', '[data-panel="desk.delay"]', 'use[href="#g-onair"]', 'use[href="#g-open"]']) if (root.querySelector(sel)) found.push(sel);
      const lamp = root.querySelector('[data-panel="rehearsal.lamp"]');
      if (!lamp || !/rehearsal/i.test(lamp.textContent ?? '')) found.push('no REHEARSAL lamp');
      return found;
    });

  // Leave nothing of an earlier run behind.
  await api('POST', '/api/v1/rehearsal/end', {});

  // ---------------------------------------------------------------- real: the control plane
  {
    const { ctx, page } = await open(1440, 900, 'dark', installRecorder);
    const before = (await api('GET', '/api/v1/rehearsal')).body;
    check('real: the station has a rehearsal studio, and no rehearsal desk in this stack', before.enabled === true && before.desk?.linked === false && before.session === null, JSON.stringify(before.desk));
    const phase = await page.locator('[data-panel="desk.studio"]').getAttribute('data-rehearsal');
    check('real: the Desk tab offers Live or Rehearsal, and marks the rehearsal desk not answering', phase === 'desk-down', `data-rehearsal=${phase}`);
    await shot(page, 'desk-studio-choice-1440-dark');

    await toRehearsal(page);
    check('real: the rehearsal desk says it is not answering, and still offers to open', /not answering yet/.test(await text(page, '[data-testid="rehearsal-open"]')) && /Open the rehearsal studio for/.test(await text(page, '[data-testid="rehearsal-open"]')));
    check('real: nothing of the air is drawn (no session)', (await airMarks(page)).length === 0, (await airMarks(page)).join(' '));
    await shot(page, 'rehearsal-no-session-desk-down-1440-dark');

    await page.locator('[data-testid="rehearsal-open"] select').selectOption('steeped');
    await page.locator('[data-testid="rehearsal-open"] button[type=submit]').click();
    await page.locator('[data-panel="rehearsal.waiting"]').waitFor({ timeout: 10000 });
    const opened = (await api('GET', '/api/v1/rehearsal')).body;
    check('real: Open for Steeped opens the session at the station', opened.session?.show === 'steeped', JSON.stringify(opened.session));
    check('real: the open rehearsal says its desk is starting again, and that live is not affected', /starting again/.test(await text(page, '[data-testid="rehearsal-desk-down"]')));
    await shot(page, 'rehearsal-open-empty-desk-down-1440-dark');

    const second = await api('POST', '/api/v1/rehearsal/open', { show: 'kettle-drive' });
    check('real: another show is refused while one rehearses (409, in a sentence)', second.status === 409 && /Steeped/.test(second.body.error ?? ''), `${second.status} ${second.body.error ?? ''}`);

    // Every key of the live desk, pressed at the rehearsal desk: nothing leaves for the live desk.
    await page.evaluate(() => (window.__sent.length = 0));
    await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
    for (const k of ['Digit1', 'Digit2', 'KeyQ', 'KeyZ', 'F1', 'F6', 'Delete', 'Delete', 'Shift+Enter', 'Control+Shift+KeyE', 'Control+KeyR', 'Control+KeyR', 'F7', 'KeyP', 'Control+KeyD', 'Control+KeyD']) {
      await page.keyboard.press(k);
      await sleep(120);
    }
    await page.keyboard.down('KeyA');
    await sleep(150);
    await page.keyboard.up('KeyA');
    await sleep(600);
    const sent = await page.evaluate(() => window.__sent);
    const stray = sent.filter((s) => !s.path.startsWith('/api/v1/rehearsal/'));
    check('real: slot keys, F1 to F6, Del Del, ⇧⏎, ⌃⇧E, ⌃R ⌃R, F7, P and ⌃D ⌃D send nothing to the live desk, playout or the carts', stray.length === 0, JSON.stringify(sent.map((s) => `${s.method} ${s.path}`)));
    check('real: and the page is still the rehearsal desk (⌃R did not reload it, ⌃⇧E opened no dialog)', (await page.locator('[data-panel="rehearsal.stage"]').count()) === 1 && (await page.locator('dialog[open]').count()) === 0);

    // Nobody is there, so there is nobody to move.
    await page.locator('[data-testid="rehearsal-move"]').click();
    await page.locator('[data-panel="rehearsal.move"][open]').waitFor({ timeout: 5000 });
    check('real: the move dialog with nobody there offers no Move', /nobody to move/.test(await text(page, '[data-panel="rehearsal.move"]')) && (await page.locator('[data-panel="rehearsal.move"] button[type=submit]').isDisabled()));
    await shot(page, 'rehearsal-move-nobody-1440-dark');
    await page.keyboard.press('Escape');
    await sleep(300);
    check('real: Esc is Stay here', (await page.locator('dialog[open]').count()) === 0 && (await api('GET', '/api/v1/rehearsal')).body.session?.show === 'steeped');

    // The invitation dialog at the live desk, while the rehearsal is open: where the link leads.
    await page.locator('[data-panel="desk.studio"] button').first().click();
    await page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 10000 });
    const invite = page.locator('[data-panel="desk.waiting"] button', { hasText: 'Invite someone…' }).first();
    if (await invite.count()) {
      await invite.click();
      await page.locator('[data-panel="desk.invite"][open]').waitFor({ timeout: 5000 });
      const where = await text(page, '[data-panel="desk.invite"][open] [data-testid="inv-where"]');
      check('real: the invitation dialog asks where the link leads, the live studio first', /Live studio[\s\S]*Rehearsal studio · Steeped/.test(where), where.replace(/\s+/g, ' ').slice(0, 120));
      const roles = await text(page, '[data-panel="desk.invite"][open]');
      check('real: and no role says on air the moment they join', !/the moment they join/i.test(roles) && /standing by/.test(roles));
      await shot(page, 'desk-invite-where-1440-dark');
      await page.keyboard.press('Escape');
    } else {
      check('real: the invitation dialog at the live desk', false, 'Invite someone… is not drawn on this stack’s live desk');
    }

    // End rehearsal: two presses.
    await toRehearsal(page);
    await page.locator('[data-testid="rehearsal-end"]').click();
    await sleep(200);
    check('real: End rehearsal asks for a second press', /Press again/.test(await text(page, '[data-testid="rehearsal-end"]')) && (await api('GET', '/api/v1/rehearsal')).body.session?.show === 'steeped');
    await page.locator('[data-testid="rehearsal-end"]').click();
    await page.locator('[data-testid="rehearsal-open"]').waitFor({ timeout: 10000 });
    check('real: the second press ends the session at the station', (await api('GET', '/api/v1/rehearsal')).body.session === null);

    // Settings: the read-only line.
    await page.locator('[data-panel="studio.frame"] .settings-tab').click();
    await page.locator('[data-testid="settings-rehearsal"]').waitFor({ timeout: 10000 });
    const line = await text(page, '[data-testid="settings-rehearsal"]');
    check('real: Settings says the rehearsal studio is on, that its desk is not answering, and names the setting', /Rehearsal studio: on/.test(line) && /not answering/.test(line) && /REHEARSAL/.test(line), line);
    check('real: and offers no control to change it', (await page.locator('[data-testid="settings-rehearsal"] button, [data-testid="settings-rehearsal"] input').count()) === 0);
    await shot(page, 'settings-rehearsal-on-1440-dark');
    await ctx.close();
  }

  // ---------------------------------------------------------------- fixture: the screens
  const live = (await api('GET', '/api/v1/studio')).body;
  let first = true;
  for (const [w, h] of SIZES) {
    for (const theme of THEMES) {
      const fx = fixtures(Date.now());
      const tag = `${w}-${theme}`;

      // Screen 2, three people; screen 3, the move; then screen 4, the live desk after it.
      const src = (p, slot) => ({ identity: p.identity, name: p.name, role: p.role, on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: p.lufs, lufs_peak: p.lufs + 3, slot, caller: null });
      const studio = {
        ...live,
        show: { ...(live.show ?? {}), title: 'The Morning Kettle', rundown: rundown(fx.start) },
        desk: { ...(live.desk ?? {}), reachable: true, error: null, playout: { connected: true, for_ms: 3_600_000 } },
        mode: 'auto',
        step: { ...(live.step ?? {}), step: 'full' },
        sources: PEOPLE.map((p, i) => src(p, i + 1)),
        post: { show: { slug: 'steeped', name: 'Steeped', start_ms: fx.start, started: false }, source: null, source_name: null, state: null, standing_since_ms: null, on_air_at_ms: null, grace_ends_ms: null, held_by: null, late_ends_ms: null, holders: [], link_lost: null, window: { ...fx.window, rehearsing: false } },
      };
      {
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: fx.three, move: fx.move, studio });
        await toRehearsal(page);
        if (first) {
          const marks = await airMarks(page);
          check('fixture: three people at the rehearsal desk, and nothing of the air drawn', marks.length === 0, marks.join(' '));
          const tiles = await page.locator('[data-panel^="rehearsal.tile."]').count();
          const words = await text(page, '[data-panel="rehearsal.stage"]');
          check('fixture: a tile each, saying In the mix, Muted or Not in the mix', tiles === 3 && /IN THE MIX/i.test(words) && /MUTED/i.test(words) && /NOT IN THE MIX/i.test(words), `${tiles} tiles`);
          check('fixture: the window line says when the show is on air and to move by then', /On air at \d\d:\d\d: move by then/.test(await text(page, '[data-testid="rehearsal-window"]')), await text(page, '[data-testid="rehearsal-window"]'));
          const fit = await page.evaluate(() => {
            const bar = document.querySelector('[data-panel="rehearsal.status"]').getBoundingClientRect();
            const out = [...document.querySelectorAll('.desk.rehearsal .tile, .desk.rehearsal .panel')].filter((e) => e.getBoundingClientRect().bottom > bar.top + 1 || e.getBoundingClientRect().right > innerWidth + 1);
            return { over: out.length, scroll: document.documentElement.scrollWidth > innerWidth + 1 };
          });
          check('fixture: every tile and panel is whole above the status bar at 1,440 × 900', fit.over === 0 && !fit.scroll, JSON.stringify(fit));
          // A strip's key goes to the rehearsal's route, and only there.
          await page.evaluate(() => (window.__sent.length = 0));
          await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
          await page.keyboard.press('Digit3');
          await page.keyboard.press('KeyX');
          await sleep(400);
          const sent = await page.evaluate(() => window.__sent);
          check('fixture: 3 puts Kirsty in the mix and X unmutes Gregor, by the rehearsal’s own route', sent.length === 2 && sent.every((s) => s.path === '/api/v1/rehearsal/commands') && /"on_air".*guest-kirsty|guest-kirsty.*"on_air"/.test(sent[0].body) && /"mute".*guest-gregor|guest-gregor.*"mute"/.test(sent[1].body), JSON.stringify(sent));
        }
        await shot(page, `rehearsal-three-${tag}`);

        await page.locator('[data-testid="rehearsal-move"]').click();
        await page.locator('[data-panel="rehearsal.move"][open]').waitFor({ timeout: 5000 });
        await sleep(300);
        if (first) {
          const d = await text(page, '[data-panel="rehearsal.move"]');
          check('fixture: the move dialog lists who moves with their opening, and the plan’s sentence', /Nobody goes on air until the handover\. Their links keep working\./.test(d) && /Eilidh Fraser-MacLeod[\s\S]*On at the handover[\s\S]*Gregor Stewart[\s\S]*On at the handover, muted[\s\S]*Kirsty Wilson[\s\S]*Stands by until you put them on/.test(d), d.replace(/\s+/g, ' ').slice(0, 200));
          check('fixture: Move has the focus, so ⏎ confirms', await page.evaluate(() => document.activeElement?.matches('[data-panel="rehearsal.move"] button[type=submit]') ?? false));
        }
        await shot(page, `rehearsal-move-${tag}`);

        await page.keyboard.press('Enter');
        await page.locator('[data-panel="desk.stage"]').waitFor({ timeout: 10000 });
        await sleep(700);
        if (first) {
          const posted = await page.evaluate(() => window.__sent.filter((s) => s.path === '/api/v1/rehearsal/move').length);
          check('fixture: ⏎ moves, once, and the Desk tab shows the live desk', posted === 1);
          const notes = await page.locator('[data-panel="desk.waiting"] [data-testid="from-rehearsal"]').allInnerTexts();
          check('fixture: the three stand by under Not on air, each saying From rehearsal and what the handover does', notes.length === 3 && notes.some((n) => /on at the handover, muted/.test(n)) && notes.some((n) => /stands by until you put them on/.test(n)), notes.join(' | '));
          check('fixture: nobody moved is on the stage', (await page.locator('[data-panel="desk.stage"] .tiles [data-panel^="desk.tile."]').count()) === 0);
          const fit = await page.evaluate(() => {
            const bar = document.querySelector('[data-panel="desk.status"]').getBoundingClientRect();
            const low = [...document.querySelectorAll('.desk .col.left > *, .desk .col.right > *, .desk .stage')].filter((e) => e.getBoundingClientRect().bottom > bar.top + 1);
            const choice = document.querySelector('[data-panel="desk.studio"]').getBoundingClientRect();
            const clock = document.querySelector('[data-panel="desk.clock"]').getBoundingClientRect();
            const stageTop = document.querySelector('[data-panel="desk.stage"]').getBoundingClientRect().top;
            return { low: low.map((e) => e.getAttribute('data-panel') ?? e.className), choiceAboveClock: choice.bottom <= clock.top, stageTop, scroll: document.documentElement.scrollWidth > innerWidth + 1 || document.documentElement.scrollHeight > innerHeight + 1 };
          });
          check('fixture: with the choice of studio heading the left column, the live desk’s columns still end above the status bar at 1,440 × 900', fit.low.length === 0 && fit.choiceAboveClock && !fit.scroll, JSON.stringify(fit));
          check('fixture: the line under the handover window names who the handover puts on', /The handover puts Eilidh and Gregor on air · from rehearsal/.test(await text(page, '[data-testid="opening-line"]')), await text(page, '[data-testid="opening-line"]'));
        }
        await shot(page, `desk-standing-by-${tag}`);
        await ctx.close();
      }

      // Screen 4's window while the show is still rehearsing: "… rehearsing".
      {
        const studio2 = { ...studio, sources: [], post: { ...studio.post, window: fx.window } };
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: fx.three, studio: studio2 });
        if (first) check('fixture: the live desk’s handover window says the next show is rehearsing', /rehearsing/.test(await text(page, '[data-testid="handover-window"]')), await text(page, '[data-testid="handover-window"]'));
        await shot(page, `desk-window-rehearsing-${tag}`);
        // Screen 5: the invitation dialog with the choice of studio, on Rehearsal.
        const invite = page.locator('[data-panel="desk.waiting"] button', { hasText: 'Invite someone…' }).first();
        if (await invite.count()) {
          await invite.click();
          await page.locator('[data-panel="desk.invite"][open]').waitFor({ timeout: 5000 });
          await page.locator('[data-panel="desk.invite"][open] input[value="rehearsal"]').check();
          await page.locator('[data-panel="desk.invite"][open] #inv-name').fill('Gregor Stewart');
          await sleep(400);
          if (first) {
            const d = await text(page, '[data-panel="desk.invite"][open]');
            check('fixture: a rehearsal link is for a guest or a presenter of the show, and its words speak of no air', /Guest[\s\S]*Presenter/.test(d) && !/\bDJ\b/.test(d.split('They join as')[1] ?? '') && !/Not on air/.test(d.split('They join as')[1] ?? 'Not on air'), (d.split('They join as')[1] ?? '').replace(/\s+/g, ' ').slice(0, 160));
          }
          await shot(page, `desk-invite-rehearsal-${tag}`);
        } else if (first) check('fixture: the invitation dialog with the choice of studio', false, 'Invite someone… is not drawn');
        await ctx.close();
      }

      // Screen 2, no session with the desk answering, and a session whose desk is not answering.
      {
        const closed = { ...fx.three, session: null, people: [], window: null };
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: closed });
        await toRehearsal(page);
        if (first) check('fixture: no session, the desk answering: Open the rehearsal studio for…, and no word of a fault', /Open the rehearsal studio for/.test(await text(page, '[data-testid="rehearsal-open"]')) && !/not answering/.test(await text(page, '.desk.rehearsal')));
        await shot(page, `rehearsal-no-session-${tag}`);
        await ctx.close();
      }
      {
        const down = { ...fx.three, desk: { linked: false, error: 'the rehearsal desk is not answering' } };
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: down });
        await toRehearsal(page);
        if (first) check('fixture: a session whose desk is not answering keeps its people and says the desk is starting again', /starting again/.test(await text(page, '[data-testid="rehearsal-desk-down"]')) && (await page.locator('[data-panel^="rehearsal.tile."]').count()) === 3 && /not answering/.test(await text(page, '[data-panel="rehearsal.status"]')));
        await shot(page, `rehearsal-desk-down-${tag}`);
        await ctx.close();
      }

      // Screen 2, another show's session: the station refuses a press (403), and the screen is read-only.
      {
        const refuse = 'Only the presenter of Kettle Drive, a producer or an admin runs its rehearsal.';
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: fx.other, refuse });
        await toRehearsal(page);
        await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
        await page.keyboard.press('Digit1');
        await page.locator('[data-testid="rehearsal-readonly"]').waitFor({ timeout: 5000 });
        await sleep(300);
        if (first) {
          check('fixture: a refused press makes another show’s rehearsal read-only, in the station’s words', (await text(page, '[data-testid="rehearsal-readonly"]')).includes(refuse));
          check('fixture: with no Move, no End and no invitation', (await page.locator('[data-testid="rehearsal-move"], [data-testid="rehearsal-end"]').count()) === 0 && !/Invite someone/.test(await text(page, '[data-panel="rehearsal.waiting"]')));
          check('fixture: and every tile’s controls off', (await page.locator('[data-panel^="rehearsal.tile."] .controls button:not([disabled])').count()) === 0);
        }
        await shot(page, `rehearsal-read-only-${tag}`);
        await ctx.close();
      }

      // Screens 1 and 9, off at this station: the choice cannot be made, and Settings says why.
      {
        const { ctx, page } = await open(w, h, theme, installFixture, { rehearsal: fx.off });
        const second = page.locator('[data-panel="desk.studio"] button').nth(1);
        if (first) {
          check('fixture: off at this station, Rehearsal cannot be chosen and names the setting', (await second.getAttribute('aria-disabled')) === 'true' && /REHEARSAL=true/.test((await second.getAttribute('title')) ?? ''), (await second.getAttribute('title')) ?? '');
          await second.click({ force: true });
          await sleep(300);
          check('fixture: and pressing it leaves the live desk', (await page.locator('[data-panel="rehearsal.stage"]').count()) === 0);
        }
        await shot(page, `desk-studio-off-${tag}`);
        await page.locator('[data-panel="studio.frame"] .settings-tab').click();
        await page.locator('[data-testid="settings-rehearsal"]').waitFor({ timeout: 10000 });
        if (first) check('fixture: Settings says it is off and which setting turns it on', /Rehearsal studio: off/.test(await text(page, '[data-testid="settings-rehearsal"]')) && /REHEARSAL/.test(await text(page, '[data-testid="settings-rehearsal"]')));
        await shot(page, `settings-rehearsal-off-${tag}`);
        await ctx.close();
      }
      first = false;
    }
  }

  // The name plates (ADR-0070) on the rehearsal desk: every panel named, none over a control.
  {
    const fx = fixtures(Date.now());
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, timezoneId: ZONE });
    await ctx.addInitScript(installFixture, { rehearsal: fx.three });
    const page = await ctx.newPage();
    await page.goto(`${STUDIO}?audit=1`);
    await page.locator('[data-panel="studio.frame"] [data-link="live"]').waitFor({ timeout: 15000 });
    await page.keyboard.press('Control+1');
    await page.locator('[data-panel="desk.studio"]').waitFor({ timeout: 15000 });
    await toRehearsal(page);
    await sleep(1200);
    const plates = await page.evaluate(() => document.getElementById('pn-audit')?.textContent ?? '');
    check('fixture: the rehearsal desk’s name plates sit clear of controls', /plates \d+/.test(plates) && !/OVER/.test(plates), plates.split('\n').filter((l) => /OVER|no room|^plates/.test(l)).join(' | '));
    check('fixture: the stage, the lamp and the choice of studio each have a plate', ['rehearsal.stage', 'rehearsal.lamp', 'desk.studio'].every((n) => plates.split('\n').some((l) => l.startsWith(`${n} `))), plates.split('\n').filter((l) => /^(rehearsal\.(stage|lamp)|desk\.studio) /.test(l)).join(' | '));
    await page.screenshot({ path: join(OUT, 'rehearsal-1440-dark-plates.png') });
    await ctx.close();
  }

  await api('POST', '/api/v1/rehearsal/end', {});
}
