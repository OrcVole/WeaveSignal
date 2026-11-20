<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Where one show comes from, and its hosts' keys (ADR-0280; hosts-in-their-own-studios.md
  // section 4.4), drawn two ways:
  //
  //   as="field"   the show editor's Comes from (`calendar.show-source`): The station's room
  //                or The host's own studio, and under the second each host with the way in,
  //                Details…, New key… and End key…, or No key yet with Set up a host….
  //   as="block"   Settings › Your show's Broadcast from your own software, for the presenter.
  //
  // The station decides where a show comes from by its keys: `push` when one is made, `room`
  // again when the last one ends. So this field is read from the keys and changed through
  // them: choosing The host's own studio offers the setup, and the show is the host's once
  // the key is made; choosing The station's room on a show with keys says they are to end.
  // On a station older than 1.1, or for someone who may not read the show's keys, nothing
  // is drawn.
  import { untrack } from 'svelte';
  import { listShowKeys, listSources, openInBrowser, sandboxed, type KeyList, type SourceNow } from '../lib/api.ts';
  import { monitors } from '../lib/monitor.svelte.ts';
  import { placeError } from '../lib/account.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { WAY_WORD, hostRows, routeAbsent, type HostRow } from '../lib/hosts.ts';
  import Field from '../schedule/Field.svelte';
  import HostKeyFlow from './HostKeyFlow.svelte';
  import PullSource from './PullSource.svelte';

  let {
    slug,
    name,
    as = 'field',
    who = null,
    initial = null,
    now: fixedNow = undefined,
  }: {
    slug: string;
    name: string;
    as?: 'field' | 'block';
    /** Who the setup is for unless told otherwise: the presenter, in Your show. */
    who?: { label: string; staffId: string | null } | null;
    /** A test's: the show's keys and what is connected. */
    initial?: { list: KeyList; sources: SourceNow[] } | null;
    now?: number;
  } = $props();

  const seed = untrack(() => initial);
  let list = $state<KeyList | null>(seed?.list ?? null);
  let live = $state<SourceNow[]>(seed?.sources ?? []);
  /** Older than 1.1, no such show yet, not this person's to read, or the sandbox, which has no
   *  hosts: the field is left out. */
  let hidden = $state(sandboxed());
  let error = $state('');
  let note = $state('');
  /** What was picked here before the keys say the same. */
  let want = $state<'room' | 'push' | 'pull' | null>(null);
  let flow = $state<HostKeyFlow | null>(null);

  const which = $derived(slug);
  let seq = 0;
  async function load() {
    const mine = ++seq;
    const s = untrack(() => which);
    try {
      const [l, src] = await Promise.all([listShowKeys(s), listSources().catch(() => null)]);
      if (mine !== seq) return;
      list = l;
      live = src?.sources ?? [];
      error = '';
      // The keys now say what was picked: the pick has done its work.
      if (want === l.source) want = null;
    } catch (e) {
      if (mine !== seq) return;
      const status = (e as { status?: number } | null)?.status;
      // Only the first read decides whether the field is drawn: a later one that fails must not
      // take the dialogs away with a secret still showing.
      if (!list && (routeAbsent(e) || status === 403)) hidden = true;
      else error = placeError(e, []).message;
    }
  }
  $effect(() => {
    void which;
    if (seed === null && !sandboxed()) untrack(() => void load());
  });

  const rows = $derived(list ? hostRows([{ slug, name, colour_slot: 13 }], [list], live, untrack(() => fixedNow) ?? Date.now(), sched.zone).live : []);
  const comes = $derived<'room' | 'push' | 'pull'>(want ?? list?.source ?? 'room');
  /** The address a pull opens (1.2, PLAN 17b), and what the station lists of its key now. */
  const pullRow = $derived(rows.find((r) => r.key.pull_url) ?? null);
  const pullUrl = $derived(pullRow?.key.pull_url ?? null);
  const pullNow = $derived(pullRow ? (live.find((x) => x.key_id === pullRow.key.key_id) ?? null) : null);
  /** A show comes from one place: a host's keys end before a pull is made, and a pull before a host is set up. */
  const pushKeys = $derived(rows.filter((r) => !r.key.pull_url).length);
  const choice = $derived([{ slug, name, holders: rows.map((r) => r.key.label) }]);

  function pick(to: 'room' | 'push' | 'pull') {
    note = '';
    want = to === list?.source ? null : to;
  }
  /** Whether the host behind a key has their monitor link open; null when they are not connected. */
  const monitorOf = (keyId: string): 'open' | 'closed' | null => {
    const now = live.find((x) => x.key_id === keyId);
    return now && now.state !== 'not_connected' ? (now.monitor_open ? 'open' : 'closed') : null;
  };
  /** Your show: the presenter's own monitor link, where this window holds it (it made the key).
   *  The station keeps only a scrambled form, so no other window can open it from here. */
  const myMonitor = $derived(rows.map((r) => monitors.links[r.key.key_id]).find(Boolean) ?? null);
  async function openMonitor() {
    if (!myMonitor) return;
    if (await openInBrowser(myMonitor)) return void (note = 'Your monitor is open in your browser. Press Start listening there.');
    try {
      await navigator.clipboard.writeText(myMonitor);
      note = 'Could not open a browser from here. Your monitor link is copied: paste it into your browser.';
    } catch {
      note = 'Could not open a browser from here. Open the monitor link from your details.';
    }
  }
  const NO_LINK_WHY = 'This window does not hold your monitor link: it was shown once, with your details. Open it from where you kept it, or make a new key here.';

  function setUp() {
    flow?.setup({ show: slug, fixed: true, ...(rows.length === 0 && who ? { label: who.label, staffId: who.staffId } : {}) });
  }
