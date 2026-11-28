<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Make a show keyed (keyed shows, 1.3.2; the operator's click-through of 2026-11-25): the
  // station's shows, each with the tick that makes it keyed (KeyedTick, the one control) and
  // who can listen to it now. It is step 1 of the three, reached from the Keys tab (where a
  // first key opens nothing until a show is keyed) and from step 1 of Make a key. A tick saves
  // at once and the row says so; a show that has opened to everyone by itself is not ticked,
  // and its row says when it opened (1.3.3); unticking asks first; the station's own reason is shown when
  // it refuses (it keeps a show only where playout can keep it from the public). What the
  // public hears, and a time it opens to everyone, are the show editor's: each row leads
  // there. A show that does not exist yet is a press away: New keyed show….
  // Panel name `keys.keep-show` (ADR-0070).
  import { untrack } from 'svelte';
  import { FILES, KEEP, LISTEN_SHORT, holdsFiles, listenState } from '../lib/keyed.ts';
  import { APP_VERSION } from '../appVersion.ts';
  import { timeWords } from '../lib/keys.ts';
  import { ruledShow } from '../lib/keys.svelte.ts';
  import { toNewKeyedShow, toWhoCanListen } from '../lib/keyedNav.ts';
  import * as schedule from '../lib/schedule.ts';
  import type { Show } from '../lib/schedule.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import KeySteps from './KeySteps.svelte';
  import KeyedTick from './KeyedTick.svelte';

  let {
    onclose,
    onmake,
    shows: seed = null,
  }: {
    onclose: () => void;
    /** Step 2 from here: Make a key. Left out where the dialog lies over Make a key itself. */
    onmake?: () => void;
    /** The shows, a test's: drawn without asking the station. */
    shows?: Show[] | null;
  } = $props();

  let el = $state<HTMLDialogElement | null>(null);
  let shows = $state<Show[] | null>(untrack(() => seed));
  let problem = $state('');
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  /** The station holds a keyed show's podcast feed and episode files back (1.3.3). */
  const held = $derived(holdsFiles(studio.stationVersion, APP_VERSION));

  const rows = $derived([...(shows ?? [])].sort((a, b) => a.name.localeCompare(b.name)));
  const anyKeyed = $derived(rows.some((s) => listenState(ruledShow(s.slug)) === 'key'));

  async function load() {
    problem = '';
    try {
      shows = await schedule.shows();
    } catch (e) {
      problem = e instanceof Error && e.message ? e.message : 'The station did not answer. Try again.';
    }
  }

  $effect(() => {
    el?.showModal();
    untrack(() => {
      if (shows === null) void load();
    });
  });

  function close() {
    el?.close();
    onclose();
  }
  /** A show's own Who can listen, in the show editor: what the public hears, when it opens. */
  function edit(s: Show) {
    close();
    toWhoCanListen(s.slug);
  }
  /** A show that does not exist yet: New show, with Keyed show ticked. */
  function fresh() {
    close();
    toNewKeyedShow();
  }
  const when = (s: Show) => [s.rule_words?.short ?? '', `/${s.slug}`].filter(Boolean).join(' · ');
</script>

<dialog bind:this={el} class="kdialog keepshow" data-panel="keys.keep-show" aria-labelledby="kks-h" oncancel={(e) => { e.preventDefault(); close(); }}>
  <h2 id="kks-h" data-panel-title>Make a show keyed</h2>
  <KeySteps current={1} go={{ 2: onmake ? () => { close(); onmake(); } : undefined }} waits={{ 2: 'Close this to go on with the key.', 3: 'Send opens by itself the moment a key is made.' }} />
  <p class="lead">A keyed show is advertised to everyone and heard only with a key you give out. Tick <b>{KEEP.tick}</b> and the show is kept for people with a key from that moment, for every episode to come. {held ? `${FILES.tick} ${FILES.gone}` : FILES.older}</p>
  {#if problem}
    <div class="callout serious" role="alert">
      <svg aria-hidden="true"><use href="#g-fault" /></svg>
      <div><p><strong>The shows could not be read.</strong> {problem}</p><button type="button" class="btn sm" onclick={load}>Try again</button></div>
    </div>
  {:else if shows === null}
    <p role="status" class="small">Reading the shows…</p>
  {:else if rows.length === 0}
    <div class="callout" data-testid="keep-no-shows">
      <svg aria-hidden="true"><use href="#g-key" /></svg>
      <div>
        <p><strong>The station has no shows yet.</strong> Make the first one keyed from the start.</p>
        <button type="button" class="btn sm primary" onclick={fresh}><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>{KEEP.fresh}…</button>
      </div>
    </div>
  {:else}
    <ul class="keeplist" aria-label="The station’s shows, and who can listen to each">
      {#each rows as s (s.slug)}
        {@const row = ruledShow(s.slug)}
        {@const st = listenState(row)}
        <li style:--show-colour="var(--cart-{s.colour_slot})" data-show={s.slug} data-listen={st}>
          <span class="nm"><b>{s.name}</b><span class="small">{when(s)}</span></span>
          <span class="small st">{st === 'key' && row?.opens_at_ms != null ? `Opens to everyone ${timeWords(row.opens_at_ms, zone)}` : st === 'opened' && row?.opens_at_ms != null ? `Opened to everyone ${timeWords(row.opens_at_ms, zone)}` : LISTEN_SHORT[st]}</span>
          <button type="button" class="linkish" title="What the public hears meanwhile, and when it opens to everyone: in the show’s editor" onclick={() => edit(s)}>{KEEP.change}</button>
          <KeyedTick slug={s.slug} name={s.name} id="kk-{s.slug}" compact plain />
        </li>
      {/each}
    </ul>
    <p class="foot small">The same tick is on each show in the Schedule’s list, at the top of a show’s pane and on its episodes. A show’s editor has the rest under <b>Who can listen</b>: what the public hears meanwhile, and a time the show opens to everyone by itself.</p>
  {/if}
  <div class="acts">
    {#if rows.length > 0}<button type="button" class="btn left" onclick={fresh}><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>{KEEP.fresh}…</button>{/if}
    {#if onmake && anyKeyed}
      <button type="button" class="btn quiet" onclick={close}>Done</button>
      <button type="button" class="btn primary" onclick={() => { close(); onmake(); }}>Next: make a key…</button>
    {:else}
      <button type="button" class="btn" onclick={close}>Done</button>
    {/if}
  </div>
</dialog>
