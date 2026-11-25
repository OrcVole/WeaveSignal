<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The Keys tab (keyed shows, 1.3.2; ⌃6): listener keys, a producer's and an admin's. Three
  // views of the same keys: the list (make one, make many, find one, and each key's own page
  // beside it), the families (keys issued together, one a person, with their export), and the
  // leak report (which key most likely was passed on, and why). The screen says what is so at
  // every turn: no sign-in, an account that may not, a station older than keys, keys not set up,
  // no keyed show yet, no key yet, nothing used yet.
  //
  // A first key opens nothing until a show is keyed, and the way to that was not in sight (the
  // operator's click-through, 2026-11-25): Make a show keyed… lists the station's shows with one
  // press each (KeepShow, `keys.keep-show`), from the line that says no show needs a key yet and
  // from What a key does; and the dialogs that make and send a key carry the three steps.
  //
  // A listener key is not a show key: a show key (Settings, Own studios) lets a host's software
  // send to the station. Panel names `keys.toolbar`, `keys.list`, `keys.families`, `keys.leaks`,
  // `keys.about`, `keys.unready`, `keys.find` (ADR-0070); the key's page is KeyPage's.
  import { onMount, untrack } from 'svelte';
  import { account } from '../lib/account.svelte.ts';
  import { run } from '../lib/actions.ts';
  import { KIND_WORD, LEAK_WORD, STANDING, actionProblem, fileStem, fromNow, leakLevel, leakSummary, leaksCsv, limitsWords, mayManageKeys, opensWords, standing, tally, textIn, timeWords, unreadyOf, useWords, windowShort } from '../lib/keys.ts';
  import { familyCsv, findKey, type KeyFamily, type KeyText, type ListenerKey, type MadeFamily, type MadeListenerKey } from '../lib/keysApi.ts';
  import { FieldError } from '../lib/api.ts';
  import { added, closeKey, familyLabel, filtered, keyed, ks, keysUi, loadLeaks, moreKeys, openKey, refreshKeys, selectedKey, setView, type KeysView } from '../lib/keys.svelte.ts';
  import { EXPORT_NOTED, saveLabel, saveText, savedWords } from '../lib/saveFile.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import KeepShow from './KeepShow.svelte';
  import KeyPage from './KeyPage.svelte';
  import MakeKey from './MakeKey.svelte';
  import MakeMany from './MakeMany.svelte';
  import SendKey from './SendKey.svelte';

  const now = $derived(studio.now || Date.now());
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  const shows = $derived(keyed.list?.shows ?? []);
  const may = $derived(mayManageKeys(account.me));
  const selected = $derived(selectedKey());
  const counts = $derived(tally(ks.keys, now, ks.signing?.soonDays));
  const leak = $derived(ks.leaks ? leakSummary(ks.leaks.entries) : null);
  /** The head of the list: how many keys, and how many of those shown stand how. */
  const countLine = $derived(
    ks.total === 0
      ? filtered()
        ? 'None match'
        : 'None yet'
      : `${ks.total.toLocaleString('en-GB')} ${ks.total === 1 ? 'key' : 'keys'}${ks.keys.length < ks.total ? `, the newest ${ks.keys.length} shown` : ` · ${counts.works} on, ${counts.paused} paused, ${counts.over} over`}`,
  );

  // The first read waits until the station has said who this is: an account that may not
  // manage keys is told so here, not refused by the station.
  let started = false;
  $effect(() => {
    if (started || !account.loaded) return;
    started = true;
    untrack(() => {
      if (!account.me) ks.ready = unreadyOf(new FieldError(401, '', null));
      else if (!may) ks.ready = unreadyOf(new FieldError(403, '', null));
      else void refreshKeys();
    });
  });

  // ---- the filter ----
  let searchEl = $state<HTMLInputElement | null>(null);
  let typing: ReturnType<typeof setTimeout> | undefined;
  function filterNow() {
    clearTimeout(typing);
    void refreshKeys();
  }
  function filterSoon() {
    clearTimeout(typing);
    typing = setTimeout(() => void refreshKeys(), 250);
  }
  function clearFilter() {
    ks.filter = { q: '', state: '', kind: '', family: '', show: '' };
    filterNow();
  }
  onMount(() => () => clearTimeout(typing));

  // ---- the dialogs ----
  let making = $state<null | { family: string }>(null);
  let makingMany = $state(false);
  let sending = $state<{ key: ListenerKey; given: KeyText | null; why: 'send' | 'made' | 'replaced' } | null>(null);
  /** Make a show keyed…: the station's shows, each with one press. It may lie over Make a key
   *  or Send, whose step 1 opens it, so what is typed there stays. */
  let keeping = $state(false);
  /** Step 2 from Make a show keyed, or from Send: Make a key, unless one is being made already. */
  function makeNext() {
    keeping = false;
    sending = null;
    if (!making && !makingMany) making = { family: ks.filter.family };
  }
  // What another tab asked for on the way here (New show's Done, a show's Who can listen).
  $effect(() => {
    if (ks.ready !== 'ok' || !keysUi.ask) return;
    const ask = keysUi.ask;
    untrack(() => {
      keysUi.ask = null;
      if (making || makingMany || sending || keeping) return;
      if (ask === 'make') making = { family: '' };
      else keeping = true;
    });
  });

  function send(key: ListenerKey, given: KeyText | null = null, why: 'send' | 'made' | 'replaced' = 'send') {
    sending = { key, given, why };
  }
  function onMade(m: MadeListenerKey) {
    making = null;
    added(m.key);
    ks.note = `${m.key.label}’s key is made.`;
    send(m.key, { text: m.text, link: m.link }, 'made');
  }
  function onMadeMany(m: MadeFamily) {
    ks.families = [m.family, ...ks.families.filter((f) => f.id !== m.family.id)];
    ks.note = `${m.keys.length} ${m.keys.length === 1 ? 'key' : 'keys'} made in ${m.family.label}.`;
    ks.filter = { q: '', state: '', kind: '', family: m.family.id, show: '' };
    ks.view = 'keys';
    void refreshKeys();
  }

  // The keymap's presses (N, /), each acted on once.
  let seenMake = untrack(() => keysUi.make);
  let seenSearch = untrack(() => keysUi.search);
  $effect(() => {
    if (keysUi.make !== seenMake) {
      seenMake = keysUi.make;
      if (ks.ready === 'ok' && !making && !makingMany && !sending && !keeping) making = { family: '' };
    }
    if (keysUi.search !== seenSearch) {
      seenSearch = keysUi.search;
      if (ks.ready === 'ok') {
        if (ks.view !== 'keys') setView('keys');
        queueMicrotask(() => searchEl?.focus());
      }
    }
  });

  // ---- find a key ----
  let findEl = $state<HTMLDialogElement | null>(null);
  let found = $state<ListenerKey | null>(null);
  let findText = $state('');
  let findErr = $state('');
  let finding = $state(false);
  function askFind() {
    findText = '';
    findErr = '';
    found = null;
    findEl?.showModal();
  }
  async function submitFind(e: SubmitEvent) {
    e.preventDefault();
    found = null;
    const text = textIn(findText);
    if (!text) return void (findErr = 'That cannot be a key. A key is twelve letters and digits, written like KETM-7QMP-2D4X; paste the key or a link that carries one.');
    finding = true;
    findErr = '';
    try {
      found = await findKey(text);
    } catch (err) {
      findErr = err instanceof FieldError && err.status === 404 ? 'No key of this station has that text. It may be another station’s, or mistyped.' : actionProblem(err);
    } finally {
      finding = false;
    }
  }
  function openFound() {
    if (!found) return;
    const id = found.id;
    findEl?.close();
    ks.view = 'keys';
    void openKey(id);
  }

  // ---- export ----
  let exporting = $state('');
  let rowProblem = $state<{ id: string; message: string } | null>(null);
  /** A file, saved where the person says (the packaged app's save dialog) or where the browser
   *  keeps downloads. `after` is what stays true when the dialog is closed without saving. */
  async function deliver(name: string, text: string, what: string, after = '') {
    ks.note = savedWords(what, await saveText(name, text), after);
  }
  async function exportFamily(f: KeyFamily) {
    if (exporting) return;
    exporting = f.id;
    rowProblem = null;
    try {
      await deliver(`keys-${fileStem(f.label)}.csv`, await familyCsv(f.id), `${f.label}: every key with its link`, EXPORT_NOTED);
    } catch (e) {
      rowProblem = { id: f.id, message: actionProblem(e) };
    } finally {
      exporting = '';
    }
  }
  async function exportLeaks() {
    if (!ks.leaks) return;
    try {
      await deliver(`leak-report${ks.leaksFamily ? `-${fileStem(familyLabel(ks.leaksFamily) ?? '')}` : ''}.csv`, leaksCsv(ks.leaks.entries, zone), 'The leak report');
    } catch (e) {
      ks.leaksProblem = actionProblem(e);
    }
  }

  function seeFamily(f: KeyFamily) {
    ks.filter = { q: '', state: '', kind: '', family: f.id, show: '' };
    ks.view = 'keys';
    ks.note = '';
    void refreshKeys();
  }
  function leaksOf(f: KeyFamily) {
    ks.view = 'leaks';
    ks.note = '';
    void loadLeaks(f.id);
  }

  const VIEWS: readonly { view: KeysView; label: string }[] = [
    { view: 'keys', label: 'Keys' },
    { view: 'families', label: 'Families' },
    { view: 'leaks', label: 'Leak report' },
  ];
