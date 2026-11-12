<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A chatter's avatar (ADR-0235): their approved picture, else the shipped avatar they chose,
  // else the plain default, their initial on one of eight grounds. It is decoration beside a
  // name that is always printed, so it is hidden from assistive technology unless `label` asks
  // for it to be named (the settings sheet's preview). A picture that cannot load falls back to
  // the default instead of a broken-image mark.
  import { ground, initial, stockUrl } from './avatars.ts';
  import type { Avatar } from './chat.ts';

  interface Props {
    avatar: Avatar | null | undefined;
    /** The chatter's name: the default's letter and ground come from it. */
    name: string;
    /** Its side in CSS pixels. */
    size?: number;
    /** A picture to show in place of `avatar` (a blob the page fetched with a bearer). */
    src?: string | null;
    /** Where an uploaded picture is fetched; the studio fetches with its bearer and passes `src`. */
    upload?: (id: string) => string | null;
    /** Name the picture for assistive technology; left out, it is decoration. */
    label?: string;
  }
  let { avatar, name, size = 28, src = null, upload = (id) => `/api/v1/chat/avatars/${encodeURIComponent(id)}`, label }: Props = $props();

  const wanted = $derived(src ?? (avatar?.kind === 'stock' ? stockUrl(avatar.id) : avatar?.kind === 'upload' ? upload(avatar.id) : null));
  // A picture that failed is not tried again until what is asked for changes.
  let failed = $state<string | null>(null);
  const url = $derived(wanted && wanted !== failed ? wanted : null);
</script>

<span
  class="avatar ground-{ground(name)}"
  class:pic={!!url}
  style:--avatar-size="{size}px"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
  data-avatar={url ? (src ? 'own' : (avatar?.kind ?? 'own')) : 'default'}
>
  {#if url}
    <img src={url} alt="" width={size} height={size} loading="lazy" decoding="async" draggable="false" onerror={() => (failed = wanted)} />
  {:else}
    <span class="letter">{initial(name)}</span>
  {/if}
</span>

<style>
  .avatar {
    --avatar-size: 28px;
    --avatar-hue: 250;
    flex: none;
    display: inline-grid;
    place-items: center;
    width: var(--avatar-size);
    height: var(--avatar-size);
    border-radius: 50%;
    overflow: hidden;
    /* The default's ground: a tint of the surface, so it sits in either theme and the ink on it
       keeps its contrast. */
    background: color-mix(in oklab, oklch(0.62 0.13 calc(var(--avatar-hue) * var(--avatar-hue-scale, 1) + var(--avatar-hue-shift, 0))) 30%, var(--surface-2));
    color: var(--ink);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 14%, transparent);
    user-select: none;
    vertical-align: middle;
  }
  .avatar.pic {
    background: var(--surface-2);
  }
  .avatar img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }
  .letter {
    font-weight: 700;
    font-size: calc(var(--avatar-size) * 0.46);
    line-height: 1;
    letter-spacing: 0;
  }
  .ground-0 { --avatar-hue: 25; }
  .ground-1 { --avatar-hue: 70; }
  .ground-2 { --avatar-hue: 135; }
  .ground-3 { --avatar-hue: 185; }
  .ground-4 { --avatar-hue: 235; }
  .ground-5 { --avatar-hue: 280; }
  .ground-6 { --avatar-hue: 325; }
  .ground-7 { --avatar-hue: 355; }
</style>
