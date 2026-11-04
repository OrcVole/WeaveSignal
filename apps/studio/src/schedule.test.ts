// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's rules, tested without a page: the station's wall clock in its zone (a
// clocks-back Sunday has 25 hours), the week's blocks (episodes, a moved instance, a conflict,
// a weekly rule not yet drafted, automation named by the clock of each local hour), the
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
  nextInstance,
  parts,
  rangeWords,
  ruleBlocks,
  slotWords,
  slugOf,
  weekOf,
  zonedString,
  type Block,
} from './schedule/model.ts';
import type { Clock, Show, WeekEpisode, WeekHour } from './lib/schedule.ts';
import { sched } from './lib/schedule.svelte.ts';

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

  it('finds the next instance of a weekly rule across the clock change', () => {
    const sat = { weekday: 6, start: 20 * 60, minutes: 180 };
    const next = nextInstance(sat, NOW, ZONE);
    expect(hm(next, ZONE)).toBe('20:00');
    expect(parts(next, ZONE).d).toBe(24);
    const after = nextInstance(sat, next, ZONE);
    expect(parts(after, ZONE).d).toBe(31);
    expect(after - next).toBe(7 * 24 * 3_600_000 + 3_600_000);
    expect(slotWords(sat)).toBe('Weekly on Saturday, 20:00–23:00');
    expect(zonedString(next, ZONE)).toBe('2026-10-24T20:00:00+01:00[Europe/London]');
    expect(zonedString(after, ZONE)).toBe('2026-10-31T20:00:00+00:00[Europe/London]');
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

function week() {
  const eps = [
    episode(SNK, 7, at(24, 20), { title: 'Ghost Train night', guests: ['Firth Lights'] }),
    episode(SNK, 6, at(17, 20), { title: 'The harbour tapes', aired: true }),
    episode(DRIVE, 99, at(22, 16)),
    episode(SPECIAL, 1, at(22, 18), { minutes: 90, title: 'The count' }),
    episode(BREAKFAST, 212, at(23, 8), { minutes: 120 }),
  ];
  const { days } = weekOf(NOW, ZONE);
  const blocks = episodeBlocks(eps, ZONE, NOW);
  const rules = ruleBlocks([SNK, DRIVE, SPECIAL, BREAKFAST], blocks, days, ZONE, NOW);
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

  it('draws a weekly rule where no episode is drafted, and never over a drafted one', () => {
    const { rules } = week();
    // Kettle Drive has its Thursday; Kettle Breakfast's Monday instance was moved, so Monday is free
    // but in the past; Saturday Night Kettle has tonight. The special has no rule.
    expect(rules.map((r) => r.slug)).toEqual([]);
    const { days } = weekOf(NOW + 7 * 24 * 3_600_000, ZONE);
    const next = ruleBlocks([SNK, DRIVE], [], days, ZONE, NOW);
    expect(next.map((r) => [r.slug, hm(r.start, ZONE)])).toEqual([
      ['saturday-night', '20:00'],
      ['kettle-drive', '16:00'],
    ]);
    expect(next[0].kind).toBe('rule');
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
    const { body } = render(EpisodeEditor, { props: { item: w, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onCard: () => {} } });
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
    expect(body).toContain('Weekly on Saturday, 20:00–23:00, Europe/London');
    expect(body).not.toContain('as aired');
  });

  it('renders the Kind field as a real choice, and the draft keeps it only while it differs', () => {
    const live = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night' });
    let { body } = render(EpisodeEditor, { props: { item: live, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onCard: () => {} } });
    expect(body).toContain('aria-label="Kind"');
    expect(body).toMatch(/aria-pressed="true"[^>]*>Live</);
    expect(body).toMatch(/aria-pressed="false"[^>]*>Pre-recorded</);
    expect(body).not.toContain('not in the model yet');
    const rec = episode(SNK, 8, at(31, 20), { kind: 'prerecorded' });
    ({ body } = render(EpisodeEditor, { props: { item: rec, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onCard: () => {} } }));
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
    let { body } = render(EpisodeEditor, { props: { item: aired, zone: ZONE, now: NOW, us: null, onChanged: async () => {}, onCard: () => {} } });
    expect(body).toContain('Correct the public text…');
    expect(body).toContain('data-field="corrections"');
    expect(body).not.toContain('Publish correction');
    const upcoming = episode(SNK, 7, at(24, 20), { title: 'Ghost Train night' });
    ({ body } = render(EpisodeEditor, { props: { item: upcoming, zone: ZONE, now: NOW, us: false, onChanged: async () => {}, onCard: () => {} } }));
    expect(body).not.toContain('Correct the public text');
  });

  it('renders an aired episode frozen and read-only', () => {
    const w = episode(SNK, 6, at(17, 20), { title: 'The harbour tapes', aired: true });
    const { body } = render(EpisodeEditor, { props: { item: w, zone: ZONE, now: NOW, us: null, onChanged: async () => {}, onCard: () => {} } });
    expect(body).toContain('frozen');
    expect(body).toContain('as aired');
    expect(body).toContain('class="field frozen"');
    expect(body).not.toContain('Save episode');
    expect(body).not.toContain('<input');
    expect(body).toContain('Open the recording');
  });

  it('renders the show editor for a new show with the twelve colours and both layouts', () => {
    const { body } = render(ShowEditor, { props: { slug: null, show: null, onSaved: () => {}, onCard: () => {} } });
    expect(body).toContain('Make the show');
    expect((body.match(/aria-label="Bank \d+"/g) ?? []).length).toBe(12);
    expect(body).toContain('>Poster<');
    expect(body).toContain('>Slate<');
    expect(body).toContain('refused with the field named');
    expect(body).not.toContain('calendar.show-still');
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
    expect(k.down(key('KeyT'), { tab: 'desk', inTextField: false }, 0).action).toEqual({ type: 'cue', slot: 5 });
    expect(k.down(key('ArrowLeft'), { tab: 'desk', inTextField: false }, 0).action).toBeNull();
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
