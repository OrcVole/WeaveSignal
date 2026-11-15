<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Before you save (`calendar.rule.change`; ADR-0278, new-show.md section 3): what a changed
  // rule does to episodes, in three groups. Aired episodes are frozen (ADR-0055). Drafted
  // episodes still on the old rule's dates move to the new dates by default, each move listed
  // first, with Leave them where they are offered. An episode a person moved or made by hand
  // stays where it is. The plan is the control plane's; this draws it and holds the choice.
  import type { DraftedChoice, RuleChangePlan } from '../../lib/api.ts';
  import { byHandLines, changeLines, movable } from './view.ts';

  let {
    plan,
    zone,
    choice = $bindable('move'),
    name = 'rc',
  }: {
    plan: RuleChangePlan;
    zone: string;
    choice?: DraftedChoice;
    /** The radio group's name: one a page. */
    name?: string;
  } = $props();

  const lines = $derived(changeLines(plan, zone));
  const byHand = $derived(byHandLines(plan, zone));
  const moving = $derived(movable(plan));
</script>

<div class="groups" data-panel="calendar.rule.change">
  {#if plan.aired}
    <div class="grp frozen">
      <div class="gh"><svg aria-hidden="true"><use href="#g-lock" /></svg><b>{plan.aired} aired.</b></div>
      <span>Not changed: what was broadcast is not rewritten.</span>
    </div>
  {/if}
  {#if lines.length}
    <div class="grp go">
      <div class="gh"><svg aria-hidden="true"><use href="#g-repeat" /></svg><b data-panel-title>{lines.length} drafted, on the old dates.</b></div>
      <ul>
        {#each lines as l (l.ep)}
          {#if l.stays}
            <li class="stay"><span>{l.ep}</span><span>{l.stays}</span></li>
          {:else}
            <li class:clash={!!l.clash}>
              <span>{l.ep}</span><span>{l.from}</span><svg aria-label="moves to" role="img"><use href="#g-moved" /></svg><span class="to">{l.to}</span>
              {#if l.clash}<span class="why"><span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span>{l.clash}</span>{/if}
            </li>
          {/if}
        {/each}
      </ul>
      {#if moving}
        <div class="twoway" role="radiogroup" aria-label="The drafted episodes">
          <label><input type="radio" name="{name}-drafted" value="move" bind:group={choice} /> Move them to the new dates</label>
          <label><input type="radio" name="{name}-drafted" value="leave" bind:group={choice} /> Leave them where they are</label>
        </div>
        <span class="small">They keep their numbers, titles, guests and everything set on them.</span>
      {/if}
    </div>
  {/if}
  {#if byHand.length}
    <div class="grp">
      <div class="gh"><svg aria-hidden="true"><use href="#g-pin" /></svg><b>{byHand.length} moved by hand.</b></div>
      <ul>
        {#each byHand as l (l)}<li class="stay"><span>{l.split(' · ')[0]}</span><span>{l.split(' · ')[1]}. It stays where you put it.</span></li>{/each}
      </ul>
    </div>
  {/if}
</div>
