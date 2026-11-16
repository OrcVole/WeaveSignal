// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where shows clash in the week (week-clashes.md section 2): which block of a clash is open,
// which fold beside it, and what each says of the others. Pure, with no page and no library,
// so it is tested on its own; the grid adapter turns each placement into classes and sizes.
//
// One name is always readable. In a clash one block is open at the day's width less a strip
// (1.5 rem at the day's right edge); the others lie behind it at the day's full width and show
// in the strip, as a tab beside each stretch another block covers: a filled ground in the
// show's colour, a chevron and the name where it fits (the 1.0.5 test run: a thin colour line
// did not read as anything). A block whose first hour is covered is all tab: its words are not
// drawn, and its name is written in any half hour or more left clear. A block covered later
// than its first hour keeps its words down to where the cover begins, and its name is written
// again where it comes out from under the cover. Where two or more are hidden at one time, a
// count (+2) stands on the strip and opens the day. When each show would have 10 rem or more of
// its own, they stand side by side instead and nothing folds.
//
// The studio judges nothing here that the station decides (new-show.md section 8): an overlap
// in time among the blocks it holds is geometry, and the same sums place the fold. What a
// presenter would be refused is the station's to say, and is drawn only where it says it.

import type { Block } from './model.ts';
import { HOUR_MS } from './model.ts';

/** One day's part of a block: its times cut to the day, as the grid places them. */
export interface Part {
  block: Block;
  /** 0 (Monday) to 6. */
  day: number;
  /** Epoch ms, inside the day. */
  from: number;
  to: number;
}

/** How a part is drawn. `plain`: nothing clashes with it. `lane`: it clashes and there is room,
 *  so it stands beside the others. `open`: its words in full. `under`: its words down to where
 *  a block over it begins. `tab`: no words, only the strip and its name where it is clear. */
export type Role = 'plain' | 'lane' | 'open' | 'under' | 'tab';

export interface Placed extends Part {
  /** `ep:12@3`: the block and the day. */
  key: string;
  role: Role;
  /** It leaves the strip free at the day's right edge: a block behind it overlaps it. */
  narrow: boolean;
  /** Its place in the stack; the higher is in front. 0 outside a fold. */
  z: number;
  /** The other shows whose time it shares on this day, by start. */
  others: Block[];
  /** The times blocks in front of it cover, merged and in order: offsets in ms from `from`. */
  covered: [number, number][];
  /** How long from its start it is clear, in ms; its whole length when nothing covers it. */
  clear: number;
  /** Where the name is written apart from the block's own words: for a tab, the start of each
   *  clear stretch of half an hour or more (two at most); for a block that keeps its words,
   *  the start of the first such stretch below a cover. Offsets in ms from `from`. */
  labels: number[];
  /** A lane: which of `cols` it stands in, from 0. */
  col: number;
  cols: number;
}

/** Two or more shows hidden at one time: one tab with a count, which opens the day. */
export interface CountTab {
  key: string;
  day: number;
  from: number;
  to: number;
  /** The shows behind the one in front. */
  blocks: Block[];
}

export interface DayFold {
  placed: Placed[];
  counts: CountTab[];
  /** How many shows are in a clash on this day, for the header's chip. */
  clashing: number;
}

export interface FoldOptions {
  selected: string | null;
  now: number;
  /** Whether `n` shows side by side each have room (10 rem or more). */
  lanes: (n: number) => boolean;
}

const overlaps = (a: { from: number; to: number }, b: { from: number; to: number }) => a.from < b.to && b.from < a.to;

/** Automation yields: it never clashes and never folds. */
const busy = (b: Block) => b.kind !== 'automation';

/** The other shows a block shares time with, by start, each with the overlap in minutes. */
export function clashesOf(b: Block, blocks: readonly Block[]): { block: Block; minutes: number }[] {
  if (!busy(b)) return [];
  return blocks
    .filter((o) => o !== b && o.id !== b.id && busy(o) && o.start < b.end && b.start < o.end)
    .sort((x, y) => x.start - y.start || x.id.localeCompare(y.id))
    .map((o) => ({ block: o, minutes: Math.round((Math.min(b.end, o.end) - Math.max(b.start, o.start)) / 60_000) }));
}

