// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One block's markup (calendar.md §2), as HTML for whichever library draws the grid: the time
// and the repeat glyph, the show, the flags (CONFLICT, MOVED, ON AIR), the kind as a word and a
// glyph, the episode line and the people. The grid library takes HTML for an event's content,
// so a different library would differ only in what it draws around it.

import { episodeLine, hm, lengthWords, type Block } from './model.ts';

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);
}

const glyph = (id: string) => `<svg aria-hidden="true"><use href="#${id}"/></svg>`;

/** How the fold draws a block (clash.ts), with its offsets already in the grid's pixels. */
export interface BlockPlace {
  role: 'plain' | 'lane' | 'open' | 'under' | 'tab';
  /** It leaves the strip free at the day's right edge. */
  narrow: boolean;
  /** The other shows whose time it shares. */
  others: readonly Pick<Block, 'show' | 'colour'>[];
  /** Where its name is written apart from its own words, in pixels from its top. */
  labels?: readonly number[];
  /** The tabs: beside each stretch another block covers, its top and height in pixels. */
  tabs?: readonly { top: number; height: number }[];
  /** An hour or less: two lines, and a third for what it is over. */
  short?: boolean;
  /** Under an hour: the time and the name, and no more. */
  tiny?: boolean;
}

/** A tab this tall or taller has room for the show's name down it (an hour and a half). */
export const TAB_NAME_ROOM = 70;

export interface BlockContext {
  now: number;
  zone: string;
  selected: string | null;
  /** The length is printed on an overnight block that is not 24 hours of a day (the clocks-back night). */
  longNight?: boolean;
  /** Where the block stands in a clash; absent, it is drawn alone. */
  place?: BlockPlace | null;
  /** The Day view: a lane has room for the words along the line. */
  wide?: boolean;
}

/** `The Tea Break`, `The Tea Break and Schools Hour`, `A, B and C`. */
export function namesOf(list: readonly Pick<Block, 'show'>[]): string {
  const n = [...new Set(list.map((o) => o.show))];
  return n.length < 2 ? n.join('') : `${n.slice(0, -1).join(', ')} and ${n[n.length - 1]}`;
}

const kindWord = (b: Block) => (b.kind === 'rule' ? 'not drafted' : b.prerecorded ? 'pre-recorded' : 'live');

/** The classes a block's element carries, for the library's `eventClassNames`. */
export function blockClasses(b: Block, ctx: BlockContext): string[] {
  const past = b.end <= ctx.now;
  const out = ['ev'];
  if (b.kind === 'automation') out.push('auto');
  if (b.kind === 'rule') out.push('rule');
  if (past) out.push('past');
  if (ctx.selected === b.id) out.push('sel');
  if (b.conflict) out.push('conflict');
  const p = ctx.place;
  if (p?.short) out.push('short');
  if (p?.tiny) out.push('tiny');
  // A covered block that itself leaves the strip to one behind it: its tabs stand beyond its edge.
  if (p?.narrow && p.tabs?.length) out.push('spill');
  if (p?.role === 'lane') out.push('lane');
  if (p?.role === 'under') out.push('under');
  if (p?.role === 'tab') out.push('tab');
  if (p?.narrow) out.push('narrow');
  return out;
}

