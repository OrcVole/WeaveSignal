<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The station's slate picture (ADR-0010, ADR-0200): choose a PNG or JPEG, which the station
  // reads in its sandbox and the desk then shows above the slate's words while the show on
  // air has none of its own; or take it away. Inside `desk.picture`, which owns the name
  // plate. The state is `StillControl`, shared with the show editor's picture.
  import { onMount } from 'svelte';
  import { StillControl } from '../lib/still.svelte.ts';

  let { on }: { on: boolean } = $props();

  const still = new StillControl();
  let input: HTMLInputElement | undefined = $state();

  onMount(() => void still.load());

  async function choose(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    await still.choose(file);
    if (input) input.value = '';
  }
</script>

<div class="still" role="group" aria-label="Slate picture">
  <span class="small"
    >Slate picture: {#if on || still.info?.present}{still.info?.present ? `${still.info.width} × ${still.info.height}` : 'set'}{:else}none{/if}</span
  >
  <input bind:this={input} type="file" accept="image/png,image/jpeg" class="file" aria-label="Choose a slate picture" onchange={choose} disabled={still.busy} />
  {#if on || still.info?.present}
    <button type="button" class="btn" onclick={() => still.remove()} disabled={still.busy}>Remove</button>
  {/if}
  {#if still.problem}<p class="small problem" role="alert">{still.problem}</p>{/if}
</div>
