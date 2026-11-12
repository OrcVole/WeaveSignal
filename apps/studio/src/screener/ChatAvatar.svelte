<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A chatter's avatar in the screener (ADR-0235). A shipped avatar and the plain default are
  // drawn from the app's own files; a chatter's own picture, waiting (`held`) or approved, is
  // fetched with the studio's bearer (chat.svelte.ts, pictureOf), since a held one is for staff
  // only. While it loads, and if it cannot, the default shows.
  import Avatar from '../../../../packages/ui/Avatar.svelte';
  import type { Avatar as AvatarRef } from '../../../../packages/ui/chat.ts';
  import { pictureOf } from '../lib/chat.svelte.ts';

  let {
    avatar,
    name,
    size = 20,
    held = null,
    square = false,
    label,
  }: { avatar: AvatarRef | null | undefined; name: string; size?: number; held?: string | null; square?: boolean; label?: string } = $props();

  const own = $derived(held ?? (avatar?.kind === 'upload' ? avatar.id : null));
  const src = $derived(own ? (pictureOf(own) ?? null) : null);
</script>

{#if square}
  <!-- The whole square as it was sent, for judging it: the circle hides the corners. -->
  <span class="square" style:--side="{size}px" role="img" aria-label={label ?? `${name}'s picture`} data-loaded={!!src}>
    {#if src}<img {src} alt="" width={size} height={size} draggable="false" />{/if}
  </span>
{:else}
  <Avatar avatar={own ? null : avatar} {name} {size} {src} {label} upload={() => null} />
{/if}

<style>
  .square {
    display: block;
    flex: none;
    width: var(--side);
    height: var(--side);
    border-radius: var(--r-2);
    overflow: hidden;
    background: var(--surface-3);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .square img {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
