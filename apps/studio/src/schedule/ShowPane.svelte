<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The show level of the editor column (week-clashes.md section 3): the way back to the list
  // of shows, the show's name and path, and two tabs. Dates is the show's dates as a list
  // (`calendar.show-dates`), and is where the pane opens; Details is the show editor as it
  // was. It is reached from a row of the list of shows, from Show on an episode's level switch,
  // from a dotted date's pane, and from the lines that follow New show and Give it dates….
  // Under the name, on both tabs, who can listen (keyed shows, 1.3.2; the operator's
  // click-through of 2026-11-25: the pane opens at Dates and said nothing of it): the tick that
  // makes the show keyed (KeyedTick, the one control), in words whether everyone can listen or
  // only people with a key, and Who can listen…, which opens Details at the rest of the choice
  // (what the public hears meanwhile, when it opens to everyone).
  import ShowDates from './ShowDates.svelte';
  import ShowEditor from './ShowEditor.svelte';
  import type { Show, ShowView } from '../lib/schedule.ts';
  import type { IsoDate } from '../lib/api.ts';
  import type { ShowTab } from '../lib/schedule.svelte.ts';
  import type { Block } from './model.ts';
  import { account } from '../lib/account.svelte.ts';
  import { mayManageKeys } from '../lib/keys.ts';
  import KeyedTick from '../keys/KeyedTick.svelte';
  import { KEEP, LISTEN_SHORT, listenState } from '../lib/keyed.ts';
  import { keyed, ruledShow } from '../lib/keys.svelte.ts';
  import { toWhoCanListen } from '../lib/keyedNav.ts';

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

  const listen = $derived(listenState(ruledShow(show.slug)));
  const may = $derived(mayManageKeys(account.me));
</script>

<div class="head">
  <div class="crumbs"><button type="button" class="back" title="Back to the list of shows (Esc)" aria-keyshortcuts="Escape" onclick={onback}>‹ Shows</button> › Show</div>
  <h2 data-panel-title>{show.name}</h2>
  <div class="when"><span class="num">/{show.slug}</span></div>
  {#if keyed.state === 'ok'}
    <div class="listen" data-testid="pane-listen" data-listen={listen}>
      <KeyedTick slug={show.slug} name={show.name} id="sp-{show.slug}" />
      <span class="lw">{LISTEN_SHORT[listen]}</span>
      {#if may}<button type="button" class="linkish" title="What the public hears meanwhile, and when it opens to everyone" onclick={() => toWhoCanListen(show.slug)}>{KEEP.change}</button>{/if}
    </div>
  {/if}
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
