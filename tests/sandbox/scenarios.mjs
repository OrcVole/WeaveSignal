// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives every training scenario of the sandbox desk (ADR-0222) from its start card to its end
// card in a browser, with the keys its guide names, checking the simulated station at each
// step and the words the interface shows (AUTO, OPEN, LINK DEGRADED, DESK BUSY, BANNED BEFORE,
// DUMPED, MUTED); the chooser and the stand-in tab panel; both themes once; no page errors.
// The show itself is drive.mjs's. Needs the sandbox build served with its sounds, as drive.mjs
// says:
//
//   SITE=http://127.0.0.1:27911 PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/sandbox/scenarios.mjs
//
// OUT (optional) is a folder for screenshots.
import { open, SITE, check } from './lib.mjs';

const d = await open();
const { page, until, press, hold, snap, st } = d;

/** The scenario's end card is up, with what you did; then back to the chooser. */
async function ended(id) {
  await until(`${id}: the end card`, () => !!document.querySelector('[data-panel="sandbox.end"]'), 40_000);
  const did = await st(() => document.querySelectorAll('[data-panel="sandbox.end"] .learn li').length);
  check(did >= 2, `${id}: the end card says what you did (${did} lines)`);
  await snap(`${id}-end`);
  await page.getByRole('button', { name: 'Other scenarios' }).click();
  await until(`${id}: the chooser marks it running`, () => /running/i.test(document.querySelector('[data-panel="sandbox.chooser"]')?.textContent ?? ''), 5_000);
  check(d.errors.length === 0, `${id}: no page errors${d.errors.length ? `: ${d.errors.slice(0, 3).join(' | ')}` : ''}`);
}



// ---- the chooser ----
await page.goto(`${SITE}/`);
await until('the chooser opens with no hash', () => !!document.querySelector('[data-panel="sandbox.chooser"]'), 10_000);
const n = await st(() => document.querySelectorAll('[data-panel="sandbox.chooser"] .pick').length);
check(n === 11, `eleven scenarios listed (${n})`);
await snap('chooser');
await page.keyboard.press('1');
await page.keyboard.press('Delete');
await page.waitForTimeout(300);
check(await st(() => !globalThis.__kettle.person('eilidh').on_air && globalThis.__kettle.delay.dumps === 0), 'keys pressed on a card never reach the desk behind it');
check(await st(() => document.activeElement?.classList.contains('pick')), 'the chooser takes the keyboard: the first scenario is focused');
await page.keyboard.press('Enter');
await until('the first pick opens the show’s start card', () => /presenting The Morning Kettle/.test(document.querySelector('[data-panel="sandbox.start"]')?.textContent ?? ''), 5_000);
await page.getByRole('button', { name: 'Other scenarios' }).click();
await until('back to the chooser', () => !!document.querySelector('[data-panel="sandbox.chooser"]'), 5_000);
await page.locator('[data-panel="sandbox.chooser"] .pick').nth(1).click();
await until('the second pick is Running everything alone', () => /Running everything alone/.test(document.querySelector('[data-panel="sandbox.start"] h1')?.textContent ?? ''), 5_000);
check(await st(() => /two windows/.test(document.querySelector('[data-panel="sandbox.start"]')?.textContent ?? '')), 'the start card says a DJ runs two windows');
await snap('start-alone');

