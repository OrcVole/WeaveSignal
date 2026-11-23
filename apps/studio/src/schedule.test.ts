// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's rules, tested without a page: the station's wall clock in its zone (a
// clocks-back Sunday has 25 hours), the week's blocks (episodes, a moved instance, a conflict,
// a date from a show's rule not yet drafted, as the station sent it, automation named by the
// clock of each local hour), the
// editor's markers (inherited, set here, as aired), the block markup, the keymap's schedule
// keys, and the editors rendered on the server from a seeded week.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { KeyEngine, type KeyInput } from '../../../packages/ui/keymap.ts';
import { CALENDAR, panel } from '../../../packages/ui/panels.ts';
import EpisodeEditor from './schedule/EpisodeEditor.svelte';
import LogEditor from './schedule/LogEditor.svelte';
import ShowEditor from './schedule/ShowEditor.svelte';
import Schedule from './schedule/Schedule.svelte';
import { blockAria, blockClasses, blockHtml } from './schedule/blockHtml.ts';
import {
  automationBlocks,
  changeWords,
  cardFields,
  dayLength,
  episodeBlocks,
  fromFloating,
  floating,
  hm,
  instant,
  marker,
  dateBlocks,
  movedWords,
  parts,
  rangeWords,
  slugOf,
  weekOf,
  zonedString,
  type Block,
} from './schedule/model.ts';
import type { ScheduleDate } from './lib/api.ts';
import type { Clock, Show, WeekEpisode, WeekHour } from './lib/schedule.ts';
import { closeOpen, noteUnsaved, openDay, openShow, sched, select } from './lib/schedule.svelte.ts';
import { run } from './lib/actions.ts';
import { studio } from './lib/studio.svelte.ts';

const ZONE = 'Europe/London';
/** Saturday 24 October 2026, 13:57 in Edinburgh (BST). */
const NOW = Date.UTC(2026, 9, 24, 12, 57);

describe('the station clock', () => {
  it('reads the wall clock in the station zone and turns it back into a moment', () => {
    const p = parts(NOW, ZONE);
    expect([p.y, p.m, p.d, p.hh, p.mm, p.wd]).toEqual([2026, 10, 24, 13, 57, 5]);
    expect(instant(p, ZONE)).toBe(NOW);
    // After the clocks go back, 20:00 is 20:00 UTC.
    expect(instant({ y: 2026, m: 10, d: 31, hh: 20, mm: 0 }, ZONE)).toBe(Date.UTC(2026, 9, 31, 20, 0));
    expect(hm(Date.UTC(2026, 9, 31, 20, 0), ZONE)).toBe('20:00');
    // The floating date is the station's wall clock in this machine's zone, whatever that is.
    expect(fromFloating(floating(NOW, ZONE), ZONE)).toBe(NOW);
  });

  it('gives the clocks-back Sunday 25 hours and names it', () => {
    const { days } = weekOf(NOW, ZONE);
    expect(days).toHaveLength(8);
    expect(hm(days[0], ZONE)).toBe('00:00');
    expect(parts(days[0], ZONE).d).toBe(19);
    expect(dayLength(days, 5)).toBe('24 h');
    expect(dayLength(days, 6)).toBe('25 h · clocks go back');
    expect(rangeWords(days, ZONE)).toBe('19 – 25 October 2026');
    // The spring week has a 23-hour Sunday.
    const spring = weekOf(Date.UTC(2027, 2, 24), ZONE);
    expect(dayLength(spring.days, 6)).toBe('23 h · clocks go forward');
  });

  it('writes a moment as a zoned time for the control plane, either side of the clock change', () => {
    const sat = instant({ y: 2026, m: 10, d: 24, hh: 20, mm: 0 }, ZONE);
    const next = instant({ y: 2026, m: 10, d: 31, hh: 20, mm: 0 }, ZONE);
    expect(next - sat).toBe(7 * 24 * 3_600_000 + 3_600_000);
    expect(zonedString(sat, ZONE)).toBe('2026-10-24T20:00:00+01:00[Europe/London]');
    expect(zonedString(next, ZONE)).toBe('2026-10-31T20:00:00+00:00[Europe/London]');
  });
});

