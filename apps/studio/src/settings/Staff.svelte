<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Staff (ADR-0127): everyone who signs in to the studio as themselves, one row
  // each (name, username, role, on or off), with Add someone, a role change, turn off or
  // back on, a new password an admin passes on, and the email address a person's sign-in
  // codes go to (kept by the station only as a keyed hash). An admin's screen, kept compact; the
  // station is the authority and its sentences are shown as given. Panel name
  // `settings.staff` (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import { changeStaff, createStaff, listStaff, setStaffEmail, setStaffPassword, type Me, type StaffAccount, type StaffRole } from '../lib/api.ts';
  import { ROLES, emailProblem, nameProblem, passwordProblem, placeError, roleLine, roleWord, selfLockout, suggestPassword, usernameProblem } from '../lib/account.ts';

  let { me, initial = null }: { me: Me | null; initial?: StaffAccount[] | null } = $props();

  const seed = untrack(() => initial);
  let accounts = $state<StaffAccount[]>(seed ?? []);
  let loaded = $state(seed !== null);
  let loadError = $state('');
  let note = $state('');
  /** The row a request is in flight for. */
  let working = $state<string | null>(null);
  let rowError = $state<{ id: string; message: string } | null>(null);

  const myId = $derived(me?.kind === 'staff' ? (me.account?.id ?? null) : null);
  const on = $derived(accounts.filter((a) => a.disabled_ms === null));
  const sorted = $derived(
    [...accounts].sort((a, b) => Number(a.disabled_ms !== null) - Number(b.disabled_ms !== null) || a.name.localeCompare(b.name, 'en-GB')),
  );

  async function refresh() {
    loadError = '';
    try {
      accounts = await listStaff();
      loaded = true;
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  onMount(() => {
    if (seed === null) void refresh();
  });

  function put(a: StaffAccount) {
    accounts = accounts.some((x) => x.id === a.id) ? accounts.map((x) => (x.id === a.id ? a : x)) : [...accounts, a];
  }

  const offSince = (ms: number) => new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });

  // ---- a change to one row ----

  async function change(a: StaffAccount, c: { role?: StaffRole; disabled?: boolean }, said: (a: StaffAccount) => string) {
    rowError = null;
    note = '';
    working = a.id;
    try {
      const after = await changeStaff(a.id, c);
      put(after);
      note = said(after);
    } catch (e) {
      rowError = { id: a.id, message: placeError(e, []).message };
    } finally {
      working = null;
    }
  }

  function setRole(a: StaffAccount, role: StaffRole) {
    if (role === a.role) return;
    void change(a, { role }, (x) => `${x.name} is now ${role === 'admin' ? 'an admin' : `a ${roleWord(role).toLowerCase()}`}.`);
  }

  // ---- turn off (confirmed) ----

  let offEl = $state<HTMLDialogElement | null>(null);
  let turningOff = $state<StaffAccount | null>(null);

  function askOff(a: StaffAccount) {
    turningOff = a;
    offEl?.showModal();
  }

  function confirmOff() {
    const a = turningOff;
    offEl?.close();
    turningOff = null;
    if (a) void change(a, { disabled: true }, (x) => `${x.name}’s account is off. Their sign-ins have ended.`);
  }

  function turnOn(a: StaffAccount) {
    void change(a, { disabled: false }, (x) => `${x.name}’s account is on again. They can sign in.`);
  }

  // ---- a new password ----

  let pwEl = $state<HTMLDialogElement | null>(null);
  let pwFor = $state<StaffAccount | null>(null);
  let pw = $state('');
  let pwError = $state<{ password?: string; form?: string }>({});
  let pwBusy = $state(false);

  function askPassword(a: StaffAccount) {
    pwFor = a;
    pw = '';
    pwError = {};
    pwEl?.showModal();
  }

  async function onSetPassword(e: SubmitEvent) {
    e.preventDefault();
    if (!pwFor || pwBusy) return;
    const p = passwordProblem(pw, pwFor.username);
    pwError = p ? { password: p } : {};
    if (p) return document.getElementById('sp-password')?.focus();
    pwBusy = true;
    try {
      await setStaffPassword(pwFor.id, pw);
      note = `${pwFor.name} has a new password. Pass it on; their other sign-ins have ended.`;
      pwEl?.close();
      pwFor = null;
      pw = '';
    } catch (err) {
      const placed = placeError(err, ['password'] as const);
      pwError = placed.field ? { password: placed.message } : { form: placed.message };
      if (placed.field) document.getElementById('sp-password')?.focus();
    } finally {
      pwBusy = false;
    }
  }

  // ---- the address a person's codes go to ----

  let emEl = $state<HTMLDialogElement | null>(null);
  let emFor = $state<StaffAccount | null>(null);
  let em = $state('');
  let emError = $state<{ email?: string; form?: string }>({});
  let emBusy = $state(false);

  function askEmail(a: StaffAccount) {
    emFor = a;
    em = '';
    emError = {};
    emEl?.showModal();
    queueMicrotask(() => document.getElementById('se-email')?.focus());
  }

  /** Set the address, or take it away with null. The station never shows it back. */
  async function saveEmail(address: string | null) {
    if (!emFor || emBusy) return;
    if (address !== null) {
      const p = emailProblem(address);
      emError = p ? { email: p } : {};
      if (p) return document.getElementById('se-email')?.focus();
    }
    emBusy = true;
    try {
      const after = await setStaffEmail(emFor.id, address);
      put(after);
      note =
        address === null
          ? `${emFor.name} can no longer sign in with a code.`
          : `${emFor.name} can sign in with a code sent to that address. The address is not kept; they type it when they sign in.`;
      emEl?.close();
      emFor = null;
      em = '';
    } catch (err) {
      const placed = placeError(err, ['email'] as const);
      emError = placed.field ? { email: placed.message } : { form: placed.message };
      if (placed.field) document.getElementById('se-email')?.focus();
    } finally {
      emBusy = false;
    }
  }

  // ---- add someone ----

  let addEl = $state<HTMLDialogElement | null>(null);
  let draft = $state({ name: '', username: '', role: 'producer' as StaffRole, password: '' });
  let addErrors = $state<{ name?: string; username?: string; role?: string; password?: string; form?: string }>({});
  let adding = $state(false);

  function askAdd() {
    draft = { name: '', username: '', role: 'producer', password: '' };
    addErrors = {};
    addEl?.showModal();
    queueMicrotask(() => document.getElementById('ad-name')?.focus());
  }

  /** A username from the name, offered once the name is typed and the username is still empty:
   *  first name, dot, surname, lower case, letters and digits only. */
  function offerUsername() {
    if (draft.username) return;
    const parts = draft.name
      .toLowerCase()
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .split(/[\s-]+/)
      .map((p) => p.replace(/[^a-z0-9]/g, ''))
      .filter(Boolean);
    if (parts.length) draft.username = parts.slice(0, 2).join('.').slice(0, 40);
  }

  async function onAdd(e: SubmitEvent) {
    e.preventDefault();
    if (adding) return;
    const errs: typeof addErrors = {};
    const name = nameProblem(draft.name);
    const username = usernameProblem(draft.username);
    const password = passwordProblem(draft.password, draft.username);
    if (name) errs.name = name;
    if (username) errs.username = username;
    if (password) errs.password = password;
    addErrors = errs;
    const first = (['name', 'username', 'password'] as const).find((k) => errs[k]);
    if (first) return document.getElementById(`ad-${first}`)?.focus();
    adding = true;
    try {
      const a = await createStaff({
        username: draft.username.trim(),
        name: draft.name.trim(),
        role: draft.role,
        password: draft.password,
      });
      put(a);
      note = `${a.name} added as ${a.username}. Pass on the username and password; they can change the password once signed in.`;
      addEl?.close();
    } catch (err) {
      const placed = placeError(err, ['name', 'username', 'role', 'password'] as const);
      addErrors = placed.field ? { [placed.field]: placed.message } : { form: placed.message };
      if (placed.field) document.getElementById(`ad-${placed.field}`)?.focus();
    } finally {
      adding = false;
    }
  }
