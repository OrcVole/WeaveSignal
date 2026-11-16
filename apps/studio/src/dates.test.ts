// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's dates as a list (`calendar.show-dates`) and the list of shows (`calendar.shows`),
// tested without a page: the station's preview joined to the series' episodes as rows, what
// the head counts, what follows the rows, the small edits of a rule each change makes (read
// again before every save), and the shows in name order.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { CALENDAR, PANELS } from '../../../packages/ui/panels.ts';
import type { PreviewDate, Rule, RulePreview } from './lib/api.ts';
import type { EpisodeView, Show } from './lib/schedule.ts';
import ClashBox from './schedule/ClashBox.svelte';
import ShowList from './schedule/ShowList.svelte';
import { airedRows, changeRule, countWords, dateRows, draftedToCome, firstRows, hasMore, moreWords, moveIn, putBackIn, rowAria, skipIn, unmoveIn, type RuleStore } from './schedule/dates.ts';
import { instant, markConflicts, type Block } from './schedule/model.ts';
import { EMPTY_PREVIEW } from './schedule/rule/previewer.ts';
import { unscheduled } from './schedule/rule/view.ts';
import { listRows, showCounts } from './schedule/shows.ts';

const ZONE = 'Europe/London';
const at = (m: number, d: number, hh: number, mm = 0, y = 2026) => instant({ y, m, d, hh, mm }, ZONE);
/** Friday 13 November 2026, 10:00. */
const NOW = at(11, 13, 10);
const TODAY = '2026-11-13';

const RULE: Rule = { starts_on: '2026-11-04', start: 1260, minutes: 120, repeat: { kind: 'weekly', every: 1, weekdays: [3, 5] }, end: { kind: 'until', until: '2027-02-26' }, skip: ['2026-12-09'], moves: [{ from: '2026-12-04', to: '2026-12-05T17:00:00', minutes: null }] };

const date = (on: string, number: number | null, start: number, o: Partial<PreviewDate> = {}): PreviewDate => ({ on, number, start: start / 1000, minutes: 120, state: 'airs', now: false, moved_from: null, clock: null, episode: null, clashes: [], ...o });
const episode = (number: number, on: string | null, start: number, o: { aired?: boolean; title?: string } = {}): EpisodeView =>
  ({
    episode: { id: `bothy-${number}`, series_id: 's1', number, title: o.title ?? '', description: '', kind: 'live', air_at: start / 1000, minutes: null, hosts: null, guests: [], tags: null, aired_at: o.aired ? start / 1000 + 7200 : null, occurrence: on, frozen: null, files: null, cards: null },
    resolved: { show: 'The Bothy Sessions', slug: 'the-bothy-sessions', description: '', colour_slot: 12, layout: 'poster', slot: null, rule: RULE, hosts: [], guests: [], tags: [], explicit: false, series: { number: 1, title: null, first: null, last: null, episodes: null }, episode: { id: `bothy-${number}`, number, title: o.title ?? '', description: '', kind: 'live', air_at: start / 1000, minutes: null }, inherited: [], frozen: !!o.aired },
  }) as EpisodeView;

const EPISODES = [
  episode(1, '2026-11-04', at(11, 4, 21), { aired: true }),
  episode(2, '2026-11-06', at(11, 6, 21), { aired: true }),
  episode(3, '2026-11-13', at(11, 13, 21)),
  episode(4, '2026-11-18', at(11, 18, 20, 30), { title: 'Fiddle night' }),
  episode(9, null, at(11, 29, 14)),
];
const PREVIEW: RulePreview = {
  ...EMPTY_PREVIEW,
  sentence: 'Every week on Wednesday and Friday, 21:00 to 23:00, until 26 February 2027. Not on 9 December.',
  last: 'The last is Friday 26 February 2027.',
  total: 30,
  dates: [
    date('2026-11-13', 3, at(11, 13, 21), { state: 'filled', episode: { series: 1, number: 3, aired: false } }),
    date('2026-11-18', 4, at(11, 18, 21), { state: 'filled', episode: { series: 1, number: 4, aired: false } }),
    date('2026-11-20', 5, at(11, 20, 21)),
    date('2026-11-27', 6, at(11, 27, 21), { clashes: [{ show: 'St Andrew’s Weekend', slug: 'st-andrews-weekend', start: at(11, 27, 22) / 1000, minutes: 90, overlap_minutes: 60, when: null, with: [], refused: false }] }),
    date('2026-12-04', 7, at(12, 5, 17), { moved_from: '2026-12-04T21:00:00' }),
    date('2026-12-09', null, at(12, 9, 21), { state: 'skipped' }),
    date('2027-01-01', 8, at(1, 1, 21, 0, 2027)),
  ],
};
const ctx = { slug: 'the-bothy-sessions', zone: ZONE, today: TODAY, now: NOW, nominal: 1260, blocks: [] as Block[] };

