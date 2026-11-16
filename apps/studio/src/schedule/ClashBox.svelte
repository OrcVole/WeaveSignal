<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The clash in the editor (`calendar.clash`; week-clashes.md section 2): with a block in a
  // clash selected, the shows it clashes with, each with Show it (which selects that block, so
  // it opens in the week and the editor turns to it: the keyboard's way to a folded block),
  // who is on air by the schedule, and Open the day. What a presenter would be refused is the
  // station's to say, and it says so only when a draft is asked for, so it is not guessed here.
  import { clashesOf, takesAir } from './clash.ts';
  import { episodeLine, hm, lengthWords, type Block } from './model.ts';
  import { namesOf } from './blockHtml.ts';
  import { CLASH_COST } from './rule/view.ts';

  let {
    block,
    blocks,
    zone,
    onshow,
    onday,
  }: {
    block: Block;
    /** The week on screen. */
    blocks: readonly Block[];
    zone: string;
    /** Show it: select that block. */
    onshow: (id: string) => void;
    /** Open the day, the block still selected. */
    onday: () => void;
  } = $props();

  const others = $derived(clashesOf(block, blocks));
  const air = $derived(takesAir(block, others.map((o) => o.block)));
</script>

{#if others.length}
  <section class="clashbox" data-panel="calendar.clash" aria-labelledby="cb-h">
    <h3 id="cb-h" data-panel-title><span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span> Clashes with</h3>
    <ul>
      {#each others as o (o.block.id)}
        <li>
          <i style:--c={o.block.colour ? `var(--cart-${o.block.colour})` : 'var(--deemph)'}></i>
          <span><b>{o.block.show}</b> · {o.block.kind === 'rule' ? 'not drafted' : episodeLine(o.block) || 'an episode'} · {hm(o.block.start, zone)} to {hm(o.block.end, zone)} · overlaps {lengthWords(o.minutes * 60_000)}</span>
          <button type="button" class="act2" aria-label="Show {o.block.show}" onclick={() => onshow(o.block.id)}>Show it</button>
        </li>
      {/each}
    </ul>
    {#if block.kind === 'rule'}
      <p>This date is not drafted, so nothing of it airs yet. Draft it and it would clash with {namesOf(others.map((o) => o.block))}.</p>
    {:else if air.drafted > 1 && air.block}
      <p>{CLASH_COST} {#if air.tie}Both start at {hm(air.block.start, zone)}.{:else}Here that is <b>{air.block.show}</b>, from {hm(air.block.start, zone)}.{/if}</p>
    {:else}
      <p>The other date is not drafted yet. If it is, the one that starts later is on air by the schedule.</p>
    {/if}
    <div class="r"><button type="button" class="btn sm" onclick={onday}>Open the day</button></div>
  </section>
{/if}
