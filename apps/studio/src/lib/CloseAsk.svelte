<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The question before the window closes while the bridge runs (Phase 17g; lib/closeAsk.ts).
  // Keyboard first: Keep the window open has the focus, Esc keeps it open, Tab reaches Stop the
  // bridge and close. Panel name `studio.close-ask` (ADR-0070). Only the packaged app hears the
  // question; `open` shows it without one (tests).
  import { onMount } from 'svelte';
  import { packaged } from './api.ts';
  import { CLOSE_WORDS, keepOpen, onCloseAsked, stopAndClose } from './closeAsk.ts';

  let { open = false }: { open?: boolean } = $props();

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let asked = $state(false);
  let busy = $state(false);
  let problem = $state<string | null>(null);
  const shown = $derived(open || asked);

  onMount(() => {
    if (!packaged()) return;
    let stop: (() => void) | null = null;
    let gone = false;
    onCloseAsked(() => {
      problem = null;
      asked = true;
    }).then((f) => (gone ? f() : (stop = f)));
    return () => {
      gone = true;
      stop?.();
    };
  });

  $effect(() => {
    if (shown && dialogEl && !dialogEl.open) dialogEl.showModal();
  });

  async function keep() {
    asked = false;
    dialogEl?.close();
    await keepOpen().catch(() => {});
  }

  async function close() {
    if (busy) return;
    busy = true;
    try {
      await stopAndClose();
    } catch (e) {
      problem = `The window did not close (${e instanceof Error ? e.message : String(e)}). Close it again to close it without asking.`;
    } finally {
      busy = false;
    }
  }
</script>

{#if shown}
  <dialog
    bind:this={dialogEl}
    data-panel="studio.close-ask"
    aria-labelledby="ca-h"
    aria-describedby="ca-d"
    oncancel={(e) => {
      e.preventDefault();
      keep();
    }}
  >
    <h2 id="ca-h" data-panel-title>{CLOSE_WORDS.title}</h2>
    <p id="ca-d">{CLOSE_WORDS.body}</p>
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="acts">
      <!-- svelte-ignore a11y_autofocus -->
      <button type="button" class="btn primary" data-testid="close-keep" autofocus onclick={keep}>{CLOSE_WORDS.keep}</button>
      <button type="button" class="btn danger-hollow" data-testid="close-stop" onclick={close} disabled={busy} aria-busy={busy || undefined}
        >{busy ? 'Stopping…' : CLOSE_WORDS.stop}</button
      >
    </div>
  </dialog>
{/if}

<style>
  dialog {
    max-width: 30rem;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
  }
  dialog::backdrop {
    background: var(--scrim-strong);
  }
  h2 {
    font-size: var(--fs-20);
    margin-bottom: var(--s-3);
  }
  p {
    line-height: var(--lh-body);
    margin-bottom: var(--s-4);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
</style>
