<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The week grid drawn by Event Calendar (MIT, native Svelte 5): the adapter's default
  // (ADR-0021). The library places the blocks and drags them; every word and glyph inside a
  // block is ours (blockHtml.ts), and the now line is a background event at the station's
  // time, so a fixed clock and another zone draw the same. Times are floating:
  // the station's wall clock in the station's zone, whatever this machine's zone is.
  import { Calendar, Interaction, ResourceTimeGrid, TimeGrid } from '@event-calendar/core';
  import '@event-calendar/core/index.css';
  import type { GridProps } from './types.ts';
  import { movable } from './types.ts';
  import { blockAria, blockClasses, blockHtml, esc } from '../blockHtml.ts';
  import { dayLength, dayWords, floating, fromFloating, hm, parts, type Block } from '../model.ts';

  let { blocks, days, zone, now, selected, onSelect, onMove, studios = null, view = 'week', day = 0 }: GridProps = $props();

  const todayIndex = $derived(days.findIndex((d, i) => i < 7 && now >= d && now < days[i + 1]));
  const ctx = $derived({ now, zone, selected });

  function event(b: Block) {
    const longNight = b.kind === 'automation' && b.start === days[6] && days[7] - days[6] !== 24 * 3_600_000;
    return {
      id: b.id,
      start: floating(b.start, zone),
      end: floating(b.end, zone),
      title: { html: blockHtml(b, { ...ctx, longNight }) },
      editable: movable(b, now),
      classNames: blockClasses(b, ctx),
      resourceIds: b.studio ? [b.studio] : [],
      extendedProps: { block: b },
    };
  }

  const nowLine = $derived({
    id: 'now',
    start: floating(now, zone),
    end: floating(now + 60_000, zone),
    display: 'background' as const,
    classNames: ['now-line'],
    resourceIds: studios ?? [],
  });

  const events = $derived([...blocks.map(event), ...(todayIndex >= 0 ? [nowLine] : [])]);

  function header(d: Date) {
    const ms = fromFloating(d, zone);
    const i = days.findIndex((x, k) => k < 7 && ms >= x && ms < days[k + 1]);
    const label = dayWords(ms, zone);
    const today = i === todayIndex ? '<span class="today-word"> · today</span>' : '';
    const dst = i >= 0 ? dayLength(days, i) : '';
    return { html: `<b>${esc(label)}${today}</b><span class="dst">${esc(dst)}</span>` };
  }

  const options = $derived({
    view: view === 'studios' ? 'resourceTimeGridDay' : 'timeGridWeek',
    date: floating(view === 'studios' ? days[day] : days[0], zone),
    firstDay: 1 as const,
    headerToolbar: { start: '', center: '', end: '' },
    allDaySlot: false,
    slotDuration: '00:30:00',
    slotHeight: 24,
    scrollTime: '06:30:00',
    nowIndicator: false,
    height: '100%',
    slotEventOverlap: true,
    eventStartEditable: true,
    eventDurationEditable: true,
    events,
    resources: view === 'studios' && studios ? studios.map((s) => ({ id: s, title: s })) : [],
    filterEventsWithResources: view === 'studios',
    highlightedDates: todayIndex >= 0 ? [floating(days[todayIndex], zone)] : [],
    dayHeaderFormat: header,
    slotLabelFormat: { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' } as Intl.DateTimeFormatOptions,
    eventContent: () => undefined,
    eventDidMount: (info: { el: HTMLElement; event: { id: string | number; extendedProps?: Record<string, unknown> } }) => {
      const b = info.event.extendedProps?.block as Block | undefined;
      if (b) info.el.setAttribute('aria-label', blockAria(b, ctx));
      if (b) {
        // A block selects on a press and on Enter, as well as through the library's
        // eventClick: in the packaged app (WebKitGTK) a click on a block that can be dragged did
        // not reach eventClick (the operator's 1.0.2 run), and nothing showed that it could.
        const id = String(info.event.id);
        info.el.classList.add('selectable');
        // On the press, not the click: the grid's events are rebuilt as the clock ticks, so a
        // block's element can be replaced between the press and the release, and then the browser
        // fires no click at all. A drag still starts from the same press.
        info.el.addEventListener('pointerdown', (e: PointerEvent) => {
          if (e.button === 0) onSelect(id);
        });
        info.el.addEventListener('keydown', (e: KeyboardEvent) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            onSelect(id);
          }
        });
      }
      if (String(info.event.id) === 'now') {
        const label = document.createElement('span');
        label.className = 'now-label num';
        label.textContent = hm(now, zone);
        info.el.appendChild(label);
      }
    },
    eventClick: (info: { event: { id: string | number } }) => onSelect(String(info.event.id)),
    eventDrop: (info: { event: { id: string | number; start: Date; end: Date }; revert: () => void }) => {
      void onMove(String(info.event.id), fromFloating(info.event.start, zone), fromFloating(info.event.end, zone)).then((why) => {
        if (why) info.revert();
      });
    },
    eventResize: (info: { event: { id: string | number; start: Date; end: Date }; revert: () => void }) => {
      void onMove(String(info.event.id), fromFloating(info.event.start, zone), fromFloating(info.event.end, zone)).then((why) => {
        if (why) info.revert();
      });
    },
  });

  // `parts` is imported for the adapter's callers that want it typed from here.
  void parts;
</script>

<div class="ec-host" data-grid="event-calendar">
  <Calendar plugins={[TimeGrid, ResourceTimeGrid, Interaction]} {options} />
</div>

<style>
  .ec-host :global(.ec-event.selectable) {
    cursor: pointer;
  }
  .ec-host :global(.ec-event.selectable:hover) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .ec-host {
    min-height: 0;
    height: 100%;
  }
</style>
