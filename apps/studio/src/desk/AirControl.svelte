<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The master lamp with the control for automation under it (ADR-0183, ADR-0272), and the
  // one dialog the air's three controls answer in. The button hands the air to automation or
  // takes it back; End show (⌃⇧E, and its button on the stage) comes here too. It stays by the
  // lamp, the state it changes (1.1: a button of End show's own size and the lamp's own width,
  // with its words in a tooltip; it was 12 px and quiet, and a presenter in the room who wanted to
  // leave the air did not find it; beside End show it cost the stage 16 px of height, which the
  // 900 px desk has not got). End show wears the same outlined button (`.btn.outline`,
  // desk-b.css), so the proper way to end does not look the quieter of the two. A dialog owns
  // the keyboard (keymap rule 5): ⏎ confirms, Esc keeps going. Panel names `desk.air` (the
  // button) and `desk.end-show` (the dialog), ADR-0070.
  //
  // What automation is (the operator, 2026-11-17: "The automatic what?") is said in one sentence,
  // `AUTOMATION_PLAYS`, in three places: the button's tooltip here (on hover and on keyboard
  // focus, after what the press does), the AUTO lamp's tooltip (Lamp.svelte), and the dialog,
  // wherever the ask gives automation the air. A button that cannot act (no playout layer, a
  // preparing window) keeps saying why instead.
  //
  // No new key: the keymap is decided, and a laptop's F-keys are the volume keys.
  import Lamp from './Lamp.svelte';
  import Tip from '../../../../packages/ui/Tip.svelte';
  import type { LampState } from '../../../../packages/ui/states.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { confirmAir, pressMode } from '../lib/actions.ts';
  import { AUTOMATION_PLAYS, MODE_WHY, airPlan, modeButton, modeRefusal } from '../lib/air.ts';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';

  let { lamp }: { lamp: LampState } = $props();

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let modeEl = $state<HTMLButtonElement | null>(null);
  let airEl = $state<HTMLElement | null>(null);
  let busy = $state(false);

  const v = $derived(studio.view);
  const button = $derived(modeButton(v));
  const refusal = $derived(modeRefusal(v));
  /** This window is preparing (ADR-0276): the station's mode is not its to change. */
  const prep = $derived(preparing());
  const ask = $derived(studio.airAsk);
  const plan = $derived(ask && v ? airPlan(ask, v) : null);
  /** The button can act, so its tooltip says what the press does and what automation is. */
  const tipped = $derived(!prep && !refusal);

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

<div class="air" bind:this={airEl}>
  <Lamp state={lamp} clear={airEl} />
  <button
    bind:this={modeEl}
    type="button"
    class="btn quiet outline mode"
    data-panel="desk.air"
    aria-describedby={refusal ? 'air-why' : tipped ? 'air-tip' : undefined}
    aria-disabled={refusal ? 'true' : undefined}
    disabled={prep}
    title={prep ? OFF_WHY : (refusal ?? undefined)}
    onclick={() => !refusal && pressMode(button.ask === 'live' ? 'live' : 'auto')}>{button.label}</button
  >
  {#if refusal}<span id="air-why" class="why">{refusal}</span>{/if}
  {#if tipped}
    <!-- Under the button; where there is no room below, over the lamp and the button together. -->
    <Tip id="air-tip" anchor={modeEl} clear={airEl}>
      <b>{button.ask === 'auto' ? MODE_WHY.auto : MODE_WHY.live}</b>{' '}
      <span>{AUTOMATION_PLAYS}</span>
    </Tip>
  {/if}
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
      {#if plan.about}<p class="about" data-testid="air-about">{plan.about}</p>{/if}
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
    justify-items: stretch;
  }
  /* Under the lamp, as wide as it and as tall as End show's button: it reads as the lamp's own
     control. Its outline and ink are `.btn.outline`'s (desk-b.css), which End show shares. */
  .mode {
    min-height: 1.75rem;
    padding: 0 var(--s-2);
    font-size: var(--fs-13);
    letter-spacing: 0;
    text-transform: none;
    justify-content: center;
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
    width: min(30rem, calc(var(--win-w, 100vw) - 2 * var(--s-4)));
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
  /* What automation is: the dialog's small print, under what the press does. */
  .about {
    margin: 0;
    padding-top: var(--s-3);
    border-top: 1px solid var(--hairline);
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-body);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-1);
  }
</style>
