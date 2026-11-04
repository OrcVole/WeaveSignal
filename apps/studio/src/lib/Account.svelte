<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Who this window runs as (ADR-0127), in the frame: the name and role, or the station key,
  // as a quiet button; it opens a small dialog with Sign out and, for a staff session, Change
  // your password. Panel name `studio.account` (ADR-0070).
  import { changeOwnPassword, packaged } from './api.ts';
  import { account, signInAgain } from './account.svelte.ts';
  import { passwordProblem, placeError, whoLine } from './account.ts';

  const who = $derived(whoLine(account.me));
  const staff = $derived(account.me?.kind === 'staff');

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let changing = $state(false);
  let current = $state('');
  let next = $state('');
  let busy = $state(false);
  let errors = $state<{ current?: string; new?: string; form?: string }>({});
  let note = $state('');
  let signingOut = $state(false);

  function open() {
    changing = false;
    errors = {};
    note = '';
    dialogEl?.showModal();
  }

  function close() {
    dialogEl?.close();
    current = '';
    next = '';
  }

  function startChange() {
    changing = true;
    errors = {};
    note = '';
    queueMicrotask(() => document.getElementById('pw-current')?.focus());
  }

  async function onChange(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    const errs: typeof errors = {};
    if (!current) errs.current = 'Enter your current password.';
    const p = passwordProblem(next, account.me?.account?.username ?? '');
    if (p) errs.new = p;
    errors = errs;
    if (errs.current || errs.new) return document.getElementById(errs.current ? 'pw-current' : 'pw-new')?.focus();
    busy = true;
    try {
      await changeOwnPassword(current, next);
      current = '';
      next = '';
      changing = false;
      note = 'Password changed. Your other sign-ins have ended; this one stays.';
    } catch (err) {
      const placed = placeError(err, ['current', 'new'] as const);
      errors = placed.field ? { [placed.field]: placed.message } : { form: placed.message };
      if (placed.field) document.getElementById(`pw-${placed.field}`)?.focus();
    } finally {
      busy = false;
    }
  }

  async function onSignOut() {
    if (signingOut) return;
    signingOut = true;
    await signInAgain();
  }
</script>

<span class="account" data-panel="studio.account">
{#if account.loaded}
  <button type="button" class="who" onclick={open} aria-haspopup="dialog" data-panel-title>
    <span class="sr-only">Signed in as </span><b>{who.name}</b><span class="role"> · {who.role}</span>
  </button>
{/if}

<dialog bind:this={dialogEl} aria-labelledby="acct-h" onclose={() => (changing = false)}>
  <div class="head">
    <h2 id="acct-h">{who.name}</h2>
    <p class="small">
      {#if staff}
        {account.me?.account?.username} · {who.role}
      {:else if account.me?.kind === 'station'}
        The station key, with every admin right. Sign in as yourself once accounts exist.
      {:else}
        This station is older than staff accounts: the station key signs this window in.
      {/if}
    </p>
  </div>

  {#if changing}
    <form onsubmit={onChange} novalidate>
      <h3>Change your password</h3>
      <div class="field">
        <label for="pw-current">Current password</label>
        <input
          id="pw-current"
          type="password"
          bind:value={current}
          autocomplete="current-password"
          aria-invalid={errors.current ? true : undefined}
          aria-describedby={errors.current ? 'pw-current-err' : undefined}
        />
        {#if errors.current}<p id="pw-current-err" class="err">{errors.current}</p>{/if}
      </div>
      <div class="field">
        <label for="pw-new">New password</label>
        <input
          id="pw-new"
          type="password"
          bind:value={next}
          autocomplete="new-password"
          aria-invalid={errors.new ? true : undefined}
          aria-describedby={errors.new ? 'pw-new-err pw-new-hint' : 'pw-new-hint'}
        />
        <span id="pw-new-hint" class="hint">At least 10 characters. Any characters count; a few words work well.</span>
        {#if errors.new}<p id="pw-new-err" class="err">{errors.new}</p>{/if}
      </div>
      {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={() => (changing = false)} disabled={busy}>Keep current password</button>
        <button type="submit" class="btn primary" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Changing…' : 'Change password'}</button>
      </div>
    </form>
  {:else}
    <p class="small" role="status" aria-live="polite">{note}</p>
    <div class="acts stack">
      {#if staff}
        <button type="button" class="btn" onclick={startChange}>Change your password…</button>
      {/if}
      {#if packaged()}
        <button type="button" class="btn" onclick={onSignOut} disabled={signingOut} aria-busy={signingOut || undefined}>
          {signingOut ? 'Signing out…' : 'Sign out'}
        </button>
        <p class="hint">
          {#if staff}
            This computer forgets your sign-in; the station’s address is kept.
          {:else}
            This computer forgets the key; the station’s address is kept.
          {/if}
        </p>
      {/if}
      <button type="button" class="btn quiet" onclick={close}>Close</button>
    </div>
  {/if}
</dialog>
</span>

<style>
  .account {
    display: inline-flex;
    min-width: 0;
  }
  .who {
    appearance: none;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-pill);
    color: var(--ink-2);
    font-size: var(--fs-13);
    padding: 0 var(--s-3);
    min-height: 1.75rem;
    cursor: pointer;
    white-space: nowrap;
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .who:hover {
    background: var(--surface);
    border-color: var(--axis);
  }
  .who b {
    color: var(--ink);
    font-weight: 600;
  }
  dialog {
    width: min(24rem, calc(100vw - 2 * var(--s-4)));
    margin: 3rem var(--s-3) auto auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
    display: none;
    gap: var(--s-4);
  }
  dialog[open] {
    display: grid;
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.35);
  }
  .head {
    display: grid;
    gap: var(--s-1);
  }
  h2 {
    font-size: var(--fs-20);
  }
  h3 {
    font-size: var(--fs-16);
  }
  .small {
    line-height: var(--lh-body);
  }
  .small:empty {
    display: none;
  }
  form {
    display: grid;
    gap: var(--s-3);
  }
  .field {
    display: grid;
    gap: var(--s-1);
  }
  label {
    font-weight: 600;
  }
  input {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    width: 100%;
    color-scheme: dark;
  }
  :global(:root[data-theme='light']) input {
    color-scheme: light;
  }
  input[aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .hint {
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  .acts.stack {
    display: grid;
    justify-content: stretch;
  }
  .acts.stack .btn {
    width: 100%;
  }
</style>
