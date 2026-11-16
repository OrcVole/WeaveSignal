// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Phase 0: the build from before the list (73cb2a9) signs in to Kettle FM with the station key,
// in the throwaway profile. What it leaves is what the new build must carry over.
import { open, sleep } from './wd.mjs';
import fs from 'node:fs';
import { execSync } from 'node:child_process';
const here = new URL('.', import.meta.url).pathname;
const old = process.env.OLD_STUDIO;
if (!old) throw new Error('OLD_STUDIO: the path of a studio binary built from before the list (r105, 73cb2a9)');
fs.mkdirSync(here + 'run', { recursive: true });
fs.writeFileSync(here + 'run/keychain-before.txt', execSync('secret-tool search --all service weavesignal-studio 2>&1 | grep attribute.username || true'));
const s = await open(old);
try {
  await s.waitFor('[data-panel="setup.sign-in"]');
  await s.click('.other:last-of-type');   // Use the station key instead
  await s.waitFor('#si-key');
  await s.type('#si-url', 'http://127.0.0.1:26511');
  await s.type('#si-key', 'proof-key-a');
  await s.click('button[type=submit]');
  await s.waitFor('[data-panel="studio.frame"]', 20000);
  await s.wait('the station name', async () => (await s.text('.brand'))?.includes('Kettle FM'));
  console.log('old build signed in; frame says:', JSON.stringify(await s.text('.brand')), 'width', await s.js('return innerWidth'), 'frame height', await s.js('return Math.round(document.querySelector(".frame").getBoundingClientRect().height)'), 'brand width', await s.js('return Math.round(document.querySelector(".brand").getBoundingClientRect().width)'));
  await s.shot(here + 'run/p0-old.png');
  await sleep(500);
} finally { await s.close(); }
const cfg = here + 'run/profile/config/org.weavesignal.studio/';
console.log('settings folder:', fs.readdirSync(cfg).join(', '));
console.log('station.json:', fs.readFileSync(cfg + 'station.json', 'utf8').replace(/\s+/g, ' '));