const show = (slug: string, name: string, colour_slot: number, slot: Show['slot']): Show => ({
  id: slug,
  slug,
  name,
  description: '',
  colour_slot,
  layout: 'poster',
  slot,
  hosts: [],
  tags: [],
  explicit: false,
  feed_guid: null,
  cards: null,
  presenter: '',
  contact: '',
  links: [],
  commercial_music: false,
  presenter_account: null,
});

function episode(
  s: Show,
  n: number,
  airMs: number,
  o: { minutes?: number; title?: string; hosts?: string[]; aired?: boolean; inherited?: string[]; guests?: string[]; kind?: 'live' | 'prerecorded' } = {},
): WeekEpisode {
  const resolved = {
    show: s.name,
    slug: s.slug,
    description: '',
    colour_slot: s.colour_slot,
    layout: s.layout,
    slot: s.slot,
    hosts: o.hosts ?? ['Shona Campbell', 'Catriona MacKenzie-Buchanan'],
    guests: o.guests ?? [],
    tags: ['live music'],
    explicit: false,
    series: { number: 3, title: 'autumn', first: '2026-09-05', last: '2026-11-07', episodes: 10 },
    episode: { id: `${s.slug}-${n}`, number: n, title: o.title ?? '', description: '', kind: o.kind ?? 'live', air_at: airMs / 1000, minutes: o.minutes ?? null },
    inherited: o.inherited ?? ['hosts', 'tags', 'minutes'],
    frozen: !!o.aired,
  };
  return {
    show: s,
    series: { id: `${s.slug}-s3`, show_id: s.id, number: 3, title: 'autumn', description: null, hosts: null, slot: null, starts_on: '2026-09-05', planned_episodes: 10, tags: null, cards: null },
    episode: {
      id: `${s.slug}-${n}`,
      series_id: `${s.slug}-s3`,
      number: n,
      title: o.title ?? '',
      description: '',
      kind: o.kind ?? 'live',
      air_at: airMs / 1000,
      minutes: o.minutes ?? null,
      hosts: null,
      guests: o.guests ?? [],
      tags: null,
      aired_at: o.aired ? airMs / 1000 + 3 * 3600 : null,
      frozen: o.aired ? resolved : null,
      files: null,
      cards: null,
    },
    resolved,
  };
}

const SNK = show('saturday-night', 'Saturday Night Kettle', 3, { weekday: 6, start: 20 * 60, minutes: 180 });
const DRIVE = show('kettle-drive', 'Kettle Drive', 7, { weekday: 4, start: 16 * 60, minutes: 180 });
const SPECIAL = show('council-election-special', 'Council election special', 8, null);
const BREAKFAST = show('kettle-breakfast', 'Kettle Breakfast', 1, { weekday: 1, start: 7 * 60, minutes: 180 });
const at = (d: number, hh: number, mm = 0) => instant({ y: 2026, m: 10, d, hh, mm }, ZONE);

const CLOCKS: Clock[] = [
  { id: 'c1', name: 'Overnight playlist', slots: [{ category: 'a' }] },
  { id: 'c2', name: 'Daytime playlist', slots: [{ category: 'a' }] },
];
const HOURS: WeekHour[] = [];
for (let wd = 0; wd < 7; wd++) for (let h = 0; h < 24; h++) HOURS.push({ weekday: wd, hour: h, clock: h < 6 ? 'c1' : 'c2' });

/** A date a rule gives, as the station sends it with the week. */
const date = (s: Show, on: string, hh: number, o: Partial<ScheduleDate> = {}): ScheduleDate => {
  const [y, m, d] = on.split('-').map(Number);
  return { show: s.slug, series: 3, on, number: null, start: instant({ y, m, d, hh, mm: 0 }, ZONE) / 1000, minutes: 180, moved_from: null, clock: null, ...o };
};

function week(dates: ScheduleDate[] | null = []) {
  const eps = [
    episode(SNK, 7, at(24, 20), { title: 'Ghost Train night', guests: ['Firth Lights'] }),
    episode(SNK, 6, at(17, 20), { title: 'The harbour tapes', aired: true }),
    episode(DRIVE, 99, at(22, 16)),
    episode(SPECIAL, 1, at(22, 18), { minutes: 90, title: 'The count' }),
    episode(BREAKFAST, 212, at(23, 8), { minutes: 120 }),
  ];
  const { days } = weekOf(NOW, ZONE);
  const blocks = episodeBlocks(eps, ZONE, NOW);
  const rules = dateBlocks(dates, [SNK, DRIVE, SPECIAL, BREAKFAST]);
  const auto = automationBlocks([...blocks, ...rules], days, ZONE, HOURS, CLOCKS, null);
  return { eps, days, blocks, rules, auto };
}

