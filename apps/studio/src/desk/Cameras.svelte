<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The cameras the desk sees, as small pictures refreshed every two seconds (a JPEG the desk
  // makes about once a second per camera), so the presenter sees what they can cut to. Inside
  // `desk.picture`, which owns the name plate. The one on air is marked.
  import { untrack } from 'svelte';
  import { cameraThumbs } from '../lib/video.ts';

  let {
    cameras,
    onair = null,
    label,
  }: {
    cameras: string[];
    /** Whose camera is on air now. */
    onair?: string | null;
    label: (identity: string) => string;
  } = $props();

  let urls = $state<Record<string, string>>({});
  /** Only whether there are any matters to the polling; the list itself changes every view. */
  const any = $derived(cameras.length > 0);

  $effect(() => {
    if (!any) {
      urls = {};
      return;
    }
    let stopped = false;
    const tick = async () => {
      try {
        const t = await cameraThumbs();
        if (!stopped) urls = t;
      } catch {
        // The desk may be restarting or video off: the names stay, the pictures wait.
        if (!stopped) urls = {};
      }
    };
    untrack(() => void tick());
    const timer = setInterval(tick, 2000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  });
</script>

{#if any}
  <ul class="cams" aria-label="Cameras">
    {#each cameras as id (id)}
      <li class:onair={onair === id}>
        {#if urls[id]}<img src={urls[id]} alt="Camera of {label(id)}" width="96" height="53" />{:else}<span class="blank" role="img" aria-label="No picture yet for {label(id)}"></span>{/if}
        <span class="who">{label(id)}</span>{#if onair === id}<span class="air">on air</span>{/if}
      </li>
    {/each}
  </ul>
{/if}