/** Each day's parts of the week's blocks, automation left out. A block that crosses midnight
 *  has a part on each day it touches. */
export function dayParts(blocks: readonly Block[], days: readonly number[]): Part[][] {
  const out: Part[][] = Array.from({ length: 7 }, () => []);
  for (const b of blocks) {
    if (!busy(b)) continue;
    for (let d = 0; d < 7; d++) {
      const from = Math.max(b.start, days[d]);
      const to = Math.min(b.end, days[d + 1]);
      if (to > from) out[d].push({ block: b, day: d, from, to });
    }
  }
  for (const list of out) list.sort((a, b) => a.from - b.from || a.block.id.localeCompare(b.block.id));
  return out;
}

/** Parts that share time, taken together: each group is one clash (or one block alone). The
 *  calendar library groups the same way (`groupChunks`): in start order, a part joins the group
 *  while it starts before the group's end. */
export function clusters(parts: readonly Part[]): Part[][] {
  const out: Part[][] = [];
  let end = -Infinity;
  for (const p of [...parts].sort((a, b) => a.from - b.from || a.block.id.localeCompare(b.block.id))) {
    if (out.length && p.from < end) {
      out[out.length - 1].push(p);
      end = Math.max(end, p.to);
    } else {
      out.push([p]);
      end = p.to;
    }
  }
  return out;
}

/** The lane each part of a group stands in, by the library's own rule: in start order, the
 *  first lane whose last block has ended. */
export function lanesOf(group: readonly Part[]): { cols: number; col: Map<Part, number> } {
  const ends: number[] = [];
  const col = new Map<Part, number>();
  for (const p of group) {
    let i = ends.findIndex((e) => e <= p.from);
    if (i < 0) i = ends.push(p.to) - 1;
    else ends[i] = p.to;
    col.set(p, i);
  }
  return { cols: ends.length, col };
}

/** Which of a clash is in front (section 2, *Which block is open*): the selected block; the
 *  block on air now; the drafted episode that starts last, since by the schedule it is the one
 *  on air where they overlap (a tie goes to the higher episode number, as the station's own
 *  order does); with no episode, the dotted date that starts first. */
export function front(list: readonly Part[], o: Pick<FoldOptions, 'selected' | 'now'>): Part {
  const sel = list.find((p) => p.block.id === o.selected);
  if (sel) return sel;
  const air = list.find((p) => p.block.start <= o.now && p.block.end > o.now);
  if (air) return air;
  const eps = list.filter((p) => p.block.kind === 'episode');
  if (eps.length) {
    return [...eps].sort((a, b) => b.block.start - a.block.start || (b.block.number ?? 0) - (a.block.number ?? 0) || b.block.id.localeCompare(a.block.id))[0];
  }
  return [...list].sort((a, b) => a.block.start - b.block.start || a.block.id.localeCompare(b.block.id))[0];
}

/** The times of `over` inside `p`, merged, as offsets from `p.from`. */
function coverOf(p: Part, over: readonly Part[]): [number, number][] {
  const cuts = over
    .filter((q) => overlaps(p, q))
    .map((q): [number, number] => [Math.max(q.from, p.from) - p.from, Math.min(q.to, p.to) - p.from])
    .sort((a, b) => a[0] - b[0]);
  const out: [number, number][] = [];
  for (const c of cuts) {
    const last = out[out.length - 1];
    if (last && c[0] <= last[1]) last[1] = Math.max(last[1], c[1]);
    else out.push([c[0], c[1]]);
  }
  return out;
}

/** How long a part is clear from its start. */
function clearOf(p: Part, covered: readonly [number, number][]): number {
  if (!covered.length) return p.to - p.from;
  return covered[0][0];
}

/** A block keeps its words when an hour or more of it is clear from its start, or all of it. */
const worded = (p: Part, covered: readonly [number, number][]) => !covered.length || clearOf(p, covered) >= HOUR_MS;