describe('the week', () => {
  it('places episodes, flags a conflict on both, and a moved instance with its usual time', () => {
    const { blocks } = week();
    const snk = blocks.find((b) => b.id === 'ep:saturday-night-7')!;
    expect(hm(snk.start, ZONE)).toBe('20:00');
    expect(snk.end - snk.start).toBe(180 * 60_000);
    expect(snk.aired).toBe(false);
    expect(snk.conflict).toBe(false);
    expect(blocks.find((b) => b.id === 'ep:kettle-drive-99')!.conflict).toBe(true);
    expect(blocks.find((b) => b.id === 'ep:council-election-special-1')!.conflict).toBe(true);
    expect(blocks.find((b) => b.id === 'ep:kettle-breakfast-212')!.moved).toBe('was Mon 07:00');
    expect(blocks.find((b) => b.id === 'ep:saturday-night-6')!.frozen).toBe(true);
  });

  it('draws the dotted blocks the station sends, and works out none itself (ADR-0278)', () => {
    // The station sent no dates for this week: every date has its episode.
    expect(week().rules).toEqual([]);
    // A station too old to send dates draws none; the list of shows offers the next episode instead.
    expect(week(null).rules).toEqual([]);
    const sent = [date(SNK, '2026-10-31', 20, { number: 8 }), date(DRIVE, '2026-10-29', 16), date(SNK, '2026-11-07', 21, { moved_from: '2026-11-06T20:00:00' })];
    const next = dateBlocks(sent, [SNK, DRIVE]);
    expect(next.map((r) => [r.slug, hm(r.start, ZONE), r.on])).toEqual([
      ['saturday-night', '20:00', '2026-10-31'],
      ['kettle-drive', '16:00', '2026-10-29'],
      ['saturday-night', '21:00', '2026-11-07'],
    ]);
    expect(next[0]).toMatchObject({ kind: 'rule', id: 'rule:saturday-night:2026-10-31', show: 'Saturday Night Kettle', colour: 3, series: 3, number: 8, weekly: true });
    // The length is the station's, to the minute, on the night the clocks go back too.
    expect(next[0].end - next[0].start).toBe(180 * 60_000);
    expect(next[2].moved).toBe('was Fri 6 Nov');
    // A date for a show this window does not know is not drawn.
    expect(dateBlocks([date(SPECIAL, '2026-10-31', 20)], [SNK])).toEqual([]);
    const ctx = { now: NOW, zone: ZONE, selected: null };
    expect(blockClasses(next[0], ctx)).toEqual(['ev', 'rule']);
    expect(blockHtml(next[0], ctx)).toContain('Not drafted');
    expect(blockHtml(next[0], ctx)).toContain('Ep 8 · Select it to draft or skip');
    expect(blockAria(next[0], ctx)).toBe('Saturday Night Kettle, a date from its rule, 20:00 to 23:00, not drafted yet: select it to draft the episode or skip the date');
    expect(blockHtml(next[2], ctx)).toContain('Moved · was Fri 6 Nov');
  });

  it('says an episode is moved from the rule date the station says it fills', () => {
    const ep = episode(SNK, 8, at(31, 20));
    const filled = (occurrence: string | null, rule = true) => ({
      episode: { ...ep.episode, occurrence },
      resolved: { ...ep.resolved, ...(rule ? { rule: { starts_on: null, start: 1200, minutes: 180, repeat: { kind: 'weekly' as const, every: 1, weekdays: [6] }, end: { kind: 'never' as const }, skip: [], moves: [] } } : {}) },
    });
    // On its rule's date and time: not moved.
    expect(movedWords(filled('2026-10-31'), at(31, 20), ZONE)).toBeNull();
    // Aired on another day, or at another time that day.
    expect(movedWords(filled('2026-10-30'), at(31, 20), ZONE)).toBe('was Fri 30 Oct');
    expect(movedWords(filled('2026-10-31'), at(31, 21), ZONE)).toBe('was 20:00');
    // Made or left by hand: it is its own, not moved.
    expect(movedWords(filled(null), at(31, 21), ZONE)).toBeNull();
    // An older station says no date: the weekly slot is read as before.
    expect(movedWords(ep, at(30, 20), ZONE)).toBe('was Sat 20:00');
  });

  it('fills every gap with automation named by the clock of each local hour, 25 hours on Sunday', () => {
    const { auto, days } = week();
    const sunday = auto.filter((b) => b.start >= days[6]);
    expect(sunday.map((b) => [b.show, hm(b.start, ZONE), hm(b.end, ZONE)])).toEqual([
      ['Overnight playlist', '00:00', '06:00'],
      ['Daytime playlist', '06:00', '00:00'],
    ]);
    // The overnight run is seven hours long on the clocks-back night.
    expect(sunday[0].end - sunday[0].start).toBe(7 * 3_600_000);
    expect(sunday[0].end + sunday[1].end - sunday[1].start - sunday[0].start).toBe(25 * 3_600_000);
    // Saturday: overnight, daytime to 20:00, then after the show.
    const saturday = auto.filter((b) => b.start >= days[5] && b.start < days[6]);
    expect(saturday.map((b) => [b.show, hm(b.start, ZONE), hm(b.end, ZONE)])).toEqual([
      ['Overnight playlist', '00:00', '06:00'],
      ['Daytime playlist', '06:00', '20:00'],
      ['Daytime playlist', '23:00', '00:00'],
    ]);
    for (const b of auto) expect(b.kind).toBe('automation');
  });

  it('marks a block in words and classes: aired, on air, conflict, moved, automation', () => {
    const { blocks, auto } = week();
    const ctx = { now: NOW, zone: ZONE, selected: 'ep:saturday-night-7' };
    const snk = blocks.find((b) => b.id === 'ep:saturday-night-7')!;
    expect(blockClasses(snk, ctx)).toEqual(['ev', 'sel']);
    expect(blockHtml(snk, ctx)).toContain('<b>Saturday Night Kettle</b>');
    expect(blockHtml(snk, ctx)).toContain('S3 Ep 7 · Ghost Train night');
    expect(blockHtml(snk, ctx)).toContain('Shona Campbell, Catriona MacKenzie-Buchanan');
    expect(blockAria(snk, ctx)).toBe('Saturday Night Kettle, live, 20:00 to 23:00, S3 Ep 7 · Ghost Train night');
    const aired = blocks.find((b) => b.id === 'ep:saturday-night-6')!;
    expect(blockClasses(aired, ctx)).toEqual(['ev', 'past']);
    expect(blockHtml(aired, ctx)).toContain('Live · aired');
    // A pre-recorded episode says so (ADR-0188), in words and to a screen reader.
    const tape = { ...snk, prerecorded: true };
    expect(blockHtml(tape, ctx)).toContain('Pre-recorded');
    expect(blockHtml(tape, ctx)).not.toContain('Live');
    expect(blockAria(tape, ctx)).toBe('Saturday Night Kettle, pre-recorded, 20:00 to 23:00, S3 Ep 7 · Ghost Train night');
    const drive = blocks.find((b) => b.id === 'ep:kettle-drive-99')!;
    expect(blockHtml(drive, ctx)).toContain('Conflict');
    expect(blockClasses(drive, ctx)).toContain('conflict');
    expect(blockHtml(blocks.find((b) => b.id === 'ep:kettle-breakfast-212')!, ctx)).toContain('Moved · was Mon 07:00');
    const onAir = auto.find((b) => b.start <= NOW && b.end > NOW)!;
    expect(blockHtml(onAir, ctx)).toContain('On air');
    expect(blockHtml(onAir, ctx)).toContain('Automation');
    expect(blockClasses(onAir, ctx)).toEqual(['ev', 'auto']);
    // Everything typed is escaped.
    const rude: Block = { ...snk, show: '<b>x</b>' };
    expect(blockHtml(rude, ctx)).toContain('&lt;b&gt;x&lt;/b&gt;');
  });

  // Studios as columns (1.3.0; rehearsal-mode-build-plan.md section 2.2): the station says
  // which column an episode is in and where its host comes from.
  it('takes an episode’s studio and where its host comes from as the station sends them', () => {
    const own: WeekEpisode = { ...episode(DRIVE, 99, at(22, 16)), studio: 'studio', from: 'own' };
    const room: WeekEpisode = { ...episode(SNK, 7, at(24, 20)), studio: 'studio', from: 'room' };
    const [a, b] = episodeBlocks([own, room], ZONE, NOW);
    expect([a.studio, a.from, b.studio, b.from]).toEqual(['studio', 'own', 'studio', 'room']);
    // A station older than 1.3.0 sends neither: it has the one studio, and its hosts are in the room.
    const [old] = episodeBlocks([episode(SNK, 7, at(24, 20))], ZONE, NOW);
    expect([old.studio, old.from]).toEqual(['studio', 'room']);
    // A dotted date and automation are on air too, so they are in the same column.
    const { rules, auto } = week([date(SNK, '2026-10-24', 20)]);
    expect([...rules, ...auto].every((x) => x.studio === 'studio')).toBe(true);
  });

  it('says Own studio on the block in the Studios view only: the week is as it was', () => {
    const [own] = episodeBlocks([{ ...episode(SNK, 7, at(24, 20)), studio: 'studio', from: 'own' }], ZONE, NOW);
    const ctx = { now: NOW, zone: ZONE, selected: null };
    // The week never printed a studio, and still does not: every show there is in the one.
    expect(blockHtml(own, ctx)).not.toMatch(/studio/i);
    expect(blockAria(own, ctx)).toBe('Saturday Night Kettle, live, 20:00 to 23:00, S3 Ep 7');
    const cols = { ...ctx, columns: true, wide: true };
    expect(blockHtml(own, cols)).toContain('<use href="#g-disc"/></svg>Own studio</span>');
    expect(blockAria(own, cols)).toBe('Saturday Night Kettle, live, 20:00 to 23:00, S3 Ep 7, the host in their own studio');
    // A host in the room carries no mark: it is the usual case.
    const [room] = episodeBlocks([{ ...episode(SNK, 7, at(24, 20)), studio: 'studio', from: 'room' }], ZONE, NOW);
    expect(blockHtml(room, cols)).not.toContain('Own studio');
    expect(blockAria(room, cols)).not.toContain('own studio');
  });
});

