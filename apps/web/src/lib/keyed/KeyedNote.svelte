<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // On the join page (keyed shows, 1.3.2): someone invited to a show that needs a key is told
  // who will hear them, and that they need no key themselves. Quiet, one line; nothing is drawn
  // for a public show or at a station with no keyed shows.
  import type { KeyedShow } from './routes.ts';
  // The show is the invitation's own (1.3.2), so the line is there before the show starts; on a
  // rehearsal link it speaks of when the show airs.
  import { invitedWords } from './words.ts';

  let { show, rehearsal = false, now = Date.now(), zone = undefined }: { show: KeyedShow | null; rehearsal?: boolean; now?: number; zone?: string } = $props();
</script>

{#if show}
  <p class="note" data-panel="join.keyed" data-testid="join-keyed">
    <svg aria-hidden="true"><use href="#g-key" /></svg>
    <span><b>A show for people with a key.</b> {invitedWords(show, { rehearsal, now, zone })}</span>
  </p>
{/if}
