<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The sign-in has ended (a 401 under a staff session or the station key, ADR-0127): one
  // calm notice over the surface with one way back, rather than a silent reconnect loop. It
  // stays until Sign in again is pressed. Panel name `studio.session-ended` (ADR-0070).
  import { account, signInAgain } from './account.svelte.ts';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let busy = $state(false);

  /** The window knew it ran as staff: the station's sentence is about their sign-in. A
   *  window that never got an answer (a session that idled out overnight, or a refused key)
   *  cannot tell the two apart, and says only what is true of both. */
  const staff = $derived(account.me?.kind === 'staff');

  $effect(() => {
    if (account.ended && dialogEl && !dialogEl.open) dialogEl.showModal();
  });

  async function again() {
    if (busy) return;
    busy = true;
    await signInAgain();
  }
</script>

{#if account.ended}
  <dialog
    bind:this={dialogEl}
    data-panel="studio.session-ended"
    aria-labelledby="se-h"
    aria-describedby="se-d"
    oncancel={(e) => e.preventDefault()}
  >
    <h2 id="se-h" data-panel-title>Your sign-in has ended</h2>
    <p id="se-d">
      {#if staff}{account.ended} {/if}Sign in again to carry on. Nothing here is lost, and the station’s address is kept.
    </p>
    <div class="acts">
      <button type="button" class="btn primary" onclick={again} disabled={busy} aria-busy={busy || undefined}>{busy ? 'One moment…' : 'Sign in again'}</button>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    max-width: 28rem;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.55);
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
  }
</style>