describe('the editor', () => {
  it('marks a field inherited, set here or as aired (ADR-0055)', () => {
    const r = { inherited: ['hosts', 'tags'], frozen: false };
    expect(marker(r, 'hosts')).toBe('inherited');
    expect(marker(r, 'title')).toBe('set');
    expect(marker(r, 'guests', false)).toBe('own');
    expect(marker({ inherited: [], frozen: true }, 'hosts')).toBe('frozen');
  });

  it('builds the card fields from a resolved episode, cut to the level', () => {
    const w = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night', guests: ['Firth Lights', 'Dr Una Robertson'] });
    const f = cardFields(w.resolved, 'episode', ZONE);
    expect(f.layout).toBe('poster');
    expect(f.show.slot).toEqual({ weekday: 'saturday', start: '20:00', end: '23:00' });
    expect(f.series?.number).toBe(3);
    expect(f.episode).toEqual({ number: 7, title: 'Ghost Train night', air: '2026-10-24T20:00:00+01:00[Europe/London]' });
    expect(f.guests).toEqual(['Firth Lights', 'Dr Una Robertson']);
    const s = cardFields(w.resolved, 'show', ZONE);
    expect(s.series).toBeUndefined();
    expect(s.guests).toEqual([]);
    expect(slugOf('Saturday Night Kettle')).toBe('saturday-night-kettle');
    expect(slugOf('Rùnach & co.')).toBe('runach-co');
  });

  it('renders the episode editor with its markers, the path and the foot', () => {
    const w = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night', guests: ['Firth Lights', 'Dr Una Robertson'] });
    const { body } = render(EpisodeEditor, { props: { item: w, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } });
    expect(body).toContain('Saturday Night Kettle');
    expect(body).toContain('Episode 7 · Ghost Train night');
    expect(body).toContain('Sat 24 Oct 2026 · 20:00–23:00');
    expect(body).toContain('Tonight');
    expect(body).toContain('from Series 3');
    expect(body).toContain('Set here');
    expect(body).toContain('class="field inh"');
    expect(body).toContain('class="field set"');
    expect(body).toContain('Firth Lights');
    expect(body).toContain('>Create next episode');
    expect(body).toContain('>Draft the season');
    // A station that stores no rules writes no words for one: the weekly time, in short.
    expect(body).toContain('Sat, 20:00–23:00, Europe/London');
    // The date field is not drafted: its Move… is saved as it is pressed, and says so.
    expect(body).toContain('Sat, 20:00–23:00, Europe/London. A move is saved as you make it.');
    expect(body).toContain('>Move…</button>');
    expect(body).not.toContain('(unsaved)');
    expect(body).not.toContain('as aired');
  });

  it('renders the Kind field as a real choice, and the draft keeps it only while it differs', () => {
    const live = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night' });
    let { body } = render(EpisodeEditor, { props: { item: live, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } });
    expect(body).toContain('aria-label="Kind"');
    expect(body).toMatch(/aria-pressed="true"[^>]*>Live</);
    expect(body).toMatch(/aria-pressed="false"[^>]*>Pre-recorded</);
    expect(body).not.toContain('not in the model yet');
    const rec = episode(SNK, 8, at(31, 20), { kind: 'prerecorded' });
    ({ body } = render(EpisodeEditor, { props: { item: rec, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } }));
    expect(body).toMatch(/aria-pressed="true"[^>]*>Pre-recorded</);
    expect(body).toMatch(/aria-pressed="false"[^>]*>Live</);
  });

  it('words a correction-log line by what it changed', () => {
    expect(
      changeWords({
        old_values: { title: 'The ferrys', guests: ['Morag Ross', 'Gregor Stuart'], description: '' },
        new_values: { title: 'The ferries', guests: ['Morag Ross', 'Gregor Stewart'], description: 'The island ferries.' },
      }),
    ).toEqual([
      'title: The ferrys → The ferries',
      'guests: Morag Ross, Gregor Stuart → Morag Ross, Gregor Stewart',
      'description: empty → The island ferries.',
    ]);
    expect(changeWords({ old_values: { guests: [] }, new_values: { guests: ['Firth Lights'] } })).toEqual(['guests: none → Firth Lights']);
  });

  it('offers a correction on an aired episode, and none on one yet to air', () => {
    const aired = episode(SNK, 6, at(17, 20), { title: 'The harbour tapes', aired: true });
    let { body } = render(EpisodeEditor, { props: { item: aired, zone: ZONE, now: NOW, us: null, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } });
    expect(body).toContain('Correct the public text…');
    expect(body).toContain('data-field="corrections"');
    expect(body).not.toContain('Publish correction');
    const upcoming = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night' });
    ({ body } = render(EpisodeEditor, { props: { item: upcoming, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } }));
    expect(body).not.toContain('Correct the public text');
  });

  it('renders an aired episode frozen and read-only', () => {
    const w = episode(SNK, 6, at(17, 20), { title: 'The harbour tapes', aired: true });
    const { body } = render(EpisodeEditor, { props: { item: w, zone: ZONE, now: NOW, us: null, onChanged: async () => {}, onMove: async () => null, onCard: () => {} } });
    expect(body).toContain('frozen');
    expect(body).toContain('as aired');
    expect(body).toContain('class="field frozen"');
    expect(body).not.toContain('Save episode');
    expect(body).not.toContain('<input');
    // An aired episode is not moved: no Move…, and nothing said of one.
    expect(body).not.toContain('Move…');
    expect(body).not.toContain('A move is saved');
    expect(body).toContain('Open the recording');
  });

  it('renders the show editor with the twelve colours, both layouts and when it airs', () => {
    const { body } = render(ShowEditor, { props: { slug: SNK.slug, show: SNK, onSaved: () => {}, onCard: () => {} } });
    expect(body).toContain('Save show');
    expect((body.match(/aria-label="Bank \d+"/g) ?? []).length).toBe(12);
    expect(body).toContain('>Poster<');
    expect(body).toContain('>Slate<');
    expect(body).toContain('refused with the field named');
    // A show is made in the New show dialog, so the editor asks for no path.
    expect(body).not.toContain('aria-label="Path"');
    expect(body).not.toContain('Weekly rule');
    expect(body).toContain('data-panel="calendar.show-rule"');
    expect(body).toContain('When it airs');
  });

  it("renders an existing show's slate picture as its own panel (ADR-0200), and only then", () => {
    const show = { slug: 'the-long-lunch', name: 'The Long Lunch', description: '', colour_slot: 2, layout: 'slate', slot: null, tags: [], explicit: false } as never;
    const { body } = render(ShowEditor, { props: { slug: 'the-long-lunch', show, onSaved: () => {}, onCard: () => {}, embedded: true } });
    expect(body).toContain('data-panel="calendar.show-still"');
    expect(body).toContain('Slate picture');
    expect(CALENDAR.map((p) => p.name)).toContain('calendar.show-still');
  });

  it('renders the log editor for an automation block', () => {
    const { auto } = week();
    const b = auto.find((x) => x.show === 'Overnight playlist')!;
    const { body } = render(LogEditor, { props: { block: b, zone: ZONE, now: NOW } });
    expect(body).toContain('Overnight playlist · 6 h');
    expect(body).toContain('the US complement rule is never relaxed');
  });

  it('draws the tab under its registered panel names, and only registered names', () => {
    sched.zone = ZONE;
    sched.anchor = NOW;
    const { body } = render(Schedule, { props: { live: false } });
    for (const name of ['calendar.toolbar', 'calendar.week', 'calendar.episode-editor']) {
      expect(body.includes(`data-panel="${name}"`), name).toBe(true);
    }
    for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
    expect(CALENDAR.map((p) => p.name)).toContain('calendar.log-editor');
    expect(CALENDAR.map((p) => p.name)).toContain('calendar.card-editor');
    expect(body).toContain('19 – 25 October 2026');
  });
});

