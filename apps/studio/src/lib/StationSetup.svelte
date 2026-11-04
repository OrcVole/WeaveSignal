<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The packaged app's sign-in (ADR-0127): which station to run, and who you are. A username
  // and password give a session whose token goes into this computer's keychain from the Rust
  // side (ADR-0121); the window then reloads and follows the station. A station with mail can
  // instead email a six-digit code to the address its admin set for you: ask for it with your
  // username and that address, then type it (ADR-0127). The quieter route keeps the station
  // key, which is how the first admin gets in before any account exists.
  // Panel name `setup.sign-in` (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import Mark from './Mark.svelte';
  import { askSignInCode, checkStation, finishSignInCode, lastStation, saveStation, signIn } from './api.ts';
  import { cleanCode, codeProblem, emailProblem, placeError } from './account.ts';
  import { checkAddress, isTailscaleHttp, TAILSCALE_NOTE } from './address.ts';

  let {
    initialUrl = '',
    initialMode = 'password',
    initialAsked = false,
  }: { initialUrl?: string; initialMode?: 'password' | 'code' | 'key'; initialAsked?: boolean } = $props();

  // The prop seeds the address once (a test's); after that the field is the person's.
  let url = $state(untrack(() => initialUrl));
  let username = $state('');
  let password = $state('');
  let key = $state('');
  let email = $state('');
  let code = $state('');
  /** Which way in is showing: a password, a code emailed to you, or the station key. */
  let mode = $state<'password' | 'code' | 'key'>(untrack(() => initialMode));
  /** The code step: a code has been asked for, and is typed here. */
  let asked = $state(untrack(() => initialAsked));
  let busy = $state(false);
  let errors = $state<{ url?: string; username?: string; password?: string; key?: string; email?: string; code?: string; form?: string }>({});

  let urlEl = $state<HTMLInputElement | null>(null);

  // ADR-0197: the Tailscale sentence shows for a plain http address in 100.64.0.0/10 until the
  // first sign-in to that station succeeds, then never again for it (kept on this computer).
  const TAILSCALE_SEEN = 'weavesignal.tailscaleNoteSeen';
  function seenStations(): string[] {
    try {
      const v = JSON.parse(globalThis.localStorage?.getItem(TAILSCALE_SEEN) ?? '[]');
      return Array.isArray(v) ? v : [];
    } catch {
      return [];
    }
  }
  function markSeen(base: string) {
    if (!isTailscaleHttp(base)) return;
    try {
      const all = seenStations();
      if (!all.includes(base)) globalThis.localStorage?.setItem(TAILSCALE_SEEN, JSON.stringify([...all, base]));
    } catch {
      // No storage here: the sentence shows again next time, which is harmless.
    }
  }
  let tailscaleNote = $derived.by(() => {
    const r = checkAddress(url);
    return r.ok && isTailscaleHttp(r.base) && !seenStations().includes(r.base);
  });
  let userEl = $state<HTMLInputElement | null>(null);
  let keyEl = $state<HTMLInputElement | null>(null);

  onMount(async () => {
    if (!url) url = (await lastStation()) ?? '';
    (url ? userEl : urlEl)?.focus();
  });

  /** The address as an origin, or the sentence for its field (ADR-0139: plain http only nearby). */
  function cleanUrl(): string | null {
    const r = checkAddress(url);
    if (!r.ok) {
      errors = { url: r.message };
      return null;
    }
    return r.base;
  }

  function focusFirst() {
    const first = (['url', 'username', 'password', 'email', 'code', 'key'] as const).find((k) => errors[k]);
    if (first) document.getElementById(`si-${first}`)?.focus();
  }

  async function onSignIn(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    errors = {};
    const base = cleanUrl();
    if (base) {
      if (!username.trim()) errors = { ...errors, username: 'Enter your username.' };
      if (!password) errors = { ...errors, password: 'Enter your password.' };
    }
    if (!base || errors.username || errors.password) return focusFirst();
    busy = true;
    try {
      await signIn(base, username.trim(), password);
      markSeen(base);
      password = '';
      location.reload();
    } catch (err) {
      const p = placeError(err, ['username', 'password'] as const);
      errors = p.field ? { [p.field]: p.message } : { form: p.message };
      busy = false;
      if (p.field) focusFirst();
    }
  }

  async function onConnect(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    errors = {};
    const base = cleanUrl();
    if (base && !key.trim()) errors = { key: 'Enter the station key.' };
    if (!base || errors.key) return focusFirst();
    busy = true;
    try {
      const status = await checkStation(base, key.trim());
      if (status === 401 || status === 403) {
        errors = { key: 'The station refused that key.' };
        busy = false;
        return focusFirst();
      }
      if (status < 200 || status > 299) {
        errors = { form: `The station answered ${status}.` };
        busy = false;
        return;
      }
    } catch (err) {
      errors = { form: typeof err === 'string' ? err : 'No station answered at that address.' };
      busy = false;
      return;
    }
    try {
      await saveStation(base, key.trim());
      markSeen(base);
    } catch (err) {
      errors = { form: `The key could not be kept on this computer: ${err}` };
      busy = false;
      return;
    }
    key = '';
    location.reload();
  }

  async function onAsk(e?: SubmitEvent) {
    e?.preventDefault();
    if (busy) return;
    errors = {};
    const base = cleanUrl();
    if (base) {
      if (!username.trim()) errors = { ...errors, username: 'Enter your username.' };
      const bad = emailProblem(email);
      if (bad) errors = { ...errors, email: bad };
    }
    if (!base || errors.username || errors.email) return focusFirst();
    busy = true;
    try {
      await askSignInCode(base, username.trim(), email.trim());
      asked = true;
      code = '';
      busy = false;
      queueMicrotask(() => document.getElementById('si-code')?.focus());
    } catch (err) {
      const p = placeError(err, ['email'] as const);
      errors = p.field ? { [p.field]: p.message } : { form: p.message };
      busy = false;
      if (p.field) focusFirst();
    }
  }

  async function onFinish(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    errors = {};
    const base = cleanUrl();
    const bad = codeProblem(code);
    if (base && bad) errors = { code: bad };
    if (!base || errors.code) return focusFirst();
    busy = true;
    try {
      await finishSignInCode(base, username.trim(), email.trim(), cleanCode(code));
      markSeen(base);
      code = '';
      location.reload();
    } catch (err) {
      const p = placeError(err, ['code'] as const);
      errors = p.field ? { [p.field]: p.message } : { form: p.message };
      busy = false;
      if (p.field) focusFirst();
    }
  }

  function swap(to: 'password' | 'code' | 'key') {
    mode = to;
    asked = false;
    code = '';
    errors = {};
    busy = false;
    queueMicrotask(() => (url ? (to === 'key' ? keyEl : userEl) : urlEl)?.focus());
  }

  /** Back from the code step to the address, to correct a slip or ask somewhere else. */
  function again() {
    asked = false;
    code = '';
    errors = {};
    queueMicrotask(() => document.getElementById('si-email')?.focus());
  }
