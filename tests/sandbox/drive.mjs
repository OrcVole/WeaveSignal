// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Drives the website's "Try the desk" (ADR-0212) through the whole show, as a visitor would,
// with the keys the guide names, and checks the simulated station and the sound at each step:
// Eilidh on air with a level on her meter, the jingle, Gregor, Niall screened in his booth and
// sent, cued and taken to air on his strip, Jessie on air and dumped (the listeners' delay back
// to 0 and rebuilding), playout pulled and back, the show ended; every tab opened once, and no
// page errors. It needs the sandbox build and its sounds served as the site serves them:
//
//   (cd apps/studio && pnpm build:sandbox)
//   # serve apps/studio/dist-sandbox with media/sandbox -> the sounds inside it:
//   python3 -m http.server 27911 --bind 127.0.0.1
//   SITE=http://127.0.0.1:27911 PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core node tests/sandbox/drive.mjs
//
// OUT (optional) is a folder for a screenshot at each step. Playwright is not a dependency of
// the repository (see tests/m2/drive.mjs). The training scenarios are scenarios.mjs's; what the
// two share is lib.mjs.
import { open, SITE, check } from './lib.mjs';

const d = await open();
const { page, until, press, hold, snap, st } = d;

// The show is the first scenario (ADR-0222): its hash opens its start card past the chooser.
await page.goto(`${SITE}/#show`);
await page.getByRole('button', { name: 'Start with sound' }).waitFor();
await snap('start');
await page.getByRole('button', { name: 'Start with sound' }).click();
await until('the sound starts', () => !document.querySelector('[aria-labelledby="sandbox-title"]'), 20_000);

await press('1');
await until('Eilidh is on air', () => globalThis.__kettle.person('eilidh').on_air);
await press('F1');
await until('the jingle sounds', () => globalThis.__kettle.sounding.some((s) => s.cart === 'jingle-1'));
await until('Eilidh opens the show after the jingle', () => globalThis.__kettle.speaking?.line.id === 'l-open', 15_000);
await page.waitForTimeout(1500);
await until('her meter has a level', () => (globalThis.__kettle.levels.eilidh ?? -99) > -50, 5_000);
await snap('on-air');

await press('2');
await until('Gregor is on air', () => globalThis.__kettle.person('gregor').on_air);
await until('Gregor speaks', () => globalThis.__kettle.speaking?.line.id === 'l-gregor-here', 25_000);
await until('Niall rings after the phones line', () => globalThis.__kettle.callerState('niall') === 'waiting', 40_000);
await press('F2');

await press('Control+2');
await press('1');
await hold('t', 400);
await until('Niall is screened', () => globalThis.__kettle.callerState('niall') === 'screening');
await until('Shona answers him in the booth', () => globalThis.__kettle.speaking?.line.id === 'l-s1', 5_000);
await snap('screener');
await until('the booth conversation ends', () => globalThis.__kettle.done('niall-booth'), 30_000);
await press('p');
await until('Niall is ready for the desk', () => globalThis.__kettle.callerState('niall') === 'ready');

await press('Control+1');
const slot = await st(() => globalThis.__kettle.slots.indexOf('niall') + 1);
check(slot === 4, `Niall is on strip 4 (strip ${slot})`);
await press('r');
await until('Niall is cued and hears Eilidh', () => globalThis.__kettle.speaking?.line.id === 'l-e-cue', 10_000);
await page.waitForTimeout(3500);
await press('4');
await until('Niall is on air', () => globalThis.__kettle.person('niall').on_air);
await snap('niall-on-air');
await until('Niall is done', () => globalThis.__kettle.done('niall-air'), 35_000);
await press('4');

await until('Jessie rings', () => globalThis.__kettle.callerState('jessie') === 'waiting', 15_000);
await press('Control+2');
const jq = await st(() => globalThis.__kettle.callers.findIndex((c) => c.identity === 'jessie') + 1);
await press(String(jq));
await hold('t', 400);
await until('Jessie is screened', () => globalThis.__kettle.callerState('jessie') === 'screening');
await until('her booth line ends', () => globalThis.__kettle.done('jessie-booth'), 15_000);
await press('p');
await press('Control+1');
const js = await st(() => globalThis.__kettle.slots.indexOf('jessie') + 1);
check(js > 0, `Jessie has a strip (${js})`);
await press(String(js));
await until('Jessie swears on air', () => globalThis.__kettle.speaking?.line.id === 'l-j', 15_000);
await page.waitForTimeout(5000);
await press('Delete');
await press('Delete');
await until('the delay was dumped', () => globalThis.__kettle.delay.dumps === 1 && globalThis.__kettle.delay.rebuilding);
await snap('dumped');
await page.getByRole('button', { name: 'Listeners' }).click();
await until('Eilidh apologises', () => globalThis.__kettle.speaking?.line.id === 'l-e-sorry', 10_000);

await page.getByRole('button', { name: 'Pull the plug' }).waitFor({ timeout: 15_000 });
check(true, 'the guide offers to pull the plug');
await page.getByRole('button', { name: 'Pull the plug' }).click();
await until('playout is down', () => !globalThis.__kettle.playout);
await until('the lamp says Playout down', () => /playout down/i.test(document.body.innerText), 5_000);
await snap('playout-down');
await until('playout is back', () => globalThis.__kettle.playout, 20_000);
await until('Eilidh says we are back', () => globalThis.__kettle.done('back'), 20_000);
await page.getByRole('button', { name: 'End the show' }).click();
await until('the show ends', () => globalThis.__kettle.done('end'), 20_000);
await snap('ended');

for (const [key, name] of [['Control+3', 'carts'], ['Control+5', 'schedule'], ['Control+4', 'chat'], ['Control+2', 'screener'], ['Control+1', 'desk']]) {
  await press(key);
  await page.waitForTimeout(600);
  await snap(`tab-${name}`);
}
check(d.errors.length === 0, `no page errors${d.errors.length ? `: ${d.errors.slice(0, 5).join(' | ')}` : ''}`);
await d.done();
