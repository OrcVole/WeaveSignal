<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Station time zone (ADR-0189): the station's one zone, kept in its database.
  // Until someone chooses it the station offers the zone from its configuration and the panel
  // asks for a yes or another name; there is no silent UTC. Once chosen, a change warns that
  // every stored show slot and clock hour moves with it. An admin's panel. Panel name
  // `settings.zone` (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import * as api from '../lib/schedule.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { placeError } from '../lib/account.ts';
  import { ZONE_CHANGE_WARNING, knownZones, zoneProblem } from '../lib/zone.ts';

  let { initial = null, zones = null }: { initial?: api.StationZone | null; zones?: string[] | null } = $props();

  const seed = untrack(() => initial);
  let current = $state<api.StationZone | null>(seed);
  let draft = $state(seed?.zone ?? '');
  let loadError = $state('');
  let error = $state('');
  let note = $state('');
  let busy = $state(false);
  const known = untrack(() => zones ?? knownZones());

  const asking = $derived(current !== null && !current.set);
  const changed = $derived(current !== null && draft.trim() !== current.zone);

  async function refresh() {
    loadError = '';
    try {
      current = await api.stationZone();
      draft = current.zone;
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  onMount(() => {
    if (seed === null) void refresh();
  });

  async function onSave(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !current) return;
    error = '';
    note = '';
    const name = draft.trim();
    const problem = zoneProblem(name, known);
    if (problem) {
      error = problem;
      return document.getElementById('tz-name')?.focus();
    }
    busy = true;
    try {
      // The zone is one field of the playout settings, which the station takes whole.
      const settings = await api.playoutSettings();
      const saved = await api.putPlayoutSettings({ ...settings, time_zone: name });
      current = { zone: saved.time_zone, set: true };
      draft = saved.time_zone;
      // The Schedule tab follows at once, without waiting for its next fetch.
      sched.zone = saved.time_zone;
      sched.zoneSet = true;
      sched.version++;
      note = `The station’s time zone is ${saved.time_zone}.`;
    } catch (err) {
      error = placeError(err, []).message;
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" data-panel="settings.zone" aria-labelledby="tz-h">
  <div class="ph">
    <h2 id="tz-h" data-panel-title>Station time zone</h2>
    {#if current}<span class="small">{current.set ? current.zone : 'Not chosen yet'}</span>{/if}
  </div>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load the time zone.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !current}
    <p role="status" class="small">Loading the time zone…</p>
  {:else}
    {#if asking}
      <div class="callout warning" role="status">
        <svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>
        <div>
          <p><strong>Choose the station’s time zone.</strong></p>
          <p>
            Shows, episodes and automation’s clock hours all run in one zone. Nobody has chosen it yet, so the station is
            using {current.zone} from its configuration. Confirm it or type another before you schedule anything.
          </p>
        </div>
      </div>
    {:else}
      <p class="help">Shows, episodes and automation’s clock hours all run in this zone.</p>
    {/if}

    <form onsubmit={onSave} novalidate aria-labelledby="tz-h">
      <div class="field">
        <label for="tz-name">Time zone</label>
        <input
          id="tz-name"
          type="text"
          list="tz-list"
          bind:value={draft}
          autocomplete="off"
          autocapitalize="none"
          spellcheck="false"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? 'tz-err tz-hint' : 'tz-hint'}
        />
        <datalist id="tz-list">{#each known as z (z)}<option value={z}></option>{/each}</datalist>
        <span id="tz-hint" class="hint">An IANA name, such as Europe/London or America/New_York.</span>
        {#if error}<p id="tz-err" class="err" role="alert">{error}</p>{/if}
      </div>
      {#if !asking && changed}
        <p class="callout warning plain" role="note">{ZONE_CHANGE_WARNING}</p>
      {/if}
      <div class="acts">
        <button type="submit" class="btn primary" disabled={busy || (!asking && !changed)}>
          {asking ? (changed ? 'Use this time zone' : `Use ${current.zone}`) : 'Change time zone'}
        </button>
      </div>
    </form>
    <p class="small" role="status" aria-live="polite">{note}</p>
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
  .field {
    display: grid;
    gap: var(--s-1);
    max-width: 28rem;
  }
  .field > label {
    font-weight: 600;
  }
  input[type='text'] {
    font: inherit;
    font-family: var(--font-mono);
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
    width: 100%;
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
  .callout.plain {
    display: block;
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
  .acts {
    display: flex;
    gap: var(--s-2);
    margin-top: var(--s-2);
  }
  form {
    display: grid;
    gap: var(--s-3);
  }
</style>
