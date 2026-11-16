// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule drivers' stand-in station (tests/newshow/station.mjs) served over HTTP on
// loopback, so the real studio app can sign in to it, show its week and read a calendar file
// into it. It
// logs what the import routes were sent. Test tooling, not part of the product.
import http from 'node:http';
import { Station, clashWeek } from '../newshow/station.mjs';

const port = Number(process.env.PORT);
// With the week driver's week of clashes (Monday 16 November 2026), so the run can look at the
// fold in the app's own webview too.
const station = clashWeek(new Station({ version: '1.0.5' }));
station.add({ slug: 'the-late-kettle', name: 'The Late Kettle', colour_slot: 8, presenter: 'Shona Campbell', hosts: ['Shona Campbell'], rule: { starts_on: '2026-11-03', start: 1260, minutes: 120, repeat: { kind: 'weekly', every: 1, weekdays: [2] }, end: { kind: 'never' }, skip: [], moves: [] } });
const send = (res, status, body, type = 'application/json') => {
  res.writeHead(status, { 'content-type': type });
  res.end(typeof body === 'string' ? body : JSON.stringify(body));
};
http
  .createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    if (url.pathname === '/__imports') return send(res, 200, station.calls.filter((c) => c.path.startsWith('/api/v1/schedule/import')));
    // The app asks for the cart wall to learn whether the station takes its key.
    if (url.pathname === '/api/v1/carts') return send(res, 200, { version: 1, banks: [], carts: [] });
    if (url.pathname === '/api/v1/studio/events' || url.pathname.startsWith('/api/v1/carts') || url.pathname.startsWith('/api/v1/chat')) return send(res, 503, { error: 'not in the stand-in' });
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
      if (typeof out === 'string') send(res, status, out, 'text/calendar; charset=utf-8');
      else send(res, status, out);
    });
  })
  .listen(port, '127.0.0.1', () => console.log(`Kettle FM (stand-in) on ${port}`));
