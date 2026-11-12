<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A picture posted in the chat, full size (screener.attachment, ADR-0262): what the room will
  // see if it is approved, at the size the station rewrote it to, so text in a screenshot can be
  // read before deciding. Fetched with the studio's bearer. Esc or Close puts it away.
  import { attachmentOf, chat } from '../lib/chat.svelte.ts';

  const a = $derived(chat.viewing);
  const src = $derived(a ? attachmentOf(a.id, 'full') : null);
  let box: HTMLElement | undefined = $state();
  $effect(() => {
    box?.focus();
  });
  function keys(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      chat.viewing = null;
    }
  }
</script>

{#if a}
  <div
    class="view"
    data-panel="screener.attachment"
    data-chat="attachment-view"
    role="dialog"
    aria-modal="true"
    aria-labelledby="att-h"
    tabindex="-1"
    bind:this={box}
    onkeydown={keys}
  >
    <div class="head">
      <h3 id="att-h" data-panel-title title={a.name}>{a.name}</h3>
      <span class="small muted">{a.label} · {a.width} × {a.height} pixels, as the room would see it</span>
      <span class="sp"></span>
      <button type="button" class="btn sm" onclick={() => (chat.viewing = null)}>Close <kbd>Esc</kbd></button>
    </div>
    <div class="pic">
      {#if src}<img {src} alt="The picture {a.name}, full size" />{:else if src === null}<p class="small">Couldn’t load it.</p>{:else}<p
          class="small"
          role="status">Loading…</p>{/if}
    </div>
  </div>
{/if}

<style>
  .view {
    position: fixed;
    inset: var(--s-5);
    z-index: 40;
    display: grid;
    grid-template-rows: auto 1fr;
    background: var(--surface);
    border: 1.5px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-2);
    min-width: 0;
  }
  .view:focus-visible {
    outline: var(--focus-ring);
    outline-offset: var(--focus-offset);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--s-3);
    padding: var(--s-3) var(--s-4);
    border-bottom: 1px solid var(--hairline);
    min-width: 0;
    flex-wrap: wrap;
  }
  .head h3 {
    font-size: var(--fs-16);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 40ch;
  }
  .sp {
    flex: 1;
  }
  .pic {
    overflow: auto;
    padding: var(--s-3);
    display: grid;
    place-items: start center;
    background: var(--surface-2);
  }
  .pic img {
    max-width: 100%;
    height: auto;
    background: var(--paper);
  }
</style>
