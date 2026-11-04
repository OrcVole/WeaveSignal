<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The guest's own picture before they join (ADR-0171), panel `join.camera-check`. The camera
  // is opened here and shown to them alone; nothing is sent to the studio until they press
  // join. If the camera will not start they can still join, with sound.
  import type { CameraPreview } from './camera-preview.svelte.ts';

  let { preview }: { preview: CameraPreview } = $props();

  let video = $state<HTMLVideoElement | undefined>();
  $effect(() => {
    if (video) video.srcObject = preview.shown ? new MediaStream([preview.shown]) : null;
  });
</script>

<div class="cam" data-panel="join.camera-check">
  <span class="label">Your camera</span>
  {#if preview.shown}
    <video bind:this={video} class="mirror" class:off={!preview.sendOnJoin} autoplay muted playsinline aria-label="Your own picture, as the studio will see it"></video>
    <p class="small" role="status">This is you. Nothing is sent until you press join.</p>
    <label class="check">
      <input type="checkbox" bind:checked={preview.sendOnJoin} />
      <span>Camera on when I join</span>
    </label>
  {:else if preview.starting}
    <p class="small" role="status">Asking your browser for the camera…</p>
  {:else}
    {#if preview.problem}<p class="problem" role="alert">{preview.problem}</p>{/if}
    <button type="button" class="btn wide" onclick={() => preview.start()}>Try the camera again</button>
  {/if}
</div>

<style>
  .cam {
    display: grid;
    gap: var(--s-2);
  }
  .label {
    font-weight: 650;
  }
  .mirror {
    width: 100%;
    max-width: 24rem;
    aspect-ratio: 16 / 9;
    background: #000;
    border-radius: 6px;
    transform: scaleX(-1);
  }
  .mirror.off {
    opacity: 0.3;
  }
</style>
