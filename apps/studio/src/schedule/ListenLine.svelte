<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Who can listen, said on an episode and on a date not drafted yet (keyed shows, 1.3.2; the
  // operator's click-through of 2026-11-25: an episode showed nothing of it, a past one least
  // of all). The rule is the show's, never one episode's: the line says which rule applies, in
  // words, and leads to the show's Who can listen. An aired episode says plainly that a key
  // keeps the whole show, not that episode. A producer and an admin have the tick that makes
  // the show keyed, here too (KeyedTick: the operator, 2026-11-25, "pick a show or an episode
  // and tick it"), and the way to the rest of the choice; anyone else reads it. Nothing is
  // drawn at a station older than 1.3.2. The line also says what becomes of the show's podcast
  // feed and episode files (1.3.3: held back while the show is keyed, an aired episode's too),
  // so the tick on an aired episode is not read as touching only the episodes to come.
  import { account } from '../lib/account.svelte.ts';
  import { mayManageKeys, timeWords } from '../lib/keys.ts';
  import KeyedTick from '../keys/KeyedTick.svelte';
  import { KEEP, holdsFiles, listenLine, listenState } from '../lib/keyed.ts';
  import { APP_VERSION } from '../appVersion.ts';
  import { keyed, ruledShow } from '../lib/keys.svelte.ts';
  import { toWhoCanListen } from '../lib/keyedNav.ts';
  import { studio } from '../lib/studio.svelte.ts';

  let { slug, show, zone, aired = false }: { slug: string; show: string; zone: string; /** The episode has aired. */ aired?: boolean } = $props();

  const row = $derived(ruledShow(slug));
  const state = $derived(listenState(row));
  const may = $derived(mayManageKeys(account.me));
  const when = $derived(row?.opens_at_ms != null ? timeWords(row.opens_at_ms, zone) : null);
  const line = $derived(listenLine(state, { show, aired, opens: state === 'key' ? when : null, opened: state === 'opened' ? when : null, held: holdsFiles(studio.stationVersion, APP_VERSION) }));
</script>

{#if keyed.state === 'ok'}
  <div class="listenline" data-testid="listen-line" data-listen={state} data-aired={aired || undefined}>
    <KeyedTick {slug} name={show} id="ll-{slug}" plain />
    <p>
      <b>{line.lead}</b>
      {line.rest}
      {#if may}<button type="button" class="linkish" onclick={() => toWhoCanListen(slug)}>{KEEP.change}</button>{:else}A producer or an admin sets who can listen.{/if}
    </p>
  </div>
{/if}
