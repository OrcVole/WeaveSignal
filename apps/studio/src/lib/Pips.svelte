<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A step as four pips, filled up to it (ADR-0271). Equal in height, so they do not read as
  // signal bars; they take the text's colour and never a state's. Always beside the step's word.
  import type { ShowStep } from './api.ts';
  import { rank } from './steps.ts';

  let { step }: { step: ShowStep } = $props();
  const n = $derived(rank(step) + 1);
</script>

<svg class="pips" viewBox="0 0 23 10" aria-hidden="true">
  {#each [0, 1, 2, 3] as i (i)}<rect x={0.5 + i * 6} y="0.5" width="4" height="9" rx="1" class:on={i < n} />{/each}
</svg>
