<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Beside the station's name in every public page's header: the station's own icon when it
  // has set one (ADR-0237), else WeaveSignal's mark. Decorative either way: the name is text.
  import Mark from '../../../studio/src/lib/Mark.svelte';
  import { brand } from './brand.svelte.ts';
</script>

{#if brand.icon}
  <img class="brand-icon" src={brand.icon.url} width="28" height="28" alt="" decoding="async" />
{:else}
  <Mark />
{/if}
