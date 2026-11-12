<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Your show (ADR-0218): the show tied to the signed-in account, as the setup
  // wizard left it, with the wizard to run again; or the invitation to set one up. Drawn for
  // a member of staff signed in as themselves; the station key has no account to tie a show
  // to. Panel name `settings.my-show` (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import type { Me } from '../lib/api.ts';
  import type { Show } from '../lib/schedule.ts';
  import { loadShows, mine, openSetup, setup } from '../lib/wizard.svelte.ts';
  import { hhmm, WEEKDAYS, myShow } from '../lib/wizard.ts';

  let { me, initial = null }: { me: Me; initial?: Show[] | null } = $props();

  const seed = untrack(() => initial);
  /** The show, from the props (a test's) or the window's last fetch. */
  const show = $derived(seed ? myShow(me, seed) : mine());
  const loaded = $derived(seed !== null || setup.loaded);
  const error = $derived(seed ? '' : setup.error);

  onMount(() => {
    if (seed === null) void loadShows();
  });

  function when(s: Show): string {
    if (!s.slot) return 'No usual time yet';
    const day = WEEKDAYS[s.slot.weekday - 1] ?? 'Saturday';
    return s.slot.minutes ? `${day}s, ${hhmm(s.slot.start)} to ${hhmm(s.slot.start + s.slot.minutes)}` : `${day}s from ${hhmm(s.slot.start)}`;
  }
</script>

<section class="panel" data-panel="settings.my-show" aria-labelledby="ms-h">
  <div class="ph">
    <h2 id="ms-h" data-panel-title>Your show</h2>
    {#if show}<span class="small">/{show.slug}</span>{/if}
  </div>

  {#if error}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load the shows.</strong> {error}</p>
        <button type="button" class="btn sm" onclick={() => void loadShows()}>Try again</button>
      </div>
    </div>
  {:else if !loaded}
    <p role="status" class="small">Loading your show…</p>
  {:else if show}
    <p class="help">What the station keeps about you and your show: callers agree to be recorded under this name and contact while it is on air (ADR-0214), and new episodes start from the usual time.</p>
    <dl class="facts">
      <dt>Show</dt>
      <dd>{show.name}</dd>
      <dt>On-air name</dt>
      <dd>{#if show.presenter}{show.presenter}{:else}<span class="muted">not set: the consent words name no presenter</span>{/if}</dd>
      <dt>Takedown contact</dt>
      <dd>{#if show.contact}{show.contact}<span class="muted">, then the station’s as the host</span>{:else}<span class="muted">the station’s alone</span>{/if}</dd>
      <dt>Links</dt>
      <dd>
        {#if show.links.length}
          <ul class="links">{#each show.links as l (l)}<li>{l}</li>{/each}</ul>
        {:else}<span class="muted">none</span>{/if}
      </dd>
      <dt>Usual time</dt>
      <dd>{when(show)}</dd>
      <dt>Commercial music</dt>
      <dd>{show.commercial_music ? 'Yes: a UK webcast needs PPL and PRS for Music licences' : 'No'}</dd>
    </dl>
    <div class="acts">
      <button type="button" class="btn" onclick={openSetup}>Change…</button>
    </div>
  {:else}
    <p class="help">
      You have not set up a show yet. Three short steps: your on-air name and takedown contact, your show and when it is on, done.
      The answers are kept in the station as the defaults for you and your show; nothing is emailed.
    </p>
    <div class="acts">
      <button type="button" class="btn primary" onclick={openSetup}>Set up your show…</button>
    </div>
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
  .ph .small {
    font-family: var(--font-mono);
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    column-gap: var(--s-4);
    row-gap: var(--s-2);
    margin: 0;
    max-width: 60rem;
    line-height: var(--lh-body);
  }
  dt {
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
    padding-top: 0.15rem;
  }
  dd {
    margin: 0;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .links {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
    font-family: var(--font-mono);
    font-size: var(--fs-13);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .callout {
    display: grid;
    grid-template-columns: 1.25rem 1fr;
    gap: var(--s-2);
    align-items: start;
    border: 1.5px solid var(--status-serious);
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
</style>
