// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The hosts driver's stand-in station (tests/hosts/station.mjs) served over HTTP on loopback,
// so the real studio app can sign in to it, draw a host's own studio on its desk and set a
// host up in Settings. The day is the real one and its clock runs, because the app's own clock
// does. Duncan is in the room and Finlay's software is connected over SRT, standing by.
// Test tooling, not part of the product.
import http from 'node:http';
import { HostStation } from '../station.mjs';

const port = Number(process.env.PORT);
const today = new Date().toISOString().slice(0, 10);
const station = new HostStation({ today, role: 'admin', host: null });
const tick = () => (station.now = Math.floor(Date.now() / 1000));
tick();
station.join('duncan', 'Duncan Boyd', 'presenter');
const finlay = station.give('steeped', 'Finlay Grant', 'srt');
station.connect(finlay.key_id, 'srt', 'mpegts/aac 48 kHz stereo');
setInterval(tick, 500);

const send = (res, status, body) => {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
};
http
  .createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    // What the studio sent, and what the station holds, for the driver to read.
    if (url.pathname === '/__calls') return send(res, 200, station.calls.filter((c) => c.method !== 'GET'));
    if (url.pathname === '/__keys') return send(res, 200, station.keys.map((k) => ({ key_id: k.key_id, label: k.label, show: k.show, secret: k.secret, monitor: k.monitor, revoked_ms: k.revoked_ms })));
    if (url.pathname === '/__desk') return send(res, 200, station.people);
    // The app asks for the cart wall to learn whether the station takes its key.
    if (url.pathname === '/api/v1/carts') return send(res, 200, { version: 1, banks: [], carts: [] });
    // The studio's view, as server-sent events: the desk as it stands, each second.
    if (url.pathname === '/api/v1/studio/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
      const say = () => res.write(`data: ${JSON.stringify(station.studioView())}\n\n`);
      say();
      const every = setInterval(say, 1000);
      req.on('close', () => clearInterval(every));
      return;
    }
    if (url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return send(res, 503, { error: 'not in the stand-in' });
    let text = '';
    req.on('data', (c) => (text += c));
    req.on('end', () => {
      let body;
      try {
        body = text ? JSON.parse(text) : undefined;
      } catch {
        return send(res, 422, { error: 'not JSON' });
      }
      const [status, out] = station.handle(req.method, url.pathname + url.search, body);
      send(res, status, out);
    });
  })
  .listen(port, '127.0.0.1', () => console.log(`Kettle FM (stand-in, hosts) on ${port}`));