</script>

<div class="keys" data-surface="keys">
  {#if ks.ready === 'loading'}
    <div class="center"><p role="status" class="small">Reading the keys…</p></div>
  {:else if ks.ready !== 'ok'}
    <div class="center">
      <section class="panel unready" data-panel="keys.unready" data-unready={ks.ready.kind} aria-labelledby="ku-title">
        <svg class="big" aria-hidden="true"><use href="#g-key" /></svg>
        <h2 id="ku-title" data-panel-title>{ks.ready.title}</h2>
        <p>{ks.ready.body}</p>
        {#if ks.ready.said}<p class="said mono" data-testid="station-said">{ks.ready.said}</p>{/if}
        {#if ks.ready.retry}<button type="button" class="btn" onclick={() => { ks.ready = 'loading'; void refreshKeys(); }}>Try again</button>{/if}
      </section>
    </div>
  {:else}
    <div class="toolbar" data-panel="keys.toolbar">
      <h1 data-panel-title>Keys <span class="sub">for listeners</span></h1>
      <div class="seg" role="group" aria-label="View">
        {#each VIEWS as v (v.view)}
          <button type="button" aria-pressed={ks.view === v.view} onclick={() => setView(v.view)}>{v.label}{#if v.view === 'leaks' && leak && leak.signs > 0}<span class="dot" aria-label="{leak.signs} with a sign">{leak.signs}</span>{/if}</button>
        {/each}
      </div>
      <span class="spacer"></span>
      <button type="button" class="btn" aria-haspopup="dialog" onclick={askFind}>Find a key…</button>
      <button type="button" class="btn" aria-haspopup="dialog" onclick={() => (makingMany = true)}>Make many…</button>
      <button type="button" class="btn primary" aria-haspopup="dialog" onclick={() => (making = { family: ks.filter.family })}>Make a key… <kbd>N</kbd></button>
    </div>

    <div class="body" class:with-page={!!selected}>
      <div class="main">
        {#if ks.note}<p class="noteline" role="status" data-testid="keys-note"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg>{ks.note}</p>{/if}
        {#if keyed.state === 'ok' && shows.length === 0}
          <div class="callout" data-testid="no-keyed-shows">
            <svg aria-hidden="true"><use href="#g-key" /></svg>
            <div>
              <p><strong>No show needs a key yet.</strong> A key made now opens nothing until a show is kept for people with a key. Choose the show here, or open it on the Schedule tab and set <b>Who can listen</b> to <b>People with a key</b>.</p>
              <div class="row">
                <button type="button" class="btn sm primary" aria-haspopup="dialog" onclick={() => (keeping = true)}>Make a show keyed…</button>
                <button type="button" class="btn sm" onclick={() => void run({ type: 'tab', tab: 'schedule' })}>Open the Schedule</button>
              </div>
            </div>
          </div>
        {/if}

        {#if ks.view === 'keys'}
          <section class="panel list" data-panel="keys.list" aria-labelledby="kl-h">
            <div class="ph">
              <h2 id="kl-h" data-panel-title>{ks.filter.family ? familyLabel(ks.filter.family) : 'Every key'}</h2>
              <span class="small" data-testid="keys-count">
                {countLine}
              </span>
              {#if filtered()}<button type="button" class="btn sm quiet" onclick={clearFilter}>Clear the filter</button>{/if}
              <span class="spacer"></span>
              {#if (ks.all ?? 0) > 0}
                <div class="filters" role="group" aria-label="Narrow the list">
                  <label class="search"><span class="sr-only">Search by name or note</span><input bind:this={searchEl} type="search" placeholder="Search a name or a note" bind:value={ks.filter.q} oninput={filterSoon} /><kbd aria-hidden="true">/</kbd></label>
                  <label class="pick"><span class="sr-only">State</span>
                    <select bind:value={ks.filter.state} onchange={filterNow}>
                      <option value="">Any state</option><option value="active">On</option><option value="paused">Paused</option><option value="revoked">Revoked</option>
                    </select>
                  </label>
                  <label class="pick"><span class="sr-only">Family</span>
                    <select bind:value={ks.filter.family} onchange={filterNow}>
                      <option value="">Every family</option>
                      {#each ks.families as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
                    </select>
                  </label>
                  <label class="pick"><span class="sr-only">Show</span>
                    <select bind:value={ks.filter.show} onchange={filterNow}>
                      <option value="">Any show</option>
                      {#each shows as s (s.id)}<option value={s.id}>Opens {s.name}</option>{/each}
                    </select>
                  </label>
                </div>
              {/if}
            </div>
            {#if ks.total === 0 && !filtered()}
              <div class="empty" data-testid="no-keys">
                <p><strong>No keys yet.</strong> A key lets one listener, or one group, hear the shows you keep for key holders. Make one for a person, or many at once from a list.</p>
                <div class="row">
                  <button type="button" class="btn primary" onclick={() => (making = { family: '' })}>Make a key…</button>
                  <button type="button" class="btn" onclick={() => (makingMany = true)}>Make many…</button>
                </div>
              </div>
            {:else if ks.total === 0}
              <p class="empty">No key matches. Change the search or the filter, or clear it.</p>
            {:else}
              <div class="table-wrap">
                <table>
                  <caption class="sr-only">Listener keys: who each is for, what it opens, when it works, its limits, its use and where it stands</caption>
                  <thead>
                    <tr><th scope="col">For</th><th scope="col">Opens</th><th scope="col">Works</th><th scope="col">Limits</th><th scope="col">Use</th><th scope="col">State</th></tr>
                  </thead>
                  <tbody>
                    {#each ks.keys as k (k.id)}
                      {@const st = standing(k, now, ks.signing?.soonDays)}
                      <tr class:off={st === 'ended' || st === 'revoked' || st === 'replaced' || st === 'used-up'} class:sel={ks.selected === k.id}>
                        <th scope="row">
                          <button type="button" class="rowbtn" aria-current={ks.selected === k.id ? 'true' : undefined} onclick={() => (ks.selected === k.id ? closeKey() : openKey(k.id))}>{k.label}</button>
                          <span class="small block">{[KIND_WORD[k.kind], ks.filter.family ? null : familyLabel(k.family_id), k.note || null].filter(Boolean).join(' · ')}</span>
                        </th>
                        <td>{opensWords(k, shows)}</td>
                        <td>{windowShort(k, now, zone)}</td>
                        <td>{limitsWords(k)}</td>
                        <td>{useWords(k, now)}</td>
                        <td><span class="state sm {STANDING[st].cls}">{STANDING[st].word}</span></td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
              {#if ks.keys.length < ks.total}
                <div class="more"><button type="button" class="btn" disabled={ks.loadingMore} aria-busy={ks.loadingMore || undefined} onclick={moreKeys}>{ks.loadingMore ? 'Reading…' : `Show more (${(ks.total - ks.keys.length).toLocaleString('en-GB')} left)`}</button></div>
              {/if}
            {/if}
          </section>
        {:else if ks.view === 'families'}
          <section class="panel list" data-panel="keys.families" aria-labelledby="kf-h">
            <div class="ph">
              <h2 id="kf-h" data-panel-title>Families</h2>
              <span class="small">Keys issued together, one a person, so that a key found where it should not be says whose it was.</span>
            </div>
            {#if ks.families.length === 0}
              <div class="empty" data-testid="no-families">
                <p><strong>No families yet.</strong> Make many keys at once, from a list of people or as numbered tickets, and they are a family: each copy its own key, traced and replaced on its own.</p>
                <div class="row"><button type="button" class="btn primary" onclick={() => (makingMany = true)}>Make many…</button></div>
              </div>
            {:else}
              <div class="table-wrap">
                <table>
                  <caption class="sr-only">Key families: each family, how many keys it has issued, who made it and when, and what to do with it</caption>
                  <thead><tr><th scope="col">Family</th><th scope="col">Keys</th><th scope="col">Made</th><th scope="col"><span class="sr-only">Actions</span></th></tr></thead>
                  <tbody>
                    {#each ks.families as f (f.id)}
                      <tr>
                        <th scope="row">{f.label}{#if f.note}<span class="small block">{f.note}</span>{/if}</th>
                        <td class="num">{f.keys.toLocaleString('en-GB')}</td>
                        <td>{timeWords(f.created_ms, zone)}<span class="small block">by {f.created_by}</span></td>
                        <td class="acts">
                          <button type="button" class="btn sm" onclick={() => seeFamily(f)}>See its keys</button>
                          <button type="button" class="btn sm" onclick={() => leaksOf(f)}>Leak report</button>
                          <button type="button" class="btn sm" disabled={!!exporting} aria-busy={exporting === f.id || undefined} title="Every key of the family with its link, as CSV. The station notes on each key that its text went out." onclick={() => exportFamily(f)}>{saveLabel('Export CSV')}</button>
                        </td>
                      </tr>
                      {#if rowProblem?.id === f.id}<tr class="row-err"><td colspan="4"><p class="err" role="alert">{rowProblem.message}</p></td></tr>{/if}
                    {/each}
                  </tbody>
                </table>
              </div>
              <p class="small foot">An export holds every key’s text: keep the file as you would a list of passwords. The station notes on each key that its text went out.</p>
            {/if}
          </section>
        {:else}
          <section class="panel list" data-panel="keys.leaks" aria-labelledby="kk-h">
            <div class="ph">
              <h2 id="kk-h" data-panel-title>Leak report</h2>
              <span class="small">The key most likely passed on is first.</span>
              <span class="spacer"></span>
              <label class="pick"><span class="sr-only">Family</span>
                <select value={ks.leaksFamily} onchange={(e) => void loadLeaks(e.currentTarget.value)}>
                  <option value="">Every key</option>
                  {#each ks.families as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
                </select>
              </label>
              <button type="button" class="btn sm" disabled={ks.leaksLoading} onclick={() => loadLeaks()}>Read again</button>
              <button type="button" class="btn sm" disabled={!ks.leaks || ks.leaks.entries.length === 0} title="Counts and signs, never a key’s text or an address" onclick={exportLeaks}>{saveLabel('Export CSV')}</button>
            </div>
            {#if ks.leaksProblem}
              <div class="callout serious" role="alert">
                <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
                <div><p><strong>The report could not be read.</strong> {ks.leaksProblem}</p><button type="button" class="btn sm" onclick={() => loadLeaks()}>Try again</button></div>
              </div>
            {:else if !ks.leaks}
              <p role="status" class="small">Reading the report…</p>
            {:else}
              <p class="summary" data-testid="leak-summary"><b>{leak?.line}</b> A sign is a hint, not a verdict: a phone on mobile data changes address, so the report adds several signs up and says which. An address is never shown, only counted.</p>
              {#if ks.leaks.entries.length === 0}
                <p class="empty">{ks.leaksFamily ? 'This family has no keys.' : 'There are no keys to report on yet.'}</p>
              {:else if leak?.used === 0}
                <p class="empty" data-testid="leaks-unused"><strong>Nothing used yet.</strong> Once keys are in use, each is listed here with where it has been heard from: how many addresses, how many devices, and whether many new ones came at once.</p>
              {:else}
                <div class="table-wrap">
                  <table class="leaks">
                    <caption class="sr-only">The leak report: each key, how likely it was passed on and why, and its addresses, devices and listens</caption>
                    <thead>
                      <tr>
                        <th scope="col">Key</th><th scope="col">Sign</th><th scope="col">Why</th>
                        <th scope="col" class="n" title="Different addresses it was heard from in the last 7 days · the last 24 hours · ever">Addresses<span class="small block">7 d · 24 h · ever</span></th>
                        <th scope="col" class="n">Devices</th><th scope="col" class="n">Listens</th>
                        <th scope="col" class="n" title="The most new addresses within any one hour">Most new<span class="small block">in an hour</span></th>
                        <th scope="col">Last heard</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each ks.leaks.entries as e (e.key.id)}
                        {@const lv = leakLevel(e)}
                        <tr class:off={lv === 'unused'} class:sel={ks.selected === e.key.id} data-leak={lv}>
                          <th scope="row">
                            <button type="button" class="rowbtn" aria-current={ks.selected === e.key.id ? 'true' : undefined} onclick={() => (ks.selected === e.key.id ? closeKey() : openKey(e.key.id))}>{e.key.label}</button>
                            <span class="small block">{[e.family ?? KIND_WORD[e.key.kind], e.key.state !== 'active' ? STANDING[standing(e.key, now, ks.signing?.soonDays)].word.toLowerCase() : null].filter(Boolean).join(' · ')}</span>
                          </th>
                          <td><span class="state sm {LEAK_WORD[lv].cls}">{LEAK_WORD[lv].word}</span>{#if e.score > 0}<span class="small block num">score {e.score}</span>{/if}</td>
                          <td class="why">{#if e.reasons.length}<ul>{#each e.reasons as r (r)}<li>{r}</li>{/each}</ul>{/if}</td>
                          <td class="n num">{e.addresses_7d} · {e.addresses_24h} · {e.addresses}</td>
                          <td class="n num">{e.devices}</td>
                          <td class="n num">{e.listens}</td>
                          <td class="n num">{e.peak_new_addresses_in_an_hour}{#if e.spike}<span class="spike" title="A burst of new addresses: what a key posted in public looks like">burst</span>{/if}</td>
                          <td>{e.last_seen_ms != null ? fromNow(e.last_seen_ms, now) : ''}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
                <p class="small foot">To act on one, open it: <b>Replace…</b> ends that key and issues a new one to the same person, and every other key goes on working. Found a key posted somewhere? <button type="button" class="linkish" onclick={askFind}>Find a key…</button> says whose it was.</p>
              {/if}
            {/if}
          </section>
        {/if}
      </div>

      {#if selected}
        {#key selected.id}
          <KeyPage key={selected} onsend={send} leak={ks.leaks?.entries.find((e) => e.key.id === selected.id) ?? null} />
        {/key}
      {:else}
        <aside class="keypage about" data-panel="keys.about" aria-labelledby="kab-h">
          <h2 id="kab-h" data-panel-title>What a key does</h2>
          <p>A show kept for key holders is advertised to everyone and heard only with a key. You give each listener, or each group, a key; they open its link or type it into the player, with no account and no password.</p>
          <p>Everything about a key can be changed after it is in someone’s hands: the shows it opens, when it works, its limits. Choose a key to see its use, its devices and its changes.</p>
          <h3>Shows that need a key</h3>
          {#if keyed.state !== 'ok'}
            <p class="quiet">The station did not say which shows need a key.</p>
          {:else if shows.length === 0}
            <p class="quiet" data-testid="about-no-shows">None yet. A show’s <b>Who can listen</b> is in its editor on the Schedule tab, and here:</p>
          {:else}
            <ul class="showlist">
              {#each shows as s (s.id)}
                <li><svg aria-hidden="true"><use href="#g-key" /></svg><span><b>{s.name}</b><span class="small block">{s.now === 'public' ? 'Open to everyone now' : 'Only a key opens it'}{s.now === 'key' && s.opens_at_ms != null ? ` · opens to everyone ${timeWords(s.opens_at_ms, zone)}` : ''}</span></span></li>
              {/each}
            </ul>
          {/if}
          {#if keyed.state === 'ok'}<div><button type="button" class="btn sm" aria-haspopup="dialog" data-testid="about-keep" onclick={() => (keeping = true)}>{shows.length === 0 ? 'Make a show keyed…' : 'Make another show keyed…'}</button></div>{/if}
          <h3>What it does not do</h3>
          <p class="quiet">A key limits who may listen, not what a listener does with the sound: someone with a key can still record the show. The limits and the leak report make passing a key on harder and easier to see; they do not stop it.</p>
          <p class="quiet">A listener key is not a show key. A host’s show key, for sending from their own studio, is in Settings under Own studios.</p>
        </aside>
      {/if}
    </div>
  {/if}
</div>

{#if making}<MakeKey family={making.family} onmade={onMade} onclose={() => (making = null)} onkeep={() => (keeping = true)} />{/if}
{#if makingMany}<MakeMany onmade={onMadeMany} onclose={() => (makingMany = false)} onkeep={() => (keeping = true)} />{/if}
{#if sending}
  {#key sending.key.id}
    <SendKey key={sending.key} given={sending.given} why={sending.why} onclose={() => (sending = null)} onkeep={() => (keeping = true)} onmake={makeNext} />
  {/key}
{/if}
<!-- After the others, so that it lies over them: step 1 from Make a key keeps what is typed there. -->
{#if keeping}<KeepShow onclose={() => (keeping = false)} onmake={making || makingMany || sending ? undefined : makeNext} />{/if}

<dialog bind:this={findEl} class="kdialog narrow" data-panel="keys.find" aria-labelledby="kfi-h">
  <form onsubmit={submitFind} novalidate>
    <h2 id="kfi-h" data-panel-title>Find a key</h2>
    <p class="lead">For a key found where it should not be: a post, a forum, a forwarded message. Paste it, or the link that carries it, and the station says whose it was.</p>
    <div class="field">
      <label for="kfi-text">The key, or its link</label>
      <input id="kfi-text" type="text" class="mono" bind:value={findText} autocomplete="off" spellcheck="false" aria-invalid={findErr ? true : undefined} aria-describedby={findErr ? 'kfi-err' : undefined} />
      {#if findErr}<p id="kfi-err" class="err" role="alert">{findErr}</p>{/if}
    </div>
    {#if found}
      <div class="found" role="status" data-testid="found-key">
        <p><b>It is {found.label}’s key</b>{#if familyLabel(found.family_id)}, of the family {familyLabel(found.family_id)}{/if}. It {STANDING[standing(found, now, ks.signing?.soonDays)].word === 'Works' ? 'works now' : `stands as: ${STANDING[standing(found, now, ks.signing?.soonDays)].word.toLowerCase()}`}.</p>
        <button type="button" class="btn sm primary" onclick={openFound}>Open the key</button>
      </div>
    {/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => findEl?.close()}>Close</button>
      <button type="submit" class="btn primary" disabled={finding} aria-busy={finding || undefined}>{finding ? 'Asking…' : 'Find it'}</button>
    </div>
  </form>
</dialog>
