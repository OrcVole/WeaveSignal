// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A stand-in station for the station switcher's proof run: the few routes the studio app needs to
// sign in and draw a desk, with a log of every request (path and bearer) so the run can say
// which key went to which station. Test keys only. Not part of the product.
import http from 'node:http';
const [name, port, key, version, onAir] = [process.env.NAME, Number(process.env.PORT), process.env.KEY, process.env.VERSION, process.env.ON_AIR === '1'];
const template = JSON.parse(process.env.TEMPLATE);
const log = [];
const sessions = new Map();
let streams = 0, ended = [], n = 0;
const person = (slot, identity, pname, on) => ({ slot, identity, name: pname, role: slot === 1 ? 'presenter' : 'guest', on_air: on, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: on ? -21.5 : null, lufs_peak: on ? -18 : null, caller: null });
const view = () => ({ ...template, station: name, now_ms: Date.now(), desk: { ...template.desk, reachable: true, error: null, playout: { connected: true, for_ms: 60000 } },
  seats: { presenter: 'eilidh', screener: null, presenter_name: 'Eilidh Fraser-MacLeod', screener_name: '' },
  sources: onAir ? [person(1, 'eilidh', 'Eilidh Fraser-MacLeod', true), person(2, 'gregor', 'Gregor Stewart', true), person(3, 'kirsty', 'Kirsty Wilson', false)] : [person(1, 'eilidh', 'Eilidh Fraser-MacLeod', false)],
  callers: [], notices: [] });
const send = (res, status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(body === undefined ? '' : JSON.stringify(body)); };
http.createServer((req, res) => {
  const auth = (req.headers.authorization ?? '').replace(/^Bearer /, '');
  const path = req.url.split('?')[0];
  if (path === '/__log') return send(res, 200, { name, log, streams, ended });
  log.push({ n: ++n, t: Date.now(), m: req.method, path, auth });
  if (path === '/health') return send(res, 200, { status: 'ok', version });
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    if (path === '/api/v1/sessions' && req.method === 'POST') {
      const b = JSON.parse(body || '{}');
      if (b.password !== 'a long password for the proof') return send(res, 401, { error: 'That username and password do not match an account here.' });
      const token = `rss_${name.replace(/\W/g, '')}_${sessions.size + 1}`;
      const account = { id: `acct-${b.username}`, username: b.username, name: 'Eilidh Fraser-MacLeod', role: 'producer', disabled_ms: null, created_ms: 1 };
      sessions.set(token, account);
      return send(res, 201, { token, account, expires_ms: Date.now() + 3600000 });
    }
    const who = auth === key ? { kind: 'station', role: 'admin', preparing: false } : sessions.has(auth) ? { kind: 'staff', role: 'producer', account: sessions.get(auth), preparing: false } : null;
    if (!who) return send(res, 401, { error: 'Your sign-in has ended. Sign in again.' });
    if (path === '/api/v1/sessions/me' && req.method === 'DELETE') { ended.push(auth); sessions.delete(auth); res.writeHead(204); return res.end(); }
    if (path === '/api/v1/sessions/me') return send(res, 200, who);
    if (path === '/api/v1/carts') return send(res, 200, { version: 1, banks: [], carts: [] });
    if (path === '/api/v1/studio') return send(res, 200, view());
    if (path === '/api/v1/studio/events') {
      streams++;
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      const tick = () => res.write(`data: ${JSON.stringify(view())}\n\n`);
      tick();
      const t = setInterval(tick, 1000);
      res.on('close', () => { clearInterval(t); streams--; log.push({ n: ++n, t: Date.now(), m: 'CLOSE', path, auth }); });
      return;
    }
    return send(res, 404, { error: 'This stand-in station has no such route.' });
  });
}).listen(port, '127.0.0.1', () => console.log(`${name} on ${port}`));
