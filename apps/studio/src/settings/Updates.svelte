<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Updates (ADR-0015, ADR-0140): the studio app's own updates, from the project's
  // signed feed. It says when a newer version is out, with the notes, and installs it on request;
  // it never installs by itself, and it refuses while a strip is open or a talk key is held
  // (lib/update.ts). After an AppImage update, the version before it is kept for a rollback.
  // A browser build has no updater, so the panel is not drawn there. Panel name
  // `settings.updates` (ADR-0070).
  import { onMount } from 'svelte';
  import { packaged } from '../lib/api.ts';
  import { checkNow, gate, installNow, rollbackNow, updates } from '../lib/update.svelte.ts';
  import { dateWords, progressWords } from '../lib/update.ts';

  const here = packaged();
  const g = $derived(gate());
  const check = $derived(updates.check);
  const available = $derived(check?.available ?? null);
  const busy = $derived(updates.phase !== 'idle');
  /** What stops the install now: the desk's state, or the refusal the last click met. */
  const why = $derived(g.blocked ?? updates.refused);

  // Opening Settings asks the feed once if it has not been asked yet (the frame asks a little
  // after the app opens).
  onMount(() => {
    if (here && updates.check === null) void checkNow();
  });
</script>

{#if here}
  <section class="panel" data-panel="settings.updates" aria-labelledby="upd-h">
    <div class="ph">
      <h2 id="upd-h" data-panel-title>Updates</h2>
      {#if check}<span class="small">WeaveSignal {check.current}</span>{/if}
    </div>

    {#if updates.error}
      <div class="callout serious" role="alert">
        <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
        <div>
          <p>{updates.error}</p>
          <button type="button" class="btn sm" disabled={busy} onclick={checkNow}>Check again</button>
        </div>
      </div>
    {/if}

    {#if !check}
      {#if updates.phase === 'checking'}<p role="status" class="small">Checking for updates…</p>{/if}
    {:else if !check.enabled}
      <p class="help">
        This build does not update itself: it was made without the project’s update key. Install a newer package
        by hand when there is one.
      </p>
    {:else if available}
      <div class="callout" class:good={!available.skipped} role="status">
        <svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg>
        <div>
          <p>
            <strong>Version {available.version} is available.</strong>
            {#if available.date !== null}Released {dateWords(available.date)}.{/if}
            {#if available.skipped}You rolled back from this version.{/if}
          </p>
          {#if available.notes}<p class="notes" data-testid="update-notes">{available.notes}</p>{/if}
        </div>
      </div>

      {#if why}
        <p class="callout warning" role="status" data-testid="update-refusal">
          <svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>
          <span>{why}</span>
        </p>
      {:else if g.caution}
        <p class="small" role="note">{g.caution}</p>
      {/if}

      <div class="acts">
        <button type="button" class="btn primary" disabled={busy || !!g.blocked} onclick={installNow}>
          {#if updates.phase === 'downloading'}Downloading…{:else if updates.phase === 'installing'}Installing…{:else if available.downloaded}Install and restart{:else}Download and install{/if}
        </button>
        <button type="button" class="btn" disabled={busy} onclick={checkNow}>Check again</button>
      </div>
      {#if updates.phase === 'downloading' && updates.progress}
        <p class="small" role="status" aria-live="polite">{progressWords(updates.progress.done, updates.progress.total)}</p>
      {:else}
        <p class="hint">
          The update is checked against the project’s signature before anything is installed. The studio app
          restarts when it is done; nothing happens until you ask.
        </p>
      {/if}
    {:else}
      <p class="help" role="status">
        You have the latest version.{#if updates.checkedAt} Checked {new Date(updates.checkedAt).toLocaleTimeString()}.{/if}
      </p>
      <div class="acts">
        <button type="button" class="btn" disabled={busy} onclick={checkNow}>
          {updates.phase === 'checking' ? 'Checking…' : 'Check now'}
        </button>
      </div>
    {/if}

    {#if check?.previous}
      <div class="back" data-testid="update-rollback">
        <p class="help">
          Version {check.previous} is kept from before the last update. If this version misbehaves, roll back to
          it; the studio app restarts, and this version is not offered again until a newer one is out.
        </p>
        <div class="acts">
          <button type="button" class="btn" disabled={busy || !!g.blocked} onclick={rollbackNow}>
            {updates.phase === 'rolling-back' ? 'Rolling back…' : `Roll back to ${check.previous}`}
          </button>
        </div>
      </div>
    {/if}
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
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .notes {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-width: 60rem;
    margin-top: var(--s-2);
    color: var(--ink-2);
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
  .callout.good {
    border-color: var(--status-good);
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
    flex-wrap: wrap;
  }
  .back {
    display: grid;
    gap: var(--s-2);
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
  }
</style>