describe('a show’s dates as rows', () => {
  const rows = dateRows(PREVIEW, EPISODES, ctx);

  it('joins the station’s dates to the series’ episodes, in order, with an episode made by hand in its place', () => {
    expect(rows.map((r) => `${r.key}:${r.kind}:${r.ep}:${r.day}:${r.times}`)).toEqual([
      '2026-11-13:drafted:Ep 3:Fri 13 Nov:21:00–23:00',
      '2026-11-18:drafted:Ep 4:Wed 18 Nov:20:30–22:30',
      '2026-11-20:open:Ep 5:Fri 20 Nov:21:00–23:00',
      '2026-11-27:open:Ep 6:Fri 27 Nov:21:00–23:00',
      'ep:bothy-9:hand:Ep 9:Sun 29 Nov:14:00–16:00',
      '2026-12-04:open:Ep 7:Sat 5 Dec:17:00–19:00',
      '2026-12-09:skipped::Wed 9 Dec:21:00–23:00',
      '2027-01-01:open:Ep 8:Fri 1 Jan 2027:21:00–23:00',
    ]);
  });

  it('says what each is: a title, a move, a clash, the block that stands for it, and which date Draft is on', () => {
    const by = (k: string) => rows.find((r) => r.key === k)!;
    expect(by('2026-11-18')).toMatchObject({ title: 'Fiddle night', moved: 'was 21:00', blockId: 'ep:bothy-4', first: false });
    expect(by('2026-11-20')).toMatchObject({ first: true, blockId: 'rule:the-bothy-sessions:2026-11-20', clash: null });
    expect(by('2026-11-27')).toMatchObject({ first: false, clash: 'St Andrew’s Weekend, 22:00 to 23:30 · overlaps 1 h', refused: null });
    expect(by('2026-12-04')).toMatchObject({ moved: 'was Fri 4 Dec', air: '2026-12-05', at: '17:00' });
    expect(by('2026-12-09')).toMatchObject({ kind: 'skipped', number: null, blockId: null });
    expect(by('ep:bothy-9')).toMatchObject({ on: null, moved: null });
    expect(rowAria(by('2026-11-20'))).toBe('Episode 5, Friday 20 November, 21:00 to 23:00, not drafted. Space shows it in the week.');
    expect(rowAria(by('2026-12-09'))).toBe('Wednesday 9 December, 21:00 to 23:00, skipped.');
  });

  it('draws what the station refuses this session, in its words', () => {
    const refused = { ...PREVIEW, dates: PREVIEW.dates.map((d) => (d.on === '2026-11-27' ? { ...d, clashes: d.clashes.map((c) => ({ ...c, refused: true })) } : d)) };
    expect(dateRows(refused, EPISODES, ctx).find((r) => r.key === '2026-11-27')!.refused).toBe('St Andrew’s Weekend has that time.');
  });

  it('knows a drafted episode’s clash only while its block is in the week on screen', () => {
    const mine: Block = { id: 'ep:bothy-4', kind: 'episode', start: at(11, 18, 20, 30), end: at(11, 18, 22, 30), show: 'The Bothy Sessions', slug: 'the-bothy-sessions', colour: 12, series: 1, number: 4, title: '', hosts: [], weekly: true, aired: false, frozen: false, conflict: false, moved: null, clock: null, studio: null };
    const other: Block = { ...mine, id: 'rule:late:2026-11-18', kind: 'rule', show: 'The Late Shift', slug: 'late', start: at(11, 18, 22), end: at(11, 19, 0) };
    const blocks = markConflicts([mine, other]);
    expect(dateRows(PREVIEW, EPISODES, { ...ctx, blocks }).find((r) => r.key === '2026-11-18')!.clash).toBe('The Late Shift, 22:00 to 00:00 · overlaps 30 min');
    expect(rows.find((r) => r.key === '2026-11-18')!.clash).toBeNull();
  });

  it('lists what has aired, newest first, and counts what is drafted to come', () => {
    expect(airedRows(EPISODES, { number: 1 }, ZONE).map((a) => `${a.ep} ${a.day} ${a.times}`)).toEqual(['Ep 2 Fri 6 Nov 2026 21:00–23:00', 'Ep 1 Wed 4 Nov 2026 21:00–23:00']);
    expect(draftedToCome(EPISODES, NOW)).toBe(2);
  });

  it('counts in the head only what the station sent, and says what follows the rows shown', () => {
    // The station sent every date the rule still gives (`more` is false): they can be counted.
    const whole = { ...PREVIEW, total: null, more: false };
    expect(countWords(rows, whole, RULE, TODAY, 2)).toEqual({ lead: '6 to come:', rest: '2 drafted, 4 not drafted. 1 made by hand. 1 skipped. 1 clashes with another show.' });
    expect(firstRows(rows, 3).map((r) => r.key)).toEqual(['2026-11-13', '2026-11-18', '2026-11-20']);
    expect(moreWords(rows, 3, whole, RULE, PREVIEW.last)).toEqual({ more: true, words: 'and 4 more, to Friday 26 February 2027' });
    expect(moreWords(rows, 10, whole, RULE, PREVIEW.last)).toEqual({ more: false, words: 'Those are all 7.' });
    // It sent sixty and says there are more: nothing is counted that was not sent.
    const part = { ...PREVIEW, total: null, more: true };
    expect(hasMore(part, RULE)).toBe(true);
    expect(countWords(rows, part, RULE, TODAY, 2)).toEqual({ lead: '2 drafted', rest: 'of the next 6; more follow. 1 made by hand. 1 skipped. 1 of the next 6 clashes with another show.' });
    expect(moreWords(rows, 3, part, RULE, PREVIEW.last)).toEqual({ more: true, words: 'and 4 or more, to Friday 26 February 2027' });
    expect(moreWords(rows, 10, part, RULE, PREVIEW.last)).toEqual({ more: false, words: 'and more after, to Friday 26 February 2027' });
    // A rule with no end goes on.
    const endless = { ...RULE, end: { kind: 'never' as const } };
    expect(countWords(rows, part, endless, TODAY, 2)).toEqual({ lead: '2 drafted', rest: 'of the next 6; the dates go on with no end. 1 made by hand. 1 skipped. 1 of the next 6 clashes with another show.' });
    expect(moreWords(rows, 3, part, endless)).toEqual({ more: true, words: 'and more after, with no end' });
    // A reply that says neither is read by its count, or by the rule's end.
    expect(hasMore(PREVIEW, RULE)).toBe(true);
    expect(hasMore({ ...PREVIEW, total: 6 }, RULE)).toBe(false);
    expect(hasMore({ ...PREVIEW, total: null }, RULE)).toBe(false);
    expect(hasMore({ ...PREVIEW, total: null }, endless)).toBe(true);
  });

  it('with no rule lists the episodes made by hand alone', () => {
    expect(dateRows(null, EPISODES, ctx).map((r) => `${r.kind}:${r.ep}`)).toEqual(['hand:Ep 3', 'hand:Ep 4', 'hand:Ep 9']);
  });
});