// ---- alone: the DJ and the screener ----
await d.start('alone');
check(await st(() => !!document.querySelector('[data-panel="sandbox.tab"]')), 'alone: the stand-in tab panel is in the bar');
check(await st(() => document.querySelector('[data-panel="sandbox.guide"] .step')?.getAttribute('aria-live') === 'polite'), 'alone: the guide step is a polite live region');
await press('1');
await press('F1');
await until('alone: Eilidh opens', () => globalThis.__kettle.speaking?.line.id === 'l-open', 15_000);
await until('alone: a caption shows her line', () => /Eilidh/.test(document.querySelector('[data-panel="sandbox.caption"]')?.textContent ?? ''), 5_000);
await until('alone: the unrecorded line is captioned and marked', () => /not recorded/.test(document.querySelector('[data-panel="sandbox.caption"]')?.textContent ?? ''), 20_000);
await snap('alone-caption');
await until('alone: Niall rings', () => globalThis.__kettle.callerState('niall') === 'waiting', 20_000);
await press('Control+2');
await press('1');
await hold('t', 300);
check(await st(() => globalThis.__kettle.person('shona') === undefined && globalThis.__kettle.view(Date.now()).seats.screener === null), 'alone: nobody is in the screener’s seat');
await press('p');
await until('alone: Niall is sent to the strip', () => globalThis.__kettle.callerState('niall') === 'ready');
await press('Control+1');
await press('r');
await until('alone: Niall is cued and heard on the strip', () => globalThis.__kettle.person('niall').cue && globalThis.__kettle.speaking?.line.id === 'l-n1', 10_000);
await snap('alone-cued');
// The light theme once, on a live surface: the bar, the tab panel and the caption in it.
await page.getByRole('button', { name: 'Light' }).click();
await page.waitForTimeout(300);
await snap('alone-cued-light');
await page.getByRole('button', { name: 'Scenarios' }).click();
await until('alone: the chooser opens over the running scenario', () => !!document.querySelector('[data-panel="sandbox.chooser"]'), 5_000);
await snap('chooser-light');
await page.keyboard.press('Escape');
await until('alone: Escape closes the chooser', () => !document.querySelector('[data-panel="sandbox.chooser"]'), 5_000);
await page.getByRole('button', { name: 'Dark' }).click();
await until('alone: his booth lines end', () => globalThis.__kettle.done('niall-cue'), 15_000);
await hold('f', 1200);
await until('alone: talkback on the strip is heard', () => globalThis.__kettle.speaking?.line.id === 'l-e-cue', 5_000);
await until('alone: the tab panel says who you talked to', () => /Talking to Niall|Live|In your own cue|Mic on/.test(document.querySelector('[data-panel="sandbox.tab"]')?.textContent ?? ''), 2_000);
await until('alone: the talk beat ends', () => globalThis.__kettle.done('niall-talk'), 10_000);
await press('4');
await until('alone: Niall is on air', () => globalThis.__kettle.open('niall'));
await until('alone: the call ends', () => globalThis.__kettle.done('niall-air'), 30_000);
await press('4');
await press('F3');
await until('alone: applause fired', () => globalThis.__kettle.fired.includes('applause-1'));
await press('1');
await until('alone: handed over to playout', () => globalThis.__kettle.has('handed-over'), 5_000);
await until('alone: the lamp says STANDBY', () => /standby/i.test(document.body.innerText), 5_000);
await ended('alone');

// ---- queue: the screener with four callers ----
await d.start('queue');
await until('queue: four callers rang', () => globalThis.__kettle.callers.length === 4, 15_000);
await press('Control+2');
await press('1');
await hold('t', 800);
await until('queue: Shona answers Niall', () => globalThis.__kettle.speaking?.line.id === 'l-s1', 5_000);
await press('p');
await until('queue: the presenter takes Niall', () => globalThis.__kettle.open('niall'), 10_000);
await press('2');
await hold('t', 500);
await press('p');
await until('queue: the presenter declines Dougal while busy', () => globalThis.__kettle.callerRec('dougal')?.declined?.reason === 'not now', 10_000);
await until('queue: the screener shows DESK BUSY', () => /desk busy/i.test(document.body.innerText), 5_000);
await snap('queue-desk-busy');
await press('3');
await hold('t', 500);
await until('queue: Flora is screened', () => globalThis.__kettle.callerState('flora') === 'screening');
await press('w');
await until('queue: Flora is held in the queue', () => globalThis.__kettle.backs === 1);
await press('4');
await hold('t', 500);
await press('Control+d');
await press('Control+d');
await until('queue: Alastair is dropped', () => (globalThis.__kettle.drops.alastair ?? 0) === 1);
await until('queue: Niall is off air', () => globalThis.__kettle.done('niall-air') && !globalThis.__kettle.open('niall'), 40_000);
await press('2');
await press('p');
await until('queue: Dougal is taken when sent again', () => globalThis.__kettle.open('dougal'), 10_000);
await ended('queue');

// ---- dump ----
await d.start('dump');
await until('dump: Jessie is ready on strip 4', () => globalThis.__kettle.view(Date.now()).sources.find((x) => x.identity === 'jessie')?.slot === 4, 5_000);
await press('1');
await press('4');
await until('dump: Jessie swears', () => globalThis.__kettle.speaking?.line.id === 'l-j', 15_000);
await page.waitForTimeout(5000);
await press('Delete');
await press('Delete');
await until('dump: the delay was dumped', () => globalThis.__kettle.delay.dumps === 1 && globalThis.__kettle.delay.rebuilding);
await until('dump: the chip says DUMPED', () => /dumped/i.test(document.body.innerText), 5_000);
await page.getByRole('button', { name: 'Listeners' }).click();
await until('dump: Eilidh apologises', () => globalThis.__kettle.done('sorry'), 15_000);
await press('4');
await ended('dump');

