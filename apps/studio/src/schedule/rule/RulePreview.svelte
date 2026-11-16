<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // What this makes (`calendar.rule.preview`; new-show.md sections 1, 4 and 6): the rule said
  // back as one sentence (`calendar.rule.summary`), the small months, and the next dates as
  // rows, each with Skip and Move…. Every date, state, clash and word of the sentence is the
  // control plane's; this draws them. The small months are one stop for Tab and a grid inside
  // (the arrows a day, PgUp and PgDn a month, Home and End the week's ends, ⏎ to the date's
  // row, Space to pick a chosen date); the rows are one stop, ↑ ↓ inside, S to skip or put
  // back and M to move, on the focused row only.
  import { tick } from 'svelte';
  import type { IsoDate } from '../../lib/api.ts';
  import { WEEKDAYS, WEEKDAYS_SHORT, addDays, addMonths, hhmm, minutesOf, weekdayOf, type RuleForm } from './form.ts';
  import type { PreviewState } from './previewer.ts';
  import { afterWords, boundsWords, clashNotice, live, months as monthsOf, moveBounds, rows as rowsOf } from './view.ts';
  import { edges } from './edges.ts';
  import TimeField from '../TimeField.svelte';

  let {
    state: st,
    form,
    zone,
    today,
    colour = null,
    dropped = '',
    mayRefuse = false,
    onskip,
    onputback,
    onmove,
    onunmove,
    onpick,
    onmore,
    ontry,
    onretry,
    moving: movingFirst = null,
  }: {
    state: PreviewState;
    form: RuleForm;
    zone: string;
    /** The station's date today. */
    today: IsoDate;
    /** The show's bank slot, for the band on the sentence and the rows. */
    colour?: number | null;
    /** `1 skipped date no longer applies.`, said once after the pattern changed. */
    dropped?: string;
    /** This session is one the station refuses a clash: a later date's clash is worded for it. */
    mayRefuse?: boolean;
    onskip: (on: IsoDate) => void;
    onputback: (on: IsoDate) => void;
    onmove: (on: IsoDate, to: IsoDate, at: string) => void;
    onunmove: (on: IsoDate) => void;
    /** A date added to or taken from the chosen dates. */
    onpick: (iso: IsoDate) => void;
    onmore: () => void;
    /** The offered clear time taken: a start, minutes after midnight. */
    ontry: (start: number) => void;
    onretry: () => void;
    /** The row whose Move… is open (a test's). */
    moving?: IsoDate | null;
  } = $props();

  const answer = $derived(st.answer);
  const rows = $derived(answer ? rowsOf(answer, zone, today, form.from) : []);
  const airs = $derived(answer ? live(answer) : []);
  const grid = $derived(monthsOf(answer, form, zone, today));
  const notice = $derived(answer ? clashNotice(answer, zone, mayRefuse) : null);
  const after = $derived(answer ? afterWords(answer, form) : null);
  const picking = $derived(form.kind === 'dates');
  const once = $derived(form.kind === 'once');
  const anySkip = $derived(rows.some((r) => r.state === 'skipped'));
  const anyClash = $derived(rows.some((r) => r.clash));
  const anyFilled = $derived(rows.some((r) => r.state === 'filled'));
  /** The station's problems no field of the questions shows. */
  const loose = $derived((answer?.problems ?? []).filter((p) => !['starts_on', 'first', 'start', 'minutes', 'repeat', 'end', 'moves'].includes(p.field)));
  /** Why there are no dates, without the heading said twice (the station may lead with it). */
  const why = $derived((answer?.problems ?? []).filter((p) => p.field !== 'moves').map((p) => p.message.replace(/^This makes no dates\.\s*/, '')).filter(Boolean));

  // ---- Move…: one row open at a time ----
  // svelte-ignore state_referenced_locally
  let moving = $state<IsoDate | null>(movingFirst);
  let moveDate = $state('');
  let moveTime = $state('');
  let moveErr = $state('');
  const bounds = $derived(answer && moving ? moveBounds(answer, moving, zone, today) : null);

  let host = $state<HTMLElement | null>(null);
  const rowEl = (on: IsoDate) => host?.querySelector<HTMLElement>(`[data-row="${on}"]`) ?? null;

  async function openMove(on: IsoDate) {
    const r = rows.find((x) => x.on === on);
    if (!r) return;
    moving = on;
    moveDate = r.air;
    moveTime = r.times.slice(0, 5);
    moveErr = '';
    await tick();
    host?.querySelector<HTMLInputElement>('#rp-mv-date')?.focus();
  }
  async function closeMove(on: IsoDate) {
    moving = null;
    await tick();
    rowEl(on)?.focus();
  }
  function doMove(on: IsoDate) {
    if (!moveDate || minutesOf(moveTime) === null) {
      moveErr = 'Give a date and a time, such as 20:00.';
      return;
    }
    // The neighbours shown say at once what the station would refuse; it is still the judge.
    if (bounds && (moveDate <= bounds.after || (bounds.before !== null && moveDate >= bounds.before))) {
      moveErr = boundsWords(bounds);
      return;
    }
    onmove(on, moveDate, hhmm(minutesOf(moveTime)!));
    void closeMove(on);
  }

  // ---- the rows: one stop, the arrows inside ----
  // Tab stops at one row, then at that row's own buttons, and goes on: ten rows are not twenty
  // stops between the questions and Make the show. The other rows' buttons are reached with
  // ↑ ↓ and Tab, or with S and M on the row.
  let rowFocus = $state<IsoDate | null>(null);
  const rowStop = $derived(rows.some((r) => r.on === rowFocus) ? rowFocus : (rows[0]?.on ?? null));

  /** A row's own button was pressed, and the button will be replaced by what it did (Skip by
   *  Put back) once the station answers: the focus goes to its row first, which stays, so it
   *  is not dropped out of the dialog. */
  function onRow(on: IsoDate, act: (on: IsoDate) => void) {
    rowEl(on)?.focus();
    act(on);
  }
  /** Remove on a chosen date's row: the row goes, so the focus moves to the row that takes its
   *  place (the next, else the one before), else to the calendar. */
  function removeRow(on: IsoDate) {
    const i = rows.findIndex((r) => r.on === on);
    const near = rows[i + 1] ?? rows[i - 1];
    (near ? rowEl(near.on) : host?.querySelector<HTMLElement>('.months [role="gridcell"][tabindex="0"]'))?.focus();
    onpick(on);
  }
  function rowKeys(e: KeyboardEvent, on: IsoDate) {
    if (e.target !== e.currentTarget || e.ctrlKey || e.metaKey || e.altKey) return;
    const i = rows.findIndex((r) => r.on === on);
    const r = rows[i];
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const next = rows[i + (e.key === 'ArrowDown' ? 1 : -1)];
      if (next) rowEl(next.on)?.focus();
    } else if (e.key === 's' || e.key === 'S') {
      if (r.state === 'skipped') onputback(on);
      else if (r.state === 'airs' && !once && !picking) onskip(on);
      else return;
    } else if (e.key === 'm' || e.key === 'M') {
      if (r.state === 'airs' && !r.moved && !once) void openMove(on);
      else return;
    } else return;
    e.preventDefault();
  }

  // ---- the small months: one stop, a grid inside ----
  let dayFocus = $state<IsoDate | null>(null);
  const drawn = $derived(new Set(grid.flatMap((m) => m.days.map((d) => d.iso))));
  const dayStop = $derived(dayFocus && drawn.has(dayFocus) ? dayFocus : (grid.flatMap((m) => m.days).find((d) => d.state === 'air' || d.state === 'clash')?.iso ?? grid[0]?.days[0]?.iso ?? null));
  function dayKeys(e: KeyboardEvent, iso: IsoDate, row: IsoDate | null) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    let to: IsoDate | null = null;
    if (e.key === 'ArrowLeft') to = addDays(iso, -1);
    else if (e.key === 'ArrowRight') to = addDays(iso, 1);
    else if (e.key === 'ArrowUp') to = addDays(iso, -7);
    else if (e.key === 'ArrowDown') to = addDays(iso, 7);
    else if (e.key === 'PageUp') to = addMonths(iso, -1);
    else if (e.key === 'PageDown') to = addMonths(iso, 1);
    else if (e.key === 'Home') to = addDays(iso, 1 - weekdayOf(iso));
    else if (e.key === 'End') to = addDays(iso, 7 - weekdayOf(iso));
    else if (e.key === 'Enter' && row) {
      e.preventDefault();
      rowEl(row)?.focus();
      return;
    } else if (e.key === ' ' && picking) {
      e.preventDefault();
      onpick(iso);
      return;
    } else return;
    e.preventDefault();
    // A date in a month that is not drawn is not reached: the months shown are the ones that matter.
    if (to && drawn.has(to)) {
      dayFocus = to;
      host?.querySelector<HTMLElement>(`[data-day="${to}"]`)?.focus();
    }
  }
