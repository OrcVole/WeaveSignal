<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A chat message the studio put on air (D32): video viewers see it as a lower third, so the
  // player shows it as text beside the stream for everyone listening to sound alone.
  import type { Msg } from '../../../../../packages/ui/chat.ts';

  let { m }: { m: Msg | null } = $props();
</script>

{#if m}
  <p class="onair" data-chat="featured" data-id={m.id}>
    <span class="state fill-onair"><svg aria-hidden="true"><use href="#g-lower" /></svg>On air</span>
    <span class="text"><b>{m.author.name}:</b> {m.body}</span>
  </p>
{/if}

<style>
  .onair {
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    padding: var(--s-3);
    border-radius: var(--r-2);
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-left: 4px solid var(--live-onair);
  }
  .text {
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
</style>
