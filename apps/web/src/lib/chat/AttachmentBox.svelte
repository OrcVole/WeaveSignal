<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A picture or document a message carries (ADR-0262). A picture is its thumbnail, which opens
  // it full size; a document is its name, type and size with a Download link: it is never
  // opened in the page, and the station serves it as a download under a name of its own, the
  // chatter's name being shown here as text only. While the station decides, its author sees it
  // waiting (a picture from the file they chose, on this page).
  import { attachmentUrl, sizeWords, type Attachment } from '../../../../../packages/ui/chat.ts';

  let {
    a,
    waiting = false,
    preview = null,
    onview,
  }: { a: Attachment; waiting?: boolean; preview?: string | null; onview: (a: Attachment) => void } = $props();

  const w = $derived(a.thumb_width ?? 240);
  const h = $derived(a.thumb_height ?? 160);
  // Shown at most 240 wide and 240 tall, its shape kept.
  const scale = $derived(Math.min(1, 240 / w, 240 / h));
  const src = $derived(waiting ? preview : attachmentUrl(a, 'thumb'));
</script>

{#if a.kind === 'picture'}
  {#if src}
    <button
      type="button"
      class="thumb"
      data-chat="attachment-thumb"
      data-id={a.id}
      disabled={waiting}
      onclick={() => onview(a)}
      aria-label={waiting ? `${a.name}, waiting for the station` : `Open the picture ${a.name} full size`}
    >
      <img {src} alt="" width={Math.round(w * scale)} height={Math.round(h * scale)} loading="lazy" decoding="async" />
    </button>
  {:else}
    <span class="file" data-chat="attachment-picture-waiting"
      ><svg aria-hidden="true"><use href="#g-hold" /></svg><span class="meta"><b class="name">{a.name}</b><span class="small">Picture · {sizeWords(a.bytes)}</span></span></span
    >
  {/if}
{:else}
  <span class="file" data-chat="attachment-doc" data-id={a.id}>
    <svg aria-hidden="true" viewBox="0 0 20 20"
      ><path d="M5 2h7l4 4v12H5z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" /><path
        d="M12 2v4h4"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linejoin="round"
      /></svg
    >
    <span class="meta"><b class="name" title={a.name}>{a.name}</b><span class="small">{a.label} · {sizeWords(a.bytes)}</span></span>
    {#if !waiting}
      <a class="btn" href={attachmentUrl(a)} download rel="noopener nofollow" data-chat="attachment-download" aria-label="Download {a.name} ({a.label}, {sizeWords(a.bytes)})"
        >Download</a
      >
    {/if}
  </span>
{/if}

<style>
  .thumb {
    display: inline-block;
    padding: 0;
    margin-top: var(--s-1);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    overflow: hidden;
    background: #fff;
    cursor: zoom-in;
    max-width: 100%;
    line-height: 0;
  }
  .thumb[disabled] {
    cursor: default;
    opacity: 0.75;
  }
  .thumb img {
    display: block;
    max-width: 100%;
    height: auto;
  }
  .file {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    margin-top: var(--s-1);
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    background: var(--surface-2);
    max-width: 100%;
  }
  .file svg {
    width: 20px;
    height: 20px;
    flex: none;
  }
  .meta {
    display: grid;
    min-width: 0;
    flex: 1;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
