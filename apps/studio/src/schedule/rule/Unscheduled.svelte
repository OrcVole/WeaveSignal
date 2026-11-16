<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Nothing coming up (`calendar.unscheduled`; new-show.md section 6): in the right panel with
  // nothing selected, the shows that draw nothing in the week. No dates yet: shows without a
  // rule, each with Give it dates…. Finished: a one-off that has aired or a rule that has
  // ended, with why; closed by default and counted, because a finished show is not a fault.
  // A show's name opens the show's pane (its Dates, and its Details), which a show with no
  // block has no other way to. Since 1.0.5 these two groups stand at the foot of the list of
  // shows (`calendar.shows`).
  import type { Show } from '../../lib/schedule.ts';
  import { unscheduled } from './view.ts';

  let {
    shows,
    rules = true,
    ondates,
    onedit,
    ondraft,
  }: {
    shows: readonly Show[];
    /** The station stores rules; an older one sends no dates for the week. */
    rules?: boolean;
    /** Give it dates…, or Give it new dates…. */
    ondates: (show: Show) => void;
    onedit: (show: Show) => void;
    /** An older station: draft the show's next episode, since the week cannot offer it. */
    ondraft: (show: Show) => void;
  } = $props();

  const u = $derived(unscheduled(shows, rules));
  const any = $derived(u.none.length + u.finished.length + u.toDraft.length > 0);
</script>

{#if any}
  <div class="unsched" data-panel="calendar.unscheduled" data-testid="unscheduled-shows">
    {#if u.none.length}
      <div class="g">
        <p><b data-panel-title>No dates yet</b>, so nothing of {u.none.length === 1 ? 'it' : 'them'} is in the week:</p>
        <ul>
          {#each u.none as r (r.show.slug)}
            <li>
              <button type="button" class="nm" title="Open the show: its dates and its details" onclick={() => onedit(r.show)}>{r.show.name}</button>
              <span class="wy">{r.why}</span>
              <button type="button" class="btn sm primary" onclick={() => ondates(r.show)}>Give it dates…</button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if u.toDraft.length}
      <div class="g done">
        <p><b data-panel-title={u.none.length ? undefined : ''}>This station runs an older WeaveSignal</b>, so the week cannot show the dates still to draft. Update the station; until then, draft each show’s next episode here:</p>
        <ul>
          {#each u.toDraft as r (r.show.slug)}
            <li>
              <button type="button" class="nm" title="Open the show: its dates and its details" onclick={() => onedit(r.show)}>{r.show.name}</button>
              <span class="wy">{r.why}</span>
              <button type="button" class="btn sm" onclick={() => ondraft(r.show)}>Draft the next episode</button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if u.finished.length}
      <details class="g done">
        <summary><b data-panel-title={u.none.length || u.toDraft.length ? undefined : ''}>Finished</b> · {u.finished.length} {u.finished.length === 1 ? 'show' : 'shows'}</summary>
        <ul>
          {#each u.finished as r (r.show.slug)}
            <li>
              <button type="button" class="nm" title="Open the show: its dates and its details" onclick={() => onedit(r.show)}>{r.show.name}</button>
              <span class="wy">{r.why}</span>
              <button type="button" class="btn sm" onclick={() => ondates(r.show)}>Give it new dates…</button>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>
{/if}
