// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A small W3C WebDriver client for the proof run (tauri-driver on :4455).
const BASE = 'http://127.0.0.1:4455';
async function call(method, path, body) {
  const res = await fetch(BASE + path, { method, headers: { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const j = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(j.value ?? j).slice(0, 300)}`);
  return j.value;
}
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export async function open(application) {
  const v = await call('POST', '/session', { capabilities: { alwaysMatch: { 'tauri:options': { application } } } });
  const id = v.sessionId;
  const s = {
    id,
    js: (script, ...args) => call('POST', `/session/${id}/execute/sync`, { script, args }),
    async find(css) { const e = await call('POST', `/session/${id}/element`, { using: 'css selector', value: css }); return Object.values(e)[0]; },
    async click(css) { const e = await s.find(css); await call('POST', `/session/${id}/element/${e}/click`, {}); },
    async type(css, text) { const e = await s.find(css); await call('POST', `/session/${id}/element/${e}/clear`, {}); await call('POST', `/session/${id}/element/${e}/value`, { text }); },
    async keys(text) { const e = await call('GET', `/session/${id}/element/active`); await call('POST', `/session/${id}/element/${Object.values(e)[0]}/value`, { text }); },
    text: (css) => s.js(`const e=document.querySelector(arguments[0]); return e ? e.innerText : null;`, css),
    has: (css) => s.js(`return !!document.querySelector(arguments[0]);`, css),
    async wait(what, fn, ms = 15000) { const end = Date.now() + ms; let last; for (;;) { try { last = await fn(); if (last) return last; } catch (e) { last = String(e).slice(0, 200); } if (Date.now() > end) throw new Error(`waited for ${what}; last: ${JSON.stringify(last)}`); await sleep(250); } },
    waitFor: (css, ms) => s.wait(css, () => s.has(css), ms),
    async shot(file) { const b64 = await call('GET', `/session/${id}/screenshot`); (await import('node:fs')).writeFileSync(file, Buffer.from(b64, 'base64')); },
    rect: (r) => call('POST', `/session/${id}/window/rect`, r),
    close: () => call('DELETE', `/session/${id}`).catch(() => {}),
  };
  return s;
}