/** What a screen reader hears for the block. */
export function blockAria(b: Block, ctx: BlockContext): string {
  const when = `${hm(b.start, ctx.zone)} to ${hm(b.end, ctx.zone)}`;
  if (b.kind === 'automation') return `Automation, ${b.show}, ${when}`;
  const others = ctx.place?.others ?? [];
  const clash = b.conflict ? (others.length ? `conflict, clashes with ${namesOf(others)}` : 'conflict') : '';
  const folded = ctx.place?.role === 'tab' ? 'folded: press Enter to bring it forward' : ctx.place?.role === 'under' ? 'partly folded: press Enter to bring it forward' : '';
  if (b.kind === 'rule') {
    const head = `${b.show}, a date from its rule, ${when}, not drafted yet`;
    const tail = folded || 'select it to draft the episode or skip the date';
    return clash ? `${head}, ${clash}, ${tail}` : `${head}: ${tail}`;
  }
  const parts = [b.show, b.prerecorded ? 'pre-recorded' : 'live', when];
  const ep = episodeLine(b);
  if (ep) parts.push(ep);
  if (b.studio) parts.push(b.studio);
  if (b.end <= ctx.now) parts.push('aired');
  if (clash) parts.push(clash);
  if (b.moved) parts.push('moved this week');
  if (b.start <= ctx.now && b.end > ctx.now) parts.push('on air');
  if (folded) parts.push(folded);
  return parts.join(', ');
}

/** What a pointer is told on a block in a clash, where the block itself may have no room for
 *  the word: `The Tea Break, live, 15:00 to 16:00, clashes with Schools Hour.` Empty otherwise. */
export function blockTitle(b: Block, ctx: BlockContext): string {
  const others = ctx.place?.others ?? [];
  if (!b.conflict || !others.length) return '';
  const say = `${b.show}, ${kindWord(b)}, ${hm(b.start, ctx.zone)} to ${hm(b.end, ctx.zone)}, clashes with ${namesOf(others)}.`;
  return ctx.place?.role === 'tab' ? `${say} Folded: press to bring it forward.` : say;
}

/** What a pointer is told on a tab itself: the block's own sentence, and that a press brings
 *  it forward. A block that keeps its words above is folded only beside the cover. */
export function tabTitle(b: Block, ctx: BlockContext): string {
  const say = blockTitle(b, ctx);
  if (!say || ctx.place?.role === 'tab') return say;
  return `${say} Folded here: press to bring it forward.`;
}

/** The chevron on a tab: it points at the open block, where a press brings this one. */
const CHEVRON = '<svg class="chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M7.5 2.5 4 6l3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';

/** The tabs of a block that lies behind another: beside each stretch that is covered, a filled
 *  ground in the show's colour with a chevron, and the show's name down it where the tab is
 *  tall enough. Its tooltip says the rest. */
function tabsHtml(b: Block, ctx: BlockContext): string {
  const tabs = ctx.place?.tabs ?? [];
  if (!tabs.length) return '';
  const title = esc(tabTitle(b, ctx));
  return tabs
    .map((t) => `<span class="ftab${t.height < TAB_NAME_ROOM ? ' bare' : ''}${ctx.place?.narrow ? ' out' : ''}" style="top:${Math.round(t.top)}px;height:${Math.round(t.height)}px"${title ? ` title="${title}"` : ''}>${CHEVRON}<span class="fn">${esc(b.show)}</span></span>`)
    .join('');
}

/** The name written apart from the block's words, in each stretch the fold names. */
function labelsHtml(b: Block, ctx: BlockContext): string {
  return (ctx.place?.labels ?? []).map((top) => `<span class="lbl" style="top:${Math.round(top) + 3}px">${esc(b.show)}</span>`).join('');
}

/** The block's inner HTML: a column of the time row, the name and the rest, and after it the
 *  tabs of a block that lies behind another. A block that is all tab has no words: its tab, and
 *  its name wherever half an hour of it is clear. */
export function blockHtml(b: Block, ctx: BlockContext): string {
  const p = ctx.place;
  if (p?.role === 'tab') return `<div class="evb tabb">${labelsHtml(b, ctx)}</div>${tabsHtml(b, ctx)}`;
  return `<div class="evb${ctx.wide ? ' wide' : ''}">${blockInner(b, ctx)}</div>${p?.role === 'under' ? labelsHtml(b, ctx) : ''}${tabsHtml(b, ctx)}`;
}

/** The tab that stands for two or more shows hidden at one time. */
export function countHtml(n: number): string {
  return `<div class="evb tabb"><span class="plus">+${n}</span></div>`;
}

