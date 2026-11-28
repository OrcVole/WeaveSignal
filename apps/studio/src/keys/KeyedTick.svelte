<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The one control that makes a show keyed (keyed shows, 1.3.2; the operator, 2026-11-25:
  // "pick a show or an episode and tick it to make it keyed … one consistent control"): a tick
  // box with the key and the words Keyed show, the same wherever a show is in sight: a row of
  // the Schedule's list of shows, the top of a show's pane, an episode and a date not drafted
  // yet, the Keys tab's list of shows. New show's tick has the same look and words.
  //
  // The tick means one thing: a key is needed to listen to the show now. It is ticked on a show
  // kept for people with a key and on no other. A show that was kept and has opened to everyone
  // by itself (its time to open has passed) is not ticked, since everyone can listen (1.3.3; in
  // 1.3.2 it stayed ticked, and an open show could be taken for a kept one): the words beside
  // it say when it opened, and ticking it keeps it again from that moment. The tooltip says
  // what the state means and what a press does.
  //
  // Ticking it keeps the show for people with a key from that moment (the rule it had before
  // is kept: what the public hears, a time to come when it opens to everyone), and a line says
  // so, with what becomes of its podcast feed and episode files (1.3.3: held back while it is
  // keyed, earlier episodes included; a station of 1.3.2 holds nothing back, and is said to).
  // Unticking a keyed show would let everyone hear what was kept, so it asks once first.
  // A refusal is said in the station's terms (it keeps a show only where playout can keep it
  // from the public) and the tick goes back. It is a producer's and an admin's: anyone else
  // sees the mark on a keyed show and no control. Nothing is drawn at a station older than
  // 1.3.2. Who can listen is the show's, never one episode's: where the tick stands on an
  // episode the words beside it say so.
  import { account } from '../lib/account.svelte.ts';
  import { mayManageKeys, timeWords } from '../lib/keys.ts';
  import { KEEP, KEYED, holdsFiles, keptWords, listenState, openedAgainWords, openedWords, tickTip, ticked } from '../lib/keyed.ts';
  import { APP_VERSION } from '../appVersion.ts';
  import { keyed, keepShow, openToEveryone, ruledShow } from '../lib/keys.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';

  let {
    slug,
    name,
    id,
    compact = false,
    plain = false,
    tabindex = 0,
  }: {
    slug: string;
    name: string;
    /** A prefix for its ids: unique on the page. */
    id: string;
    /** In a row of a list: the tick alone, with what it did under the row. */
    compact?: boolean;
    /** Where the words beside it already say how the show stands (an episode's line, a row
     *  with its own column for it): the tick does not say again that the show has opened. */
    plain?: boolean;
    tabindex?: number;
  } = $props();

  const may = $derived(mayManageKeys(account.me));
  const row = $derived(ruledShow(slug));
  const st = $derived(listenState(row));
  const on = $derived(ticked(st));
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  /** When a show that was kept opened to everyone, in the station's time. */
  const openedAt = $derived(st === 'opened' && row?.opens_at_ms != null ? timeWords(row.opens_at_ms, zone) : null);
  /** The station holds a keyed show's podcast feed and episode files back (1.3.3). */
  const held = $derived(holdsFiles(studio.stationVersion, APP_VERSION));
  /** What the tick's state means and what a press does: the label's tooltip, and the tick's
   *  description for a screen reader. */
  const tip = $derived(tickTip(st, { opened: openedAt, held }));
  let busy = $state(false);
  let problem = $state('');
  let said = $state('');
  /** Unticking a keyed show asks first. */
  let asking = $state(false);
  let box = $state<HTMLInputElement | null>(null);

  async function press(e: Event) {
    const want = (e.currentTarget as HTMLInputElement).checked;
    problem = '';
    said = '';
    if (!want) {
      // Not yet: the box stays ticked until the answer.
      (e.currentTarget as HTMLInputElement).checked = true;
      asking = true;
      return;
    }
    busy = true;
    try {
      await keepShow(slug);
      said = keptWords(name, held);
    } catch (err) {
      problem = err instanceof Error ? err.message : 'The station did not answer. Try again.';
      if (box) box.checked = false;
    } finally {
      busy = false;
    }
  }

  async function open() {
    busy = true;
    try {
      await openToEveryone(slug);
      asking = false;
      said = openedAgainWords(name, held);
    } catch (err) {
      problem = err instanceof Error ? err.message : 'The station did not answer. Try again.';
    } finally {
      busy = false;
      box?.focus();
    }
  }
</script>

{#if keyed.state === 'ok'}
  {#if may}
    <span class="keyedtick" class:compact class:on data-testid="keyed-tick" data-show={slug} data-listen={st}>
      <label title={tip}>
        <input bind:this={box} type="checkbox" checked={on} disabled={busy} {tabindex} aria-describedby="{id}-kt-tip {id}-kt-say" onchange={press} />
        <svg aria-hidden="true"><use href="#g-key" /></svg><span class="w">{KEEP.tick}</span><span class="sr-only">: {name}</span>
      </label>
      <!-- The tooltip's words, for a screen reader: what the state means and what a press does. -->
      <span id="{id}-kt-tip" hidden>{tip}</span>
      <span id="{id}-kt-say" class="say">
        {#if asking}
          <span class="ask" role="group" aria-label="Open {name} to everyone">
            <span>Open {name} to everyone again? Its keys are kept{held ? ', and its episodes return to the podcast feed' : ''}.</span>
            <button type="button" class="btn sm danger-hollow" disabled={busy} aria-busy={busy || undefined} onclick={open}>Open to everyone</button>
            <!-- svelte-ignore a11y_autofocus -->
            <button type="button" class="btn sm quiet" autofocus onclick={() => { asking = false; box?.focus(); }}>Keep it keyed</button>
          </span>
        {:else if busy}
          <span role="status">Saving…</span>
        {:else if problem}
          <span class="err" role="alert">{problem}</span>
        {:else if said}
          <span role="status" data-testid="keyed-tick-said">{said}</span>
        {:else if st === 'opened' && !plain}
          <span data-testid="keyed-tick-opened">{openedWords(openedAt)}</span>
        {/if}
      </span>
    </span>
  {:else if st === 'key'}
    <span class="keyedmark keyed" title={KEYED.tip}><svg aria-hidden="true"><use href="#g-key" /></svg>{KEYED.word}</span>
  {/if}
{/if}
