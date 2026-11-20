<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The rota (`calendar.rota`; 1.2, handover-window.md): the coming twelve hours as a plain
  // list. For each airing: when, the show, who is down for it, where they come from, and how
  // far they have got; the one on air is marked. Read from the station every five seconds while
  // the list is in view, and at once when the desk's handover window changes. It changes
  // nothing: the Schedule tab's week is where shows and their hosts are set.
  import { onDestroy, onMount, untrack } from 'svelte';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { getRota, type RotaView } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { routeAbsent } from '../lib/hosts.ts';
  import { rotaHead, rotaLines } from '../lib/rota.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';

  let { initial = null, onweek }: { initial?: RotaView | null; onweek: () => void } = $props();

  const seed = untrack(() => initial);
  let view = $state<RotaView | null>(seed);
  let error = $state('');
  /** The station has no rota: it is older than 1.2. */
  let absent = $state(false);
  let loading = $state(!seed);
  let seq = 0;

  async function load() {
    const mine = ++seq;
    try {
      const v = await getRota();
      if (mine !== seq) return;
      view = v;
      error = '';
    } catch (e) {
      if (mine !== seq) return;
      if (routeAbsent(e)) absent = true;
      else error = placeError(e, []).message;
    } finally {
      if (mine === seq) loading = false;
    }
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    if (seed) return;
    void load();
    timer = setInterval(() => !absent && void load(), 5000);
  });
  onDestroy(() => clearInterval(timer));
  // The window's state is the rota's news too: read again as it changes, not five seconds on.
  const windowState = $derived(studio.view?.post?.window?.state ?? null);
  let seen: string | null | undefined;
  $effect(() => {
    const s = windowState;
    if (seen !== undefined && s !== seen && !seed && !absent) untrack(() => void load());
    seen = s;
  });

  const zone = $derived(sched.zone || studio.zone);
  const lines = $derived(rotaLines(view, zone));
  const head = $derived(rotaHead(view, zone));
</script>

<section class="rota" data-panel="calendar.rota" aria-labelledby="rota-h">
  <div class="rhead">
    <div>
      <h2 id="rota-h" data-panel-title>Rota</h2>
      <p class="sub">The next twelve hours: who is down for each show, where they come from, and whether they are ready.</p>
    </div>
    {#if head}<span class="span num" data-testid="rota-head">{head}</span>{/if}
  </div>

  {#if absent}
    <p class="empty" data-testid="rota-absent"><b>This station has no rota yet.</b> The rota comes with WeaveSignal 1.2. The week still shows every show and its time.</p>
    <p><button type="button" class="btn" onclick={onweek}>Back to the week <kbd>W</kbd></button></p>
  {:else if error && !view}
    <div class="callout" role="alert">
      <p><b>The rota could not be read.</b> {error}</p>
      <button type="button" class="btn sm" onclick={load}>Try again</button>
    </div>
  {:else if loading && !view}
    <p class="empty" role="status">Reading the rota…</p>
  {:else if !lines.length}
    <p class="empty" data-testid="rota-empty"><b>Nothing on the schedule in the next twelve hours.</b> Shows appear here once they have a time. Give one a time in the week.</p>
    <p><button type="button" class="btn" onclick={onweek}>Back to the week <kbd>W</kbd></button></p>
  {:else}
    {#if error}<p class="stale" role="status">Not read since {head.split(' ')[0]}: {error}</p>{/if}
    <div class="scroll">
      <table>
        <caption class="sr-only">The rota: each show in the next twelve hours, in order</caption>
        <thead>
          <tr><th scope="col">When</th><th scope="col">Show</th><th scope="col">Who</th><th scope="col">From</th><th scope="col">Ready</th></tr>
        </thead>
        <tbody>
          {#each lines as r (r.key)}
            <tr class:now={r.now} aria-current={r.now ? 'true' : undefined} data-testid="rota-row" title={r.said}>
              <td class="when"><span class="num">{r.time}</span><span class="rel">{r.when}</span></td>
              <th scope="row">{r.show}</th>
              <td class:nobody={r.nobody}>{r.nobody ? 'Nobody down for it' : r.who}</td>
              <td class="from">{r.from ?? '—'}</td>
              <td class="st">
                {#if r.chip}<StateChip chip={r.chip.chip} word={r.chip.word} sm focusable={false} hoverable={false} />{:else if r.quiet}<span class="quiet">{r.quiet}</span>{:else}<span class="quiet" aria-hidden="true">—</span>{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="foot">A host in their own studio is ready once their software has been steady for a few seconds; a presenter in the room, once they are at the desk. To change who is down for a show, open it in the week.</p>
  {/if}
</section>

<style>
  .rota {
    grid-column: 1 / -1;
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    box-shadow: var(--e-1);
    padding: var(--s-4);
    display: grid;
    gap: var(--s-3);
    align-content: start;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }
  .rhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s-4);
    flex-wrap: wrap;
  }
  h2 {
    font-size: var(--fs-20);
  }
  .sub,
  .foot {
    color: var(--ink-2);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .span {
    color: var(--ink-2);
    font-size: var(--fs-13);
  }
  .empty {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 44rem;
  }
  .empty b {
    color: var(--ink);
  }
  .stale {
    font-size: var(--fs-13);
    color: var(--ink-2);
    border-left: 3px solid var(--status-warning);
    padding-left: var(--s-2);
  }
  .callout {
    border: 1px solid var(--hairline);
    border-left: 3px solid var(--status-serious);
    border-radius: var(--r-1);
    padding: var(--s-2) var(--s-3);
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    max-width: 44rem;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    max-width: 72rem;
  }
  thead th {
    text-align: start;
    font-size: var(--fs-12);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-2);
    padding: 0 var(--s-3) var(--s-1) var(--s-2);
    border-bottom: 1px solid var(--hairline);
    white-space: nowrap;
  }
  tbody th,
  tbody td {
    text-align: start;
    vertical-align: middle;
    padding: var(--s-2) var(--s-3) var(--s-2) var(--s-2);
    border-bottom: 1px solid var(--hairline);
    font-size: var(--fs-14);
    line-height: var(--lh-label);
  }
  tbody th {
    font-weight: 600;
  }
  tbody tr.now {
    background: var(--surface-2);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .when {
    white-space: nowrap;
  }
  .when .num {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .when .rel {
    display: block;
    font-size: var(--fs-12);
    color: var(--ink-2);
  }
  .nobody,
  .quiet,
  .from {
    color: var(--ink-2);
  }
  .st {
    white-space: nowrap;
  }
</style>
