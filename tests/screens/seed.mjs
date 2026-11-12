// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// Kettle FM's week for the Schedule tab (the calendar design's shows and people, named from
// the Tartan list, ADR-0071): the shows with their colours (ADR-0066) and weekly rules, a
// series each with episodes drafted from the rule across the clock change, Saturday Night
// Kettle's series 3 with its aired episodes frozen, a one-off special that conflicts with
// Kettle Drive, four clocks on the week grid, a few tone tracks in the library, and the log
// generated for the week ahead. Idempotent: run it again and it puts the same things.
//
//   CONTROL=http://127.0.0.1:26310 ADMIN_TOKEN=... node tests/screens/seed.mjs

const CONTROL = process.env.CONTROL ?? 'http://127.0.0.1:26310';
const TOKEN = process.env.ADMIN_TOKEN ?? 'screens-admin-test-only';
const ZONE = process.env.STATION_ZONE ?? 'Europe/London';

async function call(method, path, body, raw = false) {
  const res = await fetch(`${CONTROL}${path}`, {
    method,
    headers: { Authorization: `Bearer ${TOKEN}`, ...(body !== undefined && !raw ? { 'Content-Type': 'application/json' } : {}) },
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text}`);
  return text ? JSON.parse(text) : null;
}

/** Monday of the week that holds `d`, as a civil date in the station's zone. */
function mondayOf(d) {
  const p = Object.fromEntries(
    new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, year: 'numeric', month: '2-digit', day: '2-digit', weekday: 'short' })
      .formatToParts(d)
      .map((x) => [x.type, x.value]),
  );
  const wd = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'].indexOf(p.weekday);
  const m = new Date(Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day) - wd));
  return m.toISOString().slice(0, 10);
}

const now = new Date();
// The series start two weeks back, so the week before this one (with its clock change, if the
// run is near one) has drafted episodes too, and the earliest can be frozen as aired.
const start = mondayOf(new Date(now.getTime() - 14 * 86_400_000));

// weekday 1 Monday .. 7 Sunday; start in minutes after midnight, station time.
const slot = (weekday, hh, mm, minutes) => ({ weekday, start: hh * 60 + mm, minutes });
const SHOWS = [
  { slug: 'kettle-breakfast', name: 'Kettle Breakfast', colour_slot: 1, layout: 'poster', slot: slot(1, 7, 0, 180), hosts: ['Ailsa MacMillan'], tags: ['breakfast', 'news'], description: 'Up with the kettle: news, travel and the first brew of the day.' },
  { slug: 'the-long-lunch', name: 'The Long Lunch', colour_slot: 2, layout: 'slate', slot: slot(3, 12, 0, 120), hosts: ['Duncan Boyd'], tags: ['lunchtime', 'requests'], description: 'Two hours of requests and the lunchtime quiz.' },
  { slug: 'kettle-drive', name: 'Kettle Drive', colour_slot: 7, layout: 'poster', slot: slot(4, 16, 0, 180), hosts: ['Heather Douglas'], tags: ['drive', 'travel'], description: 'Home with Heather: travel, sport and the big tunes.' },
  { slug: 'thursday-session', name: 'Thursday Session', colour_slot: 5, layout: 'slate', slot: slot(4, 20, 0, 120), hosts: ['Heather Douglas'], tags: ['live music'], description: 'A live session from the Kettle studio every Thursday.' },
  { slug: 'firth-sessions', name: 'Firth Sessions', colour_slot: 9, layout: 'slate', slot: slot(2, 20, 0, 60), hosts: ['Rory Bruce'], tags: ['folk', 'recorded'], description: 'Folk from both shores of the Forth, recorded on Mondays.' },
  { slug: 'weekend-brunch', name: 'Weekend Brunch', colour_slot: 6, layout: 'poster', slot: slot(6, 8, 0, 180), hosts: ['Hamish Wallace'], tags: ['weekend'], description: 'Saturday mornings, slowly.' },
  { slug: 'saturday-night', name: 'Saturday Night Kettle', colour_slot: 3, layout: 'poster', slot: slot(6, 20, 0, 180), hosts: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], tags: ['live music', 'Fife', 'requests'], description: 'Live music and requests to see Saturday out.' },
  { slug: 'the-archive-hour', name: 'The Archive Hour', colour_slot: 10, layout: 'slate', slot: slot(7, 20, 0, 60), hosts: ['Magnus Henderson'], tags: ['archive'], description: 'An hour from the station’s archive, introduced.' },
  { slug: 'council-election-special', name: 'Council election special', colour_slot: 8, layout: 'poster', slot: null, hosts: ['Heather Douglas', 'Duncan Boyd'], tags: ['news', 'politics'], description: 'The count, live from the Glen Pavilion.' },
];

for (const s of SHOWS) {
  const { slug, ...input } = s;
  await call('PUT', `/api/v1/shows/${slug}`, { ...input, explicit: false, step: 'full' });
}

// Series 1 of every weekly show, starting two weeks back, drafted to a few weeks ahead; Saturday
// Night Kettle is in its series 3 of ten, from the first Saturday, as the design has it.
for (const s of SHOWS) {
  if (!s.slot) continue;
  const n = s.slug === 'saturday-night' ? 3 : 1;
  const view = await call('GET', `/api/v1/shows/${s.slug}`);
  if (!view.series.some((x) => x.number === n)) {
    await call('PUT', `/api/v1/shows/${s.slug}/series/${n}`, {
      starts_on: start,
      ...(s.slug === 'saturday-night' ? { planned_episodes: 10, title: 'autumn' } : {}),
    });
    const made = await call('POST', `/api/v1/shows/${s.slug}/series/${n}/episodes`, { count: s.slug === 'saturday-night' ? 7 : 4 });
    // The first two instances have gone out: frozen as aired.
    for (const e of made.slice(0, 2)) {
      if (e.episode.air_at * 1000 < now.getTime()) await call('POST', `/api/v1/shows/${s.slug}/series/${n}/episodes/${e.episode.number}/air`);
    }
    if (s.slug === 'saturday-night') {
      const tonight = made.find((e) => e.episode.air_at * 1000 > now.getTime());
      if (tonight) {
        await call('PATCH', `/api/v1/shows/${s.slug}/series/${n}/episodes/${tonight.episode.number}`, {
          title: 'Ghost Train night',
          guests: ['Firth Lights', 'Dr Una Robertson'],
          description: 'Firth Lights play "Ghost Train" live in the studio, and Dr Una Robertson on the Forth’s lost ferries.',
        });
      }
    }
    if (s.slug === 'kettle-breakfast') {
      // Friday's Kettle Breakfast moved an hour later this week: MOVED · was 07:00 on the grid.
      const future = made.filter((e) => e.episode.air_at * 1000 > now.getTime());
      const e = future[0];
      if (e) {
        const at = new Date(e.episode.air_at * 1000 + 4 * 86_400_000 + 3_600_000);
        const z = zoned(at);
        await call('PATCH', `/api/v1/shows/${s.slug}/series/${n}/episodes/${e.episode.number}`, { air_at: z, minutes: 120 });
      }
    }
  }
}

// The one-off special, in the way of Kettle Drive: Thursday 18:00 to 19:30 this week.
{
  const slug = 'council-election-special';
  const view = await call('GET', `/api/v1/shows/${slug}`);
  if (!view.series.length) {
    await call('PUT', `/api/v1/shows/${slug}/series/1`, { title: 'the count' });
    const thursday = new Date(Date.parse(`${mondayOf(now)}T00:00:00Z`) + 3 * 86_400_000);
    const at = zonedAt(thursday, 18, 0);
    await call('POST', `/api/v1/shows/${slug}/series/1/episodes`, { count: 1, title: 'The count', guests: ['Dr Una Robertson'] });
    await call('PATCH', `/api/v1/shows/${slug}/series/1/episodes/1`, { air_at: at, minutes: 90 });
  }
}

/** `2026-10-29T18:00:00+00:00[Europe/London]` for a civil date's wall clock in the zone. */
function zonedAt(dayUtc, hh, mm) {
  const y = dayUtc.getUTCFullYear(), m = dayUtc.getUTCMonth() + 1, d = dayUtc.getUTCDate();
  // Find the offset by formatting a guess.
  let guess = Date.UTC(y, m - 1, d, hh, mm);
  for (let i = 0; i < 3; i++) {
    const p = Object.fromEntries(new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }).formatToParts(new Date(guess)).map((x) => [x.type, x.value]));
    const got = Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), Number(p.hour) % 24, Number(p.minute));
    const want = Date.UTC(y, m - 1, d, hh, mm);
    if (got === want) break;
    guess += want - got;
  }
  return zoned(new Date(guess));
}
function zoned(date) {
  const p = Object.fromEntries(new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', timeZoneName: 'longOffset' }).formatToParts(date).map((x) => [x.type, x.value]));
  const off = p.timeZoneName === 'GMT' ? '+00:00' : p.timeZoneName.replace('GMT', '');
  return `${p.year}-${p.month}-${p.day}T${String(Number(p.hour) % 24).padStart(2, '0')}:${p.minute}:00${off}[${ZONE}]`;
}

// Clocks and the week grid: every local hour has a clock, so automation has a name.
const CLOCKS = [
  ['11111111-1111-4111-8111-111111111101', 'Overnight playlist', [0, 6]],
  ['11111111-1111-4111-8111-111111111102', 'Daytime playlist', [6, 19]],
  ['11111111-1111-4111-8111-111111111103', 'Evening playlist', [19, 22]],
  ['11111111-1111-4111-8111-111111111104', 'Late playlist', [22, 24]],
];
for (const [id, name] of CLOCKS) {
  await call('PUT', `/api/v1/schedule/clocks/${id}`, { name, slots: [{ category: 'a' }, { category: 'gold' }, { category: 'a' }] });
}
const hours = [];
for (let wd = 0; wd < 7; wd++) for (const [id, , [from, to]] of CLOCKS) for (let h = from; h < to; h++) hours.push({ weekday: wd, hour: h, clock: id });
await call('PUT', '/api/v1/schedule/week', { hours });

// A small library: eight-second tones, two artists, one album, so the separation rules bite.
const tracks = await call('GET', '/api/v1/library/tracks');
if (tracks.tracks.length < 6) {
  const bands = ['Firth Lights', 'Rùnach', 'Leith Shanty Collective'];
  for (let i = 0; i < 9; i++) {
    const f = 220 * 2 ** (i / 6);
    const q = new URLSearchParams({ title: `Tone ${Math.round(f)}`, artist: bands[i % 3], album: i < 3 ? 'Harbour Tapes' : '', label: 'Kettle Records', category: i % 2 ? 'gold' : 'a' });
    await call('PUT', `/api/v1/library/tracks?${q}`, wav(f), true);
  }
}
await call('POST', '/api/v1/schedule/generate', {});
console.log('seeded: Kettle FM’s shows, series and episodes, four clocks, the week grid, the library, the log');

/** An eight-second stereo sine as a WAV file. */
function wav(freq) {
  const rate = 48000, secs = 8, frames = rate * secs, data = frames * 4;
  const b = Buffer.alloc(44 + data);
  b.write('RIFF', 0); b.writeUInt32LE(36 + data, 4); b.write('WAVEfmt ', 8); b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(2, 22);
  b.writeUInt32LE(rate, 24); b.writeUInt32LE(rate * 4, 28); b.writeUInt16LE(4, 32); b.writeUInt16LE(16, 34); b.write('data', 36); b.writeUInt32LE(data, 40);
  for (let i = 0; i < frames; i++) {
    const v = Math.round(Math.sin((2 * Math.PI * freq * i) / rate) * 0.3 * 32767);
    b.writeInt16LE(v, 44 + i * 4); b.writeInt16LE(v, 46 + i * 4);
  }
  return b;
}