describe('the changes one date makes to a rule', () => {
  it('skips, puts back, moves and unmoves, leaving the rest of the rule as it was', () => {
    expect(skipIn(RULE, '2026-12-04')).toMatchObject({ skip: ['2026-12-04', '2026-12-09'], moves: [] });
    expect(putBackIn(RULE, '2026-12-09').skip).toEqual([]);
    expect(moveIn(RULE, '2026-11-25', '2026-11-26', '20:00').moves).toEqual([{ from: '2026-11-25', to: '2026-11-26T20:00:00', minutes: null }, RULE.moves[0]]);
    expect(moveIn(RULE, '2026-12-09', '2026-12-10', '20:00').skip).toEqual([]);
    expect(unmoveIn(RULE, '2026-12-04').moves).toEqual([]);
    expect(skipIn(RULE, '2026-12-04').repeat).toEqual(RULE.repeat);
  });

  it('reads the rule from the station before every save, and saves it where it was read from', async () => {
    const calls: string[] = [];
    let held = RULE;
    const store: RuleStore = {
      read: async (slug, series) => {
        calls.push(`read ${slug} ${series}`);
        return { rule: held, series: null };
      },
      write: async (slug, series, rule) => {
        calls.push(`write ${slug} ${series}`);
        held = rule;
      },
    };
    // Another window skipped a date in between: this change is made to the rule as it is now.
    held = skipIn(held, '2026-11-25');
    expect(await changeRule(store, 'the-bothy-sessions', 1, (r) => skipIn(r, '2026-12-02'))).toBe(true);
    expect(held.skip).toEqual(['2026-11-25', '2026-12-02', '2026-12-09']);
    expect(calls).toEqual(['read the-bothy-sessions 1', 'write the-bothy-sessions null']);
    expect(await changeRule({ ...store, read: async () => null }, 'x', null, (r) => r)).toBe(false);
  });
});

