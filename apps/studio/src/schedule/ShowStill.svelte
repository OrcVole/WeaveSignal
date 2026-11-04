<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A show's own slate picture (ADR-0200), one field of the show editor: a preview of what
  // the slate shows while this show is on air, Choose or Replace a PNG or JPEG, Remove. The
  // station reads the picture in its sandbox, the same route as the station's still; its
  // sentence is shown when it refuses one. The state is `StillControl`, shared with the desk.
  import { onMount, tick, untrack } from 'svelte';
  import Field from './Field.svelte';
  import { StillControl } from '../lib/still.svelte.ts';

  let { slug }: { slug: string } = $props();

  // The slug seeds the control once; the editor is made anew for another show.
  const still = new StillControl(untrack(() => slug), true);
  let input: HTMLInputElement | undefined = $state();
  let chooser: HTMLButtonElement | undefined = $state();

  onMount(() => void still.load());

  const present = $derived(still.info?.present ?? false);
  const status = $derived(
    still.phase === 'looking'
      ? 'Looking for a picture…'
      : still.phase === 'sending'
        ? 'Sending the picture…'
        : still.phase === 'removing'
          ? 'Taking the picture away…'
          : present && still.info
            ? `${still.info.width} × ${still.info.height}, ${sizeWords(still.info.bytes)}`
            : 'No picture of its own.',
  );

  function sizeWords(bytes: number): string {
    return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  async function choose(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    await still.choose(file);
    if (input) input.value = '';
  }

  async function remove() {
    await still.remove();
    // Remove leaves with the picture; the focus goes to Choose… rather than nowhere.
    await tick();
    chooser?.focus();
  }
</script>

<Field label="Slate picture" marker={present ? 'set' : 'own'} panel="calendar.show-still" plain hint="The slate shows it while this show is on air; otherwise it shows the station's picture.">
  {#snippet actions()}
    <button bind:this={chooser} type="button" class="act" onclick={() => input?.click()} disabled={still.busy}>{present ? 'Replace…' : 'Choose…'}</button>
    {#if present}<button type="button" class="act" onclick={remove} disabled={still.busy}>Remove</button>{/if}
  {/snippet}
  <div class="still-preview" class:none={!present}>
    {#if still.image}
      <img src={still.image} alt="This show's slate still, as the slate shows it" />
    {/if}
    <span class="status" aria-live="polite">{status}</span>
    {#if still.problem}
      <div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span><b>Refused:</b> {still.problem}</span></div>
    {/if}
  </div>
  <input bind:this={input} type="file" accept="image/png,image/jpeg" aria-label="Choose the show's slate picture, a PNG or JPEG" onchange={choose} disabled={still.busy} hidden />
</Field>
