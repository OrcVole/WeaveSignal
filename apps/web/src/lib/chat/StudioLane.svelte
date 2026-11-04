<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The private lane between this chatter and the studio (ADR-0040): what they sent, and the
  // studio's replies. An invitation offers the call-in lobby on this same page.
  import type { LaneEntry } from './client.svelte.ts';
  import { hhmm } from './model.ts';

  let { entries, inCall, oninvite }: { entries: LaneEntry[]; inCall: boolean; oninvite: () => void } = $props();
</script>

{#if entries.length}
  <section class="card lane" aria-labelledby="lane-h">
    <h3 id="lane-h">You and the studio</h3>
    <p class="small">Private: only the producer and the presenter see what you send here.</p>
    <ol>
      {#each entries as e (e.id)}
        <li class:mine={e.mine} data-chat={e.mine ? 'mine' : 'private'} data-id={e.id}>
          <p class="small"><b>{e.mine ? 'You' : e.from}</b> · <time datetime={new Date(e.at_ms).toISOString()}>{hhmm(e.at_ms)}</time></p>
          <p class="words">{e.body}</p>
          {#if e.invite && !e.mine}
            {#if inCall}
              <p class="small">You're on a call with the studio now.</p>
            {:else}
              <button type="button" class="btn primary" onclick={oninvite} data-chat="invite"><svg aria-hidden="true"><use href="#g-mic" /></svg>Call the studio</button>
            {/if}
          {/if}
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  h3 {
    font-size: var(--fs-16);
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-2);
  }
  li {
    display: grid;
    gap: var(--s-1);
    justify-items: start;
    padding: var(--s-2) var(--s-3);
    border-radius: var(--r-2);
    background: var(--surface-2);
    border-left: 3px solid var(--accent);
  }
  li.mine {
    background: transparent;
    border-left-color: var(--axis);
  }
  .words {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
