<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // One editor field with its marker (calendar.md §4): inherited (dotted, "from Series 3",
  // Set here), set here (accent, Clear or Reset), as aired (a lock, read-only), the
  // episode's own (guests), or a station setting. Actions are 28 px buttons (review finding 11).
  import type { Snippet } from 'svelte';
  import type { Marker } from './model.ts';

  let {
    label,
    marker = 'own',
    from = '',
    hint = '',
    multi = false,
    plain = false,
    panel,
    actions,
    children,
  }: {
    label: string;
    marker?: Marker | 'setting';
    from?: string;
    hint?: string;
    multi?: boolean;
    /** The children are inputs that draw their own box. */
    plain?: boolean;
    /** A field that is a panel of its own (ADR-0070): its name; the label is the plate's anchor. */
    panel?: string;
    actions?: Snippet;
    children: Snippet;
  } = $props();

  const cls = $derived(marker === 'inherited' ? 'inh' : marker === 'set' ? 'set' : marker === 'frozen' ? 'frozen' : '');
</script>

<div class="field {cls}" data-field={label.toLowerCase().replace(/[^a-z0-9]+/g, '-')} data-panel={panel}>
  <div class="lab">
    <span data-panel-title={panel ? '' : undefined}>{label}</span>
    {#if marker === 'inherited'}
      <span class="src"><svg aria-hidden="true"><use href="#g-up" /></svg>{from}</span>
    {:else if marker === 'set'}
      <span class="src"><svg aria-hidden="true"><use href="#g-pin" /></svg>set here</span>
    {:else if marker === 'frozen'}
      <span class="src"><svg aria-hidden="true"><use href="#g-lock" /></svg>as aired</span>
    {:else if marker === 'setting'}
      <span class="src">station setting</span>
    {/if}
    {#if actions && marker !== 'frozen'}{@render actions()}{/if}
  </div>
  {#if plain}
    {@render children()}
  {:else}
    <div class="val" class:multi>{@render children()}</div>
  {/if}
  {#if hint}<div class="hint">{@html hint}</div>{/if}
</div>
