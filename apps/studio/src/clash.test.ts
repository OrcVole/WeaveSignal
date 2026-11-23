// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where shows clash in the week (week-clashes.md section 2), tested without a page or the
// calendar library: every block in a clash says so (a dotted date too), which block is open,
// which fold beside it and how, the count that stands for two or more hidden at one time,
// lanes when there is room, a clash that crosses midnight, and what the block itself says.
// The cases are the note's own: Wednesday, Thursday and Saturday of its sample week, both of
// its edge cases, and the long show with a short one at each end.
import { describe, expect, it } from 'vitest';
import { tabStretches, clashCounts, clashesOf, clusters, dayParts, foldWeek, front, lanesOf, shownBlock, takesAir, type DayFold, type FoldOptions } from './schedule/clash.ts';
import { TAB_NAME_ROOM, tabTitle, blockAria, blockClasses, blockHtml, blockTitle, countHtml, countTitle, namesOf } from './schedule/blockHtml.ts';
import { instant, markConflicts, weekOf, type Block } from './schedule/model.ts';
import { dayToOpen, keyView, moveDay, moveWeek, openDay, sched, select, setView, showInWeek, backToShows, openShow, step, today, weekBlocks, weekRehearsals } from './lib/schedule.svelte.ts';
import type { RehearsalBlock } from './lib/api.ts';
import type { WeekEpisode } from './lib/schedule.ts';

const ZONE = 'Europe/London';
/** Monday 16 November 2026, 10:12: the design's own week. */
const at = (d: number, hh: number, mm = 0) => instant({ y: 2026, m: 11, d, hh, mm }, ZONE);
const NOW = at(16, 10, 12);
const { days } = weekOf(NOW, ZONE);

let n = 0;
function block(show: string, day: number, from: [number, number], to: [number, number], kind: 'episode' | 'rule' = 'episode', o: Partial<Block> = {}): Block {
  const start = at(day, from[0], from[1]);
  const end = to[0] >= 24 ? at(day + 1, to[0] - 24, to[1]) : at(day, to[0], to[1]);
  const slug = show.toLowerCase().replace(/[^a-z]+/g, '-');
  return { id: kind === 'rule' ? `rule:${slug}:2026-11-${day}` : `ep:${slug}-${day}`, kind, start, end, show, slug, colour: (n++ % 12) + 1, series: 1, number: 1, title: '', hosts: [], weekly: true, aired: false, frozen: false, conflict: false, moved: null, clock: null, studio: null, ...o };
}

const fold = (blocks: Block[], o: Partial<FoldOptions> = {}): DayFold[] => foldWeek(markConflicts(blocks), days, { selected: null, now: NOW, lanes: () => false, ...o });
/** `Name:role` for each block drawn on a day, front first, with `!` where it leaves the strip. */
const drawn = (f: DayFold) => [...f.placed].sort((a, b) => b.z - a.z).map((p) => `${p.block.show}:${p.role}${p.narrow ? '!' : ''}`);

describe('what clashes', () => {
  it('marks every block that shares time with another, a dotted date too, and never automation', () => {
    const tea = block('The Tea Break', 18, [15, 0], [16, 0]);
    const schools = block('Schools Hour', 18, [15, 0], [16, 0], 'rule');
    const drive = block('Kettle Drive', 18, [16, 0], [19, 0]);
    const auto = { ...block('Daytime mix', 18, [14, 0], [15, 30]), kind: 'automation' as const, id: 'auto:2:1' };
    markConflicts([tea, schools, drive, auto]);
    expect([tea.conflict, schools.conflict, drive.conflict, auto.conflict]).toEqual([true, true, false, false]);
    expect(clashesOf(tea, [tea, schools, drive, auto]).map((c) => [c.block.show, c.minutes])).toEqual([['Schools Hour', 60]]);
    expect(clashesOf(auto, [tea, schools, auto])).toEqual([]);
  });

  it('counts the shows in clashes on each day, and a clash that crosses midnight on both days', () => {
    const late = block('The Late Shift', 20, [22, 30], [24, 30], 'rule');
    const bothy = block('The Bothy Sessions', 20, [21, 0], [23, 0]);
    const brunch = block('Weekend Brunch', 21, [0, 0], [1, 0], 'rule');
    const drive = block('Kettle Drive', 17, [16, 0], [19, 0]);
    expect(clashCounts([late, bothy, brunch, drive], days)).toEqual([0, 0, 0, 0, 2, 2, 0]);
    expect(dayParts([late], days).map((p) => p.length)).toEqual([0, 0, 0, 0, 1, 1, 0]);
  });

  it('groups as the calendar library does, and gives each its lane', () => {
    const a = block('A', 18, [14, 0], [17, 0]);
    const b = block('B', 18, [14, 0], [15, 0]);
    const c = block('C', 18, [16, 0], [17, 0]);
    const d = block('D', 18, [18, 0], [19, 0]);
    const groups = clusters(dayParts([a, b, c, d], days)[2]);
    expect(groups.map((g) => g.map((p) => p.block.show).join(''))).toEqual(['ABC', 'D']);
    const lanes = lanesOf(groups[0]);
    expect(lanes.cols).toBe(2);
    expect(groups[0].map((p) => lanes.col.get(p))).toEqual([0, 1, 1]);
  });
});

