<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The master lamp with the control for automation beside it (ADR-0183, ADR-0272), and the
  // one dialog the air's three controls answer in. The button hands the air to automation or
  // takes it back; End show (⌃⇧E, and its button on the stage) comes here too. A dialog owns
  // the keyboard (keymap rule 5): ⏎ confirms, Esc keeps going. Panel names `desk.air` (the
  // button) and `desk.end-show` (the dialog), ADR-0070.
  //
  // No new key: the keymap is decided, and a laptop's F-keys are the volume keys.
  import Lamp from './Lamp.svelte';
  import type { LampState } from '../../../../packages/ui/states.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { confirmAir, pressMode } from '../lib/actions.ts';
  import { airPlan, modeButton, modeRefusal } from '../lib/air.ts';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';

  let { lamp }: { lamp: LampState } = $props();

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let busy = $state(false);

  const v = $derived(studio.view);
  const button = $derived(modeButton(v));
  const refusal = $derived(modeRefusal(v));
  /** This window is preparing (ADR-0276): the station's mode is not its to change. */
  const prep = $derived(preparing());
  const ask = $derived(studio.airAsk);
  const plan = $derived(ask && v ? airPlan(ask, v) : null);

  // Open the dialog when something asks (the key, a button); a close from outside is the same
  // as keeping going.
  $effect(() => {
    if (ask && dialogEl && !dialogEl.open) dialogEl.showModal();
  });

  function keep() {
    dialogEl?.close();
  }

  async function confirm(e: SubmitEvent) {
    e.preventDefault();
    if (!ask || busy) return;
    busy = true;
    const what = ask;
    dialogEl?.close();
    try {
      await confirmAir(what);
    } finally {
      busy = false;
    }
  }
</script>

<div class="air">
  <Lamp state={lamp} />
  <button
    type="button"
    class="btn quiet mode"
    data-panel="desk.air"
    aria-describedby={refusal ? 'air-why' : undefined}
    aria-disabled={refusal ? 'true' : undefined}
    disabled={prep}
    title={prep ? OFF_WHY : (refusal ?? undefined)}
    onclick={() => !refusal && pressMode(button.ask === 'live' ? 'live' : 'auto')}>{button.label}</button
  >
  {#if refusal}<span id="air-why" class="why">{refusal}</span>{/if}
</div>

<dialog
  bind:this={dialogEl}
  data-panel="desk.end-show"
  aria-labelledby="air-h"
  onclose={() => (studio.airAsk = null)}
>
  {#if plan}
    <form onsubmit={confirm}>
      <h2 id="air-h" data-panel-title>{plan.title}</h2>
      <p class="lede">{plan.lede}</p>
      <ul>
        {#each plan.lines as line (line)}<li>{line}</li>{/each}
      </ul>
      <div class="acts">
        <button type="button" class="btn quiet" onclick={keep}>{plan.keep} <kbd>Esc</kbd></button>
        <!-- svelte-ignore a11y_autofocus -->
        <button type="submit" class="btn danger-hollow" autofocus disabled={busy}>{plan.confirm} <kbd>⏎</kbd></button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  .air {
    display: grid;
    gap: var(--s-1);
    justify-items: start;
  }
  /* A small button under the lamp: the lamp keeps its place, the control is one glance away. */
  .mode {
    min-height: 1.5rem;
    padding: 0 var(--s-2);
    font-size: var(--fs-12);
    letter-spacing: 0;
    text-transform: none;
    color: var(--ink);
    border-color: var(--axis);
  }
  .mode[aria-disabled='true'] {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .why {
    font-size: var(--fs-12);
    color: var(--ink-2);
    max-width: 12rem;
    line-height: 1.3;
    letter-spacing: 0;
    text-transform: none;
    font-weight: 400;
  }
  dialog {
    width: min(30rem, calc(100vw - 2 * var(--s-4)));
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
    font-size: var(--fs-14);
    letter-spacing: 0;
    text-transform: none;
    font-weight: 400;
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  form {
    display: grid;
    gap: var(--s-3);
  }
  h2 {
    font-size: var(--fs-20);
  }
  .lede {
    color: var(--ink);
    line-height: var(--lh-body);
  }
  ul {
    margin: 0;
    padding-left: var(--s-4);
    color: var(--ink-2);
    line-height: var(--lh-body);
    display: grid;
    gap: var(--s-1);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-1);
  }
</style>
