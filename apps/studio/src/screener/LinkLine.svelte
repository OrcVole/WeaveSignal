<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The chat link's state, one line at the top of a chat panel while it is not live: never a
  // toast per failed try.
  import { chat } from '../lib/chat.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';

  const wait = $derived(Math.max(0, Math.ceil((chat.retryAt - studio.now) / 1000)));
</script>

{#if chat.link === 'off' || chat.link === 'connecting'}
  <p class="empty" data-chat="link">Connecting to the chat…</p>
{:else if chat.link === 'lost'}
  <p class="empty" data-chat="link">
    <b>The chat is not connected</b>{chat.why ? `: ${chat.why}` : ''}. What shows may be out of date. {wait > 0
      ? `Trying again in ${wait} s.`
      : 'Trying again now.'}
  </p>
{/if}