describe('which block is open', () => {
  const parts = (blocks: Block[]) => dayParts(blocks, days).flat();
  it('the selected block, then the one on air, then the drafted episode that starts last, then the first dotted date', () => {
    const early = block('Early', 16, [9, 0], [11, 0]);
    const late = block('Late', 16, [10, 30], [12, 0]);
    const dotted = block('Dotted', 16, [9, 30], [11, 0], 'rule');
    const dotted2 = block('Dotted too', 16, [10, 0], [11, 0], 'rule');
    const all = parts([early, late, dotted, dotted2]);
    expect(front(all, { selected: dotted.id, now: NOW }).block.show).toBe('Dotted');
    // 10:12 on Monday: Early is on air; Late has not started.
    expect(front(all, { selected: null, now: NOW }).block.show).toBe('Early');
    expect(front(all, { selected: null, now: at(15, 12) }).block.show).toBe('Late');
    expect(front(parts([dotted, dotted2]), { selected: null, now: at(15, 12) }).block.show).toBe('Dotted');
  });

  it('a tie between episodes goes to the higher episode number, as the station’s own order does', () => {
    const sport = block('Saturday Sport', 21, [15, 0], [16, 0], 'episode', { number: 12 });
    const fair = block('Record Fair Live', 21, [15, 0], [16, 0], 'episode', { number: 1 });
    expect(front(parts([fair, sport]), { selected: null, now: NOW }).block.show).toBe('Saturday Sport');
    expect(takesAir(fair, [sport])).toMatchObject({ tie: true, drafted: 2 });
  });
});

