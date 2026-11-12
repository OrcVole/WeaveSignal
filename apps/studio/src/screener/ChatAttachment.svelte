<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A picture or document posted in the chat (ADR-0262), as the screener shows it: a picture's
  // thumbnail, fetched with the studio's bearer (a waiting one is for staff and moderators only),
  // which opens it full size (screener.attachment); a document as its name, its type and its size,
  // with Save to check, never opened here. The name is the chatter's, shown as text only.
  import { attachmentUrl, sizeWords, type Attachment } from '../../../../packages/ui/chat.ts';
  import { attachmentOf, chat, chatSay, saveAttachment } from '../lib/chat.svelte.ts';

  let { a, size = 160 }: { a: Attachment; size?: number } = $props();

  const src = $derived(a.kind === 'picture' ? attachmentOf(a.id, 'thumb') : null);
  const ratio = $derived(a.thumb_width && a.thumb_height ? a.thumb_height / a.thumb_width : 0.6);
  let saving = $state(false);

  async function save() {
    if (saving) return;
    saving = true;
    try {
      await saveAttachment(a.id);
    } catch (e) {
      chatSay('warning', 'Not saved', e instanceof Error ? e.message : String(e));
    } finally {
      saving = false;
    }
  }
</script>

{#if a.kind === 'picture'}
  <button
    type="button"
    class="thumb"
    data-chat="attachment-thumb"
    data-id={a.id}
    style:--w="{size}px"
    style:--h="{Math.round(size * Math.min(ratio, 1.5))}px"
    onclick={() => (chat.viewing = a)}
    aria-label="Open {a.name} full size ({a.label}, {a.width} by {a.height})"
  >
    {#if src}<img {src} alt="" draggable="false" />{:else if src === null}<span class="gone">Couldn’t load it</span>{/if}
  </button>
{:else}
  <span class="doc" data-chat="attachment-doc" data-id={a.id} data-href={attachmentUrl(a)}>
    <svg class="g" aria-hidden="true"><use href="#g-inbox" /></svg>
    <span class="meta"><b class="name" title={a.name}>{a.name}</b><span class="hint">{a.label} · {sizeWords(a.bytes)}</span></span>
    <button type="button" class="btn sm" data-chat="attachment-save" disabled={saving} onclick={save}>Save to check</button>
  </span>
{/if}

<style>
  .thumb {
    display: block;
    width: var(--w);
    height: var(--h);
    padding: 0;
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    background: var(--surface-3);
    overflow: hidden;
    cursor: zoom-in;
  }
  .thumb:focus-visible {
    outline: var(--focus-ring);
    outline-offset: var(--focus-offset);
  }
  .thumb img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: var(--paper);
  }
  .gone {
    font-size: var(--fs-12);
    color: var(--muted);
  }
  .doc {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    max-width: 100%;
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-1) var(--s-2);
    background: var(--surface-2);
  }
  .doc .g {
    width: 1.25rem;
    height: 1.25rem;
    flex: none;
  }
  .meta {
    display: grid;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    font-size: var(--fs-12);
    color: var(--muted);
  }
</style>
