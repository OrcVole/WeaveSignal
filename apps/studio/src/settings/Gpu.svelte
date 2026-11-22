<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings › Graphics card (`settings.graphics-card`; 1.2.1, ADR-0292): an admin's. When the
  // station's server has an NVIDIA card the desk could encode video on (NVENC, if the card has
  // an AV1 encoder), this says so in plain words: that it is there and not used, with the two
  // steps that put it to use, or that the desk has it. Nothing here turns it on, and an update never does: the card is handed to the
  // desk where the station is installed (lib/gpu.ts says why).
  //
  // A server with no card, the Cloudron package, a station older than 1.2.1 and the sandbox
  // draw nothing.
  import { onMount, untrack } from 'svelte';
  import { gpuFacts, gpuWords, type GpuFacts } from '../lib/gpu.ts';

  /** `initial` is a test's facts: the panel then asks nothing. */
  let { initial = null }: { initial?: GpuFacts | null } = $props();
  const seed = untrack(() => initial);
  let facts = $state<GpuFacts | null>(seed);
  const words = $derived(gpuWords(facts));

  onMount(async () => {
    if (seed) return;
    facts = await gpuFacts().catch(() => null);
  });
</script>

{#if words}
  <section class="panel" data-panel="settings.graphics-card" data-gpu={words.state} aria-labelledby="gpu-h">
    <div class="ph">
      <h2 id="gpu-h" data-panel-title>Graphics card</h2>
      <span class="tag" data-testid="gpu-tag">{words.tag}</span>
    </div>
    <p class="what" data-testid="gpu-what"><b>{words.lead}</b> {words.rest}</p>
    {#if words.steps}
      <div class="offer" data-testid="gpu-offer">
        <h3>To encode video on the card</h3>
        <ol>
          {#each words.steps as s (s.text)}
            <li>{s.text}{#if s.code}{' '}<code>{s.code}</code>.{/if}</li>
          {/each}
        </ol>
      </div>
    {/if}
    <p class="hint" data-testid="gpu-after">{words.after}</p>
  </section>
{/if}

<style>
  .panel {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    box-shadow: var(--e-1);
    padding: var(--s-4);
    display: grid;
    gap: var(--s-3);
    min-width: 0;
  }
  .ph {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
  .ph h2 {
    font-size: var(--fs-20);
  }
  /* A word in a ring, not a state chip: this is a fact about the install, not a live state. */
  .tag {
    font-size: var(--fs-13);
    color: var(--ink-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-1);
    padding: 0 var(--s-2);
    line-height: 1.6;
    white-space: nowrap;
  }
  [data-gpu='offered'] .tag {
    border-left: 3px solid var(--accent);
    color: var(--ink);
  }
  .what {
    color: var(--ink);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .what b {
    font-weight: 600;
  }
  .offer {
    display: grid;
    gap: var(--s-2);
    border-left: 3px solid var(--accent);
    background: var(--surface-2);
    border-radius: var(--r-1);
    padding: var(--s-3) var(--s-4);
    max-width: 60rem;
  }
  h3 {
    font-size: var(--fs-14);
    font-weight: 600;
  }
  ol {
    margin: 0;
    padding-left: 1.4rem;
    display: grid;
    gap: var(--s-1);
    line-height: var(--lh-body);
    font-size: var(--fs-14);
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
    background: var(--surface-3);
    border-radius: var(--r-1);
    padding: 0 0.3em;
    user-select: all;
  }
  .hint {
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
</style>
