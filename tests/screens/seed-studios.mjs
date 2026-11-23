// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The Studios view's day for the screens stack (tests/screens/run-screens.sh, after seed.mjs):
// the design's own picture (the project folder's rehearsal-mode-build-plan.md, screen 7),
// placed around the moment it runs so that the picture is the same at any hour:
//
// - The Morning Kettle on air now (it began two to three hours ago and runs four hours), its
//   host Eilidh Fraser-MacLeod in the room;
// - Steeped straight after it, two hours, its host Finlay Grant in his own studio (the show
//   has a key, so the station says `from: own`);
// - the rehearsal studio booked for Steeped for the half hour before it goes on air, while The
//   Morning Kettle is still on air;
// - a session of The Morning Kettle as it happened, earlier (opened three hours before its
//   show, forty minutes long);
// - tomorrow, Kettle Drive has the rehearsal studio from 15:00 to 16:00 (the booking in the way
//   for the editor's refusal), and the day after has nothing booked.
//
// The bookings go through the station's own routes. A session cannot be made in the past
// through a route (and needs a rehearsal desk), so it is written as SQL to SEED_SQL, which
// run-screens.sh runs. Within about four hours of midnight, station time, part of the picture
// falls on the next day; and on a day when Kettle FM's own week has a show in those hours the
// Studio column shows the clash, side by side. Names are from the Tartan list (ADR-0071).
//
//   CONTROL=http://127.0.0.1:26310 ADMIN_TOKEN=... SEED_SQL=run/seed-studios.sql node tests/screens/seed-studios.mjs

const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:26310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'screens-admin-test-only';
const ZONE = process.env.STATION_ZONE ?? 'Europe/London';
const STATION = process.env.STATION_ID ?? '6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10';
const SEED_SQL = process.env.SEED_SQL ?? null;

async function call(method, path, body, ok = []) {
  const res = await fetch(`${CONTROL}${path}`, {
    method,
    headers: { Authorization: `Bearer ${TOKEN}`, ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok && !ok.includes(res.status)) throw new Error(`${method} ${path}: ${res.status} ${text}`);
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

const HOUR = 3_600_000;
const wall = (ms) => Object.fromEntries(new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', timeZoneName: 'longOffset' }).formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
/** `2026-11-25T10:00:00+00:00[Europe/London]` for a moment. */
function zoned(ms) {
  const p = wall(ms);
  const off = p.timeZoneName === 'GMT' ? '+00:00' : p.timeZoneName.replace('GMT', '');
  return `${p.year}-${p.month}-${p.day}T${String(Number(p.hour) % 24).padStart(2, '0')}:${p.minute}:00${off}[${ZONE}]`;
}
/** The moment the station's wall clock reads `hh:mm` on the day `days` after today. */
function onDay(days, hh, mm = 0) {
  const p = wall(Date.now());
  const want = Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day) + days, hh, mm);
  let guess = want;
  for (let i = 0; i < 3; i++) {
    const q = wall(guess);
    guess += want - Date.UTC(Number(q.year), Number(q.month) - 1, Number(q.day), Number(q.hour) % 24, Number(q.minute));
  }
  return guess;
}

const now = Date.now();
/** The hour The Morning Kettle began: two to three hours ago. */
const kettleStart = Math.floor(now / HOUR) * HOUR - 2 * HOUR;
const steepedStart = kettleStart + 4 * HOUR;

const SHOWS = [
  { slug: 'the-morning-kettle', name: 'The Morning Kettle', colour_slot: 4, layout: 'poster', slot: null, hosts: ['Eilidh Fraser-MacLeod'], tags: ['talk'], description: 'Eilidh and guests, with the kettle on.', at: kettleStart, minutes: 240 },
  { slug: 'steeped', name: 'Steeped', colour_slot: 11, layout: 'slate', slot: null, hosts: ['Finlay Grant'], tags: ['music'], description: 'Finlay Grant, from his own studio.', at: steepedStart, minutes: 120 },
];
for (const s of SHOWS) {
  const { slug, at, minutes, ...input } = s;
  await call('PUT', `/api/v1/shows/${slug}`, { ...input, explicit: false, step: 'full' });
  const view = (await call('GET', `/api/v1/shows/${slug}`)).body;
  if (!view.series.length) {
    await call('PUT', `/api/v1/shows/${slug}/series/1`, {});
    await call('POST', `/api/v1/shows/${slug}/series/1/episodes`, { count: 1 });
  }
  await call('PATCH', `/api/v1/shows/${slug}/series/1/episodes/1`, { air_at: zoned(at), minutes });
}

// Steeped's host comes in from his own studio: a key for the show, which is what makes the
// station say `from: own`. A station that will not make one (no sources set up) leaves him in
// the room, and the driver says which it found.
const keys = await call('GET', '/api/v1/shows/steeped/keys', undefined, [404, 409, 503]);
if (keys.status === 200 && !(keys.body?.keys ?? []).length) {
  const made = await call('POST', '/api/v1/shows/steeped/keys', { label: 'Finlay Grant', hint: 'icecast' }, [400, 403, 404, 409, 422, 503]);
  if (made.status >= 300) console.log(`no key for Steeped (${made.status} ${made.body?.error ?? ''}): its host stays in the room`);
}

// The bookings: Steeped's half hour before it goes on air, and Kettle Drive's hour tomorrow.
// One that is already there (a second run) is in its own way: 409, and left.
const BOOKINGS = [
  { show: 'steeped', start_ms: steepedStart - HOUR / 2, end_ms: steepedStart },
  { show: 'kettle-drive', start_ms: onDay(1, 15), end_ms: onDay(1, 16) },
];
for (const b of BOOKINGS) {
  const r = await call('POST', '/api/v1/rehearsal/bookings', b, [409, 422, 503]);
  if (r.status === 503) throw new Error('the rehearsal studio is off: run-screens.sh must give the control plane a [rehearsal] section');
  if (r.status === 422) console.log(`not booked (${r.body?.error ?? '422'}): ${b.show} at ${zoned(b.start_ms)}`);
}

// A session as it happened: The Morning Kettle rehearsed three hours before it went on air.
if (SEED_SQL) {
  const from = new Date(kettleStart - 3 * HOUR + 2 * 60_000).toISOString();
  const to = new Date(kettleStart - 3 * HOUR + 40 * 60_000).toISOString();
  const { writeFileSync } = await import('node:fs');
  writeFileSync(
    SEED_SQL,
    [
      'BEGIN;',
      `INSERT INTO rehearsal_session (id, station_id, show_id, opened_by, opened_at, ended_at, ended_how)`,
      `  SELECT '5e551011-0000-4000-8000-000000000001', '${STATION}', id, 'Eilidh', '${from}', '${to}', 'ended' FROM show WHERE station_id = '${STATION}' AND slug = 'the-morning-kettle'`,
      `  ON CONFLICT (id) DO UPDATE SET opened_at = EXCLUDED.opened_at, ended_at = EXCLUDED.ended_at;`,
      'COMMIT;',
      '',
    ].join('\n'),
  );
}
console.log(`seeded the Studios view: The Morning Kettle on air since ${zoned(kettleStart).slice(11, 16)}, Steeped at ${zoned(steepedStart).slice(11, 16)} with the rehearsal studio booked before it, a session earlier, Kettle Drive's booking tomorrow`);
