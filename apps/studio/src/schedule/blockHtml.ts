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

export interface BlockContext {
  now: number;
  zone: string;
  selected: string | null;
  /** The length is printed on an overnight block that is not 24 hours of a day (the clocks-back night). */
  longNight?: boolean;
}

/** The classes a block's element carries, for the library's `eventClassNames`. */
export function blockClasses(b: Block, ctx: BlockContext): string[] {
  const past = b.end <= ctx.now;
  const out = ['ev'];
  if (b.kind === 'automation') out.push('auto');
  if (b.kind === 'rule') out.push('rule');
  if (past) out.push('past');
  if (ctx.selected === b.id) out.push('sel');
  if (b.conflict) out.push('conflict');
  return out;
}

/** What a screen reader hears for the block. */
export function blockAria(b: Block, ctx: BlockContext): string {
  const when = `${hm(b.start, ctx.zone)} to ${hm(b.end, ctx.zone)}`;
  if (b.kind === 'automation') return `Automation, ${b.show}, ${when}`;
  if (b.kind === 'rule') return `${b.show}, weekly rule, ${when}, not drafted yet`;
  const parts = [b.show, 'live', when];
  const ep = episodeLine(b);
  if (ep) parts.push(ep);
  if (b.studio) parts.push(b.studio);
  if (b.end <= ctx.now) parts.push('aired');
  if (b.conflict) parts.push('conflict');
  if (b.moved) parts.push('moved this week');
  if (b.start <= ctx.now && b.end > ctx.now) parts.push('on air');
  return parts.join(', ');
}

/** The block's inner HTML: one root, a column of the time row, the name and the rest. */
export function blockHtml(b: Block, ctx: BlockContext): string {
  return `<div class="evb">${blockInner(b, ctx)}</div>`;
}

function blockInner(b: Block, ctx: BlockContext): string {
  const past = b.end <= ctx.now;
  const live = b.start <= ctx.now && b.end > ctx.now;
  const time = `${hm(b.start, ctx.zone)}–${hm(b.end, ctx.zone)}`;
  const long = b.end - b.start;
  let h = `<span class="t"><span class="num">${time}</span>`;
  if (b.kind === 'automation' && ctx.longNight) h += `<span class="rep">${lengthWords(long)}</span>`;
  else if (b.weekly && b.kind !== 'automation') h += `<span class="rep" title="instance of a weekly rule">${glyph('g-repeat')}</span>`;
  h += '</span>';
  if (b.kind === 'automation') {
    h += `<b>${esc(b.show)}</b><div class="rest"><span class="kind">${glyph('g-auto')}Automation</span>`;
    if (live) h += `<span class="flag onair">${glyph('g-onair')}On air</span>`;
    return h + '</div>';
  }
  h += `<b>${esc(b.show)}</b><div class="rest">`;
  if (b.kind === 'rule') {
    h += `<span class="kind">${glyph('g-repeat')}Not drafted</span><span class="ep">Weekly rule · draft it</span>`;
    return h + '</div>';
  }
  if (b.conflict) h += `<span class="flag">${glyph('g-fault')}Conflict</span>`;
  if (b.moved) h += `<span class="flag moved">${glyph('g-moved')}Moved · ${esc(b.moved)}</span>`;
  h += `<span class="kind">${glyph('g-mic')}Live${past ? ' · aired' : ''}</span>`;
  const ep = episodeLine(b);
  if (ep) h += `<span class="ep">${esc(ep)}</span>`;
  if (b.studio) h += `<span class="ep studio">${esc(b.studio)}</span>`;
  if (b.hosts.length) h += `<span class="who">${esc(b.hosts.join(', '))}</span>`;
  if (live) h += `<span class="flag onair">${glyph('g-onair')}On air</span>`;
  return h + '</div>';
}
