<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The show level of the editor column (week-clashes.md section 3): the way back to the list
  // of shows, the show's name and path, and two tabs. Dates is the show's dates as a list
  // (`calendar.show-dates`), and is where the pane opens; Details is the show editor as it
  // was. It is reached from a row of the list of shows, from Show on an episode's level switch,
  // from a dotted date's pane, and from the lines that follow New show and Give it dates….
  import ShowDates from './ShowDates.svelte';
  import ShowEditor from './ShowEditor.svelte';
  import type { Show, ShowView } from '../lib/schedule.ts';
  import type { IsoDate } from '../lib/api.ts';
  import type { ShowTab } from '../lib/schedule.svelte.ts';
  import type { Block } from './model.ts';

  let {
    show,
    tab,
    zone,
    now,
    today,
    blocks,
    selected,
    ontab,
    onback,
    onChanged,
    onTell,
    onShow,
    onOpen,
    onGiveDates,
    onSaved,
    onDeleted,
    onCancel,
  }: {
    show: Show;
    tab: ShowTab;
    zone: string;
    now: number;
    today: IsoDate;
    blocks: readonly Block[];
    selected: string | null;
    ontab: (tab: ShowTab) => void;
    /** ‹ Shows. */
    onback: () => void;
    onChanged: () => Promise<void>;
    onTell: (title: string, message: string, act?: (() => Promise<void>) | null, label?: string) => void;
    onShow: (blockId: string | null, ms: number) => void;
    onOpen: (blockId: string, ms: number) => void;
    onGiveDates: () => void;
    onSaved: (v: ShowView) => Promise<void> | void;
    onDeleted?: (name: string) => Promise<void> | void;
    onCancel: () => void;
  } = $props();
</script>

<div class="head">
  <div class="crumbs"><button type="button" class="back" title="Back to the list of shows (Esc)" aria-keyshortcuts="Escape" onclick={onback}>‹ Shows</button> › Show</div>
  <h2 data-panel-title>{show.name}</h2>
  <div class="when"><span class="num">/{show.slug}</span></div>
  <div class="levels two" role="tablist" aria-label="The show">
    <button type="button" role="tab" aria-selected={tab === 'dates'} onclick={() => ontab('dates')}>Dates</button>
    <button type="button" role="tab" aria-selected={tab === 'details'} onclick={() => ontab('details')}>Details</button>
  </div>
</div>
{#if tab === 'details'}
  <ShowEditor slug={show.slug} {show} {onSaved} onRule={onChanged} {onDeleted} {onCancel} onCard={() => {}} embedded />
{:else}
  <ShowDates slug={show.slug} {show} {zone} {now} {today} {blocks} {selected} {onChanged} {onTell} {onShow} {onOpen} {onGiveDates} />
{/if}
