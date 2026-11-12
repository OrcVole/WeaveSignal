<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Send to another Icecast server (ADR-0226): whether the programme goes to a DJ's
  // AzuraCast (or any Icecast server) as a source, and when: while the desk is on air (the
  // default), always, or never. The server, its user and its password are the deployment's
  // settings (the how-to says where); this panel shows whether the connection holds and the last
  // error in words, and never the password. An admin's panel. Panel name `settings.icecast-out`
  // (ADR-0070).
  import { onDestroy, onMount, untrack } from 'svelte';
  import { placeError } from '../lib/account.ts';
  import {
    MODES,
    chooseMode,
    errorWords,
    icecastOut,
    sizeWords,
    statusWords,
    type IcecastOut,
    type Mode,
  } from '../lib/icecastOut.ts';

  // `admin`: whoever is looking can change the station's settings (ADR-0246); a DJ is told to ask.
  let { initial = null, admin = true }: { initial?: IcecastOut | null; admin?: boolean } = $props();

  const seed = untrack(() => initial);
  let view = $state<IcecastOut | null>(seed);
  let loadError = $state('');
  let error = $state('');
  let busy = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;

  const status = $derived(view ? statusWords(view) : null);
  const lastError = $derived(view ? errorWords(view) : '');

  async function refresh() {
    try {
      view = await icecastOut();
      loadError = '';
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  onMount(() => {
    if (seed === null) void refresh();
    // The connection comes and goes with the show: look again every few seconds.
    timer = setInterval(() => void refresh(), 4000);
  });
  onDestroy(() => clearInterval(timer));

  async function onChoose(mode: Mode) {
    if (busy || !view || view.mode === mode) return;
    busy = true;
    error = '';
    try {
      view = await chooseMode(mode);
    } catch (e) {
      error = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" data-panel="settings.icecast-out" aria-labelledby="ice-h">
  <div class="ph">
    <h2 id="ice-h" data-panel-title>Send to another Icecast server</h2>
    {#if view?.configured}<span class="small">{view.bitrate} kbit/s MP3</span>{/if}
  </div>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load this.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !view || !status}
    <p role="status" class="small">Loading…</p>
  {:else if !view.configured}
    <p class="help">
      This station does not send its programme to another Icecast server, such as a DJ’s AzuraCast.
      {#if admin}
        To turn it on, set <code>ICECAST_OUT</code> and the server’s details in the station’s settings; the how-to “Send
        your programme to AzuraCast or another Icecast server” has the steps.
      {:else}
        Ask your station’s admin to turn it on: it is a setting of the station, with the other server’s details.
      {/if}
    </p>
  {:else}
    <p class="help">
      The programme listeners hear, with the profanity delay and Dump already in it, goes to the other server as a
      source. Its station leaves its own automation while yours is live.
    </p>

    <div class="callout {status.tone}" role="status" aria-live="polite">
      <svg class="g-{status.tone}" aria-hidden="true"
        ><use href={status.tone === 'good' ? '#g-good' : status.tone === 'plain' ? '#g-standby' : '#g-fault'} /></svg
      >
      <div>
        <p><strong>{status.text}</strong></p>
        {#if view.connected}<p class="small">{sizeWords(view.bytes_sent)} sent since the station started.</p>{/if}
        {#if lastError}<p class="small">Last error: {lastError}</p>{/if}
      </div>
    </div>

    <fieldset disabled={busy}>
      <legend>When to send</legend>
      {#each MODES as m (m.mode)}
        <label class="choice">
          <input
            type="radio"
            name="ice-mode"
            value={m.mode}
            checked={view.mode === m.mode}
            onchange={() => onChoose(m.mode)}
          />
          <span>
            <strong>{m.label}</strong>
            <span class="hint">{m.help}</span>
          </span>
        </label>
      {/each}
    </fieldset>
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}
</section>

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
  .ph {
    display: flex;
    align-items: baseline;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
  .ph h2 {
    font-size: var(--fs-20);
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .hint {
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
    display: grid;
    gap: var(--s-2);
    min-width: 0;
  }
  legend {
    font-weight: 600;
    margin-bottom: var(--s-1);
  }
  .choice {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: start;
    max-width: 40rem;
  }
  .choice input {
    margin-top: 0.3rem;
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  .callout {
    display: grid;
    grid-template-columns: 1.25rem 1fr;
    gap: var(--s-2);
    align-items: start;
    border: 1.5px solid var(--axis);
    border-left-width: 4px;
    border-radius: var(--r-2);
    padding: var(--s-3);
    background: var(--surface);
    line-height: var(--lh-body);
  }
  .callout svg {
    width: 1.25rem;
    height: 1.25rem;
    margin-top: 0.1rem;
  }
  .callout.serious {
    border-color: var(--status-serious);
  }
  .callout.warning {
    border-color: var(--status-warning);
  }
  .callout.good {
    border-color: var(--status-good);
  }
</style>
