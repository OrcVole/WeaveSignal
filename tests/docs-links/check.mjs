// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Checks the built website (docs/dist): every internal link, image, script, stylesheet, font,
// video and audio source resolves to a file in the build, the redirect pages included, and every
// fragment points at an id on its page. Nothing points at /updates/, which the deploy adds.
// Usage: node tests/docs-links/check.mjs [docs/dist]
import { readdirSync, readFileSync, statSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';

const root = resolve(process.argv[2] ?? 'docs/dist');
const pages = [];
const walk = (d) => {
  for (const n of readdirSync(d)) {
    const f = join(d, n);
    if (statSync(f).isDirectory()) walk(f);
    else if (n.endsWith('.html')) pages.push(f);
  }
};
walk(root);

const ids = new Map();
const idsOf = (file) => {
  if (!ids.has(file)) {
    const html = readFileSync(file, 'utf8');
    ids.set(file, new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])));
  }
  return ids.get(file);
};
const fileFor = (path) => {
  const p = decodeURIComponent(path.split('?')[0]);
  const candidates = p.endsWith('/') ? [join(root, p, 'index.html')] : [join(root, p), join(root, p, 'index.html'), join(root, `${p}.html`)];
  return candidates.find((c) => existsSync(c) && statSync(c).isFile());
};

let bad = 0;
let count = 0;
const report = (page, what, why) => {
  bad++;
  console.log(`${page.slice(root.length)}: ${what}: ${why}`);
};
for (const page of pages) {
  const html = readFileSync(page, 'utf8');
  const refs = [...html.matchAll(/\s(?:href|src|poster|content)="([^"]*)"/g)].map((m) => m[1]);
  const srcsets = [...html.matchAll(/\ssrcset="([^"]*)"/g)].flatMap((m) => m[1].split(',').map((s) => s.trim().split(/\s+/)[0]));
  for (const ref of [...refs, ...srcsets]) {
    if (!ref || ref.startsWith('#') && ref.length === 1) continue;
    if (/^(https?:|mailto:|tel:|data:|javascript:)/.test(ref)) {
      if (/^https:\/\/weavesignal\.top\/updates\/[^/]+\/?$/.test(ref) || /^https:\/\/weavesignal\.top\/updates\/1\.0\.0\//.test(ref)) report(page, ref, 'a bare /updates/<version>/ directory, which has no index');
      continue;
    }
    if (ref.startsWith('//')) continue;
    if (!ref.startsWith('/') && !ref.startsWith('#')) continue; // a relative reference inside an attribute that is not a link (e.g. content="…")
    count++;
    const [path, frag] = ref.split('#');
    if (path.startsWith('/updates/')) report(page, ref, 'points into /updates/, which the deploy adds');
    const target = path === '' ? page : fileFor(path);
    if (!target) {
      report(page, ref, 'no such file in the build');
      continue;
    }
    if (frag && target.endsWith('.html') && !idsOf(target).has(frag)) report(page, ref, `no id "${frag}" on that page`);
  }
}
console.log(`${pages.length} pages, ${count} internal references, ${bad} problems`);
process.exit(bad ? 1 : 0);