describe('the fold', () => {
  it('Wednesday: an episode is open over a dotted date at the same hour, which is a tab', () => {
    const tea = block('The Tea Break', 18, [15, 0], [16, 0]);
    const schools = block('Schools Hour', 18, [15, 0], [16, 0], 'rule');
    const f = fold([schools, tea])[2];
    expect(drawn(f)).toEqual(['The Tea Break:open!', 'Schools Hour:tab']);
    expect(f.clashing).toBe(2);
    expect(f.counts).toEqual([]);
    const tab = f.placed.find((p) => p.role === 'tab')!;
    expect(tab.covered).toEqual([[0, 3_600_000]]);
    expect(tab.labels).toEqual([]);
    // Selecting the dotted date opens it, and the episode folds.
    expect(drawn(fold([schools, tea], { selected: schools.id })[2])).toEqual(['Schools Hour:open!', 'The Tea Break:tab']);
  });

  it('Thursday: a block covered later than its first hour keeps its words above the cover', () => {
    const drive = block('Kettle Drive', 19, [16, 0], [19, 0]);
    const budget = block('Budget Night Special', 19, [18, 0], [19, 30]);
    const f = fold([drive, budget])[3];
    expect(drawn(f)).toEqual(['Budget Night Special:open!', 'Kettle Drive:under']);
    const under = f.placed.find((p) => p.role === 'under')!;
    expect(under.clear).toBe(2 * 3_600_000);
    expect(under.covered).toEqual([[2 * 3_600_000, 3 * 3_600_000]]);
    // Nothing of it is clear below the cover, so its name is written once, in its own words.
    expect(under.labels).toEqual([]);
    expect(takesAir(drive, [budget])).toMatchObject({ tie: false, drafted: 2 });
    expect(takesAir(drive, [budget]).block?.show).toBe('Budget Night Special');
  });

  it('Saturday: three at one hour are one open block and a count that stands for the other two', () => {
    const sport = block('Saturday Sport', 21, [15, 0], [16, 0], 'episode', { number: 12 });
    const choice = block('Listeners’ Choice', 21, [15, 0], [16, 0], 'rule', { number: 9 });
    const fair = block('Record Fair Live', 21, [15, 0], [16, 0], 'episode', { number: 1 });
    const f = fold([sport, choice, fair])[5];
    // The two behind have nothing of their own in view, so they are left to the count.
    expect(drawn(f)).toEqual(['Saturday Sport:open!']);
    expect(f.counts).toHaveLength(1);
    expect(f.counts[0]).toMatchObject({ day: 5, from: at(21, 15), to: at(21, 16) });
    expect(f.counts[0].blocks.map((b) => b.show)).toEqual(['Record Fair Live', 'Listeners’ Choice']);
    expect(f.clashing).toBe(3);
    expect(countHtml(2)).toContain('+2');
    expect(countTitle(f.counts[0].blocks)).toBe('2 more shows at this time: Record Fair Live, Listeners’ Choice. Press to open the day.');
  });

  it('a show that starts half an hour before the one over it is a tab with its name in that half hour', () => {
    const garden = block('Gardeners’ Corner', 21, [14, 30], [16, 0]);
    const classics = block('Kettle Classics', 21, [15, 0], [16, 0]);
    const f = fold([garden, classics])[5];
    expect(drawn(f)).toEqual(['Kettle Classics:open!', 'Gardeners’ Corner:tab']);
    const tab = f.placed.find((p) => p.role === 'tab')!;
    expect(tab.clear).toBe(1_800_000);
    expect(tab.labels).toEqual([0]);
  });

  it('a long show with a short one at each end: the later short one is open, the long show keeps its words, the earlier is its tab', () => {
    const requests = block('Sunday Requests', 22, [14, 0], [17, 0], 'episode', { number: 40 });
    const firth = block('Firth Sessions', 22, [14, 0], [15, 0], 'episode', { number: 7 });
    const archive = block('The Archive Hour', 22, [16, 0], [17, 0], 'episode', { number: 22 });
    const f = fold([requests, firth, archive])[6];
    expect(drawn(f)).toEqual(['The Archive Hour:open!', 'Sunday Requests:under!', 'Firth Sessions:tab']);
    expect(f.placed.find((p) => p.block === requests)!.clear).toBe(2 * 3_600_000);
    expect(f.counts).toEqual([]);
    // Selecting the long show opens it, and the two short ones are two tabs at their own times.
    const sel = fold([requests, firth, archive], { selected: requests.id })[6];
    expect(drawn(sel)).toEqual(['Sunday Requests:open!', 'The Archive Hour:tab', 'Firth Sessions:tab']);
    expect(sel.placed.filter((p) => p.role === 'tab').every((p) => !p.narrow)).toBe(true);
  });

  it('an hour over the middle of a three-hour show (the 1.0.5 test run): the long show keeps its words above, a tab beside, and its name again below', () => {
    const lunch = block('The Long Lunch', 17, [12, 0], [15, 0], 'episode', { number: 9 });
    const steeped = block('Steeped', 17, [13, 0], [14, 0], 'episode', { number: 1 });
    const f = fold([lunch, steeped])[1];
    expect(drawn(f)).toEqual(['Steeped:open!', 'The Long Lunch:under']);
    const under = f.placed.find((p) => p.block === lunch)!;
    expect(under.clear).toBe(3_600_000);
    expect(under.covered).toEqual([[3_600_000, 2 * 3_600_000]]);
    expect(under.labels).toEqual([2 * 3_600_000]);
    expect(f.clashing).toBe(2);
    // Pressed, the long show comes forward and the hour is all tab, with no name of its own in view.
    const sel = fold([lunch, steeped], { selected: lunch.id })[1];
    expect(drawn(sel)).toEqual(['The Long Lunch:open!', 'Steeped:tab']);
    expect(sel.placed.find((p) => p.block === steeped)!.labels).toEqual([]);
  });

  it('a tab gives way to a count for the time the count stands on the strip', () => {
    const H = 3_600_000;
    const p = { from: 15 * H, covered: [[0, 1.5 * H], [2 * H, 3 * H]] as [number, number][] };
    expect(tabStretches(p, [])).toEqual([[15 * H, 16.5 * H], [17 * H, 18 * H]]);
    // A count over the middle half hour cuts the first tab in two; one over all of the second removes it.
    expect(tabStretches(p, [{ from: 15.5 * H, to: 16 * H }, { from: 17 * H, to: 18 * H }])).toEqual([[15 * H, 15.5 * H], [16 * H, 16.5 * H]]);
    expect(tabStretches(p, [{ from: 14 * H, to: 15.5 * H }])).toEqual([[15.5 * H, 16.5 * H], [17 * H, 18 * H]]);
    // The real case: an hour's episode, a second from its half hour, and a long dotted date behind both.
    const a = block('Evensong', 22, [15, 0], [16, 0], 'episode', { number: 1 });
    const b = block('The Sunday Play', 22, [15, 30], [16, 30], 'episode', { number: 1 });
    const c = block('Sunday Requests', 22, [15, 0], [18, 0], 'rule');
    const f = fold([a, b, c])[6];
    const behind = f.placed.find((x) => x.block === c)!;
    expect(f.counts.map((x) => [x.from - behind.from, x.to - behind.from])).toEqual([[0.5 * H, H]]);
    expect(tabStretches(behind, f.counts).map(([x, y]) => [x - behind.from, y - behind.from])).toEqual([[0, 0.5 * H], [H, 1.5 * H]]);
  });

  it('a chain: the block behind two others shows beside both, with its name where it is clear', () => {
    const a = block('A', 18, [15, 0], [16, 0], 'rule');
    const b = block('B', 18, [15, 30], [17, 0], 'rule');
    const c = block('C', 18, [16, 30], [18, 0], 'rule');
    const f = fold([a, b, c])[2];
    // A is first; C keeps its words behind nothing; B is behind both and named in the half hour between.
    expect(drawn(f)).toEqual(['A:open!', 'C:open!', 'B:tab']);
    const tab = f.placed.find((p) => p.block === b)!;
    expect(tab.covered).toEqual([[0, 1_800_000], [3_600_000, 5_400_000]]);
    expect(tab.labels).toEqual([1_800_000]);
    expect(f.counts).toEqual([]);
  });

  it('stands them side by side when each lane has room, and folds nothing', () => {
    const tea = block('The Tea Break', 18, [15, 0], [16, 0]);
    const schools = block('Schools Hour', 18, [15, 0], [16, 0], 'rule');
    const third = block('Third', 18, [15, 0], [16, 0], 'rule');
    const two = fold([tea, schools], { lanes: (k) => 20 / k >= 10 })[2];
    expect(two.placed.map((p) => [p.role, p.col, p.cols])).toEqual([['lane', 0, 2], ['lane', 1, 2]]);
    expect(two.placed.every((p) => p.others.length === 1)).toBe(true);
    // Three need 30 rem; in 20 they fold.
    expect(fold([tea, schools, third], { lanes: (k) => 20 / k >= 10 })[2].placed.some((p) => p.role === 'lane')).toBe(false);
  });

  it('folds a clash that crosses midnight for each day’s part', () => {
    const bothy = block('The Bothy Sessions', 20, [21, 0], [23, 0]);
    const late = block('The Late Shift', 20, [22, 30], [24, 30], 'rule');
    const f = fold([bothy, late]);
    expect(drawn(f[4])).toEqual(['The Bothy Sessions:open!', 'The Late Shift:tab']);
    // An hour of it is clear before midnight, so its name is written there.
    expect(f[4].placed.find((p) => p.block === late)!.labels).toEqual([1_800_000]);
    expect(drawn(f[5])).toEqual(['The Late Shift:plain']);
    expect(f[5].placed[0]).toMatchObject({ from: days[5], to: at(21, 0, 30), key: `${late.id}@5` });
  });

  it('leaves a block alone, and automation out of it', () => {
    const drive = block('Kettle Drive', 17, [16, 0], [19, 0]);
    const auto = { ...block('Daytime mix', 17, [10, 0], [16, 0]), kind: 'automation' as const, id: 'auto:1:1' };
    const f = fold([drive, auto])[1];
    expect(drawn(f)).toEqual(['Kettle Drive:plain']);
    expect(f.clashing).toBe(0);
  });
});

