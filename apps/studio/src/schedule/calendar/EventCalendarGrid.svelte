<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The week grid drawn by Event Calendar (MIT, native Svelte 5): the adapter's default
  // (ADR-0021). The library places the blocks and drags them; every word and glyph inside a
  // block is ours (blockHtml.ts), and the now line is a background event at the station's
  // time, so a fixed clock and another zone draw the same. Times are floating:
  // the station's wall clock in the station's zone, whatever this machine's zone is.
  //
  // Where shows clash (week-clashes.md): the library would split the day into columns and draw
  // each block half over the next, so the model works out the fold itself (clash.ts) and this
  // adapter overrides where a clashing block sits. The library writes a block's
  // `inset-inline-start`, `inline-size` and `z-index` inline, and then appends the event's own
  // `style` to the same attribute (`BaseEvent.svelte`), so the later declarations here win with
  // no `!important`. One block is open at the day's width less the strip; the others lie behind
  // it and show in the strip as tabs (blockHtml.ts: a filled ground in the show's colour, a
  // chevron and the name); two or more hidden at one time are a count that opens the day.
  // With 10 rem or more for each lane (measured here against a rem probe, since the page is
  // zoomed) they stand side by side instead. The Day view is the library's `timeGridDay`, with
  // the week's seven days as a strip of buttons inside its one column head; a day's header in
  // the week is a button that opens that day.
  //
  // The library makes a block's element again whenever the events change (its chunk ids follow
  // the event objects), so nothing kept on an element outlives a selection or a minute: the
  // label, the tooltip and the listeners are set each time one mounts, the focus is put back by
  // the block's key, and the fold's swap of widths is animated from where the block was.
  import { tick, untrack } from 'svelte';
  import { Calendar, Interaction, ResourceTimeGrid, TimeGrid } from '@event-calendar/core';
  import '@event-calendar/core/index.css';
  import type { GridProps } from './types.ts';
  import { movable } from './types.ts';
  import { blockAria, blockClasses, blockHtml, blockTitle, countHtml, countTitle, esc, type BlockPlace } from '../blockHtml.ts';
  import { HOUR_MS, dayLength, dayWords, floating, fromFloating, hm, longDayWords, parts, type Block } from '../model.ts';
  import { foldWeek, tabStretches, type CountTab, type Placed } from '../clash.ts';
  import { remPx } from '../../lib/rem.ts';

  let { blocks, days, zone, now, selected, onSelect, onClear, onMove, onOpenDay, studios = null, view = 'week', day = 0, reveal = null }: GridProps = $props();

  /** The slot is half an hour and 24 px tall, in the week and in the day alike. */
  const SLOT = 24;
  /** The strip a folded block shows in: 1.5 rem, the target size (24 px) where a rem is 16 px. */
  const TAB_REM = 1.5;
  /** Lanes replace the fold when each has this much. */
  const LANE_REM = 10;
  /** Wall-clock time between two moments as the grid's pixels (a clocks-change night included). */
  const px = (from: number, to: number) => ((floating(to, zone).getTime() - floating(from, zone).getTime()) / HOUR_MS) * SLOT * 2;

  const todayIndex = $derived(days.findIndex((d, i) => i < 7 && now >= d && now < days[i + 1]));
  const ctx = $derived({ now, zone, selected });

  // ---- a day's width, in rem ----
  let host = $state<HTMLDivElement>();
  let dayRem = $state(0);
  function measure() {
    const col = host?.querySelector<HTMLElement>('.ec-body .ec-day');
    if (!col) return;
    const rem = Math.round((col.getBoundingClientRect().width / remPx()) * 10) / 10;
    if (rem > 0 && rem !== dayRem) dayRem = rem;
  }
  $effect(() => {
    const el = host;
    if (!el) return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    const raf = requestAnimationFrame(measure);
    return () => {
      ro.disconnect();
      cancelAnimationFrame(raf);
    };
  });
  $effect(() => {
    // The columns are the library's to draw: measured again once a view or a week has drawn.
    void view;
    void days[0];
    void tick().then(() => requestAnimationFrame(measure));
  });

  // ---- the fold ----
  const lanes = $derived(view === 'week' ? (n: number) => dayRem > 0 && dayRem / n >= LANE_REM : () => true);
  const folds = $derived(foldWeek(blocks, days, { selected, now, lanes }));
  const clashing = $derived(folds.map((f) => f.clashing));

  function placeOf(p: Placed): BlockPlace {
    return {
      role: p.role,
      narrow: p.narrow,
      others: p.others,
      labels: p.labels.map((off) => px(p.from, p.from + off)),
      // A tab stands beside each stretch that is covered; the block's own top border is a pixel.
      // A count on the strip takes the tab's place for its time.
      tabs: tabStretches(p, folds[p.day]?.counts ?? []).map(([a, b]) => ({ top: Math.max(0, px(p.from, a) - 1), height: px(a, b) })),
      // An hour's block has room for its time, its name and one tight line more.
      short: px(p.from, p.to) < SLOT * 2 + 6,
      tiny: px(p.from, p.to) < SLOT * 2 - 4,
    };
  }

  const colourOf = (b: Block) => (b.colour ? `var(--cart-${b.colour})` : 'var(--deemph)');
  const WIDE = '100% - var(--ec-event-col-gap)';

  function styleOf(p: Placed): string[] {
    const s = [`--show-colour:${colourOf(p.block)}`];
    if (p.role === 'lane') {
      s.push(`inset-inline-start:calc((${WIDE}) / ${p.cols} * ${p.col})`, `inline-size:calc((${WIDE}) / ${p.cols} - 2px)`, 'z-index:1');
    } else if (p.role !== 'plain') {
      s.push('inset-inline-start:0', `inline-size:calc(${WIDE}${p.narrow ? ` - ${TAB_REM}rem - 2px` : ''})`, `z-index:${p.z}`);
      if (p.role === 'under') s.push(`--clear:${Math.max(0, Math.round(px(p.from, p.from + p.clear)) - 2)}px`);
    }
    return s;
  }

  type Props = { block?: Block; key: string; place?: BlockPlace | null; count?: CountTab; day?: number };

  function eventOf(b: Block, p: Placed | null, whole: boolean) {
    const longNight = b.kind === 'automation' && b.start === days[6] && days[7] - days[6] !== 24 * 3_600_000;
    const place = p ? placeOf(p) : null;
    const c = { ...ctx, longNight, place, wide: view !== 'week' };
    const folded = place?.role === 'tab';
    return {
      id: whole ? b.id : p!.key,
      start: floating(whole ? b.start : p!.from, zone),
      end: floating(whole ? b.end : p!.to, zone),
      title: { html: blockHtml(b, c) },
      // A tab does not drag: the first press brings it forward, the next drags it. A part of a
      // block cut at midnight does not drag either: it is moved from its editor.
      editable: whole && !folded && movable(b, now),
      classNames: blockClasses(b, c),
      style: p ? styleOf(p) : [`--show-colour:${colourOf(b)}`],
      resourceIds: b.studio ? [b.studio] : [],
      extendedProps: { block: b, key: p?.key ?? b.id, place, day: p?.day } satisfies Props,
    };
  }

  function countOf(c: CountTab) {
    // The count's ground is the hidden shows' colours side by side, each as a tab's ground is.
    const n = c.blocks.length;
    const stripes = c.blocks.map((b, i) => `color-mix(in srgb, ${colourOf(b)} var(--fold-tint), var(--surface)) ${(i * 100) / n}% ${((i + 1) * 100) / n}%`).join(', ');
    return {
      id: c.key,
      start: floating(c.from, zone),
      end: floating(c.to, zone),
      title: { html: countHtml(c.blocks.length) },
      editable: false,
      classNames: ['ev', 'tab', 'count'],
      style: [`inset-inline-start:calc(${WIDE} - ${TAB_REM}rem)`, `inline-size:${TAB_REM}rem`, 'z-index:40', `--stripes:linear-gradient(90deg, ${stripes})`],
      resourceIds: [],
      extendedProps: { key: c.key, count: c, day: c.day } satisfies Props,
    };
  }

  /** The week's events. A block the fold touches on one day is one event, as ever. One that
   *  crosses midnight and clashes is an event for each day's part, because the library gives an
   *  event one set of classes and each part may fold its own way. */
  const blockEvents = $derived.by(() => {
    const by = new Map<string, Placed[]>();
    for (const f of folds) for (const p of f.placed) by.set(p.block.id, [...(by.get(p.block.id) ?? []), p]);
    const out = [];
    for (const b of blocks) {
      if (b.kind === 'automation') {
        out.push(eventOf(b, null, true));
        continue;
      }
      const placed = by.get(b.id) ?? [];
      const touched = days.slice(0, 7).filter((d, i) => b.start < days[i + 1] && b.end > d).length;
      // In the week this window does not hold, or wholly behind others and left to the count.
      if (!placed.length) continue;
      if (touched <= 1 || placed.every((p) => p.role === 'plain')) out.push(eventOf(b, placed[0], true));
      else for (const p of placed) out.push(eventOf(b, p, false));
    }
    for (const f of folds) for (const c of f.counts) out.push(countOf(c));
    return out;
  });

  const nowLine = $derived({
    id: 'now',
    start: floating(now, zone),
    end: floating(now + 60_000, zone),
    display: 'background' as const,
    classNames: ['now-line'],
    resourceIds: studios ?? [],
  });

  const events = $derived([...blockEvents, ...(todayIndex >= 0 ? [nowLine] : [])]);

  // ---- the heads ----
  const chip = (n: number) => (n ? `<span class="cl"><svg aria-hidden="true"><use href="#g-fault"/></svg>${n} clash</span>` : '');
  const clashWords = (n: number) => (n ? `, ${n} shows clash` : '');

  /** A day's header in the week: a button that opens the day, with the count of the shows that
   *  clash on it before its length. */
  function weekHead(i: number, ms: number): string {
    const long = longDayWords(ms, zone);
    const today = i === todayIndex ? '<span class="today-word"> · today</span>' : '';
    const dst = i >= 0 ? dayLength(days, i) : '';
    const n = clashing[i] ?? 0;
    return `<button type="button" class="dayhd" data-day="${i}" title="Open ${esc(long)}" aria-label="${esc(long)}${clashWords(n)}, open the day"><b>${esc(dayWords(ms, zone))}${today}</b><span class="dst">${chip(n)}<span>${esc(dst)}</span></span></button>`;
  }

  /** The Day view's head: the week's seven days as buttons, the open day pressed. */
  function strip(): string {
    let h = '<div class="strip" role="group" aria-label="The days of this week" data-panel="calendar.day.strip">';
    for (let i = 0; i < 7; i++) {
      const p = parts(days[i], zone);
      const long = longDayWords(days[i], zone);
      const n = clashing[i] ?? 0;
      const len = dayLength(days, i);
      const today = i === todayIndex;
      h += `<button type="button" data-day="${i}" aria-pressed="${i === day}" title="${i === day ? esc(long) : `Open ${esc(long)}`}" aria-label="${esc(long)}${today ? ', today' : ''}${clashWords(n)}${len === '24 h' ? '' : `, ${esc(len)}`}"><b>${esc(dayWords(days[i], zone).split(' ')[0])} ${p.d}</b>${today ? '<span class="today-word">today</span>' : ''}${chip(n)}${len === '24 h' ? '' : `<span class="dst">${esc(len)}</span>`}</button>`;
    }
    return h + '</div>';
  }

  /** A new function whenever what a head says changes, so the library draws the heads again. */
  const header = $derived.by(() => {
    const html = view === 'day' ? strip() : null;
    const counts = clashing;
    const today = todayIndex;
    return (d: Date) => {
      if (html !== null) return { html };
      const ms = fromFloating(d, zone);
      const i = days.findIndex((x, k) => k < 7 && ms >= x && ms < days[k + 1]);
      if (view === 'week' && i >= 0 && counts) return { html: weekHead(i, ms) };
      const word = i === today ? '<span class="today-word"> · today</span>' : '';
      return { html: `<b>${esc(dayWords(ms, zone))}${word}</b><span class="dst">${esc(i >= 0 ? dayLength(days, i) : '')}</span>` };
    };
  });

  /** A press on a day's header, or on a day of the strip. */
  function headClick(e: MouseEvent) {
    const b = (e.target as HTMLElement | null)?.closest<HTMLElement>('button[data-day]');
    if (!b || !host?.contains(b)) return;
    focusHead = true;
    onOpenDay?.(Number(b.dataset.day));
  }

  /** A press on the grid where no block is clears the selection. Automation fills every gap in
   *  time, so the empty grid is small: the gap the library keeps beside a day's blocks (the press
   *  lands on the day's own column), and in a window wider than the Day view's 80 rem the plane
   *  beside the day (it lands on the scroller). A press on a block, a tab or a count never comes
   *  here as one of those; the heads and the time axis are other elements; and the scroller's own
   *  scrollbars are left out by their share of its box, which holds at any zoom. */
  function gridPress(e: PointerEvent) {
    if (e.button !== 0 || !onClear) return;
    const t = e.target as HTMLElement | null;
    if (!t || !host?.contains(t)) return;
    if (t.classList.contains('ec-day')) return onClear();
    if (!t.classList.contains('ec-main')) return;
    const r = t.getBoundingClientRect();
    const inside = (at: number, size: number, client: number, offset: number) => size > 0 && offset > 0 && at / size < client / offset;
    if (inside(e.clientX - r.left, r.width, t.clientWidth, t.offsetWidth) && inside(e.clientY - r.top, r.height, t.clientHeight, t.offsetHeight)) onClear();
  }

  // ---- focus, scroll and the swap, across the library's redraws ----
  /** The block that had the focus before the events changed; its new element takes it back. */
  let refocus: string | null = null;
  /** A head was pressed: the head that stands for the same day takes the focus. */
  let focusHead = false;
  /** Whether each block left the strip free when it was last drawn, for the swap. */
  let wasNarrow = new Map<string, boolean>();
  $effect.pre(() => {
    void events;
    const a = document.activeElement as HTMLElement | null;
    refocus = a && host?.contains(a) && a.dataset.key ? a.dataset.key : null;
  });
  $effect(() => {
    void events;
    const next = new Map<string, boolean>();
    for (const f of untrack(() => folds)) for (const p of f.placed) if (p.role !== 'plain' && p.role !== 'lane') next.set(p.key, p.narrow);
    // After the elements have mounted and looked at the old map.
    void tick().then(() => {
      wasNarrow = next;
      refocus = null;
    });
  });

  // The view changed: the grid keeps its place in the day (a show is as tall as it was a moment
  // ago), and the focus goes to the open day's button, or back to that day's header.
  let lastView = untrack(() => view);
  let lastDay = untrack(() => day);
  let keepScroll: number | null = null;
  $effect.pre(() => {
    if (view !== lastView) keepScroll = host?.querySelector('.ec-main')?.scrollTop ?? null;
  });
  $effect(() => {
    const changed = view !== lastView;
    const moved = day !== lastDay;
    lastView = view;
    lastDay = day;
    if (!changed && !(moved && focusHead)) return;
    const top = keepScroll;
    keepScroll = null;
    focusHead = false;
    void tick().then(() =>
      requestAnimationFrame(() => {
        const main = host?.querySelector<HTMLElement>('.ec-main');
        if (main && top !== null && changed) main.scrollTop = top;
        if (view === 'studios') return;
        const to = view === 'day' ? host?.querySelector<HTMLElement>('.strip button[aria-pressed="true"]') : host?.querySelector<HTMLElement>(`button.dayhd[data-day="${day}"]`);
        to?.focus({ preventScroll: true });
      }),
    );
  });

  // A block asked for from outside the grid (a row of a show's dates): scrolled into view.
  let revealed = 0;
  $effect(() => {
    const r = reveal;
    void events;
    if (!r || r.n === revealed) return;
    void tick().then(() =>
      requestAnimationFrame(() => {
        const main = host?.querySelector<HTMLElement>('.ec-main');
        const el = [...(host?.querySelectorAll<HTMLElement>('.ec-event[data-key]') ?? [])].find((x) => x.dataset.key === r.id || x.dataset.key?.startsWith(`${r.id}@`));
        if (!main || !el) return;
        revealed = r.n;
        const head = host?.querySelector<HTMLElement>('.ec-header')?.getBoundingClientRect().height ?? 0;
        const [m, e] = [main.getBoundingClientRect(), el.getBoundingClientRect()];
        const scale = m.height / main.clientHeight || 1;
        if (e.top < m.top + head) main.scrollTop -= (m.top + head - e.top) / scale + SLOT;
        else if (e.bottom > m.bottom) main.scrollTop += Math.min(e.bottom - m.bottom, e.top - m.top - head) / scale + SLOT;
      }),
    );
  });

  const still = () => globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

  function mounted(info: { el: HTMLElement; event: { id: string | number; extendedProps?: Record<string, unknown> } }) {
    const x = (info.event.extendedProps ?? {}) as Partial<Props>;
    const { el } = info;
    if (String(info.event.id) === 'now') {
      const label = document.createElement('span');
      label.className = 'now-label num';
      label.textContent = hm(now, zone);
      el.appendChild(label);
      return;
    }
    if (!x.key) return;
    el.dataset.key = x.key;
    el.classList.add('selectable');
    const act = x.count ? () => onOpenDay?.(x.count!.day) : () => onSelect(x.block!.id);
    if (x.count) {
      const say = countTitle(x.count.blocks);
      el.setAttribute('aria-label', say);
      el.title = say;
    } else if (x.block) {
      const c = { ...ctx, place: x.place ?? null };
      el.setAttribute('aria-label', blockAria(x.block, c));
      const title = blockTitle(x.block, c);
      if (title) el.title = title;
    }
    // A block selects on a press and on Enter, as well as through the library's eventClick: in
    // the packaged app (WebKitGTK) a click on a block that can be dragged did not reach
    // eventClick (the operator's 1.0.2 run), and nothing showed that it could. On the press, not
    // the click: the grid's events are rebuilt as the clock ticks, so a block's element can be
    // replaced between the press and the release, and then the browser fires no click at all.
    // A drag still starts from the same press.
    el.addEventListener('pointerdown', (e: PointerEvent) => {
      if (e.button === 0) act();
    });
    el.addEventListener('keydown', (e: KeyboardEvent) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        e.stopPropagation();
        act();
      }
    });
    if (refocus === x.key) el.focus({ preventScroll: true });
    // The swap: a block that was folded and is now open, or the other way, takes 160 ms to its
    // new width. Nothing under reduced motion.
    const was = wasNarrow.get(x.key);
    const narrow = el.classList.contains('narrow');
    if (was !== undefined && was !== narrow && !x.count && !still() && typeof el.animate === 'function') {
      const w = el.offsetWidth;
      const gap = TAB_REM * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16) + 2;
      el.animate([{ inlineSize: `${narrow ? w + gap : w - gap}px` }, { inlineSize: `${w}px` }], { duration: 160, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)' });
    }
  }

  /** The block an event stands for: a part of a block cut at midnight names its block. */
  const blockId = (id: string | number) => String(id).replace(/@\d$/, '');

  const options = $derived({
    view: view === 'studios' ? 'resourceTimeGridDay' : view === 'day' ? 'timeGridDay' : 'timeGridWeek',
    date: floating(view === 'week' ? days[0] : days[Math.min(6, Math.max(0, day))], zone),
    firstDay: 1 as const,
    headerToolbar: { start: '', center: '', end: '' },
    allDaySlot: false,
    slotDuration: '00:30:00',
    slotHeight: SLOT,
    scrollTime: '06:30:00',
    nowIndicator: false,
    height: '100%',
    // Every clashing block is placed here (the fold in the week, lanes in the day), so the
    // library's own overlap, each block half over the next, is never what is seen.
    slotEventOverlap: false,
    // The day is at most 80 rem wide beside the axis; a wider window leaves the rest to the plane.
    columnWidth: view === 'day' ? 'minmax(0, 80rem)' : undefined,
    eventStartEditable: true,
    eventDurationEditable: true,
    events,
    resources: view === 'studios' && studios ? studios.map((s) => ({ id: s, title: s })) : [],
    filterEventsWithResources: view === 'studios',
    highlightedDates: todayIndex >= 0 ? [floating(days[todayIndex], zone)] : [],
    dayHeaderFormat: header,
    slotLabelFormat: { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' } as Intl.DateTimeFormatOptions,
    eventContent: () => undefined,
    eventDidMount: mounted,
    eventClick: (info: { event: { id: string | number; extendedProps?: Record<string, unknown> } }) => {
      const x = (info.event.extendedProps ?? {}) as Partial<Props>;
      if (x.count) onOpenDay?.(x.count.day);
      else onSelect(blockId(info.event.id));
    },
    eventDrop: (info: { event: { id: string | number; start: Date; end: Date }; revert: () => void }) => {
      void onMove(blockId(info.event.id), fromFloating(info.event.start, zone), fromFloating(info.event.end, zone)).then((why) => {
        if (why) info.revert();
      });
    },
    eventResize: (info: { event: { id: string | number; start: Date; end: Date }; revert: () => void }) => {
      void onMove(blockId(info.event.id), fromFloating(info.event.start, zone), fromFloating(info.event.end, zone)).then((why) => {
        if (why) info.revert();
      });
    },
  });
</script>

<!-- The heads are buttons inside the library's own header, so one listener here hears them all. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="ec-host" class:dayview={view === 'day'} data-grid="event-calendar" data-view={view} data-day-rem={dayRem || undefined} bind:this={host} onclick={headClick} onpointerdown={gridPress}>
  <Calendar plugins={[TimeGrid, ResourceTimeGrid, Interaction]} {options} />
</div>

<style>
  .ec-host {
    min-height: 0;
    height: 100%;
  }
</style>