describe('the keymap on the schedule tab', () => {
  const key = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({ code, ctrl: false, shift: false, alt: false, meta: false, repeat: false, ...mods });
  const schedule = { tab: 'schedule' as const, inTextField: false };

  it('moves a week with the arrows and goes to today with T, on this tab only', () => {
    const k = new KeyEngine();
    expect(k.down(key('ArrowLeft'), schedule, 0).action).toEqual({ type: 'week', delta: -1 });
    expect(k.down(key('ArrowRight'), schedule, 0).action).toEqual({ type: 'week', delta: 1 });
    expect(k.down(key('KeyT'), schedule, 0).action).toEqual({ type: 'today' });
    expect(k.down(key('KeyD'), schedule, 0).action).toEqual({ type: 'view', view: 'day' });
    expect(k.down(key('KeyW'), schedule, 0).action).toEqual({ type: 'view', view: 'week' });
    expect(k.down(key('KeyD'), { tab: 'schedule', inTextField: true }, 0).action).toBeNull();
    expect(k.down(key('KeyD', { shift: true }), schedule, 0).action).toBeNull();
    expect(k.down(key('KeyT'), { tab: 'desk', inTextField: false }, 0).action).toEqual({ type: 'cue', slot: 5 });
    expect(k.down(key('ArrowLeft'), { tab: 'desk', inTextField: false }, 0).action).toBeNull();
  });

  it('Esc is the global layer’s escape here too: it leaves a text field first, and a second press is the tab’s', () => {
    const k = new KeyEngine();
    expect(k.down(key('Escape'), { ...schedule, inTextField: true }, 0).action).toEqual({ type: 'leave-field' });
    expect(k.down(key('Escape'), schedule, 0)).toEqual({ action: { type: 'escape' }, handled: true });
  });

  it('keeps the global layer and leaves text fields alone', () => {
    const k = new KeyEngine();
    expect(k.down(key('Digit1', { ctrl: true }), schedule, 0).action).toEqual({ type: 'tab', tab: 'desk' });
    expect(k.down(key('F2'), schedule, 0).action).toEqual({ type: 'cart', n: 2 });
    expect(k.down(key('KeyT', { ctrl: true }), schedule, 0).action).toEqual({ type: 'talk-seat', held: true });
    expect(k.down(key('KeyT'), { ...schedule, inTextField: true }, 0).handled).toBe(false);
    expect(k.down(key('ArrowLeft'), { ...schedule, inTextField: true }, 0).handled).toBe(false);
    expect(k.down(key('KeyT', { shift: true }), schedule, 0).handled).toBe(false);
  });
});