describe('what a block in a clash says', () => {
  const tea = block('The Tea Break', 18, [15, 0], [16, 0]);
  const schools = block('Schools Hour', 18, [15, 0], [16, 0], 'rule', { colour: 9 });
  markConflicts([tea, schools]);
  const ctx = { now: NOW, zone: ZONE, selected: null };

  it('the open block: the triangle leads its time, then CONFLICT and the other show with a square of its colour', () => {
    const c = { ...ctx, place: { role: 'open' as const, narrow: true, others: [schools] } };
    const html = blockHtml(tea, c);
    expect(html).toMatch(/<span class="tl"><span class="cf"><svg[^>]*><use href="#g-fault"\/><\/svg><\/span><span class="num">15:00–16:00<\/span>/);
    expect(html).toContain('Conflict</span><span class="over">over <i style="--c:var(--cart-9)"></i>Schools Hour</span>');
    expect(blockClasses(tea, c)).toEqual(['ev', 'conflict', 'narrow']);
    expect(blockTitle(tea, c)).toBe('The Tea Break, live, 15:00 to 16:00, clashes with Schools Hour.');
    expect(blockAria(tea, c)).toBe('The Tea Break, live, 15:00 to 16:00, Ep 1, conflict, clashes with Schools Hour');
    expect(blockHtml(tea, { ...c, place: { ...c.place, others: [schools, schools, tea] } })).toContain('over 3 shows');
  });

  it('a dotted date in a clash says CONFLICT and still says Not drafted', () => {
    const c = { ...ctx, place: { role: 'open' as const, narrow: true, others: [tea] } };
    const html = blockHtml(schools, c);
    expect(html).toContain('Conflict');
    expect(html).toContain('Not drafted');
    expect(blockClasses(schools, c)).toEqual(['ev', 'rule', 'conflict', 'narrow']);
  });

  it('a tab has no words of its own: a filled tab with a chevron beside the cover, its name where it is clear, and the rest in its tooltip', () => {
    const c = { ...ctx, place: { role: 'tab' as const, narrow: false, others: [tea], labels: [0], tabs: [{ top: 23, height: 48 }] } };
    const html = blockHtml(schools, c);
    const tip = 'Schools Hour, not drafted, 15:00 to 16:00, clashes with The Tea Break. Folded: press to bring it forward.';
    expect(html).toBe(`<div class="evb tabb"><span class="lbl" style="top:3px">Schools Hour</span></div><span class="ftab bare" style="top:23px;height:48px" title="${tip}"><svg class="chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M7.5 2.5 4 6l3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg><span class="fn">Schools Hour</span></span>`);
    expect(tabTitle(schools, c)).toBe(tip);
    // An hour's tab has the chevron alone; from an hour and a half it has the name down it too.
    expect(blockHtml(tea, { ...c, place: { ...c.place, labels: [], tabs: [{ top: 0, height: TAB_NAME_ROOM - 1 }] } })).toContain('class="ftab bare"');
    expect(blockHtml(tea, { ...c, place: { ...c.place, labels: [], tabs: [{ top: 0, height: TAB_NAME_ROOM }] } })).toMatch(/<span class="ftab" style="top:0px;height:70px"[^>]*>.*<span class="fn">The Tea Break<\/span><\/span>$/);
    expect(blockClasses(schools, c)).toEqual(['ev', 'rule', 'conflict', 'tab']);
    expect(blockTitle(schools, c)).toBe('Schools Hour, not drafted, 15:00 to 16:00, clashes with The Tea Break. Folded: press to bring it forward.');
    expect(blockAria(schools, c)).toBe('Schools Hour, a date from its rule, 15:00 to 16:00, not drafted yet, conflict, clashes with The Tea Break, folded: press Enter to bring it forward');
  });

  it('a block that keeps its words above a cover has a tab beside the cover and its name below it', () => {
    const lunch = block('The Long Lunch', 17, [12, 0], [15, 0], 'episode', { colour: 6 });
    const steeped = block('Steeped', 17, [13, 0], [14, 0], 'episode', { colour: 3 });
    markConflicts([lunch, steeped]);
    const c = { ...ctx, place: { role: 'under' as const, narrow: false, others: [steeped], labels: [96], tabs: [{ top: 47, height: 48 }] } };
    const html = blockHtml(lunch, c);
    expect(html).toMatch(/^<div class="evb">.*<b>The Long Lunch<\/b>.*<\/div><span class="lbl" style="top:99px">The Long Lunch<\/span><span class="ftab bare" style="top:47px;height:48px" title="The Long Lunch, live, 12:00 to 15:00, clashes with Steeped\. Folded here: press to bring it forward\.">/);
    expect(blockClasses(lunch, c)).toEqual(['ev', 'conflict', 'under']);
    // The block's own tooltip is as it was; a screen reader hears that part of it is folded.
    expect(blockTitle(lunch, c)).toBe('The Long Lunch, live, 12:00 to 15:00, clashes with Steeped.');
    expect(blockAria(lunch, c)).toContain('conflict, clashes with Steeped, partly folded: press Enter to bring it forward');
    // The hour over it says what it is over, with a square of the long show's colour, even at an hour tall.
    const open = { ...ctx, place: { role: 'open' as const, narrow: true, others: [lunch], short: true } };
    expect(blockHtml(steeped, open)).toContain('<span class="over">over <i style="--c:var(--cart-6)"></i>The Long Lunch</span>');
    expect(blockClasses(steeped, open)).toEqual(['ev', 'conflict', 'short', 'narrow']);
    expect(blockClasses(steeped, { ...open, place: { ...open.place, tiny: true } })).toEqual(['ev', 'conflict', 'short', 'tiny', 'narrow']);
    // A covered block that leaves the strip to one behind it stands its tab out beyond its own edge.
    const chained = { ...c, place: { ...c.place, narrow: true } };
    expect(blockClasses(lunch, chained)).toEqual(['ev', 'conflict', 'spill', 'under', 'narrow']);
    expect(blockHtml(lunch, chained)).toContain('<span class="ftab bare out"');
    expect(blockClasses(steeped, open)).not.toContain('spill');
    // Nothing covered: no tab.
    expect(blockHtml(steeped, open)).not.toContain('ftab');
  });

  it('in the Day view the chip is always in view, so the time row has no triangle, and the overlap is said', () => {
    const drive = block('Kettle Drive', 19, [16, 0], [19, 0], 'episode', { colour: 2 });
    const budget = block('Budget Night Special', 19, [18, 0], [19, 30]);
    markConflicts([drive, budget]);
    const html = blockHtml(budget, { ...ctx, wide: true, place: { role: 'lane', narrow: false, others: [drive] } });
    expect(html).toContain('<div class="evb wide">');
    expect(html).not.toContain('class="cf"');
    expect(html).toContain('over <i style="--c:var(--cart-2)"></i>Kettle Drive · 1 h</span>');
  });

  it('says nothing more of a block that clashes with nothing', () => {
    const drive = block('Kettle Drive', 17, [16, 0], [19, 0]);
    expect(blockTitle(drive, { ...ctx, place: { role: 'plain', narrow: false, others: [] } })).toBe('');
    expect(blockHtml(drive, ctx)).not.toContain('class="cf"');
    expect(namesOf([tea, schools, drive])).toBe('The Tea Break, Schools Hour and Kettle Drive');
  });
});

