<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The three steps from a show to a key in a listener's hands (keyed shows, 1.3.2; the
  // operator's click-through of 2026-11-25: "the create key thing should give hints,
  // breadcrumbs to follow"): 1 Keep a show for people with a key, 2 Make a key for it, 3 Send
  // it. One line at the top of each dialog on the way, the step it stands at marked, each other
  // step a button that goes there, and under each where it is found. A step that cannot be
  // gone to from here (Send, before there is a key) is words, and says why in its tooltip.
  // Each carries a small drawing: a show's block in the week, a key, a message on its way.
  import { KEY_STEPS, type KeyStep } from '../lib/keyed.ts';

  let {
    current,
    go = {},
    waits = {},
    compact = false,
  }: {
    /** One line of numbers and words, for a narrow column (the show editor). */
    compact?: boolean;
    /** The step this dialog is. */
    current: KeyStep;
    /** Where each other step goes; one left out is drawn as words. */
    go?: Partial<Record<KeyStep, (() => void) | undefined>>;
    /** Why a step has nowhere to go from here, for its tooltip. */
    waits?: Partial<Record<KeyStep, string>>;
  } = $props();
</script>

<ol class="ksteps" class:compact aria-label="From a show to a key in a listener’s hands" data-testid="key-steps">
  {#each KEY_STEPS as s (s.n)}
    {@const here = s.n === current}
    {@const act = here ? undefined : go[s.n]}
    <li class:here class:done={s.n < current} aria-current={here ? 'step' : undefined} data-step={s.n}>
      <span class="n" aria-hidden="true">{s.n}</span>
      <svg class="pic" viewBox="0 0 14 14" aria-hidden="true">
        {#if s.n === 1}
          <rect x="1.5" y="2.5" width="11" height="9.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.3" /><path d="M1.5 5.5h11M4.5 1.2v2.4M9.5 1.2v2.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /><rect x="4" y="7.3" width="6" height="2.6" rx="0.8" fill="currentColor" />
        {:else if s.n === 2}
          <circle cx="4.4" cy="4.4" r="2.9" fill="none" stroke="currentColor" stroke-width="1.4" /><path d="M6.5 6.5l5.8 5.8M9.5 9.5l1.7-1.7M11.5 11.5l1.4-1.4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        {:else}
          <path d="M12.6 1.4 1.4 6l4.3 1.9L7.6 12.4z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M12.6 1.4 5.7 7.9" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
        {/if}
      </svg>
      {#if act}
        <button type="button" class="what go" onclick={act}><span class="sr-only">Step {s.n}: </span><span class="lab">{s.label}</span><span class="where">{s.where}</span></button>
      {:else}
        <span class="what" title={here ? undefined : waits[s.n]}><span class="sr-only">Step {s.n}{here ? ', where you are' : ''}: </span><span class="lab">{s.label}</span><span class="where">{here ? 'You are here' : s.where}</span>{#if !here && waits[s.n]}<span class="sr-only"> {waits[s.n]}</span>{/if}</span>
      {/if}
    </li>
  {/each}
</ol>