</script>

<section
  bind:this={host}
  use:edges
  class="preview"
  class:stale={st.stale && !st.unreachable}
  data-panel="calendar.rule.preview"
  aria-label="What this makes"
  style:--show-colour={colour ? `var(--cart-${colour})` : undefined}
>
  <h3 data-panel-title>What this makes</h3>

  {#if st.unreachable}
    <div class="warn plain" role="alert">
      <div class="top"><svg aria-hidden="true"><use href="#g-degraded" /></svg><span>The station cannot be reached, so the dates cannot be shown.</span></div>
      <span>Your answers are kept.</span>
      <button type="button" class="btn sm" onclick={onretry}>Try again</button>
    </div>
  {:else if !answer}
    <div class="summary wait" data-panel="calendar.rule.summary" role="status" aria-live="polite">
      <p class="say" data-panel-title>{st.stale ? 'Working out the dates…' : 'Pick a date and a time, and the next dates appear here.'}</p>
    </div>
  {:else if !airs.length}
    <div class="summary none" data-panel="calendar.rule.summary" role="status" aria-live="polite">
      <p class="say" data-panel-title>This makes no dates.</p>
      {#each why as m (m)}<p class="last">{m}</p>{:else}{#if !answer.problems.length}<p class="last">Pick a date still to come.</p>{/if}{/each}
      {#if dropped}<p class="dropped" role="status">{dropped}</p>{/if}
    </div>
  {:else}
    <div class="summary" data-panel="calendar.rule.summary" role="status" aria-live="polite">
      <p class="say" data-panel-title>{answer.sentence}</p>
      {#if answer.last && !once}<p class="last">{answer.last}</p>{/if}
    </div>
    {#if st.working}<p class="working" role="status">Working out the dates…</p>{/if}
    {#if dropped}<p class="dropped" role="status">{dropped}</p>{/if}
    {#each loose as p (p.message)}<p class="err" role="alert">{p.message}</p>{/each}

    {#if notice}
      <div class="warn" class:refused={notice.kind === 'refused'} role={notice.kind === 'refused' ? 'alert' : 'status'} data-clash={notice.kind}>
        <div class="top"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{notice.title}</span></div>
        <span>{notice.body}</span>
        {#if notice.offer}<button type="button" class="btn sm" onclick={() => ontry(notice.offer!.start)}>{notice.offer.label}</button>{/if}
      </div>
    {/if}
  {/if}

  {#if !st.unreachable && (answer || picking)}
    <div class="months" class:pick={picking} role="group" aria-label={picking ? 'Pick the dates: Space adds or removes one' : 'The next dates in the calendar'}>
      {#each grid as m (m.caption)}
        <div class="month">
          <div class="cap" id="rp-m-{m.y}-{m.m}">{m.caption}</div>
          <div class="grid" role="grid" aria-labelledby="rp-m-{m.y}-{m.m}">
            <div role="row">
              {#each WEEKDAYS_SHORT as w, i (w)}<span class="h" role="columnheader" aria-label={WEEKDAYS[i]}>{w.charAt(0)}</span>{/each}
            </div>
            {#each Array.from({ length: Math.ceil((m.lead + m.days.length) / 7) }, (_, w) => w) as w (w)}
              <div role="row">
                {#each Array.from({ length: 7 }, (_, c) => w * 7 + c - m.lead) as i (i)}
                  {@const d = m.days[i]}
                  {#if d}
                    {@const on = picking && form.dates.includes(d.iso)}
                    <span
                      class="d {picking ? (on ? 'air' : '') : d.state}"
                      class:today={d.today}
                      class:go={!picking && !!d.row}
                      role="gridcell"
                      tabindex={d.iso === dayStop ? 0 : -1}
                      data-day={d.iso}
                      aria-selected={picking ? on : undefined}
                      aria-label={picking ? `${d.label.split(':')[0]}${on ? ': chosen' : ''}` : d.label}
                      title={d.label}
                      onkeydown={(e) => dayKeys(e, d.iso, d.row)}
                      onfocus={() => (dayFocus = d.iso)}
                      onclick={() => { dayFocus = d.iso; if (picking) onpick(d.iso); else if (d.row) rowEl(d.row)?.focus(); }}
                    >{d.d}</span>
                  {:else}
                    <span role="gridcell"></span>
                  {/if}
                {/each}
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
    <div class="keyrow" aria-hidden="true">
      <span><i class="air"></i>{picking ? 'chosen' : 'airs'}</span>
      {#if anySkip}<span><i class="skip"></i>skipped</span>{/if}
      {#if anyClash}<span><i class="clash"></i>clashes</span>{/if}
      {#if anyFilled}<span><i class="filled"></i>an episode already</span>{/if}
    </div>
  {/if}

  {#if !st.unreachable && answer && rows.length}
    <h3 id="rp-nd">Next dates</h3>
    <ol class="nd" aria-labelledby="rp-nd">
      {#each rows as r (r.on)}
        {@const below = !!(r.clash || r.moved || r.clock || r.now)}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <li
          class="r"
          class:skipped={r.state === 'skipped'}
          class:clash={!!r.clash}
          class:drafted={r.state === 'filled'}
          data-row={r.on}
          tabindex={r.on === rowStop ? 0 : -1}
          onfocusin={() => (rowFocus = r.on)}
          onkeydown={(e) => rowKeys(e, r.on)}
        >
          <span class="n num">{r.ep}</span>
          <span class="on">{r.day}</span>
          <span class="t">{r.times}</span>
          {#if r.state === 'skipped'}
            <span class="st"><span class="flag skip"><svg aria-hidden="true"><use href="#g-muted" /></svg>Skipped</span></span>
            <span class="do"><button type="button" class="act2" tabindex={r.on === rowStop ? 0 : -1} onclick={() => onRow(r.on, onputback)}>Put back</button></span>
          {:else}
            {#if below || r.filled}
              <span class="st" class:below>
                {#if r.now}<span class="tag">under way now</span><span class="why">episode 1 will be this one</span>{/if}
                {#if r.filled}<span class="tag">{r.filled}</span>{/if}
                {#if r.moved}<span class="flag moved"><svg aria-hidden="true"><use href="#g-moved" /></svg>Moved · {r.moved}</span>{/if}
                {#if r.clash}<span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span><span class="why">{r.clash}</span>{/if}
                {#if r.clock}<span class="tag">{r.clock}</span>{/if}
              </span>
            {/if}
            <span class="do">
              {#if r.state === 'airs' && !once}
                {#if r.moved}
                  <button type="button" class="act2" tabindex={r.on === rowStop ? 0 : -1} onclick={() => onRow(r.on, onunmove)}>Back to the rule</button>
                {:else}
                  {#if !picking}<button type="button" class="act2" tabindex={r.on === rowStop ? 0 : -1} onclick={() => onRow(r.on, onskip)}>Skip</button>{:else}<button type="button" class="act2" tabindex={r.on === rowStop ? 0 : -1} aria-label="Remove {r.day}" onclick={() => removeRow(r.on)}>Remove</button>{/if}
                  <button type="button" class="act2" tabindex={r.on === rowStop ? 0 : -1} aria-expanded={moving === r.on} onclick={() => (moving === r.on ? closeMove(r.on) : openMove(r.on))}>Move…</button>
                {/if}
              {/if}
            </span>
          {/if}
        </li>
        {#if moving === r.on}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <li class="mv" onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); doMove(r.on); } else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); void closeMove(r.on); } }}>
            <label for="rp-mv-date">Move this date to</label>
            <input id="rp-mv-date" type="date" bind:value={moveDate} aria-invalid={moveErr ? true : undefined} aria-describedby="rp-mv-hint" />
            <span class="at"><label for="rp-mv-time">at</label> <TimeField id="rp-mv-time" bind:value={moveTime} oncarry={(d) => (moveDate = addDays(moveDate, d))} /></span>
            <button type="button" class="btn sm primary" onclick={() => doMove(r.on)}>Move</button>
            <button type="button" class="btn sm quiet" onclick={() => closeMove(r.on)}>Cancel</button>
            <!-- Refused for its neighbours, the reason is the hint's own sentence: said once, as the error. -->
            {#if moveErr}<p id={bounds && moveErr === boundsWords(bounds) ? 'rp-mv-hint' : undefined} class="err" role="alert">{moveErr}</p>{/if}
            {#if bounds && moveErr !== boundsWords(bounds)}<span id="rp-mv-hint" class="hint">{boundsWords(bounds)}</span>{/if}
          </li>
        {/if}
      {/each}
    </ol>
    {#if after}
      <p class="after">
        {#if after.more}<button type="button" class="btn sm" onclick={onmore}>Show more dates</button>{/if}
        {#if after.words}<span>{after.words}</span>{/if}
      </p>
    {/if}
  {/if}
</section>
