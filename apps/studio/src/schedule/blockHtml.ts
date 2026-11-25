// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One block's markup (calendar.md §2), as HTML for whichever library draws the grid: the time
// and the repeat glyph, the show, the flags (CONFLICT, MOVED, ON AIR), the kind as a word and a
// glyph, the episode line and the people. The grid library takes HTML for an event's content,
// so a different library would differ only in what it draws around it.

import { REHEARSAL, STUDIO, episodeLine, hm, lengthWords, type Block, type Rehearsal } from './model.ts';
import { KEYED } from '../lib/keyed.ts';
import { columnCaption, rehearsalKind, sessionWords } from './studios.ts';

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
  /** The Studios view (1.3.0): a show whose host is in their own studio says so, since the
   *  column cannot (every show on air is in the one column). */
  columns?: boolean;
  /** The shows kept for people with a key now, by path (keyed shows, 1.3.2). */
  keyed?: ReadonlySet<string>;
}

/** The show is kept for people with a key: its blocks carry the key beside the time. */
const isKeyed = (b: Block, ctx: BlockContext) => b.kind !== 'automation' && !!ctx.keyed?.has(b.slug);
/** The key beside a block's time, on every keyed block: who may listen, not a state of the air.
 *  The word stands with it wherever the time's line has the room (the Day view, a wide column);
 *  the styles decide (`.kw`). */
const keyedMark = () => `<span class="rep keyed" title="${esc(KEYED.tip)}">${glyph('g-key')}<span class="kw">${KEYED.word}</span></span>`;
/** And the word on a line of its own under the kind, in a column too narrow for it beside the
 *  time: left out whole, as every line is, where the block is too short (an hour's block keeps
 *  the key beside its time, and the tooltip). */
const keyedLine = (b: Block, ctx: BlockContext) => (isKeyed(b, ctx) && !ctx.wide ? `<span class="kind kk" title="${esc(KEYED.tip)}">${glyph('g-key')}${KEYED.word}</span>` : '');

/** `Own studio`, on a show whose host comes in with a show key: the desk's own mark for it. */
const OWN_STUDIO = 'Own studio';
const ownStudio = (b: Block, ctx: BlockContext) => !!ctx.columns && b.kind === 'episode' && b.from === 'own';

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
    const head = `${b.show}${isKeyed(b, ctx) ? ', keyed, for people with a key' : ''}, a date from its rule, ${when}, not drafted yet`;
    const tail = folded || 'select it to draft the episode or skip the date';
    return clash ? `${head}, ${clash}, ${tail}` : `${head}: ${tail}`;
  }
  const parts = [b.show, b.prerecorded ? 'pre-recorded' : 'live', when];
  const ep = episodeLine(b);
  if (ep) parts.push(ep);
  if (ownStudio(b, ctx)) parts.push('the host in their own studio');
  if (isKeyed(b, ctx)) parts.push('keyed, for people with a key');
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
  if (isKeyed(b, ctx)) h += keyedMark();
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
    h += `<span class="kind">${glyph('g-repeat')}Not drafted</span>${keyedLine(b, ctx)}<span class="ep">${b.number !== null ? `Ep ${b.number} · ` : ''}Select it to draft or skip</span>`;
    return h + '</div>';
  }
  if (b.moved) h += `<span class="flag moved">${glyph('g-moved')}Moved · ${esc(b.moved)}</span>`;
  // Live or pre-recorded (ADR-0188; issue 10: every block said Live).
  h += `<span class="kind">${b.prerecorded ? `${glyph('g-tape')}Pre-recorded` : `${glyph('g-mic')}Live`}${past ? ' · aired' : ''}</span>`;
  h += keyedLine(b, ctx);
  const ep = episodeLine(b);
  if (ep) h += `<span class="ep">${esc(ep)}</span>`;
  if (ownStudio(b, ctx)) h += `<span class="kind own" title="The host comes in from their own studio, with the show’s key">${glyph('g-disc')}${OWN_STUDIO}</span>`;
  if (b.hosts.length) h += `<span class="who">${esc(b.hosts.join(', '))}</span>`;
  if (live) h += `<span class="flag onair">${glyph('g-onair')}On air</span>`;
  return h + '</div>';
}

// ---- the Studios view (1.3.0; rehearsal-mode-build-plan.md section 2) ----
// The Rehearsal column's blocks are not shows on air and are drawn apart from them: a booking
// is the show's colour band on a hatched ground, a session a plain one, muted once it is over.
// Nothing here is a lamp, nothing is red, and no word of it is ON AIR or OPEN (ADR-0062).