describe('the list of shows', () => {
  const show = (name: string, o: Partial<Show> = {}): Show => ({ id: name, slug: name.toLowerCase().replace(/[^a-z]+/g, '-'), name, description: '', colour_slot: 3, layout: 'poster', slot: null, rule: RULE, rule_words: { sentence: '', short: 'Wed and Fri, 21:00–23:00', last: null, last_on: null, notes: [] }, next_on: { on: '2026-11-18', start: 1, minutes: 120, reason: null, last_on: null }, hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, presenter: '', contact: '', links: [], commercial_music: false, presenter_account: null, ...o });
  const shows = [show('Kettle Drive'), show('The Bothy Sessions', { next_on: { on: TODAY, start: 1, minutes: 120, reason: null, last_on: null } }), show('Northern Lights', { rule: null, rule_words: null, next_on: { on: null, start: null, minutes: null, reason: 'none', last_on: null } }), show('Budget Night Special')];

  it('lists the shows with a date to come by name, a leading The set aside, and counts the rest', () => {
    expect(listRows(shows, true).map((s) => s.name)).toEqual(['The Bothy Sessions', 'Budget Night Special', 'Kettle Drive']);
    expect(showCounts(3, unscheduled(shows, true))).toBe('3 with dates to come · 1 with none yet');
    // An older station says no next date: every show is in a group below instead.
    expect(listRows(shows, false)).toEqual([]);
  });

  it('draws a row for each with its rule in short and its next date, and the shows with nothing coming up at its foot', () => {
    const { body } = render(ShowList, { props: { shows, today: TODAY, onopen: () => {}, ondates: () => {}, ondraft: () => {} } });
    const text = body.replace(/<!--[^>]*-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ');
    expect(text).toContain('Shows 3 with dates to come · 1 with none yet');
    expect(text).toContain('The Bothy Sessions Wed and Fri, 21:00–23:00 · next today');
    expect(text).toContain('Kettle Drive Wed and Fri, 21:00–23:00 · next Wed 18 Nov');
    expect(body).toContain('data-panel="calendar.unscheduled"');
    expect(text).toContain('Northern Lights');
    // Twelve or fewer need no field to find one.
    expect(body).not.toContain('Find a show');
    // One stop for Tab: the first row.
    expect(body.match(/class="showrow"[^>]*tabindex="0"/g)).toHaveLength(1);
  });
});

describe('the clash in the editor', () => {
  const b = (id: string, show: string, kind: 'episode' | 'rule', start: number, end: number, number = 1): Block => ({ id, kind, start, end, show, slug: id, colour: 4, series: 1, number, title: '', hosts: [], weekly: true, aired: false, frozen: false, conflict: false, moved: null, clock: null, studio: null });
  const text = (block: Block, blocks: Block[]) => render(ClashBox, { props: { block, blocks, zone: ZONE, onshow: () => {}, onday: () => {} } }).body.replace(/<!--[^>]*-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
  const drive = b('ep:drive', 'Kettle Drive', 'episode', at(11, 19, 16), at(11, 19, 19), 214);
  const budget = b('ep:budget', 'Budget Night Special', 'episode', at(11, 19, 18), at(11, 19, 19, 30));
  const schools = b('rule:schools', 'Schools Hour', 'rule', at(11, 18, 15), at(11, 18, 16));
  const tea = b('ep:tea', 'The Tea Break', 'episode', at(11, 18, 15), at(11, 18, 16), 31);
  const fair = b('ep:fair', 'Record Fair Live', 'episode', at(11, 18, 15), at(11, 18, 16));

  it('lists what a block clashes with, and says who is on air by the schedule', () => {
    expect(text(budget, [drive, budget])).toBe('Conflict Clashes with Kettle Drive · Ep 214 · 16:00 to 19:00 · overlaps 1 h Show it Where two shows overlap, the one that starts later is on air by the schedule: its presenter is named to callers and its step shapes the studio. Here that is Budget Night Special , from 18:00. Open the day');
    expect(text(tea, [tea, fair])).toContain('Both start at 15:00.');
  });

  it('says what drafting a dotted date would do, and that the other date is not drafted', () => {
    expect(text(schools, [schools, tea])).toContain('The Tea Break · Ep 31 · 15:00 to 16:00 · overlaps 1 h');
    expect(text(schools, [schools, tea])).toContain('This date is not drafted, so nothing of it airs yet. Draft it and it would clash with The Tea Break.');
    expect(text(tea, [schools, tea])).toContain('Schools Hour · not drafted · 15:00 to 16:00 · overlaps 1 h');
    expect(text(tea, [schools, tea])).toContain('The other date is not drafted yet. If it is, the one that starts later is on air by the schedule.');
  });

  it('draws nothing for a block that clashes with nothing', () => {
    expect(render(ClashBox, { props: { block: drive, blocks: [drive, tea], zone: ZONE, onshow: () => {}, onday: () => {} } }).body).not.toContain('calendar.clash');
  });
});

describe('the panel names', () => {
  it('registers the five that came with the week’s clashes and a show’s dates', () => {
    for (const name of ['calendar.day', 'calendar.day.strip', 'calendar.clash', 'calendar.shows', 'calendar.show-dates']) {
      expect(PANELS[name], name).toBeTruthy();
      expect(CALENDAR.some((p) => p.name === name), name).toBe(true);
    }
  });
});