// Esc on the Schedule tab, and a press on the grid where no block is: both close what the editor
// column holds and clear the selection, as ‹ Shows does (the operator's 1.0.5 run: once a block
// or a show was open, the only ways out were another block, or D and W).
describe('Esc on the schedule tab', () => {
  const week = weekOf(NOW, ZONE).days;
  /** With no episodes and no clocks, automation fills each day as one block. */
  const monday = `auto:0:${week[0]}`;
  const lunch = show('the-long-lunch', 'The Long Lunch', 2, null);
  const reset = () => {
    Object.assign(sched, { zone: ZONE, anchor: NOW, view: 'week', selected: null, pane: 'episode', editShow: null, showTab: 'dates', episodes: [], dates: null, hours: [], clocks: [], defaultClock: null, shows: [lunch], notClosed: null });
    Object.assign(studio, { tab: 'schedule', now: NOW, skew: 0, note: null, offNote: null, view: null });
  };

  it('with nothing open it does nothing', () => {
    reset();
    expect(closeOpen(NOW)).toEqual({ did: 'nothing' });
  });

  it('clears a selected block, and the column is back on the list of shows', () => {
    reset();
    select(monday);
    expect([sched.selected, sched.pane]).toEqual([monday, 'log']);
    expect(closeOpen(NOW)).toEqual({ did: 'closed', block: monday, show: null });
    expect([sched.selected, sched.pane, sched.editShow]).toEqual([null, 'episode', null]);
    // The card editor of a selected block closes with it, and the next block opens as itself.
    select(monday);
    sched.pane = 'card';
    expect(closeOpen(NOW).did).toBe('closed');
    expect([sched.selected, sched.pane]).toEqual([null, 'episode']);
  });

  it('closes a show’s own pane, and says which show for the focus to go back to its row', () => {
    reset();
    openShow('the-long-lunch', 'details');
    expect(closeOpen(NOW)).toEqual({ did: 'closed', block: null, show: 'the-long-lunch' });
    expect([sched.pane, sched.editShow]).toEqual(['episode', null]);
  });

  it('leaves a selection the grid does not draw: another day in the Day view, another week', () => {
    reset();
    select(monday);
    openDay(week[2] + 12 * 3_600_000);
    expect(closeOpen(NOW)).toEqual({ did: 'nothing' });
    expect(sched.selected).toBe(monday);
    openDay(week[0] + 12 * 3_600_000);
    expect(closeOpen(NOW).did).toBe('closed');
    reset();
    sched.selected = 'ep:in-another-week';
    expect(closeOpen(NOW)).toEqual({ did: 'nothing' });
  });

  it('is never destructive: over changes that are not saved nothing closes, and the tab is told why', () => {
    reset();
    select(monday);
    const editor = {};
    noteUnsaved(editor, 'The episode');
    expect(closeOpen(NOW)).toEqual({ did: 'unsaved', what: 'The episode' });
    expect(sched.selected).toBe(monday);
    expect(sched.notClosed?.what).toBe('The episode');
    const first = sched.notClosed?.n;
    closeOpen(NOW);
    expect(sched.notClosed?.n).not.toBe(first);
    // Saved or discarded, the editor says so, and Esc closes it.
    noteUnsaved(editor, null);
    expect(closeOpen(NOW).did).toBe('closed');
    expect(sched.selected).toBeNull();
  });

  it('the key: a note in the toast slot goes first, then the selection, and on another tab the selection is left', async () => {
    reset();
    select(monday);
    studio.note = { id: 1, tone: 'good', title: 'A note', message: 'of this window', until: NOW + 6000 };
    await run({ type: 'escape' });
    expect([studio.note, sched.selected]).toEqual([null, monday]);
    await run({ type: 'escape' });
    expect([sched.selected, sched.pane]).toEqual([null, 'episode']);
    // With nothing open Esc goes on to what it does on every tab, and changes nothing here.
    await run({ type: 'escape' });
    expect(sched.selected).toBeNull();
    select(monday);
    studio.tab = 'desk';
    await run({ type: 'escape' });
    expect(sched.selected).toBe(monday);
    studio.tab = 'schedule';
    noteUnsaved(sched, 'The show');
    await run({ type: 'escape' });
    expect([sched.selected, sched.notClosed?.what]).toEqual([monday, 'The show']);
    noteUnsaved(sched, null);
    reset();
  });
});