</script>

{#snippet keys()}
  {#if rows.length}
    <ul class="keys" aria-label="Hosts with a key for {name}">
      {#each rows as r (r.key.id)}{@render keyRow(r)}{/each}
    </ul>
  {/if}
{/snippet}

{#snippet keyRow(r: HostRow)}
  <li data-key={r.key.key_id}>
    <span class="k-who">
      <b>{r.key.label}</b>
      <span class="k-how">{WAY_WORD[r.key.hint] ?? r.key.hint}{r.status.lines[0] ? ` · ${r.status.lines[0]}` : ''}</span>
    </span>
    <span class="k-acts">
      <button type="button" class="btn sm quiet" onclick={() => flow?.details(r.key, name, monitorOf(r.key.key_id))}>Details…<span class="sr-only">{` for ${r.key.label}`}</span></button>
      <button type="button" class="btn sm quiet" disabled={r.key.ends_on_disconnect} onclick={() => flow?.renew(r.key, { slug, name }, r.status)}>New key…<span class="sr-only">{` for ${r.key.label}`}</span></button>
      <button type="button" class="btn sm quiet" onclick={() => flow?.end(r.key, { slug, name }, r.status, r.last)}>End key…<span class="sr-only">{` for ${r.key.label}`}</span></button>
    </span>
  </li>
{/snippet}

{#if !hidden && (list || error)}
  {#if as === 'field'}
    <Field
      label="Comes from"
      marker={list?.source === 'push' || list?.source === 'pull' ? 'set' : 'own'}
      plain
      panel="calendar.show-source"
      hint="From the room, from a host’s own software with a key that belongs to this show, or from a stream the station opens itself. A host or a stream arrives standing by, and goes on air at the show’s time."
    >
      <span class="seg" role="group" aria-label="Where {name} comes from">
        <button type="button" aria-pressed={comes === 'room'} onclick={() => pick('room')}>The station’s room</button>
        <button type="button" aria-pressed={comes === 'push'} onclick={() => pick('push')}>The host’s own studio</button>
        <button type="button" aria-pressed={comes === 'pull'} title="A partner station’s stream, or a host’s own server: the station opens it itself" onclick={() => pick('pull')}>A stream the station pulls</button>
      </span>
      {#if error}
        <p class="src-note err" role="alert">{error}</p>
      {:else if comes === 'pull'}
        <PullSource
          {slug}
          {name}
          url={pullUrl}
          now={pullNow}
          blocked={pushKeys ? `${name} comes from the host’s own studio now. A show comes from one place: end ${pushKeys === 1 ? 'the key' : 'each key'} below, then pull a stream.` : ''}
          onMade={load}
        />
        {@render keys()}
      {:else if comes === 'push' && pullRow}
        <p class="src-note" data-testid="pull-first">
          <span>{name} is pulled by the station now. A show comes from one place: end the pull’s key below, then set up a host.</span>
        </p>
        {@render keys()}
      {:else if comes === 'push'}
        {@render keys()}
        {#if rows.length === 0}
          <p class="src-note" data-testid="no-key">
            <span><b>No key yet.</b> {name} comes from the host’s own studio once a key is made.</span>
            <button type="button" class="btn sm primary" onclick={setUp}>Set up a host…</button>
          </p>
        {:else}
          <p class="src-note plain"><button type="button" class="btn sm" onclick={setUp}>Add a host…</button><span>A co-host or a stand-in gets a key of their own.</span></p>
        {/if}
      {:else if rows.length}
        <p class="src-note" data-testid="back-to-room">
          <span>{name} comes from the room again when its last key ends. End {rows.length === 1 ? 'the key' : 'each key'} below.</span>
        </p>
        {@render keys()}
      {/if}
      {#if note}<p class="src-note plain" role="status">{note}</p>{/if}
    </Field>
  {:else}
    <div class="own" data-testid="my-show-own">
      <h3>Broadcast from your own software</h3>
      {#if error}
        <p class="err" role="alert">{error}</p>
      {:else if rows.length}
        <p class="help">{name} comes from your own studio. Your software sends with these details; you arrive standing by, and go on air at your show’s time.</p>
        {@render keys()}
      {:else}
        <p class="help">Present {name} from a studio of your own, with software such as BUTT, Mixxx or OBS, instead of joining the room. You get a key for this show.</p>
        <div><button type="button" class="btn" onclick={setUp}>Broadcast from your own software…</button></div>
      {/if}
      {#if rows.length && !error}
        <div class="mon" data-testid="my-monitor">
          <button type="button" class="btn" aria-disabled={myMonitor ? undefined : 'true'} title={myMonitor ? 'Opens your monitor link in your browser' : NO_LINK_WHY} onclick={openMonitor}>Open my monitor</button>
          <p class="help small">
            {#if myMonitor}
              Your monitor link, in your browser: it says whether the station hears you, so it is also how you test your connection.
            {:else}
              Your monitor link came with your details, shown once. Keep it open beside your software: it says whether the station hears you, so it is also how you test your connection. This window can
              open it only for a key made here, until it is closed.
            {/if}
          </p>
        </div>
      {/if}
      {#if note}<p class="small" role="status">{note}</p>{/if}
    </div>
  {/if}
  <HostKeyFlow bind:this={flow} shows={choice} onChanged={load} onNote={(n) => (note = n)} />
{/if}

<style>
  .seg {
    justify-self: start;
  }
  .keys {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-1);
  }
  .keys li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-2);
    flex-wrap: wrap;
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    padding: var(--s-1) var(--s-2);
    background: var(--surface-2);
    font-size: var(--fs-14);
    line-height: var(--lh-label);
  }
  .k-who {
    display: grid;
    min-width: 0;
  }
  .k-how {
    font-size: var(--fs-12);
    color: var(--ink-2);
  }
  .k-acts {
    display: flex;
    gap: var(--s-1);
    flex-wrap: wrap;
  }
  .src-note {
    margin: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-2);
    flex-wrap: wrap;
    font-size: var(--fs-13);
    line-height: var(--lh-label);
    color: var(--ink);
    background: var(--surface-2);
    border-left: 3px solid var(--accent);
    border-radius: var(--r-1);
    padding: var(--s-2);
  }
  .src-note.plain {
    background: transparent;
    border-left: 0;
    padding: 0;
    color: var(--ink-2);
    justify-content: flex-start;
  }
  .src-note.err,
  .err {
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
  }
  .own {
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
    display: grid;
    gap: var(--s-2);
    max-width: 60rem;
  }
  .own h3 {
    font-size: var(--fs-16);
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    margin: 0;
  }
  .help.small {
    font-size: var(--fs-13);
  }
  .mon {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--s-3);
    align-items: center;
  }
</style>
