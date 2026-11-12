<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A picture posted in the chat, full size (ADR-0262), in a real <dialog>: focus moves to its
  // heading on opening and back to the thumbnail that opened it. The picture is the one the
  // station rewrote (about 1,600 pixels on its long side, so text stays legible); it can also be
  // opened in a tab of its own.
  import { tick } from 'svelte';
  import { attachmentUrl, type Attachment } from '../../../../../packages/ui/chat.ts';

  let { a, onclose }: { a: Attachment | null; onclose: () => void } = $props();
  let dialog = $state<HTMLDialogElement | undefined>();
  let heading = $state<HTMLElement | undefined>();
  let opener: HTMLElement | null = null;

  $effect(() => {
    if (!a || !dialog || dialog.open) return;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    void tick().then(() => heading?.focus());
  });

  function closed() {
    opener?.focus();
    opener = null;
    onclose();
  }
</script>

<dialog class="sheet view" bind:this={dialog} onclose={closed} aria-labelledby="pv-h" data-chat="picture-view">
  {#if a}
    <div class="inner">
      <div class="top">
        <h2 id="pv-h" tabindex="-1" bind:this={heading} title={a.name}>{a.name}</h2>
        <button type="button" class="btn quiet" onclick={() => dialog?.close()}>Close</button>
      </div>
      <div class="pic">
        <img src={attachmentUrl(a)} alt="The picture {a.name}" width={a.width} height={a.height} />
      </div>
      <p class="small">
        {a.label}, {a.width} by {a.height} pixels.
        <a href={attachmentUrl(a)} target="_blank" rel="noopener noreferrer nofollow" data-chat="picture-tab">Open in a new tab</a>
      </p>
    </div>
  {/if}
</dialog>

<style>
  .view {
    max-width: min(96vw, 1680px);
    width: min(96vw, 1680px);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    justify-content: space-between;
  }
  .top h2 {
    font-size: var(--fs-16);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .pic {
    overflow: auto;
    max-height: 75vh;
    background: #fff;
    border-radius: var(--r-2);
  }
  .pic img {
    display: block;
    max-width: 100%;
    height: auto;
  }
</style>
