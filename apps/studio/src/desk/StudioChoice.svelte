<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The Desk tab's studio (1.3.0, `desk.studio`; rehearsal-mode-build-plan.md, screen 1): Live
  // studio or Rehearsal studio, at the head of the Desk tab's left column, in the same place on
  // both desks so the way back is where the way in was. Not in the frame: at 1,440 px the frame
  // has no room left, and a second row there would cost the stage its height.
  //
  // It says what is so: off at this station, Rehearsal cannot be chosen and its tooltip names
  // the setting that turns it on; a rehearsal desk that is not answering can be chosen, because
  // that desk's screen says what is wrong; an open rehearsal is marked with its show. A station
  // older than 1.3.0 has no rehearsal studio, and the choice is then not drawn at all.
  import { rehearsal, rehearsalPhase } from '../lib/rehearsal.svelte.ts';
  import { studioChoice } from '../lib/rehearsalDesk.ts';
  import { switchDeskStudio } from '../lib/rehearsalDesk.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';

  const phase = $derived(rehearsalPhase());
  const choice = $derived(studioChoice(phase, rehearsal.view));
</script>

<div class="studio-choice" role="group" aria-label="The Desk tab’s studio" data-panel="desk.studio" data-rehearsal={phase}>
  <button type="button" aria-pressed={studio.deskStudio === 'live'} title="The live desk: what listeners hear." onclick={() => switchDeskStudio('live')}
    ><span data-panel-title>Live studio</span></button
  >
  <button type="button" aria-pressed={studio.deskStudio === 'rehearsal'} aria-disabled={choice.can ? undefined : 'true'} title={choice.why} onclick={() => choice.can && switchDeskStudio('rehearsal')}
    ><span>Rehearsal studio</span>{#if choice.mark === 'off'}<span class="mark">off</span>{:else if choice.mark === 'not answering'}<svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg><span class="sr-only"> (its desk is not answering)</span>{:else if choice.mark}<svg class="g-accent" aria-hidden="true"><use href="#g-loop" /></svg><span class="sr-only"> (open for {choice.mark})</span>{/if}</button
  >
</div>

<style>
  .studio-choice {
    display: grid;
    grid-template-columns: 1fr 1fr;
    border: 1px solid var(--axis);
    border-radius: var(--r-pill);
    overflow: hidden;
    background: var(--surface);
  }
  button {
    appearance: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--s-1);
    min-height: 1.75rem;
    min-width: 0;
    padding: 0 var(--s-2);
    border: 0;
    background: transparent;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--fs-13);
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  button + button {
    border-left: 1px solid var(--axis);
  }
  button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-ink);
  }
  button[aria-disabled='true'] {
    color: var(--muted);
    cursor: not-allowed;
  }
  button:focus-visible {
    outline: var(--focus-ring);
    outline-offset: -2px;
  }
  svg {
    width: 12px;
    height: 12px;
    flex: none;
  }
  button[aria-pressed='true'] svg {
    color: inherit;
  }
  .mark {
    font-size: var(--fs-12);
    font-weight: 400;
    border: 1px dashed var(--axis);
    border-radius: var(--r-pill);
    padding: 0 var(--s-1);
    line-height: 1.4;
  }
</style>