/** The clear stretches of a part, as [from, to] offsets. */
function gaps(p: Part, covered: readonly [number, number][]): [number, number][] {
  const out: [number, number][] = [];
  let at = 0;
  for (const [a, b] of covered) {
    if (a > at) out.push([at, a]);
    at = Math.max(at, b);
  }
  if (at < p.to - p.from) out.push([at, p.to - p.from]);
  return out;
}

/** One clash, folded. The stack is built from the front: first the block the rule above names,
 *  then, among the rest, one that would still keep its words behind those already placed (so
 *  as many names as can be are readable), by the same rule; when none would, the rule alone. */
function foldGroup(group: readonly Part[], o: FoldOptions): { placed: Placed[]; counts: CountTab[] } {
  const day = group[0].day;
  const others = (p: Part) => group.filter((q) => q !== p && overlaps(p, q)).map((q) => q.block);
  const key = (p: Part) => `${p.block.id}@${p.day}`;
  if (group.length === 1) {
    const p = group[0];
    return { placed: [{ ...p, key: key(p), role: 'plain', narrow: false, z: 0, others: [], covered: [], clear: p.to - p.from, labels: [], col: 0, cols: 1 }], counts: [] };
  }
  const lanes = lanesOf(group);
  if (o.lanes(lanes.cols)) {
    return { placed: group.map((p) => ({ ...p, key: key(p), role: 'lane' as const, narrow: false, z: 0, others: others(p), covered: [], clear: p.to - p.from, labels: [], col: lanes.col.get(p) ?? 0, cols: lanes.cols })), counts: [] };
  }

  // The stack, front first.
  const stack: Part[] = [];
  let rest = [...group];
  while (rest.length) {
    const keep = rest.filter((p) => worded(p, coverOf(p, stack)));
    // Only the first choice looks at the selection and the air: behind it the schedule's order stands.
    const next = front(keep.length ? keep : rest, stack.length ? { selected: null, now: -Infinity } : o);
    stack.push(next);
    rest = rest.filter((p) => p !== next);
  }

  const n = stack.length;
  const rank = new Map(stack.map((p, i) => [p, i]));
  const placed: Placed[] = stack.map((p, i) => {
    const covered = coverOf(p, stack.slice(0, i));
    const clear = clearOf(p, covered);
    const role: Role = !covered.length ? 'open' : worded(p, covered) ? 'under' : 'tab';
    const room = gaps(p, covered).filter(([a, b]) => b - a >= HOUR_MS / 2).map(([a]) => a);
    // A block that keeps its words has them in its first clear stretch; the name is written
    // again where it comes out from under the cover, so the part below is not a blank block.
    const labels = role === 'tab' ? room.slice(0, 2) : role === 'under' ? room.filter((a) => a > 0).slice(0, 1) : [];
    return { ...p, key: key(p), role, narrow: stack.slice(i + 1).some((q) => overlaps(p, q)), z: 2 * (n - i), others: others(p), covered, clear, labels, col: 0, cols: 1 };
  });

  // Where three or more share a time, two or more are behind the one in front: a count.
  const edges = [...new Set(group.flatMap((p) => [p.from, p.to]))].sort((a, b) => a - b);
  const deep: { from: number; to: number; at: Part[]; fronts: Set<Part> }[] = [];
  for (let i = 0; i + 1 < edges.length; i++) {
    const seg = { from: edges[i], to: edges[i + 1] };
    const at = group.filter((p) => overlaps(p, seg));
    if (at.length < 3) continue;
    const top = [...at].sort((a, b) => rank.get(a)! - rank.get(b)!)[0];
    const last = deep[deep.length - 1];
    if (last && last.to === seg.from) {
      last.to = seg.to;
      for (const p of at) if (!last.at.includes(p)) last.at.push(p);
      last.fronts.add(top);
    } else deep.push({ ...seg, at: [...at], fronts: new Set([top]) });
  }
  const counts: CountTab[] = deep.map((d) => {
    const byRank = [...d.at].sort((a, b) => rank.get(a)! - rank.get(b)!);
    let behind = byRank.filter((p) => !d.fronts.has(p));
    if (behind.length < 2) behind = byRank.slice(1);
    return { key: `more:${day}:${d.from}`, day, from: d.from, to: d.to, blocks: behind.map((p) => p.block) };
  });

  // A block with nothing of it in view is left to the count: all of it is behind another, and
  // either it leaves the strip to a block behind it, or the count stands on all of its strip.
  const underCount = (p: Placed) => p.covered.every(([a, b]) => counts.some((c) => c.from <= p.from + a && c.to >= p.from + b));
  const seen = placed.filter((p) => {
    const anyClear = gaps(p, p.covered).length > 0;
    return anyClear || (!p.narrow && !underCount(p));
  });
  return { placed: seen, counts };
}

