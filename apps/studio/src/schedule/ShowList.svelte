<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The list of shows (`calendar.shows`; week-clashes.md section 3): the editor column with
  // nothing open. Every show with dates to come, by name, each with its rule in short and its
  // next date; a row opens that show at its Dates. Under them the two groups that draw nothing
  // in the week (`calendar.unscheduled`: no dates yet, and finished), and the help text. Above
  // twelve shows a field finds one by its name. The rows are one stop for Tab: ↑ ↓, Home and
  // End move between them, and ⏎ opens the show. Each row ends with the tick that makes the
  // show keyed (KeyedTick, 1.3.2; the operator, 2026-11-25: "pick a show … and tick it"): a
  // producer's and an admin's, one more stop for Tab on the row that has the focus. Anyone
  // else sees the word Keyed, with the key, on a show kept for people with a key.
  import type { Show } from '../lib/schedule.ts';
  import Unscheduled from './rule/Unscheduled.svelte';
  import { dateWords } from './rule/form.ts';
  import { unscheduled } from './rule/view.ts';
  import { listRows, showCounts } from './shows.ts';
  import KeyedTick from '../keys/KeyedTick.svelte';

  let {
    shows,
    rules = true,
    today,
    onopen,
    ondates,
    ondraft,
    keyed = undefined,
  }: {
    /** The shows kept for people with a key now, by path (keyed shows, 1.3.2). */
    keyed?: ReadonlySet<string>;
    shows: readonly Show[];
    /** The station stores rules; an older one sends no dates for the week. */
    rules?: boolean;
    /** The station's date today. */
    today: string;
    /** A show's row, or its name in a group below: its pane, at Dates. */
    onopen: (show: Show) => void;
    ondates: (show: Show) => void;
    ondraft: (show: Show) => void;
  } = $props();

  let find = $state('');
  const all = $derived(listRows(shows, rules));
  const rows = $derived(find.trim() ? all.filter((s) => s.name.toLowerCase().includes(find.trim().toLowerCase())) : all);
  const u = $derived(unscheduled(shows, rules));
  const counts = $derived(showCounts(all.length, u));

  const next = (s: Show) => (s.next_on?.on ? (s.next_on.on === today ? 'today' : dateWords(s.next_on.on, { weekday: 'short', month: 'short' })) : '');

  // One stop for Tab, the arrows inside.
  let focused = $state<string | null>(null);
  const stop = $derived(rows.some((s) => s.slug === focused) ? focused : (rows[0]?.slug ?? null));
  let list = $state<HTMLUListElement | null>(null);
  function keys(e: KeyboardEvent, i: number) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1 : -1;
    if (to < 0 || to >= rows.length || (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End')) return;
    e.preventDefault();
    e.stopPropagation();
    list?.querySelector<HTMLElement>(`[data-show="${rows[to].slug}"]`)?.focus();
  }
</script>

<div class="head">
  <h2 data-panel-title>Shows</h2>
  <div class="when">{counts}</div>
</div>
<div class="fields">
  {#if all.length > 12}
    <label class="findshow">
      <span class="sr-only">Find a show</span>
      <input type="search" placeholder="Find a show" bind:value={find} />
    </label>
  {/if}
  {#if rows.length}
    <ul class="showlist" bind:this={list} aria-label="Shows with dates to come">
      {#each rows as s, i (s.slug)}
        <li class="showitem">
          <button
            type="button"
            class="showrow"
            data-show={s.slug}
            tabindex={s.slug === stop ? 0 : -1}
            style:--show-colour="var(--cart-{s.colour_slot})"
            onfocus={() => (focused = s.slug)}
            onkeydown={(e) => keys(e, i)}
            onclick={() => onopen(s)}
          >
            <b>{s.name}{#if keyed?.has(s.slug)}<span class="sr-only">, keyed</span>{/if}</b>
            <span><span class="num">{s.rule_words?.short ?? ''}</span>{#if next(s)}{s.rule_words?.short ? ' · ' : ''}next {next(s)}{/if}</span>
          </button>
          <!-- The one control that makes a show keyed (a producer's and an admin's); anyone else sees the mark. -->
          <KeyedTick slug={s.slug} name={s.name} id="sl-{s.slug}" compact tabindex={s.slug === stop ? 0 : -1} />
        </li>
      {/each}
    </ul>
  {:else if find.trim()}
    <p class="caption" role="status">No show has that in its name.</p>
  {/if}
  <Unscheduled {shows} {rules} {ondates} onedit={onopen} {ondraft} />
  <div class="nothing flat">
    <p>Select a block in the week: an <b>episode</b> opens its editor; an <b>automation</b> block opens the generated hour; a dotted <b>date from a show’s rule</b> offers to draft the episode or skip the date. A day’s name at the top of the week opens that day, with a lane for each show.</p>
    <p>Keys on this tab: <kbd>←</kbd> <kbd>→</kbd> move a week (a day in the Day view), <kbd>T</kbd> is today, <kbd>D</kbd> the Day view, <kbd>W</kbd> the week. The grid's blocks take focus with <kbd>Tab</kbd> and open with <kbd>⏎</kbd>; <kbd>Esc</kbd> closes what is open and comes back to this list.</p>
  </div>
</div>