/** `2 more shows at this time: Listeners’ Choice, Record Fair Live. Press to open the day.` */
export function countTitle(blocks: readonly Pick<Block, 'show'>[]): string {
  const names = [...new Set(blocks.map((b) => b.show))];
  return `${names.length} more shows at this time: ${names.join(', ')}. Press to open the day.`;
}

/** `over ▪ The Tea Break`, with a square of the other show's colour; `over 2 shows`. In the
 *  Day view the overlap follows: `over ▪ Kettle Drive · 1 h`. */
function overLine(b: Block, ctx: BlockContext): string {
  const o = ctx.place?.others ?? [];
  if (!o.length) return '';
  if (o.length > 1) return `<span class="over">over ${o.length} shows</span>`;
  const sq = `<i style="--c:${o[0].colour ? `var(--cart-${o[0].colour})` : 'var(--deemph)'}"></i>`;
  const full = o[0] as Partial<Block>;
  const len = ctx.wide && typeof full.start === 'number' && typeof full.end === 'number' ? ` · ${lengthWords(Math.min(b.end, full.end) - Math.max(b.start, full.start))}` : '';
  return `<span class="over">over ${sq}${esc(o[0].show)}${len}</span>`;
}

function blockInner(b: Block, ctx: BlockContext): string {
  const past = b.end <= ctx.now;
  const live = b.start <= ctx.now && b.end > ctx.now;
  const time = `${hm(b.start, ctx.zone)}–${hm(b.end, ctx.zone)}`;
  const long = b.end - b.start;
  // The warning triangle leads the time row, where neither a one-hour block nor a narrow one
  // can cut it off; in the Day view the chip is always in view, so the row has none.
  const mark = b.conflict && !ctx.wide ? `<span class="cf">${glyph('g-fault')}</span>` : '';
  let h = `<span class="t"><span class="tl">${mark}<span class="num">${time}</span></span>`;
  if (b.kind === 'automation' && ctx.longNight) h += `<span class="rep">${lengthWords(long)}</span>`;
  else if (b.weekly && b.kind !== 'automation') h += `<span class="rep" title="a date from the show’s rule">${glyph('g-repeat')}</span>`;
  h += '</span>';
  if (b.kind === 'automation') {
    h += `<b>${esc(b.show)}</b><div class="rest"><span class="kind">${glyph('g-auto')}Automation</span>`;
    if (live) h += `<span class="flag onair">${glyph('g-onair')}On air</span>`;
    return h + '</div>';
  }
  h += `<b>${esc(b.show)}</b><div class="rest">`;
  if (b.conflict) h += `<span class="flag">${glyph('g-fault')}Conflict</span>${overLine(b, ctx)}`;
  if (b.kind === 'rule') {
    if (b.moved) h += `<span class="flag moved">${glyph('g-moved')}Moved · ${esc(b.moved)}</span>`;
    h += `<span class="kind">${glyph('g-repeat')}Not drafted</span><span class="ep">${b.number !== null ? `Ep ${b.number} · ` : ''}Select it to draft or skip</span>`;
    return h + '</div>';
  }
  if (b.moved) h += `<span class="flag moved">${glyph('g-moved')}Moved · ${esc(b.moved)}</span>`;
  // Live or pre-recorded (ADR-0188; issue 10: every block said Live).
  h += `<span class="kind">${b.prerecorded ? `${glyph('g-tape')}Pre-recorded` : `${glyph('g-mic')}Live`}${past ? ' · aired' : ''}</span>`;
  const ep = episodeLine(b);
  if (ep) h += `<span class="ep">${esc(ep)}</span>`;
  if (b.studio) h += `<span class="ep studio">${esc(b.studio)}</span>`;
  if (b.hosts.length) h += `<span class="who">${esc(b.hosts.join(', '))}</span>`;
  if (live) h += `<span class="flag onair">${glyph('g-onair')}On air</span>`;
  return h + '</div>';
}
