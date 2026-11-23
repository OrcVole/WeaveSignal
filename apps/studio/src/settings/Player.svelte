<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > The player (1.3.1; K3 of the first-sound run): where the station's player is, and
  // the two buttons that open it in the browser or copy its address. For whoever runs this
  // window, whatever their role: listening is not a permission. The app makes no sound of its
  // own, and this panel says so once. The rule for the address is lib/player.ts; the sandbox
  // has no station to listen to, so the panel is not drawn there. Panel name `settings.player`
  // (ADR-0070).
  import { sandboxed } from '../lib/api.ts';
  import PlayerLink from '../lib/PlayerLink.svelte';
  import { PLAYER } from '../lib/player.ts';
</script>

{#if !sandboxed()}
  <section class="panel" data-panel="settings.player" aria-labelledby="pl-h">
    <div class="ph">
      <h2 id="pl-h" data-panel-title>The player</h2>
    </div>
    <p class="help">
      {PLAYER.sound} The player is the station’s web page: it plays what listeners hear, behind the desk by the figure the Desk shows (about twenty seconds), and takes calls and chat.
    </p>
    <PlayerLink id="pl" detail />
  </section>
{/if}

<style>
  .panel {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    box-shadow: var(--e-1);
    padding: var(--s-4);
    display: grid;
    gap: var(--s-3);
    min-width: 0;
  }
  .ph { display: flex; align-items: baseline; gap: var(--s-3); flex-wrap: wrap; }
  .ph h2 { font-size: var(--fs-20); }
  .help { color: var(--ink-2); line-height: var(--lh-body); max-width: 60rem; }
</style>