describe('the Day view and the show pane, as the tab holds them', () => {
  const reset = () => Object.assign(sched, { zone: ZONE, anchor: NOW, view: 'week', selected: null, pane: 'episode', editShow: null, showTab: 'dates', reveal: null });

  it('Day opens on the selected block’s day, else today when it is in the week, else Monday', () => {
    expect(dayToOpen(at(19, 18), NOW, days)).toBe(at(19, 18));
    expect(dayToOpen(null, NOW, days)).toBe(NOW);
    expect(dayToOpen(null, at(25, 9), days)).toBe(days[0] + 12 * 3_600_000);
    // A block selected in another week does not hold the day.
    expect(dayToOpen(at(25, 9), NOW, days)).toBe(NOW);
  });

  it('the editor shows the selected block only while the grid draws it: in the Day view, on its own day', () => {
    // The operator's 1.0.5 run: an automation block pressed on Monday, then Wednesday opened by
    // its header. The column showed Monday's log beside a day with nothing selected in it.
    const auto: Block = { ...block('Daytime mix', 16, [10, 0], [16, 0]), id: `auto:0:${at(16, 10)}`, kind: 'automation' };
    const late = block('The Late Shift', 20, [22, 30], [24, 30], 'rule');
    const blocks = [auto, block('The Tea Break', 18, [15, 0], [16, 0]), late];
    // The week draws every block, so whatever is selected is shown.
    expect(shownBlock(blocks, days, auto.id, null)).toBe(auto);
    // The Day view on Monday shows Monday's block; on Wednesday it does not, and nothing is shown.
    expect(shownBlock(blocks, days, auto.id, 0)).toBe(auto);
    expect(shownBlock(blocks, days, auto.id, 2)).toBeNull();
    // A block that crosses midnight is drawn on both its days.
    expect(shownBlock(blocks, days, late.id, 4)).toBe(late);
    expect(shownBlock(blocks, days, late.id, 5)).toBe(late);
    expect(shownBlock(blocks, days, late.id, 6)).toBeNull();
    // Nothing selected, or a block of another week: nothing, in either view.
    expect(shownBlock(blocks, days, null, null)).toBeNull();
    expect(shownBlock(blocks, days, 'ep:elsewhere', null)).toBeNull();
    expect(shownBlock(blocks, days, 'ep:elsewhere', 3)).toBeNull();
  });

  it('opens a day, moves by a day, and reads the week again only when the day leaves it', () => {
    reset();
    const v = sched.version;
    openDay(at(21, 15));
    expect([sched.view, sched.anchor, sched.version]).toEqual(['day', at(21, 12), v]);
    moveDay(1);
    expect([sched.anchor, sched.version]).toEqual([at(22, 12), v]);
    moveDay(1);
    expect([sched.anchor, sched.version]).toEqual([at(23, 12), v + 1]);
    setView('week');
    expect(sched.view).toBe('week');
    expect(weekOf(sched.anchor, ZONE).days[0]).toBe(at(23, 0));
  });

  it('the keys ← → step a day in the Day view and a week in the Week view; D and W choose the view (ADR-0283)', () => {
    reset();
    openDay(at(20, 9));
    step(1);
    expect([sched.view, sched.anchor]).toEqual(['day', at(21, 12)]);
    step(-2);
    expect(sched.anchor).toBe(at(19, 12));
    keyView('week', NOW);
    expect(sched.view).toBe('week');
    step(1);
    expect([sched.view, weekOf(sched.anchor, ZONE).days[0]]).toEqual(['week', at(23, 0)]);
    reset();
    keyView('day', NOW);
    const todayAt = sched.anchor;
    expect(sched.view).toBe('day');
    openDay(NOW);
    expect(sched.anchor).toBe(todayAt);
  });

  it('a week from the Day view keeps the weekday; T is today', () => {
    reset();
    openDay(at(20, 9));
    moveWeek(1);
    expect([sched.view, sched.anchor]).toEqual(['day', at(27, 12)]);
    moveWeek(-2);
    expect(sched.anchor).toBe(at(13, 12));
    today(NOW);
    expect([sched.view, sched.anchor]).toEqual(['day', NOW]);
    reset();
    moveWeek(1);
    expect(sched.anchor).toBe(at(23, 12));
  });

  it('a row shown in the week keeps the show’s pane; a block pressed in the week turns the editor to it', () => {
    reset();
    openShow('the-bothy-sessions');
    expect([sched.pane, sched.editShow, sched.showTab]).toEqual(['show', 'the-bothy-sessions', 'dates']);
    showInWeek('ep:5', at(25, 21));
    expect([sched.pane, sched.selected, weekOf(sched.anchor, ZONE).days[0]]).toEqual(['show', 'ep:5', at(23, 0)]);
    expect(sched.reveal?.id).toBe('ep:5');
    select('ep:6');
    expect([sched.pane, sched.selected]).toEqual(['episode', 'ep:6']);
    openShow('the-bothy-sessions', 'details');
    backToShows();
    expect([sched.pane, sched.editShow, sched.selected]).toEqual(['episode', null, null]);
  });
});