</script>

<section class="panel" data-panel="settings.staff" aria-labelledby="staff-h">
  <div class="ph">
    <h2 id="staff-h" data-panel-title>Staff</h2>
    <span class="small">{#if loaded}{on.length} {on.length === 1 ? 'person' : 'people'} can sign in{#if accounts.length - on.length}, {accounts.length - on.length} off{/if}{/if}</span>
    <button type="button" class="btn sm add" onclick={askAdd}>Add someone…</button>
  </div>
  <p class="help">
    Everyone who runs the studio signs in as themselves. An admin also manages staff, retention and erasure; every role runs the desk, the screener, the carts and the schedule.
  </p>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load the staff list.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !loaded}
    <p role="status" class="small">Loading staff…</p>
  {:else if accounts.length === 0}
    <p class="empty">No one has an account yet. Add the first admin, then the people who run shows; each signs in with their own username and password.</p>
  {:else}
    <div class="table-wrap">
      <table>
        <caption class="sr-only">Staff accounts: name, username, role, whether the account is on, and the actions on each</caption>
        <thead>
          <tr><th scope="col">Name</th><th scope="col">Username</th><th scope="col">Role</th><th scope="col">Account</th><th scope="col"><span class="sr-only">Actions</span></th></tr>
        </thead>
        <tbody>
          {#each sorted as a (a.id)}
            {@const mine = a.id === myId}
            {@const off = a.disabled_ms !== null}
            {@const busy = working === a.id}
            <tr class:off aria-busy={busy || undefined}>
              <th scope="row">{a.name}{#if mine}<span class="you">{' (you)'}</span><span class="small block">Another admin changes your role or turns your account off.</span>{/if}</th>
              <td class="mono">{a.username}</td>
              <td>
                <label class="sr-only" for="role-{a.id}">Role of {a.name}</label>
                <select
                  id="role-{a.id}"
                  value={a.role}
                  disabled={busy || off || mine}
                  onchange={(e) => setRole(a, e.currentTarget.value as StaffRole)}
                >
                  {#each ROLES as r (r)}<option value={r}>{roleWord(r)}</option>{/each}
                </select>
              </td>
              <td>
                {#if off}
                  <span class="state sm hollow-off">Off</span>
                  <span class="small"> since {offSince(a.disabled_ms ?? 0)}</span>
                {:else}
                  On
                {/if}
                {#if a.has_email}<span class="small block">Code sign-in on</span>{/if}
              </td>
              <td class="acts">
                <button type="button" class="btn sm" disabled={busy || off} onclick={() => askPassword(a)}>Set password…<span class="sr-only">{` for ${a.name}`}</span></button>
                <button type="button" class="btn sm" disabled={busy || off} onclick={() => askEmail(a)}>Email…<span class="sr-only">{` for ${a.name}’s sign-in codes`}</span></button>
                {#if off}
                  <button type="button" class="btn sm" disabled={busy} onclick={() => turnOn(a)}>Turn on<span class="sr-only">{` ${a.name}’s account`}</span></button>
                {:else}
                  <button
                    type="button"
                    class="btn sm quiet"
                    disabled={busy || selfLockout(me, a.id, { disabled: true })}
                    onclick={() => askOff(a)}>Turn off…<span class="sr-only">{` ${a.name}’s account`}</span></button
                  >
                {/if}
              </td>
            </tr>
            {#if rowError?.id === a.id}
              <tr class="row-err"><td colspan="5"><p class="err" role="alert">{rowError.message}</p></td></tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  <p class="small" role="status" aria-live="polite">{note}</p>
</section>

<dialog bind:this={addEl} aria-labelledby="ad-h">
  <form onsubmit={onAdd} novalidate>
    <h2 id="ad-h">Add someone</h2>
    <div class="field">
      <label for="ad-name">Name</label>
      <input
        id="ad-name"
        type="text"
        bind:value={draft.name}
        onblur={offerUsername}
        autocomplete="off"
        maxlength="80"
        aria-invalid={addErrors.name ? true : undefined}
        aria-describedby={addErrors.name ? 'ad-name-err ad-name-hint' : 'ad-name-hint'}
      />
      <span id="ad-name-hint" class="hint">As it appears on air and in the log.</span>
      {#if addErrors.name}<p id="ad-name-err" class="err">{addErrors.name}</p>{/if}
    </div>
    <div class="field">
      <label for="ad-username">Username</label>
      <input
        id="ad-username"
        type="text"
        bind:value={draft.username}
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        maxlength="40"
        aria-invalid={addErrors.username ? true : undefined}
        aria-describedby={addErrors.username ? 'ad-username-err ad-username-hint' : 'ad-username-hint'}
      />
      <span id="ad-username-hint" class="hint">Lower-case letters, digits, dots, underscores and hyphens; up to 40.</span>
      {#if addErrors.username}<p id="ad-username-err" class="err">{addErrors.username}</p>{/if}
    </div>
    <div class="field">
      <label for="ad-role">Role</label>
      <select id="ad-role" bind:value={draft.role} aria-describedby="ad-role-hint">
        {#each ROLES as r (r)}<option value={r}>{roleWord(r)}</option>{/each}
      </select>
      <span id="ad-role-hint" class="hint">{roleLine(draft.role)}</span>
    </div>
    <div class="field">
      <label for="ad-password">First password</label>
      <div class="row">
        <input
          id="ad-password"
          type="text"
          bind:value={draft.password}
          autocomplete="off"
          spellcheck="false"
          aria-invalid={addErrors.password ? true : undefined}
          aria-describedby={addErrors.password ? 'ad-password-err ad-password-hint' : 'ad-password-hint'}
        />
        <button type="button" class="btn sm" onclick={() => (draft.password = suggestPassword())}>Suggest one</button>
      </div>
      <span id="ad-password-hint" class="hint">At least 10 characters. Pass it on to them; they can change it once signed in.</span>
      {#if addErrors.password}<p id="ad-password-err" class="err">{addErrors.password}</p>{/if}
    </div>
    {#if addErrors.form}<p class="err" role="alert">{addErrors.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => addEl?.close()} disabled={adding}>Cancel</button>
      <button type="submit" class="btn primary" disabled={adding} aria-busy={adding || undefined}>{adding ? 'Adding…' : 'Add'}</button>
    </div>
  </form>
</dialog>

<dialog bind:this={offEl} aria-labelledby="off-h" aria-describedby="off-d">
  <h2 id="off-h">Turn off {turningOff?.name}’s account?</h2>
  <p id="off-d">Their sign-ins end now, everywhere, and they cannot sign in until the account is turned back on. Nothing they did is deleted.</p>
  <div class="acts">
    <button type="button" class="btn" onclick={() => offEl?.close()}>Keep on</button>
    <button type="button" class="btn danger-hollow" onclick={confirmOff}>Turn off</button>
  </div>
</dialog>

<dialog bind:this={pwEl} aria-labelledby="sp-h">
  <form onsubmit={onSetPassword} novalidate>
    <h2 id="sp-h">New password for {pwFor?.name}</h2>
    <div class="field">
      <label for="sp-password">New password</label>
      <div class="row">
        <input
          id="sp-password"
          type="text"
          bind:value={pw}
          autocomplete="off"
          spellcheck="false"
          aria-invalid={pwError.password ? true : undefined}
          aria-describedby={pwError.password ? 'sp-password-err sp-password-hint' : 'sp-password-hint'}
        />
        <button type="button" class="btn sm" onclick={() => (pw = suggestPassword())}>Suggest one</button>
      </div>
      <span id="sp-password-hint" class="hint">At least 10 characters. Their other sign-ins end; pass it on, and they can change it afterwards.</span>
      {#if pwError.password}<p id="sp-password-err" class="err">{pwError.password}</p>{/if}
    </div>
    {#if pwError.form}<p class="err" role="alert">{pwError.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => pwEl?.close()} disabled={pwBusy}>Keep current</button>
      <button type="submit" class="btn primary" disabled={pwBusy} aria-busy={pwBusy || undefined}>{pwBusy ? 'Setting…' : 'Set password'}</button>
    </div>
  </form>
</dialog>

<dialog bind:this={emEl} aria-labelledby="se-h">
  <form onsubmit={(e) => { e.preventDefault(); void saveEmail(em); }} novalidate>
    <h2 id="se-h">Email for {emFor?.name}’s sign-in codes</h2>
    <div class="field">
      <label for="se-email">Email address</label>
      <input
        id="se-email"
        type="email"
        bind:value={em}
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        aria-invalid={emError.email ? true : undefined}
        aria-describedby={emError.email ? 'se-email-err se-email-hint' : 'se-email-hint'}
      />
      <span id="se-email-hint" class="hint">
        {emFor?.has_email ? 'An address is set; this one replaces it. ' : ''}The station keeps only a scrambled form of it and never shows it back, so they type it when they ask for a code. It needs the station’s mail to be on. One address belongs to one account.
      </span>
      {#if emError.email}<p id="se-email-err" class="err">{emError.email}</p>{/if}
    </div>
    {#if emError.form}<p class="err" role="alert">{emError.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => emEl?.close()} disabled={emBusy}>Cancel</button>
      {#if emFor?.has_email}
        <button type="button" class="btn danger-hollow" disabled={emBusy} onclick={() => saveEmail(null)}>Take it away</button>
      {/if}
      <button type="submit" class="btn primary" disabled={emBusy} aria-busy={emBusy || undefined}>{emBusy ? 'Saving…' : 'Set address'}</button>
    </div>
  </form>
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
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
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
    vertical-align: middle;
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
  tr.off td.mono {
    color: var(--muted);
  }
  .you {
    font-weight: 400;
    color: var(--ink-2);
  }
  .small.block {
    display: block;
    font-weight: 400;
    margin-top: var(--s-1);
  }
  td.acts {
    text-align: right;
    white-space: nowrap;
  }
  td.acts .btn + .btn {
    margin-left: var(--s-1);
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
  td select {
    min-height: 1.75rem;
    font-size: var(--fs-13);
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
    width: min(30rem, calc(100vw - 2 * var(--s-4)));
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
  .field > label {
    font-weight: 600;
  }
  .field input,
  .field select {
    width: 100%;
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