// ---- banned before ----
await d.start('banned');
await until('banned: Tavish rings', () => globalThis.__kettle.callerState('tavish') === 'waiting', 5_000);
await press('Control+2');
await until('banned: the queue says BANNED BEFORE with the date', () => /banned before/i.test(document.body.innerText) && /2026-09-12/.test(document.body.innerText), 5_000);
await snap('banned-queue');
await press('1');
await hold('t', 500);
await until('banned: he is heard out', () => globalThis.__kettle.done('tavish-booth'), 15_000);
await press('p');
await until('banned: the presenter declines him', () => !!globalThis.__kettle.callerRec('tavish')?.declined, 10_000);
await until('banned: DESK BUSY beside BANNED BEFORE', () => /desk busy/i.test(document.body.innerText) && /banned before/i.test(document.body.innerText), 5_000);
await press('Control+d');
await press('Control+d');
await until('banned: dropped', () => globalThis.__kettle.drops.tavish === 1);
await until('banned: he rings again, still marked', () => globalThis.__kettle.view(Date.now()).callers[0]?.banned_before !== null && globalThis.__kettle.view(Date.now()).callers.length === 1, 15_000);
await press('1');
await press('Control+d');
await press('Control+d');
await until('banned: dropped again', () => globalThis.__kettle.drops.tavish === 2);
await ended('banned');

// ---- carts and a bed ----
await d.start('carts');
await press('1');
await press('F2');
await until('carts: the bed plays and Eilidh speaks over it', () => globalThis.__kettle.speaking?.line.id === 'i-eilidh', 10_000);
await until('carts: the bed ducks under her voice', () => globalThis.__kettle.view(Date.now()).carts.ducked, 3_000);
await press('F3');
await press('F1');
await until('carts: shots stack over the bed', () => globalThis.__kettle.fired.includes('applause-1') && globalThis.__kettle.fired.includes('jingle-1'));
await press('Control+3');
await press('Space');
await press('Shift+Space');
await press('Shift+Space');
await until('carts: everything faded', () => globalThis.__kettle.fades === 1, 5_000);
await press('Shift+Digit2');
await until('carts: the bed is cued in the headphones only', () => globalThis.__kettle.cartCues === 1 && globalThis.__kettle.sounding.some((x) => x.cue), 5_000);
await snap('carts-cue');
await press('Escape');
await press('Control+1');
await press('1');
await ended('carts');

// ---- auto ----
await d.start('auto');
await until('auto: the lamp says AUTO', () => /\bAUTO\b/.test(document.body.innerText), 5_000);
await press('1');
await until('auto: an open fader says OPEN, not ON AIR', () => globalThis.__kettle.open('eilidh') && /\bOPEN\b/.test(document.body.innerText), 5_000);
await snap('auto-open');
await page.getByRole('button', { name: 'Listeners' }).click();
await page.getByRole('button', { name: 'Studio' }).click();
await page.getByRole('button', { name: 'Take the station live' }).click();
await until('auto: the station is live and the lamp ON AIR', () => globalThis.__kettle.mode === 'live' && /ON AIR/.test(document.body.innerText), 5_000);
await press('t');
await until('auto: Finlay is cued and heard', () => globalThis.__kettle.speaking?.line.id === 'i-finlay', 20_000);
await until('auto: Finlay’s line ends', () => globalThis.__kettle.done('finlay'), 10_000);
await press('1');
await page.getByRole('button', { name: 'Back to auto' }).click();
await until('auto: handed back to automation', () => globalThis.__kettle.has('handed-back'), 5_000);
await ended('auto');