// The tab's ground and its ink (week-clashes.md section 2), worked out from the two stylesheets
// as a browser mixes and paints them: `color-mix(in srgb, <show colour> <tint>, var(--surface))`,
// to the nearest of 256 levels, under `--fold-ink`. The driver (tests/newshow/week.mjs) measures the tabs of its own week in a real
// browser; this holds every show colour, and the ground of a show with none, in each theme.
const fsModule = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsModule)) as { readFileSync(path: URL, encoding: 'utf8'): string };
const TOKENS = readFileSync(new URL('../../../packages/ui/tokens.css', import.meta.url), 'utf8');
const SCHEDULE_CSS = readFileSync(new URL('./styles/schedule.css', import.meta.url), 'utf8');

describe('the fold tab’s ground and ink, from the tokens', () => {
  type Theme = 'dark' | 'light' | 'night';
  const body = (css: string, selector: string) => css.slice(css.indexOf(`${selector} {`) + selector.length + 2).split('}')[0];
  const declared = (text: string) => Object.fromEntries([...text.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
  const dark = declared(body(TOKENS, ':root'));
  const tokens: Record<Theme, Record<string, string>> = {
    dark,
    light: { ...dark, ...declared(body(TOKENS, ':root[data-theme="light"]')) },
    night: { ...dark, ...declared(body(TOKENS, ':root[data-theme="night"]')) },
  };
  // The fold's own two, as schedule.css sets them: for every theme, then for Light and for Night.
  const base = declared(body(SCHEDULE_CSS, '.schedule .ec'));
  const fold: Record<Theme, Record<string, string>> = {
    dark: base,
    light: { ...base, ...declared(body(SCHEDULE_CSS, ':root[data-theme="light"] .schedule .ec')) },
    night: { ...base, ...declared(body(SCHEDULE_CSS, ':root[data-theme="night"] .schedule .ec')) },
  };
  const hex = (t: Theme, value: string): number[] => {
    const v = value.startsWith('var(') ? tokens[t][value.slice(4, -1)] : value;
    expect(v, `${t} ${value}`).toMatch(/^#[0-9a-f]{6}$/);
    return [1, 3, 5].map((i) => parseInt(v.slice(i, i + 2), 16));
  };
  const lum = (c: number[]) => {
    const [r, g, b] = c.map((v) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const ratio = (a: number[], b: number[]) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  /** The least contrast of the tab's ink on its ground over the twelve show colours and `--deemph`. */
  function least(t: Theme) {
    const tint = Number(fold[t]['--fold-tint'].replace('%', '')) / 100;
    const ink = hex(t, fold[t]['--fold-ink']);
    const surface = hex(t, 'var(--surface)');
    const grounds = [...Array.from({ length: 12 }, (_, i) => `var(--cart-${i + 1})`), 'var(--deemph)'].map((c) => hex(t, c).map((v, i) => Math.round(v * tint + surface[i] * (1 - tint))));
    return { tint, ink: Math.min(...grounds.map((g) => ratio(ink, g))), stands: Math.min(...grounds.map((g) => ratio(g, surface))) };
  }

  it('is set once, and used by the tab and by the count', () => {
    expect(Object.keys(fold.dark)).toEqual(expect.arrayContaining(['--fold-tint', '--fold-ink']));
    expect(SCHEDULE_CSS).toContain('.ftab { ');
    expect(body(SCHEDULE_CSS, '.schedule .ev .ftab')).toContain('color-mix(in srgb, var(--show-colour, var(--deemph)) var(--fold-tint), var(--surface))');
    expect(body(SCHEDULE_CSS, '.schedule .ev .ftab')).toContain('color: var(--fold-ink)');
    expect(body(SCHEDULE_CSS, '.schedule .ev.tab.count .plus')).toContain('color: var(--fold-ink)');
  });

  it('the name on a tab is 4.5:1 or more on every show colour, in Dark, Light and Night', () => {
    const got = { dark: least('dark'), light: least('light'), night: least('night') };
    for (const t of ['dark', 'light', 'night'] as const) expect(got[t].ink, `${t}: ${got[t].ink.toFixed(2)}:1 at ${got[t].tint * 100} %`).toBeGreaterThanOrEqual(4.5);
    // The figures the stylesheet and the design note give.
    expect([got.dark.tint, got.light.tint, got.night.tint]).toEqual([0.6, 0.6, 0.5]);
    expect([got.dark.ink, got.light.ink, got.night.ink].map((r) => r.toFixed(2))).toEqual(['5.87', '4.92', '4.71']);
  });

  it('the ground reads as the show’s colour: stronger against the surface than the 40 % it replaced', () => {
    const at40 = (t: Theme) => {
      const surface = hex(t, 'var(--surface)');
      return Math.max(...Array.from({ length: 12 }, (_, i) => ratio(hex(t, `var(--cart-${i + 1})`).map((v, k) => v * 0.4 + surface[k] * 0.6), surface)));
    };
    for (const t of ['dark', 'light', 'night'] as const) expect(least(t).tint).toBeGreaterThan(0.4);
    expect(at40('dark')).toBeLessThan(2);
  });
});

// Studios as columns (1.3.0; rehearsal-mode-build-plan.md section 2.1). Every show on air is in
// the one column, wherever its host is, because there is one programme; the rehearsal studio's
// bookings and sessions are beside it and never on air. So the clash rule is not keyed on the
// column, and nothing of the rehearsal studio is ever in a clash.
describe('clashes and the studios', () => {
  it('an own-studio show overlapping a room show is still a clash', () => {
    // The Morning Kettle from the room; Steeped's host comes in from their own studio at 13:00.
    const kettle = block('The Morning Kettle', 18, [10, 0], [14, 0], 'episode', { studio: 'studio', from: 'room' });
    const steeped = block('Steeped', 18, [13, 0], [15, 0], 'episode', { studio: 'studio', from: 'own' });
    markConflicts([kettle, steeped]);
    expect([kettle.conflict, steeped.conflict]).toEqual([true, true]);
    expect(clashesOf(steeped, [kettle, steeped]).map((o) => [o.block.show, o.minutes])).toEqual([['The Morning Kettle', 60]]);
    expect(clashCounts([kettle, steeped], days)[2]).toBe(2);
    expect(takesAir(kettle, [steeped]).block).toBe(steeped);
  });

  it('is never keyed on the column: a block with no column, or another one, clashes the same', () => {
    // Before 1.3.0 a clash counted only within one `studio` value. A dotted date carries the
    // column too, and a block made without one (an older build's) is not let off.
    const a = block('The Tea Break', 18, [15, 0], [16, 0], 'episode', { studio: 'studio' });
    const b = block('Schools Hour', 18, [15, 0], [16, 0], 'rule', { studio: null });
    const c = block('Kettle Drive', 18, [15, 30], [18, 0], 'episode', { studio: 'somewhere-else' });
    markConflicts([a, b, c]);
    expect([a.conflict, b.conflict, c.conflict]).toEqual([true, true, true]);
  });

  /** One episode as the station sends it with the week, on Wednesday 18 November. */
  const aired = (slug: string, name: string, hh: number, minutes: number, from?: 'room' | 'own'): WeekEpisode =>
    ({
      show: { slug, name },
      series: { number: 1 },
      episode: { id: `${slug}-1`, number: 1, title: '', air_at: at(18, hh) / 1000, minutes, aired_at: null, kind: 'live' },
      resolved: { show: name, slug, colour_slot: 4, hosts: [], rule: null, slot: null, episode: { title: '', minutes } },
      ...(from ? { studio: 'studio', from } : {}),
    }) as unknown as WeekEpisode;
  const booked = (o: Partial<RehearsalBlock> = {}): RehearsalBlock => ({ id: 'b1', kind: 'booking', show: 'steeped', show_name: 'Steeped', colour_slot: 13, start_ms: at(18, 13, 30), end_ms: at(18, 14), by: 'Finlay', open: false, ended: null, ...o });
  const seed = (rehearsals: RehearsalBlock[]) =>
    Object.assign(sched, { zone: ZONE, anchor: NOW, view: 'week', selected: null, booking: null, dates: null, hours: [], clocks: [], defaultClock: null, shows: [], episodes: [aired('the-morning-kettle', 'The Morning Kettle', 10, 240, 'room'), aired('steeped', 'Steeped', 14, 120, 'own')], rehearsals });

  it('a booking beside an episode is not a clash', () => {
    // Steeped is booked to rehearse at 13:30 while The Morning Kettle is on air until 14:00.
    seed([booked()]);
    const { blocks } = weekBlocks(NOW);
    const kettle = blocks.find((b) => b.slug === 'the-morning-kettle')!;
    expect(kettle.conflict).toBe(false);
    expect(blocks.some((b) => b.conflict)).toBe(false);
    expect(clashesOf(kettle, blocks)).toEqual([]);
    expect(clashCounts(blocks, days)).toEqual([0, 0, 0, 0, 0, 0, 0]);
    // A session while a show is on air is not one either, open or over.
    seed([booked(), booked({ id: 's1', kind: 'session', start_ms: at(18, 11), end_ms: at(18, 11, 40), by: 'Kirsty', ended: 'ended' }), booked({ id: 's2', kind: 'session', start_ms: at(18, 13, 32), end_ms: at(18, 13, 40), open: true })]);
    expect(weekBlocks(at(18, 13, 45)).blocks.some((b) => b.conflict)).toBe(false);
  });

  it('the rehearsal studio is never among the week’s blocks, and automation does not yield to it', () => {
    seed([]);
    const bare = weekBlocks(NOW).blocks;
    seed([booked({ start_ms: at(18, 16), end_ms: at(18, 17) }), booked({ id: 's1', kind: 'session', start_ms: at(18, 18), end_ms: at(18, 19), ended: 'moved' })]);
    const withStudio = weekBlocks(NOW).blocks;
    // 16:00 to 19:00 is automation's, booked or not: the same blocks, to the id.
    expect(withStudio).toEqual(bare);
    expect(withStudio.filter((b) => b.kind === 'automation' && b.start <= at(18, 16) && b.end >= at(18, 19))).toHaveLength(1);
    expect(withStudio.every((b) => b.kind === 'episode' || b.kind === 'automation')).toBe(true);
    // They are the Rehearsal column's alone, bookings first.
    expect(weekRehearsals(NOW).map((r) => [r.id, r.kind, r.show])).toEqual([
      ['reh:b1', 'booking', 'Steeped'],
      ['reh:s1', 'session', 'Steeped'],
    ]);
    // The fold sees shows only, so nothing of the rehearsal studio folds or is counted.
    expect(foldWeek(withStudio, days, { selected: null, now: NOW, lanes: () => false })[2].placed.map((p) => p.block.show)).toEqual(['The Morning Kettle', 'Steeped']);
    seed([]);
    sched.episodes = [];
  });
});
