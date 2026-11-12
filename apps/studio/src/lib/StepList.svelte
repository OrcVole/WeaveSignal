<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The step list (ADR-0271): four radio rows, each with its pips, its word and one line. One
  // component for the frame's dialog, the show editor and the DJ's wizard, so the four steps
  // read the same wherever a show's step is set.
  import type { ShowStep } from './api.ts';
  import { STEPS, STEP_SHORT, STEP_WORD } from './steps.ts';
  import Pips from './Pips.svelte';

  let {
    value = $bindable(),
    name,
    now = null,
    what = STEP_SHORT,
    locked = false,
    why = '',
    describedby,
    onpick,
  }: {
    /** The step picked. */
    value: ShowStep;
    /** The radio group's name: one a page, so two lists never share a choice. */
    name: string;
    /** The step in force, marked "now"; null where there is none to mark (the editor, the wizard). */
    now?: ShowStep | null;
    /** The line beside each step: the short ones, or the editor's longer ones. */
    what?: Record<ShowStep, string>;
    /** This account may not change it: the list is to read. */
    locked?: boolean;
    /** What unlocks it, said on each locked choice and once under the list (the operator's 1.0.2 run). */
    why?: string;
    describedby?: string;
    /** Told of a pick, for a caller that holds the step inside a draft rather than binding it. */
    onpick?: (step: ShowStep) => void;
  } = $props();
</script>

<fieldset class="steplist" class:locked aria-describedby={describedby}>
  <legend class="sr-only">Step</legend>
  {#each STEPS as s (s)}
    <label title={locked && why && now !== s ? why : undefined}>
      <input type="radio" {name} value={s} bind:group={value} disabled={locked} data-step={s} onchange={() => onpick?.(s)} />
      <Pips step={s} /><b>{STEP_WORD[s]}</b><span class="what">{what[s]}</span>
      {#if now === s}<span class="nowtag">now</span>{:else}<span></span>{/if}
    </label>
  {/each}
  {#if locked && why}<p class="locked-why" data-testid="step-locked-why"><svg aria-hidden="true" viewBox="0 0 16 16"><path d="M5 7V5a3 3 0 0 1 6 0v2M4 7h8v6H4z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>{why}</p>{/if}
</fieldset>