// ---- next-dj: set up the next DJ ----
await d.start('next-dj');
check(await st(() => globalThis.__kettle.person('finlay') === undefined), 'next-dj: Finlay is not in the room at the start');
await press('1');
await until('next-dj: Eilidh’s link is captioned', () => globalThis.__kettle.speaking?.line.id === 't-nd-on', 10_000);
// The invite, from the desk, by keyboard alone: the button at the head of Not on air, the dialog
// (a name, the role DJ first), the link, then Esc, and the focus back on the button.
check(await st(() => !document.querySelector('[data-handover]')), 'next-dj: no handover button with no DJ in the room');
const inviteBtn = page.getByRole('button', { name: 'Invite someone…' });
await inviteBtn.focus();
await page.keyboard.press('Enter');
await until('next-dj: the invite dialog opens on the name', () => document.querySelector('[data-panel="desk.invite"]')?.open && document.activeElement?.id === 'inv-name', 5_000);
check(await st(() => document.querySelector('input[name="inv-role"]:checked')?.value === 'dj'), 'next-dj: the role is DJ unless changed');
check(await st(() => /Joins off air/.test(document.querySelector('[data-panel="desk.invite"]').textContent) && (document.querySelector('[data-panel="desk.invite"]').textContent.match(/On air the moment they join/g) ?? []).length === 2), 'next-dj: the dialog says a DJ joins off air, a guest and a presenter on air');
await page.keyboard.press('Enter');
await until('next-dj: an empty name is asked for in words', () => /Enter their name/.test(document.getElementById('inv-name-err')?.textContent ?? ''), 3_000);
// Eilidh is on air on strip 1 and Z is her mute key: typed into the name, neither reaches the desk.
await page.keyboard.type('Finlay Grant 1z');
await page.keyboard.press('Delete');
await page.keyboard.press('Backspace');
await page.keyboard.press('Backspace');
await page.keyboard.press('Backspace');
check(await st(() => { const e = globalThis.__kettle.person('eilidh'); return e.on_air && !e.mute && globalThis.__kettle.delay.dumps === 0 && document.getElementById('inv-name').value === 'Finlay Grant'; }), 'next-dj: keys typed in the dialog never reach the desk behind it');
await snap('next-dj-invite');
await page.keyboard.press('Enter');
await until('next-dj: the link is shown, for four hours, with Copy link focused', () => /\/join\/#sandbox-dj-/.test(document.getElementById('inv-link')?.value ?? '') && /works for 4 hours, until \d\d:\d\d/.test(document.querySelector('[data-panel="desk.invite"]').textContent) && document.activeElement?.id === 'inv-copy', 5_000);
await page.keyboard.press('Enter');
await until('next-dj: Copy link says what happened', () => /Copied|selected/.test(document.querySelector('[data-panel="desk.invite"] [role="status"]')?.textContent ?? ''), 3_000);
await snap('next-dj-invite-link');
await page.keyboard.press('Escape');
await until('next-dj: Esc closes the dialog and the focus is back on Invite someone…', () => !document.querySelector('[data-panel="desk.invite"]').open && document.activeElement?.textContent?.trim() === 'Invite someone…', 3_000);
await inviteBtn.blur();
await until('next-dj: Finlay joins off air on strip 5', () => globalThis.__kettle.view(Date.now()).sources.some((x) => x.identity === 'finlay' && x.slot === 5 && !x.on_air), 15_000);
await until('next-dj: the desk says he joined', () => /Finlay joined the room/.test(document.body.innerText), 5_000);
await until('next-dj: his tile and the rundown offer Bring Finlay on air ⇧⏎', () => document.querySelector('[data-panel="desk.tile.5"] [data-handover="on"]')?.textContent.replace(/\s+/g, ' ').trim() === 'Bring Finlay on air⇧⏎' && !!document.querySelector('[data-panel="desk.rundown"] [data-handover="on"]'), 5_000);
await snap('next-dj-joined');
await press('t');
await until('next-dj: Finlay is cued and heard', () => globalThis.__kettle.speaking?.line.id === 'i-finlay', 20_000);
await until('next-dj: his level check ends', () => globalThis.__kettle.done('level'), 20_000);
await hold('g', 600);
await until('next-dj: the plan is agreed on talkback', () => globalThis.__kettle.done('plan'), 25_000);
await press('t');
await until('next-dj: he stands by, off air', () => globalThis.__kettle.has('standing-by') && !globalThis.__kettle.open('finlay'), 5_000);
await ended('next-dj');

// ---- handover: hand over to the next DJ ----
await d.start('handover');
check(await st(() => globalThis.__kettle.view(Date.now()).sources.some((x) => x.identity === 'finlay' && x.slot === 5 && !x.on_air)), 'handover: Finlay stands by on strip 5');
await press('1');
await until('handover: the last link', () => globalThis.__kettle.done('last-link'), 15_000);
await press('t');
await hold('g', 600);
await until('handover: Finlay is counted in on talkback', () => globalThis.__kettle.done('count'), 15_000);
// The handover button must not move the four live buttons: Finlay's tile keeps them at the offset
// of a tile without the button (playout's beside him while he waits, Eilidh's in the overlap).
const liveTop = (n) => st(`(() => { const t = document.querySelector('[data-panel="desk.tile.${n}"]'); return Math.round(t.querySelector('.controls').getBoundingClientRect().top - t.getBoundingClientRect().top); })()`);
check(await st(() => !!document.querySelector('[data-panel="desk.tile.5"] [data-handover="on"]')) && (await liveTop(5)) === (await liveTop(6)), 'handover: waiting, his four live buttons sit where playout’s do, with the button under them');
await press('Shift+Enter');
await until('handover: both on air, the overlap', () => globalThis.__kettle.open('eilidh') && globalThis.__kettle.open('finlay') && (document.body.innerText.match(/ON AIR/g) ?? []).length >= 2, 5_000);
await until('handover: the button now says Hand over to Finlay, on his tile and the rundown', () => /^Hand over to Finlay/.test(document.querySelector('[data-panel="desk.tile.5"] [data-handover="off"]')?.textContent.trim() ?? '') && !!document.querySelector('[data-panel="desk.rundown"] [data-handover="off"]'), 5_000);
check((await liveTop(5)) === (await liveTop(1)), 'handover: in the overlap, his four live buttons sit where Eilidh’s do');
await snap('handover-overlap');
await until('handover: the overlap is said', () => globalThis.__kettle.done('overlap'), 40_000);
// The second press from the button, not the key: the same action.
await page.locator('[data-panel="desk.tile.5"] [data-handover="off"]').click();
await until('handover: the air is Finlay’s and the desk names Steeped', () => globalThis.__kettle.has('handed-over') && /Steeped/.test(document.body.innerText), 5_000);
check(await st(() => !document.querySelector('[data-handover]') && globalThis.__kettle.view(Date.now()).seats.presenter === 'finlay'), 'handover: no button left to press, and the presenter’s seat is Finlay’s');
await snap('handover-his');
await ended('handover');

// ---- link ----
await d.start('link');
await press('1');
await press('4');
await until('link: Iona is on air', () => globalThis.__kettle.open('iona'));
await until('link: her link degrades', () => globalThis.__kettle.has('degraded'), 30_000);
await until('link: the tile and status line say LINK DEGRADED', () => (document.body.innerText.match(/link degraded/gi) ?? []).length >= 2, 5_000);
await snap('link-degraded');
await press('4');
await until('link: her link drops', () => globalThis.__kettle.has('dropped'), 20_000);
await press('Control+2');
await until('link: the queue says DROPPED with the place held', () => /dropped/i.test(document.body.innerText) && /within 5 min/.test(document.body.innerText), 5_000);
await press('Control+1');
await until('link: she is back on strip 4', () => globalThis.__kettle.has('returned') && globalThis.__kettle.slots[3] === 'iona', 30_000);
await press('4');
await until('link: she finishes', () => globalThis.__kettle.done('iona-back'), 30_000);
await press('4');
await ended('link');

// ---- mic ----
await d.start('mic');
await until('mic: the tab starts muted', () => !globalThis.__kettle.mic && /Mic off/.test(document.querySelector('[data-panel="sandbox.tab"]')?.textContent ?? ''), 5_000);
await press('1');
await page.waitForTimeout(1500);
check(await st(() => globalThis.__kettle.open('eilidh') && globalThis.__kettle.speaking === null && (globalThis.__kettle.levels.eilidh ?? null) === null), 'mic: ON AIR, nothing said, the meter flat');
await snap('mic-off');
await page.getByRole('button', { name: 'Unmute' }).click();
await until('mic: unmuted in the tab, Eilidh is heard', () => globalThis.__kettle.mic && globalThis.__kettle.speaking?.line.id === 'i-eilidh', 5_000);
await page.waitForTimeout(1500);
await until('mic: her meter has a level', () => (globalThis.__kettle.levels.eilidh ?? -99) > -50, 5_000);
await until('mic: the line ends', () => globalThis.__kettle.done('hello'), 10_000);
await press('z');
await until('mic: the desk mute says MUTED and the lamp STANDBY', () => globalThis.__kettle.has('muted') && /muted/i.test(document.body.innerText) && /standby/i.test(document.body.innerText), 5_000);
await press('z');
await until('mic: unmuted at the desk', () => globalThis.__kettle.done('unmuted'), 10_000);
await press('1');
await ended('mic');

await d.done();
