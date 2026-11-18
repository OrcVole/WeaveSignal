<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The mark of a count in its last seconds (14a): the clock glyph, amber for the last 30 s and
  // red for the last 10. One shape at both levels: the triangle is FAULT's alone (design system,
  // section 3), and a count that runs out is going to plan. One place on the desk: directly
  // before the figure it marks, on the figure's own line (the clock, the tile's late count, the
  // next show's card, a kept place's line). The colour is never the only sign: the figure reads
  // what is left and is heavier (`[data-hurry]` in desk-b.css), and a screen reader is told the
  // words (`h.word`) by the count that draws this. Nothing is drawn while the count has more
  // than 30 s. Not a panel: it is part of whichever count draws it.
  import type { Hurry } from '../lib/post.ts';

  let { h }: { h: Hurry | null } = $props();
</script>

{#if h}<svg class="hurry {h.level === 'red' ? 'g-critical' : 'g-warning'}" data-hurry-glyph={h.level} aria-hidden="true"><use href="#g-delay" /></svg>{/if}