/** Where a covered block's tabs stand, as [from, to] in epoch ms: each stretch another block
 *  covers, less the times a count stands on the strip, since the count is drawn there and a
 *  tab under it would lose its chevron or its name to it. */
export function tabStretches(p: Pick<Placed, 'from' | 'covered'>, counts: readonly Pick<CountTab, 'from' | 'to'>[]): [number, number][] {
  let out = p.covered.map(([a, b]): [number, number] => [p.from + a, p.from + b]);
  for (const c of counts) {
    out = out.flatMap(([a, b]): [number, number][] => {
      if (c.to <= a || c.from >= b) return [[a, b]];
      const cut: [number, number][] = [
        [a, Math.max(a, c.from)],
        [Math.min(b, c.to), b],
      ];
      return cut.filter(([x, y]) => y > x);
    });
  }
  return out;
}

/** One day's parts, folded. */
export function foldDay(parts: readonly Part[], o: FoldOptions): DayFold {
  const out: DayFold = { placed: [], counts: [], clashing: 0 };
  for (const g of clusters(parts)) {
    const f = foldGroup(g, o);
    out.placed.push(...f.placed);
    out.counts.push(...f.counts);
    if (g.length > 1) out.clashing += g.length;
  }
  return out;
}

/** The week, a fold for each day. */
export function foldWeek(blocks: readonly Block[], days: readonly number[], o: FoldOptions): DayFold[] {
  return dayParts(blocks, days).map((parts) => foldDay(parts, o));
}

/** How many shows are in a clash on each day of the week, for the headers and the strip. */
export function clashCounts(blocks: readonly Block[], days: readonly number[]): number[] {
  return dayParts(blocks, days).map((parts) => clusters(parts).reduce((n, g) => n + (g.length > 1 ? g.length : 0), 0));
}

/** The day of the week a moment falls on, 0 to 6, or -1 outside the week. */
export function dayIndex(ms: number, days: readonly number[]): number {
  return days.findIndex((d, i) => i < 7 && ms >= d && ms < days[i + 1]);
}

/** The block the editor column shows: the selected one, while the grid draws it. The week draws
 *  every block it holds; the Day view draws one day's (`day`, 0 to 6; null in the week), so a
 *  block selected on another day is not shown there. It stays the selection, and is shown again
 *  in the week or on its own day, as a block selected in another week is. */
export function shownBlock(blocks: readonly Block[], days: readonly number[], id: string | null, day: number | null): Block | null {
  if (id === null) return null;
  const b = blocks.find((x) => x.id === id) ?? null;
  if (!b || day === null) return b;
  return b.start < days[day + 1] && b.end > days[day] ? b : null;
}

/** Who is on air where a block and the others overlap, by the schedule: the drafted episode
 *  that starts later. `tie` when two start together, which the station alone can settle. */
export function takesAir(b: Block, others: readonly Block[]): { block: Block | null; tie: boolean; drafted: number } {
  const eps = [b, ...others].filter((x) => x.kind === 'episode').sort((x, y) => y.start - x.start);
  if (eps.length < 2) return { block: eps[0] ?? null, tie: false, drafted: eps.length };
  return { block: eps[0], tie: eps[0].start === eps[1].start, drafted: eps.length };
}

