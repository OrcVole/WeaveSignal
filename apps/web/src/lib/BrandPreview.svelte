<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The top of the player as a listener will see it, in both themes at once (ADR-0237): the
  // banner, the icon, the name, the main button and a link, drawn with each theme's own
  // grounds and the accent derived for it. A picture, not a control: it is hidden from
  // assistive technology, and the same facts are in the words beside the colour field.
  import Mark from '../../../studio/src/lib/Mark.svelte';
  import { GROUNDS, type Derived, type ThemeName } from '../../../../packages/wizard/accent.ts';

  let {
    station,
    derived: shades,
    icon,
    banner,
  }: { station: string; derived: Record<ThemeName, Derived | null> | null; icon: string | null; banner: string | null } = $props();

  const themes: ThemeName[] = ['dark', 'light'];
</script>

<div class="previews" aria-hidden="true">
  {#each themes as theme (theme)}
    {@const g = GROUNDS[theme]}
    {@const d = shades?.[theme]}
    <div
      class="frame"
      style:--p-plane={g.plane}
      style:--p-surface={g.surface}
      style:--p-ink={g.ink}
      style:--p-accent={d?.accent ?? g.accent}
      style:--p-accent-ink={d?.ink ?? g.accentInk}
    >
      <span class="tag">{theme === 'dark' ? 'Dark theme' : 'Light theme'}</span>
      {#if banner}<img class="p-banner" src={banner} alt="" />{/if}
      <div class="p-top">
        {#if icon}<img class="p-icon" src={icon} alt="" />{:else}<span class="p-mark"><Mark /></span>{/if}
        <span class="p-name">{station}</span>
      </div>
      <div class="p-btn">Listen live</div>
      <div class="p-card"><span class="p-link">Chat rules and contacts</span></div>
    </div>
  {/each}
</div>

<style>
  .previews { display: grid; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); gap: var(--s-3); }
  .frame { display: grid; gap: 0.6rem; align-content: start; padding: 0.75rem; border-radius: var(--r-3); border: 1px solid var(--axis); background: var(--p-plane); color: var(--p-ink); min-width: 0; }
  .tag { font-size: var(--fs-12); letter-spacing: var(--tracking-caps); text-transform: uppercase; font-weight: 650; opacity: 0.75; line-height: var(--lh-label); }
  .p-banner { display: block; width: 100%; aspect-ratio: 3 / 1; object-fit: cover; border-radius: var(--r-2); }
  .p-top { display: flex; align-items: center; gap: var(--s-2); min-width: 0; }
  .p-icon { width: 24px; height: 24px; object-fit: contain; border-radius: var(--r-1); flex: none; }
  .p-mark { display: inline-flex; color: var(--p-accent); }
  .p-name { font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .p-btn { display: flex; align-items: center; justify-content: center; min-height: 2.5rem; border-radius: var(--r-2); background: var(--p-accent); color: var(--p-accent-ink); font-weight: 650; }
  .p-card { background: var(--p-surface); border-radius: var(--r-2); padding: 0.6rem 0.75rem; border: 1px solid color-mix(in srgb, var(--p-ink) 12%, transparent); }
  .p-link { color: var(--p-accent); text-decoration: underline; font-size: var(--fs-14); }
</style>