/** A place held: a dashed square. A place used: the square with something in it. */
const HELD = '<svg viewBox="0 0 14 14" aria-hidden="true"><rect x="2" y="2" width="10" height="10" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="2.6 1.9"/></svg>';
const USED = '<svg viewBox="0 0 14 14" aria-hidden="true"><rect x="2" y="2" width="10" height="10" rx="2" fill="none" stroke="currentColor" stroke-width="1.5"/><rect x="5" y="5" width="4" height="4" rx="1" fill="currentColor"/></svg>';

export interface RehearsalContext {
  now: number;
  zone: string;
  selected: string | null;
  /** Under an hour on the grid: the time and the name on one line, and no more. */
  tiny?: boolean;
  /** An hour or less: no second row of words. */
  short?: boolean;
  /** The booking being made, not yet at the station. */
  draft?: boolean;
}

/** `13:30–14:00`; a session still open has no end yet. */
const rehearsalTime = (r: Rehearsal, zone: string) => (r.open ? `${hm(r.start, zone)}–now` : `${hm(r.start, zone)}–${hm(r.end, zone)}`);

export function rehearsalClasses(r: Rehearsal, ctx: RehearsalContext): string[] {
  const out = ['ev', 'reh', r.kind === 'booking' ? 'booking' : 'session'];
  if (r.past) out.push('past');
  if (r.open) out.push('open');
  if (ctx.draft) out.push('draft');
  if (ctx.selected === r.id) out.push('sel');
  if (ctx.short) out.push('short');
  if (ctx.tiny) out.push('tiny');
  return out;
}

/** A block of the Rehearsal column: the time, the show, what it is and who by. One under an
 *  hour tall has a single line, so what it is stands on that line after the name, and who by
 *  is left to its tooltip. */
export function rehearsalHtml(r: Rehearsal, ctx: RehearsalContext): string {
  const kind = `<span class="kind${ctx.tiny ? ' inl' : ''}">${r.kind === 'booking' ? HELD : USED}${esc(ctx.draft ? 'New booking' : rehearsalKind(r))}</span>`;
  const by = ctx.draft || !r.by ? '' : `<span class="who">by ${esc(r.by)}</span>`;
  const head = `<div class="evb wide"><span class="t"><span class="tl"><span class="num">${rehearsalTime(r, ctx.zone)}</span></span></span><b>${esc(r.show || 'No show yet')}</b>`;
  return ctx.tiny ? `${head}${kind}</div>` : `${head}<div class="rest">${kind}${by}</div></div>`;
}

/** What a screen reader hears for it. Never "on air": nothing in this column is. */
export function rehearsalAria(r: Rehearsal, ctx: RehearsalContext): string {
  if (ctx.draft) return `A new booking of the rehearsal studio${r.show ? ` for ${r.show}` : ''}, ${hm(r.start, ctx.zone)} to ${hm(r.end, ctx.zone)}, not saved yet`;
  if (r.kind === 'booking') return `${r.show}, the rehearsal studio ${r.past ? 'was booked' : 'booked'}, ${hm(r.start, ctx.zone)} to ${hm(r.end, ctx.zone)}${r.by ? `, by ${r.by}` : ''}`;
  return `${r.show}, a rehearsal${r.open ? ' under way' : ''}. ${sessionWords(r, ctx.zone)}`;
}

/** A column's head in the Studios view: its name, what it is, and for the Rehearsal column the
 *  way to book it without a drag (`book`), or why not (`bookWhy`). */
export function studioHeadHtml(col: { id: string; name: string }, o: { book?: boolean; bookWhy?: string } = {}): string {
  const cap = columnCaption(col.id);
  let h = `<div class="studiohd" data-studio="${esc(col.id)}"><span class="nm"><b>${esc(col.name)}</b>${cap ? `<span class="cap">${esc(cap)}</span>` : ''}</span>`;
  if (col.id === REHEARSAL) {
    if (o.book) h += `<button type="button" class="act2 bookbtn" data-book aria-label="Book the rehearsal studio…" title="Hold the rehearsal studio for a show. Or drag across empty time in this column.">Book…</button>`;
    else if (o.bookWhy) h += `<span class="ro" title="${esc(o.bookWhy)}">${glyph('g-lock')}To read</span>`;
  }
  return h + '</div>';
}

/** Which column a block of the week stands in: everything on air is in the first. */
export const columnOf = (b: Pick<Block, 'studio'>): string => b.studio ?? STUDIO;
