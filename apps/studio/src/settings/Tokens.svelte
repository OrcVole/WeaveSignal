<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > API tokens (ADR-0195): a credential for each integration (a station website, a
  // stream deck, a script), named, limited to the scopes chosen, with an optional end date.
  // The secret is shown once, when the token is made, and the station keeps only a scrambled
  // form of it. An admin's panel; the station is the authority and its sentences are shown as
  // given. Panel name `settings.tokens` (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import { listTokens, makeToken, revokeToken, type ApiScope, type ApiTokenInfo } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { EXPIRIES, expiryWords, lastUsedWords, nameProblem, ordered, scopeAction, scopeGroups, scopesProblem, stateOf } from '../lib/tokens.ts';

  let { initial = null, now = Date.now() }: { initial?: { tokens: ApiTokenInfo[]; scopes: ApiScope[] } | null; now?: number } = $props();

  const seed = untrack(() => initial);
  let tokens = $state<ApiTokenInfo[]>(seed?.tokens ?? []);
  let scopes = $state<ApiScope[]>(seed?.scopes ?? []);
  let loaded = $state(seed !== null);
  let loadError = $state('');
  let note = $state('');
  let working = $state<string | null>(null);
  let rowError = $state<{ id: string; message: string } | null>(null);

  const groups = $derived(scopeGroups(scopes));
  const sorted = $derived(ordered(tokens, now));
  const live = $derived(tokens.filter((t) => stateOf(t, now) === 'active').length);

  async function refresh() {
    loadError = '';
    try {
      const r = await listTokens();
      tokens = r.tokens;
      scopes = r.scopes;
      loaded = true;
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  onMount(() => {
    if (seed === null) void refresh();
  });

  // ---- make one ----

  let makeEl = $state<HTMLDialogElement | null>(null);
  let draft = $state({ name: '', chosen: [] as string[], days: 365 as number | null });
  let errors = $state<{ name?: string; scopes?: string; form?: string }>({});
  let making = $state(false);

  function askMake() {
    draft = { name: '', chosen: [], days: 365 };
    errors = {};
    makeEl?.showModal();
    queueMicrotask(() => document.getElementById('tk-name')?.focus());
  }

  function toggle(name: string, on: boolean) {
    draft.chosen = on ? [...draft.chosen, name] : draft.chosen.filter((s) => s !== name);
  }

  // The secret, shown once.
  let secretEl = $state<HTMLDialogElement | null>(null);
  let shown = $state<{ name: string; secret: string } | null>(null);
  let copied = $state(false);

  async function onMake(e: SubmitEvent) {
    e.preventDefault();
    if (making) return;
    const errs: typeof errors = {};
    const name = nameProblem(draft.name);
    const sc = scopesProblem(draft.chosen);
    if (name) errs.name = name;
    if (sc) errs.scopes = sc;
    errors = errs;
    if (name) return document.getElementById('tk-name')?.focus();
    if (sc) return document.getElementById('tk-scopes')?.focus();
    making = true;
    try {
      const r = await makeToken({
        name: draft.name.trim(),
        scopes: draft.chosen,
        ...(draft.days === null ? {} : { expires_in_days: draft.days }),
      });
      tokens = [r.token, ...tokens];
      shown = { name: r.token.name, secret: r.secret };
      copied = false;
      note = '';
      makeEl?.close();
      secretEl?.showModal();
    } catch (err) {
      const placed = placeError(err, ['name', 'scopes', 'expires_in_days'] as const);
      errors = placed.field === 'name' || placed.field === 'scopes' ? { [placed.field]: placed.message } : { form: placed.message };
      if (placed.field === 'name') document.getElementById('tk-name')?.focus();
    } finally {
      making = false;
    }
  }

  async function copy() {
    if (!shown) return;
    try {
      await navigator.clipboard.writeText(shown.secret);
      copied = true;
    } catch {
      // Nothing to copy with: the secret is selectable in its box.
      document.getElementById('tk-secret')?.focus();
    }
  }

  function doneWithSecret() {
    note = `${shown?.name} is ready. Its secret is not kept anywhere you can read it again; make a new token if it is lost.`;
    secretEl?.close();
    shown = null;
  }

  // ---- revoke (confirmed) ----

  let revEl = $state<HTMLDialogElement | null>(null);
  let revoking = $state<ApiTokenInfo | null>(null);

  function askRevoke(t: ApiTokenInfo) {
    revoking = t;
    revEl?.showModal();
  }

  async function confirmRevoke() {
    const t = revoking;
    revEl?.close();
    revoking = null;
    if (!t) return;
    rowError = null;
    note = '';
    working = t.id;
    try {
      await revokeToken(t.id);
      tokens = tokens.map((x) => (x.id === t.id ? { ...x, revoked_ms: Date.now() } : x));
      note = `${t.name} is revoked. Anything using it is refused from now on.`;
    } catch (e) {
      rowError = { id: t.id, message: placeError(e, []).message };
    } finally {
      working = null;
    }
  }
</script>

<section class="panel" data-panel="settings.tokens" aria-labelledby="tokens-h">
  <div class="ph">
    <h2 id="tokens-h" data-panel-title>API tokens</h2>
    <span class="small">{#if loaded}{live} {live === 1 ? 'token works' : 'tokens work'}{/if}</span>
    <button type="button" class="btn sm add" onclick={askMake} disabled={!loaded}>Make a token…</button>
  </div>
  <p class="help">
    A token lets a program use the station’s API without the station key: a website, a stream deck, a script. Each is named, limited to the scopes you choose, and can be revoked on its own. It can never manage staff, retention, erasure, settings or other tokens.
  </p>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load the tokens.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !loaded}
    <p role="status" class="small">Loading tokens…</p>
  {:else if tokens.length === 0}
    <p class="empty">No tokens yet. Make one when something outside the studio needs to read or drive it.</p>
  {:else}
    <div class="table-wrap">
      <table>
        <caption class="sr-only">API tokens: name, what each may do, when it ends and was last used, and the action on each</caption>
        <thead>
          <tr><th scope="col">Name</th><th scope="col">May</th><th scope="col">Status</th><th scope="col"><span class="sr-only">Actions</span></th></tr>
        </thead>
        <tbody>
          {#each sorted as t (t.id)}
            {@const state = stateOf(t, now)}
            {@const busy = working === t.id}
            <tr class:off={state !== 'active'} aria-busy={busy || undefined}>
              <th scope="row">
                {t.name}
                <span class="small block mono">{t.prefix}…</span>
                <span class="small block">Made by {t.created_by}</span>
              </th>
              <td>
                <ul class="scopes">
                  {#each t.scopes as s (s)}<li class="mono">{s}</li>{/each}
                </ul>
              </td>
              <td>
                {#if state === 'active'}<span class="state sm">Working</span>{:else if state === 'expired'}<span class="state sm hollow-off">Expired</span>{:else}<span class="state sm hollow-off">Revoked</span>{/if}
                <span class="small block">{expiryWords(t, now)}</span>
                <span class="small block">{lastUsedWords(t)}</span>
              </td>
              <td class="acts">
                {#if t.revoked_ms === null}
                  <button type="button" class="btn sm quiet" disabled={busy} onclick={() => askRevoke(t)}>Revoke…<span class="sr-only">{` ${t.name}`}</span></button>
                {/if}
              </td>
            </tr>
            {#if rowError?.id === t.id}
              <tr class="row-err"><td colspan="4"><p class="err" role="alert">{rowError.message}</p></td></tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  <p class="small" role="status" aria-live="polite">{note}</p>
</section>

<dialog bind:this={makeEl} aria-labelledby="tk-h">
  <form onsubmit={onMake} novalidate>
    <h2 id="tk-h">Make an API token</h2>
    <div class="field">
      <label for="tk-name">Name</label>
      <input
        id="tk-name"
        type="text"
        bind:value={draft.name}
        autocomplete="off"
        maxlength="80"
        aria-invalid={errors.name ? true : undefined}
        aria-describedby={errors.name ? 'tk-name-err tk-name-hint' : 'tk-name-hint'}
      />
      <span id="tk-name-hint" class="hint">What will use it, for example “Station website” or “Stream deck”.</span>
      {#if errors.name}<p id="tk-name-err" class="err">{errors.name}</p>{/if}
    </div>
    <fieldset id="tk-scopes" tabindex="-1" aria-describedby={errors.scopes ? 'tk-scopes-err' : undefined}>
      <legend>May</legend>
      {#each groups as g (g.group)}
        <div class="group">
          <span class="gname">{g.group}</span>
          {#each g.scopes as s (s.name)}
            <label class="scope">
              <input type="checkbox" checked={draft.chosen.includes(s.name)} onchange={(e) => toggle(s.name, e.currentTarget.checked)} />
              <span><span class="mono">{scopeAction(s.name)}</span> <span class="hint">{s.description}</span></span>
            </label>
          {/each}
        </div>
      {/each}
      {#if errors.scopes}<p id="tk-scopes-err" class="err">{errors.scopes}</p>{/if}
    </fieldset>
    <div class="field">
      <label for="tk-days">Lasts</label>
      <select id="tk-days" bind:value={draft.days}>
        {#each EXPIRIES as x (x.label)}<option value={x.days}>{x.label}</option>{/each}
      </select>
      <span class="hint">A token that ends is a token that cannot be forgotten about. Make a new one when it does.</span>
    </div>
    {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => makeEl?.close()} disabled={making}>Cancel</button>
      <button type="submit" class="btn primary" disabled={making} aria-busy={making || undefined}>{making ? 'Making…' : 'Make the token'}</button>
    </div>
  </form>
</dialog>

<dialog bind:this={secretEl} aria-labelledby="tks-h" aria-describedby="tks-d" oncancel={(e) => e.preventDefault()}>
  <h2 id="tks-h">Copy the secret for {shown?.name}</h2>
  <p id="tks-d">This is the only time it is shown. The station keeps a scrambled form, not the secret, so it cannot be read again. Put it where the program can read it; it goes in <span class="mono">Authorization: Bearer …</span>.</p>
  <div class="row">
    <input id="tk-secret" type="text" readonly value={shown?.secret ?? ''} aria-label="The secret" onfocus={(e) => e.currentTarget.select()} />
    <button type="button" class="btn sm" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
  </div>
  <div class="acts">
    <button type="button" class="btn primary" onclick={doneWithSecret}>I have copied it</button>
  </div>
</dialog>

<dialog bind:this={revEl} aria-labelledby="tkr-h" aria-describedby="tkr-d">
  <h2 id="tkr-h">Revoke {revoking?.name}?</h2>
  <p id="tkr-d">Anything using this token is refused from now on, and it cannot be brought back. Make a new token if it is still needed.</p>
  <div class="acts">
    <button type="button" class="btn" onclick={() => revEl?.close()}>Keep it</button>
    <button type="button" class="btn danger-hollow" onclick={confirmRevoke}>Revoke</button>
  </div>
</dialog>

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
  .ph .add {
    margin-left: auto;
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
  }
  .field > .hint {
    display: block;
  }
  .empty {
    color: var(--ink-2);
    border: 1px dashed var(--axis);
    border-radius: var(--r-2);
    padding: var(--s-3);
    line-height: var(--lh-body);
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    line-height: var(--lh-body);
  }
  th,
  td {
    text-align: left;
    vertical-align: top;
    padding: var(--s-2) var(--s-3) var(--s-2) 0;
    border-top: 1px solid var(--hairline);
  }
  thead th {
    border-top: 0;
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
    padding-top: 0;
  }
  tbody th {
    font-weight: 600;
  }
  tr.off th,
  tr.off td {
    color: var(--muted);
  }
  .small.block {
    display: block;
    font-weight: 400;
    margin-top: var(--s-1);
  }
  .scopes {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
  }
  .scopes li {
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    padding: 0 var(--s-1);
    font-size: var(--fs-13);
  }
  td.acts {
    text-align: right;
    white-space: nowrap;
  }
  tr.row-err td {
    border-top: 0;
    padding-top: 0;
  }
  select,
  input[type='text'] {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
  }
  :global(:root[data-theme='light']) select,
  :global(:root[data-theme='light']) input {
    color-scheme: light;
  }
  [aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
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
  .callout .btn {
    margin-top: var(--s-2);
  }
  .small:empty {
    display: none;
  }
  dialog {
    width: min(34rem, calc(100vw - 2 * var(--s-4)));
    max-height: calc(100vh - 2 * var(--s-4));
    overflow-y: auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.55);
  }
  dialog h2 {
    font-size: var(--fs-20);
    margin-bottom: var(--s-3);
  }
  dialog p {
    line-height: var(--lh-body);
    margin-bottom: var(--s-3);
  }
  dialog form {
    display: grid;
    gap: var(--s-3);
  }
  dialog form h2 {
    margin-bottom: 0;
  }
  .field {
    display: grid;
    gap: var(--s-1);
  }
  .field > label,
  legend {
    font-weight: 600;
  }
  .field input,
  .field select {
    width: 100%;
  }
  fieldset {
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3);
    display: grid;
    gap: var(--s-2);
  }
  .group {
    display: grid;
    gap: var(--s-1);
  }
  .gname {
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .scope {
    display: flex;
    gap: var(--s-2);
    align-items: baseline;
    line-height: var(--lh-body);
  }
  .row {
    display: flex;
    gap: var(--s-2);
    align-items: center;
  }
  .row input {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-2);
  }
</style>