</script>

<main class="setup" data-panel="setup.sign-in" aria-labelledby="si-h">
  <div class="card">
    <header class="head">
      <span class="brand" aria-hidden="true"><Mark /></span>
      <h1 id="si-h" data-panel-title>{mode === 'key' ? 'Connect with the station key' : 'Sign in'}</h1>
      <p class="lede">
        {#if mode === 'key'}
          The key is set where the station runs. It is how the first admin gets in before any account exists.
        {:else if mode === 'code'}
          {#if asked}
            If that username and address belong to an account here, a code is on its way. It works for ten minutes, once. Check your junk folder if it is slow.
          {:else}
            Your station’s address, your username, and the email address your admin set for you. We email you a code.
          {/if}
        {:else}
          Your station’s address, then your username and password.
        {/if}
      </p>
    </header>

    {#if mode === 'key'}
      <form onsubmit={onConnect} novalidate>
        <div class="field">
          <label for="si-url">Station address</label>
          <input
            id="si-url"
            type="url"
            bind:value={url}
            bind:this={urlEl}
            autocomplete="url"
            spellcheck="false"
            placeholder="https://radio.example.org"
            aria-invalid={errors.url ? true : undefined}
            aria-describedby={errors.url ? 'si-url-err' : undefined}
          />
          {#if errors.url}<p id="si-url-err" class="err">{errors.url}</p>{/if}
          {#if tailscaleNote && !errors.url}<p class="note" role="status">{TAILSCALE_NOTE}</p>{/if}
        </div>
        <div class="field">
          <label for="si-key">Station key</label>
          <input
            id="si-key"
            type="password"
            bind:value={key}
            bind:this={keyEl}
            autocomplete="off"
            aria-invalid={errors.key ? true : undefined}
            aria-describedby={errors.key ? 'si-key-err' : undefined}
          />
          {#if errors.key}<p id="si-key-err" class="err">{errors.key}</p>{/if}
        </div>
        {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
        <button type="submit" class="btn primary big" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Connecting…' : 'Connect'}</button>
        <button type="button" class="btn quiet other" onclick={() => swap('password')}>Sign in with a username instead</button>
      </form>
    {:else if mode === 'code'}
      {#if !asked}
        <form onsubmit={onAsk} novalidate>
          <div class="field">
            <label for="si-url">Station address</label>
            <input
              id="si-url"
              type="url"
              bind:value={url}
              bind:this={urlEl}
              autocomplete="url"
              spellcheck="false"
              placeholder="https://radio.example.org"
              aria-invalid={errors.url ? true : undefined}
              aria-describedby={errors.url ? 'si-url-err' : undefined}
            />
            {#if errors.url}<p id="si-url-err" class="err">{errors.url}</p>{/if}
            {#if tailscaleNote && !errors.url}<p class="note" role="status">{TAILSCALE_NOTE}</p>{/if}
          {#if tailscaleNote && !errors.url}<p class="note" role="status">{TAILSCALE_NOTE}</p>{/if}
          </div>
          <div class="field">
            <label for="si-username">Username</label>
            <input
              id="si-username"
              type="text"
              bind:value={username}
              bind:this={userEl}
              autocomplete="username"
              autocapitalize="none"
              spellcheck="false"
              aria-invalid={errors.username ? true : undefined}
              aria-describedby={errors.username ? 'si-username-err' : undefined}
            />
            {#if errors.username}<p id="si-username-err" class="err">{errors.username}</p>{/if}
          </div>
          <div class="field">
            <label for="si-email">Email address</label>
            <input
              id="si-email"
              type="email"
              bind:value={email}
              autocomplete="email"
              autocapitalize="none"
              spellcheck="false"
              aria-invalid={errors.email ? true : undefined}
              aria-describedby={errors.email ? 'si-email-err' : undefined}
            />
            {#if errors.email}<p id="si-email-err" class="err">{errors.email}</p>{/if}
          </div>
          {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
          <button type="submit" class="btn primary big" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Asking…' : 'Email me a code'}</button>
          <button type="button" class="btn quiet other" onclick={() => swap('password')}>Sign in with a password instead</button>
        </form>
      {:else}
        <form onsubmit={onFinish} novalidate>
          <div class="field">
            <label for="si-code">Code from the email</label>
            <input
              id="si-code"
              type="text"
              inputmode="numeric"
              bind:value={code}
              autocomplete="one-time-code"
              spellcheck="false"
              maxlength="12"
              aria-invalid={errors.code ? true : undefined}
              aria-describedby={errors.code ? 'si-code-err' : undefined}
            />
            {#if errors.code}<p id="si-code-err" class="err">{errors.code}</p>{/if}
          </div>
          {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
          <button type="submit" class="btn primary big" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Signing in…' : 'Sign in'}</button>
          <button type="button" class="btn quiet other" disabled={busy} onclick={() => onAsk()}>Send another code</button>
          <button type="button" class="btn quiet other" onclick={again}>Use a different address</button>
        </form>
      {/if}
    {:else}
      <form onsubmit={onSignIn} novalidate>
        <div class="field">
          <label for="si-url">Station address</label>
          <input
            id="si-url"
            type="url"
            bind:value={url}
            bind:this={urlEl}
            autocomplete="url"
            spellcheck="false"
            placeholder="https://radio.example.org"
            aria-invalid={errors.url ? true : undefined}
            aria-describedby={errors.url ? 'si-url-err' : undefined}
          />
          {#if errors.url}<p id="si-url-err" class="err">{errors.url}</p>{/if}
          {#if tailscaleNote && !errors.url}<p class="note" role="status">{TAILSCALE_NOTE}</p>{/if}
        </div>
        <div class="field">
          <label for="si-username">Username</label>
          <input
            id="si-username"
            type="text"
            bind:value={username}
            bind:this={userEl}
            autocomplete="username"
            autocapitalize="none"
            spellcheck="false"
            aria-invalid={errors.username ? true : undefined}
            aria-describedby={errors.username ? 'si-username-err' : undefined}
          />
          {#if errors.username}<p id="si-username-err" class="err">{errors.username}</p>{/if}
        </div>
        <div class="field">
          <label for="si-password">Password</label>
          <input
            id="si-password"
            type="password"
            bind:value={password}
            autocomplete="current-password"
            aria-invalid={errors.password ? true : undefined}
            aria-describedby={errors.password ? 'si-password-err' : undefined}
          />
          {#if errors.password}<p id="si-password-err" class="err">{errors.password}</p>{/if}
        </div>
        {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
        <button type="submit" class="btn primary big" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Signing in…' : 'Sign in'}</button>
        <button type="button" class="btn quiet other" onclick={() => swap('code')}>Email me a code instead</button>
        <button type="button" class="btn quiet other" onclick={() => swap('key')}>Use the station key instead</button>
      </form>
    {/if}
  </div>
</main>

<style>
  .setup {
    display: grid;
    place-items: center;
    min-height: 100vh;
    padding: var(--s-4);
    background: var(--plane);
    overflow: auto;
  }
  .card {
    width: min(26rem, 100%);
    display: grid;
    gap: var(--s-5);
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    box-shadow: var(--e-2);
    padding: var(--s-6) var(--s-5);
  }
  .head {
    display: grid;
    gap: var(--s-2);
  }
  .brand {
    display: inline-flex;
    color: var(--ink);
    margin-bottom: var(--s-2);
  }
  h1 {
    font-size: var(--fs-24);
  }
  .lede {
    color: var(--ink-2);
    line-height: var(--lh-body);
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
    min-height: 2.5rem;
    padding: 0 var(--s-3);
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
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  .note {
    color: var(--ink-2);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--muted);
    padding-left: var(--s-2);
  }
  .btn.big {
    margin-top: var(--s-1);
  }
  .other {
    justify-self: center;
    font-weight: 400;
    text-decoration: underline;
    text-underline-offset: 3px;
  }
</style>
